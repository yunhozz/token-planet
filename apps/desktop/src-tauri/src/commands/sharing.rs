use serde::{Deserialize, Serialize};
use tauri::State;

use crate::domain::planet::{PlanetAvatar, WorldPlanet};
use crate::sync::auth::{
    AuthConfig, AuthError, SessionStore, StoredSession, SupabaseAuthClient, SESSION_GATE,
};
use crate::sync::client::{
    CreatedWorldInvite, InviteAcceptStatus, InviteRevokeResult, SupabaseSyncClient, SyncError,
    WorldInvite, WorldMember,
};
use crate::AppState;

#[derive(Clone, Deserialize, Serialize)]
pub struct SharedWorld {
    id: String,
    name: String,
    timezone: String,
    is_owner: bool,
    member_count: u8,
}

#[derive(Clone, Deserialize, Serialize)]
struct CachedWorldView {
    world: SharedWorld,
    planet_members: Vec<WorldPlanet>,
}

#[derive(Serialize)]
pub struct SharingState {
    phase: &'static str,
    user_id: Option<String>,
    world: Option<SharedWorld>,
    sync_status: &'static str,
    pending: u64,
    last_synced_at: Option<String>,
    planet_members: Vec<WorldPlanet>,
}

fn local_state(state: &AppState, phase: &'static str, user_id: Option<String>) -> SharingState {
    let (paused, pending) = state
        .ledger
        .lock()
        .ok()
        .map(|ledger| {
            (
                ledger.sharing_paused().unwrap_or(false),
                ledger.pending_growth_journal_count().unwrap_or(0),
            )
        })
        .unwrap_or_default();
    SharingState {
        phase,
        user_id,
        world: None,
        sync_status: if phase == "signed_in" {
            if paused {
                "paused"
            } else if pending > 0 {
                "queued"
            } else {
                "synced"
            }
        } else {
            "local"
        },
        pending,
        last_synced_at: None,
        planet_members: Vec::new(),
    }
}

