use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::collectors::ParsedRecord;
use crate::domain::cosmetic_shop::ActiveEffects;
use crate::domain::guest_shop_import::{
    GuestFirstResetReceipt, GuestFirstResetRequestKind, GuestFirstResetStatus,
};
use crate::domain::usage::UsageCoverage;

use super::ledger::ScanError;

#[derive(Serialize)]
struct StoredOccurrenceFingerprint<'a> {
    source_id: &'a str,
    agent: &'a str,
    kind: &'a str,
    bucket_date: &'a str,
    occurred_at_utc: &'a str,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cache_read_tokens: Option<i64>,
    cache_write_tokens: Option<i64>,
    total_tokens: Option<i64>,
    coverage: &'a str,
    parser_version: i64,
}

pub(crate) fn initialize_guest_provenance_schema(
    connection: &Connection,
    allow_new_lineage: bool,
) -> Result<(), ScanError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS guest_provenance_meta (
            singleton INTEGER PRIMARY KEY CHECK(singleton=1),
            seed_allowed INTEGER NOT NULL CHECK(seed_allowed IN (0,1))
         );
         CREATE TABLE IF NOT EXISTS guest_provenance_lineage (
            singleton INTEGER PRIMARY KEY CHECK(singleton=1),
            lineage_id TEXT NOT NULL UNIQUE,
            device_id TEXT NOT NULL,
            activated_at_utc TEXT NOT NULL,
            clock_policy TEXT NOT NULL CHECK(clock_policy='guest_utc_monotonic_v1'),
            eligible INTEGER NOT NULL CHECK(eligible IN (0,1)),
            ingest_watermark INTEGER NOT NULL CHECK(ingest_watermark>=0),
            occurrence_count INTEGER NOT NULL CHECK(occurrence_count>=0),
            last_mutation_seq INTEGER NOT NULL CHECK(last_mutation_seq>=0),
            last_ingested_at_utc TEXT
         );
         CREATE TABLE IF NOT EXISTS guest_provenance_occurrence_key (
            event_key TEXT PRIMARY KEY,
            occurrence_id TEXT NOT NULL UNIQUE,
            ingest_seq INTEGER NOT NULL UNIQUE CHECK(ingest_seq>0),
            current_record_version INTEGER NOT NULL CHECK(current_record_version>0),
            content_fingerprint TEXT NOT NULL,
            effective INTEGER NOT NULL CHECK(effective IN (0,1))
         );
         CREATE TABLE IF NOT EXISTS guest_provenance_occurrence_version (
            occurrence_id TEXT NOT NULL,
            record_version INTEGER NOT NULL CHECK(record_version>0),
            ingest_seq INTEGER NOT NULL CHECK(ingest_seq>0),
            lineage_id TEXT NOT NULL,
            device_id TEXT NOT NULL,
            agent TEXT NOT NULL,
            occurred_at_utc TEXT NOT NULL,
            ingested_at_utc TEXT NOT NULL,
            cycle_id TEXT NOT NULL,
            total_tokens INTEGER,
            coverage TEXT NOT NULL,
            PRIMARY KEY(occurrence_id,record_version),
            UNIQUE(lineage_id,ingest_seq,record_version)
         );
         CREATE TABLE IF NOT EXISTS guest_provenance_mutation (
            mutation_seq INTEGER PRIMARY KEY CHECK(mutation_seq>0),
            lineage_id TEXT NOT NULL,
            event_key TEXT NOT NULL,
            mutation_kind TEXT NOT NULL,
            replaced_event_key TEXT
         );
         CREATE TABLE IF NOT EXISTS guest_provenance_cycle (
            cycle_id TEXT PRIMARY KEY,
            lineage_id TEXT NOT NULL,
            started_at_utc TEXT NOT NULL,
            ended_at_utc TEXT,
            baseline_revision INTEGER NOT NULL CHECK(baseline_revision=0),
            active_instance_ids_json TEXT NOT NULL CHECK(active_instance_ids_json='[]'),
            effects_json TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS guest_provenance_reset_receipt (
            request_id TEXT PRIMARY KEY,
            lineage_id TEXT NOT NULL,
            previous_cycle_id TEXT NOT NULL,
            new_cycle_id TEXT NOT NULL UNIQUE,
            reset_at_utc TEXT NOT NULL,
            receipt_json TEXT NOT NULL
         );
         CREATE TRIGGER IF NOT EXISTS guest_provenance_occurrence_version_no_update
         BEFORE UPDATE ON guest_provenance_occurrence_version
         BEGIN SELECT RAISE(ABORT,'immutable guest occurrence version'); END;
         CREATE TRIGGER IF NOT EXISTS guest_provenance_occurrence_version_no_delete
         BEFORE DELETE ON guest_provenance_occurrence_version
         BEGIN SELECT RAISE(ABORT,'immutable guest occurrence version'); END;
         CREATE TRIGGER IF NOT EXISTS guest_provenance_mutation_no_update
         BEFORE UPDATE ON guest_provenance_mutation
         BEGIN SELECT RAISE(ABORT,'immutable guest provenance mutation'); END;
         CREATE TRIGGER IF NOT EXISTS guest_provenance_mutation_no_delete
         BEFORE DELETE ON guest_provenance_mutation
         BEGIN SELECT RAISE(ABORT,'immutable guest provenance mutation'); END;
         CREATE TRIGGER IF NOT EXISTS guest_provenance_usage_update
         AFTER UPDATE ON usage_record
         WHEN EXISTS(SELECT 1 FROM guest_provenance_occurrence_key WHERE event_key=OLD.event_key)
         BEGIN
            INSERT INTO guest_provenance_mutation(mutation_seq,lineage_id,event_key,mutation_kind)
            SELECT last_mutation_seq+1,lineage_id,OLD.event_key,'content_correction'
            FROM guest_provenance_lineage
            WHERE singleton=1 AND last_mutation_seq<9223372036854775807;
            UPDATE guest_provenance_lineage
            SET eligible=0,
                last_mutation_seq=CASE WHEN last_mutation_seq<9223372036854775807
                                       THEN last_mutation_seq+1 ELSE last_mutation_seq END
            WHERE singleton=1;
         END;
         CREATE TRIGGER IF NOT EXISTS guest_provenance_usage_delete
         AFTER DELETE ON usage_record
         WHEN EXISTS(SELECT 1 FROM guest_provenance_occurrence_key WHERE event_key=OLD.event_key)
         BEGIN
            INSERT INTO guest_provenance_mutation(mutation_seq,lineage_id,event_key,mutation_kind)
            SELECT last_mutation_seq+1,lineage_id,OLD.event_key,'delete'
            FROM guest_provenance_lineage
            WHERE singleton=1 AND last_mutation_seq<9223372036854775807;
            UPDATE guest_provenance_lineage
            SET eligible=0,
                last_mutation_seq=CASE WHEN last_mutation_seq<9223372036854775807
                                       THEN last_mutation_seq+1 ELSE last_mutation_seq END
            WHERE singleton=1;
         END;
         CREATE TRIGGER IF NOT EXISTS guest_provenance_response_replaces_snapshot
         AFTER INSERT ON usage_record
         WHEN NEW.kind='response'
           AND EXISTS(
             SELECT 1 FROM usage_record old
             JOIN guest_provenance_occurrence_key k ON k.event_key=old.event_key AND k.effective=1
             WHERE old.source_id=NEW.source_id AND old.agent=NEW.agent AND old.kind='snapshot'
           )
         BEGIN
            INSERT INTO guest_provenance_mutation(
                mutation_seq,lineage_id,event_key,mutation_kind,replaced_event_key
            )
            SELECT l.last_mutation_seq+1,l.lineage_id,
                   (SELECT old.event_key FROM usage_record old
                    JOIN guest_provenance_occurrence_key k
                      ON k.event_key=old.event_key AND k.effective=1
                    WHERE old.source_id=NEW.source_id AND old.agent=NEW.agent
                      AND old.kind='snapshot' ORDER BY old.event_key LIMIT 1),
                   'effective_record_replaced',NEW.event_key
            FROM guest_provenance_lineage l
            WHERE l.singleton=1 AND l.last_mutation_seq<9223372036854775807;
            UPDATE guest_provenance_lineage
            SET eligible=0,
                last_mutation_seq=CASE WHEN last_mutation_seq<9223372036854775807
                                       THEN last_mutation_seq+1 ELSE last_mutation_seq END
            WHERE singleton=1;
            UPDATE guest_provenance_occurrence_key
            SET effective=0
            WHERE event_key IN (
              SELECT old.event_key FROM usage_record old
              WHERE old.source_id=NEW.source_id AND old.agent=NEW.agent AND old.kind='snapshot'
            );
         END;",
    )?;
    connection.execute(
        "INSERT OR IGNORE INTO guest_provenance_meta(singleton,seed_allowed) VALUES (1,?1)",
        [i64::from(allow_new_lineage)],
    )?;
    Ok(())
}

