use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row, TransactionBehavior};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::domain::cosmetic_shop::{
    ActiveEffects, GuestActivityDay, GuestAvatarEquipment, GuestAvatarOwned,
    GuestCosmeticEquipment, GuestCycleSettlement, GuestDailyAgentTotal, GuestEffectContribution,
    GuestEffectTimelineState, GuestEraProgress, GuestGameReward, GuestGrowthJournalCycle,
    GuestGrowthJournalEntry, GuestGrowthJournalState, GuestLandscapeEditVersion,
    GuestLandscapeInstance, GuestLandscapePlacement, GuestNaturalObject, GuestNaturalRemoval,
    GuestPendingPurchase, GuestPlanetProfile, GuestPurchaseOwnershipProof, GuestPurchaseProof,
    GuestRemovalDebit, GuestRemovalProof, GuestResetSettlementProof, GuestShopCycle,
    GuestShopImportData, GuestShopImportDisposition, GuestShopImportIntegrityIssue,
    GuestShopImportSnapshot, GuestShopImportStatus, GuestShopPurchase, GuestShopWalletCredit,
    GuestUnverifiedWalletClaim, GuestUsageAggregate,
};
use crate::domain::cosmetic_shop::{ShopActionResult, ShopActionStatus, ShopCategory, ShopRequest};
use crate::domain::landscape_geometry::{terrain_bounds, validate_placement, LandscapePoint};
use crate::domain::planet::PlanetObject;
use crate::storage::ledger::{Ledger, ScanError};

#[derive(Serialize)]
struct FingerprintInput<'a> {
    source_account_id: &'a str,
    data: &'a GuestShopImportData,
}

