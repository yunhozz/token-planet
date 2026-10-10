use std::collections::{BTreeMap, HashSet};

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::collectors::discovery::SourceHealth;
use crate::domain::growth_journal::{GrowthJournal, GrowthJournalCycle, GrowthJournalEntry};
use crate::domain::usage::{Agent, UsageCoverage};
use crate::storage::ledger::{agent_name, Ledger, ScanError};

#[derive(Default)]
struct DailyAgent {
    total: Option<u64>,
    incomplete: bool,
    unsupported: bool,
}

impl Ledger {
    pub(crate) fn initialize_growth_journal(&self) -> Result<(), ScanError> {
        self.connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS growth_journal_state (
                account_id TEXT PRIMARY KEY, generation INTEGER NOT NULL DEFAULT 0,
                deleted_at_utc TEXT
             );
             CREATE TABLE IF NOT EXISTS growth_journal_cycle (
                account_id TEXT NOT NULL, cycle_id TEXT NOT NULL,
                started_at_utc TEXT, ended_at_utc TEXT,
                wallet_credit INTEGER, wallet_credit_at_utc TEXT,
                PRIMARY KEY(account_id, cycle_id)
             );
             CREATE TABLE IF NOT EXISTS growth_journal_entry (
                account_id TEXT NOT NULL, device_id TEXT NOT NULL,
                cycle_id TEXT NOT NULL, bucket_date TEXT NOT NULL, agent TEXT NOT NULL,
                revision INTEGER NOT NULL, acknowledged_revision INTEGER NOT NULL DEFAULT 0,
                generation INTEGER NOT NULL, present INTEGER NOT NULL,
                confirmed_tokens INTEGER, coverage TEXT NOT NULL, payload_hash TEXT NOT NULL,
                PRIMARY KEY(account_id, device_id, cycle_id, bucket_date, agent)
             );
             CREATE TABLE IF NOT EXISTS growth_journal_remote_cycle (
                account_id TEXT NOT NULL, cycle_id TEXT NOT NULL,
                started_at_utc TEXT, ended_at_utc TEXT,
                wallet_credit INTEGER, wallet_credit_at_utc TEXT,
                PRIMARY KEY(account_id, cycle_id)
             );
             CREATE TABLE IF NOT EXISTS growth_journal_remote_entry (
                account_id TEXT NOT NULL, device_id TEXT NOT NULL,
                cycle_id TEXT NOT NULL, bucket_date TEXT NOT NULL, agent TEXT NOT NULL,
                revision INTEGER NOT NULL, generation INTEGER NOT NULL, present INTEGER NOT NULL,
                confirmed_tokens INTEGER, coverage TEXT NOT NULL, payload_hash TEXT NOT NULL,
                PRIMARY KEY(account_id, device_id, cycle_id, bucket_date, agent)
             );",
        )?;
        if super::device_reset::recovery_required(&self.connection) {
            return Ok(());
        }
        let account_id = self.current_planet_account_id()?;
        self.connection.execute(
            "INSERT OR IGNORE INTO growth_journal_state(account_id) VALUES (?1)",
            [account_id],
        )?;
        Ok(())
    }

    pub fn prepare_growth_journal(&mut self) -> Result<(), ScanError> {
        if !self.guest_import_game_mutations_allowed()? {
            return Ok(());
        }
        let data_version = self
            .connection
            .query_row("PRAGMA data_version", [], |row| row.get::<_, u64>(0))?;
        let signature = (self.connection.total_changes(), data_version);
        if self.growth_journal_signature == Some(signature) {
            return Ok(());
        }

        let transaction = self.connection.transaction()?;
        Self::prepare_growth_journal_in_transaction(&transaction)?;
        transaction.commit()?;

        let final_data_version = self
            .connection
            .query_row("PRAGMA data_version", [], |row| row.get::<_, u64>(0))?;
        if final_data_version == data_version {
            self.growth_journal_signature =
                Some((self.connection.total_changes(), final_data_version));
        }
        Ok(())
    }

    pub(crate) fn prepare_growth_journal_in_transaction(
        transaction: &Transaction<'_>,
    ) -> Result<(), ScanError> {
        let account_id =
            journal_setting(transaction, "planet_account_id")?.ok_or(ScanError::Database)?;
        let device_id =
            journal_setting(transaction, "planet_device_id")?.ok_or(ScanError::Database)?;
        transaction.execute(
            "INSERT OR IGNORE INTO growth_journal_state(account_id) VALUES (?1)",
            [&account_id],
        )?;
        let (generation, deleted_at) =
            Self::growth_journal_cursor_in_connection(transaction, &account_id)?;
        let deleted_at = deleted_at
            .map(|value| parse_timestamp(&value))
            .transpose()?;
        let cycles = Self::derived_growth_cycles_in_connection(
            transaction,
            &account_id,
            deleted_at.clone(),
        )?;
        for cycle in &cycles {
            transaction.execute(
                "INSERT INTO growth_journal_cycle(
                    account_id,cycle_id,started_at_utc,ended_at_utc,wallet_credit,wallet_credit_at_utc
                 ) VALUES (?1,?2,?3,?4,?5,?6)
                 ON CONFLICT(account_id,cycle_id) DO UPDATE SET
                    started_at_utc=excluded.started_at_utc,
                    ended_at_utc=coalesce(excluded.ended_at_utc,growth_journal_cycle.ended_at_utc),
                    wallet_credit=coalesce(excluded.wallet_credit,growth_journal_cycle.wallet_credit),
                    wallet_credit_at_utc=coalesce(excluded.wallet_credit_at_utc,growth_journal_cycle.wallet_credit_at_utc)",
                params![
                    account_id,
                    cycle.cycle_id,
                    cycle.started_at_utc,
                    cycle.ended_at_utc,
                    cycle.wallet_credit.map(as_i64).transpose()?,
                    cycle.wallet_credit_at_utc,
                ],
            )?;
        }

        let timezone: chrono_tz::Tz = journal_setting(transaction, "planet_timezone")?
            .ok_or(ScanError::Database)?
            .parse()
            .map_err(|_| ScanError::TimezoneMismatch)?;
        let mut statement = transaction.prepare(
            "SELECT r.agent,r.occurred_at_utc,r.total_tokens,r.coverage
             FROM usage_record r JOIN planet_usage_owner o ON o.event_key=r.event_key
             WHERE o.account_id=?1
               AND (r.kind='response' OR NOT EXISTS (
                 SELECT 1 FROM usage_record other
                 WHERE other.source_id=r.source_id AND other.kind='response' AND other.agent=r.agent
               ))
             ORDER BY r.occurred_at_utc",
        )?;
        let rows = statement.query_map([&account_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        let enabled = [
            (
                "codex",
                Self::agent_enabled_in_connection(transaction, Agent::Codex)?,
            ),
            (
                "claude_code",
                Self::agent_enabled_in_connection(transaction, Agent::ClaudeCode)?,
            ),
        ];
        let source_health = [
            (
                "codex",
                Self::journal_source_health_in_connection(transaction, Agent::Codex)?,
            ),
            (
                "claude_code",
                Self::journal_source_health_in_connection(transaction, Agent::ClaudeCode)?,
            ),
        ];
        let mut daily: BTreeMap<(String, String, String), DailyAgent> = BTreeMap::new();
        for row in rows {
            let (agent, occurred_at, total, coverage) = row?;
            let at = parse_timestamp(&occurred_at)?;
            if deleted_at
                .as_ref()
                .is_some_and(|cutoff| at <= cutoff.clone())
            {
                continue;
            }
            let mut selected_cycle = None;
            for cycle in &cycles {
                if contains(cycle, at)? {
                    selected_cycle = Some(cycle);
                    break;
                }
            }
            let Some(cycle) = selected_cycle else {
                continue;
            };
            let date = at.with_timezone(&timezone).format("%Y-%m-%d").to_string();
            let group = daily
                .entry((cycle.cycle_id.clone(), date, agent.clone()))
                .or_default();
            if !enabled_for(&agent, &enabled)? {
                continue;
            }
            if let Some(value) = total {
                let value = u64::try_from(value).map_err(|_| ScanError::InvalidCount)?;
                group.total = Some(
                    group
                        .total
                        .unwrap_or(0)
                        .checked_add(value)
                        .ok_or(ScanError::InvalidCount)?,
                );
            }
            group.incomplete |= coverage != "complete";
            group.unsupported |= coverage == "unsupported";
        }

        let mut active_keys = HashSet::new();
        for ((cycle_id, bucket_date, agent), aggregate) in daily {
            let parsed_agent = parse_agent(&agent)?;
            let agent_enabled = enabled_for(&agent, &enabled)?;
            let source_status = source_health
                .iter()
                .find(|(name, _)| *name == agent.as_str())
                .and_then(|(_, health)| *health);
            let source_incomplete =
                source_status.is_some_and(|health| health != SourceHealth::Ready);
            let coverage = if !agent_enabled {
                UsageCoverage::UserDisabled
            } else {
                match aggregate.total {
                    Some(_) if aggregate.incomplete || source_incomplete => UsageCoverage::Partial,
                    Some(_) => UsageCoverage::Complete,
                    None if aggregate.unsupported
                        || source_status == Some(SourceHealth::UnsupportedFormat) =>
                    {
                        UsageCoverage::Unsupported
                    }
                    None if aggregate.incomplete
                        || source_status == Some(SourceHealth::Partial) =>
                    {
                        UsageCoverage::Partial
                    }
                    None => UsageCoverage::Unavailable,
                }
            };
            let entry = GrowthJournalEntry {
                device_id: device_id.clone(),
                cycle_id: cycle_id.clone(),
                bucket_date: bucket_date.clone(),
                agent: parsed_agent,
                revision: 0,
                generation,
                present: true,
                confirmed_tokens: if agent_enabled { aggregate.total } else { None },
                coverage,
                payload_hash: String::new(),
            };
            active_keys.insert((cycle_id, bucket_date, agent));
            Self::upsert_local_growth_entry_in_connection(transaction, &account_id, entry)?;
        }

        let stale: Vec<(String, String, String)> = {
            let mut statement = transaction.prepare(
                "SELECT cycle_id,bucket_date,agent FROM growth_journal_entry
                 WHERE account_id=?1 AND device_id=?2 AND generation=?3 AND present=1",
            )?;
            let rows = statement
                .query_map(params![account_id, device_id, as_i64(generation)?], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        for (cycle_id, bucket_date, agent) in stale {
            if active_keys.contains(&(cycle_id.clone(), bucket_date.clone(), agent.clone())) {
                continue;
            }
            Self::upsert_local_growth_entry_in_connection(
                transaction,
                &account_id,
                GrowthJournalEntry {
                    device_id: device_id.clone(),
                    cycle_id,
                    bucket_date,
                    agent: parse_agent(&agent)?,
                    revision: 0,
                    generation,
                    present: false,
                    confirmed_tokens: None,
                    coverage: UsageCoverage::Unavailable,
                    payload_hash: String::new(),
                },
            )?;
        }
        Ok(())
    }

    pub fn growth_journal(&self) -> Result<GrowthJournal, ScanError> {
        let account_id = self.current_planet_account_id()?;
        let (generation, deleted_at) = self.growth_journal_cursor(&account_id)?;
        let cutoff = deleted_at.as_deref().map(parse_timestamp).transpose()?;
        let mut cycles = BTreeMap::new();
        for table in ["growth_journal_remote_cycle", "growth_journal_cycle"] {
            let mut statement = self.connection.prepare(&format!(
                "SELECT cycle_id,started_at_utc,ended_at_utc,wallet_credit,wallet_credit_at_utc
                 FROM {table} WHERE account_id=?1"
            ))?;
            let rows = statement.query_map([&account_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            })?;
            for row in rows {
                let (cycle_id, started_at_utc, ended_at_utc, wallet_credit, wallet_credit_at_utc) =
                    row?;
                let cycle = GrowthJournalCycle {
                    cycle_id: cycle_id.clone(),
                    started_at_utc,
                    ended_at_utc,
                    wallet_credit: wallet_credit.map(as_u64).transpose()?,
                    wallet_credit_at_utc,
                };
                cycles
                    .entry(cycle_id)
                    .and_modify(|existing: &mut GrowthJournalCycle| merge_cycle(existing, &cycle))
                    .or_insert(cycle);
            }
        }
        let mut visible_cycles = BTreeMap::new();
        for (cycle_id, cycle) in cycles {
            if cycle_is_visible(&cycle, cutoff)? {
                visible_cycles.insert(cycle_id, cycle);
            }
        }
        let cycles = visible_cycles;

        let mut entries: BTreeMap<(String, String, String, String), GrowthJournalEntry> =
            BTreeMap::new();
        for table in ["growth_journal_remote_entry", "growth_journal_entry"] {
            let mut statement = self.connection.prepare(&format!(
                "SELECT device_id,cycle_id,bucket_date,agent,revision,generation,present,
                        confirmed_tokens,coverage,payload_hash
                 FROM {table} WHERE account_id=?1"
            ))?;
            let rows = statement.query_map([&account_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, bool>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                ))
            })?;
            for row in rows {
                let (
                    device_id,
                    cycle_id,
                    bucket_date,
                    agent,
                    revision,
                    row_generation,
                    present,
                    tokens,
                    coverage,
                    payload_hash,
                ) = row?;
                if as_u64(row_generation)? != generation {
                    continue;
                }
                let entry = GrowthJournalEntry {
                    device_id,
                    cycle_id,
                    bucket_date,
                    agent: parse_agent(&agent)?,
                    revision: as_u64(revision)?,
                    generation,
                    present,
                    confirmed_tokens: tokens.map(as_u64).transpose()?,
                    coverage: parse_coverage(&coverage)?,
                    payload_hash,
                };
                if entry.payload_hash != entry.compute_hash() {
                    return Err(ScanError::Database);
                }
                let key = (
                    entry.device_id.clone(),
                    entry.cycle_id.clone(),
                    entry.bucket_date.clone(),
                    agent_name(entry.agent).to_owned(),
                );
                entries
                    .entry(key)
                    .and_modify(|existing| {
                        if entry.revision > existing.revision {
                            *existing = entry.clone();
                        }
                    })
                    .or_insert(entry);
            }
        }
        entries.retain(|_, entry| entry.present && cycles.contains_key(&entry.cycle_id));
        Ok(GrowthJournal {
            generation,
            deleted_at_utc: deleted_at,
            timezone: Some(self.planet_timezone()?.to_string()),
            cycles: cycles.into_values().collect(),
            entries: entries.into_values().collect(),
        })
    }

    pub fn local_growth_journal(&self) -> Result<GrowthJournal, ScanError> {
        let device_id =
            journal_setting(&self.connection, "planet_device_id")?.ok_or(ScanError::Database)?;
        let mut journal = self.growth_journal()?;
        journal.entries.retain(|entry| entry.device_id == device_id);
        Ok(journal)
    }

    pub fn pending_growth_journal_entries(&self) -> Result<Vec<GrowthJournalEntry>, ScanError> {
        let account_id = self.current_planet_account_id()?;
        let (generation, _) = self.growth_journal_cursor(&account_id)?;
        let mut statement = self.connection.prepare(
            "SELECT device_id,cycle_id,bucket_date,agent,revision,generation,present,
                    confirmed_tokens,coverage,payload_hash
             FROM growth_journal_entry
             WHERE account_id=?1 AND generation=?2 AND revision>acknowledged_revision
             ORDER BY cycle_id,bucket_date,agent",
        )?;
        let rows = statement.query_map(params![account_id, as_i64(generation)?], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, bool>(6)?,
                row.get::<_, Option<i64>>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
            ))
        })?;
        rows.map(|row| {
            let (
                device_id,
                cycle_id,
                bucket_date,
                agent,
                revision,
                generation,
                present,
                tokens,
                coverage,
                payload_hash,
            ) = row?;
            Ok(GrowthJournalEntry {
                device_id,
                cycle_id,
                bucket_date,
                agent: parse_agent(&agent)?,
                revision: as_u64(revision)?,
                generation: as_u64(generation)?,
                present,
                confirmed_tokens: tokens.map(as_u64).transpose()?,
                coverage: parse_coverage(&coverage)?,
                payload_hash,
            })
        })
        .collect()
    }

    pub fn pending_growth_journal_count(&self) -> Result<u64, ScanError> {
        let account_id = self.current_planet_account_id()?;
        let (generation, _) = self.growth_journal_cursor(&account_id)?;
        let count: i64 = self.connection.query_row(
            "SELECT count(*) FROM growth_journal_entry
             WHERE account_id=?1 AND generation=?2 AND revision>acknowledged_revision",
            params![account_id, as_i64(generation)?],
            |row| row.get(0),
        )?;
        as_u64(count)
    }

    pub fn apply_growth_journal_state(&mut self, remote: &GrowthJournal) -> Result<(), ScanError> {
        self.require_guest_import_game_mutations_allowed()?;
        let account_id = self.current_planet_account_id()?;
        let (local_generation, _) = self.growth_journal_cursor(&account_id)?;
        if remote.generation < local_generation {
            return Ok(());
        }
        if remote.generation > local_generation {
            self.connection.execute(
                "DELETE FROM growth_journal_entry WHERE account_id=?1",
                [&account_id],
            )?;
            self.connection.execute(
                "DELETE FROM growth_journal_cycle WHERE account_id=?1",
                [&account_id],
            )?;
            self.connection.execute(
                "DELETE FROM growth_journal_remote_entry WHERE account_id=?1",
                [&account_id],
            )?;
            self.connection.execute(
                "DELETE FROM growth_journal_remote_cycle WHERE account_id=?1",
                [&account_id],
            )?;
        }
        self.connection.execute(
            "INSERT INTO growth_journal_state(account_id,generation,deleted_at_utc) VALUES (?1,?2,?3)
             ON CONFLICT(account_id) DO UPDATE SET generation=excluded.generation,
                deleted_at_utc=excluded.deleted_at_utc",
            params![account_id, as_i64(remote.generation)?, remote.deleted_at_utc],
        )?;
        if let Some(timezone) = &remote.timezone {
            self.set_planet_timezone(timezone)?;
        }
        self.connection.execute(
            "DELETE FROM growth_journal_remote_cycle WHERE account_id=?1",
            [&account_id],
        )?;
        self.connection.execute(
            "DELETE FROM growth_journal_remote_entry WHERE account_id=?1",
            [&account_id],
        )?;
        for cycle in &remote.cycles {
            self.connection.execute(
                "INSERT INTO growth_journal_remote_cycle(
                    account_id,cycle_id,started_at_utc,ended_at_utc,wallet_credit,wallet_credit_at_utc
                 ) VALUES (?1,?2,?3,?4,?5,?6)",
                params![
                    account_id, cycle.cycle_id, cycle.started_at_utc, cycle.ended_at_utc,
                    cycle.wallet_credit.map(as_i64).transpose()?, cycle.wallet_credit_at_utc,
                ],
            )?;
        }
        let local_device =
            journal_setting(&self.connection, "planet_device_id")?.ok_or(ScanError::Database)?;
        for entry in &remote.entries {
            if entry.generation != remote.generation || entry.payload_hash != entry.compute_hash() {
                return Err(ScanError::Database);
            }
            self.connection.execute(
                "INSERT INTO growth_journal_remote_entry(
                    account_id,device_id,cycle_id,bucket_date,agent,revision,generation,present,
                    confirmed_tokens,coverage,payload_hash
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                params![
                    account_id,
                    entry.device_id,
                    entry.cycle_id,
                    entry.bucket_date,
                    agent_name(entry.agent),
                    as_i64(entry.revision)?,
                    as_i64(entry.generation)?,
                    entry.present,
                    entry.confirmed_tokens.map(as_i64).transpose()?,
                    coverage_name(entry.coverage),
                    entry.payload_hash,
                ],
            )?;
            if entry.device_id == local_device {
                self.merge_remote_local_entry(&account_id, entry)?;
            }
        }
        Ok(())
    }

    fn merge_remote_local_entry(
        &self,
        account_id: &str,
        remote: &GrowthJournalEntry,
    ) -> Result<(), ScanError> {
        let local: Option<(i64, String)> = self
            .connection
            .query_row(
                "SELECT revision,payload_hash FROM growth_journal_entry
             WHERE account_id=?1 AND device_id=?2 AND cycle_id=?3 AND bucket_date=?4 AND agent=?5",
                params![
                    account_id,
                    remote.device_id,
                    remote.cycle_id,
                    remote.bucket_date,
                    agent_name(remote.agent)
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let replace = match local {
            None => true,
            Some((revision, _)) if as_u64(revision)? < remote.revision => true,
            Some((revision, hash))
                if as_u64(revision)? == remote.revision && hash == remote.payload_hash =>
            {
                self.connection.execute(
                    "UPDATE growth_journal_entry SET acknowledged_revision=max(acknowledged_revision,?6)
                     WHERE account_id=?1 AND device_id=?2 AND cycle_id=?3 AND bucket_date=?4
                       AND agent=?5 AND revision=?6 AND payload_hash=?7",
                    params![
                        account_id,
                        remote.device_id,
                        remote.cycle_id,
                        remote.bucket_date,
                        agent_name(remote.agent),
                        as_i64(remote.revision)?,
                        remote.payload_hash,
                    ],
                )?;
                false
            }
            Some((revision, hash))
                if as_u64(revision)? == remote.revision && hash != remote.payload_hash =>
            {
                return Err(ScanError::Database)
            }
            _ => false,
        };
        if replace {
            self.connection.execute(
                "INSERT INTO growth_journal_entry(
                    account_id,device_id,cycle_id,bucket_date,agent,revision,acknowledged_revision,
                    generation,present,confirmed_tokens,coverage,payload_hash
                 ) VALUES (?1,?2,?3,?4,?5,?6,?6,?7,?8,?9,?10,?11)
                 ON CONFLICT(account_id,device_id,cycle_id,bucket_date,agent) DO UPDATE SET
                    revision=excluded.revision, acknowledged_revision=excluded.acknowledged_revision,
                    generation=excluded.generation, present=excluded.present,
                    confirmed_tokens=excluded.confirmed_tokens, coverage=excluded.coverage,
                    payload_hash=excluded.payload_hash",
                params![
                    account_id, remote.device_id, remote.cycle_id, remote.bucket_date,
                    agent_name(remote.agent), as_i64(remote.revision)?, as_i64(remote.generation)?,
                    remote.present, remote.confirmed_tokens.map(as_i64).transpose()?,
                    coverage_name(remote.coverage), remote.payload_hash,
                ],
            )?;
        }
        Ok(())
    }

    fn current_planet_account_id(&self) -> Result<String, ScanError> {
        journal_setting(&self.connection, "planet_account_id")?.ok_or(ScanError::Database)
    }

    fn journal_source_health_in_connection(
        connection: &Connection,
        agent: Agent,
    ) -> Result<Option<SourceHealth>, ScanError> {
        journal_setting(
            connection,
            &format!("sharing_source_health:{}", agent_name(agent)),
        )?
        .map(|value| serde_json::from_str(&value).map_err(|_| ScanError::Database))
        .transpose()
    }

    fn growth_journal_cursor(&self, account_id: &str) -> Result<(u64, Option<String>), ScanError> {
        Self::growth_journal_cursor_in_connection(&self.connection, account_id)
    }

    fn growth_journal_cursor_in_connection(
        connection: &Connection,
        account_id: &str,
    ) -> Result<(u64, Option<String>), ScanError> {
        connection
            .query_row(
                "SELECT generation,deleted_at_utc FROM growth_journal_state WHERE account_id=?1",
                [account_id],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?)),
            )
            .optional()?
            .map(|(generation, deleted_at)| Ok((as_u64(generation)?, deleted_at)))
            .unwrap_or_else(|| Ok((0, None)))
    }

    fn agent_enabled_in_connection(
        connection: &Connection,
        agent: Agent,
    ) -> Result<bool, ScanError> {
        let key = format!("{}_enabled", agent_name(agent));
        Ok(journal_setting(connection, &key)?.as_deref() != Some("false"))
    }

    fn derived_growth_cycles_in_connection(
        connection: &Connection,
        account_id: &str,
        deleted_at: Option<DateTime<Utc>>,
    ) -> Result<Vec<GrowthJournalCycle>, ScanError> {
        let current_cycle =
            journal_setting(connection, "planet_current_cycle_id")?.ok_or(ScanError::Database)?;
        let current_started = journal_setting(connection, "planet_cycle_started_at_utc")?
            .ok_or(ScanError::Database)?;
        let mut credit_statement = connection.prepare(
            "SELECT previous_cycle_id,amount,created_at_utc FROM planet_wallet_credit
             ORDER BY created_at_utc",
        )?;
        let credit_rows = credit_statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut credits = credit_rows
            .map(|row| {
                let (previous_cycle_id, amount, created_at_utc) = row?;
                Ok((
                    parse_timestamp(&created_at_utc)?,
                    previous_cycle_id,
                    as_u64(amount)?,
                    created_at_utc,
                ))
            })
            .collect::<Result<Vec<_>, ScanError>>()?;
        credits.sort_by_key(|(created_at, _, _, _)| created_at.clone());
        let mut start = Some(parse_timestamp(
            &journal_setting(connection, "planet_activation_at_utc")?.ok_or(ScanError::Database)?,
        )?);
        let mut cycles = Vec::new();
        for (end, previous_cycle_id, amount, created_at_utc) in credits {
            if previous_cycle_id == current_cycle {
                continue;
            }
            if start.as_ref().is_some_and(|started| started >= &end) {
                start = None;
            }
            cycles.push(GrowthJournalCycle {
                cycle_id: previous_cycle_id,
                started_at_utc: start.map(|started| started.to_rfc3339()),
                ended_at_utc: Some(created_at_utc.clone()),
                wallet_credit: Some(amount),
                wallet_credit_at_utc: Some(created_at_utc),
            });
            start = Some(end);
        }
        cycles.push(GrowthJournalCycle {
            cycle_id: current_cycle,
            started_at_utc: Some(current_started),
            ended_at_utc: None,
            wallet_credit: None,
            wallet_credit_at_utc: None,
        });
        let mut local_statement = connection.prepare(
            "SELECT cycle_id,started_at_utc,ended_at_utc,wallet_credit,wallet_credit_at_utc
             FROM growth_journal_cycle WHERE account_id=?1",
        )?;
        let local_rows = local_statement.query_map([&account_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })?;
        for row in local_rows {
            let (cycle_id, started_at_utc, ended_at_utc, wallet_credit, wallet_credit_at_utc) =
                row?;
            let saved = GrowthJournalCycle {
                cycle_id: cycle_id.clone(),
                started_at_utc,
                ended_at_utc,
                wallet_credit: wallet_credit.map(as_u64).transpose()?,
                wallet_credit_at_utc,
            };
            if let Some(local) = cycles.iter_mut().find(|local| local.cycle_id == cycle_id) {
                merge_cycle(local, &saved);
            } else {
                cycles.push(saved);
            }
        }
        let mut remote_statement = connection.prepare(
            "SELECT cycle_id,started_at_utc,ended_at_utc,wallet_credit,wallet_credit_at_utc
             FROM growth_journal_remote_cycle WHERE account_id=?1",
        )?;
        let remote_rows = remote_statement.query_map([&account_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })?;
        for row in remote_rows {
            let (cycle_id, started_at_utc, ended_at_utc, wallet_credit, wallet_credit_at_utc) =
                row?;
            let remote = GrowthJournalCycle {
                cycle_id: cycle_id.clone(),
                started_at_utc,
                ended_at_utc,
                wallet_credit: wallet_credit.map(as_u64).transpose()?,
                wallet_credit_at_utc,
            };
            if let Some(local) = cycles.iter_mut().find(|local| local.cycle_id == cycle_id) {
                let mut merged = remote;
                merge_cycle(&mut merged, local);
                *local = merged;
            } else {
                cycles.push(remote);
            }
        }
        if let Some(cutoff) = deleted_at {
            let mut visible = Vec::new();
            for cycle in cycles {
                let keep = match cycle.ended_at_utc.as_deref() {
                    Some(end) => parse_timestamp(end)? > cutoff,
                    None => true,
                };
                if keep {
                    visible.push(cycle);
                }
            }
            return Ok(visible);
        }
        Ok(cycles)
    }

    fn upsert_local_growth_entry_in_connection(
        connection: &Connection,
        account_id: &str,
        mut entry: GrowthJournalEntry,
    ) -> Result<(), ScanError> {
        let old: Option<(i64, i64, i64, bool, Option<i64>, String)> = connection
            .query_row(
                "SELECT revision,acknowledged_revision,generation,present,confirmed_tokens,coverage
             FROM growth_journal_entry
             WHERE account_id=?1 AND device_id=?2 AND cycle_id=?3 AND bucket_date=?4 AND agent=?5",
                params![
                    account_id,
                    entry.device_id,
                    entry.cycle_id,
                    entry.bucket_date,
                    agent_name(entry.agent)
                ],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .optional()?;
        let (revision, acknowledged) = match old {
            Some((revision, acknowledged, generation, present, tokens, coverage))
                if as_u64(generation)? == entry.generation
                    && present == entry.present
                    && tokens == entry.confirmed_tokens.map(as_i64).transpose()?
                    && coverage == coverage_name(entry.coverage) =>
            {
                (as_u64(revision)?, as_u64(acknowledged)?)
            }
            Some((revision, _, _, _, _, _)) => (
                as_u64(revision)?
                    .checked_add(1)
                    .ok_or(ScanError::InvalidCount)?,
                0,
            ),
            None => (1, 0),
        };
        entry.revision = revision;
        entry.payload_hash.clear();
        entry = entry.seal();
        connection.execute(
            "INSERT INTO growth_journal_entry(
                account_id,device_id,cycle_id,bucket_date,agent,revision,acknowledged_revision,
                generation,present,confirmed_tokens,coverage,payload_hash
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
             ON CONFLICT(account_id,device_id,cycle_id,bucket_date,agent) DO UPDATE SET
                revision=excluded.revision, acknowledged_revision=excluded.acknowledged_revision,
                generation=excluded.generation, present=excluded.present,
                confirmed_tokens=excluded.confirmed_tokens, coverage=excluded.coverage,
                payload_hash=excluded.payload_hash",
            params![
                account_id,
                entry.device_id,
                entry.cycle_id,
                entry.bucket_date,
                agent_name(entry.agent),
                as_i64(entry.revision)?,
                as_i64(acknowledged)?,
                as_i64(entry.generation)?,
                entry.present,
                entry.confirmed_tokens.map(as_i64).transpose()?,
                coverage_name(entry.coverage),
                entry.payload_hash,
            ],
        )?;
        Ok(())
    }
}

