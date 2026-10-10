use keyring::{Entry, Error as KeyringError};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::sync::client::shared_http_client;

#[derive(Clone)]
pub struct AuthConfig {
    pub base_url: String,
    pub publishable_key: String,
}

impl AuthConfig {
    pub fn from_env() -> Option<Self> {
        let base_url = std::env::var("TOKEN_PLANET_SUPABASE_URL")
            .ok()
            .or_else(|| option_env!("TOKEN_PLANET_SUPABASE_URL").map(str::to_owned))?;
        let publishable_key = std::env::var("TOKEN_PLANET_SUPABASE_PUBLISHABLE_KEY")
            .ok()
            .or_else(|| option_env!("TOKEN_PLANET_SUPABASE_PUBLISHABLE_KEY").map(str::to_owned))?;
        if base_url.is_empty() || publishable_key.is_empty() {
            return None;
        }
        Some(Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            publishable_key,
        })
    }
}

#[derive(Clone, Deserialize, PartialEq, Serialize)]
pub struct AuthUser {
    pub id: String,
    pub email: Option<String>,
}

#[derive(Clone, Deserialize, PartialEq, Serialize)]
pub struct StoredSession {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub user: AuthUser,
}

#[derive(Deserialize)]
struct AuthSessionResponse {
    access_token: String,
    refresh_token: String,
    expires_in: i64,
    expires_at: Option<i64>,
    user: AuthUser,
}

impl AuthSessionResponse {
    fn into_stored(self) -> StoredSession {
        StoredSession {
            access_token: self.access_token,
            refresh_token: self.refresh_token,
            expires_at: self
                .expires_at
                .unwrap_or_else(|| chrono::Utc::now().timestamp() + self.expires_in),
            user: self.user,
        }
    }
}

#[derive(Debug)]
pub enum AuthError {
    CredentialStore,
    Transport,
    Rejected(u16),
    InvalidResponse,
    SignedOut,
}

pub static SESSION_GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn can_save_refresh(current: &StoredSession, latest: Option<&StoredSession>) -> bool {
    latest == Some(current)
}

pub trait LocalSessionRemover: Send + Sync {
    fn service_id(&self) -> Result<String, AuthError>;
    fn remove_local_session(&self) -> Result<(), AuthError>;
}

pub struct SessionStore {
    entry: Entry,
    service_id: String,
}