fn offline_state(state: &AppState, user_id: &str) -> Result<SharingState, String> {
    let mut ledger = state.ledger.lock().map_err(|_| "로컬 공동 세계 오류")?;
    if let Some((world_id, cached_user_id, timezone)) = ledger
        .cached_world_scope()
        .map_err(|_| "로컬 공동 세계 오류")?
    {
        if cached_user_id != user_id {
            return Err("다른 계정의 공동 세계 정보입니다".into());
        }
        ledger
            .prepare_shared_snapshots(&world_id, user_id, timezone)
            .map_err(|_| "공유 집계를 준비할 수 없습니다")?;
    }
    let cached = ledger
        .cached_world_view()
        .map_err(|_| "로컬 공동 세계 오류")?
        .ok_or("저장된 공동 세계가 없습니다")?;
    let (world, planet_members) = match serde_json::from_str::<CachedWorldView>(&cached) {
        Ok(view) => (view.world, view.planet_members),
        Err(_) => (
            serde_json::from_str::<SharedWorld>(&cached)
                .map_err(|_| "저장된 공동 세계 정보가 잘못되었습니다")?,
            Vec::new(),
        ),
    };
    let paused = ledger
        .sharing_paused()
        .map_err(|_| "로컬 동기화 상태 오류")?;
    let pending = ledger
        .pending_snapshot_count()
        .map_err(|_| "로컬 대기열 오류")?
        .saturating_add(
            ledger
                .pending_growth_journal_count()
                .map_err(|_| "로컬 일지 대기열 오류")?,
        );
    Ok(SharingState {
        phase: "shared",
        user_id: Some(user_id.to_owned()),
        world: Some(world),
        sync_status: if paused { "paused" } else { "failed" },
        pending,
        last_synced_at: None,
        planet_members,
    })
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

pub async fn get_sharing_state_inner(state: State<'_, AppState>) -> Result<SharingState, String> {
    let _gate = state.sync_gate.lock().await;
    let Some(config) = AuthConfig::from_env() else {
        return Ok(local_state(&state, "unavailable", None));
    };
    let store = SessionStore::new(&config).map_err(|_| "보안 저장소를 열 수 없습니다")?;
    let Some(saved) = store.load().map_err(|_| "로그인 정보를 읽을 수 없습니다")?
    else {
        return Ok(local_state(&state, "signed_out", None));
    };
    state.select_planet_account(&saved.user.id)?;
    let auth = SupabaseAuthClient::new(config.clone());
    let session = match auth.session(&store).await {
        Ok(session) => session,
        Err(AuthError::Transport) => {
            let saved = store
                .load()
                .map_err(|_| "로그인 정보를 읽을 수 없습니다")?
                .ok_or("로그인 정보가 없습니다")?;
            return offline_state(&state, &saved.user.id);
        }
        Err(_) => return Err("로그인 세션을 확인할 수 없습니다".into()),
    };
    let client = SupabaseSyncClient::new(&config.base_url, &config.publishable_key);
    let user_id = session.user.id.clone();
    let Some(shell) = (match client.current_world(&session.access_token).await {
        Ok(shell) => shell,
        Err(SyncError::Transport) => return offline_state(&state, &session.user.id),
        Err(_) => return Err("공동 세계를 불러올 수 없습니다".into()),
    }) else {
        return Ok(local_state(&state, "signed_in", Some(user_id)));
    };
    let planet_members = match client.world_planets(&session.access_token, &shell.id).await {
        Ok(members) => members,
        Err(SyncError::Transport) => return offline_state(&state, &session.user.id),
        Err(_) => return Err("그룹 행성 상태를 불러올 수 없습니다".into()),
    };
    let timezone = shell.timezone.parse().map_err(|_| "세계 시간대 오류")?;
    let world = SharedWorld {
        id: shell.id,
        name: shell.name,
        timezone: shell.timezone,
        is_owner: shell.owner_id == session.user.id,
        member_count: planet_members.len() as u8,
    };
    let (paused, pending) = {
        let mut ledger = state
            .ledger
            .lock()
            .map_err(|_| "로컬 동기화 상태를 읽을 수 없습니다")?;
        ledger
            .prepare_shared_snapshots(&world.id, &session.user.id, timezone)
            .map_err(|_| "공유 집계를 준비할 수 없습니다")?;
        ledger
            .set_cached_world_view(
                &serde_json::to_string(&CachedWorldView {
                    world: world.clone(),
                    planet_members: planet_members.clone(),
                })
                .map_err(|_| "세계 정보 오류")?,
            )
            .map_err(|_| "세계 정보를 저장할 수 없습니다")?;
        (
            ledger
                .sharing_paused()
                .map_err(|_| "로컬 동기화 상태를 읽을 수 없습니다")?,
            ledger
                .pending_snapshot_count()
                .map_err(|_| "로컬 대기열을 읽을 수 없습니다")?
                .saturating_add(
                    ledger
                        .pending_growth_journal_count()
                        .map_err(|_| "로컬 일지 대기열을 읽을 수 없습니다")?,
                ),
        )
    };
    Ok(SharingState {
        phase: "shared",
        user_id: Some(user_id),
        world: Some(world),
        sync_status: if paused {
            "paused"
        } else if state
            .sync_failed
            .lock()
            .map(|failed| *failed)
            .unwrap_or(false)
        {
            "failed"
        } else if pending > 0 {
            "queued"
        } else {
            "synced"
        },
        pending,
        last_synced_at: None,
        planet_members,
    })
}

pub async fn start_anonymous_session_inner(
    state: State<'_, AppState>,
) -> Result<SharingState, String> {
    let sync_gate = state.sync_gate.lock().await;
    let _session_gate = SESSION_GATE.lock().await;
    let config = configured()?;
    let store = SessionStore::new(&config).map_err(|_| "보안 저장소를 열 수 없습니다")?;
    let session = match store.load().map_err(|_| "로그인 정보를 읽을 수 없습니다")? {
        Some(session) => session,
        None => {
            let session = SupabaseAuthClient::new(config)
                .sign_in_anonymously()
                .await
                .map_err(|_| "공유 계정을 만들 수 없습니다. 연결과 익명 로그인을 확인하세요")?;
            store
                .save(&session)
                .map_err(|_| "공유 계정을 이 기기에 저장할 수 없습니다")?;
            session
        }
    };
    state.select_planet_account(&session.user.id)?;
    drop(_session_gate);
    drop(sync_gate);
    get_sharing_state_inner(state).await
}

fn save_sharing_nickname(state: &AppState, nickname: &str) -> Result<(), String> {
    let nickname = nickname.trim();
    if nickname.is_empty() || nickname.chars().count() > 24 {
        return Err("닉네임은 1~24자로 입력하세요".into());
    }
    let mut ledger = state
        .ledger
        .lock()
        .map_err(|_| "행성 닉네임을 저장할 수 없습니다")?;
    let avatar = ledger
        .planet_profile()
        .map_err(|_| "행성 프로필을 읽을 수 없습니다")?
        .map(|profile| profile.avatar)
        .unwrap_or(PlanetAvatar::Masculine);
    ledger
        .set_planet_profile(nickname, avatar)
        .map_err(|_| "행성 닉네임을 저장할 수 없습니다".into())
}

pub async fn create_shared_world_inner(
    name: String,
    nickname: String,
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
    save_sharing_nickname(&state, &nickname)?;
    client
        .create_world(&session.access_token, &session.user.id, name, &timezone)
        .await
        .map_err(|_| "세계를 만들 수 없습니다. 이미 다른 세계에 참여 중인지 확인하세요")?;
    get_sharing_state_inner(state).await
}

pub async fn join_world_inner(
    code: String,
    nickname: String,
    state: State<'_, AppState>,
) -> Result<SharingState, String> {
    let (client, session) = signed_in().await?;
    save_sharing_nickname(&state, &nickname)?;
    let result = client
        .accept_world_invite(&session.access_token, code.trim())
        .await
        .map_err(|_| "가입 응답을 확인하지 못했습니다. 같은 초대 코드로 다시 시도하세요")?;
    match result.status {
        InviteAcceptStatus::Accepted | InviteAcceptStatus::AlreadyAccepted => {}
        InviteAcceptStatus::Unavailable => {
            return Err("사용할 수 없는 초대입니다. 소유자에게 새 초대를 요청하세요".into())
        }
        InviteAcceptStatus::AlreadyMember => return Err("이미 다른 그룹에 참여 중입니다".into()),
        InviteAcceptStatus::WorldFull => return Err("그룹 정원 10명이 모두 찼습니다".into()),
        InviteAcceptStatus::RateLimited => {
            return Err("시도가 너무 많습니다. 15분 후 다시 시도하세요".into())
        }
    }
    get_sharing_state_inner(state).await
}

pub async fn create_world_invite_inner() -> Result<CreatedWorldInvite, String> {
    let (client, session) = signed_in().await?;
    let id = world_id(&client, &session).await?;
    client
        .create_world_invite(&session.access_token, &id)
        .await
        .map_err(|_| {
            "발급 응답을 확인하지 못했습니다. 목록에서 초대를 철회한 뒤 다시 발급하세요".into()
        })
}

pub async fn list_world_invites_inner() -> Result<Vec<WorldInvite>, String> {
    let (client, session) = signed_in().await?;
    let id = world_id(&client, &session).await?;
    client
        .list_world_invites(&session.access_token, &id)
        .await
        .map_err(|_| "초대 목록을 불러오지 못했습니다".into())
}

pub async fn revoke_world_invite_inner(invite_id: String) -> Result<InviteRevokeResult, String> {
    let (client, session) = signed_in().await?;
    client
        .revoke_world_invite(&session.access_token, &invite_id)
        .await
        .map_err(|_| "초대를 철회하지 못했습니다".into())
}

pub async fn list_world_members_inner() -> Result<Vec<WorldMember>, String> {
    let (client, session) = signed_in().await?;
    let world_id = world_id(&client, &session).await?;
    client
        .list_members(&session.access_token, &world_id)
        .await
        .map_err(|_| "참여자 목록을 불러올 수 없습니다".into())
}

pub async fn pause_sharing_inner(
    paused: bool,
    state: State<'_, AppState>,
) -> Result<SharingState, String> {
    let _gate = state.sync_gate.lock().await;
    let has_cached_world = state
        .ledger
        .lock()
        .map_err(|_| "로컬 공동 세계 정보를 읽을 수 없습니다")?
        .cached_world_scope()
        .map_err(|_| "로컬 공동 세계 정보를 읽을 수 없습니다")?
        .is_some();
    if !paused && has_cached_world {
        let (client, session) = signed_in().await?;
        if let Some(world) = client
            .current_world(&session.access_token)
            .await
            .map_err(|_| "공동 세계를 불러올 수 없습니다")?
        {
            client
                .resume_my_sync(&session.access_token, &world.id)
                .await
                .map_err(|_| "공동 세계 동기화를 재개할 수 없습니다")?;
        }
    }
    state
        .ledger
        .lock()
        .map_err(|_| "로컬 동기화 상태를 변경할 수 없습니다")?
        .set_sharing_paused(paused)
        .map_err(|_| "로컬 동기화 상태를 변경할 수 없습니다")?;
    drop(_gate);
    get_sharing_state_inner(state).await
}

pub async fn transfer_world_owner_inner(
    new_owner_id: String,
    state: State<'_, AppState>,
) -> Result<SharingState, String> {
    let (client, session) = signed_in().await?;
    let world_id = world_id(&client, &session).await?;
    client
        .transfer_owner(&session.access_token, &world_id, &new_owner_id)
        .await
        .map_err(|_| "소유권을 이전할 수 없습니다")?;
    get_sharing_state_inner(state).await
}

pub async fn leave_world_inner(state: State<'_, AppState>) -> Result<SharingState, String> {
    let _gate = state.sync_gate.lock().await;
    let (client, session) = signed_in().await?;
    let shell = client
        .current_world(&session.access_token)
        .await
        .map_err(|_| "공동 세계를 불러올 수 없습니다")?
        .ok_or("참여 중인 공동 세계가 없습니다")?;
    let timezone: chrono_tz::Tz = shell.timezone.parse().map_err(|_| "세계 시간대 오류")?;
    let today = chrono::Utc::now()
        .with_timezone(&timezone)
        .format("%Y-%m-%d")
        .to_string();
    client
        .leave_world(&session.access_token, &shell.id)
        .await
        .map_err(|_| "세계에서 나갈 수 없습니다. 소유자는 먼저 소유권을 이전하세요")?;
    {
        let mut ledger = state
            .ledger
            .lock()
            .map_err(|_| "로컬 대기열을 정리할 수 없습니다")?;
        ledger
            .stop_sharing_through(&today)
            .map_err(|_| "로컬 대기열을 정리할 수 없습니다")?;
        ledger
            .clear_sharing_scope()
            .map_err(|_| "로컬 대기열을 정리할 수 없습니다")?;
    }
    drop(_gate);
    get_sharing_state_inner(state).await
}

pub async fn delete_synced_usage_inner(state: State<'_, AppState>) -> Result<SharingState, String> {
    let _gate = state.sync_gate.lock().await;
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
        let timezone = ledger
            .cached_world_scope()
            .map_err(|_| "세계 시간대를 읽을 수 없습니다")?
            .ok_or("세계 시간대를 읽을 수 없습니다")?
            .2;
        let today = chrono::Utc::now()
            .with_timezone(&timezone)
            .format("%Y-%m-%d")
            .to_string();
        ledger
            .stop_sharing_through(&today)
            .map_err(|_| "로컬 동기화 상태를 변경할 수 없습니다")?;
    }
    drop(_gate);
    get_sharing_state_inner(state).await
}