impl Ledger {
    /// Captures local guest data durably for one target account. It never switches ownership
    /// and does not make the snapshot eligible for server credit or import.
    pub fn capture_guest_shop_import(
        &mut self,
        target_account_id: &str,
    ) -> Result<GuestShopImportStatus, ScanError> {
        validate_target_account(target_account_id)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let account_id = active_account_id(&tx)?;
        if account_id != "local" {
            return Err(ScanError::InvalidShopState);
        }

        let current_data = capture_data(&tx)?;
        let current_fingerprint = fingerprint(&account_id, &current_data)?;
        let existing: Option<(String, String, String)> = tx
            .query_row(
                "SELECT import_id,source_fingerprint,snapshot_json
                 FROM guest_shop_import_capture WHERE target_account_id=?1",
                [target_account_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;

        if let Some((import_id, stored_fingerprint, snapshot_json)) = existing {
            let snapshot: GuestShopImportSnapshot =
                serde_json::from_str(&snapshot_json).map_err(|_| ScanError::Database)?;
            validate_snapshot_payload(
                &snapshot,
                &import_id,
                target_account_id,
                &account_id,
                &stored_fingerprint,
            )?;
            tx.commit()?;
            return Ok(GuestShopImportStatus {
                snapshot,
                source_matches_current: stored_fingerprint == current_fingerprint,
            });
        }

        let snapshot = GuestShopImportSnapshot {
            import_id: uuid::Uuid::new_v4().to_string(),
            target_account_id: target_account_id.to_owned(),
            source_account_id: account_id.clone(),
            source_fingerprint: current_fingerprint,
            disposition: disposition(&current_data),
            data: current_data,
        };
        let snapshot_json = serde_json::to_string(&snapshot).map_err(|_| ScanError::Database)?;
        let captured_at_utc = Utc::now().to_rfc3339();
        tx.execute(
            "INSERT INTO guest_shop_import_capture(
                target_account_id,import_id,source_fingerprint,snapshot_json,captured_at_utc
             ) VALUES (?1,?2,?3,?4,?5)",
            params![
                target_account_id,
                snapshot.import_id,
                snapshot.source_fingerprint,
                snapshot_json,
                captured_at_utc,
            ],
        )?;
        tx.commit()?;
        Ok(GuestShopImportStatus {
            snapshot,
            source_matches_current: true,
        })
    }

    /// Reads a previously captured payload without creating or changing one.
    pub fn pending_guest_shop_import(
        &self,
        target_account_id: &str,
    ) -> Result<Option<GuestShopImportStatus>, ScanError> {
        validate_target_account(target_account_id)?;
        let tx = self.connection.unchecked_transaction()?;
        let pending: Option<(String, String, String)> = tx
            .query_row(
                "SELECT import_id,source_fingerprint,snapshot_json
                 FROM guest_shop_import_capture WHERE target_account_id=?1",
                [target_account_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((import_id, stored_fingerprint, snapshot_json)) = pending else {
            tx.commit()?;
            return Ok(None);
        };

        let snapshot: GuestShopImportSnapshot =
            serde_json::from_str(&snapshot_json).map_err(|_| ScanError::Database)?;
        validate_snapshot_payload(
            &snapshot,
            &import_id,
            target_account_id,
            "local",
            &stored_fingerprint,
        )?;
        let source_matches_current = if active_account_id(&tx)? == "local" {
            let current = capture_data(&tx)?;
            fingerprint("local", &current)? == stored_fingerprint
        } else {
            false
        };
        tx.commit()?;
        Ok(Some(GuestShopImportStatus {
            snapshot,
            source_matches_current,
        }))
    }
}

fn validate_target_account(target_account_id: &str) -> Result<(), ScanError> {
    let raw = target_account_id
        .strip_prefix("account:")
        .ok_or(ScanError::InvalidShopState)?;
    let parsed = uuid::Uuid::parse_str(raw).map_err(|_| ScanError::InvalidShopState)?;
    if parsed.to_string() != raw {
        return Err(ScanError::InvalidShopState);
    }
    Ok(())
}

fn active_account_id(connection: &Connection) -> Result<String, ScanError> {
    connection
        .query_row(
            "SELECT value FROM setting WHERE key='planet_account_id'",
            [],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

fn setting(connection: &Connection, key: &str) -> Result<Option<String>, ScanError> {
    connection
        .query_row("SELECT value FROM setting WHERE key=?1", [key], |row| {
            row.get(0)
        })
        .optional()
        .map_err(Into::into)
}

fn required_setting(connection: &Connection, key: &str) -> Result<String, ScanError> {
    setting(connection, key)?.ok_or(ScanError::Database)
}

fn to_u64(value: i64) -> Result<u64, rusqlite::Error> {
    u64::try_from(value).map_err(|_| rusqlite::Error::InvalidQuery)
}

fn to_u32(value: i64) -> Result<u32, rusqlite::Error> {
    u32::try_from(value).map_err(|_| rusqlite::Error::InvalidQuery)
}

fn to_u8(value: i64) -> Result<u8, rusqlite::Error> {
    u8::try_from(value).map_err(|_| rusqlite::Error::InvalidQuery)
}

fn query_rows<T, F>(
    connection: &Connection,
    sql: &str,
    account_id: &str,
    mapper: F,
) -> Result<Vec<T>, ScanError>
where
    F: FnMut(&Row<'_>) -> rusqlite::Result<T>,
{
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map([account_id], mapper)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn query_all<T, F>(connection: &Connection, sql: &str, mapper: F) -> Result<Vec<T>, ScanError>
where
    F: FnMut(&Row<'_>) -> rusqlite::Result<T>,
{
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map([], mapper)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn fingerprint(source_account_id: &str, data: &GuestShopImportData) -> Result<String, ScanError> {
    let bytes = serde_json::to_vec(&FingerprintInput {
        source_account_id,
        data,
    })
    .map_err(|_| ScanError::Database)?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn validate_snapshot_payload(
    snapshot: &GuestShopImportSnapshot,
    import_id: &str,
    target_account_id: &str,
    source_account_id: &str,
    stored_fingerprint: &str,
) -> Result<(), ScanError> {
    if snapshot.import_id != import_id
        || snapshot.target_account_id != target_account_id
        || snapshot.source_account_id != source_account_id
        || snapshot.source_fingerprint != stored_fingerprint
    {
        return Err(ScanError::InvalidShopState);
    }
    if fingerprint(source_account_id, &snapshot.data)? != stored_fingerprint {
        return Err(ScanError::InvalidShopState);
    }
    let actual_issues = validate_local_integrity(&snapshot.data);
    if snapshot.data.integrity_issues != actual_issues
        || snapshot.disposition != disposition(&snapshot.data)
    {
        return Err(ScanError::InvalidShopState);
    }
    Ok(())
}

fn disposition(data: &GuestShopImportData) -> GuestShopImportDisposition {
    if !data.unverified_planet_wallet_claims.is_empty()
        || data.reset_receipts_unverifiable
        || data.legacy_partial_import_pending
        || !data.integrity_issues.is_empty()
        || (!data.effect_history.is_empty() && !data.effect_cycle_bounds_authoritative)
        || data
            .daily_agent_totals
            .iter()
            .any(|aggregate| aggregate.total_tokens.is_none() || aggregate.coverage != "complete")
        || data
            .usage_aggregates
            .iter()
            .any(|aggregate| aggregate.cycle_id.is_none())
    {
        GuestShopImportDisposition::SourceUnverifiable
    } else {
        GuestShopImportDisposition::LocalIntegrityValidated
    }
}

fn capture_data(connection: &Connection) -> Result<GuestShopImportData, ScanError> {
    if active_account_id(connection)? != "local" {
        return Err(ScanError::InvalidShopState);
    }
    let current_cycle_id = required_setting(connection, "planet_current_cycle_id")?;
    let current_cycle_started_at_utc = required_setting(connection, "planet_cycle_started_at_utc")?;
    let planet_last_reset_at_utc = setting(connection, "planet_last_reset_at_utc")?;
    let reset_available_at_utc = setting(connection, "planet_reset_available_at_utc")?;
    let world_timezone = required_setting(connection, "world_timezone")?;
    let planet_timezone = required_setting(connection, "planet_timezone")?;
    let planet_device_id = required_setting(connection, "planet_device_id")?;
    let reward_timezone: Option<String> = connection
        .query_row(
            "SELECT reward_timezone FROM shop_account_state WHERE account_id='local'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let shop_state_revision: Option<i64> = connection
        .query_row(
            "SELECT state_revision FROM shop_account_state WHERE account_id='local'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let (reward_timezone, shop_state_revision) = match (reward_timezone, shop_state_revision) {
        (Some(timezone), Some(revision)) => (
            timezone,
            u64::try_from(revision).map_err(|_| ScanError::InvalidCount)?,
        ),
        _ => return Err(ScanError::InvalidShopState),
    };
    let effect_timeline_state = connection
        .query_row(
            "SELECT current_cycle_id,effect_revision,server_time_utc,reward_timezone
             FROM shop_effect_timeline_state WHERE account_id='local'",
            [],
            |row| {
                Ok(GuestEffectTimelineState {
                    current_cycle_id: row.get(0)?,
                    effect_revision: to_u64(row.get(1)?)?,
                    server_time_utc: row.get(2)?,
                    reward_timezone: row.get(3)?,
                })
            },
        )
        .optional()?;
    let effect_cycle_bounds_authoritative: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM shop_effect_cycle_bounds_state WHERE account_id='local')",
        [],
        |row| row.get(0),
    )?;
    let contribution_canonical_version: Option<u64> = connection
        .query_row(
            "SELECT canonical_version FROM shop_contribution_state WHERE account_id='local'",
            [],
            |row| to_u64(row.get(0)?),
        )
        .optional()?;
    let profile = match (
        setting(connection, "planet_nickname")?,
        setting(connection, "planet_avatar")?,
    ) {
        (Some(nickname), Some(avatar)) => {
            let avatar = match avatar.as_str() {
                "masculine" => crate::domain::planet::PlanetAvatar::Masculine,
                "feminine" => crate::domain::planet::PlanetAvatar::Feminine,
                _ => return Err(ScanError::InvalidProfile),
            };
            Some(GuestPlanetProfile { nickname, avatar })
        }
        (None, None) => None,
        _ => return Err(ScanError::InvalidProfile),
    };

    let growth_journal_state = connection
        .query_row(
            "SELECT generation,deleted_at_utc FROM growth_journal_state WHERE account_id='local'",
            [],
            |row| Ok((to_u64(row.get(0)?)?, row.get::<_, Option<String>>(1)?)),
        )
        .optional()?
        .map(|(generation, deleted_at_utc)| GuestGrowthJournalState {
            generation,
            deleted_at_utc,
        });
    let growth_journal_cycles = query_rows(
        connection,
        "SELECT cycle_id,started_at_utc,ended_at_utc,wallet_credit,wallet_credit_at_utc
         FROM growth_journal_cycle WHERE account_id=?1 ORDER BY cycle_id",
        "local",
        |row| {
            Ok(GuestGrowthJournalCycle {
                cycle_id: row.get(0)?,
                started_at_utc: row.get(1)?,
                ended_at_utc: row.get(2)?,
                wallet_credit: row.get::<_, Option<i64>>(3)?.map(to_u64).transpose()?,
                wallet_credit_at_utc: row.get(4)?,
            })
        },
    )?;
    let growth_journal_entries = query_rows(
        connection,
        "SELECT device_id,cycle_id,bucket_date,agent,revision,acknowledged_revision,
                generation,present,confirmed_tokens,coverage,payload_hash
         FROM growth_journal_entry WHERE account_id=?1
         ORDER BY cycle_id,bucket_date,agent,device_id",
        "local",
        |row| {
            Ok(GuestGrowthJournalEntry {
                device_id: row.get(0)?,
                cycle_id: row.get(1)?,
                bucket_date: row.get(2)?,
                agent: row.get(3)?,
                revision: to_u64(row.get(4)?)?,
                acknowledged_revision: to_u64(row.get(5)?)?,
                generation: to_u64(row.get(6)?)?,
                present: row.get::<_, i64>(7)? != 0,
                confirmed_tokens: row.get::<_, Option<i64>>(8)?.map(to_u64).transpose()?,
                coverage: row.get(9)?,
                payload_hash: row.get(10)?,
            })
        },
    )?;

    let effect_history = query_rows(
        connection,
        "SELECT cycle_id,revision,started_at_utc,ended_at_utc,active_instance_ids_json,effects_json
         FROM shop_effect_history WHERE account_id=?1 ORDER BY cycle_id,revision",
        "local",
        |row| {
            let instance_ids: String = row.get(4)?;
            let effects: String = row.get(5)?;
            Ok(crate::domain::cosmetic_shop::ShopEffectInterval {
                cycle_id: row.get(0)?,
                revision: to_u64(row.get(1)?)?,
                started_at_utc: row.get(2)?,
                ended_at_utc: row.get(3)?,
                active_instance_ids: serde_json::from_str(&instance_ids)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                effects: serde_json::from_str::<ActiveEffects>(&effects)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
            })
        },
    )?;
    let effect_cycle_bounds = query_rows(
        connection,
        "SELECT cycle_id,started_at_utc,ended_at_utc FROM shop_effect_cycle_bound
         WHERE account_id=?1 ORDER BY cycle_id",
        "local",
        |row| {
            Ok(crate::domain::cosmetic_shop::ShopCycleBound {
                cycle_id: row.get(0)?,
                started_at_utc: row.get(1)?,
                ended_at_utc: row.get(2)?,
            })
        },
    )?;

    let landscape_instances = query_rows(
        connection,
        "SELECT instance_id,sku,variation_index,seed,variation_version,acquired_at_utc
         FROM shop_landscape_instance WHERE account_id=?1 ORDER BY instance_id",
        "local",
        |row| {
            Ok(GuestLandscapeInstance {
                instance_id: row.get(0)?,
                sku: row.get(1)?,
                variation_index: to_u8(row.get(2)?)?,
                seed: row.get(3)?,
                variation_version: to_u32(row.get(4)?)?,
                acquired_at_utc: row.get(5)?,
            })
        },
    )?;
    let placements = query_rows(
        connection,
        "SELECT instance_id,cycle_id,x,y,version FROM shop_landscape_placement
         WHERE account_id=?1 ORDER BY instance_id",
        "local",
        |row| {
            Ok(GuestLandscapePlacement {
                instance_id: row.get(0)?,
                cycle_id: row.get(1)?,
                x: row.get(2)?,
                y: row.get(3)?,
                version: to_u64(row.get(4)?)?,
            })
        },
    )?;
    if placements
        .iter()
        .any(|placement| !placement.x.is_finite() || !placement.y.is_finite())
    {
        return Err(ScanError::InvalidShopState);
    }
    let landscape_edit_versions = query_rows(
        connection,
        "SELECT instance_id,version FROM shop_landscape_edit_version
         WHERE account_id=?1 ORDER BY instance_id",
        "local",
        |row| {
            Ok(GuestLandscapeEditVersion {
                instance_id: row.get(0)?,
                version: to_u64(row.get(1)?)?,
            })
        },
    )?;
    let avatar_owned = query_rows(
        connection,
        "SELECT sku,purchase_id,price,acquired_at_utc FROM shop_avatar_owned
         WHERE account_id=?1 ORDER BY sku",
        "local",
        |row| {
            Ok(GuestAvatarOwned {
                sku: row.get(0)?,
                purchase_id: row.get(1)?,
                price: to_u64(row.get(2)?)?,
                acquired_at_utc: row.get(3)?,
            })
        },
    )?;
    let avatar_equipment = query_rows(
        connection,
        "SELECT slot,sku,version FROM shop_avatar_equipment
         WHERE account_id=?1 ORDER BY slot",
        "local",
        |row| {
            Ok(GuestAvatarEquipment {
                slot: row.get(0)?,
                sku: row.get(1)?,
                version: to_u64(row.get(2)?)?,
            })
        },
    )?;
    let cosmetic_equipment = query_rows(
        connection,
        "SELECT cycle_id,slot_id,sku,version FROM cosmetic_equipment
         WHERE account_id=?1 ORDER BY cycle_id,slot_id",
        "local",
        |row| {
            Ok(GuestCosmeticEquipment {
                cycle_id: row.get(0)?,
                slot_id: row.get(1)?,
                sku: row.get(2)?,
                version: to_u64(row.get(3)?)?,
            })
        },
    )?;
    let pending_purchases = query_rows(
        connection,
        "SELECT sku,purchase_id,catalog_revision,created_at_utc
         FROM cosmetic_pending_purchase WHERE account_id=?1 ORDER BY sku",
        "local",
        |row| {
            Ok(GuestPendingPurchase {
                sku: row.get(0)?,
                purchase_id: row.get(1)?,
                catalog_revision: to_u32(row.get(2)?)?,
                created_at_utc: row.get(3)?,
            })
        },
    )?;
    let cosmetic_purchases = query_rows(
        connection,
        "SELECT purchase_id,sku,price,purchased_at_utc FROM cosmetic_purchase
         WHERE account_id=?1 ORDER BY purchased_at_utc,purchase_id",
        "local",
        |row| {
            Ok(GuestShopPurchase {
                purchase_id: row.get(0)?,
                sku: row.get(1)?,
                price: to_u64(row.get(2)?)?,
                purchased_at_utc: row.get(3)?,
            })
        },
    )?;
    let purchases = query_rows(
        connection,
        "SELECT purchase_id,sku,price,purchased_at_utc FROM shop_purchase
         WHERE account_id=?1 ORDER BY purchased_at_utc,purchase_id",
        "local",
        |row| {
            Ok(GuestShopPurchase {
                purchase_id: row.get(0)?,
                sku: row.get(1)?,
                price: to_u64(row.get(2)?)?,
                purchased_at_utc: row.get(3)?,
            })
        },
    )?;
    let purchase_proofs =
        purchase_proofs(connection, &purchases, &landscape_instances, &avatar_owned)?;
    let natural_removals = query_rows(
        connection,
        "SELECT cycle_id,stage,ordinal,version,removed_at_utc FROM shop_natural_removal
         WHERE account_id=?1 ORDER BY cycle_id,stage,ordinal",
        "local",
        |row| {
            Ok(GuestNaturalRemoval {
                cycle_id: row.get(0)?,
                stage: to_u8(row.get(1)?)?,
                ordinal: to_u32(row.get(2)?)?,
                version: to_u64(row.get(3)?)?,
                removed_at_utc: row.get(4)?,
            })
        },
    )?;
    let removal_debits = query_rows(
        connection,
        "SELECT request_id,amount,created_at_utc FROM shop_natural_removal_debit
         WHERE account_id=?1 ORDER BY created_at_utc,request_id",
        "local",
        |row| {
            Ok(GuestRemovalDebit {
                request_id: row.get(0)?,
                amount: to_u64(row.get(1)?)?,
                created_at_utc: row.get(2)?,
            })
        },
    )?;
    let removal_proofs = removal_proofs(connection, &natural_removals, &removal_debits)?;
    let effect_contributions = query_rows(
        connection,
        "SELECT device_id,cycle_id,date,effect_revision,canonical_version,tokens,growth_bps,wallet_bps
         FROM shop_effect_contribution WHERE account_id=?1
         ORDER BY cycle_id,date,device_id,effect_revision",
        "local",
        |row| {
            Ok(GuestEffectContribution {
                device_id: row.get(0)?,
                cycle_id: row.get(1)?,
                date: row.get(2)?,
                effect_revision: to_u64(row.get(3)?)?,
                canonical_version: to_u64(row.get(4)?)?,
                tokens: to_u64(row.get(5)?)?,
                growth_bps: to_u32(row.get(6)?)? as u16,
                wallet_bps: to_u32(row.get(7)?)? as u16,
            })
        },
    )?;
    let activity_days = query_rows(
        connection,
        "SELECT reward_date,cycle_id,first_occurred_at_utc,canonical_version,tokens
         FROM shop_activity_day WHERE account_id=?1 ORDER BY reward_date",
        "local",
        |row| {
            Ok(GuestActivityDay {
                reward_date: row.get(0)?,
                cycle_id: row.get(1)?,
                first_occurred_at_utc: row.get(2)?,
                canonical_version: to_u64(row.get(3)?)?,
                tokens: to_u64(row.get(4)?)?,
            })
        },
    )?;
    let game_rewards = query_rows(
        connection,
        "SELECT reward_id,trigger_key,kind,cycle_id,amount,effect_snapshot_json,awarded_at_utc
         FROM shop_game_reward WHERE account_id=?1 ORDER BY awarded_at_utc,reward_id",
        "local",
        |row| {
            let effects: String = row.get(5)?;
            Ok(GuestGameReward {
                reward_id: row.get(0)?,
                trigger_key: row.get(1)?,
                kind: row.get(2)?,
                cycle_id: row.get(3)?,
                amount: to_u64(row.get(4)?)?,
                effects: serde_json::from_str(&effects)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                awarded_at_utc: row.get(6)?,
            })
        },
    )?;
    let wallet_credits = query_rows(
        connection,
        "SELECT credit_id,trigger_key,cycle_id,amount,created_at_utc FROM shop_wallet_credit
         WHERE account_id=?1 ORDER BY created_at_utc,credit_id",
        "local",
        |row| {
            Ok(GuestShopWalletCredit {
                credit_id: row.get(0)?,
                trigger_key: row.get(1)?,
                cycle_id: row.get(2)?,
                amount: to_u64(row.get(3)?)?,
                created_at_utc: row.get(4)?,
            })
        },
    )?;
    let unverified_planet_wallet_claims = query_all(
        connection,
        "SELECT previous_cycle_id,amount,created_at_utc FROM planet_wallet_credit
         ORDER BY previous_cycle_id",
        |row| {
            Ok(GuestUnverifiedWalletClaim {
                previous_cycle_id: row.get(0)?,
                claimed_amount: to_u64(row.get(1)?)?,
                created_at_utc: row.get(2)?,
            })
        },
    )?;
    let cycle_settlements = query_rows(
        connection,
        "SELECT cycle_id,amount,settled_at_utc FROM shop_cycle_settlement
         WHERE account_id=?1 ORDER BY cycle_id",
        "local",
        |row| {
            Ok(GuestCycleSettlement {
                cycle_id: row.get(0)?,
                amount: to_u64(row.get(1)?)?,
                settled_at_utc: row.get(2)?,
            })
        },
    )?;
    let era_progress = query_rows(
        connection,
        "SELECT cycle_id,stage,trigger_key,effect_snapshot_json,awarded_at_utc
         FROM shop_era_progress WHERE account_id=?1 ORDER BY cycle_id,stage",
        "local",
        |row| {
            let effects: String = row.get(3)?;
            Ok(GuestEraProgress {
                cycle_id: row.get(0)?,
                stage: to_u8(row.get(1)?)?,
                trigger_key: row.get(2)?,
                effects: serde_json::from_str(&effects)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                awarded_at_utc: row.get(4)?,
            })
        },
    )?;

    // Only aggregate columns are selected. Event keys, source paths, timestamps and raw log
    // content never enter the durable import payload. Ownership filtering prevents usage owned
    // by a different signed account from leaking into a guest capture.
    let aggregate_rows = query_all(
        connection,
        "SELECT r.bucket_date,r.agent,count(*),sum(r.total_tokens),
                CASE WHEN min(r.coverage)=max(r.coverage) THEN min(r.coverage) ELSE 'mixed' END
         FROM usage_record r JOIN planet_usage_owner owner ON owner.event_key=r.event_key
         WHERE owner.account_id='local' AND (r.kind='response' OR NOT EXISTS (
           SELECT 1 FROM usage_record other
           JOIN planet_usage_owner other_owner ON other_owner.event_key=other.event_key
           WHERE other.source_id=r.source_id AND other.kind='response' AND other.agent=r.agent
             AND other_owner.account_id='local'
         ))
         GROUP BY r.bucket_date,r.agent ORDER BY r.bucket_date,r.agent",
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                to_u64(row.get(2)?)?,
                row.get::<_, Option<i64>>(3)?.map(to_u64).transpose()?,
                row.get::<_, String>(4)?,
            ))
        },
    )?;
    let mut cycles_for_usage: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    for entry in &growth_journal_entries {
        if entry.present {
            cycles_for_usage
                .entry((entry.bucket_date.clone(), entry.agent.clone()))
                .or_default()
                .insert(entry.cycle_id.clone());
        }
    }
    let usage_aggregates = aggregate_rows
        .into_iter()
        .map(
            |(bucket_date, agent, event_count, total_tokens, coverage)| {
                let matching_cycles = cycles_for_usage.get(&(bucket_date.clone(), agent.clone()));
                let cycle_id = matching_cycles
                    .filter(|cycles| cycles.len() == 1)
                    .and_then(|cycles| cycles.iter().next().cloned());
                GuestUsageAggregate {
                    cycle_id,
                    bucket_date,
                    agent,
                    event_count,
                    total_tokens,
                    coverage,
                }
            },
        )
        .collect::<Vec<_>>();
    let daily_agent_totals = usage_aggregates
        .iter()
        .map(|aggregate| GuestDailyAgentTotal {
            bucket_date: aggregate.bucket_date.clone(),
            agent: aggregate.agent.clone(),
            total_tokens: aggregate.total_tokens,
            coverage: aggregate.coverage.clone(),
        })
        .collect::<Vec<_>>();
    let lifetime_usage_tokens = usage_aggregates.iter().try_fold(0_u64, |total, aggregate| {
        total.checked_add(aggregate.total_tokens?)
    });
    let current_cycle_usage_tokens = if usage_aggregates
        .iter()
        .any(|aggregate| aggregate.cycle_id.is_none() || aggregate.total_tokens.is_none())
    {
        None
    } else {
        usage_aggregates
            .iter()
            .filter(|aggregate| aggregate.cycle_id.as_deref() == Some(&current_cycle_id))
            .try_fold(0_u64, |total, aggregate| {
                total.checked_add(aggregate.total_tokens?)
            })
    };
    let mut cycle_totals = BTreeMap::<String, u64>::new();
    for entry in &growth_journal_entries {
        if !entry.present {
            continue;
        }
        if let Some(tokens) = entry.confirmed_tokens {
            let sum = cycle_totals.entry(entry.cycle_id.clone()).or_default();
            *sum = sum.checked_add(tokens).ok_or(ScanError::InvalidCount)?;
        }
    }
    let cycle_usage_totals = cycle_totals
        .into_iter()
        .map(
            |(cycle_id, total_tokens)| crate::domain::cosmetic_shop::GuestCycleUsageTotal {
                cycle_id,
                total_tokens,
            },
        )
        .collect::<Vec<_>>();

    let natural_objects = query_all(
        connection,
        "SELECT cycle_id,stage,ordinal,kind,x,y,seed FROM planet_object
         ORDER BY cycle_id,stage,ordinal",
        |row| {
            let seed: String = row.get(6)?;
            Ok(GuestNaturalObject {
                cycle_id: row.get(0)?,
                stage: to_u8(row.get(1)?)?,
                ordinal: to_u32(row.get(2)?)?,
                kind: row.get(3)?,
                x: to_u8(row.get(4)?)?,
                y: to_u8(row.get(5)?)?,
                seed: seed.parse().map_err(|_| rusqlite::Error::InvalidQuery)?,
            })
        },
    )?;

    let mut cycle_map = BTreeMap::<String, GuestShopCycle>::new();
    let mut merge_cycle =
        |cycle_id: String, started: Option<String>, ended: Option<String>, settled: Option<u64>| {
            let cycle = cycle_map.entry(cycle_id.clone()).or_insert(GuestShopCycle {
                cycle_id,
                started_at_utc: None,
                ended_at_utc: None,
                is_current: false,
                settled_bonus_tokens: None,
            });
            if cycle.started_at_utc.is_none() {
                cycle.started_at_utc = started;
            }
            if ended.is_some() {
                cycle.ended_at_utc = ended;
            }
            if settled.is_some() {
                cycle.settled_bonus_tokens = settled;
            }
        };
    for cycle in &growth_journal_cycles {
        merge_cycle(
            cycle.cycle_id.clone(),
            cycle.started_at_utc.clone(),
            cycle.ended_at_utc.clone(),
            cycle.wallet_credit,
        );
    }
    for bound in &effect_cycle_bounds {
        merge_cycle(
            bound.cycle_id.clone(),
            Some(bound.started_at_utc.clone()),
            bound.ended_at_utc.clone(),
            None,
        );
    }
    for settlement in &cycle_settlements {
        merge_cycle(
            settlement.cycle_id.clone(),
            None,
            Some(settlement.settled_at_utc.clone()),
            Some(settlement.amount),
        );
    }
    for cycle_id in effect_contributions
        .iter()
        .map(|row| row.cycle_id.as_str())
        .chain(activity_days.iter().map(|row| row.cycle_id.as_str()))
        .chain(natural_removals.iter().map(|row| row.cycle_id.as_str()))
        .chain(natural_objects.iter().map(|row| row.cycle_id.as_str()))
    {
        merge_cycle(cycle_id.to_owned(), None, None, None);
    }
    for cycle in cycle_map.values_mut() {
        cycle.is_current = cycle.cycle_id == current_cycle_id;
    }
    let current_cycle = GuestShopCycle {
        cycle_id: current_cycle_id.clone(),
        started_at_utc: Some(current_cycle_started_at_utc),
        ended_at_utc: None,
        is_current: true,
        settled_bonus_tokens: cycle_settlements
            .iter()
            .find(|settlement| settlement.cycle_id == current_cycle_id)
            .map(|settlement| settlement.amount),
    };
    cycle_map.remove(&current_cycle_id);
    let historical_cycles = cycle_map.into_values().collect::<Vec<_>>();

    let (reset_settlement_proofs, reset_receipts_unverifiable) = reset_proofs(
        connection,
        &cycle_settlements,
        &unverified_planet_wallet_claims,
        &effect_history,
        &growth_journal_cycles,
        &historical_cycles,
        &current_cycle,
        reset_available_at_utc.as_deref(),
    )?;
    let legacy_partial_import_pending: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM cosmetic_guest_import
         WHERE account_id='local' AND imported_at_utc IS NULL)",
        [],
        |row| row.get(0),
    )?;

    let mut data = GuestShopImportData {
        world_timezone,
        planet_timezone,
        reward_timezone,
        planet_device_id,
        shop_state_revision,
        profile,
        activation_at_utc: required_setting(connection, "planet_activation_at_utc")?,
        current_cycle,
        historical_cycles,
        last_reset_at_utc: planet_last_reset_at_utc,
        reset_available_at_utc,
        effect_timeline_state,
        effect_cycle_bounds_authoritative,
        contribution_canonical_version,
        natural_objects,
        landscape_instances,
        placements,
        landscape_edit_versions,
        avatar_owned,
        avatar_equipment,
        cosmetic_equipment,
        pending_purchases,
        cosmetic_purchases,
        purchases,
        purchase_proofs,
        natural_removals,
        removal_debits,
        removal_proofs,
        effect_history,
        effect_cycle_bounds,
        effect_contributions,
        activity_days,
        game_rewards,
        wallet_credits,
        unverified_planet_wallet_claims,
        cycle_settlements,
        era_progress,
        daily_agent_totals,
        usage_aggregates,
        lifetime_usage_tokens,
        current_cycle_usage_tokens,
        cycle_usage_totals,
        growth_journal_state,
        growth_journal_cycles,
        growth_journal_entries,
        reset_settlement_proofs,
        integrity_issues: Vec::new(),
        reset_receipts_unverifiable,
        legacy_partial_import_pending,
    };
    data.integrity_issues = validate_local_integrity(&data);
    Ok(data)
}

fn purchase_proofs(
    connection: &Connection,
    purchases: &[GuestShopPurchase],
    landscape_instances: &[GuestLandscapeInstance],
    avatar_owned: &[GuestAvatarOwned],
) -> Result<Vec<GuestPurchaseProof>, ScanError> {
    use crate::domain::cosmetic_shop::{
        shop_products, GuestPurchaseOwnershipProof::*, QuoteTarget,
    };

    let receipts = query_rows(
        connection,
        "SELECT request_id,payload_json,result_json FROM shop_action_request
         WHERE account_id=?1 ORDER BY request_id",
        "local",
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        },
    )?
    .into_iter()
    .map(|(request_id, payload, result)| (request_id, (payload, result)))
    .collect::<BTreeMap<_, _>>();
    let products = shop_products();
    let mut proofs_by_id = BTreeMap::<String, GuestPurchaseProof>::new();
    let mut landscape_receipts = BTreeMap::<
        String,
        Vec<(
            String,
            crate::domain::cosmetic_shop::ShopQuote,
            Vec<crate::domain::cosmetic_shop::LandscapeInstance>,
        )>,
    >::new();

    for purchase in purchases {
        let Some((payload_json, result_json)) = receipts.get(&purchase.purchase_id) else {
            continue;
        };
        let Ok(ShopRequest::Purchase { request_id, quote }) =
            serde_json::from_str::<ShopRequest>(payload_json)
        else {
            continue;
        };
        if request_id != purchase.purchase_id || quote.price != purchase.price {
            continue;
        }
        let QuoteTarget::Purchase { sku } = quote.target.clone() else {
            continue;
        };
        if sku != purchase.sku {
            continue;
        }
        let Some(result_json) = result_json else {
            continue;
        };
        let Ok(result) = serde_json::from_str::<ShopActionResult>(result_json) else {
            continue;
        };
        if result.request_id != request_id
            || result.status != ShopActionStatus::Purchased
            || result.confirmed_quote.as_ref() != Some(&quote)
            || result.state.account_id != "local"
        {
            continue;
        }
        let Some(product) = products.iter().find(|product| product.sku == sku) else {
            continue;
        };
        match product.category {
            ShopCategory::Avatar => {
                let directly_owned = avatar_owned.iter().any(|owned| {
                    owned.purchase_id == purchase.purchase_id
                        && owned.sku == sku
                        && owned.price == purchase.price
                });
                if directly_owned
                    && result
                        .state
                        .avatar_owned_skus
                        .iter()
                        .any(|owned| owned == &sku)
                {
                    proofs_by_id.insert(
                        purchase.purchase_id.clone(),
                        GuestPurchaseProof {
                            request_id,
                            quote,
                            status: ShopActionStatus::Purchased,
                            ownership: Avatar { sku: sku.clone() },
                        },
                    );
                }
            }
            ShopCategory::Landscape => {
                let instances = result
                    .state
                    .landscape_instances
                    .into_iter()
                    .filter(|instance| instance.sku == sku)
                    .collect();
                landscape_receipts.entry(sku.clone()).or_default().push((
                    purchase.purchase_id.clone(),
                    quote,
                    instances,
                ));
            }
        }
    }

    for (sku, mut receipts) in landscape_receipts {
        let purchase_count = purchases
            .iter()
            .filter(|purchase| purchase.sku == sku)
            .count();
        let final_owned = landscape_instances
            .iter()
            .filter(|instance| instance.sku == sku)
            .collect::<Vec<_>>();
        if purchase_count == 0
            || purchase_count > 5
            || receipts.len() != purchase_count
            || final_owned.len() != purchase_count
        {
            continue;
        }
        receipts.sort_by_key(|(_, _, instances)| instances.len());
        let mut previous =
            BTreeMap::<String, crate::domain::cosmetic_shop::LandscapeInstance>::new();
        let mut group_proofs = Vec::with_capacity(receipts.len());
        let mut sequence_is_complete = true;
        for (position, (request_id, quote, instances)) in receipts.into_iter().enumerate() {
            let expected_count = position + 1;
            let current = instances
                .into_iter()
                .map(|instance| (instance.instance_id.clone(), instance))
                .collect::<BTreeMap<_, _>>();
            if current.len() != expected_count {
                sequence_is_complete = false;
                break;
            }
            let added = current
                .keys()
                .filter(|instance_id| !previous.contains_key(*instance_id))
                .cloned()
                .collect::<Vec<_>>();
            let removed = previous
                .keys()
                .any(|instance_id| !current.contains_key(instance_id));
            if added.len() != 1 || removed {
                sequence_is_complete = false;
                break;
            }
            if previous.iter().any(|(instance_id, prior)| {
                current
                    .get(instance_id)
                    .is_none_or(|current| !same_instance_acquisition(prior, current))
            }) {
                sequence_is_complete = false;
                break;
            }
            let new_instance_id = added[0].clone();
            let new_instance = &current[&new_instance_id];
            if new_instance.variation_index as usize != expected_count - 1
                || current.values().any(|instance| {
                    !final_owned.iter().any(|final_instance| {
                        final_instance.instance_id == instance.instance_id
                            && same_instance_capture(instance, final_instance)
                    })
                })
            {
                sequence_is_complete = false;
                break;
            }
            group_proofs.push(GuestPurchaseProof {
                request_id,
                quote,
                status: ShopActionStatus::Purchased,
                ownership: Landscape {
                    instance_id: new_instance_id,
                },
            });
            previous = current;
        }
        let final_ids = final_owned
            .iter()
            .map(|instance| instance.instance_id.as_str())
            .collect::<BTreeSet<_>>();
        if sequence_is_complete
            && previous.keys().map(String::as_str).collect::<BTreeSet<_>>() == final_ids
        {
            for proof in group_proofs {
                proofs_by_id.insert(proof.request_id.clone(), proof);
            }
        }
    }

    Ok(purchases
        .iter()
        .filter_map(|purchase| proofs_by_id.remove(&purchase.purchase_id))
        .collect())
}

fn removal_proofs(
    connection: &Connection,
    tombstones: &[GuestNaturalRemoval],
    debits: &[GuestRemovalDebit],
) -> Result<Vec<GuestRemovalProof>, ScanError> {
    use crate::domain::cosmetic_shop::QuoteTarget;

    let receipts = query_rows(
        connection,
        "SELECT request_id,payload_json,result_json FROM shop_action_request
         WHERE account_id=?1 ORDER BY request_id",
        "local",
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        },
    )?
    .into_iter()
    .map(|(request_id, payload, result)| (request_id, (payload, result)))
    .collect::<BTreeMap<_, _>>();
    let mut proofs = Vec::new();
    for debit in debits {
        let Some((payload_json, result_json)) = receipts.get(&debit.request_id) else {
            continue;
        };
        let Ok(ShopRequest::RemoveNatural {
            request_id,
            key,
            quote,
            ..
        }) = serde_json::from_str::<ShopRequest>(payload_json)
        else {
            continue;
        };
        let QuoteTarget::RemoveNatural { key: quoted_key } = &quote.target else {
            continue;
        };
        if request_id != debit.request_id || quoted_key != &key || quote.price != debit.amount {
            continue;
        }
        let Some(result_json) = result_json else {
            continue;
        };
        let Ok(result) = serde_json::from_str::<ShopActionResult>(result_json) else {
            continue;
        };
        if result.request_id != request_id
            || result.status != ShopActionStatus::Removed
            || result.confirmed_quote.as_ref() != Some(&quote)
            || result.state.account_id != "local"
            || !result.state.removed_natural_keys.iter().any(|removed| {
                removed.cycle_id == key.cycle_id
                    && removed.stage == key.stage
                    && removed.ordinal == key.ordinal
            })
        {
            continue;
        }
        let matching_tombstones = tombstones
            .iter()
            .filter(|removed| {
                removed.cycle_id == key.cycle_id
                    && removed.stage == key.stage
                    && removed.ordinal == key.ordinal
            })
            .count();
        if matching_tombstones != 1 {
            continue;
        }
        proofs.push(GuestRemovalProof {
            request_id,
            target: key,
            quote,
            status: ShopActionStatus::Removed,
        });
    }
    Ok(proofs)
}

fn same_instance_acquisition(
    left: &crate::domain::cosmetic_shop::LandscapeInstance,
    right: &crate::domain::cosmetic_shop::LandscapeInstance,
) -> bool {
    left.instance_id == right.instance_id
        && left.sku == right.sku
        && left.variation_index == right.variation_index
        && left.seed == right.seed
        && left.variation_version == right.variation_version
}

fn same_instance_capture(
    source: &crate::domain::cosmetic_shop::LandscapeInstance,
    captured: &GuestLandscapeInstance,
) -> bool {
    source.instance_id == captured.instance_id
        && source.sku == captured.sku
        && source.variation_index == captured.variation_index
        && source.seed == captured.seed
        && source.variation_version == captured.variation_version
}

fn validate_local_integrity(data: &GuestShopImportData) -> Vec<GuestShopImportIntegrityIssue> {
    use crate::domain::cosmetic_shop::{shop_products, AvatarSlot, ShopCategory};

    let products = shop_products();
    let products_by_sku = products
        .iter()
        .map(|product| (product.sku.as_str(), product))
        .collect::<BTreeMap<_, _>>();
    let mut issues = Vec::new();

    const MAX_LAYOUT_NATURAL_OBJECTS: usize = 154;
    // The 24x10 layout has 240 cells; reserved rows account for 48, connector
    // columns for 20, and right-edge columns for 18 additional cells.
    let current_natural_count = data
        .natural_objects
        .iter()
        .filter(|object| object.cycle_id == data.current_cycle.cycle_id)
        .count();
    let layout_over_capacity = current_natural_count > MAX_LAYOUT_NATURAL_OBJECTS;
    if layout_over_capacity {
        issues.push(GuestShopImportIntegrityIssue::TooManyNaturalObjects {
            count: current_natural_count,
        });
    }
    let current_natural_objects = if layout_over_capacity {
        Vec::new()
    } else {
        data.natural_objects
            .iter()
            .filter(|object| object.cycle_id == data.current_cycle.cycle_id)
            .map(|object| PlanetObject {
                stage: object.stage,
                ordinal: object.ordinal,
                kind: object.kind.clone(),
                x: object.x,
                y: object.y,
                seed: object.seed,
            })
            .collect::<Vec<_>>()
    };
    let terrain = (!layout_over_capacity).then(|| terrain_bounds(&current_natural_objects));

    let check_timestamp =
        |issues: &mut Vec<GuestShopImportIntegrityIssue>, field: String, value: Option<&str>| {
            if value.is_some_and(|value| DateTime::parse_from_rfc3339(value).is_err()) {
                issues.push(GuestShopImportIntegrityIssue::InvalidTimestamp { field });
            }
        };
    let check_date =
        |issues: &mut Vec<GuestShopImportIntegrityIssue>, field: String, value: &str| {
            if NaiveDate::parse_from_str(value, "%Y-%m-%d").is_err() {
                issues.push(GuestShopImportIntegrityIssue::InvalidTimestamp { field });
            }
        };
    let check_timezone =
        |issues: &mut Vec<GuestShopImportIntegrityIssue>, field: String, value: &str| {
            if value.parse::<chrono_tz::Tz>().is_err() {
                issues.push(GuestShopImportIntegrityIssue::InvalidTimezone { field });
            }
        };

    check_timezone(&mut issues, "world_timezone".into(), &data.world_timezone);
    check_timezone(&mut issues, "planet_timezone".into(), &data.planet_timezone);
    check_timezone(&mut issues, "reward_timezone".into(), &data.reward_timezone);
    check_timestamp(
        &mut issues,
        "activation_at_utc".into(),
        Some(&data.activation_at_utc),
    );
    check_timestamp(
        &mut issues,
        "current_cycle.started_at_utc".into(),
        data.current_cycle.started_at_utc.as_deref(),
    );
    check_timestamp(
        &mut issues,
        "current_cycle.ended_at_utc".into(),
        data.current_cycle.ended_at_utc.as_deref(),
    );
    check_timestamp(
        &mut issues,
        "last_reset_at_utc".into(),
        data.last_reset_at_utc.as_deref(),
    );
    check_timestamp(
        &mut issues,
        "reset_available_at_utc".into(),
        data.reset_available_at_utc.as_deref(),
    );
    for (index, cycle) in data.historical_cycles.iter().enumerate() {
        check_timestamp(
            &mut issues,
            format!("historical_cycles[{index}].started_at_utc"),
            cycle.started_at_utc.as_deref(),
        );
        check_timestamp(
            &mut issues,
            format!("historical_cycles[{index}].ended_at_utc"),
            cycle.ended_at_utc.as_deref(),
        );
    }
    if let Some(timeline) = &data.effect_timeline_state {
        check_timestamp(
            &mut issues,
            "effect_timeline_state.server_time_utc".into(),
            Some(&timeline.server_time_utc),
        );
        check_timezone(
            &mut issues,
            "effect_timeline_state.reward_timezone".into(),
            &timeline.reward_timezone,
        );
        if timeline.current_cycle_id != data.current_cycle.cycle_id
            || timeline.reward_timezone != data.reward_timezone
        {
            issues.push(GuestShopImportIntegrityIssue::InvalidEffectTimeline {
                reason: "timeline_state_mismatch".into(),
            });
        }
    }
    for (index, instance) in data.landscape_instances.iter().enumerate() {
        check_timestamp(
            &mut issues,
            format!("landscape_instances[{index}].acquired_at_utc"),
            Some(&instance.acquired_at_utc),
        );
    }
    for (index, owned) in data.avatar_owned.iter().enumerate() {
        check_timestamp(
            &mut issues,
            format!("avatar_owned[{index}].acquired_at_utc"),
            Some(&owned.acquired_at_utc),
        );
    }
    for (index, purchase) in data
        .cosmetic_purchases
        .iter()
        .chain(&data.purchases)
        .enumerate()
    {
        check_timestamp(
            &mut issues,
            format!("purchases[{index}].purchased_at_utc"),
            Some(&purchase.purchased_at_utc),
        );
    }
    for (index, purchase) in data.pending_purchases.iter().enumerate() {
        check_timestamp(
            &mut issues,
            format!("pending_purchases[{index}].created_at_utc"),
            Some(&purchase.created_at_utc),
        );
    }
    for (index, removal) in data.natural_removals.iter().enumerate() {
        check_timestamp(
            &mut issues,
            format!("natural_removals[{index}].removed_at_utc"),
            Some(&removal.removed_at_utc),
        );
    }
    for (index, debit) in data.removal_debits.iter().enumerate() {
        check_timestamp(
            &mut issues,
            format!("removal_debits[{index}].created_at_utc"),
            Some(&debit.created_at_utc),
        );
    }
    for (index, bound) in data.effect_cycle_bounds.iter().enumerate() {
        check_timestamp(
            &mut issues,
            format!("effect_cycle_bounds[{index}].started_at_utc"),
            Some(&bound.started_at_utc),
        );
        check_timestamp(
            &mut issues,
            format!("effect_cycle_bounds[{index}].ended_at_utc"),
            bound.ended_at_utc.as_deref(),
        );
    }
    for (index, interval) in data.effect_history.iter().enumerate() {
        check_timestamp(
            &mut issues,
            format!("effect_history[{index}].started_at_utc"),
            Some(&interval.started_at_utc),
        );
        check_timestamp(
            &mut issues,
            format!("effect_history[{index}].ended_at_utc"),
            interval.ended_at_utc.as_deref(),
        );
    }
    for (index, contribution) in data.effect_contributions.iter().enumerate() {
        check_date(
            &mut issues,
            format!("effect_contributions[{index}].date"),
            &contribution.date,
        );
    }
    for (index, activity) in data.activity_days.iter().enumerate() {
        check_date(
            &mut issues,
            format!("activity_days[{index}].reward_date"),
            &activity.reward_date,
        );
        check_timestamp(
            &mut issues,
            format!("activity_days[{index}].first_occurred_at_utc"),
            Some(&activity.first_occurred_at_utc),
        );
    }
    for (index, reward) in data.game_rewards.iter().enumerate() {
        check_timestamp(
            &mut issues,
            format!("game_rewards[{index}].awarded_at_utc"),
            Some(&reward.awarded_at_utc),
        );
    }
    for (index, credit) in data.wallet_credits.iter().enumerate() {
        check_timestamp(
            &mut issues,
            format!("wallet_credits[{index}].created_at_utc"),
            Some(&credit.created_at_utc),
        );
    }
    for (index, claim) in data.unverified_planet_wallet_claims.iter().enumerate() {
        check_timestamp(
            &mut issues,
            format!("unverified_planet_wallet_claims[{index}].created_at_utc"),
            Some(&claim.created_at_utc),
        );
    }
    for (index, settlement) in data.cycle_settlements.iter().enumerate() {
        check_timestamp(
            &mut issues,
            format!("cycle_settlements[{index}].settled_at_utc"),
            Some(&settlement.settled_at_utc),
        );
    }
    for (index, progress) in data.era_progress.iter().enumerate() {
        check_timestamp(
            &mut issues,
            format!("era_progress[{index}].awarded_at_utc"),
            Some(&progress.awarded_at_utc),
        );
    }
    for (index, aggregate) in data.daily_agent_totals.iter().enumerate() {
        check_date(
            &mut issues,
            format!("daily_agent_totals[{index}].bucket_date"),
            &aggregate.bucket_date,
        );
    }
    for (index, aggregate) in data.usage_aggregates.iter().enumerate() {
        check_date(
            &mut issues,
            format!("usage_aggregates[{index}].bucket_date"),
            &aggregate.bucket_date,
        );
    }
    if let Some(state) = &data.growth_journal_state {
        check_timestamp(
            &mut issues,
            "growth_journal_state.deleted_at_utc".into(),
            state.deleted_at_utc.as_deref(),
        );
    }
    for (index, cycle) in data.growth_journal_cycles.iter().enumerate() {
        check_timestamp(
            &mut issues,
            format!("growth_journal_cycles[{index}].started_at_utc"),
            cycle.started_at_utc.as_deref(),
        );
        check_timestamp(
            &mut issues,
            format!("growth_journal_cycles[{index}].ended_at_utc"),
            cycle.ended_at_utc.as_deref(),
        );
        check_timestamp(
            &mut issues,
            format!("growth_journal_cycles[{index}].wallet_credit_at_utc"),
            cycle.wallet_credit_at_utc.as_deref(),
        );
    }
    for (index, entry) in data.growth_journal_entries.iter().enumerate() {
        check_date(
            &mut issues,
            format!("growth_journal_entries[{index}].bucket_date"),
            &entry.bucket_date,
        );
    }
    for (index, proof) in data.reset_settlement_proofs.iter().enumerate() {
        check_timestamp(
            &mut issues,
            format!("reset_settlement_proofs[{index}].reset_at_utc"),
            Some(&proof.reset_at_utc),
        );
        check_timestamp(
            &mut issues,
            format!("reset_settlement_proofs[{index}].old_cycle_started_at_utc"),
            proof.old_cycle_started_at_utc.as_deref(),
        );
        check_timestamp(
            &mut issues,
            format!("reset_settlement_proofs[{index}].new_cycle_started_at_utc"),
            proof.new_cycle_started_at_utc.as_deref(),
        );
        check_timestamp(
            &mut issues,
            format!("reset_settlement_proofs[{index}].reset_available_at_utc"),
            proof.reset_available_at_utc.as_deref(),
        );
        if let Some(claim) = &proof.raw_wallet_claim {
            check_timestamp(
                &mut issues,
                format!("reset_settlement_proofs[{index}].raw_wallet_claim.created_at_utc"),
                Some(&claim.created_at_utc),
            );
        }
    }

    let mut instance_ids = BTreeSet::new();
    let mut landscape_counts = BTreeMap::<String, u64>::new();
    for instance in &data.landscape_instances {
        instance_ids.insert(instance.instance_id.as_str());
        let count = landscape_counts.entry(instance.sku.clone()).or_default();
        *count += 1;
        match products_by_sku.get(instance.sku.as_str()) {
            Some(product) if product.category == ShopCategory::Landscape => {}
            _ => issues.push(GuestShopImportIntegrityIssue::UnknownLandscapeSku {
                sku: instance.sku.clone(),
            }),
        }
    }
    for (sku, count) in landscape_counts {
        if count > 5 {
            issues.push(GuestShopImportIntegrityIssue::TooManyLandscapeInstances { sku, count });
        }
    }
    for placement in &data.placements {
        if !instance_ids.contains(placement.instance_id.as_str()) {
            issues.push(GuestShopImportIntegrityIssue::PlacementMissingInstance {
                instance_id: placement.instance_id.clone(),
            });
        }
        if placement.cycle_id != data.current_cycle.cycle_id {
            issues.push(GuestShopImportIntegrityIssue::PlacementCycleMismatch {
                instance_id: placement.instance_id.clone(),
                cycle_id: placement.cycle_id.clone(),
            });
        }
        if let Some(instance) = data
            .landscape_instances
            .iter()
            .find(|instance| instance.instance_id == placement.instance_id)
        {
            let product = products_by_sku.get(instance.sku.as_str()).copied();
            if let Some(terrain) = terrain {
                let valid = product.is_some_and(|product| {
                    product.category == ShopCategory::Landscape
                        && validate_placement(
                            product,
                            LandscapePoint {
                                x: placement.x,
                                y: placement.y,
                            },
                            terrain,
                        )
                        .is_ok()
                });
                if !valid {
                    issues.push(GuestShopImportIntegrityIssue::InvalidPlacementGeometry {
                        instance_id: placement.instance_id.clone(),
                    });
                }
            }
        }
    }

    let mut owned_avatar = BTreeSet::new();
    for owned in &data.avatar_owned {
        owned_avatar.insert(owned.sku.as_str());
        match products_by_sku.get(owned.sku.as_str()) {
            Some(product) if product.category == ShopCategory::Avatar => {}
            _ => issues.push(GuestShopImportIntegrityIssue::UnknownAvatarSku {
                sku: owned.sku.clone(),
            }),
        }
    }
    for equipment in &data.avatar_equipment {
        let Some(sku) = equipment.sku.as_deref() else {
            continue;
        };
        if !owned_avatar.contains(sku) {
            issues.push(GuestShopImportIntegrityIssue::AvatarEquipmentNotOwned {
                slot: equipment.slot.clone(),
                sku: sku.to_owned(),
            });
        }
        let expected_slot = match equipment.slot.as_str() {
            "head" => Some(AvatarSlot::Head),
            "outfit" => Some(AvatarSlot::Outfit),
            "face" => Some(AvatarSlot::Face),
            "back" => Some(AvatarSlot::Back),
            _ => None,
        };
        if products_by_sku
            .get(sku)
            .and_then(|product| product.avatar_slot)
            != expected_slot
        {
            issues.push(GuestShopImportIntegrityIssue::AvatarEquipmentSlotMismatch {
                slot: equipment.slot.clone(),
                sku: sku.to_owned(),
            });
        }
    }
    for purchase in &data.purchases {
        if !products_by_sku.contains_key(purchase.sku.as_str()) {
            issues.push(GuestShopImportIntegrityIssue::UnknownPurchaseSku {
                sku: purchase.sku.clone(),
            });
        }
    }
    let mut proven_landscape_ids = BTreeSet::new();
    let mut proven_avatar_purchase_ids = BTreeSet::new();
    for purchase in &data.purchases {
        let matching = data
            .purchase_proofs
            .iter()
            .filter(|proof| proof.request_id == purchase.purchase_id)
            .collect::<Vec<_>>();
        let valid = if matching.len() != 1 {
            false
        } else {
            let proof = matching[0];
            let quoted_sku = match &proof.quote.target {
                crate::domain::cosmetic_shop::QuoteTarget::Purchase { sku } => Some(sku.as_str()),
                _ => None,
            };
            let category = products_by_sku
                .get(purchase.sku.as_str())
                .map(|product| product.category);
            let quote_matches = proof.request_id == purchase.purchase_id
                && proof.status == ShopActionStatus::Purchased
                && quoted_sku == Some(purchase.sku.as_str())
                && proof.quote.price == purchase.price;
            if !quote_matches {
                false
            } else {
                match (&proof.ownership, category) {
                    (
                        GuestPurchaseOwnershipProof::Landscape { instance_id },
                        Some(ShopCategory::Landscape),
                    ) => {
                        let owned = data.landscape_instances.iter().any(|instance| {
                            instance.instance_id == *instance_id && instance.sku == purchase.sku
                        });
                        owned && proven_landscape_ids.insert(instance_id.clone())
                    }
                    (GuestPurchaseOwnershipProof::Avatar { sku }, Some(ShopCategory::Avatar)) => {
                        let owned = data.avatar_owned.iter().any(|instance| {
                            instance.purchase_id == purchase.purchase_id
                                && instance.sku == *sku
                                && instance.sku == purchase.sku
                                && instance.price == purchase.price
                        });
                        owned && proven_avatar_purchase_ids.insert(purchase.purchase_id.clone())
                    }
                    _ => false,
                }
            }
        };
        if !valid {
            issues.push(GuestShopImportIntegrityIssue::PurchaseProofUnverifiable {
                purchase_id: purchase.purchase_id.clone(),
            });
        }
    }
    for proof in &data.purchase_proofs {
        if data
            .purchases
            .iter()
            .filter(|purchase| purchase.purchase_id == proof.request_id)
            .count()
            != 1
        {
            issues.push(GuestShopImportIntegrityIssue::PurchaseProofUnverifiable {
                purchase_id: proof.request_id.clone(),
            });
        }
    }
    for instance in &data.landscape_instances {
        if !proven_landscape_ids.contains(&instance.instance_id) {
            issues.push(
                GuestShopImportIntegrityIssue::LandscapeOwnershipUnverifiable {
                    instance_id: instance.instance_id.clone(),
                },
            );
        }
    }
    for owned in &data.avatar_owned {
        if !proven_avatar_purchase_ids.contains(&owned.purchase_id) {
            issues.push(GuestShopImportIntegrityIssue::AvatarOwnershipUnverifiable {
                purchase_id: owned.purchase_id.clone(),
            });
        }
    }
    let mut proven_removal_targets = BTreeSet::<(String, u8, u32)>::new();
    for debit in &data.removal_debits {
        let matching = data
            .removal_proofs
            .iter()
            .filter(|proof| proof.request_id == debit.request_id)
            .collect::<Vec<_>>();
        let valid = if matching.len() != 1 {
            false
        } else {
            let proof = matching[0];
            let quote_targets_key = match &proof.quote.target {
                crate::domain::cosmetic_shop::QuoteTarget::RemoveNatural { key } => {
                    key == &proof.target
                }
                _ => false,
            };
            let matching_tombstones = data
                .natural_removals
                .iter()
                .filter(|removed| {
                    removed.cycle_id == proof.target.cycle_id
                        && removed.stage == proof.target.stage
                        && removed.ordinal == proof.target.ordinal
                })
                .count();
            let key = (
                proof.target.cycle_id.clone(),
                proof.target.stage,
                proof.target.ordinal,
            );
            let valid = proof.status == ShopActionStatus::Removed
                && proof.request_id == debit.request_id
                && proof.quote.price == debit.amount
                && quote_targets_key
                && matching_tombstones == 1
                && proven_removal_targets.insert(key);
            valid
        };
        if !valid {
            issues.push(GuestShopImportIntegrityIssue::RemovalProofUnverifiable {
                request_id: debit.request_id.clone(),
            });
        }
    }
    for proof in &data.removal_proofs {
        if data
            .removal_debits
            .iter()
            .filter(|debit| debit.request_id == proof.request_id)
            .count()
            != 1
        {
            issues.push(GuestShopImportIntegrityIssue::RemovalProofUnverifiable {
                request_id: proof.request_id.clone(),
            });
        }
    }
    for tombstone in &data.natural_removals {
        if !proven_removal_targets.contains(&(
            tombstone.cycle_id.clone(),
            tombstone.stage,
            tombstone.ordinal,
        )) {
            issues.push(
                GuestShopImportIntegrityIssue::NaturalTombstoneUnverifiable {
                    cycle_id: tombstone.cycle_id.clone(),
                    stage: tombstone.stage,
                    ordinal: tombstone.ordinal,
                },
            );
        }
    }
    let legacy_skus = crate::domain::cosmetic_shop::legacy_cosmetic_products()
        .into_iter()
        .map(|product| product.sku)
        .collect::<BTreeSet<_>>();
    for purchase in &data.cosmetic_purchases {
        if !legacy_skus.contains(&purchase.sku)
            && !products_by_sku.contains_key(purchase.sku.as_str())
        {
            issues.push(GuestShopImportIntegrityIssue::UnknownLegacyCosmeticSku {
                sku: purchase.sku.clone(),
            });
        }
    }
    if !data.effect_cycle_bounds_authoritative
        && (!data.effect_cycle_bounds.is_empty() || !data.effect_history.is_empty())
    {
        issues.push(GuestShopImportIntegrityIssue::InvalidEffectTimeline {
            reason: "cycle_bounds_not_authoritative".into(),
        });
    }
    let mut bound_cycles = BTreeSet::new();
    let mut parsed_bounds = Vec::new();
    for bound in &data.effect_cycle_bounds {
        let started_at = DateTime::parse_from_rfc3339(&bound.started_at_utc)
            .ok()
            .map(|time| time.with_timezone(&Utc));
        let ended_at = bound
            .ended_at_utc
            .as_deref()
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .map(|time| time.with_timezone(&Utc));
        if bound.cycle_id.trim().is_empty() || !bound_cycles.insert(bound.cycle_id.as_str()) {
            issues.push(GuestShopImportIntegrityIssue::InvalidEffectTimeline {
                reason: "duplicate_or_empty_cycle_bound".into(),
            });
        }
        if let Some(started_at) = started_at {
            if ended_at.is_some_and(|ended_at| ended_at <= started_at) {
                issues.push(GuestShopImportIntegrityIssue::InvalidEffectTimeline {
                    reason: "cycle_bound_end_not_after_start".into(),
                });
            }
            parsed_bounds.push((bound.cycle_id.as_str(), started_at, ended_at));
        } else {
            issues.push(GuestShopImportIntegrityIssue::InvalidEffectTimeline {
                reason: "invalid_cycle_bound_start".into(),
            });
        }
    }
    parsed_bounds.sort_by_key(|(_, started_at, _)| *started_at);
    for (index, (cycle_id, _, ended_at)) in parsed_bounds.iter().enumerate() {
        let is_current = *cycle_id == data.current_cycle.cycle_id;
        if (is_current && (index + 1 != parsed_bounds.len() || ended_at.is_some()))
            || (!is_current && ended_at.is_none())
            || (index > 0
                && (parsed_bounds[index - 1].1 >= parsed_bounds[index].1
                    || parsed_bounds[index - 1]
                        .2
                        .is_none_or(|previous_end| previous_end > parsed_bounds[index].1)))
        {
            issues.push(GuestShopImportIntegrityIssue::InvalidEffectTimeline {
                reason: "cycle_bounds_out_of_order_or_open".into(),
            });
        }
    }
    let mut parsed_intervals = Vec::new();
    for interval in &data.effect_history {
        for instance_id in &interval.active_instance_ids {
            if !instance_ids.contains(instance_id.as_str()) {
                issues.push(
                    GuestShopImportIntegrityIssue::EffectReferencesUnknownInstance {
                        instance_id: instance_id.clone(),
                    },
                );
            }
        }
        let started_at = DateTime::parse_from_rfc3339(&interval.started_at_utc)
            .ok()
            .map(|time| time.with_timezone(&Utc));
        let ended_at = interval
            .ended_at_utc
            .as_deref()
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .map(|time| time.with_timezone(&Utc));
        let Some(started_at) = started_at else {
            issues.push(GuestShopImportIntegrityIssue::InvalidEffectTimeline {
                reason: "invalid_interval_start".into(),
            });
            continue;
        };
        if interval.revision == 0
            || ended_at.is_some_and(|ended_at| ended_at <= started_at)
            || interval
                .active_instance_ids
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            issues.push(GuestShopImportIntegrityIssue::InvalidEffectTimeline {
                reason: "invalid_interval_order_or_revision".into(),
            });
        }
        let Some((_, bound_start, bound_end)) = parsed_bounds
            .iter()
            .find(|(cycle_id, _, _)| *cycle_id == interval.cycle_id)
        else {
            issues.push(GuestShopImportIntegrityIssue::InvalidEffectTimeline {
                reason: "interval_missing_cycle_bound".into(),
            });
            continue;
        };
        if started_at < *bound_start
            || ended_at
                .is_some_and(|ended_at| bound_end.is_some_and(|bound_end| ended_at > bound_end))
            || (bound_end.is_some() && ended_at.is_none())
        {
            issues.push(GuestShopImportIntegrityIssue::InvalidEffectTimeline {
                reason: "interval_outside_cycle_bound".into(),
            });
        }
        if let Some(server_time) = data
            .effect_timeline_state
            .as_ref()
            .and_then(|state| DateTime::parse_from_rfc3339(&state.server_time_utc).ok())
            .map(|time| time.with_timezone(&Utc))
        {
            if started_at > server_time || ended_at.is_some_and(|ended_at| ended_at > server_time) {
                issues.push(GuestShopImportIntegrityIssue::InvalidEffectTimeline {
                    reason: "interval_after_server_time".into(),
                });
            }
        }
        parsed_intervals.push((interval, started_at, ended_at));
    }
    parsed_intervals.sort_by_key(|(_, started_at, _)| *started_at);
    for pair in parsed_intervals.windows(2) {
        let (previous, previous_start, previous_end) = pair[0];
        let (next, next_start, _) = pair[1];
        if previous_start >= next_start
            || previous.revision >= next.revision
            || previous_end.is_none_or(|ended_at| ended_at > next_start)
        {
            issues.push(GuestShopImportIntegrityIssue::InvalidEffectTimeline {
                reason: "effect_intervals_overlap_or_reorder".into(),
            });
        }
    }
    if parsed_intervals
        .last()
        .is_some_and(|(interval, _, ended_at)| {
            ended_at.is_none() && interval.cycle_id != data.current_cycle.cycle_id
        })
    {
        issues.push(GuestShopImportIntegrityIssue::InvalidEffectTimeline {
            reason: "open_interval_not_current_cycle".into(),
        });
    }

    let mut known_cycles = BTreeSet::from([data.current_cycle.cycle_id.as_str()]);
    known_cycles.extend(bound_cycles.iter().copied());
    known_cycles.extend(data.historical_cycles.iter().filter_map(|cycle| {
        (cycle.started_at_utc.is_some()
            || cycle.ended_at_utc.is_some()
            || cycle.settled_bonus_tokens.is_some())
        .then_some(cycle.cycle_id.as_str())
    }));
    known_cycles.extend(data.natural_objects.iter().map(|row| row.cycle_id.as_str()));
    known_cycles.extend(
        data.natural_removals
            .iter()
            .map(|row| row.cycle_id.as_str()),
    );
    known_cycles.extend(
        data.effect_contributions
            .iter()
            .map(|row| row.cycle_id.as_str()),
    );
    known_cycles.extend(data.effect_history.iter().map(|row| row.cycle_id.as_str()));
    known_cycles.extend(data.game_rewards.iter().map(|row| row.cycle_id.as_str()));
    known_cycles.extend(data.wallet_credits.iter().map(|row| row.cycle_id.as_str()));
    known_cycles.extend(
        data.cycle_settlements
            .iter()
            .map(|row| row.cycle_id.as_str()),
    );
    known_cycles.extend(data.era_progress.iter().map(|row| row.cycle_id.as_str()));
    known_cycles.extend(
        data.usage_aggregates
            .iter()
            .filter_map(|row| row.cycle_id.as_deref()),
    );
    known_cycles.extend(
        data.cycle_usage_totals
            .iter()
            .map(|row| row.cycle_id.as_str()),
    );
    known_cycles.extend(
        data.growth_journal_cycles
            .iter()
            .map(|row| row.cycle_id.as_str()),
    );
    known_cycles.extend(
        data.growth_journal_entries
            .iter()
            .map(|row| row.cycle_id.as_str()),
    );
    known_cycles.extend(data.reset_settlement_proofs.iter().flat_map(|proof| {
        [
            proof.previous_cycle_id.as_str(),
            proof.new_cycle_id.as_str(),
        ]
    }));
    for activity in &data.activity_days {
        if !known_cycles.contains(activity.cycle_id.as_str()) {
            issues.push(GuestShopImportIntegrityIssue::ActivityCycleUnknown {
                cycle_id: activity.cycle_id.clone(),
            });
        }
        if let Some((_, started_at, ended_at)) = parsed_bounds
            .iter()
            .find(|(cycle_id, _, _)| *cycle_id == activity.cycle_id)
        {
            if let Ok(occurred_at) = DateTime::parse_from_rfc3339(&activity.first_occurred_at_utc) {
                let occurred_at = occurred_at.with_timezone(&Utc);
                if occurred_at < *started_at
                    || ended_at.is_some_and(|ended_at| occurred_at >= ended_at)
                {
                    issues.push(GuestShopImportIntegrityIssue::InvalidEffectTimeline {
                        reason: "activity_outside_cycle_bound".into(),
                    });
                }
            }
        }
    }
    add_reset_proof_integrity_issues(data, &mut issues);
    issues
}

fn add_reset_proof_integrity_issues(
    data: &GuestShopImportData,
    issues: &mut Vec<GuestShopImportIntegrityIssue>,
) {
    let cycle_ids = data
        .cycle_settlements
        .iter()
        .map(|settlement| settlement.cycle_id.clone())
        .chain(
            data.unverified_planet_wallet_claims
                .iter()
                .map(|claim| claim.previous_cycle_id.clone()),
        )
        .chain(
            data.reset_settlement_proofs
                .iter()
                .map(|proof| proof.previous_cycle_id.clone()),
        )
        .collect::<BTreeSet<_>>();

    for previous_cycle_id in cycle_ids {
        let settlements = data
            .cycle_settlements
            .iter()
            .filter(|settlement| settlement.cycle_id == previous_cycle_id)
            .collect::<Vec<_>>();
        let wallet_claims = data
            .unverified_planet_wallet_claims
            .iter()
            .filter(|claim| claim.previous_cycle_id == previous_cycle_id)
            .collect::<Vec<_>>();
        let proofs = data
            .reset_settlement_proofs
            .iter()
            .filter(|proof| proof.previous_cycle_id == previous_cycle_id)
            .collect::<Vec<_>>();
        let complete = if settlements.len() != 1 || proofs.len() != 1 || wallet_claims.len() > 1 {
            false
        } else {
            let proof = proofs[0];
            let claim_matches = match wallet_claims.as_slice() {
                [] => proof.raw_wallet_claim.is_none(),
                [claim] => proof.raw_wallet_claim.as_ref() == Some(*claim),
                _ => false,
            };
            proof.new_cycle_id != previous_cycle_id
                && proof.settled_bonus_tokens == Some(settlements[0].amount)
                && claim_matches
                && proof.old_cycle_started_at_utc.is_some()
                && proof.new_cycle_started_at_utc.is_some()
                && proof.final_effect_revision.is_some()
                && proof.reset_available_at_utc.is_some()
        };
        if !complete {
            issues
                .push(GuestShopImportIntegrityIssue::ResetProofUnverifiable { previous_cycle_id });
        }
    }
}

fn reset_proofs(
    connection: &Connection,
    settlements: &[GuestCycleSettlement],
    wallet_claims: &[GuestUnverifiedWalletClaim],
    effect_history: &[crate::domain::cosmetic_shop::ShopEffectInterval],
    journal_cycles: &[GuestGrowthJournalCycle],
    historical_cycles: &[GuestShopCycle],
    current_cycle: &GuestShopCycle,
    reset_available_at_utc: Option<&str>,
) -> Result<(Vec<GuestResetSettlementProof>, bool), ScanError> {
    let receipts = query_rows(
        connection,
        "SELECT request_id,payload_json,result_json,created_at_utc FROM shop_action_request
         WHERE account_id=?1 ORDER BY created_at_utc,request_id",
        "local",
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
            ))
        },
    )?;
    let mut proofs = Vec::new();
    let mut unverifiable = false;
    for (stored_request_id, payload_json, result_json, created_at_utc) in receipts {
        let request: ShopRequest = match serde_json::from_str(&payload_json) {
            Ok(request) => request,
            Err(_) => {
                unverifiable = true;
                continue;
            }
        };
        let ShopRequest::ResetPlanet {
            request_id,
            cycle_id: previous_cycle_id,
        } = request
        else {
            continue;
        };
        if request_id != stored_request_id {
            unverifiable = true;
            continue;
        }
        let Some(result_json) = result_json else {
            unverifiable = true;
            continue;
        };
        let result: crate::domain::cosmetic_shop::ShopActionResult =
            match serde_json::from_str(&result_json) {
                Ok(result) => result,
                Err(_) => {
                    unverifiable = true;
                    continue;
                }
            };
        if result.status != ShopActionStatus::Reset {
            unverifiable = true;
            continue;
        }
        if result.request_id != request_id
            || result.state.account_id != "local"
            || result.state.current_cycle_id == previous_cycle_id
        {
            unverifiable = true;
            continue;
        }
        let new_cycle_id = result.state.current_cycle_id;
        let final_interval = effect_history
            .iter()
            .filter(|interval| interval.cycle_id == previous_cycle_id)
            .max_by_key(|interval| interval.revision);
        let old_cycle_started_at_utc = journal_cycles
            .iter()
            .find(|cycle| cycle.cycle_id == previous_cycle_id)
            .and_then(|cycle| cycle.started_at_utc.clone())
            .or_else(|| {
                historical_cycles
                    .iter()
                    .find(|cycle| cycle.cycle_id == previous_cycle_id)
                    .and_then(|cycle| cycle.started_at_utc.clone())
            });
        let new_cycle_started_at_utc = if current_cycle.cycle_id == new_cycle_id {
            current_cycle.started_at_utc.clone()
        } else {
            historical_cycles
                .iter()
                .find(|cycle| cycle.cycle_id == new_cycle_id)
                .and_then(|cycle| cycle.started_at_utc.clone())
        };
        let proof_reset_available_at_utc = if current_cycle.cycle_id == new_cycle_id {
            reset_available_at_utc.map(str::to_owned)
        } else {
            None
        };
        proofs.push(GuestResetSettlementProof {
            request_id,
            previous_cycle_id: previous_cycle_id.clone(),
            new_cycle_id,
            reset_at_utc: created_at_utc,
            raw_wallet_claim: wallet_claims
                .iter()
                .find(|claim| claim.previous_cycle_id == previous_cycle_id)
                .cloned(),
            settled_bonus_tokens: settlements
                .iter()
                .find(|settlement| settlement.cycle_id == previous_cycle_id)
                .map(|settlement| settlement.amount),
            final_effect_revision: final_interval.map(|interval| interval.revision),
            final_effects: final_interval.map(|interval| interval.effects.clone()),
            final_active_instance_ids: final_interval
                .map(|interval| interval.active_instance_ids.clone())
                .unwrap_or_default(),
            old_cycle_started_at_utc,
            new_cycle_started_at_utc,
            reset_available_at_utc: proof_reset_available_at_utc,
        });
    }
    if proofs.iter().any(|proof| {
        proof.old_cycle_started_at_utc.is_none()
            || proof.new_cycle_started_at_utc.is_none()
            || proof.settled_bonus_tokens.is_none()
            || proof.final_effect_revision.is_none()
            || proof.reset_available_at_utc.is_none()
    }) {
        unverifiable = true;
    }
    if settlements.iter().any(|settlement| {
        !proofs
            .iter()
            .any(|proof| proof.previous_cycle_id == settlement.cycle_id)
    }) || wallet_claims.iter().any(|claim| {
        !proofs
            .iter()
            .any(|proof| proof.previous_cycle_id == claim.previous_cycle_id)
    }) {
        unverifiable = true;
    }
    Ok((proofs, unverifiable))
}

