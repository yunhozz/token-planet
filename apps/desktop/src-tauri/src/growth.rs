use serde::Serialize;

use crate::collectors::discovery::ScanSummary;
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
}

pub fn contribution_credit(known_tokens: u64, k: f64) -> f64 {
    (1.0 + known_tokens as f64 / k).log2()
}

pub fn world_snapshot(ledger: &Ledger, usage: ScanSummary) -> Result<WorldSnapshot, ScanError> {
    let growth_credit = ledger
        .daily_known_totals()?
        .into_iter()
        .map(|tokens| contribution_credit(tokens, K_TOKENS as f64))
        .sum();
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
        });
    Ok(WorldSnapshot {
        usage,
        growth_credit,
        stage,
        progress_to_next,
        incomplete,
    })
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
