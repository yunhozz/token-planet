use rusqlite::{params, OptionalExtension};

use crate::domain::usage::Agent;
use crate::storage::ledger::{agent_name, Ledger};
use crate::sync::aggregate::DailyUsageSnapshot;

#[derive(Debug, Eq, PartialEq)]
pub enum OutboxError {
    Database,
    InvalidPayload,
    StaleRevision,
    RevisionConflict,
    UnexpectedAck,
}

impl From<rusqlite::Error> for OutboxError {
    fn from(_: rusqlite::Error) -> Self {
        Self::Database
    }
}

impl Ledger {
    pub fn queue_snapshot(&mut self, snapshot: &DailyUsageSnapshot) -> Result<(), OutboxError> {
        let revision = i64::try_from(snapshot.revision).map_err(|_| OutboxError::InvalidPayload)?;
        if revision < 1
            || snapshot.device_id.is_empty()
            || chrono::NaiveDate::parse_from_str(&snapshot.bucket_date, "%Y-%m-%d").is_err()
            || snapshot.payload_hash != snapshot.compute_hash()
        {
            return Err(OutboxError::InvalidPayload);
        }
        let saved: Option<(i64, String)> = self
            .connection
            .query_row(
                "SELECT revision,payload_hash FROM outbox_snapshot WHERE device_id=?1 AND bucket_date=?2 AND agent=?3",
                params![snapshot.device_id, snapshot.bucket_date, agent_name(snapshot.agent)],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((old_revision, old_hash)) = saved {
            if revision < old_revision {
                return Err(OutboxError::StaleRevision);
            }
            if revision == old_revision {
                return if snapshot.payload_hash == old_hash {
                    Ok(())
                } else {
                    Err(OutboxError::RevisionConflict)
                };
            }
        }
        let payload = serde_json::to_string(snapshot).map_err(|_| OutboxError::InvalidPayload)?;
        self.connection.execute(
            "INSERT INTO outbox_snapshot(device_id,bucket_date,agent,revision,payload_hash,payload_json)
             VALUES (?1,?2,?3,?4,?5,?6)
             ON CONFLICT(device_id,bucket_date,agent) DO UPDATE SET
             revision=excluded.revision,payload_hash=excluded.payload_hash,payload_json=excluded.payload_json",
            params![snapshot.device_id, snapshot.bucket_date, agent_name(snapshot.agent), revision, snapshot.payload_hash, payload],
        )?;
        Ok(())
    }

    pub fn pending_snapshots(&self) -> Result<Vec<DailyUsageSnapshot>, OutboxError> {
        let mut statement = self.connection.prepare(
            "SELECT payload_json FROM outbox_snapshot WHERE revision>acknowledged_revision ORDER BY bucket_date,device_id,agent",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| serde_json::from_str(&row?).map_err(|_| OutboxError::InvalidPayload))
            .collect()
    }

    pub fn acknowledge_snapshot(
        &mut self,
        device_id: &str,
        bucket_date: &str,
        agent: Agent,
        revision: u64,
    ) -> Result<(), OutboxError> {
        let revision = i64::try_from(revision).map_err(|_| OutboxError::UnexpectedAck)?;
        let local: Option<i64> = self
            .connection
            .query_row(
                "SELECT revision FROM outbox_snapshot WHERE device_id=?1 AND bucket_date=?2 AND agent=?3",
                params![device_id, bucket_date, agent_name(agent)],
                |row| row.get(0),
            )
            .optional()?;
        if local.is_none_or(|local| revision > local) {
            return Err(OutboxError::UnexpectedAck);
        }
        self.connection.execute(
            "UPDATE outbox_snapshot SET acknowledged_revision=MAX(acknowledged_revision,?4)
             WHERE device_id=?1 AND bucket_date=?2 AND agent=?3",
            params![device_id, bucket_date, agent_name(agent), revision],
        )?;
        Ok(())
    }

    pub fn next_snapshot_revision(
        &self,
        device_id: &str,
        bucket_date: &str,
        agent: Agent,
    ) -> Result<u64, OutboxError> {
        let revision: Option<i64> = self
            .connection
            .query_row(
                "SELECT revision FROM outbox_snapshot WHERE device_id=?1 AND bucket_date=?2 AND agent=?3",
                params![device_id, bucket_date, agent_name(agent)],
                |row| row.get(0),
            )
            .optional()?;
        u64::try_from(revision.unwrap_or(0))
            .ok()
            .and_then(|revision| revision.checked_add(1))
            .ok_or(OutboxError::InvalidPayload)
    }
}

#[cfg(test)]
mod tests {
    use super::OutboxError;
    use crate::domain::usage::{Agent, UsageCoverage};
    use crate::storage::ledger::Ledger;
    use crate::sync::aggregate::DailyUsageSnapshot;
    use chrono_tz::UTC;

    fn snapshot(revision: u64, total: Option<u64>) -> DailyUsageSnapshot {
        DailyUsageSnapshot {
            device_id: "device-1".into(),
            bucket_date: "2026-09-25".into(),
            bucket_policy_version: 1,
            agent: Agent::Codex,
            schema_version: 1,
            revision,
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            total_tokens: total,
            coverage: if total.is_some() {
                UsageCoverage::Complete
            } else {
                UsageCoverage::Unavailable
            },
            payload_hash: String::new(),
        }
        .seal()
    }

    #[test]
    fn latest_revision_replaces_pending_and_stale_ack_cannot_clear_it() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), UTC).unwrap();
        ledger.queue_snapshot(&snapshot(1, Some(42))).unwrap();
        ledger.queue_snapshot(&snapshot(1, Some(42))).unwrap();
        assert_eq!(
            ledger.pending_snapshots().unwrap(),
            vec![snapshot(1, Some(42))]
        );
        assert!(matches!(
            ledger.queue_snapshot(&snapshot(1, Some(43))),
            Err(OutboxError::RevisionConflict)
        ));
        ledger.queue_snapshot(&snapshot(2, Some(43))).unwrap();
        ledger
            .acknowledge_snapshot("device-1", "2026-09-25", Agent::Codex, 1)
            .unwrap();
        assert_eq!(
            ledger.pending_snapshots().unwrap(),
            vec![snapshot(2, Some(43))]
        );
        ledger
            .acknowledge_snapshot("device-1", "2026-09-25", Agent::Codex, 2)
            .unwrap();
        assert!(ledger.pending_snapshots().unwrap().is_empty());
        assert_eq!(
            ledger
                .next_snapshot_revision("device-1", "2026-09-25", Agent::Codex)
                .unwrap(),
            3
        );
    }

    #[test]
    fn pending_unknown_count_survives_restart_without_becoming_zero() {
        let file = tempfile::NamedTempFile::new().unwrap();
        {
            let mut ledger = Ledger::open(file.path(), UTC).unwrap();
            ledger.queue_snapshot(&snapshot(1, None)).unwrap();
        }
        let ledger = Ledger::open(file.path(), UTC).unwrap();
        let pending = ledger.pending_snapshots().unwrap();
        assert_eq!(pending[0].total_tokens, None);
        assert_eq!(pending[0].coverage, UsageCoverage::Unavailable);
    }
}
