use tauri::State;

use crate::domain::cosmetic_shop::{
    CosmeticEquipAction, CosmeticEquipResult, CosmeticEquipStatus, CosmeticPurchaseAction,
    CosmeticShopState, QuoteTarget, ShopActionResult, ShopActionStatus, ShopQuote, ShopRequest,
    ShopState,
};
use crate::sync::auth::{AuthConfig, AuthError, SessionStore, StoredSession, SupabaseAuthClient};
use crate::sync::client::{SupabaseSyncClient, SyncError};
use crate::sync::worker::import_pending_guest_cosmetics;
use crate::{storage::ledger::Ledger, AppState};
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

type ShopRpcFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, SyncError>> + Send + 'a>>;

trait ShopRpc: Send + Sync {
    fn get_state<'a>(&'a self, access_token: &'a str) -> ShopRpcFuture<'a, ShopState>;
    fn quote_action<'a>(
        &'a self,
        access_token: &'a str,
        target: &'a QuoteTarget,
    ) -> ShopRpcFuture<'a, ShopQuote>;
    fn apply_action<'a>(
        &'a self,
        access_token: &'a str,
        request: &'a ShopRequest,
    ) -> ShopRpcFuture<'a, ShopActionResult>;
}

impl ShopRpc for SupabaseSyncClient {
    fn get_state<'a>(&'a self, access_token: &'a str) -> ShopRpcFuture<'a, ShopState> {
        Box::pin(self.get_my_shop_state(access_token))
    }

    fn quote_action<'a>(
        &'a self,
        access_token: &'a str,
        target: &'a QuoteTarget,
    ) -> ShopRpcFuture<'a, ShopQuote> {
        Box::pin(self.quote_shop_action(access_token, target))
    }

    fn apply_action<'a>(
        &'a self,
        access_token: &'a str,
        request: &'a ShopRequest,
    ) -> ShopRpcFuture<'a, ShopActionResult> {
        Box::pin(self.apply_shop_action(access_token, request))
    }
}

#[derive(Clone)]
enum CanonicalShopSession {
    Guest,
    Unavailable(String),
    Online {
        account_id: String,
        cycle_id: String,
        access_token: String,
    },
}

fn request_id(request: &ShopRequest) -> &str {
    match request {
        ShopRequest::Purchase { request_id, .. }
        | ShopRequest::Place { request_id, .. }
        | ShopRequest::Retrieve { request_id, .. }
        | ShopRequest::EquipAvatar { request_id, .. }
        | ShopRequest::RemoveNatural { request_id, .. }
        | ShopRequest::ResetPlanet { request_id, .. } => request_id,
    }
}

fn offline_shop_action(
    ledger: &Mutex<Ledger>,
    request: &ShopRequest,
    reason: &str,
) -> Result<ShopActionResult, String> {
    let mut shop = ledger
        .lock()
        .map_err(|_| "상점 저장소 오류")?
        .shop_state()
        .map_err(|_| "상점 상태를 읽을 수 없습니다")?;
    shop.action_unavailable_reason = Some(reason.to_owned());
    Ok(ShopActionResult {
        status: ShopActionStatus::Unavailable,
        request_id: request_id(request).to_owned(),
        confirmed_quote: None,
        state: shop,
    })
}

fn canonical_local_shop_state(
    ledger: &Mutex<Ledger>,
    reason: Option<&str>,
) -> Result<ShopState, String> {
    let mut shop = ledger
        .lock()
        .map_err(|_| "상점 저장소 오류")?
        .shop_state()
        .map_err(|_| "상점 상태를 읽을 수 없습니다")?;
    if let Some(reason) = reason {
        shop.action_unavailable_reason = Some(reason.to_owned());
    }
    Ok(shop)
}

fn ensure_shop_context(
    ledger: &Mutex<Ledger>,
    account_id: &str,
    cycle_id: &str,
) -> Result<(), String> {
    let ledger = ledger.lock().map_err(|_| "상점 저장소 오류")?;
    let actual_account = ledger
        .cosmetic_account_id()
        .map_err(|_| "상점 계정 정보를 읽을 수 없습니다")?;
    let actual_cycle = ledger
        .planet_cycle_id()
        .map_err(|_| "행성 주기를 읽을 수 없습니다")?;
    if actual_account != account_id || actual_cycle != cycle_id {
        return Err("계정 또는 행성 주기가 변경되어 상점 응답을 반영할 수 없습니다".into());
    }
    Ok(())
}

fn online_shop_context(
    ledger: &Mutex<Ledger>,
    user_id: &str,
    access_token: &str,
) -> Result<CanonicalShopSession, String> {
    let ledger = ledger.lock().map_err(|_| "상점 저장소 오류")?;
    let account_id = ledger
        .cosmetic_account_id()
        .map_err(|_| "상점 계정 정보를 읽을 수 없습니다")?;
    let expected_account = format!("account:{user_id}");
    if account_id != expected_account {
        return Err("로그인 계정과 현재 상점 계정이 다릅니다".into());
    }
    let cycle_id = ledger
        .planet_cycle_id()
        .map_err(|_| "행성 주기를 읽을 수 없습니다")?;
    Ok(CanonicalShopSession::Online {
        account_id,
        cycle_id,
        access_token: access_token.to_owned(),
    })
}

async fn get_shop_state_for_session(
    ledger: &Mutex<Ledger>,
    session: CanonicalShopSession,
    rpc: Option<&dyn ShopRpc>,
) -> Result<ShopState, String> {
    match session {
        CanonicalShopSession::Guest => {
            if ledger
                .lock()
                .map_err(|_| "상점 저장소 오류")?
                .cosmetic_account_id()
                .map_err(|_| "상점 계정 정보를 읽을 수 없습니다")?
                .starts_with("account:")
            {
                return Err("로그인 계정의 상점은 서버 연결 후 이용할 수 있습니다".into());
            }
            canonical_local_shop_state(ledger, None)
        }
        CanonicalShopSession::Unavailable(reason) => {
            canonical_local_shop_state(ledger, Some(&reason))
        }
        CanonicalShopSession::Online {
            account_id,
            cycle_id,
            access_token,
        } => {
            ensure_shop_context(ledger, &account_id, &cycle_id)?;
            let rpc = rpc.ok_or("온라인 상점 연결을 사용할 수 없습니다")?;
            let shop = rpc
                .get_state(&access_token)
                .await
                .map_err(sync_error_reason)?;
            store_remote_shop_state(ledger, &account_id, &cycle_id, &shop, true)?;
            Ok(shop)
        }
    }
}

async fn quote_shop_action_for_session(
    ledger: &Mutex<Ledger>,
    session: CanonicalShopSession,
    rpc: Option<&dyn ShopRpc>,
    target: &QuoteTarget,
) -> Result<ShopQuote, String> {
    match session {
        CanonicalShopSession::Guest => {
            let ledger = ledger.lock().map_err(|_| "상점 저장소 오류")?;
            if ledger
                .cosmetic_account_id()
                .map_err(|_| "상점 계정 정보를 읽을 수 없습니다")?
                .starts_with("account:")
            {
                return Err("로그인 계정의 견적은 서버 연결 후 이용할 수 있습니다".into());
            }
            ledger
                .quote_shop(target)
                .map_err(|_| "상점 견적을 계산할 수 없습니다".into())
        }
        CanonicalShopSession::Unavailable(reason) => Err(reason),
        CanonicalShopSession::Online {
            account_id,
            cycle_id,
            access_token,
        } => {
            if let QuoteTarget::RemoveNatural { key } = target {
                if key.cycle_id != cycle_id {
                    return Err("자연 개체 견적의 주기가 현재 주기와 다릅니다".into());
                }
            }
            ensure_shop_context(ledger, &account_id, &cycle_id)?;
            let rpc = rpc.ok_or("온라인 상점 연결을 사용할 수 없습니다")?;
            let quote = rpc
                .quote_action(&access_token, target)
                .await
                .map_err(sync_error_reason)?;
            ensure_shop_context(ledger, &account_id, &cycle_id)?;
            if &quote.target != target {
                return Err("서버 견적 대상이 요청과 다릅니다".into());
            }
            Ok(quote)
        }
    }
}

