use tauri::State;

use crate::domain::growth_journal::GrowthJournal;
use crate::sync::auth::{AuthConfig, AuthError, SessionStore, SupabaseAuthClient};
use crate::sync::client::{SupabaseSyncClient, SyncError};
use crate::AppState;

fn local_journal(state: &AppState) -> Result<GrowthJournal, String> {
    let mut ledger = state.ledger.lock().map_err(|_| "일지 저장소 오류")?;
    ledger
        .prepare_growth_journal()
        .map_err(|_| "일지를 준비할 수 없습니다")?;
    ledger
        .local_growth_journal()
        .map_err(|_| "일지를 읽을 수 없습니다".to_string())
}

fn cached_journal(state: &AppState) -> Result<GrowthJournal, String> {
    let mut ledger = state.ledger.lock().map_err(|_| "일지 저장소 오류")?;
    ledger
        .prepare_growth_journal()
        .map_err(|_| "일지를 준비할 수 없습니다")?;
    ledger
        .growth_journal()
        .map_err(|_| "일지를 읽을 수 없습니다".to_string())
}

#[tauri::command]
pub async fn get_growth_journal(state: State<'_, AppState>) -> Result<GrowthJournal, String> {
    let _gate = state.sync_gate.lock().await;
    let Some(config) = AuthConfig::from_env() else {
        return local_journal(&state);
    };
    let store = SessionStore::new(&config).map_err(|_| "보안 저장소를 열 수 없습니다")?;
    let Some(saved) = store.load().map_err(|_| "로그인 정보를 읽을 수 없습니다")?
    else {
        return local_journal(&state);
    };
    state.select_planet_account(&saved.user.id)?;
    let session = match SupabaseAuthClient::new(config.clone())
        .session(&store)
        .await
    {
        Ok(session) => session,
        Err(AuthError::Transport) => return cached_journal(&state),
        Err(_) => return Err("로그인 세션을 확인할 수 없습니다".into()),
    };
    state.select_planet_account(&session.user.id)?;
    let client = SupabaseSyncClient::new(&config.base_url, &config.publishable_key);
    match client.growth_journal(&session.access_token).await {
        Ok(remote) => {
            let mut ledger = state.ledger.lock().map_err(|_| "일지 저장소 오류")?;
            ledger
                .apply_growth_journal_state(&remote)
                .map_err(|_| "서버 일지를 반영할 수 없습니다")?;
            ledger
                .prepare_growth_journal()
                .map_err(|_| "일지를 갱신할 수 없습니다")?;
            ledger
                .growth_journal()
                .map_err(|_| "일지를 읽을 수 없습니다".to_string())
        }
        Err(SyncError::Transport) => cached_journal(&state),
        Err(_) => Err("성장 일지를 불러올 수 없습니다".into()),
    }
}

#[tauri::command]
pub async fn delete_growth_journal(state: State<'_, AppState>) -> Result<GrowthJournal, String> {
    let _gate = state.sync_gate.lock().await;
    let config = AuthConfig::from_env().ok_or("로그인 상태에서 개인 일지를 삭제할 수 있습니다")?;
    let store = SessionStore::new(&config).map_err(|_| "보안 저장소를 열 수 없습니다")?;
    let saved = store
        .load()
        .map_err(|_| "로그인 정보를 읽을 수 없습니다")?
        .ok_or("로그인 상태에서 개인 일지를 삭제할 수 있습니다")?;
    state.select_planet_account(&saved.user.id)?;
    let session = SupabaseAuthClient::new(config.clone())
        .session(&store)
        .await
        .map_err(|error| match error {
            AuthError::Transport => "서버에 연결할 수 없습니다",
            _ => "로그인 세션을 확인할 수 없습니다",
        })?;
    state.select_planet_account(&session.user.id)?;
    let client = SupabaseSyncClient::new(&config.base_url, &config.publishable_key);
    let remote = client
        .delete_growth_journal(&session.access_token)
        .await
        .map_err(|_| "개인 일지를 삭제할 수 없습니다")?;
    let mut ledger = state.ledger.lock().map_err(|_| "일지 저장소 오류")?;
    ledger
        .apply_growth_journal_state(&remote)
        .map_err(|_| "삭제 상태를 반영할 수 없습니다")?;
    ledger
        .prepare_growth_journal()
        .map_err(|_| "삭제 이후 일지를 갱신할 수 없습니다")?;
    ledger
        .growth_journal()
        .map_err(|_| "일지를 읽을 수 없습니다".to_string())
}
