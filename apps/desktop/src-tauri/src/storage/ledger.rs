use std::{collections::BTreeMap, fmt, path::Path};

use chrono::{DateTime, Duration, Utc};
use chrono_tz::Tz;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use crate::collectors::{ParsedRecord, RecordKind};
use crate::domain::cosmetic_shop::{
    ResetShopResult, ShopActionStatus, ShopEffectTimeline, ShopRequest,
};
use crate::domain::planet::{
    PlanetAvatar, PlanetDeviceContribution, PlanetObject, PlanetProfile, PlanetState,
    PlanetWalletCredit,
};
use crate::domain::usage::{Agent, TokenUsage, UsageCoverage};

use super::cosmetic_shop::store_confirmed_shop_state_in_transaction;
use super::shop_effects::{
    apply_confirmed_shop_effect_timeline_in_transaction,
    validate_reset_timeline_current_bound,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScanError {
    Database,
    SourceIo,
    SourcePermission,
    InvalidCount,
    TimezoneMismatch,
    ResetCooldown,
    InvalidProfile,
    InvalidShopState,
}

impl fmt::Display for ScanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Database => "local database error",
                Self::SourceIo => "source file unavailable",
                Self::SourcePermission => "source file permission denied",
                Self::InvalidCount => "token count exceeds local storage range",
                Self::TimezoneMismatch => "world timezone differs from saved ledger",
                Self::ResetCooldown => "planet reset is available 24 hours after the last reset",
                Self::InvalidProfile => "planet profile is invalid",
                Self::InvalidShopState => "shop state is invalid",
            }
        )
    }
}

impl std::error::Error for ScanError {}

impl From<rusqlite::Error> for ScanError {
    fn from(_: rusqlite::Error) -> Self {
        Self::Database
    }
}

