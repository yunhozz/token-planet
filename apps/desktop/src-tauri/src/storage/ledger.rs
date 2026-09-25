use std::{collections::BTreeMap, fmt, path::Path};

use chrono_tz::Tz;
use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::collectors::{ParsedRecord, RecordKind};
use crate::domain::usage::{Agent, TokenUsage, UsageCoverage};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScanError {
    Database,
    SourceIo,
    SourcePermission,
    InvalidCount,
    TimezoneMismatch,
}

impl fmt::Display for ScanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Database => "local database error",
                Self::SourceIo => "source file unavailable",
                Self::SourcePermission => "source file permission denied",
                Self::InvalidCount => "token count exceeds local storage range",
                Self::TimezoneMismatch => "world timezone differs from saved ledger",
            }
        )
    }
}

impl std::error::Error for ScanError {}

impl From<rusqlite::Error> for ScanError {
    fn from(_: rusqlite::Error) -> Self {
        Self::Database
    }
}

pub struct Ledger {
    pub(crate) connection: Connection,
    pub timezone: Tz,
}

impl Ledger {
    pub fn saved_timezone(path: &Path) -> Result<Option<Tz>, ScanError> {
        if !path.exists() {
            return Ok(None);
        }
        let connection = Connection::open(path)?;
        let saved: Option<String> = connection
            .query_row(
                "SELECT value FROM setting WHERE key='world_timezone'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        saved
            .map(|value| value.parse().map_err(|_| ScanError::TimezoneMismatch))
            .transpose()
    }

    pub fn open(path: &Path, timezone: Tz) -> Result<Self, ScanError> {
        let connection = Connection::open(path)?;
        connection.execute_batch(
            "\
            PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS setting (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS source_checkpoint (
                source_id TEXT PRIMARY KEY, file_fingerprint TEXT NOT NULL,
                byte_offset INTEGER NOT NULL, parser_version INTEGER NOT NULL,
                last_snapshot_total INTEGER, status TEXT NOT NULL DEFAULT 'complete'
            );
            CREATE TABLE IF NOT EXISTS usage_record (
                event_key TEXT PRIMARY KEY, source_id TEXT NOT NULL,
                agent TEXT NOT NULL, kind TEXT NOT NULL,
                bucket_date TEXT NOT NULL, occurred_at_utc TEXT NOT NULL,
                input_tokens INTEGER, output_tokens INTEGER,
                cache_read_tokens INTEGER, cache_write_tokens INTEGER,
                total_tokens INTEGER, coverage TEXT NOT NULL, parser_version INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS daily_agent_total (
                agent TEXT NOT NULL, bucket_date TEXT NOT NULL,
                total_tokens INTEGER, coverage TEXT NOT NULL,
                PRIMARY KEY (agent, bucket_date)
            );
            CREATE TABLE IF NOT EXISTS outbox_snapshot (
                device_id TEXT NOT NULL, bucket_date TEXT NOT NULL, agent TEXT NOT NULL,
                revision INTEGER NOT NULL, acknowledged_revision INTEGER NOT NULL DEFAULT 0,
                payload_hash TEXT NOT NULL, payload_json TEXT NOT NULL,
                PRIMARY KEY (device_id, bucket_date, agent)
            );",
        )?;
        let saved: Option<String> = connection
            .query_row(
                "SELECT value FROM setting WHERE key='world_timezone'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(saved) = saved {
            if saved != timezone.to_string() {
                return Err(ScanError::TimezoneMismatch);
            }
        } else {
            connection.execute(
                "INSERT INTO setting(key,value) VALUES ('world_timezone',?1)",
                [timezone.to_string()],
            )?;
        }
        Ok(Self {
            connection,
            timezone,
        })
    }

    pub fn insert(&mut self, record: &ParsedRecord) -> Result<(), ScanError> {
        let timezone = self.timezone;
        let tx = self.connection.transaction()?;
        insert_record(&tx, record, "", timezone)?;
        rebuild_daily(&tx)?;
        tx.commit()?;
        Ok(())
    }

    pub fn daily_total(&self, agent: Agent, bucket_date: &str) -> Result<Option<u64>, ScanError> {
        let value: Option<Option<i64>> = self
            .connection
            .query_row(
                "SELECT total_tokens FROM daily_agent_total WHERE agent=?1 AND bucket_date=?2",
                params![agent_name(agent), bucket_date],
                |row| row.get(0),
            )
            .optional()?;
        value.flatten().map(as_u64).transpose()
    }

    pub fn custom_root(&self, agent: Agent) -> Result<Option<std::path::PathBuf>, ScanError> {
        let key = format!("{}_custom_root", agent_name(agent));
        let saved: Option<String> = self
            .connection
            .query_row("SELECT value FROM setting WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(saved.map(std::path::PathBuf::from))
    }

    pub fn set_custom_root(&mut self, agent: Agent, folder: &Path) -> Result<(), ScanError> {
        let key = format!("{}_custom_root", agent_name(agent));
        self.connection.execute(
            "INSERT INTO setting(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, folder.to_string_lossy().as_ref()],
        )?;
        Ok(())
    }

    pub fn ensure_source_root(&mut self, agent: Agent, root_key: &str) -> Result<(), ScanError> {
        let key = format!("{}_active_root", agent_name(agent));
        let saved: Option<String> = self
            .connection
            .query_row("SELECT value FROM setting WHERE key=?1", [&key], |row| {
                row.get(0)
            })
            .optional()?;
        if saved.as_deref() == Some(root_key) {
            return Ok(());
        }
        let tx = self.connection.transaction()?;
        if saved.is_some() {
            tx.execute(
                "DELETE FROM usage_record WHERE agent=?1",
                [agent_name(agent)],
            )?;
            tx.execute(
                "DELETE FROM source_checkpoint WHERE source_id LIKE ?1",
                [format!("{}:%", agent_name(agent))],
            )?;
            rebuild_daily(&tx)?;
        }
        tx.execute(
            "INSERT INTO setting(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, root_key],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn agent_enabled(&self, agent: Agent) -> Result<bool, ScanError> {
        let key = format!("{}_enabled", agent_name(agent));
        let saved: Option<String> = self
            .connection
            .query_row("SELECT value FROM setting WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(saved.as_deref() != Some("false"))
    }

    pub fn set_agent_enabled(&mut self, agent: Agent, enabled: bool) -> Result<(), ScanError> {
        let key = format!("{}_enabled", agent_name(agent));
        self.connection.execute(
            "INSERT INTO setting(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, if enabled { "true" } else { "false" }],
        )?;
        Ok(())
    }

    pub fn daily_known_totals(&self) -> Result<Vec<u64>, ScanError> {
        let mut statement = self.connection.prepare(
            "SELECT SUM(total_tokens) FROM daily_agent_total WHERE total_tokens IS NOT NULL
             AND ((agent='codex' AND ?1) OR (agent='claude_code' AND ?2))
             GROUP BY bucket_date ORDER BY bucket_date",
        )?;
        let rows = statement.query_map(
            params![
                self.agent_enabled(Agent::Codex)?,
                self.agent_enabled(Agent::ClaudeCode)?
            ],
            |row| row.get::<_, i64>(0),
        )?;
        rows.map(|row| as_u64(row?)).collect()
    }

    pub fn all_time_usage(&self, agent: Agent) -> Result<TokenUsage, ScanError> {
        let mut statement = self
            .connection
            .prepare("SELECT total_tokens, coverage FROM daily_agent_total WHERE agent=?1")?;
        let rows = statement.query_map([agent_name(agent)], |row| {
            Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut known: Option<u64> = None;
        let mut incomplete = false;
        let mut unsupported = false;
        for row in rows {
            let (total, coverage) = row?;
            if let Some(total) = total {
                let total = as_u64(total)?;
                known = Some(
                    known
                        .unwrap_or(0)
                        .checked_add(total)
                        .ok_or(ScanError::InvalidCount)?,
                );
            }
            if coverage != "complete" {
                incomplete = true;
            }
            if coverage == "unsupported" {
                unsupported = true;
            }
        }
        let coverage = match (known, incomplete, unsupported) {
            (Some(_), true, _) => UsageCoverage::Partial,
            (Some(_), false, _) => UsageCoverage::Complete,
            (None, _, true) => UsageCoverage::Unsupported,
            (None, _, _) => UsageCoverage::Unavailable,
        };
        Ok(TokenUsage {
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            total_tokens: known,
            coverage,
        })
    }
}

pub(crate) fn agent_name(agent: Agent) -> &'static str {
    match agent {
        Agent::Codex => "codex",
        Agent::ClaudeCode => "claude_code",
    }
}

fn coverage_name(coverage: UsageCoverage) -> &'static str {
    match coverage {
        UsageCoverage::Complete => "complete",
        UsageCoverage::Partial => "partial",
        UsageCoverage::Unavailable => "unavailable",
        UsageCoverage::Unsupported => "unsupported",
        UsageCoverage::UserDisabled => "user_disabled",
    }
}

fn as_i64(value: u64) -> Result<i64, ScanError> {
    i64::try_from(value).map_err(|_| ScanError::InvalidCount)
}
fn as_u64(value: i64) -> Result<u64, ScanError> {
    u64::try_from(value).map_err(|_| ScanError::InvalidCount)
}
fn optional_i64(value: Option<u64>) -> Result<Option<i64>, ScanError> {
    value.map(as_i64).transpose()
}

pub(crate) fn insert_record(
    tx: &Transaction<'_>,
    record: &ParsedRecord,
    source_id: &str,
    timezone: Tz,
) -> Result<(), ScanError> {
    let date = record
        .occurred_at_utc
        .with_timezone(&timezone)
        .format("%Y-%m-%d")
        .to_string();
    let kind = match record.kind {
        RecordKind::Response => "response",
        RecordKind::CumulativeSnapshot => "snapshot",
    };
    let version = match record.agent {
        Agent::Codex => crate::collectors::codex::PARSER_VERSION,
        Agent::ClaudeCode => crate::collectors::claude_code::PARSER_VERSION,
    };
    let event_key = if record.kind == RecordKind::CumulativeSnapshot {
        format!("{source_id}:{}", record.event_key)
    } else {
        record.event_key.clone()
    };
    tx.execute("INSERT INTO usage_record (
        event_key,source_id,agent,kind,bucket_date,occurred_at_utc,
        input_tokens,output_tokens,cache_read_tokens,cache_write_tokens,total_tokens,coverage,parser_version
    ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
    ON CONFLICT(event_key) DO UPDATE SET
      input_tokens=excluded.input_tokens,output_tokens=excluded.output_tokens,
      cache_read_tokens=excluded.cache_read_tokens,cache_write_tokens=excluded.cache_write_tokens,
      total_tokens=excluded.total_tokens,coverage=excluded.coverage,
      parser_version=excluded.parser_version
    WHERE usage_record.total_tokens IS NULL AND excluded.total_tokens IS NOT NULL", params![
        event_key, source_id, agent_name(record.agent), kind, date, record.occurred_at_utc.to_rfc3339(),
        optional_i64(record.usage.input_tokens)?, optional_i64(record.usage.output_tokens)?,
        optional_i64(record.usage.cache_read_tokens)?, optional_i64(record.usage.cache_write_tokens)?,
        optional_i64(record.usage.total_tokens)?, coverage_name(record.usage.coverage), version,
    ])?;
    Ok(())
}

pub(crate) fn rebuild_daily(tx: &Transaction<'_>) -> Result<(), ScanError> {
    let mut groups: BTreeMap<(String, String), (Option<u64>, bool, bool)> = BTreeMap::new();
    {
        let mut statement = tx.prepare("SELECT agent,bucket_date,total_tokens,coverage FROM usage_record r
            WHERE r.kind='response' OR NOT EXISTS (
                SELECT 1 FROM usage_record other WHERE other.source_id=r.source_id AND other.kind='response' AND other.agent=r.agent
            )")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        for row in rows {
            let (agent, date, total, coverage) = row?;
            let entry = groups.entry((agent, date)).or_insert((None, false, false));
            if let Some(total) = total {
                entry.0 = Some(
                    entry
                        .0
                        .unwrap_or(0)
                        .checked_add(as_u64(total)?)
                        .ok_or(ScanError::InvalidCount)?,
                );
            }
            if coverage != "complete" {
                entry.1 = true;
            }
            if coverage == "unsupported" {
                entry.2 = true;
            }
        }
    }
    tx.execute("DELETE FROM daily_agent_total", [])?;
    for ((agent, date), (known, incomplete, unsupported)) in groups {
        let coverage = if known.is_some() && incomplete {
            "partial"
        } else if known.is_some() {
            "complete"
        } else if unsupported {
            "unsupported"
        } else {
            "unavailable"
        };
        tx.execute("INSERT INTO daily_agent_total(agent,bucket_date,total_tokens,coverage) VALUES (?1,?2,?3,?4)",
            params![agent, date, optional_i64(known)?, coverage])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Ledger;
    use crate::collectors::codex;
    use crate::domain::usage::Agent;
    use chrono_tz::Asia::Seoul;

    const RESPONSE: &str = r#"{"timestamp":"2026-09-24T15:30:00Z","type":"token_usage_record","payload":{"session_id":"s1","response_id":"r1","usage":{"total_tokens":42}}}"#;

    #[test]
    fn later_complete_response_replaces_unavailable_same_identity() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), Seoul).unwrap();
        let unavailable = codex::parse_line(r#"{"timestamp":"2026-09-24T15:30:00Z","type":"token_usage_record","payload":{"session_id":"s1","response_id":"r1"}}"#).unwrap().unwrap();
        let complete = codex::parse_line(RESPONSE).unwrap().unwrap();
        ledger.insert(&unavailable).unwrap();
        ledger.insert(&complete).unwrap();
        assert_eq!(
            ledger.daily_total(Agent::Codex, "2026-09-25").unwrap(),
            Some(42)
        );
    }

    #[test]
    fn custom_source_root_is_local_and_survives_restart() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let folder = std::path::PathBuf::from("/example/private/sessions");
        {
            let mut ledger = Ledger::open(file.path(), Seoul).unwrap();
            ledger.set_custom_root(Agent::Codex, &folder).unwrap();
        }
        let ledger = Ledger::open(file.path(), Seoul).unwrap();
        assert_eq!(ledger.custom_root(Agent::Codex).unwrap(), Some(folder));
    }

    #[test]
    fn saved_world_timezone_survives_device_timezone_change() {
        let file = tempfile::NamedTempFile::new().unwrap();
        Ledger::open(file.path(), Seoul).unwrap();
        assert_eq!(Ledger::saved_timezone(file.path()).unwrap(), Some(Seoul));
    }

    #[test]
    fn repeated_record_and_reopened_ledger_keep_one_total_in_world_timezone() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let record = codex::parse_line(RESPONSE).unwrap().unwrap();
        {
            let mut ledger = Ledger::open(file.path(), Seoul).unwrap();
            ledger.insert(&record).unwrap();
            ledger.insert(&record).unwrap();
            assert_eq!(
                ledger.daily_total(Agent::Codex, "2026-09-25").unwrap(),
                Some(42)
            );
            assert_eq!(
                ledger.daily_total(Agent::Codex, "2026-09-24").unwrap(),
                None
            );
        }
        let ledger = Ledger::open(file.path(), Seoul).unwrap();
        assert_eq!(
            ledger.daily_total(Agent::Codex, "2026-09-25").unwrap(),
            Some(42)
        );
    }
}
