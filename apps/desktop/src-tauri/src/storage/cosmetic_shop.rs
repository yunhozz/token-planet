use std::collections::BTreeSet;

use super::ledger::{Ledger, ScanError};
use crate::domain::cosmetic_shop::{
    cosmetic_products, cosmetic_slots, legacy_equivalent, CosmeticEquipResult,
    CosmeticPurchaseResult, CosmeticPurchaseStatus, CosmeticShopState, EquippedCosmetic,
    GuestCosmeticImport,
};
use crate::domain::planet::PlanetWalletCredit;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Transaction};

fn active_account_id(connection: &Connection) -> Result<String, ScanError> {
    connection
        .query_row("SELECT value FROM setting WHERE key='planet_account_id'", [], |row| {
            row.get(0)
        })
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
             );",
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
        let products = cosmetic_products();
        let mut owned_statement = self.connection.prepare(
            "SELECT sku FROM cosmetic_purchase WHERE account_id=?1 ORDER BY sku",
        )?;
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
                slots
                    .iter()
                    .any(|slot| slot.slot_id == equipped.slot_id)
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
            current_cycle_id,
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

    pub fn pending_guest_cosmetic_import(
        &self,
    ) -> Result<Option<GuestCosmeticImport>, ScanError> {
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

    pub fn store_confirmed_cosmetic_state(
        &mut self,
        state: &CosmeticShopState,
    ) -> Result<(), ScanError> {
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
        let account_id = active_account_id(&self.connection)?;
        if !account_id.starts_with("account:")
            || !cosmetic_products().iter().any(|product| product.sku == sku)
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
        state.slot_versions.insert(result.slot_id.clone(), result.version);
        self.store_confirmed_cosmetic_state(&state)?;
        self.cosmetic_shop_state()
    }

    pub fn mark_guest_cosmetic_imported(
        &mut self,
        import_id: &str,
        state: &CosmeticShopState,
    ) -> Result<(), ScanError> {
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
        if !cosmetic_slots().iter().any(|slot| slot.slot_id == slot_id) {
            return Err(ScanError::InvalidProfile);
        }
        let transaction = self.connection.transaction()?;
        let account_id = active_account_id(&transaction)?;
        let cycle_id = active_cycle_id(&transaction)?;
        if let Some(sku) = sku {
            let product = cosmetic_products()
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
    let Some(product) = cosmetic_products().into_iter().find(|product| product.sku == sku) else {
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
        params![account_id, purchase_id, sku, to_i64(product.price)?, Utc::now().to_rfc3339()],
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
        cosmetic_products, cosmetic_slots, legacy_equivalent, CosmeticPurchaseResult,
        CosmeticPurchaseStatus, CosmeticShopState, EquippedCosmetic,
    };
    use crate::storage::ledger::Ledger;
    use rusqlite::params;

    #[test]
    fn local_catalog_has_four_slots_and_all_twenty_products_at_the_registered_prices() {
        assert_eq!(
            cosmetic_slots().iter().map(|slot| slot.slot_id.as_str()).collect::<Vec<_>>(),
            vec!["sky", "ring", "surface", "forecourt"],
        );
        let products = cosmetic_products();
        assert_eq!(products.len(), 20);
        let sale_prices = products
            .iter()
            .filter(|product| product.purchasable)
            .map(|product| (product.sku.as_str(), product.price))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(sale_prices, BTreeMap::from([
            ("star_cluster_v2", 500_000), ("aurora_v2", 2_000_000),
            ("thin_ring_v2", 500_000), ("double_ring_v2", 2_000_000),
            ("flag_v2", 500_000), ("crystal_tower_v2", 2_000_000),
            ("meteor_shower", 1_000_000), ("moonlets", 3_000_000),
            ("flower_garden", 1_000_000), ("observatory", 5_000_000),
            ("pond", 750_000), ("lantern", 1_500_000),
            ("rover", 3_000_000), ("greenhouse", 5_000_000),
        ]));
        for (sku, price) in [
            ("star_cluster", 100_000), ("aurora", 500_000),
            ("thin_ring", 100_000), ("double_ring", 500_000),
            ("flag", 100_000), ("crystal_tower", 500_000),
        ] {
            let product = products.iter().find(|product| product.sku == sku).unwrap();
            assert_eq!(product.price, price, "historical price for {sku}");
            assert!(!product.purchasable, "legacy product {sku} must be retired");
        }
        assert_eq!(legacy_equivalent("star_cluster_v2"), Some("star_cluster"));
        assert_eq!(legacy_equivalent("aurora_v2"), Some("aurora"));
        assert_eq!(legacy_equivalent("thin_ring_v2"), Some("thin_ring"));
        assert_eq!(legacy_equivalent("double_ring_v2"), Some("double_ring"));
        assert_eq!(legacy_equivalent("flag_v2"), Some("flag"));
        assert_eq!(legacy_equivalent("crystal_tower_v2"), Some("crystal_tower"));
        assert_eq!(legacy_equivalent("meteor_shower"), None);
    }

    #[test]
    fn legacy_owned_style_cannot_be_charged_again_at_the_new_price() {
        let directory = tempfile::tempdir().unwrap();
        let mut ledger = Ledger::open(&directory.path().join("ledger.sqlite3"), chrono_tz::UTC).unwrap();
        ledger.connection.execute(
            "INSERT INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
             VALUES ('seed-cycle',600000,'2026-09-28T00:00:00Z')",
            [],
        ).unwrap();
        let account_id = super::active_account_id(&ledger.connection).unwrap();
        ledger.connection.execute(
            "INSERT INTO cosmetic_purchase(account_id,purchase_id,sku,price,purchased_at_utc)
             VALUES (?1,'legacy-purchase','star_cluster',100000,'2026-09-28T00:00:00Z')",
            params![account_id],
        ).unwrap();

        let first = ledger.purchase_guest_cosmetic(
            "11111111-1111-4111-8111-111111111111",
            "star_cluster_v2",
        ).unwrap();
        assert_eq!(first.status, CosmeticPurchaseStatus::AlreadyOwned);
        assert_eq!(first.price, 500_000);
        assert_eq!(first.available_balance, 500_000);
        let replay = ledger.purchase_guest_cosmetic(
            "11111111-1111-4111-8111-111111111111",
            "star_cluster_v2",
        ).unwrap();
        assert_eq!(replay, first);
        let other_device = ledger.purchase_guest_cosmetic(
            "22222222-2222-4222-8222-222222222222",
            "star_cluster_v2",
        ).unwrap();
        assert_eq!(other_device.status, CosmeticPurchaseStatus::AlreadyOwned);
        assert_eq!(other_device.available_balance, 500_000);
        let rows: i64 = ledger.connection.query_row(
            "SELECT count(*) FROM cosmetic_purchase WHERE account_id=?1",
            params![account_id],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(rows, 1, "the old purchase amount remains the only ledger charge");
    }

    #[test]
    fn ledger_opens_cosmetic_purchase_and_equipment_tables() {
        let directory = tempfile::tempdir().unwrap();
        let ledger = Ledger::open(&directory.path().join("ledger.sqlite3"), chrono_tz::UTC).unwrap();
        for table in ["cosmetic_purchase", "cosmetic_purchase_request", "cosmetic_equipment"] {
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
    fn guest_purchase_spends_new_catalog_price_once_and_persists() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.sqlite3");
        let mut ledger = Ledger::open(&path, chrono_tz::UTC).unwrap();
        ledger.connection.execute(
            "INSERT INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
             VALUES ('seed-cycle',500000,'2026-09-28T00:00:00Z')",
            [],
        ).unwrap();

        let first = ledger.purchase_guest_cosmetic(
            "11111111-1111-4111-8111-111111111111",
            "star_cluster_v2",
        ).unwrap();
        assert_eq!(first.status, CosmeticPurchaseStatus::Purchased);
        assert_eq!(first.price, 500_000);
        assert_eq!(first.available_balance, 0);

        let replay = ledger.purchase_guest_cosmetic(
            "11111111-1111-4111-8111-111111111111",
            "star_cluster_v2",
        ).unwrap();
        assert_eq!(replay, first);
        let duplicate = ledger.purchase_guest_cosmetic(
            "22222222-2222-4222-8222-222222222222",
            "star_cluster_v2",
        ).unwrap();
        assert_eq!(duplicate.status, CosmeticPurchaseStatus::AlreadyOwned);
        ledger.connection.execute(
            "INSERT INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
             VALUES ('later-cycle',100,'2026-09-29T00:00:00Z')",
            [],
        ).unwrap();
        let duplicate_replay = ledger.purchase_guest_cosmetic(
            "22222222-2222-4222-8222-222222222222",
            "star_cluster_v2",
        ).unwrap();
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
        ledger.connection.execute(
            "INSERT INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
             VALUES ('seed-cycle',499999,'2026-09-28T00:00:00Z')",
            [],
        ).unwrap();

        let first = ledger.purchase_guest_cosmetic(
            "33333333-3333-4333-8333-333333333333",
            "aurora_v2",
        );
        let purchases: i64 = ledger.connection.query_row(
            "SELECT count(*) FROM cosmetic_purchase",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(purchases, 0);
        let first = first.unwrap();
        assert_eq!(first.status, CosmeticPurchaseStatus::InsufficientBalance);
        assert_eq!(first.price, 2_000_000);
        assert_eq!(first.available_balance, 499999);

        ledger.connection.execute(
            "INSERT INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
             VALUES ('later-cycle',1,'2026-09-29T00:00:00Z')",
            [],
        ).unwrap();
        let replay = ledger.purchase_guest_cosmetic(
            "33333333-3333-4333-8333-333333333333",
            "aurora_v2",
        ).unwrap();
        assert_eq!(replay.status, CosmeticPurchaseStatus::InsufficientBalance);
        assert_eq!(replay.available_balance, 499999);
        let purchases: i64 = ledger.connection.query_row(
            "SELECT count(*) FROM cosmetic_purchase",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(purchases, 0);
    }

    #[test]
    fn equipment_is_cycle_scoped_and_account_scoped() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.sqlite3");
        let mut ledger = Ledger::open(&path, chrono_tz::UTC).unwrap();
        ledger.connection.execute(
            "INSERT INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
             VALUES ('seed-cycle',1250000,'2026-09-28T00:00:00Z')",
            [],
        ).unwrap();
        ledger.ensure_planet_account("alice").unwrap();
        ledger.purchase_guest_cosmetic(
            "44444444-4444-4444-8444-444444444444",
            "star_cluster_v2",
        ).unwrap();
        ledger.purchase_guest_cosmetic(
            "55555555-5555-4555-8555-555555555555",
            "pond",
        ).unwrap();
        ledger.equip_guest_cosmetic("sky", Some("star_cluster_v2")).unwrap();
        ledger.equip_guest_cosmetic("forecourt", Some("pond")).unwrap();
        let equipped_cycle = ledger.planet_cycle_id().unwrap();

        ledger.ensure_planet_account("bob").unwrap();
        assert!(ledger.cosmetic_shop_state().unwrap().owned_skus.is_empty());
        assert!(ledger.cosmetic_shop_state().unwrap().equipped.is_empty());

        ledger.ensure_planet_account("alice").unwrap();
        let state = ledger.cosmetic_shop_state().unwrap();
        assert_eq!(state.owned_skus, vec!["pond", "star_cluster_v2"]);
        assert_eq!(state.equipped.len(), 2);
        assert!(state.equipped.iter().any(|item| item.slot_id == "sky" && item.sku == "star_cluster_v2"));
        assert!(state.equipped.iter().any(|item| item.slot_id == "forecourt" && item.sku == "pond"));

        ledger.reset_planet(chrono::DateTime::parse_from_rfc3339("2026-09-28T01:00:00Z").unwrap().to_utc()).unwrap();
        let state = ledger.cosmetic_shop_state().unwrap();
        assert_eq!(state.current_cycle_id, ledger.planet_cycle_id().unwrap());
        assert_ne!(state.current_cycle_id, equipped_cycle);
        assert_eq!(state.owned_skus, vec!["pond", "star_cluster_v2"]);
        assert!(state.equipped.is_empty());
    }

    #[test]
    fn unknown_equipment_keys_survive_catalog_projection() {
        let directory = tempfile::tempdir().unwrap();
        let ledger = Ledger::open(&directory.path().join("ledger.sqlite3"), chrono_tz::UTC).unwrap();
        let account_id = super::active_account_id(&ledger.connection).unwrap();
        let cycle_id = super::active_cycle_id(&ledger.connection).unwrap();
        ledger.connection.execute(
            "INSERT INTO cosmetic_equipment(account_id,cycle_id,slot_id,sku,version)
             VALUES (?1,?2,'future_slot','future_item',7)",
            rusqlite::params![account_id, cycle_id],
        ).unwrap();

        let state = ledger.cosmetic_shop_state().unwrap();
        assert!(state.equipped.is_empty());
        let stored: (String, String) = ledger.connection.query_row(
            "SELECT slot_id,sku FROM cosmetic_equipment WHERE account_id=?1 AND cycle_id=?2",
            rusqlite::params![account_id, cycle_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap();
        assert_eq!(stored, ("future_slot".into(), "future_item".into()));
    }

    #[test]
    fn guest_import_failure_retains_guest_state_and_account_ownership() {
        let directory = tempfile::tempdir().unwrap();
        let mut ledger = Ledger::open(&directory.path().join("ledger.sqlite3"), chrono_tz::UTC).unwrap();
        ledger.connection.execute(
            "INSERT INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
             VALUES ('guest-credit',500000,'2026-09-28T00:00:00Z')",
            [],
        ).unwrap();
        ledger.purchase_guest_cosmetic(
            "66666666-6666-4666-8666-666666666666",
            "star_cluster_v2",
        ).unwrap();

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
    fn account_switch_restores_only_matching_cosmetics() {
        let directory = tempfile::tempdir().unwrap();
        let mut ledger = Ledger::open(&directory.path().join("ledger.sqlite3"), chrono_tz::UTC).unwrap();
        ledger.ensure_planet_account("alice").unwrap();
        let alice_cycle = ledger.planet_cycle_id().unwrap();
        ledger.store_confirmed_cosmetic_state(&CosmeticShopState {
            slots: cosmetic_slots(),
            products: cosmetic_products(),
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
        }).unwrap();

        ledger.ensure_planet_account("bob").unwrap();
        let bob_cycle = ledger.planet_cycle_id().unwrap();
        ledger.store_confirmed_cosmetic_state(&CosmeticShopState {
            slots: cosmetic_slots(),
            products: cosmetic_products(),
            current_cycle_id: bob_cycle,
            available_balance: 700000,
            owned_skus: vec!["thin_ring".into()],
            equipped: vec![],
            slot_versions: BTreeMap::new(),
            actions_require_online: true,
            action_unavailable_reason: None,
            guest_import_pending: false,
            guest_import_error: None,
        }).unwrap();

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
                products: cosmetic_products(),
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
        let mut ledger = Ledger::open(&directory.path().join("ledger.sqlite3"), chrono_tz::UTC).unwrap();
        ledger.connection.execute(
            "INSERT INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
             VALUES ('guest-credit',250000,'2026-09-28T00:00:00Z')",
            [],
        ).unwrap();
        ledger.ensure_planet_account("alice").unwrap();

        assert!(ledger.planet_wallet_credits_for_server_upload().unwrap().is_empty());
        let pending = ledger.pending_guest_cosmetic_import().unwrap().unwrap();
        let mut canonical = ledger.cosmetic_shop_state().unwrap();
        canonical.available_balance = 250000;
        ledger.mark_guest_cosmetic_imported(&pending.import_id, &canonical).unwrap();

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
        let alice_id = ledger.prepare_cosmetic_purchase_request("star_cluster_v2", 1).unwrap();
        drop(ledger);

        let mut ledger = Ledger::open(&path, chrono_tz::UTC).unwrap();
        assert_eq!(ledger.prepare_cosmetic_purchase_request("star_cluster_v2", 1).unwrap(), alice_id);
        let alice_other_sku = ledger.prepare_cosmetic_purchase_request("aurora_v2", 1).unwrap();
        assert_ne!(alice_id, alice_other_sku);
        ledger.ensure_planet_account("bob").unwrap();
        let bob_id = ledger.prepare_cosmetic_purchase_request("star_cluster_v2", 1).unwrap();
        assert_ne!(alice_id, bob_id);
        ledger.ensure_planet_account("alice").unwrap();
        assert_eq!(ledger.prepare_cosmetic_purchase_request("star_cluster_v2", 1).unwrap(), alice_id);
    }
}
