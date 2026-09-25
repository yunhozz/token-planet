use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::collectors::discovery::ScanSummary;
use crate::domain::planet::PlanetState;
use crate::domain::usage::UsageCoverage;
use crate::storage::ledger::{Ledger, ScanError};

pub const K_TOKENS: u64 = 100_000;
pub const STAGE_THRESHOLDS: [f64; 4] = [5.0, 20.0, 50.0, 100.0];

#[derive(Clone, Debug, Serialize)]
pub struct WorldSnapshot {
    pub usage: ScanSummary,
    pub growth_credit: f64,
    pub stage: u8,
    pub progress_to_next: f64,
    pub incomplete: bool,
    pub planet: PlanetState,
}

pub fn contribution_credit(known_tokens: u64, k: f64) -> f64 {
    (1.0 + known_tokens as f64 / k).log2()
}

pub fn world_snapshot(ledger: &Ledger, usage: ScanSummary) -> Result<WorldSnapshot, ScanError> {
    let (daily, local_current_tokens, local_lifetime_tokens) = ledger.planet_usage_totals()?;
    let mut growth_credit: f64 = daily
        .values()
        .copied()
        .map(|tokens| contribution_credit(tokens, K_TOKENS as f64))
        .sum();
    let cycle_id = ledger.planet_cycle_id()?;
    let mut current_planet_tokens = local_current_tokens;
    let mut remote_incomplete = false;
    if let Some((remote_cycle, remote_tokens, remote_growth, incomplete)) =
        ledger.synced_planet_metrics()?
    {
        if remote_cycle == cycle_id {
            current_planet_tokens = current_planet_tokens.max(remote_tokens);
            growth_credit = growth_credit.max(remote_growth);
            remote_incomplete = incomplete;
        }
    }
    let lifetime_tokens =
        local_lifetime_tokens.max(ledger.synced_planet_lifetime_tokens()?.unwrap_or_default());
    let stage = STAGE_THRESHOLDS
        .iter()
        .take_while(|&&threshold| growth_credit >= threshold)
        .count() as u8;
    let progress_to_next = if stage as usize == STAGE_THRESHOLDS.len() {
        1.0
    } else {
        let prior = if stage == 0 {
            0.0
        } else {
            STAGE_THRESHOLDS[stage as usize - 1]
        };
        (growth_credit - prior) / (STAGE_THRESHOLDS[stage as usize] - prior)
    };
    let incomplete = [usage.codex.coverage, usage.claude_code.coverage]
        .iter()
        .any(|coverage| {
            !matches!(
                coverage,
                UsageCoverage::Complete | UsageCoverage::UserDisabled
            )
        })
        || remote_incomplete;
    ensure_objects(ledger, growth_credit)?;
    let wallet_credits = ledger.planet_wallet_credits()?;
    let wallet_balance = wallet_credits.iter().try_fold(0_u64, |balance, credit| {
        balance
            .checked_add(credit.amount)
            .ok_or(ScanError::InvalidCount)
    })?;
    let now = chrono::Utc::now();
    let last_reset = ledger.last_reset_at()?;
    let reset_available = last_reset.map(|last| last + chrono::Duration::hours(24));
    let can_reset = reset_available.is_none_or(|available| now >= available);
    let planet = PlanetState {
        version: 1,
        profile: ledger.planet_profile()?,
        timezone: ledger.planet_timezone()?.to_string(),
        current_cycle_id: cycle_id,
        cycle_started_at_utc: ledger.planet_cycle_started_at()?,
        last_reset_at_utc: last_reset.map(|value| value.to_rfc3339()),
        wallet_balance,
        wallet_credits,
        current_planet_tokens,
        lifetime_tokens,
        growth_credit,
        stage,
        progress_to_next,
        incomplete,
        can_reset,
        reset_available_at_utc: reset_available.map(|value| value.to_rfc3339()),
        objects: ledger.planet_objects()?,
    };
    Ok(WorldSnapshot {
        usage,
        growth_credit,
        stage,
        progress_to_next,
        incomplete,
        planet,
    })
}

fn target_object_counts(credits: f64) -> [u32; 5] {
    let thresholds = [5.0, 20.0, 50.0, 100.0];
    let intervals = [1.0, 2.0, 4.0, 8.0, 16.0];
    let mut start = 0.0;
    let mut remainder = 0.0;
    let mut counts = [0; 5];
    for stage in 0..5 {
        let end = thresholds.get(stage).copied().unwrap_or(f64::INFINITY);
        let segment = (credits.min(end) - start).max(0.0);
        let available = segment + remainder;
        counts[stage] = ((available / intervals[stage]) + 1e-9).floor() as u32;
        remainder = (available - f64::from(counts[stage]) * intervals[stage]).max(0.0);
        start = end;
    }
    counts
}