fn store_remote_shop_state(
    ledger: &Mutex<Ledger>,
    account_id: &str,
    captured_cycle_id: &str,
    shop: &ShopState,
    allow_server_cycle_change: bool,
) -> Result<(), String> {
    if shop.account_id != account_id
        || (!allow_server_cycle_change && shop.current_cycle_id != captured_cycle_id)
    {
        return Err("서버 상점 응답의 계정 또는 행성 주기가 다릅니다".into());
    }
    let mut ledger = ledger.lock().map_err(|_| "상점 저장소 오류")?;
    let actual_account = ledger
        .cosmetic_account_id()
        .map_err(|_| "상점 계정 정보를 읽을 수 없습니다")?;
    let actual_cycle = ledger
        .planet_cycle_id()
        .map_err(|_| "행성 주기를 읽을 수 없습니다")?;
    if actual_account != account_id || actual_cycle != captured_cycle_id {
        return Err("계정 또는 행성 주기가 변경되어 상점 응답을 반영할 수 없습니다".into());
    }
    ledger
        .store_confirmed_shop_state(shop)
        .map_err(|_| "상점 상태를 저장할 수 없습니다".into())
}

fn validate_natural_removal_response(
    request: &ShopRequest,
    result: &ShopActionResult,
) -> Result<(), String> {
    let ShopRequest::RemoveNatural { key, quote, .. } = request else {
        return Ok(());
    };
    let expected_target = QuoteTarget::RemoveNatural { key: key.clone() };
    if quote.target != expected_target {
        return Err("자연 개체 견적 대상이 요청과 다릅니다".into());
    }
    if !matches!(
        result.status,
        ShopActionStatus::Removed
            | ShopActionStatus::AlreadyRemoved
            | ShopActionStatus::InsufficientBalance
            | ShopActionStatus::QuoteChanged
            | ShopActionStatus::CatalogMismatch
            | ShopActionStatus::VersionConflict
            | ShopActionStatus::CycleMismatch
            | ShopActionStatus::NotOwned
            | ShopActionStatus::RequestConflict
            | ShopActionStatus::Unavailable
    ) {
        return Err("서버가 자연 개체 제거에 예상하지 못한 결과를 반환했습니다".into());
    }
    if result
        .confirmed_quote
        .as_ref()
        .is_some_and(|confirmed| confirmed.target != expected_target)
    {
        return Err("서버 확인 견적의 대상이 요청과 다릅니다".into());
    }
    if result.status == ShopActionStatus::Removed && result.confirmed_quote.as_ref() != Some(quote)
    {
        return Err("서버 제거 결과의 확인 견적이 요청과 다릅니다".into());
    }
    if matches!(result.status, ShopActionStatus::Removed | ShopActionStatus::AlreadyRemoved)
        && !result.state.removed_natural_keys.contains(key)
    {
        return Err("서버 제거 결과에 정식 제거 상태가 없습니다".into());
    }
    Ok(())
}

async fn apply_shop_request_for_session(
    ledger: &Mutex<Ledger>,
    session: CanonicalShopSession,
    rpc: Option<&dyn ShopRpc>,
    request: &ShopRequest,
) -> Result<ShopActionResult, String> {
    match session {
        CanonicalShopSession::Guest => {
            let mut ledger = ledger.lock().map_err(|_| "상점 저장소 오류")?;
            if ledger
                .cosmetic_account_id()
                .map_err(|_| "상점 계정 정보를 읽을 수 없습니다")?
                .starts_with("account:")
            {
                return Err("로그인 계정의 상점은 서버 연결 후 이용할 수 있습니다".into());
            }
            ledger
                .apply_guest_shop_request(request, chrono::Utc::now())
                .map_err(|_| "상점 요청을 처리할 수 없습니다".into())
        }
        CanonicalShopSession::Unavailable(reason) => {
            offline_shop_action(ledger, request, &reason)
        }
        CanonicalShopSession::Online {
            account_id,
            cycle_id,
            access_token,
        } => {
            ensure_shop_context(ledger, &account_id, &cycle_id)?;
            if matches!(request, ShopRequest::ResetPlanet { .. }) {
                let reason = "행성 초기화는 서버 상점 연결 후 지원됩니다";
                return offline_shop_action(ledger, request, reason);
            }
            if let ShopRequest::RemoveNatural { key, quote, .. } = request {
                if key.cycle_id != cycle_id {
                    return Err("자연 개체 요청의 주기가 현재 주기와 다릅니다".into());
                }
                if quote.target != (QuoteTarget::RemoveNatural { key: key.clone() }) {
                    return Err("자연 개체 견적 대상이 요청과 다릅니다".into());
                }
            }
            let rpc = rpc.ok_or("온라인 상점 연결을 사용할 수 없습니다")?;
            let result = rpc
                .apply_action(&access_token, request)
                .await
                .map_err(sync_error_reason)?;
            if result.request_id != request_id(request) {
                return Err("서버 상점 응답의 요청 ID가 다릅니다".into());
            }
            validate_natural_removal_response(request, &result)?;
            store_remote_shop_state(
                ledger,
                &account_id,
                &cycle_id,
                &result.state,
                !matches!(request, ShopRequest::RemoveNatural { .. })
                    && result.status == ShopActionStatus::CycleMismatch,
            )?;
            Ok(result)
        }
    }
}

enum ShopSession {
    Guest,
    Online {
        config: AuthConfig,
        session: StoredSession,
    },
    Unavailable(&'static str),
}

async fn shop_session(state: &AppState) -> Result<ShopSession, String> {
    let account_id = state
        .ledger
        .lock()
        .map_err(|_| "상점 저장소 오류")?
        .cosmetic_account_id()
        .map_err(|_| "상점 계정 정보를 읽을 수 없습니다")?;
    let Some(config) = AuthConfig::from_env() else {
        return Ok(if account_id.starts_with("account:") {
            ShopSession::Unavailable("로그인 계정의 상점은 서버 연결 후 이용할 수 있습니다")
        } else {
            ShopSession::Guest
        });
    };
    let store = SessionStore::new(&config).map_err(|_| "보안 저장소를 열 수 없습니다")?;
    let Some(saved) = store.load().map_err(|_| "로그인 정보를 읽을 수 없습니다")? else {
        return Ok(if account_id.starts_with("account:") {
            ShopSession::Unavailable("로그인 계정의 상점은 서버 연결 후 이용할 수 있습니다")
        } else {
            ShopSession::Guest
        });
    };
    state.select_planet_account(&saved.user.id)?;
    match SupabaseAuthClient::new(config.clone()).session(&store).await {
        Ok(session) => {
            state.select_planet_account(&session.user.id)?;
            Ok(ShopSession::Online { config, session })
        }
        Err(AuthError::Transport) => Ok(ShopSession::Unavailable("서버에 연결할 수 없습니다")),
        Err(_) => Ok(ShopSession::Unavailable("로그인 세션을 확인할 수 없습니다")),
    }
}

fn local_shop_state(state: &AppState, reason: Option<&str>) -> Result<CosmeticShopState, String> {
    let mut shop = state
        .ledger
        .lock()
        .map_err(|_| "상점 저장소 오류")?
        .cosmetic_shop_state()
        .map_err(|_| "상점 상태를 읽을 수 없습니다")?;
    if shop.guest_import_pending {
        shop.guest_import_error = reason.map(str::to_owned);
    }
    shop.action_unavailable_reason = reason.map(str::to_owned);
    Ok(shop)
}

fn save_remote_shop_state(
    state: &AppState,
    shop: &CosmeticShopState,
) -> Result<(), String> {
    state
        .ledger
        .lock()
        .map_err(|_| "상점 저장소 오류")?
        .store_confirmed_cosmetic_state(shop)
        .map_err(|_| "상점 상태를 저장할 수 없습니다".into())
}

fn sync_error_reason(error: SyncError) -> &'static str {
    match error {
        SyncError::Transport => "서버에 연결할 수 없습니다",
        SyncError::Rejected(401 | 403) => "로그인 세션을 확인할 수 없습니다",
        SyncError::Rejected(_) => "상점 요청을 처리할 수 없습니다",
        SyncError::InvalidSnapshot | SyncError::InvalidResponse => "상점 응답을 확인할 수 없습니다",
    }
}

async fn sharing_pause_reason(
    state: &AppState,
    client: &SupabaseSyncClient,
    access_token: &str,
) -> Option<String> {
    match state
        .ledger
        .lock()
        .ok()
        .and_then(|ledger| ledger.sharing_paused().ok())
    {
        Some(true) => return Some("동기화가 일시정지되어 구매와 장착 변경을 할 수 없습니다".into()),
        Some(false) => {}
        None => return Some("동기화 상태를 확인할 수 없습니다".into()),
    }
    match client.current_world(access_token).await {
        Ok(Some(world)) => match client.my_sync_policy(access_token, &world.id).await {
            Ok(policy) if policy.paused => {
                Some("공동 세계 동기화가 일시정지되어 구매와 장착 변경을 할 수 없습니다".into())
            }
            Ok(_) => None,
            Err(_) => Some("공동 세계 동기화 상태를 확인할 수 없습니다".into()),
        },
        Ok(None) => None,
        Err(_) => Some("공동 세계 동기화 상태를 확인할 수 없습니다".into()),
    }
}

