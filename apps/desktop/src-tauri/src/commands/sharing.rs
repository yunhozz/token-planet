use serde::Serialize;
use tauri::State;

use crate::sync::auth::{AuthConfig, SessionStore, StoredSession, SupabaseAuthClient};
use crate::sync::client::{InviteInfo, InviteLink, SupabaseSyncClient, WorldMember};
use crate::AppState;

#[derive(Serialize)]
pub struct SharedWorld {
    id: String,
    name: String,
    timezone: String,
    is_owner: bool,
    member_count: u8,
    known_tokens: Option<u64>,
    growth_credit: f64,
    stage: u8,
    progress_to_next: f64,
    incomplete: bool,
}

#[derive(Serialize)]
pub struct SharingState {
    phase: &'static str,
    email: Option<String>,
    world: Option<SharedWorld>,
    sync_status: &'static str,
    pending: u64,
    last_synced_at: Option<String>,
}

fn local_state(phase: &'static str, email: Option<String>) -> SharingState {
    SharingState {
        phase,
        email,
        world: None,
        sync_status: "local",
        pending: 0,
        last_synced_at: None,
    }
}

fn configured() -> Result<AuthConfig, String> {
    AuthConfig::from_env().ok_or_else(|| "공동 세계 서버 설정이 없습니다".into())
}

async fn signed_in() -> Result<(SupabaseSyncClient, StoredSession), String> {
    let config = configured()?;
    let store = SessionStore::new(&config).map_err(|_| "보안 저장소를 열 수 없습니다")?;
    let auth = SupabaseAuthClient::new(config.clone());
    let session = auth
        .session(&store)
        .await
        .map_err(|_| "로그인 세션을 확인할 수 없습니다")?;
    let client = SupabaseSyncClient::new(&config.base_url, &config.publishable_key);
    Ok((client, session))
}

async fn world_id(client: &SupabaseSyncClient, session: &StoredSession) -> Result<String, String> {
    client
        .current_world(&session.access_token)
        .await
        .map_err(|_| "공동 세계를 불러올 수 없습니다")?
        .map(|world| world.id)
        .ok_or_else(|| "참여 중인 공동 세계가 없습니다".into())
}

#[tauri::command]
pub async fn get_sharing_state(state: State<'_, AppState>) -> Result<SharingState, String> {
    let Some(config) = AuthConfig::from_env() else {
        return Ok(local_state("unavailable", None));
    };
    let store = SessionStore::new(&config).map_err(|_| "보안 저장소를 열 수 없습니다")?;
    if store
        .load()
        .map_err(|_| "로그인 정보를 읽을 수 없습니다")?
        .is_none()
    {
        return Ok(local_state("signed_out", None));
    }
    let auth = SupabaseAuthClient::new(config.clone());
    let session = auth
        .session(&store)
        .await
        .map_err(|_| "로그인 세션을 확인할 수 없습니다")?;
    let client = SupabaseSyncClient::new(&config.base_url, &config.publishable_key);
    let email = session.user.email.clone();
    let Some(shell) = client
        .current_world(&session.access_token)
        .await
        .map_err(|_| "공동 세계를 불러올 수 없습니다")?
    else {
        return Ok(local_state("signed_in", email));
    };
    let summary = client
        .world_summary(&session.access_token, &shell.id)
        .await
        .map_err(|_| "세계 성장 상태를 불러올 수 없습니다")?;
    let (paused, pending) = {
        let ledger = state
            .ledger
            .lock()
            .map_err(|_| "로컬 동기화 상태를 읽을 수 없습니다")?;
        (
            ledger
                .sharing_paused()
                .map_err(|_| "로컬 동기화 상태를 읽을 수 없습니다")?,
            ledger
                .pending_snapshot_count()
                .map_err(|_| "로컬 대기열을 읽을 수 없습니다")?,
        )
    };
    Ok(SharingState {
        phase: "shared",
        email,
        world: Some(SharedWorld {
            id: shell.id,
            name: shell.name,
            timezone: shell.timezone,
            is_owner: shell.owner_id == session.user.id,
            member_count: summary.member_count,
            known_tokens: summary.known_tokens,
            growth_credit: summary.growth_credit,
            stage: summary.stage,
            progress_to_next: summary.progress_to_next,
            incomplete: summary.incomplete,
        }),
        sync_status: if paused {
            "paused"
        } else if pending > 0 {
            "queued"
        } else {
            "synced"
        },
        pending,
        last_synced_at: summary.last_update,
    })
}

#[tauri::command]
pub async fn request_email_code(email: String) -> Result<(), String> {
    if !email.contains('@') || email.len() > 254 {
        return Err("이메일 주소를 확인하세요".into());
    }
    SupabaseAuthClient::new(configured()?)
        .request_email_code(&email)
        .await
        .map_err(|_| "인증코드를 보낼 수 없습니다".into())
}

#[tauri::command]
pub async fn verify_email_code(
    email: String,
    code: String,
    state: State<'_, AppState>,
) -> Result<SharingState, String> {
    let config = configured()?;
    let session = SupabaseAuthClient::new(config.clone())
        .verify_email_code(&email, &code)
        .await
        .map_err(|_| "인증코드를 확인할 수 없습니다")?;
    SessionStore::new(&config)
        .map_err(|_| "보안 저장소를 열 수 없습니다")?
        .save(&session)
        .map_err(|_| "로그인 정보를 저장할 수 없습니다")?;
    get_sharing_state(state).await
}