pub struct Ledger {
    pub(crate) connection: Connection,
    pub timezone: Tz,
    pub(super) growth_journal_signature: Option<(u64, u64)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignedResetIntent {
    pub account_id: String,
    pub request_id: uuid::Uuid,
    pub expected_old_cycle_id: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SharedDailyTotal {
    pub agent: Agent,
    pub bucket_date: String,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub coverage: UsageCoverage,
}

fn add_optional(total: &mut Option<u64>, value: Option<i64>) -> Result<(), ScanError> {
    if let Some(value) = value {
        *total = Some(
            total
                .unwrap_or(0)
                .checked_add(as_u64(value)?)
                .ok_or(ScanError::InvalidCount)?,
        );
    }
    Ok(())
}

impl Ledger {
    pub fn shared_daily_totals(&self, timezone: Tz) -> Result<Vec<SharedDailyTotal>, ScanError> {
        self.shared_daily_totals_after(timezone, None)
    }

    pub fn reform_daily_totals(&self, timezone: Tz) -> Result<Vec<SharedDailyTotal>, ScanError> {
        self.shared_daily_totals_after(timezone, Some(self.planet_activation_at()?))
    }

    fn shared_daily_totals_after(
        &self,
        timezone: Tz,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<SharedDailyTotal>, ScanError> {
        let mut statement = self.connection.prepare("SELECT agent,occurred_at_utc,input_tokens,output_tokens,
            cache_read_tokens,cache_write_tokens,total_tokens,coverage FROM usage_record r
            WHERE (?1 IS NULL OR (r.occurred_at_utc > ?1 AND EXISTS (
                SELECT 1 FROM planet_usage_owner o WHERE o.event_key=r.event_key
                AND o.account_id=(SELECT value FROM setting WHERE key='planet_account_id')
            ))) AND (r.kind='response' OR NOT EXISTS (
                SELECT 1 FROM usage_record other WHERE other.source_id=r.source_id AND other.kind='response' AND other.agent=r.agent
                AND (?1 IS NULL OR EXISTS (SELECT 1 FROM planet_usage_owner o WHERE o.event_key=other.event_key
                  AND o.account_id=(SELECT value FROM setting WHERE key='planet_account_id')))
            ))")?;
        let rows = statement.query_map(params![since.map(|value| value.to_rfc3339())], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<i64>>(5)?,
                row.get::<_, Option<i64>>(6)?,
                row.get::<_, String>(7)?,
            ))
        })?;
        let mut groups: BTreeMap<(String, String), (SharedDailyTotal, bool, bool)> =
            BTreeMap::new();
        for row in rows {
            let (agent, occurred_at, input, output, cache_read, cache_write, total, coverage) =
                row?;
            let timestamp = chrono::DateTime::parse_from_rfc3339(&occurred_at)
                .map_err(|_| ScanError::Database)?;
            let date = timestamp
                .with_timezone(&timezone)
                .format("%Y-%m-%d")
                .to_string();
            let entry = groups
                .entry((agent.clone(), date.clone()))
                .or_insert_with(|| {
                    (
                        SharedDailyTotal {
                            agent: if agent == "codex" {
                                Agent::Codex
                            } else {
                                Agent::ClaudeCode
                            },
                            bucket_date: date,
                            input_tokens: None,
                            output_tokens: None,
                            cache_read_tokens: None,
                            cache_write_tokens: None,
                            total_tokens: None,
                            coverage: UsageCoverage::Unavailable,
                        },
                        false,
                        false,
                    )
                });
            add_optional(&mut entry.0.input_tokens, input)?;
            add_optional(&mut entry.0.output_tokens, output)?;
            add_optional(&mut entry.0.cache_read_tokens, cache_read)?;
            add_optional(&mut entry.0.cache_write_tokens, cache_write)?;
            add_optional(&mut entry.0.total_tokens, total)?;
            entry.1 |= coverage != "complete";
            entry.2 |= coverage == "unsupported";
        }
        Ok(groups
            .into_values()
            .map(|(mut total, incomplete, unsupported)| {
                total.coverage = match (total.total_tokens, incomplete, unsupported) {
                    (Some(_), true, _) => UsageCoverage::Partial,
                    (Some(_), false, _) => UsageCoverage::Complete,
                    (None, _, true) => UsageCoverage::Unsupported,
                    (None, _, _) => UsageCoverage::Unavailable,
                };
                total
            })
            .collect())
    }
    pub fn saved_timezone(path: &Path) -> Result<Option<Tz>, ScanError> {
        if !path.exists() {
            return Ok(None);
        }
        let connection = Connection::open(path)?;
        let saved: Option<String> = connection
            .query_row(
                "SELECT value FROM setting WHERE key='world_timezone'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        saved
            .map(|value| value.parse().map_err(|_| ScanError::TimezoneMismatch))
            .transpose()
    }

    pub fn open(path: &Path, timezone: Tz) -> Result<Self, ScanError> {
        let connection = Connection::open(path)?;
        connection.execute_batch(
            "\
            PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS setting (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS source_checkpoint (
                source_id TEXT PRIMARY KEY, file_fingerprint TEXT NOT NULL,
                byte_offset INTEGER NOT NULL, parser_version INTEGER NOT NULL,
                last_snapshot_total INTEGER, status TEXT NOT NULL DEFAULT 'complete'
            );
            CREATE TABLE IF NOT EXISTS usage_record (
                event_key TEXT PRIMARY KEY, source_id TEXT NOT NULL,
                agent TEXT NOT NULL, kind TEXT NOT NULL,
                bucket_date TEXT NOT NULL, occurred_at_utc TEXT NOT NULL,
                input_tokens INTEGER, output_tokens INTEGER,
                cache_read_tokens INTEGER, cache_write_tokens INTEGER,
                total_tokens INTEGER, coverage TEXT NOT NULL, parser_version INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS usage_record_source_agent_kind
                ON usage_record(source_id, agent, kind);
            CREATE TABLE IF NOT EXISTS daily_agent_total (
                agent TEXT NOT NULL, bucket_date TEXT NOT NULL,
                total_tokens INTEGER, coverage TEXT NOT NULL,
                PRIMARY KEY (agent, bucket_date)
            );
            CREATE TABLE IF NOT EXISTS outbox_snapshot (
                device_id TEXT NOT NULL, bucket_date TEXT NOT NULL, agent TEXT NOT NULL,
                revision INTEGER NOT NULL, acknowledged_revision INTEGER NOT NULL DEFAULT 0,
                payload_hash TEXT NOT NULL, payload_json TEXT NOT NULL,
                PRIMARY KEY (device_id, bucket_date, agent)
            );
            CREATE TABLE IF NOT EXISTS planet_object (
                cycle_id TEXT NOT NULL, stage INTEGER NOT NULL, ordinal INTEGER NOT NULL,
                kind TEXT NOT NULL, x INTEGER NOT NULL, y INTEGER NOT NULL, seed TEXT NOT NULL,
                PRIMARY KEY (cycle_id, stage, ordinal)
            );
            CREATE TABLE IF NOT EXISTS planet_wallet_credit (
                previous_cycle_id TEXT PRIMARY KEY, amount INTEGER NOT NULL,
                created_at_utc TEXT NOT NULL
            );",
        )?;
        let saved: Option<String> = connection
            .query_row(
                "SELECT value FROM setting WHERE key='world_timezone'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(saved) = saved {
            if saved != timezone.to_string() {
                return Err(ScanError::TimezoneMismatch);
            }
        } else {
            connection.execute(
                "INSERT INTO setting(key,value) VALUES ('world_timezone',?1)",
                [timezone.to_string()],
            )?;
        }
        if setting_value(&connection, "planet_activation_at_utc")?.is_none() {
            set_setting_value(
                &connection,
                "planet_activation_at_utc",
                &Utc::now().to_rfc3339(),
            )?;
        }
        if setting_value(&connection, "planet_current_cycle_id")?.is_none() {
            set_setting_value(
                &connection,
                "planet_current_cycle_id",
                &uuid::Uuid::new_v4().to_string(),
            )?;
        }
        if setting_value(&connection, "planet_cycle_started_at_utc")?.is_none() {
            let activation = setting_value(&connection, "planet_activation_at_utc")?
                .ok_or(ScanError::Database)?;
            set_setting_value(&connection, "planet_cycle_started_at_utc", &activation)?;
        }
        if setting_value(&connection, "planet_timezone")?.is_none() {
            set_setting_value(&connection, "planet_timezone", &timezone.to_string())?;
        }
        if setting_value(&connection, "planet_device_id")?.is_none() {
            set_setting_value(
                &connection,
                "planet_device_id",
                &uuid::Uuid::new_v4().to_string(),
            )?;
        }
        let mut ledger = Self {
            connection,
            timezone,
            growth_journal_signature: None,
        };
        ledger.initialize_planet_accounts()?;
        ledger.initialize_growth_journal()?;
        ledger.initialize_cosmetic_shop()?;
        ledger.initialize_shop_effect_storage()?;
        Ok(ledger)
    }

    pub fn insert(&mut self, record: &ParsedRecord) -> Result<(), ScanError> {
        let timezone = self.timezone;
        let tx = self.connection.transaction()?;
        insert_record(&tx, record, "", timezone)?;
        rebuild_daily(&tx)?;
        tx.commit()?;
        Ok(())
    }

    pub fn daily_total(&self, agent: Agent, bucket_date: &str) -> Result<Option<u64>, ScanError> {
        let value: Option<Option<i64>> = self
            .connection
            .query_row(
                "SELECT total_tokens FROM daily_agent_total WHERE agent=?1 AND bucket_date=?2",
                params![agent_name(agent), bucket_date],
                |row| row.get(0),
            )
            .optional()?;
        value.flatten().map(as_u64).transpose()
    }

    pub fn custom_root(&self, agent: Agent) -> Result<Option<std::path::PathBuf>, ScanError> {
        let key = format!("{}_custom_root", agent_name(agent));
        let saved: Option<String> = self
            .connection
            .query_row("SELECT value FROM setting WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(saved.map(std::path::PathBuf::from))
    }

    pub fn set_custom_root(&mut self, agent: Agent, folder: &Path) -> Result<(), ScanError> {
        let key = format!("{}_custom_root", agent_name(agent));
        self.connection.execute(
            "INSERT INTO setting(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, folder.to_string_lossy().as_ref()],
        )?;
        Ok(())
    }

    pub fn ensure_source_root(&mut self, agent: Agent, root_key: &str) -> Result<(), ScanError> {
        let key = format!("{}_active_root", agent_name(agent));
        let saved: Option<String> = self
            .connection
            .query_row("SELECT value FROM setting WHERE key=?1", [&key], |row| {
                row.get(0)
            })
            .optional()?;
        if saved.as_deref() == Some(root_key) {
            return Ok(());
        }
        let tx = self.connection.transaction()?;
        if saved.is_some() {
            tx.execute(
                "DELETE FROM usage_record WHERE agent=?1",
                [agent_name(agent)],
            )?;
            tx.execute(
                "DELETE FROM source_checkpoint WHERE source_id LIKE ?1",
                [format!("{}:%", agent_name(agent))],
            )?;
            rebuild_daily(&tx)?;
        }
        tx.execute(
            "INSERT INTO setting(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, root_key],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn agent_enabled(&self, agent: Agent) -> Result<bool, ScanError> {
        let key = format!("{}_enabled", agent_name(agent));
        let saved: Option<String> = self
            .connection
            .query_row("SELECT value FROM setting WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(saved.as_deref() != Some("false"))
    }

    pub fn set_agent_enabled(&mut self, agent: Agent, enabled: bool) -> Result<(), ScanError> {
        let key = format!("{}_enabled", agent_name(agent));
        self.connection.execute(
            "INSERT INTO setting(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, if enabled { "true" } else { "false" }],
        )?;
        Ok(())
    }

    pub fn daily_known_totals(&self) -> Result<Vec<u64>, ScanError> {
        let mut statement = self.connection.prepare(
            "SELECT SUM(total_tokens) FROM daily_agent_total WHERE total_tokens IS NOT NULL
             AND ((agent='codex' AND ?1) OR (agent='claude_code' AND ?2))
             GROUP BY bucket_date ORDER BY bucket_date",
        )?;
        let rows = statement.query_map(
            params![
                self.agent_enabled(Agent::Codex)?,
                self.agent_enabled(Agent::ClaudeCode)?
            ],
            |row| row.get::<_, i64>(0),
        )?;
        rows.map(|row| as_u64(row?)).collect()
    }

    pub fn planet_usage_totals(&self) -> Result<(BTreeMap<String, u64>, u64, u64), ScanError> {
        let activation = self.planet_activation_at()?;
        let planet_timezone = self.planet_timezone()?;
        let local_cycle_start = self.last_reset_at()?.unwrap_or(activation).max(activation);
        let confirmed_cycle_start = self.confirmed_current_cycle_start()?;
        let server_cycle_bound_is_known = confirmed_cycle_start.is_some();
        let cycle_start = confirmed_cycle_start.unwrap_or(local_cycle_start);
        let mut statement = self.connection.prepare(
            "SELECT r.occurred_at_utc,r.total_tokens FROM usage_record r
             WHERE r.total_tokens IS NOT NULL AND r.occurred_at_utc > ?1
               AND ((r.agent='codex' AND ?2) OR (r.agent='claude_code' AND ?3))
               AND EXISTS (SELECT 1 FROM planet_usage_owner o WHERE o.event_key=r.event_key
                 AND o.account_id=(SELECT value FROM setting WHERE key='planet_account_id'))
               AND (r.kind='response' OR NOT EXISTS (
                 SELECT 1 FROM usage_record other
                 WHERE other.source_id=r.source_id AND other.kind='response' AND other.agent=r.agent
                   AND EXISTS (SELECT 1 FROM planet_usage_owner o WHERE o.event_key=other.event_key
                     AND o.account_id=(SELECT value FROM setting WHERE key='planet_account_id'))
               ))
             ORDER BY r.occurred_at_utc",
        )?;
        let rows = statement.query_map(
            params![
                activation.to_rfc3339(),
                self.agent_enabled(Agent::Codex)?,
                self.agent_enabled(Agent::ClaudeCode)?,
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )?;
        let mut daily = BTreeMap::new();
        let mut lifetime = 0_u64;
        let mut current = 0_u64;
        for row in rows {
            let (occurred_at, tokens) = row?;
            let tokens = as_u64(tokens)?;
            lifetime = lifetime
                .checked_add(tokens)
                .ok_or(ScanError::InvalidCount)?;
            let timestamp = DateTime::parse_from_rfc3339(&occurred_at)
                .map_err(|_| ScanError::Database)?
                .with_timezone(&Utc);
            let belongs_to_current_cycle = if server_cycle_bound_is_known {
                timestamp >= cycle_start
            } else {
                timestamp > cycle_start
            };
            if belongs_to_current_cycle {
                current = current.checked_add(tokens).ok_or(ScanError::InvalidCount)?;
                let date = timestamp
                    .with_timezone(&planet_timezone)
                    .format("%Y-%m-%d")
                    .to_string();
                let total = daily.entry(date).or_insert(0_u64);
                *total = total.checked_add(tokens).ok_or(ScanError::InvalidCount)?;
            }
        }
        Ok((daily, current, lifetime))
    }

    pub fn planet_timezone(&self) -> Result<Tz, ScanError> {
        setting_value(&self.connection, "planet_timezone")?
            .ok_or(ScanError::Database)?
            .parse()
            .map_err(|_| ScanError::TimezoneMismatch)
    }

    pub fn set_planet_timezone(&self, timezone: &str) -> Result<(), ScanError> {
        let _: Tz = timezone.parse().map_err(|_| ScanError::TimezoneMismatch)?;
        set_setting_value(&self.connection, "planet_timezone", timezone)
    }

    pub fn planet_device_contribution(
        &self,
        incomplete: bool,
    ) -> Result<PlanetDeviceContribution, ScanError> {
        let (daily_tokens, current_planet_tokens, lifetime_tokens) = self.planet_usage_totals()?;
        Ok(PlanetDeviceContribution {
            device_id: setting_value(&self.connection, "planet_device_id")?
                .ok_or(ScanError::Database)?,
            current_cycle_id: self.planet_cycle_id()?,
            lifetime_tokens,
            current_planet_tokens,
            daily_tokens,
            incomplete,
        })
    }

    pub fn planet_profile(&self) -> Result<Option<PlanetProfile>, ScanError> {
        let nickname = setting_value(&self.connection, "planet_nickname")?;
        let avatar = setting_value(&self.connection, "planet_avatar")?;
        match (nickname, avatar) {
            (Some(nickname), Some(avatar)) => {
                let avatar = match avatar.as_str() {
                    "masculine" => PlanetAvatar::Masculine,
                    "feminine" => PlanetAvatar::Feminine,
                    _ => return Err(ScanError::InvalidProfile),
                };
                Ok(Some(PlanetProfile { nickname, avatar }))
            }
            (None, None) => Ok(None),
            _ => Err(ScanError::InvalidProfile),
        }
    }

    pub fn set_planet_profile(
        &mut self,
        nickname: &str,
        avatar: PlanetAvatar,
    ) -> Result<(), ScanError> {
        let nickname = nickname.trim();
        if nickname.is_empty() || nickname.chars().count() > 24 {
            return Err(ScanError::InvalidProfile);
        }
        let avatar = match avatar {
            PlanetAvatar::Masculine => "masculine",
            PlanetAvatar::Feminine => "feminine",
        };
        let tx = self.connection.transaction()?;
        tx.execute(
            "INSERT INTO setting(key,value) VALUES ('planet_nickname',?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [nickname],
        )?;
        tx.execute(
            "INSERT INTO setting(key,value) VALUES ('planet_avatar',?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [avatar],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn planet_activation_at(&self) -> Result<DateTime<Utc>, ScanError> {
        parse_utc_setting(&self.connection, "planet_activation_at_utc")
    }

    pub fn planet_cycle_id(&self) -> Result<String, ScanError> {
        setting_value(&self.connection, "planet_current_cycle_id")?.ok_or(ScanError::Database)
    }

    pub fn planet_cycle_started_at(&self) -> Result<String, ScanError> {
        setting_value(&self.connection, "planet_cycle_started_at_utc")?.ok_or(ScanError::Database)
    }

    pub fn last_reset_at(&self) -> Result<Option<DateTime<Utc>>, ScanError> {
        match setting_value(&self.connection, "planet_last_reset_at_utc")? {
            Some(value) => DateTime::parse_from_rfc3339(&value)
                .map(|date| Some(date.with_timezone(&Utc)))
                .map_err(|_| ScanError::Database),
            None => Ok(None),
        }
    }

    pub fn reset_available_at(&self) -> Result<Option<DateTime<Utc>>, ScanError> {
        if let Some(value) = setting_value(&self.connection, "planet_reset_available_at_utc")? {
            return DateTime::parse_from_rfc3339(&value)
                .map(|date| Some(date.with_timezone(&Utc)))
                .map_err(|_| ScanError::Database);
        }
        Ok(self.last_reset_at()?.map(|last| last + Duration::hours(24)))
    }

    pub fn planet_wallet_credits(&self) -> Result<Vec<PlanetWalletCredit>, ScanError> {
        let mut statement = self.connection.prepare(
            "SELECT previous_cycle_id,amount,created_at_utc FROM planet_wallet_credit
             ORDER BY created_at_utc",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        rows.map(|row| {
            let (previous_cycle_id, amount, created_at_utc) = row?;
            Ok(PlanetWalletCredit {
                previous_cycle_id,
                amount: as_u64(amount)?,
                created_at_utc,
            })
        })
        .collect()
    }

    pub fn reset_planet(&mut self, now: DateTime<Utc>) -> Result<u64, ScanError> {
        if self.reset_available_at()?.is_some_and(|available| now < available) {
            return Err(ScanError::ResetCooldown);
        }
        let previous_cycle_id = self.planet_cycle_id()?;
        let (_, local_current_tokens, _) = self.planet_usage_totals()?;
        let current_tokens = self
            .synced_planet_metrics()?
            .filter(|(cycle_id, _, _, _)| cycle_id == &previous_cycle_id)
            .map(|(_, remote_tokens, _, _)| local_current_tokens.max(remote_tokens))
            .unwrap_or(local_current_tokens);
        let reset = self.reset_guest_planet(&format!("legacy-reset:{previous_cycle_id}"),&previous_cycle_id,now)?;
        match reset.status {
            crate::domain::cosmetic_shop::ShopActionStatus::Reset => Ok(current_tokens),
            crate::domain::cosmetic_shop::ShopActionStatus::CycleMismatch => Err(ScanError::ResetCooldown),
            _ => Err(ScanError::InvalidShopState),
        }
    }

    pub fn synced_planet_metrics(&self) -> Result<Option<(String, u64, f64, bool)>, ScanError> {
        let Some(cycle_id) = setting_value(&self.connection, "planet_remote_cycle_id")? else {
            return Ok(None);
        };
        let current_tokens = setting_value(&self.connection, "planet_remote_current_tokens")?
            .ok_or(ScanError::Database)?
            .parse()
            .map_err(|_| ScanError::Database)?;
        let growth_credit = setting_value(&self.connection, "planet_remote_growth_credit")?
            .ok_or(ScanError::Database)?
            .parse()
            .map_err(|_| ScanError::Database)?;
        let incomplete = setting_value(&self.connection, "planet_remote_incomplete")?
            .is_some_and(|value| value == "true");
        Ok(Some((cycle_id, current_tokens, growth_credit, incomplete)))
    }

    pub fn synced_planet_lifetime_tokens(&self) -> Result<Option<u64>, ScanError> {
        setting_value(&self.connection, "planet_remote_lifetime_tokens")?
            .map(|value| value.parse().map_err(|_| ScanError::Database))
            .transpose()
    }

    pub fn planet_objects(&self) -> Result<Vec<PlanetObject>, ScanError> {
        let cycle_id = self.planet_cycle_id()?;
        let mut statement = self.connection.prepare(
            "SELECT stage,ordinal,kind,x,y,seed FROM planet_object
             WHERE cycle_id=?1 ORDER BY stage,ordinal",
        )?;
        let rows = statement.query_map([cycle_id], |row| {
            Ok((
                row.get::<_, u8>(0)?,
                row.get::<_, u32>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, u8>(3)?,
                row.get::<_, u8>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?;
        rows.map(|row| {
            let (stage, ordinal, kind, x, y, seed) = row?;
            Ok(PlanetObject {
                stage,
                ordinal,
                kind,
                x,
                y,
                seed: seed.parse().map_err(|_| ScanError::Database)?,
            })
        })
        .collect()
    }

    pub fn ensure_planet_object(
        &self,
        stage: u8,
        ordinal: u32,
        kind: &str,
        x: u8,
        y: u8,
        seed: u64,
    ) -> Result<(), ScanError> {
        self.connection.execute(
            "INSERT OR IGNORE INTO planet_object(cycle_id,stage,ordinal,kind,x,y,seed)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                self.planet_cycle_id()?,
                stage,
                ordinal,
                kind,
                x,
                y,
                seed.to_string()
            ],
        )?;
        Ok(())
    }

    pub fn merge_remote_planet_state(&mut self, remote: &PlanetState) -> Result<(), ScanError> {
        let first_remote_sync =
            setting_value(&self.connection, "planet_remote_cycle_id")?.is_none();
        let had_local_reset = self.last_reset_at()?.is_some();
        let local_cycle = self.planet_cycle_id()?;
        let remote_reset = remote
            .last_reset_at_utc
            .as_deref()
            .unwrap_or(&remote.cycle_started_at_utc);
        let remote_reset = DateTime::parse_from_rfc3339(remote_reset)
            .map_err(|_| ScanError::Database)?
            .with_timezone(&Utc);
        let local_reset = self.last_reset_at()?.unwrap_or(
            DateTime::parse_from_rfc3339(&self.planet_cycle_started_at()?)
                .map_err(|_| ScanError::Database)?
                .with_timezone(&Utc),
        );
        let remote_blocks_recent_reset = remote.last_reset_at_utc.is_some()
            && local_cycle != remote.current_cycle_id
            && local_reset < remote_reset + Duration::hours(24);
        if (first_remote_sync && !had_local_reset)
            || remote_reset > local_reset
            || remote_blocks_recent_reset
        {
            let tx = self.connection.transaction()?;
            if let Some(last_reset) = &remote.last_reset_at_utc {
                tx.execute(
                    "INSERT INTO setting(key,value) VALUES ('planet_last_reset_at_utc',?1)
                     ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                    [last_reset],
                )?;
            } else {
                tx.execute(
                    "DELETE FROM setting WHERE key='planet_last_reset_at_utc'",
                    [],
                )?;
            }
            tx.execute(
                "INSERT INTO setting(key,value) VALUES ('planet_cycle_started_at_utc',?1)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                [&remote.cycle_started_at_utc],
            )?;
            tx.execute(
                "INSERT INTO setting(key,value) VALUES ('planet_current_cycle_id',?1)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                [&remote.current_cycle_id],
            )?;
            tx.execute("DELETE FROM planet_object", [])?;
            if remote_blocks_recent_reset {
                tx.execute(
                    "DELETE FROM planet_wallet_credit WHERE previous_cycle_id=?1",
                    [&remote.current_cycle_id],
                )?;
            }
            tx.commit()?;
        }
        for credit in &remote.wallet_credits {
            self.connection.execute(
                "INSERT OR IGNORE INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
                 VALUES (?1,?2,?3)",
                params![credit.previous_cycle_id, as_i64(credit.amount)?, credit.created_at_utc],
            )?;
            self.connection.execute(
                "UPDATE planet_wallet_credit SET amount=?2,created_at_utc=?3 WHERE previous_cycle_id=?1",
                params![credit.previous_cycle_id, as_i64(credit.amount)?, credit.created_at_utc],
            )?;
        }
        if self.planet_cycle_id()? == remote.current_cycle_id {
            for (key, value) in [
                ("planet_remote_cycle_id", remote.current_cycle_id.clone()),
                (
                    "planet_remote_current_tokens",
                    remote.current_planet_tokens.to_string(),
                ),
                (
                    "planet_remote_lifetime_tokens",
                    remote.lifetime_tokens.to_string(),
                ),
                (
                    "planet_remote_growth_credit",
                    remote.growth_credit.to_string(),
                ),
                ("planet_remote_incomplete", remote.incomplete.to_string()),
            ] {
                set_setting_value(&self.connection, key, &value)?;
            }
            if let Some(reset_available_at) = &remote.reset_available_at_utc {
                set_setting_value(
                    &self.connection,
                    "planet_reset_available_at_utc",
                    reset_available_at,
                )?;
            } else {
                self.connection.execute(
                    "DELETE FROM setting WHERE key='planet_reset_available_at_utc'",
                    [],
                )?;
            }
            for object in &remote.objects {
                self.ensure_planet_object(
                    object.stage,
                    object.ordinal,
                    &object.kind,
                    object.x,
                    object.y,
                    object.seed,
                )?;
            }
        }
        if first_remote_sync || self.planet_profile()?.is_none() {
            if let Some(profile) = &remote.profile {
                self.set_planet_profile(&profile.nickname, profile.avatar)?;
            }
        }
        self.set_planet_timezone(&remote.timezone)?;
        Ok(())
    }

    /// Atomically applies a server-confirmed signed reset to the cached planet,
    /// shop state, and effect timeline. No reset reward is settled locally.
    pub fn apply_confirmed_reset_result(
        &mut self,
        result: &ResetShopResult,
        timeline: &ShopEffectTimeline,
        expected_account_id: &str,
        expected_old_cycle_id: &str,
    ) -> Result<(), ScanError> {
        self.apply_confirmed_reset_result_inner(
            result,
            timeline,
            expected_account_id,
            expected_old_cycle_id,
            None,
        )
    }

    pub fn prepare_signed_reset_intent(
        &mut self,
        request_id: uuid::Uuid,
        expected_old_cycle_id: &str,
    ) -> Result<SignedResetIntent, ScanError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let active_account: String = transaction.query_row(
            "SELECT value FROM setting WHERE key='planet_account_id'",
            [],
            |row| row.get(0),
        )?;
        let active_cycle: String = transaction.query_row(
            "SELECT value FROM setting WHERE key='planet_current_cycle_id'",
            [],
            |row| row.get(0),
        )?;
        let (account_id, account_scope) = signed_account_scope(&active_account)?;
        if active_account != account_scope
            || active_cycle != expected_old_cycle_id
            || expected_old_cycle_id.trim().is_empty()
        {
            return Err(ScanError::InvalidShopState);
        }

        let intent = SignedResetIntent {
            account_id,
            request_id,
            expected_old_cycle_id: expected_old_cycle_id.to_owned(),
        };
        if let Some(pending) = pending_signed_reset_intent_in(&transaction, &account_scope)? {
            if pending == intent {
                return Ok(intent);
            }
            return Err(ScanError::InvalidShopState);
        }
        let request_id_text = request_id.to_string();
        let existing: Option<i64> = transaction
            .query_row(
                "SELECT 1 FROM shop_action_request WHERE account_id=?1 AND request_id=?2",
                rusqlite::params![account_scope, request_id_text],
                |row| row.get(0),
            )
            .optional()?;
        if existing.is_some() {
            return Err(ScanError::InvalidShopState);
        }
        let payload_json = serde_json::to_string(&ShopRequest::ResetPlanet {
            request_id: request_id_text.clone(),
            cycle_id: expected_old_cycle_id.to_owned(),
        })
        .map_err(|_| ScanError::InvalidShopState)?;
        transaction.execute(
            "INSERT INTO shop_action_request(account_id,request_id,payload_json,result_json,created_at_utc)
             VALUES (?1,?2,?3,NULL,?4)",
            rusqlite::params![
                account_scope,
                request_id_text,
                payload_json,
                Utc::now().to_rfc3339(),
            ],
        )?;
        transaction.commit()?;
        Ok(intent)
    }

    pub fn pending_signed_reset_intent(
        &self,
        account_id: &str,
    ) -> Result<Option<SignedResetIntent>, ScanError> {
        let (_, account_scope) = signed_account_scope(account_id)?;
        pending_signed_reset_intent_in(&self.connection, &account_scope)
    }

    pub fn apply_confirmed_signed_reset_result(
        &mut self,
        result: &ResetShopResult,
        timeline: &ShopEffectTimeline,
        intent: &SignedResetIntent,
    ) -> Result<(), ScanError> {
        self.apply_confirmed_reset_result_inner(
            result,
            timeline,
            &intent.account_id,
            &intent.expected_old_cycle_id,
            Some(intent),
        )
    }

    pub fn record_rejected_signed_reset_result(
        &mut self,
        result: &ResetShopResult,
        intent: &SignedResetIntent,
    ) -> Result<(), ScanError> {
        if result.action.status == ShopActionStatus::Reset
            || result.action.request_id != intent.request_id.to_string()
            || result.action.state.account_id != format!("account:{}", intent.account_id)
            || result.action.state.current_cycle_id != result.planet_state.current_cycle_id
        {
            return Err(ScanError::InvalidShopState);
        }
        let receipt = serde_json::to_string(result).map_err(|_| ScanError::InvalidShopState)?;
        self.finish_signed_reset_intent_without_cache_change(intent, &receipt)
    }

    fn finish_signed_reset_intent_without_cache_change(
        &mut self,
        intent: &SignedResetIntent,
        receipt_json: &str,
    ) -> Result<(), ScanError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let account_id: String = transaction.query_row(
            "SELECT value FROM setting WHERE key='planet_account_id'",
            [],
            |row| row.get(0),
        )?;
        let cycle_id: String = transaction.query_row(
            "SELECT value FROM setting WHERE key='planet_current_cycle_id'",
            [],
            |row| row.get(0),
        )?;
        if account_id != format!("account:{}", intent.account_id)
            || cycle_id != intent.expected_old_cycle_id
            || pending_signed_reset_intent_in(&transaction, &account_id)?.as_ref() != Some(intent)
        {
            return Err(ScanError::InvalidShopState);
        }
        let payload_json = serde_json::to_string(&ShopRequest::ResetPlanet {
            request_id: intent.request_id.to_string(),
            cycle_id: intent.expected_old_cycle_id.clone(),
        })
        .map_err(|_| ScanError::InvalidShopState)?;
        let completed = transaction.execute(
            "UPDATE shop_action_request SET result_json=?4
             WHERE account_id=?1 AND request_id=?2 AND payload_json=?3 AND result_json IS NULL",
            rusqlite::params![
                account_id,
                intent.request_id.to_string(),
                payload_json,
                receipt_json,
            ],
        )?;
        if completed != 1 {
            return Err(ScanError::InvalidShopState);
        }
        transaction.commit()?;
        Ok(())
    }

    fn apply_confirmed_reset_result_inner(
        &mut self,
        result: &ResetShopResult,
        timeline: &ShopEffectTimeline,
        expected_account_id: &str,
        expected_old_cycle_id: &str,
        intent: Option<&SignedResetIntent>,
    ) -> Result<(), ScanError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let account_id: String = transaction.query_row(
            "SELECT value FROM setting WHERE key='planet_account_id'",
            [],
            |row| row.get(0),
        )?;
        let expected_account_id = expected_account_id
            .strip_prefix("account:")
            .unwrap_or(expected_account_id);
        let expected_account_uuid = uuid::Uuid::parse_str(expected_account_id)
            .map_err(|_| ScanError::InvalidShopState)?;
        let actual_account_uuid = account_id
            .strip_prefix("account:")
            .and_then(|id| uuid::Uuid::parse_str(id).ok());
        let actual_cycle_id: String = transaction.query_row(
            "SELECT value FROM setting WHERE key='planet_current_cycle_id'",
            [],
            |row| row.get(0),
        )?;
        let reset_state = &result.planet_state;
        let new_cycle_id = &reset_state.current_cycle_id;
        if !account_id.starts_with("account:")
            || actual_account_uuid != Some(expected_account_uuid)
            || actual_cycle_id != expected_old_cycle_id
            || expected_old_cycle_id.trim().is_empty()
            || new_cycle_id.trim().is_empty()
            || new_cycle_id == expected_old_cycle_id
            || result.action.status != ShopActionStatus::Reset
            || result.action.request_id.trim().is_empty()
            || result.action.state.account_id != account_id
            || result.action.state.current_cycle_id != *new_cycle_id
            || timeline.current_cycle_id != *new_cycle_id
        {
            return Err(ScanError::InvalidShopState);
        }
        if let Some(intent) = intent {
            let expected_scope = format!("account:{}", intent.account_id);
            if account_id != expected_scope
                || result.action.request_id != intent.request_id.to_string()
                || intent.expected_old_cycle_id != expected_old_cycle_id
                || pending_signed_reset_intent_in(&transaction, &account_id)?.as_ref()
                    != Some(intent)
            {
                return Err(ScanError::InvalidShopState);
            }
        }

        let cycle_started_at = DateTime::parse_from_rfc3339(&reset_state.cycle_started_at_utc)
            .map_err(|_| ScanError::InvalidShopState)?
            .with_timezone(&Utc);
        let reset_at = reset_state
            .last_reset_at_utc
            .as_deref()
            .ok_or(ScanError::InvalidShopState)?;
        let reset_at = DateTime::parse_from_rfc3339(reset_at)
            .map_err(|_| ScanError::InvalidShopState)?
            .with_timezone(&Utc);
        if cycle_started_at != reset_at {
            return Err(ScanError::InvalidShopState);
        }
        validate_reset_timeline_current_bound(
            timeline,
            new_cycle_id,
            &reset_state.cycle_started_at_utc,
        )?;

        apply_confirmed_reset_planet_state_in_transaction(
            &transaction,
            reset_state,
            &account_id,
            expected_old_cycle_id,
        )?;
        store_confirmed_shop_state_in_transaction(&transaction, &result.action.state)?;
        apply_confirmed_shop_effect_timeline_in_transaction(
            &transaction,
            timeline,
            &expected_account_uuid.to_string(),
            new_cycle_id,
        )?;
        if let Some(intent) = intent {
            let payload_json = serde_json::to_string(&ShopRequest::ResetPlanet {
                request_id: intent.request_id.to_string(),
                cycle_id: intent.expected_old_cycle_id.clone(),
            })
            .map_err(|_| ScanError::InvalidShopState)?;
            let result_json =
                serde_json::to_string(result).map_err(|_| ScanError::InvalidShopState)?;
            let completed = transaction.execute(
                "UPDATE shop_action_request SET result_json=?4
                 WHERE account_id=?1 AND request_id=?2 AND payload_json=?3 AND result_json IS NULL",
                rusqlite::params![
                    account_id,
                    intent.request_id.to_string(),
                    payload_json,
                    result_json,
                ],
            )?;
            if completed != 1 {
                return Err(ScanError::InvalidShopState);
            }
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn all_time_usage(&self, agent: Agent) -> Result<TokenUsage, ScanError> {
        let mut statement = self
            .connection
            .prepare("SELECT total_tokens, coverage FROM daily_agent_total WHERE agent=?1")?;
        let rows = statement.query_map([agent_name(agent)], |row| {
            Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut known: Option<u64> = None;
        let mut incomplete = false;
        let mut unsupported = false;
        for row in rows {
            let (total, coverage) = row?;
            if let Some(total) = total {
                let total = as_u64(total)?;
                known = Some(
                    known
                        .unwrap_or(0)
                        .checked_add(total)
                        .ok_or(ScanError::InvalidCount)?,
                );
            }
            if coverage != "complete" {
                incomplete = true;
            }
            if coverage == "unsupported" {
                unsupported = true;
            }
        }
        let coverage = match (known, incomplete, unsupported) {
            (Some(_), true, _) => UsageCoverage::Partial,
            (Some(_), false, _) => UsageCoverage::Complete,
            (None, _, true) => UsageCoverage::Unsupported,
            (None, _, _) => UsageCoverage::Unavailable,
        };
        Ok(TokenUsage {
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            total_tokens: known,
            coverage,
        })
    }
}

fn signed_account_scope(account_id: &str) -> Result<(String, String), ScanError> {
    let user_id = account_id.strip_prefix("account:").unwrap_or(account_id);
    let user_id = uuid::Uuid::parse_str(user_id)
        .map_err(|_| ScanError::InvalidShopState)?
        .to_string();
    Ok((user_id.clone(), format!("account:{user_id}")))
}

fn pending_signed_reset_intent_in(
    connection: &Connection,
    account_scope: &str,
) -> Result<Option<SignedResetIntent>, ScanError> {
    let (_, account_scope) = signed_account_scope(account_scope)?;
    let mut statement = connection.prepare(
        "SELECT request_id,payload_json FROM shop_action_request
         WHERE account_id=?1 AND result_json IS NULL ORDER BY created_at_utc,request_id",
    )?;
    let mut rows = statement.query([account_scope.as_str()])?;
    let mut pending = None;
    while let Some(row) = rows.next()? {
        let request_id: String = row.get(0)?;
        let payload_json: String = row.get(1)?;
        let request: ShopRequest =
            serde_json::from_str(&payload_json).map_err(|_| ScanError::InvalidShopState)?;
        let ShopRequest::ResetPlanet {
            request_id: payload_request_id,
            cycle_id,
        } = request
        else {
            return Err(ScanError::InvalidShopState);
        };
        let parsed_request_id = uuid::Uuid::parse_str(&payload_request_id)
            .map_err(|_| ScanError::InvalidShopState)?;
        if parsed_request_id.to_string() != request_id || cycle_id.trim().is_empty() {
            return Err(ScanError::InvalidShopState);
        }
        let intent = SignedResetIntent {
            account_id: signed_account_scope(&account_scope)?.0,
            request_id: parsed_request_id,
            expected_old_cycle_id: cycle_id,
        };
        if pending.replace(intent).is_some() {
            return Err(ScanError::InvalidShopState);
        }
    }
    Ok(pending)
}

fn setting_value(connection: &Connection, key: &str) -> Result<Option<String>, ScanError> {
    connection
        .query_row("SELECT value FROM setting WHERE key=?1", [key], |row| {
            row.get(0)
        })
        .optional()
        .map_err(Into::into)
}

fn apply_confirmed_reset_planet_state_in_transaction(
    connection: &Connection,
    state: &PlanetState,
    account_id: &str,
    expected_old_cycle_id: &str,
) -> Result<(), ScanError> {
    let active_cycle: String = connection.query_row(
        "SELECT value FROM setting WHERE key='planet_current_cycle_id'",
        [],
        |row| row.get(0),
    )?;
    if active_cycle != expected_old_cycle_id
        || state.current_cycle_id.trim().is_empty()
        || state.current_cycle_id == expected_old_cycle_id
        || state.current_planet_tokens != 0
        || !state.growth_credit.is_finite()
        || state.growth_credit < 0.0
        || !state.progress_to_next.is_finite()
    {
        return Err(ScanError::InvalidShopState);
    }

    let cycle_started = DateTime::parse_from_rfc3339(&state.cycle_started_at_utc)
        .map_err(|_| ScanError::InvalidShopState)?
        .with_timezone(&Utc);
    let last_reset = state
        .last_reset_at_utc
        .as_deref()
        .ok_or(ScanError::InvalidShopState)?;
    let last_reset = DateTime::parse_from_rfc3339(last_reset)
        .map_err(|_| ScanError::InvalidShopState)?
        .with_timezone(&Utc);
    let reset_available = state
        .reset_available_at_utc
        .as_deref()
        .ok_or(ScanError::InvalidShopState)?;
    let reset_available = DateTime::parse_from_rfc3339(reset_available)
        .map_err(|_| ScanError::InvalidShopState)?
        .with_timezone(&Utc);
    if cycle_started != last_reset || reset_available <= last_reset {
        return Err(ScanError::InvalidShopState);
    }
    let _: Tz = state
        .timezone
        .parse()
        .map_err(|_| ScanError::TimezoneMismatch)?;

    let current_nickname = setting_value(connection, "planet_nickname")?;
    let current_avatar = setting_value(connection, "planet_avatar")?;
    match (current_nickname, current_avatar) {
        (Some(_), Some(_)) => {}
        (None, None) => {
            if let Some(profile) = &state.profile {
                let nickname = profile.nickname.trim();
                if nickname.is_empty() || nickname.chars().count() > 24 {
                    return Err(ScanError::InvalidProfile);
                }
                let avatar = match profile.avatar {
                    PlanetAvatar::Masculine => "masculine",
                    PlanetAvatar::Feminine => "feminine",
                };
                set_setting_value(connection, "planet_nickname", nickname)?;
                set_setting_value(connection, "planet_avatar", avatar)?;
            }
        }
        _ => return Err(ScanError::InvalidProfile),
    }

    let mut seen_cycles = std::collections::HashSet::new();
    for credit in &state.wallet_credits {
        if credit.previous_cycle_id.trim().is_empty()
            || !seen_cycles.insert(&credit.previous_cycle_id)
        {
            return Err(ScanError::InvalidShopState);
        }
        DateTime::parse_from_rfc3339(&credit.created_at_utc)
            .map_err(|_| ScanError::InvalidShopState)?;
        as_i64(credit.amount)?;
    }
    let mut seen_objects = std::collections::HashSet::new();
    for object in &state.objects {
        if !seen_objects.insert((object.stage, object.ordinal)) {
            return Err(ScanError::InvalidShopState);
        }
    }

    for (key, value) in [
        ("planet_current_cycle_id", state.current_cycle_id.as_str()),
        ("planet_cycle_started_at_utc", state.cycle_started_at_utc.as_str()),
        ("planet_last_reset_at_utc", last_reset.to_rfc3339().as_str()),
        (
            "planet_reset_available_at_utc",
            reset_available.to_rfc3339().as_str(),
        ),
        ("planet_timezone", state.timezone.as_str()),
        ("planet_remote_cycle_id", state.current_cycle_id.as_str()),
        (
            "planet_remote_current_tokens",
            &state.current_planet_tokens.to_string(),
        ),
        ("planet_remote_lifetime_tokens", &state.lifetime_tokens.to_string()),
        ("planet_remote_growth_credit", &state.growth_credit.to_string()),
        ("planet_remote_incomplete", &state.incomplete.to_string()),
    ] {
        set_setting_value(connection, key, value)?;
    }

    connection.execute("DELETE FROM planet_object", [])?;
    for object in &state.objects {
        connection.execute(
            "INSERT INTO planet_object(cycle_id,stage,ordinal,kind,x,y,seed)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                state.current_cycle_id,
                object.stage,
                object.ordinal,
                object.kind,
                object.x,
                object.y,
                object.seed.to_string(),
            ],
        )?;
    }

    connection.execute("DELETE FROM planet_wallet_credit", [])?;
    for credit in &state.wallet_credits {
        connection.execute(
            "INSERT INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
             VALUES (?1,?2,?3)",
            params![credit.previous_cycle_id, as_i64(credit.amount)?, credit.created_at_utc],
        )?;
    }

    // Ensure the reset result remains scoped to the same authenticated planet.
    let active_account: String = connection.query_row(
        "SELECT value FROM setting WHERE key='planet_account_id'",
        [],
        |row| row.get(0),
    )?;
    if active_account != account_id {
        return Err(ScanError::InvalidShopState);
    }
    Ok(())
}

fn set_setting_value(connection: &Connection, key: &str, value: &str) -> Result<(), ScanError> {
    connection.execute(
        "INSERT INTO setting(key,value) VALUES (?1,?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value
         WHERE setting.value IS NOT excluded.value",
        params![key, value],
    )?;
    Ok(())
}

fn parse_utc_setting(connection: &Connection, key: &str) -> Result<DateTime<Utc>, ScanError> {
    let value = setting_value(connection, key)?.ok_or(ScanError::Database)?;
    DateTime::parse_from_rfc3339(&value)
        .map(|date| date.with_timezone(&Utc))
        .map_err(|_| ScanError::Database)
}

pub(crate) fn agent_name(agent: Agent) -> &'static str {
    match agent {
        Agent::Codex => "codex",
        Agent::ClaudeCode => "claude_code",
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
fn optional_i64(value: Option<u64>) -> Result<Option<i64>, ScanError> {
    value.map(as_i64).transpose()
}

pub(crate) fn insert_record(
    connection: &Connection,
    record: &ParsedRecord,
    source_id: &str,
    timezone: Tz,
) -> Result<bool, ScanError> {
    let date = record
        .occurred_at_utc
        .with_timezone(&timezone)
        .format("%Y-%m-%d")
        .to_string();
    let kind = match record.kind {
        RecordKind::Response => "response",
        RecordKind::CumulativeSnapshot => "snapshot",
    };
    let version = match record.agent {
        Agent::Codex => crate::collectors::codex::PARSER_VERSION,
        Agent::ClaudeCode => crate::collectors::claude_code::PARSER_VERSION,
    };
    let event_key = if record.kind == RecordKind::CumulativeSnapshot {
        format!("{source_id}:{}", record.event_key)
    } else {
        record.event_key.clone()
    };
    let changed = connection.execute("INSERT INTO usage_record (
        event_key,source_id,agent,kind,bucket_date,occurred_at_utc,
        input_tokens,output_tokens,cache_read_tokens,cache_write_tokens,total_tokens,coverage,parser_version
    ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
    ON CONFLICT(event_key) DO UPDATE SET
      source_id=excluded.source_id,agent=excluded.agent,kind=excluded.kind,
      bucket_date=excluded.bucket_date,occurred_at_utc=excluded.occurred_at_utc,
      input_tokens=excluded.input_tokens,output_tokens=excluded.output_tokens,
      cache_read_tokens=excluded.cache_read_tokens,cache_write_tokens=excluded.cache_write_tokens,
      total_tokens=excluded.total_tokens,coverage=excluded.coverage,
      parser_version=excluded.parser_version
    WHERE usage_record.source_id IS NOT excluded.source_id
      OR usage_record.agent IS NOT excluded.agent
      OR usage_record.kind IS NOT excluded.kind
      OR usage_record.bucket_date IS NOT excluded.bucket_date
      OR usage_record.occurred_at_utc IS NOT excluded.occurred_at_utc
      OR usage_record.input_tokens IS NOT excluded.input_tokens
      OR usage_record.output_tokens IS NOT excluded.output_tokens
      OR usage_record.cache_read_tokens IS NOT excluded.cache_read_tokens
      OR usage_record.cache_write_tokens IS NOT excluded.cache_write_tokens
      OR usage_record.total_tokens IS NOT excluded.total_tokens
      OR usage_record.coverage IS NOT excluded.coverage
      OR usage_record.parser_version IS NOT excluded.parser_version", params![
        event_key, source_id, agent_name(record.agent), kind, date, record.occurred_at_utc.to_rfc3339(),
        optional_i64(record.usage.input_tokens)?, optional_i64(record.usage.output_tokens)?,
        optional_i64(record.usage.cache_read_tokens)?, optional_i64(record.usage.cache_write_tokens)?,
        optional_i64(record.usage.total_tokens)?, coverage_name(record.usage.coverage), version,
    ])?;
    connection.execute(
        "INSERT OR IGNORE INTO planet_usage_owner(event_key,account_id)
         VALUES (?1,(SELECT value FROM setting WHERE key='planet_account_id'))",
        [&event_key],
    )?;
    Ok(changed > 0)
}

pub(crate) fn rebuild_daily(connection: &Connection) -> Result<(), ScanError> {
    let mut groups: BTreeMap<(String, String), (Option<u64>, bool, bool)> = BTreeMap::new();
    {
        let mut statement = connection.prepare("SELECT agent,bucket_date,total_tokens,coverage FROM usage_record r
            WHERE r.kind='response' OR NOT EXISTS (
                SELECT 1 FROM usage_record other WHERE other.source_id=r.source_id AND other.kind='response' AND other.agent=r.agent
            )")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        for row in rows {
            let (agent, date, total, coverage) = row?;
            let entry = groups.entry((agent, date)).or_insert((None, false, false));
            if let Some(total) = total {
                entry.0 = Some(
                    entry
                        .0
                        .unwrap_or(0)
                        .checked_add(as_u64(total)?)
                        .ok_or(ScanError::InvalidCount)?,
                );
            }
            if coverage != "complete" {
                entry.1 = true;
            }
            if coverage == "unsupported" {
                entry.2 = true;
            }
        }
    }
    connection.execute("DELETE FROM daily_agent_total", [])?;
    for ((agent, date), (known, incomplete, unsupported)) in groups {
        let coverage = if known.is_some() && incomplete {
            "partial"
        } else if known.is_some() {
            "complete"
        } else if unsupported {
            "unsupported"
        } else {
            "unavailable"
        };
        connection.execute("INSERT INTO daily_agent_total(agent,bucket_date,total_tokens,coverage) VALUES (?1,?2,?3,?4)",
            params![agent, date, optional_i64(known)?, coverage])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Ledger;
    use crate::collectors::codex;
    use crate::domain::usage::Agent;
    use chrono_tz::Asia::Seoul;

    fn account_record(key: &str, tokens: u64) -> crate::collectors::ParsedRecord {
        crate::collectors::ParsedRecord {
            agent: Agent::Codex,
            kind: crate::collectors::RecordKind::Response,
            event_key: key.into(),
            occurred_at_utc: chrono::Utc::now(),
            usage: crate::domain::usage::TokenUsage {
                input_tokens: None,
                output_tokens: None,
                cache_read_tokens: None,
                cache_write_tokens: None,
                total_tokens: Some(tokens),
                coverage: crate::domain::usage::UsageCoverage::Complete,
            },
        }
    }

    fn remote_planet_state(
        cycle_id: &str,
        cycle_started_at_utc: &str,
        last_reset_at_utc: &str,
        reset_available_at_utc: Option<&str>,
    ) -> crate::domain::planet::PlanetState {
        crate::domain::planet::PlanetState {
            version: 1,
            profile: None,
            timezone: "UTC".into(),
            current_cycle_id: cycle_id.into(),
            cycle_started_at_utc: cycle_started_at_utc.into(),
            last_reset_at_utc: Some(last_reset_at_utc.into()),
            wallet_balance: 0,
            wallet_credits: vec![],
            current_planet_tokens: 0,
            lifetime_tokens: 0,
            growth_credit: 0.0,
            stage: 0,
            progress_to_next: 0.0,
            incomplete: false,
            can_reset: false,
            reset_available_at_utc: reset_available_at_utc.map(str::to_owned),
            objects: vec![],
            removed_natural_keys: vec![],
        }
    }

    fn reset_deadline_snapshot(ledger: &Ledger) -> Option<chrono::DateTime<chrono::Utc>> {
        let unavailable_usage = crate::domain::usage::TokenUsage {
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            total_tokens: None,
            coverage: crate::domain::usage::UsageCoverage::Unavailable,
        };
        let summary = crate::collectors::discovery::ScanSummary {
            codex: unavailable_usage.clone(),
            claude_code: unavailable_usage,
            codex_source: crate::collectors::discovery::SourceHealth::UsageUnavailable,
            claude_code_source: crate::collectors::discovery::SourceHealth::UsageUnavailable,
            confirmed_subtotal: None,
            complete_total: None,
            scanned_at_utc: chrono::Utc::now(),
        };
        let value = crate::growth::world_snapshot(ledger, summary)
            .unwrap()
            .planet
            .reset_available_at_utc;
        value.map(|value| {
            chrono::DateTime::parse_from_rfc3339(&value)
                .unwrap()
                .with_timezone(&chrono::Utc)
        })
    }

    #[test]
    fn remote_reset_deadline_tracks_only_the_accepted_current_cycle() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        ledger.ensure_planet_account("reset-deadline-user").unwrap();
        ledger.connection.execute_batch(
            "INSERT INTO setting(key,value) VALUES
               ('planet_cycle_started_at_utc','2026-10-02T00:00:00Z'),
               ('planet_last_reset_at_utc','2026-10-02T00:00:00Z'),
               ('planet_current_cycle_id','cycle-current'),
               ('planet_remote_cycle_id','cycle-current'),
               ('planet_reset_available_at_utc','2026-10-03T00:00:00Z')
             ON CONFLICT(key) DO UPDATE SET value=excluded.value;",
        ).unwrap();

        ledger.merge_remote_planet_state(&remote_planet_state(
            "cycle-current",
            "2026-10-02T00:00:00Z",
            "2026-10-02T00:00:00Z",
            Some("2026-10-02T18:00:00Z"),
        )).unwrap();
        assert_eq!(
            reset_deadline_snapshot(&ledger),
            Some(chrono::DateTime::parse_from_rfc3339("2026-10-02T18:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc)),
            "the canonical server deadline should replace the old 24-hour fallback",
        );

        ledger.merge_remote_planet_state(&remote_planet_state(
            "cycle-current",
            "2026-10-02T00:00:00Z",
            "2026-10-02T00:00:00Z",
            None,
        )).unwrap();
        assert_eq!(
            reset_deadline_snapshot(&ledger),
            Some(chrono::DateTime::parse_from_rfc3339("2026-10-03T00:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc)),
            "a canonical None removes the override and returns to the legacy fallback",
        );

        ledger.merge_remote_planet_state(&remote_planet_state(
            "cycle-next",
            "2026-10-03T00:00:00Z",
            "2026-10-03T00:00:00Z",
            Some("2026-10-03T18:00:00Z"),
        )).unwrap();
        assert_eq!(
            reset_deadline_snapshot(&ledger),
            Some(chrono::DateTime::parse_from_rfc3339("2026-10-03T18:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc)),
            "an accepted new cycle uses its own canonical deadline",
        );

        ledger.connection.execute(
            "UPDATE setting SET value='2026-10-04T00:00:00Z' WHERE key='planet_reset_available_at_utc'",
            [],
        ).unwrap();
        ledger.merge_remote_planet_state(&remote_planet_state(
            "cycle-stale",
            "2026-10-02T00:00:00Z",
            "2026-10-02T00:00:00Z",
            Some("2026-10-02T18:00:00Z"),
        )).unwrap();
        assert_eq!(
            reset_deadline_snapshot(&ledger),
            Some(chrono::DateTime::parse_from_rfc3339("2026-10-04T00:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc)),
            "a rejected stale cycle must not overwrite the active deadline",
        );
    }

    #[test]
    fn signed_reset_intent_survives_reopen_and_cannot_change_request_or_old_cycle() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let account_id = "00000000-0000-0000-0000-000000000041";
        let request_id = uuid::Uuid::parse_str("90000000-0000-0000-0000-000000000041").unwrap();
        let original_cycle;
        {
            let mut ledger = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
            ledger.ensure_planet_account(account_id).unwrap();
            original_cycle = ledger.planet_cycle_id().unwrap();
            let intent = ledger
                .prepare_signed_reset_intent(request_id, &original_cycle)
                .unwrap();
            assert_eq!(intent.request_id, request_id);
            assert_eq!(intent.expected_old_cycle_id, original_cycle);
        }

        let mut reopened = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        let intent = reopened
            .pending_signed_reset_intent(account_id)
            .unwrap()
            .expect("uncertain reset intent remains available after restart");
        assert_eq!(intent.request_id, request_id);
        assert_eq!(intent.expected_old_cycle_id, original_cycle);
        assert!(reopened
            .prepare_signed_reset_intent(uuid::Uuid::new_v4(), &original_cycle)
            .is_err(), "a pending reset cannot be replaced by a new request id");
        assert!(reopened
            .prepare_signed_reset_intent(request_id, "different-old-cycle")
            .is_err(), "the persisted request id cannot be rebound to another cycle");
    }

    #[test]
    fn planet_accounts_restore_profile_wallet_objects_and_owned_usage_after_restart() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), Seoul).unwrap();
        ledger
            .set_planet_profile("Alice", crate::domain::planet::PlanetAvatar::Feminine)
            .unwrap();
        let alice_record = account_record("alice:response", 42);
        ledger.insert(&alice_record).unwrap();
        ledger
            .ensure_world_scope("world-a", "alice", Seoul)
            .unwrap();
        assert_eq!(
            ledger
                .planet_device_contribution(false)
                .unwrap()
                .lifetime_tokens,
            42
        );
        let alice_cycle_before_reset = ledger.planet_cycle_id().unwrap();
        assert!(matches!(
            ledger.reset_planet(chrono::Utc::now()),
            Err(crate::storage::ledger::ScanError::InvalidShopState),
        ));
        assert_eq!(ledger.planet_cycle_id().unwrap(), alice_cycle_before_reset);
        assert!(ledger.planet_wallet_credits().unwrap().is_empty());
        // Model a server-confirmed wallet snapshot while testing account persistence.
        ledger.connection.execute(
            "INSERT INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
             VALUES (?1,42,'2026-10-01T00:00:00Z')",
            [&alice_cycle_before_reset],
        ).unwrap();
        ledger
            .ensure_planet_object(0, 0, "tree", 20, 30, 12)
            .unwrap();
        let alice_cycle = ledger.planet_cycle_id().unwrap();

        ledger.ensure_world_scope("world-b", "bob", Seoul).unwrap();
        assert!(
            ledger.planet_profile().unwrap().is_none(),
            "Bob must not inherit Alice's profile"
        );
        assert!(ledger.planet_wallet_credits().unwrap().is_empty());
        assert!(ledger.planet_objects().unwrap().is_empty());
        assert_eq!(
            ledger
                .planet_device_contribution(false)
                .unwrap()
                .lifetime_tokens,
            0
        );
        ledger
            .set_planet_profile("Bob", crate::domain::planet::PlanetAvatar::Masculine)
            .unwrap();
        ledger.insert(&account_record("bob:response", 7)).unwrap();
        // Replaying an old record in a different account must retain its owner.
        ledger
            .connection
            .execute(
                "DELETE FROM usage_record WHERE event_key='alice:response'",
                [],
            )
            .unwrap();
        ledger.insert(&alice_record).unwrap();
        assert_eq!(
            ledger
                .planet_device_contribution(false)
                .unwrap()
                .lifetime_tokens,
            7
        );
        assert_eq!(
            ledger.reform_daily_totals(Seoul).unwrap()[0].total_tokens,
            Some(7)
        );
        drop(ledger);

        let mut ledger = Ledger::open(file.path(), Seoul).unwrap();
        ledger
            .ensure_world_scope("world-a", "alice", Seoul)
            .unwrap();
        assert_eq!(ledger.planet_profile().unwrap().unwrap().nickname, "Alice");
        assert_eq!(ledger.planet_cycle_id().unwrap(), alice_cycle);
        assert_eq!(ledger.planet_wallet_credits().unwrap()[0].amount, 42);
        assert_eq!(ledger.planet_objects().unwrap()[0].kind, "tree");
        assert_eq!(
            ledger
                .planet_device_contribution(false)
                .unwrap()
                .lifetime_tokens,
            42
        );
    }

    #[test]
    fn another_accounts_response_cannot_replace_a_cumulative_fallback() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), Seoul).unwrap();
        ledger.ensure_planet_account("alice").unwrap();
        let mut snapshot = account_record("alice:snapshot", 100);
        snapshot.kind = crate::collectors::RecordKind::CumulativeSnapshot;
        ledger.insert(&snapshot).unwrap();
        ledger.ensure_planet_account("bob").unwrap();
        ledger.insert(&account_record("bob:response", 7)).unwrap();
        ledger.ensure_planet_account("alice").unwrap();
        assert_eq!(
            ledger
                .planet_device_contribution(false)
                .unwrap()
                .lifetime_tokens,
            100
        );
        assert_eq!(
            ledger.reform_daily_totals(Seoul).unwrap()[0].total_tokens,
            Some(100)
        );
    }

    #[test]
    fn old_world_cache_does_not_prove_the_owner_of_an_uploaded_planet() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), Seoul).unwrap();
        ledger
            .set_planet_profile("Alice", crate::domain::planet::PlanetAvatar::Feminine)
            .unwrap();
        ledger
            .connection
            .execute_batch(
                "DELETE FROM setting WHERE key='planet_account_id';
          INSERT INTO setting(key,value) VALUES ('sharing_user_id','alice'),
          ('planet_remote_lifetime_tokens','500'); DELETE FROM planet_usage_owner;",
            )
            .unwrap();
        drop(ledger);
        let mut ledger = Ledger::open(file.path(), Seoul).unwrap();
        ledger.ensure_planet_account("bob").unwrap();
        assert!(ledger.planet_profile().unwrap().is_none());
        assert!(ledger.synced_planet_lifetime_tokens().unwrap().is_none());
        ledger.ensure_planet_account("alice").unwrap();
        assert!(ledger.planet_profile().unwrap().is_none());
        assert!(ledger.synced_planet_lifetime_tokens().unwrap().is_none());
        let archived: i64 = ledger
            .connection
            .query_row(
                "SELECT COUNT(*) FROM planet_account_state WHERE account_id='legacy'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(archived, 1);
    }

    #[test]
    fn old_uploaded_planet_with_unknown_owner_is_not_claimed_by_the_next_login() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), Seoul).unwrap();
        ledger
            .set_planet_profile("Unknown", crate::domain::planet::PlanetAvatar::Feminine)
            .unwrap();
        ledger
            .connection
            .execute_batch(
                "DELETE FROM setting WHERE key='planet_account_id';
          INSERT INTO setting(key,value) VALUES ('planet_remote_lifetime_tokens','500');
          DELETE FROM planet_usage_owner;",
            )
            .unwrap();
        drop(ledger);
        let mut ledger = Ledger::open(file.path(), Seoul).unwrap();
        ledger.ensure_planet_account("bob").unwrap();
        assert!(ledger.planet_profile().unwrap().is_none());
        assert!(ledger.synced_planet_lifetime_tokens().unwrap().is_none());
        let archived: i64 = ledger
            .connection
            .query_row(
                "SELECT COUNT(*) FROM planet_account_state WHERE account_id='legacy'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(archived, 1);
    }

    const RESPONSE: &str = r#"{"timestamp":"2026-09-24T15:30:00Z","type":"token_usage_record","payload":{"session_id":"s1","response_id":"r1","usage":{"total_tokens":42}}}"#;

    #[test]
    fn later_complete_response_replaces_unavailable_same_identity() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), Seoul).unwrap();
        let unavailable = codex::parse_line(r#"{"timestamp":"2026-09-24T15:30:00Z","type":"token_usage_record","payload":{"session_id":"s1","response_id":"r1"}}"#).unwrap().unwrap();
        let complete = codex::parse_line(RESPONSE).unwrap().unwrap();
        ledger.insert(&unavailable).unwrap();
        ledger.insert(&complete).unwrap();
        assert_eq!(
            ledger.daily_total(Agent::Codex, "2026-09-25").unwrap(),
            Some(42)
        );
    }

    #[test]
    fn custom_source_root_is_local_and_survives_restart() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let folder = std::path::PathBuf::from("/example/private/sessions");
        {
            let mut ledger = Ledger::open(file.path(), Seoul).unwrap();
            ledger.set_custom_root(Agent::Codex, &folder).unwrap();
        }
        let ledger = Ledger::open(file.path(), Seoul).unwrap();
        assert_eq!(ledger.custom_root(Agent::Codex).unwrap(), Some(folder));
    }

    #[test]
    fn saved_world_timezone_survives_device_timezone_change() {
        let file = tempfile::NamedTempFile::new().unwrap();
        Ledger::open(file.path(), Seoul).unwrap();
        assert_eq!(Ledger::saved_timezone(file.path()).unwrap(), Some(Seoul));
    }

    #[test]
    fn repeated_record_and_reopened_ledger_keep_one_total_in_world_timezone() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let record = codex::parse_line(RESPONSE).unwrap().unwrap();
        {
            let mut ledger = Ledger::open(file.path(), Seoul).unwrap();
            ledger.insert(&record).unwrap();
            ledger.insert(&record).unwrap();
            assert_eq!(
                ledger.daily_total(Agent::Codex, "2026-09-25").unwrap(),
                Some(42)
            );
            assert_eq!(
                ledger.daily_total(Agent::Codex, "2026-09-24").unwrap(),
                None
            );
        }
        let ledger = Ledger::open(file.path(), Seoul).unwrap();
        assert_eq!(
            ledger.daily_total(Agent::Codex, "2026-09-25").unwrap(),
            Some(42)
        );
    }

    #[test]
    fn shared_daily_totals_use_creator_timezone_and_keep_unknown() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), Seoul).unwrap();
        let known = codex::parse_line(RESPONSE).unwrap().unwrap();
        let unknown = codex::parse_line(r#"{"timestamp":"2026-09-24T15:40:00Z","type":"token_usage_record","payload":{"session_id":"s1","response_id":"r2"}}"#).unwrap().unwrap();
        ledger.insert(&known).unwrap();
        ledger.insert(&unknown).unwrap();
        let shared = ledger.shared_daily_totals(chrono_tz::UTC).unwrap();
        assert_eq!(shared.len(), 1);
        assert_eq!(shared[0].bucket_date, "2026-09-24");
        assert_eq!(shared[0].total_tokens, Some(42));
        assert_eq!(
            shared[0].coverage,
            crate::domain::usage::UsageCoverage::Partial
        );
    }
}
