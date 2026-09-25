use serde::Deserialize;
use serde_json::Value;

use crate::collectors::{parse_timestamp, ParseError, ParsedRecord, RecordKind};
use crate::domain::usage::{Agent, TokenUsage, UsageCoverage};

pub const PARSER_VERSION: u16 = 1;

#[derive(Deserialize)]
struct RawLine {
    #[serde(rename = "type")]
    record_type: Option<String>,
    timestamp: Option<String>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    #[serde(rename = "requestId")]
    request_id: Option<String>,
    message: Option<RawMessage>,
}

#[derive(Deserialize)]
struct RawMessage {
    id: Option<String>,
    stop_reason: Option<String>,
    usage: Option<Value>,
}

pub fn parse_line(line: &str) -> Result<Option<ParsedRecord>, ParseError> {
    let raw: RawLine = serde_json::from_str(line).map_err(|_| ParseError::InvalidJson)?;
    if raw.record_type.as_deref() != Some("assistant") {
        return Ok(None);
    }
    let timestamp = parse_timestamp(raw.timestamp.as_deref())?;
    let session = raw
        .session_id
        .filter(|value| !value.is_empty())
        .ok_or(ParseError::InvalidIdentity)?;
    let message = raw.message.ok_or(ParseError::InvalidIdentity)?;
    let request = raw
        .request_id
        .or(message.id)
        .filter(|value| !value.is_empty())
        .ok_or(ParseError::InvalidIdentity)?;
    let finished = message
        .stop_reason
        .as_ref()
        .is_some_and(|reason| !reason.is_empty());
    Ok(Some(ParsedRecord {
        agent: Agent::ClaudeCode,
        kind: RecordKind::Response,
        event_key: format!("claude:response:{session}:{request}"),
        occurred_at_utc: timestamp,
        usage: parse_usage(message.usage.as_ref(), finished),
    }))
}

fn parse_usage(value: Option<&Value>, finished: bool) -> TokenUsage {
    let number = |name: &str| {
        value
            .and_then(|usage| usage.get(name))
            .and_then(Value::as_u64)
    };
    let input_tokens = number("input_tokens");
    let output_tokens = number("output_tokens");
    let cache_read_tokens = number("cache_read_input_tokens");
    let cache_write_tokens = number("cache_creation_input_tokens");
    let all = input_tokens
        .zip(output_tokens)
        .zip(cache_read_tokens)
        .zip(cache_write_tokens);
    let total_tokens = if finished {
        all.and_then(|(((input, output), cache_read), cache_write)| {
            input
                .checked_add(output)?
                .checked_add(cache_read)?
                .checked_add(cache_write)
        })
    } else {
        None
    };
    let any_known = [
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
    ]
    .iter()
    .any(Option::is_some);
    let coverage = if total_tokens.is_some() {
        UsageCoverage::Complete
    } else if !finished || value.is_none() || any_known {
        UsageCoverage::Unavailable
    } else {
        UsageCoverage::Unsupported
    };
    TokenUsage {
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        total_tokens,
        coverage,
    }
}

#[cfg(test)]
mod tests {
    use super::parse_line;
    use crate::collectors::RecordKind;
    use crate::domain::usage::{Agent, UsageCoverage};

    const COMPLETE: &str = r#"{"timestamp":"2026-09-25T00:00:00Z","type":"assistant","sessionId":"s1","message":{"id":"m1","stop_reason":"end_turn","usage":{"input_tokens":30,"output_tokens":12,"cache_read_input_tokens":8,"cache_creation_input_tokens":3,"output_tokens_details":{"thinking_tokens":4}}}}"#;

    #[test]
    fn complete_final_message_sums_four_nonoverlapping_categories() {
        let record = parse_line(COMPLETE).unwrap().unwrap();
        assert_eq!(record.agent, Agent::ClaudeCode);
        assert_eq!(record.kind, RecordKind::Response);
        assert_eq!(record.usage.total_tokens, Some(53));
        assert_eq!(record.usage.cache_read_tokens, Some(8));
        assert_eq!(record.usage.cache_write_tokens, Some(3));
        assert_eq!(record.usage.coverage, UsageCoverage::Complete);
    }

    #[test]
    fn message_without_usage_is_unavailable_not_zero() {
        let record = parse_line(r#"{"timestamp":"2026-09-25T00:00:00Z","type":"assistant","sessionId":"s1","message":{"id":"m2","stop_reason":"end_turn"}}"#).unwrap().unwrap();
        assert_eq!(record.usage.total_tokens, None);
        assert_eq!(record.usage.coverage, UsageCoverage::Unavailable);
    }

    #[test]
    fn unknown_usage_layout_is_unsupported() {
        let record = parse_line(r#"{"timestamp":"2026-09-25T00:00:00Z","type":"assistant","sessionId":"s1","message":{"id":"m3","stop_reason":"end_turn","usage":{"billed_tokens":53}}}"#).unwrap().unwrap();
        assert_eq!(record.usage.total_tokens, None);
        assert_eq!(record.usage.coverage, UsageCoverage::Unsupported);
    }

    #[test]
    fn unfinished_message_does_not_contribute_partial_usage() {
        let record = parse_line(r#"{"timestamp":"2026-09-25T00:00:00Z","type":"assistant","sessionId":"s1","message":{"id":"m4","stop_reason":null,"usage":{"input_tokens":30,"output_tokens":1,"cache_read_input_tokens":8,"cache_creation_input_tokens":3}}}"#).unwrap().unwrap();
        assert_eq!(record.usage.total_tokens, None);
        assert_eq!(record.usage.coverage, UsageCoverage::Unavailable);
    }

    #[test]
    fn same_message_identity_has_same_key() {
        let first = parse_line(COMPLETE).unwrap().unwrap();
        let replay = parse_line(r#"{"timestamp":"2026-09-25T00:01:00Z","type":"assistant","sessionId":"s1","message":{"id":"m1","stop_reason":"end_turn","usage":{"input_tokens":30,"output_tokens":12,"cache_read_input_tokens":8,"cache_creation_input_tokens":3}}}"#).unwrap().unwrap();
        assert_eq!(first.event_key, replay.event_key);
    }

    #[test]
    fn unrelated_user_message_is_ignored() {
        assert!(
            parse_line(r#"{"timestamp":"2026-09-25T00:00:00Z","type":"user"}"#)
                .unwrap()
                .is_none()
        );
    }
}
