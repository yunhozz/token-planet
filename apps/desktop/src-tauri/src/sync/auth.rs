use keyring::{Entry, Error as KeyringError};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone)]
pub struct AuthConfig {
    pub base_url: String,
    pub publishable_key: String,
}

impl AuthConfig {
    pub fn from_env() -> Option<Self> {
        let base_url = std::env::var("TOKEN_WORLD_SUPABASE_URL")
            .ok()
            .or_else(|| option_env!("TOKEN_WORLD_SUPABASE_URL").map(str::to_owned))?;
        let publishable_key = std::env::var("TOKEN_WORLD_SUPABASE_PUBLISHABLE_KEY")
            .ok()
            .or_else(|| option_env!("TOKEN_WORLD_SUPABASE_PUBLISHABLE_KEY").map(str::to_owned))?;
        if base_url.is_empty() || publishable_key.is_empty() {
            return None;
        }
        Some(Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            publishable_key,
        })
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct AuthUser {
    pub id: String,
    pub email: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct StoredSession {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub user: AuthUser,
}

#[derive(Debug)]
pub enum AuthError {
    CredentialStore,
    Transport,
    Rejected(u16),
    InvalidResponse,
    SignedOut,
}

pub struct SessionStore {
    entry: Entry,
}

impl SessionStore {
    pub fn new(config: &AuthConfig) -> Result<Self, AuthError> {
        let id = Sha256::digest(config.base_url.as_bytes());
        let username = id
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let entry =
            Entry::new("Token World session", &username).map_err(|_| AuthError::CredentialStore)?;
        Ok(Self { entry })
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

    pub fn save(&self, session: &StoredSession) -> Result<(), AuthError> {
        let value = serde_json::to_string(session).map_err(|_| AuthError::InvalidResponse)?;
        self.entry
            .set_password(&value)
            .map_err(|_| AuthError::CredentialStore)
    }

    pub fn delete(&self) -> Result<(), AuthError> {
        match self.entry.delete_credential() {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(_) => Err(AuthError::CredentialStore),
        }
    }
}

pub struct SupabaseAuthClient {
    http: Client,
    config: AuthConfig,
}

impl SupabaseAuthClient {
    pub fn new(config: AuthConfig) -> Self {
        Self {
            http: Client::new(),
            config,
        }
    }

    pub async fn request_email_code(&self, email: &str) -> Result<(), AuthError> {
        let response = self
            .http
            .post(format!("{}/auth/v1/otp", self.config.base_url))
            .header("apikey", &self.config.publishable_key)
            .json(&serde_json::json!({ "email": email, "create_user": true }))
            .send()
            .await
            .map_err(|_| AuthError::Transport)?;
        if !response.status().is_success() {
            return Err(AuthError::Rejected(response.status().as_u16()));
        }
        Ok(())
    }

    pub async fn verify_email_code(
        &self,
        email: &str,
        code: &str,
    ) -> Result<StoredSession, AuthError> {
        let response = self
            .http
            .post(format!("{}/auth/v1/verify", self.config.base_url))
            .header("apikey", &self.config.publishable_key)
            .json(&serde_json::json!({ "email": email, "token": code, "type": "email" }))
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
        let current = store.load()?.ok_or(AuthError::SignedOut)?;
        if current.expires_at > chrono::Utc::now().timestamp() + 60 {
            return Ok(current);
        }
        let refreshed = self.refresh(&current.refresh_token).await?;
        store.save(&refreshed)?;
        Ok(refreshed)
    }

    pub async fn logout_local(&self, access_token: &str) -> Result<(), AuthError> {
        let response = self
            .http
            .post(format!(
                "{}/auth/v1/logout?scope=local",
                self.config.base_url
            ))
            .header("apikey", &self.config.publishable_key)
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|_| AuthError::Transport)?;
        if !response.status().is_success() {
            return Err(AuthError::Rejected(response.status().as_u16()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{AuthUser, StoredSession};

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
}