#[cfg(test)]
mod native_empty_bootstrap_capture_tests {
    use super::*;
    use crate::collectors::{ParsedRecord, RecordKind};
    use crate::domain::planet::PlanetAvatar;
    use crate::domain::usage::{Agent, TokenUsage, UsageCoverage};

    #[test]
    fn serializes_native_empty_capture_from_local_storage() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("native-empty-shop.sqlite3");
        let mut ledger = Ledger::open(&database, chrono_tz::UTC).unwrap();
        ledger
            .set_planet_profile("Synthetic Native", PlanetAvatar::Masculine)
            .unwrap();
        ledger.prepare_growth_journal().unwrap();

        let status = ledger
            .capture_guest_shop_import("account:00000000-0000-4000-a000-000000000001")
            .unwrap();
        let data = &status.snapshot.data;

        assert!(status.source_matches_current);
        assert_eq!(data.profile.as_ref().unwrap().nickname, "Synthetic Native");
        assert_eq!(data.shop_state_revision, 0);
        assert!(data.natural_objects.is_empty());
        assert!(data.historical_cycles.is_empty());
        assert!(data.growth_journal_entries.is_empty());
        assert_eq!(data.growth_journal_state.as_ref().unwrap().generation, 0);

        let wire = serde_json::json!({ "schema_version": 1, "snapshot": status.snapshot });
        println!(
            "NATIVE_EMPTY_CAPTURE={}",
            serde_json::to_string(&wire).unwrap()
        );
    }

    #[test]
    fn serializes_native_ordinary_first_reset_capture_without_manufactured_proof() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("native-first-reset-shop.sqlite3");
        let mut ledger = Ledger::open(&database, chrono_tz::UTC).unwrap();
        ledger
            .set_planet_profile("Synthetic First Reset", PlanetAvatar::Masculine)
            .unwrap();

        let reset_at = Utc::now() + chrono::Duration::minutes(1);
        let occurred_at = reset_at - chrono::Duration::seconds(30);
        ledger
            .insert(&ParsedRecord {
                agent: Agent::Codex,
                kind: RecordKind::Response,
                event_key: "native-first-reset-raw-usage".into(),
                occurred_at_utc: occurred_at,
                usage: TokenUsage {
                    input_tokens: None,
                    output_tokens: None,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    total_tokens: Some(1_000_000),
                    coverage: UsageCoverage::Complete,
                },
            })
            .unwrap();
        let previous_cycle_id = ledger.planet_cycle_id().unwrap();

        assert_eq!(ledger.reset_planet(reset_at).unwrap(), 1_000_000);
        let current_cycle_id = ledger.planet_cycle_id().unwrap();
        assert_ne!(current_cycle_id, previous_cycle_id);
        ledger.prepare_growth_journal().unwrap();

        let status = ledger
            .capture_guest_shop_import("account:00000000-0000-4000-a000-000000000001")
            .unwrap();
        let data = &status.snapshot.data;
        assert_eq!(
            status.snapshot.disposition,
            GuestShopImportDisposition::SourceUnverifiable
        );
        assert_eq!(data.lifetime_usage_tokens, Some(1_000_000));
        assert_eq!(data.current_cycle_usage_tokens, Some(0));
        assert_eq!(data.current_cycle.cycle_id, current_cycle_id);
        assert_eq!(data.current_cycle.settled_bonus_tokens, None);
        assert!(data.cycle_settlements.iter().any(|settlement| {
            settlement.cycle_id == previous_cycle_id && settlement.amount == 0
        }));
        assert!(data.unverified_planet_wallet_claims.iter().any(|claim| {
            claim.previous_cycle_id == previous_cycle_id && claim.claimed_amount == 1_000_000
        }));
        assert!(data
            .effect_history
            .iter()
            .all(|interval| interval.cycle_id == current_cycle_id));
        assert!(data.effect_cycle_bounds.is_empty());
        assert!(!data.effect_cycle_bounds_authoritative);
        assert!(data.effect_timeline_state.is_none());
        assert_eq!(data.contribution_canonical_version, Some(1));
        assert!(data.reset_receipts_unverifiable);
        assert_eq!(data.reset_settlement_proofs.len(), 1);
        assert_eq!(
            data.reset_settlement_proofs[0].previous_cycle_id,
            previous_cycle_id
        );
        assert_eq!(data.reset_settlement_proofs[0].final_effect_revision, None);
        assert_eq!(data.reset_settlement_proofs[0].final_effects, None);

        let wire = serde_json::json!({ "schema_version": 1, "snapshot": status.snapshot });
        println!(
            "NATIVE_FIRST_RESET_CAPTURE={}",
            serde_json::to_string(&wire).unwrap()
        );
    }
}
