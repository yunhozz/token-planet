use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use super::guest_provenance::initialize_guest_provenance_schema;
use super::ledger::{Ledger, ScanError};
use crate::domain::cosmetic_shop::ActiveEffects;
use crate::domain::device_reset::{DeviceResetPhase, DeviceResetState};

// Children precede parents. Every non-internal table must be explicitly classified.
const USER_TABLES: &[&str] = &[
    "shop_landscape_placement",
    "shop_landscape_edit_version",
    "shop_landscape_instance",
    "source_checkpoint",
    "usage_record",
    "daily_agent_total",
    "outbox_snapshot",
    "planet_object",
    "planet_wallet_credit",
    "planet_account_state",
    "planet_usage_owner",
    "growth_journal_state",
    "growth_journal_cycle",
    "growth_journal_entry",
    "growth_journal_remote_cycle",
    "growth_journal_remote_entry",
    "cosmetic_purchase",
    "cosmetic_purchase_request",
    "cosmetic_equipment",
    "cosmetic_guest_import",
    "cosmetic_shop_remote_state",
    "cosmetic_pending_purchase",
    "shop_account_state",
    "shop_purchase",
    "shop_action_request",
    "shop_avatar_owned",
    "shop_avatar_equipment",
    "shop_natural_removal",
    "shop_natural_removal_debit",
    "shop_remote_state",
    "guest_shop_import_capture",
    "shop_effect_history",
    "shop_effect_cycle_bound",
    "shop_effect_cycle_bounds_state",
    "shop_effect_timeline_state",
    "shop_effect_contribution",
    "shop_contribution_state",
    "shop_activity_day",
    "shop_game_reward",
    "shop_wallet_credit",
    "shop_cycle_settlement",
    "shop_era_progress",
    "guest_provenance_meta",
    "guest_provenance_lineage",
    "guest_provenance_occurrence_key",
    "guest_provenance_occurrence_version",
    "guest_provenance_mutation",
    "guest_provenance_cycle",
    "guest_provenance_reset_receipt",
    "guest_shop_import_v2_capture",
];
const TEMPORARILY_REMOVED_TRIGGERS: &[&str] = &[
    "guest_provenance_occurrence_version_no_delete",
    "guest_provenance_mutation_no_delete",
    "guest_provenance_usage_delete",
];

pub(crate) fn initialize_device_reset_schema(connection: &Connection) -> Result<(), ScanError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS device_reset_control (
        singleton INTEGER PRIMARY KEY CHECK(singleton=1), state_json TEXT NOT NULL
    )",
    )?;
    Ok(())
}

fn table_inventory(connection: &Connection) -> Result<BTreeSet<String>, ScanError> {
    let mut query = connection.prepare(
        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
    )?;
    let result = query
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(result)
}

fn trigger_inventory(connection: &Connection) -> Result<BTreeMap<String, String>, ScanError> {
    let mut query =
        connection.prepare("SELECT name,sql FROM sqlite_master WHERE type='trigger'")?;
    let result = query
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    Ok(result)
}

fn verify_inventory(connection: &Connection) -> Result<BTreeMap<String, String>, ScanError> {
    for table in table_inventory(connection)? {
        if !USER_TABLES.contains(&table.as_str())
            && !["setting", "shop_schema_version", "device_reset_control"].contains(&table.as_str())
        {
            return Err(ScanError::Database);
        }
    }
    // Compare actual definitions to the application's schema, never trust a trigger name alone.
    let reference = Connection::open_in_memory()?;
    reference.execute_batch(
        "CREATE TABLE usage_record(event_key TEXT,kind TEXT,source_id TEXT,agent TEXT)",
    )?;
    initialize_guest_provenance_schema(&reference, false)?;
    let expected = trigger_inventory(&reference)?;
    let actual = trigger_inventory(connection)?;
    if actual != expected {
        return Err(ScanError::Database);
    }
    Ok(actual)
}

