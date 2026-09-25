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
    ordinal: Option<u64>,
    payload: Option<RawPayload>,
}

#[derive(Deserialize)]
struct RawPayload {
    #[serde(rename = "type")]
    payload_type: Option<String>,
    session_id: Option<String>,
    thread_id: Option<String>,
    response_id: Option<String>,
    usage: Option<Value>,
    info: Option<RawInfo>,
}

#[derive(Deserialize)]
struct RawInfo {
    total_token_usage: Option<Value>,
}

pub fn parse_line(line: &str) -> Result<Option<ParsedRecord>, ParseError> {
    let raw: RawLine = serde_json::from_str(line).map_err(|_| ParseError::InvalidJson)?;
    match raw.record_type.as_deref() {
        Some("token_usage_record") => {
            let payload = raw.payload.ok_or(ParseError::InvalidIdentity)?;
            let session = payload
                .session_id
                .or(payload.thread_id)
                .filter(|value| !value.is_empty())
                .ok_or(ParseError::InvalidIdentity)?;
            let response = payload
                .response_id
                .filter(|value| !value.is_empty())
                .ok_or(ParseError::InvalidIdentity)?;
            let timestamp = parse_timestamp(raw.timestamp.as_deref())?;
            Ok(Some(ParsedRecord {
                agent: Agent::Codex,
                kind: RecordKind::Response,
                event_key: format!("codex:response:{session}:{response}"),
                occurred_at_utc: timestamp,
                usage: parse_usage(payload.usage.as_ref()),
            }))
        }
        Some("event_msg")
            if raw
                .payload
                .as_ref()
                .and_then(|payload| payload.payload_type.as_deref())
                == Some("token_count") =>
        {
            let payload = raw.payload.ok_or(ParseError::InvalidIdentity)?;
            let timestamp_text = raw.timestamp.ok_or(ParseError::InvalidIdentity)?;
            let timestamp = parse_timestamp(Some(&timestamp_text))?;
            let usage = parse_usage(
                payload
                    .info
                    .as_ref()
                    .and_then(|info| info.total_token_usage.as_ref()),
            );
            let total = usage
                .total_tokens
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_owned());
            Ok(Some(ParsedRecord {
                agent: Agent::Codex,
                kind: RecordKind::CumulativeSnapshot,
                event_key: format!(
                    "codex:snapshot:{timestamp_text}:{}:{total}",
                    raw.ordinal
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "-".to_owned())
                ),
                occurred_at_utc: timestamp,
                usage,
            }))
        }
        _ => Ok(None),
    }
}

fn parse_usage(value: Option<&Value>) -> TokenUsage {
    let number = |name: &str| {
        value
            .and_then(|usage| usage.get(name))
            .and_then(Value::as_u64)
    };
    let total_tokens = number("total_tokens");
    TokenUsage {
        input_tokens: number("input_tokens"),
        output_tokens: number("output_tokens"),
        cache_read_tokens: number("cached_input_tokens"),
        cache_write_tokens: number("cache_write_input_tokens"),
        total_tokens,
        coverage: if total_tokens.is_some() {
            UsageCoverage::Complete
        } else if value.is_some() {
            UsageCoverage::Unsupported
        } else {
            UsageCoverage::Unavailable
        },
    }
}

#[cfg(test)]
mod tests {
    use super::parse_line;
    use crate::collectors::{ParseError, RecordKind};
    use crate::domain::usage::{Agent, UsageCoverage};

    const RESPONSE: &str = r#"{"timestamp":"2026-09-25T00:00:00Z","type":"token_usage_record","payload":{"session_id":"s1","response_id":"r1","usage":{"input_tokens":30,"cached_input_tokens":8,"cache_write_input_tokens":0,"output_tokens":12,"reasoning_output_tokens":4,"total_tokens":42}}}"#;

    #[test]
    fn response_total_is_counted_once_without_breakdown_additions() {
        let record = parse_line(RESPONSE).unwrap().unwrap();
        assert_eq!(record.agent, Agent::Codex);
        assert_eq!(record.kind, RecordKind::Response);
        assert_eq!(record.usage.total_tokens, Some(42));
        assert_eq!(record.usage.cache_read_tokens, Some(8));
        assert_eq!(record.usage.coverage, UsageCoverage::Complete);
    }

    #[test]
    fn same_response_identity_has_same_key_across_replays() {
        let first = parse_line(RESPONSE).unwrap().unwrap();
        let replay = parse_line(r#"{"timestamp":"2026-09-25T00:01:00Z","type":"token_usage_record","payload":{"session_id":"s1","response_id":"r1","usage":{"total_tokens":42}}}"#).unwrap().unwrap();
        let other_session = parse_line(r#"{"timestamp":"2026-09-25T00:01:00Z","type":"token_usage_record","payload":{"session_id":"s2","response_id":"r1","usage":{"total_tokens":42}}}"#).unwrap().unwrap();
        assert_eq!(first.event_key, replay.event_key);
        assert_ne!(first.event_key, other_session.event_key);
    }

    #[test]
    fn cumulative_event_is_marked_as_snapshot_for_delta_handling() {
        let record = parse_line(r#"{"timestamp":"2026-09-25T00:02:00Z","ordinal":4,"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":30,"output_tokens":12,"total_tokens":42}}}}"#).unwrap().unwrap();
        assert_eq!(record.kind, RecordKind::CumulativeSnapshot);
        assert_eq!(record.usage.total_tokens, Some(42));
    }

    #[test]
    fn unrelated_record_is_ignored() {
        assert!(parse_line(r#"{"timestamp":"2026-09-25T00:00:00Z","type":"event_msg","payload":{"type":"message"}}"#).unwrap().is_none());
    }

    #[test]
    fn response_without_authoritative_total_is_unsupported() {
        let record = parse_line(r#"{"timestamp":"2026-09-25T00:00:00Z","type":"token_usage_record","payload":{"session_id":"s1","response_id":"r2","usage":{"input_tokens":30,"output_tokens":12}}}"#).unwrap().unwrap();
        assert_eq!(record.usage.coverage, UsageCoverage::Unsupported);
        assert_eq!(record.usage.total_tokens, None);
    }

    #[test]
    fn malformed_row_error_does_not_include_record_text() {
        let error = parse_line("{private-sentinel").unwrap_err();
        assert_eq!(error, ParseError::InvalidJson);
        assert!(!format!("{error:?}").contains("private-sentinel"));
    }

    #[test]
    fn response_without_timestamp_is_rejected() {
        let error = parse_line(r#"{"type":"token_usage_record","payload":{"session_id":"s1","response_id":"r1","usage":{"total_tokens":42}}}"#).unwrap_err();
        assert_eq!(error, ParseError::InvalidIdentity);
    }
}
