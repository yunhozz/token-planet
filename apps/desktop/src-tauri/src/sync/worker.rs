use std::{future::Future, pin::Pin, time::Duration};

use crate::domain::cosmetic_shop::{ResetShopResult, ShopActionStatus, ShopEffectTimeline};
#[cfg(test)]
use crate::domain::planet::{PlanetDeviceContribution, PlanetEffectContributionSegment};
use crate::domain::planet::{PlanetDeviceContributionSnapshot, PlanetState};
use crate::domain::usage::UsageCoverage;
use crate::growth::WorldSnapshot;
use crate::storage::ledger::{Ledger, SignedResetIntent};
use crate::sync::auth::{AuthConfig, SessionStore, SupabaseAuthClient};
use crate::sync::client::{SupabaseSyncClient, SyncError};
use crate::AppState;

#[derive(Default)]
pub struct RetryDelay {
    failures: u8,
}

impl RetryDelay {
    pub fn next_after(&mut self, success: bool) -> Duration {
        if success {
            self.failures = 0;
            return Duration::from_secs(60);
        }
        self.failures = self.failures.saturating_add(1);
        Duration::from_secs((5_u64 << (self.failures - 1).min(4)).min(60))
    }
}

fn queue_cached(state: &AppState, user_id: &str) {
    if let Ok(mut ledger) = state.ledger.lock() {
        if let Ok(Some((world_id, cached_user_id, timezone))) = ledger.cached_world_scope() {
            if cached_user_id == user_id {
                let _ = ledger.prepare_shared_snapshots(&world_id, user_id, timezone);
            }
        }
    }
}

pub(crate) fn guest_shop_import_pending(ledger: &Ledger) -> Result<bool, String> {
    Ok(ledger
        .has_local_guest_shop_state()
        .map_err(|_| "게스트 상점 가져오기 상태를 확인할 수 없습니다")?
        || ledger
            .has_unimported_guest_shop_state()
            .map_err(|_| "게스트 상점 가져오기 상태를 확인할 수 없습니다")?)
}

pub(crate) fn require_guest_shop_import_complete(ledger: &Ledger) -> Result<(), String> {
    if guest_shop_import_pending(ledger)? {
        return Err("게스트 상점 가져오기를 완료한 뒤 동기화할 수 있습니다".into());
    }
    Ok(())
}

fn require_guest_shop_import_complete_from_state(state: &AppState) -> Result<(), String> {
    let ledger = state
        .ledger
        .lock()
        .map_err(|_| "게스트 상점 가져오기 상태를 확인할 수 없습니다")?;
    require_guest_shop_import_complete(&ledger)
}

#[cfg(test)]
fn bootstrap_contribution_snapshot(
    raw: PlanetDeviceContribution,
) -> Result<PlanetDeviceContributionSnapshot, String> {
    let current_total = raw
        .daily_tokens
        .values()
        .try_fold(0_u64, |total, tokens| total.checked_add(*tokens))
        .ok_or("일별 토큰 합계 범위 오류")?;
    if current_total != raw.current_planet_tokens {
        return Err("초기 일별 토큰 합계가 현재 행성 토큰과 다릅니다".into());
    }
    let daily_segments = raw
        .daily_tokens
        .iter()
        .map(|(date, tokens)| PlanetEffectContributionSegment {
            cycle_id: raw.current_cycle_id.clone(),
            date: date.clone(),
            effect_revision: 0,
            tokens: *tokens,
        })
        .collect();
    Ok(PlanetDeviceContributionSnapshot {
        raw,
        canonical_version: 0,
        daily_segments,
        activity_days: Vec::new(),
    })
}

pub(crate) trait PrivateEffectSyncApi {
    fn my_planet_state<'a>(
        &'a self,
        access_token: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<PlanetState>, SyncError>> + Send + 'a>>;

    fn get_my_shop_effect_timeline<'a>(
        &'a self,
        access_token: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<ShopEffectTimeline, SyncError>> + Send + 'a>>;

    fn upload_planet_state_with_effects<'a>(
        &'a self,
        access_token: &'a str,
        state: &'a PlanetState,
        contribution: &'a PlanetDeviceContributionSnapshot,
    ) -> Pin<Box<dyn Future<Output = Result<PlanetState, SyncError>> + Send + 'a>>;
}

impl PrivateEffectSyncApi for SupabaseSyncClient {
    fn my_planet_state<'a>(
        &'a self,
        access_token: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<PlanetState>, SyncError>> + Send + 'a>> {
        Box::pin(SupabaseSyncClient::my_planet_state(self, access_token))
    }

    fn get_my_shop_effect_timeline<'a>(
        &'a self,
        access_token: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<ShopEffectTimeline, SyncError>> + Send + 'a>> {
        Box::pin(SupabaseSyncClient::get_my_shop_effect_timeline(
            self,
            access_token,
        ))
    }

    fn upload_planet_state_with_effects<'a>(
        &'a self,
        access_token: &'a str,
        state: &'a PlanetState,
        contribution: &'a PlanetDeviceContributionSnapshot,
    ) -> Pin<Box<dyn Future<Output = Result<PlanetState, SyncError>> + Send + 'a>> {
        Box::pin(SupabaseSyncClient::upload_planet_state_with_effects(
            self,
            access_token,
            state,
            contribution,
        ))
    }
}

pub(crate) trait SignedResetSyncApi: PrivateEffectSyncApi {
    fn reset_my_planet<'a>(
        &'a self,
        access_token: &'a str,
        request_id: uuid::Uuid,
        expected_cycle_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<ResetShopResult, SyncError>> + Send + 'a>>;
}

impl SignedResetSyncApi for SupabaseSyncClient {
    fn reset_my_planet<'a>(
        &'a self,
        access_token: &'a str,
        request_id: uuid::Uuid,
        expected_cycle_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<ResetShopResult, SyncError>> + Send + 'a>> {
        Box::pin(SupabaseSyncClient::reset_my_planet(
            self,
            access_token,
            request_id,
            expected_cycle_id,
        ))
    }
}

fn require_planet_sync_context(
    ledger: &Ledger,
    account_id: &str,
    cycle_id: &str,
) -> Result<(), String> {
    let expected_account_id = format!("account:{account_id}");
    if ledger
        .cosmetic_account_id()
        .map_err(|_| "행성 동기화 계정을 확인할 수 없습니다")?
        != expected_account_id
        || ledger
            .planet_cycle_id()
            .map_err(|_| "행성 동기화 주기를 확인할 수 없습니다")?
            != cycle_id
    {
        return Err("계정 또는 행성 주기가 변경되어 동기화를 중단했습니다".into());
    }
    Ok(())
}

#[must_use = "a caller requiring an uploaded contribution must reject HeldForSharingPause"]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PrivateEffectSyncOutcome {
    Uploaded { can_reset: bool },
    HeldForSharingPause,
}

#[derive(Debug)]
pub(crate) enum SignedResetCompletion {
    Confirmed(WorldSnapshot),
    ConfirmedViewUnavailable {
        account_id: String,
        request_id: uuid::Uuid,
        expected_old_cycle_id: String,
        new_cycle_id: String,
    },
}

impl PrivateEffectSyncOutcome {
    pub(crate) fn require_uploaded(self) -> Result<bool, String> {
        match self {
            Self::Uploaded { can_reset } => Ok(can_reset),
            Self::HeldForSharingPause => {
                Err("공유 일시 중지로 행성 기여 업로드가 보류되었습니다".into())
            }
        }
    }
}

fn latest_usage_incomplete(state: &AppState) -> Result<bool, String> {
    let latest = state
        .latest
        .lock()
        .map_err(|_| "사용량 상태를 읽을 수 없습니다")?;
    let snapshot = latest.as_ref().ok_or("사용량 상태를 준비할 수 없습니다")?;
    Ok([
        snapshot.usage.codex.coverage,
        snapshot.usage.claude_code.coverage,
    ]
    .iter()
    .any(|coverage| {
        !matches!(
            coverage,
            UsageCoverage::Complete | UsageCoverage::UserDisabled
        )
    }))
}

/// Fetches and applies private canonical state before offering a contribution.
/// Callers must resolve the world policy first and require `Uploaded` when a
/// contribution is a prerequisite for another mutating operation.
pub(crate) async fn sync_private_effect_contribution<C: PrivateEffectSyncApi>(
    state: &AppState,
    account_id: &str,
    access_token: &str,
    client: &C,
    world_policy_paused: bool,
) -> Result<PrivateEffectSyncOutcome, String> {
    let captured_cycle_id = {
        let ledger = state.ledger.lock().map_err(|_| "행성 상태 오류")?;
        let cycle_id = ledger
            .planet_cycle_id()
            .map_err(|_| "행성 주기를 읽을 수 없습니다")?;
        require_planet_sync_context(&ledger, account_id, &cycle_id)?;
        cycle_id
    };

    let Some(remote_state) = client
        .my_planet_state(access_token)
        .await
        .map_err(|_| "행성 동기화 상태를 불러올 수 없습니다")?
    else {
        return Err("서버 행성 상태가 확인될 때까지 기여 동기화를 보류합니다".into());
    };

    {
        let mut ledger = state.ledger.lock().map_err(|_| "행성 상태 오류")?;
        require_planet_sync_context(&ledger, account_id, &captured_cycle_id)?;
        if remote_state.current_cycle_id.trim().is_empty() {
            return Err("서버 행성 주기를 확인할 수 없습니다".into());
        }
        ledger
            .record_canonical_planet_history(&format!("account:{account_id}"), &remote_state)
            .map_err(|_| "행성 순서 이력을 반영할 수 없습니다")?;
        ledger
            .merge_remote_planet_state(&remote_state)
            .map_err(|_| "행성 동기화 상태를 반영할 수 없습니다")?;
        require_planet_sync_context(&ledger, account_id, &remote_state.current_cycle_id)?;
    }

    let cycle_id = remote_state.current_cycle_id.as_str();
    {
        let ledger = state.ledger.lock().map_err(|_| "행성 상태 오류")?;
        require_planet_sync_context(&ledger, account_id, cycle_id)?;
    }
    let timeline = client
        .get_my_shop_effect_timeline(access_token)
        .await
        .map_err(|_| "개인 상점 효과 이력을 불러올 수 없습니다")?;

    {
        let mut ledger = state.ledger.lock().map_err(|_| "행성 상태 오류")?;
        require_planet_sync_context(&ledger, account_id, cycle_id)?;
        if timeline.account_id != account_id || timeline.current_cycle_id != cycle_id {
            return Err("개인 상점 효과 이력이 현재 계정 또는 주기와 다릅니다".into());
        }
        ledger
            .apply_confirmed_shop_effect_timeline(&timeline, account_id, cycle_id)
            .map_err(|_| "개인 상점 효과 이력을 반영할 수 없습니다")?;
    }

    state.rebuild_snapshot_from_latest_usage()?;
    let incomplete = latest_usage_incomplete(state)?;
    let contribution = {
        let ledger = state.ledger.lock().map_err(|_| "행성 상태 오류")?;
        require_planet_sync_context(&ledger, account_id, cycle_id)?;
        let contribution = ledger
            .shop_device_contribution(incomplete)
            .map_err(|_| "효과별 행성 집계를 준비할 수 없습니다")?;
        if contribution.raw.current_cycle_id != cycle_id {
            return Err("효과별 행성 집계 주기가 변경되었습니다".into());
        }
        contribution
    };

    let can_upload = {
        let ledger = state.ledger.lock().map_err(|_| "행성 상태 오류")?;
        require_planet_sync_context(&ledger, account_id, cycle_id)?;
        !world_policy_paused
            && !ledger
                .sharing_paused()
                .map_err(|_| "로컬 동기화 설정을 확인할 수 없습니다")?
    };
    if !can_upload {
        return Ok(PrivateEffectSyncOutcome::HeldForSharingPause);
    }
    let canonical_state = client
        .upload_planet_state_with_effects(access_token, &remote_state, &contribution)
        .await
        .map_err(|_| "효과별 행성 집계 전송 실패")?;
    let can_reset = canonical_state.can_reset;

    {
        let mut ledger = state.ledger.lock().map_err(|_| "행성 상태 오류")?;
        require_planet_sync_context(&ledger, account_id, cycle_id)?;
        if canonical_state.current_cycle_id != cycle_id {
            return Err("서버 응답의 행성 주기가 변경되었습니다".into());
        }
        ledger
            .record_canonical_planet_history(&format!("account:{account_id}"), &canonical_state)
            .map_err(|_| "행성 순서 이력을 반영할 수 없습니다")?;
        ledger
            .merge_remote_planet_state(&canonical_state)
            .map_err(|_| "행성 동기화 상태를 반영할 수 없습니다")?;
    }
    state.rebuild_snapshot_from_latest_usage()?;
    Ok(PrivateEffectSyncOutcome::Uploaded { can_reset })
}

/// Retries a previously persisted reset intent before fetching or merging a
/// normal PlanetState. The local intent remains pending for transport,
/// malformed-response, timeline, and cache errors so the same UUID is retried.
pub(crate) async fn recover_pending_signed_reset<C: SignedResetSyncApi>(
    state: &AppState,
    account_id: &str,
    access_token: &str,
    client: &C,
) -> Result<Option<SignedResetCompletion>, String> {
    let intent = state
        .ledger
        .lock()
        .map_err(|_| "행성 상태 오류")?
        .pending_signed_reset_intent(account_id)
        .map_err(|_| "대기 중인 행성 초기화를 확인할 수 없습니다")?;
    let Some(intent) = intent else {
        return Ok(None);
    };
    finish_signed_reset_intent(state, &intent, access_token, client)
        .await
        .map(Some)
}

/// Starts a reset only after a successful current-cycle contribution upload.
/// A durable prior intent takes the recovery path and never repeats preupload.
pub(crate) async fn reset_signed_planet<C: SignedResetSyncApi>(
    state: &AppState,
    account_id: &str,
    access_token: &str,
    client: &C,
    world_policy_paused: bool,
) -> Result<SignedResetCompletion, String> {
    if let Some(completion) =
        recover_pending_signed_reset(state, account_id, access_token, client).await?
    {
        return Ok(completion);
    }
    {
        let ledger = state.ledger.lock().map_err(|_| "행성 상태 오류")?;
        require_guest_shop_import_complete(&ledger)?;
    }
    let old_cycle_id = {
        let ledger = state.ledger.lock().map_err(|_| "행성 상태 오류")?;
        let cycle_id = ledger
            .planet_cycle_id()
            .map_err(|_| "행성 주기를 확인할 수 없습니다")?;
        require_planet_sync_context(&ledger, account_id, &cycle_id)?;
        cycle_id
    };
    let can_reset = sync_private_effect_contribution(
        state,
        account_id,
        access_token,
        client,
        world_policy_paused,
    )
    .await?
    .require_uploaded()?;

    let intent = {
        let mut ledger = state.ledger.lock().map_err(|_| "행성 상태 오류")?;
        require_planet_sync_context(&ledger, account_id, &old_cycle_id)?;
        if !can_reset {
            return Err("서버에서 행성 초기화 대기 시간이 끝나지 않은 것으로 확인했습니다".into());
        }
        ledger
            .prepare_signed_reset_intent(uuid::Uuid::new_v4(), &old_cycle_id)
            .map_err(|_| "행성 초기화 요청을 안전하게 저장할 수 없습니다")?
    };
    finish_signed_reset_intent(state, &intent, access_token, client).await
}

async fn finish_signed_reset_intent<C: SignedResetSyncApi>(
    state: &AppState,
    intent: &SignedResetIntent,
    access_token: &str,
    client: &C,
) -> Result<SignedResetCompletion, String> {
    {
        let ledger = state.ledger.lock().map_err(|_| "행성 상태 오류")?;
        require_planet_sync_context(&ledger, &intent.account_id, &intent.expected_old_cycle_id)?;
        if ledger
            .pending_signed_reset_intent(&intent.account_id)
            .map_err(|_| "대기 중인 행성 초기화를 확인할 수 없습니다")?
            .as_ref()
            != Some(intent)
        {
            return Err("대기 중인 행성 초기화 요청이 변경되었습니다".into());
        }
    }

    let result = match client
        .reset_my_planet(
            access_token,
            intent.request_id,
            &intent.expected_old_cycle_id,
        )
        .await
    {
        Ok(result) => result,
        Err(SyncError::Rejected(status_code)) => {
            return Err(format!(
                "행성 초기화 HTTP {status_code} 응답을 확인할 수 없습니다. 같은 요청으로 재시도합니다"
            ));
        }
        Err(_) => {
            return Err("행성 초기화 응답을 확인할 수 없습니다. 같은 요청으로 재시도합니다".into())
        }
    };

    {
        let ledger = state.ledger.lock().map_err(|_| "행성 상태 오류")?;
        require_planet_sync_context(&ledger, &intent.account_id, &intent.expected_old_cycle_id)?;
    }
    if result.action.request_id != intent.request_id.to_string()
        || result.action.state.current_cycle_id != result.planet_state.current_cycle_id
    {
        return Err("행성 초기화 응답을 검증할 수 없습니다. 같은 요청으로 재시도합니다".into());
    }
    if result.action.status != ShopActionStatus::Reset {
        let status_message = match result.action.status {
            ShopActionStatus::CycleMismatch => {
                "서버의 행성 주기가 변경되어 초기화 요청을 종료했습니다"
            }
            ShopActionStatus::RequestConflict => {
                "초기화 요청 ID가 다른 요청과 충돌하여 종료했습니다"
            }
            _ => "서버가 행성 초기화를 확정하지 않았습니다",
        };
        let mut ledger = state.ledger.lock().map_err(|_| "행성 상태 오류")?;
        require_planet_sync_context(&ledger, &intent.account_id, &intent.expected_old_cycle_id)?;
        ledger
            .record_rejected_signed_reset_result(&result, intent)
            .map_err(|_| "서버의 초기화 거절을 안전하게 기록할 수 없습니다")?;
        return Err(status_message.into());
    }
    if result.planet_state.current_cycle_id == intent.expected_old_cycle_id {
        return Err("서버 초기화 응답에 새 행성 주기가 없습니다".into());
    }

    let timeline = client
        .get_my_shop_effect_timeline(access_token)
        .await
        .map_err(|_| "초기화된 상점 효과 이력을 확인할 수 없습니다. 같은 요청으로 재시도합니다")?;
    {
        let mut ledger = state.ledger.lock().map_err(|_| "행성 상태 오류")?;
        require_planet_sync_context(&ledger, &intent.account_id, &intent.expected_old_cycle_id)?;
        if timeline.account_id != intent.account_id
            || timeline.current_cycle_id != result.planet_state.current_cycle_id
        {
            return Err("초기화된 상점 효과 이력이 현재 계정 또는 주기와 다릅니다".into());
        }
        ledger
            .apply_confirmed_signed_reset_result(&result, &timeline, intent)
            .map_err(|_| "확정된 행성 초기화 상태를 안전하게 저장할 수 없습니다")?;
    }
    match state.rebuild_snapshot_from_latest_usage() {
        Ok(snapshot) => Ok(SignedResetCompletion::Confirmed(snapshot)),
        Err(_) => Ok(SignedResetCompletion::ConfirmedViewUnavailable {
            account_id: intent.account_id.clone(),
            request_id: intent.request_id,
            expected_old_cycle_id: intent.expected_old_cycle_id.clone(),
            new_cycle_id: result.planet_state.current_cycle_id,
        }),
    }
}

pub async fn sync_once(state: &AppState) -> Result<(), String> {
    sync_once_event(state).await.data
}

pub(crate) async fn sync_once_event(
    state: &AppState,
) -> crate::domain::device_reset::LocalEnvelope<Result<(), String>> {
    let generation = state.lifecycle.generation().await;
    let permit = match state.lifecycle.enter(generation).await {
        Ok(permit) => permit,
        Err(error) => {
            return crate::domain::device_reset::LocalEnvelope {
                generation: error.generation,
                data: Err(error.message),
            }
        }
    };
    let result = sync_once_inner(state).await;
    if let Ok(mut failed) = state.sync_failed.lock() {
        *failed = result.is_err();
    }
    crate::domain::device_reset::LocalEnvelope {
        generation: permit.generation(),
        data: result,
    }
}

async fn sync_once_inner(state: &AppState) -> Result<(), String> {
    let _gate = state.sync_gate.lock().await;
    sync_after_guest_import_check(state).await
}

#[cfg(test)]
async fn sync_guest_import_before_legacy_sync<T, F, Fut>(
    state: &AppState,
    client: &T,
    access_token: &str,
    target: &str,
    legacy_sync: F,
) -> Result<crate::sync::guest_shop_import::GuestImportSyncOutcome, String>
where
    T: crate::sync::guest_shop_import::GuestShopImportTransport,
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<(), String>>,
{
    use crate::sync::guest_shop_import::{sync_pending_guest_shop_import, GuestImportSyncOutcome};

    let outcome = sync_pending_guest_shop_import(state, client, access_token, target).await?;
    if matches!(
        outcome,
        GuestImportSyncOutcome::NoPending | GuestImportSyncOutcome::Imported
    ) {
        if outcome == GuestImportSyncOutcome::Imported {
            let user_id = target
                .strip_prefix("account:")
                .ok_or("게스트 상점 가져오기 계정을 확인할 수 없습니다")?;
            state.select_planet_account(user_id)?;
        }
        legacy_sync().await?;
    }
    Ok(outcome)
}