fn contains(cycle: &GrowthJournalCycle, at: DateTime<Utc>) -> Result<bool, ScanError> {
    let Some(start) = cycle.started_at_utc.as_deref() else {
        return Ok(false);
    };
    let start = parse_timestamp(start)?;
    let ended = cycle
        .ended_at_utc
        .as_deref()
        .map(parse_timestamp)
        .transpose()?;
    Ok(at >= start && ended.is_none_or(|end| at < end))
}

fn cycle_is_visible(
    cycle: &GrowthJournalCycle,
    cutoff: Option<DateTime<Utc>>,
) -> Result<bool, ScanError> {
    let Some(cutoff) = cutoff else {
        return Ok(true);
    };
    Ok(cycle
        .ended_at_utc
        .as_deref()
        .is_none_or(|end| parse_timestamp(end).is_ok_and(|end| end > cutoff)))
}

fn merge_cycle(target: &mut GrowthJournalCycle, incoming: &GrowthJournalCycle) {
    if target.started_at_utc.is_none() {
        target.started_at_utc = incoming.started_at_utc.clone();
    }
    if target.ended_at_utc.is_none() {
        target.ended_at_utc = incoming.ended_at_utc.clone();
    }
    if target.wallet_credit.is_none() {
        target.wallet_credit = incoming.wallet_credit;
    }
    if target.wallet_credit_at_utc.is_none() {
        target.wallet_credit_at_utc = incoming.wallet_credit_at_utc.clone();
    }
}