fn load_state(connection: &Connection) -> Result<DeviceResetState, ScanError> {
    let json: Option<String> = connection
        .query_row(
            "SELECT state_json FROM device_reset_control WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    let state: DeviceResetState = json
        .map(|json| serde_json::from_str(&json).map_err(|_| ScanError::Database))
        .transpose()?
        .unwrap_or_default();
    if state.generation > i64::MAX as u64 {
        return Err(ScanError::InvalidCount);
    }
    if state.phase != DeviceResetPhase::Idle
        && (state.request_id.is_none()
            || state.cutoff_at_utc.is_none()
            || state.new_cycle_id.is_none()
            || state.new_device_id.is_none()
            || state.new_lineage_id.is_none()
            || state.service_id.is_none())
    {
        return Err(ScanError::Database);
    }
    if state.phase == DeviceResetPhase::Idle {
        if state != DeviceResetState::default() {
            return Err(ScanError::Database);
        }
    } else {
        if state.generation == 0 {
            return Err(ScanError::Database);
        }
        for id in [
            &state.request_id,
            &state.new_cycle_id,
            &state.new_device_id,
            &state.new_lineage_id,
        ] {
            let value = id.as_deref().ok_or(ScanError::Database)?;
            if uuid::Uuid::parse_str(value)
                .map_err(|_| ScanError::Database)?
                .to_string()
                != value
            {
                return Err(ScanError::Database);
            }
        }
        let cutoff = state.cutoff_at_utc.as_deref().ok_or(ScanError::Database)?;
        let parsed = DateTime::parse_from_rfc3339(cutoff)
            .map_err(|_| ScanError::Database)?
            .with_timezone(&Utc);
        if parsed.to_rfc3339_opts(SecondsFormat::Micros, true) != cutoff
            || state.service_id.as_deref().is_none_or(str::is_empty)
        {
            return Err(ScanError::Database);
        }
    }
    Ok(state)
}

fn save_state(connection: &Connection, state: &DeviceResetState) -> Result<(), ScanError> {
    let json = serde_json::to_string(state).map_err(|_| ScanError::Database)?;
    connection.execute("INSERT INTO device_reset_control(singleton,state_json) VALUES (1,?1) ON CONFLICT(singleton) DO UPDATE SET state_json=excluded.state_json", [json])?;
    Ok(())
}

pub(crate) fn recovery_required(connection: &Connection) -> bool {
    let exists = connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='device_reset_control')", [], |row| row.get::<_,bool>(0));
    match exists {
        Ok(false) => false,
        Ok(true) => load_state(connection)
            .map(|state| {
                matches!(
                    state.phase,
                    DeviceResetPhase::Pending | DeviceResetPhase::LocalCommitted
                )
            })
            .unwrap_or(true),
        Err(_) => true,
    }
}

pub(crate) fn cutoff_in_connection(
    connection: &Connection,
) -> Result<Option<DateTime<Utc>>, ScanError> {
    load_state(connection)?
        .cutoff_at_utc
        .map(|value| {
            DateTime::parse_from_rfc3339(&value)
                .map(|value| value.with_timezone(&Utc))
                .map_err(|_| ScanError::Database)
        })
        .transpose()
}

pub fn record_after_reset_cutoff(
    occurred_at: DateTime<Utc>,
    cutoff: Option<DateTime<Utc>>,
) -> bool {
    cutoff.is_none_or(|cutoff| occurred_at > cutoff)
}

pub(crate) fn record_eligible_after_reset(
    connection: &Connection,
    record: &crate::collectors::ParsedRecord,
) -> Result<bool, ScanError> {
    let cutoff = cutoff_in_connection(connection)?;
    Ok(record_after_reset_cutoff(record.occurred_at_utc, cutoff)
        && (cutoff.is_none() || record.kind == crate::collectors::RecordKind::Response))
}

impl Ledger {
    pub fn device_reset_state(&self) -> Result<DeviceResetState, ScanError> {
        load_state(&self.connection)
    }
    pub fn device_reset_cutoff(&self) -> Result<Option<DateTime<Utc>>, ScanError> {
        cutoff_in_connection(&self.connection)
    }
    pub fn local_generation(&self) -> Result<u64, ScanError> {
        Ok(self.device_reset_state()?.generation)
    }

    pub fn prepare_device_reset(
        &mut self,
        expected_generation: u64,
        service_id: &str,
        cutoff: DateTime<Utc>,
    ) -> Result<DeviceResetState, ScanError> {
        verify_inventory(&self.connection)?;
        let previous = self.device_reset_state()?;
        if previous.generation != expected_generation {
            return Err(ScanError::InvalidShopState);
        }
        if matches!(
            previous.phase,
            DeviceResetPhase::Pending | DeviceResetPhase::LocalCommitted
        ) {
            return Ok(previous);
        }
        if service_id.is_empty() {
            return Err(ScanError::InvalidShopState);
        }
        let generation = previous
            .generation
            .checked_add(1)
            .filter(|n| *n <= i64::MAX as u64)
            .ok_or(ScanError::InvalidCount)?;
        let timezone: String = self.connection.query_row(
            "SELECT value FROM setting WHERE key='world_timezone'",
            [],
            |r| r.get(0),
        )?;
        if timezone != self.timezone.to_string() {
            return Err(ScanError::TimezoneMismatch);
        }
        let state = DeviceResetState {
            request_id: Some(uuid::Uuid::new_v4().to_string()),
            generation,
            phase: DeviceResetPhase::Pending,
            cutoff_at_utc: Some(cutoff.to_rfc3339_opts(SecondsFormat::Micros, true)),
            new_cycle_id: Some(uuid::Uuid::new_v4().to_string()),
            new_device_id: Some(uuid::Uuid::new_v4().to_string()),
            new_lineage_id: Some(uuid::Uuid::new_v4().to_string()),
            service_id: Some(service_id.to_owned()),
        };
        save_state(&self.connection, &state)?;
        Ok(state)
    }

