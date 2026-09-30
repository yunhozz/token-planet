use std::collections::BTreeMap;

use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::ledger::{Ledger, ScanError};
use crate::domain::cosmetic_shop::{GuestCosmeticImport, GuestCosmeticPurchase};
use crate::domain::planet::{PlanetObject, PlanetWalletCredit};

#[derive(Deserialize, Serialize)]
struct SavedPlanet {
    settings: BTreeMap<String, String>,
    objects: Vec<PlanetObject>,
    wallet_credits: Vec<PlanetWalletCredit>,
}

impl Ledger {
    pub(crate) fn initialize_planet_accounts(&mut self) -> Result<(), ScanError> {
        let tx = self.connection.transaction()?;
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS planet_account_state (
                account_id TEXT PRIMARY KEY, state_json TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS planet_usage_owner (
                event_key TEXT PRIMARY KEY, account_id TEXT NOT NULL
             );",
        )?;
        tx.execute_batch(
            "CREATE INDEX IF NOT EXISTS planet_usage_owner_account_event
             ON planet_usage_owner(account_id, event_key);",
        )?;
        // Older versions did not record the planet's account. A cached world
        // may belong to an earlier login, so preserve previously shared data
        // separately and restore the signed-in account from its server state.
        tx.execute(
            "INSERT OR IGNORE INTO setting(key,value)
             SELECT 'planet_account_id', CASE
               WHEN EXISTS (SELECT 1 FROM setting WHERE key IN ('sharing_user_id','planet_remote_lifetime_tokens'))
                 THEN 'legacy'
               ELSE 'local' END",
            [],
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO planet_usage_owner(event_key,account_id)
             SELECT event_key,(SELECT value FROM setting WHERE key='planet_account_id')
             FROM usage_record",
            [],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Switch all derived planet data atomically, retaining each event's owner
    /// across account changes, source rewrites, and restarts.
    pub fn ensure_planet_account(&mut self, user_id: &str) -> Result<bool, ScanError> {
        let account_id = format!("account:{user_id}");
        let current: String = self.connection.query_row(
            "SELECT value FROM setting WHERE key='planet_account_id'",
            [],
            |row| row.get(0),
        )?;
        if current == account_id {
            return Ok(false);
        }
        if current == "local" {
            let guest_wallet_credits = self.planet_wallet_credits()?;
            let guest_purchases = {
                let mut statement = self.connection.prepare(
                    "SELECT purchase_id,sku,price,purchased_at_utc FROM cosmetic_purchase
                     WHERE account_id='local' ORDER BY purchased_at_utc,purchase_id",
                )?;
                let rows = statement.query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })?;
                rows.map(|row| {
                    let (purchase_id, sku, price, purchased_at_utc) = row?;
                    Ok(GuestCosmeticPurchase {
                        purchase_id,
                        sku,
                        price: u64::try_from(price).map_err(|_| rusqlite::Error::InvalidQuery)?,
                        purchased_at_utc,
                    })
                })
                .collect::<Result<Vec<_>, rusqlite::Error>>()?
            };
            let guest_import = if guest_wallet_credits.is_empty() && guest_purchases.is_empty() {
                None
            } else {
                Some(GuestCosmeticImport {
                    import_id: uuid::Uuid::new_v4().to_string(),
                    wallet_credits: guest_wallet_credits,
                    purchases: guest_purchases,
                })
            };
            let guest_import_json = guest_import
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .map_err(|_| ScanError::Database)?;
            let tx = self.connection.transaction()?;
            if let (Some(import), Some(payload_json)) = (guest_import, guest_import_json) {
                tx.execute(
                    "INSERT OR IGNORE INTO cosmetic_guest_import(account_id,import_id,payload_json)
                     VALUES (?1,?2,?3)",
                    params![account_id, import.import_id, payload_json],
                )?;
            }
            tx.execute(
                "UPDATE planet_usage_owner SET account_id=?1 WHERE account_id='local'",
                [&account_id],
            )?;
            tx.execute(
                "UPDATE setting SET value=?1 WHERE key='planet_account_id'",
                [&account_id],
            )?;
            for table in [
                "growth_journal_state",
                "growth_journal_cycle",
                "growth_journal_entry",
                "growth_journal_remote_cycle",
                "growth_journal_remote_entry",
            ] {
                tx.execute(
                    &format!("UPDATE {table} SET account_id=?1 WHERE account_id='local'"),
                    [&account_id],
                )?;
            }
            tx.commit()?;
            return Ok(true);
        }
        let settings = {
            let mut query = self.connection.prepare(
                "SELECT key,value FROM setting WHERE key GLOB 'planet_*' AND key <> 'planet_account_id'",
            )?;
            let rows = query.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
            rows.collect::<Result<BTreeMap<String, String>, _>>()?
        };
        let saved = SavedPlanet {
            settings,
            objects: self.planet_objects()?,
            wallet_credits: self.planet_wallet_credits()?,
        };
        let saved_json = serde_json::to_string(&saved).map_err(|_| ScanError::Database)?;
        let next: Option<String> = self
            .connection
            .query_row(
                "SELECT state_json FROM planet_account_state WHERE account_id=?1",
                [&account_id],
                |row| row.get(0),
            )
            .optional()?;
        let next = match next {
            Some(value) => {
                serde_json::from_str::<SavedPlanet>(&value).map_err(|_| ScanError::Database)?
            }
            None => {
                let now = Utc::now().to_rfc3339();
                SavedPlanet {
                    settings: BTreeMap::from([
                        ("planet_activation_at_utc".into(), now.clone()),
                        ("planet_cycle_started_at_utc".into(), now),
                        (
                            "planet_current_cycle_id".into(),
                            uuid::Uuid::new_v4().to_string(),
                        ),
                        ("planet_device_id".into(), uuid::Uuid::new_v4().to_string()),
                        ("planet_timezone".into(), self.timezone.to_string()),
                    ]),
                    objects: Vec::new(),
                    wallet_credits: Vec::new(),
                }
            }
        };
        let tx = self.connection.transaction()?;
        tx.execute(
            "INSERT INTO planet_account_state(account_id,state_json) VALUES (?1,?2)
             ON CONFLICT(account_id) DO UPDATE SET state_json=excluded.state_json",
            params![current, saved_json],
        )?;
        tx.execute(
            "DELETE FROM setting WHERE key GLOB 'planet_*' AND key <> 'planet_account_id'",
            [],
        )?;
        tx.execute("DELETE FROM planet_object", [])?;
        tx.execute("DELETE FROM planet_wallet_credit", [])?;
        for (key, value) in &next.settings {
            tx.execute(
                "INSERT INTO setting(key,value) VALUES (?1,?2)",
                params![key, value],
            )?;
        }
        let cycle = next
            .settings
            .get("planet_current_cycle_id")
            .ok_or(ScanError::Database)?;
        for object in &next.objects {
            tx.execute(
                "INSERT INTO planet_object(cycle_id,stage,ordinal,kind,x,y,seed) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![cycle, object.stage, object.ordinal, object.kind, object.x, object.y, object.seed.to_string()],
            )?;
        }
        for credit in &next.wallet_credits {
            tx.execute(
                "INSERT INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc) VALUES (?1,?2,?3)",
                params![credit.previous_cycle_id, i64::try_from(credit.amount).map_err(|_| ScanError::InvalidCount)?, credit.created_at_utc],
            )?;
        }
        tx.execute(
            "UPDATE setting SET value=?1 WHERE key='planet_account_id'",
            [&account_id],
        )?;
        tx.commit()?;
        Ok(true)
    }
}