#[tauri::command]
pub async fn get_sharing_state(
    state: State<'_, AppState>,
    context: crate::domain::device_reset::LocalContext,
) -> Result<
    crate::domain::device_reset::LocalEnvelope<SharingState>,
    crate::domain::device_reset::LocalCommandError,
> {
    let permit = state.lifecycle.enter(context.generation).await?;
    let result = get_sharing_state_inner(state.clone()).await;
    result
        .map(|data| crate::domain::device_reset::LocalEnvelope {
            generation: permit.generation(),
            data,
        })
        .map_err(|error| {
            crate::domain::device_reset::LocalCommandError::from_error(permit.generation(), error)
        })
}

#[tauri::command]
pub async fn start_anonymous_session(
    state: State<'_, AppState>,
    context: crate::domain::device_reset::LocalContext,
) -> Result<
    crate::domain::device_reset::LocalEnvelope<SharingState>,
    crate::domain::device_reset::LocalCommandError,
> {
    let permit = state.lifecycle.enter(context.generation).await?;
    let result = start_anonymous_session_inner(state.clone()).await;
    result
        .map(|data| crate::domain::device_reset::LocalEnvelope {
            generation: permit.generation(),
            data,
        })
        .map_err(|error| {
            crate::domain::device_reset::LocalCommandError::from_error(permit.generation(), error)
        })
}

