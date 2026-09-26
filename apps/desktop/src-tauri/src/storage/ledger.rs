use std::{collections::BTreeMap, fmt, path::Path};

use chrono::{DateTime, Duration, Utc};
use chrono_tz::Tz;
use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::collectors::{ParsedRecord, RecordKind};
use crate::domain::planet::{
    PlanetAvatar, PlanetDeviceContribution, PlanetObject, PlanetProfile, PlanetState,
    PlanetWalletCredit,
};
use crate::domain::usage::{Agent, TokenUsage, UsageCoverage};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScanError {
    Database,
    SourceIo,
    SourcePermission,
    InvalidCount,
    TimezoneMismatch,
    ResetCooldown,
    InvalidProfile,
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
        };
        ledger.initialize_planet_accounts()?;
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
        let cycle_start = self.last_reset_at()?.unwrap_or(activation).max(activation);
        let mut statement = self.connection.prepare(
            "SELECT r.occurred_at_utc,r.total_tokens FROM usage_record r
             WHERE r.total_tokens IS NOT NULL AND r.occurred_at_utc > ?1
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
        let rows = statement.query_map(params![activation.to_rfc3339()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
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
            if timestamp > cycle_start {
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
        if self
            .last_reset_at()?
            .is_some_and(|last| now < last + Duration::hours(24))
        {
            return Err(ScanError::ResetCooldown);
        }
        let previous_cycle_id = self.planet_cycle_id()?;
        let (_, local_current_tokens, _) = self.planet_usage_totals()?;
        let current_tokens = self
            .synced_planet_metrics()?
            .filter(|(cycle_id, _, _, _)| cycle_id == &previous_cycle_id)
            .map(|(_, remote_tokens, _, _)| local_current_tokens.max(remote_tokens))
            .unwrap_or(local_current_tokens);
        let new_cycle_id = uuid::Uuid::new_v4().to_string();
        let tx = self.connection.transaction()?;
        tx.execute(
            "INSERT OR IGNORE INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
             VALUES (?1,?2,?3)",
            params![previous_cycle_id, as_i64(current_tokens)?, now.to_rfc3339()],
        )?;
        for (key, value) in [
            ("planet_last_reset_at_utc", now.to_rfc3339()),
            ("planet_cycle_started_at_utc", now.to_rfc3339()),
            ("planet_current_cycle_id", new_cycle_id),
        ] {
            tx.execute(
                "INSERT INTO setting(key,value) VALUES (?1,?2)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![key, value],
            )?;
        }
        tx.execute("DELETE FROM setting WHERE key IN ('planet_remote_cycle_id','planet_remote_current_tokens','planet_remote_growth_credit','planet_remote_incomplete')", [])?;
        tx.execute("DELETE FROM planet_object", [])?;
        tx.commit()?;
        Ok(current_tokens)
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

fn setting_value(connection: &Connection, key: &str) -> Result<Option<String>, ScanError> {
    connection
        .query_row("SELECT value FROM setting WHERE key=?1", [key], |row| {
            row.get(0)
        })
        .optional()
        .map_err(Into::into)
}

fn set_setting_value(connection: &Connection, key: &str, value: &str) -> Result<(), ScanError> {
    connection.execute(
        "INSERT INTO setting(key,value) VALUES (?1,?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
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
    tx: &Transaction<'_>,
    record: &ParsedRecord,
    source_id: &str,
    timezone: Tz,
) -> Result<(), ScanError> {
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
    tx.execute("INSERT INTO usage_record (
        event_key,source_id,agent,kind,bucket_date,occurred_at_utc,
        input_tokens,output_tokens,cache_read_tokens,cache_write_tokens,total_tokens,coverage,parser_version
    ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
    ON CONFLICT(event_key) DO UPDATE SET
      input_tokens=excluded.input_tokens,output_tokens=excluded.output_tokens,
      cache_read_tokens=excluded.cache_read_tokens,cache_write_tokens=excluded.cache_write_tokens,
      total_tokens=excluded.total_tokens,coverage=excluded.coverage,
      parser_version=excluded.parser_version
    WHERE usage_record.total_tokens IS NULL AND excluded.total_tokens IS NOT NULL", params![
        event_key, source_id, agent_name(record.agent), kind, date, record.occurred_at_utc.to_rfc3339(),
        optional_i64(record.usage.input_tokens)?, optional_i64(record.usage.output_tokens)?,
        optional_i64(record.usage.cache_read_tokens)?, optional_i64(record.usage.cache_write_tokens)?,
        optional_i64(record.usage.total_tokens)?, coverage_name(record.usage.coverage), version,
    ])?;
    tx.execute(
        "INSERT OR IGNORE INTO planet_usage_owner(event_key,account_id)
         VALUES (?1,(SELECT value FROM setting WHERE key='planet_account_id'))",
        [&event_key],
    )?;
    Ok(())
}

pub(crate) fn rebuild_daily(tx: &Transaction<'_>) -> Result<(), ScanError> {
    let mut groups: BTreeMap<(String, String), (Option<u64>, bool, bool)> = BTreeMap::new();
    {
        let mut statement = tx.prepare("SELECT agent,bucket_date,total_tokens,coverage FROM usage_record r
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
    tx.execute("DELETE FROM daily_agent_total", [])?;
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
        tx.execute("INSERT INTO daily_agent_total(agent,bucket_date,total_tokens,coverage) VALUES (?1,?2,?3,?4)",
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
        ledger.reset_planet(chrono::Utc::now()).unwrap();
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