#[cfg(test)]
async fn sync_authenticated_legacy_after_guest_import<T, F, Fut>(
    state: &AppState,
    user_id: &str,
    access_token: &str,
    client: &T,
    legacy_sync: F,
) -> Result<crate::sync::guest_shop_import::GuestImportSyncOutcome, String>
where
    T: crate::sync::guest_shop_import::GuestShopImportTransport,
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<(), String>>,
{
    let target = format!("account:{user_id}");
    sync_guest_import_before_legacy_sync(state, client, access_token, &target, legacy_sync).await
}

async fn sync_authenticated_legacy_after_guest_import_with_state_read<T, R, RFut, F, Fut>(
    state: &AppState,
    user_id: &str,
    access_token: &str,
    client: &T,
    read_confirmed_state: R,
    legacy_sync: F,
) -> Result<crate::sync::guest_shop_import::GuestImportSyncOutcome, String>
where
    T: crate::sync::guest_shop_import::GuestShopImportTransport,
    R: FnOnce() -> RFut,
    RFut: Future<Output = Result<Option<PlanetState>, SyncError>>,
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<(), String>>,
{
    use crate::sync::guest_shop_import::{sync_pending_guest_shop_import, GuestImportSyncOutcome};

    let target = format!("account:{user_id}");
    let outcome = sync_pending_guest_shop_import(state, client, access_token, &target).await?;
    match outcome {
        GuestImportSyncOutcome::NoPending | GuestImportSyncOutcome::Imported => {
            if outcome == GuestImportSyncOutcome::Imported {
                state.select_planet_account(user_id)?;
            }
            legacy_sync().await?;
        }
        GuestImportSyncOutcome::ImportedWithCorrectionHold => {
            let cycle_id = {
                let ledger = state.ledger.lock().map_err(|_| "행성 상태 오류")?;
                require_guest_imported_state_read_context(&ledger, &target)?;
                ledger
                    .planet_cycle_id()
                    .map_err(|_| "행성 주기를 확인할 수 없습니다")?
            };
            let remote_state = read_confirmed_state().await;
            {
                let ledger = state.ledger.lock().map_err(|_| "행성 상태 오류")?;
                require_guest_imported_state_read_context(&ledger, &target)?;
                if ledger
                    .planet_cycle_id()
                    .map_err(|_| "행성 주기를 확인할 수 없습니다")?
                    != cycle_id
                {
                    return Err("계정 또는 행성 주기가 변경되어 상태 조회를 중단했습니다".into());
                }
            }
            if remote_state
                .map_err(|_| "서버 행성 상태를 불러올 수 없습니다")?
                .is_none()
            {
                return Err("서버 행성 상태가 확인될 때까지 조회를 보류합니다".into());
            }
        }
        GuestImportSyncOutcome::Held => {}
    }
    Ok(outcome)
}

fn require_guest_imported_state_read_context(
    ledger: &Ledger,
    target_account_id: &str,
) -> Result<(), String> {
    let selected_auth_account: String = ledger
        .connection
        .query_row(
            "SELECT coalesce((SELECT value FROM setting WHERE key='selected_auth_account_id'), '')",
            [],
            |row| row.get(0),
        )
        .map_err(|_| "선택한 계정의 행성 상태를 확인할 수 없습니다")?;
    let active_planet_account: String = ledger
        .connection
        .query_row(
            "SELECT coalesce((SELECT value FROM setting WHERE key='planet_account_id'), '')",
            [],
            |row| row.get(0),
        )
        .map_err(|_| "활성 행성 소유권을 확인할 수 없습니다")?;
    let cached_planet_owner_exists: bool = ledger
        .connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM planet_account_state WHERE account_id=?1)",
            [target_account_id],
            |row| row.get(0),
        )
        .map_err(|_| "활성 행성 소유권을 확인할 수 없습니다")?;
    if selected_auth_account != target_account_id
        || active_planet_account != target_account_id
        || ledger
            .cosmetic_account_id()
            .map_err(|_| "활성 행성 소유권을 확인할 수 없습니다")?
            != target_account_id
        || !cached_planet_owner_exists
    {
        return Err("선택 계정의 행성 소유권을 확인할 수 없어 상태 조회를 보류합니다".into());
    }
    Ok(())
}

async fn refresh_saved_session_or_queue_cached_state<Fut>(
    state: &AppState,
    saved_user_id: &str,
    refresh: Fut,
) -> Result<crate::sync::auth::StoredSession, String>
where
    Fut: Future<Output = Result<crate::sync::auth::StoredSession, crate::sync::auth::AuthError>>,
{
    match refresh.await {
        Ok(session) => Ok(session),
        Err(_) => {
            queue_cached(state, saved_user_id);
            Err("공동 세계 연결 실패".into())
        }
    }
}

fn select_planet_account_for_authenticated_sync(
    state: &AppState,
    user_id: &str,
) -> Result<(), String> {
    match state.select_planet_account_with_outcome(user_id) {
        Ok(()) => Ok(()),
        Err(crate::PlanetAccountSwitchError::GuestShopImportPending) => {
            let target = format!("account:{user_id}");
            let target_has_pending_import = state
                .ledger
                .lock()
                .map_err(|_| "게스트 가져오기 상태 오류")?
                .pending_guest_shop_import_request(&target)
                .map_err(|_| "게스트 상점 가져오기 상태를 확인할 수 없습니다")?
                .is_some();
            if target_has_pending_import {
                Ok(())
            } else {
                Err(crate::PlanetAccountSwitchError::GuestShopImportPending.into_message())
            }
        }
        Err(error) => Err(error.into_message()),
    }
}

async fn sync_after_guest_import_check(state: &AppState) -> Result<(), String> {
    let Some(config) = AuthConfig::from_env() else {
        return require_guest_shop_import_complete_from_state(state);
    };
    let store = SessionStore::new(&config).map_err(|_| "보안 저장소 오류")?;
    let Some(saved) = store.load().map_err(|_| "로그인 정보 오류")? else {
        return require_guest_shop_import_complete_from_state(state);
    };
    select_planet_account_for_authenticated_sync(state, &saved.user.id)?;
    if state.has_usage_scan_failure() {
        return Err("사용량 기록을 확인한 뒤 동기화할 수 있습니다".into());
    }
    let auth_client = SupabaseAuthClient::new(config.clone());
    let session = refresh_saved_session_or_queue_cached_state(
        state,
        &saved.user.id,
        auth_client.session(&store),
    )
    .await?;
    let client = SupabaseSyncClient::new(&config.base_url, &config.publishable_key);
    sync_authenticated_legacy_after_guest_import_with_state_read(
        state,
        &session.user.id,
        &session.access_token,
        &client,
        || PrivateEffectSyncApi::my_planet_state(&client, &session.access_token),
        || sync_legacy_authenticated(state, &saved, &session, &client),
    )
    .await?;
    Ok(())
}

async fn sync_legacy_authenticated(
    state: &AppState,
    saved: &crate::sync::auth::StoredSession,
    session: &crate::sync::auth::StoredSession,
    client: &SupabaseSyncClient,
) -> Result<(), String> {
    // A remote reset may already have succeeded even if the previous process
    // lost its response. Resolve its durable receipt before any normal state
    // fetch can merge a newer cycle into the local ledger.
    if recover_pending_signed_reset(state, &session.user.id, &session.access_token, client)
        .await?
        .is_some()
    {
        return Ok(());
    }
    let shell = match client.current_world(&session.access_token).await {
        Ok(shell) => shell,
        Err(_) => {
            queue_cached(state, &saved.user.id);
            return Err("공동 세계 연결 실패".into());
        }
    };
    let policy = if let Some(shell) = &shell {
        state
            .ledger
            .lock()
            .map_err(|_| "로컬 대기열 오류")?
            .ensure_world_scope(
                &shell.id,
                &session.user.id,
                shell.timezone.parse().map_err(|_| "세계 시간대 오류")?,
            )
            .map_err(|_| "세계 동기화 범위 오류")?;
        Some(
            client
                .my_sync_policy(&session.access_token, &shell.id)
                .await
                .map_err(|_| "동기화 정책 확인 실패")?,
        )
    } else {
        None
    };
    let world_policy_paused = policy.as_ref().is_some_and(|policy| policy.paused);
    let private_effect_outcome = sync_private_effect_contribution(
        state,
        &session.user.id,
        &session.access_token,
        client,
        world_policy_paused,
    )
    .await?;
    match private_effect_outcome.require_uploaded() {
        Ok(_) => {}
        Err(_) => {
            // Scheduled sync can refresh read-only state while publication is paused.
        }
    }
    import_pending_guest_cosmetics(state, &client, &session.access_token).await?;
    let remote = client
        .growth_journal(&session.access_token)
        .await
        .map_err(|_| "성장 일지 상태를 불러올 수 없습니다")?;
    let (journal, pending) = {
        let mut ledger = state.ledger.lock().map_err(|_| "성장 일지 대기열 오류")?;
        ledger
            .apply_growth_journal_state(&remote)
            .map_err(|_| "성장 일지 상태를 반영할 수 없습니다")?;
        ledger
            .prepare_growth_journal()
            .map_err(|_| "성장 일지를 준비할 수 없습니다")?;
        (
            ledger
                .growth_journal()
                .map_err(|_| "성장 일지를 읽을 수 없습니다")?,
            ledger
                .pending_growth_journal_entries()
                .map_err(|_| "성장 일지 대기열을 읽을 수 없습니다")?,
        )
    };
    let publish_planet = !world_policy_paused
        && !state
            .ledger
            .lock()
            .map_err(|_| "로컬 동기화 설정 오류")?
            .sharing_paused()
            .map_err(|_| "로컬 동기화 설정 오류")?;
    if publish_planet {
        let canonical = client
            .upsert_growth_journal(
                &session.access_token,
                journal.generation,
                journal
                    .timezone
                    .as_deref()
                    .ok_or("행성 시간대를 읽을 수 없습니다")?,
                &journal.cycles,
                &pending,
            )
            .await
            .map_err(|_| "성장 일지 동기화 실패")?;
        let mut ledger = state.ledger.lock().map_err(|_| "성장 일지 상태 오류")?;
        ledger
            .apply_growth_journal_state(&canonical)
            .map_err(|_| "성장 일지 동기화 상태를 반영할 수 없습니다")?;
        ledger
            .prepare_growth_journal()
            .map_err(|_| "성장 일지를 갱신할 수 없습니다")?;
    }
    let Some(shell) = shell else { return Ok(()) };
    let timezone = shell.timezone.parse().map_err(|_| "세계 시간대 오류")?;
    let policy = policy.ok_or("동기화 정책을 확인할 수 없습니다")?;
    let pending = {
        let mut ledger = state.ledger.lock().map_err(|_| "로컬 대기열 오류")?;
        ledger
            .ensure_world_scope(&shell.id, &session.user.id, timezone)
            .map_err(|_| "세계 동기화 범위 오류")?;
        if let Some(cutoff) = policy.deleted_through.as_deref() {
            ledger
                .adopt_remote_deletion(cutoff, policy.paused)
                .map_err(|_| "삭제 상태 반영 오류")?;
        }
        ledger
            .prepare_shared_snapshots(&shell.id, &session.user.id, timezone)
            .map_err(|_| "집계 준비 오류")?;
        let publish_planet =
            !world_policy_paused && !ledger.sharing_paused().map_err(|_| "동기화 설정 오류")?;
        if !publish_planet {
            return Ok(());
        }
        ledger.pending_snapshots().map_err(|_| "로컬 대기열 오류")?
    };
    for snapshot in pending {
        let revision = client
            .upload_snapshot(&session.access_token, &shell.id, &snapshot)
            .await
            .map_err(|_| "집계 전송 실패")?;
        state
            .ledger
            .lock()
            .map_err(|_| "로컬 대기열 오류")?
            .acknowledge_snapshot(
                &snapshot.device_id,
                &snapshot.bucket_date,
                snapshot.agent,
                revision,
            )
            .map_err(|_| "집계 확인 오류")?;
    }
    Ok(())
}