#[tauri::command]
pub async fn create_shared_world(
    name: String,
    nickname: String,
    state: State<'_, AppState>,
    context: crate::domain::device_reset::LocalContext,
) -> Result<
    crate::domain::device_reset::LocalEnvelope<SharingState>,
    crate::domain::device_reset::LocalCommandError,
> {
    let permit = state.lifecycle.enter(context.generation).await?;
    let result = create_shared_world_inner(name, nickname, state.clone()).await;
    result
        .map(|data| crate::domain::device_reset::LocalEnvelope {
            generation: permit.generation(),
            data,
        })
        .map_err(|error| {
            crate::domain::device_reset::LocalCommandError::from_error(permit.generation(), error)
        })
}

#[tauri::command]
pub async fn join_world(
    code: String,
    nickname: String,
    state: State<'_, AppState>,
    context: crate::domain::device_reset::LocalContext,
) -> Result<
    crate::domain::device_reset::LocalEnvelope<SharingState>,
    crate::domain::device_reset::LocalCommandError,
> {
    let permit = state.lifecycle.enter(context.generation).await?;
    let result = join_world_inner(code, nickname, state.clone()).await;
    result
        .map(|data| crate::domain::device_reset::LocalEnvelope {
            generation: permit.generation(),
            data,
        })
        .map_err(|error| {
            crate::domain::device_reset::LocalCommandError::from_error(permit.generation(), error)
        })
}