    pub fn commit_device_reset(&mut self, request_id: &str) -> Result<DeviceResetState, ScanError> {
        let timezone = self.timezone.to_string();
        let tx = self.connection.transaction()?;
        let mut state = load_state(&tx)?;
        if state.request_id.as_deref() != Some(request_id) {
            return Err(ScanError::InvalidShopState);
        }
        if matches!(
            state.phase,
            DeviceResetPhase::LocalCommitted | DeviceResetPhase::Completed
        ) {
            return Ok(state);
        }
        if state.phase != DeviceResetPhase::Pending {
            return Err(ScanError::InvalidShopState);
        }
        let triggers = verify_inventory(&tx)?;
        for name in TEMPORARILY_REMOVED_TRIGGERS {
            tx.execute_batch(&format!("DROP TRIGGER {name}"))?;
        }
        #[cfg(test)]
        tests::checkpoint("after_drop")?;
        let tables = table_inventory(&tx)?;
        for name in USER_TABLES {
            if tables.contains(*name) {
                tx.execute(&format!("DELETE FROM {name}"), [])?;
            }
        }
        tx.execute("DELETE FROM setting WHERE key!='world_timezone'", [])?;
        #[cfg(test)]
        tests::checkpoint("after_delete")?;
        let cutoff = state.cutoff_at_utc.as_deref().ok_or(ScanError::Database)?;
        let cycle = state.new_cycle_id.as_deref().ok_or(ScanError::Database)?;
        let device = state.new_device_id.as_deref().ok_or(ScanError::Database)?;
        let lineage = state.new_lineage_id.as_deref().ok_or(ScanError::Database)?;
        for (key, value) in [
            ("planet_account_id", "local"),
            ("planet_activation_at_utc", cutoff),
            ("planet_cycle_started_at_utc", cutoff),
            ("planet_current_cycle_id", cycle),
            ("planet_device_id", device),
            ("planet_timezone", timezone.as_str()),
        ] {
            tx.execute(
                "INSERT INTO setting(key,value) VALUES (?1,?2)",
                params![key, value],
            )?;
        }
        tx.execute("INSERT INTO guest_provenance_meta VALUES (1,0)", [])?;
        tx.execute("INSERT INTO guest_provenance_lineage VALUES (1,?1,?2,?3,'guest_utc_monotonic_v1',1,0,0,0,NULL)", params![lineage,device,cutoff])?;
        let effects =
            serde_json::to_string(&ActiveEffects::default()).map_err(|_| ScanError::Database)?;
        tx.execute(
            "INSERT INTO guest_provenance_cycle VALUES (?1,?2,?3,NULL,0,'[]',?4)",
            params![cycle, lineage, cutoff, effects],
        )?;
        #[cfg(test)]
        tests::checkpoint("during_seed")?;
        tx.execute(
            "INSERT INTO shop_account_state VALUES ('local',0,?1)",
            [&timezone],
        )?;
        tx.execute(
            "INSERT INTO growth_journal_state(account_id) VALUES ('local')",
            [],
        )?;
        for name in TEMPORARILY_REMOVED_TRIGGERS {
            tx.execute_batch(triggers.get(*name).ok_or(ScanError::Database)?)?;
            #[cfg(test)]
            tests::checkpoint("during_restore")?;
        }
        if trigger_inventory(&tx)? != triggers {
            return Err(ScanError::Database);
        }
        let violations: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_foreign_key_check)",
            [],
            |r| r.get(0),
        )?;
        if violations {
            return Err(ScanError::Database);
        }
        state.phase = DeviceResetPhase::LocalCommitted;
        save_state(&tx, &state)?;
        tx.commit()?;
        self.growth_journal_signature = None;
        Ok(state)
    }

    pub fn complete_device_reset(
        &mut self,
        request_id: &str,
    ) -> Result<DeviceResetState, ScanError> {
        let mut state = self.device_reset_state()?;
        if state.request_id.as_deref() != Some(request_id)
            || !matches!(
                state.phase,
                DeviceResetPhase::LocalCommitted | DeviceResetPhase::Completed
            )
        {
            return Err(ScanError::InvalidShopState);
        }
        state.phase = DeviceResetPhase::Completed;
        save_state(&self.connection, &state)?;
        Ok(state)
    }
}

#[cfg(test)]
mod tests {
    use super::super::ledger::Ledger;
    use chrono::{TimeZone, Utc};

    // Missing atomic reset must leave user settings behind and fail this assertion.
    #[test]
    fn local_reset_preserves_timezone_schema_version() {
        let mut ledger =
            Ledger::open(std::path::Path::new(":memory:"), chrono_tz::Asia::Seoul).unwrap();
        ledger
            .connection
            .execute(
                "INSERT INTO setting VALUES ('private_user_setting','secret')",
                [],
            )
            .unwrap();
        let reset = ledger
            .prepare_device_reset(
                0,
                "test-service",
                Utc.with_ymd_and_hms(2026, 10, 10, 0, 0, 0).unwrap(),
            )
            .unwrap();
        ledger
            .commit_device_reset(reset.request_id.as_deref().unwrap())
            .unwrap();
        let left: i64 = ledger
            .connection
            .query_row(
                "SELECT COUNT(*) FROM setting WHERE key='private_user_setting'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(left, 0);
        assert_eq!(ledger.timezone, chrono_tz::Asia::Seoul);

        let timezone: String = ledger
            .connection
            .query_row(
                "SELECT value FROM setting WHERE key='world_timezone'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(timezone, "Asia/Seoul");
        assert_eq!(ledger.cosmetic_account_id().unwrap(), "local");
    }

    #[test]
    fn local_reset_unknown_table_rejected_before_delete() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        ledger.connection.execute_batch("CREATE TABLE future_user_data(value TEXT); INSERT INTO future_user_data VALUES ('keep')").unwrap();
        assert!(ledger
            .prepare_device_reset(0, "test-service", Utc::now())
            .is_err());
        assert_eq!(
            ledger
                .connection
                .query_row("SELECT value FROM future_user_data", [], |r| r
                    .get::<_, String>(0))
                .unwrap(),
            "keep"
        );
    }

    #[test]
    fn local_reset_retry_reuses_ids_and_cutoff() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let first = ledger
            .prepare_device_reset(0, "test-service", Utc::now())
            .unwrap();
        let retry = ledger
            .prepare_device_reset(1, "other-service", Utc::now())
            .unwrap();
        assert_eq!(first, retry);
        let committed = ledger
            .commit_device_reset(first.request_id.as_deref().unwrap())
            .unwrap();
        let again = ledger
            .commit_device_reset(first.request_id.as_deref().unwrap())
            .unwrap();
        assert_eq!(committed, again);
        assert_eq!(committed.generation, 1);
        assert_eq!(
            ledger.planet_cycle_id().unwrap(),
            first.new_cycle_id.unwrap()
        );
    }

