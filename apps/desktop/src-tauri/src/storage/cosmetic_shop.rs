use std::collections::BTreeSet;

use super::ledger::{Ledger, ScanError};
use super::shop_effects::load_shop_reward_state;
use crate::domain::cosmetic_shop::{
    cosmetic_slots, legacy_cosmetic_products, legacy_equivalent, shop_products, ActiveEffects,
    AvatarEquipment, AvatarEquipmentItem, AvatarSlot, CosmeticEquipResult, CosmeticPurchaseResult,
    CosmeticPurchaseStatus, CosmeticShopState, EquippedCosmetic, GuestCosmeticImport,
    LandscapeInstance, LandscapePlacement, QuoteTarget, ShopActionResult, ShopActionStatus,
    ShopCategory, ShopQuote, ShopRequest, ShopState,
};
use crate::domain::guest_shop_import::{
    GuestFirstResetReceipt, GuestFirstResetRequest, GuestFirstResetRequestKind,
    GuestFirstResetResult, GuestFirstResetStatus,
};
use crate::domain::landscape_geometry::{terrain_bounds, validate_placement, LandscapePoint};
use crate::domain::planet::{PlanetObject, PlanetWalletCredit};
use crate::domain::shop_effects::{capped_effects, discounted_price, weighted_growth};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};