#[tauri::command]
pub async fn create_world_invite(
    state: State<'_, AppState>,
    context: crate::domain::device_reset::LocalContext,
) -> Result<
    crate::domain::device_reset::LocalEnvelope<CreatedWorldInvite>,
    crate::domain::device_reset::LocalCommandError,
> {
    let permit = state.lifecycle.enter(context.generation).await?;
    let result = create_world_invite_inner().await;
    result
        .map(|data| crate::domain::device_reset::LocalEnvelope {
            generation: permit.generation(),
            data,
        })
        .map_err(|error| {
            crate::domain::device_reset::LocalCommandError::from_error(permit.generation(), error)
        })
}

#[tauri::command]
pub async fn list_world_invites(
    state: State<'_, AppState>,
    context: crate::domain::device_reset::LocalContext,
) -> Result<
    crate::domain::device_reset::LocalEnvelope<Vec<WorldInvite>>,
    crate::domain::device_reset::LocalCommandError,
> {
    let permit = state.lifecycle.enter(context.generation).await?;
    let result = list_world_invites_inner().await;
    result
        .map(|data| crate::domain::device_reset::LocalEnvelope {
            generation: permit.generation(),
            data,
        })
        .map_err(|error| {
            crate::domain::device_reset::LocalCommandError::from_error(permit.generation(), error)
        })
}

