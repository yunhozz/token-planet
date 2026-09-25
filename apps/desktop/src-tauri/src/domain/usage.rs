use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Agent {
    Codex,
    ClaudeCode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageCoverage {
    Complete,
    Partial,
    Unavailable,
    Unsupported,
    UserDisabled,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub coverage: UsageCoverage,
}

pub fn known_subtotal(usages: &[TokenUsage]) -> Option<u64> {
    let mut known = usages
        .iter()
        .filter(|usage| usage.coverage != UsageCoverage::UserDisabled)
        .filter_map(|usage| usage.total_tokens);
    let first = known.next()?;
    Some(known.fold(first, u64::saturating_add))
}

#[cfg(test)]
mod tests {
    use super::{known_subtotal, Agent, TokenUsage, UsageCoverage};

    fn usage(total_tokens: Option<u64>, coverage: UsageCoverage) -> TokenUsage {
        TokenUsage {
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            total_tokens,
            coverage,
        }
    }

    #[test]
    fn sums_source_totals_without_adding_breakdowns_twice() {
        let codex = TokenUsage {
            input_tokens: Some(30),
            output_tokens: Some(12),
            cache_read_tokens: Some(8),
            cache_write_tokens: None,
            total_tokens: Some(42),
            coverage: UsageCoverage::Complete,
        };
        let claude = usage(Some(12), UsageCoverage::Partial);
        assert_eq!(known_subtotal(&[codex, claude]), Some(54));
    }

    #[test]
    fn measured_zero_differs_from_unknown() {
        let measured_zero = usage(Some(0), UsageCoverage::Complete);
        let unknown = usage(None, UsageCoverage::Unavailable);
        assert_eq!(known_subtotal(&[measured_zero]), Some(0));
        assert_eq!(known_subtotal(&[unknown]), None);
    }

    #[test]
    fn user_disabled_source_is_excluded_even_with_stale_count() {
        let disabled = usage(Some(900), UsageCoverage::UserDisabled);
        let enabled = usage(Some(42), UsageCoverage::Complete);
        assert_eq!(known_subtotal(&[disabled, enabled]), Some(42));
    }

    #[test]
    fn unsupported_usage_without_number_stays_unknown() {
        let unsupported = usage(None, UsageCoverage::Unsupported);
        assert_eq!(known_subtotal(&[unsupported]), None);
    }

    #[test]
    fn agent_names_serialize_for_the_frontend_contract() {
        assert_eq!(serde_json::to_string(&Agent::Codex).unwrap(), "\"codex\"");
        assert_eq!(
            serde_json::to_string(&Agent::ClaudeCode).unwrap(),
            "\"claude_code\""
        );
    }
}