fn active_account_id(connection: &Connection) -> Result<String, ScanError> {
    connection
        .query_row(
            "SELECT value FROM setting WHERE key='planet_account_id'",
            [],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

fn active_cycle_id(connection: &Connection) -> Result<String, ScanError> {
    connection
        .query_row(
            "SELECT value FROM setting WHERE key='planet_current_cycle_id'",
            [],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

fn available_balance(connection: &Connection, account_id: &str) -> Result<u64, ScanError> {
    let credits: i64 = connection.query_row(
        "SELECT coalesce(sum(amount), 0) FROM planet_wallet_credit",
        [],
        |row| row.get(0),
    )?;
    let purchases: i64 = connection.query_row(
        "SELECT coalesce(sum(price), 0) FROM cosmetic_purchase WHERE account_id=?1",
        [account_id],
        |row| row.get(0),
    )?;
    let balance = credits
        .checked_sub(purchases)
        .filter(|balance| *balance >= 0)
        .ok_or(ScanError::InvalidCount)?;
    u64::try_from(balance).map_err(|_| ScanError::InvalidCount)
}

fn to_i64(value: u64) -> Result<i64, ScanError> {
    i64::try_from(value).map_err(|_| ScanError::InvalidCount)
}

fn empty_avatar_equipment() -> AvatarEquipment {
    let empty = AvatarEquipmentItem {
        sku: None,
        version: 0,
    };
    AvatarEquipment {
        head: empty.clone(),
        outfit: empty.clone(),
        face: empty.clone(),
        back: empty,
    }
}

fn shop_available_balance(connection: &Connection, account_id: &str) -> Result<u64, ScanError> {
    let credits: i64 = connection.query_row(
        "SELECT coalesce(sum(amount),0) FROM planet_wallet_credit",
        [],
        |row| row.get(0),
    )?;
    let game_credits: i64 = connection.query_row(
        "SELECT coalesce(sum(amount),0) FROM shop_wallet_credit WHERE account_id=?1",
        [account_id],
        |row| row.get(0),
    )?;
    let purchases: i64 = connection.query_row(
        "SELECT
           (SELECT coalesce(sum(price),0) FROM shop_purchase WHERE account_id=?1)
           + (SELECT coalesce(sum(price),0) FROM cosmetic_purchase WHERE account_id=?1)",
        [account_id],
        |row| row.get(0),
    )?;
    let natural_removal_debits: i64 = connection.query_row(
        "SELECT coalesce(sum(amount),0) FROM shop_natural_removal_debit WHERE account_id=?1",
        [account_id],
        |row| row.get(0),
    )?;
    let available = credits
        .checked_add(game_credits)
        .and_then(|value| value.checked_sub(purchases))
        .and_then(|value| value.checked_sub(natural_removal_debits))
        .filter(|value| *value >= 0)
        .ok_or(ScanError::InvalidCount)?;
    u64::try_from(available).map_err(|_| ScanError::InvalidCount)
}

pub(crate) fn store_confirmed_shop_state_in_transaction(
    connection: &Connection,
    state: &ShopState,
) -> Result<(), ScanError> {
    let account_id = active_account_id(connection)?;
    if !account_id.starts_with("account:") || state.account_id != account_id {
        return Err(ScanError::InvalidShopState);
    }
    let prior_json: Option<String> = connection
        .query_row(
            "SELECT state_json FROM shop_remote_state WHERE account_id=?1",
            [&account_id],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(json) = prior_json {
        let prior: ShopState = serde_json::from_str(&json).map_err(|_| ScanError::Database)?;
        if prior.state_revision > state.state_revision {
            return Err(ScanError::InvalidShopState);
        }
    }
    let serialized = serde_json::to_string(state).map_err(|_| ScanError::Database)?;
    connection.execute(
        "INSERT INTO shop_remote_state(account_id,state_json,updated_at_utc)
         VALUES (?1,?2,?3) ON CONFLICT(account_id) DO UPDATE SET
           state_json=excluded.state_json,updated_at_utc=excluded.updated_at_utc",
        params![account_id, serialized, Utc::now().to_rfc3339()],
    )?;
    connection.execute(
        "INSERT INTO shop_account_state(account_id,state_revision,reward_timezone)
         VALUES (?1,?2,?3) ON CONFLICT(account_id) DO UPDATE SET
           state_revision=excluded.state_revision",
        params![
            account_id,
            to_i64(state.state_revision)?,
            state.reward_state.reward_timezone
        ],
    )?;
    Ok(())
}

impl Ledger {
    pub(crate) fn initialize_cosmetic_shop(&mut self) -> Result<(), ScanError> {
        self.connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS cosmetic_purchase (
                account_id TEXT NOT NULL,
                purchase_id TEXT NOT NULL,
                sku TEXT NOT NULL,
                price INTEGER NOT NULL CHECK(price >= 0),
                purchased_at_utc TEXT NOT NULL,
                PRIMARY KEY (account_id, purchase_id),
                UNIQUE (account_id, sku)
             );
             CREATE TABLE IF NOT EXISTS cosmetic_purchase_request (
                account_id TEXT NOT NULL,
                purchase_id TEXT NOT NULL,
                sku TEXT NOT NULL,
                result_json TEXT NOT NULL,
                PRIMARY KEY (account_id, purchase_id)
             );
             CREATE TABLE IF NOT EXISTS cosmetic_equipment (
                account_id TEXT NOT NULL,
                cycle_id TEXT NOT NULL,
                slot_id TEXT NOT NULL,
                sku TEXT,
                version INTEGER NOT NULL DEFAULT 0 CHECK(version >= 0),
                PRIMARY KEY (account_id, cycle_id, slot_id)
             );
             CREATE TABLE IF NOT EXISTS cosmetic_guest_import (
                account_id TEXT PRIMARY KEY,
                import_id TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                imported_at_utc TEXT
             );
             CREATE TABLE IF NOT EXISTS cosmetic_shop_remote_state (
                account_id TEXT PRIMARY KEY,
                state_json TEXT NOT NULL,
                updated_at_utc TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS cosmetic_pending_purchase (
                account_id TEXT NOT NULL,
                sku TEXT NOT NULL,
                purchase_id TEXT NOT NULL,
                catalog_revision INTEGER NOT NULL CHECK(catalog_revision > 0),
                created_at_utc TEXT NOT NULL,
                PRIMARY KEY (account_id, sku),
                UNIQUE (account_id, purchase_id)
             );
             CREATE TABLE IF NOT EXISTS shop_schema_version (
                id INTEGER PRIMARY KEY CHECK(id=1),
                version INTEGER NOT NULL CHECK(version >= 0)
             );
             CREATE TABLE IF NOT EXISTS shop_account_state (
                account_id TEXT PRIMARY KEY,
                state_revision INTEGER NOT NULL DEFAULT 0 CHECK(state_revision >= 0),
                reward_timezone TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS shop_purchase (
                account_id TEXT NOT NULL,
                purchase_id TEXT NOT NULL,
                sku TEXT NOT NULL,
                price INTEGER NOT NULL CHECK(price >= 0),
                purchased_at_utc TEXT NOT NULL,
                PRIMARY KEY(account_id,purchase_id)
             );
             CREATE TABLE IF NOT EXISTS shop_action_request (
                account_id TEXT NOT NULL,
                request_id TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                result_json TEXT,
                created_at_utc TEXT NOT NULL,
                PRIMARY KEY(account_id,request_id)
             );
             CREATE TABLE IF NOT EXISTS shop_landscape_instance (
                account_id TEXT NOT NULL,
                instance_id TEXT NOT NULL,
                sku TEXT NOT NULL,
                variation_index INTEGER NOT NULL CHECK(variation_index BETWEEN 0 AND 4),
                seed TEXT NOT NULL,
                variation_version INTEGER NOT NULL CHECK(variation_version > 0),
                acquired_at_utc TEXT NOT NULL,
                PRIMARY KEY(account_id,instance_id),
                UNIQUE(account_id,sku,variation_index)
             );
             CREATE INDEX IF NOT EXISTS shop_landscape_instance_by_sku
                ON shop_landscape_instance(account_id,sku);
             CREATE TABLE IF NOT EXISTS shop_landscape_placement (
                account_id TEXT NOT NULL,
                instance_id TEXT NOT NULL,
                cycle_id TEXT NOT NULL,
                x REAL NOT NULL,
                y REAL NOT NULL,
                version INTEGER NOT NULL DEFAULT 0 CHECK(version >= 0),
                PRIMARY KEY(account_id,instance_id),
                FOREIGN KEY(account_id,instance_id)
                  REFERENCES shop_landscape_instance(account_id,instance_id) ON DELETE CASCADE
             );
             CREATE TABLE IF NOT EXISTS shop_landscape_edit_version (
                account_id TEXT NOT NULL,
                instance_id TEXT NOT NULL,
                version INTEGER NOT NULL DEFAULT 0 CHECK(version >= 0),
                PRIMARY KEY(account_id,instance_id),
                FOREIGN KEY(account_id,instance_id)
                  REFERENCES shop_landscape_instance(account_id,instance_id) ON DELETE CASCADE
             );
             CREATE TABLE IF NOT EXISTS shop_avatar_owned (
                account_id TEXT NOT NULL,
                sku TEXT NOT NULL,
                purchase_id TEXT NOT NULL,
                price INTEGER NOT NULL CHECK(price >= 0),
                acquired_at_utc TEXT NOT NULL,
                PRIMARY KEY(account_id,sku),
                UNIQUE(account_id,purchase_id)
             );
             CREATE TABLE IF NOT EXISTS shop_avatar_equipment (
                account_id TEXT NOT NULL,
                slot TEXT NOT NULL CHECK(slot IN ('head','outfit','face','back')),
                sku TEXT,
                version INTEGER NOT NULL DEFAULT 0 CHECK(version >= 0),
                PRIMARY KEY(account_id,slot)
             );
             CREATE TABLE IF NOT EXISTS shop_natural_removal (
                account_id TEXT NOT NULL,
                cycle_id TEXT NOT NULL,
                stage INTEGER NOT NULL CHECK(stage BETWEEN 0 AND 4),
                ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
                version INTEGER NOT NULL DEFAULT 1 CHECK(version > 0),
                removed_at_utc TEXT NOT NULL,
                PRIMARY KEY(account_id,cycle_id,stage,ordinal)
             );
             CREATE TABLE IF NOT EXISTS shop_natural_removal_debit (
                account_id TEXT NOT NULL,
                request_id TEXT NOT NULL,
                amount INTEGER NOT NULL CHECK(amount >= 0),
                created_at_utc TEXT NOT NULL,
                PRIMARY KEY(account_id,request_id)
             );
             CREATE TABLE IF NOT EXISTS shop_remote_state (
                account_id TEXT PRIMARY KEY,
                state_json TEXT NOT NULL,
                updated_at_utc TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS guest_shop_import_capture (
                target_account_id TEXT PRIMARY KEY,
                import_id TEXT NOT NULL UNIQUE,
                source_fingerprint TEXT NOT NULL,
                snapshot_json TEXT NOT NULL,
                captured_at_utc TEXT NOT NULL
             );",
        )?;

        let schema_version: Option<i64> = self
            .connection
            .query_row(
                "SELECT version FROM shop_schema_version WHERE id=1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if schema_version.unwrap_or(0) < 1 {
            let transaction = self.connection.transaction()?;
            transaction.execute_batch(
                "DELETE FROM cosmetic_purchase;
                 DELETE FROM cosmetic_purchase_request;
                 DELETE FROM cosmetic_equipment;
                 DELETE FROM cosmetic_guest_import;
                 DELETE FROM cosmetic_shop_remote_state;
                 DELETE FROM cosmetic_pending_purchase;",
            )?;
            transaction.execute(
                "INSERT INTO shop_schema_version(id,version) VALUES (1,1)
                 ON CONFLICT(id) DO UPDATE SET version=excluded.version",
                [],
            )?;
            transaction.commit()?;
        }
        self.connection.execute(
            "INSERT OR IGNORE INTO shop_account_state(account_id,state_revision,reward_timezone)
             VALUES (?1,0,?2)",
            params![
                active_account_id(&self.connection)?,
                self.timezone.to_string()
            ],
        )?;
        Ok(())
    }

    pub fn cosmetic_account_id(&self) -> Result<String, ScanError> {
        active_account_id(&self.connection)
    }

    pub fn cosmetic_shop_state(&self) -> Result<CosmeticShopState, ScanError> {
        let account_id = active_account_id(&self.connection)?;
        let current_cycle_id = active_cycle_id(&self.connection)?;
        let actions_require_online = account_id.starts_with("account:");
        let guest_import_pending = self.pending_guest_cosmetic_import()?.is_some();
        if actions_require_online {
            let cached: Option<String> = self
                .connection
                .query_row(
                    "SELECT state_json FROM cosmetic_shop_remote_state WHERE account_id=?1",
                    [&account_id],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(cached) = cached {
                let mut state: CosmeticShopState =
                    serde_json::from_str(&cached).map_err(|_| ScanError::Database)?;
                if state.current_cycle_id != current_cycle_id {
                    state.current_cycle_id = current_cycle_id;
                    state.equipped.clear();
                    state.slot_versions.clear();
                }
                state.guest_import_pending = guest_import_pending;
                state.guest_import_error = None;
                return Ok(state);
            }
        }
        let slots = cosmetic_slots();
        let products = legacy_cosmetic_products();
        let mut owned_statement = self
            .connection
            .prepare("SELECT sku FROM cosmetic_purchase WHERE account_id=?1 ORDER BY sku")?;
        let owned_skus = owned_statement
            .query_map([&account_id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let mut equipped_statement = self.connection.prepare(
            "SELECT slot_id,sku,version FROM cosmetic_equipment
             WHERE account_id=?1 AND cycle_id=?2 AND sku IS NOT NULL ORDER BY slot_id",
        )?;
        let equipped = equipped_statement
            .query_map(params![account_id, current_cycle_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })?
            .map(|row| {
                let (slot_id, sku, version) = row?;
                Ok(EquippedCosmetic {
                    slot_id,
                    sku,
                    version: u64::try_from(version).map_err(|_| rusqlite::Error::InvalidQuery)?,
                })
            })
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;
        let equipped = equipped
            .into_iter()
            .filter(|equipped| {
                slots.iter().any(|slot| slot.slot_id == equipped.slot_id)
                    && products.iter().any(|product| {
                        product.slot_id == equipped.slot_id && product.sku == equipped.sku
                    })
            })
            .collect();
        let mut versions_statement = self.connection.prepare(
            "SELECT slot_id,version FROM cosmetic_equipment
             WHERE account_id=?1 AND cycle_id=?2 ORDER BY slot_id",
        )?;
        let slot_versions = versions_statement
            .query_map(params![account_id, current_cycle_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .map(|row| {
                let (slot_id, version) = row?;
                Ok((
                    slot_id,
                    u64::try_from(version).map_err(|_| rusqlite::Error::InvalidQuery)?,
                ))
            })
            .collect::<Result<_, rusqlite::Error>>()?;
        Ok(CosmeticShopState {
            slots,
            products,
            current_cycle_id: current_cycle_id.clone(),
            available_balance: if actions_require_online {
                0
            } else {
                available_balance(&self.connection, &account_id)?
            },
            owned_skus,
            equipped,
            slot_versions,
            actions_require_online,
            action_unavailable_reason: None,
            guest_import_pending,
            guest_import_error: None,
        })
    }

    pub fn shop_state(&self) -> Result<ShopState, ScanError> {
        let account_id = active_account_id(&self.connection)?;
        let current_cycle_id = active_cycle_id(&self.connection)?;
        let actions_require_online = account_id.starts_with("account:");
        let guest_import_pending = self.pending_guest_cosmetic_import()?.is_some();
        if actions_require_online {
            let cached: Option<String> = self
                .connection
                .query_row(
                    "SELECT state_json FROM shop_remote_state WHERE account_id=?1",
                    [&account_id],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(cached) = cached {
                let mut state: ShopState =
                    serde_json::from_str(&cached).map_err(|_| ScanError::Database)?;
                if state.account_id != account_id {
                    return Err(ScanError::InvalidShopState);
                }
                if state.current_cycle_id != current_cycle_id {
                    state.current_cycle_id = current_cycle_id;
                    state.placements.clear();
                    state.effects = ActiveEffects::default();
                    state.removed_natural_keys.clear();
                }
                state.guest_import_pending = guest_import_pending;
                return Ok(state);
            }
        }

        let products = shop_products();
        let mut instances_statement = self.connection.prepare(
            "SELECT i.instance_id,i.sku,i.variation_index,i.seed,i.variation_version,
                    coalesce(v.version,0)
             FROM shop_landscape_instance i
             LEFT JOIN shop_landscape_edit_version v
               ON v.account_id=i.account_id AND v.instance_id=i.instance_id
             WHERE i.account_id=?1 ORDER BY i.sku,i.variation_index",
        )?;
        let landscape_instances = instances_statement
            .query_map([&account_id], |row| {
                Ok(LandscapeInstance {
                    instance_id: row.get(0)?,
                    sku: row.get(1)?,
                    variation_index: row.get(2)?,
                    seed: row.get(3)?,
                    variation_version: row.get(4)?,
                    placement_version: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut placements_statement = self.connection.prepare(
            "SELECT instance_id,cycle_id,x,y,version FROM shop_landscape_placement
             WHERE account_id=?1 AND cycle_id=?2 ORDER BY instance_id",
        )?;
        let placements = placements_statement
            .query_map(params![account_id, current_cycle_id], |row| {
                Ok(LandscapePlacement {
                    instance_id: row.get(0)?,
                    cycle_id: row.get(1)?,
                    x: row.get(2)?,
                    y: row.get(3)?,
                    version: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let placed_ids = placements
            .iter()
            .map(|placement| placement.instance_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let active_instances = landscape_instances
            .iter()
            .filter(|instance| placed_ids.contains(instance.instance_id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        let effects = capped_effects(&products, &active_instances)
            .map_err(|_| ScanError::InvalidShopState)?;

        let mut avatar_owned_statement = self
            .connection
            .prepare("SELECT sku FROM shop_avatar_owned WHERE account_id=?1 ORDER BY sku")?;
        let avatar_owned_skus = avatar_owned_statement
            .query_map([&account_id], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        let mut avatar_equipment = empty_avatar_equipment();
        let mut equipment_statement = self.connection.prepare(
            "SELECT slot,sku,version FROM shop_avatar_equipment WHERE account_id=?1 ORDER BY slot",
        )?;
        for row in equipment_statement.query_map([&account_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })? {
            let (slot, sku, version) = row?;
            let item = AvatarEquipmentItem {
                sku,
                version: u64::try_from(version).map_err(|_| rusqlite::Error::InvalidQuery)?,
            };
            match slot.as_str() {
                "head" => avatar_equipment.head = item,
                "outfit" => avatar_equipment.outfit = item,
                "face" => avatar_equipment.face = item,
                "back" => avatar_equipment.back = item,
                _ => return Err(ScanError::InvalidShopState),
            }
        }
        let state_revision: Option<i64> = self
            .connection
            .query_row(
                "SELECT state_revision FROM shop_account_state WHERE account_id=?1",
                [&account_id],
                |row| row.get(0),
            )
            .optional()?;
        let reward_state = self.shop_reward_state(&account_id, &current_cycle_id)?;
        Ok(ShopState {
            account_id: account_id.clone(),
            current_cycle_id: current_cycle_id.clone(),
            catalog_revision: 1,
            state_revision: u64::try_from(state_revision.unwrap_or(0))
                .map_err(|_| ScanError::InvalidCount)?,
            available_balance: if actions_require_online {
                0
            } else {
                shop_available_balance(&self.connection, &account_id)?
            },
            products,
            landscape_instances,
            placements,
            removed_natural_keys: removed_natural_keys(
                &self.connection,
                &account_id,
                &current_cycle_id,
            )?,
            avatar_owned_skus,
            avatar_equipment,
            effects,
            reward_state,
            action_unavailable_reason: actions_require_online
                .then(|| "shop state requires an online confirmation".to_string()),
            guest_import_pending,
            guest_import_error: None,
        })
    }

    pub fn quote_shop(&self, target: &QuoteTarget) -> Result<ShopQuote, ScanError> {
        quote_for_state(&self.connection, target, &self.shop_state()?)
    }

    pub fn planet_removed_natural_keys(
        &self,
    ) -> Result<Vec<crate::domain::cosmetic_shop::NaturalObjectKey>, ScanError> {
        let account_id = active_account_id(&self.connection)?;
        let cycle_id = active_cycle_id(&self.connection)?;
        removed_natural_keys(&self.connection, &account_id, &cycle_id)
    }

    pub fn reset_guest_planet(
        &mut self,
        request_id: &str,
        cycle_id: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<ShopActionResult, ScanError> {
        self.apply_guest_shop_request(
            &ShopRequest::ResetPlanet {
                request_id: request_id.to_owned(),
                cycle_id: cycle_id.to_owned(),
            },
            now,
        )
    }

    fn apply_guest_reset_request(
        &mut self,
        request: &ShopRequest,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<ShopActionResult, ScanError> {
        let ShopRequest::ResetPlanet {
            request_id,
            cycle_id,
        } = request
        else {
            return Err(ScanError::InvalidShopState);
        };
        let account_id = active_account_id(&self.connection)?;
        if account_id.starts_with("account:") || request_id.trim().is_empty() {
            return Err(ScanError::InvalidShopState);
        }
        let eligible_guest_lineage: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM guest_provenance_lineage
             WHERE singleton=1 AND eligible=1)",
            [],
            |row| row.get(0),
        )?;
        let now = if eligible_guest_lineage {
            chrono::DateTime::from_timestamp_micros(now.timestamp_micros())
                .ok_or(ScanError::InvalidShopState)?
        } else {
            now
        };
        let now_utc = if eligible_guest_lineage {
            now.to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
        } else {
            now.to_rfc3339()
        };
        let payload_json =
            serde_json::to_string(request).map_err(|_| ScanError::InvalidShopState)?;
        if let Some(result) =
            replayed_guest_shop_action(&self.connection, &account_id, request_id, &payload_json)?
        {
            return Ok(result);
        }
        let current_cycle = active_cycle_id(&self.connection)?;
        if cycle_id != &current_cycle {
            return self.record_guest_reset_status(
                request_id,
                &payload_json,
                ShopActionStatus::CycleMismatch,
                now,
            );
        }
        if self
            .reset_available_at()?
            .is_some_and(|available| now < available)
        {
            return Err(ScanError::ResetCooldown);
        }

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let account_id = active_account_id(&transaction)?;
        if let Some(result) =
            replayed_guest_shop_action(&transaction, &account_id, request_id, &payload_json)?
        {
            return Ok(result);
        }
        if active_cycle_id(&transaction)? != *cycle_id {
            let result = ShopActionResult {
                status: ShopActionStatus::CycleMismatch,
                request_id: request_id.clone(),
                confirmed_quote: None,
                state: guest_shop_state(&transaction)?,
            };
            let result_json = serde_json::to_string(&result).map_err(|_| ScanError::Database)?;
            transaction.execute(
                "INSERT INTO shop_action_request(account_id,request_id,payload_json,result_json,created_at_utc)
                 VALUES (?1,?2,?3,?4,?5)",
                params![account_id,request_id,payload_json,result_json,now_utc],
            )?;
            transaction.commit()?;
            return Ok(result);
        }
        if reset_available_at_from(&transaction)?.is_some_and(|available| now < available) {
            return Err(ScanError::ResetCooldown);
        }
        Ledger::prepare_guest_import_source_in_transaction(&transaction, now)?;
        let current_tokens = current_guest_cycle_tokens_in_connection(&transaction, cycle_id)?;
        let frozen_deadline_before = reset_available_at_from(&transaction)?;
        let state_before_reset = guest_shop_state(&transaction)?;
        let record_zero_baseline = guest_first_reset_can_keep_zero_baseline(
            &transaction,
            &account_id,
            cycle_id,
            &state_before_reset,
            current_tokens,
            request_id,
            frozen_deadline_before.is_none(),
            now,
        )?;
        let reset_bps = state_before_reset.effects.reset_cooldown_bps.min(2_500);
        let cooldown_seconds = (86_400_i64 * i64::from(10_000 - reset_bps)) / 10_000;
        let reset_available_at = now + chrono::Duration::seconds(cooldown_seconds.max(64_800));
        let reset_available_at_utc = if eligible_guest_lineage {
            reset_available_at.to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
        } else {
            reset_available_at.to_rfc3339()
        };

        let new_cycle_id = uuid::Uuid::new_v4().to_string();
        transaction.execute(
            "INSERT OR IGNORE INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
             VALUES (?1,?2,?3)",
            params![cycle_id, to_i64(current_tokens)?, now_utc.as_str()],
        )?;
        let next_effect_revision: i64 = transaction.query_row(
            "SELECT coalesce(max(revision),0)+1 FROM shop_effect_history WHERE account_id=?1",
            [&account_id],
            |row| row.get(0),
        )?;
        if !record_zero_baseline {
            transaction.execute(
                "UPDATE shop_effect_history SET ended_at_utc=?3
                 WHERE account_id=?1 AND cycle_id=?2 AND ended_at_utc IS NULL",
                params![account_id, cycle_id, now_utc.as_str()],
            )?;
            let baseline_effects = serde_json::to_string(&ActiveEffects::default())
                .map_err(|_| ScanError::Database)?;
            transaction.execute(
                "INSERT INTO shop_effect_history(account_id,cycle_id,revision,started_at_utc,
                 active_instance_ids_json,effects_json) VALUES (?1,?2,?3,?4,'[]',?5)",
                params![
                    account_id,
                    new_cycle_id,
                    next_effect_revision,
                    now_utc.as_str(),
                    baseline_effects
                ],
            )?;
        }
        transaction.execute(
            "INSERT OR IGNORE INTO shop_landscape_edit_version(account_id,instance_id,version)
             SELECT account_id,instance_id,0 FROM shop_landscape_placement
             WHERE account_id=?1 AND cycle_id=?2",
            params![account_id, cycle_id],
        )?;
        transaction.execute(
            "UPDATE shop_landscape_edit_version SET version=version+1
             WHERE account_id=?1 AND instance_id IN (
               SELECT instance_id FROM shop_landscape_placement WHERE account_id=?1 AND cycle_id=?2
             )",
            params![account_id, cycle_id],
        )?;
        transaction.execute(
            "DELETE FROM shop_landscape_placement WHERE account_id=?1 AND cycle_id=?2",
            params![account_id, cycle_id],
        )?;
        transaction.execute("DELETE FROM planet_object WHERE cycle_id=?1", [cycle_id])?;
        transaction.execute(
            "DELETE FROM cosmetic_equipment WHERE account_id=?1 AND cycle_id=?2",
            params![account_id, cycle_id],
        )?;
        for (key, value) in [
            ("planet_last_reset_at_utc", now_utc.clone()),
            (
                "planet_reset_available_at_utc",
                reset_available_at_utc.clone(),
            ),
            ("planet_cycle_started_at_utc", now_utc.clone()),
            ("planet_current_cycle_id", new_cycle_id),
        ] {
            transaction.execute(
                "INSERT INTO setting(key,value) VALUES (?1,?2)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![key, value],
            )?;
        }
        transaction.execute(
            "DELETE FROM setting WHERE key IN ('planet_remote_cycle_id','planet_remote_current_tokens',
             'planet_remote_growth_credit','planet_remote_incomplete')",
            [],
        )?;
        increment_state_revision(&transaction, &account_id)?;
        Ledger::prepare_growth_journal_in_transaction(&transaction)?;
        let state = guest_shop_state(&transaction)?;
        let result = ShopActionResult {
            status: ShopActionStatus::Reset,
            request_id: request_id.clone(),
            confirmed_quote: None,
            state,
        };
        let result_json = serde_json::to_string(&result).map_err(|_| ScanError::Database)?;
        transaction.execute(
            "INSERT INTO shop_action_request(account_id,request_id,payload_json,result_json,created_at_utc)
             VALUES (?1,?2,?3,?4,?5)",
            params![account_id,request_id,payload_json,result_json,now_utc.as_str()],
        )?;
        let reset_receipt = GuestFirstResetReceipt {
            request: GuestFirstResetRequest {
                kind: GuestFirstResetRequestKind::ResetPlanet,
                request_id: request_id.clone(),
                cycle_id: cycle_id.clone(),
            },
            result: GuestFirstResetResult {
                status: GuestFirstResetStatus::Reset,
                request_id: request_id.clone(),
                previous_cycle_id: cycle_id.clone(),
                new_cycle_id: result.state.current_cycle_id.clone(),
                reset_at_utc: now_utc.clone(),
                final_effect_revision: if record_zero_baseline {
                    0
                } else {
                    u64::try_from(next_effect_revision).map_err(|_| ScanError::InvalidCount)?
                },
                final_active_instance_ids: Vec::new(),
                final_effects: result.state.effects.clone(),
                frozen_deadline_before_reset_utc: frozen_deadline_before
                    .map(|deadline| deadline.to_rfc3339_opts(chrono::SecondsFormat::Micros, true)),
                reset_available_at_utc: reset_available_at_utc.clone(),
                raw_tokens: current_tokens,
                bonus_tokens: 0,
                credited_tokens: current_tokens,
                shop_state_revision: result.state.state_revision,
            },
        };
        super::guest_provenance::record_guest_first_reset_in_transaction(
            &transaction,
            &reset_receipt,
        )?;
        transaction.commit()?;
        Ok(result)
    }

    fn record_guest_reset_status(
        &mut self,
        request_id: &str,
        payload_json: &str,
        status: ShopActionStatus,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<ShopActionResult, ScanError> {
        let transaction = self.connection.transaction()?;
        let account_id = active_account_id(&transaction)?;
        if let Some(result) =
            replayed_guest_shop_action(&transaction, &account_id, request_id, payload_json)?
        {
            return Ok(result);
        }
        let result = ShopActionResult {
            status,
            request_id: request_id.to_owned(),
            confirmed_quote: None,
            state: guest_shop_state(&transaction)?,
        };
        let result_json = serde_json::to_string(&result).map_err(|_| ScanError::Database)?;
        transaction.execute(
            "INSERT INTO shop_action_request(account_id,request_id,payload_json,result_json,created_at_utc)
             VALUES (?1,?2,?3,?4,?5)",
            params![account_id,request_id,payload_json,result_json,now.to_rfc3339()],
        )?;
        transaction.commit()?;
        Ok(result)
    }

    pub fn apply_guest_shop_request(
        &mut self,
        request: &ShopRequest,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<ShopActionResult, ScanError> {
        self.require_guest_import_game_mutations_allowed()?;
        if active_account_id(&self.connection)?.starts_with("account:") {
            return Err(ScanError::InvalidShopState);
        }
        let request_id = request_id(request).to_owned();
        if request_id.trim().is_empty() {
            return Err(ScanError::InvalidShopState);
        }
        if matches!(request, ShopRequest::ResetPlanet { .. }) {
            return self.apply_guest_reset_request(request, now);
        }
        let payload_json =
            serde_json::to_string(request).map_err(|_| ScanError::InvalidShopState)?;
        let transaction = self.connection.transaction()?;
        let account_id = active_account_id(&transaction)?;
        if let Some((prior_payload, prior_result)) = transaction.query_row(
            "SELECT payload_json,result_json FROM shop_action_request WHERE account_id=?1 AND request_id=?2",
            params![account_id, request_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        ).optional()? {
            if prior_payload != payload_json {
                let state = guest_shop_state(&transaction)?;
                return Ok(ShopActionResult { status: ShopActionStatus::RequestConflict, request_id, confirmed_quote: None, state });
            }
            let result = prior_result.ok_or(ScanError::InvalidShopState)?;
            let mut result: ShopActionResult = serde_json::from_str(&result).map_err(|_| ScanError::Database)?;
            result.state = guest_shop_state(&transaction)?;
            return Ok(result);
        }

        let status = match request {
            ShopRequest::Purchase { quote, .. } => {
                apply_guest_purchase(&transaction, &account_id, quote, &request_id, now)?
            }
            ShopRequest::Place {
                cycle_id,
                instance_id,
                expected_version,
                x,
                y,
                ..
            } => apply_guest_place(
                &transaction,
                &account_id,
                cycle_id,
                instance_id,
                *expected_version,
                *x,
                *y,
                now,
            )?,
            ShopRequest::Retrieve {
                cycle_id,
                instance_id,
                expected_version,
                ..
            } => apply_guest_retrieve(
                &transaction,
                &account_id,
                cycle_id,
                instance_id,
                *expected_version,
                now,
            )?,
            ShopRequest::EquipAvatar {
                slot,
                sku,
                expected_version,
                ..
            } => apply_guest_avatar(
                &transaction,
                &account_id,
                *slot,
                sku.as_deref(),
                *expected_version,
            )?,
            ShopRequest::RemoveNatural {
                key,
                expected_version,
                quote,
                ..
            } => apply_guest_natural_removal(
                &transaction,
                &account_id,
                key,
                *expected_version,
                quote,
                &request_id,
                now,
            )?,
            ShopRequest::ResetPlanet { .. } => ShopActionStatus::Unavailable,
        };
        let state = guest_shop_state(&transaction)?;
        let confirmed_quote = match request {
            ShopRequest::Purchase { quote, .. } | ShopRequest::RemoveNatural { quote, .. } => {
                Some(quote.clone())
            }
            _ => None,
        };
        let result = ShopActionResult {
            status,
            request_id: request_id.clone(),
            confirmed_quote,
            state,
        };
        let result_json = serde_json::to_string(&result).map_err(|_| ScanError::Database)?;
        transaction.execute(
            "INSERT INTO shop_action_request(account_id,request_id,payload_json,result_json,created_at_utc)
             VALUES (?1,?2,?3,?4,?5)",
            params![account_id, request_id, payload_json, result_json, now.to_rfc3339()],
        )?;
        transaction.commit()?;
        Ok(result)
    }

    pub fn store_confirmed_shop_state(&mut self, state: &ShopState) -> Result<(), ScanError> {
        self.require_guest_import_game_mutations_allowed()?;
        let transaction = self.connection.transaction()?;
        store_confirmed_shop_state_in_transaction(&transaction, state)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn pending_guest_cosmetic_import(&self) -> Result<Option<GuestCosmeticImport>, ScanError> {
        let account_id = active_account_id(&self.connection)?;
        let pending: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT import_id,payload_json FROM cosmetic_guest_import
                 WHERE account_id=?1 AND imported_at_utc IS NULL",
                [&account_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        pending
            .map(|(import_id, json)| {
                let mut payload: GuestCosmeticImport =
                    serde_json::from_str(&json).map_err(|_| ScanError::Database)?;
                payload.import_id = import_id;
                Ok(payload)
            })
            .transpose()
    }

    /// Returns true while a guest shop payload still needs a complete import.
    /// The legacy cosmetic importer is only a partial migration, so its
    /// imported marker does not stand in for the newer shop ownership state.
    pub fn has_unimported_guest_shop_state(&self) -> Result<bool, ScanError> {
        if !self.guest_import_game_mutations_allowed()? {
            return Ok(true);
        }
        let account_id = active_account_id(&self.connection)?;
        if !account_id.starts_with("account:") {
            return Ok(false);
        }
        if self.pending_guest_cosmetic_import()?.is_some() {
            return Ok(true);
        }

        self.has_local_guest_shop_state()
    }

    /// Checks guest shop ownership while the active planet account is still
    /// local, before first login can move local planet data to a signed account.
    pub fn has_local_guest_shop_state(&self) -> Result<bool, ScanError> {
        if !self.guest_import_game_mutations_allowed()? {
            return Ok(true);
        }
        let has_local_shop_rows: bool = self.connection.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM shop_account_state WHERE account_id='local' AND state_revision>0
                 UNION ALL SELECT 1 FROM shop_purchase WHERE account_id='local'
                 UNION ALL SELECT 1 FROM shop_action_request WHERE account_id='local'
                 UNION ALL SELECT 1 FROM shop_landscape_instance WHERE account_id='local'
                 UNION ALL SELECT 1 FROM shop_landscape_placement WHERE account_id='local'
                 UNION ALL SELECT 1 FROM shop_avatar_owned WHERE account_id='local'
                 UNION ALL SELECT 1 FROM shop_avatar_equipment
                   WHERE account_id='local' AND (sku IS NOT NULL OR version>0)
                 UNION ALL SELECT 1 FROM shop_natural_removal WHERE account_id='local'
                 UNION ALL SELECT 1 FROM shop_natural_removal_debit WHERE account_id='local'
                 UNION ALL SELECT 1 FROM shop_game_reward WHERE account_id='local'
                 UNION ALL SELECT 1 FROM shop_wallet_credit WHERE account_id='local' AND amount>0
                 UNION ALL SELECT 1 FROM shop_cycle_settlement WHERE account_id='local'
             )",
            [],
            |row| row.get(0),
        )?;
        if has_local_shop_rows {
            return Ok(true);
        }

        let mut statement = self.connection.prepare(
            "SELECT active_instance_ids_json,effects_json FROM shop_effect_history
             WHERE account_id='local'",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (active_ids_json, effects_json) = row?;
            let active_ids: Vec<String> =
                serde_json::from_str(&active_ids_json).map_err(|_| ScanError::Database)?;
            let effects: ActiveEffects =
                serde_json::from_str(&effects_json).map_err(|_| ScanError::Database)?;
            if !active_ids.is_empty() || effects != ActiveEffects::default() {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn store_confirmed_cosmetic_state(
        &mut self,
        state: &CosmeticShopState,
    ) -> Result<(), ScanError> {
        self.require_guest_import_game_mutations_allowed()?;
        let account_id = active_account_id(&self.connection)?;
        if !account_id.starts_with("account:") {
            return Err(ScanError::InvalidProfile);
        }
        let mut canonical = state.clone();
        canonical.actions_require_online = true;
        canonical.action_unavailable_reason = None;
        canonical.guest_import_pending = false;
        canonical.guest_import_error = None;
        let json = serde_json::to_string(&canonical).map_err(|_| ScanError::Database)?;
        self.connection.execute(
            "INSERT INTO cosmetic_shop_remote_state(account_id,state_json,updated_at_utc)
             VALUES (?1,?2,?3)
             ON CONFLICT(account_id) DO UPDATE SET state_json=excluded.state_json,
               updated_at_utc=excluded.updated_at_utc",
            params![account_id, json, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn prepare_cosmetic_purchase_request(
        &mut self,
        sku: &str,
        catalog_revision: u32,
    ) -> Result<String, ScanError> {
        self.require_guest_import_game_mutations_allowed()?;
        let account_id = active_account_id(&self.connection)?;
        if !account_id.starts_with("account:")
            || !legacy_cosmetic_products()
                .iter()
                .any(|product| product.sku == sku)
        {
            return Err(ScanError::InvalidProfile);
        }
        let transaction = self.connection.transaction()?;
        let existing: Option<String> = transaction
            .query_row(
                "SELECT purchase_id FROM cosmetic_pending_purchase
                 WHERE account_id=?1 AND sku=?2",
                params![account_id, sku],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(purchase_id) = existing {
            transaction.commit()?;
            return Ok(purchase_id);
        }
        let purchase_id = uuid::Uuid::new_v4().to_string();
        transaction.execute(
            "INSERT INTO cosmetic_pending_purchase(account_id,sku,purchase_id,catalog_revision,created_at_utc)
             VALUES (?1,?2,?3,?4,?5)",
            params![account_id, sku, purchase_id, catalog_revision, Utc::now().to_rfc3339()],
        )?;
        transaction.commit()?;
        Ok(purchase_id)
    }

    pub fn cancel_cosmetic_purchase_request(&mut self, purchase_id: &str) -> Result<(), ScanError> {
        self.require_guest_import_game_mutations_allowed()?;
        let account_id = active_account_id(&self.connection)?;
        self.connection.execute(
            "DELETE FROM cosmetic_pending_purchase WHERE account_id=?1 AND purchase_id=?2",
            params![account_id, purchase_id],
        )?;
        Ok(())
    }

    pub fn complete_cosmetic_purchase_request(
        &mut self,
        result: &CosmeticPurchaseResult,
    ) -> Result<(), ScanError> {
        self.require_guest_import_game_mutations_allowed()?;
        let account_id = active_account_id(&self.connection)?;
        if !account_id.starts_with("account:") {
            return Err(ScanError::InvalidProfile);
        }
        let transaction = self.connection.transaction()?;
        if result.status == CosmeticPurchaseStatus::Purchased {
            transaction.execute(
                "INSERT OR IGNORE INTO cosmetic_purchase(account_id,purchase_id,sku,price,purchased_at_utc)
                 VALUES (?1,?2,?3,?4,?5)",
                params![
                    account_id,
                    result.purchase_id,
                    result.sku,
                    to_i64(result.price)?,
                    Utc::now().to_rfc3339()
                ],
            )?;
        }
        let result_json = serde_json::to_string(result).map_err(|_| ScanError::Database)?;
        transaction.execute(
            "INSERT INTO cosmetic_purchase_request(account_id,purchase_id,sku,result_json)
             VALUES (?1,?2,?3,?4)
             ON CONFLICT(account_id,purchase_id) DO UPDATE SET result_json=excluded.result_json",
            params![account_id, result.purchase_id, result.sku, result_json],
        )?;
        transaction.execute(
            "DELETE FROM cosmetic_pending_purchase WHERE account_id=?1 AND purchase_id=?2",
            params![account_id, result.purchase_id],
        )?;
        let cached: Option<String> = transaction
            .query_row(
                "SELECT state_json FROM cosmetic_shop_remote_state WHERE account_id=?1",
                [&account_id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(cached) = cached {
            let mut state: CosmeticShopState =
                serde_json::from_str(&cached).map_err(|_| ScanError::Database)?;
            if result.status == CosmeticPurchaseStatus::Purchased
                && !state.owned_skus.iter().any(|sku| sku == &result.sku)
            {
                state.owned_skus.push(result.sku.clone());
                state.owned_skus.sort();
            }
            state.available_balance = result.available_balance;
            let json = serde_json::to_string(&state).map_err(|_| ScanError::Database)?;
            transaction.execute(
                "UPDATE cosmetic_shop_remote_state SET state_json=?2,updated_at_utc=?3 WHERE account_id=?1",
                params![account_id, json, Utc::now().to_rfc3339()],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn apply_confirmed_cosmetic_equipment(
        &mut self,
        result: &CosmeticEquipResult,
    ) -> Result<CosmeticShopState, ScanError> {
        self.require_guest_import_game_mutations_allowed()?;
        let mut state = self.cosmetic_shop_state()?;
        if state.current_cycle_id != result.cycle_id {
            state.current_cycle_id = result.cycle_id.clone();
            state.equipped.clear();
            state.slot_versions.clear();
        }
        state.equipped.retain(|item| item.slot_id != result.slot_id);
        if let Some(sku) = &result.sku {
            state.equipped.push(EquippedCosmetic {
                slot_id: result.slot_id.clone(),
                sku: sku.clone(),
                version: result.version,
            });
        }
        state
            .slot_versions
            .insert(result.slot_id.clone(), result.version);
        self.store_confirmed_cosmetic_state(&state)?;
        self.cosmetic_shop_state()
    }

    pub fn mark_guest_cosmetic_imported(
        &mut self,
        import_id: &str,
        state: &CosmeticShopState,
    ) -> Result<(), ScanError> {
        self.require_guest_import_game_mutations_allowed()?;
        let account_id = active_account_id(&self.connection)?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE cosmetic_guest_import SET imported_at_utc=?3
             WHERE account_id=?1 AND import_id=?2 AND imported_at_utc IS NULL",
            params![account_id, import_id, Utc::now().to_rfc3339()],
        )?;
        let mut canonical = state.clone();
        canonical.actions_require_online = true;
        canonical.action_unavailable_reason = None;
        canonical.guest_import_pending = false;
        canonical.guest_import_error = None;
        let json = serde_json::to_string(&canonical).map_err(|_| ScanError::Database)?;
        transaction.execute(
            "INSERT INTO cosmetic_shop_remote_state(account_id,state_json,updated_at_utc)
             VALUES (?1,?2,?3)
             ON CONFLICT(account_id) DO UPDATE SET state_json=excluded.state_json,
               updated_at_utc=excluded.updated_at_utc",
            params![account_id, json, Utc::now().to_rfc3339()],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn planet_wallet_credits_for_server_upload(
        &self,
    ) -> Result<Vec<PlanetWalletCredit>, ScanError> {
        if !self.guest_import_game_mutations_allowed()? {
            return Ok(Vec::new());
        }
        let Some(pending) = self.pending_guest_cosmetic_import()? else {
            return self.planet_wallet_credits();
        };
        let guest_credit_ids: BTreeSet<&str> = pending
            .wallet_credits
            .iter()
            .map(|credit| credit.previous_cycle_id.as_str())
            .collect();
        Ok(self
            .planet_wallet_credits()?
            .into_iter()
            .filter(|credit| !guest_credit_ids.contains(credit.previous_cycle_id.as_str()))
            .collect())
    }

    pub fn purchase_guest_cosmetic(
        &mut self,
        purchase_id: &str,
        sku: &str,
    ) -> Result<CosmeticPurchaseResult, ScanError> {
        self.require_guest_import_game_mutations_allowed()?;
        let transaction = self.connection.transaction()?;
        let result = purchase_in_transaction(&transaction, purchase_id, sku)?;
        transaction.commit()?;
        Ok(result)
    }

    pub fn equip_guest_cosmetic(
        &mut self,
        slot_id: &str,
        sku: Option<&str>,
    ) -> Result<u64, ScanError> {
        self.require_guest_import_game_mutations_allowed()?;
        if !cosmetic_slots().iter().any(|slot| slot.slot_id == slot_id) {
            return Err(ScanError::InvalidProfile);
        }
        let transaction = self.connection.transaction()?;
        let account_id = active_account_id(&transaction)?;
        let cycle_id = active_cycle_id(&transaction)?;
        if let Some(sku) = sku {
            let product = legacy_cosmetic_products()
                .into_iter()
                .find(|product| product.sku == sku && product.slot_id == slot_id)
                .ok_or(ScanError::InvalidProfile)?;
            let owned: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM cosmetic_purchase WHERE account_id=?1 AND sku=?2)",
                params![account_id, product.sku],
                |row| row.get(0),
            )?;
            if !owned {
                return Err(ScanError::InvalidProfile);
            }
        }
        let current_version: Option<i64> = transaction
            .query_row(
                "SELECT version FROM cosmetic_equipment
                 WHERE account_id=?1 AND cycle_id=?2 AND slot_id=?3",
                params![account_id, cycle_id, slot_id],
                |row| row.get(0),
            )
            .optional()?;
        let next_version = u64::try_from(current_version.unwrap_or(0))
            .map_err(|_| ScanError::Database)?
            .checked_add(1)
            .ok_or(ScanError::InvalidCount)?;
        transaction.execute(
            "INSERT INTO cosmetic_equipment(account_id,cycle_id,slot_id,sku,version)
             VALUES (?1,?2,?3,?4,?5)
             ON CONFLICT(account_id,cycle_id,slot_id) DO UPDATE SET
               sku=excluded.sku,version=excluded.version",
            params![account_id, cycle_id, slot_id, sku, to_i64(next_version)?],
        )?;
        transaction.commit()?;
        Ok(next_version)
    }
}

fn quote_for_state(
    connection: &Connection,
    target: &QuoteTarget,
    state: &ShopState,
) -> Result<ShopQuote, ScanError> {
    let effect_revision: i64 = connection.query_row(
        "SELECT coalesce(max(revision),0) FROM shop_effect_history WHERE account_id=?1",
        [&state.account_id],
        |row| row.get(0),
    )?;
    let price = match target {
        QuoteTarget::Purchase { sku } => {
            let product = state
                .products
                .iter()
                .find(|product| &product.sku == sku)
                .ok_or(ScanError::InvalidShopState)?;
            if !product.purchasable {
                return Err(ScanError::InvalidShopState);
            }
            discounted_price(product.price, state.effects.shop_discount_bps)
                .map_err(|_| ScanError::InvalidShopState)?
        }
        QuoteTarget::RemoveNatural { key } => {
            if key.cycle_id != state.current_cycle_id || key.stage > 4 {
                return Err(ScanError::InvalidShopState);
            }
            let object_exists: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM planet_object WHERE cycle_id=?1 AND stage=?2 AND ordinal=?3)",
                params![key.cycle_id,key.stage,key.ordinal], |row| row.get(0),
            )?;
            let removed: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM shop_natural_removal WHERE account_id=?1 AND cycle_id=?2 AND stage=?3 AND ordinal=?4)",
                params![state.account_id,key.cycle_id,key.stage,key.ordinal], |row| row.get(0),
            )?;
            if !object_exists || removed {
                return Err(ScanError::InvalidShopState);
            }
            const STAGE_PRICES: [u64; 5] = [100_000, 250_000, 500_000, 1_000_000, 2_000_000];
            discounted_price(
                STAGE_PRICES[usize::from(key.stage)],
                state.effects.natural_removal_discount_bps,
            )
            .map_err(|_| ScanError::InvalidShopState)?
        }
    };
    Ok(ShopQuote {
        target: target.clone(),
        catalog_revision: state.catalog_revision,
        effect_revision: u32::try_from(effect_revision).map_err(|_| ScanError::InvalidCount)?
            as u64,
        price,
    })
}

fn removed_natural_keys(
    connection: &Connection,
    account_id: &str,
    cycle_id: &str,
) -> Result<Vec<crate::domain::cosmetic_shop::NaturalObjectKey>, ScanError> {
    let mut statement = connection.prepare(
        "SELECT cycle_id,stage,ordinal FROM shop_natural_removal
         WHERE account_id=?1 AND cycle_id=?2 ORDER BY stage,ordinal",
    )?;
    let rows = statement
        .query_map(params![account_id, cycle_id], |row| {
            Ok(crate::domain::cosmetic_shop::NaturalObjectKey {
                cycle_id: row.get(0)?,
                stage: row.get(1)?,
                ordinal: row.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into);
    rows
}

fn guest_shop_state(connection: &Connection) -> Result<ShopState, ScanError> {
    let account_id = active_account_id(connection)?;
    let current_cycle_id = active_cycle_id(connection)?;
    let products = shop_products();
    let mut instances_statement = connection.prepare(
        "SELECT i.instance_id,i.sku,i.variation_index,i.seed,i.variation_version,
                coalesce(v.version,0)
         FROM shop_landscape_instance i
         LEFT JOIN shop_landscape_edit_version v
           ON v.account_id=i.account_id AND v.instance_id=i.instance_id
         WHERE i.account_id=?1 ORDER BY i.sku,i.variation_index",
    )?;
    let landscape_instances = instances_statement
        .query_map([&account_id], |row| {
            Ok(LandscapeInstance {
                instance_id: row.get(0)?,
                sku: row.get(1)?,
                variation_index: row.get(2)?,
                seed: row.get(3)?,
                variation_version: row.get(4)?,
                placement_version: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut placements_statement = connection.prepare(
        "SELECT instance_id,cycle_id,x,y,version FROM shop_landscape_placement
         WHERE account_id=?1 AND cycle_id=?2 ORDER BY instance_id",
    )?;
    let placements = placements_statement
        .query_map(params![account_id, current_cycle_id], |row| {
            Ok(LandscapePlacement {
                instance_id: row.get(0)?,
                cycle_id: row.get(1)?,
                x: row.get(2)?,
                y: row.get(3)?,
                version: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let placed_ids = placements
        .iter()
        .map(|placement| placement.instance_id.as_str())
        .collect::<BTreeSet<_>>();
    let active_instances = landscape_instances
        .iter()
        .filter(|instance| placed_ids.contains(instance.instance_id.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let effects =
        capped_effects(&products, &active_instances).map_err(|_| ScanError::InvalidShopState)?;

    let mut avatar_owned_statement =
        connection.prepare("SELECT sku FROM shop_avatar_owned WHERE account_id=?1 ORDER BY sku")?;
    let avatar_owned_skus = avatar_owned_statement
        .query_map([&account_id], |row| row.get(0))?
        .collect::<Result<Vec<String>, _>>()?;
    let mut avatar_equipment = empty_avatar_equipment();
    let mut equipment_statement = connection.prepare(
        "SELECT slot,sku,version FROM shop_avatar_equipment WHERE account_id=?1 ORDER BY slot",
    )?;
    for row in equipment_statement.query_map([&account_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })? {
        let (slot, sku, version) = row?;
        let item = AvatarEquipmentItem {
            sku,
            version: u64::try_from(version).map_err(|_| rusqlite::Error::InvalidQuery)?,
        };
        match slot.as_str() {
            "head" => avatar_equipment.head = item,
            "outfit" => avatar_equipment.outfit = item,
            "face" => avatar_equipment.face = item,
            "back" => avatar_equipment.back = item,
            _ => return Err(ScanError::InvalidShopState),
        }
    }
    let state_revision: Option<i64> = connection
        .query_row(
            "SELECT state_revision FROM shop_account_state WHERE account_id=?1",
            [&account_id],
            |row| row.get(0),
        )
        .optional()?;
    let guest_import_pending: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM cosmetic_guest_import WHERE account_id=?1 AND imported_at_utc IS NULL)",
        [&account_id], |row| row.get(0),
    )?;
    Ok(ShopState {
        account_id: account_id.clone(),
        current_cycle_id: current_cycle_id.clone(),
        catalog_revision: 1,
        state_revision: u64::try_from(state_revision.unwrap_or(0))
            .map_err(|_| ScanError::InvalidCount)?,
        available_balance: shop_available_balance(connection, &account_id)?,
        products,
        landscape_instances,
        placements,
        removed_natural_keys: removed_natural_keys(connection, &account_id, &current_cycle_id)?,
        avatar_owned_skus,
        avatar_equipment,
        effects,
        reward_state: load_shop_reward_state(connection, &account_id, &current_cycle_id)?,
        action_unavailable_reason: None,
        guest_import_pending,
        guest_import_error: None,
    })
}

fn request_id(request: &ShopRequest) -> &str {
    match request {
        ShopRequest::Purchase { request_id, .. }
        | ShopRequest::Place { request_id, .. }
        | ShopRequest::Retrieve { request_id, .. }
        | ShopRequest::EquipAvatar { request_id, .. }
        | ShopRequest::RemoveNatural { request_id, .. }
        | ShopRequest::ResetPlanet { request_id, .. } => request_id,
    }
}

fn replayed_guest_shop_action(
    connection: &Connection,
    account_id: &str,
    request_id: &str,
    payload_json: &str,
) -> Result<Option<ShopActionResult>, ScanError> {
    let prior: Option<(String,Option<String>)> = connection.query_row(
        "SELECT payload_json,result_json FROM shop_action_request WHERE account_id=?1 AND request_id=?2",
        params![account_id,request_id],
        |row| Ok((row.get(0)?,row.get(1)?)),
    ).optional()?;
    let Some((prior_payload, prior_result)) = prior else {
        return Ok(None);
    };
    if prior_payload != payload_json {
        return Ok(Some(ShopActionResult {
            status: ShopActionStatus::RequestConflict,
            request_id: request_id.to_owned(),
            confirmed_quote: None,
            state: guest_shop_state(connection)?,
        }));
    }
    let mut result: ShopActionResult =
        serde_json::from_str(&prior_result.ok_or(ScanError::InvalidShopState)?)
            .map_err(|_| ScanError::Database)?;
    result.state = guest_shop_state(connection)?;
    Ok(Some(result))
}

fn guest_first_reset_can_keep_zero_baseline(
    connection: &Connection,
    account_id: &str,
    cycle_id: &str,
    state: &ShopState,
    current_tokens: u64,
    request_id: &str,
    no_prior_deadline: bool,
    reset_at: chrono::DateTime<Utc>,
) -> Result<bool, ScanError> {
    let lineage: Option<(String, String, String)> = connection
        .query_row(
            "SELECT lineage_id,device_id,activated_at_utc
             FROM guest_provenance_lineage WHERE singleton=1 AND eligible=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((lineage_id, device_id, activated_at)) = lineage else {
        return Ok(false);
    };
    let baseline: Option<(String, Option<String>, i64, String, String)> = connection
        .query_row(
            "SELECT started_at_utc,ended_at_utc,baseline_revision,
                    active_instance_ids_json,effects_json
             FROM guest_provenance_cycle WHERE cycle_id=?1 AND lineage_id=?2",
            params![cycle_id, lineage_id],
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
    let Some((started_at, ended_at, revision, active_ids, effects_json)) = baseline else {
        return Ok(false);
    };
    let expected_effects =
        serde_json::to_string(&ActiveEffects::default()).map_err(|_| ScanError::Database)?;
    let active_cycle = active_cycle_id(connection)?;
    let current_device: String = connection.query_row(
        "SELECT value FROM setting WHERE key='planet_device_id'",
        [],
        |row| row.get(0),
    )?;
    let cycle_started = connection.query_row(
        "SELECT value FROM setting WHERE key='planet_cycle_started_at_utc'",
        [],
        |row| row.get::<_, String>(0),
    )?;
    let activation_setting = connection.query_row(
        "SELECT value FROM setting WHERE key='planet_activation_at_utc'",
        [],
        |row| row.get::<_, String>(0),
    )?;
    let prior_reset: Option<String> = connection
        .query_row(
            "SELECT value FROM setting WHERE key='planet_last_reset_at_utc'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let prior_receipt: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM guest_provenance_reset_receipt WHERE lineage_id=?1)",
        [&lineage_id],
        |row| row.get(0),
    )?;
    if !super::guest_provenance::reset_time_covers_guest_occurrences(
        connection,
        &lineage_id,
        cycle_id,
        reset_at,
    )? {
        return Ok(false);
    }
    if ended_at.is_some()
        || revision != 0
        || active_ids != "[]"
        || effects_json != expected_effects
        || active_cycle != cycle_id
        || started_at != activated_at
        || cycle_started != started_at
        || activation_setting != started_at
        || prior_reset.is_some()
        || prior_receipt
        || !no_prior_deadline
        || device_id != current_device
        || uuid::Uuid::parse_str(&device_id).map_or(true, |value| value.is_nil())
        || uuid::Uuid::parse_str(request_id).map_or(true, |value| {
            value.is_nil() || value.to_string() != request_id
        })
        || account_id != "local"
        || state.account_id != account_id
        || state.current_cycle_id != cycle_id
        || current_tokens == 0
        || state.effects != ActiveEffects::default()
        || state.available_balance != 0
        || !state.landscape_instances.is_empty()
        || !state.placements.is_empty()
        || !state.removed_natural_keys.is_empty()
        || !state.avatar_owned_skus.is_empty()
        || state.avatar_equipment.head.sku.is_some()
        || state.avatar_equipment.outfit.sku.is_some()
        || state.avatar_equipment.face.sku.is_some()
        || state.avatar_equipment.back.sku.is_some()
        || state.reward_state.settled_cycle_tokens != 0
        || state.reward_state.era_reward_tokens != 0
        || state.reward_state.streak_reward_tokens != 0
    {
        return Ok(false);
    }

    let unsupported_state: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM shop_effect_history WHERE account_id=?1)
           OR EXISTS(SELECT 1 FROM shop_effect_cycle_bound WHERE account_id=?1)
           OR EXISTS(SELECT 1 FROM shop_effect_cycle_bounds_state WHERE account_id=?1)
           OR EXISTS(SELECT 1 FROM shop_effect_timeline_state WHERE account_id=?1)
           OR EXISTS(SELECT 1 FROM cosmetic_purchase WHERE account_id=?1)
           OR EXISTS(SELECT 1 FROM cosmetic_purchase_request WHERE account_id=?1)
           OR EXISTS(SELECT 1 FROM shop_purchase WHERE account_id=?1)
           OR EXISTS(SELECT 1 FROM shop_action_request WHERE account_id=?1)
           OR EXISTS(SELECT 1 FROM shop_landscape_instance WHERE account_id=?1)
           OR EXISTS(SELECT 1 FROM shop_landscape_placement WHERE account_id=?1)
           OR EXISTS(SELECT 1 FROM shop_avatar_owned WHERE account_id=?1)
           OR EXISTS(SELECT 1 FROM shop_avatar_equipment WHERE account_id=?1 AND sku IS NOT NULL)
           OR EXISTS(SELECT 1 FROM cosmetic_equipment WHERE account_id=?1 AND sku IS NOT NULL)
           OR EXISTS(SELECT 1 FROM shop_natural_removal WHERE account_id=?1)
           OR EXISTS(SELECT 1 FROM shop_natural_removal_debit WHERE account_id=?1)
           OR EXISTS(SELECT 1 FROM shop_game_reward WHERE account_id=?1)
           OR EXISTS(SELECT 1 FROM shop_wallet_credit WHERE account_id=?1)
           OR EXISTS(SELECT 1 FROM shop_era_progress WHERE account_id=?1)
           OR EXISTS(SELECT 1 FROM cosmetic_pending_purchase WHERE account_id=?1)
           OR EXISTS(SELECT 1 FROM cosmetic_guest_import WHERE account_id=?1)
           OR EXISTS(SELECT 1 FROM cosmetic_shop_remote_state WHERE account_id=?1)
           OR EXISTS(SELECT 1 FROM shop_remote_state WHERE account_id=?1)
           OR EXISTS(SELECT 1 FROM planet_object)
           OR EXISTS(SELECT 1 FROM planet_wallet_credit)
           OR EXISTS(SELECT 1 FROM shop_cycle_settlement
                     WHERE account_id=?1 AND amount<>0)",
        [account_id],
        |row| row.get(0),
    )?;
    let remote_state: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM setting WHERE key IN
          ('planet_remote_cycle_id','planet_remote_current_tokens',
           'planet_remote_growth_credit','planet_remote_incomplete'))",
        [],
        |row| row.get(0),
    )?;
    if unsupported_state || remote_state {
        return Ok(false);
    }

    let mut statement = connection.prepare(
        "SELECT date,tokens,growth_bps,wallet_bps FROM shop_effect_contribution
         WHERE account_id=?1 AND cycle_id=?2 ORDER BY date,device_id,effect_revision",
    )?;
    let rows = statement.query_map(params![account_id, cycle_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })?;
    let mut last_date = None;
    let mut date_total = 0_u64;
    let mut growth = 0.0_f64;
    for row in rows {
        let (date, tokens, growth_bps, wallet_bps) = row?;
        if growth_bps != 0 || wallet_bps != 0 || tokens < 0 {
            return Ok(false);
        }
        if last_date
            .as_deref()
            .is_some_and(|previous| previous != date)
        {
            growth += weighted_growth(date_total, &[]).map_err(|_| ScanError::InvalidShopState)?;
            if growth >= crate::growth::STAGE_THRESHOLDS[0] {
                return Ok(false);
            }
            date_total = 0;
        }
        date_total = date_total
            .checked_add(u64::try_from(tokens).map_err(|_| ScanError::InvalidCount)?)
            .ok_or(ScanError::InvalidCount)?;
        last_date = Some(date);
    }
    if last_date.is_some() {
        growth += weighted_growth(date_total, &[]).map_err(|_| ScanError::InvalidShopState)?;
    }
    Ok(growth < crate::growth::STAGE_THRESHOLDS[0])
}

fn current_guest_cycle_tokens_in_connection(
    connection: &Connection,
    cycle_id: &str,
) -> Result<u64, ScanError> {
    let account_id = active_account_id(connection)?;
    let activation: String = connection.query_row(
        "SELECT value FROM setting WHERE key='planet_activation_at_utc'",
        [],
        |row| row.get(0),
    )?;
    let last_reset: Option<String> = connection
        .query_row(
            "SELECT value FROM setting WHERE key='planet_last_reset_at_utc'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let activation_time = chrono::DateTime::parse_from_rfc3339(&activation)
        .map_err(|_| ScanError::Database)?
        .with_timezone(&Utc);
    let cycle_start = last_reset
        .as_deref()
        .map(chrono::DateTime::parse_from_rfc3339)
        .transpose()
        .map_err(|_| ScanError::Database)?
        .map(|value| value.with_timezone(&Utc))
        .unwrap_or(activation_time)
        .max(activation_time);
    let inclusive_cycle_boundary: bool = connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM guest_provenance_lineage l
           JOIN guest_provenance_cycle b ON b.lineage_id=l.lineage_id
           WHERE l.singleton=1 AND l.eligible=1 AND l.device_id=(
             SELECT value FROM setting WHERE key='planet_device_id'
           ) AND b.cycle_id=(SELECT value FROM setting WHERE key='planet_current_cycle_id')
             AND b.started_at_utc=(SELECT value FROM setting WHERE key='planet_cycle_started_at_utc')
             AND (SELECT value FROM setting WHERE key='planet_account_id')='local'
         )",
        [],
        |row| row.get(0),
    )?;
    let codex_enabled: Option<String> = connection
        .query_row(
            "SELECT value FROM setting WHERE key='codex_enabled'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let claude_enabled: Option<String> = connection
        .query_row(
            "SELECT value FROM setting WHERE key='claude_code_enabled'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let mut statement = connection.prepare(
        "SELECT r.occurred_at_utc,r.total_tokens FROM usage_record r
         WHERE r.total_tokens IS NOT NULL
           AND (r.occurred_at_utc>?1 OR (?2 AND substr(r.occurred_at_utc,1,19)=substr(?1,1,19)))
           AND ((r.agent='codex' AND ?3) OR (r.agent='claude_code' AND ?4))
           AND EXISTS(SELECT 1 FROM planet_usage_owner o
                      WHERE o.event_key=r.event_key AND o.account_id=?5)
           AND (r.kind='response' OR NOT EXISTS(
             SELECT 1 FROM usage_record other
             WHERE other.source_id=r.source_id AND other.kind='response' AND other.agent=r.agent
               AND EXISTS(SELECT 1 FROM planet_usage_owner o
                          WHERE o.event_key=other.event_key AND o.account_id=?5)
           ))
         ORDER BY r.occurred_at_utc",
    )?;
    let rows = statement.query_map(
        params![
            activation,
            inclusive_cycle_boundary,
            codex_enabled.as_deref() != Some("false"),
            claude_enabled.as_deref() != Some("false"),
            account_id,
        ],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
    )?;
    let mut local_tokens = 0_u64;
    for row in rows {
        let (occurred_at, tokens) = row?;
        let occurred_at = chrono::DateTime::parse_from_rfc3339(&occurred_at)
            .map_err(|_| ScanError::Database)?
            .with_timezone(&Utc);
        if occurred_at < activation_time
            || (!inclusive_cycle_boundary && occurred_at == activation_time)
        {
            continue;
        }
        let in_cycle = if inclusive_cycle_boundary {
            occurred_at >= cycle_start
        } else {
            occurred_at > cycle_start
        };
        if !in_cycle {
            continue;
        }
        local_tokens = local_tokens
            .checked_add(u64::try_from(tokens).map_err(|_| ScanError::InvalidCount)?)
            .ok_or(ScanError::InvalidCount)?;
    }
    let remote_cycle: Option<String> = connection
        .query_row(
            "SELECT value FROM setting WHERE key='planet_remote_cycle_id'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if remote_cycle.as_deref() == Some(cycle_id) {
        let remote_tokens: String = connection.query_row(
            "SELECT value FROM setting WHERE key='planet_remote_current_tokens'",
            [],
            |row| row.get(0),
        )?;
        let remote_tokens = remote_tokens.parse().map_err(|_| ScanError::Database)?;
        return Ok(local_tokens.max(remote_tokens));
    }
    Ok(local_tokens)
}

fn reset_available_at_from(
    connection: &Connection,
) -> Result<Option<chrono::DateTime<chrono::Utc>>, ScanError> {
    let frozen: Option<String> = connection
        .query_row(
            "SELECT value FROM setting WHERE key='planet_reset_available_at_utc'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(value) = frozen {
        return chrono::DateTime::parse_from_rfc3339(&value)
            .map(|time| Some(time.with_timezone(&chrono::Utc)))
            .map_err(|_| ScanError::Database);
    }
    let last: Option<String> = connection
        .query_row(
            "SELECT value FROM setting WHERE key='planet_last_reset_at_utc'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    last.map(|value| {
        chrono::DateTime::parse_from_rfc3339(&value)
            .map(|time| time.with_timezone(&chrono::Utc) + chrono::Duration::hours(24))
            .map_err(|_| ScanError::Database)
    })
    .transpose()
}

fn increment_state_revision(connection: &Connection, account_id: &str) -> Result<(), ScanError> {
    let changed = connection.execute(
        "UPDATE shop_account_state SET state_revision=state_revision+1 WHERE account_id=?1",
        [account_id],
    )?;
    if changed != 1 {
        return Err(ScanError::InvalidShopState);
    }
    Ok(())
}

fn apply_guest_natural_removal(
    connection: &Connection,
    account_id: &str,
    key: &crate::domain::cosmetic_shop::NaturalObjectKey,
    expected_version: u64,
    quoted: &ShopQuote,
    request_id: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<ShopActionStatus, ScanError> {
    let state = guest_shop_state(connection)?;
    if key.cycle_id != state.current_cycle_id {
        return Ok(ShopActionStatus::CycleMismatch);
    }
    let QuoteTarget::RemoveNatural { key: quoted_key } = &quoted.target else {
        return Ok(ShopActionStatus::QuoteChanged);
    };
    if quoted_key != key {
        return Ok(ShopActionStatus::QuoteChanged);
    }
    if state.removed_natural_keys.contains(key) {
        return Ok(ShopActionStatus::AlreadyRemoved);
    }
    if expected_version != 0 {
        return Ok(ShopActionStatus::VersionConflict);
    }
    let object_exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM planet_object WHERE cycle_id=?1 AND stage=?2 AND ordinal=?3)",
        params![key.cycle_id, key.stage, key.ordinal],
        |row| row.get(0),
    )?;
    if !object_exists {
        return Ok(ShopActionStatus::NotOwned);
    }
    let current_quote = quote_for_state(connection, &quoted.target, &state)?;
    if quoted.catalog_revision != state.catalog_revision {
        return Ok(ShopActionStatus::CatalogMismatch);
    }
    if *quoted != current_quote {
        return Ok(ShopActionStatus::QuoteChanged);
    }
    let balance = shop_available_balance(connection, account_id)?;
    if balance < quoted.price {
        return Ok(ShopActionStatus::InsufficientBalance);
    }
    connection.execute(
        "INSERT INTO shop_natural_removal(account_id,cycle_id,stage,ordinal,version,removed_at_utc)
         VALUES (?1,?2,?3,?4,1,?5)",
        params![
            account_id,
            key.cycle_id,
            key.stage,
            key.ordinal,
            now.to_rfc3339()
        ],
    )?;
    connection.execute(
        "INSERT INTO shop_natural_removal_debit(account_id,request_id,amount,created_at_utc)
         VALUES (?1,?2,?3,?4)",
        params![
            account_id,
            request_id,
            to_i64(quoted.price)?,
            now.to_rfc3339()
        ],
    )?;
    increment_state_revision(connection, account_id)?;
    Ok(ShopActionStatus::Removed)
}

fn apply_guest_purchase(
    connection: &Connection,
    account_id: &str,
    quoted: &ShopQuote,
    request_id: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<ShopActionStatus, ScanError> {
    let state = guest_shop_state(connection)?;
    let current_quote = quote_for_state(connection, &quoted.target, &state)?;
    if quoted.catalog_revision != state.catalog_revision {
        return Ok(ShopActionStatus::CatalogMismatch);
    }
    if *quoted != current_quote {
        return Ok(ShopActionStatus::QuoteChanged);
    }
    let QuoteTarget::Purchase { sku } = &quoted.target else {
        return Ok(ShopActionStatus::Unavailable);
    };
    let product = state
        .products
        .iter()
        .find(|product| &product.sku == sku)
        .ok_or(ScanError::InvalidShopState)?;
    let (owned, count) = match product.category {
        ShopCategory::Landscape => {
            let count: i64 = connection.query_row(
                "SELECT count(*) FROM shop_landscape_instance WHERE account_id=?1 AND sku=?2",
                params![account_id, sku],
                |row| row.get(0),
            )?;
            (false, count)
        }
        ShopCategory::Avatar => {
            let owned: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM shop_avatar_owned WHERE account_id=?1 AND sku=?2)",
                params![account_id, sku],
                |row| row.get(0),
            )?;
            (owned, 0)
        }
    };
    if owned {
        return Ok(ShopActionStatus::AlreadyOwned);
    }
    if product.category == ShopCategory::Landscape && count >= 5 {
        return Ok(ShopActionStatus::LimitReached);
    }
    let balance = shop_available_balance(connection, account_id)?;
    if balance < quoted.price {
        return Ok(ShopActionStatus::InsufficientBalance);
    }
    let new_balance = balance
        .checked_sub(quoted.price)
        .ok_or(ScanError::InvalidCount)?;
    let variation = if product.category == ShopCategory::Landscape {
        let mut statement = connection.prepare(
            "SELECT variation_index FROM shop_landscape_instance WHERE account_id=?1 AND sku=?2",
        )?;
        let used = statement
            .query_map(params![account_id, sku], |row| row.get::<_, u8>(0))?
            .collect::<Result<BTreeSet<_>, _>>()?;
        (0_u8..5)
            .find(|index| !used.contains(index))
            .ok_or(ScanError::InvalidShopState)?
    } else {
        0
    };
    connection.execute(
        "INSERT INTO shop_purchase(account_id,purchase_id,sku,price,purchased_at_utc)
         VALUES (?1,?2,?3,?4,?5)",
        params![
            account_id,
            request_id,
            sku,
            to_i64(quoted.price)?,
            now.to_rfc3339()
        ],
    )?;
    if product.category == ShopCategory::Landscape {
        let instance_id = uuid::Uuid::new_v4().to_string();
        let seed = uuid::Uuid::new_v4().to_string();
        connection.execute(
            "INSERT INTO shop_landscape_instance(account_id,instance_id,sku,variation_index,seed,variation_version,acquired_at_utc)
             VALUES (?1,?2,?3,?4,?5,1,?6)",
            params![account_id,instance_id,sku,variation,seed,now.to_rfc3339()],
        )?;
        connection.execute(
            "INSERT INTO shop_landscape_edit_version(account_id,instance_id,version) VALUES (?1,?2,0)",
            params![account_id,instance_id],
        )?;
    } else {
        connection.execute(
            "INSERT INTO shop_avatar_owned(account_id,sku,purchase_id,price,acquired_at_utc)
             VALUES (?1,?2,?3,?4,?5)",
            params![
                account_id,
                sku,
                request_id,
                to_i64(quoted.price)?,
                now.to_rfc3339()
            ],
        )?;
    }
    let _ = new_balance;
    increment_state_revision(connection, account_id)?;
    Ok(ShopActionStatus::Purchased)
}

fn current_natural_objects(
    connection: &Connection,
    cycle_id: &str,
) -> Result<Vec<PlanetObject>, ScanError> {
    let mut statement = connection.prepare(
        "SELECT stage,ordinal,kind,x,y,seed FROM planet_object WHERE cycle_id=?1 ORDER BY stage,ordinal",
    )?;
    let rows = statement
        .query_map([cycle_id], |row| {
            let seed: String = row.get(5)?;
            Ok(PlanetObject {
                stage: row.get(0)?,
                ordinal: row.get(1)?,
                kind: row.get(2)?,
                x: row.get(3)?,
                y: row.get(4)?,
                seed: seed.parse().map_err(|_| rusqlite::Error::InvalidQuery)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into);
    rows
}

fn active_instance_ids(
    connection: &Connection,
    account_id: &str,
    cycle_id: &str,
) -> Result<Vec<String>, ScanError> {
    let mut statement = connection.prepare(
        "SELECT p.instance_id FROM shop_landscape_placement p
         JOIN shop_landscape_instance i ON i.account_id=p.account_id AND i.instance_id=p.instance_id
         WHERE p.account_id=?1 AND p.cycle_id=?2 ORDER BY p.instance_id",
    )?;
    let rows = statement
        .query_map(params![account_id, cycle_id], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into);
    rows
}

fn record_effect_change(
    connection: &Connection,
    account_id: &str,
    cycle_id: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), ScanError> {
    let active_ids = active_instance_ids(connection, account_id, cycle_id)?;
    let active_json = serde_json::to_string(&active_ids).map_err(|_| ScanError::Database)?;
    let latest: Option<(i64, String)> = connection
        .query_row(
            "SELECT revision,active_instance_ids_json FROM shop_effect_history
         WHERE account_id=?1 AND cycle_id=?2 ORDER BY revision DESC LIMIT 1",
            params![account_id, cycle_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if latest.as_ref().is_some_and(|(_, ids)| ids == &active_json) {
        return Ok(());
    }
    if let Some((revision, _)) = latest {
        connection.execute(
            "UPDATE shop_effect_history SET ended_at_utc=?3 WHERE account_id=?1 AND cycle_id=?2 AND revision=?4",
            params![account_id,cycle_id,now.to_rfc3339(),revision],
        )?;
    }
    let state = guest_shop_state(connection)?;
    let revision: i64 = connection.query_row(
        "SELECT coalesce(max(revision),0)+1 FROM shop_effect_history WHERE account_id=?1",
        [account_id],
        |row| row.get(0),
    )?;
    let effects_json = serde_json::to_string(&state.effects).map_err(|_| ScanError::Database)?;
    connection.execute(
        "INSERT INTO shop_effect_history(account_id,cycle_id,revision,started_at_utc,active_instance_ids_json,effects_json)
         VALUES (?1,?2,?3,?4,?5,?6)",
        params![account_id,cycle_id,revision,now.to_rfc3339(),active_json,effects_json],
    )?;
    Ok(())
}

fn stored_instance_version(
    connection: &Connection,
    account_id: &str,
    instance_id: &str,
) -> Result<u64, ScanError> {
    let version: Option<i64> = connection.query_row(
        "SELECT version FROM shop_landscape_edit_version WHERE account_id=?1 AND instance_id=?2",
        params![account_id,instance_id], |row| row.get(0),
    ).optional()?;
    u64::try_from(version.unwrap_or(0)).map_err(|_| ScanError::InvalidCount)
}

fn apply_guest_place(
    connection: &Connection,
    account_id: &str,
    cycle_id: &str,
    instance_id: &str,
    expected_version: u64,
    x: f64,
    y: f64,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<ShopActionStatus, ScanError> {
    let current_cycle = active_cycle_id(connection)?;
    if cycle_id != current_cycle {
        return Ok(ShopActionStatus::CycleMismatch);
    }
    let sku: Option<String> = connection
        .query_row(
            "SELECT sku FROM shop_landscape_instance WHERE account_id=?1 AND instance_id=?2",
            params![account_id, instance_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(sku) = sku else {
        return Ok(ShopActionStatus::NotOwned);
    };
    let current_version = stored_instance_version(connection, account_id, instance_id)?;
    if expected_version != current_version {
        return Ok(ShopActionStatus::VersionConflict);
    }
    let product = shop_products()
        .into_iter()
        .find(|product| product.sku == sku)
        .ok_or(ScanError::InvalidShopState)?;
    let natural = current_natural_objects(connection, &current_cycle)?;
    let terrain = terrain_bounds(&natural);
    if validate_placement(&product, LandscapePoint { x, y }, terrain).is_err() {
        return Ok(ShopActionStatus::InvalidPlacement);
    }
    let before = active_instance_ids(connection, account_id, &current_cycle)?;
    let next_version = current_version
        .checked_add(1)
        .ok_or(ScanError::InvalidCount)?;
    connection.execute(
        "INSERT INTO shop_landscape_edit_version(account_id,instance_id,version) VALUES (?1,?2,?3)
         ON CONFLICT(account_id,instance_id) DO UPDATE SET version=excluded.version",
        params![account_id, instance_id, to_i64(next_version)?],
    )?;
    connection.execute(
        "INSERT INTO shop_landscape_placement(account_id,instance_id,cycle_id,x,y,version)
         VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(account_id,instance_id) DO UPDATE SET
           cycle_id=excluded.cycle_id,x=excluded.x,y=excluded.y,version=excluded.version",
        params![
            account_id,
            instance_id,
            current_cycle,
            x,
            y,
            to_i64(next_version)?
        ],
    )?;
    let after = active_instance_ids(connection, account_id, &current_cycle)?;
    if before != after {
        record_effect_change(connection, account_id, &current_cycle, now)?;
    }
    increment_state_revision(connection, account_id)?;
    Ok(ShopActionStatus::Placed)
}

fn apply_guest_retrieve(
    connection: &Connection,
    account_id: &str,
    cycle_id: &str,
    instance_id: &str,
    expected_version: u64,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<ShopActionStatus, ScanError> {
    let current_cycle = active_cycle_id(connection)?;
    if cycle_id != current_cycle {
        return Ok(ShopActionStatus::CycleMismatch);
    }
    let owned: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM shop_landscape_instance WHERE account_id=?1 AND instance_id=?2)",
        params![account_id,instance_id], |row| row.get(0),
    )?;
    if !owned {
        return Ok(ShopActionStatus::NotOwned);
    }
    let current_version = stored_instance_version(connection, account_id, instance_id)?;
    if expected_version != current_version {
        return Ok(ShopActionStatus::VersionConflict);
    }
    let placed: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM shop_landscape_placement
         WHERE account_id=?1 AND instance_id=?2 AND cycle_id=?3)",
        params![account_id, instance_id, current_cycle],
        |row| row.get(0),
    )?;
    if !placed {
        return Ok(ShopActionStatus::VersionConflict);
    }
    let before = active_instance_ids(connection, account_id, &current_cycle)?;
    connection.execute(
        "DELETE FROM shop_landscape_placement WHERE account_id=?1 AND instance_id=?2",
        params![account_id, instance_id],
    )?;
    let next_version = current_version
        .checked_add(1)
        .ok_or(ScanError::InvalidCount)?;
    connection.execute(
        "UPDATE shop_landscape_edit_version SET version=?3 WHERE account_id=?1 AND instance_id=?2",
        params![account_id, instance_id, to_i64(next_version)?],
    )?;
    let after = active_instance_ids(connection, account_id, &current_cycle)?;
    if before != after {
        record_effect_change(connection, account_id, &current_cycle, now)?;
    }
    increment_state_revision(connection, account_id)?;
    Ok(ShopActionStatus::Retrieved)
}

fn apply_guest_avatar(
    connection: &Connection,
    account_id: &str,
    slot: AvatarSlot,
    sku: Option<&str>,
    expected_version: u64,
) -> Result<ShopActionStatus, ScanError> {
    let slot_name = match slot {
        AvatarSlot::Head => "head",
        AvatarSlot::Outfit => "outfit",
        AvatarSlot::Face => "face",
        AvatarSlot::Back => "back",
    };
    if let Some(sku) = sku {
        let product = shop_products()
            .into_iter()
            .find(|product| product.sku == sku);
        if !product.is_some_and(|product| {
            product.category == ShopCategory::Avatar && product.avatar_slot == Some(slot)
        }) {
            return Ok(ShopActionStatus::NotOwned);
        }
        let owned: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM shop_avatar_owned WHERE account_id=?1 AND sku=?2)",
            params![account_id, sku],
            |row| row.get(0),
        )?;
        if !owned {
            return Ok(ShopActionStatus::NotOwned);
        }
    }
    let current: Option<i64> = connection
        .query_row(
            "SELECT version FROM shop_avatar_equipment WHERE account_id=?1 AND slot=?2",
            params![account_id, slot_name],
            |row| row.get(0),
        )
        .optional()?;
    let current = u64::try_from(current.unwrap_or(0)).map_err(|_| ScanError::InvalidCount)?;
    if expected_version != current {
        return Ok(ShopActionStatus::VersionConflict);
    }
    let next = current.checked_add(1).ok_or(ScanError::InvalidCount)?;
    connection.execute(
        "INSERT INTO shop_avatar_equipment(account_id,slot,sku,version) VALUES (?1,?2,?3,?4)
         ON CONFLICT(account_id,slot) DO UPDATE SET sku=excluded.sku,version=excluded.version",
        params![account_id, slot_name, sku, to_i64(next)?],
    )?;
    increment_state_revision(connection, account_id)?;
    Ok(if sku.is_some() {
        ShopActionStatus::Equipped
    } else {
        ShopActionStatus::Unequipped
    })
}

fn purchase_in_transaction(
    transaction: &Transaction<'_>,
    purchase_id: &str,
    sku: &str,
) -> Result<CosmeticPurchaseResult, ScanError> {
    let account_id = active_account_id(transaction)?;
    let prior: Option<(String, String)> = transaction
        .query_row(
            "SELECT sku,result_json FROM cosmetic_purchase_request
             WHERE account_id=?1 AND purchase_id=?2",
            params![account_id, purchase_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if let Some((prior_sku, result_json)) = prior {
        if prior_sku == sku {
            return serde_json::from_str(&result_json).map_err(|_| ScanError::Database);
        }
        return Ok(CosmeticPurchaseResult {
            purchase_id: purchase_id.into(),
            sku: sku.into(),
            status: CosmeticPurchaseStatus::RequestConflict,
            price: 0,
            available_balance: available_balance(transaction, &account_id)?,
        });
    }

    let balance = available_balance(transaction, &account_id)?;
    let Some(product) = legacy_cosmetic_products()
        .into_iter()
        .find(|product| product.sku == sku)
    else {
        return Err(ScanError::InvalidProfile);
    };
    let price = product.price;
    let owned: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM cosmetic_purchase WHERE account_id=?1 AND sku=?2)",
        params![account_id, sku],
        |row| row.get(0),
    )?;
    let legacy_owned = match legacy_equivalent(sku) {
        Some(old_sku) => transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM cosmetic_purchase WHERE account_id=?1 AND sku=?2)",
            params![account_id, old_sku],
            |row| row.get::<_, bool>(0),
        )?,
        None => false,
    };
    if owned || legacy_owned {
        let result = CosmeticPurchaseResult {
            purchase_id: purchase_id.into(),
            sku: sku.into(),
            status: CosmeticPurchaseStatus::AlreadyOwned,
            price,
            available_balance: balance,
        };
        store_purchase_request(transaction, &account_id, purchase_id, sku, &result)?;
        return Ok(result);
    }
    if !product.purchasable {
        let result = CosmeticPurchaseResult {
            purchase_id: purchase_id.into(),
            sku: sku.into(),
            status: CosmeticPurchaseStatus::CatalogMismatch,
            price,
            available_balance: balance,
        };
        store_purchase_request(transaction, &account_id, purchase_id, sku, &result)?;
        return Ok(result);
    }
    let Some(resulting_balance) = balance.checked_sub(price) else {
        let result = CosmeticPurchaseResult {
            purchase_id: purchase_id.into(),
            sku: sku.into(),
            status: CosmeticPurchaseStatus::InsufficientBalance,
            price,
            available_balance: balance,
        };
        store_purchase_request(transaction, &account_id, purchase_id, sku, &result)?;
        return Ok(result);
    };
    transaction.execute(
        "INSERT INTO cosmetic_purchase(account_id,purchase_id,sku,price,purchased_at_utc)
         VALUES (?1,?2,?3,?4,?5)",
        params![
            account_id,
            purchase_id,
            sku,
            to_i64(product.price)?,
            Utc::now().to_rfc3339()
        ],
    )?;
    let status = CosmeticPurchaseStatus::Purchased;
    let result = CosmeticPurchaseResult {
        purchase_id: purchase_id.into(),
        sku: sku.into(),
        status,
        price,
        available_balance: resulting_balance,
    };
    store_purchase_request(transaction, &account_id, purchase_id, sku, &result)?;
    Ok(result)
}

fn store_purchase_request(
    transaction: &Transaction<'_>,
    account_id: &str,
    purchase_id: &str,
    sku: &str,
    result: &CosmeticPurchaseResult,
) -> Result<(), ScanError> {
    let result_json = serde_json::to_string(result).map_err(|_| ScanError::Database)?;
    transaction.execute(
        "INSERT INTO cosmetic_purchase_request(account_id,purchase_id,sku,result_json)
         VALUES (?1,?2,?3,?4)",
        params![account_id, purchase_id, sku, result_json],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::domain::cosmetic_shop::{
        cosmetic_slots, legacy_cosmetic_products, shop_products, CosmeticPurchaseResult,
        CosmeticPurchaseStatus, CosmeticShopState, EquippedCosmetic, QuoteTarget, ShopActionStatus,
        ShopRequest,
    };
    use crate::storage::ledger::Ledger;
    use rusqlite::params;

    fn insert_natural_objects(ledger: &Ledger, cycle_id: &str, count: usize) {
        for ordinal in 0..count {
            ledger
                .connection
                .execute(
                    "INSERT INTO planet_object(cycle_id,stage,ordinal,kind,x,y,seed)
                 VALUES (?1,0,?2,'tree',25,50,?3)",
                    params![cycle_id, ordinal as i64, ordinal.to_string()],
                )
                .unwrap();
        }
    }

    fn signed_sync_is_held_for_local_shop_row(statement: &str) -> bool {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        ledger.connection.execute_batch(statement).unwrap();
        ledger
            .ensure_planet_account("00000000-0000-0000-0000-000000000053")
            .unwrap();
        ledger.has_unimported_guest_shop_state().unwrap()
    }

    #[test]
    fn local_catalog_exposes_all_forty_eight_shop_products() {
        assert_eq!(
            cosmetic_slots()
                .iter()
                .map(|slot| slot.slot_id.as_str())
                .collect::<Vec<_>>(),
            vec!["sky", "ring", "surface", "forecourt"],
        );
        let products = shop_products();
        assert_eq!(products.len(), 48);
        let actual_prices = products
            .iter()
            .map(|product| (product.sku.as_str(), product.price))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(actual_prices.len(), 48);
        assert_eq!(
            actual_prices,
            BTreeMap::from([
                ("land_pond", 5_000_000),
                ("land_well", 15_000_000),
                ("land_greenhouse", 40_000_000),
                ("land_reservoir", 100_000_000),
                ("land_crystal", 5_000_000),
                ("land_school", 15_000_000),
                ("land_observatory", 40_000_000),
                ("land_laboratory", 100_000_000),
                ("land_market", 5_000_000),
                ("land_trading_post", 15_000_000),
                ("land_freight", 40_000_000),
                ("land_bazaar", 100_000_000),
                ("land_rover", 5_000_000),
                ("land_clocktower", 15_000_000),
                ("land_launchpad", 40_000_000),
                ("land_portal", 100_000_000),
                ("land_toolbox", 5_000_000),
                ("land_excavator", 15_000_000),
                ("land_cutter", 40_000_000),
                ("land_recycler", 100_000_000),
                ("land_flag", 5_000_000),
                ("land_thin_ring", 15_000_000),
                ("land_double_ring", 40_000_000),
                ("land_moonlets", 100_000_000),
                ("land_lantern", 5_000_000),
                ("land_stars", 15_000_000),
                ("land_aurora", 40_000_000),
                ("land_meteors", 100_000_000),
                ("land_garden", 5_000_000),
                ("land_tree", 15_000_000),
                ("land_bench", 40_000_000),
                ("land_fountain", 100_000_000),
                ("avatar_explorer_hat", 100_000_000),
                ("avatar_crown", 200_000_000),
                ("avatar_space_helmet", 350_000_000),
                ("avatar_halo", 500_000_000),
                ("avatar_workwear", 100_000_000),
                ("avatar_labwear", 200_000_000),
                ("avatar_spacesuit", 350_000_000),
                ("avatar_nebula_suit", 500_000_000),
                ("avatar_glasses", 100_000_000),
                ("avatar_sunglasses", 200_000_000),
                ("avatar_goggles", 350_000_000),
                ("avatar_hud", 500_000_000),
                ("avatar_backpack", 100_000_000),
                ("avatar_cape", 200_000_000),
                ("avatar_jetpack", 350_000_000),
                ("avatar_wings", 500_000_000),
            ])
        );
        assert_eq!(
            products
                .iter()
                .filter(|product| product.category
                    == crate::domain::cosmetic_shop::ShopCategory::Landscape)
                .count(),
            32
        );
        assert_eq!(
            products
                .iter()
                .filter(|product| product.category
                    == crate::domain::cosmetic_shop::ShopCategory::Avatar)
                .count(),
            16
        );
        assert_eq!(
            products
                .iter()
                .filter(|product| product.placement_zone
                    == Some(crate::domain::cosmetic_shop::PlacementZone::Sky))
                .count(),
            6
        );
        assert_eq!(
            products
                .iter()
                .filter(|product| product.effect_type
                    == Some(crate::domain::cosmetic_shop::ShopEffectType::CivilizationGrowth))
                .count(),
            8
        );
        for product in products.iter().filter(|product| {
            product.category == crate::domain::cosmetic_shop::ShopCategory::Avatar
        }) {
            assert!(
                product.effect_type.is_none(),
                "avatar {} must not grant a game effect",
                product.sku
            );
            assert!(
                product.placement_zone.is_none(),
                "avatar {} must not be placeable",
                product.sku
            );
        }
    }

    #[test]
    fn legacy_owned_style_cannot_be_charged_again_at_the_new_price() {
        let directory = tempfile::tempdir().unwrap();
        let mut ledger =
            Ledger::open(&directory.path().join("ledger.sqlite3"), chrono_tz::UTC).unwrap();
        ledger
            .connection
            .execute(
                "INSERT INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
             VALUES ('seed-cycle',600000,'2026-09-28T00:00:00Z')",
                [],
            )
            .unwrap();
        let account_id = super::active_account_id(&ledger.connection).unwrap();
        ledger
            .connection
            .execute(
                "INSERT INTO cosmetic_purchase(account_id,purchase_id,sku,price,purchased_at_utc)
             VALUES (?1,'legacy-purchase','star_cluster',100000,'2026-09-28T00:00:00Z')",
                params![account_id],
            )
            .unwrap();

        let first = ledger
            .purchase_guest_cosmetic("11111111-1111-4111-8111-111111111111", "star_cluster_v2")
            .unwrap();
        assert_eq!(first.status, CosmeticPurchaseStatus::AlreadyOwned);
        assert_eq!(first.price, 500_000);
        assert_eq!(first.available_balance, 500_000);
        let replay = ledger
            .purchase_guest_cosmetic("11111111-1111-4111-8111-111111111111", "star_cluster_v2")
            .unwrap();
        assert_eq!(replay, first);
        let other_device = ledger
            .purchase_guest_cosmetic("22222222-2222-4222-8222-222222222222", "star_cluster_v2")
            .unwrap();
        assert_eq!(other_device.status, CosmeticPurchaseStatus::AlreadyOwned);
        assert_eq!(other_device.available_balance, 500_000);
        let rows: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM cosmetic_purchase WHERE account_id=?1",
                params![account_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            rows, 1,
            "the old purchase amount remains the only ledger charge"
        );
    }

    #[test]
    fn ledger_opens_cosmetic_purchase_and_equipment_tables() {
        let directory = tempfile::tempdir().unwrap();
        let ledger =
            Ledger::open(&directory.path().join("ledger.sqlite3"), chrono_tz::UTC).unwrap();
        for table in [
            "cosmetic_purchase",
            "cosmetic_purchase_request",
            "cosmetic_equipment",
        ] {
            let count: i64 = ledger
                .connection
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    params![table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "missing {table}");
        }
    }

    #[test]
    fn shop_schema_transition_reopens_twice_and_preserves_usage_wallet_and_account_scope() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.sqlite3");
        let before = Ledger::open(&path, chrono_tz::UTC).unwrap();
        let schema_version_table: i64 = before.connection.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='shop_schema_version'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(
            schema_version_table, 1,
            "shop migration needs an explicit schema version"
        );

        before
            .connection
            .execute(
                "INSERT INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
             VALUES ('migration-credit',1250000,'2026-09-28T00:00:00Z')",
                [],
            )
            .unwrap();
        before.connection.execute(
            "INSERT INTO usage_record(event_key,source_id,agent,kind,bucket_date,occurred_at_utc,
             input_tokens,output_tokens,cache_read_tokens,cache_write_tokens,total_tokens,coverage,parser_version)
             VALUES ('kept-event','codex:test','codex','response','2026-09-28',
             '2026-09-28T00:00:00Z',NULL,NULL,NULL,NULL,500,'complete',1)",
            [],
        ).unwrap();
        let local_account = before.cosmetic_account_id().unwrap();
        let cycle = before.planet_cycle_id().unwrap();
        before
            .connection
            .execute(
                "INSERT INTO cosmetic_purchase(account_id,purchase_id,sku,price,purchased_at_utc)
             VALUES (?1,'old-purchase','pond',100000,'2026-09-28T00:00:00Z')",
                [&local_account],
            )
            .unwrap();
        before
            .connection
            .execute(
                "INSERT INTO cosmetic_equipment(account_id,cycle_id,slot_id,sku,version)
             VALUES (?1,?2,'forecourt','pond',1)",
                rusqlite::params![local_account, cycle],
            )
            .unwrap();
        before.connection.execute(
            "INSERT INTO cosmetic_pending_purchase(account_id,sku,purchase_id,catalog_revision,created_at_utc)
             VALUES (?1,'pond','old-pending',1,'2026-09-28T00:00:00Z')",
            [&local_account],
        ).unwrap();
        before
            .connection
            .execute(
                "INSERT INTO cosmetic_purchase_request(account_id,purchase_id,sku,result_json)
             VALUES (?1,'old-request','pond','{}')",
                [&local_account],
            )
            .unwrap();
        before
            .connection
            .execute(
                "INSERT INTO cosmetic_shop_remote_state(account_id,state_json,updated_at_utc)
             VALUES (?1,'{}','2026-09-28T00:00:00Z')",
                [&local_account],
            )
            .unwrap();
        before.connection.execute(
            "INSERT INTO cosmetic_guest_import(account_id,import_id,payload_json,imported_at_utc)
             VALUES (?1,'old-import','{}',NULL)",
            [&local_account],
        ).unwrap();
        before
            .connection
            .execute("UPDATE shop_schema_version SET version=0 WHERE id=1", [])
            .unwrap();
        drop(before);

        let migrated = Ledger::open(&path, chrono_tz::UTC).unwrap();
        let version: i64 = migrated
            .connection
            .query_row(
                "SELECT version FROM shop_schema_version WHERE id=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, 1);
        for table in [
            "cosmetic_purchase",
            "cosmetic_purchase_request",
            "cosmetic_equipment",
            "cosmetic_shop_remote_state",
            "cosmetic_pending_purchase",
            "cosmetic_guest_import",
        ] {
            let rows: i64 = migrated
                .connection
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(rows, 0, "legacy shop table {table} must be cleared once");
        }
        assert_eq!(migrated.cosmetic_account_id().unwrap(), local_account);
        let usage_rows: i64 = migrated
            .connection
            .query_row(
                "SELECT count(*) FROM usage_record WHERE event_key='kept-event'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(usage_rows, 1);
        assert_eq!(
            migrated.cosmetic_shop_state().unwrap().available_balance,
            1_250_000
        );

        migrated
            .connection
            .execute(
                "INSERT INTO cosmetic_purchase(account_id,purchase_id,sku,price,purchased_at_utc)
             VALUES (?1,'post-migration','pond',5,'2026-09-29T00:00:00Z')",
                [&local_account],
            )
            .unwrap();
        drop(migrated);
        let mut reopened = Ledger::open(&path, chrono_tz::UTC).unwrap();
        let post_migration_rows: i64 = reopened
            .connection
            .query_row(
                "SELECT count(*) FROM cosmetic_purchase WHERE purchase_id='post-migration'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            post_migration_rows, 1,
            "a completed migration must not clear state again"
        );

        reopened.ensure_planet_account("alice").unwrap();
        let alice = reopened.cosmetic_account_id().unwrap();
        reopened.connection.execute(
            "INSERT INTO shop_landscape_instance(account_id,instance_id,sku,variation_index,seed,variation_version,acquired_at_utc)
             VALUES (?1,'alice-tree','land_tree',0,'seed-alice',1,'2026-09-29T00:00:00Z')",
            [&alice],
        ).unwrap();
        let alice_instances: i64 = reopened
            .connection
            .query_row(
                "SELECT count(*) FROM shop_landscape_instance WHERE account_id=?1",
                [&alice],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(alice_instances, 1);
        reopened.ensure_planet_account("bob").unwrap();
        let bob = reopened.cosmetic_account_id().unwrap();
        let bob_instances: i64 = reopened
            .connection
            .query_row(
                "SELECT count(*) FROM shop_landscape_instance WHERE account_id=?1",
                [&bob],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(bob_instances, 0);
        reopened.ensure_planet_account("alice").unwrap();
        let alice_instances: i64 = reopened.connection.query_row(
            "SELECT count(*) FROM shop_landscape_instance WHERE account_id=?1 AND instance_id='alice-tree'",
            [&alice], |row| row.get(0),
        ).unwrap();
        assert_eq!(alice_instances, 1);
    }

    #[test]
    fn guest_shop_repeats_purchases_idempotently_and_caps_each_sku_at_five() {
        let directory = tempfile::tempdir().unwrap();
        let mut ledger =
            Ledger::open(&directory.path().join("ledger.sqlite3"), chrono_tz::UTC).unwrap();
        ledger
            .connection
            .execute(
                "INSERT INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
             VALUES ('shop-funds',1000000000,'2026-10-01T00:00:00Z')",
                [],
            )
            .unwrap();
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-01T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let pond_quote = ledger
            .quote_shop(&QuoteTarget::Purchase {
                sku: "land_pond".into(),
            })
            .unwrap();
        let first_request = ShopRequest::Purchase {
            request_id: "pond-0".into(),
            quote: pond_quote,
        };
        let first = ledger
            .apply_guest_shop_request(&first_request, now)
            .unwrap();
        assert_eq!(first.status, ShopActionStatus::Purchased);
        assert_eq!(first.state.available_balance, 995_000_000);
        assert_eq!(first.state.landscape_instances.len(), 1);
        assert_eq!(first.state.landscape_instances[0].variation_index, 0);
        let second_quote = ledger
            .quote_shop(&QuoteTarget::Purchase {
                sku: "land_well".into(),
            })
            .unwrap();
        let second = ledger
            .apply_guest_shop_request(
                &ShopRequest::Purchase {
                    request_id: "well-0".into(),
                    quote: second_quote,
                },
                now,
            )
            .unwrap();
        assert_eq!(second.status, ShopActionStatus::Purchased);
        let replay = ledger
            .apply_guest_shop_request(&first_request, now)
            .unwrap();
        assert_eq!(replay.status, first.status);
        assert_eq!(replay.confirmed_quote, first.confirmed_quote);
        assert_eq!(replay.state.available_balance, 980_000_000);
        assert_eq!(replay.state.landscape_instances.len(), 2);

        let different_quote = ledger
            .quote_shop(&QuoteTarget::Purchase {
                sku: "land_well".into(),
            })
            .unwrap();
        let conflict = ledger
            .apply_guest_shop_request(
                &ShopRequest::Purchase {
                    request_id: "pond-0".into(),
                    quote: different_quote,
                },
                now,
            )
            .unwrap();
        assert_eq!(conflict.status, ShopActionStatus::RequestConflict);
        assert_eq!(conflict.state.available_balance, 980_000_000);

        for index in 1..5 {
            let quote = ledger
                .quote_shop(&QuoteTarget::Purchase {
                    sku: "land_pond".into(),
                })
                .unwrap();
            let result = ledger
                .apply_guest_shop_request(
                    &ShopRequest::Purchase {
                        request_id: format!("pond-{index}"),
                        quote,
                    },
                    now,
                )
                .unwrap();
            assert_eq!(result.status, ShopActionStatus::Purchased);
            assert_eq!(result.state.landscape_instances.len(), index + 2);
            assert!(result
                .state
                .landscape_instances
                .iter()
                .any(|instance| instance.sku == "land_pond"
                    && instance.variation_index == index as u8));
        }
        let balance_before_sixth = ledger.shop_state().unwrap().available_balance;
        let quote = ledger
            .quote_shop(&QuoteTarget::Purchase {
                sku: "land_pond".into(),
            })
            .unwrap();
        let sixth = ledger
            .apply_guest_shop_request(
                &ShopRequest::Purchase {
                    request_id: "pond-5".into(),
                    quote,
                },
                now,
            )
            .unwrap();
        assert_eq!(sixth.status, ShopActionStatus::LimitReached);
        assert_eq!(sixth.state.available_balance, balance_before_sixth);
        assert_eq!(
            sixth
                .state
                .landscape_instances
                .iter()
                .filter(|instance| instance.sku == "land_pond")
                .count(),
            5
        );

        let unplaced_discount_quote = ledger
            .quote_shop(&QuoteTarget::Purchase {
                sku: "land_bazaar".into(),
            })
            .unwrap();
        assert_eq!(
            unplaced_discount_quote.price, 100_000_000,
            "an unplaced discount product cannot discount its own purchase"
        );
    }

    #[test]
    fn guest_shop_places_and_retrieves_owned_instances_and_discounts_avatar_quotes() {
        let directory = tempfile::tempdir().unwrap();
        let mut ledger =
            Ledger::open(&directory.path().join("ledger.sqlite3"), chrono_tz::UTC).unwrap();
        ledger
            .connection
            .execute(
                "INSERT INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
             VALUES ('shop-funds',3000000000,'2026-10-01T00:00:00Z')",
                [],
            )
            .unwrap();
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-01T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let account = super::active_account_id(&ledger.connection).unwrap();
        for variation in 0..5 {
            ledger.connection.execute(
                "INSERT INTO shop_landscape_instance(account_id,instance_id,sku,variation_index,seed,variation_version,acquired_at_utc)
                 VALUES (?1,?2,'land_bazaar',?3,?2,1,'2026-10-01T00:00:00Z')",
                rusqlite::params![account, format!("bazaar-{variation}"), variation],
            ).unwrap();
            ledger.connection.execute(
                "INSERT INTO shop_landscape_placement(account_id,instance_id,cycle_id,x,y,version)
                 VALUES (?1,?2,(SELECT value FROM setting WHERE key='planet_current_cycle_id'),?3,200,1)",
                rusqlite::params![account, format!("bazaar-{variation}"), 160.0 + f64::from(variation) * 100.0],
            ).unwrap();
        }
        let avatar_quote = ledger
            .quote_shop(&QuoteTarget::Purchase {
                sku: "avatar_explorer_hat".into(),
            })
            .unwrap();
        assert_eq!(avatar_quote.price, 85_000_000);

        let pond_quote = ledger
            .quote_shop(&QuoteTarget::Purchase {
                sku: "land_pond".into(),
            })
            .unwrap();
        assert_eq!(
            pond_quote.price, 4_250_000,
            "only the five already placed discount instances apply"
        );
        let purchase = ledger
            .apply_guest_shop_request(
                &ShopRequest::Purchase {
                    request_id: "place-pond-purchase".into(),
                    quote: pond_quote,
                },
                now,
            )
            .unwrap();
        let instance_id = purchase
            .state
            .landscape_instances
            .iter()
            .find(|instance| instance.sku == "land_pond")
            .unwrap()
            .instance_id
            .clone();
        let cycle_id = purchase.state.current_cycle_id.clone();
        let placed = ledger
            .apply_guest_shop_request(
                &ShopRequest::Place {
                    request_id: "place-pond".into(),
                    cycle_id: cycle_id.clone(),
                    instance_id: instance_id.clone(),
                    expected_version: 0,
                    x: 160.0,
                    y: 200.0,
                },
                now,
            )
            .unwrap();
        assert_eq!(placed.status, ShopActionStatus::Placed);
        assert_eq!(placed.state.effects.token_earning_bps, 100);
        let stale_quote_result = ledger
            .apply_guest_shop_request(
                &ShopRequest::Purchase {
                    request_id: "stale-avatar-quote".into(),
                    quote: avatar_quote.clone(),
                },
                now,
            )
            .unwrap();
        assert_eq!(stale_quote_result.status, ShopActionStatus::QuoteChanged);
        assert_eq!(
            stale_quote_result.state.available_balance,
            placed.state.available_balance
        );
        let invalid_placement = ledger
            .apply_guest_shop_request(
                &ShopRequest::Place {
                    request_id: "outside-landscape".into(),
                    cycle_id: cycle_id.clone(),
                    instance_id: instance_id.clone(),
                    expected_version: 1,
                    x: 1390.0,
                    y: 500.0,
                },
                now,
            )
            .unwrap();
        assert_eq!(invalid_placement.status, ShopActionStatus::InvalidPlacement);
        let wrong_cycle = ledger
            .apply_guest_shop_request(
                &ShopRequest::Place {
                    request_id: "wrong-cycle".into(),
                    cycle_id: "stale-cycle".into(),
                    instance_id: instance_id.clone(),
                    expected_version: 1,
                    x: 160.0,
                    y: 200.0,
                },
                now,
            )
            .unwrap();
        assert_eq!(wrong_cycle.status, ShopActionStatus::CycleMismatch);
        let missing_owner = ledger
            .apply_guest_shop_request(
                &ShopRequest::Retrieve {
                    request_id: "retrieve-unowned".into(),
                    cycle_id: cycle_id.clone(),
                    instance_id: "other-account-instance".into(),
                    expected_version: 1,
                },
                now,
            )
            .unwrap();
        assert_eq!(missing_owner.status, ShopActionStatus::NotOwned);
        let stale_retrieve = ledger
            .apply_guest_shop_request(
                &ShopRequest::Retrieve {
                    request_id: "stale-retrieve".into(),
                    cycle_id: cycle_id.clone(),
                    instance_id: instance_id.clone(),
                    expected_version: 0,
                },
                now,
            )
            .unwrap();
        assert_eq!(stale_retrieve.status, ShopActionStatus::VersionConflict);
        assert_eq!(stale_retrieve.state.effects.token_earning_bps, 100);
        let retrieved = ledger
            .apply_guest_shop_request(
                &ShopRequest::Retrieve {
                    request_id: "retrieve-pond".into(),
                    cycle_id,
                    instance_id: instance_id.clone(),
                    expected_version: 1,
                },
                now,
            )
            .unwrap();
        assert_eq!(retrieved.status, ShopActionStatus::Retrieved);
        assert_eq!(retrieved.state.effects.token_earning_bps, 0);
        assert_eq!(
            retrieved
                .state
                .landscape_instances
                .iter()
                .find(|instance| instance.instance_id == instance_id)
                .unwrap()
                .placement_version,
            2
        );
        let stale_replacement = ledger
            .apply_guest_shop_request(
                &ShopRequest::Place {
                    request_id: "stale-replace-pond".into(),
                    cycle_id: retrieved.state.current_cycle_id.clone(),
                    instance_id: instance_id.clone(),
                    expected_version: 0,
                    x: 160.0,
                    y: 200.0,
                },
                now,
            )
            .unwrap();
        assert_eq!(stale_replacement.status, ShopActionStatus::VersionConflict);
        let placed_again = ledger
            .apply_guest_shop_request(
                &ShopRequest::Place {
                    request_id: "replace-pond".into(),
                    cycle_id: retrieved.state.current_cycle_id.clone(),
                    instance_id: instance_id.clone(),
                    expected_version: 2,
                    x: 160.0,
                    y: 200.0,
                },
                now,
            )
            .unwrap();
        assert_eq!(placed_again.status, ShopActionStatus::Placed);
        assert_eq!(
            placed_again
                .state
                .placements
                .iter()
                .find(|placement| placement.instance_id == instance_id)
                .unwrap()
                .version,
            3
        );

        let fresh_avatar_quote = ledger
            .quote_shop(&QuoteTarget::Purchase {
                sku: "avatar_explorer_hat".into(),
            })
            .unwrap();
        let avatar_purchase = ledger
            .apply_guest_shop_request(
                &ShopRequest::Purchase {
                    request_id: "buy-avatar".into(),
                    quote: fresh_avatar_quote,
                },
                now,
            )
            .unwrap();
        assert_eq!(avatar_purchase.status, ShopActionStatus::Purchased);
        assert_eq!(avatar_purchase.confirmed_quote.unwrap().price, 85_000_000);
        let avatar_balance = avatar_purchase.state.available_balance;
        let duplicate_avatar_quote = ledger
            .quote_shop(&QuoteTarget::Purchase {
                sku: "avatar_explorer_hat".into(),
            })
            .unwrap();
        let duplicate_avatar = ledger
            .apply_guest_shop_request(
                &ShopRequest::Purchase {
                    request_id: "buy-avatar-again".into(),
                    quote: duplicate_avatar_quote,
                },
                now,
            )
            .unwrap();
        assert_eq!(duplicate_avatar.status, ShopActionStatus::AlreadyOwned);
        assert_eq!(duplicate_avatar.state.available_balance, avatar_balance);
        assert_eq!(
            duplicate_avatar
                .state
                .avatar_owned_skus
                .iter()
                .filter(|sku| *sku == "avatar_explorer_hat")
                .count(),
            1
        );
        let equipped = ledger
            .apply_guest_shop_request(
                &ShopRequest::EquipAvatar {
                    request_id: "equip-avatar".into(),
                    slot: crate::domain::cosmetic_shop::AvatarSlot::Head,
                    sku: Some("avatar_explorer_hat".into()),
                    expected_version: 0,
                },
                now,
            )
            .unwrap();
        assert_eq!(equipped.status, ShopActionStatus::Equipped);
        assert_eq!(
            equipped.state.avatar_equipment.head.sku.as_deref(),
            Some("avatar_explorer_hat")
        );
        let unequipped = ledger
            .apply_guest_shop_request(
                &ShopRequest::EquipAvatar {
                    request_id: "unequip-avatar".into(),
                    slot: crate::domain::cosmetic_shop::AvatarSlot::Head,
                    sku: None,
                    expected_version: 1,
                },
                now,
            )
            .unwrap();
        assert_eq!(unequipped.status, ShopActionStatus::Unequipped);
        assert_eq!(unequipped.state.avatar_equipment.head.sku, None);
    }

    #[test]
    fn guest_shop_insufficient_balance_is_recorded_without_any_debit() {
        let directory = tempfile::tempdir().unwrap();
        let mut ledger =
            Ledger::open(&directory.path().join("ledger.sqlite3"), chrono_tz::UTC).unwrap();
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-01T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let quote = ledger
            .quote_shop(&QuoteTarget::Purchase {
                sku: "land_pond".into(),
            })
            .unwrap();
        let result = ledger
            .apply_guest_shop_request(
                &ShopRequest::Purchase {
                    request_id: "unfunded-pond".into(),
                    quote,
                },
                now,
            )
            .unwrap();
        assert_eq!(result.status, ShopActionStatus::InsufficientBalance);
        assert_eq!(result.state.available_balance, 0);
        assert!(result.state.landscape_instances.is_empty());
        let purchases: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM shop_purchase WHERE sku='land_pond'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(purchases, 0);
    }

    #[test]
    fn natural_removal_quotes_use_generation_stage_cost_and_the_discount_cap() {
        let ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let account = super::active_account_id(&ledger.connection).unwrap();
        let cycle = ledger.planet_cycle_id().unwrap();
        for stage in 0..5 {
            ledger
                .ensure_planet_object(stage, 0, "tree", 50, 50, u64::from(stage))
                .unwrap();
        }
        for sku in ["land_recycler", "land_cutter", "land_excavator"] {
            for variation in 0..5 {
                let instance_id = format!("{sku}-{variation}");
                ledger.connection.execute(
                    "INSERT INTO shop_landscape_instance(account_id,instance_id,sku,variation_index,
                     seed,variation_version,acquired_at_utc) VALUES (?1,?2,?3,?4,?5,1,'2026-10-01T00:00:00Z')",
                    rusqlite::params![account,instance_id,sku,variation,format!("seed-{instance_id}")],
                ).unwrap();
                ledger.connection.execute(
                    "INSERT INTO shop_landscape_placement(account_id,instance_id,cycle_id,x,y,version)
                     VALUES (?1,?2,?3,200,200,1)",
                    rusqlite::params![account,instance_id,cycle],
                ).unwrap();
            }
        }

        let targets =
            (0..5).map(
                |stage| crate::domain::cosmetic_shop::QuoteTarget::RemoveNatural {
                    key: crate::domain::cosmetic_shop::NaturalObjectKey {
                        cycle_id: cycle.clone(),
                        stage,
                        ordinal: 0,
                    },
                },
            );
        let actual = targets
            .map(|target| ledger.quote_shop(&target).map(|quote| quote.price))
            .collect::<Result<Vec<_>, _>>();

        assert_eq!(
            actual,
            Ok(vec![70_000, 175_000, 350_000, 700_000, 1_400_000])
        );
    }

    #[test]
    fn guest_natural_removal_debits_once_and_keeps_the_generated_object_as_a_tombstone() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let account = super::active_account_id(&ledger.connection).unwrap();
        let cycle = ledger.planet_cycle_id().unwrap();
        ledger
            .ensure_planet_object(0, 0, "tree", 50, 50, 17)
            .unwrap();
        ledger
            .connection
            .execute(
                "INSERT INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
             VALUES ('seed-wallet',500000,'2026-10-01T00:00:00Z')",
                [],
            )
            .unwrap();
        let key = crate::domain::cosmetic_shop::NaturalObjectKey {
            cycle_id: cycle.clone(),
            stage: 0,
            ordinal: 0,
        };
        let quote = ledger
            .quote_shop(&QuoteTarget::RemoveNatural { key: key.clone() })
            .unwrap();
        let request = ShopRequest::RemoveNatural {
            request_id: "remove-first-tree".into(),
            key: key.clone(),
            expected_version: 0,
            quote: quote.clone(),
        };
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-01T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);

        let removed = ledger.apply_guest_shop_request(&request, now).unwrap();
        assert_eq!(removed.status, ShopActionStatus::Removed);
        assert_eq!(removed.confirmed_quote, Some(quote.clone()));
        assert_eq!(removed.state.available_balance, 400_000);
        assert_eq!(removed.state.removed_natural_keys, vec![key.clone()]);
        assert_eq!(ledger.planet_objects().unwrap().len(), 1);

        // A replay preserves its confirmed outcome but returns the latest canonical state.
        let replay = ledger.apply_guest_shop_request(&request, now).unwrap();
        assert_eq!(replay.status, ShopActionStatus::Removed);
        assert_eq!(replay.confirmed_quote, Some(quote.clone()));
        assert_eq!(replay.state.available_balance, 400_000);
        assert_eq!(replay.state.removed_natural_keys, vec![key.clone()]);

        let already_removed = ledger
            .apply_guest_shop_request(
                &ShopRequest::RemoveNatural {
                    request_id: "remove-same-tree-again".into(),
                    key,
                    expected_version: 0,
                    quote,
                },
                now,
            )
            .unwrap();
        assert_eq!(already_removed.status, ShopActionStatus::AlreadyRemoved);
        assert_eq!(already_removed.state.available_balance, 400_000);
        assert_eq!(
            ledger
                .connection
                .query_row(
                    "SELECT count(*) FROM shop_natural_removal WHERE account_id=?1 AND cycle_id=?2",
                    rusqlite::params![account, cycle],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
        assert_eq!(
            ledger
                .connection
                .query_row(
                    "SELECT count(*) FROM shop_natural_removal_debit WHERE account_id=?1",
                    [&account],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
    }

    #[test]
    fn guest_natural_removal_rejects_stale_versions_and_insufficient_balance_without_tombstones() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let cycle = ledger.planet_cycle_id().unwrap();
        ledger
            .ensure_planet_object(1, 2, "rock", 64, 20, 29)
            .unwrap();
        let key = crate::domain::cosmetic_shop::NaturalObjectKey {
            cycle_id: cycle.clone(),
            stage: 1,
            ordinal: 2,
        };
        let quote = ledger
            .quote_shop(&QuoteTarget::RemoveNatural { key: key.clone() })
            .unwrap();
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-01T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);

        let stale = ledger
            .apply_guest_shop_request(
                &ShopRequest::RemoveNatural {
                    request_id: "stale-natural-version".into(),
                    key: key.clone(),
                    expected_version: 1,
                    quote: quote.clone(),
                },
                now,
            )
            .unwrap();
        assert_eq!(stale.status, ShopActionStatus::VersionConflict);
        assert!(stale.state.removed_natural_keys.is_empty());

        let unfunded = ledger
            .apply_guest_shop_request(
                &ShopRequest::RemoveNatural {
                    request_id: "unfunded-natural-removal".into(),
                    key,
                    expected_version: 0,
                    quote,
                },
                now,
            )
            .unwrap();
        assert_eq!(unfunded.status, ShopActionStatus::InsufficientBalance);
        assert!(unfunded.state.removed_natural_keys.is_empty());
        assert_eq!(unfunded.state.available_balance, 0);
        assert_eq!(ledger.planet_objects().unwrap().len(), 1);
        assert_eq!(
            ledger
                .connection
                .query_row("SELECT count(*) FROM shop_natural_removal", [], |row| row
                    .get::<_, i64>(
                    0
                ),)
                .unwrap(),
            0
        );
        assert_eq!(
            ledger
                .connection
                .query_row(
                    "SELECT count(*) FROM shop_natural_removal_debit",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn guest_reset_settles_once_freezes_the_maximum_cooldown_and_returns_placements_to_inventory() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let account = super::active_account_id(&ledger.connection).unwrap();
        let old_cycle = ledger.planet_cycle_id().unwrap();
        ledger
            .connection
            .execute(
                "UPDATE setting SET value='2026-09-29T00:00:00Z'
             WHERE key IN ('planet_activation_at_utc','planet_cycle_started_at_utc')",
                [],
            )
            .unwrap();
        let effects = crate::domain::cosmetic_shop::ActiveEffects {
            token_earning_bps: 100,
            reset_cooldown_bps: 2_500,
            ..Default::default()
        };
        ledger.connection.execute(
            "INSERT INTO shop_effect_history(account_id,cycle_id,revision,started_at_utc,
             active_instance_ids_json,effects_json) VALUES (?1,?2,1,'2026-09-29T00:00:00Z','[]',?3)",
            rusqlite::params![account, old_cycle, serde_json::to_string(&effects).unwrap()],
        ).unwrap();
        ledger
            .insert(&crate::collectors::ParsedRecord {
                agent: crate::domain::usage::Agent::Codex,
                kind: crate::collectors::RecordKind::Response,
                event_key: "reset-cycle-token-event".into(),
                occurred_at_utc: chrono::DateTime::parse_from_rfc3339("2026-09-30T10:00:00Z")
                    .unwrap()
                    .with_timezone(&chrono::Utc),
                usage: crate::domain::usage::TokenUsage {
                    input_tokens: None,
                    output_tokens: None,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    total_tokens: Some(100),
                    coverage: crate::domain::usage::UsageCoverage::Complete,
                },
            })
            .unwrap();

        for (sku, effect_bps) in [("land_portal", 300), ("land_launchpad", 200)] {
            for variation in 0..5 {
                let instance_id = format!("reset-{sku}-{variation}");
                ledger.connection.execute(
                    "INSERT INTO shop_landscape_instance(account_id,instance_id,sku,variation_index,
                     seed,variation_version,acquired_at_utc) VALUES (?1,?2,?3,?4,?5,1,'2026-10-01T00:00:00Z')",
                    rusqlite::params![account,instance_id,sku,variation,format!("seed-{instance_id}")],
                ).unwrap();
                ledger
                    .connection
                    .execute(
                        "INSERT INTO shop_landscape_edit_version(account_id,instance_id,version)
                     VALUES (?1,?2,3)",
                        rusqlite::params![account, instance_id],
                    )
                    .unwrap();
                ledger.connection.execute(
                    "INSERT INTO shop_landscape_placement(account_id,instance_id,cycle_id,x,y,version)
                     VALUES (?1,?2,?3,150,150,1)",
                    rusqlite::params![account,instance_id,old_cycle],
                ).unwrap();
            }
            assert!(effect_bps > 0);
        }
        ledger
            .connection
            .execute(
                "INSERT INTO shop_avatar_owned(account_id,sku,purchase_id,price,acquired_at_utc)
             VALUES (?1,'avatar_explorer_hat','avatar-purchase',100000000,'2026-10-01T00:00:00Z')",
                [&account],
            )
            .unwrap();
        ledger
            .connection
            .execute(
                "INSERT INTO shop_avatar_equipment(account_id,slot,sku,version)
             VALUES (?1,'head','avatar_explorer_hat',2)",
                [&account],
            )
            .unwrap();
        ledger
            .connection
            .execute(
                "INSERT INTO cosmetic_equipment(account_id,cycle_id,slot_id,sku,version)
             VALUES (?1,?2,'forecourt','pond',3)",
                rusqlite::params![account, old_cycle],
            )
            .unwrap();
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-02T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        for table in [
            "shop_cycle_settlement",
            "shop_game_reward",
            "shop_effect_contribution",
            "shop_activity_day",
            "growth_journal_cycle",
            "growth_journal_entry",
        ] {
            let count: i64 = ledger
                .connection
                .query_row(
                    &format!("SELECT count(*) FROM {table} WHERE account_id=?1"),
                    [&account],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 0, "fixture starts without {table} rows");
        }
        let initial_receipt_count: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM guest_provenance_reset_receipt",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(initial_receipt_count, 0);
        let initial_provenance: (i64, i64, i64, Option<String>, i64) = ledger
            .connection
            .query_row(
                "SELECT l.eligible,l.last_mutation_seq,l.occurrence_count,b.ended_at_utc,
                        b.baseline_revision
                 FROM guest_provenance_lineage l
                 JOIN guest_provenance_cycle b ON b.lineage_id=l.lineage_id
                 WHERE l.singleton=1 AND b.cycle_id=?1",
                [&old_cycle],
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
        let initial_journal_generation: (i64, Option<String>) = ledger
            .connection
            .query_row(
                "SELECT generation,deleted_at_utc FROM growth_journal_state WHERE account_id=?1",
                [&account],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        ledger
            .connection
            .execute_batch(
                "CREATE TRIGGER fail_guest_reset_wallet
             BEFORE INSERT ON planet_wallet_credit
             BEGIN SELECT RAISE(ABORT, 'injected reset body failure'); END;",
            )
            .unwrap();
        assert!(matches!(
            ledger.reset_guest_planet("reset-cycle-one", &old_cycle, now),
            Err(crate::storage::ledger::ScanError::Database),
        ));
        assert_eq!(ledger.planet_cycle_id().unwrap(), old_cycle);
        assert_eq!(ledger.reset_available_at().unwrap(), None);
        assert_eq!(ledger.shop_state().unwrap().placements.len(), 10);
        assert_eq!(ledger.connection.query_row::<i64,_,_>(
            "SELECT count(*) FROM shop_action_request WHERE account_id=?1 AND request_id='reset-cycle-one'",
            [&account],|row|row.get(0),
        ).unwrap(),0);
        assert_eq!(ledger.connection.query_row::<i64,_,_>(
            "SELECT count(*) FROM shop_effect_history WHERE account_id=?1 AND cycle_id=?2 AND ended_at_utc IS NULL",
            rusqlite::params![account,old_cycle],|row|row.get(0),
        ).unwrap(),1);
        assert_eq!(
            ledger
                .connection
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM planet_wallet_credit WHERE previous_cycle_id=?1",
                    [&old_cycle],
                    |row| row.get(0),
                )
                .unwrap(),
            0
        );
        for table in [
            "shop_cycle_settlement",
            "shop_game_reward",
            "shop_effect_contribution",
            "shop_activity_day",
            "growth_journal_cycle",
            "growth_journal_entry",
        ] {
            let count: i64 = ledger
                .connection
                .query_row(
                    &format!("SELECT count(*) FROM {table} WHERE account_id=?1"),
                    [&account],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 0, "failed reset rolled back {table}");
        }
        let provenance_after_failure: (i64, i64, i64, Option<String>, i64) = ledger
            .connection
            .query_row(
                "SELECT l.eligible,l.last_mutation_seq,l.occurrence_count,b.ended_at_utc,
                        b.baseline_revision
                 FROM guest_provenance_lineage l
                 JOIN guest_provenance_cycle b ON b.lineage_id=l.lineage_id
                 WHERE l.singleton=1 AND b.cycle_id=?1",
                [&old_cycle],
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
        assert_eq!(provenance_after_failure, initial_provenance);
        assert_eq!(
            ledger
                .connection
                .query_row(
                    "SELECT count(*) FROM guest_provenance_reset_receipt",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            initial_receipt_count
        );
        assert_eq!(
            ledger
                .connection
                .query_row(
                    "SELECT generation,deleted_at_utc FROM growth_journal_state WHERE account_id=?1",
                    [&account],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap(),
            initial_journal_generation
        );
        ledger
            .connection
            .execute_batch("DROP TRIGGER fail_guest_reset_wallet;")
            .unwrap();

        // Preparation rolled back with the reset; retry commits it exactly once.
        let result = ledger
            .reset_guest_planet("reset-cycle-one", &old_cycle, now)
            .unwrap();

        assert_eq!(result.status, ShopActionStatus::Reset);
        assert_ne!(result.state.current_cycle_id, old_cycle);
        assert!(result.state.placements.is_empty());
        assert_eq!(result.state.landscape_instances.len(), 10);
        assert!(result
            .state
            .landscape_instances
            .iter()
            .all(|instance| instance.placement_version == 4));
        assert_eq!(result.state.effects.reset_cooldown_bps, 0);
        assert_eq!(result.state.available_balance, 101);
        assert_eq!(
            ledger
                .connection
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM cosmetic_equipment WHERE account_id=?1 AND cycle_id=?2",
                    rusqlite::params![account, old_cycle],
                    |row| row.get(0),
                )
                .unwrap(),
            0
        );
        assert_eq!(ledger.planet_usage_totals().unwrap().1, 0);
        assert_eq!(ledger.planet_usage_totals().unwrap().2, 100);
        assert_eq!(
            ledger.reset_available_at().unwrap(),
            Some(now + chrono::Duration::hours(18))
        );
        assert_eq!(
            ledger
                .shop_state()
                .unwrap()
                .avatar_equipment
                .head
                .sku
                .as_deref(),
            Some("avatar_explorer_hat")
        );
        assert!(ledger
            .shop_state()
            .unwrap()
            .avatar_owned_skus
            .contains(&"avatar_explorer_hat".into()));
        assert_eq!(
            ledger
                .connection
                .query_row(
                    "SELECT count(*) FROM shop_activity_day WHERE account_id=?1",
                    [&account],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
        assert_eq!(ledger.connection.query_row(
            "SELECT count(*) FROM shop_cycle_settlement WHERE account_id=?1 AND cycle_id=?2 AND amount=1",
            rusqlite::params![account,old_cycle], |row| row.get::<_, i64>(0),
        ).unwrap(), 1);
        assert_eq!(ledger.connection.query_row::<i64,_,_>(
            "SELECT count(*) FROM shop_game_reward WHERE account_id=?1 AND kind='cycle_token' AND amount=1",
            [&account],|row|row.get(0),
        ).unwrap(),1);
        let journal_cycle: (Option<String>, Option<i64>) = ledger
            .connection
            .query_row(
                "SELECT ended_at_utc,wallet_credit FROM growth_journal_cycle
                 WHERE account_id=?1 AND cycle_id=?2",
                rusqlite::params![account, old_cycle],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(journal_cycle.0.as_deref(), Some(now.to_rfc3339().as_str()));
        assert_eq!(journal_cycle.1, Some(100));
        assert_eq!(
            ledger
                .connection
                .query_row::<i64, _, _>(
                    "SELECT amount FROM planet_wallet_credit WHERE previous_cycle_id=?1",
                    [&old_cycle],
                    |row| row.get(0),
                )
                .unwrap(),
            100
        );

        // The event arrives after reset but occurred in the closed cycle.
        ledger
            .insert(&crate::collectors::ParsedRecord {
                agent: crate::domain::usage::Agent::Codex,
                kind: crate::collectors::RecordKind::Response,
                event_key: "late-old-cycle-activity".into(),
                occurred_at_utc: chrono::DateTime::parse_from_rfc3339("2026-10-01T10:00:00Z")
                    .unwrap()
                    .with_timezone(&chrono::Utc),
                usage: crate::domain::usage::TokenUsage {
                    input_tokens: None,
                    output_tokens: None,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    total_tokens: Some(10),
                    coverage: crate::domain::usage::UsageCoverage::Complete,
                },
            })
            .unwrap();
        ledger.rebuild_shop_contributions().unwrap();
        assert_eq!(ledger.connection.query_row::<String,_,_>(
            "SELECT cycle_id FROM shop_activity_day WHERE account_id=?1 AND reward_date='2026-10-01'",
            [&account], |row| row.get(0),
        ).unwrap(), old_cycle);
        ledger
            .settle_guest_rewards(now + chrono::Duration::minutes(1))
            .unwrap();
        assert_eq!(
            ledger
                .connection
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM shop_game_reward WHERE account_id=?1 AND kind='streak'",
                    [&account],
                    |row| row.get(0),
                )
                .unwrap(),
            0
        );

        // A current-cycle activity day may use the closed-cycle previous day for continuity.
        let current_effects = crate::domain::cosmetic_shop::ActiveEffects {
            streak_reward_tokens: 1_000,
            ..Default::default()
        };
        let current_effect_revision: i64 = ledger
            .connection
            .query_row(
                "SELECT coalesce(max(revision),0)+1 FROM shop_effect_history WHERE account_id=?1",
                [&account],
                |row| row.get(0),
            )
            .unwrap();
        ledger
            .connection
            .execute(
                "INSERT INTO shop_effect_history(account_id,cycle_id,revision,started_at_utc,
             active_instance_ids_json,effects_json) VALUES (?1,?2,?3,?4,'[]',?5)",
                rusqlite::params![
                    account,
                    result.state.current_cycle_id,
                    current_effect_revision,
                    now.to_rfc3339(),
                    serde_json::to_string(&current_effects).unwrap()
                ],
            )
            .unwrap();
        ledger
            .insert(&crate::collectors::ParsedRecord {
                agent: crate::domain::usage::Agent::Codex,
                kind: crate::collectors::RecordKind::Response,
                event_key: "current-cycle-activity".into(),
                occurred_at_utc: now + chrono::Duration::hours(1),
                usage: crate::domain::usage::TokenUsage {
                    input_tokens: None,
                    output_tokens: None,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    total_tokens: Some(20),
                    coverage: crate::domain::usage::UsageCoverage::Complete,
                },
            })
            .unwrap();
        ledger
            .settle_guest_rewards(now + chrono::Duration::hours(2))
            .unwrap();
        assert_eq!(ledger.connection.query_row::<i64,_,_>(
            "SELECT coalesce(sum(amount),0) FROM shop_game_reward WHERE account_id=?1 AND kind='streak'",
            [&account], |row| row.get(0),
        ).unwrap(), 1_000);

        let replay = ledger
            .apply_guest_shop_request(
                &ShopRequest::ResetPlanet {
                    request_id: "reset-cycle-one".into(),
                    cycle_id: old_cycle.clone(),
                },
                now + chrono::Duration::minutes(1),
            )
            .unwrap();
        assert_eq!(replay.status, ShopActionStatus::Reset);
        assert_eq!(replay.state.current_cycle_id, result.state.current_cycle_id);
        assert_eq!(replay.state.available_balance, 1_101);
        assert_eq!(
            ledger.reset_available_at().unwrap(),
            Some(now + chrono::Duration::hours(18))
        );
        assert_eq!(
            ledger
                .connection
                .query_row(
                    "SELECT count(*) FROM planet_wallet_credit WHERE previous_cycle_id=?1",
                    [&old_cycle],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
    }

    #[test]
    fn purchase_quote_revision_does_not_restart_when_the_planet_cycle_changes() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let account = super::active_account_id(&ledger.connection).unwrap();
        let old_cycle = ledger.planet_cycle_id().unwrap();
        let effects = crate::domain::cosmetic_shop::ActiveEffects::default();
        ledger.connection.execute(
            "INSERT INTO shop_effect_history(account_id,cycle_id,revision,started_at_utc,
             active_instance_ids_json,effects_json) VALUES (?1,?2,1,'2026-10-01T00:00:00Z','[]',?3)",
            rusqlite::params![account,old_cycle,serde_json::to_string(&effects).unwrap()],
        ).unwrap();
        let stale_quote = ledger
            .quote_shop(&QuoteTarget::Purchase {
                sku: "land_market".into(),
            })
            .unwrap();
        assert_eq!(stale_quote.effect_revision, 1);
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-01T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let reset = ledger
            .reset_guest_planet("reset-for-quote-revision", &old_cycle, now)
            .unwrap();
        assert_eq!(reset.status, ShopActionStatus::Reset);

        // The first effect snapshot in a new cycle must not reuse the old cycle's revision.
        super::record_effect_change(
            &ledger.connection,
            &account,
            &reset.state.current_cycle_id,
            now + chrono::Duration::seconds(1),
        )
        .unwrap();
        let current_quote = ledger
            .quote_shop(&QuoteTarget::Purchase {
                sku: "land_market".into(),
            })
            .unwrap();
        assert!(current_quote.effect_revision > stale_quote.effect_revision);
        let before_stale_purchase = ledger.shop_state().unwrap();
        let stale_purchase = ledger
            .apply_guest_shop_request(
                &ShopRequest::Purchase {
                    request_id: "purchase-with-pre-reset-quote".into(),
                    quote: stale_quote,
                },
                now + chrono::Duration::seconds(2),
            )
            .unwrap();
        assert_eq!(stale_purchase.status, ShopActionStatus::QuoteChanged);
        assert_eq!(
            stale_purchase.state.landscape_instances,
            before_stale_purchase.landscape_instances
        );
        assert_eq!(
            stale_purchase.state.avatar_owned_skus,
            before_stale_purchase.avatar_owned_skus
        );
        assert_eq!(
            stale_purchase.state.available_balance,
            before_stale_purchase.available_balance
        );
        assert_eq!(
            ledger
                .connection
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM shop_purchase WHERE account_id=?1 AND sku='land_market'",
                    [&account],
                    |row| row.get(0),
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn guest_reset_cooldown_failure_can_retry_with_the_same_request_id() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let first_cycle = ledger.planet_cycle_id().unwrap();
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-01T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let first = ledger
            .reset_guest_planet("first-reset", &first_cycle, now)
            .unwrap();
        assert_eq!(first.status, ShopActionStatus::Reset);

        let next_cycle = first.state.current_cycle_id.clone();
        let retry_at = ledger.reset_available_at().unwrap().unwrap();
        assert_eq!(retry_at, now + chrono::Duration::hours(24));
        assert!(matches!(
            ledger.reset_guest_planet(
                "second-reset",
                &next_cycle,
                retry_at - chrono::Duration::seconds(1)
            ),
            Err(crate::storage::ledger::ScanError::ResetCooldown),
        ));
        assert_eq!(
            ledger
                .connection
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM shop_action_request WHERE request_id='second-reset'",
                    [],
                    |row| row.get(0),
                )
                .unwrap(),
            0
        );

        let retried = ledger
            .reset_guest_planet("second-reset", &next_cycle, retry_at)
            .unwrap();
        assert_eq!(retried.status, ShopActionStatus::Reset);
        assert_ne!(retried.state.current_cycle_id, next_cycle);
    }

    #[test]
    fn guest_reset_rejects_authenticated_accounts_without_mutation() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        ledger.ensure_planet_account("alice").unwrap();
        let before_cycle = ledger.planet_cycle_id().unwrap();
        let before = ledger.shop_state().unwrap();
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-01T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);

        assert!(matches!(
            ledger.reset_guest_planet("signed-reset", &before_cycle, now),
            Err(crate::storage::ledger::ScanError::InvalidShopState),
        ));
        assert_eq!(ledger.planet_cycle_id().unwrap(), before_cycle);
        assert_eq!(
            ledger.shop_state().unwrap().available_balance,
            before.available_balance
        );
        assert_eq!(
            ledger
                .connection
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM shop_action_request WHERE request_id='signed-reset'",
                    [],
                    |row| row.get(0),
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn guest_shop_rejects_an_instance_owned_by_another_account() {
        let directory = tempfile::tempdir().unwrap();
        let mut ledger =
            Ledger::open(&directory.path().join("ledger.sqlite3"), chrono_tz::UTC).unwrap();
        ledger.ensure_planet_account("bob").unwrap();
        let bob = super::active_account_id(&ledger.connection).unwrap();
        ledger.connection.execute(
            "INSERT INTO shop_landscape_instance(account_id,instance_id,sku,variation_index,seed,variation_version,acquired_at_utc)
             VALUES (?1,'bob-private-pond','land_pond',0,'bob-seed',1,'2026-10-01T00:00:00Z')",
            [&bob],
        ).unwrap();
        ledger
            .connection
            .execute(
                "INSERT INTO shop_landscape_edit_version(account_id,instance_id,version)
             VALUES (?1,'bob-private-pond',0)",
                [&bob],
            )
            .unwrap();

        // Model logout to the guest account while retaining Bob's private rows.
        ledger
            .connection
            .execute(
                "UPDATE setting SET value='local' WHERE key='planet_account_id'",
                [],
            )
            .unwrap();
        let before = ledger.shop_state().unwrap();
        assert_eq!(before.account_id, "local");
        assert_ne!(bob, before.account_id);
        assert!(before.landscape_instances.is_empty());
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-01T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let place = ledger
            .apply_guest_shop_request(
                &ShopRequest::Place {
                    request_id: "foreign-place".into(),
                    cycle_id: before.current_cycle_id.clone(),
                    instance_id: "bob-private-pond".into(),
                    expected_version: 0,
                    x: 160.0,
                    y: 200.0,
                },
                now,
            )
            .unwrap();
        assert_eq!(place.status, ShopActionStatus::NotOwned);
        assert_eq!(place.state.landscape_instances, before.landscape_instances);
        assert_eq!(place.state.placements, before.placements);
        assert_eq!(place.state.available_balance, before.available_balance);
        let retrieve = ledger
            .apply_guest_shop_request(
                &ShopRequest::Retrieve {
                    request_id: "foreign-retrieve".into(),
                    cycle_id: before.current_cycle_id,
                    instance_id: "bob-private-pond".into(),
                    expected_version: 0,
                },
                now,
            )
            .unwrap();
        assert_eq!(retrieve.status, ShopActionStatus::NotOwned);
        let bob_rows: i64 = ledger.connection.query_row(
            "SELECT count(*) FROM shop_landscape_instance WHERE account_id=?1 AND instance_id='bob-private-pond'",
            [&bob], |row| row.get(0),
        ).unwrap();
        assert_eq!(bob_rows, 1);
        let guest_rows: i64 = ledger.connection.query_row(
            "SELECT count(*) FROM shop_landscape_placement WHERE account_id='local' AND instance_id='bob-private-pond'",
            [], |row| row.get(0),
        ).unwrap();
        assert_eq!(guest_rows, 0);
    }

    #[test]
    fn guest_purchase_spends_new_catalog_price_once_and_persists() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.sqlite3");
        let mut ledger = Ledger::open(&path, chrono_tz::UTC).unwrap();
        ledger
            .connection
            .execute(
                "INSERT INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
             VALUES ('seed-cycle',500000,'2026-09-28T00:00:00Z')",
                [],
            )
            .unwrap();

        let first = ledger
            .purchase_guest_cosmetic("11111111-1111-4111-8111-111111111111", "star_cluster_v2")
            .unwrap();
        assert_eq!(first.status, CosmeticPurchaseStatus::Purchased);
        assert_eq!(first.price, 500_000);
        assert_eq!(first.available_balance, 0);

        let replay = ledger
            .purchase_guest_cosmetic("11111111-1111-4111-8111-111111111111", "star_cluster_v2")
            .unwrap();
        assert_eq!(replay, first);
        let duplicate = ledger
            .purchase_guest_cosmetic("22222222-2222-4222-8222-222222222222", "star_cluster_v2")
            .unwrap();
        assert_eq!(duplicate.status, CosmeticPurchaseStatus::AlreadyOwned);
        ledger
            .connection
            .execute(
                "INSERT INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
             VALUES ('later-cycle',100,'2026-09-29T00:00:00Z')",
                [],
            )
            .unwrap();
        let duplicate_replay = ledger
            .purchase_guest_cosmetic("22222222-2222-4222-8222-222222222222", "star_cluster_v2")
            .unwrap();
        assert_eq!(duplicate_replay, duplicate);
        drop(ledger);

        let reopened = Ledger::open(&path, chrono_tz::UTC).unwrap();
        let state = reopened.cosmetic_shop_state().unwrap();
        assert_eq!(state.available_balance, 100);
        assert_eq!(state.owned_skus, vec!["star_cluster_v2"]);
    }

    #[test]
    fn guest_purchase_rejects_insufficient_balance_without_rows() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.sqlite3");
        let mut ledger = Ledger::open(&path, chrono_tz::UTC).unwrap();
        ledger
            .connection
            .execute(
                "INSERT INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
             VALUES ('seed-cycle',499999,'2026-09-28T00:00:00Z')",
                [],
            )
            .unwrap();

        let first =
            ledger.purchase_guest_cosmetic("33333333-3333-4333-8333-333333333333", "aurora_v2");
        let purchases: i64 = ledger
            .connection
            .query_row("SELECT count(*) FROM cosmetic_purchase", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(purchases, 0);
        let first = first.unwrap();
        assert_eq!(first.status, CosmeticPurchaseStatus::InsufficientBalance);
        assert_eq!(first.price, 2_000_000);
        assert_eq!(first.available_balance, 499999);

        ledger
            .connection
            .execute(
                "INSERT INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
             VALUES ('later-cycle',1,'2026-09-29T00:00:00Z')",
                [],
            )
            .unwrap();
        let replay = ledger
            .purchase_guest_cosmetic("33333333-3333-4333-8333-333333333333", "aurora_v2")
            .unwrap();
        assert_eq!(replay.status, CosmeticPurchaseStatus::InsufficientBalance);
        assert_eq!(replay.available_balance, 499999);
        let purchases: i64 = ledger
            .connection
            .query_row("SELECT count(*) FROM cosmetic_purchase", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(purchases, 0);
    }

    #[test]
    fn equipment_is_account_scoped_and_signed_reset_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.sqlite3");
        let mut ledger = Ledger::open(&path, chrono_tz::UTC).unwrap();
        ledger
            .connection
            .execute(
                "INSERT INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
             VALUES ('seed-cycle',1250000,'2026-09-28T00:00:00Z')",
                [],
            )
            .unwrap();
        ledger.ensure_planet_account("alice").unwrap();
        ledger
            .purchase_guest_cosmetic("44444444-4444-4444-8444-444444444444", "star_cluster_v2")
            .unwrap();
        ledger
            .purchase_guest_cosmetic("55555555-5555-4555-8555-555555555555", "pond")
            .unwrap();
        ledger
            .equip_guest_cosmetic("sky", Some("star_cluster_v2"))
            .unwrap();
        ledger
            .equip_guest_cosmetic("forecourt", Some("pond"))
            .unwrap();
        let equipped_cycle = ledger.planet_cycle_id().unwrap();

        ledger.ensure_planet_account("bob").unwrap();
        assert!(ledger.cosmetic_shop_state().unwrap().owned_skus.is_empty());
        assert!(ledger.cosmetic_shop_state().unwrap().equipped.is_empty());

        ledger.ensure_planet_account("alice").unwrap();
        let state = ledger.cosmetic_shop_state().unwrap();
        assert_eq!(state.owned_skus, vec!["pond", "star_cluster_v2"]);
        assert_eq!(state.equipped.len(), 2);
        assert!(state
            .equipped
            .iter()
            .any(|item| item.slot_id == "sky" && item.sku == "star_cluster_v2"));
        assert!(state
            .equipped
            .iter()
            .any(|item| item.slot_id == "forecourt" && item.sku == "pond"));

        assert!(matches!(
            ledger.reset_planet(
                chrono::DateTime::parse_from_rfc3339("2026-09-28T01:00:00Z")
                    .unwrap()
                    .to_utc()
            ),
            Err(crate::storage::ledger::ScanError::InvalidShopState),
        ));
        let state = ledger.cosmetic_shop_state().unwrap();
        assert_eq!(state.current_cycle_id, ledger.planet_cycle_id().unwrap());
        assert_eq!(state.current_cycle_id, equipped_cycle);
        assert_eq!(state.owned_skus, vec!["pond", "star_cluster_v2"]);
        assert_eq!(state.equipped.len(), 2);
    }

    #[test]
    fn unknown_equipment_keys_survive_catalog_projection() {
        let directory = tempfile::tempdir().unwrap();
        let ledger =
            Ledger::open(&directory.path().join("ledger.sqlite3"), chrono_tz::UTC).unwrap();
        let account_id = super::active_account_id(&ledger.connection).unwrap();
        let cycle_id = super::active_cycle_id(&ledger.connection).unwrap();
        ledger
            .connection
            .execute(
                "INSERT INTO cosmetic_equipment(account_id,cycle_id,slot_id,sku,version)
             VALUES (?1,?2,'future_slot','future_item',7)",
                rusqlite::params![account_id, cycle_id],
            )
            .unwrap();

        let state = ledger.cosmetic_shop_state().unwrap();
        assert!(state.equipped.is_empty());
        let stored: (String, String) = ledger
            .connection
            .query_row(
                "SELECT slot_id,sku FROM cosmetic_equipment WHERE account_id=?1 AND cycle_id=?2",
                rusqlite::params![account_id, cycle_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(stored, ("future_slot".into(), "future_item".into()));
    }

    #[test]
    fn unimported_guest_shop_state_without_legacy_import_row_blocks_signed_sync() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let local_account = super::active_account_id(&ledger.connection).unwrap();
        let cycle_id = super::active_cycle_id(&ledger.connection).unwrap();
        ledger.connection.execute(
            "INSERT INTO shop_landscape_instance(account_id,instance_id,sku,variation_index,seed,variation_version,acquired_at_utc)
             VALUES (?1,'guest-tree','land_tree',0,'seed',1,'2026-10-01T00:00:00Z')",
            [&local_account],
        ).unwrap();

        assert!(!ledger.has_unimported_guest_shop_state().unwrap());
        assert!(ledger.has_local_guest_shop_state().unwrap());

        ledger
            .ensure_planet_account("00000000-0000-0000-0000-000000000051")
            .unwrap();

        assert!(ledger.pending_guest_cosmetic_import().unwrap().is_none());
        assert!(ledger.has_unimported_guest_shop_state().unwrap());
        assert_eq!(ledger.planet_cycle_id().unwrap(), cycle_id);
    }

    #[test]
    fn empty_guest_shop_baseline_allows_signed_cold_bootstrap() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        ledger
            .ensure_planet_account("00000000-0000-0000-0000-000000000052")
            .unwrap();

        assert!(ledger.pending_guest_cosmetic_import().unwrap().is_none());
        assert!(!ledger.has_unimported_guest_shop_state().unwrap());
    }

    #[test]
    fn raw_only_guest_era_bookkeeping_allows_first_signed_cold_bootstrap() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let cycle_id = ledger.planet_cycle_id().unwrap();
        let occurred_at = ledger.planet_activation_at().unwrap() + chrono::Duration::seconds(1);
        ledger
            .insert(&crate::collectors::ParsedRecord {
                agent: crate::domain::usage::Agent::Codex,
                kind: crate::collectors::RecordKind::Response,
                event_key: "raw-only-era-progress".into(),
                occurred_at_utc: occurred_at,
                usage: crate::domain::usage::TokenUsage {
                    input_tokens: None,
                    output_tokens: None,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    total_tokens: Some(4_000_000),
                    coverage: crate::domain::usage::UsageCoverage::Complete,
                },
            })
            .unwrap();

        // This is the same contribution/reward lifecycle run by AppState::scan.
        ledger.rebuild_shop_contributions().unwrap();
        ledger
            .settle_guest_rewards(occurred_at + chrono::Duration::minutes(1))
            .unwrap();
        assert!(!ledger.has_local_guest_shop_state().unwrap());
        assert!(
            ledger
                .shop_growth_credit_by_date()
                .unwrap()
                .values()
                .sum::<f64>()
                >= crate::growth::STAGE_THRESHOLDS[0]
        );
        assert_eq!(
            ledger
                .connection
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM shop_era_progress WHERE account_id='local'",
                    [],
                    |row| row.get(0),
                )
                .unwrap(),
            1,
            "the real guest lifecycle records zero-value era progress"
        );
        assert_eq!(
            ledger
                .connection
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM shop_game_reward WHERE account_id='local' AND amount>0",
                    [],
                    |row| row.get(0),
                )
                .unwrap(),
            0
        );
        for table in [
            "shop_landscape_instance",
            "shop_landscape_placement",
            "shop_avatar_owned",
            "shop_purchase",
            "shop_action_request",
            "shop_natural_removal",
        ] {
            let count: i64 = ledger
                .connection
                .query_row(
                    &format!("SELECT count(*) FROM {table} WHERE account_id='local'"),
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 0, "raw usage must not create {table} state");
        }
        assert_eq!(
            ledger
                .connection
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM shop_wallet_credit WHERE account_id='local' AND amount>0",
                    [],
                    |row| row.get(0),
                )
                .unwrap(),
            0
        );
        assert_eq!(ledger.connection.query_row::<i64, _, _>(
            "SELECT count(*) FROM shop_cycle_settlement WHERE account_id='local' AND amount>0",
            [], |row| row.get(0),
        ).unwrap(), 0);
        {
            let mut effects = ledger
                .connection
                .prepare("SELECT effects_json FROM shop_effect_history WHERE account_id='local'")
                .unwrap();
            for row in effects
                .query_map([], |row| row.get::<_, String>(0))
                .unwrap()
            {
                let decoded: crate::domain::cosmetic_shop::ActiveEffects =
                    serde_json::from_str(&row.unwrap()).unwrap();
                assert_eq!(
                    decoded,
                    crate::domain::cosmetic_shop::ActiveEffects::default()
                );
            }
        }

        ledger
            .ensure_planet_account("00000000-0000-0000-0000-000000000055")
            .unwrap();

        assert!(ledger.pending_guest_cosmetic_import().unwrap().is_none());
        assert_eq!(ledger.planet_cycle_id().unwrap(), cycle_id);
        assert_eq!(ledger.planet_usage_totals().unwrap().1, 4_000_000);
        assert!(!ledger.has_unimported_guest_shop_state().unwrap());
    }

    #[test]
    fn unimported_guest_avatar_receipts_removals_rewards_and_effects_hold_signed_sync() {
        let local_rows = [
            "INSERT INTO shop_avatar_owned(account_id,sku,purchase_id,price,acquired_at_utc)
             VALUES ('local','avatar_hat','avatar-purchase',100,'2026-10-01T00:00:00Z')",
            "INSERT INTO shop_purchase(account_id,purchase_id,sku,price,purchased_at_utc)
             VALUES ('local','shop-purchase','land_tree',100,'2026-10-01T00:00:00Z')",
            "INSERT INTO shop_action_request(account_id,request_id,payload_json,result_json,created_at_utc)
             VALUES ('local','action-receipt','{}','{}','2026-10-01T00:00:00Z')",
            "INSERT INTO shop_natural_removal(account_id,cycle_id,stage,ordinal,version,removed_at_utc)
             VALUES ('local','local-cycle',0,0,1,'2026-10-01T00:00:00Z')",
            "INSERT INTO shop_natural_removal_debit(account_id,request_id,amount,created_at_utc)
             VALUES ('local','removal-debit',100,'2026-10-01T00:00:00Z')",
            "INSERT INTO shop_game_reward(account_id,reward_id,trigger_key,kind,cycle_id,amount,effect_snapshot_json,awarded_at_utc)
             VALUES ('local','reward','reward-trigger','era','local-cycle',100,'{}','2026-10-01T00:00:00Z')",
            "INSERT INTO shop_wallet_credit(account_id,credit_id,trigger_key,cycle_id,amount,created_at_utc)
             VALUES ('local','credit','credit-trigger','local-cycle',100,'2026-10-01T00:00:00Z')",
            "INSERT INTO shop_cycle_settlement(account_id,cycle_id,amount,settled_at_utc)
             VALUES ('local','local-cycle',0,'2026-10-01T00:00:00Z')",
            "INSERT INTO shop_effect_history(account_id,cycle_id,revision,started_at_utc,active_instance_ids_json,effects_json)
             VALUES ('local','local-cycle',1,'2026-10-01T00:00:00Z','[]',
               '{\"token_earning_bps\":100,\"civilization_growth_bps\":0,\"shop_discount_bps\":0,\"reset_cooldown_bps\":0,\"natural_removal_discount_bps\":0,\"era_reward_tokens\":0,\"streak_reward_tokens\":0}')",
        ];

        for row in local_rows {
            assert!(
                signed_sync_is_held_for_local_shop_row(row),
                "missed local state row: {row}"
            );
        }
    }

    #[test]
    fn pending_legacy_guest_import_holds_sync_even_when_full_import_is_not_available() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        ledger
            .connection
            .execute(
                "INSERT INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
             VALUES ('legacy-cycle',100,'2026-10-01T00:00:00Z')",
                [],
            )
            .unwrap();
        ledger
            .ensure_planet_account("00000000-0000-0000-0000-000000000054")
            .unwrap();

        assert!(ledger.pending_guest_cosmetic_import().unwrap().is_some());
        assert!(ledger.has_unimported_guest_shop_state().unwrap());
    }

    #[test]
    fn guest_import_failure_retains_guest_state_and_account_ownership() {
        let directory = tempfile::tempdir().unwrap();
        let mut ledger =
            Ledger::open(&directory.path().join("ledger.sqlite3"), chrono_tz::UTC).unwrap();
        ledger
            .connection
            .execute(
                "INSERT INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
             VALUES ('guest-credit',500000,'2026-09-28T00:00:00Z')",
                [],
            )
            .unwrap();
        ledger
            .purchase_guest_cosmetic("66666666-6666-4666-8666-666666666666", "star_cluster_v2")
            .unwrap();

        ledger.ensure_planet_account("alice").unwrap();
        let pending = ledger.pending_guest_cosmetic_import().unwrap().unwrap();
        assert_eq!(pending.wallet_credits.len(), 1);
        assert_eq!(pending.purchases.len(), 1);
        assert_eq!(pending.purchases[0].sku, "star_cluster_v2");
        let account_state = ledger.cosmetic_shop_state().unwrap();
        assert!(account_state.owned_skus.is_empty());
        assert_eq!(account_state.available_balance, 0);

        ledger.ensure_planet_account("bob").unwrap();
        assert!(ledger.pending_guest_cosmetic_import().unwrap().is_none());
        ledger.ensure_planet_account("alice").unwrap();
        let restored = ledger.pending_guest_cosmetic_import().unwrap().unwrap();
        assert_eq!(restored.import_id, pending.import_id);
        assert_eq!(restored.purchases, pending.purchases);
    }

    #[test]
    fn guest_shop_import_capture_is_durable_immutable_and_source_scoped() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.sqlite3");
        let target_account_id = "account:00000000-0000-0000-0000-000000000071";
        let original_snapshot;
        {
            let mut ledger = Ledger::open(&path, chrono_tz::UTC).unwrap();
            let cycle_id = super::active_cycle_id(&ledger.connection).unwrap();
            let profile = crate::domain::planet::PlanetProfile {
                nickname: "Guest planet".into(),
                avatar: crate::domain::planet::PlanetAvatar::Feminine,
            };
            ledger
                .set_planet_profile(&profile.nickname, profile.avatar)
                .unwrap();
            ledger.connection.execute(
                "INSERT INTO shop_landscape_instance(account_id,instance_id,sku,variation_index,seed,variation_version,acquired_at_utc)
                 VALUES ('local','guest-tree','land_tree',1,'stable-seed',2,'2026-10-01T00:00:00Z')",
                [],
            ).unwrap();
            ledger.connection.execute(
                "INSERT INTO shop_landscape_placement(account_id,instance_id,cycle_id,x,y,version)
                 VALUES ('local','guest-tree',?1,0.25,0.75,3)",
                [&cycle_id],
            ).unwrap();
            ledger.connection.execute(
                "INSERT INTO shop_avatar_owned(account_id,sku,purchase_id,price,acquired_at_utc)
                 VALUES ('local','avatar_explorer_hat','avatar-purchase',100,'2026-10-01T00:00:00Z')",
                [],
            ).unwrap();
            ledger
                .connection
                .execute(
                    "INSERT INTO shop_avatar_equipment(account_id,slot,sku,version)
                 VALUES ('local','head','avatar_explorer_hat',2)",
                    [],
                )
                .unwrap();
            ledger
                .connection
                .execute(
                    "INSERT INTO shop_purchase(account_id,purchase_id,sku,price,purchased_at_utc)
                 VALUES ('local','land-purchase','land_tree',100,'2026-10-01T00:00:00Z')",
                    [],
                )
                .unwrap();
            ledger.connection.execute(
                "INSERT INTO shop_effect_history(account_id,cycle_id,revision,started_at_utc,active_instance_ids_json,effects_json)
                 VALUES ('local',?1,1,'2026-10-01T00:00:00Z','[\"guest-tree\"]',
                   '{\"token_earning_bps\":100,\"civilization_growth_bps\":0,\"shop_discount_bps\":0,\"reset_cooldown_bps\":0,\"natural_removal_discount_bps\":0,\"era_reward_tokens\":0,\"streak_reward_tokens\":0}')",
                [&cycle_id],
            ).unwrap();
            ledger
                .connection
                .execute(
                    "INSERT INTO shop_effect_cycle_bound(account_id,cycle_id,started_at_utc)
                 VALUES ('local',?1,'2026-10-01T00:00:00Z')",
                    [&cycle_id],
                )
                .unwrap();
            ledger.connection.execute(
                "INSERT INTO shop_effect_contribution(account_id,device_id,cycle_id,date,effect_revision,canonical_version,tokens,growth_bps,wallet_bps)
                 VALUES ('local','device-local',?1,'2026-10-01',1,2,42,100,200)",
                [&cycle_id],
            ).unwrap();
            ledger.connection.execute(
                "INSERT INTO shop_activity_day(account_id,reward_date,cycle_id,first_occurred_at_utc,canonical_version,tokens)
                 VALUES ('local','2026-10-01',?1,'2026-10-01T00:00:00Z',2,42)",
                [&cycle_id],
            ).unwrap();
            ledger.connection.execute(
                "INSERT INTO shop_wallet_credit(account_id,credit_id,trigger_key,cycle_id,amount,created_at_utc)
                 VALUES ('local','reward-credit','reward-trigger',?1,12,'2026-10-01T00:00:00Z')",
                [&cycle_id],
            ).unwrap();
            ledger
                .connection
                .execute(
                    "INSERT INTO shop_cycle_settlement(account_id,cycle_id,amount,settled_at_utc)
                 VALUES ('local',?1,42,'2026-10-01T00:00:00Z')",
                    [&cycle_id],
                )
                .unwrap();
            ledger.connection.execute(
                "INSERT INTO shop_game_reward(account_id,reward_id,trigger_key,kind,cycle_id,amount,effect_snapshot_json,awarded_at_utc)
                 VALUES ('local','era-reward','era:local:1','era',?1,7,
                   '{\"token_earning_bps\":0,\"civilization_growth_bps\":0,\"shop_discount_bps\":0,\"reset_cooldown_bps\":0,\"natural_removal_discount_bps\":0,\"era_reward_tokens\":0,\"streak_reward_tokens\":0}',
                   '2026-10-01T00:00:00Z')",
                [&cycle_id],
            ).unwrap();
            ledger.connection.execute(
                "INSERT INTO shop_era_progress(account_id,cycle_id,stage,trigger_key,effect_snapshot_json,awarded_at_utc)
                 VALUES ('local',?1,1,'era:local:1',
                   '{\"token_earning_bps\":0,\"civilization_growth_bps\":0,\"shop_discount_bps\":0,\"reset_cooldown_bps\":0,\"natural_removal_discount_bps\":0,\"era_reward_tokens\":0,\"streak_reward_tokens\":0}',
                   '2026-10-01T00:00:00Z')",
                [&cycle_id],
            ).unwrap();
            ledger.connection.execute(
                "INSERT INTO shop_natural_removal(account_id,cycle_id,stage,ordinal,version,removed_at_utc)
                 VALUES ('local',?1,0,2,4,'2026-10-01T00:00:00Z')",
                [&cycle_id],
            ).unwrap();
            ledger.connection.execute(
                "INSERT INTO shop_natural_removal_debit(account_id,request_id,amount,created_at_utc)
                 VALUES ('local','remove-request',9,'2026-10-01T00:00:00Z')",
                [],
            ).unwrap();
            ledger.connection.execute(
                "INSERT INTO growth_journal_cycle(account_id,cycle_id,started_at_utc,ended_at_utc,wallet_credit,wallet_credit_at_utc)
                 VALUES ('local','guest-historical-cycle','2026-09-01T00:00:00Z','2026-10-01T00:00:00Z',21,'2026-10-01T00:00:00Z')",
                [],
            ).unwrap();
            ledger.connection.execute(
                "INSERT INTO shop_effect_cycle_bound(account_id,cycle_id,started_at_utc,ended_at_utc)
                 VALUES ('local','guest-historical-cycle','2026-09-01T00:00:00Z','2026-10-01T00:00:00Z')",
                [],
            ).unwrap();
            ledger
                .connection
                .execute(
                    "INSERT INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
                 VALUES ('legacy-wallet-claim',500,'2026-10-01T00:00:00Z')",
                    [],
                )
                .unwrap();
            ledger
                .insert(&crate::collectors::ParsedRecord {
                    agent: crate::domain::usage::Agent::Codex,
                    kind: crate::collectors::RecordKind::Response,
                    event_key: "raw-private-log-sentinel".into(),
                    occurred_at_utc: chrono::DateTime::parse_from_rfc3339("2026-10-01T00:00:01Z")
                        .unwrap()
                        .with_timezone(&chrono::Utc),
                    usage: crate::domain::usage::TokenUsage {
                        input_tokens: None,
                        output_tokens: None,
                        cache_read_tokens: None,
                        cache_write_tokens: None,
                        total_tokens: Some(42),
                        coverage: crate::domain::usage::UsageCoverage::Complete,
                    },
                })
                .unwrap();
            let signed_account_id = "account:00000000-0000-0000-0000-000000000099";
            ledger.connection.execute(
                "INSERT INTO usage_record(event_key,source_id,agent,kind,bucket_date,occurred_at_utc,total_tokens,coverage,parser_version)
                 VALUES ('signed-private-log-sentinel','other-private-source','codex','response',
                   '2026-10-01','2026-10-01T00:00:02Z',999,'complete',1)",
                [],
            ).unwrap();
            ledger.connection.execute(
                "INSERT INTO planet_usage_owner(event_key,account_id) VALUES ('signed-private-log-sentinel',?1)",
                [signed_account_id],
            ).unwrap();

            let changes_before_capture = ledger.connection.total_changes();
            let first = ledger.capture_guest_shop_import(target_account_id).unwrap();
            assert_eq!(ledger.connection.total_changes(), changes_before_capture + 1,
                "capture may persist its own immutable row but must not rebuild or settle source state");
            assert_eq!(first.snapshot.target_account_id, target_account_id);
            assert_eq!(first.snapshot.source_account_id, "local");
            assert_eq!(
                first.snapshot.disposition,
                crate::domain::cosmetic_shop::GuestShopImportDisposition::SourceUnverifiable
            );
            assert!(first.source_matches_current);
            let encoded = serde_json::to_string(&first.snapshot).unwrap();
            for field in [
                "historical_cycles",
                "landscape_instances",
                "placements",
                "avatar_owned",
                "avatar_equipment",
                "purchases",
                "removals",
                "effect_history",
                "effect_cycle_bounds",
                "contributions",
                "activity_days",
                "rewards",
                "wallet_credits",
                "settlements",
                "era_progress",
                "usage_aggregates",
            ] {
                assert!(
                    encoded.contains(field),
                    "snapshot is missing typed section {field}"
                );
            }
            assert!(
                !encoded.contains("raw-private-log-sentinel"),
                "capture must not include raw log identities"
            );
            assert!(
                !encoded.contains("signed-private-log-sentinel"),
                "capture must not include another account's raw usage identity"
            );
            assert_eq!(first.snapshot.data.landscape_instances.len(), 1);
            assert_eq!(
                first.snapshot.data.landscape_instances[0].instance_id,
                "guest-tree"
            );
            assert_eq!(
                first.snapshot.data.landscape_instances[0].seed,
                "stable-seed"
            );
            assert_eq!(
                first.snapshot.data.landscape_instances[0].variation_version,
                2
            );
            assert_eq!(
                first.snapshot.data.profile.as_ref().unwrap().nickname,
                "Guest planet"
            );
            assert_eq!(first.snapshot.data.placements.len(), 1);
            assert_eq!(first.snapshot.data.placements[0].cycle_id, cycle_id);
            assert_eq!(first.snapshot.data.placements[0].x, 0.25);
            assert_eq!(first.snapshot.data.placements[0].version, 3);
            assert_eq!(
                first.snapshot.data.avatar_owned[0].sku,
                "avatar_explorer_hat"
            );
            assert_eq!(first.snapshot.data.avatar_equipment[0].slot, "head");
            assert_eq!(
                first.snapshot.data.effect_history[0]
                    .effects
                    .token_earning_bps,
                100
            );
            assert_eq!(first.snapshot.data.effect_contributions[0].tokens, 42);
            assert_eq!(
                first.snapshot.data.effect_contributions[0].canonical_version,
                2
            );
            assert_eq!(first.snapshot.data.activity_days[0].tokens, 42);
            assert_eq!(first.snapshot.data.game_rewards[0].amount, 7);
            assert_eq!(first.snapshot.data.era_progress[0].stage, 1);
            assert_eq!(first.snapshot.data.wallet_credits[0].amount, 12);
            assert_eq!(first.snapshot.data.cycle_settlements[0].amount, 42);
            assert_eq!(
                first.snapshot.data.unverified_planet_wallet_claims[0].claimed_amount, 500,
                "legacy planet wallet amounts are preserved as claims, never grantable credits"
            );
            assert_eq!(first.snapshot.data.natural_removals[0].ordinal, 2);
            assert_eq!(first.snapshot.data.removal_debits[0].amount, 9);
            assert!(first
                .snapshot
                .data
                .historical_cycles
                .iter()
                .any(|cycle| cycle.cycle_id == "guest-historical-cycle"
                    && cycle.ended_at_utc.as_deref() == Some("2026-10-01T00:00:00Z")));
            assert!(first
                .snapshot
                .data
                .usage_aggregates
                .iter()
                .any(|aggregate| aggregate.bucket_date == "2026-10-01"
                    && aggregate.total_tokens == Some(42)));
            assert!(
                !first
                    .snapshot
                    .data
                    .usage_aggregates
                    .iter()
                    .any(|aggregate| aggregate.total_tokens == Some(999)),
                "usage owned by another account must not enter guest aggregates"
            );
            assert_eq!(first.snapshot.data.lifetime_usage_tokens, Some(42));
            assert_eq!(
                first.snapshot.data.current_cycle_usage_tokens, None,
                "unassigned raw aggregates do not acquire an invented cycle boundary"
            );
            original_snapshot = first.snapshot;

            ledger
                .connection
                .execute(
                    "DELETE FROM usage_record WHERE event_key='signed-private-log-sentinel'",
                    [],
                )
                .unwrap();
            let after_foreign_usage_removed = ledger
                .pending_guest_shop_import(target_account_id)
                .unwrap()
                .unwrap();
            assert!(
                after_foreign_usage_removed.source_matches_current,
                "foreign-account usage must not affect the local source fingerprint"
            );
            assert_eq!(after_foreign_usage_removed.snapshot, original_snapshot);

            let changes_before_same_source_retry = ledger.connection.total_changes();
            let same_source_retry = ledger.capture_guest_shop_import(target_account_id).unwrap();
            assert_eq!(same_source_retry.snapshot, original_snapshot);
            assert!(same_source_retry.source_matches_current);
            assert_eq!(
                ledger.connection.total_changes(),
                changes_before_same_source_retry,
                "same-source retry reuses the captured UUID without writing another row"
            );

            ledger.connection.execute(
                "UPDATE shop_landscape_placement SET x=0.5 WHERE account_id='local' AND instance_id='guest-tree'",
                [],
            ).unwrap();
            let changes_before_retry = ledger.connection.total_changes();
            let retry = ledger.capture_guest_shop_import(target_account_id).unwrap();
            assert_eq!(
                ledger.connection.total_changes(),
                changes_before_retry,
                "retry must not replace the immutable snapshot or mutate source rows"
            );
            assert_eq!(
                retry.snapshot, original_snapshot,
                "a retry must retain the captured payload and import ID"
            );
            assert!(
                !retry.source_matches_current,
                "changed source must be marked stale"
            );
            assert_eq!(
                ledger.cosmetic_account_id().unwrap(),
                "local",
                "capture must not switch ownership"
            );
            assert_eq!(
                ledger
                    .connection
                    .query_row::<i64, _, _>(
                        "SELECT count(*) FROM shop_landscape_instance WHERE account_id='local'",
                        [],
                        |row| row.get(0),
                    )
                    .unwrap(),
                1,
                "capture must leave source rows in place"
            );
        }

        let reopened = Ledger::open(&path, chrono_tz::UTC).unwrap();
        let changes_before_pending_read = reopened.connection.total_changes();
        let pending = reopened
            .pending_guest_shop_import(target_account_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            reopened.connection.total_changes(),
            changes_before_pending_read,
            "reading the durable capture must be pure"
        );
        assert_eq!(pending.snapshot, original_snapshot);
        assert!(!pending.source_matches_current);
        assert_eq!(reopened.cosmetic_account_id().unwrap(), "local");
    }

    #[test]
    fn guest_shop_import_integrity_covers_geometry_timestamps_and_timezones() {
        use crate::domain::cosmetic_shop::GuestShopImportDisposition;

        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let cycle_id = super::active_cycle_id(&ledger.connection).unwrap();
        ledger
            .connection
            .execute(
                "UPDATE setting SET value='not-a-timestamp' WHERE key='planet_activation_at_utc'",
                [],
            )
            .unwrap();
        ledger
            .connection
            .execute(
                "UPDATE setting SET value='Mars/Phobos' WHERE key='planet_timezone'",
                [],
            )
            .unwrap();
        ledger.connection.execute(
            "INSERT INTO shop_landscape_instance(account_id,instance_id,sku,variation_index,seed,variation_version,acquired_at_utc)
             VALUES ('local','bad-placement','land_tree',0,'seed',1,'2026-10-01T00:00:00Z')",
            [],
        ).unwrap();
        ledger
            .connection
            .execute(
                "INSERT INTO shop_landscape_placement(account_id,instance_id,cycle_id,x,y,version)
             VALUES ('local','bad-placement',?1,1390,500,1)",
                [&cycle_id],
            )
            .unwrap();

        let captured = ledger
            .capture_guest_shop_import("account:00000000-0000-0000-0000-000000000074")
            .unwrap();

        assert_eq!(
            captured.snapshot.disposition,
            GuestShopImportDisposition::SourceUnverifiable
        );
        let issues = serde_json::to_value(&captured.snapshot.data.integrity_issues).unwrap();
        let issues = issues.as_array().unwrap();
        assert!(issues
            .iter()
            .any(|issue| issue["kind"] == "invalid_placement_geometry"
                && issue["instance_id"] == "bad-placement"));
        assert!(issues
            .iter()
            .any(|issue| issue["kind"] == "invalid_timestamp"
                && issue["field"] == "activation_at_utc"));
        assert!(issues.iter().any(
            |issue| issue["kind"] == "invalid_timezone" && issue["field"] == "planet_timezone"
        ));
    }

    #[test]
    fn guest_shop_import_marks_overcapacity_natural_layout_unverifiable_without_geometry_work() {
        use crate::domain::cosmetic_shop::GuestShopImportDisposition;

        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let cycle_id = super::active_cycle_id(&ledger.connection).unwrap();
        insert_natural_objects(&ledger, &cycle_id, 155);

        let captured = ledger
            .capture_guest_shop_import("account:00000000-0000-0000-0000-000000000078")
            .unwrap();

        assert_eq!(
            captured.snapshot.disposition,
            GuestShopImportDisposition::SourceUnverifiable
        );
        let issues = serde_json::to_value(&captured.snapshot.data.integrity_issues).unwrap();
        assert!(issues
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["kind"] == "too_many_natural_objects" && issue["count"] == 155));
    }

    #[test]
    fn guest_shop_import_parses_planet_object_seed_text() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let cycle_id = super::active_cycle_id(&ledger.connection).unwrap();
        insert_natural_objects(&ledger, &cycle_id, 1);

        let captured = ledger
            .capture_guest_shop_import("account:00000000-0000-0000-0000-000000000081")
            .unwrap();

        assert_eq!(captured.snapshot.data.natural_objects.len(), 1);
        assert_eq!(captured.snapshot.data.natural_objects[0].seed, 0);
    }

    #[test]
    fn guest_shop_import_bounds_geometry_work_for_overcapacity_valid_sku_placement() {
        use crate::domain::cosmetic_shop::GuestShopImportDisposition;

        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let cycle_id = super::active_cycle_id(&ledger.connection).unwrap();
        insert_natural_objects(&ledger, &cycle_id, 155);
        ledger.connection.execute(
            "INSERT INTO shop_landscape_instance(account_id,instance_id,sku,variation_index,seed,variation_version,acquired_at_utc)
             VALUES ('local','tree-instance','land_tree',0,'seed',1,'2026-10-01T00:00:00Z')",
            [],
        ).unwrap();
        ledger
            .connection
            .execute(
                "INSERT INTO shop_landscape_placement(account_id,instance_id,cycle_id,x,y,version)
             VALUES ('local','tree-instance',?1,100,200,1)",
                [&cycle_id],
            )
            .unwrap();

        let captured = ledger
            .capture_guest_shop_import("account:00000000-0000-0000-0000-000000000079")
            .unwrap();

        assert_eq!(
            captured.snapshot.disposition,
            GuestShopImportDisposition::SourceUnverifiable
        );
        let issues = serde_json::to_value(&captured.snapshot.data.integrity_issues).unwrap();
        assert!(issues
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["kind"] == "too_many_natural_objects" && issue["count"] == 155));
    }

    #[test]
    fn guest_shop_import_pending_rejects_oversized_mutated_snapshot_before_geometry_validation() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let cycle_id = super::active_cycle_id(&ledger.connection).unwrap();
        let target_account_id = "account:00000000-0000-0000-0000-000000000080";
        ledger.connection.execute(
            "INSERT INTO shop_landscape_instance(account_id,instance_id,sku,variation_index,seed,variation_version,acquired_at_utc)
             VALUES ('local','tree-instance','land_tree',0,'seed',1,'2026-10-01T00:00:00Z')",
            [],
        ).unwrap();
        ledger
            .connection
            .execute(
                "INSERT INTO shop_landscape_placement(account_id,instance_id,cycle_id,x,y,version)
             VALUES ('local','tree-instance',?1,100,200,1)",
                [&cycle_id],
            )
            .unwrap();
        ledger.capture_guest_shop_import(target_account_id).unwrap();
        let mut snapshot: serde_json::Value = ledger
            .connection
            .query_row(
                "SELECT snapshot_json FROM guest_shop_import_capture WHERE target_account_id=?1",
                [target_account_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap()
            .parse()
            .unwrap();
        snapshot["data"]["natural_objects"] = serde_json::Value::Array(
            (0..155)
                .map(|ordinal| {
                    serde_json::json!({
                        "cycle_id": cycle_id,
                        "stage": 0,
                        "ordinal": ordinal,
                        "kind": "tree",
                        "x": 25,
                        "y": 50,
                        "seed": ordinal,
                    })
                })
                .collect(),
        );
        ledger
            .connection
            .execute(
                "UPDATE guest_shop_import_capture SET snapshot_json=?2 WHERE target_account_id=?1",
                params![target_account_id, snapshot.to_string()],
            )
            .unwrap();

        assert!(ledger.pending_guest_shop_import(target_account_id).is_err());
    }

    #[test]
    fn guest_shop_import_rejects_nonfinite_placement_coordinates_before_freezing_json() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let cycle_id = super::active_cycle_id(&ledger.connection).unwrap();
        ledger.connection.execute(
            "INSERT INTO shop_landscape_instance(account_id,instance_id,sku,variation_index,seed,variation_version,acquired_at_utc)
             VALUES ('local','nonfinite-placement','land_tree',0,'seed',1,'2026-10-01T00:00:00Z')",
            [],
        ).unwrap();
        ledger
            .connection
            .execute(
                "INSERT INTO shop_landscape_placement(account_id,instance_id,cycle_id,x,y,version)
             VALUES ('local','nonfinite-placement',?1,?2,100,1)",
                params![cycle_id, f64::INFINITY],
            )
            .unwrap();

        let captured =
            ledger.capture_guest_shop_import("account:00000000-0000-0000-0000-000000000077");

        assert!(
            captured.is_err(),
            "nonfinite coordinates cannot be represented faithfully in the JSON snapshot"
        );
        let frozen_rows: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM guest_shop_import_capture",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(frozen_rows, 0);
    }

    #[test]
    fn guest_shop_import_integrity_covers_effect_bounds_and_activity_cycle_references() {
        use crate::domain::cosmetic_shop::GuestShopImportDisposition;

        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let cycle_id = super::active_cycle_id(&ledger.connection).unwrap();
        let started_at: String = ledger
            .connection
            .query_row(
                "SELECT value FROM setting WHERE key='planet_cycle_started_at_utc'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        ledger
            .connection
            .execute(
                "INSERT INTO shop_effect_cycle_bounds_state(account_id) VALUES ('local')",
                [],
            )
            .unwrap();
        ledger.connection.execute(
            "INSERT INTO shop_effect_cycle_bound(account_id,cycle_id,started_at_utc,ended_at_utc)
             VALUES ('local',?1,?2,'2020-01-01T00:00:00Z')",
            params![cycle_id, started_at],
        ).unwrap();
        ledger.connection.execute(
            "INSERT INTO shop_effect_history(account_id,cycle_id,revision,started_at_utc,ended_at_utc,active_instance_ids_json,effects_json)
             VALUES ('local',?1,1,?2,'2019-01-01T00:00:00Z','[]',
               '{\"token_earning_bps\":0,\"civilization_growth_bps\":0,\"shop_discount_bps\":0,\"reset_cooldown_bps\":0,\"natural_removal_discount_bps\":0,\"era_reward_tokens\":0,\"streak_reward_tokens\":0}')",
            params![cycle_id, started_at],
        ).unwrap();
        ledger.connection.execute(
            "INSERT INTO shop_activity_day(account_id,reward_date,cycle_id,first_occurred_at_utc,canonical_version,tokens)
             VALUES ('local','2026-10-01','orphan-cycle','2026-10-01T00:00:00Z',1,1)",
            [],
        ).unwrap();

        let captured = ledger
            .capture_guest_shop_import("account:00000000-0000-0000-0000-000000000075")
            .unwrap();

        assert_eq!(
            captured.snapshot.disposition,
            GuestShopImportDisposition::SourceUnverifiable
        );
        let issues = serde_json::to_value(&captured.snapshot.data.integrity_issues).unwrap();
        let issues = issues.as_array().unwrap();
        assert!(issues
            .iter()
            .any(|issue| issue["kind"] == "invalid_effect_timeline"));
        assert!(issues
            .iter()
            .any(|issue| issue["kind"] == "activity_cycle_unknown"
                && issue["cycle_id"] == "orphan-cycle"));
    }

    #[test]
    fn guest_shop_import_reads_reject_mutated_payload_and_inconsistent_disposition() {
        use crate::domain::cosmetic_shop::GuestShopImportDisposition;

        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let target_account_id = "account:00000000-0000-0000-0000-000000000076";
        let captured = ledger.capture_guest_shop_import(target_account_id).unwrap();
        assert_eq!(
            captured.snapshot.disposition,
            GuestShopImportDisposition::LocalIntegrityValidated
        );

        let mut persisted: serde_json::Value = ledger
            .connection
            .query_row(
                "SELECT snapshot_json FROM guest_shop_import_capture WHERE target_account_id=?1",
                [target_account_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap()
            .parse()
            .unwrap();
        persisted["data"]["reward_timezone"] = serde_json::Value::String("Pacific/Apia".into());
        ledger
            .connection
            .execute(
                "UPDATE guest_shop_import_capture SET snapshot_json=?2 WHERE target_account_id=?1",
                params![target_account_id, persisted.to_string()],
            )
            .unwrap();
        assert!(ledger.pending_guest_shop_import(target_account_id).is_err());

        persisted["data"]["reward_timezone"] =
            serde_json::Value::String(captured.snapshot.data.reward_timezone.clone());
        persisted["disposition"] = serde_json::Value::String("source_unverifiable".into());
        ledger
            .connection
            .execute(
                "UPDATE guest_shop_import_capture SET snapshot_json=?2 WHERE target_account_id=?1",
                params![target_account_id, persisted.to_string()],
            )
            .unwrap();
        assert!(ledger.pending_guest_shop_import(target_account_id).is_err());
        assert!(ledger.capture_guest_shop_import(target_account_id).is_err());
    }

    #[test]
    fn guest_shop_import_marks_missing_successful_purchase_receipt_unverifiable() {
        use crate::domain::cosmetic_shop::GuestShopImportDisposition;

        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let old_cycle = ledger.planet_cycle_id().unwrap();
        ledger
            .connection
            .execute(
                "UPDATE setting SET value='2026-09-29T00:00:00Z'
             WHERE key IN ('planet_activation_at_utc','planet_cycle_started_at_utc')",
                [],
            )
            .unwrap();
        ledger
            .insert(&crate::collectors::ParsedRecord {
                agent: crate::domain::usage::Agent::Codex,
                kind: crate::collectors::RecordKind::Response,
                event_key: "proof-purchase-funding-event".into(),
                occurred_at_utc: chrono::DateTime::parse_from_rfc3339("2026-09-30T10:00:00Z")
                    .unwrap()
                    .with_timezone(&chrono::Utc),
                usage: crate::domain::usage::TokenUsage {
                    input_tokens: None,
                    output_tokens: None,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    total_tokens: Some(6_000_000),
                    coverage: crate::domain::usage::UsageCoverage::Complete,
                },
            })
            .unwrap();
        let reset_at = chrono::DateTime::parse_from_rfc3339("2026-10-01T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let reset = ledger
            .reset_guest_planet("proof-funding-reset", &old_cycle, reset_at)
            .unwrap();
        assert_eq!(reset.status, ShopActionStatus::Reset);
        let quote = ledger
            .quote_shop(&QuoteTarget::Purchase {
                sku: "land_pond".into(),
            })
            .unwrap();
        let purchase = ledger
            .apply_guest_shop_request(
                &ShopRequest::Purchase {
                    request_id: "proof-missing-purchase-receipt".into(),
                    quote,
                },
                reset_at + chrono::Duration::seconds(1),
            )
            .unwrap();
        assert_eq!(purchase.status, ShopActionStatus::Purchased);
        assert_eq!(purchase.state.landscape_instances.len(), 1);
        let verified = ledger
            .capture_guest_shop_import("account:00000000-0000-0000-0000-000000000084")
            .unwrap();
        let verified_proofs = serde_json::to_value(&verified.snapshot.data).unwrap();
        let verified_proofs = verified_proofs["purchase_proofs"].as_array().unwrap();
        assert_eq!(verified_proofs.len(), 1);
        assert_eq!(
            verified_proofs[0]["request_id"],
            "proof-missing-purchase-receipt"
        );
        assert_eq!(verified_proofs[0]["status"], "purchased");
        assert_eq!(verified_proofs[0]["quote"]["target"]["sku"], "land_pond");
        assert_eq!(verified_proofs[0]["ownership"]["kind"], "landscape");
        assert_eq!(
            verified_proofs[0]["ownership"]["instance_id"],
            purchase.state.landscape_instances[0].instance_id,
        );
        ledger
            .connection
            .execute(
                "DELETE FROM shop_action_request WHERE account_id='local' AND request_id=?1",
                ["proof-missing-purchase-receipt"],
            )
            .unwrap();

        let captured = ledger
            .capture_guest_shop_import("account:00000000-0000-0000-0000-000000000085")
            .unwrap();

        assert_eq!(
            captured.snapshot.disposition,
            GuestShopImportDisposition::SourceUnverifiable
        );
        assert_eq!(captured.snapshot.data.purchases.len(), 1);
        let issues = serde_json::to_value(&captured.snapshot.data.integrity_issues).unwrap();
        assert!(
            issues
                .as_array()
                .unwrap()
                .iter()
                .any(|issue| issue["kind"] == "purchase_proof_unverifiable"
                    && issue["purchase_id"] == "proof-missing-purchase-receipt"),
            "a missing successful receipt needs its own typed integrity issue"
        );
    }

    #[test]
    fn guest_shop_import_binds_removed_tombstone_to_successful_receipt() {
        use crate::domain::cosmetic_shop::GuestShopImportDisposition;

        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let old_cycle = ledger.planet_cycle_id().unwrap();
        ledger
            .connection
            .execute(
                "UPDATE setting SET value='2026-09-29T00:00:00Z'
             WHERE key IN ('planet_activation_at_utc','planet_cycle_started_at_utc')",
                [],
            )
            .unwrap();
        ledger
            .insert(&crate::collectors::ParsedRecord {
                agent: crate::domain::usage::Agent::Codex,
                kind: crate::collectors::RecordKind::Response,
                event_key: "proof-removal-funding-event".into(),
                occurred_at_utc: chrono::DateTime::parse_from_rfc3339("2026-09-30T10:00:00Z")
                    .unwrap()
                    .with_timezone(&chrono::Utc),
                usage: crate::domain::usage::TokenUsage {
                    input_tokens: None,
                    output_tokens: None,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    total_tokens: Some(100_000),
                    coverage: crate::domain::usage::UsageCoverage::Complete,
                },
            })
            .unwrap();
        let reset_at = chrono::DateTime::parse_from_rfc3339("2026-10-01T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let reset = ledger
            .reset_guest_planet("proof-removal-funding-reset", &old_cycle, reset_at)
            .unwrap();
        assert_eq!(reset.status, ShopActionStatus::Reset);
        let cycle_id = reset.state.current_cycle_id;
        ledger
            .ensure_planet_object(0, 0, "tree", 25, 50, 17)
            .unwrap();
        let target = crate::domain::cosmetic_shop::NaturalObjectKey {
            cycle_id: cycle_id.clone(),
            stage: 0,
            ordinal: 0,
        };
        let quote = ledger
            .quote_shop(&QuoteTarget::RemoveNatural {
                key: target.clone(),
            })
            .unwrap();
        let removed = ledger
            .apply_guest_shop_request(
                &ShopRequest::RemoveNatural {
                    request_id: "proof-removed-tree".into(),
                    key: target.clone(),
                    expected_version: 0,
                    quote,
                },
                reset_at + chrono::Duration::seconds(1),
            )
            .unwrap();
        assert_eq!(removed.status, ShopActionStatus::Removed);
        assert_eq!(removed.state.removed_natural_keys, vec![target.clone()]);

        let verified = ledger
            .capture_guest_shop_import("account:00000000-0000-0000-0000-000000000086")
            .unwrap();
        let verified_wire = serde_json::to_value(&verified.snapshot.data).unwrap();
        let proof = &verified_wire["removal_proofs"].as_array().unwrap()[0];
        assert_eq!(proof["request_id"], "proof-removed-tree");
        assert_eq!(proof["status"], "removed");
        assert_eq!(proof["target"]["cycle_id"], cycle_id);
        assert_eq!(proof["target"]["stage"], 0);
        assert_eq!(proof["target"]["ordinal"], 0);
        assert_eq!(proof["quote"]["target"]["key"]["cycle_id"], cycle_id);
        assert_eq!(
            proof["quote"]["price"],
            verified_wire["removal_debits"][0]["amount"]
        );

        ledger
            .connection
            .execute(
                "DELETE FROM shop_action_request WHERE account_id='local' AND request_id=?1",
                ["proof-removed-tree"],
            )
            .unwrap();
        let captured = ledger
            .capture_guest_shop_import("account:00000000-0000-0000-0000-000000000087")
            .unwrap();
        assert_eq!(
            captured.snapshot.disposition,
            GuestShopImportDisposition::SourceUnverifiable
        );
        let issues = serde_json::to_value(&captured.snapshot.data.integrity_issues).unwrap();
        assert!(
            issues
                .as_array()
                .unwrap()
                .iter()
                .any(|issue| issue["kind"] == "removal_proof_unverifiable"
                    && issue["request_id"] == "proof-removed-tree"),
            "a missing removal receipt needs its own typed integrity issue"
        );
    }

    #[test]
    fn guest_shop_import_rejects_reset_receipt_from_another_account() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let previous_cycle_id = ledger.planet_cycle_id().unwrap();
        ledger
            .connection
            .execute(
                "UPDATE setting SET value='2026-09-29T00:00:00Z'
             WHERE key IN ('planet_activation_at_utc','planet_cycle_started_at_utc')",
                [],
            )
            .unwrap();
        ledger
            .insert(&crate::collectors::ParsedRecord {
                agent: crate::domain::usage::Agent::Codex,
                kind: crate::collectors::RecordKind::Response,
                event_key: "proof-reset-account-event".into(),
                occurred_at_utc: chrono::DateTime::parse_from_rfc3339("2026-09-30T10:00:00Z")
                    .unwrap()
                    .with_timezone(&chrono::Utc),
                usage: crate::domain::usage::TokenUsage {
                    input_tokens: None,
                    output_tokens: None,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    total_tokens: Some(6_000_000),
                    coverage: crate::domain::usage::UsageCoverage::Complete,
                },
            })
            .unwrap();
        let reset_at = chrono::DateTime::parse_from_rfc3339("2026-10-01T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let reset = ledger
            .reset_guest_planet("proof-reset-account-bound", &previous_cycle_id, reset_at)
            .unwrap();
        assert_eq!(reset.status, ShopActionStatus::Reset);

        let valid_capture = ledger
            .capture_guest_shop_import("account:00000000-0000-0000-0000-000000000088")
            .unwrap();
        assert!(
            valid_capture
                .snapshot
                .data
                .reset_settlement_proofs
                .iter()
                .any(|proof| proof.request_id == "proof-reset-account-bound"),
            "the real local reset receipt should produce a typed proof"
        );

        let mut result: serde_json::Value = ledger.connection.query_row(
            "SELECT result_json FROM shop_action_request WHERE account_id='local' AND request_id=?1",
            ["proof-reset-account-bound"],
            |row| row.get::<_, String>(0),
        ).unwrap().parse().unwrap();
        result["state"]["account_id"] = serde_json::Value::String("account:foreign".into());
        ledger.connection.execute(
            "UPDATE shop_action_request SET result_json=?2 WHERE account_id='local' AND request_id=?1",
            params!["proof-reset-account-bound", result.to_string()],
        ).unwrap();

        let tampered_capture = ledger
            .capture_guest_shop_import("account:00000000-0000-0000-0000-000000000089")
            .unwrap();
        assert!(
            !tampered_capture
                .snapshot
                .data
                .reset_settlement_proofs
                .iter()
                .any(|proof| proof.request_id == "proof-reset-account-bound"),
            "a reset receipt whose result names another account cannot prove the local reset"
        );
        let issues =
            serde_json::to_value(&tampered_capture.snapshot.data.integrity_issues).unwrap();
        assert!(
            issues
                .as_array()
                .unwrap()
                .iter()
                .any(|issue| issue["kind"] == "reset_proof_unverifiable"
                    && issue["previous_cycle_id"] == previous_cycle_id),
            "an unproven reset settlement needs a specific typed integrity issue"
        );
        assert!(tampered_capture.snapshot.data.reset_receipts_unverifiable);
    }

    #[test]
    fn signed_account_cannot_capture_guest_shop_import() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let account_id = "00000000-0000-0000-0000-000000000072";
        ledger.ensure_planet_account(account_id).unwrap();
        let before_account = ledger.cosmetic_account_id().unwrap();
        let before_cycle = ledger.planet_cycle_id().unwrap();

        let result = ledger.capture_guest_shop_import(&format!("account:{account_id}"));

        assert!(
            result.is_err(),
            "capture is allowed only while local guest state remains the source"
        );
        assert_eq!(ledger.cosmetic_account_id().unwrap(), before_account);
        assert_eq!(ledger.planet_cycle_id().unwrap(), before_cycle);
        assert!(ledger
            .pending_guest_shop_import(&format!("account:{account_id}"))
            .unwrap()
            .is_none());
    }

    #[test]
    fn empty_local_guest_capture_is_only_local_integrity_validated() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let target_account_id = "account:00000000-0000-0000-0000-000000000073";

        let captured = ledger.capture_guest_shop_import(target_account_id).unwrap();

        assert_eq!(
            captured.snapshot.disposition,
            crate::domain::cosmetic_shop::GuestShopImportDisposition::LocalIntegrityValidated
        );
        assert!(captured.source_matches_current);
        assert!(captured.snapshot.data.landscape_instances.is_empty());
        assert!(captured
            .snapshot
            .data
            .unverified_planet_wallet_claims
            .is_empty());
        assert_eq!(ledger.cosmetic_account_id().unwrap(), "local");
    }

    #[test]
    fn guest_shop_import_requires_a_canonical_signed_target_id() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let before = ledger.connection.total_changes();

        let invalid = ledger.capture_guest_shop_import("account:NOT-A-CANONICAL-UUID");

        assert!(invalid.is_err());
        assert!(ledger
            .pending_guest_shop_import("account:NOT-A-CANONICAL-UUID")
            .is_err());
        assert_eq!(ledger.connection.total_changes(), before);
        assert_eq!(ledger.cosmetic_account_id().unwrap(), "local");
    }

    #[test]
    fn account_switch_restores_only_matching_cosmetics() {
        let directory = tempfile::tempdir().unwrap();
        let mut ledger =
            Ledger::open(&directory.path().join("ledger.sqlite3"), chrono_tz::UTC).unwrap();
        ledger.ensure_planet_account("alice").unwrap();
        let alice_cycle = ledger.planet_cycle_id().unwrap();
        ledger
            .store_confirmed_cosmetic_state(&CosmeticShopState {
                slots: cosmetic_slots(),
                products: legacy_cosmetic_products(),
                current_cycle_id: alice_cycle,
                available_balance: 900000,
                owned_skus: vec!["star_cluster".into()],
                equipped: vec![EquippedCosmetic {
                    slot_id: "sky".into(),
                    sku: "star_cluster".into(),
                    version: 4,
                }],
                slot_versions: BTreeMap::from([("sky".into(), 4)]),
                actions_require_online: true,
                action_unavailable_reason: None,
                guest_import_pending: false,
                guest_import_error: None,
            })
            .unwrap();

        ledger.ensure_planet_account("bob").unwrap();
        let bob_cycle = ledger.planet_cycle_id().unwrap();
        ledger
            .store_confirmed_cosmetic_state(&CosmeticShopState {
                slots: cosmetic_slots(),
                products: legacy_cosmetic_products(),
                current_cycle_id: bob_cycle,
                available_balance: 700000,
                owned_skus: vec!["thin_ring".into()],
                equipped: vec![],
                slot_versions: BTreeMap::new(),
                actions_require_online: true,
                action_unavailable_reason: None,
                guest_import_pending: false,
                guest_import_error: None,
            })
            .unwrap();

        let bob = ledger.cosmetic_shop_state().unwrap();
        assert_eq!(bob.available_balance, 700000);
        assert_eq!(bob.owned_skus, vec!["thin_ring"]);
        assert!(bob.equipped.is_empty());
        ledger.ensure_planet_account("alice").unwrap();
        let alice = ledger.cosmetic_shop_state().unwrap();
        assert_eq!(alice.available_balance, 900000);
        assert_eq!(alice.owned_skus, vec!["star_cluster"]);
        assert_eq!(alice.equipped[0].sku, "star_cluster");
    }

    #[test]
    fn already_owned_legacy_equivalent_does_not_add_v2_to_cached_ownership() {
        let directory = tempfile::tempdir().unwrap();
        let mut ledger =
            Ledger::open(&directory.path().join("ledger.sqlite3"), chrono_tz::UTC).unwrap();
        ledger.ensure_planet_account("alice").unwrap();
        let cycle_id = ledger.planet_cycle_id().unwrap();
        ledger
            .store_confirmed_cosmetic_state(&CosmeticShopState {
                slots: cosmetic_slots(),
                products: legacy_cosmetic_products(),
                current_cycle_id: cycle_id,
                available_balance: 600_000,
                owned_skus: vec!["star_cluster".into()],
                equipped: vec![],
                slot_versions: BTreeMap::new(),
                actions_require_online: true,
                action_unavailable_reason: None,
                guest_import_pending: false,
                guest_import_error: None,
            })
            .unwrap();
        let purchase_id = ledger
            .prepare_cosmetic_purchase_request("star_cluster_v2", 1)
            .unwrap();

        ledger
            .complete_cosmetic_purchase_request(&CosmeticPurchaseResult {
                purchase_id,
                sku: "star_cluster_v2".into(),
                status: CosmeticPurchaseStatus::AlreadyOwned,
                price: 500_000,
                available_balance: 600_000,
            })
            .unwrap();

        let state = ledger.cosmetic_shop_state().unwrap();
        assert_eq!(state.owned_skus, vec!["star_cluster"]);
        assert_eq!(state.available_balance, 600_000);
        let purchase_rows: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM cosmetic_purchase WHERE account_id=?1",
                [ledger.cosmetic_account_id().unwrap()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            purchase_rows, 0,
            "an AlreadyOwned response must not create a local purchase row"
        );
    }

    #[test]
    fn guest_wallet_credits_are_not_uploaded_until_import_is_confirmed() {
        let directory = tempfile::tempdir().unwrap();
        let mut ledger =
            Ledger::open(&directory.path().join("ledger.sqlite3"), chrono_tz::UTC).unwrap();
        ledger
            .connection
            .execute(
                "INSERT INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
             VALUES ('guest-credit',250000,'2026-09-28T00:00:00Z')",
                [],
            )
            .unwrap();
        ledger.ensure_planet_account("alice").unwrap();

        assert!(ledger
            .planet_wallet_credits_for_server_upload()
            .unwrap()
            .is_empty());
        let pending = ledger.pending_guest_cosmetic_import().unwrap().unwrap();
        let mut canonical = ledger.cosmetic_shop_state().unwrap();
        canonical.available_balance = 250000;
        ledger
            .mark_guest_cosmetic_imported(&pending.import_id, &canonical)
            .unwrap();

        let upload_credits = ledger.planet_wallet_credits_for_server_upload().unwrap();
        assert_eq!(upload_credits.len(), 1);
        assert_eq!(upload_credits[0].previous_cycle_id, "guest-credit");
    }

    #[test]
    fn pending_purchase_id_survives_restart_and_is_scoped_to_account_and_sku() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.sqlite3");
        let mut ledger = Ledger::open(&path, chrono_tz::UTC).unwrap();
        ledger.ensure_planet_account("alice").unwrap();
        let alice_id = ledger
            .prepare_cosmetic_purchase_request("star_cluster_v2", 1)
            .unwrap();
        drop(ledger);

        let mut ledger = Ledger::open(&path, chrono_tz::UTC).unwrap();
        assert_eq!(
            ledger
                .prepare_cosmetic_purchase_request("star_cluster_v2", 1)
                .unwrap(),
            alice_id
        );
        let alice_other_sku = ledger
            .prepare_cosmetic_purchase_request("aurora_v2", 1)
            .unwrap();
        assert_ne!(alice_id, alice_other_sku);
        ledger.ensure_planet_account("bob").unwrap();
        let bob_id = ledger
            .prepare_cosmetic_purchase_request("star_cluster_v2", 1)
            .unwrap();
        assert_ne!(alice_id, bob_id);
        ledger.ensure_planet_account("alice").unwrap();
        assert_eq!(
            ledger
                .prepare_cosmetic_purchase_request("star_cluster_v2", 1)
                .unwrap(),
            alice_id
        );
    }
}