pub(crate) fn initialize_guest_provenance_in_transaction(
    transaction: &Transaction<'_>,
    now: DateTime<Utc>,
) -> Result<(), ScanError> {
    let seed_allowed: i64 = transaction.query_row(
        "SELECT seed_allowed FROM guest_provenance_meta WHERE singleton=1",
        [],
        |row| row.get(0),
    )?;
    if seed_allowed == 0 {
        return Ok(());
    }
    let already_initialized: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM guest_provenance_lineage WHERE singleton=1)",
        [],
        |row| row.get(0),
    )?;
    if already_initialized {
        transaction.execute(
            "UPDATE guest_provenance_meta SET seed_allowed=0 WHERE singleton=1",
            [],
        )?;
        return Ok(());
    }
    let old_usage_exists: bool =
        transaction.query_row("SELECT EXISTS(SELECT 1 FROM usage_record)", [], |row| {
            row.get(0)
        })?;
    if old_usage_exists {
        transaction.execute(
            "UPDATE guest_provenance_meta SET seed_allowed=0 WHERE singleton=1",
            [],
        )?;
        return Ok(());
    }
    let account_id: String = transaction.query_row(
        "SELECT value FROM setting WHERE key='planet_account_id'",
        [],
        |row| row.get(0),
    )?;
    let device_id: String = transaction.query_row(
        "SELECT value FROM setting WHERE key='planet_device_id'",
        [],
        |row| row.get(0),
    )?;
    let activation: String = transaction.query_row(
        "SELECT value FROM setting WHERE key='planet_activation_at_utc'",
        [],
        |row| row.get(0),
    )?;
    let current_cycle_id: String = transaction.query_row(
        "SELECT value FROM setting WHERE key='planet_current_cycle_id'",
        [],
        |row| row.get(0),
    )?;
    let current_cycle_start: String = transaction.query_row(
        "SELECT value FROM setting WHERE key='planet_cycle_started_at_utc'",
        [],
        |row| row.get(0),
    )?;
    let parsed_activation = match canonical_timestamp(&activation) {
        Ok(value) => value,
        Err(_) => {
            transaction.execute(
                "UPDATE guest_provenance_meta SET seed_allowed=0 WHERE singleton=1",
                [],
            )?;
            return Ok(());
        }
    };
    let parsed_cycle_start = match canonical_timestamp(&current_cycle_start) {
        Ok(value) => value,
        Err(_) => {
            transaction.execute(
                "UPDATE guest_provenance_meta SET seed_allowed=0 WHERE singleton=1",
                [],
            )?;
            return Ok(());
        }
    };
    if account_id != "local"
        || !is_canonical_uuid(&device_id)
        || !is_canonical_uuid(&current_cycle_id)
        || parsed_cycle_start != parsed_activation
        || now.to_rfc3339_opts(SecondsFormat::Micros, true) < parsed_activation
    {
        transaction.execute(
            "UPDATE guest_provenance_meta SET seed_allowed=0 WHERE singleton=1",
            [],
        )?;
        return Ok(());
    }
    let lineage_id = uuid::Uuid::new_v4().to_string();
    transaction.execute(
        "INSERT INTO guest_provenance_lineage(
            singleton,lineage_id,device_id,activated_at_utc,clock_policy,eligible,
            ingest_watermark,occurrence_count,last_mutation_seq,last_ingested_at_utc
         ) VALUES (1,?1,?2,?3,'guest_utc_monotonic_v1',1,0,0,0,NULL)",
        params![lineage_id, device_id, parsed_activation],
    )?;
    record_guest_cycle_baseline_in_transaction(
        transaction,
        &current_cycle_id,
        DateTime::parse_from_rfc3339(&parsed_cycle_start)
            .map_err(|_| ScanError::InvalidShopState)?
            .with_timezone(&Utc),
    )?;
    transaction.execute(
        "UPDATE guest_provenance_meta SET seed_allowed=0 WHERE singleton=1",
        [],
    )?;
    Ok(())
}

