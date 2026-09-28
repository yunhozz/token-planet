use std::time::Duration;

use crate::domain::usage::UsageCoverage;
use crate::sync::auth::{AuthConfig, SessionStore, SupabaseAuthClient};
use crate::sync::client::SupabaseSyncClient;
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

pub async fn sync_once(state: &AppState) -> Result<(), String> {
    let _gate = state.sync_gate.lock().await;
    let Some(config) = AuthConfig::from_env() else {
        return Ok(());
    };
    let store = SessionStore::new(&config).map_err(|_| "보안 저장소 오류")?;
    let Some(saved) = store.load().map_err(|_| "로그인 정보 오류")? else {
        return Ok(());
    };
    state.select_planet_account(&saved.user.id)?;
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
    import_pending_guest_cosmetics(state, &client, &session.access_token).await?;
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
    if let Some(remote) = client
        .my_planet_state(&session.access_token)
        .await
        .map_err(|_| "행성 동기화 상태를 불러올 수 없습니다")?
    {
        state
            .ledger
            .lock()
            .map_err(|_| "로컬 행성 상태 오류")?
            .merge_remote_planet_state(&remote)
            .map_err(|_| "행성 동기화 상태를 반영할 수 없습니다")?;
        state
            .scan()
            .map_err(|_| "행성 상태를 새로 계산할 수 없습니다")?;
    }
    let server_upload_credits = state
        .ledger
        .lock()
        .map_err(|_| "로컬 지갑 오류")?
        .planet_wallet_credits_for_server_upload()
        .map_err(|_| "서버 지갑 전송 내역 오류")?;
    let server_upload_balance = server_upload_credits.iter().try_fold(0_u64, |balance, credit| {
        balance
            .checked_add(credit.amount)
            .ok_or("서버 지갑 잔액 범위 오류")
    })?;
    let local_snapshot = state
        .latest
        .lock()
        .map_err(|_| "행성 상태를 읽을 수 없습니다")?
        .as_ref()
        .map(|snapshot| {
            let incomplete = [
                snapshot.usage.codex.coverage,
                snapshot.usage.claude_code.coverage,
            ]
            .iter()
            .any(|coverage| {
                !matches!(
                    coverage,
                    UsageCoverage::Complete | UsageCoverage::UserDisabled
                )
            });
            let mut planet = snapshot.planet.clone();
            planet.wallet_credits = server_upload_credits.clone();
            planet.wallet_balance = server_upload_balance;
            (planet, incomplete)
        });
    let sharing_paused = state
        .ledger
        .lock()
        .map_err(|_| "로컬 동기화 설정 오류")?
        .sharing_paused()
        .map_err(|_| "로컬 동기화 설정 오류")?;
    let publish_planet = !sharing_paused && policy.as_ref().is_none_or(|policy| !policy.paused);
    if let Some((local_planet, incomplete)) = local_snapshot
        .filter(|(planet, _)| planet.profile.is_some())
        .filter(|_| publish_planet)
    {
        let contribution = state
            .ledger
            .lock()
            .map_err(|_| "로컬 행성 상태 오류")?
            .planet_device_contribution(incomplete)
            .map_err(|_| "행성별 일일 집계를 준비할 수 없습니다")?;
        let canonical = client
            .upload_planet_state(&session.access_token, &local_planet, &contribution)
            .await
            .map_err(|_| "행성 상태 전송 실패")?;
        state
            .ledger
            .lock()
            .map_err(|_| "로컬 행성 상태 오류")?
            .merge_remote_planet_state(&canonical)
            .map_err(|_| "행성 동기화 상태를 반영할 수 없습니다")?;
        state
            .scan()
            .map_err(|_| "행성 상태를 새로 계산할 수 없습니다")?;
    }

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
        if ledger.sharing_paused().map_err(|_| "동기화 설정 오류")? {
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
    use super::RetryDelay;

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
