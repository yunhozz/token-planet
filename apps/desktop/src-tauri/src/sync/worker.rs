use std::time::Duration;

use crate::domain::planet::{
    PlanetDeviceContribution, PlanetDeviceContributionSnapshot,
    PlanetEffectContributionSegment,
};
use crate::domain::usage::UsageCoverage;
use crate::sync::auth::{AuthConfig, SessionStore, SupabaseAuthClient};
use crate::sync::client::SupabaseSyncClient;
use crate::storage::ledger::Ledger;
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

fn require_guest_shop_import_complete(ledger: &Ledger) -> Result<(), String> {
    if ledger
        .has_unimported_guest_shop_state()
        .map_err(|_| "게스트 상점 가져오기 상태를 확인할 수 없습니다")?
    {
        return Err("게스트 상점 가져오기를 완료한 뒤 동기화할 수 있습니다".into());
    }
    Ok(())
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
            .rebuild_snapshot_from_latest_usage()
            .map_err(|_| "행성 상태를 새로 계산할 수 없습니다")?;
    }
    let server_upload_credits = state
        .ledger
        .lock()
        .map_err(|_| "로컬 지갑 오류")?
        .planet_wallet_credits_for_server_upload()
        .map_err(|_| "서버 지갑 전송 내역 오류")?;
    let server_upload_balance =
        server_upload_credits
            .iter()
            .try_fold(0_u64, |balance, credit| {
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
            .rebuild_snapshot_from_latest_usage()
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
    use super::{
        bootstrap_contribution_snapshot, require_guest_shop_import_complete,
        RetryDelay,
    };
    use crate::collectors::{ParsedRecord, RecordKind};
    use crate::domain::cosmetic_shop::{
        ActiveEffects, ShopCycleBound, ShopEffectInterval, ShopEffectTimeline,
    };
    use crate::domain::planet::PlanetDeviceContribution;
    use crate::domain::usage::{Agent, TokenUsage, UsageCoverage};
    use crate::storage::ledger::Ledger;
    use chrono::{DateTime, Utc};
    use chrono_tz::UTC;
    use std::{collections::BTreeMap, path::Path};

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