pub(crate) fn record_guest_cycle_baseline_in_transaction(
    transaction: &Transaction<'_>,
    cycle_id: &str,
    started_at: DateTime<Utc>,
) -> Result<(), ScanError> {
    let lineage: Option<(String, bool)> = transaction
        .query_row(
            "SELECT lineage_id,eligible FROM guest_provenance_lineage WHERE singleton=1",
            [],
            |row| Ok((row.get(0)?, row.get::<_, i64>(1)? == 1)),
        )
        .optional()?;
    let Some((lineage_id, eligible)) = lineage else {
        return Ok(());
    };
    if !eligible {
        return Ok(());
    }
    let started_at = canonical_timestamp(&started_at.to_rfc3339())?;
    let effects_json =
        serde_json::to_string(&ActiveEffects::default()).map_err(|_| ScanError::Database)?;
    let existing: Option<(String, String, Option<String>, i64, String, String)> = transaction
        .query_row(
            "SELECT lineage_id,started_at_utc,ended_at_utc,baseline_revision,
                    active_instance_ids_json,effects_json
             FROM guest_provenance_cycle WHERE cycle_id=?1",
            [cycle_id],
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
    if let Some((stored_lineage, stored_start, stored_end, revision, active_ids, effects)) =
        existing
    {
        if stored_lineage == lineage_id
            && stored_start == started_at
            && stored_end.is_none()
            && revision == 0
            && active_ids == "[]"
            && effects == effects_json
        {
            return Ok(());
        }
        set_ineligible(transaction)?;
        return Ok(());
    }
    transaction.execute(
        "INSERT INTO guest_provenance_cycle(
            cycle_id,lineage_id,started_at_utc,baseline_revision,
            active_instance_ids_json,effects_json
         ) VALUES (?1,?2,?3,0,'[]',?4)",
        params![cycle_id, lineage_id, started_at, effects_json],
    )?;
    Ok(())
}

