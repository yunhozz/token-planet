use tauri::State;

use crate::domain::cosmetic_shop::{
    CosmeticEquipAction, CosmeticEquipResult, CosmeticEquipStatus, CosmeticPurchaseAction,
    CosmeticShopState,
};
use crate::sync::auth::{AuthConfig, AuthError, SessionStore, StoredSession, SupabaseAuthClient};
use crate::sync::client::{SupabaseSyncClient, SyncError};
use crate::sync::worker::import_pending_guest_cosmetics;
use crate::AppState;

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
pub async fn get_shop_state(state: State<'_, AppState>) -> Result<CosmeticShopState, String> {
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

    use super::stale_equip_result;
    use crate::domain::cosmetic_shop::{
        CosmeticEquipStatus, CosmeticShopState, CosmeticSlot, EquippedCosmetic,
    };

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