fn journal_setting(
    connection: &rusqlite::Connection,
    key: &str,
) -> Result<Option<String>, ScanError> {
    connection
        .query_row("SELECT value FROM setting WHERE key=?1", [key], |row| {
            row.get(0)
        })
        .optional()
        .map_err(Into::into)
}

fn parse_timestamp(value: &str) -> Result<DateTime<Utc>, ScanError> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| ScanError::Database)
}

fn parse_agent(value: &str) -> Result<Agent, ScanError> {
    match value {
        "codex" => Ok(Agent::Codex),
        "claude_code" => Ok(Agent::ClaudeCode),
        _ => Err(ScanError::Database),
    }
}

fn enabled_for(agent: &str, enabled: &[(&str, bool); 2]) -> Result<bool, ScanError> {
    enabled
        .iter()
        .find(|(name, _)| *name == agent)
        .map(|(_, value)| *value)
        .ok_or(ScanError::Database)
}

fn parse_coverage(value: &str) -> Result<UsageCoverage, ScanError> {
    match value {
        "complete" => Ok(UsageCoverage::Complete),
        "partial" => Ok(UsageCoverage::Partial),
        "unavailable" => Ok(UsageCoverage::Unavailable),
        "unsupported" => Ok(UsageCoverage::Unsupported),
        "user_disabled" => Ok(UsageCoverage::UserDisabled),
        _ => Err(ScanError::Database),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collectors::{ParsedRecord, RecordKind};
    use crate::domain::usage::TokenUsage;
    use chrono_tz::Asia::Seoul;

    fn at(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn record(event_key: &str, occurred_at: &str, total: Option<u64>) -> ParsedRecord {
        ParsedRecord {
            agent: Agent::Codex,
            kind: RecordKind::Response,
            event_key: event_key.into(),
            occurred_at_utc: at(occurred_at),
            usage: TokenUsage {
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
            },
        }
    }

    #[test]
    fn reset_boundary_splits_same_local_date_and_keeps_wallet_credit() {
        let temp = tempfile::tempdir().unwrap();
        let mut ledger = Ledger::open(&temp.path().join("ledger.db"), Seoul).unwrap();
        ledger
            .connection
            .execute(
                "UPDATE setting SET value=?1 WHERE key='planet_activation_at_utc'",
                ["2026-09-28T00:00:00Z"],
            )
            .unwrap();
        ledger
            .connection
            .execute(
                "UPDATE setting SET value='cycle-before' WHERE key='planet_current_cycle_id'",
                [],
            )
            .unwrap();
        ledger
            .connection
            .execute(
                "UPDATE setting SET value='2026-09-28T00:00:00Z' WHERE key='planet_cycle_started_at_utc'",
                [],
            )
            .unwrap();
        ledger
            .insert(&record(
                "before-reset",
                "2026-09-28T14:29:00Z",
                Some(120_000),
            ))
            .unwrap();
        assert_eq!(
            ledger.reset_planet(at("2026-09-28T14:30:00Z")).unwrap(),
            120_000
        );
        let current_cycle = ledger.planet_cycle_id().unwrap();
        ledger
            .insert(&record("after-reset", "2026-09-28T14:31:00Z", Some(80_000)))
            .unwrap();

        ledger.prepare_growth_journal().unwrap();
        let journal = ledger.growth_journal().unwrap();
        assert_eq!(journal.cycles.len(), 2);
        let completed = journal
            .cycles
            .iter()
            .find(|cycle| cycle.cycle_id == "cycle-before")
            .unwrap();
        assert_eq!(completed.wallet_credit, Some(120_000));
        assert_eq!(
            completed.ended_at_utc.as_deref(),
            Some("2026-09-28T14:30:00+00:00")
        );
        assert!(journal
            .cycles
            .iter()
            .any(|cycle| cycle.cycle_id == current_cycle));
        assert_eq!(journal.entries.len(), 2);
        assert!(journal
            .entries
            .iter()
            .all(|entry| entry.bucket_date == "2026-09-28"));
        assert!(journal.entries.iter().any(|entry| {
            entry.cycle_id == "cycle-before" && entry.confirmed_tokens == Some(120_000)
        }));
        assert!(journal.entries.iter().any(|entry| {
            entry.cycle_id == current_cycle && entry.confirmed_tokens == Some(80_000)
        }));
    }

    #[test]
    fn corrections_acknowledge_server_rows_and_deletion_blocks_old_history() {
        let temp = tempfile::tempdir().unwrap();
        let mut ledger = Ledger::open(&temp.path().join("ledger.db"), Seoul).unwrap();
        ledger
            .connection
            .execute(
                "UPDATE setting SET value=?1 WHERE key='planet_activation_at_utc'",
                ["2026-09-27T00:00:00Z"],
            )
            .unwrap();
        ledger
            .connection
            .execute(
                "UPDATE setting SET value='2026-09-27T00:00:00Z' WHERE key='planet_cycle_started_at_utc'",
                [],
            )
            .unwrap();
        let current_cycle = ledger.planet_cycle_id().unwrap();
        ledger
            .insert(&record("corrected", "2026-09-28T10:00:00Z", Some(100)))
            .unwrap();
        ledger.prepare_growth_journal().unwrap();
        let first = ledger.pending_growth_journal_entries().unwrap();
        assert_eq!(first[0].revision, 1);

        ledger
            .insert(&record("corrected", "2026-09-28T10:00:00Z", Some(200)))
            .unwrap();
        ledger.prepare_growth_journal().unwrap();
        let corrected = ledger.pending_growth_journal_entries().unwrap();
        assert_eq!(corrected[0].revision, 2);
        assert_eq!(corrected[0].confirmed_tokens, Some(200));
        let local = ledger.growth_journal().unwrap();
        ledger
            .apply_growth_journal_state(&GrowthJournal {
                generation: local.generation,
                deleted_at_utc: local.deleted_at_utc.clone(),
                timezone: local.timezone.clone(),
                cycles: local.cycles,
                entries: corrected,
            })
            .unwrap();
        assert_eq!(ledger.pending_growth_journal_count().unwrap(), 0);

        let cutoff = "2026-09-28T11:00:00Z";
        ledger
            .apply_growth_journal_state(&GrowthJournal {
                generation: 1,
                deleted_at_utc: Some(cutoff.into()),
                timezone: Some("Asia/Seoul".into()),
                cycles: Vec::new(),
                entries: Vec::new(),
            })
            .unwrap();
        ledger.prepare_growth_journal().unwrap();
        assert!(ledger.growth_journal().unwrap().entries.is_empty());

        ledger
            .insert(&record("after-delete", "2026-09-28T11:01:00Z", Some(50)))
            .unwrap();
        ledger.prepare_growth_journal().unwrap();
        let after_delete = ledger.growth_journal().unwrap();
        assert_eq!(after_delete.generation, 1);
        assert_eq!(after_delete.entries.len(), 1);
        assert_eq!(after_delete.entries[0].cycle_id, current_cycle);
        assert_eq!(after_delete.entries[0].confirmed_tokens, Some(50));
    }

    #[test]
    fn partial_collection_without_a_confirmed_count_stays_unknown_and_partial() {
        let temp = tempfile::tempdir().unwrap();
        let mut ledger = Ledger::open(&temp.path().join("ledger.db"), Seoul).unwrap();
        ledger
            .connection
            .execute(
                "UPDATE setting SET value='2026-09-27T00:00:00Z' WHERE key='planet_activation_at_utc'",
                [],
            )
            .unwrap();
        ledger
            .connection
            .execute(
                "UPDATE setting SET value='2026-09-27T00:00:00Z' WHERE key='planet_cycle_started_at_utc'",
                [],
            )
            .unwrap();
        let mut partial = record("unknown-partial", "2026-09-28T10:00:00Z", None);
        partial.usage.coverage = UsageCoverage::Partial;
        ledger.insert(&partial).unwrap();

        ledger.prepare_growth_journal().unwrap();
        let entry = ledger.growth_journal().unwrap().entries.remove(0);
        assert_eq!(entry.confirmed_tokens, None);
        assert_eq!(entry.coverage, UsageCoverage::Partial);
    }

    #[test]
    fn failed_preparation_does_not_leave_partial_cycle_writes() {
        let temp = tempfile::tempdir().unwrap();
        let mut ledger = Ledger::open(&temp.path().join("ledger.db"), Seoul).unwrap();
        ledger
            .connection
            .execute(
                "UPDATE setting SET value='2026-09-27T00:00:00Z' WHERE key='planet_activation_at_utc'",
                [],
            )
            .unwrap();
        ledger
            .connection
            .execute(
                "UPDATE setting SET value='2026-09-27T00:00:00Z' WHERE key='planet_cycle_started_at_utc'",
                [],
            )
            .unwrap();
        ledger
            .insert(&record(
                "aborted-prepare",
                "2026-09-28T10:00:00Z",
                Some(100),
            ))
            .unwrap();
        ledger
            .connection
            .execute_batch(
                "CREATE TRIGGER reject_journal_entry BEFORE INSERT ON growth_journal_entry
                 BEGIN SELECT RAISE(ABORT, 'test preparation failure'); END;",
            )
            .unwrap();

        assert!(ledger.prepare_growth_journal().is_err());
        let cycle_count: i64 = ledger
            .connection
            .query_row("SELECT COUNT(*) FROM growth_journal_cycle", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(cycle_count, 0);
    }

    #[test]
    fn transaction_preparation_rolls_back_and_preserves_committed_journal_output() {
        let temp = tempfile::tempdir().unwrap();
        let mut ledger = Ledger::open(&temp.path().join("ledger.db"), Seoul).unwrap();
        ledger
            .connection
            .execute(
                "UPDATE setting SET value='2026-09-27T00:00:00Z' WHERE key='planet_activation_at_utc'",
                [],
            )
            .unwrap();
        ledger
            .connection
            .execute(
                "UPDATE setting SET value='2026-09-27T00:00:00Z' WHERE key='planet_cycle_started_at_utc'",
                [],
            )
            .unwrap();
        let cycle_id = ledger.planet_cycle_id().unwrap();
        ledger
            .insert(&record("first", "2026-09-28T10:00:00Z", Some(100)))
            .unwrap();
        ledger.prepare_growth_journal().unwrap();
        let committed_before = ledger.growth_journal().unwrap();
        assert_eq!(committed_before.entries.len(), 1);
        assert_eq!(committed_before.entries[0].confirmed_tokens, Some(100));

        ledger
            .insert(&record("second", "2026-09-28T11:00:00Z", Some(200)))
            .unwrap();
        {
            let transaction = ledger.connection.transaction().unwrap();
            Ledger::prepare_growth_journal_in_transaction(&transaction).unwrap();
            let prepared_tokens: i64 = transaction
                .query_row(
                    "SELECT confirmed_tokens FROM growth_journal_entry
                     WHERE account_id=(SELECT value FROM setting WHERE key='planet_account_id')
                       AND cycle_id=?1 AND bucket_date='2026-09-28' AND agent='codex' AND present=1",
                    [&cycle_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(prepared_tokens, 300);
        }

        assert_eq!(ledger.growth_journal().unwrap(), committed_before);

        let transaction = ledger.connection.transaction().unwrap();
        Ledger::prepare_growth_journal_in_transaction(&transaction).unwrap();
        transaction.commit().unwrap();
        let committed_after = ledger.growth_journal().unwrap();
        assert_eq!(committed_after.cycles.len(), 1);
        assert_eq!(committed_after.cycles[0].cycle_id, cycle_id);
        assert_eq!(committed_after.entries.len(), 1);
        assert_eq!(committed_after.entries[0].bucket_date, "2026-09-28");
        assert_eq!(committed_after.entries[0].confirmed_tokens, Some(300));
        assert_eq!(committed_after.entries[0].revision, 2);
        assert_eq!(committed_after.entries[0].coverage, UsageCoverage::Complete);
    }

    #[test]
    fn incomplete_source_keeps_known_tokens_but_marks_the_day_partial() {
        let temp = tempfile::tempdir().unwrap();
        let mut ledger = Ledger::open(&temp.path().join("ledger.db"), Seoul).unwrap();
        ledger
            .connection
            .execute(
                "UPDATE setting SET value='2026-09-27T00:00:00Z' WHERE key='planet_activation_at_utc'",
                [],
            )
            .unwrap();
        ledger
            .connection
            .execute(
                "UPDATE setting SET value='2026-09-27T00:00:00Z' WHERE key='planet_cycle_started_at_utc'",
                [],
            )
            .unwrap();
        ledger
            .set_source_health(Agent::Codex, SourceHealth::Partial)
            .unwrap();
        ledger
            .insert(&record("known-partial", "2026-09-28T10:00:00Z", Some(100)))
            .unwrap();

        ledger.prepare_growth_journal().unwrap();
        let journal = ledger.growth_journal().unwrap();
        assert_eq!(journal.entries[0].confirmed_tokens, Some(100));
        assert_eq!(journal.entries[0].coverage, UsageCoverage::Partial);
    }

    #[test]
    fn cached_journal_skips_unchanged_writes_and_refreshes_after_local_or_external_correction() {
        let temp = tempfile::tempdir().unwrap();
        let db = temp.path().join("ledger.db");
        let mut ledger = Ledger::open(&db, Seoul).unwrap();
        ledger
            .connection
            .execute(
                "UPDATE setting SET value='2026-09-27T00:00:00Z' WHERE key='planet_activation_at_utc'",
                [],
            )
            .unwrap();
        ledger
            .connection
            .execute(
                "UPDATE setting SET value='2026-09-27T00:00:00Z' WHERE key='planet_cycle_started_at_utc'",
                [],
            )
            .unwrap();
        let event = "same-row-correction";
        ledger
            .insert(&record(event, "2026-09-28T10:00:00Z", Some(100)))
            .unwrap();
        ledger.prepare_growth_journal().unwrap();
        assert_eq!(
            ledger.growth_journal().unwrap().entries[0].confirmed_tokens,
            Some(100)
        );

        let writes_before = ledger.connection.total_changes();
        ledger.prepare_growth_journal().unwrap();
        assert_eq!(ledger.connection.total_changes(), writes_before);
        assert_eq!(
            ledger.growth_journal().unwrap().entries[0].confirmed_tokens,
            Some(100)
        );

        ledger
            .insert(&record(event, "2026-09-28T10:00:00Z", Some(200)))
            .unwrap();
        ledger.prepare_growth_journal().unwrap();
        assert_eq!(
            ledger.growth_journal().unwrap().entries[0].confirmed_tokens,
            Some(200)
        );
        assert_eq!(
            ledger
                .connection
                .query_row("SELECT COUNT(*) FROM usage_record", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );

        let external = rusqlite::Connection::open(&db).unwrap();
        external
            .execute(
                "UPDATE usage_record SET total_tokens=300 WHERE event_key=?1",
                [event],
            )
            .unwrap();
        ledger.prepare_growth_journal().unwrap();
        assert_eq!(
            ledger.growth_journal().unwrap().entries[0].confirmed_tokens,
            Some(300)
        );
        assert_eq!(
            ledger
                .connection
                .query_row("SELECT COUNT(*) FROM usage_record", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
}
