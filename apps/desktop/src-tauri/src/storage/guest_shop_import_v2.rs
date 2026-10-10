use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::domain::{
    cosmetic_shop::{ActiveEffects, GuestShopImportData, ShopCycleBound},
    guest_shop_import::{
        canonical_json_bytes, prefix_fingerprint, source_fingerprint, GuestClockPolicy,
        GuestFirstResetReceipt, GuestFirstResetRequestKind, GuestFirstResetStatus,
        GuestImportPhase, GuestImportSourceRelation, GuestOccurrence, GuestOccurrenceCoverage,
        GuestProvenanceAuthority, GuestProvenanceBaseline, GuestProvenanceCycleBound,
        GuestProvenanceDomain, GuestProvenanceV1, GuestShopImportV2Request,
        GuestShopImportV2Snapshot, GuestShopImportV2Status,
    },
    usage::Agent,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GuestImportCompletion {
    Imported,
    ImportedWithCorrectionHold,
    Held,
}

use super::{
    ledger::{Ledger, ScanError},
    shop_import,
};

fn stored_u64(value: i64) -> Result<u64, ScanError> {
    u64::try_from(value).map_err(|_| ScanError::InvalidCount)
}

fn read_guest_provenance(
    connection: &Connection,
    captured_at_utc: &str,
) -> Result<Option<GuestProvenanceV1>, ScanError> {
    let lineage: Option<(String, String, String, i64, i64, i64, i64)> = connection
        .query_row(
            "SELECT lineage_id,device_id,activated_at_utc,eligible,ingest_watermark,
                    occurrence_count,last_mutation_seq
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
        .optional()?;
    let Some((lineage_id, device_id, activated_at_utc, eligible, watermark, count, last_mutation)) =
        lineage
    else {
        return Ok(None);
    };
    if eligible != 1 {
        return Ok(None);
    }
    let watermark = stored_u64(watermark)?;
    let count = stored_u64(count)?;
    let last_mutation = stored_u64(last_mutation)?;
    let (mutation_count, non_insert_count): (i64, i64) = connection.query_row(
        "SELECT count(*),sum(CASE WHEN mutation_kind!='occurrence_inserted' THEN 1 ELSE 0 END)
         FROM guest_provenance_mutation WHERE lineage_id=?1",
        [&lineage_id],
        |row| Ok((row.get(0)?, row.get::<_, Option<i64>>(1)?.unwrap_or(0))),
    )?;
    if watermark == 0
        || watermark != count
        || last_mutation != watermark
        || stored_u64(mutation_count)? != watermark
        || non_insert_count != 0
    {
        return Ok(None);
    }

    let mut occurrence_statement = connection.prepare(
        "SELECT k.occurrence_id,k.current_record_version,k.ingest_seq,v.device_id,v.agent,
                v.occurred_at_utc,v.ingested_at_utc,v.cycle_id,v.total_tokens,v.coverage
         FROM guest_provenance_occurrence_key k
         JOIN guest_provenance_occurrence_version v
           ON v.occurrence_id=k.occurrence_id
          AND v.record_version=k.current_record_version
         WHERE k.effective=1 ORDER BY k.ingest_seq",
    )?;
    let occurrence_rows = occurrence_statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, Option<i64>>(8)?,
                row.get::<_, String>(9)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut occurrences = Vec::with_capacity(occurrence_rows.len());
    for (
        occurrence_id,
        record_version,
        ingest_seq,
        occurrence_device_id,
        agent,
        occurred_at_utc,
        ingested_at_utc,
        cycle_id,
        total_tokens,
        coverage,
    ) in occurrence_rows
    {
        let Ok(agent) = serde_json::from_value::<Agent>(serde_json::Value::String(agent)) else {
            return Ok(None);
        };
        let (Some(total_tokens), true) = (total_tokens, coverage == "complete") else {
            return Ok(None);
        };
        occurrences.push(GuestOccurrence {
            occurrence_id,
            record_version: stored_u64(record_version)?,
            ingest_seq: stored_u64(ingest_seq)?,
            device_id: occurrence_device_id,
            agent,
            occurred_at_utc,
            ingested_at_utc,
            cycle_id,
            total_tokens: stored_u64(total_tokens)?,
            coverage: GuestOccurrenceCoverage::Complete,
        });
    }
    if stored_u64(i64::try_from(occurrences.len()).map_err(|_| ScanError::InvalidCount)?)? != count
        || occurrences
            .iter()
            .enumerate()
            .any(|(index, occurrence)| occurrence.ingest_seq != (index + 1) as u64)
    {
        return Ok(None);
    }

    let mut cycle_statement = connection.prepare(
        "SELECT cycle_id,started_at_utc,ended_at_utc,baseline_revision,
                active_instance_ids_json,effects_json
         FROM guest_provenance_cycle WHERE lineage_id=?1 ORDER BY started_at_utc",
    )?;
    let cycle_rows = cycle_statement
        .query_map([&lineage_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut cycle_bounds = Vec::with_capacity(cycle_rows.len());
    let mut baselines = Vec::with_capacity(cycle_rows.len());
    for (cycle_id, started_at_utc, ended_at_utc, revision, active_ids_json, effects_json) in
        cycle_rows
    {
        if revision != 0 {
            return Ok(None);
        }
        let active_instance_ids: Vec<String> =
            serde_json::from_str(&active_ids_json).map_err(|_| ScanError::Database)?;
        let effects: ActiveEffects =
            serde_json::from_str(&effects_json).map_err(|_| ScanError::Database)?;
        cycle_bounds.push(GuestProvenanceCycleBound {
            cycle_id: cycle_id.clone(),
            started_at_utc: started_at_utc.clone(),
            ended_at_utc: ended_at_utc.clone(),
        });
        baselines.push(GuestProvenanceBaseline {
            cycle_id,
            revision: 0,
            started_at_utc,
            ended_at_utc,
            active_instance_ids,
            effects,
        });
    }
    let receipt_rows: Vec<String> = {
        let mut statement = connection.prepare(
            "SELECT receipt_json FROM guest_provenance_reset_receipt
             WHERE lineage_id=?1 ORDER BY reset_at_utc",
        )?;
        let rows = statement
            .query_map([&lineage_id], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    if receipt_rows.len() != 1 || cycle_bounds.len() != 2 {
        return Ok(None);
    }
    let reset_receipt: GuestFirstResetReceipt =
        serde_json::from_str(&receipt_rows[0]).map_err(|_| ScanError::Database)?;
    let candidate = GuestProvenanceV1 {
        version: 1,
        domain: GuestProvenanceDomain::OrdinaryFirstResetZeroEffectV1,
        authority: GuestProvenanceAuthority::ClientSelfReported,
        lineage_id,
        device_id,
        activated_at_utc,
        clock_policy: GuestClockPolicy::GuestUtcMonotonicV1,
        ingest_watermark: watermark,
        occurrence_count: count,
        prefix_fingerprint: String::new(),
        occurrences,
        cycle_bounds,
        baselines,
        reset_receipt,
    };
    if !is_valid_first_reset_provenance(&candidate, captured_at_utc) {
        return Ok(None);
    }
    let mut provenance = candidate;
    provenance.prefix_fingerprint = match prefix_fingerprint(&provenance) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    let encoded = serde_json::to_value(&provenance).map_err(|_| ScanError::Database)?;
    match serde_json::from_value::<GuestProvenanceV1>(encoded) {
        Ok(validated) => Ok(Some(validated)),
        Err(_) => Ok(None),
    }
}

fn is_valid_first_reset_provenance(provenance: &GuestProvenanceV1, captured_at_utc: &str) -> bool {
    let receipt = &provenance.reset_receipt;
    if receipt.request.kind != GuestFirstResetRequestKind::ResetPlanet
        || receipt.result.status != GuestFirstResetStatus::Reset
        || receipt.request.request_id != receipt.result.request_id
        || receipt.request.cycle_id != receipt.result.previous_cycle_id
        || receipt.request.cycle_id == receipt.result.new_cycle_id
        || receipt.result.final_effect_revision != 0
        || !receipt.result.final_active_instance_ids.is_empty()
        || receipt.result.final_effects != ActiveEffects::default()
        || receipt.result.frozen_deadline_before_reset_utc.is_some()
        || receipt.result.bonus_tokens != 0
        || receipt.result.raw_tokens == 0
        || receipt.result.raw_tokens != receipt.result.credited_tokens
        || provenance.cycle_bounds.len() != 2
        || provenance.baselines.len() != 2
    {
        return false;
    }
    let old = provenance
        .cycle_bounds
        .iter()
        .find(|bound| bound.cycle_id == receipt.result.previous_cycle_id);
    let new = provenance
        .cycle_bounds
        .iter()
        .find(|bound| bound.cycle_id == receipt.result.new_cycle_id);
    let Some(old) = old else { return false };
    let Some(new) = new else { return false };
    let (Some(activation), Some(reset_at), Some(captured_at)) = (
        parse_utc(&provenance.activated_at_utc),
        parse_utc(&receipt.result.reset_at_utc),
        parse_utc(captured_at_utc),
    ) else {
        return false;
    };
    old.ended_at_utc.as_deref() == Some(receipt.result.reset_at_utc.as_str())
        && new.started_at_utc == receipt.result.reset_at_utc
        && new.ended_at_utc.is_none()
        && activation < reset_at
        && reset_at <= captured_at
        && provenance.occurrences.iter().all(|occurrence| {
            let (Some(occurred_at), Some(ingested_at)) = (
                parse_utc(&occurrence.occurred_at_utc),
                parse_utc(&occurrence.ingested_at_utc),
            ) else {
                return false;
            };
            occurrence.device_id == provenance.device_id
                && activation <= occurred_at
                && occurred_at <= ingested_at
                && ingested_at <= captured_at
                && if occurrence.cycle_id == receipt.result.previous_cycle_id {
                    occurred_at < reset_at
                } else {
                    occurrence.cycle_id == receipt.result.new_cycle_id && occurred_at >= reset_at
                }
        })
        && provenance
            .occurrences
            .iter()
            .filter(|occurrence| occurrence.cycle_id == receipt.result.previous_cycle_id)
            .try_fold(0_u64, |sum, occurrence| {
                sum.checked_add(occurrence.total_tokens)
            })
            == Some(receipt.result.raw_tokens)
        && provenance.baselines.iter().all(|baseline| {
            baseline.revision == 0
                && baseline.active_instance_ids.is_empty()
                && baseline.effects == ActiveEffects::default()
        })
}

fn parse_utc(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|parsed| parsed.with_timezone(&Utc))
}

pub(super) fn capture_guest_shop_import_request(
    ledger: &mut Ledger,
    target_account_id: &str,
) -> Result<Option<GuestShopImportV2Status>, ScanError> {
    shop_import::validate_target_account(target_account_id)?;
    let transaction = ledger
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)?;

    if capture_table_exists(&transaction)? {
        ensure_source_manifest_column(&transaction)?;
        let existing: Option<(String, String, String, String, i64, Option<String>)> = transaction
            .query_row(
                "SELECT import_id,source_fingerprint,request_json,phase,correction_hold,
                        source_manifest_json
                 FROM guest_shop_import_v2_capture WHERE target_account_id=?1",
                [target_account_id],
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
        if let Some((
            import_id,
            stored_fingerprint,
            request_json,
            phase,
            correction_hold,
            source_manifest_json,
        )) = existing
        {
            let request: GuestShopImportV2Request = match serde_json::from_str(&request_json) {
                Ok(request) => request,
                Err(_) => {
                    persist_capture_hold(&transaction, &import_id)?;
                    transaction.commit()?;
                    return Err(ScanError::InvalidShopState);
                }
            };
            if validate_stored_request(
                &request,
                &import_id,
                target_account_id,
                &stored_fingerprint,
                &request_json,
            )
            .is_err()
            {
                persist_capture_hold(&transaction, &import_id)?;
                transaction.commit()?;
                return Err(ScanError::InvalidShopState);
            }
            let mut phase = parse_capture_phase(&phase)?;
            let correction_hold = match correction_hold {
                0 => false,
                1 => true,
                _ => return Err(ScanError::InvalidShopState),
            };
            let relation =
                classify_source_relation(&transaction, &request, source_manifest_json.as_deref())?;
            let correction_hold = persist_correction_hold(
                &transaction,
                &import_id,
                relation,
                &mut phase,
                correction_hold,
            )?;
            transaction.commit()?;
            return Ok(Some(GuestShopImportV2Status {
                request,
                source_relation: relation,
                phase,
                correction_hold,
            }));
        }
    }

    if capture_table_exists(&transaction)? {
        let other_target_capture: Option<String> = transaction
            .query_row(
                "SELECT import_id FROM guest_shop_import_v2_capture
                 WHERE target_account_id<>?1 ORDER BY target_account_id LIMIT 1",
                [target_account_id],
                |row| row.get(0),
            )
            .optional()?;
        if other_target_capture.is_some() {
            transaction.commit()?;
            return Err(ScanError::InvalidShopState);
        }
    }

    if shop_import::has_legacy_capture(&transaction, target_account_id)? {
        transaction.commit()?;
        return Ok(None);
    }

    let captured_at_utc = Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true);
    if read_guest_provenance(&transaction, &captured_at_utc)?.is_none() {
        transaction.commit()?;
        return Ok(None);
    }
    Ledger::prepare_growth_journal_in_transaction(&transaction)?;
    let raw = Ledger::planet_device_contribution_from_connection(&transaction, false)?;
    let canonical_payload =
        super::shop_effects::capture_shop_device_contribution_from_connection(&transaction, raw)?;
    let Some((data, provenance)) = collect_v2_domain_source_at(&transaction, &captured_at_utc)?
    else {
        transaction.commit()?;
        return Ok(None);
    };
    let source_manifest = read_current_source_manifest(&transaction)?.ok_or(ScanError::Database)?;
    let import_id = uuid::Uuid::new_v4().to_string();
    let initial_phase = if has_positive_new_cycle_usage(&provenance) {
        "held"
    } else {
        "captured"
    };
    let mut snapshot = GuestShopImportV2Snapshot {
        import_id: import_id.clone(),
        target_account_id: target_account_id.to_owned(),
        source_account_id: "local".to_owned(),
        source_fingerprint: String::new(),
        disposition: v2_disposition(&data, &provenance),
        captured_at_utc,
        provenance,
        canonical_payload,
        data,
    };
    snapshot.source_fingerprint = source_fingerprint(&snapshot).map_err(|_| ScanError::Database)?;
    let source_fingerprint = snapshot.source_fingerprint.clone();
    let request = GuestShopImportV2Request {
        schema_version: 2,
        snapshot,
    };
    if !source_manifest_matches_request(&source_manifest, &request) {
        return Err(ScanError::InvalidShopState);
    }
    let source_manifest_json =
        serde_json::to_string(&source_manifest).map_err(|_| ScanError::Database)?;
    let request_bytes = canonical_json_bytes(&request).map_err(|_| ScanError::Database)?;
    let request_json = String::from_utf8(request_bytes).map_err(|_| ScanError::Database)?;

    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS guest_shop_import_v2_capture (
            target_account_id TEXT PRIMARY KEY,
            import_id TEXT NOT NULL UNIQUE,
            source_fingerprint TEXT NOT NULL,
            request_json TEXT NOT NULL,
            phase TEXT NOT NULL CHECK(phase IN ('captured','attempt_started','held','imported')),
            correction_hold INTEGER NOT NULL CHECK(correction_hold IN (0,1)),
            source_manifest_json TEXT NOT NULL
         );",
    )?;
    transaction.execute(
        "INSERT INTO guest_shop_import_v2_capture(
            target_account_id,import_id,source_fingerprint,request_json,phase,correction_hold,
            source_manifest_json
         ) VALUES (?1,?2,?3,?4,?5,0,?6)",
        params![
            target_account_id,
            import_id,
            source_fingerprint,
            request_json,
            initial_phase,
            source_manifest_json,
        ],
    )?;
    transaction.commit()?;
    Ok(Some(GuestShopImportV2Status {
        request,
        source_relation: GuestImportSourceRelation::Exact,
        phase: parse_capture_phase(initial_phase)?,
        correction_hold: false,
    }))
}

impl Ledger {
    /// Persists the attempt boundary before the immutable captured request is dispatched.
    pub fn mark_guest_shop_import_attempt_started(
        &mut self,
        target_account_id: &str,
        import_id: uuid::Uuid,
    ) -> Result<(), ScanError> {
        shop_import::validate_target_account(target_account_id)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let selected_account: Option<String> = transaction
            .query_row(
                "SELECT value FROM setting WHERE key='selected_auth_account_id'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if selected_account.as_deref() != Some(target_account_id) {
            return Err(ScanError::InvalidShopState);
        }
        let stored: Option<(String, String)> = transaction
            .query_row(
                "SELECT import_id,phase FROM guest_shop_import_v2_capture
                 WHERE target_account_id=?1",
                [target_account_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((stored_import_id, phase)) = stored else {
            return Err(ScanError::InvalidShopState);
        };
        if stored_import_id != import_id.to_string() {
            return Err(ScanError::InvalidShopState);
        }
        match parse_capture_phase(&phase)? {
            GuestImportPhase::Captured => {
                transaction.execute(
                    "UPDATE guest_shop_import_v2_capture SET phase='attempt_started'
                     WHERE target_account_id=?1 AND import_id=?2 AND phase='captured'",
                    params![target_account_id, import_id.to_string()],
                )?;
            }
            GuestImportPhase::AttemptStarted => {}
            GuestImportPhase::Held | GuestImportPhase::Imported => {
                return Err(ScanError::InvalidShopState);
            }
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn complete_guest_shop_import(
        &mut self,
        target_account_id: &str,
        result: &crate::domain::guest_shop_import::GuestShopImportV2Result,
    ) -> Result<GuestImportCompletion, ScanError> {
        shop_import::validate_target_account(target_account_id)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_source_manifest_column(&transaction)?;
        let columns = {
            let mut statement =
                transaction.prepare("PRAGMA table_info(guest_shop_import_v2_capture)")?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        if !columns.iter().any(|column| column == "result_json") {
            transaction.execute(
                "ALTER TABLE guest_shop_import_v2_capture ADD COLUMN result_json TEXT",
                [],
            )?;
        }

        let selected_account: Option<String> = transaction
            .query_row(
                "SELECT value FROM setting WHERE key='selected_auth_account_id'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if selected_account.as_deref() != Some(target_account_id) {
            transaction.rollback()?;
            return Ok(GuestImportCompletion::Held);
        }

        let stored: Option<(
            String,
            String,
            String,
            String,
            i64,
            Option<String>,
            Option<String>,
        )> = transaction
            .query_row(
                "SELECT import_id,source_fingerprint,request_json,phase,correction_hold,
                        source_manifest_json,result_json
                 FROM guest_shop_import_v2_capture WHERE target_account_id=?1",
                [target_account_id],
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
            .optional()?;
        let Some((import_id, fingerprint, request_json, phase, held, manifest, stored_result)) =
            stored
        else {
            return Err(ScanError::InvalidShopState);
        };
        let request: GuestShopImportV2Request =
            serde_json::from_str(&request_json).map_err(|_| ScanError::InvalidShopState)?;
        validate_stored_request(
            &request,
            &import_id,
            target_account_id,
            &fingerprint,
            &request_json,
        )?;
        crate::sync::client::validate_guest_shop_import_v2_result(&request, result)
            .map_err(|_| ScanError::InvalidShopState)?;
        let phase = parse_capture_phase(&phase)?;
        let held = match held {
            0 => false,
            1 => true,
            _ => return Err(ScanError::InvalidShopState),
        };
        let result_json = String::from_utf8(
            canonical_json_bytes(result).map_err(|_| ScanError::InvalidShopState)?,
        )
        .map_err(|_| ScanError::InvalidShopState)?;

        if phase == GuestImportPhase::Imported {
            if stored_result.as_deref() != Some(result_json.as_str()) {
                return Err(ScanError::InvalidShopState);
            }
            transaction.rollback()?;
            return Ok(if held {
                GuestImportCompletion::ImportedWithCorrectionHold
            } else {
                GuestImportCompletion::Imported
            });
        }
        if phase != GuestImportPhase::AttemptStarted {
            return Err(ScanError::InvalidShopState);
        }

        if result.status != crate::domain::guest_shop_import::GuestImportStatus::Imported {
            transaction.execute(
                "UPDATE guest_shop_import_v2_capture
                 SET phase='held',correction_hold=1
                 WHERE target_account_id=?1 AND import_id=?2",
                params![target_account_id, import_id],
            )?;
            transaction.commit()?;
            return Ok(GuestImportCompletion::Held);
        }
        let shop_state = result
            .shop_state
            .as_ref()
            .ok_or(ScanError::InvalidShopState)?;
        let planet_state = result
            .planet_state
            .as_ref()
            .ok_or(ScanError::InvalidShopState)?;
        let timeline = result
            .effect_timeline
            .as_ref()
            .ok_or(ScanError::InvalidShopState)?;
        let canonical = result
            .canonical_contribution
            .as_ref()
            .ok_or(ScanError::InvalidShopState)?;
        let journal = result
            .journal_confirmation
            .as_ref()
            .ok_or(ScanError::InvalidShopState)?;
        let ack = result.ack.as_ref().ok_or(ScanError::InvalidShopState)?;

        let active_account: String = transaction.query_row(
            "SELECT value FROM setting WHERE key='planet_account_id'",
            [],
            |row| row.get(0),
        )?;
        if active_account != "local" && active_account != target_account_id {
            return Err(ScanError::InvalidShopState);
        }
        let source_relation =
            classify_source_relation(&transaction, &request, manifest.as_deref())?;
        let correction_hold = held
            || matches!(
                source_relation,
                GuestImportSourceRelation::CapturedPrefixChanged
                    | GuestImportSourceRelation::Unverifiable
            );

        let current_settings = {
            let mut statement = transaction.prepare(
                "SELECT key,value FROM setting WHERE key GLOB 'planet_*'
                 AND key<>'planet_account_id' ORDER BY key",
            )?;
            let rows = statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<Result<BTreeMap<_, _>, _>>()?;
            rows
        };
        let current_objects = {
            let mut statement = transaction.prepare(
                "SELECT stage,ordinal,kind,x,y,seed FROM planet_object ORDER BY stage,ordinal",
            )?;
            let rows = statement
                .query_map([], |row| {
                    Ok(serde_json::json!({
                        "stage": row.get::<_, u8>(0)?,
                        "ordinal": row.get::<_, u32>(1)?,
                        "kind": row.get::<_, String>(2)?,
                        "x": row.get::<_, u8>(3)?,
                        "y": row.get::<_, u8>(4)?,
                        "seed": row.get::<_, String>(5)?.parse::<u64>().map_err(|_| rusqlite::Error::InvalidQuery)?,
                    }))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        let current_wallet_credits = {
            let mut statement = transaction.prepare(
                "SELECT previous_cycle_id,amount,created_at_utc FROM planet_wallet_credit
                 ORDER BY previous_cycle_id",
            )?;
            let rows = statement
                .query_map([], |row| {
                    Ok(serde_json::json!({
                        "previous_cycle_id": row.get::<_, String>(0)?,
                        "amount": u64::try_from(row.get::<_, i64>(1)?).map_err(|_| rusqlite::Error::InvalidQuery)?,
                        "created_at_utc": row.get::<_, String>(2)?,
                    }))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        let local_planet_json = serde_json::to_string(&serde_json::json!({
            "settings": current_settings,
            "objects": current_objects,
            "wallet_credits": current_wallet_credits,
        }))
        .map_err(|_| ScanError::Database)?;
        transaction.execute(
            "INSERT INTO planet_account_state(account_id,state_json) VALUES ('local',?1)
             ON CONFLICT(account_id) DO UPDATE SET state_json=excluded.state_json",
            [&local_planet_json],
        )?;

        let profile = planet_state
            .profile
            .as_ref()
            .ok_or(ScanError::InvalidProfile)?;
        let avatar = match profile.avatar {
            crate::domain::planet::PlanetAvatar::Masculine => "masculine",
            crate::domain::planet::PlanetAvatar::Feminine => "feminine",
        };
        let mut target_settings = current_settings;
        for (key, value) in [
            ("planet_nickname", profile.nickname.clone()),
            ("planet_avatar", avatar.to_owned()),
            ("planet_timezone", planet_state.timezone.clone()),
            (
                "planet_current_cycle_id",
                planet_state.current_cycle_id.clone(),
            ),
            (
                "planet_cycle_started_at_utc",
                planet_state.cycle_started_at_utc.clone(),
            ),
            (
                "planet_remote_cycle_id",
                planet_state.current_cycle_id.clone(),
            ),
            (
                "planet_remote_current_tokens",
                planet_state.current_planet_tokens.to_string(),
            ),
            (
                "planet_remote_lifetime_tokens",
                planet_state.lifetime_tokens.to_string(),
            ),
            (
                "planet_remote_growth_credit",
                planet_state.growth_credit.to_string(),
            ),
            (
                "planet_remote_incomplete",
                planet_state.incomplete.to_string(),
            ),
        ] {
            target_settings.insert(key.to_owned(), value);
        }
        if let Some(last_reset) = &planet_state.last_reset_at_utc {
            target_settings.insert("planet_last_reset_at_utc".into(), last_reset.clone());
        }
        if let Some(reset_available) = &planet_state.reset_available_at_utc {
            target_settings.insert(
                "planet_reset_available_at_utc".into(),
                reset_available.clone(),
            );
        }
        let target_planet_json = serde_json::to_string(&serde_json::json!({
            "settings": target_settings,
            "objects": planet_state.objects,
            "wallet_credits": planet_state.wallet_credits,
        }))
        .map_err(|_| ScanError::Database)?;
        transaction.execute(
            "INSERT INTO planet_account_state(account_id,state_json) VALUES (?1,?2)
             ON CONFLICT(account_id) DO UPDATE SET state_json=excluded.state_json",
            params![target_account_id, target_planet_json],
        )?;
        transaction.execute(
            "DELETE FROM setting WHERE key GLOB 'planet_*' AND key<>'planet_account_id'",
            [],
        )?;
        for (key, value) in &target_settings {
            transaction.execute(
                "INSERT INTO setting(key,value) VALUES (?1,?2)",
                params![key, value],
            )?;
        }
        transaction.execute("DELETE FROM planet_object", [])?;
        for object in &planet_state.objects {
            transaction.execute(
                "INSERT INTO planet_object(cycle_id,stage,ordinal,kind,x,y,seed)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![
                    planet_state.current_cycle_id,
                    object.stage,
                    object.ordinal,
                    object.kind,
                    object.x,
                    object.y,
                    object.seed.to_string(),
                ],
            )?;
        }
        super::ledger::record_ordinal_history_in(&transaction, target_account_id, planet_state)?;
        transaction.execute("DELETE FROM planet_wallet_credit", [])?;
        for credit in &planet_state.wallet_credits {
            transaction.execute(
                "INSERT INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
                 VALUES (?1,?2,?3)",
                params![
                    credit.previous_cycle_id,
                    i64::try_from(credit.amount).map_err(|_| ScanError::InvalidCount)?,
                    credit.created_at_utc,
                ],
            )?;
        }
        transaction.execute(
            "UPDATE planet_usage_owner SET account_id=?1 WHERE account_id='local'",
            [target_account_id],
        )?;
        transaction.execute(
            "UPDATE setting SET value=?1 WHERE key='planet_account_id'",
            [target_account_id],
        )?;

        transaction.execute(
            "INSERT INTO growth_journal_state(account_id,generation,deleted_at_utc)
             VALUES (?1,?2,?3) ON CONFLICT(account_id) DO UPDATE SET
             generation=excluded.generation,deleted_at_utc=excluded.deleted_at_utc",
            params![
                target_account_id,
                i64::try_from(journal.generation).map_err(|_| ScanError::InvalidCount)?,
                journal.deleted_at_utc,
            ],
        )?;
        for cycle in &journal.cycles {
            transaction.execute(
                "INSERT INTO growth_journal_cycle(account_id,cycle_id,started_at_utc,ended_at_utc,
                 wallet_credit,wallet_credit_at_utc) VALUES (?1,?2,?3,?4,?5,?6)
                 ON CONFLICT(account_id,cycle_id) DO UPDATE SET
                 started_at_utc=excluded.started_at_utc,ended_at_utc=excluded.ended_at_utc,
                 wallet_credit=excluded.wallet_credit,wallet_credit_at_utc=excluded.wallet_credit_at_utc",
                params![
                    target_account_id,
                    cycle.cycle_id,
                    cycle.started_at_utc,
                    cycle.ended_at_utc,
                    cycle.wallet_credit.map(i64::try_from).transpose().map_err(|_| ScanError::InvalidCount)?,
                    cycle.wallet_credit_at_utc,
                ],
            )?;
        }
        for entry in &journal.entries {
            if entry.payload_hash != entry.compute_hash() {
                return Err(ScanError::InvalidShopState);
            }
            transaction.execute(
                "INSERT INTO growth_journal_entry(account_id,device_id,cycle_id,bucket_date,agent,
                 revision,acknowledged_revision,generation,present,confirmed_tokens,coverage,payload_hash)
                 VALUES (?1,?2,?3,?4,?5,?6,0,?7,?8,?9,?10,?11)
                 ON CONFLICT(account_id,device_id,cycle_id,bucket_date,agent) DO UPDATE SET
                 revision=excluded.revision,acknowledged_revision=0,generation=excluded.generation,
                 present=excluded.present,confirmed_tokens=excluded.confirmed_tokens,
                 coverage=excluded.coverage,payload_hash=excluded.payload_hash
                 WHERE excluded.revision>=growth_journal_entry.revision",
                params![
                    target_account_id,
                    entry.device_id,
                    entry.cycle_id,
                    entry.bucket_date,
                    crate::storage::ledger::agent_name(entry.agent),
                    i64::try_from(entry.revision).map_err(|_| ScanError::InvalidCount)?,
                    i64::try_from(entry.generation).map_err(|_| ScanError::InvalidCount)?,
                    entry.present,
                    entry.confirmed_tokens.map(i64::try_from).transpose().map_err(|_| ScanError::InvalidCount)?,
                    match entry.coverage {
                        crate::domain::usage::UsageCoverage::Complete => "complete",
                        crate::domain::usage::UsageCoverage::Partial => "partial",
                        crate::domain::usage::UsageCoverage::Unavailable => "unavailable",
                        crate::domain::usage::UsageCoverage::Unsupported => "unsupported",
                        crate::domain::usage::UsageCoverage::UserDisabled => "user_disabled",
                    },
                    entry.payload_hash,
                ],
            )?;
        }
        for ack_entry in &ack.journal_entries {
            transaction.execute(
                "UPDATE growth_journal_entry SET acknowledged_revision=max(acknowledged_revision,?8)
                 WHERE account_id=?1 AND device_id=?2 AND cycle_id=?3 AND bucket_date=?4 AND agent=?5
                   AND generation=?6 AND revision=?8 AND payload_hash=?7",
                params![
                    target_account_id,
                    ack_entry.logical_key.device_id,
                    ack_entry.logical_key.cycle_id,
                    ack_entry.logical_key.bucket_date,
                    crate::storage::ledger::agent_name(ack_entry.logical_key.agent),
                    i64::try_from(ack_entry.logical_key.generation).map_err(|_| ScanError::InvalidCount)?,
                    ack_entry.payload_hash,
                    i64::try_from(ack_entry.revision).map_err(|_| ScanError::InvalidCount)?,
                ],
            )?;
        }
        let source_entries = {
            let mut statement = transaction.prepare(
                "SELECT device_id,cycle_id,bucket_date,agent,revision,generation,present,
                        confirmed_tokens,coverage,payload_hash
                 FROM growth_journal_entry WHERE account_id='local'",
            )?;
            let rows = statement
                .query_map([], |row| {
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
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        for (
            device_id,
            cycle_id,
            date,
            agent,
            revision,
            generation,
            present,
            tokens,
            coverage,
            hash,
        ) in source_entries
        {
            transaction.execute(
                "INSERT INTO growth_journal_entry(account_id,device_id,cycle_id,bucket_date,agent,
                 revision,acknowledged_revision,generation,present,confirmed_tokens,coverage,payload_hash)
                 VALUES (?1,?2,?3,?4,?5,?6,0,?7,?8,?9,?10,?11)
                 ON CONFLICT(account_id,device_id,cycle_id,bucket_date,agent) DO UPDATE SET
                 revision=excluded.revision,
                 acknowledged_revision=growth_journal_entry.acknowledged_revision,
                 generation=excluded.generation,
                 present=excluded.present,confirmed_tokens=excluded.confirmed_tokens,
                 coverage=excluded.coverage,payload_hash=excluded.payload_hash
                 WHERE excluded.revision>growth_journal_entry.revision",
                params![target_account_id,device_id,cycle_id,date,agent,revision,generation,
                    present,tokens,coverage,hash],
            )?;
        }

        super::cosmetic_shop::store_confirmed_shop_state_in_transaction(&transaction, shop_state)?;
        super::shop_effects::seed_guest_import_contribution_baseline_in_transaction(
            &transaction,
            target_account_id,
            request.snapshot.canonical_payload.canonical_version,
            &request.snapshot.data.effect_contributions,
            &request.snapshot.data.activity_days,
        )?;
        super::shop_effects::apply_confirmed_shop_effect_timeline_in_transaction(
            &transaction,
            timeline,
            target_account_id,
            &planet_state.current_cycle_id,
        )?;
        let raw = Ledger::planet_device_contribution_from_connection(&transaction, false)?;
        let persisted = super::shop_effects::capture_shop_device_contribution_from_connection(
            &transaction,
            raw,
        )?;
        if source_relation == GuestImportSourceRelation::Exact
            && canonical_json_bytes(&persisted).map_err(|_| ScanError::InvalidShopState)?
                != canonical_json_bytes(canonical).map_err(|_| ScanError::InvalidShopState)?
        {
            return Err(ScanError::InvalidShopState);
        }

        transaction.execute(
            "UPDATE guest_shop_import_v2_capture
             SET phase='imported',correction_hold=?3,result_json=?4
             WHERE target_account_id=?1 AND import_id=?2 AND phase='attempt_started'",
            params![
                target_account_id,
                import_id,
                if correction_hold { 1_i64 } else { 0_i64 },
                result_json,
            ],
        )?;
        transaction.commit()?;
        Ok(if correction_hold {
            GuestImportCompletion::ImportedWithCorrectionHold
        } else {
            GuestImportCompletion::Imported
        })
    }
}

fn capture_table_exists(connection: &Connection) -> Result<bool, ScanError> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master
             WHERE type='table' AND name='guest_shop_import_v2_capture')",
            [],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

pub(super) fn guest_import_game_mutations_allowed(ledger: &Ledger) -> Result<bool, ScanError> {
    let transaction = ledger.connection.unchecked_transaction()?;
    if !capture_table_exists(&transaction)? {
        transaction.commit()?;
        return Ok(true);
    }
    ensure_source_manifest_column(&transaction)?;
    let stored: Vec<(String, String, String, String, String, i64, Option<String>)> = {
        let mut statement = transaction.prepare(
            "SELECT target_account_id,import_id,source_fingerprint,request_json,phase,correction_hold,
                    source_manifest_json
             FROM guest_shop_import_v2_capture ORDER BY target_account_id",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    if stored.is_empty() {
        transaction.commit()?;
        return Ok(true);
    }
    if stored.len() != 1 {
        transaction.commit()?;
        return Ok(false);
    }

    let (target_account_id, import_id, fingerprint, request_json, phase, correction_hold, manifest) =
        stored
            .into_iter()
            .next()
            .ok_or(ScanError::InvalidShopState)?;
    let request: GuestShopImportV2Request = match serde_json::from_str(&request_json) {
        Ok(request) => request,
        Err(_) => {
            persist_capture_hold(&transaction, &import_id)?;
            transaction.commit()?;
            return Ok(false);
        }
    };
    if validate_stored_request(
        &request,
        &import_id,
        &target_account_id,
        &fingerprint,
        &request_json,
    )
    .is_err()
    {
        persist_capture_hold(&transaction, &import_id)?;
        transaction.commit()?;
        return Ok(false);
    }
    let mut phase = match parse_capture_phase(&phase) {
        Ok(phase) => phase,
        Err(_) => {
            persist_capture_hold(&transaction, &import_id)?;
            transaction.commit()?;
            return Ok(false);
        }
    };
    let already_held = match correction_hold {
        0 => false,
        1 => true,
        _ => {
            persist_capture_hold(&transaction, &import_id)?;
            transaction.commit()?;
            return Ok(false);
        }
    };
    let relation = classify_source_relation(&transaction, &request, manifest.as_deref())?;
    let correction_hold =
        persist_correction_hold(&transaction, &import_id, relation, &mut phase, already_held)?;
    transaction.commit()?;
    Ok(phase == GuestImportPhase::Imported && !correction_hold)
}

pub(super) fn has_guest_shop_import_v2_candidate(ledger: &Ledger) -> Result<bool, ScanError> {
    let account_id: String = ledger.connection.query_row(
        "SELECT value FROM setting WHERE key='planet_account_id'",
        [],
        |row| row.get(0),
    )?;
    if account_id != "local" {
        return Ok(false);
    }
    Ok(collect_v2_domain_source(&ledger.connection)?.is_some())
}

pub(super) fn guest_shop_import_v2_target(ledger: &Ledger) -> Result<Option<String>, ScanError> {
    if !capture_table_exists(&ledger.connection)? {
        return Ok(None);
    }
    ledger
        .connection
        .query_row(
            "SELECT target_account_id FROM guest_shop_import_v2_capture
             ORDER BY target_account_id LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(Into::into)
}

fn validate_stored_request(
    request: &GuestShopImportV2Request,
    import_id: &str,
    target_account_id: &str,
    stored_fingerprint: &str,
    request_json: &str,
) -> Result<(), ScanError> {
    let fingerprint = source_fingerprint(&request.snapshot).map_err(|_| ScanError::Database)?;
    let canonical_json = canonical_json_bytes(request).map_err(|_| ScanError::Database)?;
    if request.schema_version != 2
        || request.snapshot.import_id != import_id
        || request.snapshot.target_account_id != target_account_id
        || request.snapshot.source_account_id != "local"
        || request.snapshot.source_fingerprint != stored_fingerprint
        || fingerprint != stored_fingerprint
        || canonical_json.as_slice() != request_json.as_bytes()
    {
        return Err(ScanError::InvalidShopState);
    }
    Ok(())
}

fn parse_capture_phase(value: &str) -> Result<GuestImportPhase, ScanError> {
    match value {
        "captured" => Ok(GuestImportPhase::Captured),
        "attempt_started" => Ok(GuestImportPhase::AttemptStarted),
        "held" => Ok(GuestImportPhase::Held),
        "imported" => Ok(GuestImportPhase::Imported),
        _ => Err(ScanError::InvalidShopState),
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredSourceManifest {
    lineage_id: String,
    device_id: String,
    activated_at_utc: String,
    clock_policy: String,
    eligible: i64,
    ingest_watermark: i64,
    occurrence_count: i64,
    last_mutation_seq: i64,
    entries: Vec<StoredSourceOccurrence>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredSourceOccurrence {
    event_key: String,
    occurrence_id: String,
    ingest_seq: i64,
    current_record_version: i64,
    content_fingerprint: String,
    effective: i64,
    version: Option<StoredSourceVersion>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredSourceVersion {
    lineage_id: String,
    device_id: String,
    agent: String,
    occurred_at_utc: String,
    ingested_at_utc: String,
    cycle_id: String,
    total_tokens: Option<i64>,
    coverage: String,
}

#[derive(Clone, Debug)]
struct StoredMutation {
    mutation_seq: i64,
    event_key: String,
    mutation_kind: String,
}

fn ensure_source_manifest_column(connection: &Connection) -> Result<(), ScanError> {
    let mut statement = connection.prepare("PRAGMA table_info(guest_shop_import_v2_capture)")?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    if !columns
        .iter()
        .any(|column| column == "source_manifest_json")
    {
        connection.execute(
            "ALTER TABLE guest_shop_import_v2_capture ADD COLUMN source_manifest_json TEXT",
            [],
        )?;
    }
    Ok(())
}

fn read_current_source_manifest(
    connection: &Connection,
) -> Result<Option<StoredSourceManifest>, ScanError> {
    let lineage: Option<(String, String, String, String, i64, i64, i64, i64)> = connection
        .query_row(
            "SELECT lineage_id,device_id,activated_at_utc,clock_policy,eligible,
                    ingest_watermark,occurrence_count,last_mutation_seq
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
                    row.get(7)?,
                ))
            },
        )
        .optional()?;
    let Some((
        lineage_id,
        device_id,
        activated_at_utc,
        clock_policy,
        eligible,
        ingest_watermark,
        occurrence_count,
        last_mutation_seq,
    )) = lineage
    else {
        return Ok(None);
    };

    let mut statement = connection.prepare(
        "SELECT k.event_key,k.occurrence_id,k.ingest_seq,k.current_record_version,
                k.content_fingerprint,k.effective,v.lineage_id,v.device_id,v.agent,
                v.occurred_at_utc,v.ingested_at_utc,v.cycle_id,v.total_tokens,v.coverage
         FROM guest_provenance_occurrence_key k
         JOIN guest_provenance_occurrence_version v
           ON v.occurrence_id=k.occurrence_id
          AND v.record_version=k.current_record_version
         ORDER BY k.ingest_seq",
    )?;
    let entries = statement
        .query_map([], |row| {
            Ok(StoredSourceOccurrence {
                event_key: row.get(0)?,
                occurrence_id: row.get(1)?,
                ingest_seq: row.get(2)?,
                current_record_version: row.get(3)?,
                content_fingerprint: row.get(4)?,
                effective: row.get(5)?,
                version: Some(StoredSourceVersion {
                    lineage_id: row.get(6)?,
                    device_id: row.get(7)?,
                    agent: row.get(8)?,
                    occurred_at_utc: row.get(9)?,
                    ingested_at_utc: row.get(10)?,
                    cycle_id: row.get(11)?,
                    total_tokens: row.get(12)?,
                    coverage: row.get(13)?,
                }),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Some(StoredSourceManifest {
        lineage_id,
        device_id,
        activated_at_utc,
        clock_policy,
        eligible,
        ingest_watermark,
        occurrence_count,
        last_mutation_seq,
        entries,
    }))
}

fn source_manifest_matches_request(
    manifest: &StoredSourceManifest,
    request: &GuestShopImportV2Request,
) -> bool {
    let provenance = &request.snapshot.provenance;
    let Ok(watermark) = i64::try_from(provenance.ingest_watermark) else {
        return false;
    };
    let Ok(count) = i64::try_from(provenance.occurrence_count) else {
        return false;
    };
    if manifest.lineage_id != provenance.lineage_id
        || manifest.device_id != provenance.device_id
        || manifest.activated_at_utc != provenance.activated_at_utc
        || manifest.clock_policy != "guest_utc_monotonic_v1"
        || manifest.eligible != 1
        || manifest.ingest_watermark != watermark
        || manifest.occurrence_count != count
        || manifest.last_mutation_seq != watermark
        || manifest.entries.len() != provenance.occurrences.len()
    {
        return false;
    }
    provenance
        .occurrences
        .iter()
        .zip(&manifest.entries)
        .enumerate()
        .all(|(index, (occurrence, entry))| {
            let Ok(expected_seq) = i64::try_from(index + 1) else {
                return false;
            };
            let Ok(record_version) = i64::try_from(occurrence.record_version) else {
                return false;
            };
            let Ok(ingest_seq) = i64::try_from(occurrence.ingest_seq) else {
                return false;
            };
            let Ok(total_tokens) = i64::try_from(occurrence.total_tokens) else {
                return false;
            };
            let Ok(agent) = serde_json::to_value(&occurrence.agent) else {
                return false;
            };
            let Some(agent) = agent.as_str() else {
                return false;
            };
            entry.ingest_seq == expected_seq
                && entry.ingest_seq == ingest_seq
                && entry.current_record_version == record_version
                && entry.occurrence_id == occurrence.occurrence_id
                && !entry.event_key.is_empty()
                && !entry.content_fingerprint.is_empty()
                && entry.effective == 1
                && entry.version.as_ref().is_some_and(|version| {
                    version.lineage_id == provenance.lineage_id
                        && version.device_id == occurrence.device_id
                        && version.agent == agent
                        && version.occurred_at_utc == occurrence.occurred_at_utc
                        && version.ingested_at_utc == occurrence.ingested_at_utc
                        && version.cycle_id == occurrence.cycle_id
                        && version.total_tokens == Some(total_tokens)
                        && version.coverage == "complete"
                })
        })
}

fn classify_source_relation(
    connection: &Connection,
    request: &GuestShopImportV2Request,
    stored_manifest_json: Option<&str>,
) -> Result<GuestImportSourceRelation, ScanError> {
    let Some(stored_manifest_json) = stored_manifest_json else {
        return Ok(GuestImportSourceRelation::Unverifiable);
    };
    let Ok(captured) = serde_json::from_str::<StoredSourceManifest>(stored_manifest_json) else {
        return Ok(GuestImportSourceRelation::Unverifiable);
    };
    if !source_manifest_matches_request(&captured, request) {
        return Ok(GuestImportSourceRelation::Unverifiable);
    }
    let current = match read_current_source_manifest(connection) {
        Ok(Some(current)) => current,
        Ok(None) | Err(_) => return Ok(GuestImportSourceRelation::Unverifiable),
    };
    if current.lineage_id != captured.lineage_id
        || current.device_id != captured.device_id
        || current.activated_at_utc != captured.activated_at_utc
        || current.clock_policy != captured.clock_policy
    {
        return Ok(GuestImportSourceRelation::CapturedPrefixChanged);
    }

    let mut captured_prefix_changed = current.ingest_watermark < captured.ingest_watermark
        || current.occurrence_count < captured.occurrence_count;
    for captured_entry in &captured.entries {
        match current
            .entries
            .iter()
            .find(|entry| entry.ingest_seq == captured_entry.ingest_seq)
        {
            Some(current_entry) if current_entry == captured_entry => {}
            _ => captured_prefix_changed = true,
        }
    }

    let captured_keys: BTreeSet<&str> = captured
        .entries
        .iter()
        .map(|entry| entry.event_key.as_str())
        .collect();
    let mutations = match read_source_mutations(connection, &captured.lineage_id) {
        Ok(mutations) => mutations,
        Err(_) => return Ok(GuestImportSourceRelation::Unverifiable),
    };

    let mut mutation_log_valid = true;
    for (index, mutation) in mutations.iter().enumerate() {
        let Ok(expected_seq) = i64::try_from(index + 1) else {
            mutation_log_valid = false;
            break;
        };
        if mutation.mutation_seq != expected_seq {
            mutation_log_valid = false;
            break;
        }
        if mutation.mutation_kind != "occurrence_inserted"
            && captured_keys.contains(mutation.event_key.as_str())
        {
            captured_prefix_changed = true;
        }
    }
    if captured_prefix_changed {
        return Ok(GuestImportSourceRelation::CapturedPrefixChanged);
    }
    if !mutation_log_valid
        || current.eligible != 1
        || current.ingest_watermark < 0
        || current.occurrence_count < 0
        || current.last_mutation_seq < 0
        || current.ingest_watermark != current.occurrence_count
        || usize::try_from(current.ingest_watermark).ok() != Some(current.entries.len())
        || usize::try_from(current.last_mutation_seq).ok() != Some(mutations.len())
        || captured.last_mutation_seq != captured.ingest_watermark
    {
        return Ok(GuestImportSourceRelation::Unverifiable);
    }

    for (index, mutation) in mutations.iter().enumerate() {
        let expected_seq = i64::try_from(index + 1).map_err(|_| ScanError::InvalidCount)?;
        let Some(source_entry) = current.entries.get(index) else {
            return Ok(GuestImportSourceRelation::Unverifiable);
        };
        if mutation.mutation_seq != expected_seq
            || mutation.event_key != source_entry.event_key
            || mutation.mutation_kind != "occurrence_inserted"
            || source_entry.ingest_seq != expected_seq
            || source_entry.effective != 1
            || source_entry.current_record_version != 1
            || source_entry.version.is_none()
        {
            if captured_keys.contains(mutation.event_key.as_str()) {
                return Ok(GuestImportSourceRelation::CapturedPrefixChanged);
            }
            return Ok(GuestImportSourceRelation::Unverifiable);
        }
    }
    if current.ingest_watermark == captured.ingest_watermark {
        Ok(GuestImportSourceRelation::Exact)
    } else {
        Ok(GuestImportSourceRelation::AppendOnly)
    }
}

fn read_source_mutations(
    connection: &Connection,
    lineage_id: &str,
) -> Result<Vec<StoredMutation>, ScanError> {
    let mut statement = connection.prepare(
        "SELECT mutation_seq,event_key,mutation_kind
         FROM guest_provenance_mutation WHERE lineage_id=?1 ORDER BY mutation_seq",
    )?;
    let mutations = statement
        .query_map([lineage_id], |row| {
            Ok(StoredMutation {
                mutation_seq: row.get(0)?,
                event_key: row.get(1)?,
                mutation_kind: row.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(mutations)
}

fn persist_correction_hold(
    connection: &Connection,
    import_id: &str,
    relation: GuestImportSourceRelation,
    phase: &mut GuestImportPhase,
    already_held: bool,
) -> Result<bool, ScanError> {
    if already_held
        || !matches!(
            relation,
            GuestImportSourceRelation::CapturedPrefixChanged
                | GuestImportSourceRelation::Unverifiable
        )
    {
        return Ok(already_held);
    }
    connection.execute(
        "UPDATE guest_shop_import_v2_capture
         SET phase=CASE WHEN phase='captured' THEN 'held' ELSE phase END,
             correction_hold=1
         WHERE import_id=?1",
        [import_id],
    )?;
    if *phase == GuestImportPhase::Captured {
        *phase = GuestImportPhase::Held;
    }
    Ok(true)
}

fn persist_capture_hold(connection: &Connection, import_id: &str) -> Result<(), ScanError> {
    connection.execute(
        "UPDATE guest_shop_import_v2_capture
         SET phase=CASE WHEN phase='captured' THEN 'held' ELSE phase END,
             correction_hold=1
         WHERE import_id=?1",
        [import_id],
    )?;
    Ok(())
}

pub(super) fn pending_guest_shop_import_request(
    ledger: &Ledger,
    target_account_id: &str,
) -> Result<Option<GuestShopImportV2Status>, ScanError> {
    shop_import::validate_target_account(target_account_id)?;
    let transaction = ledger.connection.unchecked_transaction()?;
    if !capture_table_exists(&transaction)? {
        transaction.commit()?;
        return Ok(None);
    }
    ensure_source_manifest_column(&transaction)?;
    let stored: Option<(String, String, String, String, i64, Option<String>)> = transaction
        .query_row(
            "SELECT import_id,source_fingerprint,request_json,phase,correction_hold,
                    source_manifest_json
             FROM guest_shop_import_v2_capture WHERE target_account_id=?1",
            [target_account_id],
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
    let Some((import_id, fingerprint, request_json, phase, correction_hold, source_manifest)) =
        stored
    else {
        transaction.commit()?;
        return Ok(None);
    };
    let request: GuestShopImportV2Request = match serde_json::from_str(&request_json) {
        Ok(request) => request,
        Err(_) => {
            persist_capture_hold(&transaction, &import_id)?;
            transaction.commit()?;
            return Err(ScanError::InvalidShopState);
        }
    };
    if validate_stored_request(
        &request,
        &import_id,
        target_account_id,
        &fingerprint,
        &request_json,
    )
    .is_err()
    {
        persist_capture_hold(&transaction, &import_id)?;
        transaction.commit()?;
        return Err(ScanError::InvalidShopState);
    }
    let mut phase = parse_capture_phase(&phase)?;
    let correction_hold = match correction_hold {
        0 => false,
        1 => true,
        _ => return Err(ScanError::InvalidShopState),
    };
    let relation = classify_source_relation(&transaction, &request, source_manifest.as_deref())?;
    let correction_hold = persist_correction_hold(
        &transaction,
        &import_id,
        relation,
        &mut phase,
        correction_hold,
    )?;
    transaction.commit()?;
    Ok(Some(GuestShopImportV2Status {
        request,
        source_relation: relation,
        phase,
        correction_hold,
    }))
}

pub(super) fn guest_import_source_relation(
    ledger: &Ledger,
    import_id: uuid::Uuid,
) -> Result<GuestImportSourceRelation, ScanError> {
    let transaction = ledger.connection.unchecked_transaction()?;
    if !capture_table_exists(&transaction)? {
        return Err(ScanError::InvalidShopState);
    }
    ensure_source_manifest_column(&transaction)?;
    let import_id = import_id.to_string();
    let stored: Option<(String, String, String, String, i64, Option<String>)> = transaction
        .query_row(
            "SELECT target_account_id,source_fingerprint,request_json,phase,correction_hold,
                    source_manifest_json
             FROM guest_shop_import_v2_capture WHERE import_id=?1",
            [&import_id],
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
    let Some((target_account_id, fingerprint, request_json, phase, correction_hold, manifest)) =
        stored
    else {
        return Err(ScanError::InvalidShopState);
    };
    let request: GuestShopImportV2Request = match serde_json::from_str(&request_json) {
        Ok(request) => request,
        Err(_) => {
            persist_capture_hold(&transaction, &import_id)?;
            transaction.commit()?;
            return Ok(GuestImportSourceRelation::Unverifiable);
        }
    };
    if validate_stored_request(
        &request,
        &import_id,
        &target_account_id,
        &fingerprint,
        &request_json,
    )
    .is_err()
    {
        persist_capture_hold(&transaction, &import_id)?;
        transaction.commit()?;
        return Ok(GuestImportSourceRelation::Unverifiable);
    }
    let mut phase = parse_capture_phase(&phase)?;
    let correction_hold = match correction_hold {
        0 => false,
        1 => true,
        _ => return Err(ScanError::InvalidShopState),
    };
    let relation = classify_source_relation(&transaction, &request, manifest.as_deref())?;
    persist_correction_hold(
        &transaction,
        &import_id,
        relation,
        &mut phase,
        correction_hold,
    )?;
    transaction.commit()?;
    Ok(relation)
}

fn collect_v2_domain_source(
    connection: &Connection,
) -> Result<Option<(GuestShopImportData, GuestProvenanceV1)>, ScanError> {
    let captured_at_utc = Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true);
    collect_v2_domain_source_at(connection, &captured_at_utc)
}

fn collect_v2_domain_source_at(
    connection: &Connection,
    captured_at_utc: &str,
) -> Result<Option<(GuestShopImportData, GuestProvenanceV1)>, ScanError> {
    let Some(provenance) = read_guest_provenance(connection, captured_at_utc)? else {
        return Ok(None);
    };
    let mut data = shop_import::capture_data(connection)?;
    data.effect_cycle_bounds = provenance
        .cycle_bounds
        .iter()
        .map(|bound| ShopCycleBound {
            cycle_id: bound.cycle_id.clone(),
            started_at_utc: bound.started_at_utc.clone(),
            ended_at_utc: bound.ended_at_utc.clone(),
        })
        .collect();
    data.effect_cycle_bounds_authoritative = false;
    if let Some((usage_aggregates, current_cycle_usage_tokens)) =
        provenance_grouped_usage_aggregates(&data, &provenance)
    {
        data.usage_aggregates = usage_aggregates;
        data.current_cycle_usage_tokens = Some(current_cycle_usage_tokens);
    }

    let receipt = &provenance.reset_receipt;
    let result = &receipt.result;
    let old_bound = provenance
        .cycle_bounds
        .iter()
        .find(|bound| bound.cycle_id == result.previous_cycle_id);
    let new_bound = provenance
        .cycle_bounds
        .iter()
        .find(|bound| bound.cycle_id == result.new_cycle_id);
    let settlement = data
        .cycle_settlements
        .iter()
        .find(|settlement| settlement.cycle_id == result.previous_cycle_id);
    let matching_claims = data
        .unverified_planet_wallet_claims
        .iter()
        .filter(|claim| claim.previous_cycle_id == result.previous_cycle_id)
        .collect::<Vec<_>>();
    let receipt_claim = matching_claims.first().copied().filter(|claim| {
        matching_claims.len() == 1
            && claim.claimed_amount == result.raw_tokens
            && claim.created_at_utc == result.reset_at_utc
    });
    let proof_index = data.reset_settlement_proofs.iter().position(|proof| {
        proof.previous_cycle_id == result.previous_cycle_id
            && proof.request_id == receipt.request.request_id
            && proof.new_cycle_id == result.new_cycle_id
            && proof.reset_at_utc == result.reset_at_utc
    });
    let local_receipt_matches = data.reset_settlement_proofs.len() == 1
        && proof_index.is_some()
        && settlement.is_some_and(|settlement| settlement.amount == result.bonus_tokens)
        && receipt_claim.is_some()
        && old_bound.is_some_and(|bound| {
            bound.ended_at_utc.as_deref() == Some(result.reset_at_utc.as_str())
        })
        && new_bound.is_some_and(|bound| {
            bound.started_at_utc == result.reset_at_utc && bound.ended_at_utc.is_none()
        })
        && data.current_cycle.cycle_id == result.new_cycle_id
        && data.current_cycle.started_at_utc.as_deref() == Some(result.reset_at_utc.as_str())
        && data.last_reset_at_utc.as_deref() == Some(result.reset_at_utc.as_str())
        && data.reset_available_at_utc.as_deref() == Some(result.reset_available_at_utc.as_str());
    if local_receipt_matches {
        let proof = &mut data.reset_settlement_proofs[proof_index.expect("checked above")];
        proof.raw_wallet_claim = receipt_claim.cloned();
        proof.settled_bonus_tokens = settlement.map(|settlement| settlement.amount);
        proof.final_effect_revision = Some(result.final_effect_revision);
        proof.final_effects = Some(result.final_effects.clone());
        proof.final_active_instance_ids = result.final_active_instance_ids.clone();
        proof.old_cycle_started_at_utc = old_bound.map(|bound| bound.started_at_utc.clone());
        proof.new_cycle_started_at_utc = new_bound.map(|bound| bound.started_at_utc.clone());
        proof.reset_available_at_utc = Some(result.reset_available_at_utc.clone());
        data.reset_receipts_unverifiable = false;
    }
    data.integrity_issues =
        shop_import::validate_local_integrity_for_guest_provenance(&data, &provenance);
    Ok(Some((data, provenance)))
}

fn v2_disposition(
    data: &GuestShopImportData,
    provenance: &GuestProvenanceV1,
) -> crate::domain::cosmetic_shop::GuestShopImportDisposition {
    if has_positive_new_cycle_usage(provenance)
        || provenance_grouped_usage_aggregates(data, provenance).is_none()
    {
        return crate::domain::cosmetic_shop::GuestShopImportDisposition::SourceUnverifiable;
    }
    let verified_reset_claim = verified_v2_reset_wallet_claim(data, provenance);
    shop_import::disposition_v2(data, verified_reset_claim)
}

fn provenance_grouped_usage_aggregates(
    data: &GuestShopImportData,
    provenance: &GuestProvenanceV1,
) -> Option<(Vec<crate::domain::cosmetic_shop::GuestUsageAggregate>, u64)> {
    type DayAgent = (String, String);
    type CycleDayAgent = (String, String, String);
    type Totals = (u64, u64);

    let timezone = data.world_timezone.parse::<chrono_tz::Tz>().ok()?;
    let mut proven_by_cycle = BTreeMap::<CycleDayAgent, Totals>::new();
    for occurrence in &provenance.occurrences {
        if occurrence.coverage != GuestOccurrenceCoverage::Complete {
            return None;
        }
        let occurred_at = parse_utc(&occurrence.occurred_at_utc)?;
        let bucket_date = occurred_at
            .with_timezone(&timezone)
            .format("%Y-%m-%d")
            .to_string();
        let agent = match occurrence.agent {
            Agent::Codex => "codex",
            Agent::ClaudeCode => "claude_code",
        };
        let totals = proven_by_cycle
            .entry((bucket_date, agent.to_owned(), occurrence.cycle_id.clone()))
            .or_default();
        totals.0 = totals.0.checked_add(1)?;
        totals.1 = totals.1.checked_add(occurrence.total_tokens)?;
    }

    let mut proven_by_day = BTreeMap::<DayAgent, Totals>::new();
    let mut proven_cycles = BTreeMap::<DayAgent, BTreeSet<String>>::new();
    for ((bucket_date, agent, cycle_id), totals) in &proven_by_cycle {
        let key = (bucket_date.clone(), agent.clone());
        let day_totals = proven_by_day.entry(key.clone()).or_default();
        day_totals.0 = day_totals.0.checked_add(totals.0)?;
        day_totals.1 = day_totals.1.checked_add(totals.1)?;
        proven_cycles
            .entry(key)
            .or_default()
            .insert(cycle_id.clone());
    }

    let mut raw_by_day = BTreeMap::<DayAgent, Totals>::new();
    for aggregate in &data.usage_aggregates {
        if aggregate.coverage != "complete" {
            return None;
        }
        let key = (aggregate.bucket_date.clone(), aggregate.agent.clone());
        let tokens = aggregate.total_tokens?;
        if let Some(cycle_id) = &aggregate.cycle_id {
            let cycles = proven_cycles.get(&key)?;
            if !cycles.contains(cycle_id) {
                return None;
            }
        }
        let totals = raw_by_day.entry(key).or_default();
        totals.0 = totals.0.checked_add(aggregate.event_count)?;
        totals.1 = totals.1.checked_add(tokens)?;
    }
    if raw_by_day != proven_by_day {
        return None;
    }

    let mut daily_by_day = BTreeMap::<DayAgent, u64>::new();
    for daily in &data.daily_agent_totals {
        if daily.coverage != "complete" {
            return None;
        }
        let key = (daily.bucket_date.clone(), daily.agent.clone());
        if daily_by_day.insert(key, daily.total_tokens?).is_some() {
            return None;
        }
    }
    let proven_daily = proven_by_day
        .iter()
        .map(|(key, totals)| (key.clone(), totals.1))
        .collect::<BTreeMap<_, _>>();
    if daily_by_day != proven_daily {
        return None;
    }

    let lifetime_total = proven_by_day
        .values()
        .try_fold(0_u64, |total, (_, tokens)| total.checked_add(*tokens))?;
    if data.lifetime_usage_tokens != Some(lifetime_total)
        || data.current_cycle.cycle_id != provenance.reset_receipt.result.new_cycle_id
    {
        return None;
    }
    let current_cycle_tokens = proven_by_cycle
        .iter()
        .filter(|((_, _, cycle_id), _)| cycle_id == &data.current_cycle.cycle_id)
        .try_fold(0_u64, |total, (_, (_, tokens))| total.checked_add(*tokens))?;
    if data
        .current_cycle_usage_tokens
        .is_some_and(|tokens| tokens != current_cycle_tokens)
    {
        return None;
    }

    let usage_aggregates = proven_by_cycle
        .into_iter()
        .map(
            |((bucket_date, agent, cycle_id), (event_count, total_tokens))| {
                crate::domain::cosmetic_shop::GuestUsageAggregate {
                    cycle_id: Some(cycle_id),
                    bucket_date,
                    agent,
                    event_count,
                    total_tokens: Some(total_tokens),
                    coverage: "complete".to_owned(),
                }
            },
        )
        .collect();
    Some((usage_aggregates, current_cycle_tokens))
}

fn has_positive_new_cycle_usage(provenance: &GuestProvenanceV1) -> bool {
    provenance.occurrences.iter().any(|occurrence| {
        occurrence.cycle_id == provenance.reset_receipt.result.new_cycle_id
            && occurrence.total_tokens > 0
    })
}

fn verified_v2_reset_wallet_claim(
    data: &GuestShopImportData,
    provenance: &GuestProvenanceV1,
) -> bool {
    if data.reset_receipts_unverifiable {
        return false;
    }
    let [claim] = data.unverified_planet_wallet_claims.as_slice() else {
        return false;
    };
    let [proof] = data.reset_settlement_proofs.as_slice() else {
        return false;
    };
    let receipt = &provenance.reset_receipt;
    let request = &receipt.request;
    let result = &receipt.result;
    request.kind == GuestFirstResetRequestKind::ResetPlanet
        && request.request_id == result.request_id
        && request.cycle_id == result.previous_cycle_id
        && result.status == GuestFirstResetStatus::Reset
        && result.raw_tokens > 0
        && result
            .raw_tokens
            .checked_add(result.bonus_tokens)
            .is_some_and(|credited| credited == result.credited_tokens)
        && claim.previous_cycle_id == result.previous_cycle_id
        && claim.claimed_amount == result.raw_tokens
        && claim.created_at_utc == result.reset_at_utc
        && proof.request_id == request.request_id
        && proof.previous_cycle_id == result.previous_cycle_id
        && proof.new_cycle_id == result.new_cycle_id
        && proof.reset_at_utc == result.reset_at_utc
        && proof.raw_wallet_claim.as_ref() == Some(claim)
        && proof.settled_bonus_tokens == Some(result.bonus_tokens)
        && proof.final_effect_revision == Some(result.final_effect_revision)
        && proof.final_effects.as_ref() == Some(&result.final_effects)
        && proof.final_active_instance_ids == result.final_active_instance_ids
        && proof.reset_available_at_utc.as_deref() == Some(result.reset_available_at_utc.as_str())
}

#[cfg(test)]
pub(crate) mod guest_import_v2_capture_tests {
    use super::*;
    use crate::collectors::{ParsedRecord, RecordKind};
    use crate::domain::cosmetic_shop::{
        AvatarEquipment, AvatarEquipmentItem, RewardState, ShopEffectTimeline, ShopState,
    };
    use crate::domain::growth_journal::{GrowthJournal, GrowthJournalCycle, GrowthJournalEntry};
    use crate::domain::guest_shop_import::{
        canonical_json_bytes, GuestImportAck, GuestImportAckJournalEntry, GuestJournalLogicalKey,
        GuestShopImportV2Result,
    };
    use crate::domain::planet::{PlanetAvatar, PlanetProfile, PlanetState, PlanetWalletCredit};
    use crate::domain::usage::{TokenUsage, UsageCoverage};
    use crate::storage::shop_import::PendingGuestShopImport;
    use chrono::Duration;
    use sha2::Digest;

    const TARGET_ACCOUNT: &str = "account:00000000-0000-4000-a000-000000000071";
    const OTHER_TARGET_ACCOUNT: &str = "account:00000000-0000-4000-a000-000000000072";

    fn raw_one_date_record(
        event_key: &str,
        occurred_at_utc: chrono::DateTime<Utc>,
    ) -> ParsedRecord {
        raw_one_date_record_with_tokens(event_key, occurred_at_utc, 1_000_000)
    }

    fn raw_one_date_record_with_tokens(
        event_key: &str,
        occurred_at_utc: chrono::DateTime<Utc>,
        total_tokens: u64,
    ) -> ParsedRecord {
        ParsedRecord {
            agent: Agent::Codex,
            kind: RecordKind::Response,
            event_key: event_key.to_owned(),
            occurred_at_utc,
            usage: TokenUsage {
                input_tokens: None,
                output_tokens: None,
                cache_read_tokens: None,
                cache_write_tokens: None,
                total_tokens: Some(total_tokens),
                coverage: UsageCoverage::Complete,
            },
        }
    }

    pub(crate) fn prepared_first_reset_ledger() -> Ledger {
        prepared_first_reset_ledger_at(std::path::Path::new(":memory:"))
    }

    fn prepared_first_reset_ledger_at(path: &std::path::Path) -> Ledger {
        let mut ledger = Ledger::open(path, chrono_tz::UTC).unwrap();
        let occurred_at = Utc::now();
        ledger
            .insert(&raw_one_date_record("v2-capture-raw-1m", occurred_at))
            .unwrap();
        ledger.rebuild_shop_contributions().unwrap();
        ledger
            .settle_guest_rewards(occurred_at + Duration::milliseconds(1))
            .unwrap();
        let old_cycle_id = ledger.planet_cycle_id().unwrap();
        ledger
            .settle_guest_cycle_tokens(&old_cycle_id, occurred_at + Duration::milliseconds(2))
            .unwrap();
        ledger.prepare_growth_journal().unwrap();
        let reset_at = Utc::now() + Duration::milliseconds(250);
        assert_eq!(ledger.reset_planet(reset_at).unwrap(), 1_000_000);
        ledger.prepare_growth_journal().unwrap();
        if Utc::now() < reset_at {
            std::thread::sleep((reset_at - Utc::now()).to_std().unwrap());
        }
        ledger
    }

    fn prepared_first_reset_ledger_with_multiple_ordinary_scans() -> Ledger {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let occurred_at = Utc::now();
        let latest_occurrence_at = occurred_at + Duration::milliseconds(2);
        if Utc::now() < latest_occurrence_at {
            std::thread::sleep((latest_occurrence_at - Utc::now()).to_std().unwrap());
        }
        for (event_key, total_tokens, offset_ms) in [
            ("v2-multiscan-raw-1", 400_000, 0_i64),
            ("v2-multiscan-raw-2", 300_000, 1_i64),
            ("v2-multiscan-raw-3", 300_000, 2_i64),
        ] {
            ledger
                .insert(&raw_one_date_record_with_tokens(
                    event_key,
                    occurred_at + Duration::milliseconds(offset_ms),
                    total_tokens,
                ))
                .unwrap();
            ledger.rebuild_shop_contributions().unwrap();
        }
        let ordinary_scan_version: i64 = ledger
            .connection
            .query_row(
                "SELECT canonical_version FROM shop_contribution_state WHERE account_id='local'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(ordinary_scan_version > 1);
        ledger
            .settle_guest_rewards(occurred_at + Duration::milliseconds(4))
            .unwrap();
        let old_cycle_id = ledger.planet_cycle_id().unwrap();
        ledger
            .settle_guest_cycle_tokens(&old_cycle_id, occurred_at + Duration::milliseconds(5))
            .unwrap();
        ledger.prepare_growth_journal().unwrap();
        let reset_at = Utc::now() + Duration::milliseconds(250);
        assert_eq!(ledger.reset_planet(reset_at).unwrap(), 1_000_000);
        ledger.prepare_growth_journal().unwrap();
        if Utc::now() < reset_at {
            std::thread::sleep((reset_at - Utc::now()).to_std().unwrap());
        }
        ledger
    }

    pub(crate) fn imported_result_for(
        request: &GuestShopImportV2Request,
    ) -> GuestShopImportV2Result {
        let snapshot = &request.snapshot;
        let provenance = &snapshot.provenance;
        let current_cycle = provenance
            .cycle_bounds
            .last()
            .expect("first-reset provenance has a current cycle");
        let canonical = snapshot.canonical_payload.clone();
        let canonical_fingerprint = sha2::Sha256::digest(canonical_json_bytes(&canonical).unwrap())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let new_cycle = &provenance.reset_receipt.result.new_cycle_id;
        let reset_result = &provenance.reset_receipt.result;
        let reset_credit = PlanetWalletCredit {
            previous_cycle_id: reset_result.previous_cycle_id.clone(),
            amount: reset_result.credited_tokens,
            created_at_utc: reset_result.reset_at_utc.clone(),
        };
        let account_uuid = snapshot
            .target_account_id
            .strip_prefix("account:")
            .expect("target uses account scope");

        let shop_state = ShopState {
            account_id: snapshot.target_account_id.clone(),
            current_cycle_id: new_cycle.clone(),
            catalog_revision: 1,
            state_revision: snapshot.data.shop_state_revision,
            available_balance: reset_result.credited_tokens,
            products: vec![],
            landscape_instances: vec![],
            placements: vec![],
            removed_natural_keys: vec![],
            avatar_owned_skus: vec![],
            avatar_equipment: AvatarEquipment {
                head: AvatarEquipmentItem {
                    sku: None,
                    version: 0,
                },
                outfit: AvatarEquipmentItem {
                    sku: None,
                    version: 0,
                },
                face: AvatarEquipmentItem {
                    sku: None,
                    version: 0,
                },
                back: AvatarEquipmentItem {
                    sku: None,
                    version: 0,
                },
            },
            effects: ActiveEffects::default(),
            reward_state: RewardState::default(),
            action_unavailable_reason: None,
            guest_import_pending: false,
            guest_import_error: None,
        };
        let last_reset = reset_result.reset_at_utc.clone();
        let planet_state = PlanetState {
            version: 1,
            profile: Some(PlanetProfile {
                nickname: "Imported planet".into(),
                avatar: PlanetAvatar::Masculine,
            }),
            timezone: snapshot.data.planet_timezone.clone(),
            current_cycle_id: new_cycle.clone(),
            cycle_started_at_utc: current_cycle.started_at_utc.clone(),
            last_reset_at_utc: Some(last_reset),
            wallet_balance: reset_result.credited_tokens,
            wallet_credits: vec![reset_credit],
            current_planet_tokens: canonical.raw.current_planet_tokens,
            lifetime_tokens: canonical.raw.lifetime_tokens,
            growth_credit: 0.0,
            stage: 0,
            progress_to_next: 0.0,
            incomplete: canonical.raw.incomplete,
            can_reset: false,
            reset_available_at_utc: Some(
                provenance
                    .reset_receipt
                    .result
                    .reset_available_at_utc
                    .clone(),
            ),
            objects: vec![],
            removed_natural_keys: vec![],
        };
        let effect_timeline = ShopEffectTimeline {
            account_id: account_uuid.into(),
            current_cycle_id: new_cycle.clone(),
            effect_revision: 0,
            server_time_utc: Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true),
            reward_timezone: snapshot.data.reward_timezone.clone(),
            cycle_bounds: provenance
                .cycle_bounds
                .iter()
                .map(|bound| ShopCycleBound {
                    cycle_id: bound.cycle_id.clone(),
                    started_at_utc: bound.started_at_utc.clone(),
                    ended_at_utc: bound.ended_at_utc.clone(),
                })
                .collect(),
            intervals: vec![],
        };
        let journal_state = snapshot.data.growth_journal_state.as_ref();
        let journal_cycles = snapshot
            .data
            .growth_journal_cycles
            .iter()
            .map(|cycle| GrowthJournalCycle {
                cycle_id: cycle.cycle_id.clone(),
                started_at_utc: cycle.started_at_utc.clone(),
                ended_at_utc: cycle.ended_at_utc.clone(),
                wallet_credit: cycle.wallet_credit,
                wallet_credit_at_utc: cycle.wallet_credit_at_utc.clone(),
            })
            .collect();
        let journal_entries = snapshot
            .data
            .growth_journal_entries
            .iter()
            .map(|entry| {
                let agent =
                    serde_json::from_value(serde_json::Value::String(entry.agent.clone())).unwrap();
                let coverage =
                    serde_json::from_value(serde_json::Value::String(entry.coverage.clone()))
                        .unwrap();
                GrowthJournalEntry {
                    device_id: entry.device_id.clone(),
                    cycle_id: entry.cycle_id.clone(),
                    bucket_date: entry.bucket_date.clone(),
                    agent,
                    revision: entry.revision,
                    generation: entry.generation,
                    present: entry.present,
                    confirmed_tokens: entry.confirmed_tokens,
                    coverage,
                    payload_hash: entry.payload_hash.clone(),
                }
            })
            .collect::<Vec<_>>();
        let journal = GrowthJournal {
            generation: journal_state.map_or(0, |state| state.generation),
            deleted_at_utc: journal_state.and_then(|state| state.deleted_at_utc.clone()),
            timezone: Some(snapshot.data.planet_timezone.clone()),
            cycles: journal_cycles,
            entries: journal_entries,
        };
        let ack = GuestImportAck {
            lineage_id: provenance.lineage_id.clone(),
            device_id: provenance.device_id.clone(),
            ingest_watermark: provenance.ingest_watermark,
            occurrence_count: provenance.occurrence_count,
            prefix_fingerprint: provenance.prefix_fingerprint.clone(),
            canonical_version: canonical.canonical_version,
            canonical_payload_fingerprint: canonical_fingerprint,
            journal_entries: journal
                .entries
                .iter()
                .map(|entry| GuestImportAckJournalEntry {
                    logical_key: GuestJournalLogicalKey {
                        device_id: entry.device_id.clone(),
                        cycle_id: entry.cycle_id.clone(),
                        bucket_date: entry.bucket_date.clone(),
                        agent: entry.agent,
                        generation: entry.generation,
                    },
                    revision: entry.revision,
                    payload_hash: entry.payload_hash.clone(),
                })
                .collect(),
        };

        GuestShopImportV2Result {
            schema_version: 2,
            import_id: snapshot.import_id.clone(),
            account_id: snapshot.target_account_id.clone(),
            source_fingerprint: snapshot.source_fingerprint.clone(),
            status: crate::domain::guest_shop_import::GuestImportStatus::Imported,
            shop_state: Some(shop_state),
            planet_state: Some(planet_state),
            effect_timeline: Some(effect_timeline),
            canonical_contribution: Some(canonical),
            journal_confirmation: Some(journal),
            ack: Some(ack),
        }
    }

    fn guest_import_cache_snapshot(ledger: &Ledger) -> Vec<(String, String)> {
        [
            ("setting", "SELECT * FROM setting ORDER BY key"),
            (
                "planet_account_state",
                "SELECT * FROM planet_account_state ORDER BY account_id",
            ),
            (
                "planet_usage_owner",
                "SELECT * FROM planet_usage_owner ORDER BY event_key",
            ),
            ("usage_record", "SELECT * FROM usage_record ORDER BY event_key"),
            (
                "growth_journal_state",
                "SELECT * FROM growth_journal_state ORDER BY account_id",
            ),
            (
                "growth_journal_cycle",
                "SELECT * FROM growth_journal_cycle ORDER BY account_id,cycle_id",
            ),
            (
                "growth_journal_entry",
                "SELECT * FROM growth_journal_entry
                 ORDER BY account_id,device_id,cycle_id,bucket_date,agent",
            ),
            (
                "planet_object",
                "SELECT * FROM planet_object ORDER BY cycle_id,stage,ordinal",
            ),
            (
                "planet_wallet_credit",
                "SELECT * FROM planet_wallet_credit ORDER BY previous_cycle_id",
            ),
            (
                "shop_remote_state",
                "SELECT * FROM shop_remote_state ORDER BY account_id",
            ),
            (
                "shop_account_state",
                "SELECT * FROM shop_account_state ORDER BY account_id",
            ),
            (
                "shop_effect_history",
                "SELECT * FROM shop_effect_history ORDER BY account_id,cycle_id,revision,started_at_utc",
            ),
            (
                "shop_effect_cycle_bound",
                "SELECT * FROM shop_effect_cycle_bound ORDER BY account_id,cycle_id",
            ),
            (
                "shop_effect_contribution",
                "SELECT * FROM shop_effect_contribution
                 ORDER BY account_id,device_id,cycle_id,date,effect_revision",
            ),
            (
                "shop_activity_day",
                "SELECT * FROM shop_activity_day ORDER BY account_id,reward_date",
            ),
            (
                "shop_contribution_state",
                "SELECT * FROM shop_contribution_state ORDER BY account_id",
            ),
            (
                "guest_shop_import_v2_capture_schema",
                "PRAGMA table_info(guest_shop_import_v2_capture)",
            ),
            (
                "guest_shop_import_v2_capture",
                "SELECT * FROM guest_shop_import_v2_capture ORDER BY target_account_id",
            ),
        ]
        .into_iter()
        .map(|(name, query)| {
            let mut statement = ledger.connection.prepare(query).unwrap();
            let rows = statement
                .query_map([], |row| {
                    (0..row.as_ref().column_count())
                        .map(|index| {
                            row.get::<_, rusqlite::types::Value>(index)
                                .map(|value| format!("{value:?}"))
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            (name.to_owned(), format!("{rows:?}"))
        })
        .collect()
    }

    #[test]
    fn guest_import_v2_completion_imports_valid_receipt_and_replays_idempotently() {
        let mut ledger = prepared_first_reset_ledger();
        let captured = ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap();
        let PendingGuestShopImport::V2(capture) = captured else {
            panic!("schema-2 import capture expected, got {captured:#?}");
        };
        let request = capture.request;
        ledger.set_selected_auth_account(TARGET_ACCOUNT).unwrap();
        ledger
            .mark_guest_shop_import_attempt_started(
                TARGET_ACCOUNT,
                request.snapshot.import_id.parse().unwrap(),
            )
            .unwrap();
        let result = imported_result_for(&request);

        assert!(
            crate::sync::client::validate_guest_shop_import_v2_result(&request, &result).is_ok(),
            "the server-shaped imported receipt must pass native transport validation"
        );
        assert_eq!(
            ledger
                .complete_guest_shop_import(TARGET_ACCOUNT, &result)
                .unwrap(),
            GuestImportCompletion::Imported
        );

        let (active_account, current_cycle): (String, String) = ledger
            .connection
            .query_row(
                "SELECT
                   (SELECT value FROM setting WHERE key='planet_account_id'),
                   (SELECT value FROM setting WHERE key='planet_current_cycle_id')",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(active_account, TARGET_ACCOUNT);
        assert_eq!(
            current_cycle,
            request
                .snapshot
                .provenance
                .reset_receipt
                .result
                .new_cycle_id
        );
        let (phase, correction_hold, result_json): (String, i64, String) = ledger
            .connection
            .query_row(
                "SELECT phase,correction_hold,result_json FROM guest_shop_import_v2_capture
                 WHERE target_account_id=?1",
                [TARGET_ACCOUNT],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(phase, "imported");
        assert_eq!(correction_hold, 0);
        assert_eq!(
            result_json,
            String::from_utf8(canonical_json_bytes(&result).unwrap()).unwrap()
        );

        for ack_entry in &result.ack.as_ref().unwrap().journal_entries {
            let (revision, acknowledged_revision, payload_hash): (i64, i64, String) = ledger
                .connection
                .query_row(
                    "SELECT revision,acknowledged_revision,payload_hash FROM growth_journal_entry
                     WHERE account_id=?1 AND device_id=?2 AND cycle_id=?3 AND bucket_date=?4 AND agent=?5",
                    rusqlite::params![
                        TARGET_ACCOUNT,
                        ack_entry.logical_key.device_id,
                        ack_entry.logical_key.cycle_id,
                        ack_entry.logical_key.bucket_date,
                        crate::storage::ledger::agent_name(ack_entry.logical_key.agent),
                    ],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap();
            assert_eq!(revision, ack_entry.revision as i64);
            assert_eq!(acknowledged_revision, ack_entry.revision as i64);
            assert_eq!(payload_hash, ack_entry.payload_hash);
        }

        let before_replay = guest_import_cache_snapshot(&ledger);
        assert_eq!(
            ledger
                .complete_guest_shop_import(TARGET_ACCOUNT, &result)
                .unwrap(),
            GuestImportCompletion::Imported
        );
        assert_eq!(guest_import_cache_snapshot(&ledger), before_replay);
    }

    #[test]
    fn guest_import_v2_completion_preserves_canonical_version_after_multiple_ordinary_scans() {
        let mut ledger = prepared_first_reset_ledger_with_multiple_ordinary_scans();
        let PendingGuestShopImport::V2(capture) = ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("schema-2 import capture expected");
        };
        let request = capture.request;
        let captured_canonical = request.snapshot.canonical_payload.clone();
        assert!(captured_canonical.canonical_version > 1);
        ledger.set_selected_auth_account(TARGET_ACCOUNT).unwrap();
        ledger
            .mark_guest_shop_import_attempt_started(
                TARGET_ACCOUNT,
                request.snapshot.import_id.parse().unwrap(),
            )
            .unwrap();
        let result = imported_result_for(&request);

        let completion = ledger.complete_guest_shop_import(TARGET_ACCOUNT, &result);
        assert_eq!(
            completion,
            Ok(GuestImportCompletion::Imported),
            "exact completion failed for captured canonical version {}",
            captured_canonical.canonical_version
        );
        let imported_canonical = result.canonical_contribution.as_ref().unwrap();
        let persisted_canonical =
            crate::storage::shop_effects::shop_device_contribution_from_connection(
                &ledger.connection,
                imported_canonical.raw.clone(),
            )
            .unwrap();
        assert_eq!(persisted_canonical, *imported_canonical);
    }

    #[test]
    fn guest_import_v2_completion_advances_canonical_version_for_append_after_multiple_scans() {
        let mut ledger = prepared_first_reset_ledger_with_multiple_ordinary_scans();
        let captured = ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap();
        let PendingGuestShopImport::V2(capture) = captured else {
            panic!("schema-2 import capture expected, got {captured:#?}");
        };
        let request = capture.request;
        let captured_version = request.snapshot.canonical_payload.canonical_version;
        assert!(captured_version > 1);
        let import_id = uuid::Uuid::parse_str(&request.snapshot.import_id).unwrap();
        ledger.set_selected_auth_account(TARGET_ACCOUNT).unwrap();
        ledger
            .mark_guest_shop_import_attempt_started(TARGET_ACCOUNT, import_id)
            .unwrap();

        let reset_at = parse_utc(
            &request
                .snapshot
                .provenance
                .reset_receipt
                .result
                .reset_at_utc,
        )
        .unwrap();
        let appended_event_key = "v2-multiscan-canonical-append";
        ledger
            .insert(&raw_one_date_record_with_tokens(
                appended_event_key,
                reset_at - Duration::milliseconds(1),
                1,
            ))
            .unwrap();
        let transaction = ledger.connection.transaction().unwrap();
        Ledger::prepare_growth_journal_in_transaction(&transaction).unwrap();
        transaction.commit().unwrap();
        assert_eq!(
            ledger.guest_import_source_relation(import_id).unwrap(),
            GuestImportSourceRelation::AppendOnly
        );

        let result = imported_result_for(&request);
        assert_eq!(
            ledger
                .complete_guest_shop_import(TARGET_ACCOUNT, &result)
                .unwrap(),
            GuestImportCompletion::Imported
        );
        let imported_canonical = result.canonical_contribution.as_ref().unwrap();
        let persisted_canonical =
            crate::storage::shop_effects::shop_device_contribution_from_connection(
                &ledger.connection,
                imported_canonical.raw.clone(),
            )
            .unwrap();
        assert!(
            persisted_canonical.canonical_version > captured_version,
            "append changed canonical contributions but version stayed at {} (captured {})",
            persisted_canonical.canonical_version,
            captured_version
        );
        let owner: String = ledger
            .connection
            .query_row(
                "SELECT account_id FROM planet_usage_owner WHERE event_key=?1",
                [appended_event_key],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(owner, TARGET_ACCOUNT);
    }

    #[test]
    fn cycle_ordinal_completed_import_uses_canonical_credits_only() {
        let directory = tempfile::tempdir().unwrap();
        let mut ledger = prepared_first_reset_ledger_at(&directory.path().join("ordinal.sqlite"));
        let before = ledger.planet_ordinal().unwrap();
        let PendingGuestShopImport::V2(capture) = ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("capture expected");
        };
        assert_eq!(
            ledger.planet_ordinal().unwrap(),
            before,
            "pending payload must not increase ordinal"
        );
        ledger.set_selected_auth_account(TARGET_ACCOUNT).unwrap();
        ledger
            .mark_guest_shop_import_attempt_started(
                TARGET_ACCOUNT,
                uuid::Uuid::parse_str(&capture.request.snapshot.import_id).unwrap(),
            )
            .unwrap();
        let result = imported_result_for(&capture.request);
        ledger
            .complete_guest_shop_import(TARGET_ACCOUNT, &result)
            .unwrap();
        assert_eq!(ledger.planet_ordinal().unwrap().current, Some(2));
        ledger
            .complete_guest_shop_import(TARGET_ACCOUNT, &result)
            .unwrap();
        assert_eq!(ledger.planet_ordinal().unwrap().current, Some(2));
    }

    #[test]
    fn guest_import_v2_completion_duplicate_after_reopen_is_idempotent() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("guest-import-completion.sqlite");
        let mut ledger = prepared_first_reset_ledger_at(&path);
        let PendingGuestShopImport::V2(capture) = ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("schema-2 import capture expected");
        };
        let request = capture.request;
        ledger.set_selected_auth_account(TARGET_ACCOUNT).unwrap();
        ledger
            .mark_guest_shop_import_attempt_started(
                TARGET_ACCOUNT,
                request.snapshot.import_id.parse().unwrap(),
            )
            .unwrap();
        let result = imported_result_for(&request);

        assert_eq!(
            ledger
                .complete_guest_shop_import(TARGET_ACCOUNT, &result)
                .unwrap(),
            GuestImportCompletion::Imported
        );
        let committed_snapshot = guest_import_cache_snapshot(&ledger);
        let committed_result_json: String = ledger
            .connection
            .query_row(
                "SELECT result_json FROM guest_shop_import_v2_capture WHERE target_account_id=?1",
                [TARGET_ACCOUNT],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            committed_result_json,
            String::from_utf8(canonical_json_bytes(&result).unwrap()).unwrap()
        );
        drop(ledger);

        let mut reopened = Ledger::open(&path, chrono_tz::UTC).unwrap();
        assert_eq!(
            guest_import_cache_snapshot(&reopened),
            committed_snapshot,
            "reopen must preserve the completed local cache and capture receipt"
        );
        assert_eq!(
            reopened
                .complete_guest_shop_import(TARGET_ACCOUNT, &result)
                .unwrap(),
            GuestImportCompletion::Imported
        );
        assert_eq!(
            guest_import_cache_snapshot(&reopened),
            committed_snapshot,
            "a duplicate completion after reopen must not rewrite committed state"
        );
    }

    #[test]
    fn guest_import_v2_completion_rolls_back_all_cache_and_ack_writes_if_final_marker_fails() {
        let mut ledger = prepared_first_reset_ledger();
        let PendingGuestShopImport::V2(capture) = ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("schema-2 import capture expected")
        };
        let request = capture.request;
        ledger.set_selected_auth_account(TARGET_ACCOUNT).unwrap();
        ledger
            .mark_guest_shop_import_attempt_started(
                TARGET_ACCOUNT,
                request.snapshot.import_id.parse().unwrap(),
            )
            .unwrap();
        let result = imported_result_for(&request);
        ledger
            .connection
            .execute_batch(
                "CREATE TRIGGER fail_v2_import_marker
                 BEFORE UPDATE OF phase ON guest_shop_import_v2_capture
                 WHEN NEW.phase='imported'
                 BEGIN SELECT RAISE(ABORT,'injected final import marker failure'); END;",
            )
            .unwrap();
        let before = guest_import_cache_snapshot(&ledger);

        let completion = ledger.complete_guest_shop_import(TARGET_ACCOUNT, &result);

        assert_eq!(
            completion,
            Err(crate::storage::ledger::ScanError::Database),
            "the otherwise-valid completion must fail at the injected final marker trigger"
        );
        assert_eq!(guest_import_cache_snapshot(&ledger), before);
        ledger
            .connection
            .execute_batch("DROP TRIGGER fail_v2_import_marker")
            .unwrap();
        assert_eq!(
            ledger
                .complete_guest_shop_import(TARGET_ACCOUNT, &result)
                .unwrap(),
            GuestImportCompletion::Imported,
            "retrying the same ID and result after removing only the injected trigger must commit"
        );
    }

    #[test]
    fn guest_import_v2_completion_rejects_altered_journal_generation_without_writes() {
        let mut ledger = prepared_first_reset_ledger();
        let PendingGuestShopImport::V2(capture) = ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("schema-2 import capture expected")
        };
        let request = capture.request;
        ledger.set_selected_auth_account(TARGET_ACCOUNT).unwrap();
        ledger
            .mark_guest_shop_import_attempt_started(
                TARGET_ACCOUNT,
                request.snapshot.import_id.parse().unwrap(),
            )
            .unwrap();
        let captured_generation = request
            .snapshot
            .data
            .growth_journal_state
            .as_ref()
            .map_or(0, |state| state.generation);
        let mut result = imported_result_for(&request);
        result.journal_confirmation.as_mut().unwrap().generation = captured_generation + 1;
        let before = guest_import_cache_snapshot(&ledger);

        assert_eq!(
            ledger.complete_guest_shop_import(TARGET_ACCOUNT, &result),
            Err(crate::storage::ledger::ScanError::InvalidShopState)
        );
        assert_eq!(
            guest_import_cache_snapshot(&ledger),
            before,
            "a receipt with changed journal metadata must not mutate any cache or capture row"
        );
    }

    #[test]
    fn guest_import_v2_completion_holds_when_selected_auth_account_differs() {
        let mut ledger = prepared_first_reset_ledger();
        let PendingGuestShopImport::V2(capture) = ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("schema-2 import capture expected")
        };
        let request = capture.request;
        ledger.set_selected_auth_account(TARGET_ACCOUNT).unwrap();
        ledger
            .mark_guest_shop_import_attempt_started(
                TARGET_ACCOUNT,
                request.snapshot.import_id.parse().unwrap(),
            )
            .unwrap();
        ledger
            .set_selected_auth_account(OTHER_TARGET_ACCOUNT)
            .unwrap();
        let result = imported_result_for(&request);
        let before = guest_import_cache_snapshot(&ledger);

        let completion = ledger
            .complete_guest_shop_import(TARGET_ACCOUNT, &result)
            .unwrap();

        assert_eq!(completion, GuestImportCompletion::Held);
        assert_eq!(guest_import_cache_snapshot(&ledger), before);
    }

    #[test]
    fn guest_import_v2_completion_held_server_result_does_not_transfer_local_ownership() {
        let mut ledger = prepared_first_reset_ledger();
        let PendingGuestShopImport::V2(capture) = ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("schema-2 import capture expected");
        };
        let request = capture.request;
        ledger.set_selected_auth_account(TARGET_ACCOUNT).unwrap();
        ledger
            .mark_guest_shop_import_attempt_started(
                TARGET_ACCOUNT,
                request.snapshot.import_id.parse().unwrap(),
            )
            .unwrap();
        let result = GuestShopImportV2Result {
            schema_version: 2,
            import_id: request.snapshot.import_id.clone(),
            account_id: request.snapshot.target_account_id.clone(),
            source_fingerprint: request.snapshot.source_fingerprint.clone(),
            status: crate::domain::guest_shop_import::GuestImportStatus::SourceUnverifiable,
            shop_state: None,
            planet_state: None,
            effect_timeline: None,
            canonical_contribution: None,
            journal_confirmation: None,
            ack: None,
        };
        assert!(
            crate::sync::client::validate_guest_shop_import_v2_result(&request, &result).is_ok()
        );
        let before = guest_import_cache_snapshot(&ledger);

        assert_eq!(
            ledger
                .complete_guest_shop_import(TARGET_ACCOUNT, &result)
                .unwrap(),
            GuestImportCompletion::Held
        );
        let after = guest_import_cache_snapshot(&ledger);
        let business_tables = |snapshot: &[(String, String)]| {
            snapshot
                .iter()
                .filter(|(name, _)| {
                    name != "guest_shop_import_v2_capture_schema"
                        && name != "guest_shop_import_v2_capture"
                })
                .cloned()
                .collect::<Vec<_>>()
        };
        assert_eq!(
            business_tables(&after),
            business_tables(&before),
            "a server-held result must leave all planet, usage, journal, and shop state unchanged"
        );
        let before_capture_schema = before
            .iter()
            .find(|(name, _)| name == "guest_shop_import_v2_capture_schema")
            .unwrap();
        let after_capture_schema = after
            .iter()
            .find(|(name, _)| name == "guest_shop_import_v2_capture_schema")
            .unwrap();
        assert!(!before_capture_schema.1.contains("result_json"));
        assert!(after_capture_schema.1.contains("result_json"));
        let (phase, correction_hold, owner, active_account): (
            String,
            i64,
            Option<String>,
            String,
        ) = ledger
            .connection
            .query_row(
                "SELECT
                    (SELECT phase FROM guest_shop_import_v2_capture WHERE target_account_id=?1),
                    (SELECT correction_hold FROM guest_shop_import_v2_capture WHERE target_account_id=?1),
                    (SELECT account_id FROM planet_usage_owner WHERE event_key=?2),
                    (SELECT value FROM setting WHERE key='planet_account_id')",
                rusqlite::params![TARGET_ACCOUNT, "v2-capture-raw-1m"],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(phase, "held");
        assert_eq!(correction_hold, 1);
        assert_eq!(owner.as_deref(), Some("local"));
        assert_eq!(active_account, "local");
    }

    #[test]
    fn guest_import_v2_completion_keeps_append_delta_unacknowledged_without_correction_hold() {
        let mut ledger = prepared_first_reset_ledger();
        let PendingGuestShopImport::V2(capture) = ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("schema-2 import capture expected");
        };
        let request = capture.request;
        let import_id = uuid::Uuid::parse_str(&request.snapshot.import_id).unwrap();
        ledger.set_selected_auth_account(TARGET_ACCOUNT).unwrap();
        ledger
            .mark_guest_shop_import_attempt_started(TARGET_ACCOUNT, import_id)
            .unwrap();

        let reset_at = parse_utc(
            &request
                .snapshot
                .provenance
                .reset_receipt
                .result
                .reset_at_utc,
        )
        .unwrap();
        let appended_event_key = "v2-completion-raw-append";
        ledger
            .insert(&raw_one_date_record_with_tokens(
                appended_event_key,
                reset_at - Duration::milliseconds(1),
                1,
            ))
            .unwrap();
        let transaction = ledger.connection.transaction().unwrap();
        Ledger::prepare_growth_journal_in_transaction(&transaction).unwrap();
        transaction.commit().unwrap();
        assert_eq!(
            ledger.guest_import_source_relation(import_id).unwrap(),
            GuestImportSourceRelation::AppendOnly
        );

        let result = imported_result_for(&request);
        let timeline_account_id = &result.effect_timeline.as_ref().unwrap().account_id;
        assert_eq!(
            timeline_account_id,
            request
                .snapshot
                .target_account_id
                .strip_prefix("account:")
                .unwrap(),
            "the server timeline uses a bare auth UUID while the native account key is prefixed"
        );
        assert_ne!(timeline_account_id, &request.snapshot.target_account_id);
        let snapshot = &request.snapshot;
        let expected_cycle = &snapshot.provenance.reset_receipt.result.new_cycle_id;
        let shop = result.shop_state.as_ref().unwrap();
        let planet = result.planet_state.as_ref().unwrap();
        let timeline = result.effect_timeline.as_ref().unwrap();
        let canonical = result.canonical_contribution.as_ref().unwrap();
        let journal = result.journal_confirmation.as_ref().unwrap();
        let ack = result.ack.as_ref().unwrap();
        let canonical_fingerprint = sha2::Sha256::digest(canonical_json_bytes(canonical).unwrap())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let correlation_checks = [
            ("wire shape", result.validate_wire_shape().is_ok()),
            (
                "shop account",
                shop.account_id == snapshot.target_account_id,
            ),
            ("shop cycle", shop.current_cycle_id == *expected_cycle),
            ("planet cycle", planet.current_cycle_id == *expected_cycle),
            (
                "timeline account is bare target UUID",
                timeline.account_id == snapshot.target_account_id.strip_prefix("account:").unwrap(),
            ),
            (
                "timeline account equals prefixed native key",
                timeline.account_id == snapshot.target_account_id,
            ),
            (
                "timeline cycle",
                timeline.current_cycle_id == *expected_cycle,
            ),
            (
                "canonical device",
                canonical.raw.device_id == snapshot.provenance.device_id,
            ),
            (
                "canonical cycle",
                canonical.raw.current_cycle_id == *expected_cycle,
            ),
            (
                "canonical payload",
                canonical == &snapshot.canonical_payload,
            ),
            (
                "canonical version",
                canonical.canonical_version == ack.canonical_version,
            ),
            (
                "ACK lineage",
                ack.lineage_id == snapshot.provenance.lineage_id,
            ),
            ("ACK device", ack.device_id == snapshot.provenance.device_id),
            (
                "ACK watermark",
                ack.ingest_watermark == snapshot.provenance.ingest_watermark,
            ),
            (
                "ACK count",
                ack.occurrence_count == snapshot.provenance.occurrence_count,
            ),
            (
                "ACK prefix",
                ack.prefix_fingerprint == snapshot.provenance.prefix_fingerprint,
            ),
            (
                "ACK canonical fingerprint",
                ack.canonical_payload_fingerprint == canonical_fingerprint,
            ),
            (
                "ACK journal rows",
                ack.journal_entries.iter().all(|ack_entry| {
                    journal.entries.iter().any(|entry| {
                        entry.device_id == ack_entry.logical_key.device_id
                            && entry.cycle_id == ack_entry.logical_key.cycle_id
                            && entry.bucket_date == ack_entry.logical_key.bucket_date
                            && entry.agent == ack_entry.logical_key.agent
                            && entry.generation == ack_entry.logical_key.generation
                            && entry.revision == ack_entry.revision
                            && entry.payload_hash == ack_entry.payload_hash
                    })
                }),
            ),
        ];
        assert!(
            crate::sync::client::validate_guest_shop_import_v2_result(&request, &result).is_ok(),
            "imported result correlation rejected a server-shaped result: {correlation_checks:?}"
        );
        let mut wrong_timeline_account = imported_result_for(&request);
        wrong_timeline_account
            .effect_timeline
            .as_mut()
            .unwrap()
            .account_id = OTHER_TARGET_ACCOUNT
            .strip_prefix("account:")
            .unwrap()
            .into();
        assert!(crate::sync::client::validate_guest_shop_import_v2_result(
            &request,
            &wrong_timeline_account,
        )
        .is_err());

        let mut wrong_timeline_cycle = imported_result_for(&request);
        wrong_timeline_cycle
            .effect_timeline
            .as_mut()
            .unwrap()
            .current_cycle_id = "00000000-0000-4000-a000-000000000099".into();
        assert!(crate::sync::client::validate_guest_shop_import_v2_result(
            &request,
            &wrong_timeline_cycle,
        )
        .is_err());

        let completion = ledger
            .complete_guest_shop_import(TARGET_ACCOUNT, &result)
            .unwrap();

        assert_eq!(completion, GuestImportCompletion::Imported);
        let (phase, correction_hold, result_json): (String, i64, Option<String>) = ledger
            .connection
            .query_row(
                "SELECT phase,correction_hold,result_json FROM guest_shop_import_v2_capture
                 WHERE target_account_id=?1",
                [TARGET_ACCOUNT],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(phase, "imported");
        assert_eq!(correction_hold, 0);
        assert_eq!(
            result_json.as_deref(),
            Some(std::str::from_utf8(&canonical_json_bytes(&result).unwrap()).unwrap())
        );

        let (raw_exists, owner): (bool, Option<String>) = ledger
            .connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM usage_record WHERE event_key=?1),
                        (SELECT account_id FROM planet_usage_owner WHERE event_key=?1)",
                [appended_event_key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!(raw_exists, "append raw usage must survive local completion");
        assert_eq!(owner.as_deref(), Some(TARGET_ACCOUNT));

        let ack_entry = result
            .ack
            .as_ref()
            .unwrap()
            .journal_entries
            .first()
            .expect("the captured prefix includes a journal acknowledgement");
        let (local_revision, local_payload_hash): (i64, String) = ledger
            .connection
            .query_row(
                "SELECT revision,payload_hash FROM growth_journal_entry
                 WHERE account_id='local' AND device_id=?1 AND cycle_id=?2 AND bucket_date=?3 AND agent=?4",
                rusqlite::params![
                    ack_entry.logical_key.device_id,
                    ack_entry.logical_key.cycle_id,
                    ack_entry.logical_key.bucket_date,
                    crate::storage::ledger::agent_name(ack_entry.logical_key.agent),
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!(local_revision > ack_entry.revision as i64);
        let (revision, acknowledged_revision, payload_hash): (i64, i64, String) = ledger
            .connection
            .query_row(
                "SELECT revision,acknowledged_revision,payload_hash FROM growth_journal_entry
                 WHERE account_id=?1 AND device_id=?2 AND cycle_id=?3 AND bucket_date=?4 AND agent=?5",
                rusqlite::params![
                    TARGET_ACCOUNT,
                    ack_entry.logical_key.device_id,
                    ack_entry.logical_key.cycle_id,
                    ack_entry.logical_key.bucket_date,
                    crate::storage::ledger::agent_name(ack_entry.logical_key.agent),
                ],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(revision, local_revision);
        assert_eq!(acknowledged_revision, ack_entry.revision as i64);
        assert_eq!(payload_hash, local_payload_hash);
        assert_ne!(payload_hash, ack_entry.payload_hash);

        let before_replay = guest_import_cache_snapshot(&ledger);
        let replay = ledger
            .complete_guest_shop_import(TARGET_ACCOUNT, &result)
            .unwrap();
        assert_eq!(replay, GuestImportCompletion::Imported);
        assert_eq!(guest_import_cache_snapshot(&ledger), before_replay);
    }

    #[test]
    fn fresh_v2_capture_is_persisted_before_dispatch_returns() {
        let mut ledger = prepared_first_reset_ledger();
        ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap();

        let table_count: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table'
                 AND name='guest_shop_import_v2_capture'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 1, "v2 capture table must be created");
        let row_count: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM guest_shop_import_v2_capture WHERE target_account_id=?1",
                [TARGET_ACCOUNT],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(row_count, 1, "one durable v2 row must back the response");
    }

    #[test]
    fn exports_native_v2_first_reset_fixture_from_durable_capture() {
        let mut ledger = prepared_first_reset_ledger();
        let actual_builder_payload = ledger.shop_device_contribution(false).unwrap();
        let PendingGuestShopImport::V2(captured) = ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("a normal first reset must produce a schema 2 capture");
        };

        assert_eq!(captured.request.schema_version, 2);
        assert_eq!(
            captured.request.snapshot.canonical_payload,
            actual_builder_payload
        );
        assert_eq!(
            captured.request.snapshot.disposition,
            crate::domain::cosmetic_shop::GuestShopImportDisposition::LocalIntegrityValidated
        );
        assert_eq!(captured.request.snapshot.provenance.occurrence_count, 1);
        assert_eq!(
            captured.request.snapshot.provenance.occurrences[0].total_tokens,
            1_000_000
        );
        assert_eq!(captured.request.snapshot.provenance.ingest_watermark, 1);
        assert_eq!(
            captured
                .request
                .snapshot
                .canonical_payload
                .raw
                .current_planet_tokens,
            0
        );
        assert!(captured
            .request
            .snapshot
            .data
            .reset_settlement_proofs
            .iter()
            .all(|proof| proof.settled_bonus_tokens == Some(0)));
        assert!(captured.request.snapshot.data.game_rewards.is_empty());
        assert!(captured.request.snapshot.data.wallet_credits.is_empty());
        assert!(captured.request.snapshot.data.era_progress.is_empty());
        assert_eq!(
            captured
                .request
                .snapshot
                .provenance
                .reset_receipt
                .result
                .frozen_deadline_before_reset_utc,
            None
        );

        let request_bytes = canonical_json_bytes(&captured.request).unwrap();
        let persisted_json: String = ledger
            .connection
            .query_row(
                "SELECT request_json FROM guest_shop_import_v2_capture WHERE target_account_id=?1",
                [TARGET_ACCOUNT],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(persisted_json.as_bytes(), request_bytes);

        if let Some(output_dir) = std::env::var_os("SHOP_GUEST_IMPORT_V2_FIXTURE_DIR") {
            let output_dir = std::path::PathBuf::from(output_dir);
            std::fs::create_dir_all(&output_dir).unwrap();
            let request_json = String::from_utf8(request_bytes).unwrap();
            assert!(!request_json.contains("$native_v2_first_reset_payload$"));
            let fixture_inc = format!(
                "create function pg_temp.shop_guest_import_v2_first_reset_request()\nreturns jsonb\nlanguage sql\nimmutable\nas $native_v2_first_reset_fixture$\n  select $native_v2_first_reset_payload${request_json}$native_v2_first_reset_payload$::jsonb;\n$native_v2_first_reset_fixture$;\n"
            );
            std::fs::write(
                output_dir.join("shop_guest_import_v2_first_reset.json"),
                &request_json,
            )
            .unwrap();
            std::fs::write(
                output_dir.join("shop_guest_import_v2_first_reset.inc"),
                fixture_inc,
            )
            .unwrap();
        }
    }

    #[test]
    fn v2_disposition_accepts_a_receipt_verified_raw_wallet_claim() {
        let mut ledger = prepared_first_reset_ledger();
        let v1_data = shop_import::capture_data(&ledger.connection).unwrap();
        assert_eq!(
            shop_import::disposition(&v1_data),
            crate::domain::cosmetic_shop::GuestShopImportDisposition::SourceUnverifiable
        );

        let PendingGuestShopImport::V2(captured) = ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("a normal receipt-backed first reset must capture schema 2");
        };

        assert_eq!(
            captured.request.snapshot.disposition,
            crate::domain::cosmetic_shop::GuestShopImportDisposition::LocalIntegrityValidated
        );
        assert_eq!(
            captured
                .request
                .snapshot
                .data
                .unverified_planet_wallet_claims
                .len(),
            1,
            "the native raw wallet claim remains in the request"
        );
        assert!(
            !captured
                .request
                .snapshot
                .data
                .effect_cycle_bounds_authoritative
        );
        assert!(captured.request.snapshot.data.integrity_issues.is_empty());
    }

    #[test]
    fn v2_disposition_keeps_a_mismatched_raw_wallet_claim_unverifiable() {
        let mut ledger = prepared_first_reset_ledger();
        ledger
            .connection
            .execute("UPDATE planet_wallet_credit SET amount=amount+1", [])
            .unwrap();

        let PendingGuestShopImport::V2(captured) = ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("a receipt-backed source with a mismatched claim remains a schema 2 capture");
        };

        assert_eq!(
            captured.request.snapshot.disposition,
            crate::domain::cosmetic_shop::GuestShopImportDisposition::SourceUnverifiable
        );
        assert!(captured.request.snapshot.data.reset_receipts_unverifiable);
        assert!(!captured.request.snapshot.data.integrity_issues.is_empty());
    }

    #[test]
    fn captures_actual_builder_payload_and_prefix() {
        let mut ledger = prepared_first_reset_ledger();
        let actual_builder_payload = ledger.shop_device_contribution(false).unwrap();

        let PendingGuestShopImport::V2(captured) = ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("a fresh, receipt-backed first reset must capture a schema 2 request");
        };

        assert_eq!(captured.request.schema_version, 2);
        assert_eq!(
            captured.request.snapshot.canonical_payload,
            actual_builder_payload
        );
        assert_eq!(
            captured.request.snapshot.provenance.prefix_fingerprint,
            prefix_fingerprint(&captured.request.snapshot.provenance).unwrap()
        );
        assert_eq!(
            captured.request.snapshot.source_fingerprint,
            source_fingerprint(&captured.request.snapshot).unwrap()
        );
        let table_count: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table'
                 AND name='guest_shop_import_v2_capture'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 1, "v2 capture must be durable");
        let (stored_import_id, stored_request_json, phase, correction_hold): (
            String,
            String,
            String,
            i64,
        ) = ledger
            .connection
            .query_row(
                "SELECT import_id,request_json,phase,correction_hold
                 FROM guest_shop_import_v2_capture WHERE target_account_id=?1",
                [TARGET_ACCOUNT],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(stored_import_id, captured.request.snapshot.import_id);
        assert_eq!(phase, "captured");
        assert_eq!(correction_hold, 0);
        let persisted_request: GuestShopImportV2Request =
            serde_json::from_str(&stored_request_json).unwrap();
        assert_eq!(persisted_request, captured.request);
        assert_eq!(
            stored_request_json.as_bytes(),
            canonical_json_bytes(&captured.request).unwrap()
        );

        let PendingGuestShopImport::V2(recaptured) = ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("a v2 target capture must retain its versioned request");
        };
        assert_eq!(recaptured.request, captured.request);
        let row_count: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM guest_shop_import_v2_capture WHERE target_account_id=?1",
                [TARGET_ACCOUNT],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(row_count, 1, "recapture must retain one target row");
    }

    #[test]
    fn append_after_capture_keeps_request_and_classifies_append_only() {
        let mut ledger = prepared_first_reset_ledger();
        let PendingGuestShopImport::V2(captured) = ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("a fresh first reset must capture a schema 2 request");
        };
        let stored_request_json: String = ledger
            .connection
            .query_row(
                "SELECT request_json FROM guest_shop_import_v2_capture WHERE target_account_id=?1",
                [TARGET_ACCOUNT],
                |row| row.get(0),
            )
            .unwrap();
        let reset_at = parse_utc(
            &captured
                .request
                .snapshot
                .provenance
                .reset_receipt
                .result
                .reset_at_utc,
        )
        .unwrap();
        ledger
            .insert(&raw_one_date_record(
                "v2-capture-raw-append-1",
                reset_at - Duration::milliseconds(1),
            ))
            .unwrap();

        let PendingGuestShopImport::V2(pending) = ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("a durable v2 capture must remain v2 after append");
        };
        assert_eq!(
            pending.source_relation,
            GuestImportSourceRelation::AppendOnly
        );
        assert_eq!(pending.request, captured.request);
        assert!(!pending.correction_hold);
        let import_id = uuid::Uuid::parse_str(&captured.request.snapshot.import_id).unwrap();
        assert_eq!(
            ledger.guest_import_source_relation(import_id).unwrap(),
            GuestImportSourceRelation::AppendOnly
        );
        let Some(PendingGuestShopImport::V2(pending_read)) = ledger
            .pending_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("pending read must preserve the durable v2 request");
        };
        assert_eq!(pending_read.request, captured.request);
        assert_eq!(
            pending_read.source_relation,
            GuestImportSourceRelation::AppendOnly
        );
        let current_request_json: String = ledger
            .connection
            .query_row(
                "SELECT request_json FROM guest_shop_import_v2_capture WHERE target_account_id=?1",
                [TARGET_ACCOUNT],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(current_request_json, stored_request_json);
    }

    #[test]
    fn captured_prefix_correction_keeps_id_and_payload_and_sets_hold() {
        let mut ledger = prepared_first_reset_ledger();
        let PendingGuestShopImport::V2(captured) = ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("a fresh first reset must capture a schema 2 request");
        };
        let stored_request_json: String = ledger
            .connection
            .query_row(
                "SELECT request_json FROM guest_shop_import_v2_capture WHERE target_account_id=?1",
                [TARGET_ACCOUNT],
                |row| row.get(0),
            )
            .unwrap();
        ledger
            .connection
            .execute(
                "UPDATE usage_record SET total_tokens=total_tokens+1 WHERE event_key=?1",
                ["v2-capture-raw-1m"],
            )
            .unwrap();

        let PendingGuestShopImport::V2(pending) = ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("a durable v2 capture must remain v2 after correction");
        };
        assert_eq!(
            pending.source_relation,
            GuestImportSourceRelation::CapturedPrefixChanged
        );
        assert!(pending.correction_hold);
        assert_eq!(pending.phase, GuestImportPhase::Held);
        assert_eq!(pending.request, captured.request);
        let (import_id, current_request_json): (String, String) = ledger
            .connection
            .query_row(
                "SELECT import_id,request_json FROM guest_shop_import_v2_capture WHERE target_account_id=?1",
                [TARGET_ACCOUNT],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(import_id, captured.request.snapshot.import_id);
        assert_eq!(current_request_json, stored_request_json);
        let import_uuid = uuid::Uuid::parse_str(&captured.request.snapshot.import_id).unwrap();
        assert_eq!(
            ledger.guest_import_source_relation(import_uuid).unwrap(),
            GuestImportSourceRelation::CapturedPrefixChanged
        );
        let Some(PendingGuestShopImport::V2(pending_read)) = ledger
            .pending_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("corrected v2 request must remain pending as a v2 hold");
        };
        assert_eq!(pending_read.request, captured.request);
        assert_eq!(
            pending_read.source_relation,
            GuestImportSourceRelation::CapturedPrefixChanged
        );
        assert!(pending_read.correction_hold);
        assert_eq!(pending_read.phase, GuestImportPhase::Held);
    }

    #[test]
    fn post_reset_zero_occurrence_is_captured_as_valid_v2_with_empty_daily_totals() {
        let mut ledger = prepared_first_reset_ledger();
        let reset_at = parse_utc(
            &ledger
                .connection
                .query_row::<String, _, _>(
                    "SELECT reset_at_utc FROM guest_provenance_reset_receipt",
                    [],
                    |row| row.get(0),
                )
                .unwrap(),
        )
        .unwrap();
        let new_cycle_id = ledger.planet_cycle_id().unwrap();
        let old_cycle_id: String = ledger
            .connection
            .query_row(
                "SELECT previous_cycle_id FROM guest_provenance_reset_receipt",
                [],
                |row| row.get(0),
            )
            .unwrap();
        ledger
            .insert(&raw_one_date_record_with_tokens(
                "v2-capture-post-reset-zero",
                reset_at + Duration::microseconds(1),
                0,
            ))
            .unwrap();

        let PendingGuestShopImport::V2(captured) = ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("a complete zero-token occurrence after reset remains in the v2 domain");
        };
        let actual_builder = ledger.shop_device_contribution(false).unwrap();

        assert_eq!(
            captured.request.snapshot.disposition,
            crate::domain::cosmetic_shop::GuestShopImportDisposition::LocalIntegrityValidated,
            "a verified zero-token current-cycle occurrence stays in the supported source domain"
        );
        assert_eq!(captured.request.snapshot.canonical_payload, actual_builder);
        assert_eq!(
            captured
                .request
                .snapshot
                .canonical_payload
                .raw
                .current_planet_tokens,
            0
        );
        assert!(captured
            .request
            .snapshot
            .canonical_payload
            .raw
            .daily_tokens
            .is_empty());
        assert_eq!(
            captured.request.snapshot.data.current_cycle_usage_tokens,
            Some(0)
        );
        let old_usage = captured
            .request
            .snapshot
            .data
            .usage_aggregates
            .iter()
            .find(|aggregate| aggregate.cycle_id.as_deref() == Some(old_cycle_id.as_str()))
            .expect("the old cycle retains its proven date/agent aggregate");
        let new_usage = captured
            .request
            .snapshot
            .data
            .usage_aggregates
            .iter()
            .find(|aggregate| aggregate.cycle_id.as_deref() == Some(new_cycle_id.as_str()))
            .expect("the new cycle retains its proven zero-token date/agent aggregate");
        assert_eq!(
            (old_usage.event_count, old_usage.total_tokens),
            (1, Some(1_000_000))
        );
        assert_eq!(
            (new_usage.event_count, new_usage.total_tokens),
            (1, Some(0))
        );
        assert_eq!(old_usage.bucket_date, new_usage.bucket_date);
        assert_eq!(captured.request.snapshot.data.daily_agent_totals.len(), 1);
        assert_eq!(
            captured.request.snapshot.data.daily_agent_totals[0].total_tokens,
            Some(1_000_000)
        );
        let post_reset = captured
            .request
            .snapshot
            .provenance
            .occurrences
            .iter()
            .find(|occurrence| occurrence.cycle_id == new_cycle_id)
            .expect("the zero-token occurrence remains in the captured provenance prefix");
        assert_eq!(post_reset.total_tokens, 0);
        assert_eq!(captured.request.snapshot.provenance.occurrence_count, 2);
    }

    #[test]
    fn positive_new_cycle_usage_is_captured_but_stays_held() {
        let mut ledger = prepared_first_reset_ledger();
        let reset_at = parse_utc(
            &ledger
                .connection
                .query_row::<String, _, _>(
                    "SELECT reset_at_utc FROM guest_provenance_reset_receipt",
                    [],
                    |row| row.get(0),
                )
                .unwrap(),
        )
        .unwrap();
        ledger
            .insert(&raw_one_date_record_with_tokens(
                "v2-capture-post-reset-positive",
                reset_at + Duration::microseconds(1),
                23,
            ))
            .unwrap();

        let PendingGuestShopImport::V2(captured) = ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("positive new-cycle usage must remain on the v2 hold path");
        };
        assert_eq!(
            captured.request.snapshot.disposition,
            crate::domain::cosmetic_shop::GuestShopImportDisposition::SourceUnverifiable
        );
        assert_eq!(
            captured
                .request
                .snapshot
                .canonical_payload
                .raw
                .current_planet_tokens,
            23
        );
        assert_eq!(captured.phase, GuestImportPhase::Held);
        assert!(!captured.correction_hold);
        let (phase, request_json): (String, String) = ledger
            .connection
            .query_row(
                "SELECT phase,request_json FROM guest_shop_import_v2_capture WHERE target_account_id=?1",
                [TARGET_ACCOUNT],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(phase, "held");
        assert_eq!(
            serde_json::from_str::<GuestShopImportV2Request>(&request_json).unwrap(),
            captured.request
        );
    }

    #[test]
    fn v2_capture_reopens_with_same_request_and_target_identity() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("guest-import.sqlite");
        let mut ledger = prepared_first_reset_ledger_at(&path);
        let PendingGuestShopImport::V2(captured) = ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("a fresh first reset must capture a schema 2 request");
        };
        let captured_json: String = ledger
            .connection
            .query_row(
                "SELECT request_json FROM guest_shop_import_v2_capture WHERE target_account_id=?1",
                [TARGET_ACCOUNT],
                |row| row.get(0),
            )
            .unwrap();
        drop(ledger);

        let reopened = Ledger::open(&path, chrono_tz::UTC).unwrap();
        let Some(PendingGuestShopImport::V2(restored)) = reopened
            .pending_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("reopening must retain the versioned v2 capture");
        };
        assert_eq!(restored.request, captured.request);
        assert_eq!(
            restored.request.snapshot.import_id,
            captured.request.snapshot.import_id
        );
        let reopened_json: String = reopened
            .connection
            .query_row(
                "SELECT request_json FROM guest_shop_import_v2_capture WHERE target_account_id=?1",
                [TARGET_ACCOUNT],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(reopened_json, captured_json);
    }

    #[test]
    fn unverifiable_v2_source_persists_hold_without_rewriting_request() {
        let mut ledger = prepared_first_reset_ledger();
        let PendingGuestShopImport::V2(captured) = ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("a fresh first reset must capture a schema 2 request");
        };
        let stored_request_json: String = ledger
            .connection
            .query_row(
                "SELECT request_json FROM guest_shop_import_v2_capture WHERE target_account_id=?1",
                [TARGET_ACCOUNT],
                |row| row.get(0),
            )
            .unwrap();
        ledger
            .connection
            .execute(
                "UPDATE guest_shop_import_v2_capture SET source_manifest_json='{}'
                 WHERE target_account_id=?1",
                [TARGET_ACCOUNT],
            )
            .unwrap();

        let import_uuid = uuid::Uuid::parse_str(&captured.request.snapshot.import_id).unwrap();
        assert_eq!(
            ledger.guest_import_source_relation(import_uuid).unwrap(),
            GuestImportSourceRelation::Unverifiable
        );
        let Some(PendingGuestShopImport::V2(pending)) = ledger
            .pending_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("unverifiable source must retain the immutable v2 request");
        };
        assert_eq!(pending.request, captured.request);
        assert_eq!(
            pending.source_relation,
            GuestImportSourceRelation::Unverifiable
        );
        assert!(pending.correction_hold);
        assert_eq!(pending.phase, GuestImportPhase::Held);
        let (import_id, request_json, correction_hold): (String, String, i64) = ledger
            .connection
            .query_row(
                "SELECT import_id,request_json,correction_hold
                 FROM guest_shop_import_v2_capture WHERE target_account_id=?1",
                [TARGET_ACCOUNT],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(import_id, captured.request.snapshot.import_id);
        assert_eq!(request_json, stored_request_json);
        assert_eq!(correction_hold, 1);
    }

    #[test]
    fn attempt_started_correction_keeps_phase_and_immutable_request() {
        let mut ledger = prepared_first_reset_ledger();
        let PendingGuestShopImport::V2(captured) = ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("a fresh first reset must capture a schema 2 request");
        };
        let stored_request_json: String = ledger
            .connection
            .query_row(
                "SELECT request_json FROM guest_shop_import_v2_capture WHERE target_account_id=?1",
                [TARGET_ACCOUNT],
                |row| row.get(0),
            )
            .unwrap();
        ledger
            .connection
            .execute(
                "UPDATE guest_shop_import_v2_capture SET phase='attempt_started'
                 WHERE target_account_id=?1",
                [TARGET_ACCOUNT],
            )
            .unwrap();
        ledger
            .connection
            .execute(
                "UPDATE usage_record SET total_tokens=total_tokens+1 WHERE event_key=?1",
                ["v2-capture-raw-1m"],
            )
            .unwrap();

        let Some(PendingGuestShopImport::V2(pending)) = ledger
            .pending_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("correction must retain the attempted v2 request");
        };
        assert_eq!(
            pending.source_relation,
            GuestImportSourceRelation::CapturedPrefixChanged
        );
        assert_eq!(pending.phase, GuestImportPhase::AttemptStarted);
        assert!(pending.correction_hold);
        assert_eq!(pending.request, captured.request);
        let (import_id, request_json, phase, correction_hold): (String, String, String, i64) =
            ledger
                .connection
                .query_row(
                    "SELECT import_id,request_json,phase,correction_hold
                 FROM guest_shop_import_v2_capture WHERE target_account_id=?1",
                    [TARGET_ACCOUNT],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .unwrap();
        assert_eq!(import_id, captured.request.snapshot.import_id);
        assert_eq!(request_json, stored_request_json);
        assert_eq!(phase, "attempt_started");
        assert_eq!(correction_hold, 1);
    }

    #[test]
    fn guest_import_v2_completion_attempted_capture_correction_returns_imported_with_atomic_correction_hold(
    ) {
        let mut ledger = prepared_first_reset_ledger();
        let PendingGuestShopImport::V2(captured) = ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("a fresh first reset must capture a schema 2 request");
        };
        let request = captured.request;
        ledger.set_selected_auth_account(TARGET_ACCOUNT).unwrap();
        ledger
            .mark_guest_shop_import_attempt_started(
                TARGET_ACCOUNT,
                request.snapshot.import_id.parse().unwrap(),
            )
            .unwrap();
        ledger
            .connection
            .execute(
                "UPDATE usage_record SET total_tokens=total_tokens+1 WHERE event_key=?1",
                ["v2-capture-raw-1m"],
            )
            .unwrap();
        let Some(PendingGuestShopImport::V2(held_capture)) = ledger
            .pending_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("the attempted capture must remain available with a correction hold");
        };
        assert_eq!(
            held_capture.source_relation,
            GuestImportSourceRelation::CapturedPrefixChanged
        );
        assert_eq!(held_capture.phase, GuestImportPhase::AttemptStarted);
        assert!(held_capture.correction_hold);
        assert_eq!(held_capture.request, request);

        let result = imported_result_for(&request);
        assert_eq!(
            ledger
                .complete_guest_shop_import(TARGET_ACCOUNT, &result)
                .unwrap(),
            GuestImportCompletion::ImportedWithCorrectionHold
        );
        let (phase, correction_hold, stored_result): (String, i64, String) = ledger
            .connection
            .query_row(
                "SELECT phase,correction_hold,result_json FROM guest_shop_import_v2_capture
                 WHERE target_account_id=?1",
                [TARGET_ACCOUNT],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(phase, "imported");
        assert_eq!(correction_hold, 1);
        assert_eq!(
            stored_result,
            String::from_utf8(canonical_json_bytes(&result).unwrap()).unwrap()
        );
        let (raw_tokens, owner, active_account): (i64, Option<String>, String) = ledger
            .connection
            .query_row(
                "SELECT
                    (SELECT total_tokens FROM usage_record WHERE event_key=?1),
                    (SELECT account_id FROM planet_usage_owner WHERE event_key=?1),
                    (SELECT value FROM setting WHERE key='planet_account_id')",
                ["v2-capture-raw-1m"],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(raw_tokens, 1_000_001);
        assert_eq!(owner.as_deref(), Some(TARGET_ACCOUNT));
        assert_eq!(active_account, TARGET_ACCOUNT);
    }

    #[test]
    fn attempt_started_unverifiable_source_keeps_phase_and_immutable_request() {
        let mut ledger = prepared_first_reset_ledger();
        let PendingGuestShopImport::V2(captured) = ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("a fresh first reset must capture a schema 2 request");
        };
        let stored_request_json: String = ledger
            .connection
            .query_row(
                "SELECT request_json FROM guest_shop_import_v2_capture WHERE target_account_id=?1",
                [TARGET_ACCOUNT],
                |row| row.get(0),
            )
            .unwrap();
        ledger
            .connection
            .execute(
                "UPDATE guest_shop_import_v2_capture
                 SET phase='attempt_started',source_manifest_json='{}'
                 WHERE target_account_id=?1",
                [TARGET_ACCOUNT],
            )
            .unwrap();

        let Some(PendingGuestShopImport::V2(pending)) = ledger
            .pending_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("unverifiable lineage must retain the attempted v2 request");
        };
        assert_eq!(
            pending.source_relation,
            GuestImportSourceRelation::Unverifiable
        );
        assert_eq!(pending.phase, GuestImportPhase::AttemptStarted);
        assert!(pending.correction_hold);
        assert_eq!(pending.request, captured.request);
        let (import_id, request_json, phase, correction_hold): (String, String, String, i64) =
            ledger
                .connection
                .query_row(
                    "SELECT import_id,request_json,phase,correction_hold
                 FROM guest_shop_import_v2_capture WHERE target_account_id=?1",
                    [TARGET_ACCOUNT],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .unwrap();
        assert_eq!(import_id, captured.request.snapshot.import_id);
        assert_eq!(request_json, stored_request_json);
        assert_eq!(phase, "attempt_started");
        assert_eq!(correction_hold, 1);
    }

    #[test]
    fn attempt_started_invalid_serialized_capture_keeps_durable_phase() {
        let mut ledger = prepared_first_reset_ledger();
        let PendingGuestShopImport::V2(captured) = ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("a fresh first reset must capture a schema 2 request");
        };
        ledger
            .connection
            .execute(
                "UPDATE guest_shop_import_v2_capture
                 SET phase='attempt_started',request_json='not-json'
                 WHERE target_account_id=?1",
                [TARGET_ACCOUNT],
            )
            .unwrap();

        assert!(matches!(
            ledger.capture_guest_shop_import_request(TARGET_ACCOUNT),
            Err(ScanError::InvalidShopState)
        ));
        let (import_id, request_json, phase, correction_hold): (String, String, String, i64) =
            ledger
                .connection
                .query_row(
                    "SELECT import_id,request_json,phase,correction_hold
                 FROM guest_shop_import_v2_capture WHERE target_account_id=?1",
                    [TARGET_ACCOUNT],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .unwrap();
        assert_eq!(import_id, captured.request.snapshot.import_id);
        assert_eq!(request_json, "not-json");
        assert_eq!(phase, "attempt_started");
        assert_eq!(correction_hold, 1);
    }

    #[test]
    fn switching_target_cannot_create_a_second_v2_capture() {
        let mut ledger = prepared_first_reset_ledger();
        let PendingGuestShopImport::V2(captured) = ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("a fresh first reset must capture a schema 2 request");
        };
        let stored_request_json: String = ledger
            .connection
            .query_row(
                "SELECT request_json FROM guest_shop_import_v2_capture WHERE target_account_id=?1",
                [TARGET_ACCOUNT],
                |row| row.get(0),
            )
            .unwrap();

        assert!(matches!(
            ledger.capture_guest_shop_import_request(OTHER_TARGET_ACCOUNT),
            Err(ScanError::InvalidShopState)
        ));
        let (row_count, import_id, request_json, phase, correction_hold): (
            i64,
            String,
            String,
            String,
            i64,
        ) = ledger
            .connection
            .query_row(
                "SELECT count(*),min(import_id),min(request_json),min(phase),min(correction_hold)
                 FROM guest_shop_import_v2_capture",
                [],
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
            .unwrap();
        assert_eq!(row_count, 1);
        assert_eq!(import_id, captured.request.snapshot.import_id);
        assert_eq!(request_json, stored_request_json);
        assert_eq!(phase, "captured");
        assert_eq!(correction_hold, 0);
        let PendingGuestShopImport::V2(restored) = ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("returning to the original target must recover the captured request");
        };
        assert_eq!(restored.request, captured.request);
        assert!(!restored.correction_hold);
    }

    #[test]
    fn embedded_target_mismatch_is_unverifiable_and_durably_held() {
        let mut ledger = prepared_first_reset_ledger();
        let PendingGuestShopImport::V2(captured) = ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("a fresh first reset must capture a schema 2 request");
        };
        let mut tampered_request = captured.request.clone();
        tampered_request.snapshot.target_account_id = OTHER_TARGET_ACCOUNT.to_owned();
        tampered_request.snapshot.source_fingerprint =
            source_fingerprint(&tampered_request.snapshot).unwrap();
        let tampered_json =
            String::from_utf8(canonical_json_bytes(&tampered_request).unwrap()).unwrap();
        ledger
            .connection
            .execute(
                "UPDATE guest_shop_import_v2_capture SET request_json=?1
                 WHERE target_account_id=?2",
                params![tampered_json, TARGET_ACCOUNT],
            )
            .unwrap();

        let import_id = uuid::Uuid::parse_str(&captured.request.snapshot.import_id).unwrap();
        assert_eq!(
            ledger.guest_import_source_relation(import_id).unwrap(),
            GuestImportSourceRelation::Unverifiable
        );
        let (stored_id, stored_json, phase, correction_hold): (String, String, String, i64) =
            ledger
                .connection
                .query_row(
                    "SELECT import_id,request_json,phase,correction_hold
                 FROM guest_shop_import_v2_capture WHERE target_account_id=?1",
                    [TARGET_ACCOUNT],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .unwrap();
        assert_eq!(stored_id, captured.request.snapshot.import_id);
        assert_eq!(stored_json, tampered_json);
        assert_eq!(phase, "held");
        assert_eq!(correction_hold, 1);
        assert!(matches!(
            ledger.capture_guest_shop_import_request(TARGET_ACCOUNT),
            Err(ScanError::InvalidShopState)
        ));
        let row_count: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM guest_shop_import_v2_capture",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(row_count, 1, "mismatch must never mint a replacement ID");
    }

    #[test]
    fn unavailable_provenance_is_unverifiable_and_durably_held() {
        let mut ledger = prepared_first_reset_ledger();
        let PendingGuestShopImport::V2(captured) = ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("a fresh first reset must capture a schema 2 request");
        };
        let stored_request_json: String = ledger
            .connection
            .query_row(
                "SELECT request_json FROM guest_shop_import_v2_capture WHERE target_account_id=?1",
                [TARGET_ACCOUNT],
                |row| row.get(0),
            )
            .unwrap();
        ledger
            .connection
            .execute_batch("DROP TABLE guest_provenance_mutation")
            .unwrap();

        let import_id = uuid::Uuid::parse_str(&captured.request.snapshot.import_id).unwrap();
        assert_eq!(
            ledger.guest_import_source_relation(import_id).unwrap(),
            GuestImportSourceRelation::Unverifiable
        );
        let (request_json, phase, correction_hold): (String, String, i64) = ledger
            .connection
            .query_row(
                "SELECT request_json,phase,correction_hold
                 FROM guest_shop_import_v2_capture WHERE target_account_id=?1",
                [TARGET_ACCOUNT],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(request_json, stored_request_json);
        assert_eq!(phase, "held");
        assert_eq!(correction_hold, 1);
    }

    #[test]
    fn same_connection_canonical_reader_matches_public_builder_bytes_and_hash() {
        let ledger = prepared_first_reset_ledger();
        let public_snapshot = ledger.shop_device_contribution(false).unwrap();
        let public_raw = ledger.planet_device_contribution(false).unwrap();
        let transaction = ledger.connection.unchecked_transaction().unwrap();
        let connection_raw =
            Ledger::planet_device_contribution_from_connection(&transaction, false).unwrap();
        let connection_snapshot =
            crate::storage::shop_effects::shop_device_contribution_from_connection(
                &transaction,
                connection_raw.clone(),
            )
            .unwrap();
        transaction.commit().unwrap();
        let public_raw_bytes = serde_json::to_vec(&public_raw).unwrap();
        let connection_raw_bytes = serde_json::to_vec(&connection_raw).unwrap();
        let public_bytes = serde_json::to_vec(&public_snapshot).unwrap();
        let connection_bytes = serde_json::to_vec(&connection_snapshot).unwrap();

        assert_eq!(public_raw_bytes, connection_raw_bytes);
        assert_eq!(
            sha2::Sha256::digest(&public_raw_bytes),
            sha2::Sha256::digest(&connection_raw_bytes)
        );
        assert_eq!(public_bytes, connection_bytes);
        assert_eq!(
            sha2::Sha256::digest(&public_bytes),
            sha2::Sha256::digest(&connection_bytes)
        );
    }

    #[test]
    fn v2_source_uses_persisted_provenance_cycle_bounds() {
        let ledger = prepared_first_reset_ledger();
        let Some((captured_data, _)) = collect_v2_domain_source(&ledger.connection).unwrap() else {
            panic!("the actual first-reset provenance should be readable");
        };
        let expected_bounds = {
            let mut statement = ledger
                .connection
                .prepare(
                    "SELECT cycle_id,started_at_utc,ended_at_utc
                     FROM guest_provenance_cycle ORDER BY started_at_utc",
                )
                .unwrap();
            statement
                .query_map([], |row| {
                    Ok(crate::domain::cosmetic_shop::ShopCycleBound {
                        cycle_id: row.get(0)?,
                        started_at_utc: row.get(1)?,
                        ended_at_utc: row.get(2)?,
                    })
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };

        assert_eq!(captured_data.effect_cycle_bounds, expected_bounds);
        assert!(!captured_data.effect_cycle_bounds_authoritative);
        assert!(captured_data.effect_timeline_state.is_none());
        assert!(captured_data.effect_history.is_empty());
    }

    #[test]
    fn v2_source_reset_proof_uses_persisted_receipt_effects() {
        let ledger = prepared_first_reset_ledger();
        let Some((captured_data, _)) = collect_v2_domain_source(&ledger.connection).unwrap() else {
            panic!("the actual first-reset provenance should be readable");
        };
        let receipt_json: String = ledger
            .connection
            .query_row(
                "SELECT receipt_json FROM guest_provenance_reset_receipt",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let receipt: crate::domain::guest_shop_import::GuestFirstResetReceipt =
            serde_json::from_str(&receipt_json).unwrap();
        let reset_proof = captured_data
            .reset_settlement_proofs
            .iter()
            .find(|proof| proof.previous_cycle_id == receipt.result.previous_cycle_id)
            .unwrap();

        assert_eq!(
            reset_proof.final_effect_revision,
            Some(receipt.result.final_effect_revision)
        );
        assert_eq!(
            reset_proof.final_effects.as_ref(),
            Some(&receipt.result.final_effects)
        );
        assert_eq!(
            reset_proof.final_active_instance_ids,
            receipt.result.final_active_instance_ids
        );
        assert_eq!(
            reset_proof
                .raw_wallet_claim
                .as_ref()
                .map(|claim| claim.claimed_amount),
            Some(receipt.result.raw_tokens)
        );
        assert_eq!(captured_data.unverified_planet_wallet_claims.len(), 1);
        assert_eq!(
            captured_data.unverified_planet_wallet_claims[0].previous_cycle_id,
            receipt.result.previous_cycle_id
        );
        assert_eq!(
            captured_data.unverified_planet_wallet_claims[0].claimed_amount,
            receipt.result.raw_tokens
        );
        assert_eq!(
            captured_data.unverified_planet_wallet_claims[0].created_at_utc,
            receipt.result.reset_at_utc
        );
        assert_eq!(
            reset_proof.raw_wallet_claim.as_ref(),
            Some(&captured_data.unverified_planet_wallet_claims[0])
        );
        assert!(captured_data.wallet_credits.is_empty());
        assert!(!captured_data.reset_receipts_unverifiable);
        assert!(
            captured_data.integrity_issues.is_empty(),
            "unexpected source integrity issues: {:?}",
            captured_data.integrity_issues
        );
    }

    #[test]
    fn v2_validator_rejects_bounds_that_disagree_with_provenance() {
        let ledger = prepared_first_reset_ledger();
        let Some((mut data, provenance)) = collect_v2_domain_source(&ledger.connection).unwrap()
        else {
            panic!("the actual first-reset provenance should be readable");
        };
        data.effect_cycle_bounds[0].cycle_id = "00000000-0000-4000-a000-000000000099".to_owned();

        let issues = shop_import::validate_local_integrity_for_guest_provenance(&data, &provenance);
        assert!(issues.iter().any(|issue| matches!(
            issue,
            crate::domain::cosmetic_shop::GuestShopImportIntegrityIssue::InvalidEffectTimeline {
                reason
            } if reason == "cycle_bounds_not_authoritative"
        )));
    }

    #[test]
    fn v2_source_rejects_receipt_cooldown_that_disagrees_with_local_reset() {
        let ledger = prepared_first_reset_ledger();
        let receipt_json: String = ledger
            .connection
            .query_row(
                "SELECT receipt_json FROM guest_provenance_reset_receipt",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let mut receipt: serde_json::Value = serde_json::from_str(&receipt_json).unwrap();
        let original_deadline = receipt["result"]["reset_available_at_utc"]
            .as_str()
            .unwrap();
        let altered_deadline = (parse_utc(original_deadline).unwrap() + Duration::seconds(1))
            .to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
        receipt["result"]["reset_available_at_utc"] = altered_deadline.into();
        ledger
            .connection
            .execute(
                "UPDATE guest_provenance_reset_receipt SET receipt_json=?1",
                [receipt.to_string()],
            )
            .unwrap();

        let Some((data, provenance)) = collect_v2_domain_source(&ledger.connection).unwrap() else {
            panic!(
                "the typed provenance remains parseable but must fail local receipt correlation"
            );
        };
        let proof = &data.reset_settlement_proofs[0];
        assert!(data.reset_receipts_unverifiable);
        assert_eq!(proof.final_effect_revision, None);
        assert_eq!(data.unverified_planet_wallet_claims.len(), 1);
        assert!(data.integrity_issues.iter().any(|issue| matches!(
            issue,
            crate::domain::cosmetic_shop::GuestShopImportIntegrityIssue::ResetProofUnverifiable {
                previous_cycle_id
            } if previous_cycle_id == &provenance.reset_receipt.result.previous_cycle_id
        )));
    }

    #[test]
    fn existing_v1_capture_is_not_upgraded() {
        let mut ledger = prepared_first_reset_ledger();
        let first = ledger.capture_guest_shop_import(TARGET_ACCOUNT).unwrap();
        let first_bytes = serde_json::to_vec(&first.snapshot).unwrap();
        let first_bytes_hash = sha2::Sha256::digest(&first_bytes);

        let PendingGuestShopImport::Legacy(restored) = ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap()
        else {
            panic!("an existing schema 1 capture must remain schema 1");
        };
        let restored_bytes = serde_json::to_vec(&restored.snapshot).unwrap();

        assert_eq!(restored_bytes, first_bytes);
        assert_eq!(sha2::Sha256::digest(&restored_bytes), first_bytes_hash);
        assert_eq!(
            restored.snapshot.source_fingerprint,
            first.snapshot.source_fingerprint
        );
        let v2_rows: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table'
                 AND name='guest_shop_import_v2_capture'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(v2_rows, 0);
    }

    #[test]
    fn captured_v2_freezes_reward_settlement_while_raw_usage_keeps_ingesting() {
        let mut ledger = prepared_first_reset_ledger();
        let captured = ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap();
        assert!(matches!(captured, PendingGuestShopImport::V2(_)));

        let mut appended = raw_one_date_record("v2-capture-append-3m2", Utc::now());
        appended.usage.total_tokens = Some(3_200_000);
        ledger.insert(&appended).unwrap();

        let raw_rows_before_settlement: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM usage_record WHERE event_key='v2-capture-append-3m2'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let contributions_before_settlement: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM shop_effect_contribution WHERE account_id='local'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let rewards_before_settlement: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM shop_game_reward WHERE account_id='local'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let cycle_id = ledger.planet_cycle_id().unwrap();
        let settlements_before: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM shop_cycle_settlement WHERE account_id='local'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        ledger.settle_guest_rewards(Utc::now()).unwrap();
        assert!(matches!(
            ledger.settle_guest_cycle_tokens(&cycle_id, Utc::now()),
            Err(ScanError::InvalidShopState)
        ));
        assert!(matches!(
            ledger.rebuild_shop_contributions(),
            Err(ScanError::InvalidShopState)
        ));

        let raw_rows_after_settlement: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM usage_record WHERE event_key='v2-capture-append-3m2'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let contributions_after_settlement: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM shop_effect_contribution WHERE account_id='local'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let rewards_after_settlement: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM shop_game_reward WHERE account_id='local'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let settlements_after: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM shop_cycle_settlement WHERE account_id='local'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(raw_rows_before_settlement, 1);
        assert_eq!(raw_rows_after_settlement, 1);
        assert_eq!(
            contributions_after_settlement, contributions_before_settlement,
            "a frozen capture must not rebuild mutable contribution state"
        );
        assert_eq!(
            rewards_after_settlement, rewards_before_settlement,
            "a frozen capture must not finalize new guest rewards"
        );
        assert_eq!(settlements_after, settlements_before);
    }

    #[test]
    fn captured_v2_blocks_planet_mutations_without_breaking_read_paths() {
        let mut ledger = prepared_first_reset_ledger();
        ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap();

        assert!(matches!(
            ledger.set_planet_profile(
                "Changed while pending",
                crate::domain::planet::PlanetAvatar::Masculine
            ),
            Err(ScanError::InvalidShopState)
        ));
        assert!(matches!(
            ledger.set_planet_timezone("America/New_York"),
            Err(ScanError::InvalidShopState)
        ));

        let objects_before = ledger.planet_objects().unwrap();
        ledger
            .ensure_planet_object(1, 0, "tree", 50, 50, 9)
            .unwrap();
        assert_eq!(ledger.planet_objects().unwrap(), objects_before);

        let cycle_id = ledger.planet_cycle_id().unwrap();
        assert!(matches!(
            ledger.reset_planet(Utc::now() + Duration::days(2)),
            Err(ScanError::InvalidShopState)
        ));
        assert_eq!(ledger.planet_cycle_id().unwrap(), cycle_id);
    }

    #[test]
    fn captured_v2_freezes_public_journal_preparation_but_not_raw_inserts() {
        let mut ledger = prepared_first_reset_ledger();
        ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap();
        let mut appended = raw_one_date_record("v2-journal-gate-append", Utc::now());
        appended.usage.total_tokens = Some(3_200_000);
        ledger.insert(&appended).unwrap();

        let entries_before: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM growth_journal_entry WHERE account_id='local'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        ledger.prepare_growth_journal().unwrap();
        let entries_after: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM growth_journal_entry WHERE account_id='local'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(entries_after, entries_before);
        assert_eq!(
            ledger
                .connection
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM usage_record WHERE event_key='v2-journal-gate-append'",
                    [],
                    |row| row.get(0),
                )
                .unwrap(),
            1,
            "raw usage remains durable while the game journal is frozen"
        );
    }

    #[test]
    fn captured_v2_rejects_guest_shop_requests_before_recording_them() {
        let mut ledger = prepared_first_reset_ledger();
        ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap();
        let quote = ledger
            .quote_shop(&crate::domain::cosmetic_shop::QuoteTarget::Purchase {
                sku: "land_pond".into(),
            })
            .unwrap();
        let request = crate::domain::cosmetic_shop::ShopRequest::Purchase {
            request_id: "blocked-v2-purchase".into(),
            quote,
        };
        let before: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM shop_action_request WHERE account_id='local'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert!(matches!(
            ledger.apply_guest_shop_request(&request, Utc::now()),
            Err(ScanError::InvalidShopState)
        ));
        let after: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM shop_action_request WHERE account_id='local'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(after, before);
    }

    #[test]
    fn v2_pending_capture_is_reported_to_the_existing_sync_preflight() {
        let mut ledger = prepared_first_reset_ledger();
        ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap();

        assert!(!ledger.guest_import_game_mutations_allowed().unwrap());
        assert!(ledger.has_local_guest_shop_state().unwrap());
        assert!(ledger.has_unimported_guest_shop_state().unwrap());
        assert!(ledger
            .planet_wallet_credits_for_server_upload()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn captured_v2_blocks_direct_planet_account_transfer() {
        let mut ledger = prepared_first_reset_ledger();
        ledger
            .capture_guest_shop_import_request(TARGET_ACCOUNT)
            .unwrap();

        assert!(matches!(
            ledger.ensure_planet_account("00000000-0000-4000-a000-000000000071"),
            Err(ScanError::InvalidShopState)
        ));
        assert_eq!(ledger.cosmetic_account_id().unwrap(), "local");
        assert_eq!(
            ledger
                .connection
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM guest_shop_import_v2_capture WHERE target_account_id=?1",
                    [TARGET_ACCOUNT],
                    |row| row.get(0),
                )
                .unwrap(),
            1
        );
    }
}