fn ensure_objects(ledger: &Ledger, credits: f64) -> Result<(), ScanError> {
    let targets = target_object_counts(credits);
    let mut counts = [0_u32; 5];
    for object in ledger.planet_objects()? {
        if let Some(count) = counts.get_mut(object.stage as usize) {
            *count += 1;
        }
    }
    let cycle_id = ledger.planet_cycle_id()?;
    let kinds: [&[&str]; 5] = [
        &["rock", "water", "tree", "fern", "creature"],
        &["camp", "crops", "cottage", "path", "well"],
        &["house", "workshop", "plaza", "road", "market"],
        &["factory", "power", "rail", "tower", "district"],
        &["laboratory", "satellite", "rocket", "solar", "habitat"],
    ];
    for stage in 0..5_u8 {
        for ordinal in counts[stage as usize]..targets[stage as usize] {
            let digest = Sha256::digest(format!("{cycle_id}:{stage}:{ordinal}").as_bytes());
            let seed = u64::from_be_bytes(digest[..8].try_into().expect("sha256 prefix"));
            let kind = kinds[stage as usize][(seed as usize) % kinds[stage as usize].len()];
            let x = ((seed % 88) + 6) as u8;
            let y = (((seed >> 8) % 52) + 28) as u8;
            ledger.ensure_planet_object(stage, ordinal, kind, x, y, seed)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{contribution_credit, world_snapshot, K_TOKENS, STAGE_THRESHOLDS};
    use crate::collectors::{
        discovery::{ScanSummary, SourceHealth},
        ParsedRecord, RecordKind,
    };
    use crate::domain::usage::{Agent, TokenUsage, UsageCoverage};
    use crate::storage::ledger::Ledger;
    use chrono::{DateTime, Utc};
    use chrono_tz::UTC;
    use std::path::Path;

    fn usage(total: Option<u64>, coverage: UsageCoverage) -> TokenUsage {
        TokenUsage {
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            total_tokens: total,
            coverage,
        }
    }

    fn summary(codex: TokenUsage, claude: TokenUsage) -> ScanSummary {
        ScanSummary {
            codex,
            claude_code: claude,
            codex_source: SourceHealth::Ready,
            claude_code_source: SourceHealth::Ready,
            confirmed_subtotal: None,
            complete_total: None,
            scanned_at_utc: Utc::now(),
        }
    }

    fn insert(ledger: &mut Ledger, agent: Agent, key: &str, tokens: u64) {
        ledger
            .insert(&ParsedRecord {
                agent,
                kind: RecordKind::Response,
                event_key: key.into(),
                occurred_at_utc: DateTime::parse_from_rfc3339("2026-09-25T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                usage: usage(Some(tokens), UsageCoverage::Complete),
            })
            .unwrap();
    }

    #[test]
    fn zero_known_tokens_yield_proto_planet() {
        let ledger = Ledger::open(Path::new(":memory:"), UTC).unwrap();
        let world = world_snapshot(
            &ledger,
            summary(
                usage(Some(0), UsageCoverage::Complete),
                usage(Some(0), UsageCoverage::Complete),
            ),
        )
        .unwrap();
        assert_eq!(world.growth_credit, 0.0);
        assert_eq!(world.stage, 0);
        assert_eq!(world.progress_to_next, 0.0);
    }

    #[test]
    fn enabled_sources_combine_before_daily_curve() {
        let mut ledger = Ledger::open(Path::new(":memory:"), UTC).unwrap();
        insert(&mut ledger, Agent::Codex, "codex:r1", 50_000);
        insert(&mut ledger, Agent::ClaudeCode, "claude:r1", 50_000);
        let world = world_snapshot(
            &ledger,
            summary(
                usage(Some(50_000), UsageCoverage::Complete),
                usage(Some(50_000), UsageCoverage::Complete),
            ),
        )
        .unwrap();
        assert!((world.growth_credit - 1.0).abs() < 0.000001);
        assert_eq!(world.stage, 0);
        assert_eq!(K_TOKENS, 100_000);
        assert_eq!(STAGE_THRESHOLDS, [5.0, 20.0, 50.0, 100.0]);
    }

    #[test]
    fn missing_source_keeps_world_incomplete_while_known_credit_grows() {
        let mut ledger = Ledger::open(Path::new(":memory:"), UTC).unwrap();
        insert(&mut ledger, Agent::Codex, "codex:r1", 100_000);
        let world = world_snapshot(
            &ledger,
            summary(
                usage(Some(100_000), UsageCoverage::Complete),
                usage(None, UsageCoverage::Unavailable),
            ),
        )
        .unwrap();
        assert_eq!(world.growth_credit, 1.0);
        assert!(world.incomplete);
    }

    #[test]
    fn disabled_agent_history_does_not_grow_the_world() {
        let mut ledger = Ledger::open(Path::new(":memory:"), UTC).unwrap();
        insert(&mut ledger, Agent::Codex, "codex:r1", 100_000);
        ledger.set_agent_enabled(Agent::Codex, false).unwrap();
        let world = world_snapshot(
            &ledger,
            summary(
                usage(None, UsageCoverage::UserDisabled),
                usage(None, UsageCoverage::Unavailable),
            ),
        )
        .unwrap();
        assert_eq!(world.growth_credit, 0.0);
    }

    #[test]
    fn high_daily_usage_has_diminishing_marginal_credit() {
        assert!(
            contribution_credit(1_000_000, 100_000.0)
                < 2.0 * contribution_credit(500_000, 100_000.0)
        );
    }
}