pub(crate) fn record_guest_first_reset_in_transaction(
    transaction: &Transaction<'_>,
    receipt: &GuestFirstResetReceipt,
) -> Result<(), ScanError> {
    let lineage: Option<(String, bool, String, String)> = transaction
        .query_row(
            "SELECT lineage_id,eligible,device_id,activated_at_utc
             FROM guest_provenance_lineage WHERE singleton=1",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get::<_, i64>(1)? == 1,
                    row.get(2)?,
                    row.get(3)?,
                ))
            },
        )
        .optional()?;
    let Some((lineage_id, eligible, device_id, activation)) = lineage else {
        return Ok(());
    };
    if !eligible {
        return Ok(());
    }
    let prior_receipt: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM guest_provenance_reset_receipt WHERE lineage_id=?1)",
        [&lineage_id],
        |row| row.get(0),
    )?;
    if prior_receipt {
        set_ineligible(transaction)?;
        return Ok(());
    }

    let result = &receipt.result;
    let reset_at = match canonical_timestamp(&result.reset_at_utc) {
        Ok(value) => value,
        Err(_) => {
            set_ineligible(transaction)?;
            return Ok(());
        }
    };
    let parsed_reset = DateTime::parse_from_rfc3339(&reset_at)
        .map_err(|_| ScanError::InvalidShopState)?
        .with_timezone(&Utc);
    let parsed_activation = DateTime::parse_from_rfc3339(&activation)
        .map_err(|_| ScanError::InvalidShopState)?
        .with_timezone(&Utc);
    let reset_time_covers_occurrences = reset_time_covers_guest_occurrences(
        transaction,
        &lineage_id,
        &receipt.request.cycle_id,
        parsed_reset,
    )?;
    let expected_deadline = (parsed_reset + chrono::Duration::seconds(86_400))
        .to_rfc3339_opts(SecondsFormat::Micros, true);
    let active_cycle: Option<String> = transaction
        .query_row(
            "SELECT value FROM setting WHERE key='planet_current_cycle_id'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let active_cycle_start: Option<String> = transaction
        .query_row(
            "SELECT value FROM setting WHERE key='planet_cycle_started_at_utc'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let old_baseline: Option<(String, Option<String>, i64, String, String)> = transaction
        .query_row(
            "SELECT started_at_utc,ended_at_utc,baseline_revision,
                    active_instance_ids_json,effects_json
             FROM guest_provenance_cycle WHERE cycle_id=?1 AND lineage_id=?2",
            params![receipt.request.cycle_id, lineage_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()?;
    let effects_json =
        serde_json::to_string(&ActiveEffects::default()).map_err(|_| ScanError::Database)?;
    let old_bound_is_valid =
        old_baseline
            .as_ref()
            .is_some_and(|(_, ended, revision, active_ids, effects)| {
                ended.is_none() && *revision == 0 && active_ids == "[]" && effects == &effects_json
            });
    let new_cycle_exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM guest_provenance_cycle WHERE cycle_id=?1)",
        [&result.new_cycle_id],
        |row| row.get(0),
    )?;
    let reset_available_at = canonical_timestamp(&result.reset_available_at_utc).ok();
    if receipt.request.kind != GuestFirstResetRequestKind::ResetPlanet
        || !is_canonical_uuid(&receipt.request.request_id)
        || receipt.request.request_id != result.request_id
        || receipt.request.cycle_id != result.previous_cycle_id
        || receipt.request.cycle_id != receipt.result.previous_cycle_id
        || result.status != GuestFirstResetStatus::Reset
        || !is_canonical_uuid(&result.new_cycle_id)
        || result.previous_cycle_id == result.new_cycle_id
        || reset_at != result.reset_at_utc
        || parsed_reset <= parsed_activation
        || !reset_time_covers_occurrences
        || active_cycle.as_deref() != Some(result.new_cycle_id.as_str())
        || active_cycle_start.as_deref() != Some(reset_at.as_str())
        || result.final_effect_revision != 0
        || !result.final_active_instance_ids.is_empty()
        || result.final_effects != ActiveEffects::default()
        || result.frozen_deadline_before_reset_utc.is_some()
        || reset_available_at.as_deref() != Some(expected_deadline.as_str())
        || result.raw_tokens == 0
        || result.bonus_tokens != 0
        || result.credited_tokens != result.raw_tokens
        || !old_bound_is_valid
        || new_cycle_exists
        || device_id.is_empty()
    {
        set_ineligible(transaction)?;
        return Ok(());
    }

    let expected_started_at = old_baseline
        .as_ref()
        .map(|(started, _, _, _, _)| started.as_str())
        .ok_or(ScanError::InvalidShopState)?;
    let old_reset_time = canonical_timestamp(&result.reset_at_utc)?;
    transaction.execute(
        "UPDATE guest_provenance_cycle SET ended_at_utc=?3
         WHERE cycle_id=?1 AND lineage_id=?2 AND ended_at_utc IS NULL",
        params![receipt.request.cycle_id, lineage_id, old_reset_time],
    )?;
    let new_started = DateTime::parse_from_rfc3339(&reset_at)
        .map_err(|_| ScanError::InvalidShopState)?
        .with_timezone(&Utc);
    record_guest_cycle_baseline_in_transaction(transaction, &result.new_cycle_id, new_started)?;
    let stored_old_start: String = transaction.query_row(
        "SELECT started_at_utc FROM guest_provenance_cycle WHERE cycle_id=?1",
        [&receipt.request.cycle_id],
        |row| row.get(0),
    )?;
    if stored_old_start != expected_started_at {
        set_ineligible(transaction)?;
        return Ok(());
    }
    let receipt_json = serde_json::to_string(receipt).map_err(|_| ScanError::Database)?;
    transaction.execute(
        "INSERT INTO guest_provenance_reset_receipt(
            request_id,lineage_id,previous_cycle_id,new_cycle_id,reset_at_utc,receipt_json
         ) VALUES (?1,?2,?3,?4,?5,?6)",
        params![
            receipt.request.request_id,
            lineage_id,
            receipt.request.cycle_id,
            result.new_cycle_id,
            reset_at,
            receipt_json
        ],
    )?;
    Ok(())
}