    #[test]
    fn local_reset_with_immutable_provenance_rows_succeeds() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        ledger.connection.execute_batch("INSERT INTO guest_provenance_mutation VALUES (1,'old-lineage','event','delete',NULL)").unwrap();
        let reset = ledger
            .prepare_device_reset(0, "test-service", Utc::now())
            .unwrap();
        ledger
            .commit_device_reset(reset.request_id.as_deref().unwrap())
            .unwrap();
        assert_eq!(
            ledger
                .connection
                .query_row("SELECT COUNT(*) FROM guest_provenance_mutation", [], |r| {
                    r.get::<_, i64>(0)
                })
                .unwrap(),
            0
        );
        ledger.connection.execute_batch("INSERT INTO guest_provenance_mutation VALUES (1,'new-lineage','event','delete',NULL)").unwrap();
        assert!(ledger
            .connection
            .execute("DELETE FROM guest_provenance_mutation", [])
            .is_err());
        assert!(ledger
            .connection
            .execute(
                "UPDATE guest_provenance_mutation SET event_key='changed'",
                []
            )
            .is_err());
    }

    #[test]
    fn local_reset_corrupt_control_refuses_delete() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let mut reset = ledger
            .prepare_device_reset(0, "test-service", Utc::now())
            .unwrap();
        reset.new_cycle_id = Some("invalid-id".into());
        super::save_state(&ledger.connection, &reset).unwrap();
        assert!(ledger
            .commit_device_reset(reset.request_id.as_deref().unwrap())
            .is_err());
        assert_ne!(ledger.planet_cycle_id().unwrap(), "invalid-id");
    }

    #[test]
    fn local_reset_clears_populated_inventory_for_multiple_accounts() {
        use rusqlite::types::Value;
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        ledger.connection.execute_batch("CREATE TABLE guest_shop_import_v2_capture(target_account_id TEXT PRIMARY KEY, import_id TEXT NOT NULL UNIQUE, source_fingerprint TEXT NOT NULL, request_json TEXT NOT NULL, phase TEXT NOT NULL CHECK(phase IN ('captured','attempt_started','held','imported')), correction_hold INTEGER NOT NULL CHECK(correction_hold IN (0,1)), source_manifest_json TEXT NOT NULL)").unwrap();
        let triggers = super::trigger_inventory(&ledger.connection).unwrap();
        for table in super::USER_TABLES.iter().rev() {
            let columns: Vec<(String, String)> = ledger
                .connection
                .prepare(&format!("PRAGMA table_info({table})"))
                .unwrap()
                .query_map([], |r| Ok((r.get(1)?, r.get(2)?)))
                .unwrap()
                .map(Result::unwrap)
                .collect();
            let account = columns
                .iter()
                .find(|(name, _)| name == "account_id" || name == "target_account_id");
            let copies = if account.is_some() { 2 } else { 1 };
            let count: i64 = ledger
                .connection
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            if count == 0 || account.is_some() {
                for suffix in 0..copies {
                    let values: Vec<Value> = columns
                        .iter()
                        .map(|(name, kind)| {
                            if kind == "INTEGER" {
                                return Value::Integer(if name == "baseline_revision" {
                                    0
                                } else {
                                    1
                                });
                            }
                            Value::Text(match name.as_str() {
                                "account_id" | "target_account_id" => {
                                    format!("old-account-{suffix}")
                                }
                                "active_instance_ids_json" => "[]".into(),
                                "clock_policy" => "guest_utc_monotonic_v1".into(),
                                "slot" => "head".into(),
                                "kind" if *table == "shop_game_reward" => "cycle_token".into(),
                                "kind" => "response".into(),
                                "phase" => "captured".into(),
                                "coverage" => "complete".into(),
                                "agent" => "codex".into(),
                                "reward_timezone" => "UTC".into(),
                                _ if name.ends_with("_json") => "{}".into(),
                                _ if name.ends_with("_utc") => "1999-01-01T00:00:00Z".into(),
                                "bucket_date" => "1999-01-01".into(),
                                _ => format!("old-{suffix}-{name}"),
                            })
                        })
                        .collect();
                    let sql = format!(
                        "INSERT INTO {table} ({}) VALUES ({})",
                        columns
                            .iter()
                            .map(|(n, _)| n.as_str())
                            .collect::<Vec<_>>()
                            .join(","),
                        vec!["?"; columns.len()].join(",")
                    );
                    ledger
                        .connection
                        .execute(&sql, rusqlite::params_from_iter(values))
                        .unwrap_or_else(|error| panic!("seed {table}: {error}"));
                }
            }
            let count: i64 = ledger
                .connection
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            assert!(count > 0, "{table} must be populated before reset");
            if let Some((column, _)) = account {
                let count: i64 = ledger.connection.query_row(&format!("SELECT COUNT(*) FROM {table} WHERE {column} IN ('old-account-0','old-account-1')"), [], |r| r.get(0)).unwrap();
                assert_eq!(count, 2, "{table} has two account fixtures");
            }
        }
        let reset = ledger
            .prepare_device_reset(0, "fake-service", Utc::now())
            .unwrap();
        ledger
            .commit_device_reset(reset.request_id.as_deref().unwrap())
            .unwrap();
        let fresh_seed_tables = [
            "guest_provenance_meta",
            "guest_provenance_lineage",
            "guest_provenance_cycle",
            "shop_account_state",
            "growth_journal_state",
        ];
        for table in super::USER_TABLES {
            let count: i64 = ledger
                .connection
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(
                count,
                i64::from(fresh_seed_tables.contains(table)),
                "post-reset rows in {table}"
            );
        }
        assert_eq!(
            ledger.planet_cycle_id().unwrap(),
            reset.new_cycle_id.unwrap()
        );
        assert_eq!(ledger.cosmetic_account_id().unwrap(), "local");
        let lineage: String = ledger
            .connection
            .query_row("SELECT lineage_id FROM guest_provenance_lineage", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(lineage, reset.new_lineage_id.unwrap());
        assert_eq!(
            super::trigger_inventory(&ledger.connection).unwrap(),
            triggers
        );
        let violations: i64 = ledger
            .connection
            .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(violations, 0);
    }

    #[test]
    fn local_reset_inventory_covers_schema() {
        let ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        assert_eq!(super::USER_TABLES.len(), 50);
        assert_eq!(
            super::table_inventory(&ledger.connection).unwrap().len(),
            52
        ); // optional capture absent + control
        super::verify_inventory(&ledger.connection).unwrap();
    }

    #[test]
    fn local_reset_unknown_trigger_rejected_before_delete() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let cycle = ledger.planet_cycle_id().unwrap();
        ledger.connection.execute_batch("CREATE TRIGGER unknown_protection BEFORE DELETE ON usage_record BEGIN SELECT RAISE(ABORT,'unknown'); END").unwrap();
        assert!(ledger
            .prepare_device_reset(0, "test-service", Utc::now())
            .is_err());
        assert_eq!(ledger.planet_cycle_id().unwrap(), cycle);
        assert_eq!(ledger.device_reset_state().unwrap().generation, 0);
    }

    #[test]
    fn local_reset_rollback_restores_data_and_schema_on_guest_seed_failure() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let cycle = ledger.planet_cycle_id().unwrap();
        let triggers = super::trigger_inventory(&ledger.connection).unwrap();
        ledger.connection.execute_batch("DROP TABLE guest_provenance_meta; CREATE TABLE guest_provenance_meta(singleton INTEGER PRIMARY KEY,seed_allowed INTEGER CHECK(seed_allowed=1)); INSERT INTO guest_provenance_meta VALUES (1,1)").unwrap();
        let reset = ledger
            .prepare_device_reset(0, "test-service", Utc::now())
            .unwrap();
        assert!(ledger
            .commit_device_reset(reset.request_id.as_deref().unwrap())
            .is_err());
        assert_eq!(ledger.planet_cycle_id().unwrap(), cycle);
        assert_eq!(
            super::trigger_inventory(&ledger.connection).unwrap(),
            triggers
        );
        assert_eq!(ledger.device_reset_state().unwrap(), reset);
    }

    thread_local! { static FAIL_AT: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) }; }
    pub(super) fn checkpoint(point: &str) -> Result<(), super::ScanError> {
        if FAIL_AT.with(|fail| fail.get() == Some(point)) {
            Err(super::ScanError::Database)
        } else {
            Ok(())
        }
    }

    #[test]
    fn local_reset_rollback_restores_data_and_schema_all_checkpoints() {
        for point in [
            "after_drop",
            "after_delete",
            "during_seed",
            "during_restore",
        ] {
            let mut ledger =
                Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
            let cycle = ledger.planet_cycle_id().unwrap();
            let triggers = super::trigger_inventory(&ledger.connection).unwrap();
            ledger.connection.execute_batch("INSERT INTO guest_provenance_mutation VALUES (1,'old-lineage','event','delete',NULL)").unwrap();
            let reset = ledger
                .prepare_device_reset(0, "test-service", Utc::now())
                .unwrap();
            FAIL_AT.with(|fail| fail.set(Some(point)));
            let result = ledger.commit_device_reset(reset.request_id.as_deref().unwrap());
            FAIL_AT.with(|fail| fail.set(None));
            assert!(result.is_err(), "{point}");
            assert_eq!(ledger.planet_cycle_id().unwrap(), cycle, "{point}");
            assert_eq!(
                super::trigger_inventory(&ledger.connection).unwrap(),
                triggers,
                "{point}"
            );
            assert_eq!(ledger.device_reset_state().unwrap(), reset, "{point}");
            assert_eq!(
                ledger
                    .connection
                    .query_row("SELECT COUNT(*) FROM guest_provenance_mutation", [], |r| {
                        r.get::<_, i64>(0)
                    })
                    .unwrap(),
                1,
                "{point}"
            );
            ledger
                .commit_device_reset(reset.request_id.as_deref().unwrap())
                .unwrap();
        }
    }

    #[test]
    fn local_reset_optional_capture_all_phases() {
        for phase in ["captured", "attempt_started", "held", "imported"] {
            let mut ledger =
                Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
            // A late-created registered table must be cleared independently of its row phase.
            ledger.connection.execute_batch("CREATE TABLE guest_shop_import_v2_capture(target_account_id TEXT PRIMARY KEY, import_id TEXT UNIQUE NOT NULL, source_fingerprint TEXT NOT NULL, request_json TEXT NOT NULL, phase TEXT CHECK(phase IN ('captured','attempt_started','held','imported')), correction_hold INTEGER CHECK(correction_hold IN (0,1)), source_manifest_json TEXT NOT NULL)").unwrap();
            ledger.connection.execute("INSERT INTO guest_shop_import_v2_capture VALUES ('account:test','import','fingerprint','{}',?1,0,'{}')", [phase]).unwrap();
            let reset = ledger
                .prepare_device_reset(0, "test-service", Utc::now())
                .unwrap();
            ledger
                .commit_device_reset(reset.request_id.as_deref().unwrap())
                .unwrap();
            assert_eq!(
                ledger
                    .connection
                    .query_row(
                        "SELECT COUNT(*) FROM guest_shop_import_v2_capture",
                        [],
                        |r| r.get::<_, i64>(0)
                    )
                    .unwrap(),
                0
            );
        }
    }

    #[test]
    fn local_reset_usage_delete_still_records_mutation_after_success() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let reset = ledger
            .prepare_device_reset(0, "test-service", Utc::now())
            .unwrap();
        ledger
            .commit_device_reset(reset.request_id.as_deref().unwrap())
            .unwrap();
        let at = Utc::now() + chrono::Duration::seconds(1);
        ledger
            .insert(&crate::collectors::ParsedRecord {
                event_key: "new-event".into(),
                agent: crate::domain::usage::Agent::Codex,
                kind: crate::collectors::RecordKind::Response,
                occurred_at_utc: at,
                usage: crate::domain::usage::TokenUsage {
                    input_tokens: Some(10),
                    output_tokens: Some(0),
                    cache_read_tokens: Some(0),
                    cache_write_tokens: Some(0),
                    total_tokens: Some(10),
                    coverage: crate::domain::usage::UsageCoverage::Complete,
                },
            })
            .unwrap();
        ledger
            .connection
            .execute("DELETE FROM usage_record", [])
            .unwrap();
        assert_eq!(ledger.connection.query_row("SELECT mutation_kind FROM guest_provenance_mutation ORDER BY mutation_seq DESC LIMIT 1", [], |r| r.get::<_,String>(0)).unwrap(), "delete");
    }

    fn usage_record(
        key: &str,
        at: chrono::DateTime<Utc>,
        kind: crate::collectors::RecordKind,
        tokens: u64,
    ) -> crate::collectors::ParsedRecord {
        crate::collectors::ParsedRecord {
            event_key: key.into(),
            agent: crate::domain::usage::Agent::Codex,
            kind,
            occurred_at_utc: at,
            usage: crate::domain::usage::TokenUsage {
                input_tokens: Some(tokens),
                output_tokens: Some(0),
                cache_read_tokens: Some(0),
                cache_write_tokens: Some(0),
                total_tokens: Some(tokens),
                coverage: crate::domain::usage::UsageCoverage::Complete,
            },
        }
    }

    #[test]
    fn local_reset_cutoff_same_second_not_inclusive() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let cutoff = Utc.with_ymd_and_hms(2026, 10, 1, 0, 0, 0).unwrap()
            + chrono::Duration::microseconds(500000);
        let reset = ledger
            .prepare_device_reset(0, "test-service", cutoff)
            .unwrap();
        ledger
            .commit_device_reset(reset.request_id.as_deref().unwrap())
            .unwrap();
        for (key, delta, amount) in [("before", -1, 10), ("equal", 0, 20), ("after", 1, 30)] {
            ledger
                .insert(&usage_record(
                    key,
                    cutoff + chrono::Duration::microseconds(delta),
                    crate::collectors::RecordKind::Response,
                    amount,
                ))
                .unwrap();
        }
        assert_eq!(
            ledger
                .daily_total(crate::domain::usage::Agent::Codex, "2026-10-01")
                .unwrap(),
            Some(60)
        );
        let (_, current, lifetime) = ledger.planet_usage_totals().unwrap();
        assert_eq!((current, lifetime), (30, 30));
        assert_eq!(
            ledger
                .connection
                .query_row(
                    "SELECT COUNT(*) FROM guest_provenance_occurrence_key",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        assert_eq!(
            ledger.shared_daily_totals(chrono_tz::UTC).unwrap()[0].total_tokens,
            Some(30)
        );
    }

    #[test]
    fn local_reset_completed_reopen_does_not_backfill_excluded_usage() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("cutoff.sqlite3");
        let cutoff = Utc.with_ymd_and_hms(2026, 10, 1, 0, 0, 0).unwrap();
        for (kind, at) in [
            (
                crate::collectors::RecordKind::CumulativeSnapshot,
                cutoff + chrono::Duration::seconds(1),
            ),
            (crate::collectors::RecordKind::Response, cutoff),
        ] {
            {
                let mut ledger = Ledger::open(&path, chrono_tz::UTC).unwrap();
                let generation = ledger.device_reset_state().unwrap().generation;
                let reset = ledger
                    .prepare_device_reset(generation, "fake-service", cutoff)
                    .unwrap();
                ledger
                    .commit_device_reset(reset.request_id.as_deref().unwrap())
                    .unwrap();
                ledger
                    .complete_device_reset(reset.request_id.as_deref().unwrap())
                    .unwrap();
                ledger
                    .insert(&usage_record("excluded", at, kind, 1_000_000))
                    .unwrap();
                assert_eq!(
                    ledger
                        .connection
                        .query_row("SELECT COUNT(*) FROM usage_record", [], |r| r
                            .get::<_, i64>(0))
                        .unwrap(),
                    1
                );
                assert_eq!(
                    ledger
                        .connection
                        .query_row("SELECT COUNT(*) FROM planet_usage_owner", [], |r| r
                            .get::<_, i64>(0))
                        .unwrap(),
                    0
                );
            }
            let mut ledger = Ledger::open(&path, chrono_tz::UTC).unwrap();
            assert_eq!(
                ledger
                    .connection
                    .query_row("SELECT COUNT(*) FROM planet_usage_owner", [], |r| r
                        .get::<_, i64>(0))
                    .unwrap(),
                0,
                "reopen must preserve excluded owner status"
            );
            let (_, current, lifetime) = ledger.planet_usage_totals().unwrap();
            assert_eq!((current, lifetime), (0, 0));
            ledger.prepare_growth_journal().unwrap();
            ledger.rebuild_shop_contributions().unwrap();
            ledger
                .settle_guest_rewards(cutoff + chrono::Duration::days(1))
                .unwrap();
            for table in [
                "shop_game_reward",
                "shop_effect_contribution",
                "growth_journal_entry",
                "guest_provenance_occurrence_key",
            ] {
                assert_eq!(
                    ledger
                        .connection
                        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r
                            .get::<_, i64>(0))
                        .unwrap(),
                    0,
                    "{table}"
                );
            }
            assert!(!ledger.has_guest_shop_import_v2_candidate().unwrap());
        }
    }
    #[test]
    fn local_reset_no_cutoff_legacy_backfill_keeps_snapshot_ownership() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("legacy.sqlite3");
        {
            let mut ledger = Ledger::open(&path, chrono_tz::UTC).unwrap();
            ledger
                .insert(&usage_record(
                    "legacy-snapshot",
                    Utc::now() + chrono::Duration::seconds(1),
                    crate::collectors::RecordKind::CumulativeSnapshot,
                    1000,
                ))
                .unwrap();
            ledger
                .connection
                .execute("DELETE FROM planet_usage_owner", [])
                .unwrap();
            ledger
                .connection
                .execute("DELETE FROM setting WHERE key='planet_account_id'", [])
                .unwrap();
        }
        let ledger = Ledger::open(&path, chrono_tz::UTC).unwrap();
        assert_eq!(ledger.device_reset_cutoff().unwrap(), None);
        assert_eq!(
            ledger
                .connection
                .query_row("SELECT account_id FROM planet_usage_owner", [], |r| r
                    .get::<_, String>(0))
                .unwrap(),
            "local"
        );
        assert_eq!(ledger.planet_usage_totals().unwrap().1, 1000);
    }

    #[test]
    fn local_reset_cutoff_cumulative_fallback_cannot_restore_old_total() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let cutoff = Utc.with_ymd_and_hms(2026, 10, 1, 0, 0, 0).unwrap();
        let reset = ledger
            .prepare_device_reset(0, "test-service", cutoff)
            .unwrap();
        ledger
            .commit_device_reset(reset.request_id.as_deref().unwrap())
            .unwrap();
        ledger
            .insert(&usage_record(
                "snapshot",
                cutoff + chrono::Duration::seconds(1),
                crate::collectors::RecordKind::CumulativeSnapshot,
                1000,
            ))
            .unwrap();
        assert_eq!(
            ledger
                .daily_total(crate::domain::usage::Agent::Codex, "2026-10-01")
                .unwrap(),
            Some(1000)
        );
        let (_, current, lifetime) = ledger.planet_usage_totals().unwrap();
        assert_eq!((current, lifetime), (0, 0));
        assert!(ledger
            .shared_daily_totals(chrono_tz::UTC)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn local_reset_default_roots_and_enabled() {
        let mut ledger =
            Ledger::open(std::path::Path::new(":memory:"), chrono_tz::Asia::Seoul).unwrap();
        ledger
            .set_custom_root(
                crate::domain::usage::Agent::Codex,
                std::path::Path::new("/old/custom"),
            )
            .unwrap();
        ledger
            .set_agent_enabled(crate::domain::usage::Agent::Codex, false)
            .unwrap();
        ledger
            .set_agent_enabled(crate::domain::usage::Agent::ClaudeCode, false)
            .unwrap();
        let reset = ledger
            .prepare_device_reset(0, "test-service", Utc::now())
            .unwrap();
        ledger
            .commit_device_reset(reset.request_id.as_deref().unwrap())
            .unwrap();
        assert_eq!(
            ledger
                .custom_root(crate::domain::usage::Agent::Codex)
                .unwrap(),
            None
        );
        assert!(ledger
            .agent_enabled(crate::domain::usage::Agent::Codex)
            .unwrap());
        assert!(ledger
            .agent_enabled(crate::domain::usage::Agent::ClaudeCode)
            .unwrap());
        let config =
            crate::collectors::discovery::prepare_default_source_config(ledger.timezone).unwrap();
        assert_eq!(config.timezone, chrono_tz::Asia::Seoul);
        assert_ne!(config.codex_root, std::path::PathBuf::from("/old/custom"));
    }

    #[test]
    fn local_reset_cutoff_survives_reopen_source_rewrite_account_switch() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("isolated.sqlite3");
        let cutoff = Utc.with_ymd_and_hms(2026, 10, 1, 0, 0, 0).unwrap();
        {
            let mut ledger = Ledger::open(&path, chrono_tz::UTC).unwrap();
            let reset = ledger
                .prepare_device_reset(0, "test-service", cutoff)
                .unwrap();
            ledger
                .commit_device_reset(reset.request_id.as_deref().unwrap())
                .unwrap();
            ledger
                .complete_device_reset(reset.request_id.as_deref().unwrap())
                .unwrap();
        }
        let mut ledger = Ledger::open(&path, chrono_tz::UTC).unwrap();
        assert_eq!(ledger.device_reset_cutoff().unwrap(), Some(cutoff));
        ledger
            .ensure_planet_account("11111111-1111-4111-8111-111111111111")
            .unwrap();
        ledger
            .ensure_source_root(crate::domain::usage::Agent::Codex, "/replacement")
            .unwrap();
        ledger
            .insert(&usage_record(
                "old-rewrite",
                cutoff,
                crate::collectors::RecordKind::Response,
                1000,
            ))
            .unwrap();
        assert_eq!(ledger.planet_usage_totals().unwrap().2, 0);
        assert!(ledger
            .shared_daily_totals(chrono_tz::UTC)
            .unwrap()
            .is_empty());
        assert_eq!(ledger.device_reset_cutoff().unwrap(), Some(cutoff));
    }

    #[test]
    fn local_reset_cutoff_blocks_rewards_journal_import_upload() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let cutoff = Utc.with_ymd_and_hms(2026, 10, 1, 0, 0, 0).unwrap();
        let reset = ledger
            .prepare_device_reset(0, "test-service", cutoff)
            .unwrap();
        ledger
            .commit_device_reset(reset.request_id.as_deref().unwrap())
            .unwrap();
        ledger
            .insert(&usage_record(
                "old",
                cutoff,
                crate::collectors::RecordKind::Response,
                1_000_000,
            ))
            .unwrap();
        ledger.prepare_growth_journal().unwrap();
        ledger.rebuild_shop_contributions().unwrap();
        ledger
            .settle_guest_rewards(cutoff + chrono::Duration::days(1))
            .unwrap();
        ledger
            .prepare_shared_snapshots("world-test", "user-test", chrono_tz::UTC)
            .unwrap();
        for table in [
            "shop_game_reward",
            "shop_effect_contribution",
            "growth_journal_entry",
            "guest_provenance_occurrence_key",
        ] {
            assert_eq!(
                ledger
                    .connection
                    .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r
                        .get::<_, i64>(0))
                    .unwrap(),
                0,
                "{table}"
            );
        }
        assert!(ledger
            .pending_snapshots()
            .unwrap()
            .iter()
            .all(
                |snapshot| snapshot.total_tokens.is_none() && snapshot.bucket_date != "2026-10-01"
            ));
        assert!(!ledger.has_guest_shop_import_v2_candidate().unwrap());
    }

    #[test]
    fn local_reset_pending_reopen_does_not_seed_old_account_data() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("isolated.sqlite3");
        {
            let mut ledger = Ledger::open(&path, chrono_tz::UTC).unwrap();
            ledger
                .ensure_planet_account("11111111-1111-4111-8111-111111111111")
                .unwrap();
            ledger
                .connection
                .execute_batch("DELETE FROM growth_journal_state; DELETE FROM shop_account_state")
                .unwrap();
            ledger
                .prepare_device_reset(0, "fake-service", Utc::now())
                .unwrap();
        }
        let ledger = Ledger::open(&path, chrono_tz::UTC).unwrap();
        for table in ["growth_journal_state", "shop_account_state"] {
            assert_eq!(
                ledger
                    .connection
                    .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r
                        .get::<_, i64>(0))
                    .unwrap(),
                0,
                "{table}"
            );
        }
        assert_eq!(
            ledger.device_reset_state().unwrap().phase,
            crate::domain::device_reset::DeviceResetPhase::Pending
        );
    }
}
