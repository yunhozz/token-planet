pub mod claude_code;
pub mod codex;
pub mod discovery;

use chrono::{DateTime, Utc};
use std::fmt;

use crate::domain::usage::{Agent, TokenUsage};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordKind {
    Response,
    CumulativeSnapshot,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedRecord {
    pub agent: Agent,
    pub kind: RecordKind,
    pub event_key: String,
    pub occurred_at_utc: DateTime<Utc>,
    pub usage: TokenUsage,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseError {
    InvalidJson,
    InvalidIdentity,
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson => formatter.write_str("invalid JSONL record"),
            Self::InvalidIdentity => formatter.write_str("missing or invalid usage identity"),
        }
    }
}

impl std::error::Error for ParseError {}

fn parse_timestamp(value: Option<&str>) -> Result<DateTime<Utc>, ParseError> {
    let value = value.ok_or(ParseError::InvalidIdentity)?;
    DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|_| ParseError::InvalidIdentity)
}
