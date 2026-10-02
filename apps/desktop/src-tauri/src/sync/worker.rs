use std::{future::Future, pin::Pin, time::Duration};

use crate::domain::cosmetic_shop::{ResetShopResult, ShopActionStatus, ShopEffectTimeline};
use crate::domain::planet::{PlanetDeviceContributionSnapshot, PlanetState};
#[cfg(test)]
use crate::domain::planet::{PlanetDeviceContribution, PlanetEffectContributionSegment};
use crate::domain::usage::UsageCoverage;
use crate::growth::WorldSnapshot;
use crate::sync::auth::{AuthConfig, SessionStore, SupabaseAuthClient};
use crate::sync::client::{SupabaseSyncClient, SyncError};
use crate::storage::ledger::{Ledger, SignedResetIntent};
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
            Self::HeldForSharingPause => Err("공유 일시 중지로 행성 기여 업로드가 보류되었습니다".into()),
        }
    }
}

fn latest_usage_incomplete(state: &AppState) -> Result<bool, String> {
    let latest = state
        .latest
        .lock()
        .map_err(|_| "사용량 상태를 읽을 수 없습니다")?;
    let snapshot = latest
        .as_ref()
        .ok_or("사용량 상태를 준비할 수 없습니다")?;
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
        require_planet_sync_context(
            &ledger,
            &intent.account_id,
            &intent.expected_old_cycle_id,
        )?;
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
        Err(_) => return Err("행성 초기화 응답을 확인할 수 없습니다. 같은 요청으로 재시도합니다".into()),
    };

    {
        let ledger = state.ledger.lock().map_err(|_| "행성 상태 오류")?;
        require_planet_sync_context(
            &ledger,
            &intent.account_id,
            &intent.expected_old_cycle_id,
        )?;
    }
    if result.action.request_id != intent.request_id.to_string()
        || result.action.state.current_cycle_id != result.planet_state.current_cycle_id
    {
        return Err("행성 초기화 응답을 검증할 수 없습니다. 같은 요청으로 재시도합니다".into());
    }
    if result.action.status != ShopActionStatus::Reset {
        let status_message = match result.action.status {
            ShopActionStatus::CycleMismatch => "서버의 행성 주기가 변경되어 초기화 요청을 종료했습니다",
            ShopActionStatus::RequestConflict => "초기화 요청 ID가 다른 요청과 충돌하여 종료했습니다",
            _ => "서버가 행성 초기화를 확정하지 않았습니다",
        };
        let mut ledger = state.ledger.lock().map_err(|_| "행성 상태 오류")?;
        require_planet_sync_context(
            &ledger,
            &intent.account_id,
            &intent.expected_old_cycle_id,
        )?;
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
        require_planet_sync_context(
            &ledger,
            &intent.account_id,
            &intent.expected_old_cycle_id,
        )?;
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
    let _gate = state.sync_gate.lock().await;
    {
        let ledger = state.ledger.lock().map_err(|_| "로컬 대기열 오류")?;
        require_guest_shop_import_complete(&ledger)?;
    }
    sync_after_guest_import_check(state).await
}