async fn remote_shop_state(
    state: &AppState,
    client: &SupabaseSyncClient,
    access_token: &str,
    import_guest: bool,
) -> Result<(CosmeticShopState, Option<String>), String> {
    let paused_reason = sharing_pause_reason(state, client, access_token).await;
    let guest_import_pending = state
        .ledger
        .lock()
        .map_err(|_| "게스트 구매 기록 오류")?
        .pending_guest_cosmetic_import()
        .map_err(|_| "게스트 구매 기록 오류")?
        .is_some();
    let import_error = if import_guest && guest_import_pending {
        match paused_reason.as_deref() {
            Some(reason) => Some(reason.to_owned()),
            None => import_pending_guest_cosmetics(state, client, access_token)
                .await
                .err(),
        }
    } else {
        None
    };
    let unavailable_reason = paused_reason.or(import_error.clone());
    match client.cosmetic_shop_state(access_token).await {
        Ok(mut shop) => {
            save_remote_shop_state(state, &shop)?;
            let import_still_pending = state
                .ledger
                .lock()
                .map_err(|_| "게스트 구매 기록 오류")?
                .pending_guest_cosmetic_import()
                .map_err(|_| "게스트 구매 기록 오류")?
                .is_some();
            if import_still_pending {
                shop.guest_import_pending = true;
                shop.guest_import_error = import_error.clone();
            }
            shop.action_unavailable_reason = unavailable_reason.clone();
            Ok((shop, unavailable_reason))
        }
        Err(SyncError::Transport) => {
            let reason = unavailable_reason
                .as_deref()
                .unwrap_or("서버에서 상점 상태를 불러오지 못했습니다");
            let shop = local_shop_state(state, Some(reason))?;
            Ok((shop, Some(reason.to_owned())))
        }
        Err(error) => Err(sync_error_reason(error).into()),
    }
}

fn purchase_action(
    state: &AppState,
    result: Option<crate::domain::cosmetic_shop::CosmeticPurchaseResult>,
    unavailable_reason: Option<&str>,
) -> Result<CosmeticPurchaseAction, String> {
    Ok(CosmeticPurchaseAction {
        result,
        state: local_shop_state(state, unavailable_reason)?,
        unavailable_reason: unavailable_reason.map(str::to_owned),
    })
}

fn equip_action(
    state: &AppState,
    result: Option<CosmeticEquipResult>,
    unavailable_reason: Option<&str>,
) -> Result<CosmeticEquipAction, String> {
    Ok(CosmeticEquipAction {
        result,
        state: local_shop_state(state, unavailable_reason)?,
        unavailable_reason: unavailable_reason.map(str::to_owned),
    })
}

fn stale_equip_result(
    requested_cycle_id: &str,
    expected_version: u64,
    state: &CosmeticShopState,
    slot_id: &str,
) -> Option<CosmeticEquipResult> {
    let status = if requested_cycle_id != state.current_cycle_id {
        CosmeticEquipStatus::CycleMismatch
    } else if expected_version != state.slot_versions.get(slot_id).copied().unwrap_or(0) {
        CosmeticEquipStatus::VersionConflict
    } else {
        return None;
    };
    Some(CosmeticEquipResult {
        status,
        cycle_id: state.current_cycle_id.clone(),
        slot_id: slot_id.to_owned(),
        sku: state
            .equipped
            .iter()
            .find(|item| item.slot_id == slot_id)
            .map(|item| item.sku.clone()),
        version: state.slot_versions.get(slot_id).copied().unwrap_or(0),
    })
}

#[tauri::command]
pub async fn get_shop_state(state: State<'_, AppState>) -> Result<ShopState, String> {
    let _gate = state.sync_gate.lock().await;
    match shop_session(&state).await? {
        ShopSession::Guest => {
            get_shop_state_for_session(&state.ledger, CanonicalShopSession::Guest, None).await
        }
        ShopSession::Unavailable(reason) => {
            get_shop_state_for_session(
                &state.ledger,
                CanonicalShopSession::Unavailable(reason.into()),
                None,
            )
            .await
        }
        ShopSession::Online { config, session } => {
            let client = SupabaseSyncClient::new(&config.base_url, &config.publishable_key);
            let canonical_session = online_shop_context(
                &state.ledger,
                &session.user.id,
                &session.access_token,
            )?;
            get_shop_state_for_session(&state.ledger, canonical_session, Some(&client)).await
        }
    }
}

#[tauri::command]
pub async fn quote_shop_action(
    target: QuoteTarget,
    state: State<'_, AppState>,
) -> Result<ShopQuote, String> {
    let _gate = state.sync_gate.lock().await;
    match shop_session(&state).await? {
        ShopSession::Guest => {
            quote_shop_action_for_session(&state.ledger, CanonicalShopSession::Guest, None, &target)
                .await
        }
        ShopSession::Unavailable(reason) => {
            quote_shop_action_for_session(
                &state.ledger,
                CanonicalShopSession::Unavailable(reason.into()),
                None,
                &target,
            )
            .await
        }
        ShopSession::Online { config, session } => {
            let client = SupabaseSyncClient::new(&config.base_url, &config.publishable_key);
            let canonical_session = online_shop_context(
                &state.ledger,
                &session.user.id,
                &session.access_token,
            )?;
            if !matches!(target, QuoteTarget::RemoveNatural { .. }) {
                if let Some(reason) =
                    sharing_pause_reason(&state, &client, &session.access_token).await
                {
                    return Err(reason);
                }
            }
            quote_shop_action_for_session(
                &state.ledger,
                canonical_session,
                Some(&client),
                &target,
            )
            .await
        }
    }
}

#[tauri::command]
pub async fn apply_shop_action(
    request: ShopRequest,
    state: State<'_, AppState>,
) -> Result<ShopActionResult, String> {
    let _gate = state.sync_gate.lock().await;
    match shop_session(&state).await? {
        ShopSession::Guest => {
            apply_shop_request_for_session(
                &state.ledger,
                CanonicalShopSession::Guest,
                None,
                &request,
            )
            .await
        }
        ShopSession::Unavailable(reason) => {
            apply_shop_request_for_session(
                &state.ledger,
                CanonicalShopSession::Unavailable(reason.into()),
                None,
                &request,
            )
            .await
        }
        ShopSession::Online { config, session } => {
            let client = SupabaseSyncClient::new(&config.base_url, &config.publishable_key);
            let canonical_session = online_shop_context(
                &state.ledger,
                &session.user.id,
                &session.access_token,
            )?;
            if !matches!(
                request,
                ShopRequest::RemoveNatural { .. } | ShopRequest::ResetPlanet { .. }
            ) {
                if let Some(reason) = sharing_pause_reason(&state, &client, &session.access_token).await {
                    if let CanonicalShopSession::Online {
                        account_id,
                        cycle_id,
                        ..
                    } = &canonical_session
                    {
                        ensure_shop_context(&state.ledger, account_id, cycle_id)?;
                    }
                    return offline_shop_action(&state.ledger, &request, &reason);
                }
            }
            apply_shop_request_for_session(
                &state.ledger,
                canonical_session,
                Some(&client),
                &request,
            )
            .await
        }
    }
}

#[tauri::command]
pub async fn get_legacy_cosmetic_shop_state(
    state: State<'_, AppState>,
) -> Result<CosmeticShopState, String> {
    let _gate = state.sync_gate.lock().await;
    match shop_session(&state).await? {
        ShopSession::Guest => local_shop_state(&state, None),
        ShopSession::Unavailable(reason) => local_shop_state(&state, Some(reason)),
        ShopSession::Online { config, session } => {
            let client = SupabaseSyncClient::new(&config.base_url, &config.publishable_key);
            let (shop, _) = remote_shop_state(&state, &client, &session.access_token, true).await?;
            Ok(shop)
        }
    }
}

