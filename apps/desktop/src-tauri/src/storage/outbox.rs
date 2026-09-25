use std::collections::BTreeMap;

use chrono_tz::Tz;
use rusqlite::{params, OptionalExtension};

use crate::collectors::discovery::SourceHealth;
use crate::domain::usage::{Agent, UsageCoverage};
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
    pub fn set_source_health(&self, agent: Agent, health: SourceHealth) -> Result<(), OutboxError> {
        let value = serde_json::to_string(&health).map_err(|_| OutboxError::InvalidPayload)?;
        self.set_setting(
            &format!("sharing_source_health:{}", agent_name(agent)),
            &value,
        )
    }

    fn source_health(&self, agent: Agent) -> Result<Option<SourceHealth>, OutboxError> {
        self.setting(&format!("sharing_source_health:{}", agent_name(agent)))?
            .map(|value| serde_json::from_str(&value).map_err(|_| OutboxError::InvalidPayload))
            .transpose()
    }

    fn setting(&self, key: &str) -> Result<Option<String>, OutboxError> {
        self.connection
            .query_row("SELECT value FROM setting WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()
            .map_err(Into::into)
    }

    fn set_setting(&self, key: &str, value: &str) -> Result<(), OutboxError> {
        self.connection.execute(
            "INSERT INTO setting(key,value) VALUES (?1,?2)
            ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    fn scope_setting_key(&self, prefix: &str) -> Result<String, OutboxError> {
        match (
            self.setting("sharing_world_id")?,
            self.setting("sharing_user_id")?,
        ) {
            (Some(world_id), Some(user_id)) => Ok(format!("{prefix}:{world_id}:{user_id}")),
            _ => Ok(prefix.to_owned()),
        }
    }

    pub fn cached_world_scope(&self) -> Result<Option<(String, String, Tz)>, OutboxError> {
        let Some(world_id) = self.setting("sharing_world_id")? else {
            return Ok(None);
        };
        let user_id = self
            .setting("sharing_user_id")?
            .ok_or(OutboxError::InvalidPayload)?;
        let timezone = self
            .setting("sharing_world_timezone")?
            .ok_or(OutboxError::InvalidPayload)?
            .parse()
            .map_err(|_| OutboxError::InvalidPayload)?;
        Ok(Some((world_id, user_id, timezone)))
    }

    pub fn cached_world_view(&self) -> Result<Option<String>, OutboxError> {
        self.setting("sharing_cached_world")
    }

    pub fn set_cached_world_view(&self, value: &str) -> Result<(), OutboxError> {
        self.set_setting("sharing_cached_world", value)
    }

    pub fn clear_cached_world_view(&self) -> Result<(), OutboxError> {
        self.connection
            .execute("DELETE FROM setting WHERE key='sharing_cached_world'", [])?;
        Ok(())
    }

    pub fn ensure_world_scope(
        &mut self,
        world_id: &str,
        user_id: &str,
        timezone: Tz,
    ) -> Result<(), OutboxError> {
        if let (Some(old_world), Some(old_user)) = (
            self.setting("sharing_world_id")?,
            self.setting("sharing_user_id")?,
        ) {
            for prefix in [
                "sharing_device_id",
                "sharing_skip_through",
                "sharing_paused",
            ] {
                let scoped = format!("{prefix}:{old_world}:{old_user}");
                if self.setting(&scoped)?.is_none() {
                    if let Some(value) = self.setting(prefix)? {
                        self.set_setting(&scoped, &value)?;
                    }
                }
                self.connection
                    .execute("DELETE FROM setting WHERE key=?1", [prefix])?;
            }
        }
        if self.setting("sharing_world_id")?.as_deref() != Some(world_id)
            || self.setting("sharing_user_id")?.as_deref() != Some(user_id)
        {
            self.clear_cached_world_view()?;
        }
        self.set_setting("sharing_world_id", world_id)?;
        self.set_setting("sharing_user_id", user_id)?;
        self.set_setting("sharing_world_timezone", &timezone.to_string())?;
        let device_key = self.scope_setting_key("sharing_device_id")?;
        let device_id = match self.setting(&device_key)? {
            Some(id) => id,
            None => {
                let id = uuid::Uuid::new_v4().to_string();
                self.set_setting(&device_key, &id)?;
                id
            }
        };
        self.set_setting("sharing_active_device_id", &device_id)?;
        Ok(())
    }

    pub fn clear_sharing_scope(&mut self) -> Result<(), OutboxError> {
        self.clear_outbox()?;
        self.connection.execute("DELETE FROM setting WHERE key IN ('sharing_world_id','sharing_user_id','sharing_world_timezone','sharing_active_device_id','sharing_cached_world')", [])?;
        Ok(())
    }

    pub fn stop_sharing_through(&mut self, date: &str) -> Result<(), OutboxError> {
        if chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
            return Err(OutboxError::InvalidPayload);
        }
        self.set_sharing_paused(true)?;
        let key = self.scope_setting_key("sharing_skip_through")?;
        if self
            .setting(&key)?
            .as_deref()
            .is_none_or(|saved| saved < date)
        {
            self.set_setting(&key, date)?;
        }
        self.clear_outbox()
    }

    pub fn adopt_remote_deletion(&mut self, date: &str, paused: bool) -> Result<(), OutboxError> {
        if chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
            return Err(OutboxError::InvalidPayload);
        }
        let key = self.scope_setting_key("sharing_skip_through")?;
        if self
            .setting(&key)?
            .as_deref()
            .is_none_or(|saved| saved < date)
        {
            self.set_setting(&key, date)?;
            self.clear_outbox()?;
        }
        if paused {
            self.set_sharing_paused(true)?;
        }
        Ok(())
    }

    pub fn prepare_shared_snapshots(
        &mut self,
        world_id: &str,
        user_id: &str,
        timezone: Tz,
    ) -> Result<(), OutboxError> {
        self.ensure_world_scope(world_id, user_id, timezone)?;
        let device_id = self
            .setting("sharing_active_device_id")?
            .ok_or(OutboxError::InvalidPayload)?;
        let mut dates: BTreeMap<
            (String, String),
            Option<crate::storage::ledger::SharedDailyTotal>,
        > = BTreeMap::new();
        for total in self
            .shared_daily_totals(timezone)
            .map_err(|_| OutboxError::Database)?
        {
            dates.insert(
                (total.bucket_date.clone(), agent_name(total.agent).into()),
                Some(total),
            );
        }
        let existing: Vec<DailyUsageSnapshot> = {
            let mut statement = self
                .connection
                .prepare("SELECT payload_json FROM outbox_snapshot WHERE device_id=?1")?;
            let snapshots = statement
                .query_map([&device_id], |row| row.get::<_, String>(0))?
                .map(|row| serde_json::from_str(&row?).map_err(|_| OutboxError::InvalidPayload))
                .collect::<Result<_, _>>()?;
            snapshots
        };
        let mut old_by_key = BTreeMap::new();
        for snapshot in existing {
            dates
                .entry((
                    snapshot.bucket_date.clone(),
                    agent_name(snapshot.agent).into(),
                ))
                .or_insert(None);
            old_by_key.insert(
                (
                    snapshot.bucket_date.clone(),
                    agent_name(snapshot.agent).to_owned(),
                ),
                snapshot,
            );
        }
        let today = chrono::Utc::now()
            .with_timezone(&timezone)
            .format("%Y-%m-%d")
            .to_string();
        for agent in [Agent::Codex, Agent::ClaudeCode] {
            dates
                .entry((today.clone(), agent_name(agent).into()))
                .or_insert(None);
        }
        let cutoff = self.setting(&self.scope_setting_key("sharing_skip_through")?)?;
        for ((date, agent_name), total) in dates {
            if cutoff.as_ref().is_some_and(|cutoff| date <= *cutoff) {
                continue;
            }
            let agent = if agent_name == "codex" {
                Agent::Codex
            } else {
                Agent::ClaudeCode
            };
            let enabled = self
                .agent_enabled(agent)
                .map_err(|_| OutboxError::Database)?;
            let (
                input_tokens,
                output_tokens,
                cache_read_tokens,
                cache_write_tokens,
                total_tokens,
                coverage,
            ) = if !enabled {
                (None, None, None, None, None, UsageCoverage::UserDisabled)
            } else if let Some(total) = total {
                (
                    total.input_tokens,
                    total.output_tokens,
                    total.cache_read_tokens,
                    total.cache_write_tokens,
                    total.total_tokens,
                    total.coverage,
                )
            } else {
                (None, None, None, None, None, UsageCoverage::Unavailable)
            };
            let coverage = if date == today && enabled {
                match self.source_health(agent)? {
                    Some(SourceHealth::Ready) | None => coverage,
                    Some(_) if total_tokens.is_some() => UsageCoverage::Partial,
                    Some(SourceHealth::UnsupportedFormat) => UsageCoverage::Unsupported,
                    Some(_) => UsageCoverage::Unavailable,
                }
            } else {
                coverage
            };
            let key = (date.clone(), agent_name);
            let old = old_by_key.get(&key);
            let revision = old.map_or(1, |old| old.revision);
            let mut candidate = DailyUsageSnapshot {
                device_id: device_id.clone(),
                bucket_date: date,
                bucket_policy_version: 1,
                agent,
                schema_version: 1,
                revision,
                input_tokens,
                output_tokens,
                cache_read_tokens,
                cache_write_tokens,
                total_tokens,
                coverage,
                payload_hash: String::new(),
            }
            .seal();
            if old.is_some_and(|old| old == &candidate) {
                continue;
            }
            if old.is_some() {
                candidate.revision = revision.checked_add(1).ok_or(OutboxError::InvalidPayload)?;
                candidate = candidate.seal();
            }
            self.queue_snapshot(&candidate)?;
        }
        Ok(())
    }
    pub fn sharing_paused(&self) -> Result<bool, OutboxError> {
        let value = self.setting(&self.scope_setting_key("sharing_paused")?)?;
        Ok(value.as_deref() == Some("true"))
    }

    pub fn set_sharing_paused(&mut self, paused: bool) -> Result<(), OutboxError> {
        self.set_setting(
            &self.scope_setting_key("sharing_paused")?,
            if paused { "true" } else { "false" },
        )
    }

    pub fn pending_snapshot_count(&self) -> Result<u64, OutboxError> {
        let active = self.setting("sharing_active_device_id")?;
        let count: i64 = self.connection.query_row(
            "SELECT count(*) FROM outbox_snapshot WHERE revision>acknowledged_revision
             AND (?1 IS NULL OR device_id=?1)",
            [active],
            |row| row.get(0),
        )?;
        u64::try_from(count).map_err(|_| OutboxError::Database)
    }

    pub fn clear_outbox(&mut self) -> Result<(), OutboxError> {
        if let Some(active) = self.setting("sharing_active_device_id")? {
            self.connection
                .execute("DELETE FROM outbox_snapshot WHERE device_id=?1", [active])?;
        } else {
            self.connection.execute("DELETE FROM outbox_snapshot", [])?;
        }
        Ok(())
    }

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
        let active = self.setting("sharing_active_device_id")?;
        let mut statement = self.connection.prepare(
            "SELECT payload_json FROM outbox_snapshot WHERE revision>acknowledged_revision
             AND (?1 IS NULL OR device_id=?1) ORDER BY bucket_date,device_id,agent",
        )?;
        let rows = statement.query_map([active], |row| row.get::<_, String>(0))?;
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

    #[test]
    fn pausing_sync_keeps_pending_local_aggregates() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), UTC).unwrap();
        ledger.queue_snapshot(&snapshot(1, Some(42))).unwrap();
        ledger.set_sharing_paused(true).unwrap();
        assert!(ledger.sharing_paused().unwrap());
        assert_eq!(ledger.pending_snapshot_count().unwrap(), 1);
        ledger.set_sharing_paused(false).unwrap();
        assert!(!ledger.sharing_paused().unwrap());
        assert_eq!(ledger.pending_snapshots().unwrap().len(), 1);
    }

    #[test]
    fn preparing_changed_history_increments_revision_without_duplicate_queue_rows() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), UTC).unwrap();
        let first = crate::collectors::codex::parse_line(r#"{"timestamp":"2026-09-24T15:30:00Z","type":"token_usage_record","payload":{"session_id":"s1","response_id":"r1","usage":{"total_tokens":42}}}"#).unwrap().unwrap();
        ledger.insert(&first).unwrap();
        ledger
            .prepare_shared_snapshots("world-1", "member-1", UTC)
            .unwrap();
        let first_snapshot = ledger
            .pending_snapshots()
            .unwrap()
            .into_iter()
            .find(|snapshot| snapshot.agent == Agent::Codex && snapshot.bucket_date == "2026-09-24")
            .unwrap();
        assert_eq!(first_snapshot.revision, 1);
        ledger
            .prepare_shared_snapshots("world-1", "member-1", UTC)
            .unwrap();
        let unchanged = ledger
            .pending_snapshots()
            .unwrap()
            .into_iter()
            .find(|snapshot| snapshot.agent == Agent::Codex && snapshot.bucket_date == "2026-09-24")
            .unwrap();
        assert_eq!(unchanged.revision, 1);
        let second = crate::collectors::codex::parse_line(r#"{"timestamp":"2026-09-24T16:30:00Z","type":"token_usage_record","payload":{"session_id":"s1","response_id":"r2","usage":{"total_tokens":8}}}"#).unwrap().unwrap();
        ledger.insert(&second).unwrap();
        ledger
            .prepare_shared_snapshots("world-1", "member-1", UTC)
            .unwrap();
        let changed = ledger
            .pending_snapshots()
            .unwrap()
            .into_iter()
            .find(|snapshot| snapshot.agent == Agent::Codex && snapshot.bucket_date == "2026-09-24")
            .unwrap();
        assert_eq!(changed.revision, 2);
        assert_eq!(changed.total_tokens, Some(50));
        assert_eq!(changed.device_id, first_snapshot.device_id);
    }

    #[test]
    fn deletion_cutoff_prevents_old_history_from_reappearing_after_resume() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), UTC).unwrap();
        let old = crate::collectors::codex::parse_line(r#"{"timestamp":"2026-09-24T15:30:00Z","type":"token_usage_record","payload":{"session_id":"s1","response_id":"r1","usage":{"total_tokens":42}}}"#).unwrap().unwrap();
        ledger.insert(&old).unwrap();
        ledger
            .prepare_shared_snapshots("world-1", "member-1", UTC)
            .unwrap();
        assert!(ledger
            .pending_snapshots()
            .unwrap()
            .iter()
            .any(|snapshot| snapshot.bucket_date == "2026-09-24"
                && snapshot.total_tokens == Some(42)));
        ledger.stop_sharing_through("2026-09-25").unwrap();
        ledger.set_sharing_paused(false).unwrap();
        ledger
            .prepare_shared_snapshots("world-1", "member-1", UTC)
            .unwrap();
        assert!(ledger
            .pending_snapshots()
            .unwrap()
            .iter()
            .all(|snapshot| snapshot.bucket_date.as_str() > "2026-09-25"));
    }

    #[test]
    fn leaving_and_rejoining_does_not_reupload_old_history() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), UTC).unwrap();
        let old = crate::collectors::codex::parse_line(r#"{"timestamp":"2026-09-24T15:30:00Z","type":"token_usage_record","payload":{"session_id":"s1","response_id":"r1","usage":{"total_tokens":42}}}"#).unwrap().unwrap();
        ledger.insert(&old).unwrap();
        ledger
            .prepare_shared_snapshots("world-1", "member-1", UTC)
            .unwrap();
        ledger.stop_sharing_through("2026-09-25").unwrap();
        ledger.clear_sharing_scope().unwrap();
        ledger
            .prepare_shared_snapshots("world-1", "member-1", UTC)
            .unwrap();
        assert!(ledger.sharing_paused().unwrap());
        assert!(ledger
            .pending_snapshots()
            .unwrap()
            .iter()
            .all(|snapshot| snapshot.bucket_date.as_str() > "2026-09-25"));
    }

    #[test]
    fn same_account_reuses_revision_but_another_account_starts_fresh() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), UTC).unwrap();
        ledger
            .prepare_shared_snapshots("world-1", "member-1", UTC)
            .unwrap();
        let original = ledger.pending_snapshots().unwrap();
        ledger
            .acknowledge_snapshot(
                &original[0].device_id,
                &original[0].bucket_date,
                original[0].agent,
                1,
            )
            .unwrap();
        ledger.clear_cached_world_view().unwrap();
        ledger
            .prepare_shared_snapshots("world-1", "member-1", UTC)
            .unwrap();
        assert_eq!(
            ledger
                .next_snapshot_revision(
                    &original[0].device_id,
                    &original[0].bucket_date,
                    original[0].agent
                )
                .unwrap(),
            2
        );
        ledger
            .prepare_shared_snapshots("world-1", "member-2", UTC)
            .unwrap();
        let other_account = ledger.pending_snapshots().unwrap();
        assert_ne!(other_account[0].device_id, original[0].device_id);
        assert_eq!(ledger.pending_snapshot_count().unwrap(), 2);
        ledger
            .prepare_shared_snapshots("world-1", "member-1", UTC)
            .unwrap();
        assert_eq!(
            ledger
                .next_snapshot_revision(
                    &original[0].device_id,
                    &original[0].bucket_date,
                    original[0].agent
                )
                .unwrap(),
            2
        );
        assert_eq!(ledger.pending_snapshot_count().unwrap(), 1);
    }

    #[test]
    fn paused_and_deleted_scope_survives_switching_accounts() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), UTC).unwrap();
        ledger
            .prepare_shared_snapshots("world-1", "member-1", UTC)
            .unwrap();
        ledger.stop_sharing_through("2026-09-25").unwrap();
        ledger
            .prepare_shared_snapshots("world-1", "member-2", UTC)
            .unwrap();
        assert!(!ledger.sharing_paused().unwrap());
        ledger
            .prepare_shared_snapshots("world-1", "member-1", UTC)
            .unwrap();
        assert!(ledger.sharing_paused().unwrap());
        assert!(ledger
            .pending_snapshots()
            .unwrap()
            .iter()
            .all(|snapshot| snapshot.bucket_date.as_str() > "2026-09-25"));
    }

    #[test]
    fn remote_deletion_pauses_this_installation_without_affecting_other_account() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), UTC).unwrap();
        ledger
            .prepare_shared_snapshots("world-1", "member-1", UTC)
            .unwrap();
        let cutoff = chrono::Utc::now().format("%Y-%m-%d").to_string();
        ledger.adopt_remote_deletion(&cutoff, true).unwrap();
        assert!(ledger.sharing_paused().unwrap());
        assert_eq!(ledger.pending_snapshot_count().unwrap(), 0);
        ledger
            .prepare_shared_snapshots("world-1", "member-1", UTC)
            .unwrap();
        assert_eq!(ledger.pending_snapshot_count().unwrap(), 0);
        ledger
            .prepare_shared_snapshots("world-1", "member-2", UTC)
            .unwrap();
        assert!(!ledger.sharing_paused().unwrap());
    }

    #[test]
    fn remote_deletion_after_account_switch_preserves_previous_accounts_queue() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), UTC).unwrap();
        ledger
            .prepare_shared_snapshots("world-1", "member-1", UTC)
            .unwrap();
        ledger
            .prepare_shared_snapshots("world-1", "member-2", UTC)
            .unwrap();
        assert_eq!(ledger.pending_snapshot_count().unwrap(), 2);
        ledger
            .ensure_world_scope("world-1", "member-1", UTC)
            .unwrap();
        let cutoff = chrono::Utc::now().format("%Y-%m-%d").to_string();
        ledger.adopt_remote_deletion(&cutoff, true).unwrap();
        ledger
            .ensure_world_scope("world-1", "member-2", UTC)
            .unwrap();
        assert_eq!(ledger.pending_snapshot_count().unwrap(), 2);
    }

    #[test]
    fn old_unscoped_deletion_settings_migrate_to_the_original_account() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), UTC).unwrap();
        ledger.set_setting("sharing_world_id", "world-1").unwrap();
        ledger.set_setting("sharing_user_id", "member-1").unwrap();
        ledger
            .set_setting("sharing_device_id", "old-device")
            .unwrap();
        ledger
            .set_setting("sharing_skip_through", "2026-09-25")
            .unwrap();
        ledger.set_setting("sharing_paused", "true").unwrap();
        ledger
            .ensure_world_scope("world-1", "member-2", UTC)
            .unwrap();
        assert!(!ledger.sharing_paused().unwrap());
        ledger
            .ensure_world_scope("world-1", "member-1", UTC)
            .unwrap();
        assert!(ledger.sharing_paused().unwrap());
        assert_eq!(
            ledger
                .setting("sharing_active_device_id")
                .unwrap()
                .as_deref(),
            Some("old-device")
        );
        ledger
            .prepare_shared_snapshots("world-1", "member-1", UTC)
            .unwrap();
        assert!(ledger
            .pending_snapshots()
            .unwrap()
            .iter()
            .all(|snapshot| snapshot.bucket_date.as_str() > "2026-09-25"));
    }

    #[test]
    fn disabled_agent_replaces_its_prior_total_with_unknown_status() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), UTC).unwrap();
        let record = crate::collectors::codex::parse_line(r#"{"timestamp":"2026-09-24T15:30:00Z","type":"token_usage_record","payload":{"session_id":"s1","response_id":"r1","usage":{"total_tokens":42}}}"#).unwrap().unwrap();
        ledger.insert(&record).unwrap();
        ledger
            .prepare_shared_snapshots("world-1", "member-1", UTC)
            .unwrap();
        ledger.set_agent_enabled(Agent::Codex, false).unwrap();
        ledger
            .prepare_shared_snapshots("world-1", "member-1", UTC)
            .unwrap();
        let changed = ledger
            .pending_snapshots()
            .unwrap()
            .into_iter()
            .find(|snapshot| snapshot.agent == Agent::Codex && snapshot.bucket_date == "2026-09-24")
            .unwrap();
        assert_eq!(changed.revision, 2);
        assert_eq!(changed.total_tokens, None);
        assert_eq!(changed.coverage, UsageCoverage::UserDisabled);
    }

    #[test]
    fn scan_failure_marks_current_shared_total_incomplete() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), UTC).unwrap();
        let now = chrono::Utc::now();
        let line = serde_json::json!({"timestamp": now.to_rfc3339(), "type":"token_usage_record", "payload":{"session_id":"s1","response_id":"r1","usage":{"total_tokens":42}}}).to_string();
        let record = crate::collectors::codex::parse_line(&line)
            .unwrap()
            .unwrap();
        ledger.insert(&record).unwrap();
        ledger
            .set_source_health(
                Agent::Codex,
                crate::collectors::discovery::SourceHealth::UnsupportedFormat,
            )
            .unwrap();
        ledger
            .prepare_shared_snapshots("world-1", "member-1", UTC)
            .unwrap();
        let snapshot = ledger
            .pending_snapshots()
            .unwrap()
            .into_iter()
            .find(|item| {
                item.agent == Agent::Codex && item.bucket_date == now.format("%Y-%m-%d").to_string()
            })
            .unwrap();
        assert_eq!(snapshot.total_tokens, Some(42));
        assert_eq!(snapshot.coverage, UsageCoverage::Partial);
    }
}