pub fn session_service_id(config: &AuthConfig) -> String {
    Sha256::digest(config.base_url.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

impl LocalSessionRemover for SessionStore {
    fn service_id(&self) -> Result<String, AuthError> {
        Ok(self.service_id.clone())
    }
    fn remove_local_session(&self) -> Result<(), AuthError> {
        SessionStore::remove_local_session(self)
    }
}

impl SessionStore {
    pub fn new(config: &AuthConfig) -> Result<Self, AuthError> {
        Self::from_service_id(&session_service_id(config))
    }

    pub fn from_service_id(service_id: &str) -> Result<Self, AuthError> {
        if service_id.len() != 64
            || !service_id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(AuthError::CredentialStore);
        }
        let entry = Entry::new("Token Planet session", service_id)
            .map_err(|_| AuthError::CredentialStore)?;
        Ok(Self {
            entry,
            service_id: service_id.to_owned(),
        })
    }

    pub fn load(&self) -> Result<Option<StoredSession>, AuthError> {
        match self.entry.get_password() {
            Ok(value) => serde_json::from_str(&value)
                .map(Some)
                .map_err(|_| AuthError::InvalidResponse),
            Err(KeyringError::NoEntry) => Ok(None),
            Err(_) => Err(AuthError::CredentialStore),
        }
    }

    pub fn remove_local_session(&self) -> Result<(), AuthError> {
        match self.entry.delete_credential() {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(_) => Err(AuthError::CredentialStore),
        }
    }

    pub fn save(&self, session: &StoredSession) -> Result<(), AuthError> {
        let value = serde_json::to_string(session).map_err(|_| AuthError::InvalidResponse)?;
        self.entry
            .set_password(&value)
            .map_err(|_| AuthError::CredentialStore)
    }
}

pub struct SupabaseAuthClient {
    http: Client,
    config: AuthConfig,
}

impl SupabaseAuthClient {
    pub fn new(config: AuthConfig) -> Self {
        Self {
            http: shared_http_client(),
            config,
        }
    }

    pub async fn sign_in_anonymously(&self) -> Result<StoredSession, AuthError> {
        let response = self
            .http
            .post(format!("{}/auth/v1/signup", self.config.base_url))
            .header("apikey", &self.config.publishable_key)
            .json(&serde_json::json!({ "data": {} }))
            .send()
            .await
            .map_err(|_| AuthError::Transport)?;
        if !response.status().is_success() {
            return Err(AuthError::Rejected(response.status().as_u16()));
        }
        response
            .json::<AuthSessionResponse>()
            .await
            .map(AuthSessionResponse::into_stored)
            .map_err(|_| AuthError::InvalidResponse)
    }

    pub async fn refresh(&self, refresh_token: &str) -> Result<StoredSession, AuthError> {
        let response = self
            .http
            .post(format!(
                "{}/auth/v1/token?grant_type=refresh_token",
                self.config.base_url
            ))
            .header("apikey", &self.config.publishable_key)
            .json(&serde_json::json!({ "refresh_token": refresh_token }))
            .send()
            .await
            .map_err(|_| AuthError::Transport)?;
        if !response.status().is_success() {
            return Err(AuthError::Rejected(response.status().as_u16()));
        }
        response
            .json()
            .await
            .map_err(|_| AuthError::InvalidResponse)
    }

    pub async fn session(&self, store: &SessionStore) -> Result<StoredSession, AuthError> {
        let _gate = SESSION_GATE.lock().await;
        let current = store.load()?.ok_or(AuthError::SignedOut)?;
        if current.expires_at > chrono::Utc::now().timestamp() + 60 {
            return Ok(current);
        }
        let refreshed = self.refresh(&current.refresh_token).await?;
        if !can_save_refresh(&current, store.load()?.as_ref()) {
            return Err(AuthError::SignedOut);
        }
        store.save(&refreshed)?;
        Ok(refreshed)
    }
}

#[cfg(test)]
pub(crate) static AUTH_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::{can_save_refresh, AuthUser, StoredSession};

    #[test]
    fn token_planet_environment_configures_auth() {
        let _env_guard = super::AUTH_ENV_LOCK.lock().unwrap();
        let names = [
            "TOKEN_PLANET_SUPABASE_URL",
            "TOKEN_PLANET_SUPABASE_PUBLISHABLE_KEY",
        ];
        let previous = names.map(|name| std::env::var(name).ok());
        std::env::set_var(names[0], "https://planet.example.test/");
        std::env::set_var(names[1], "sb_publishable_example");
        let config = super::AuthConfig::from_env();
        for (name, value) in names.into_iter().zip(previous) {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
        let config = config.expect("Token Planet public configuration must be recognized");
        assert_eq!(config.base_url, "https://planet.example.test");
        assert_eq!(config.publishable_key, "sb_publishable_example");
    }

    #[test]
    fn session_tokens_are_rust_owned_and_not_a_frontend_state() {
        let session = StoredSession {
            access_token: "access".into(),
            refresh_token: "refresh".into(),
            expires_at: 1,
            user: AuthUser {
                id: "member".into(),
                email: Some("member@example.test".into()),
            },
        };
        let stored = serde_json::to_string(&session).unwrap();
        let restored: StoredSession = serde_json::from_str(&stored).unwrap();
        assert_eq!(restored.refresh_token, "refresh");
    }

    #[test]
    fn refresh_cannot_restore_a_removed_or_replaced_session() {
        let original = StoredSession {
            access_token: "old".into(),
            refresh_token: "old-refresh".into(),
            expires_at: 1,
            user: AuthUser {
                id: "account-a".into(),
                email: None,
            },
        };
        let mut replacement = original.clone();
        replacement.user.id = "account-b".into();
        assert!(can_save_refresh(&original, Some(&original)));
        assert!(!can_save_refresh(&original, None));
        assert!(!can_save_refresh(&original, Some(&replacement)));
    }

    fn fake_store() -> super::SessionStore {
        super::SessionStore {
            entry: keyring::Entry::new_with_credential(Box::new(
                keyring::mock::MockCredential::default(),
            )),
            service_id: "f".repeat(64),
        }
    }

    #[test]
    fn local_reset_session_missing_is_success() {
        let store = fake_store();
        assert!(store.remove_local_session().is_ok());
        assert!(store.load().unwrap().is_none());
    }

    #[test]
    fn local_reset_session_corrupt_json_can_be_removed() {
        let store = fake_store();
        store.entry.set_password("{broken-json").unwrap();
        assert!(store.load().is_err());
        store.remove_local_session().unwrap();
        assert!(store.load().unwrap().is_none());
    }

    #[test]
    fn local_reset_session_targets_only_current_service() {
        let store = fake_store();
        let other = fake_store();
        store.entry.set_password("private-current").unwrap();
        other.entry.set_password("private-other").unwrap();
        store.remove_local_session().unwrap();
        assert!(matches!(
            store.entry.get_password(),
            Err(keyring::Error::NoEntry)
        ));
        assert_eq!(other.entry.get_password().unwrap(), "private-other");
    }

    #[test]
    fn local_reset_session_delete_failure_is_error() {
        let store = fake_store();
        store.entry.set_password("private-current").unwrap();
        store
            .entry
            .get_credential()
            .downcast_ref::<keyring::mock::MockCredential>()
            .unwrap()
            .set_error(keyring::Error::Invalid("entry".into(), "denied".into()));
        assert!(matches!(
            store.remove_local_session(),
            Err(super::AuthError::CredentialStore)
        ));
        assert_eq!(store.entry.get_password().unwrap(), "private-current");
    }
}
