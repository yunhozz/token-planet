use crate::{
    chat::{
        client::ChatClient,
        realtime::{ChatEvent, ChatRuntime, ConnectionStatus, EventSink, SessionSource},
        types::*,
    },
    sync::{
        auth::{AuthConfig, AuthError, SessionStore, SupabaseAuthClient},
        client::SyncError,
    },
    AppState,
};
use serde::Deserialize;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use tauri::{State, WebviewWindow};

pub const CHAT_EVENT: &str = "group-chat";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartChatRequest {
    pub world_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeChatRequest {
    pub scope: ChatScope,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SendChatRequest {
    pub scope: ChatScope,
    pub request_id: String,
    pub body: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteChatRequest {
    pub scope: ChatScope,
    pub message_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadChatRequest {
    pub scope: ChatScope,
    pub message_seq: ChatSeq,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListChatRequest {
    pub scope: ChatScope,
    pub before_seq: Option<ChatSeq>,
    pub limit: u8,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncChatRequest {
    pub scope: ChatScope,
    pub after_change_seq: ChatSeq,
    pub until_change_seq: ChatSeq,
    pub limit: u8,
}
fn require_main(label: &str) -> Result<(), ChatError> {
    if label == "main" {
        Ok(())
    } else {
        Err(ChatError::InvalidInput)
    }
}
fn session_source(config: AuthConfig) -> SessionSource {
    Arc::new(move || {
        let config = config.clone();
        Box::pin(async move {
            let store = SessionStore::new(&config).map_err(|_| ChatError::Rejected(401))?;
            let session = SupabaseAuthClient::new(config)
                .session(&store)
                .await
                .map_err(|error| match error {
                    AuthError::Transport => ChatError::Transport,
                    AuthError::Rejected(status) => ChatError::Rejected(status),
                    AuthError::InvalidResponse => ChatError::InvalidResponse,
                    AuthError::SignedOut | AuthError::CredentialStore => ChatError::Rejected(401),
                })?;
            Ok(session.access_token)
        })
    })
}
pub struct ChatController {
    config: Option<AuthConfig>,
    session: Option<SessionSource>,
    runtime: Option<ChatRuntime>,
    current: Arc<Mutex<Option<ChatScope>>>,
    changes: tokio::sync::watch::Sender<()>,
    next_generation: AtomicU64,
    operations: tokio::sync::Mutex<()>,
    events: EventSink,
}
impl ChatController {
    pub fn new(config: Option<AuthConfig>, events: EventSink) -> Self {
        let current: Arc<Mutex<Option<ChatScope>>> = Arc::new(Mutex::new(None));
        let session = config.clone().map(session_source);
        let runtime = config
            .clone()
            .zip(session.clone())
            .map(|(config, session)| {
                let active = current.clone();
                let sink = events.clone();
                ChatRuntime::new(
                    config,
                    session,
                    Arc::new(move |event| {
                        let scope = match &event {
                            ChatEvent::Connection { scope, .. }
                            | ChatEvent::Snapshot { scope, .. }
                            | ChatEvent::Message { scope, .. } => scope,
                        };
                        if active
                            .lock()
                            .is_ok_and(|current| current.as_ref() == Some(scope))
                        {
                            sink(event);
                        }
                    }),
                )
            });
        Self {
            config,
            session,
            runtime,
            current,
            changes: tokio::sync::watch::channel(()).0,
            next_generation: AtomicU64::new(1),
            operations: tokio::sync::Mutex::new(()),
            events,
        }
    }
    pub fn validate(&self, scope: &ChatScope) -> Result<(), ChatError> {
        if self
            .current
            .lock()
            .map_err(|_| ChatError::InvalidInput)?
            .as_ref()
            != Some(scope)
        {
            return Err(ChatError::InvalidInput);
        }
        Ok(())
    }
    async fn scoped<F, R>(&self, scope: &ChatScope, future: F) -> Result<R, ChatError>
    where
        F: std::future::Future<Output = Result<R, ChatError>>,
    {
        let mut changed = self.changes.subscribe();
        self.validate(scope)?;
        let result = tokio::select! {biased; _=changed.changed()=>Err(ChatError::InvalidInput),result=future=>result};
        self.validate(scope)?;
        result
    }
    async fn authorized(&self, scope: &ChatScope) -> Result<(ChatClient, String), ChatError> {
        self.validate(scope)?;
        let token = self
            .scoped(
                scope,
                (self.session.as_ref().ok_or(ChatError::InvalidInput)?)(),
            )
            .await?;
        self.validate(scope)?;
        Ok((
            ChatClient::new(self.config.as_ref().ok_or(ChatError::InvalidInput)?),
            token,
        ))
    }
    pub async fn start(&self, world: String) -> Result<ChatScope, ChatError> {
        if uuid::Uuid::parse_str(&world).is_err() || self.runtime.is_none() {
            return Err(ChatError::InvalidInput);
        }
        let _guard = self.operations.lock().await;
        let old = self
            .current
            .lock()
            .map_err(|_| ChatError::InvalidInput)?
            .clone();
        if let Some(scope) = old.as_ref().filter(|s| s.world_id == world) {
            self.runtime
                .as_ref()
                .ok_or(ChatError::InvalidInput)?
                .start(scope.clone())
                .await?;
            return Ok(scope.clone());
        }
        if let Some(old) = old {
            self.stop_locked(&old).await;
        }
        let scope = ChatScope {
            world_id: world,
            generation: self.next_generation.fetch_add(1, Ordering::SeqCst),
        };
        *self.current.lock().map_err(|_| ChatError::InvalidInput)? = Some(scope.clone());
        self.changes.send_replace(());
        // The worker owns HTTP auth refresh/context checks. Return the cancellation scope immediately.
        if let Err(error) = self
            .runtime
            .as_ref()
            .ok_or(ChatError::InvalidInput)?
            .start(scope.clone())
            .await
        {
            self.stop_locked(&scope).await;
            return Err(error);
        }
        Ok(scope)
    }
    async fn stop_locked(&self, scope: &ChatScope) {
        if self.validate(scope).is_err() {
            return;
        }
        if let Ok(mut current) = self.current.lock() {
            *current = None;
            self.changes.send_replace(());
        }
        if let Some(runtime) = &self.runtime {
            runtime.stop(scope).await;
        }
        (self.events)(ChatEvent::Connection {
            scope: scope.clone(),
            status: ConnectionStatus::Stopped,
        });
    }
    pub async fn stop(&self, scope: &ChatScope) {
        let _guard = self.operations.lock().await;
        self.stop_locked(scope).await;
    }
    pub async fn stop_all(&self) {
        let _guard = self.operations.lock().await;
        let scope = self.current.lock().ok().and_then(|s| s.clone());
        if let Some(scope) = scope {
            self.stop_locked(&scope).await;
        }
    }
    pub fn scope_for_world(&self, world: Option<&str>) -> Option<ChatScope> {
        self.current
            .lock()
            .ok()
            .and_then(|s| s.clone())
            .filter(|s| world.is_none_or(|world| s.world_id == world))
    }
    pub async fn finish_leave(
        &self,
        world: &str,
        result: Result<bool, SyncError>,
    ) -> Result<bool, SyncError> {
        if matches!(result, Ok(true)) {
            let scope = self
                .current
                .lock()
                .ok()
                .and_then(|s| s.clone())
                .filter(|s| s.world_id == world);
            if let Some(scope) = scope {
                self.stop(&scope).await;
            }
        }
        result
    }
}
#[tauri::command]
pub async fn start_group_chat(
    request: StartChatRequest,
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<ChatScope, ChatError> {
    require_main(window.label())?;
    state.chat.start(request.world_id).await
}
#[tauri::command]
pub async fn stop_group_chat(
    request: ScopeChatRequest,
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<(), ChatError> {
    require_main(window.label())?;
    state.chat.stop(&request.scope).await;
    Ok(())
}
#[tauri::command]
pub async fn get_group_chat_context(
    request: ScopeChatRequest,
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<ChatContext, ChatError> {
    require_main(window.label())?;
    let (client, token) = state.chat.authorized(&request.scope).await?;
    let result = state
        .chat
        .scoped(
            &request.scope,
            client.context(&token, &request.scope.world_id),
        )
        .await?;
    state.chat.validate(&request.scope)?;
    Ok(result)
}
#[tauri::command]
pub async fn list_group_chat_messages(
    request: ListChatRequest,
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<ChatPage, ChatError> {
    require_main(window.label())?;
    let (client, token) = state.chat.authorized(&request.scope).await?;
    let result = state
        .chat
        .scoped(
            &request.scope,
            client.list(
                &token,
                &request.scope.world_id,
                request.before_seq,
                request.limit,
            ),
        )
        .await?;
    state.chat.validate(&request.scope)?;
    Ok(result)
}
#[tauri::command]
pub async fn sync_group_chat_changes(
    request: SyncChatRequest,
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<ChatChangePage, ChatError> {
    require_main(window.label())?;
    let (client, token) = state.chat.authorized(&request.scope).await?;
    let result = state
        .chat
        .scoped(
            &request.scope,
            client.sync(
                &token,
                &request.scope.world_id,
                request.after_change_seq,
                request.until_change_seq,
                request.limit,
            ),
        )
        .await?;
    state.chat.validate(&request.scope)?;
    Ok(result)
}
#[tauri::command]
pub async fn send_group_chat_message(
    request: SendChatRequest,
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<ChatMessage, ChatError> {
    require_main(window.label())?;
    let (client, token) = state.chat.authorized(&request.scope).await?;
    let result = state
        .chat
        .scoped(
            &request.scope,
            client.send(
                &token,
                &request.scope.world_id,
                &request.request_id,
                &request.body,
            ),
        )
        .await?;
    state.chat.validate(&request.scope)?;
    Ok(result)
}
#[tauri::command]
pub async fn delete_group_chat_message(
    request: DeleteChatRequest,
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<ChatMessage, ChatError> {
    require_main(window.label())?;
    let (client, token) = state.chat.authorized(&request.scope).await?;
    let result = state
        .chat
        .scoped(
            &request.scope,
            client.delete(&token, &request.scope.world_id, &request.message_id),
        )
        .await?;
    state.chat.validate(&request.scope)?;
    Ok(result)
}
#[tauri::command]
pub async fn mark_group_chat_read(
    request: ReadChatRequest,
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<ChatReadState, ChatError> {
    require_main(window.label())?;
    let (client, token) = state.chat.authorized(&request.scope).await?;
    let result = state
        .chat
        .scoped(
            &request.scope,
            client.mark_read(&token, &request.scope.world_id, request.message_seq),
        )
        .await?;
    state.chat.validate(&request.scope)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn command_boundary_rejects_profile_claims_and_foreign_windows() {
        let request = serde_json::json!({"scope":{"world_id":"22222222-2222-4222-8222-222222222222","generation":1},"request_id":"44444444-4444-4444-8444-444444444444","body":"hello","avatar":"feminine"});
        assert!(serde_json::from_value::<SendChatRequest>(request).is_err());
        assert!(require_main("main").is_ok());
        assert!(require_main("cosmetic-shop").is_err());
    }
    #[tokio::test]
    async fn leave_capture_without_cache_preserves_new_generation() {
        let controller = ChatController::new(None, Arc::new(|_| {}));
        let old = ChatScope {
            world_id: "22222222-2222-4222-8222-222222222222".into(),
            generation: 1,
        };
        assert!(controller.scope_for_world(None).is_none());
        *controller.current.lock().unwrap() = Some(old.clone());
        let captured = controller
            .scope_for_world(None)
            .expect("capture without local cache");
        let newer = ChatScope {
            generation: 2,
            ..old
        };
        *controller.current.lock().unwrap() = Some(newer.clone());
        controller.stop(&captured).await;
        assert!(controller.validate(&newer).is_ok());
    }
    #[tokio::test]
    async fn controller_rejects_stale_scope_and_leave_failure_preserves_scope() {
        let scope = ChatScope {
            world_id: "22222222-2222-4222-8222-222222222222".into(),
            generation: 1,
        };
        let controller = ChatController::new(None, Arc::new(|_| {}));
        *controller.current.lock().unwrap() = Some(scope.clone());
        assert!(controller.validate(&scope).is_ok());
        assert!(controller
            .validate(&ChatScope {
                generation: 0,
                ..scope.clone()
            })
            .is_err());
        assert!(controller
            .finish_leave(
                &scope.world_id,
                Err(crate::sync::client::SyncError::Transport)
            )
            .await
            .is_err());
        assert!(controller.validate(&scope).is_ok());
        assert_eq!(
            controller.finish_leave(&scope.world_id, Ok(true)).await,
            Ok(true)
        );
        assert!(controller.validate(&scope).is_err());
    }
    #[tokio::test]
    async fn start_returns_scope_before_auth_and_replacement_cancellation_is_prompt() {
        let config = AuthConfig {
            base_url: "https://example.invalid".into(),
            publishable_key: "mock".into(),
        };
        let mut controller = ChatController::new(Some(config.clone()), Arc::new(|_| {}));
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = calls.clone();
        let session: SessionSource = Arc::new(move || {
            seen.fetch_add(1, Ordering::SeqCst);
            Box::pin(std::future::pending())
        });
        controller.runtime = Some(ChatRuntime::new(config, session, Arc::new(|_| {})));
        let scope = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            controller.start("22222222-2222-4222-8222-222222222222".into()),
        )
        .await
        .unwrap()
        .unwrap();
        tokio::task::yield_now().await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            controller.start(scope.world_id.clone()).await.unwrap(),
            scope
        );
        let next = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            controller.start("44444444-4444-4444-8444-444444444444".into()),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(next.generation > scope.generation);
        controller.stop(&scope).await;
        assert!(controller.validate(&next).is_ok());
        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            controller.stop(&next),
        )
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn scope_stop_cancels_pending_command_auth() {
        let mut controller = ChatController::new(
            Some(AuthConfig {
                base_url: "https://example.invalid".into(),
                publishable_key: "mock".into(),
            }),
            Arc::new(|_| {}),
        );
        controller.session = Some(Arc::new(|| Box::pin(std::future::pending())));
        let controller = Arc::new(controller);
        let scope = ChatScope {
            world_id: "22222222-2222-4222-8222-222222222222".into(),
            generation: 1,
        };
        *controller.current.lock().unwrap() = Some(scope.clone());
        let working = controller.clone();
        let old = scope.clone();
        let task = tokio::spawn(async move { working.authorized(&old).await });
        tokio::task::yield_now().await;
        controller.stop(&scope).await;
        assert!(matches!(
            tokio::time::timeout(std::time::Duration::from_millis(100), task).await,
            Ok(Ok(Err(ChatError::InvalidInput)))
        ));
    }
    #[tokio::test]
    async fn signed_out_and_foreign_group_actions_are_rejected() {
        let scope = ChatScope {
            world_id: "22222222-2222-4222-8222-222222222222".into(),
            generation: 1,
        };
        let mut controller = ChatController::new(
            Some(AuthConfig {
                base_url: "https://example.invalid".into(),
                publishable_key: "mock".into(),
            }),
            Arc::new(|_| {}),
        );
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = calls.clone();
        controller.session = Some(Arc::new(move || {
            seen.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Err(ChatError::Rejected(401)) })
        }));
        *controller.current.lock().unwrap() = Some(scope.clone());
        assert!(matches!(
            controller
                .authorized(&ChatScope {
                    world_id: "44444444-4444-4444-8444-444444444444".into(),
                    generation: 1
                })
                .await,
            Err(ChatError::InvalidInput)
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(matches!(
            controller.authorized(&scope).await,
            Err(ChatError::Rejected(401))
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