#[tauri::command]
pub async fn purchase_cosmetic(
    sku: String,
    state: State<'_, AppState>,
) -> Result<CosmeticPurchaseAction, String> {
    let _gate = state.sync_gate.lock().await;
    match shop_session(&state).await? {
        ShopSession::Guest => {
            let purchase_id = uuid::Uuid::new_v4().to_string();
            let result = state
                .ledger
                .lock()
                .map_err(|_| "상점 저장소 오류")?
                .purchase_guest_cosmetic(&purchase_id, &sku)
                .map_err(|_| "상품을 구매할 수 없습니다")?;
            purchase_action(&state, Some(result), None)
        }
        ShopSession::Unavailable(reason) => purchase_action(&state, None, Some(reason)),
        ShopSession::Online { config, session } => {
            let client = SupabaseSyncClient::new(&config.base_url, &config.publishable_key);
            let (shop, import_error) =
                remote_shop_state(&state, &client, &session.access_token, true).await?;
            if let Some(reason) = import_error {
                return purchase_action(&state, None, Some(&reason));
            }
            let product = shop
                .products
                .iter()
                .find(|product| product.sku == sku && product.purchasable)
                .ok_or("구매할 수 없는 상품입니다")?;
            let purchase_id = state
                .ledger
                .lock()
                .map_err(|_| "상점 저장소 오류")?
                .prepare_cosmetic_purchase_request(&sku, product.catalog_revision)
                .map_err(|_| "구매 요청을 저장할 수 없습니다")?;
            let result = match client
                .purchase_cosmetic(
                    &session.access_token,
                    &purchase_id,
                    &sku,
                    product.catalog_revision,
                )
                .await
            {
                Ok(result) => result,
                Err(error) => return purchase_action(&state, None, Some(sync_error_reason(error))),
            };
            state
                .ledger
                .lock()
                .map_err(|_| "상점 저장소 오류")?
                .complete_cosmetic_purchase_request(&result)
                .map_err(|_| "구매 결과를 저장할 수 없습니다")?;
            if let Ok(canonical) = client.cosmetic_shop_state(&session.access_token).await {
                save_remote_shop_state(&state, &canonical)?;
            }
            purchase_action(&state, Some(result), None)
        }
    }
}