pub async fn import_pending_guest_cosmetics(
    state: &AppState,
    client: &SupabaseSyncClient,
    access_token: &str,
) -> Result<bool, String> {
    let pending = state
        .ledger
        .lock()
        .map_err(|_| "게스트 구매 기록 오류")?
        .pending_guest_cosmetic_import()
        .map_err(|_| "게스트 구매 기록 오류")?;
    let Some(import) = pending else {
        return Ok(false);
    };
    let result = client
        .import_guest_cosmetics(access_token, &import)
        .await
        .map_err(|_| "게스트 구매 기록을 가져오지 못했습니다")?;
    if result.status != "imported" {
        return Err(match result.status.as_str() {
            "insufficient_balance" => "게스트 구매 기록의 서버 잔액을 확인할 수 없습니다".into(),
            _ => "게스트 구매 기록을 검증할 수 없습니다".into(),
        });
    }
    let canonical = client
        .cosmetic_shop_state(access_token)
        .await
        .map_err(|_| "가져온 상점 상태를 확인할 수 없습니다")?;
    state
        .ledger
        .lock()
        .map_err(|_| "게스트 구매 상태 오류")?
        .mark_guest_cosmetic_imported(&import.import_id, &canonical)
        .map_err(|_| "게스트 구매 상태를 저장할 수 없습니다")?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::{
        bootstrap_contribution_snapshot, recover_pending_signed_reset,
        require_guest_shop_import_complete, sync_once, sync_private_effect_contribution,
        PrivateEffectSyncApi, PrivateEffectSyncOutcome, RetryDelay, SignedResetSyncApi,
    };
    use crate::collectors::discovery::SourceConfig;
    use crate::collectors::{ParsedRecord, RecordKind};
    use crate::domain::cosmetic_shop::{
        ActiveEffects, GuestShopImportDisposition, ResetShopResult, ShopActionResult,
        ShopActionStatus, ShopCycleBound, ShopEffectInterval, ShopEffectTimeline,
    };
    use crate::domain::planet::{
        PlanetAvatar, PlanetDeviceContribution, PlanetDeviceContributionSnapshot, PlanetState,
    };
    use crate::domain::usage::{Agent, TokenUsage, UsageCoverage};
    use crate::storage::ledger::Ledger;
    use crate::sync::client::SyncError;
    use crate::{AppState, WindowMode};
    use chrono::{DateTime, Utc};
    use chrono_tz::UTC;
    use std::{
        collections::{BTreeMap, VecDeque},
        future::Future,
        path::Path,
        pin::Pin,
        sync::{atomic::AtomicBool, Mutex},
    };

    fn add_usage(ledger: &mut Ledger, event_key: &str, at: &str, tokens: u64) {
        ledger
            .insert(&ParsedRecord {
                agent: Agent::Codex,
                kind: RecordKind::Response,
                event_key: event_key.into(),
                occurred_at_utc: DateTime::parse_from_rfc3339(at)
                    .unwrap()
                    .with_timezone(&Utc),
                usage: TokenUsage {
                    input_tokens: None,
                    output_tokens: None,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    total_tokens: Some(tokens),
                    coverage: UsageCoverage::Complete,
                },
            })
            .unwrap();
    }

    fn app_state_with_account(user_id: &str) -> (AppState, String) {
        let mut ledger = Ledger::open(Path::new(":memory:"), UTC).unwrap();
        ledger
            .connection
            .execute(
                "UPDATE setting SET value='2026-10-01T00:00:00Z'
                 WHERE key IN ('planet_activation_at_utc','planet_cycle_started_at_utc')",
                [],
            )
            .unwrap();
        ledger.ensure_planet_account(user_id).unwrap();
        add_usage(
            &mut ledger,
            "private-sync-usage",
            "2026-10-01T12:00:00Z",
            100,
        );
        let cycle_id = ledger.planet_cycle_id().unwrap();
        let state = AppState {
            lifecycle: crate::lifecycle::LocalLifecycle::new(
                &crate::domain::device_reset::DeviceResetState::default(),
            ),
            config: Mutex::new(SourceConfig {
                codex_root: Path::new("/private/tmp/token-planet-empty-codex").to_path_buf(),
                claude_root: Path::new("/private/tmp/token-planet-empty-claude").to_path_buf(),
                timezone: UTC,
            }),
            ledger: Mutex::new(ledger),
            latest: Mutex::new(None),
            usage_scan_failed: AtomicBool::new(false),
            sync_failed: Mutex::new(false),
            sync_gate: tokio::sync::Mutex::new(()),
            window_mode: Mutex::new(WindowMode::Popup),
            mode_transitioning: AtomicBool::new(false),
            tray_press_pending: AtomicBool::new(false),
        };
        state.scan().unwrap();
        (state, cycle_id)
    }

    fn app_state_from_ledger(ledger: Ledger) -> AppState {
        let state = AppState {
            lifecycle: crate::lifecycle::LocalLifecycle::new(
                &crate::domain::device_reset::DeviceResetState::default(),
            ),
            config: Mutex::new(SourceConfig {
                codex_root: Path::new("/private/tmp/token-planet-empty-codex").to_path_buf(),
                claude_root: Path::new("/private/tmp/token-planet-empty-claude").to_path_buf(),
                timezone: UTC,
            }),
            ledger: Mutex::new(ledger),
            latest: Mutex::new(None),
            usage_scan_failed: AtomicBool::new(false),
            sync_failed: Mutex::new(false),
            sync_gate: tokio::sync::Mutex::new(()),
            window_mode: Mutex::new(WindowMode::Popup),
            mode_transitioning: AtomicBool::new(false),
            tray_press_pending: AtomicBool::new(false),
        };
        state.scan().unwrap();
        state
    }

    #[derive(Clone, Copy)]
    enum ResetContextChange {
        Account,
        Cycle,
    }

    struct MockResetRecoveryApi<'a> {
        ledger: Option<&'a Mutex<Ledger>>,
        context_change: Option<ResetContextChange>,
        result: Option<ResetShopResult>,
        timeline: ShopEffectTimeline,
        pre_reset_timeline: Option<ShopEffectTimeline>,
        remote_state: Option<PlanetState>,
        canonical_state: Option<PlanetState>,
        fail_timeline: bool,
        timeline_calls: Mutex<usize>,
        reset_failures: Mutex<usize>,
        reset_http_rejections: Mutex<VecDeque<u16>>,
        server_receipt: Mutex<Option<(uuid::Uuid, String, ResetShopResult)>>,
        usage_scan_failed_on_reset: Option<&'a AtomicBool>,
        calls: Mutex<Vec<String>>,
    }

    impl MockResetRecoveryApi<'_> {
        fn change_context_before_reset_response(&self) {
            let (Some(ledger), Some(change)) = (self.ledger, self.context_change) else {
                return;
            };
            let mut ledger = ledger.lock().unwrap();
            match change {
                ResetContextChange::Account => {
                    ledger
                        .ensure_planet_account("00000000-0000-0000-0000-000000000099")
                        .unwrap();
                }
                ResetContextChange::Cycle => {
                    ledger.connection.execute(
                        "UPDATE setting SET value='externally-changed-cycle' WHERE key='planet_current_cycle_id'",
                        [],
                    ).unwrap();
                }
            }
        }
    }

    impl PrivateEffectSyncApi for MockResetRecoveryApi<'_> {
        fn my_planet_state<'a>(
            &'a self,
            _access_token: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<Option<PlanetState>, SyncError>> + Send + 'a>>
        {
            self.calls.lock().unwrap().push("planet_state".into());
            let state = self.remote_state.clone();
            Box::pin(async move { Ok(state) })
        }

        fn get_my_shop_effect_timeline<'a>(
            &'a self,
            _access_token: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<ShopEffectTimeline, SyncError>> + Send + 'a>>
        {
            self.calls.lock().unwrap().push("timeline".into());
            let call_index = {
                let mut calls = self.timeline_calls.lock().unwrap();
                *calls += 1;
                *calls
            };
            if self.fail_timeline && (self.pre_reset_timeline.is_none() || call_index > 1) {
                return Box::pin(async { Err(SyncError::Transport) });
            }
            let timeline = if call_index == 1 {
                self.pre_reset_timeline
                    .clone()
                    .unwrap_or_else(|| self.timeline.clone())
            } else {
                self.timeline.clone()
            };
            Box::pin(async move { Ok(timeline) })
        }

        fn upload_planet_state_with_effects<'a>(
            &'a self,
            _access_token: &'a str,
            _state: &'a PlanetState,
            _contribution: &'a PlanetDeviceContributionSnapshot,
        ) -> Pin<Box<dyn Future<Output = Result<PlanetState, SyncError>> + Send + 'a>> {
            self.calls.lock().unwrap().push("upload".into());
            let state = self.canonical_state.clone();
            Box::pin(async move { state.ok_or(SyncError::Transport) })
        }
    }

    impl SignedResetSyncApi for MockResetRecoveryApi<'_> {
        fn reset_my_planet<'a>(
            &'a self,
            _access_token: &'a str,
            request_id: uuid::Uuid,
            expected_cycle_id: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<ResetShopResult, SyncError>> + Send + 'a>> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("reset:{request_id}:{expected_cycle_id}"));
            self.change_context_before_reset_response();
            let fail = {
                let mut failures = self.reset_failures.lock().unwrap();
                if *failures > 0 {
                    *failures -= 1;
                    true
                } else {
                    false
                }
            };
            Box::pin(async move {
                let mut receipt = self.server_receipt.lock().unwrap();
                if let Some((recorded_id, recorded_cycle, _)) = receipt.as_ref() {
                    if *recorded_id != request_id || recorded_cycle != expected_cycle_id {
                        return Err(SyncError::InvalidResponse);
                    }
                } else {
                    let Some(mut result) = self.result.clone() else {
                        return Err(SyncError::Transport);
                    };
                    result.action.request_id = request_id.to_string();
                    *receipt = Some((request_id, expected_cycle_id.to_owned(), result));
                }
                let result = receipt
                    .as_ref()
                    .map(|(_, _, result)| result.clone())
                    .ok_or(SyncError::Transport)?;
                if fail {
                    return Err(SyncError::Transport);
                }
                if let Some(status_code) = self.reset_http_rejections.lock().unwrap().pop_front() {
                    return Err(SyncError::Rejected(status_code));
                }
                if let Some(usage_scan_failed) = self.usage_scan_failed_on_reset {
                    usage_scan_failed.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                Ok(result)
            })
        }
    }

    #[test]
    fn lost_reset_response_reopens_and_replays_same_id_before_any_planet_state_fetch() {
        let user_id = "00000000-0000-0000-0000-000000000042";
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.sqlite3");
        let old_cycle_id;
        {
            let mut ledger = Ledger::open(&path, UTC).unwrap();
            ledger
                .connection
                .execute(
                    "UPDATE setting SET value='2026-10-01T00:00:00Z'
                 WHERE key IN ('planet_activation_at_utc','planet_cycle_started_at_utc')",
                    [],
                )
                .unwrap();
            ledger.ensure_planet_account(user_id).unwrap();
            add_usage(
                &mut ledger,
                "reset-replay-usage",
                "2026-10-01T12:00:00Z",
                100,
            );
            old_cycle_id = ledger.planet_cycle_id().unwrap();
        }
        let state = app_state_from_ledger(Ledger::open(&path, UTC).unwrap());
        let new_cycle_id = "80000000-0000-0000-0000-000000000042";
        let api = MockResetRecoveryApi {
            ledger: None,
            context_change: None,
            result: Some(server_reset_result(
                &state,
                uuid::Uuid::new_v4(),
                new_cycle_id,
            )),
            timeline: server_reset_timeline(user_id, &old_cycle_id, new_cycle_id),
            pre_reset_timeline: Some(server_timeline(user_id, &old_cycle_id)),
            remote_state: Some(server_planet_state(&old_cycle_id)),
            canonical_state: Some(server_planet_state(&old_cycle_id)),
            fail_timeline: false,
            timeline_calls: Mutex::new(0),
            // The mock server records the reset before losing its first response.
            reset_failures: Mutex::new(1),
            reset_http_rejections: Mutex::new(VecDeque::new()),
            server_receipt: Mutex::new(None),
            usage_scan_failed_on_reset: None,
            calls: Mutex::new(Vec::new()),
        };

        let first_result = tauri::async_runtime::block_on(super::reset_signed_planet(
            &state,
            user_id,
            "access-token",
            &api,
            false,
        ));
        assert!(
            first_result.is_err(),
            "the lost response leaves a retryable intent"
        );
        let first_calls = api.calls.lock().unwrap().clone();
        assert_eq!(first_calls.len(), 4);
        assert_eq!(&first_calls[..3], &["planet_state", "timeline", "upload"]);
        let request_id = first_calls[3]
            .strip_prefix("reset:")
            .unwrap()
            .split(':')
            .next()
            .unwrap();
        let request_id = uuid::Uuid::parse_str(request_id).unwrap();
        assert_eq!(first_calls[3], format!("reset:{request_id}:{old_cycle_id}"));
        {
            let ledger = state.ledger.lock().unwrap();
            let intent = ledger
                .pending_signed_reset_intent(user_id)
                .unwrap()
                .unwrap();
            assert_eq!(intent.request_id, request_id);
            assert_eq!(intent.expected_old_cycle_id, old_cycle_id);
            assert_eq!(ledger.planet_cycle_id().unwrap(), old_cycle_id);
        }

        // Reopen the same SQLite file as the next process would.
        drop(state);
        let reopened = app_state_from_ledger(Ledger::open(&path, UTC).unwrap());
        {
            let ledger = reopened.ledger.lock().unwrap();
            ledger.connection.execute(
                "INSERT INTO shop_landscape_instance(account_id,instance_id,sku,variation_index,seed,variation_version,acquired_at_utc)
                 VALUES ('local','guest-after-uncertain-reset','land_tree',0,'seed',1,'2026-10-01T00:00:00Z')",
                [],
            ).unwrap();
            assert!(ledger.has_local_guest_shop_state().unwrap());
        }
        let retry_result = tauri::async_runtime::block_on(super::reset_signed_planet(
            &reopened,
            user_id,
            "access-token",
            &api,
            true,
        ));

        assert!(
            retry_result.is_ok(),
            "the persisted reset receipt should apply after restart: {retry_result:?}"
        );
        assert_eq!(
            *api.calls.lock().unwrap(),
            vec![
                "planet_state".into(),
                "timeline".into(),
                "upload".into(),
                format!("reset:{request_id}:{old_cycle_id}"),
                format!("reset:{request_id}:{old_cycle_id}"),
                "timeline".into(),
            ],
            "restart recovery reuses UUID and old cycle before another state fetch, even if sharing is now paused",
        );
        let ledger = reopened.ledger.lock().unwrap();
        assert_eq!(ledger.planet_cycle_id().unwrap(), new_cycle_id);
        assert!(ledger
            .pending_signed_reset_intent(user_id)
            .unwrap()
            .is_none());
        assert_eq!(ledger.connection.query_row::<i64, _, _>(
            "SELECT count(*) FROM shop_action_request WHERE account_id=?1 AND request_id=?2 AND result_json IS NOT NULL",
            rusqlite::params![format!("account:{user_id}"), request_id.to_string()],
            |row| row.get(0),
        ).unwrap(), 1, "reset cache and completed intent must commit together");
        assert_eq!(ledger.connection.query_row::<i64, _, _>(
            "SELECT count(*) FROM shop_landscape_instance WHERE account_id='local' AND instance_id='guest-after-uncertain-reset'",
            [],
            |row| row.get(0),
        ).unwrap(), 1, "receipt recovery must leave guest-owned rows untouched");
        drop(ledger);

        let calls_after_recovery = api.calls.lock().unwrap().clone();
        let fresh_reset = tauri::async_runtime::block_on(super::reset_signed_planet(
            &reopened,
            user_id,
            "access-token",
            &api,
            false,
        ));
        assert!(
            fresh_reset.is_err(),
            "the next fresh reset remains held until guest import completes"
        );
        assert_eq!(
            *api.calls.lock().unwrap(),
            calls_after_recovery,
            "fresh hold occurs before another state fetch or reset call"
        );
    }

    #[test]
    fn fresh_signed_reset_holds_before_planet_state_fetch_when_guest_shop_import_is_pending() {
        let user_id = "00000000-0000-0000-0000-000000000052";
        let (state, old_cycle_id) = app_state_with_account(user_id);
        {
            let ledger = state.ledger.lock().unwrap();
            ledger.connection.execute(
                "INSERT INTO shop_landscape_instance(account_id,instance_id,sku,variation_index,seed,variation_version,acquired_at_utc)
                 VALUES ('local','unimported-guest-instance','land_tree',0,'seed',1,'2026-10-01T00:00:00Z')",
                [],
            ).unwrap();
            assert!(ledger.has_local_guest_shop_state().unwrap());
        }
        let new_cycle_id = "80000000-0000-0000-0000-000000000052";
        let api = MockResetRecoveryApi {
            ledger: None,
            context_change: None,
            result: Some(server_reset_result(
                &state,
                uuid::Uuid::new_v4(),
                new_cycle_id,
            )),
            timeline: server_reset_timeline(user_id, &old_cycle_id, new_cycle_id),
            pre_reset_timeline: Some(server_timeline(user_id, &old_cycle_id)),
            remote_state: Some(server_planet_state(&old_cycle_id)),
            canonical_state: Some(server_planet_state(&old_cycle_id)),
            fail_timeline: false,
            timeline_calls: Mutex::new(0),
            reset_failures: Mutex::new(0),
            reset_http_rejections: Mutex::new(VecDeque::new()),
            server_receipt: Mutex::new(None),
            usage_scan_failed_on_reset: None,
            calls: Mutex::new(Vec::new()),
        };

        let result = tauri::async_runtime::block_on(super::reset_signed_planet(
            &state,
            user_id,
            "access-token",
            &api,
            false,
        ));

        assert!(
            result.is_err(),
            "unimported guest shop state must hold a fresh signed reset"
        );
        assert!(
            api.calls.lock().unwrap().is_empty(),
            "the hold must precede state, timeline, upload, and reset calls"
        );
        let ledger = state.ledger.lock().unwrap();
        assert_eq!(ledger.planet_cycle_id().unwrap(), old_cycle_id);
        assert!(ledger
            .pending_signed_reset_intent(user_id)
            .unwrap()
            .is_none());
    }

    #[test]
    fn lost_reset_receipt_survives_http_401_and_429_until_same_id_recovery() {
        let user_id = "00000000-0000-0000-0000-000000000051";
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.sqlite3");
        let old_cycle_id;
        {
            let mut ledger = Ledger::open(&path, UTC).unwrap();
            ledger
                .connection
                .execute(
                    "UPDATE setting SET value='2026-10-01T00:00:00Z'
                 WHERE key IN ('planet_activation_at_utc','planet_cycle_started_at_utc')",
                    [],
                )
                .unwrap();
            ledger.ensure_planet_account(user_id).unwrap();
            add_usage(
                &mut ledger,
                "reset-http-replay-usage",
                "2026-10-01T12:00:00Z",
                100,
            );
            old_cycle_id = ledger.planet_cycle_id().unwrap();
        }
        let state = app_state_from_ledger(Ledger::open(&path, UTC).unwrap());
        let new_cycle_id = "80000000-0000-0000-0000-000000000051";
        let api = MockResetRecoveryApi {
            ledger: None,
            context_change: None,
            result: Some(server_reset_result(
                &state,
                uuid::Uuid::new_v4(),
                new_cycle_id,
            )),
            timeline: server_reset_timeline(user_id, &old_cycle_id, new_cycle_id),
            pre_reset_timeline: Some(server_timeline(user_id, &old_cycle_id)),
            remote_state: Some(server_planet_state(&old_cycle_id)),
            canonical_state: Some(server_planet_state(&old_cycle_id)),
            fail_timeline: false,
            timeline_calls: Mutex::new(0),
            // First RPC commits its receipt but loses the response; replays then
            // return transient HTTP failures before the stored receipt is received.
            reset_failures: Mutex::new(1),
            reset_http_rejections: Mutex::new(VecDeque::from([401, 429])),
            server_receipt: Mutex::new(None),
            usage_scan_failed_on_reset: None,
            calls: Mutex::new(Vec::new()),
        };

        let first_result = tauri::async_runtime::block_on(super::reset_signed_planet(
            &state,
            user_id,
            "access-token",
            &api,
            false,
        ));
        assert!(first_result.is_err());
        let first_reset_call = api.calls.lock().unwrap()[3].clone();
        let request_id = first_reset_call
            .strip_prefix("reset:")
            .unwrap()
            .split(':')
            .next()
            .unwrap();
        let request_id = uuid::Uuid::parse_str(request_id).unwrap();
        assert_eq!(
            first_reset_call,
            format!("reset:{request_id}:{old_cycle_id}")
        );
        drop(state);

        let reopened = app_state_from_ledger(Ledger::open(&path, UTC).unwrap());
        for status_code in [401, 429] {
            let retry = tauri::async_runtime::block_on(recover_pending_signed_reset(
                &reopened,
                user_id,
                "access-token",
                &api,
            ));

            assert!(
                retry.is_err(),
                "HTTP {status_code} cannot settle an ambiguous reset"
            );
            let ledger = reopened.ledger.lock().unwrap();
            let intent = ledger
                .pending_signed_reset_intent(user_id)
                .unwrap()
                .unwrap();
            assert_eq!(intent.request_id, request_id);
            assert_eq!(intent.expected_old_cycle_id, old_cycle_id);
            assert_eq!(ledger.planet_cycle_id().unwrap(), old_cycle_id);
        }

        let recovered = tauri::async_runtime::block_on(recover_pending_signed_reset(
            &reopened,
            user_id,
            "access-token",
            &api,
        ));
        assert!(matches!(
            recovered.unwrap(),
            Some(super::SignedResetCompletion::Confirmed(_))
        ));
        let calls = api.calls.lock().unwrap().clone();
        assert_eq!(
            calls,
            vec![
                "planet_state".into(),
                "timeline".into(),
                "upload".into(),
                format!("reset:{request_id}:{old_cycle_id}"),
                format!("reset:{request_id}:{old_cycle_id}"),
                format!("reset:{request_id}:{old_cycle_id}"),
                format!("reset:{request_id}:{old_cycle_id}"),
                "timeline".into(),
            ],
            "all retries reuse the original request without another PlanetState read",
        );
        let ledger = reopened.ledger.lock().unwrap();
        assert_eq!(ledger.planet_cycle_id().unwrap(), new_cycle_id);
        assert!(ledger
            .pending_signed_reset_intent(user_id)
            .unwrap()
            .is_none());
    }

    #[test]
    fn paused_preupload_prevents_signed_reset_and_creates_no_intent() {
        let user_id = "00000000-0000-0000-0000-000000000043";
        let (state, cycle_id) = app_state_with_account(user_id);
        let api = MockResetRecoveryApi {
            ledger: None,
            context_change: None,
            result: None,
            timeline: server_timeline(user_id, &cycle_id),
            pre_reset_timeline: Some(server_timeline(user_id, &cycle_id)),
            remote_state: Some(server_planet_state(&cycle_id)),
            canonical_state: Some(server_planet_state(&cycle_id)),
            fail_timeline: false,
            timeline_calls: Mutex::new(0),
            reset_failures: Mutex::new(0),
            reset_http_rejections: Mutex::new(VecDeque::new()),
            server_receipt: Mutex::new(None),
            usage_scan_failed_on_reset: None,
            calls: Mutex::new(Vec::new()),
        };

        let result = tauri::async_runtime::block_on(super::reset_signed_planet(
            &state,
            user_id,
            "access-token",
            &api,
            true,
        ));

        assert!(
            result.is_err(),
            "server sharing pause holds the required preupload"
        );
        assert_eq!(
            *api.calls.lock().unwrap(),
            vec![String::from("planet_state"), String::from("timeline")],
            "a held upload must not send a reset RPC or upload contribution",
        );
        let ledger = state.ledger.lock().unwrap();
        assert_eq!(ledger.planet_cycle_id().unwrap(), cycle_id);
        assert!(ledger
            .pending_signed_reset_intent(user_id)
            .unwrap()
            .is_none());
        assert_eq!(
            ledger
                .connection
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM shop_action_request WHERE account_id=?1",
                    [format!("account:{user_id}")],
                    |row| row.get(0),
                )
                .unwrap(),
            0,
            "a held preupload leaves no durable reset intent"
        );
    }

    #[test]
    fn local_pause_holds_actual_signed_reset_before_upload_or_intent() {
        let user_id = "00000000-0000-0000-0000-000000000052";
        let (state, cycle_id) = app_state_with_account(user_id);
        state
            .ledger
            .lock()
            .unwrap()
            .set_sharing_paused(true)
            .unwrap();
        let api = MockResetRecoveryApi {
            ledger: None,
            context_change: None,
            result: None,
            timeline: server_timeline(user_id, &cycle_id),
            pre_reset_timeline: Some(server_timeline(user_id, &cycle_id)),
            remote_state: Some(server_planet_state(&cycle_id)),
            canonical_state: Some(server_planet_state(&cycle_id)),
            fail_timeline: false,
            timeline_calls: Mutex::new(0),
            reset_failures: Mutex::new(0),
            reset_http_rejections: Mutex::new(VecDeque::new()),
            server_receipt: Mutex::new(None),
            usage_scan_failed_on_reset: None,
            calls: Mutex::new(Vec::new()),
        };

        let result = tauri::async_runtime::block_on(super::reset_signed_planet(
            &state,
            user_id,
            "access-token",
            &api,
            false,
        ));

        assert!(result.is_err(), "local sharing pause holds reset preupload");
        assert_eq!(
            *api.calls.lock().unwrap(),
            vec!["planet_state", "timeline"],
            "local pause permits private reads but blocks contribution and reset writes",
        );
        let ledger = state.ledger.lock().unwrap();
        assert_eq!(ledger.planet_cycle_id().unwrap(), cycle_id);
        assert!(ledger
            .pending_signed_reset_intent(user_id)
            .unwrap()
            .is_none());
    }

    #[test]
    fn confirmed_cooldown_prevents_signed_reset_and_creates_no_intent() {
        let user_id = "00000000-0000-0000-0000-000000000050";
        let (state, cycle_id) = app_state_with_account(user_id);
        let mut canonical_state = server_planet_state(&cycle_id);
        canonical_state.can_reset = false;
        canonical_state.reset_available_at_utc =
            Some((chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339());
        let api = MockResetRecoveryApi {
            ledger: None,
            context_change: None,
            result: None,
            timeline: server_reset_timeline(
                user_id,
                &cycle_id,
                "80000000-0000-0000-0000-000000000050",
            ),
            pre_reset_timeline: Some(server_timeline(user_id, &cycle_id)),
            remote_state: Some(server_planet_state(&cycle_id)),
            canonical_state: Some(canonical_state),
            fail_timeline: false,
            timeline_calls: Mutex::new(0),
            reset_failures: Mutex::new(0),
            reset_http_rejections: Mutex::new(VecDeque::new()),
            server_receipt: Mutex::new(None),
            usage_scan_failed_on_reset: None,
            calls: Mutex::new(Vec::new()),
        };

        let result = tauri::async_runtime::block_on(super::reset_signed_planet(
            &state,
            user_id,
            "access-token",
            &api,
            false,
        ));

        assert!(
            result.is_err(),
            "the canonical server cooldown holds a new reset"
        );
        assert_eq!(
            *api.calls.lock().unwrap(),
            vec!["planet_state", "timeline", "upload"],
            "a known cooldown must not create a reset intent or call the reset RPC",
        );
        let ledger = state.ledger.lock().unwrap();
        assert_eq!(ledger.planet_cycle_id().unwrap(), cycle_id);
        assert!(ledger
            .pending_signed_reset_intent(user_id)
            .unwrap()
            .is_none());
        assert_eq!(
            ledger
                .connection
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM shop_action_request WHERE account_id=?1",
                    [format!("account:{user_id}")],
                    |row| row.get(0),
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn reset_timeline_failure_keeps_intent_and_does_not_apply_new_cycle_cache() {
        let user_id = "00000000-0000-0000-0000-000000000046";
        let (state, old_cycle_id) = app_state_with_account(user_id);
        let request_id = uuid::Uuid::parse_str("90000000-0000-0000-0000-000000000046").unwrap();
        state
            .ledger
            .lock()
            .unwrap()
            .prepare_signed_reset_intent(request_id, &old_cycle_id)
            .unwrap();
        let api = MockResetRecoveryApi {
            ledger: None,
            context_change: None,
            result: Some(server_reset_result(
                &state,
                request_id,
                "80000000-0000-0000-0000-000000000046",
            )),
            timeline: server_reset_timeline(
                user_id,
                &old_cycle_id,
                "80000000-0000-0000-0000-000000000046",
            ),
            pre_reset_timeline: None,
            remote_state: None,
            canonical_state: None,
            fail_timeline: true,
            timeline_calls: Mutex::new(0),
            reset_failures: Mutex::new(0),
            reset_http_rejections: Mutex::new(VecDeque::new()),
            server_receipt: Mutex::new(None),
            usage_scan_failed_on_reset: None,
            calls: Mutex::new(Vec::new()),
        };

        let result = tauri::async_runtime::block_on(recover_pending_signed_reset(
            &state,
            user_id,
            "access-token",
            &api,
        ));

        assert!(result.is_err());
        assert_eq!(
            *api.calls.lock().unwrap(),
            vec![
                format!("reset:{request_id}:{old_cycle_id}"),
                "timeline".into(),
            ]
        );
        let ledger = state.ledger.lock().unwrap();
        assert_eq!(ledger.planet_cycle_id().unwrap(), old_cycle_id);
        assert_eq!(
            ledger
                .pending_signed_reset_intent(user_id)
                .unwrap()
                .unwrap()
                .request_id,
            request_id,
        );
        assert_eq!(ledger.connection.query_row::<i64, _, _>(
            "SELECT count(*) FROM shop_action_request WHERE account_id=?1 AND request_id=?2 AND result_json IS NULL",
            rusqlite::params![format!("account:{user_id}"), request_id.to_string()],
            |row| row.get(0),
        ).unwrap(), 1);
    }

    #[test]
    fn reset_receipt_update_failure_rolls_back_cache_and_same_intent_retries() {
        let user_id = "00000000-0000-0000-0000-000000000049";
        let (state, old_cycle_id) = app_state_with_account(user_id);
        let request_id = uuid::Uuid::parse_str("90000000-0000-0000-0000-000000000049").unwrap();
        state
            .ledger
            .lock()
            .unwrap()
            .prepare_signed_reset_intent(request_id, &old_cycle_id)
            .unwrap();
        let new_cycle_id = "80000000-0000-0000-0000-000000000049";
        let api = MockResetRecoveryApi {
            ledger: None,
            context_change: None,
            result: Some(server_reset_result(&state, request_id, new_cycle_id)),
            timeline: server_reset_timeline(user_id, &old_cycle_id, new_cycle_id),
            pre_reset_timeline: None,
            remote_state: None,
            canonical_state: None,
            fail_timeline: false,
            timeline_calls: Mutex::new(0),
            reset_failures: Mutex::new(0),
            reset_http_rejections: Mutex::new(VecDeque::new()),
            server_receipt: Mutex::new(None),
            usage_scan_failed_on_reset: None,
            calls: Mutex::new(Vec::new()),
        };
        let account_scope = format!("account:{user_id}");
        {
            let ledger = state.ledger.lock().unwrap();
            ledger
                .connection
                .execute_batch(&format!(
                    "CREATE TRIGGER fail_signed_reset_receipt
                 BEFORE UPDATE OF result_json ON shop_action_request
                 WHEN OLD.account_id='{account_scope}' AND OLD.request_id='{request_id}'
                 BEGIN SELECT RAISE(ABORT, 'forced receipt write failure'); END;"
                ))
                .unwrap();
        }

        let failed_apply = tauri::async_runtime::block_on(recover_pending_signed_reset(
            &state,
            user_id,
            "access-token",
            &api,
        ));

        assert!(failed_apply.is_err());
        {
            let ledger = state.ledger.lock().unwrap();
            assert_eq!(ledger.planet_cycle_id().unwrap(), old_cycle_id);
            assert_eq!(
                ledger
                    .pending_signed_reset_intent(user_id)
                    .unwrap()
                    .unwrap()
                    .request_id,
                request_id,
            );
            assert_eq!(
                ledger
                    .connection
                    .query_row::<i64, _, _>(
                        "SELECT count(*) FROM shop_effect_timeline_state WHERE account_id=?1",
                        [&account_scope],
                        |row| row.get(0),
                    )
                    .unwrap(),
                0,
                "timeline cache changes roll back with a failed intent completion"
            );
            assert_eq!(ledger.connection.query_row::<i64, _, _>(
                "SELECT count(*) FROM shop_action_request WHERE account_id=?1 AND request_id=?2 AND result_json IS NULL",
                rusqlite::params![account_scope, request_id.to_string()],
                |row| row.get(0),
            ).unwrap(), 1);
        }
        state
            .ledger
            .lock()
            .unwrap()
            .connection
            .execute_batch("DROP TRIGGER fail_signed_reset_receipt;")
            .unwrap();

        let retried = tauri::async_runtime::block_on(recover_pending_signed_reset(
            &state,
            user_id,
            "access-token",
            &api,
        ));

        assert!(
            retried.is_ok(),
            "same persisted request should succeed after cache transaction can commit: {retried:?}"
        );
        assert_eq!(
            *api.calls.lock().unwrap(),
            vec![
                format!("reset:{request_id}:{old_cycle_id}"),
                "timeline".into(),
                format!("reset:{request_id}:{old_cycle_id}"),
                "timeline".into(),
            ],
        );
        let ledger = state.ledger.lock().unwrap();
        assert_eq!(ledger.planet_cycle_id().unwrap(), new_cycle_id);
        assert!(ledger
            .pending_signed_reset_intent(user_id)
            .unwrap()
            .is_none());
    }

    #[test]
    fn committed_reset_with_unavailable_snapshot_is_reported_as_confirmed() {
        let user_id = "00000000-0000-0000-0000-000000000052";
        let (state, old_cycle_id) = app_state_with_account(user_id);
        let request_id = uuid::Uuid::parse_str("90000000-0000-0000-0000-000000000052").unwrap();
        state
            .ledger
            .lock()
            .unwrap()
            .prepare_signed_reset_intent(request_id, &old_cycle_id)
            .unwrap();
        let new_cycle_id = "80000000-0000-0000-0000-000000000052";
        let api = MockResetRecoveryApi {
            ledger: None,
            context_change: None,
            result: Some(server_reset_result(&state, request_id, new_cycle_id)),
            timeline: server_reset_timeline(user_id, &old_cycle_id, new_cycle_id),
            pre_reset_timeline: None,
            remote_state: None,
            canonical_state: None,
            fail_timeline: false,
            timeline_calls: Mutex::new(0),
            reset_failures: Mutex::new(0),
            reset_http_rejections: Mutex::new(VecDeque::new()),
            server_receipt: Mutex::new(None),
            usage_scan_failed_on_reset: Some(&state.usage_scan_failed),
            calls: Mutex::new(Vec::new()),
        };

        let recovered = tauri::async_runtime::block_on(recover_pending_signed_reset(
            &state,
            user_id,
            "access-token",
            &api,
        ));

        {
            let ledger = state.ledger.lock().unwrap();
            assert_eq!(ledger.planet_cycle_id().unwrap(), new_cycle_id);
            assert!(ledger
                .pending_signed_reset_intent(user_id)
                .unwrap()
                .is_none());
            assert_eq!(
                ledger.connection.query_row::<i64, _, _>(
                    "SELECT count(*) FROM shop_action_request WHERE account_id=?1 AND request_id=?2 AND result_json IS NOT NULL",
                    rusqlite::params![format!("account:{user_id}"), request_id.to_string()],
                    |row| row.get(0),
                ).unwrap(),
                1,
                "confirmed cache state and completed intent commit before the view refresh",
            );
        }
        match recovered.expect("a committed reset must not become a retryable error") {
            Some(super::SignedResetCompletion::ConfirmedViewUnavailable {
                account_id,
                request_id: completed_request_id,
                expected_old_cycle_id,
                new_cycle_id: completed_new_cycle_id,
            }) => {
                assert_eq!(account_id, user_id);
                assert_eq!(completed_request_id, request_id);
                assert_eq!(expected_old_cycle_id, old_cycle_id);
                assert_eq!(completed_new_cycle_id, new_cycle_id);
            }
            other => panic!("expected typed confirmed view failure, got {other:?}"),
        }
        assert_eq!(
            *api.calls.lock().unwrap(),
            vec![
                format!("reset:{request_id}:{old_cycle_id}"),
                "timeline".into(),
            ],
        );
        let follow_up = tauri::async_runtime::block_on(recover_pending_signed_reset(
            &state,
            user_id,
            "access-token",
            &api,
        ));
        assert!(matches!(follow_up, Ok(None)));
        assert_eq!(
            api.calls.lock().unwrap().len(),
            2,
            "completed reset recovery performs no second reset after a view refresh failure",
        );
    }

    #[test]
    fn account_or_cycle_change_after_reset_rpc_keeps_intent_and_never_applies_result() {
        for (user_id, context_change) in [
            (
                "00000000-0000-0000-0000-000000000047",
                ResetContextChange::Account,
            ),
            (
                "00000000-0000-0000-0000-000000000048",
                ResetContextChange::Cycle,
            ),
        ] {
            let (state, old_cycle_id) = app_state_with_account(user_id);
            let request_id = uuid::Uuid::new_v4();
            state
                .ledger
                .lock()
                .unwrap()
                .prepare_signed_reset_intent(request_id, &old_cycle_id)
                .unwrap();
            let new_cycle_id = "80000000-0000-0000-0000-000000000047";
            let api = MockResetRecoveryApi {
                ledger: Some(&state.ledger),
                context_change: Some(context_change),
                result: Some(server_reset_result(&state, request_id, new_cycle_id)),
                timeline: server_reset_timeline(user_id, &old_cycle_id, new_cycle_id),
                pre_reset_timeline: None,
                remote_state: None,
                canonical_state: None,
                fail_timeline: false,
                timeline_calls: Mutex::new(0),
                reset_failures: Mutex::new(0),
                reset_http_rejections: Mutex::new(VecDeque::new()),
                server_receipt: Mutex::new(None),
                usage_scan_failed_on_reset: None,
                calls: Mutex::new(Vec::new()),
            };

            let result = tauri::async_runtime::block_on(recover_pending_signed_reset(
                &state,
                user_id,
                "access-token",
                &api,
            ));

            assert!(result.is_err());
            assert_eq!(
                *api.calls.lock().unwrap(),
                vec![format!("reset:{request_id}:{old_cycle_id}")],
                "a stale account or cycle must stop before the timeline/cache write",
            );
            let ledger = state.ledger.lock().unwrap();
            assert_eq!(
                ledger
                    .pending_signed_reset_intent(user_id)
                    .unwrap()
                    .unwrap()
                    .request_id,
                request_id,
            );
            assert_ne!(ledger.planet_cycle_id().unwrap(), new_cycle_id);
        }
    }

    fn server_planet_state(cycle_id: &str) -> PlanetState {
        PlanetState {
            version: 1,
            profile: Some(crate::domain::planet::PlanetProfile {
                nickname: "server profile".into(),
                avatar: crate::domain::planet::PlanetAvatar::Masculine,
            }),
            timezone: "UTC".into(),
            current_cycle_id: cycle_id.into(),
            cycle_started_at_utc: "2026-10-01T00:00:00Z".into(),
            last_reset_at_utc: None,
            wallet_balance: 90_000,
            wallet_credits: vec![],
            current_planet_tokens: 0,
            lifetime_tokens: 0,
            growth_credit: 0.0,
            stage: 1,
            progress_to_next: 0.0,
            incomplete: false,
            can_reset: true,
            reset_available_at_utc: None,
            objects: vec![],
            removed_natural_keys: vec![],
        }
    }

    fn server_timeline(user_id: &str, cycle_id: &str) -> ShopEffectTimeline {
        ShopEffectTimeline {
            account_id: user_id.into(),
            current_cycle_id: cycle_id.into(),
            effect_revision: 7,
            server_time_utc: "2026-10-02T00:00:00Z".into(),
            reward_timezone: "UTC".into(),
            cycle_bounds: vec![ShopCycleBound {
                cycle_id: cycle_id.into(),
                started_at_utc: "2026-10-01T00:00:00Z".into(),
                ended_at_utc: None,
            }],
            intervals: vec![],
        }
    }

    fn server_reset_result(
        state: &AppState,
        request_id: uuid::Uuid,
        new_cycle_id: &str,
    ) -> ResetShopResult {
        let reset_at = "2026-10-02T00:00:00Z";
        let mut planet_state = server_planet_state(new_cycle_id);
        planet_state.cycle_started_at_utc = reset_at.into();
        planet_state.last_reset_at_utc = Some(reset_at.into());
        planet_state.reset_available_at_utc = Some("2026-10-02T18:00:00Z".into());
        let mut action_state = state.ledger.lock().unwrap().shop_state().unwrap();
        action_state.current_cycle_id = new_cycle_id.into();
        ResetShopResult {
            action: ShopActionResult {
                status: ShopActionStatus::Reset,
                request_id: request_id.to_string(),
                confirmed_quote: None,
                state: action_state,
            },
            planet_state,
        }
    }

    fn server_reset_timeline(
        user_id: &str,
        old_cycle_id: &str,
        cycle_id: &str,
    ) -> ShopEffectTimeline {
        let reset_at = "2026-10-02T00:00:00Z";
        ShopEffectTimeline {
            account_id: user_id.into(),
            current_cycle_id: cycle_id.into(),
            effect_revision: 7,
            server_time_utc: reset_at.into(),
            reward_timezone: "UTC".into(),
            cycle_bounds: vec![
                ShopCycleBound {
                    cycle_id: old_cycle_id.into(),
                    started_at_utc: "2026-10-01T00:00:00Z".into(),
                    ended_at_utc: Some(reset_at.into()),
                },
                ShopCycleBound {
                    cycle_id: cycle_id.into(),
                    started_at_utc: reset_at.into(),
                    ended_at_utc: None,
                },
            ],
            intervals: vec![],
        }
    }

    #[derive(Clone, Copy, PartialEq)]
    enum ContextChange {
        Account,
        Cycle,
    }

    #[derive(Clone, Copy, PartialEq)]
    enum ContextChangeAt {
        PlanetState,
        Timeline,
        Upload,
    }

    struct MockPrivateSyncApi<'a> {
        ledger: Option<&'a Mutex<Ledger>>,
        remote_state: Option<PlanetState>,
        timeline: ShopEffectTimeline,
        canonical_state: PlanetState,
        fail_state: bool,
        fail_timeline: bool,
        fail_first_upload: Mutex<bool>,
        context_change: Option<(ContextChangeAt, ContextChange)>,
        calls: Mutex<Vec<&'static str>>,
        order_calls: Option<std::sync::Arc<Mutex<Vec<&'static str>>>>,
        uploads: Mutex<Vec<(serde_json::Value, serde_json::Value)>>,
    }

    impl MockPrivateSyncApi<'_> {
        fn record_call(&self, name: &'static str) {
            self.calls.lock().unwrap().push(name);
            if let Some(order_calls) = &self.order_calls {
                order_calls.lock().unwrap().push(name);
            }
        }
    }

    impl MockPrivateSyncApi<'_> {
        fn change_context(&self, at: ContextChangeAt) {
            let (Some(ledger), Some((change_at, change))) = (self.ledger, self.context_change)
            else {
                return;
            };
            if change_at != at {
                return;
            }
            let mut ledger = ledger.lock().unwrap();
            match change {
                ContextChange::Account => {
                    ledger
                        .ensure_planet_account("00000000-0000-0000-0000-000000000099")
                        .unwrap();
                }
                ContextChange::Cycle => {
                    ledger.connection.execute(
                        "UPDATE setting SET value='changed-cycle' WHERE key='planet_current_cycle_id'",
                        [],
                    ).unwrap();
                }
            }
        }
    }

    impl PrivateEffectSyncApi for MockPrivateSyncApi<'_> {
        fn my_planet_state<'a>(
            &'a self,
            _access_token: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<Option<PlanetState>, SyncError>> + Send + 'a>>
        {
            self.record_call("planet_state");
            self.change_context(ContextChangeAt::PlanetState);
            let result = if self.fail_state {
                Err(SyncError::Transport)
            } else {
                Ok(self.remote_state.clone())
            };
            Box::pin(async move { result })
        }

        fn get_my_shop_effect_timeline<'a>(
            &'a self,
            _access_token: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<ShopEffectTimeline, SyncError>> + Send + 'a>>
        {
            self.record_call("timeline");
            self.change_context(ContextChangeAt::Timeline);
            let result = if self.fail_timeline {
                Err(SyncError::Transport)
            } else {
                Ok(self.timeline.clone())
            };
            Box::pin(async move { result })
        }

        fn upload_planet_state_with_effects<'a>(
            &'a self,
            _access_token: &'a str,
            state: &'a PlanetState,
            contribution: &'a PlanetDeviceContributionSnapshot,
        ) -> Pin<Box<dyn Future<Output = Result<PlanetState, SyncError>> + Send + 'a>> {
            self.record_call("upload");
            self.change_context(ContextChangeAt::Upload);
            self.uploads.lock().unwrap().push((
                serde_json::to_value(state).unwrap(),
                serde_json::to_value(contribution).unwrap(),
            ));
            let fail = {
                let mut remaining = self.fail_first_upload.lock().unwrap();
                let fail = *remaining;
                *remaining = false;
                fail
            };
            let result = if fail {
                Err(SyncError::Transport)
            } else {
                Ok(self.canonical_state.clone())
            };
            Box::pin(async move { result })
        }
    }

    fn mock_private_api<'a>(
        ledger: Option<&'a Mutex<Ledger>>,
        user_id: &str,
        cycle_id: &str,
    ) -> MockPrivateSyncApi<'a> {
        MockPrivateSyncApi {
            ledger,
            remote_state: Some(server_planet_state(cycle_id)),
            timeline: server_timeline(user_id, cycle_id),
            canonical_state: server_planet_state(cycle_id),
            fail_state: false,
            fail_timeline: false,
            fail_first_upload: Mutex::new(false),
            context_change: None,
            calls: Mutex::new(Vec::new()),
            order_calls: None,
            uploads: Mutex::new(Vec::new()),
        }
    }

    #[test]
    fn unimported_guest_shop_ownership_blocks_personal_sync_before_remote_work() {
        let mut ledger = Ledger::open(Path::new(":memory:"), UTC).unwrap();
        let guest_account = ledger.cosmetic_account_id().unwrap();
        let cycle_id = ledger.planet_cycle_id().unwrap();
        ledger.connection.execute(
            "INSERT INTO shop_landscape_instance(account_id,instance_id,sku,variation_index,seed,variation_version,acquired_at_utc)
             VALUES (?1,'guest-only-instance','land_tree',0,'seed',1,'2026-10-01T00:00:00Z')",
            [&guest_account],
        ).unwrap();
        ledger
            .ensure_planet_account("00000000-0000-0000-0000-000000000064")
            .unwrap();
        assert!(ledger.pending_guest_cosmetic_import().unwrap().is_none());

        let result = require_guest_shop_import_complete(&ledger);

        assert!(
            result.is_err(),
            "guest ownership requires a complete import first"
        );
        assert_eq!(ledger.planet_cycle_id().unwrap(), cycle_id);
        assert_eq!(ledger.connection.query_row::<i64, _, _>(
            "SELECT count(*) FROM shop_landscape_instance WHERE account_id=?1 AND instance_id='guest-only-instance'",
            [&guest_account], |row| row.get(0),
        ).unwrap(), 1, "the pending guest source row must remain untouched");
    }

    #[test]
    fn native_ordinary_first_reset_capture_stops_sync_before_remote_work() {
        let mut ledger = Ledger::open(Path::new(":memory:"), UTC).unwrap();
        ledger
            .set_planet_profile("Synthetic First Reset", PlanetAvatar::Masculine)
            .unwrap();
        let reset_at = Utc::now() + chrono::Duration::minutes(1);
        let occurred_at = reset_at - chrono::Duration::seconds(30);
        add_usage(
            &mut ledger,
            "native-first-reset-sync-guard",
            &occurred_at.to_rfc3339(),
            1_000_000,
        );
        let target = "account:00000000-0000-4000-a000-000000000001";
        assert_eq!(ledger.reset_planet(reset_at).unwrap(), 1_000_000);
        ledger.prepare_growth_journal().unwrap();
        let captured = ledger.capture_guest_shop_import(target).unwrap();
        assert_eq!(
            captured.snapshot.disposition,
            GuestShopImportDisposition::SourceUnverifiable
        );
        let current_cycle_id = ledger.planet_cycle_id().unwrap();
        let state = app_state_from_ledger(ledger);

        let result = tauri::async_runtime::block_on(sync_once(&state));

        assert_eq!(
            result,
            Err("게스트 상점 가져오기를 완료한 뒤 동기화할 수 있습니다".into())
        );
        let ledger = state.ledger.lock().unwrap();
        assert_eq!(ledger.planet_cycle_id().unwrap(), current_cycle_id);
        let pending = ledger.pending_guest_shop_import(target).unwrap().unwrap();
        assert_eq!(pending.snapshot.import_id, captured.snapshot.import_id);
        assert_eq!(
            pending.snapshot.disposition,
            GuestShopImportDisposition::SourceUnverifiable
        );
    }

    #[test]
    fn pending_local_guest_shop_stops_first_login_before_account_selection_or_session_rpc() {
        let _env_guard = crate::sync::auth::AUTH_ENV_LOCK.lock().unwrap();
        let old_url = std::env::var_os("TOKEN_PLANET_SUPABASE_URL");
        let old_key = std::env::var_os("TOKEN_PLANET_SUPABASE_PUBLISHABLE_KEY");
        std::env::set_var("TOKEN_PLANET_SUPABASE_URL", "");
        std::env::set_var("TOKEN_PLANET_SUPABASE_PUBLISHABLE_KEY", "");

        let ledger = Ledger::open(Path::new(":memory:"), UTC).unwrap();
        let guest_account = ledger.cosmetic_account_id().unwrap();
        let original_cycle = ledger.planet_cycle_id().unwrap();
        ledger.connection.execute(
            "INSERT INTO shop_landscape_instance(account_id,instance_id,sku,variation_index,seed,variation_version,acquired_at_utc)
             VALUES (?1,'pending-guest-instance','land_tree',0,'seed',1,'2026-10-01T00:00:00Z')",
            [&guest_account],
        ).unwrap();
        let state = AppState {
            lifecycle: crate::lifecycle::LocalLifecycle::new(
                &crate::domain::device_reset::DeviceResetState::default(),
            ),
            config: Mutex::new(SourceConfig {
                codex_root: Path::new("/tmp/token-planet-missing-codex").to_path_buf(),
                claude_root: Path::new("/tmp/token-planet-missing-claude").to_path_buf(),
                timezone: UTC,
            }),
            ledger: Mutex::new(ledger),
            latest: Mutex::new(None),
            usage_scan_failed: AtomicBool::new(false),
            sync_failed: Mutex::new(false),
            sync_gate: tokio::sync::Mutex::new(()),
            window_mode: Mutex::new(WindowMode::Popup),
            mode_transitioning: AtomicBool::new(false),
            tray_press_pending: AtomicBool::new(false),
        };

        let result = tauri::async_runtime::block_on(super::sync_once(&state));

        if let Some(value) = old_url {
            std::env::set_var("TOKEN_PLANET_SUPABASE_URL", value);
        } else {
            std::env::remove_var("TOKEN_PLANET_SUPABASE_URL");
        }
        if let Some(value) = old_key {
            std::env::set_var("TOKEN_PLANET_SUPABASE_PUBLISHABLE_KEY", value);
        } else {
            std::env::remove_var("TOKEN_PLANET_SUPABASE_PUBLISHABLE_KEY");
        }

        assert_eq!(
            result,
            Err("게스트 상점 가져오기를 완료한 뒤 동기화할 수 있습니다".into()),
            "pending local guest state must block first login before any RPC",
        );
        let ledger = state.ledger.lock().unwrap();
        assert_eq!(
            ledger.cosmetic_account_id().unwrap(),
            "local",
            "first login must not move the active account"
        );
        assert_eq!(ledger.planet_cycle_id().unwrap(), original_cycle);
        assert_eq!(ledger.connection.query_row::<i64, _, _>(
            "SELECT count(*) FROM shop_landscape_instance WHERE account_id=?1 AND instance_id='pending-guest-instance'",
            [&guest_account], |row| row.get(0),
        ).unwrap(), 1);
    }

    #[test]
    fn missing_server_planet_state_holds_without_timeline_upload_or_cache_change() {
        let user_id = "00000000-0000-0000-0000-000000000064";
        let (state, cycle_id) = app_state_with_account(user_id);
        let api = mock_private_api(Some(&state.ledger), user_id, &cycle_id);
        let mut api = api;
        api.remote_state = None;

        let result = tauri::async_runtime::block_on(sync_private_effect_contribution(
            &state,
            user_id,
            "account-token",
            &api,
            false,
        ));

        assert!(result.is_err());
        assert_eq!(*api.calls.lock().unwrap(), vec!["planet_state"]);
        assert!(api.uploads.lock().unwrap().is_empty());
        let ledger = state.ledger.lock().unwrap();
        assert_eq!(ledger.planet_cycle_id().unwrap(), cycle_id);
        assert_eq!(
            ledger
                .connection
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM shop_effect_timeline_state WHERE account_id=?1",
                    [format!("account:{user_id}")],
                    |row| row.get(0),
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn timeline_fetch_or_canonical_apply_failure_prevents_private_upload() {
        let user_id = "00000000-0000-0000-0000-000000000064";
        for fail_fetch in [true, false] {
            let (state, cycle_id) = app_state_with_account(user_id);
            let mut api = mock_private_api(Some(&state.ledger), user_id, &cycle_id);
            if fail_fetch {
                api.fail_timeline = true;
            } else {
                api.timeline.server_time_utc = "2026-09-30T00:00:00Z".into();
            }

            let result = tauri::async_runtime::block_on(sync_private_effect_contribution(
                &state,
                user_id,
                "account-token",
                &api,
                false,
            ));

            assert!(result.is_err());
            assert_eq!(*api.calls.lock().unwrap(), vec!["planet_state", "timeline"]);
            assert!(api.uploads.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn account_or_cycle_change_during_fetch_timeline_or_upload_never_applies_stale_state() {
        let user_id = "00000000-0000-0000-0000-000000000064";
        for (at, change) in [
            (ContextChangeAt::PlanetState, ContextChange::Account),
            (ContextChangeAt::Timeline, ContextChange::Cycle),
            (ContextChangeAt::Upload, ContextChange::Account),
        ] {
            let (state, cycle_id) = app_state_with_account(user_id);
            let mut api = mock_private_api(Some(&state.ledger), user_id, &cycle_id);
            api.context_change = Some((at, change));

            let result = tauri::async_runtime::block_on(sync_private_effect_contribution(
                &state,
                user_id,
                "account-token",
                &api,
                false,
            ));

            assert!(result.is_err());
            assert!(!api.uploads.lock().unwrap().is_empty() || at != ContextChangeAt::Upload);
            let ledger = state.ledger.lock().unwrap();
            if at == ContextChangeAt::Upload {
                assert_eq!(
                    ledger.cosmetic_account_id().unwrap(),
                    "account:00000000-0000-0000-0000-000000000099"
                );
                assert!(
                    ledger.planet_profile().unwrap().is_none(),
                    "stale canonical response must not be cached for the newly selected account"
                );
            } else {
                assert!(api.uploads.lock().unwrap().is_empty());
            }
        }
    }

    #[test]
    fn private_effect_sync_echoes_server_planet_while_sharing_is_active() {
        let user_id = "00000000-0000-0000-0000-000000000064";
        let (state, cycle_id) = app_state_with_account(user_id);
        let api = mock_private_api(Some(&state.ledger), user_id, &cycle_id);
        let expected_server_state =
            serde_json::to_value(api.remote_state.as_ref().unwrap()).unwrap();

        let outcome = tauri::async_runtime::block_on(sync_private_effect_contribution(
            &state,
            user_id,
            "account-token",
            &api,
            false,
        ))
        .unwrap();

        assert_eq!(
            outcome,
            PrivateEffectSyncOutcome::Uploaded { can_reset: true }
        );
        assert!(outcome.require_uploaded().unwrap());
        assert_eq!(
            *api.calls.lock().unwrap(),
            vec!["planet_state", "timeline", "upload"]
        );
        let uploads = api.uploads.lock().unwrap();
        assert_eq!(uploads.len(), 1);
        assert_eq!(uploads[0].0, expected_server_state, "upload p_state must reuse the server response without local profile/timezone/object claims");
        let contribution = &uploads[0].1;
        let keys = contribution
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            keys,
            std::collections::BTreeSet::from([
                "activity_days",
                "canonical_version",
                "current_cycle_id",
                "current_planet_tokens",
                "daily_segments",
                "daily_tokens",
                "device_id",
                "incomplete",
                "lifetime_tokens",
            ])
        );
        assert_eq!(contribution["current_cycle_id"], cycle_id);
        assert_eq!(contribution["current_planet_tokens"], 100);
        assert_eq!(contribution["daily_segments"][0]["effect_revision"], 0);
        assert_eq!(
            contribution["activity_days"][0]["first_occurred_at_utc"],
            "2026-10-01T12:00:00+00:00"
        );
    }

    #[test]
    fn private_effect_sync_holds_upload_when_local_or_server_sharing_is_paused() {
        let user_id = "00000000-0000-0000-0000-000000000064";
        for (local_pause, world_policy_paused) in [(true, false), (false, true)] {
            let (state, cycle_id) = app_state_with_account(user_id);
            if local_pause {
                state
                    .ledger
                    .lock()
                    .unwrap()
                    .set_sharing_paused(true)
                    .unwrap();
            }
            let api = mock_private_api(Some(&state.ledger), user_id, &cycle_id);

            let outcome = tauri::async_runtime::block_on(sync_private_effect_contribution(
                &state,
                user_id,
                "account-token",
                &api,
                world_policy_paused,
            ))
            .unwrap();

            assert_eq!(outcome, PrivateEffectSyncOutcome::HeldForSharingPause);
            assert!(outcome.require_uploaded().is_err());
            assert_eq!(*api.calls.lock().unwrap(), vec!["planet_state", "timeline"]);
            assert!(api.uploads.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn uncertain_upload_retry_keeps_the_same_canonical_snapshot() {
        let user_id = "00000000-0000-0000-0000-000000000064";
        let (state, cycle_id) = app_state_with_account(user_id);
        let api = mock_private_api(Some(&state.ledger), user_id, &cycle_id);
        *api.fail_first_upload.lock().unwrap() = true;

        let first = tauri::async_runtime::block_on(sync_private_effect_contribution(
            &state,
            user_id,
            "account-token",
            &api,
            false,
        ));
        let second = tauri::async_runtime::block_on(sync_private_effect_contribution(
            &state,
            user_id,
            "account-token",
            &api,
            false,
        ));

        assert!(first.is_err());
        assert!(second.is_ok());
        let uploads = api.uploads.lock().unwrap();
        assert_eq!(uploads.len(), 2);
        assert_eq!(uploads[0], uploads[1], "same usage, cycle, and timeline revision must retry the identical p_state and aggregate payload");
    }

    #[test]
    fn cold_planet_bootstrap_uses_revision_zero_segments_without_activity_claims() {
        let raw = PlanetDeviceContribution {
            device_id: "device-1".into(),
            current_cycle_id: "cycle-1".into(),
            lifetime_tokens: 90,
            current_planet_tokens: 30,
            daily_tokens: BTreeMap::from([("2026-10-01".into(), 10), ("2026-10-02".into(), 20)]),
            incomplete: false,
        };

        let snapshot = bootstrap_contribution_snapshot(raw.clone()).unwrap();

        assert_eq!(snapshot.raw, raw);
        assert_eq!(snapshot.canonical_version, 0);
        assert_eq!(
            snapshot
                .daily_segments
                .iter()
                .map(|segment| (
                    segment.cycle_id.as_str(),
                    segment.date.as_str(),
                    segment.effect_revision,
                    segment.tokens,
                ))
                .collect::<Vec<_>>(),
            vec![
                ("cycle-1", "2026-10-01", 0, 10),
                ("cycle-1", "2026-10-02", 0, 20),
            ],
        );
        assert!(snapshot.activity_days.is_empty());
    }

    #[test]
    fn cold_planet_bootstrap_rejects_mismatched_or_overflowing_raw_totals() {
        let mismatch = PlanetDeviceContribution {
            device_id: "device-1".into(),
            current_cycle_id: "cycle-1".into(),
            lifetime_tokens: 10,
            current_planet_tokens: 10,
            daily_tokens: BTreeMap::from([("2026-10-01".into(), 9)]),
            incomplete: false,
        };
        assert!(bootstrap_contribution_snapshot(mismatch).is_err());

        let overflow = PlanetDeviceContribution {
            device_id: "device-1".into(),
            current_cycle_id: "cycle-1".into(),
            lifetime_tokens: u64::MAX,
            current_planet_tokens: u64::MAX,
            daily_tokens: BTreeMap::from([
                ("2026-10-01".into(), u64::MAX),
                ("2026-10-02".into(), 1),
            ]),
            incomplete: false,
        };
        assert!(bootstrap_contribution_snapshot(overflow).is_err());
    }

    #[test]
    fn guest_and_uninitialized_signed_usage_keep_the_local_exclusive_cutoff() {
        for signed_account in [false, true] {
            let mut ledger = Ledger::open(Path::new(":memory:"), UTC).unwrap();
            if signed_account {
                ledger
                    .ensure_planet_account("00000000-0000-0000-0000-000000000042")
                    .unwrap();
            }
            ledger
                .connection
                .execute(
                    "UPDATE setting SET value='2026-09-24T00:00:00Z'
                     WHERE key IN ('planet_activation_at_utc','planet_cycle_started_at_utc')",
                    [],
                )
                .unwrap();
            ledger
                .connection
                .execute(
                    "INSERT INTO setting(key,value) VALUES ('planet_last_reset_at_utc','2026-10-01T00:00:00Z')
                     ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                    [],
                )
                .unwrap();
            add_usage(&mut ledger, "at-local-reset", "2026-10-01T00:00:00Z", 10);
            add_usage(&mut ledger, "after-local-reset", "2026-10-01T00:01:00Z", 30);

            let (daily, current, lifetime) = ledger.planet_usage_totals().unwrap();
            assert_eq!(current, 30);
            assert_eq!(lifetime, 40);
            assert_eq!(daily, BTreeMap::from([("2026-10-01".into(), 30)]));
        }
    }

    #[test]
    fn signed_current_raw_buckets_follow_server_bounds_and_keep_unknown_history_lifetime_only() {
        let mut ledger = Ledger::open(Path::new(":memory:"), UTC).unwrap();
        ledger
            .connection
            .execute(
                "UPDATE setting SET value='2026-09-24T00:00:00Z'
                 WHERE key IN ('planet_activation_at_utc','planet_cycle_started_at_utc')",
                [],
            )
            .unwrap();
        let account_id = "00000000-0000-0000-0000-000000000041";
        ledger.ensure_planet_account(account_id).unwrap();
        let cycle_id = ledger.planet_cycle_id().unwrap();
        ledger
            .connection
            .execute(
                "INSERT INTO setting(key,value) VALUES ('planet_last_reset_at_utc','2026-10-01T00:00:00Z')
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                [],
            )
            .unwrap();
        add_usage(
            &mut ledger,
            "unknown-before-bounds",
            "2026-09-25T12:00:00Z",
            100,
        );
        add_usage(&mut ledger, "known-old-cycle", "2026-09-27T12:00:00Z", 100);
        add_usage(
            &mut ledger,
            "current-bound-start",
            "2026-09-29T00:00:00Z",
            50,
        );
        add_usage(
            &mut ledger,
            "known-current-baseline",
            "2026-09-30T12:00:00Z",
            200,
        );
        add_usage(
            &mut ledger,
            "known-current-effect",
            "2026-10-01T12:00:00Z",
            300,
        );

        let timeline = ShopEffectTimeline {
            account_id: account_id.into(),
            current_cycle_id: cycle_id.clone(),
            effect_revision: 2,
            server_time_utc: "2026-10-02T00:00:00Z".into(),
            reward_timezone: "UTC".into(),
            cycle_bounds: vec![
                ShopCycleBound {
                    cycle_id: "known-previous-cycle".into(),
                    started_at_utc: "2026-09-26T00:00:00Z".into(),
                    ended_at_utc: Some("2026-09-29T00:00:00Z".into()),
                },
                ShopCycleBound {
                    cycle_id: cycle_id.clone(),
                    started_at_utc: "2026-09-29T00:00:00Z".into(),
                    ended_at_utc: None,
                },
            ],
            intervals: vec![
                ShopEffectInterval {
                    cycle_id: "known-previous-cycle".into(),
                    revision: 1,
                    started_at_utc: "2026-09-26T00:00:00Z".into(),
                    ended_at_utc: Some("2026-09-28T00:00:00Z".into()),
                    active_instance_ids: vec![],
                    effects: ActiveEffects {
                        token_earning_bps: 100,
                        ..ActiveEffects::default()
                    },
                },
                ShopEffectInterval {
                    cycle_id: cycle_id.clone(),
                    revision: 2,
                    started_at_utc: "2026-10-01T00:00:00Z".into(),
                    ended_at_utc: None,
                    active_instance_ids: vec![],
                    effects: ActiveEffects {
                        token_earning_bps: 200,
                        ..ActiveEffects::default()
                    },
                },
            ],
        };
        ledger
            .apply_confirmed_shop_effect_timeline(&timeline, account_id, &cycle_id)
            .unwrap();

        let contribution = ledger.shop_device_contribution(false).unwrap();
        let current_segment_tokens: u64 = contribution
            .daily_segments
            .iter()
            .filter(|segment| segment.cycle_id == cycle_id)
            .map(|segment| segment.tokens)
            .sum();

        assert_eq!(contribution.raw.lifetime_tokens, 750);
        assert_eq!(contribution.raw.current_planet_tokens, 550);
        assert_eq!(current_segment_tokens, 550);
        let at_cycle_start = contribution
            .daily_segments
            .iter()
            .find(|segment| segment.cycle_id == cycle_id && segment.date == "2026-09-29")
            .unwrap();
        assert_eq!(
            (at_cycle_start.effect_revision, at_cycle_start.tokens),
            (0, 50),
            "the server cycle start is inclusive for both raw and canonical attribution"
        );
        assert_eq!(
            contribution.raw.daily_tokens,
            BTreeMap::from([
                ("2026-09-29".into(), 50),
                ("2026-09-30".into(), 200),
                ("2026-10-01".into(), 300),
            ]),
        );
        assert!(contribution
            .daily_segments
            .iter()
            .all(|segment| segment.date != "2026-09-25"));
        assert!(contribution
            .activity_days
            .iter()
            .all(|day| day.reward_date != "2026-09-25"));
    }

    #[test]
    fn exact_activation_bound_matches_raw_and_canonical_source_exclusion() {
        let mut ledger = Ledger::open(Path::new(":memory:"), UTC).unwrap();
        ledger
            .connection
            .execute(
                "UPDATE setting SET value='2026-09-24T00:00:00Z'
                 WHERE key IN ('planet_activation_at_utc','planet_cycle_started_at_utc')",
                [],
            )
            .unwrap();
        let account_id = "00000000-0000-0000-0000-000000000043";
        ledger.ensure_planet_account(account_id).unwrap();
        let cycle_id = ledger.planet_cycle_id().unwrap();
        add_usage(&mut ledger, "at-activation", "2026-09-24T00:00:00Z", 10);
        add_usage(&mut ledger, "after-activation", "2026-09-24T00:01:00Z", 20);
        let timeline = ShopEffectTimeline {
            account_id: account_id.into(),
            current_cycle_id: cycle_id.clone(),
            effect_revision: 0,
            server_time_utc: "2026-09-25T00:00:00Z".into(),
            reward_timezone: "UTC".into(),
            cycle_bounds: vec![ShopCycleBound {
                cycle_id: cycle_id.clone(),
                started_at_utc: "2026-09-24T00:00:00Z".into(),
                ended_at_utc: None,
            }],
            intervals: vec![],
        };
        ledger
            .apply_confirmed_shop_effect_timeline(&timeline, account_id, &cycle_id)
            .unwrap();

        let contribution = ledger.shop_device_contribution(false).unwrap();
        assert_eq!(contribution.raw.current_planet_tokens, 20);
        assert_eq!(contribution.raw.lifetime_tokens, 20);
        assert_eq!(
            contribution.raw.daily_tokens,
            BTreeMap::from([("2026-09-24".into(), 20)]),
        );
        assert_eq!(contribution.daily_segments.len(), 1);
        assert_eq!(contribution.daily_segments[0].tokens, 20);
        assert_eq!(contribution.activity_days.len(), 1);
        assert_eq!(contribution.activity_days[0].tokens, 20);
    }

    #[test]
    fn failed_uploads_back_off_and_success_resets_interval() {
        let mut retry = RetryDelay::default();
        assert_eq!(retry.next_after(false).as_secs(), 5);
        assert_eq!(retry.next_after(false).as_secs(), 10);
        assert_eq!(retry.next_after(false).as_secs(), 20);
        assert_eq!(retry.next_after(false).as_secs(), 40);
        assert_eq!(retry.next_after(false).as_secs(), 60);
        assert_eq!(retry.next_after(true).as_secs(), 60);
        assert_eq!(retry.next_after(false).as_secs(), 5);
    }

    #[cfg(test)]
    mod guest_import_v2_worker_order_tests {
        use super::app_state_from_ledger;
        use super::sync_private_effect_contribution;
        use crate::{
            domain::guest_shop_import::{
                GuestImportPhase, GuestImportSourceRelation, GuestImportStatus,
                GuestShopImportV2Request, GuestShopImportV2Result,
            },
            storage::{
                guest_shop_import_v2::guest_import_v2_capture_tests::{
                    imported_result_for, prepared_first_reset_ledger,
                },
                PendingGuestShopImport,
            },
            sync::{
                client::SyncError,
                guest_shop_import::{
                    sync_pending_guest_shop_import, GuestImportSyncOutcome,
                    GuestShopImportTransport,
                },
            },
            AppState,
        };
        use std::{
            future::Future,
            sync::{
                atomic::{AtomicBool, Ordering},
                Arc, Mutex,
            },
        };

        const TARGET_ACCOUNT: &str = "account:00000000-0000-4000-a000-000000000071";
        const OTHER_TARGET_ACCOUNT: &str = "account:00000000-0000-4000-a000-000000000072";

        #[test]
        fn imported_first_reset_task9_worker_reaches_remote_with_fixture_account_id() {
            let user_id = TARGET_ACCOUNT.strip_prefix("account:").unwrap();
            let mut ledger = prepared_first_reset_ledger();
            let PendingGuestShopImport::V2(capture) = ledger
                .capture_guest_shop_import_request(TARGET_ACCOUNT)
                .unwrap()
            else {
                panic!("the prepared first-reset fixture must capture a schema-2 request");
            };
            let request = capture.request;
            ledger.set_selected_auth_account(TARGET_ACCOUNT).unwrap();
            ledger
                .mark_guest_shop_import_attempt_started(
                    TARGET_ACCOUNT,
                    request.snapshot.import_id.parse().unwrap(),
                )
                .unwrap();
            let imported = imported_result_for(&request);
            ledger
                .complete_guest_shop_import(TARGET_ACCOUNT, &imported)
                .unwrap();
            let state = app_state_from_ledger(ledger);
            let cycle_id = state.ledger.lock().unwrap().planet_cycle_id().unwrap();
            let mut prefixed_api = super::mock_private_api(Some(&state.ledger), user_id, &cycle_id);
            prefixed_api.fail_state = true;
            let prefixed_result = tauri::async_runtime::block_on(sync_private_effect_contribution(
                &state,
                TARGET_ACCOUNT,
                "account-token",
                &prefixed_api,
                false,
            ));
            assert_eq!(
                prefixed_result,
                Err("계정 또는 행성 주기가 변경되어 동기화를 중단했습니다".into())
            );
            assert!(prefixed_api.calls.lock().unwrap().is_empty());

            let mut api = super::mock_private_api(Some(&state.ledger), user_id, &cycle_id);
            api.fail_state = true;

            let result = tauri::async_runtime::block_on(sync_private_effect_contribution(
                &state,
                user_id,
                "account-token",
                &api,
                false,
            ));

            assert_eq!(result, Err("행성 동기화 상태를 불러올 수 없습니다".into()));
            assert_eq!(*api.calls.lock().unwrap(), vec!["planet_state"]);
        }

        fn ledger_snapshot_excluding_selected_auth(
            connection: &rusqlite::Connection,
        ) -> Vec<String> {
            use rusqlite::types::Value;

            let table_names = {
                let mut statement = connection
                    .prepare(
                        "SELECT name FROM sqlite_master WHERE type='table' \
                         AND name NOT LIKE 'sqlite_%' ORDER BY name",
                    )
                    .unwrap();
                statement
                    .query_map([], |row| row.get::<_, String>(0))
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap()
            };
            table_names
                .into_iter()
                .map(|table| {
                    let query = if table == "setting" {
                        "SELECT key,value FROM setting WHERE key!='selected_auth_account_id'"
                            .to_owned()
                    } else {
                        format!("SELECT * FROM \"{}\"", table.replace('"', "\"\""))
                    };
                    let mut statement = connection.prepare(&query).unwrap();
                    let mut rows = statement
                        .query_map([], |row| {
                            (0..row.as_ref().column_count())
                                .map(|index| {
                                    row.get::<_, Value>(index).map(|value| format!("{value:?}"))
                                })
                                .collect::<Result<Vec<_>, _>>()
                        })
                        .unwrap()
                        .collect::<Result<Vec<_>, _>>()
                        .unwrap();
                    rows.sort();
                    format!("{table}:{rows:?}")
                })
                .collect()
        }

        struct MockLegacyRemoteCalls {
            order_calls: Arc<Mutex<Vec<&'static str>>>,
        }

        impl MockLegacyRemoteCalls {
            fn record(&self, call: &'static str) {
                self.order_calls.lock().unwrap().push(call);
            }

            fn current_world(&self) {
                self.record("current_world");
            }

            fn get_shop(&self) {
                self.record("get_shop");
            }

            fn growth_journal(&self) {
                self.record("growth_journal");
            }

            fn upload_journal(&self) {
                self.record("journal_upload");
            }
        }

        struct FakeGuestShopImportTransport<'a> {
            state: &'a AppState,
            calls: Mutex<Vec<&'static str>>,
            order_calls: Arc<Mutex<Vec<&'static str>>>,
            result: Option<GuestShopImportV2Result>,
            invalid_response: bool,
            attempt_started_during_await: AtomicBool,
            same_request_during_await: AtomicBool,
            ledger_unlocked_during_await: AtomicBool,
            select_other_auth_account_after_await: Option<String>,
            snapshot_before_response: Mutex<Option<Vec<String>>>,
            append_usage_during_await: Option<(String, String, u64)>,
        }

        impl GuestShopImportTransport for FakeGuestShopImportTransport<'_> {
            fn import_guest_shop<'a>(
                &'a self,
                _access_token: &'a str,
                request: &'a GuestShopImportV2Request,
            ) -> impl Future<Output = Result<GuestShopImportV2Result, SyncError>> + Send + 'a
            {
                let request = request.clone();
                async move {
                    self.calls.lock().unwrap().push("import_guest_shop");
                    self.order_calls.lock().unwrap().push("import_guest_shop");
                    let Ok(ledger) = self.state.ledger.try_lock() else {
                        self.ledger_unlocked_during_await
                            .store(false, Ordering::SeqCst);
                        return Err(SyncError::Transport);
                    };
                    self.ledger_unlocked_during_await
                        .store(true, Ordering::SeqCst);
                    let pending = ledger
                        .pending_guest_shop_import_request(TARGET_ACCOUNT)
                        .map_err(|_| SyncError::InvalidResponse)?;
                    let attempt_started = matches!(
                        pending,
                        Some(PendingGuestShopImport::V2(ref status))
                            if status.phase == GuestImportPhase::AttemptStarted
                    );
                    let same_request = matches!(
                        pending,
                        Some(PendingGuestShopImport::V2(ref status))
                            if status.request == request
                    );
                    self.attempt_started_during_await
                        .store(attempt_started, Ordering::SeqCst);
                    self.same_request_during_await
                        .store(same_request, Ordering::SeqCst);
                    *self.snapshot_before_response.lock().unwrap() =
                        Some(ledger_snapshot_excluding_selected_auth(&ledger.connection));
                    drop(ledger);
                    if !attempt_started || !same_request {
                        return Err(SyncError::InvalidResponse);
                    }
                    if let Some(other_account) = &self.select_other_auth_account_after_await {
                        let mut ledger = self.state.ledger.lock().unwrap();
                        ledger
                            .set_selected_auth_account(other_account)
                            .map_err(|_| SyncError::InvalidResponse)?;
                    }
                    if let Some((event_key, occurred_at_utc, total_tokens)) =
                        &self.append_usage_during_await
                    {
                        let mut ledger = self.state.ledger.lock().unwrap();
                        let import_id = uuid::Uuid::parse_str(&request.snapshot.import_id).unwrap();
                        super::add_usage(&mut ledger, event_key, occurred_at_utc, *total_tokens);
                        assert_eq!(
                            ledger.guest_import_source_relation(import_id).unwrap(),
                            GuestImportSourceRelation::AppendOnly
                        );
                        let transaction = ledger
                            .connection
                            .transaction()
                            .map_err(|_| SyncError::InvalidResponse)?;
                        crate::storage::ledger::Ledger::prepare_growth_journal_in_transaction(
                            &transaction,
                        )
                        .map_err(|_| SyncError::InvalidResponse)?;
                        transaction
                            .commit()
                            .map_err(|_| SyncError::InvalidResponse)?;
                        let Some(PendingGuestShopImport::V2(status)) = ledger
                            .pending_guest_shop_import_request(TARGET_ACCOUNT)
                            .unwrap()
                        else {
                            panic!("the in-flight v2 capture must remain readable");
                        };
                        assert_eq!(
                            status.source_relation,
                            GuestImportSourceRelation::AppendOnly
                        );
                        assert!(!status.correction_hold);
                        assert_eq!(
                            ledger.guest_import_source_relation(import_id).unwrap(),
                            GuestImportSourceRelation::AppendOnly
                        );
                    }
                    if self.invalid_response {
                        return Err(SyncError::InvalidResponse);
                    }
                    Ok(self
                        .result
                        .clone()
                        .unwrap_or_else(|| imported_result_for(&request)))
                }
            }
        }

        struct LostResponseGuestShopImportTransport {
            requests: Mutex<Vec<GuestShopImportV2Request>>,
            applied_request: Mutex<Option<GuestShopImportV2Request>>,
            order_calls: Arc<Mutex<Vec<&'static str>>>,
        }

        impl GuestShopImportTransport for LostResponseGuestShopImportTransport {
            fn import_guest_shop<'a>(
                &'a self,
                _access_token: &'a str,
                request: &'a GuestShopImportV2Request,
            ) -> impl Future<Output = Result<GuestShopImportV2Result, SyncError>> + Send + 'a
            {
                let request = request.clone();
                async move {
                    self.order_calls.lock().unwrap().push("import_guest_shop");
                    let mut requests = self.requests.lock().unwrap();
                    requests.push(request.clone());
                    if requests.len() == 1 {
                        *self.applied_request.lock().unwrap() = Some(request);
                        return Err(SyncError::Transport);
                    }
                    drop(requests);
                    if self.applied_request.lock().unwrap().as_ref() != Some(&request) {
                        return Err(SyncError::InvalidResponse);
                    }
                    Ok(imported_result_for(&request))
                }
            }
        }

        #[test]
        fn guest_import_v2_worker_pending_dispatches_import_with_attempt_before_await() {
            let state = app_state_from_ledger(prepared_first_reset_ledger());
            {
                let mut ledger = state.ledger.lock().unwrap();
                let PendingGuestShopImport::V2(capture) = ledger
                    .capture_guest_shop_import_request(TARGET_ACCOUNT)
                    .unwrap()
                else {
                    panic!("schema-2 import capture expected");
                };
                assert_eq!(capture.request.schema_version, 2);
                ledger.set_selected_auth_account(TARGET_ACCOUNT).unwrap();
            }
            let transport = FakeGuestShopImportTransport {
                state: &state,
                calls: Mutex::new(Vec::new()),
                order_calls: Arc::new(Mutex::new(Vec::new())),
                result: None,
                invalid_response: false,
                attempt_started_during_await: AtomicBool::new(false),
                same_request_during_await: AtomicBool::new(false),
                ledger_unlocked_during_await: AtomicBool::new(false),
                select_other_auth_account_after_await: None,
                snapshot_before_response: Mutex::new(None),
                append_usage_during_await: None,
            };

            let result = tauri::async_runtime::block_on(sync_pending_guest_shop_import(
                &state,
                &transport,
                "test-access-token",
                TARGET_ACCOUNT,
            ));

            assert_eq!(result, Ok(GuestImportSyncOutcome::Imported));
            assert_eq!(*transport.calls.lock().unwrap(), ["import_guest_shop"]);
            assert!(transport
                .attempt_started_during_await
                .load(Ordering::SeqCst));
            assert!(transport.same_request_during_await.load(Ordering::SeqCst));
            assert!(transport
                .ledger_unlocked_during_await
                .load(Ordering::SeqCst));
        }

        #[test]
        fn authenticated_sync_keeps_new_cycle_append_pending_after_import_await() {
            const APPENDED_EVENT_KEY: &str = "v2-worker-append-during-await";
            const APPENDED_TOKENS: u64 = 1_234;

            let mut ledger = prepared_first_reset_ledger();
            let PendingGuestShopImport::V2(capture) = ledger
                .capture_guest_shop_import_request(TARGET_ACCOUNT)
                .unwrap()
            else {
                panic!("schema-2 import capture expected");
            };
            let request = capture.request;
            ledger.set_selected_auth_account(TARGET_ACCOUNT).unwrap();
            let state = app_state_from_ledger(ledger);
            let user_id = TARGET_ACCOUNT.strip_prefix("account:").unwrap();
            let imported_result = imported_result_for(&request);
            let ack = imported_result.ack.as_ref().unwrap().clone();
            let current_cycle_id = request
                .snapshot
                .provenance
                .reset_receipt
                .result
                .new_cycle_id
                .clone();
            let reset_at = chrono::DateTime::parse_from_rfc3339(
                &request
                    .snapshot
                    .provenance
                    .reset_receipt
                    .result
                    .reset_at_utc,
            )
            .unwrap()
            .with_timezone(&chrono::Utc);
            let appended_at = chrono::Utc::now();
            assert!(appended_at >= reset_at);
            let appended_at_rfc3339 = appended_at.to_rfc3339();
            let appended_bucket_date = appended_at.format("%Y-%m-%d").to_string();
            let order_calls = Arc::new(Mutex::new(Vec::new()));
            let transport = FakeGuestShopImportTransport {
                state: &state,
                calls: Mutex::new(Vec::new()),
                order_calls: order_calls.clone(),
                result: Some(imported_result),
                invalid_response: false,
                attempt_started_during_await: AtomicBool::new(false),
                same_request_during_await: AtomicBool::new(false),
                ledger_unlocked_during_await: AtomicBool::new(false),
                select_other_auth_account_after_await: None,
                snapshot_before_response: Mutex::new(None),
                append_usage_during_await: Some((
                    APPENDED_EVENT_KEY.into(),
                    appended_at_rfc3339,
                    APPENDED_TOKENS,
                )),
            };
            let legacy_order = order_calls.clone();
            let legacy_state = &state;

            let result = tauri::async_runtime::block_on(
                super::super::sync_authenticated_legacy_after_guest_import(
                    &state,
                    user_id,
                    "test-access-token",
                    &transport,
                    || async move {
                        legacy_order.lock().unwrap().push("legacy_sync");
                        let ledger = legacy_state.ledger.lock().unwrap();
                        assert_eq!(ledger.cosmetic_account_id().unwrap(), TARGET_ACCOUNT);
                        let Some(PendingGuestShopImport::V2(status)) = ledger
                            .pending_guest_shop_import_request(TARGET_ACCOUNT)
                            .unwrap()
                        else {
                            panic!("legacy continuation must observe the completed import");
                        };
                        assert_eq!(status.phase, GuestImportPhase::Imported);

                        let contribution = ledger.shop_device_contribution(false).unwrap();
                        assert_eq!(contribution.raw.current_cycle_id, current_cycle_id);
                        assert_eq!(contribution.raw.current_planet_tokens, APPENDED_TOKENS);
                        assert!(contribution.canonical_version > ack.canonical_version);

                        for acknowledged in &ack.journal_entries {
                            let acknowledged_revision: i64 = ledger
                                .connection
                                .query_row(
                                    "SELECT acknowledged_revision FROM growth_journal_entry
                                     WHERE account_id=?1 AND device_id=?2 AND cycle_id=?3
                                       AND bucket_date=?4 AND agent=?5 AND generation=?6",
                                    rusqlite::params![
                                        TARGET_ACCOUNT,
                                        acknowledged.logical_key.device_id,
                                        acknowledged.logical_key.cycle_id,
                                        acknowledged.logical_key.bucket_date,
                                        crate::storage::ledger::agent_name(
                                            acknowledged.logical_key.agent
                                        ),
                                        acknowledged.logical_key.generation,
                                    ],
                                    |row| row.get(0),
                                )
                                .unwrap();
                            assert_eq!(
                                acknowledged_revision, acknowledged.revision as i64,
                                "only the captured journal revision should be acknowledged"
                            );
                        }

                        let (revision, acknowledged_revision): (i64, i64) = ledger
                            .connection
                            .query_row(
                                "SELECT revision,acknowledged_revision FROM growth_journal_entry
                                 WHERE account_id=?1 AND device_id=?2 AND cycle_id=?3
                                   AND bucket_date=?4 AND agent='codex'",
                                rusqlite::params![
                                    TARGET_ACCOUNT,
                                    request.snapshot.provenance.device_id,
                                    current_cycle_id,
                                    appended_bucket_date,
                                ],
                                |row| Ok((row.get(0)?, row.get(1)?)),
                            )
                            .unwrap();
                        assert!(revision > acknowledged_revision);
                        assert!(!ack.journal_entries.iter().any(|entry| {
                            entry.logical_key.cycle_id == current_cycle_id
                                && entry.logical_key.bucket_date == appended_bucket_date
                        }));
                        assert_eq!(
                            ledger
                                .connection
                                .query_row::<i64, _, _>(
                                    "SELECT count(*) FROM usage_record WHERE event_key=?1",
                                    [APPENDED_EVENT_KEY],
                                    |row| row.get(0),
                                )
                                .unwrap(),
                            1,
                            "the appended raw event must remain durable"
                        );
                        Ok(())
                    },
                ),
            );

            assert_eq!(result, Ok(GuestImportSyncOutcome::Imported));
            assert_eq!(
                *order_calls.lock().unwrap(),
                ["import_guest_shop", "legacy_sync"]
            );
        }

        #[test]
        fn authenticated_sync_late_old_usage_only_increases_lifetime_after_import_await() {
            const LATE_EVENT_KEY: &str = "v2-worker-late-old-during-await";
            const LATE_TOKENS: u64 = 2_345;

            let mut ledger = prepared_first_reset_ledger();
            let PendingGuestShopImport::V2(capture) = ledger
                .capture_guest_shop_import_request(TARGET_ACCOUNT)
                .unwrap()
            else {
                panic!("schema-2 import capture expected");
            };
            let request = capture.request;
            ledger.set_selected_auth_account(TARGET_ACCOUNT).unwrap();
            let state = app_state_from_ledger(ledger);
            let user_id = TARGET_ACCOUNT.strip_prefix("account:").unwrap();
            let imported_result = imported_result_for(&request);
            let ack = imported_result.ack.as_ref().unwrap().clone();
            let imported_planet_wallet_credits = imported_result
                .planet_state
                .as_ref()
                .unwrap()
                .wallet_credits
                .clone();
            let old_cycle_id = request
                .snapshot
                .provenance
                .reset_receipt
                .request
                .cycle_id
                .clone();
            let reset_result = &request.snapshot.provenance.reset_receipt.result;
            let expected_reset_credit = imported_result
                .journal_confirmation
                .as_ref()
                .unwrap()
                .cycles
                .iter()
                .find(|cycle| cycle.cycle_id == old_cycle_id)
                .and_then(|cycle| cycle.wallet_credit)
                .expect("the imported old cycle retains its settled reset credit");
            assert_eq!(expected_reset_credit, 1_000_000);
            assert_eq!(expected_reset_credit, reset_result.credited_tokens);
            let imported_planet_state = imported_result.planet_state.as_ref().unwrap();
            assert_eq!(
                imported_planet_state.wallet_balance,
                reset_result.credited_tokens
            );
            assert_eq!(imported_planet_state.wallet_credits.len(), 1);
            assert_eq!(
                imported_planet_state.wallet_credits[0].previous_cycle_id,
                reset_result.previous_cycle_id
            );
            assert_eq!(
                imported_planet_state.wallet_credits[0].amount,
                reset_result.credited_tokens
            );
            assert_eq!(
                imported_planet_state.wallet_credits[0].created_at_utc,
                reset_result.reset_at_utc
            );
            let imported_shop_wallet_balance = imported_result
                .shop_state
                .as_ref()
                .unwrap()
                .available_balance;
            assert_eq!(imported_shop_wallet_balance, reset_result.credited_tokens);
            let captured_lifetime_tokens = imported_result
                .canonical_contribution
                .as_ref()
                .unwrap()
                .raw
                .lifetime_tokens;
            let current_cycle_id = request
                .snapshot
                .provenance
                .reset_receipt
                .result
                .new_cycle_id
                .clone();
            let reset_at = chrono::DateTime::parse_from_rfc3339(
                &request
                    .snapshot
                    .provenance
                    .reset_receipt
                    .result
                    .reset_at_utc,
            )
            .unwrap()
            .with_timezone(&chrono::Utc);
            let captured_at =
                chrono::DateTime::parse_from_rfc3339(&request.snapshot.captured_at_utc)
                    .unwrap()
                    .with_timezone(&chrono::Utc);
            assert!(captured_at > reset_at);
            let late_at = reset_at - chrono::Duration::milliseconds(1);
            let late_at_rfc3339 = late_at.to_rfc3339();
            let late_bucket_date = late_at.format("%Y-%m-%d").to_string();
            let order_calls = Arc::new(Mutex::new(Vec::new()));
            let transport = FakeGuestShopImportTransport {
                state: &state,
                calls: Mutex::new(Vec::new()),
                order_calls: order_calls.clone(),
                result: Some(imported_result),
                invalid_response: false,
                attempt_started_during_await: AtomicBool::new(false),
                same_request_during_await: AtomicBool::new(false),
                ledger_unlocked_during_await: AtomicBool::new(false),
                select_other_auth_account_after_await: None,
                snapshot_before_response: Mutex::new(None),
                append_usage_during_await: Some((
                    LATE_EVENT_KEY.into(),
                    late_at_rfc3339,
                    LATE_TOKENS,
                )),
            };
            let legacy_order = order_calls.clone();
            let legacy_state = &state;

            let result = tauri::async_runtime::block_on(
                super::super::sync_authenticated_legacy_after_guest_import(
                    &state,
                    user_id,
                    "test-access-token",
                    &transport,
                    || async move {
                        legacy_order.lock().unwrap().push("legacy_sync");
                        let ledger = legacy_state.ledger.lock().unwrap();
                        assert_eq!(ledger.cosmetic_account_id().unwrap(), TARGET_ACCOUNT);
                        let Some(PendingGuestShopImport::V2(status)) = ledger
                            .pending_guest_shop_import_request(TARGET_ACCOUNT)
                            .unwrap()
                        else {
                            panic!("legacy continuation must observe the completed import");
                        };
                        assert_eq!(status.phase, GuestImportPhase::Imported);

                        let contribution = ledger.shop_device_contribution(false).unwrap();
                        assert_eq!(contribution.raw.current_cycle_id, current_cycle_id);
                        assert_eq!(contribution.raw.current_planet_tokens, 0);
                        assert_eq!(
                            contribution.raw.lifetime_tokens,
                            captured_lifetime_tokens + LATE_TOKENS
                        );
                        assert!(contribution.canonical_version > ack.canonical_version);
                        assert_eq!(
                            ledger.planet_wallet_credits().unwrap(),
                            imported_planet_wallet_credits
                        );
                        let retained_reset_credit: Option<i64> = ledger
                            .connection
                            .query_row(
                                "SELECT wallet_credit FROM growth_journal_cycle
                                 WHERE account_id=?1 AND cycle_id=?2",
                                rusqlite::params![TARGET_ACCOUNT, old_cycle_id],
                                |row| row.get(0),
                            )
                            .unwrap();
                        assert_eq!(
                            retained_reset_credit,
                            Some(expected_reset_credit as i64),
                            "the captured reset credit must remain and must not be credited again"
                        );
                        let (credit_count, credit_total): (i64, i64) = ledger
                            .connection
                            .query_row(
                                "SELECT count(wallet_credit),coalesce(sum(wallet_credit),0)
                                 FROM growth_journal_cycle WHERE account_id=?1",
                                [TARGET_ACCOUNT],
                                |row| Ok((row.get(0)?, row.get(1)?)),
                            )
                            .unwrap();
                        assert_eq!(credit_count, 1);
                        assert_eq!(credit_total, expected_reset_credit as i64);
                        let confirmed_shop_state_json: String = ledger
                            .connection
                            .query_row(
                                "SELECT state_json FROM shop_remote_state WHERE account_id=?1",
                                [TARGET_ACCOUNT],
                                |row| row.get(0),
                            )
                            .unwrap();
                        let confirmed_shop_state: crate::domain::cosmetic_shop::ShopState =
                            serde_json::from_str(&confirmed_shop_state_json).unwrap();
                        assert_eq!(
                            confirmed_shop_state.available_balance,
                            imported_shop_wallet_balance
                        );

                        let captured_old_entry = ack
                            .journal_entries
                            .iter()
                            .find(|entry| {
                                entry.logical_key.cycle_id == old_cycle_id
                                    && entry.logical_key.bucket_date == late_bucket_date
                            })
                            .expect("captured old-cycle journal entry expected");
                        let (revision, acknowledged_revision): (i64, i64) = ledger
                            .connection
                            .query_row(
                                "SELECT revision,acknowledged_revision FROM growth_journal_entry
                                 WHERE account_id=?1 AND device_id=?2 AND cycle_id=?3
                                   AND bucket_date=?4 AND agent=?5 AND generation=?6",
                                rusqlite::params![
                                    TARGET_ACCOUNT,
                                    captured_old_entry.logical_key.device_id,
                                    captured_old_entry.logical_key.cycle_id,
                                    captured_old_entry.logical_key.bucket_date,
                                    crate::storage::ledger::agent_name(
                                        captured_old_entry.logical_key.agent
                                    ),
                                    captured_old_entry.logical_key.generation,
                                ],
                                |row| Ok((row.get(0)?, row.get(1)?)),
                            )
                            .unwrap();
                        assert!(revision > captured_old_entry.revision as i64);
                        assert_eq!(acknowledged_revision, captured_old_entry.revision as i64);
                        assert_eq!(
                            ledger
                                .connection
                                .query_row::<i64, _, _>(
                                    "SELECT count(*) FROM usage_record WHERE event_key=?1",
                                    [LATE_EVENT_KEY],
                                    |row| row.get(0),
                                )
                                .unwrap(),
                            1,
                            "late old-cycle raw usage must remain durable"
                        );
                        Ok(())
                    },
                ),
            );

            assert_eq!(result, Ok(GuestImportSyncOutcome::Imported));
            assert_eq!(
                *order_calls.lock().unwrap(),
                ["import_guest_shop", "legacy_sync"]
            );
        }

        #[test]
        fn sync_guest_import_pipeline_dispatches_before_private_contribution_upload() {
            let state = app_state_from_ledger(prepared_first_reset_ledger());
            let request = {
                let mut ledger = state.ledger.lock().unwrap();
                let PendingGuestShopImport::V2(capture) = ledger
                    .capture_guest_shop_import_request(TARGET_ACCOUNT)
                    .unwrap()
                else {
                    panic!("schema-2 import capture expected");
                };
                ledger.set_selected_auth_account(TARGET_ACCOUNT).unwrap();
                capture.request
            };
            let new_cycle_id = request
                .snapshot
                .provenance
                .reset_receipt
                .result
                .new_cycle_id
                .clone();
            let user_id = TARGET_ACCOUNT.strip_prefix("account:").unwrap();
            let import_result = imported_result_for(&request);
            let imported_timeline = import_result.effect_timeline.clone().unwrap();
            let order_calls = Arc::new(Mutex::new(Vec::new()));
            let transport = FakeGuestShopImportTransport {
                state: &state,
                calls: Mutex::new(Vec::new()),
                order_calls: order_calls.clone(),
                result: Some(import_result),
                invalid_response: false,
                attempt_started_during_await: AtomicBool::new(false),
                same_request_during_await: AtomicBool::new(false),
                ledger_unlocked_during_await: AtomicBool::new(false),
                select_other_auth_account_after_await: None,
                snapshot_before_response: Mutex::new(None),
                append_usage_during_await: None,
            };
            let mut private_api = super::mock_private_api(None, user_id, &new_cycle_id);
            private_api.timeline = imported_timeline;
            private_api.order_calls = Some(order_calls.clone());

            let result =
                tauri::async_runtime::block_on(super::super::sync_guest_import_before_legacy_sync(
                    &state,
                    &transport,
                    "test-access-token",
                    TARGET_ACCOUNT,
                    || async {
                        sync_private_effect_contribution(
                            &state,
                            user_id,
                            "test-access-token",
                            &private_api,
                            false,
                        )
                        .await
                        .map(|_| ())
                    },
                ));

            assert_eq!(
                *order_calls.lock().unwrap(),
                ["import_guest_shop", "planet_state", "timeline", "upload"]
            );
            assert_eq!(result, Ok(GuestImportSyncOutcome::Imported));
        }

        #[test]
        fn authenticated_sync_dispatches_import_before_world_private_shop_and_journal_calls() {
            let mut ledger = prepared_first_reset_ledger();
            let PendingGuestShopImport::V2(capture) = ledger
                .capture_guest_shop_import_request(TARGET_ACCOUNT)
                .unwrap()
            else {
                panic!("schema-2 import capture expected");
            };
            let request = capture.request;
            let state = app_state_from_ledger(ledger);
            let user_id = TARGET_ACCOUNT.strip_prefix("account:").unwrap();
            assert!(state.select_planet_account(user_id).is_err());
            let import_result = imported_result_for(&request);
            let imported_timeline = import_result.effect_timeline.clone().unwrap();
            let new_cycle_id = request
                .snapshot
                .provenance
                .reset_receipt
                .result
                .new_cycle_id
                .clone();
            let order_calls = Arc::new(Mutex::new(Vec::new()));
            let transport = FakeGuestShopImportTransport {
                state: &state,
                calls: Mutex::new(Vec::new()),
                order_calls: order_calls.clone(),
                result: Some(import_result),
                invalid_response: false,
                attempt_started_during_await: AtomicBool::new(false),
                same_request_during_await: AtomicBool::new(false),
                ledger_unlocked_during_await: AtomicBool::new(false),
                select_other_auth_account_after_await: None,
                snapshot_before_response: Mutex::new(None),
                append_usage_during_await: None,
            };
            let mut private_api = super::mock_private_api(None, user_id, &new_cycle_id);
            private_api.timeline = imported_timeline;
            private_api.order_calls = Some(order_calls.clone());
            let legacy_calls = MockLegacyRemoteCalls {
                order_calls: order_calls.clone(),
            };

            let result = tauri::async_runtime::block_on(
                super::super::sync_authenticated_legacy_after_guest_import(
                    &state,
                    user_id,
                    "test-access-token",
                    &transport,
                    || async {
                        legacy_calls.current_world();
                        sync_private_effect_contribution(
                            &state,
                            user_id,
                            "test-access-token",
                            &private_api,
                            false,
                        )
                        .await
                        .map(|_| ())?;
                        legacy_calls.get_shop();
                        legacy_calls.growth_journal();
                        legacy_calls.upload_journal();
                        Ok(())
                    },
                ),
            );

            assert_eq!(
                *order_calls.lock().unwrap(),
                [
                    "import_guest_shop",
                    "current_world",
                    "planet_state",
                    "timeline",
                    "upload",
                    "get_shop",
                    "growth_journal",
                    "journal_upload",
                ]
            );
            assert_eq!(result, Ok(GuestImportSyncOutcome::Imported));
            assert_eq!(
                state.ledger.lock().unwrap().cosmetic_account_id().unwrap(),
                TARGET_ACCOUNT
            );
        }

        #[test]
        fn authenticated_sync_holds_changed_guest_capture_without_legacy_continuation() {
            let mut ledger = prepared_first_reset_ledger();
            ledger
                .capture_guest_shop_import_request(TARGET_ACCOUNT)
                .unwrap();
            let state = app_state_from_ledger(ledger);
            let user_id = TARGET_ACCOUNT.strip_prefix("account:").unwrap();
            assert!(state.select_planet_account(user_id).is_err());
            state
                .ledger
                .lock()
                .unwrap()
                .connection
                .execute(
                    "UPDATE usage_record SET total_tokens=total_tokens+1 WHERE event_key=?1",
                    ["v2-capture-raw-1m"],
                )
                .unwrap();

            let order_calls = Arc::new(Mutex::new(Vec::new()));
            let transport = FakeGuestShopImportTransport {
                state: &state,
                calls: Mutex::new(Vec::new()),
                order_calls: order_calls.clone(),
                result: None,
                invalid_response: false,
                attempt_started_during_await: AtomicBool::new(false),
                same_request_during_await: AtomicBool::new(false),
                ledger_unlocked_during_await: AtomicBool::new(false),
                select_other_auth_account_after_await: None,
                snapshot_before_response: Mutex::new(None),
                append_usage_during_await: None,
            };
            let legacy_order = order_calls.clone();

            let result = tauri::async_runtime::block_on(
                super::super::sync_authenticated_legacy_after_guest_import(
                    &state,
                    user_id,
                    "test-access-token",
                    &transport,
                    || async move {
                        legacy_order.lock().unwrap().push("legacy_sync");
                        Ok(())
                    },
                ),
            );

            assert_eq!(result, Ok(GuestImportSyncOutcome::Held));
            assert!(transport.calls.lock().unwrap().is_empty());
            assert!(order_calls.lock().unwrap().is_empty());
        }

        #[test]
        fn authenticated_sync_keeps_imported_correction_hold_before_legacy_continuation() {
            let mut ledger = prepared_first_reset_ledger();
            let PendingGuestShopImport::V2(capture) = ledger
                .capture_guest_shop_import_request(TARGET_ACCOUNT)
                .unwrap()
            else {
                panic!("schema-2 import capture expected");
            };
            let request = capture.request;
            ledger.set_selected_auth_account(TARGET_ACCOUNT).unwrap();
            ledger
                .mark_guest_shop_import_attempt_started(
                    TARGET_ACCOUNT,
                    request.snapshot.import_id.parse().unwrap(),
                )
                .unwrap();
            ledger
                .connection
                .execute(
                    "UPDATE usage_record SET total_tokens=total_tokens+1 WHERE event_key=?1",
                    ["v2-capture-raw-1m"],
                )
                .unwrap();
            let imported_result = imported_result_for(&request);
            assert_eq!(
                ledger
                    .complete_guest_shop_import(TARGET_ACCOUNT, &imported_result)
                    .unwrap(),
                crate::storage::guest_shop_import_v2::GuestImportCompletion::
                    ImportedWithCorrectionHold
            );

            let state = app_state_from_ledger(ledger);
            let user_id = TARGET_ACCOUNT.strip_prefix("account:").unwrap();
            let order_calls = Arc::new(Mutex::new(Vec::new()));
            let current_cycle_id = imported_result
                .planet_state
                .as_ref()
                .unwrap()
                .current_cycle_id
                .clone();
            let mut private_api =
                super::mock_private_api(Some(&state.ledger), user_id, &current_cycle_id);
            private_api.remote_state = imported_result.planet_state.clone();
            private_api.order_calls = Some(order_calls.clone());
            let transport = FakeGuestShopImportTransport {
                state: &state,
                calls: Mutex::new(Vec::new()),
                order_calls: order_calls.clone(),
                result: None,
                invalid_response: false,
                attempt_started_during_await: AtomicBool::new(false),
                same_request_during_await: AtomicBool::new(false),
                ledger_unlocked_during_await: AtomicBool::new(false),
                select_other_auth_account_after_await: None,
                snapshot_before_response: Mutex::new(None),
                append_usage_during_await: None,
            };
            let imported_status_before = state
                .ledger
                .lock()
                .unwrap()
                .pending_guest_shop_import_request(TARGET_ACCOUNT)
                .unwrap();
            let legacy_order = order_calls.clone();

            let result = tauri::async_runtime::block_on(
                super::super::sync_authenticated_legacy_after_guest_import_with_state_read(
                    &state,
                    user_id,
                    "test-access-token",
                    &transport,
                    || {
                        super::PrivateEffectSyncApi::my_planet_state(
                            &private_api,
                            "test-access-token",
                        )
                    },
                    || async move {
                        legacy_order.lock().unwrap().push("legacy_sync");
                        Ok(())
                    },
                ),
            );

            assert_eq!(
                result,
                Ok(GuestImportSyncOutcome::ImportedWithCorrectionHold)
            );
            assert!(transport.calls.lock().unwrap().is_empty());
            assert_eq!(*private_api.calls.lock().unwrap(), ["planet_state"]);
            assert_eq!(*order_calls.lock().unwrap(), ["planet_state"]);
            assert_eq!(
                state
                    .ledger
                    .lock()
                    .unwrap()
                    .pending_guest_shop_import_request(TARGET_ACCOUNT)
                    .unwrap(),
                imported_status_before,
                "the confirmed-state read must preserve the imported correction marker and receipt"
            );
        }

        #[test]
        fn authenticated_sync_aborts_confirmed_state_read_if_account_changes_during_await() {
            let mut ledger = prepared_first_reset_ledger();
            let PendingGuestShopImport::V2(capture) = ledger
                .capture_guest_shop_import_request(TARGET_ACCOUNT)
                .unwrap()
            else {
                panic!("schema-2 import capture expected");
            };
            let request = capture.request;
            ledger.set_selected_auth_account(TARGET_ACCOUNT).unwrap();
            ledger
                .mark_guest_shop_import_attempt_started(
                    TARGET_ACCOUNT,
                    request.snapshot.import_id.parse().unwrap(),
                )
                .unwrap();
            ledger
                .connection
                .execute(
                    "UPDATE usage_record SET total_tokens=total_tokens+1 WHERE event_key=?1",
                    ["v2-capture-raw-1m"],
                )
                .unwrap();
            let imported_result = imported_result_for(&request);
            assert_eq!(
                ledger
                    .complete_guest_shop_import(TARGET_ACCOUNT, &imported_result)
                    .unwrap(),
                crate::storage::guest_shop_import_v2::GuestImportCompletion::
                    ImportedWithCorrectionHold
            );

            let state = app_state_from_ledger(ledger);
            let user_id = TARGET_ACCOUNT.strip_prefix("account:").unwrap();
            let order_calls = Arc::new(Mutex::new(Vec::new()));
            let transport = FakeGuestShopImportTransport {
                state: &state,
                calls: Mutex::new(Vec::new()),
                order_calls: order_calls.clone(),
                result: None,
                invalid_response: false,
                attempt_started_during_await: AtomicBool::new(false),
                same_request_during_await: AtomicBool::new(false),
                ledger_unlocked_during_await: AtomicBool::new(false),
                select_other_auth_account_after_await: None,
                snapshot_before_response: Mutex::new(None),
                append_usage_during_await: None,
            };
            let status_before = state
                .ledger
                .lock()
                .unwrap()
                .pending_guest_shop_import_request(TARGET_ACCOUNT)
                .unwrap();
            let snapshot_before = {
                let ledger = state.ledger.lock().unwrap();
                ledger_snapshot_excluding_selected_auth(&ledger.connection)
            };
            let read_calls = Arc::new(Mutex::new(Vec::new()));
            let read_call_record = read_calls.clone();
            let state_ref = &state;
            let remote_state = imported_result.planet_state.clone().unwrap();
            let legacy_order = order_calls.clone();

            let result = tauri::async_runtime::block_on(
                super::super::sync_authenticated_legacy_after_guest_import_with_state_read(
                    &state,
                    user_id,
                    "test-access-token",
                    &transport,
                    || async move {
                        read_call_record.lock().unwrap().push("planet_state");
                        state_ref
                            .ledger
                            .lock()
                            .unwrap()
                            .set_selected_auth_account(OTHER_TARGET_ACCOUNT)
                            .unwrap();
                        Ok(Some(remote_state))
                    },
                    || async move {
                        legacy_order.lock().unwrap().push("legacy_sync");
                        Ok(())
                    },
                ),
            );

            assert!(result.is_err(), "the post-read account guard must abort");
            assert!(transport.calls.lock().unwrap().is_empty());
            assert_eq!(*read_calls.lock().unwrap(), ["planet_state"]);
            assert!(order_calls.lock().unwrap().is_empty());
            let ledger = state.ledger.lock().unwrap();
            assert_eq!(
                snapshot_before,
                ledger_snapshot_excluding_selected_auth(&ledger.connection),
                "a late response from the old account must not mutate local state"
            );
            assert_eq!(
                ledger
                    .pending_guest_shop_import_request(TARGET_ACCOUNT)
                    .unwrap(),
                status_before,
                "the imported correction hold and receipt remain recoverable"
            );
            assert_eq!(
                ledger
                    .connection
                    .query_row::<String, _, _>(
                        "SELECT value FROM setting WHERE key='selected_auth_account_id'",
                        [],
                        |row| row.get(0),
                    )
                    .unwrap(),
                OTHER_TARGET_ACCOUNT
            );
        }

        #[test]
        fn authenticated_sync_keeps_typed_server_hold_before_legacy_continuation() {
            let mut ledger = prepared_first_reset_ledger();
            let PendingGuestShopImport::V2(capture) = ledger
                .capture_guest_shop_import_request(TARGET_ACCOUNT)
                .unwrap()
            else {
                panic!("schema-2 import capture expected");
            };
            let request = capture.request;
            ledger.set_selected_auth_account(TARGET_ACCOUNT).unwrap();
            ledger
                .mark_guest_shop_import_attempt_started(
                    TARGET_ACCOUNT,
                    request.snapshot.import_id.parse().unwrap(),
                )
                .unwrap();
            let state = app_state_from_ledger(ledger);
            let user_id = TARGET_ACCOUNT.strip_prefix("account:").unwrap();
            let held_result = GuestShopImportV2Result {
                schema_version: 2,
                import_id: request.snapshot.import_id.clone(),
                account_id: request.snapshot.target_account_id.clone(),
                source_fingerprint: request.snapshot.source_fingerprint.clone(),
                status: GuestImportStatus::SourceUnverifiable,
                shop_state: None,
                planet_state: None,
                effect_timeline: None,
                canonical_contribution: None,
                journal_confirmation: None,
                ack: None,
            };
            let order_calls = Arc::new(Mutex::new(Vec::new()));
            let transport = FakeGuestShopImportTransport {
                state: &state,
                calls: Mutex::new(Vec::new()),
                order_calls: order_calls.clone(),
                result: Some(held_result),
                invalid_response: false,
                attempt_started_during_await: AtomicBool::new(false),
                same_request_during_await: AtomicBool::new(false),
                ledger_unlocked_during_await: AtomicBool::new(false),
                select_other_auth_account_after_await: None,
                snapshot_before_response: Mutex::new(None),
                append_usage_during_await: None,
            };
            let legacy_order = order_calls.clone();

            let result = tauri::async_runtime::block_on(
                super::super::sync_authenticated_legacy_after_guest_import(
                    &state,
                    user_id,
                    "test-access-token",
                    &transport,
                    || async move {
                        legacy_order.lock().unwrap().push("legacy_sync");
                        Ok(())
                    },
                ),
            );

            assert_eq!(result, Ok(GuestImportSyncOutcome::Held));
            assert_eq!(*transport.calls.lock().unwrap(), ["import_guest_shop"]);
            assert!(order_calls
                .lock()
                .unwrap()
                .iter()
                .all(|call| *call == "import_guest_shop"));
            assert!(transport
                .attempt_started_during_await
                .load(Ordering::SeqCst));
            assert!(transport.same_request_during_await.load(Ordering::SeqCst));
        }

        #[test]
        fn authenticated_sync_without_pending_import_runs_legacy_continuation() {
            let state = app_state_from_ledger(
                crate::storage::ledger::Ledger::open(
                    std::path::Path::new(":memory:"),
                    chrono_tz::UTC,
                )
                .unwrap(),
            );
            let user_id = TARGET_ACCOUNT.strip_prefix("account:").unwrap();
            let order_calls = Arc::new(Mutex::new(Vec::new()));
            let transport = FakeGuestShopImportTransport {
                state: &state,
                calls: Mutex::new(Vec::new()),
                order_calls: order_calls.clone(),
                result: None,
                invalid_response: false,
                attempt_started_during_await: AtomicBool::new(false),
                same_request_during_await: AtomicBool::new(false),
                ledger_unlocked_during_await: AtomicBool::new(false),
                select_other_auth_account_after_await: None,
                snapshot_before_response: Mutex::new(None),
                append_usage_during_await: None,
            };
            let legacy_order = order_calls.clone();

            let result = tauri::async_runtime::block_on(
                super::super::sync_authenticated_legacy_after_guest_import(
                    &state,
                    user_id,
                    "test-access-token",
                    &transport,
                    || async move {
                        legacy_order.lock().unwrap().push("legacy_sync");
                        Ok(())
                    },
                ),
            );

            assert_eq!(result, Ok(GuestImportSyncOutcome::NoPending));
            assert!(transport.calls.lock().unwrap().is_empty());
            assert_eq!(*order_calls.lock().unwrap(), ["legacy_sync"]);
        }

        #[test]
        fn authenticated_import_completes_during_local_sharing_pause_without_legacy_upload() {
            let mut ledger = prepared_first_reset_ledger();
            ledger.set_sharing_paused(true).unwrap();
            let PendingGuestShopImport::V2(capture) = ledger
                .capture_guest_shop_import_request(TARGET_ACCOUNT)
                .unwrap()
            else {
                panic!("schema-2 import capture expected");
            };
            let request = capture.request;
            ledger.set_selected_auth_account(TARGET_ACCOUNT).unwrap();
            let state = app_state_from_ledger(ledger);
            let user_id = TARGET_ACCOUNT.strip_prefix("account:").unwrap();
            let cycle_id = request
                .snapshot
                .canonical_payload
                .raw
                .current_cycle_id
                .clone();
            let order_calls = Arc::new(Mutex::new(Vec::new()));
            let imported_result = imported_result_for(&request);
            let mut private_api = super::mock_private_api(Some(&state.ledger), user_id, &cycle_id);
            private_api.remote_state = imported_result.planet_state.clone();
            private_api.timeline = imported_result.effect_timeline.clone().unwrap();
            private_api.canonical_state = imported_result.planet_state.clone().unwrap();
            private_api.order_calls = Some(order_calls.clone());
            let transport = FakeGuestShopImportTransport {
                state: &state,
                calls: Mutex::new(Vec::new()),
                order_calls: order_calls.clone(),
                result: Some(imported_result),
                invalid_response: false,
                attempt_started_during_await: AtomicBool::new(false),
                same_request_during_await: AtomicBool::new(false),
                ledger_unlocked_during_await: AtomicBool::new(false),
                select_other_auth_account_after_await: None,
                snapshot_before_response: Mutex::new(None),
                append_usage_during_await: None,
            };

            let first_state = &state;
            let first_api = &private_api;
            let first_order = order_calls.clone();
            let first_request = request.clone();
            let first = tauri::async_runtime::block_on(
                super::super::sync_authenticated_legacy_after_guest_import(
                    &state,
                    user_id,
                    "test-access-token",
                    &transport,
                    || async move {
                        first_order
                            .lock()
                            .unwrap()
                            .push("legacy_private_effect_sync");
                        {
                            let ledger = first_state.ledger.lock().unwrap();
                            let Some(PendingGuestShopImport::V2(imported)) = ledger
                                .pending_guest_shop_import_request(TARGET_ACCOUNT)
                                .unwrap()
                            else {
                                panic!("legacy sync must start after import completion");
                            };
                            assert_eq!(imported.phase, GuestImportPhase::Imported);
                            assert_eq!(imported.request, first_request);
                        }
                        let outcome = super::sync_private_effect_contribution(
                            first_state,
                            user_id,
                            "test-access-token",
                            first_api,
                            false,
                        )
                        .await?;
                        assert_eq!(
                            outcome,
                            super::super::PrivateEffectSyncOutcome::HeldForSharingPause
                        );
                        Ok(())
                    },
                ),
            );

            assert_eq!(first, Ok(GuestImportSyncOutcome::Imported));
            assert_eq!(
                *order_calls.lock().unwrap(),
                [
                    "import_guest_shop",
                    "legacy_private_effect_sync",
                    "planet_state",
                    "timeline"
                ]
            );
            assert_eq!(
                *private_api.calls.lock().unwrap(),
                ["planet_state", "timeline"]
            );
            assert!(private_api.uploads.lock().unwrap().is_empty());
            assert_eq!(*transport.calls.lock().unwrap(), ["import_guest_shop"]);
            let committed_result_json: String = state
                .ledger
                .lock()
                .unwrap()
                .connection
                .query_row(
                    "SELECT result_json FROM guest_shop_import_v2_capture WHERE target_account_id=?1",
                    [TARGET_ACCOUNT],
                    |row| row.get(0),
                )
                .unwrap();

            let retry_state = &state;
            let retry_api = &private_api;
            let retry_order = order_calls.clone();
            let retry_request = request.clone();
            let retry = tauri::async_runtime::block_on(
                super::super::sync_authenticated_legacy_after_guest_import(
                    &state,
                    user_id,
                    "test-access-token",
                    &transport,
                    || async move {
                        retry_order
                            .lock()
                            .unwrap()
                            .push("legacy_private_effect_sync");
                        {
                            let ledger = retry_state.ledger.lock().unwrap();
                            let Some(PendingGuestShopImport::V2(imported)) = ledger
                                .pending_guest_shop_import_request(TARGET_ACCOUNT)
                                .unwrap()
                            else {
                                panic!("retry must observe the committed import receipt");
                            };
                            assert_eq!(imported.phase, GuestImportPhase::Imported);
                            assert_eq!(imported.request, retry_request);
                        }
                        let outcome = super::sync_private_effect_contribution(
                            retry_state,
                            user_id,
                            "test-access-token",
                            retry_api,
                            false,
                        )
                        .await?;
                        assert_eq!(
                            outcome,
                            super::super::PrivateEffectSyncOutcome::HeldForSharingPause
                        );
                        Ok(())
                    },
                ),
            );

            assert_eq!(retry, Ok(GuestImportSyncOutcome::Imported));
            assert_eq!(*transport.calls.lock().unwrap(), ["import_guest_shop"]);
            assert_eq!(
                *private_api.calls.lock().unwrap(),
                ["planet_state", "timeline", "planet_state", "timeline"]
            );
            assert!(private_api.uploads.lock().unwrap().is_empty());
            let ledger = state.ledger.lock().unwrap();
            assert_eq!(
                ledger
                    .connection
                    .query_row::<String, _, _>(
                        "SELECT result_json FROM guest_shop_import_v2_capture WHERE target_account_id=?1",
                        [TARGET_ACCOUNT],
                        |row| row.get(0),
                    )
                    .unwrap(),
                committed_result_json
            );
            assert_eq!(
                *order_calls.lock().unwrap(),
                [
                    "import_guest_shop",
                    "legacy_private_effect_sync",
                    "planet_state",
                    "timeline",
                    "legacy_private_effect_sync",
                    "planet_state",
                    "timeline"
                ]
            );
        }

        #[test]
        fn authenticated_selection_only_suppresses_the_guest_import_pending_outcome() {
            let mut ledger = prepared_first_reset_ledger();
            let PendingGuestShopImport::V2(capture) = ledger
                .capture_guest_shop_import_request(TARGET_ACCOUNT)
                .unwrap()
            else {
                panic!("schema-2 import capture expected");
            };
            let request = capture.request;
            ledger
                .connection
                .execute_batch(
                    "CREATE TRIGGER fail_auth_selection_insert
                     BEFORE INSERT ON setting
                     WHEN NEW.key='selected_auth_account_id'
                     BEGIN SELECT RAISE(ABORT,'injected auth selection storage failure'); END;",
                )
                .unwrap();
            let state = app_state_from_ledger(ledger);
            let user_id = TARGET_ACCOUNT.strip_prefix("account:").unwrap();

            let unrelated_storage_failure =
                super::super::select_planet_account_for_authenticated_sync(&state, user_id);

            assert_eq!(
                unrelated_storage_failure,
                Err("인증 계정을 저장할 수 없습니다".into()),
                "a pending capture must not hide an unrelated auth-selection storage error"
            );
            {
                let ledger = state.ledger.lock().unwrap();
                let Some(PendingGuestShopImport::V2(pending)) = ledger
                    .pending_guest_shop_import_request(TARGET_ACCOUNT)
                    .unwrap()
                else {
                    panic!("the original capture must remain pending");
                };
                assert_eq!(pending.phase, GuestImportPhase::Captured);
                assert_eq!(pending.request, request);
                assert_eq!(ledger.cosmetic_account_id().unwrap(), "local");
                ledger
                    .connection
                    .execute_batch("DROP TRIGGER fail_auth_selection_insert")
                    .unwrap();
            }

            assert_eq!(
                super::super::select_planet_account_for_authenticated_sync(&state, user_id),
                Ok(()),
                "the typed GuestShopImportPending outcome remains resumable"
            );
            let ledger = state.ledger.lock().unwrap();
            let Some(PendingGuestShopImport::V2(pending)) = ledger
                .pending_guest_shop_import_request(TARGET_ACCOUNT)
                .unwrap()
            else {
                panic!("the resumable capture must remain pending");
            };
            assert_eq!(pending.phase, GuestImportPhase::Captured);
            assert_eq!(pending.request, request);
            assert_eq!(ledger.cosmetic_account_id().unwrap(), "local");
        }

        #[test]
        fn authenticated_sync_retries_lost_response_with_same_import_request() {
            let mut ledger = prepared_first_reset_ledger();
            let PendingGuestShopImport::V2(capture) = ledger
                .capture_guest_shop_import_request(TARGET_ACCOUNT)
                .unwrap()
            else {
                panic!("schema-2 import capture expected");
            };
            let first_request = capture.request;
            let state = app_state_from_ledger(ledger);
            let user_id = TARGET_ACCOUNT.strip_prefix("account:").unwrap();
            assert!(state.select_planet_account(user_id).is_err());
            let order_calls = Arc::new(Mutex::new(Vec::new()));
            let transport = LostResponseGuestShopImportTransport {
                requests: Mutex::new(Vec::new()),
                applied_request: Mutex::new(None),
                order_calls: order_calls.clone(),
            };

            let first = tauri::async_runtime::block_on(
                super::super::sync_authenticated_legacy_after_guest_import(
                    &state,
                    user_id,
                    "test-access-token",
                    &transport,
                    || async { panic!("legacy continuation ran after a lost response") },
                ),
            );

            assert_eq!(
                first,
                Err("게스트 상점 가져오기를 완료하지 못했습니다".into())
            );
            assert_eq!(
                transport.requests.lock().unwrap().as_slice(),
                [first_request.clone()]
            );
            {
                let ledger = state.ledger.lock().unwrap();
                let Some(PendingGuestShopImport::V2(pending)) = ledger
                    .pending_guest_shop_import_request(TARGET_ACCOUNT)
                    .unwrap()
                else {
                    panic!("the durable attempt must remain after response loss");
                };
                assert_eq!(pending.phase, GuestImportPhase::AttemptStarted);
                assert_eq!(pending.request, first_request);
                assert_eq!(ledger.cosmetic_account_id().unwrap(), "local");
            }

            let legacy_order = order_calls.clone();
            let retry = tauri::async_runtime::block_on(
                super::super::sync_authenticated_legacy_after_guest_import(
                    &state,
                    user_id,
                    "test-access-token",
                    &transport,
                    || async move {
                        legacy_order.lock().unwrap().push("legacy_sync");
                        Ok(())
                    },
                ),
            );

            assert_eq!(retry, Ok(GuestImportSyncOutcome::Imported));
            assert_eq!(
                transport.requests.lock().unwrap().as_slice(),
                [first_request.clone(), first_request.clone()]
            );
            assert_eq!(
                *transport.applied_request.lock().unwrap(),
                Some(first_request)
            );
            assert_eq!(
                *order_calls.lock().unwrap(),
                ["import_guest_shop", "import_guest_shop", "legacy_sync"]
            );
        }

        #[test]
        fn authenticated_sync_retries_same_import_after_local_marker_failure() {
            let mut ledger = prepared_first_reset_ledger();
            let PendingGuestShopImport::V2(capture) = ledger
                .capture_guest_shop_import_request(TARGET_ACCOUNT)
                .unwrap()
            else {
                panic!("schema-2 import capture expected");
            };
            let request = capture.request;
            ledger.set_selected_auth_account(TARGET_ACCOUNT).unwrap();
            ledger
                .connection
                .execute_batch(
                    "CREATE TRIGGER fail_v2_import_marker
                     BEFORE UPDATE OF phase ON guest_shop_import_v2_capture
                     WHEN NEW.phase='imported'
                     BEGIN SELECT RAISE(ABORT,'injected final import marker failure'); END;",
                )
                .unwrap();
            let state = app_state_from_ledger(ledger);
            let user_id = TARGET_ACCOUNT.strip_prefix("account:").unwrap();
            let order_calls = Arc::new(Mutex::new(Vec::new()));
            let transport = FakeGuestShopImportTransport {
                state: &state,
                calls: Mutex::new(Vec::new()),
                order_calls: order_calls.clone(),
                result: Some(imported_result_for(&request)),
                invalid_response: false,
                attempt_started_during_await: AtomicBool::new(false),
                same_request_during_await: AtomicBool::new(false),
                ledger_unlocked_during_await: AtomicBool::new(false),
                select_other_auth_account_after_await: None,
                snapshot_before_response: Mutex::new(None),
                append_usage_during_await: None,
            };

            let first = tauri::async_runtime::block_on(
                super::super::sync_authenticated_legacy_after_guest_import(
                    &state,
                    user_id,
                    "test-access-token",
                    &transport,
                    || async { panic!("legacy continuation ran after local completion failed") },
                ),
            );

            assert_eq!(
                first,
                Err("게스트 상점 가져오기 응답을 저장할 수 없습니다".into())
            );
            assert_eq!(*transport.calls.lock().unwrap(), ["import_guest_shop"]);
            assert!(transport
                .attempt_started_during_await
                .load(Ordering::SeqCst));
            assert!(transport.same_request_during_await.load(Ordering::SeqCst));
            let before_failed_completion = transport
                .snapshot_before_response
                .lock()
                .unwrap()
                .clone()
                .expect("the in-flight response must capture the AttemptStarted snapshot");
            {
                let ledger = state.ledger.lock().unwrap();
                assert_eq!(
                    ledger_snapshot_excluding_selected_auth(&ledger.connection),
                    before_failed_completion,
                    "failed local completion must roll back cache, credits, journals, and ACKs"
                );
                let Some(PendingGuestShopImport::V2(pending)) = ledger
                    .pending_guest_shop_import_request(TARGET_ACCOUNT)
                    .unwrap()
                else {
                    panic!("the durable attempt must remain after local marker failure");
                };
                assert_eq!(pending.phase, GuestImportPhase::AttemptStarted);
                assert_eq!(pending.request, request);
                assert_eq!(ledger.cosmetic_account_id().unwrap(), "local");
                ledger
                    .connection
                    .execute_batch("DROP TRIGGER fail_v2_import_marker")
                    .unwrap();
            }

            let legacy_order = order_calls.clone();
            let legacy_state = &state;
            let retry = tauri::async_runtime::block_on(
                super::super::sync_authenticated_legacy_after_guest_import(
                    &state,
                    user_id,
                    "test-access-token",
                    &transport,
                    || async move {
                        legacy_order.lock().unwrap().push("legacy_sync");
                        let ledger = legacy_state.ledger.lock().unwrap();
                        let Some(PendingGuestShopImport::V2(completed)) = ledger
                            .pending_guest_shop_import_request(TARGET_ACCOUNT)
                            .unwrap()
                        else {
                            panic!("legacy continuation must observe the imported marker");
                        };
                        assert_eq!(completed.phase, GuestImportPhase::Imported);
                        assert_eq!(completed.request, request);
                        Ok(())
                    },
                ),
            );

            assert_eq!(retry, Ok(GuestImportSyncOutcome::Imported));
            assert_eq!(
                *transport.calls.lock().unwrap(),
                ["import_guest_shop", "import_guest_shop"]
            );
            assert!(transport.same_request_during_await.load(Ordering::SeqCst));
            assert_eq!(
                *order_calls.lock().unwrap(),
                ["import_guest_shop", "import_guest_shop", "legacy_sync"]
            );
        }

        #[test]
        fn authenticated_sync_keeps_attempt_pending_after_backend_rejection() {
            let mut ledger = prepared_first_reset_ledger();
            let PendingGuestShopImport::V2(capture) = ledger
                .capture_guest_shop_import_request(TARGET_ACCOUNT)
                .unwrap()
            else {
                panic!("schema-2 import capture expected");
            };
            let request = capture.request;
            ledger.set_selected_auth_account(TARGET_ACCOUNT).unwrap();
            let state = app_state_from_ledger(ledger);
            let user_id = TARGET_ACCOUNT.strip_prefix("account:").unwrap();
            let order_calls = Arc::new(Mutex::new(Vec::new()));
            let transport = FakeGuestShopImportTransport {
                state: &state,
                calls: Mutex::new(Vec::new()),
                order_calls: order_calls.clone(),
                result: None,
                invalid_response: true,
                attempt_started_during_await: AtomicBool::new(false),
                same_request_during_await: AtomicBool::new(false),
                ledger_unlocked_during_await: AtomicBool::new(false),
                select_other_auth_account_after_await: None,
                snapshot_before_response: Mutex::new(None),
                append_usage_during_await: None,
            };

            let result = tauri::async_runtime::block_on(
                super::super::sync_authenticated_legacy_after_guest_import(
                    &state,
                    user_id,
                    "test-access-token",
                    &transport,
                    || async { panic!("legacy continuation ran after backend rejection") },
                ),
            );

            assert_eq!(
                result,
                Err("게스트 상점 가져오기를 완료하지 못했습니다".into())
            );
            assert_eq!(*transport.calls.lock().unwrap(), ["import_guest_shop"]);
            assert!(transport
                .attempt_started_during_await
                .load(Ordering::SeqCst));
            assert!(transport.same_request_during_await.load(Ordering::SeqCst));
            let before_response = transport
                .snapshot_before_response
                .lock()
                .unwrap()
                .clone()
                .expect("the in-flight response must capture the AttemptStarted snapshot");
            {
                let ledger = state.ledger.lock().unwrap();
                assert_eq!(
                    ledger_snapshot_excluding_selected_auth(&ledger.connection),
                    before_response,
                    "a rejected backend response must not alter local cache or ACK state"
                );
                let Some(PendingGuestShopImport::V2(pending)) = ledger
                    .pending_guest_shop_import_request(TARGET_ACCOUNT)
                    .unwrap()
                else {
                    panic!("the rejected response must leave the request pending");
                };
                assert_eq!(pending.phase, GuestImportPhase::AttemptStarted);
                assert_eq!(pending.request, request);
                assert_eq!(ledger.cosmetic_account_id().unwrap(), "local");
            }
            assert_eq!(*order_calls.lock().unwrap(), ["import_guest_shop"]);
        }

        #[test]
        fn authenticated_sync_does_not_apply_import_after_auth_account_switch_during_await() {
            let mut ledger = prepared_first_reset_ledger();
            let PendingGuestShopImport::V2(capture) = ledger
                .capture_guest_shop_import_request(TARGET_ACCOUNT)
                .unwrap()
            else {
                panic!("schema-2 import capture expected");
            };
            let request = capture.request;
            let state = app_state_from_ledger(ledger);
            let user_id = TARGET_ACCOUNT.strip_prefix("account:").unwrap();
            assert!(state.select_planet_account(user_id).is_err());
            let order_calls = Arc::new(Mutex::new(Vec::new()));
            let transport = FakeGuestShopImportTransport {
                state: &state,
                calls: Mutex::new(Vec::new()),
                order_calls: order_calls.clone(),
                result: Some(imported_result_for(&request)),
                invalid_response: false,
                attempt_started_during_await: AtomicBool::new(false),
                same_request_during_await: AtomicBool::new(false),
                ledger_unlocked_during_await: AtomicBool::new(false),
                select_other_auth_account_after_await: Some(OTHER_TARGET_ACCOUNT.into()),
                snapshot_before_response: Mutex::new(None),
                append_usage_during_await: None,
            };
            let legacy_order = order_calls.clone();

            let result = tauri::async_runtime::block_on(
                super::super::sync_authenticated_legacy_after_guest_import(
                    &state,
                    user_id,
                    "test-access-token",
                    &transport,
                    || async move {
                        legacy_order.lock().unwrap().push("legacy_sync");
                        Ok(())
                    },
                ),
            );

            assert_eq!(result, Ok(GuestImportSyncOutcome::Held));
            assert_eq!(*transport.calls.lock().unwrap(), ["import_guest_shop"]);
            assert!(transport
                .attempt_started_during_await
                .load(Ordering::SeqCst));
            assert!(transport.same_request_during_await.load(Ordering::SeqCst));
            assert!(transport
                .ledger_unlocked_during_await
                .load(Ordering::SeqCst));
            assert_eq!(
                *order_calls.lock().unwrap(),
                ["import_guest_shop"],
                "legacy continuation must be skipped after the selected account changes"
            );

            let ledger = state.ledger.lock().unwrap();
            let before = transport
                .snapshot_before_response
                .lock()
                .unwrap()
                .clone()
                .expect("the in-flight response should capture its pre-response ledger state");
            assert_eq!(
                before,
                ledger_snapshot_excluding_selected_auth(&ledger.connection)
            );
            let Some(PendingGuestShopImport::V2(pending)) = ledger
                .pending_guest_shop_import_request(TARGET_ACCOUNT)
                .unwrap()
            else {
                panic!("the original target marker must remain pending");
            };
            assert_eq!(pending.phase, GuestImportPhase::AttemptStarted);
            assert_eq!(pending.request, request);
            assert_eq!(ledger.cosmetic_account_id().unwrap(), "local");
            assert_eq!(
                ledger
                    .connection
                    .query_row::<String, _, _>(
                        "SELECT value FROM setting WHERE key='selected_auth_account_id'",
                        [],
                        |row| row.get(0),
                    )
                    .unwrap(),
                OTHER_TARGET_ACCOUNT
            );
            assert_eq!(
                ledger
                    .connection
                    .query_row::<i64, _, _>(
                        "SELECT count(*) FROM planet_account_state WHERE account_id=?1",
                        [OTHER_TARGET_ACCOUNT],
                        |row| row.get(0),
                    )
                    .unwrap(),
                0
            );
        }

        #[test]
        fn session_refresh_failure_keeps_first_login_import_marker_unchanged() {
            let state = app_state_from_ledger(prepared_first_reset_ledger());
            let user_id = TARGET_ACCOUNT.strip_prefix("account:").unwrap();
            assert!(state.select_planet_account(user_id).is_err());
            let before = state
                .ledger
                .lock()
                .unwrap()
                .pending_guest_shop_import_request(TARGET_ACCOUNT)
                .unwrap()
                .unwrap();

            let result = tauri::async_runtime::block_on(
                super::super::refresh_saved_session_or_queue_cached_state(&state, user_id, async {
                    Err(crate::sync::auth::AuthError::Transport)
                }),
            );

            assert!(result.is_err());
            let ledger = state.ledger.lock().unwrap();
            assert_eq!(
                ledger
                    .pending_guest_shop_import_request(TARGET_ACCOUNT)
                    .unwrap(),
                Some(before)
            );
            assert_eq!(ledger.cosmetic_account_id().unwrap(), "local");
            assert_eq!(
                ledger
                    .connection
                    .query_row::<String, _, _>(
                        "SELECT value FROM setting WHERE key='selected_auth_account_id'",
                        [],
                        |row| row.get(0),
                    )
                    .unwrap(),
                TARGET_ACCOUNT
            );
        }
    }
}