#[allow(dead_code)] // Keep the transaction API for callers that own a Transaction handle.
pub(crate) fn record_guest_occurrence_in_transaction(
    transaction: &Transaction<'_>,
    record: &ParsedRecord,
    now: DateTime<Utc>,
) -> Result<(), ScanError> {
    record_guest_occurrence_in_connection(transaction, record, now)
}

pub(crate) fn record_guest_occurrence_in_connection(
    connection: &Connection,
    record: &ParsedRecord,
    now: DateTime<Utc>,
) -> Result<(), ScanError> {
    let account_id: Option<String> = connection
        .query_row(
            "SELECT value FROM setting WHERE key='planet_account_id'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if account_id.as_deref() != Some("local") {
        return Ok(());
    }
    let raw: Option<(
        String,
        String,
        String,
        String,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        String,
        i64,
    )> = connection
        .query_row(
            "SELECT source_id,agent,kind,bucket_date,input_tokens,output_tokens,
                    cache_read_tokens,cache_write_tokens,total_tokens,coverage,parser_version
             FROM usage_record WHERE event_key=?1",
            [&record.event_key],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                    row.get(10)?,
                ))
            },
        )
        .optional()?;
    let Some((
        source_id,
        agent,
        kind,
        bucket_date,
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        total_tokens,
        coverage,
        parser_version,
    )) = raw
    else {
        return Ok(());
    };
    let Some((
        lineage_id,
        lineage_device,
        activated_at,
        eligible,
        watermark,
        occurrence_count,
        last_ingested,
    )): Option<(String, String, String, i64, i64, i64, Option<String>)> = connection
        .query_row(
            "SELECT lineage_id,device_id,activated_at_utc,eligible,ingest_watermark,
                        occurrence_count,last_ingested_at_utc
                 FROM guest_provenance_lineage WHERE singleton=1",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .optional()?
    else {
        return Ok(());
    };
    let current_device: String = connection.query_row(
        "SELECT value FROM setting WHERE key='planet_device_id'",
        [],
        |row| row.get(0),
    )?;
    let cycle_id: String = connection.query_row(
        "SELECT value FROM setting WHERE key='planet_current_cycle_id'",
        [],
        |row| row.get(0),
    )?;
    let occurred_at_utc = match canonical_timestamp(&record.occurred_at_utc.to_rfc3339()) {
        Ok(value) => value,
        Err(_) => {
            set_ineligible(connection)?;
            return Ok(());
        }
    };
    let ingested_at_utc = now.to_rfc3339_opts(SecondsFormat::Micros, true);
    let fingerprint = content_fingerprint(
        &source_id,
        &agent,
        &kind,
        &bucket_date,
        &occurred_at_utc,
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        total_tokens,
        &coverage,
        parser_version,
    )?;
    let existing: Option<(String, i64, i64, String, i64)> = connection
        .query_row(
            "SELECT occurrence_id,current_record_version,ingest_seq,content_fingerprint,effective
             FROM guest_provenance_occurrence_key WHERE event_key=?1",
            [&record.event_key],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()?;
    let parsed_activation = DateTime::parse_from_rfc3339(&activated_at)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| ScanError::Database)?;
    let occurrence_time = DateTime::parse_from_rfc3339(&occurred_at_utc)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| ScanError::Database)?;
    let assigned_cycle: Option<String> = connection
        .query_row(
            "SELECT cycle_id FROM guest_provenance_cycle
             WHERE lineage_id=?1 AND started_at_utc<=?2
               AND (ended_at_utc IS NULL OR ?2<ended_at_utc)
             ORDER BY started_at_utc DESC LIMIT 1",
            params![lineage_id, occurred_at_utc],
            |row| row.get(0),
        )
        .optional()?;
    let (occurrence_cycle_id, missing_cycle_bound) = match assigned_cycle {
        Some(cycle_id) => (cycle_id, false),
        None => (cycle_id.clone(), true),
    };
    let ingestion_time = DateTime::parse_from_rfc3339(&ingested_at_utc)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| ScanError::Database)?;
    let previous_ingestion = last_ingested
        .as_deref()
        .map(DateTime::parse_from_rfc3339)
        .transpose()
        .map_err(|_| ScanError::Database)?
        .map(|value| value.with_timezone(&Utc));
    let total_is_valid = total_tokens.is_some_and(|value| value >= 0);
    let lifecycle_failure = if current_device != lineage_device {
        Some("device_mismatch")
    } else if !is_canonical_uuid(&current_device) {
        Some("invalid_device_id")
    } else if !is_canonical_uuid(&cycle_id) {
        Some("invalid_cycle_id")
    } else if occurrence_time < parsed_activation {
        Some("before_activation")
    } else if previous_ingestion.is_some_and(|previous| ingestion_time < previous) {
        Some("clock_regression")
    } else if occurrence_time > ingestion_time {
        Some("occurrence_after_ingestion")
    } else if missing_cycle_bound {
        Some("occurrence_outside_cycle_bounds")
    } else if coverage != coverage_name(UsageCoverage::Complete) {
        Some("incomplete_coverage")
    } else if !total_is_valid {
        Some("invalid_token_count")
    } else {
        None
    };
    if lifecycle_failure.is_some() {
        set_ineligible(connection)?;
    }
    let effective = record_is_effective(connection, &record.event_key, &source_id, &agent, &kind)?;
    if !effective {
        set_ineligible(connection)?;
        if existing.is_none() {
            append_mutation(
                connection,
                &lineage_id,
                &record.event_key,
                "superseded_snapshot",
                None,
            )?;
        }
    }

    if let Some((occurrence_id, current_version, ingest_seq, old_fingerprint, old_effective)) =
        existing
    {
        if old_fingerprint == fingerprint && old_effective == i64::from(effective) {
            if let Some(reason) = lifecycle_failure {
                append_mutation(connection, &lineage_id, &record.event_key, reason, None)?;
            }
            return Ok(());
        }
        let Some(next_version) = current_version.checked_add(1) else {
            set_ineligible(connection)?;
            append_mutation(
                connection,
                &lineage_id,
                &record.event_key,
                "record_version_overflow",
                None,
            )?;
            return Ok(());
        };
        let has_correction: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM guest_provenance_mutation
             WHERE event_key=?1 AND mutation_kind IN ('content_correction','delete','effective_record_replaced'))",
            [&record.event_key],
            |row| row.get(0),
        )?;
        if !has_correction {
            append_mutation(
                connection,
                &lineage_id,
                &record.event_key,
                "content_correction",
                None,
            )?;
        }
        if let Some(reason) = lifecycle_failure {
            append_mutation(connection, &lineage_id, &record.event_key, reason, None)?;
        }
        set_ineligible(connection)?;
        insert_occurrence_version(
            connection,
            &occurrence_id,
            next_version,
            ingest_seq,
            &lineage_id,
            &current_device,
            &agent,
            &occurred_at_utc,
            &ingested_at_utc,
            &occurrence_cycle_id,
            total_tokens,
            &coverage,
        )?;
        connection.execute(
            "UPDATE guest_provenance_occurrence_key
             SET current_record_version=?2,content_fingerprint=?3,effective=?4
             WHERE event_key=?1",
            params![
                record.event_key,
                next_version,
                fingerprint,
                i64::from(effective)
            ],
        )?;
        update_ingestion_clock(connection, &ingested_at_utc, previous_ingestion, eligible)?;
        return Ok(());
    }

    let Some(ingest_seq) = watermark.checked_add(1) else {
        set_ineligible(connection)?;
        append_mutation(
            connection,
            &lineage_id,
            &record.event_key,
            "ingest_sequence_overflow",
            None,
        )?;
        return Ok(());
    };
    let Some(next_occurrence_count) = occurrence_count.checked_add(1) else {
        set_ineligible(connection)?;
        append_mutation(
            connection,
            &lineage_id,
            &record.event_key,
            "occurrence_count_overflow",
            None,
        )?;
        return Ok(());
    };
    let current_mutation_seq: i64 = connection.query_row(
        "SELECT last_mutation_seq FROM guest_provenance_lineage WHERE singleton=1",
        [],
        |row| row.get(0),
    )?;
    let Some(mutation_seq) = current_mutation_seq.checked_add(1) else {
        set_ineligible(connection)?;
        return Ok(());
    };
    let occurrence_id = uuid::Uuid::new_v4().to_string();
    connection.execute(
        "INSERT INTO guest_provenance_occurrence_key(
            event_key,occurrence_id,ingest_seq,current_record_version,content_fingerprint,effective
         ) VALUES (?1,?2,?3,1,?4,?5)",
        params![
            record.event_key,
            occurrence_id,
            ingest_seq,
            fingerprint,
            i64::from(effective)
        ],
    )?;
    insert_occurrence_version(
        connection,
        &occurrence_id,
        1,
        ingest_seq,
        &lineage_id,
        &current_device,
        &agent,
        &occurred_at_utc,
        &ingested_at_utc,
        &occurrence_cycle_id,
        total_tokens,
        &coverage,
    )?;
    connection.execute(
        "INSERT INTO guest_provenance_mutation(mutation_seq,lineage_id,event_key,mutation_kind)
         VALUES (?1,?2,?3,'occurrence_inserted')",
        params![mutation_seq, lineage_id, record.event_key],
    )?;
    connection.execute(
        "UPDATE guest_provenance_lineage
         SET ingest_watermark=?1,occurrence_count=?2,last_mutation_seq=?3
         WHERE singleton=1",
        params![ingest_seq, next_occurrence_count, mutation_seq],
    )?;
    update_ingestion_clock(connection, &ingested_at_utc, previous_ingestion, eligible)?;
    if let Some(reason) = lifecycle_failure {
        append_mutation(connection, &lineage_id, &record.event_key, reason, None)?;
    }
    Ok(())
}