#[tauri::command]
pub async fn create_shared_world(
    name: String,
    state: State<'_, AppState>,
) -> Result<SharingState, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Err("세계 이름은 1~80자로 입력하세요".into());
    }
    let timezone = state
        .ledger
        .lock()
        .map_err(|_| "로컬 시간대를 읽을 수 없습니다")?
        .timezone
        .to_string();
    let (client, session) = signed_in().await?;
    client
        .create_world(&session.access_token, &session.user.id, name, &timezone)
        .await
        .map_err(|_| "세계를 만들 수 없습니다. 이미 다른 세계에 참여 중인지 확인하세요")?;
    get_sharing_state(state).await
}

#[tauri::command]
pub async fn join_world(code: String, state: State<'_, AppState>) -> Result<SharingState, String> {
    let (client, session) = signed_in().await?;
    client
        .accept_invite(&session.access_token, code.trim())
        .await
        .map_err(|_| "초대 코드를 사용할 수 없습니다. 유효기간과 정원을 확인하세요")?;
    get_sharing_state(state).await
}

#[tauri::command]
pub async fn create_invite() -> Result<InviteLink, String> {
    let (client, session) = signed_in().await?;
    let world_id = world_id(&client, &session).await?;
    client
        .create_invite(&session.access_token, &world_id)
        .await
        .map_err(|_| "초대 코드를 만들 수 없습니다".into())
}

#[tauri::command]
pub async fn list_invites() -> Result<Vec<InviteInfo>, String> {
    let (client, session) = signed_in().await?;
    let world_id = world_id(&client, &session).await?;
    client
        .list_invites(&session.access_token, &world_id)
        .await
        .map_err(|_| "초대 목록을 불러올 수 없습니다".into())
}

#[tauri::command]
pub async fn list_world_members() -> Result<Vec<WorldMember>, String> {
    let (client, session) = signed_in().await?;
    let world_id = world_id(&client, &session).await?;
    client
        .list_members(&session.access_token, &world_id)
        .await
        .map_err(|_| "참여자 목록을 불러올 수 없습니다".into())
}

#[tauri::command]
pub async fn revoke_invite(invite_id: String) -> Result<bool, String> {
    let (client, session) = signed_in().await?;
    client
        .revoke_invite(&session.access_token, &invite_id)
        .await
        .map_err(|_| "초대를 취소할 수 없습니다".into())
}

#[tauri::command]
pub async fn pause_sharing(
    paused: bool,
    state: State<'_, AppState>,
) -> Result<SharingState, String> {
    state
        .ledger
        .lock()
        .map_err(|_| "로컬 동기화 상태를 변경할 수 없습니다")?
        .set_sharing_paused(paused)
        .map_err(|_| "로컬 동기화 상태를 변경할 수 없습니다")?;
    get_sharing_state(state).await
}

#[tauri::command]
pub async fn transfer_world_owner(
    new_owner_id: String,
    state: State<'_, AppState>,
) -> Result<SharingState, String> {
    let (client, session) = signed_in().await?;
    let world_id = world_id(&client, &session).await?;
    client
        .transfer_owner(&session.access_token, &world_id, &new_owner_id)
        .await
        .map_err(|_| "소유권을 이전할 수 없습니다")?;
    get_sharing_state(state).await
}

#[tauri::command]
pub async fn leave_world(state: State<'_, AppState>) -> Result<SharingState, String> {
    let (client, session) = signed_in().await?;
    let world_id = world_id(&client, &session).await?;
    client
        .leave_world(&session.access_token, &world_id)
        .await
        .map_err(|_| "세계에서 나갈 수 없습니다. 소유자는 먼저 소유권을 이전하세요")?;
    state
        .ledger
        .lock()
        .map_err(|_| "로컬 대기열을 정리할 수 없습니다")?
        .clear_outbox()
        .map_err(|_| "로컬 대기열을 정리할 수 없습니다")?;
    get_sharing_state(state).await
}

#[tauri::command]
pub async fn delete_synced_usage(state: State<'_, AppState>) -> Result<SharingState, String> {
    let (client, session) = signed_in().await?;
    let world_id = world_id(&client, &session).await?;
    client
        .delete_synced_usage(&session.access_token, &world_id)
        .await
        .map_err(|_| "공유된 집계를 삭제할 수 없습니다")?;
    {
        let mut ledger = state
            .ledger
            .lock()
            .map_err(|_| "로컬 동기화 상태를 변경할 수 없습니다")?;
        ledger
            .set_sharing_paused(true)
            .map_err(|_| "로컬 동기화 상태를 변경할 수 없습니다")?;
        ledger
            .clear_outbox()
            .map_err(|_| "로컬 대기열을 정리할 수 없습니다")?;
    }
    get_sharing_state(state).await
}

#[tauri::command]
pub async fn sign_out(state: State<'_, AppState>) -> Result<SharingState, String> {
    let config = configured()?;
    let store = SessionStore::new(&config).map_err(|_| "보안 저장소를 열 수 없습니다")?;
    if let Some(session) = store.load().map_err(|_| "로그인 정보를 읽을 수 없습니다")?
    {
        let _ = SupabaseAuthClient::new(config)
            .logout_local(&session.access_token)
            .await;
    }
    store
        .delete()
        .map_err(|_| "로그인 정보를 지울 수 없습니다")?;
    state
        .ledger
        .lock()
        .map_err(|_| "로컬 대기열을 정리할 수 없습니다")?
        .clear_outbox()
        .map_err(|_| "로컬 대기열을 정리할 수 없습니다")?;
    Ok(local_state("signed_out", None))
}