#[tauri::command]
pub async fn equip_cosmetic(
    slot_id: String,
    sku: Option<String>,
    cycle_id: String,
    expected_version: u64,
    state: State<'_, AppState>,
) -> Result<CosmeticEquipAction, String> {
    let _gate = state.sync_gate.lock().await;
    match shop_session(&state).await? {
        ShopSession::Guest => {
            let result = (|| -> Result<CosmeticEquipResult, String> {
                let mut ledger = state.ledger.lock().map_err(|_| "상점 저장소 오류")?;
                let shop = ledger
                    .cosmetic_shop_state()
                    .map_err(|_| "상점 상태를 읽을 수 없습니다")?;
                if let Some(result) = stale_equip_result(&cycle_id, expected_version, &shop, &slot_id) {
                    return Ok(result);
                }
                let version = ledger
                    .equip_guest_cosmetic(&slot_id, sku.as_deref())
                    .map_err(|_| "상품을 장착할 수 없습니다")?;
                Ok(CosmeticEquipResult {
                    status: if sku.is_some() {
                        CosmeticEquipStatus::Equipped
                    } else {
                        CosmeticEquipStatus::Unequipped
                    },
                    cycle_id: shop.current_cycle_id,
                    slot_id: slot_id.clone(),
                    sku: sku.clone(),
                    version,
                })
            })();
            match result {
                Ok(result) => equip_action(&state, Some(result), None),
                Err(reason) => equip_action(&state, None, Some(&reason)),
            }
        }
        ShopSession::Unavailable(reason) => equip_action(&state, None, Some(reason)),
        ShopSession::Online { config, session } => {
            let client = SupabaseSyncClient::new(&config.base_url, &config.publishable_key);
            let (_, import_error) =
                remote_shop_state(&state, &client, &session.access_token, true).await?;
            if let Some(reason) = import_error {
                return equip_action(&state, None, Some(&reason));
            }
            let result = match client
                .equip_cosmetic(
                    &session.access_token,
                    &cycle_id,
                    &slot_id,
                    sku.as_deref(),
                    expected_version,
                )
                .await
            {
                Ok(result) => result,
                Err(error) => return equip_action(&state, None, Some(sync_error_reason(error))),
            };
            if let Ok(canonical) = client.cosmetic_shop_state(&session.access_token).await {
                save_remote_shop_state(&state, &canonical)?;
            } else if !result.cycle_id.is_empty() {
                state
                    .ledger
                    .lock()
                    .map_err(|_| "상점 저장소 오류")?
                    .apply_confirmed_cosmetic_equipment(&result)
                    .map_err(|_| "장착 결과를 저장할 수 없습니다")?;
            }
            equip_action(&state, Some(result), None)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        apply_shop_request_for_session, get_shop_state_for_session, quote_shop_action_for_session,
        stale_equip_result, CanonicalShopSession, ShopRpc, ShopRpcFuture,
    };
    use crate::domain::cosmetic_shop::{
        ActiveEffects, AvatarEquipment, AvatarEquipmentItem, CosmeticEquipStatus,
        CosmeticShopState, CosmeticSlot, EquippedCosmetic, NaturalObjectKey, QuoteTarget,
        RewardState, ShopActionResult, ShopActionStatus, ShopQuote, ShopRequest, ShopState,
    };
    use crate::storage::ledger::Ledger;
    use crate::sync::client::SyncError;
    use std::sync::{mpsc, Arc, Mutex};
    use std::time::Duration;

    #[derive(Default)]
    struct TestShopRpc {
        get_result: Mutex<Option<Result<ShopState, SyncError>>>,
        get_calls: Mutex<Vec<String>>,
        quote_result: Mutex<Option<Result<ShopQuote, SyncError>>>,
        quote_calls: Mutex<Vec<(String, QuoteTarget)>>,
        apply_result: Mutex<Option<Result<ShopActionResult, SyncError>>>,
        apply_calls: Mutex<Vec<(String, ShopRequest)>>,
    }

    impl ShopRpc for TestShopRpc {
        fn get_state<'a>(&'a self, access_token: &'a str) -> ShopRpcFuture<'a, ShopState> {
            self.get_calls.lock().unwrap().push(access_token.to_owned());
            let result = self.get_result.lock().unwrap().take().unwrap();
            Box::pin(async move { result })
        }

        fn quote_action<'a>(
            &'a self,
            access_token: &'a str,
            target: &'a QuoteTarget,
        ) -> ShopRpcFuture<'a, ShopQuote> {
            self.quote_calls
                .lock()
                .unwrap()
                .push((access_token.to_owned(), target.clone()));
            let result = self.quote_result.lock().unwrap().take().unwrap();
            Box::pin(async move { result })
        }

        fn apply_action<'a>(
            &'a self,
            access_token: &'a str,
            request: &'a ShopRequest,
        ) -> ShopRpcFuture<'a, ShopActionResult> {
            self.apply_calls
                .lock()
                .unwrap()
                .push((access_token.to_owned(), request.clone()));
            let result = self.apply_result.lock().unwrap().take().unwrap();
            Box::pin(async move { result })
        }
    }

    struct DeferredApplyRpc {
        started: mpsc::Sender<()>,
        result: Mutex<
            Option<tokio::sync::oneshot::Receiver<Result<ShopActionResult, SyncError>>>,
        >,
    }

    impl ShopRpc for DeferredApplyRpc {
        fn get_state<'a>(&'a self, _access_token: &'a str) -> ShopRpcFuture<'a, ShopState> {
            Box::pin(async { Err(SyncError::Transport) })
        }

        fn quote_action<'a>(
            &'a self,
            _access_token: &'a str,
            _target: &'a QuoteTarget,
        ) -> ShopRpcFuture<'a, ShopQuote> {
            Box::pin(async { Err(SyncError::Transport) })
        }

        fn apply_action<'a>(
            &'a self,
            _access_token: &'a str,
            _request: &'a ShopRequest,
        ) -> ShopRpcFuture<'a, ShopActionResult> {
            self.started.send(()).unwrap();
            let result = self.result.lock().unwrap().take().unwrap();
            Box::pin(async move { result.await.unwrap_or(Err(SyncError::Transport)) })
        }
    }

    fn test_ledger() -> (tempfile::TempDir, Mutex<Ledger>) {
        let directory = tempfile::tempdir().unwrap();
        let ledger = Ledger::open(&directory.path().join("ledger.sqlite3"), chrono_tz::UTC)
            .unwrap();
        (directory, Mutex::new(ledger))
    }

    fn canonical_state(account_id: &str, cycle_id: &str, revision: u64) -> ShopState {
        let empty_item = || AvatarEquipmentItem {
            sku: None,
            version: 0,
        };
        ShopState {
            account_id: account_id.into(),
            current_cycle_id: cycle_id.into(),
            catalog_revision: 1,
            state_revision: revision,
            available_balance: 12_345,
            products: vec![],
            landscape_instances: vec![],
            placements: vec![],
            removed_natural_keys: vec![],
            avatar_owned_skus: vec![],
            avatar_equipment: AvatarEquipment {
                head: empty_item(),
                outfit: empty_item(),
                face: empty_item(),
                back: empty_item(),
            },
            effects: ActiveEffects::default(),
            reward_state: RewardState::default(),
            action_unavailable_reason: None,
            guest_import_pending: false,
            guest_import_error: None,
        }
    }

    fn online_session(ledger: &Mutex<Ledger>, access_token: &str) -> CanonicalShopSession {
        let ledger = ledger.lock().unwrap();
        CanonicalShopSession::Online {
            account_id: ledger.cosmetic_account_id().unwrap(),
            cycle_id: ledger.planet_cycle_id().unwrap(),
            access_token: access_token.into(),
        }
    }

    fn placement_request(cycle_id: &str) -> ShopRequest {
        ShopRequest::Place {
            request_id: "request-42".into(),
            cycle_id: cycle_id.into(),
            instance_id: "instance-1".into(),
            expected_version: 7,
            x: 0.25,
            y: 0.75,
        }
    }

    fn natural_removal_request(cycle_id: &str) -> ShopRequest {
        let key = NaturalObjectKey {
            cycle_id: cycle_id.into(),
            stage: 0,
            ordinal: 0,
        };
        ShopRequest::RemoveNatural {
            request_id: "remove-1".into(),
            key: key.clone(),
            expected_version: 0,
            quote: ShopQuote {
                target: QuoteTarget::RemoveNatural { key },
                catalog_revision: 1,
                effect_revision: 1,
                price: 100_000,
            },
        }
    }

    #[test]
    fn online_getter_caches_the_authenticated_canonical_state() {
        let (_directory, ledger) = test_ledger();
        ledger.lock().unwrap().ensure_planet_account("alice").unwrap();
        let cycle_id = ledger.lock().unwrap().planet_cycle_id().unwrap();
        let rpc = TestShopRpc::default();
        *rpc.get_result.lock().unwrap() = Some(Ok(canonical_state("account:alice", &cycle_id, 4)));

        let state = tauri::async_runtime::block_on(get_shop_state_for_session(
            &ledger,
            online_session(&ledger, "alice-token"),
            Some(&rpc),
        ))
        .unwrap();

        assert_eq!(state.state_revision, 4);
        assert_eq!(*rpc.get_calls.lock().unwrap(), vec!["alice-token"]);
        assert_eq!(ledger.lock().unwrap().shop_state().unwrap().state_revision, 4);
    }

    #[test]
    fn online_getter_accepts_server_cycle_without_resetting_local_cycle() {
        let (_directory, ledger) = test_ledger();
        ledger.lock().unwrap().ensure_planet_account("alice").unwrap();
        let cycle_id = ledger.lock().unwrap().planet_cycle_id().unwrap();
        let rpc = TestShopRpc::default();
        *rpc.get_result.lock().unwrap() = Some(Ok(canonical_state(
            "account:alice",
            "foreign-cycle",
            4,
        )));

        let state = tauri::async_runtime::block_on(get_shop_state_for_session(
            &ledger,
            online_session(&ledger, "alice-token"),
            Some(&rpc),
        ))
        .unwrap();

        assert_eq!(state.current_cycle_id, "foreign-cycle");
        let ledger = ledger.lock().unwrap();
        let cached_json: String = ledger
            .connection
            .query_row("SELECT state_json FROM shop_remote_state", [], |row| row.get(0))
            .unwrap();
        let cached: ShopState = serde_json::from_str(&cached_json).unwrap();
        assert_eq!(cached.current_cycle_id, "foreign-cycle");
        assert_eq!(cycle_id, ledger.planet_cycle_id().unwrap());
    }

    #[test]
    fn online_quote_uses_the_authenticated_target_and_returns_matching_quote() {
        let (_directory, ledger) = test_ledger();
        ledger.lock().unwrap().ensure_planet_account("alice").unwrap();
        let target = QuoteTarget::Purchase {
            sku: "land_pond".into(),
        };
        let expected_quote = ShopQuote {
            target: target.clone(),
            catalog_revision: 3,
            effect_revision: 5,
            price: 4_500_000,
        };
        let rpc = TestShopRpc::default();
        *rpc.quote_result.lock().unwrap() = Some(Ok(expected_quote.clone()));

        let quote = tauri::async_runtime::block_on(quote_shop_action_for_session(
            &ledger,
            online_session(&ledger, "alice-token"),
            Some(&rpc),
            &target,
        ))
        .unwrap();

        assert_eq!(quote, expected_quote);
        assert_eq!(
            *rpc.quote_calls.lock().unwrap(),
            vec![("alice-token".into(), target)]
        );
    }

    #[test]
    fn online_quote_rejects_a_server_quote_for_a_different_target() {
        let (_directory, ledger) = test_ledger();
        ledger.lock().unwrap().ensure_planet_account("alice").unwrap();
        let target = QuoteTarget::Purchase {
            sku: "land_pond".into(),
        };
        let rpc = TestShopRpc::default();
        *rpc.quote_result.lock().unwrap() = Some(Ok(ShopQuote {
            target: QuoteTarget::Purchase {
                sku: "land_well".into(),
            },
            catalog_revision: 3,
            effect_revision: 5,
            price: 15_000_000,
        }));

        let result = tauri::async_runtime::block_on(quote_shop_action_for_session(
            &ledger,
            online_session(&ledger, "alice-token"),
            Some(&rpc),
            &target,
        ));

        assert!(result.is_err());
        assert_eq!(rpc.quote_calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn signed_natural_removal_quote_uses_the_authenticated_shop_rpc() {
        let (_directory, ledger) = test_ledger();
        ledger.lock().unwrap().ensure_planet_account("alice").unwrap();
        let cycle_id = ledger.lock().unwrap().planet_cycle_id().unwrap();
        let target = QuoteTarget::RemoveNatural {
            key: NaturalObjectKey {
                cycle_id,
                stage: 0,
                ordinal: 0,
            },
        };
        let rpc = TestShopRpc::default();
        let expected_quote = ShopQuote {
            target: target.clone(),
            catalog_revision: 3,
            effect_revision: 7,
            price: 100_000,
        };
        *rpc.quote_result.lock().unwrap() = Some(Ok(expected_quote.clone()));

        let result = tauri::async_runtime::block_on(quote_shop_action_for_session(
            &ledger,
            online_session(&ledger, "alice-token"),
            Some(&rpc),
            &target,
        ));

        assert_eq!(result.unwrap(), expected_quote);
        assert_eq!(
            *rpc.quote_calls.lock().unwrap(),
            vec![("alice-token".into(), target)]
        );
    }

    #[test]
    fn signed_natural_removal_quote_rejects_a_different_cycle_before_rpc() {
        let (_directory, ledger) = test_ledger();
        ledger.lock().unwrap().ensure_planet_account("alice").unwrap();
        let target = QuoteTarget::RemoveNatural {
            key: NaturalObjectKey {
                cycle_id: "other-cycle".into(),
                stage: 0,
                ordinal: 0,
            },
        };
        let rpc = TestShopRpc::default();

        let result = tauri::async_runtime::block_on(quote_shop_action_for_session(
            &ledger,
            online_session(&ledger, "alice-token"),
            Some(&rpc),
            &target,
        ));

        assert!(result.is_err());
        assert!(rpc.quote_calls.lock().unwrap().is_empty());
    }

    #[test]
    fn signed_natural_removal_rejects_a_request_quote_for_another_key_before_rpc() {
        let (_directory, ledger) = test_ledger();
        ledger.lock().unwrap().ensure_planet_account("alice").unwrap();
        let cycle_id = ledger.lock().unwrap().planet_cycle_id().unwrap();
        let mut request = natural_removal_request(&cycle_id);
        if let ShopRequest::RemoveNatural { quote, .. } = &mut request {
            quote.target = QuoteTarget::RemoveNatural {
                key: NaturalObjectKey {
                    cycle_id: cycle_id.clone(),
                    stage: 1,
                    ordinal: 0,
                },
            };
        }
        let rpc = TestShopRpc::default();

        let result = tauri::async_runtime::block_on(apply_shop_request_for_session(
            &ledger,
            online_session(&ledger, "alice-token"),
            Some(&rpc),
            &request,
        ));

        assert!(result.is_err());
        assert!(rpc.apply_calls.lock().unwrap().is_empty());
    }

    #[test]
    fn signed_offline_getter_returns_only_the_confirmed_local_projection() {
        let (_directory, ledger) = test_ledger();
        ledger.lock().unwrap().ensure_planet_account("alice").unwrap();

        let state = tauri::async_runtime::block_on(get_shop_state_for_session(
            &ledger,
            CanonicalShopSession::Unavailable("서버에 연결할 수 없습니다".into()),
            None,
        ))
        .unwrap();

        assert_eq!(state.account_id, "account:alice");
        assert_eq!(state.available_balance, 0);
        assert_eq!(
            state.action_unavailable_reason.as_deref(),
            Some("서버에 연결할 수 없습니다")
        );
        assert!(state.landscape_instances.is_empty());
    }

    #[test]
    fn online_reset_stays_unavailable_without_rpc_or_local_mutation() {
        let (_directory, ledger) = test_ledger();
        ledger.lock().unwrap().ensure_planet_account("alice").unwrap();
        let cycle_id = ledger.lock().unwrap().planet_cycle_id().unwrap();
        let before = ledger.lock().unwrap().shop_state().unwrap();
        let request = ShopRequest::ResetPlanet {
            request_id: "reset-1".into(),
            cycle_id: cycle_id.clone(),
        };
        let rpc = TestShopRpc::default();

        let result = tauri::async_runtime::block_on(apply_shop_request_for_session(
                &ledger,
                online_session(&ledger, "alice-token"),
                Some(&rpc),
                &request,
            ))
            .unwrap();
        assert_eq!(result.status, ShopActionStatus::Unavailable);
        assert_eq!(result.request_id, super::request_id(&request));

        assert!(rpc.apply_calls.lock().unwrap().is_empty());
        assert_eq!(ledger.lock().unwrap().planet_cycle_id().unwrap(), cycle_id);
        let after = ledger.lock().unwrap().shop_state().unwrap();
        assert_eq!(after.available_balance, before.available_balance);
        assert_eq!(after.landscape_instances, before.landscape_instances);
    }

    #[test]
    fn signed_natural_removal_applies_and_caches_only_the_confirmed_tombstone() {
        let (_directory, ledger) = test_ledger();
        ledger.lock().unwrap().ensure_planet_account("alice").unwrap();
        let cycle_id = ledger.lock().unwrap().planet_cycle_id().unwrap();
        let request = natural_removal_request(&cycle_id);
        let key = match &request {
            ShopRequest::RemoveNatural { key, .. } => key.clone(),
            _ => unreachable!(),
        };
        let quote = match &request {
            ShopRequest::RemoveNatural { quote, .. } => quote.clone(),
            _ => unreachable!(),
        };
        let mut state = canonical_state("account:alice", &cycle_id, 8);
        state.available_balance = 11_111;
        state.removed_natural_keys = vec![key.clone()];
        let rpc = TestShopRpc::default();
        *rpc.apply_result.lock().unwrap() = Some(Ok(ShopActionResult {
            status: ShopActionStatus::Removed,
            request_id: "remove-1".into(),
            confirmed_quote: Some(quote.clone()),
            state: state.clone(),
        }));

        let result = tauri::async_runtime::block_on(apply_shop_request_for_session(
            &ledger,
            online_session(&ledger, "alice-token"),
            Some(&rpc),
            &request,
        ))
        .unwrap();

        assert_eq!(result.status, ShopActionStatus::Removed);
        assert_eq!(result.confirmed_quote, Some(quote));
        assert_eq!(result.state, state);
        assert_eq!(
            *rpc.apply_calls.lock().unwrap(),
            vec![("alice-token".into(), request)]
        );
        let cached = ledger.lock().unwrap().shop_state().unwrap();
        assert_eq!(cached.removed_natural_keys, vec![key]);
        assert_eq!(cached.available_balance, 11_111);
        let client_wallet_rows: i64 = ledger
            .lock()
            .unwrap()
            .connection
            .query_row("SELECT count(*) FROM planet_wallet_credit", [], |row| row.get(0))
            .unwrap();
        assert_eq!(client_wallet_rows, 0);
    }

    #[test]
    fn signed_natural_removal_rejects_mismatched_server_results_before_caching() {
        for mismatch in ["request", "account", "cycle", "target", "quote", "tombstone"] {
            let (_directory, ledger) = test_ledger();
            ledger.lock().unwrap().ensure_planet_account("alice").unwrap();
            let cycle_id = ledger.lock().unwrap().planet_cycle_id().unwrap();
            let request = natural_removal_request(&cycle_id);
            let (key, quote) = match &request {
                ShopRequest::RemoveNatural { key, quote, .. } => (key.clone(), quote.clone()),
                _ => unreachable!(),
            };
            let mut state = canonical_state("account:alice", &cycle_id, 8);
            state.removed_natural_keys = vec![key.clone()];
            let mut result = ShopActionResult {
                status: ShopActionStatus::Removed,
                request_id: "remove-1".into(),
                confirmed_quote: Some(quote.clone()),
                state,
            };
            match mismatch {
                "request" => result.request_id = "other-request".into(),
                "account" => result.state.account_id = "account:bob".into(),
                "cycle" => result.state.current_cycle_id = "other-cycle".into(),
                "target" => {
                    result.confirmed_quote.as_mut().unwrap().target =
                        QuoteTarget::RemoveNatural {
                            key: NaturalObjectKey {
                                cycle_id: cycle_id.clone(),
                                stage: 1,
                                ordinal: 0,
                            },
                        };
                }
                "quote" => result.confirmed_quote.as_mut().unwrap().price += 1,
                "tombstone" => result.state.removed_natural_keys.clear(),
                _ => unreachable!(),
            }
            let rpc = TestShopRpc::default();
            *rpc.apply_result.lock().unwrap() = Some(Ok(result));

            let response = tauri::async_runtime::block_on(apply_shop_request_for_session(
                &ledger,
                online_session(&ledger, "alice-token"),
                Some(&rpc),
                &request,
            ));

            assert!(response.is_err(), "unexpectedly accepted {mismatch} mismatch");
            let cached_count: i64 = ledger
                .lock()
                .unwrap()
                .connection
                .query_row("SELECT count(*) FROM shop_remote_state", [], |row| row.get(0))
                .unwrap();
            assert_eq!(cached_count, 0, "cached {mismatch} mismatch response");
        }
    }

    #[test]
    fn signed_natural_removal_context_switch_during_rpc_does_not_change_cache() {
        for switch_account in [false, true] {
            let (_directory, ledger) = test_ledger();
            ledger.lock().unwrap().ensure_planet_account("alice").unwrap();
            let ledger = Arc::new(ledger);
            let cycle_id = ledger.lock().unwrap().planet_cycle_id().unwrap();
            let original_state = canonical_state("account:alice", &cycle_id, 3);
            ledger
                .lock()
                .unwrap()
                .store_confirmed_shop_state(&original_state)
                .unwrap();
            let original_cache: String = ledger
                .lock()
                .unwrap()
                .connection
                .query_row(
                    "SELECT state_json FROM shop_remote_state WHERE account_id='account:alice'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            let request = natural_removal_request(&cycle_id);
            let (key, quote) = match &request {
                ShopRequest::RemoveNatural { key, quote, .. } => (key.clone(), quote.clone()),
                _ => unreachable!(),
            };
            let mut state = canonical_state("account:alice", &cycle_id, 8);
            state.removed_natural_keys = vec![key];
            let result = ShopActionResult {
                status: ShopActionStatus::Removed,
                request_id: "remove-1".into(),
                confirmed_quote: Some(quote),
                state,
            };
            let (response_sender, response_receiver) = tokio::sync::oneshot::channel();
            let (started_sender, started_receiver) = mpsc::channel();
            let rpc = Arc::new(DeferredApplyRpc {
                started: started_sender,
                result: Mutex::new(Some(response_receiver)),
            });
            let session = online_session(&ledger, "alice-token");
            let thread_ledger = Arc::clone(&ledger);
            let thread_rpc = Arc::clone(&rpc);
            let worker = std::thread::spawn(move || {
                tauri::async_runtime::block_on(apply_shop_request_for_session(
                    thread_ledger.as_ref(),
                    session,
                    Some(thread_rpc.as_ref()),
                    &request,
                ))
            });

            started_receiver
                .recv_timeout(Duration::from_secs(2))
                .expect("RPC should start before switching the local context");
            if switch_account {
                ledger.lock().unwrap().ensure_planet_account("bob").unwrap();
            } else {
                ledger
                    .lock()
                    .unwrap()
                    .connection
                    .execute(
                        "UPDATE setting SET value='switched-cycle' WHERE key='planet_current_cycle_id'",
                        [],
                    )
                    .unwrap();
            }
            response_sender.send(Ok(result)).unwrap();

            assert!(worker.join().unwrap().is_err());
            let cached_for_alice: String = ledger
                .lock()
                .unwrap()
                .connection
                .query_row(
                    "SELECT state_json FROM shop_remote_state WHERE account_id='account:alice'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(cached_for_alice, original_cache);
        }
    }

    #[test]
    fn signed_natural_removal_transport_error_preserves_cache_and_retry_keeps_request_id() {
        let (_directory, ledger) = test_ledger();
        ledger.lock().unwrap().ensure_planet_account("alice").unwrap();
        let cycle_id = ledger.lock().unwrap().planet_cycle_id().unwrap();
        let request = natural_removal_request(&cycle_id);
        let before = canonical_state("account:alice", &cycle_id, 4);
        ledger
            .lock()
            .unwrap()
            .store_confirmed_shop_state(&before)
            .unwrap();
        let rpc = TestShopRpc::default();
        *rpc.apply_result.lock().unwrap() = Some(Err(SyncError::Transport));

        let first = tauri::async_runtime::block_on(apply_shop_request_for_session(
            &ledger,
            online_session(&ledger, "alice-token"),
            Some(&rpc),
            &request,
        ));
        assert!(first.is_err());
        assert_eq!(ledger.lock().unwrap().shop_state().unwrap(), before);
        assert_eq!(rpc.apply_calls.lock().unwrap().len(), 1);

        let (key, quote) = match &request {
            ShopRequest::RemoveNatural { key, quote, .. } => (key.clone(), quote.clone()),
            _ => unreachable!(),
        };
        let mut after = canonical_state("account:alice", &cycle_id, 5);
        after.removed_natural_keys = vec![key];
        *rpc.apply_result.lock().unwrap() = Some(Ok(ShopActionResult {
            status: ShopActionStatus::Removed,
            request_id: "remove-1".into(),
            confirmed_quote: Some(quote),
            state: after,
        }));
        let second = tauri::async_runtime::block_on(apply_shop_request_for_session(
            &ledger,
            online_session(&ledger, "alice-token"),
            Some(&rpc),
            &request,
        ))
        .unwrap();

        assert_eq!(second.status, ShopActionStatus::Removed);
        let calls = rpc.apply_calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].1, request);
        assert_eq!(calls[1].1, request);
    }

    #[test]
    fn guest_natural_removal_still_uses_the_local_ledger_without_rpc() {
        let (_directory, ledger) = test_ledger();
        {
            let ledger = ledger.lock().unwrap();
            ledger.ensure_planet_object(0, 0, "tree", 50, 50, 17).unwrap();
            ledger
                .connection
                .execute(
                    "INSERT INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
                     VALUES ('seed-wallet',500000,'2026-10-01T00:00:00Z')",
                    [],
                )
                .unwrap();
        }
        let cycle_id = ledger.lock().unwrap().planet_cycle_id().unwrap();
        let key = NaturalObjectKey {
            cycle_id,
            stage: 0,
            ordinal: 0,
        };
        let target = QuoteTarget::RemoveNatural { key: key.clone() };
        let quote = tauri::async_runtime::block_on(quote_shop_action_for_session(
            &ledger,
            CanonicalShopSession::Guest,
            None,
            &target,
        ))
        .unwrap();
        let request = ShopRequest::RemoveNatural {
            request_id: "guest-remove-1".into(),
            key: key.clone(),
            expected_version: 0,
            quote: quote.clone(),
        };

        let result = tauri::async_runtime::block_on(apply_shop_request_for_session(
            &ledger,
            CanonicalShopSession::Guest,
            None,
            &request,
        ))
        .unwrap();

        assert_eq!(result.status, ShopActionStatus::Removed);
        assert_eq!(result.confirmed_quote, Some(quote));
        assert_eq!(result.state.removed_natural_keys, vec![key]);
        assert_eq!(result.state.available_balance, 400_000);
    }

    #[test]
    fn online_action_rejects_a_response_with_a_different_request_id_before_caching() {
        let (_directory, ledger) = test_ledger();
        ledger.lock().unwrap().ensure_planet_account("alice").unwrap();
        let cycle_id = ledger.lock().unwrap().planet_cycle_id().unwrap();
        let request = placement_request(&cycle_id);
        let rpc = TestShopRpc::default();
        *rpc.apply_result.lock().unwrap() = Some(Ok(ShopActionResult {
            status: ShopActionStatus::Placed,
            request_id: "another-request".into(),
            confirmed_quote: None,
            state: canonical_state("account:alice", &cycle_id, 8),
        }));

        let result = tauri::async_runtime::block_on(apply_shop_request_for_session(
            &ledger,
            online_session(&ledger, "alice-token"),
            Some(&rpc),
            &request,
        ));

        assert!(result.is_err());
        let cached_count: i64 = ledger
            .lock()
            .unwrap()
            .connection
            .query_row("SELECT count(*) FROM shop_remote_state", [], |row| row.get(0))
            .unwrap();
        assert_eq!(cached_count, 0);
    }

    #[test]
    fn online_action_does_not_send_after_captured_account_context_changes() {
        let (_directory, ledger) = test_ledger();
        ledger.lock().unwrap().ensure_planet_account("alice").unwrap();
        let cycle_id = ledger.lock().unwrap().planet_cycle_id().unwrap();
        let request = placement_request(&cycle_id);
        let rpc = TestShopRpc::default();
        let session = CanonicalShopSession::Online {
            account_id: "account:bob".into(),
            cycle_id,
            access_token: "alice-token".into(),
        };

        let result = tauri::async_runtime::block_on(apply_shop_request_for_session(
            &ledger,
            session,
            Some(&rpc),
            &request,
        ));

        assert!(result.is_err());
        assert!(rpc.apply_calls.lock().unwrap().is_empty());
    }

    #[test]
    fn guest_shop_purchase_uses_the_local_ledger_without_an_rpc() {
        let (_directory, ledger) = test_ledger();
        ledger
            .lock()
            .unwrap()
            .connection
            .execute(
                "INSERT INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
                 VALUES ('seed-wallet',5000000,'2026-09-30T00:00:00Z')",
                [],
            )
            .unwrap();
        let target = QuoteTarget::Purchase {
            sku: "land_pond".into(),
        };
        let quote = tauri::async_runtime::block_on(quote_shop_action_for_session(
            &ledger,
            CanonicalShopSession::Guest,
            None,
            &target,
        ))
        .unwrap();
        assert_eq!(quote.price, 5_000_000);
        let request = ShopRequest::Purchase {
            request_id: "guest-purchase-1".into(),
            quote,
        };

        let result = tauri::async_runtime::block_on(apply_shop_request_for_session(
            &ledger,
            CanonicalShopSession::Guest,
            None,
            &request,
        ))
        .unwrap();

        assert_eq!(result.status, ShopActionStatus::Purchased);
        assert_eq!(result.state.account_id, "local");
        assert_eq!(result.state.available_balance, 0);
        assert_eq!(result.state.landscape_instances.len(), 1);
        assert_eq!(result.state.landscape_instances[0].sku, "land_pond");
        assert_eq!(result.request_id, "guest-purchase-1");
    }

    #[test]
    fn online_shop_action_sends_the_same_request_and_caches_only_the_confirmed_state() {
        let (_directory, ledger) = test_ledger();
        ledger.lock().unwrap().ensure_planet_account("alice").unwrap();
        let cycle_id = ledger.lock().unwrap().planet_cycle_id().unwrap();
        let request = placement_request(&cycle_id);
        let rpc = TestShopRpc::default();
        *rpc.apply_result.lock().unwrap() = Some(Ok(ShopActionResult {
            status: ShopActionStatus::Placed,
            request_id: "request-42".into(),
            confirmed_quote: None,
            state: canonical_state("account:alice", &cycle_id, 8),
        }));

        let result = tauri::async_runtime::block_on(apply_shop_request_for_session(
            &ledger,
            online_session(&ledger, "alice-token"),
            Some(&rpc),
            &request,
        ))
        .unwrap();

        assert_eq!(result.status, ShopActionStatus::Placed);
        assert_eq!(result.request_id, "request-42");
        assert_eq!(
            *rpc.apply_calls.lock().unwrap(),
            vec![("alice-token".into(), request)]
        );
        let cached = ledger.lock().unwrap().shop_state().unwrap();
        assert_eq!(cached.state_revision, 8);
        assert_eq!(cached.available_balance, 12_345);
    }

    #[test]
    fn online_shop_action_rejects_foreign_state_before_caching() {
        let (_directory, ledger) = test_ledger();
        ledger.lock().unwrap().ensure_planet_account("alice").unwrap();
        let cycle_id = ledger.lock().unwrap().planet_cycle_id().unwrap();
        let request = placement_request(&cycle_id);
        let rpc = TestShopRpc::default();
        *rpc.apply_result.lock().unwrap() = Some(Ok(ShopActionResult {
            status: ShopActionStatus::Placed,
            request_id: "request-42".into(),
            confirmed_quote: None,
            state: canonical_state("account:bob", &cycle_id, 8),
        }));

        let result = tauri::async_runtime::block_on(apply_shop_request_for_session(
            &ledger,
            online_session(&ledger, "alice-token"),
            Some(&rpc),
            &request,
        ));

        assert!(result.is_err());
        let cached_count: i64 = ledger
            .lock()
            .unwrap()
            .connection
            .query_row("SELECT count(*) FROM shop_remote_state", [], |row| row.get(0))
            .unwrap();
        assert_eq!(cached_count, 0);
    }

    #[test]
    fn online_action_rejects_unexpected_cycle_state_for_non_mismatch_status() {
        let (_directory, ledger) = test_ledger();
        ledger.lock().unwrap().ensure_planet_account("alice").unwrap();
        let cycle_id = ledger.lock().unwrap().planet_cycle_id().unwrap();
        let request = placement_request(&cycle_id);
        let rpc = TestShopRpc::default();
        *rpc.apply_result.lock().unwrap() = Some(Ok(ShopActionResult {
            status: ShopActionStatus::Placed,
            request_id: "request-42".into(),
            confirmed_quote: None,
            state: canonical_state("account:alice", "unexpected-cycle", 8),
        }));

        let result = tauri::async_runtime::block_on(apply_shop_request_for_session(
            &ledger,
            online_session(&ledger, "alice-token"),
            Some(&rpc),
            &request,
        ));

        assert!(result.is_err());
        let cached_count: i64 = ledger
            .lock()
            .unwrap()
            .connection
            .query_row("SELECT count(*) FROM shop_remote_state", [], |row| row.get(0))
            .unwrap();
        assert_eq!(cached_count, 0);
        assert_eq!(ledger.lock().unwrap().planet_cycle_id().unwrap(), cycle_id);
    }

    #[test]
    fn cycle_mismatch_action_can_cache_server_cycle_without_local_reset() {
        let (_directory, ledger) = test_ledger();
        ledger.lock().unwrap().ensure_planet_account("alice").unwrap();
        let old_cycle = ledger.lock().unwrap().planet_cycle_id().unwrap();
        let request = placement_request(&old_cycle);
        let rpc = TestShopRpc::default();
        *rpc.apply_result.lock().unwrap() = Some(Ok(ShopActionResult {
            status: ShopActionStatus::CycleMismatch,
            request_id: "request-42".into(),
            confirmed_quote: None,
            state: canonical_state("account:alice", "new-server-cycle", 8),
        }));

        let result = tauri::async_runtime::block_on(apply_shop_request_for_session(
            &ledger,
            online_session(&ledger, "alice-token"),
            Some(&rpc),
            &request,
        ))
        .unwrap();

        assert_eq!(result.status, ShopActionStatus::CycleMismatch);
        assert_eq!(result.state.current_cycle_id, "new-server-cycle");
        assert_eq!(ledger.lock().unwrap().planet_cycle_id().unwrap(), old_cycle);
        let cached_json: String = ledger
            .lock()
            .unwrap()
            .connection
            .query_row("SELECT state_json FROM shop_remote_state", [], |row| row.get(0))
            .unwrap();
        let cached: ShopState = serde_json::from_str(&cached_json).unwrap();
        assert_eq!(cached.current_cycle_id, "new-server-cycle");
    }

    #[test]
    fn uncertain_online_shop_action_is_not_retried_or_assigned_a_new_request_id() {
        let (_directory, ledger) = test_ledger();
        ledger.lock().unwrap().ensure_planet_account("alice").unwrap();
        let cycle_id = ledger.lock().unwrap().planet_cycle_id().unwrap();
        let request = placement_request(&cycle_id);
        let rpc = TestShopRpc::default();
        *rpc.apply_result.lock().unwrap() = Some(Err(SyncError::Transport));

        let result = tauri::async_runtime::block_on(apply_shop_request_for_session(
            &ledger,
            online_session(&ledger, "alice-token"),
            Some(&rpc),
            &request,
        ));

        assert!(result.is_err());
        let calls = rpc.apply_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, request);
        match &calls[0].1 {
            ShopRequest::Place { request_id, .. } => assert_eq!(request_id, "request-42"),
            _ => panic!("the original request must be retried unchanged"),
        }
    }

    #[test]
    fn offline_signed_shop_action_is_unavailable_without_writing_guest_receipts() {
        let (_directory, ledger) = test_ledger();
        ledger.lock().unwrap().ensure_planet_account("alice").unwrap();
        let before = ledger.lock().unwrap().shop_state().unwrap();
        let request = ShopRequest::Purchase {
            request_id: "retry-this-request".into(),
            quote: ShopQuote {
                target: QuoteTarget::Purchase {
                    sku: "land_pond".into(),
                },
                catalog_revision: before.catalog_revision,
                effect_revision: 0,
                price: 5_000_000,
            },
        };

        let result = tauri::async_runtime::block_on(apply_shop_request_for_session(
            &ledger,
            CanonicalShopSession::Unavailable("서버에 연결할 수 없습니다".into()),
            None,
            &request,
        ))
        .unwrap();

        assert_eq!(result.status, ShopActionStatus::Unavailable);
        assert_eq!(result.request_id, "retry-this-request");
        assert_eq!(result.state.account_id, "account:alice");
        assert_eq!(result.state.available_balance, before.available_balance);
        assert!(result.state.landscape_instances.is_empty());
        assert_eq!(
            result.state.action_unavailable_reason.as_deref(),
            Some("서버에 연결할 수 없습니다")
        );
        let ledger = ledger.lock().unwrap();
        let guest_receipts: i64 = ledger
            .connection
            .query_row("SELECT count(*) FROM shop_action_request", [], |row| row.get(0))
            .unwrap();
        assert_eq!(guest_receipts, 0);
    }

    #[test]
    fn stale_equip_request_returns_latest_cycle_and_slot_without_reapplying() {
        let latest = CosmeticShopState {
            slots: vec![CosmeticSlot {
                slot_id: "sky".into(),
                display_name: "하늘".into(),
            }],
            products: vec![],
            current_cycle_id: "new-cycle".into(),
            available_balance: 100_000,
            owned_skus: vec!["star_cluster".into()],
            equipped: vec![EquippedCosmetic {
                slot_id: "sky".into(),
                sku: "star_cluster".into(),
                version: 4,
            }],
            slot_versions: BTreeMap::from([("sky".into(), 4)]),
            actions_require_online: true,
            action_unavailable_reason: None,
            guest_import_pending: false,
            guest_import_error: None,
        };

        let stale_cycle = stale_equip_result("old-cycle", 3, &latest, "sky").unwrap();
        assert_eq!(stale_cycle.status, CosmeticEquipStatus::CycleMismatch);
        assert_eq!(stale_cycle.cycle_id, "new-cycle");
        assert_eq!(stale_cycle.sku.as_deref(), Some("star_cluster"));
        assert_eq!(stale_cycle.version, 4);

        let stale_version = stale_equip_result("new-cycle", 3, &latest, "sky").unwrap();
        assert_eq!(stale_version.status, CosmeticEquipStatus::VersionConflict);
        assert_eq!(stale_version.sku.as_deref(), Some("star_cluster"));
        assert_eq!(stale_version.version, 4);
        assert!(stale_equip_result("new-cycle", 4, &latest, "sky").is_none());
    }
}