fn insert_occurrence_version(
    connection: &Connection,
    occurrence_id: &str,
    record_version: i64,
    ingest_seq: i64,
    lineage_id: &str,
    device_id: &str,
    agent: &str,
    occurred_at_utc: &str,
    ingested_at_utc: &str,
    cycle_id: &str,
    total_tokens: Option<i64>,
    coverage: &str,
) -> Result<(), ScanError> {
    connection.execute(
        "INSERT INTO guest_provenance_occurrence_version(
            occurrence_id,record_version,ingest_seq,lineage_id,device_id,agent,
            occurred_at_utc,ingested_at_utc,cycle_id,total_tokens,coverage
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            occurrence_id,
            record_version,
            ingest_seq,
            lineage_id,
            device_id,
            agent,
            occurred_at_utc,
            ingested_at_utc,
            cycle_id,
            total_tokens,
            coverage,
        ],
    )?;
    Ok(())
}

fn record_is_effective(
    connection: &Connection,
    event_key: &str,
    source_id: &str,
    agent: &str,
    kind: &str,
) -> Result<bool, ScanError> {
    if kind != "snapshot" {
        return Ok(true);
    }
    let has_response: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM usage_record
         WHERE source_id=?1 AND agent=?2 AND kind='response' AND event_key<>?3)",
        params![source_id, agent, event_key],
        |row| row.get(0),
    )?;
    Ok(!has_response)
}