async fn sync_after_guest_import_check(state: &AppState) -> Result<(), String> {
    let Some(config) = AuthConfig::from_env() else {
        return Ok(());
    };
    let store = SessionStore::new(&config).map_err(|_| "보안 저장소 오류")?;
    let Some(saved) = store.load().map_err(|_| "로그인 정보 오류")? else {
        return Ok(());
    };
    state.select_planet_account(&saved.user.id)?;
    if state.has_usage_scan_failure() {
        return Err("사용량 기록을 확인한 뒤 동기화할 수 있습니다".into());
    }
    let session = match SupabaseAuthClient::new(config.clone())
        .session(&store)
        .await
    {
        Ok(session) => session,
        Err(_) => {
            queue_cached(state, &saved.user.id);
            return Err("공동 세계 연결 실패".into());
        }
    };
    let client = SupabaseSyncClient::new(&config.base_url, &config.publishable_key);
    // A remote reset may already have succeeded even if the previous process
    // lost its response. Resolve its durable receipt before any normal state
    // fetch can merge a newer cycle into the local ledger.
    if recover_pending_signed_reset(state, &session.user.id, &session.access_token, &client)
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
        &client,
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
        let publish_planet = !world_policy_paused
            && !ledger.sharing_paused().map_err(|_| "동기화 설정 오류")?;
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
        bootstrap_contribution_snapshot, require_guest_shop_import_complete,
        recover_pending_signed_reset, sync_once, sync_private_effect_contribution,
        PrivateEffectSyncApi, PrivateEffectSyncOutcome, RetryDelay, SignedResetSyncApi,
    };
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
    use crate::collectors::discovery::SourceConfig;
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

    static AUTH_ENV_LOCK: Mutex<()> = Mutex::new(());

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
        add_usage(&mut ledger, "private-sync-usage", "2026-10-01T12:00:00Z", 100);
        let cycle_id = ledger.planet_cycle_id().unwrap();
        let state = AppState {
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
        ) -> Pin<Box<dyn Future<Output = Result<Option<PlanetState>, SyncError>> + Send + 'a>> {
            self.calls.lock().unwrap().push("planet_state".into());
            let state = self.remote_state.clone();
            Box::pin(async move { Ok(state) })
        }

        fn get_my_shop_effect_timeline<'a>(
            &'a self,
            _access_token: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<ShopEffectTimeline, SyncError>> + Send + 'a>> {
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
            ledger.connection.execute(
                "UPDATE setting SET value='2026-10-01T00:00:00Z'
                 WHERE key IN ('planet_activation_at_utc','planet_cycle_started_at_utc')",
                [],
            ).unwrap();
            ledger.ensure_planet_account(user_id).unwrap();
            add_usage(&mut ledger, "reset-replay-usage", "2026-10-01T12:00:00Z", 100);
            old_cycle_id = ledger.planet_cycle_id().unwrap();
        }
        let state = app_state_from_ledger(Ledger::open(&path, UTC).unwrap());
        let new_cycle_id = "80000000-0000-0000-0000-000000000042";
        let api = MockResetRecoveryApi {
            ledger: None,
            context_change: None,
            result: Some(server_reset_result(&state, uuid::Uuid::new_v4(), new_cycle_id)),
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
        assert!(first_result.is_err(), "the lost response leaves a retryable intent");
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
            let intent = ledger.pending_signed_reset_intent(user_id).unwrap().unwrap();
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

        assert!(retry_result.is_ok(), "the persisted reset receipt should apply after restart: {retry_result:?}");
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
        assert!(ledger.pending_signed_reset_intent(user_id).unwrap().is_none());
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
        assert!(fresh_reset.is_err(), "the next fresh reset remains held until guest import completes");
        assert_eq!(*api.calls.lock().unwrap(), calls_after_recovery, "fresh hold occurs before another state fetch or reset call");
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
            result: Some(server_reset_result(&state, uuid::Uuid::new_v4(), new_cycle_id)),
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

        assert!(result.is_err(), "unimported guest shop state must hold a fresh signed reset");
        assert!(api.calls.lock().unwrap().is_empty(), "the hold must precede state, timeline, upload, and reset calls");
        let ledger = state.ledger.lock().unwrap();
        assert_eq!(ledger.planet_cycle_id().unwrap(), old_cycle_id);
        assert!(ledger.pending_signed_reset_intent(user_id).unwrap().is_none());
    }

    #[test]
    fn lost_reset_receipt_survives_http_401_and_429_until_same_id_recovery() {
        let user_id = "00000000-0000-0000-0000-000000000051";
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.sqlite3");
        let old_cycle_id;
        {
            let mut ledger = Ledger::open(&path, UTC).unwrap();
            ledger.connection.execute(
                "UPDATE setting SET value='2026-10-01T00:00:00Z'
                 WHERE key IN ('planet_activation_at_utc','planet_cycle_started_at_utc')",
                [],
            ).unwrap();
            ledger.ensure_planet_account(user_id).unwrap();
            add_usage(&mut ledger, "reset-http-replay-usage", "2026-10-01T12:00:00Z", 100);
            old_cycle_id = ledger.planet_cycle_id().unwrap();
        }
        let state = app_state_from_ledger(Ledger::open(&path, UTC).unwrap());
        let new_cycle_id = "80000000-0000-0000-0000-000000000051";
        let api = MockResetRecoveryApi {
            ledger: None,
            context_change: None,
            result: Some(server_reset_result(&state, uuid::Uuid::new_v4(), new_cycle_id)),
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
        assert_eq!(first_reset_call, format!("reset:{request_id}:{old_cycle_id}"));
        drop(state);

        let reopened = app_state_from_ledger(Ledger::open(&path, UTC).unwrap());
        for status_code in [401, 429] {
            let retry = tauri::async_runtime::block_on(recover_pending_signed_reset(
                &reopened,
                user_id,
                "access-token",
                &api,
            ));

            assert!(retry.is_err(), "HTTP {status_code} cannot settle an ambiguous reset");
            let ledger = reopened.ledger.lock().unwrap();
            let intent = ledger.pending_signed_reset_intent(user_id).unwrap().unwrap();
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
        assert!(ledger.pending_signed_reset_intent(user_id).unwrap().is_none());
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

        assert!(result.is_err(), "server sharing pause holds the required preupload");
        assert_eq!(
            *api.calls.lock().unwrap(),
            vec![String::from("planet_state"), String::from("timeline")],
            "a held upload must not send a reset RPC or upload contribution",
        );
        let ledger = state.ledger.lock().unwrap();
        assert_eq!(ledger.planet_cycle_id().unwrap(), cycle_id);
        assert!(ledger.pending_signed_reset_intent(user_id).unwrap().is_none());
        assert_eq!(ledger.connection.query_row::<i64, _, _>(
            "SELECT count(*) FROM shop_action_request WHERE account_id=?1",
            [format!("account:{user_id}")],
            |row| row.get(0),
        ).unwrap(), 0, "a held preupload leaves no durable reset intent");
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
        assert!(ledger.pending_signed_reset_intent(user_id).unwrap().is_none());
    }

    #[test]
    fn confirmed_cooldown_prevents_signed_reset_and_creates_no_intent() {
        let user_id = "00000000-0000-0000-0000-000000000050";
        let (state, cycle_id) = app_state_with_account(user_id);
        let mut canonical_state = server_planet_state(&cycle_id);
        canonical_state.can_reset = false;
        canonical_state.reset_available_at_utc = Some(
            (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
        );
        let api = MockResetRecoveryApi {
            ledger: None,
            context_change: None,
            result: None,
            timeline: server_reset_timeline(user_id, &cycle_id, "80000000-0000-0000-0000-000000000050"),
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

        assert!(result.is_err(), "the canonical server cooldown holds a new reset");
        assert_eq!(
            *api.calls.lock().unwrap(),
            vec!["planet_state", "timeline", "upload"],
            "a known cooldown must not create a reset intent or call the reset RPC",
        );
        let ledger = state.ledger.lock().unwrap();
        assert_eq!(ledger.planet_cycle_id().unwrap(), cycle_id);
        assert!(ledger.pending_signed_reset_intent(user_id).unwrap().is_none());
        assert_eq!(ledger.connection.query_row::<i64, _, _>(
            "SELECT count(*) FROM shop_action_request WHERE account_id=?1",
            [format!("account:{user_id}")],
            |row| row.get(0),
        ).unwrap(), 0);
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
            ledger.pending_signed_reset_intent(user_id).unwrap().unwrap().request_id,
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
            ledger.connection.execute_batch(&format!(
                "CREATE TRIGGER fail_signed_reset_receipt
                 BEFORE UPDATE OF result_json ON shop_action_request
                 WHEN OLD.account_id='{account_scope}' AND OLD.request_id='{request_id}'
                 BEGIN SELECT RAISE(ABORT, 'forced receipt write failure'); END;"
            )).unwrap();
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
                ledger.pending_signed_reset_intent(user_id).unwrap().unwrap().request_id,
                request_id,
            );
            assert_eq!(ledger.connection.query_row::<i64, _, _>(
                "SELECT count(*) FROM shop_effect_timeline_state WHERE account_id=?1",
                [&account_scope],
                |row| row.get(0),
            ).unwrap(), 0, "timeline cache changes roll back with a failed intent completion");
            assert_eq!(ledger.connection.query_row::<i64, _, _>(
                "SELECT count(*) FROM shop_action_request WHERE account_id=?1 AND request_id=?2 AND result_json IS NULL",
                rusqlite::params![account_scope, request_id.to_string()],
                |row| row.get(0),
            ).unwrap(), 1);
        }
        state.ledger.lock().unwrap().connection.execute_batch(
            "DROP TRIGGER fail_signed_reset_receipt;",
        ).unwrap();

        let retried = tauri::async_runtime::block_on(recover_pending_signed_reset(
            &state,
            user_id,
            "access-token",
            &api,
        ));

        assert!(retried.is_ok(), "same persisted request should succeed after cache transaction can commit: {retried:?}");
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
        assert!(ledger.pending_signed_reset_intent(user_id).unwrap().is_none());
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
            assert!(ledger.pending_signed_reset_intent(user_id).unwrap().is_none());
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
            ("00000000-0000-0000-0000-000000000047", ResetContextChange::Account),
            ("00000000-0000-0000-0000-000000000048", ResetContextChange::Cycle),
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
                ledger.pending_signed_reset_intent(user_id).unwrap().unwrap().request_id,
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
        uploads: Mutex<Vec<(serde_json::Value, serde_json::Value)>>,
    }

    impl MockPrivateSyncApi<'_> {
        fn change_context(&self, at: ContextChangeAt) {
            let (Some(ledger), Some((change_at, change))) = (self.ledger, self.context_change) else {
                return;
            };
            if change_at != at {
                return;
            }
            let mut ledger = ledger.lock().unwrap();
            match change {
                ContextChange::Account => {
                    ledger.ensure_planet_account("00000000-0000-0000-0000-000000000099").unwrap();
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
        ) -> Pin<Box<dyn Future<Output = Result<Option<PlanetState>, SyncError>> + Send + 'a>> {
            self.calls.lock().unwrap().push("planet_state");
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
        ) -> Pin<Box<dyn Future<Output = Result<ShopEffectTimeline, SyncError>> + Send + 'a>> {
            self.calls.lock().unwrap().push("timeline");
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
            self.calls.lock().unwrap().push("upload");
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
        ledger.ensure_planet_account("00000000-0000-0000-0000-000000000064").unwrap();
        assert!(ledger.pending_guest_cosmetic_import().unwrap().is_none());

        let result = require_guest_shop_import_complete(&ledger);

        assert!(result.is_err(), "guest ownership requires a complete import first");
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
        let _env_guard = AUTH_ENV_LOCK.lock().unwrap();
        let old_url = std::env::var_os("TOKEN_WORLD_SUPABASE_URL");
        let old_key = std::env::var_os("TOKEN_WORLD_SUPABASE_PUBLISHABLE_KEY");
        std::env::set_var("TOKEN_WORLD_SUPABASE_URL", "");
        std::env::set_var("TOKEN_WORLD_SUPABASE_PUBLISHABLE_KEY", "");

        let ledger = Ledger::open(Path::new(":memory:"), UTC).unwrap();
        let guest_account = ledger.cosmetic_account_id().unwrap();
        let original_cycle = ledger.planet_cycle_id().unwrap();
        ledger.connection.execute(
            "INSERT INTO shop_landscape_instance(account_id,instance_id,sku,variation_index,seed,variation_version,acquired_at_utc)
             VALUES (?1,'pending-guest-instance','land_tree',0,'seed',1,'2026-10-01T00:00:00Z')",
            [&guest_account],
        ).unwrap();
        let state = AppState {
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
            std::env::set_var("TOKEN_WORLD_SUPABASE_URL", value);
        } else {
            std::env::remove_var("TOKEN_WORLD_SUPABASE_URL");
        }
        if let Some(value) = old_key {
            std::env::set_var("TOKEN_WORLD_SUPABASE_PUBLISHABLE_KEY", value);
        } else {
            std::env::remove_var("TOKEN_WORLD_SUPABASE_PUBLISHABLE_KEY");
        }

        assert_eq!(
            result,
            Err("게스트 상점 가져오기를 완료한 뒤 동기화할 수 있습니다".into()),
            "pending local guest state must block first login before any RPC",
        );
        let ledger = state.ledger.lock().unwrap();
        assert_eq!(ledger.cosmetic_account_id().unwrap(), "local", "first login must not move the active account");
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
        assert_eq!(ledger.connection.query_row::<i64, _, _>(
            "SELECT count(*) FROM shop_effect_timeline_state WHERE account_id=?1",
            [format!("account:{user_id}")], |row| row.get(0),
        ).unwrap(), 0);
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
                assert_eq!(ledger.cosmetic_account_id().unwrap(), "account:00000000-0000-0000-0000-000000000099");
                assert!(ledger.planet_profile().unwrap().is_none(), "stale canonical response must not be cached for the newly selected account");
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
        let expected_server_state = serde_json::to_value(api.remote_state.as_ref().unwrap()).unwrap();

        let outcome = tauri::async_runtime::block_on(sync_private_effect_contribution(
            &state,
            user_id,
            "account-token",
            &api,
            false,
        )).unwrap();

        assert_eq!(outcome, PrivateEffectSyncOutcome::Uploaded { can_reset: true });
        assert!(outcome.require_uploaded().unwrap());
        assert_eq!(*api.calls.lock().unwrap(), vec!["planet_state", "timeline", "upload"]);
        let uploads = api.uploads.lock().unwrap();
        assert_eq!(uploads.len(), 1);
        assert_eq!(uploads[0].0, expected_server_state, "upload p_state must reuse the server response without local profile/timezone/object claims");
        let contribution = &uploads[0].1;
        let keys = contribution.as_object().unwrap().keys().map(String::as_str).collect::<std::collections::BTreeSet<_>>();
        assert_eq!(keys, std::collections::BTreeSet::from([
            "activity_days", "canonical_version", "current_cycle_id", "current_planet_tokens",
            "daily_segments", "daily_tokens", "device_id", "incomplete", "lifetime_tokens",
        ]));
        assert_eq!(contribution["current_cycle_id"], cycle_id);
        assert_eq!(contribution["current_planet_tokens"], 100);
        assert_eq!(contribution["daily_segments"][0]["effect_revision"], 0);
        assert_eq!(contribution["activity_days"][0]["first_occurred_at_utc"], "2026-10-01T12:00:00+00:00");
    }

    #[test]
    fn private_effect_sync_holds_upload_when_local_or_server_sharing_is_paused() {
        let user_id = "00000000-0000-0000-0000-000000000064";
        for (local_pause, world_policy_paused) in [(true, false), (false, true)] {
            let (state, cycle_id) = app_state_with_account(user_id);
            if local_pause {
                state.ledger.lock().unwrap().set_sharing_paused(true).unwrap();
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
            daily_tokens: BTreeMap::from([
                ("2026-10-01".into(), 10),
                ("2026-10-02".into(), 20),
            ]),
            incomplete: false,
        };

        let snapshot = bootstrap_contribution_snapshot(raw.clone()).unwrap();

        assert_eq!(snapshot.raw, raw);
        assert_eq!(snapshot.canonical_version, 0);
        assert_eq!(
            snapshot.daily_segments.iter().map(|segment| (
                segment.cycle_id.as_str(),
                segment.date.as_str(),
                segment.effect_revision,
                segment.tokens,
            )).collect::<Vec<_>>(),
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
        add_usage(&mut ledger, "unknown-before-bounds", "2026-09-25T12:00:00Z", 100);
        add_usage(&mut ledger, "known-old-cycle", "2026-09-27T12:00:00Z", 100);
        add_usage(&mut ledger, "current-bound-start", "2026-09-29T00:00:00Z", 50);
        add_usage(&mut ledger, "known-current-baseline", "2026-09-30T12:00:00Z", 200);
        add_usage(&mut ledger, "known-current-effect", "2026-10-01T12:00:00Z", 300);

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
            .find(|segment| {
                segment.cycle_id == cycle_id && segment.date == "2026-09-29"
            })
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
}