#[tauri::command]
pub async fn revoke_world_invite(
    invite_id: String,
    state: State<'_, AppState>,
    context: crate::domain::device_reset::LocalContext,
) -> Result<
    crate::domain::device_reset::LocalEnvelope<InviteRevokeResult>,
    crate::domain::device_reset::LocalCommandError,
> {
    let permit = state.lifecycle.enter(context.generation).await?;
    let result = revoke_world_invite_inner(invite_id).await;
    result
        .map(|data| crate::domain::device_reset::LocalEnvelope {
            generation: permit.generation(),
            data,
        })
        .map_err(|error| {
            crate::domain::device_reset::LocalCommandError::from_error(permit.generation(), error)
        })
}

#[tauri::command]
pub async fn list_world_members(
    state: State<'_, AppState>,
    context: crate::domain::device_reset::LocalContext,
) -> Result<
    crate::domain::device_reset::LocalEnvelope<Vec<WorldMember>>,
    crate::domain::device_reset::LocalCommandError,
> {
    let permit = state.lifecycle.enter(context.generation).await?;
    let result = list_world_members_inner().await;
    result
        .map(|data| crate::domain::device_reset::LocalEnvelope {
            generation: permit.generation(),
            data,
        })
        .map_err(|error| {
            crate::domain::device_reset::LocalCommandError::from_error(permit.generation(), error)
        })
}

#[tauri::command]
pub async fn pause_sharing(
    paused: bool,
    state: State<'_, AppState>,
    context: crate::domain::device_reset::LocalContext,
) -> Result<
    crate::domain::device_reset::LocalEnvelope<SharingState>,
    crate::domain::device_reset::LocalCommandError,
> {
    let permit = state.lifecycle.enter(context.generation).await?;
    let result = pause_sharing_inner(paused, state.clone()).await;
    result
        .map(|data| crate::domain::device_reset::LocalEnvelope {
            generation: permit.generation(),
            data,
        })
        .map_err(|error| {
            crate::domain::device_reset::LocalCommandError::from_error(permit.generation(), error)
        })
}

#[tauri::command]
pub async fn transfer_world_owner(
    new_owner_id: String,
    state: State<'_, AppState>,
    context: crate::domain::device_reset::LocalContext,
) -> Result<
    crate::domain::device_reset::LocalEnvelope<SharingState>,
    crate::domain::device_reset::LocalCommandError,
> {
    let permit = state.lifecycle.enter(context.generation).await?;
    let result = transfer_world_owner_inner(new_owner_id, state.clone()).await;
    result
        .map(|data| crate::domain::device_reset::LocalEnvelope {
            generation: permit.generation(),
            data,
        })
        .map_err(|error| {
            crate::domain::device_reset::LocalCommandError::from_error(permit.generation(), error)
        })
}

#[tauri::command]
pub async fn leave_world(
    state: State<'_, AppState>,
    context: crate::domain::device_reset::LocalContext,
) -> Result<
    crate::domain::device_reset::LocalEnvelope<SharingState>,
    crate::domain::device_reset::LocalCommandError,
> {
    let permit = state.lifecycle.enter(context.generation).await?;
    let result = leave_world_inner(state.clone()).await;
    result
        .map(|data| crate::domain::device_reset::LocalEnvelope {
            generation: permit.generation(),
            data,
        })
        .map_err(|error| {
            crate::domain::device_reset::LocalCommandError::from_error(permit.generation(), error)
        })
}

#[tauri::command]
pub async fn delete_synced_usage(
    state: State<'_, AppState>,
    context: crate::domain::device_reset::LocalContext,
) -> Result<
    crate::domain::device_reset::LocalEnvelope<SharingState>,
    crate::domain::device_reset::LocalCommandError,
> {
    let permit = state.lifecycle.enter(context.generation).await?;
    let result = delete_synced_usage_inner(state.clone()).await;
    result
        .map(|data| crate::domain::device_reset::LocalEnvelope {
            generation: permit.generation(),
            data,
        })
        .map_err(|error| {
            crate::domain::device_reset::LocalCommandError::from_error(permit.generation(), error)
        })
}