fn update_ingestion_clock(
    connection: &Connection,
    ingested_at_utc: &str,
    previous_ingestion: Option<DateTime<Utc>>,
    eligible_before: i64,
) -> Result<(), ScanError> {
    let parsed = DateTime::parse_from_rfc3339(ingested_at_utc)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| ScanError::Database)?;
    if previous_ingestion.is_none_or(|previous| parsed > previous) {
        connection.execute(
            "UPDATE guest_provenance_lineage SET last_ingested_at_utc=?1 WHERE singleton=1",
            [ingested_at_utc],
        )?;
    }
    if eligible_before == 0 {
        set_ineligible(connection)?;
    }
    Ok(())
}

fn append_mutation(
    connection: &Connection,
    lineage_id: &str,
    event_key: &str,
    kind: &str,
    replaced_event_key: Option<&str>,
) -> Result<(), ScanError> {
    let current: Option<i64> = connection
        .query_row(
            "SELECT last_mutation_seq FROM guest_provenance_lineage WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let Some(current) = current else {
        return Ok(());
    };
    let Some(sequence) = current.checked_add(1) else {
        set_ineligible(connection)?;
        return Ok(());
    };
    connection.execute(
        "INSERT INTO guest_provenance_mutation(
            mutation_seq,lineage_id,event_key,mutation_kind,replaced_event_key
         ) VALUES (?1,?2,?3,?4,?5)",
        params![sequence, lineage_id, event_key, kind, replaced_event_key],
    )?;
    connection.execute(
        "UPDATE guest_provenance_lineage SET eligible=0,last_mutation_seq=?1 WHERE singleton=1",
        [sequence],
    )?;
    Ok(())
}

fn set_ineligible(connection: &Connection) -> Result<(), ScanError> {
    connection.execute(
        "UPDATE guest_provenance_lineage SET eligible=0 WHERE singleton=1",
        [],
    )?;
    Ok(())
}

fn canonical_timestamp(value: &str) -> Result<String, ScanError> {
    let parsed = DateTime::parse_from_rfc3339(value).map_err(|_| ScanError::InvalidShopState)?;
    Ok(parsed
        .with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Micros, true))
}

pub(super) fn reset_time_covers_guest_occurrences(
    connection: &Connection,
    lineage_id: &str,
    cycle_id: &str,
    reset_at: DateTime<Utc>,
) -> Result<bool, ScanError> {
    let last_ingested: Option<String> = connection.query_row(
        "SELECT last_ingested_at_utc FROM guest_provenance_lineage
         WHERE singleton=1 AND lineage_id=?1",
        [lineage_id],
        |row| row.get(0),
    )?;
    let Some(last_ingested) = last_ingested
        .as_deref()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
    else {
        return Ok(false);
    };
    if reset_at < last_ingested {
        return Ok(false);
    }

    let latest_occurrence: Option<String> = connection.query_row(
        "SELECT max(v.occurred_at_utc)
         FROM guest_provenance_occurrence_key k
         JOIN guest_provenance_occurrence_version v
           ON v.occurrence_id=k.occurrence_id
          AND v.record_version=k.current_record_version
         WHERE v.lineage_id=?1 AND v.cycle_id=?2 AND k.effective=1",
        params![lineage_id, cycle_id],
        |row| row.get(0),
    )?;
    if let Some(latest_occurrence) = latest_occurrence {
        let Ok(latest_occurrence) = DateTime::parse_from_rfc3339(&latest_occurrence) else {
            return Ok(false);
        };
        let latest_occurrence = latest_occurrence.with_timezone(&Utc);
        // Cycle bounds are half-open: an occurrence at reset belongs to the new cycle.
        if reset_at <= latest_occurrence {
            return Ok(false);
        }
    }
    Ok(true)
}

fn is_canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|parsed| !parsed.is_nil() && parsed.to_string() == value)
}

fn content_fingerprint(
    source_id: &str,
    agent: &str,
    kind: &str,
    bucket_date: &str,
    occurred_at_utc: &str,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cache_read_tokens: Option<i64>,
    cache_write_tokens: Option<i64>,
    total_tokens: Option<i64>,
    coverage: &str,
    parser_version: i64,
) -> Result<String, ScanError> {
    let fingerprint = StoredOccurrenceFingerprint {
        source_id,
        agent,
        kind,
        bucket_date,
        occurred_at_utc,
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        total_tokens,
        coverage,
        parser_version,
    };
    let bytes = serde_json::to_vec(&fingerprint).map_err(|_| ScanError::Database)?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
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
