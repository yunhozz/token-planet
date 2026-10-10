use std::{collections::BTreeMap, fmt, path::Path};

use chrono::{DateTime, Duration, SecondsFormat, Utc};
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
use super::guest_provenance::{
    initialize_guest_provenance_in_transaction, initialize_guest_provenance_schema,
    record_guest_occurrence_in_connection,
};
use super::shop_effects::{
    apply_confirmed_shop_effect_timeline_in_transaction, validate_reset_timeline_current_bound,
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
            cache_read_tokens,cache_write_tokens,total_tokens,coverage,r.kind FROM usage_record r
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
                row.get::<_, String>(8)?,
            ))
        })?;
        let mut groups: BTreeMap<(String, String), (SharedDailyTotal, bool, bool)> =
            BTreeMap::new();
        let reset_cutoff = self.device_reset_cutoff()?;
        for row in rows {
            let (agent, occurred_at, input, output, cache_read, cache_write, total, coverage, kind) =
                row?;
            let timestamp = chrono::DateTime::parse_from_rfc3339(&occurred_at)
                .map_err(|_| ScanError::Database)?;
            if !super::device_reset::record_after_reset_cutoff(
                timestamp.with_timezone(&Utc),
                reset_cutoff,
            ) || (reset_cutoff.is_some() && kind != "response")
            {
                continue;
            }
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
        let has_existing_schema: bool = connection.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM sqlite_master
                WHERE type='table' AND name NOT LIKE 'sqlite_%'
             )",
            [],
            |row| row.get(0),
        )?;
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
        super::device_reset::initialize_device_reset_schema(&connection)?;
        initialize_guest_provenance_schema(&connection, !has_existing_schema)?;
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
        if !super::device_reset::recovery_required(&connection) {
            if setting_value(&connection, "planet_activation_at_utc")?.is_none() {
                set_setting_value(
                    &connection,
                    "planet_activation_at_utc",
                    &Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true),
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
        if !has_existing_schema {
            let transaction = ledger.connection.transaction()?;
            initialize_guest_provenance_in_transaction(&transaction, Utc::now())?;
            transaction.commit()?;
        }
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
        Self::planet_usage_totals_from_connection(&self.connection)
    }

    pub(crate) fn planet_usage_totals_from_connection(
        connection: &Connection,
    ) -> Result<(BTreeMap<String, u64>, u64, u64), ScanError> {
        let activation = parse_utc_setting(connection, "planet_activation_at_utc")?;
        let planet_timezone: Tz = setting_value(connection, "planet_timezone")?
            .ok_or(ScanError::Database)?
            .parse()
            .map_err(|_| ScanError::TimezoneMismatch)?;
        let local_cycle_start = setting_value(connection, "planet_last_reset_at_utc")?
            .map(|value| {
                DateTime::parse_from_rfc3339(&value)
                    .map(|date| date.with_timezone(&Utc))
                    .map_err(|_| ScanError::Database)
            })
            .transpose()?
            .unwrap_or(activation)
            .max(activation);
        let confirmed_cycle_start = Self::confirmed_current_cycle_start_in_connection(connection)?;
        let server_cycle_bound_is_known = confirmed_cycle_start.is_some();
        let cycle_start = confirmed_cycle_start.unwrap_or(local_cycle_start);
        let local_guest_cycle_bound_is_known: bool = connection.query_row(
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
        let inclusive_cycle_boundary =
            server_cycle_bound_is_known || local_guest_cycle_bound_is_known;
        let codex_enabled = Self::agent_enabled_from_connection(connection, Agent::Codex)?;
        let claude_code_enabled =
            Self::agent_enabled_from_connection(connection, Agent::ClaudeCode)?;
        let mut statement = connection.prepare(
            "SELECT r.occurred_at_utc,r.total_tokens FROM usage_record r
             WHERE r.total_tokens IS NOT NULL
               AND (r.occurred_at_utc > ?1 OR (?4 AND substr(r.occurred_at_utc,1,19)=substr(?1,1,19)))
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
                codex_enabled,
                claude_code_enabled,
                local_guest_cycle_bound_is_known,
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
            if timestamp < activation
                || (!local_guest_cycle_bound_is_known && timestamp == activation)
            {
                continue;
            }
            let belongs_to_current_cycle = if inclusive_cycle_boundary {
                timestamp >= cycle_start
            } else {
                timestamp > cycle_start
            };
            if belongs_to_current_cycle {
                current = current.checked_add(tokens).ok_or(ScanError::InvalidCount)?;
                if tokens > 0 {
                    let date = timestamp
                        .with_timezone(&planet_timezone)
                        .format("%Y-%m-%d")
                        .to_string();
                    let total = daily.entry(date).or_insert(0_u64);
                    *total = total.checked_add(tokens).ok_or(ScanError::InvalidCount)?;
                }
            }
        }
        Ok((daily, current, lifetime))
    }

    fn agent_enabled_from_connection(
        connection: &Connection,
        agent: Agent,
    ) -> Result<bool, ScanError> {
        let key = format!("{}_enabled", agent_name(agent));
        Ok(setting_value(connection, &key)?.as_deref() != Some("false"))
    }

    pub fn planet_timezone(&self) -> Result<Tz, ScanError> {
        setting_value(&self.connection, "planet_timezone")?
            .ok_or(ScanError::Database)?
            .parse()
            .map_err(|_| ScanError::TimezoneMismatch)
    }

    pub fn set_planet_timezone(&self, timezone: &str) -> Result<(), ScanError> {
        let _: Tz = timezone.parse().map_err(|_| ScanError::TimezoneMismatch)?;
        self.require_guest_import_game_mutations_allowed()?;
        set_setting_value(&self.connection, "planet_timezone", timezone)
    }

    pub fn planet_device_contribution(
        &self,
        incomplete: bool,
    ) -> Result<PlanetDeviceContribution, ScanError> {
        Self::planet_device_contribution_from_connection(&self.connection, incomplete)
    }

    pub(crate) fn planet_device_contribution_from_connection(
        connection: &Connection,
        incomplete: bool,
    ) -> Result<PlanetDeviceContribution, ScanError> {
        let (daily_tokens, current_planet_tokens, lifetime_tokens) =
            Self::planet_usage_totals_from_connection(connection)?;
        Ok(PlanetDeviceContribution {
            device_id: setting_value(connection, "planet_device_id")?.ok_or(ScanError::Database)?,
            current_cycle_id: setting_value(connection, "planet_current_cycle_id")?
                .ok_or(ScanError::Database)?,
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
        self.require_guest_import_game_mutations_allowed()?;
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
        self.require_guest_import_game_mutations_allowed()?;
        if self
            .reset_available_at()?
            .is_some_and(|available| now < available)
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
        let eligible_guest_lineage: bool = self.connection.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM guest_provenance_lineage
                WHERE singleton=1 AND eligible=1
             )",
            [],
            |row| row.get(0),
        )?;
        let request_id = if eligible_guest_lineage {
            uuid::Uuid::new_v4().to_string()
        } else {
            format!("legacy-reset:{previous_cycle_id}")
        };
        let reset = self.reset_guest_planet(&request_id, &previous_cycle_id, now)?;
        match reset.status {
            crate::domain::cosmetic_shop::ShopActionStatus::Reset => Ok(current_tokens),
            crate::domain::cosmetic_shop::ShopActionStatus::CycleMismatch => {
                Err(ScanError::ResetCooldown)
            }
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
        if !self.guest_import_game_mutations_allowed()? {
            return Ok(());
        }
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
        self.require_guest_import_game_mutations_allowed()?;
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
        self.require_guest_import_game_mutations_allowed()?;
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
        self.require_guest_import_game_mutations_allowed()?;
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
        self.require_guest_import_game_mutations_allowed()?;
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
        self.require_guest_import_game_mutations_allowed()?;
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
        let expected_account_uuid =
            uuid::Uuid::parse_str(expected_account_id).map_err(|_| ScanError::InvalidShopState)?;
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

        record_ordinal_history_in(&transaction, &account_id, reset_state)?;
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
        let parsed_request_id =
            uuid::Uuid::parse_str(&payload_request_id).map_err(|_| ScanError::InvalidShopState)?;
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
        (
            "planet_cycle_started_at_utc",
            state.cycle_started_at_utc.as_str(),
        ),
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
        (
            "planet_remote_lifetime_tokens",
            &state.lifetime_tokens.to_string(),
        ),
        (
            "planet_remote_growth_credit",
            &state.growth_credit.to_string(),
        ),
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
            params![
                credit.previous_cycle_id,
                as_i64(credit.amount)?,
                credit.created_at_utc
            ],
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
    let mut provenance_record = record.clone();
    provenance_record.event_key = event_key.clone();
    record_guest_occurrence_in_connection(connection, &provenance_record, Utc::now())?;
    if super::device_reset::record_eligible_after_reset(connection, record)? {
        connection.execute(
            "INSERT OR IGNORE INTO planet_usage_owner(event_key,account_id)
             VALUES (?1,(SELECT value FROM setting WHERE key='planet_account_id'))",
            [&event_key],
        )?;
    } else {
        connection.execute(
            "DELETE FROM planet_usage_owner WHERE event_key=?1",
            [&event_key],
        )?;
    }
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
        ledger
            .connection
            .execute_batch(
                "INSERT INTO setting(key,value) VALUES
               ('planet_cycle_started_at_utc','2026-10-02T00:00:00Z'),
               ('planet_last_reset_at_utc','2026-10-02T00:00:00Z'),
               ('planet_current_cycle_id','cycle-current'),
               ('planet_remote_cycle_id','cycle-current'),
               ('planet_reset_available_at_utc','2026-10-03T00:00:00Z')
             ON CONFLICT(key) DO UPDATE SET value=excluded.value;",
            )
            .unwrap();

        ledger
            .merge_remote_planet_state(&remote_planet_state(
                "cycle-current",
                "2026-10-02T00:00:00Z",
                "2026-10-02T00:00:00Z",
                Some("2026-10-02T18:00:00Z"),
            ))
            .unwrap();
        assert_eq!(
            reset_deadline_snapshot(&ledger),
            Some(
                chrono::DateTime::parse_from_rfc3339("2026-10-02T18:00:00Z")
                    .unwrap()
                    .with_timezone(&chrono::Utc)
            ),
            "the canonical server deadline should replace the old 24-hour fallback",
        );

        ledger
            .merge_remote_planet_state(&remote_planet_state(
                "cycle-current",
                "2026-10-02T00:00:00Z",
                "2026-10-02T00:00:00Z",
                None,
            ))
            .unwrap();
        assert_eq!(
            reset_deadline_snapshot(&ledger),
            Some(
                chrono::DateTime::parse_from_rfc3339("2026-10-03T00:00:00Z")
                    .unwrap()
                    .with_timezone(&chrono::Utc)
            ),
            "a canonical None removes the override and returns to the legacy fallback",
        );

        ledger
            .merge_remote_planet_state(&remote_planet_state(
                "cycle-next",
                "2026-10-03T00:00:00Z",
                "2026-10-03T00:00:00Z",
                Some("2026-10-03T18:00:00Z"),
            ))
            .unwrap();
        assert_eq!(
            reset_deadline_snapshot(&ledger),
            Some(
                chrono::DateTime::parse_from_rfc3339("2026-10-03T18:00:00Z")
                    .unwrap()
                    .with_timezone(&chrono::Utc)
            ),
            "an accepted new cycle uses its own canonical deadline",
        );

        ledger.connection.execute(
            "UPDATE setting SET value='2026-10-04T00:00:00Z' WHERE key='planet_reset_available_at_utc'",
            [],
        ).unwrap();
        ledger
            .merge_remote_planet_state(&remote_planet_state(
                "cycle-stale",
                "2026-10-02T00:00:00Z",
                "2026-10-02T00:00:00Z",
                Some("2026-10-02T18:00:00Z"),
            ))
            .unwrap();
        assert_eq!(
            reset_deadline_snapshot(&ledger),
            Some(
                chrono::DateTime::parse_from_rfc3339("2026-10-04T00:00:00Z")
                    .unwrap()
                    .with_timezone(&chrono::Utc)
            ),
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
        assert!(
            reopened
                .prepare_signed_reset_intent(uuid::Uuid::new_v4(), &original_cycle)
                .is_err(),
            "a pending reset cannot be replaced by a new request id"
        );
        assert!(
            reopened
                .prepare_signed_reset_intent(request_id, "different-old-cycle")
                .is_err(),
            "the persisted request id cannot be rebound to another cycle"
        );
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
        ledger
            .connection
            .execute(
                "INSERT INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
             VALUES (?1,42,'2026-10-01T00:00:00Z')",
                [&alice_cycle_before_reset],
            )
            .unwrap();
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

#[cfg(test)]
mod guest_import_v2_provenance_tests {
    use rusqlite::OptionalExtension;

    use super::Ledger;

    fn response(tokens: u64) -> crate::collectors::ParsedRecord {
        response_for("r1", tokens)
    }

    fn response_for(response_id: &str, tokens: u64) -> crate::collectors::ParsedRecord {
        let occurred_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
        let line = format!(
            r#"{{"timestamp":"{occurred_at}","type":"token_usage_record","payload":{{"session_id":"s1","response_id":"{response_id}","usage":{{"total_tokens":{tokens}}}}}}}"#
        );
        crate::collectors::codex::parse_line(&line)
            .unwrap()
            .unwrap()
    }

    fn snapshot(tokens: u64) -> crate::collectors::ParsedRecord {
        crate::collectors::ParsedRecord {
            agent: crate::domain::usage::Agent::Codex,
            kind: crate::collectors::RecordKind::CumulativeSnapshot,
            event_key: "snapshot-1".into(),
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

    fn provenance_schema_present(ledger: &Ledger) -> bool {
        let count: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN
                 ('guest_provenance_lineage','guest_provenance_occurrence_key',
                  'guest_provenance_occurrence_version','guest_provenance_mutation')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        count == 4
    }

    fn lineage_eligible(ledger: &Ledger) -> i64 {
        ledger
            .connection
            .query_row(
                "SELECT eligible FROM guest_provenance_lineage WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn mutation_count(ledger: &Ledger, kind: &str) -> i64 {
        ledger
            .connection
            .query_row(
                "SELECT count(*) FROM guest_provenance_mutation WHERE mutation_kind=?1",
                [kind],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn occurrence(ledger: &Ledger, event_key: &str) -> Option<(String, i64, i64, i64, i64)> {
        if !provenance_schema_present(ledger) {
            return None;
        }
        ledger
            .connection
            .query_row(
                "SELECT k.occurrence_id,k.current_record_version,k.ingest_seq,
                        l.ingest_watermark,l.occurrence_count
                 FROM guest_provenance_occurrence_key k
                 JOIN guest_provenance_lineage l ON l.singleton=1
                 WHERE k.event_key=?1",
                [event_key],
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
            .optional()
            .unwrap()
    }

    #[test]
    fn guest_import_v2_provenance_new_empty_lineage_records_version_one_occurrence() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        ledger.insert(&response(42)).unwrap();

        assert!(
            provenance_schema_present(&ledger),
            "a fresh local usage insertion has durable provenance storage"
        );
        let first =
            occurrence(&ledger, "codex:response:s1:r1").expect("fresh usage has one occurrence");
        assert_eq!(first.1, 1);
        assert_eq!(first.2, 1);
        assert_eq!(first.3, 1);
        assert_eq!(first.4, 1);
        assert_eq!(lineage_eligible(&ledger), 1);
        let (occurred_at, ingested_at): (String, String) = ledger
            .connection
            .query_row(
                "SELECT occurred_at_utc,ingested_at_utc
                 FROM guest_provenance_occurrence_version WHERE occurrence_id=?1",
                [&first.0],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        for timestamp in [&occurred_at, &ingested_at] {
            assert_eq!(timestamp.len(), 27);
            let parsed = chrono::DateTime::parse_from_rfc3339(timestamp).unwrap();
            assert_eq!(
                parsed.to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
                *timestamp
            );
        }
        assert_eq!(
            uuid::Uuid::parse_str(&first.0).unwrap().to_string(),
            first.0
        );

        let reopened = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        assert_eq!(occurrence(&reopened, "codex:response:s1:r1"), Some(first));
    }

    #[test]
    fn guest_import_v2_provenance_duplicate_scan_keeps_uuid_and_watermark() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        let record = response(42);
        ledger.insert(&record).unwrap();
        assert!(
            provenance_schema_present(&ledger),
            "provenance schema is initialized"
        );
        let before = occurrence(&ledger, "codex:response:s1:r1");
        ledger.insert(&record).unwrap();
        let after_duplicate = occurrence(&ledger, "codex:response:s1:r1");

        assert_eq!(before, after_duplicate);
        assert_eq!(after_duplicate.as_ref().unwrap().2, 1);
        assert_eq!(after_duplicate.as_ref().unwrap().3, 1);
    }

    #[test]
    fn guest_import_v2_provenance_changed_key_records_correction_without_double_count() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        ledger.insert(&response(42)).unwrap();
        assert!(
            provenance_schema_present(&ledger),
            "provenance schema is initialized"
        );
        let before =
            occurrence(&ledger, "codex:response:s1:r1").expect("initial source occurrence");
        ledger.insert(&response(7)).unwrap();
        let corrected =
            occurrence(&ledger, "codex:response:s1:r1").expect("corrected source occurrence");

        assert_eq!(corrected.0, before.0);
        assert_eq!(corrected.1, 2);
        assert_eq!(corrected.2, before.2);
        let bucket = chrono::Utc::now().format("%Y-%m-%d").to_string();
        assert_eq!(
            ledger
                .daily_total(crate::domain::usage::Agent::Codex, &bucket)
                .unwrap(),
            Some(7)
        );
        let corrections: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM guest_provenance_mutation
                 WHERE mutation_kind='content_correction' AND event_key='codex:response:s1:r1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(corrections, 1);
    }

    #[test]
    fn guest_import_v2_provenance_legacy_database_never_becomes_new_lineage() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let legacy = rusqlite::Connection::open(file.path()).unwrap();
        legacy
            .execute_batch(
                "CREATE TABLE setting(key TEXT PRIMARY KEY,value TEXT NOT NULL);
                 CREATE TABLE source_checkpoint(source_id TEXT PRIMARY KEY,file_fingerprint TEXT NOT NULL,
                   byte_offset INTEGER NOT NULL,parser_version INTEGER NOT NULL,last_snapshot_total INTEGER,
                   status TEXT NOT NULL DEFAULT 'complete');
                 CREATE TABLE usage_record(event_key TEXT PRIMARY KEY,source_id TEXT NOT NULL,agent TEXT NOT NULL,
                   kind TEXT NOT NULL,bucket_date TEXT NOT NULL,occurred_at_utc TEXT NOT NULL,input_tokens INTEGER,
                   output_tokens INTEGER,cache_read_tokens INTEGER,cache_write_tokens INTEGER,total_tokens INTEGER,
                   coverage TEXT NOT NULL,parser_version INTEGER NOT NULL);
                 CREATE TABLE planet_object(cycle_id TEXT NOT NULL,stage INTEGER NOT NULL,ordinal INTEGER NOT NULL,
                   kind TEXT NOT NULL,x INTEGER NOT NULL,y INTEGER NOT NULL,seed TEXT NOT NULL,
                   PRIMARY KEY(cycle_id,stage,ordinal));
                 CREATE TABLE planet_wallet_credit(previous_cycle_id TEXT PRIMARY KEY,amount INTEGER NOT NULL,
                   created_at_utc TEXT NOT NULL);
                 INSERT INTO setting(key,value) VALUES
                   ('world_timezone','UTC'),
                   ('planet_account_id','local'),
                   ('planet_device_id','11111111-1111-4111-8111-111111111111'),
                   ('planet_activation_at_utc','2026-09-01T00:00:00.000000Z'),
                   ('planet_current_cycle_id','22222222-2222-4222-8222-222222222222');
                 INSERT INTO source_checkpoint(source_id,file_fingerprint,byte_offset,parser_version,
                   last_snapshot_total,status)
                 VALUES ('codex:/legacy/session.jsonl','legacy-fingerprint',128,1,NULL,'complete');
                 INSERT INTO usage_record(event_key,source_id,agent,kind,bucket_date,occurred_at_utc,
                   input_tokens,output_tokens,cache_read_tokens,cache_write_tokens,total_tokens,coverage,parser_version)
                 VALUES ('codex:response:legacy:old','codex:/legacy/session.jsonl','codex','response',
                   '2026-09-24','2026-09-24T12:00:00.000000Z',NULL,NULL,NULL,NULL,5,'complete',1);
                 INSERT INTO planet_object(cycle_id,stage,ordinal,kind,x,y,seed)
                 VALUES ('22222222-2222-4222-8222-222222222222',1,0,'tree',3,4,'legacy-seed');
                 INSERT INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
                 VALUES ('33333333-3333-4333-8333-333333333333',17,'2026-09-01T00:00:00.000000Z');",
            )
            .unwrap();
        drop(legacy);

        let mut ledger = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        ledger.insert(&response(42)).unwrap();
        let lineage_count: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='guest_provenance_lineage'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            lineage_count, 1,
            "legacy stores receive additive provenance schema"
        );
        let lineage_rows: i64 = ledger
            .connection
            .query_row("SELECT count(*) FROM guest_provenance_lineage", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(
            lineage_rows, 0,
            "legacy data is not promoted by schema upgrade"
        );
        let mappings: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM guest_provenance_occurrence_key",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            mappings, 0,
            "new scans in a legacy database gain no synthetic provenance"
        );
        drop(ledger);

        let reopened = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        let lineage_rows: i64 = reopened
            .connection
            .query_row("SELECT count(*) FROM guest_provenance_lineage", [], |row| {
                row.get(0)
            })
            .unwrap();
        let mappings: i64 = reopened
            .connection
            .query_row(
                "SELECT count(*) FROM guest_provenance_occurrence_key",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(lineage_rows, 0, "reopening legacy data never seeds lineage");
        assert_eq!(mappings, 0, "reopening legacy data never maps old keys");
        let preserved_source: (String, i64, i64) = reopened
            .connection
            .query_row(
                "SELECT file_fingerprint,byte_offset,parser_version FROM source_checkpoint
                 WHERE source_id='codex:/legacy/session.jsonl'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(preserved_source, ("legacy-fingerprint".into(), 128, 1));
        let preserved_raw: (String, i64) = reopened
            .connection
            .query_row(
                "SELECT source_id,total_tokens FROM usage_record
                 WHERE event_key='codex:response:legacy:old'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(preserved_raw, ("codex:/legacy/session.jsonl".into(), 5));
        let preserved_game: (i64, i64) = reopened
            .connection
            .query_row(
                "SELECT (SELECT count(*) FROM planet_object),
                        (SELECT amount FROM planet_wallet_credit
                         WHERE previous_cycle_id='33333333-3333-4333-8333-333333333333')",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(preserved_game, (1, 17));
    }

    #[test]
    fn guest_import_v2_provenance_recursive_triggers_keep_duplicate_and_correction_single() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        ledger
            .connection
            .execute_batch("PRAGMA recursive_triggers=ON;")
            .unwrap();
        let record = response(42);
        ledger.insert(&record).unwrap();
        let first = occurrence(&ledger, "codex:response:s1:r1");

        ledger.insert(&record).unwrap();

        assert_eq!(occurrence(&ledger, "codex:response:s1:r1"), first);
        let after_duplicate: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM guest_provenance_mutation",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            after_duplicate, 1,
            "recursive triggers keep duplicate a no-op"
        );

        ledger.insert(&response(17)).unwrap();

        assert_eq!(
            occurrence(&ledger, "codex:response:s1:r1")
                .as_ref()
                .map(|value| value.1),
            Some(2)
        );
        assert_eq!(mutation_count(&ledger, "content_correction"), 1);
        let after_correction: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM guest_provenance_mutation",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            after_correction, 2,
            "recursive triggers record one correction"
        );
    }

    #[test]
    fn guest_import_v2_provenance_source_replacement_delete_records_correction() {
        use crate::collectors::discovery::{scan_sources, SourceConfig};
        use std::fs;

        let temp = tempfile::tempdir().unwrap();
        let codex = temp.path().join("codex");
        fs::create_dir(&codex).unwrap();
        let source = codex.join("session.jsonl");
        let line = |tokens: &str| {
            let occurred_at =
                chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
            serde_json::json!({
                "timestamp": occurred_at,
                "type": "token_usage_record",
                "payload": {
                    "session_id": "s1",
                    "response_id": "r1",
                    "usage": {"total_tokens": tokens.parse::<u64>().unwrap()}
                }
            })
            .to_string()
                + "\n"
        };
        fs::write(&source, line("42")).unwrap();
        let mut ledger = Ledger::open(&temp.path().join("ledger.db"), chrono_tz::UTC).unwrap();
        let config = SourceConfig {
            codex_root: codex,
            claude_root: temp.path().join("missing"),
            timezone: chrono_tz::UTC,
        };
        scan_sources(&config, &mut ledger).unwrap();
        assert!(
            provenance_schema_present(&ledger),
            "fresh source has provenance storage"
        );

        fs::write(&source, line("19")).unwrap();
        scan_sources(&config, &mut ledger).unwrap();

        let version: i64 = ledger
            .connection
            .query_row(
                "SELECT current_record_version FROM guest_provenance_occurrence_key
                 WHERE event_key='codex:response:s1:r1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let deletes: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM guest_provenance_mutation
                 WHERE mutation_kind='delete' AND event_key='codex:response:s1:r1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, 2);
        assert_eq!(deletes, 1);
        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
        assert_eq!(
            ledger
                .daily_total(crate::domain::usage::Agent::Codex, &date)
                .unwrap(),
            Some(19)
        );

        fs::write(&source, "").unwrap();
        scan_sources(&config, &mut ledger).unwrap();
        let after_delete: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM guest_provenance_mutation
                 WHERE mutation_kind='delete' AND event_key='codex:response:s1:r1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            after_delete, 2,
            "a delete-only replacement keeps correction evidence"
        );
        assert_eq!(
            ledger
                .daily_total(crate::domain::usage::Agent::Codex, &date)
                .unwrap(),
            None
        );
    }

    #[test]
    fn guest_import_v2_provenance_failed_source_replacement_rolls_back_delete_and_raw() {
        use crate::collectors::discovery::{scan_sources, SourceConfig};
        use std::fs;

        let temp = tempfile::tempdir().unwrap();
        let codex = temp.path().join("codex");
        fs::create_dir(&codex).unwrap();
        let source = codex.join("session.jsonl");
        let line = |tokens: &str| {
            let occurred_at =
                chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
            serde_json::json!({
                "timestamp": occurred_at,
                "type": "token_usage_record",
                "payload": {
                    "session_id": "s1",
                    "response_id": "r1",
                    "usage": {"total_tokens": tokens.parse::<u64>().unwrap()}
                }
            })
            .to_string()
                + "\n"
        };
        fs::write(&source, line("42")).unwrap();
        let mut ledger = Ledger::open(&temp.path().join("ledger.db"), chrono_tz::UTC).unwrap();
        let config = SourceConfig {
            codex_root: codex,
            claude_root: temp.path().join("missing"),
            timezone: chrono_tz::UTC,
        };
        scan_sources(&config, &mut ledger).unwrap();
        assert!(
            provenance_schema_present(&ledger),
            "fresh source has provenance storage"
        );
        let mutation_count: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM guest_provenance_mutation",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let original = occurrence(&ledger, "codex:response:s1:r1").unwrap();

        fs::write(&source, line("9223372036854775808")).unwrap();
        let error = scan_sources(&config, &mut ledger).unwrap_err();
        assert_eq!(error, super::ScanError::InvalidCount);

        let after_mutations: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM guest_provenance_mutation",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            after_mutations, mutation_count,
            "savepoint rollback removes its delete evidence"
        );
        assert_eq!(occurrence(&ledger, "codex:response:s1:r1"), Some(original));
        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
        assert_eq!(
            ledger
                .daily_total(crate::domain::usage::Agent::Codex, &date)
                .unwrap(),
            Some(42)
        );
    }

    #[test]
    fn guest_import_v2_provenance_clock_regression_holds_but_keeps_raw() {
        use crate::storage::guest_provenance::record_guest_occurrence_in_transaction;

        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        let record = response(42);
        ledger.insert(&record).unwrap();
        let last_ingested: String = ledger
            .connection
            .query_row(
                "SELECT last_ingested_at_utc FROM guest_provenance_lineage WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let regressed_time = chrono::DateTime::parse_from_rfc3339(&last_ingested)
            .unwrap()
            .with_timezone(&chrono::Utc)
            - chrono::Duration::seconds(1);
        let transaction = ledger.connection.transaction().unwrap();
        record_guest_occurrence_in_transaction(&transaction, &record, regressed_time).unwrap();
        transaction.commit().unwrap();

        assert_eq!(lineage_eligible(&ledger), 0);
        assert_eq!(mutation_count(&ledger, "clock_regression"), 1);
        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
        assert_eq!(
            ledger
                .daily_total(crate::domain::usage::Agent::Codex, &date)
                .unwrap(),
            Some(42)
        );
    }

    #[test]
    fn guest_import_v2_provenance_pre_activation_occurrence_holds_but_keeps_raw() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        let mut record = response(12);
        let activation: String = ledger
            .connection
            .query_row(
                "SELECT activated_at_utc FROM guest_provenance_lineage WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        record.occurred_at_utc = chrono::DateTime::parse_from_rfc3339(&activation)
            .unwrap()
            .with_timezone(&chrono::Utc)
            - chrono::Duration::seconds(1);
        ledger.insert(&record).unwrap();

        assert_eq!(lineage_eligible(&ledger), 0);
        assert_eq!(mutation_count(&ledger, "before_activation"), 1);
        let date = record.occurred_at_utc.format("%Y-%m-%d").to_string();
        assert_eq!(
            ledger
                .daily_total(crate::domain::usage::Agent::Codex, &date)
                .unwrap(),
            Some(12)
        );
        let stored_total: i64 = ledger
            .connection
            .query_row(
                "SELECT total_tokens FROM usage_record WHERE event_key=?1",
                [&record.event_key],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            stored_total, 12,
            "provenance holds do not discard raw input"
        );
    }

    #[test]
    fn guest_import_v2_provenance_device_change_holds_but_keeps_raw() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        ledger.insert(&response(42)).unwrap();
        let replacement_device = uuid::Uuid::new_v4().to_string();
        ledger
            .connection
            .execute(
                "UPDATE setting SET value=?1 WHERE key='planet_device_id'",
                [&replacement_device],
            )
            .unwrap();
        ledger.insert(&response_for("r2", 11)).unwrap();

        assert_eq!(lineage_eligible(&ledger), 0);
        assert_eq!(mutation_count(&ledger, "device_mismatch"), 1);
        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
        assert_eq!(
            ledger
                .daily_total(crate::domain::usage::Agent::Codex, &date)
                .unwrap(),
            Some(53)
        );
    }

    #[test]
    fn guest_import_v2_provenance_response_replaces_snapshot_without_double_count() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        ledger.insert(&snapshot(7)).unwrap();
        ledger.insert(&response(42)).unwrap();

        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
        assert_eq!(
            ledger
                .daily_total(crate::domain::usage::Agent::Codex, &date)
                .unwrap(),
            Some(42)
        );
        let snapshot_effective: i64 = ledger
            .connection
            .query_row(
                "SELECT effective FROM guest_provenance_occurrence_key
                 WHERE event_key=':snapshot-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(snapshot_effective, 0);
        assert_eq!(lineage_eligible(&ledger), 0);
        assert_eq!(mutation_count(&ledger, "effective_record_replaced"), 1);
    }

    #[test]
    fn guest_import_v2_provenance_sequence_overflow_holds_without_dropping_raw() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        ledger.insert(&response(42)).unwrap();
        ledger
            .connection
            .execute(
                "UPDATE guest_provenance_lineage SET ingest_watermark=9223372036854775807",
                [],
            )
            .unwrap();
        ledger.insert(&response_for("r2", 9)).unwrap();

        assert_eq!(lineage_eligible(&ledger), 0);
        assert_eq!(mutation_count(&ledger, "ingest_sequence_overflow"), 1);
        assert!(occurrence(&ledger, "codex:response:s1:r2").is_none());
        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
        assert_eq!(
            ledger
                .daily_total(crate::domain::usage::Agent::Codex, &date)
                .unwrap(),
            Some(51)
        );
    }

    #[test]
    fn guest_import_v2_provenance_unique_occurrence_uuid_and_sequence_are_enforced() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        ledger.insert(&response(42)).unwrap();
        let first = occurrence(&ledger, "codex:response:s1:r1").unwrap();
        let duplicate_uuid = ledger.connection.execute(
            "INSERT INTO guest_provenance_occurrence_key(
                event_key,occurrence_id,ingest_seq,current_record_version,content_fingerprint,effective
             ) VALUES ('duplicate-id',?1,2,1,'digest',1)",
            [&first.0],
        );
        assert!(duplicate_uuid.is_err());
        let duplicate_sequence = ledger.connection.execute(
            "INSERT INTO guest_provenance_occurrence_key(
                event_key,occurrence_id,ingest_seq,current_record_version,content_fingerprint,effective
             ) VALUES ('duplicate-seq',?1,1,1,'digest',1)",
            [uuid::Uuid::new_v4().to_string()],
        );
        assert!(duplicate_sequence.is_err());
        assert_eq!(occurrence(&ledger, "codex:response:s1:r1"), Some(first));
    }

    #[test]
    fn guest_import_v2_provenance_token_overflow_does_not_commit_raw_or_provenance() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        let error = ledger.insert(&response(u64::MAX)).unwrap_err();

        assert_eq!(error, super::ScanError::InvalidCount);
        let raw_count: i64 = ledger
            .connection
            .query_row("SELECT count(*) FROM usage_record", [], |row| row.get(0))
            .unwrap();
        assert_eq!(raw_count, 0);
        assert_eq!(occurrence(&ledger, "codex:response:s1:r1"), None);
        assert_eq!(lineage_eligible(&ledger), 1);
    }

    #[test]
    fn guest_import_v2_provenance_record_version_overflow_holds_and_keeps_raw() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        ledger.insert(&response(42)).unwrap();
        ledger
            .connection
            .execute(
                "UPDATE guest_provenance_occurrence_key
                 SET current_record_version=9223372036854775807
                 WHERE event_key='codex:response:s1:r1'",
                [],
            )
            .unwrap();

        ledger.insert(&response(17)).unwrap();

        assert_eq!(lineage_eligible(&ledger), 0);
        assert_eq!(mutation_count(&ledger, "record_version_overflow"), 1);
        let current_version: i64 = ledger
            .connection
            .query_row(
                "SELECT current_record_version FROM guest_provenance_occurrence_key
                 WHERE event_key='codex:response:s1:r1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(current_version, i64::MAX);
        let raw_total: i64 = ledger
            .connection
            .query_row(
                "SELECT total_tokens FROM usage_record WHERE event_key='codex:response:s1:r1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(raw_total, 17);
        let immutable_versions: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM guest_provenance_occurrence_version
                 WHERE occurrence_id=(SELECT occurrence_id FROM guest_provenance_occurrence_key
                                      WHERE event_key='codex:response:s1:r1')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(immutable_versions, 1);
    }

    #[test]
    fn guest_import_v2_provenance_mutation_sequence_overflow_holds_and_keeps_raw() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        ledger.insert(&response(42)).unwrap();
        ledger
            .connection
            .execute(
                "UPDATE guest_provenance_lineage SET last_mutation_seq=9223372036854775807",
                [],
            )
            .unwrap();

        ledger.insert(&response_for("r2", 9)).unwrap();

        assert_eq!(lineage_eligible(&ledger), 0);
        assert!(occurrence(&ledger, "codex:response:s1:r2").is_none());
        let (watermark, occurrence_count, last_mutation_seq): (i64, i64, i64) = ledger
            .connection
            .query_row(
                "SELECT ingest_watermark,occurrence_count,last_mutation_seq
                 FROM guest_provenance_lineage WHERE singleton=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            (watermark, occurrence_count, last_mutation_seq),
            (1, 1, i64::MAX)
        );
        let raw_count: i64 = ledger
            .connection
            .query_row("SELECT count(*) FROM usage_record", [], |row| row.get(0))
            .unwrap();
        assert_eq!(raw_count, 2);
        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
        assert_eq!(
            ledger
                .daily_total(crate::domain::usage::Agent::Codex, &date)
                .unwrap(),
            Some(51)
        );
    }

    #[test]
    fn guest_import_v2_provenance_future_occurrence_holds_and_keeps_raw() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        let mut record = response(23);
        record.occurred_at_utc = chrono::Utc::now() + chrono::Duration::minutes(5);
        ledger.insert(&record).unwrap();

        assert_eq!(lineage_eligible(&ledger), 0);
        assert_eq!(mutation_count(&ledger, "occurrence_after_ingestion"), 1);
        assert!(occurrence(&ledger, &record.event_key).is_some());
        let stored_total: i64 = ledger
            .connection
            .query_row(
                "SELECT total_tokens FROM usage_record WHERE event_key=?1",
                [&record.event_key],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored_total, 23);
    }
}

#[cfg(test)]
mod guest_import_v2_reset_tests {
    use super::Ledger;

    fn raw_one_date_record(
        occurred_at: chrono::DateTime<chrono::Utc>,
    ) -> crate::collectors::ParsedRecord {
        raw_record("guest-reset-one-date-raw-1m", occurred_at)
    }

    fn raw_record(
        event_key: &str,
        occurred_at: chrono::DateTime<chrono::Utc>,
    ) -> crate::collectors::ParsedRecord {
        raw_record_with_tokens(event_key, occurred_at, 1_000_000)
    }

    fn raw_record_with_tokens(
        event_key: &str,
        occurred_at: chrono::DateTime<chrono::Utc>,
        tokens: u64,
    ) -> crate::collectors::ParsedRecord {
        crate::collectors::ParsedRecord {
            agent: crate::domain::usage::Agent::Codex,
            kind: crate::collectors::RecordKind::Response,
            event_key: event_key.into(),
            occurred_at_utc: occurred_at,
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
    fn zero_current_cycle_usage_omits_daily_entry_and_positive_usage_remains_aggregated() {
        let mut ledger = Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let activation = ledger.planet_activation_at().unwrap();
        let date = (activation + chrono::Duration::microseconds(1))
            .format("%Y-%m-%d")
            .to_string();
        ledger
            .insert(&raw_record_with_tokens(
                "zero-current-cycle-usage",
                activation + chrono::Duration::microseconds(1),
                0,
            ))
            .unwrap();

        let zero = ledger.planet_device_contribution(false).unwrap();
        assert_eq!(zero.current_planet_tokens, 0);
        assert!(zero.daily_tokens.is_empty());

        ledger
            .insert(&raw_record_with_tokens(
                "positive-current-cycle-usage",
                activation + chrono::Duration::microseconds(2),
                37,
            ))
            .unwrap();
        let positive = ledger.planet_device_contribution(false).unwrap();
        assert_eq!(positive.current_planet_tokens, 37);
        assert_eq!(positive.daily_tokens.len(), 1);
        assert_eq!(positive.daily_tokens.get(&date), Some(&37));
    }

    fn table_exists(ledger: &Ledger, table: &str) -> bool {
        ledger
            .connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                [table],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn table_count(ledger: &Ledger, table: &str) -> i64 {
        if !table_exists(ledger, table) {
            return 0;
        }
        ledger
            .connection
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap()
    }

    fn table_dump(ledger: &Ledger, table: &str) -> Vec<String> {
        if !table_exists(ledger, table) {
            return Vec::new();
        }
        let mut statement = ledger
            .connection
            .prepare(&format!("SELECT * FROM {table}"))
            .unwrap();
        let column_count = statement.column_count();
        let mut rows = statement
            .query_map([], |row| {
                (0..column_count)
                    .map(|index| row.get::<_, rusqlite::types::Value>(index))
                    .collect::<Result<Vec<_>, _>>()
                    .map(|values| format!("{values:?}"))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        rows.sort();
        rows
    }

    fn reset_state_dump(ledger: &Ledger) -> Vec<(String, Vec<String>)> {
        [
            "setting",
            "usage_record",
            "planet_usage_owner",
            "daily_agent_total",
            "guest_provenance_lineage",
            "guest_provenance_occurrence_key",
            "guest_provenance_occurrence_version",
            "guest_provenance_mutation",
            "guest_provenance_cycle",
            "guest_provenance_reset_receipt",
            "planet_wallet_credit",
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
            "growth_journal_state",
            "growth_journal_cycle",
            "growth_journal_entry",
            "growth_journal_remote_cycle",
            "growth_journal_remote_entry",
            "shop_action_request",
            "shop_landscape_instance",
            "shop_landscape_placement",
            "shop_avatar_owned",
            "shop_avatar_equipment",
            "shop_purchase",
            "cosmetic_purchase",
            "cosmetic_equipment",
            "shop_natural_removal",
            "shop_natural_removal_debit",
            "planet_object",
        ]
        .into_iter()
        .map(|table| (table.to_owned(), table_dump(ledger, table)))
        .collect()
    }

    #[test]
    fn reset_uuid_links_actual_request_and_credit() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        let occurred_at = chrono::Utc::now();
        let record = raw_one_date_record(occurred_at);
        ledger.insert(&record).unwrap();
        let raw_tokens = ledger
            .reset_planet(chrono::Utc::now() + chrono::Duration::hours(1))
            .unwrap();

        assert_eq!(raw_tokens, 1_000_000);
        let (request_id, result_json): (String, String) = ledger
            .connection
            .query_row(
                "SELECT request_id,result_json FROM shop_action_request
                 WHERE account_id='local' ORDER BY created_at_utc DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!(
            uuid::Uuid::parse_str(&request_id).is_ok(),
            "new eligible guest reset must persist a UUID request before applying reset"
        );
        let action: crate::domain::cosmetic_shop::ShopActionResult =
            serde_json::from_str(&result_json).unwrap();
        assert_eq!(action.request_id, request_id);
        let (receipt_request, receipt_json): (String, String) = ledger
            .connection
            .query_row(
                "SELECT request_id,receipt_json FROM guest_provenance_reset_receipt",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(receipt_request, request_id);
        let receipt: crate::domain::guest_shop_import::GuestFirstResetReceipt =
            serde_json::from_str(&receipt_json).unwrap();
        assert_eq!(receipt.request.request_id, request_id);
        assert_eq!(receipt.result.request_id, request_id);
        assert_eq!(receipt.result.raw_tokens, 1_000_000);
        assert_eq!(receipt.result.credited_tokens, 1_000_000);
        let wallet_credit: (String, i64) = ledger
            .connection
            .query_row(
                "SELECT previous_cycle_id,amount FROM planet_wallet_credit",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(wallet_credit, (receipt.request.cycle_id, 1_000_000));
    }

    #[test]
    fn ordinary_scan_settle_reset_records_zero_baselines() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        let occurred_at = chrono::Utc::now();
        ledger.insert(&raw_one_date_record(occurred_at)).unwrap();
        ledger.rebuild_shop_contributions().unwrap();
        ledger
            .settle_guest_rewards(occurred_at + chrono::Duration::minutes(1))
            .unwrap();
        let previous_cycle_id = ledger.planet_cycle_id().unwrap();
        ledger
            .settle_guest_cycle_tokens(
                &previous_cycle_id,
                occurred_at + chrono::Duration::minutes(2),
            )
            .unwrap();
        ledger.prepare_growth_journal().unwrap();
        let reset_at = chrono::DateTime::from_timestamp_micros(
            (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp_micros(),
        )
        .unwrap();
        assert_eq!(ledger.reset_planet(reset_at).unwrap(), 1_000_000);
        let reset_at_rfc3339 = reset_at.to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
        ledger.prepare_growth_journal().unwrap();
        let journal_after_reset = serde_json::to_value(ledger.growth_journal().unwrap()).unwrap();
        let (old_end, old_credit, old_credit_at): (Option<String>, Option<i64>, Option<String>) =
            ledger
                .connection
                .query_row(
                    "SELECT ended_at_utc,wallet_credit,wallet_credit_at_utc
                     FROM growth_journal_cycle WHERE account_id='local' AND cycle_id=?1",
                    [&previous_cycle_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap();
        assert_eq!(old_end.as_deref(), Some(reset_at_rfc3339.as_str()));
        assert_eq!(old_credit, Some(1_000_000));
        assert_eq!(old_credit_at.as_deref(), Some(reset_at_rfc3339.as_str()));
        ledger.prepare_growth_journal().unwrap();
        assert_eq!(
            serde_json::to_value(ledger.growth_journal().unwrap()).unwrap(),
            journal_after_reset
        );

        assert!(
            table_exists(&ledger, "guest_provenance_cycle"),
            "fresh guest lineage must persist cycle bounds and revision-zero baselines"
        );
        let old_bound: (String, Option<String>, i64, String, String) = ledger
            .connection
            .query_row(
                "SELECT started_at_utc,ended_at_utc,baseline_revision,
                        active_instance_ids_json,effects_json
                 FROM guest_provenance_cycle WHERE cycle_id=?1",
                [&previous_cycle_id],
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
        assert_eq!(old_bound.1.as_deref(), Some(reset_at_rfc3339.as_str()));
        assert_eq!(old_bound.2, 0);
        assert_eq!(old_bound.3, "[]");
        assert_eq!(
            old_bound.4,
            serde_json::to_string(&crate::domain::cosmetic_shop::ActiveEffects::default()).unwrap()
        );

        let current_cycle_id = ledger.planet_cycle_id().unwrap();
        let new_bound: (String, Option<String>, i64, String, String) = ledger
            .connection
            .query_row(
                "SELECT started_at_utc,ended_at_utc,baseline_revision,
                        active_instance_ids_json,effects_json
                 FROM guest_provenance_cycle WHERE cycle_id=?1",
                [&current_cycle_id],
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
        assert_eq!(new_bound.0, reset_at_rfc3339);
        assert_eq!(new_bound.1, None);
        assert_eq!(new_bound.2, 0);
        assert_eq!(new_bound.3, "[]");
        assert_eq!(new_bound.4, old_bound.4);
        assert!(table_exists(&ledger, "guest_provenance_reset_receipt"));
        let positive_history: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM shop_effect_history WHERE account_id='local'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(positive_history, 0);
        ledger.rebuild_shop_contributions().unwrap();
        let current_cycle_id = ledger.planet_cycle_id().unwrap();
        let new_effect_revision: i64 = ledger
            .connection
            .query_row(
                "SELECT min(effect_revision) FROM shop_effect_contribution
                 WHERE account_id='local' AND cycle_id=?1",
                [&previous_cycle_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(new_effect_revision, 0);
        let native = ledger.shop_device_contribution(false).unwrap();
        assert_eq!(native.raw.current_planet_tokens, 0);
        assert_eq!(native.raw.lifetime_tokens, 1_000_000);
        assert!(native.daily_segments.iter().any(|segment| {
            segment.cycle_id == previous_cycle_id
                && segment.effect_revision == 0
                && segment.tokens == 1_000_000
        }));
        assert!(!native
            .daily_segments
            .iter()
            .any(|segment| segment.cycle_id == current_cycle_id));
        let frozen_deadline: String = ledger
            .connection
            .query_row(
                "SELECT value FROM setting WHERE key='planet_reset_available_at_utc'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        drop(ledger);
        let reopened = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        assert_eq!(table_count(&reopened, "guest_provenance_reset_receipt"), 1);
        assert_eq!(table_count(&reopened, "guest_provenance_cycle"), 2);
        assert_eq!(
            reopened
                .connection
                .query_row(
                    "SELECT value FROM setting WHERE key='planet_reset_available_at_utc'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            frozen_deadline
        );
    }

    #[test]
    fn reset_boundary_and_late_old_occurrences_keep_timestamp_cycle() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        let activation = ledger.planet_activation_at().unwrap();
        ledger
            .insert(&raw_record(
                "guest-reset-boundary-credit",
                activation + chrono::Duration::microseconds(1),
            ))
            .unwrap();
        let old_cycle = ledger.planet_cycle_id().unwrap();
        let reset_at =
            chrono::DateTime::from_timestamp_micros(chrono::Utc::now().timestamp_micros()).unwrap();
        assert_eq!(ledger.reset_planet(reset_at).unwrap(), 1_000_000);
        let new_cycle = ledger.planet_cycle_id().unwrap();

        ledger
            .insert(&raw_record(
                "guest-reset-late-old-occurrence",
                reset_at - chrono::Duration::microseconds(1),
            ))
            .unwrap();
        ledger
            .insert(&raw_record("guest-reset-at-boundary", reset_at))
            .unwrap();

        let assigned: Vec<(String, String)> = {
            let mut statement = ledger
                .connection
                .prepare(
                    "SELECT k.event_key,v.cycle_id
                 FROM guest_provenance_occurrence_key k
                 JOIN guest_provenance_occurrence_version v USING(occurrence_id)
                 WHERE k.event_key IN ('guest-reset-late-old-occurrence','guest-reset-at-boundary')
                 ORDER BY k.event_key",
                )
                .unwrap();
            statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert_eq!(
            assigned,
            vec![
                ("guest-reset-at-boundary".to_owned(), new_cycle.clone()),
                (
                    "guest-reset-late-old-occurrence".to_owned(),
                    old_cycle.clone(),
                ),
            ]
        );
        ledger.rebuild_shop_contributions().unwrap();
        let native = ledger.shop_device_contribution(false).unwrap();
        assert_eq!(native.raw.current_planet_tokens, 1_000_000);
        assert_eq!(native.raw.lifetime_tokens, 3_000_000);
        assert!(native.daily_segments.iter().any(|segment| {
            segment.cycle_id == old_cycle
                && segment.effect_revision == 0
                && segment.tokens == 2_000_000
        }));
        assert!(native.daily_segments.iter().any(|segment| {
            segment.cycle_id == new_cycle
                && segment.effect_revision == 0
                && segment.tokens == 1_000_000
        }));
    }

    #[test]
    fn activation_boundary_uses_only_the_matching_fresh_guest_baseline() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        let activation = ledger.planet_activation_at().unwrap();
        let current_cycle = ledger.planet_cycle_id().unwrap();
        ledger
            .insert(&raw_record("guest-activation-boundary", activation))
            .unwrap();
        let (assigned_cycle, occurred_at): (String, String) = ledger
            .connection
            .query_row(
                "SELECT cycle_id,occurred_at_utc FROM guest_provenance_occurrence_version
                 WHERE occurrence_id=(SELECT occurrence_id FROM guest_provenance_occurrence_key
                                      WHERE event_key='guest-activation-boundary')",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(assigned_cycle, current_cycle);
        assert_eq!(
            chrono::DateTime::parse_from_rfc3339(&occurred_at)
                .unwrap()
                .timestamp_micros(),
            activation.timestamp_micros()
        );
        assert_eq!(ledger.planet_usage_totals().unwrap().1, 1_000_000);
        ledger.rebuild_shop_contributions().unwrap();
        let native = ledger.shop_device_contribution(false).unwrap();
        assert_eq!(native.raw.current_planet_tokens, 1_000_000);
        assert!(native.daily_segments.iter().any(|segment| {
            segment.cycle_id == current_cycle
                && segment.effect_revision == 0
                && segment.tokens == 1_000_000
        }));
    }

    #[test]
    fn reset_clock_regression_keeps_ordinary_history_without_first_reset_receipt() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        let cycle = ledger.planet_cycle_id().unwrap();
        let observed_now =
            chrono::DateTime::from_timestamp_micros(chrono::Utc::now().timestamp_micros()).unwrap();
        let activation = observed_now - chrono::Duration::seconds(3);
        let reset_at = observed_now - chrono::Duration::seconds(2);
        let occurred_at = observed_now - chrono::Duration::seconds(1);
        let activation_text = activation.to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
        ledger
            .connection
            .execute(
                "UPDATE setting SET value=?1
                 WHERE key IN ('planet_activation_at_utc','planet_cycle_started_at_utc')",
                [&activation_text],
            )
            .unwrap();
        ledger
            .connection
            .execute(
                "UPDATE guest_provenance_lineage SET activated_at_utc=?1 WHERE singleton=1",
                [&activation_text],
            )
            .unwrap();
        ledger
            .connection
            .execute(
                "UPDATE guest_provenance_cycle SET started_at_utc=?1 WHERE cycle_id=?2",
                rusqlite::params![activation_text, cycle],
            )
            .unwrap();
        ledger.insert(&raw_one_date_record(occurred_at)).unwrap();

        let (last_ingested, eligible): (String, i64) = ledger
            .connection
            .query_row(
                "SELECT last_ingested_at_utc,eligible FROM guest_provenance_lineage
                 WHERE singleton=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let reset_text = reset_at.to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
        assert_eq!(eligible, 1);
        assert!(occurred_at > reset_at);
        assert!(last_ingested > reset_text);

        assert_eq!(ledger.reset_planet(reset_at).unwrap(), 1_000_000);

        assert_eq!(table_count(&ledger, "guest_provenance_reset_receipt"), 0);
        assert_eq!(table_count(&ledger, "shop_effect_history"), 1);
        assert_eq!(table_count(&ledger, "planet_wallet_credit"), 1);
        assert_eq!(
            ledger
                .connection
                .query_row(
                    "SELECT eligible FROM guest_provenance_lineage WHERE singleton=1",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn reset_boundary_occurrence_keeps_ordinary_history_without_first_reset_receipt() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        let cycle = ledger.planet_cycle_id().unwrap();
        let observed_now =
            chrono::DateTime::from_timestamp_micros(chrono::Utc::now().timestamp_micros()).unwrap();
        let activation = observed_now - chrono::Duration::seconds(3);
        let reset_at = observed_now - chrono::Duration::seconds(1);
        let activation_text = activation.to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
        let reset_text = reset_at.to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
        ledger
            .connection
            .execute(
                "UPDATE setting SET value=?1
                 WHERE key IN ('planet_activation_at_utc','planet_cycle_started_at_utc')",
                [&activation_text],
            )
            .unwrap();
        ledger
            .connection
            .execute(
                "UPDATE guest_provenance_lineage SET activated_at_utc=?1 WHERE singleton=1",
                [&activation_text],
            )
            .unwrap();
        ledger
            .connection
            .execute(
                "UPDATE guest_provenance_cycle SET started_at_utc=?1 WHERE cycle_id=?2",
                rusqlite::params![activation_text, cycle],
            )
            .unwrap();
        ledger.insert(&raw_one_date_record(reset_at)).unwrap();
        // Isolate the occurrence-boundary check from the separate ingestion watermark check.
        ledger
            .connection
            .execute(
                "UPDATE guest_provenance_lineage SET last_ingested_at_utc=?1 WHERE singleton=1",
                [&reset_text],
            )
            .unwrap();

        assert_eq!(ledger.reset_planet(reset_at).unwrap(), 1_000_000);

        assert_eq!(table_count(&ledger, "guest_provenance_reset_receipt"), 0);
        assert_eq!(table_count(&ledger, "shop_effect_history"), 1);
        assert_eq!(table_count(&ledger, "planet_wallet_credit"), 1);
        assert_eq!(
            ledger
                .connection
                .query_row(
                    "SELECT eligible FROM guest_provenance_lineage WHERE singleton=1",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn signed_account_does_not_use_local_guest_cycle_bounds() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        let activation = ledger.planet_activation_at().unwrap();
        ledger
            .insert(&raw_record(
                "guest-to-signed-old-record",
                activation + chrono::Duration::microseconds(1),
            ))
            .unwrap();
        let old_cycle = ledger.planet_cycle_id().unwrap();
        let reset_at =
            chrono::DateTime::from_timestamp_micros(chrono::Utc::now().timestamp_micros()).unwrap();
        assert_eq!(ledger.reset_planet(reset_at).unwrap(), 1_000_000);
        ledger
            .insert(&raw_record(
                "guest-to-signed-late-old",
                reset_at - chrono::Duration::microseconds(1),
            ))
            .unwrap();
        ledger
            .connection
            .execute(
                "UPDATE planet_usage_owner SET account_id='account:signed-fixture'",
                [],
            )
            .unwrap();
        ledger
            .connection
            .execute(
                "UPDATE setting SET value='account:signed-fixture'
                 WHERE key='planet_account_id'",
                [],
            )
            .unwrap();
        ledger.rebuild_shop_contributions().unwrap();
        let signed_guest_cycle_rows: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM shop_effect_contribution
                 WHERE account_id='account:signed-fixture' AND cycle_id=?1",
                [&old_cycle],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(signed_guest_cycle_rows, 0);
        let baseline_rows: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM shop_effect_contribution
                 WHERE account_id='account:signed-fixture' AND cycle_id='baseline'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        // Without a confirmed server timeline, signed-account rebuild is a no-op.
        // Direct canonical occurrence selection is covered in shop_effects tests.
        assert_eq!(baseline_rows, 0);
    }

    #[test]
    fn missing_task_two_baseline_holds_without_manufacturing_history_or_blocking_reset() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        let occurred_at = chrono::Utc::now();
        ledger.insert(&raw_one_date_record(occurred_at)).unwrap();
        let old_cycle = ledger.planet_cycle_id().unwrap();
        ledger
            .connection
            .execute(
                "DELETE FROM guest_provenance_cycle WHERE cycle_id=?1",
                [&old_cycle],
            )
            .unwrap();

        assert_eq!(
            ledger
                .reset_planet(chrono::Utc::now() + chrono::Duration::hours(1))
                .unwrap(),
            1_000_000
        );
        assert_eq!(table_count(&ledger, "guest_provenance_reset_receipt"), 0);
        assert_eq!(table_count(&ledger, "guest_provenance_cycle"), 0);
        assert_eq!(
            ledger
                .connection
                .query_row(
                    "SELECT eligible FROM guest_provenance_lineage WHERE singleton=1",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
        assert_eq!(table_count(&ledger, "shop_effect_history"), 1);
        assert_eq!(table_count(&ledger, "planet_wallet_credit"), 1);
    }

    #[test]
    fn zero_credit_reset_keeps_ordinary_effect_history_and_no_receipt() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        let old_cycle = ledger.planet_cycle_id().unwrap();
        assert_eq!(
            ledger
                .reset_planet(chrono::Utc::now() + chrono::Duration::hours(1))
                .unwrap(),
            0
        );
        assert_ne!(ledger.planet_cycle_id().unwrap(), old_cycle);
        assert_eq!(table_count(&ledger, "guest_provenance_reset_receipt"), 0);
        assert_eq!(table_count(&ledger, "shop_effect_history"), 1);
        assert_eq!(table_count(&ledger, "planet_wallet_credit"), 1);
        assert_eq!(
            ledger
                .connection
                .query_row(
                    "SELECT eligible FROM guest_provenance_lineage WHERE singleton=1",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn second_reset_preserves_normal_gameplay_and_does_not_add_receipt() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        ledger
            .insert(&raw_one_date_record(chrono::Utc::now()))
            .unwrap();
        let first_reset_at = chrono::DateTime::from_timestamp_micros(
            (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp_micros(),
        )
        .unwrap();
        assert_eq!(ledger.reset_planet(first_reset_at).unwrap(), 1_000_000);
        let previous_cycle = ledger.planet_cycle_id().unwrap();
        let second_reset_at = chrono::DateTime::from_timestamp_micros(
            (first_reset_at + chrono::Duration::hours(25)).timestamp_micros(),
        )
        .unwrap();
        assert_eq!(ledger.reset_planet(second_reset_at).unwrap(), 0);
        assert_ne!(ledger.planet_cycle_id().unwrap(), previous_cycle);
        assert_eq!(table_count(&ledger, "guest_provenance_reset_receipt"), 1);
        assert_eq!(table_count(&ledger, "shop_effect_history"), 1);
        assert_eq!(
            ledger
                .connection
                .query_row(
                    "SELECT eligible FROM guest_provenance_lineage WHERE singleton=1",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn threshold_growth_keeps_ordinary_reward_and_holds_import_proof() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        ledger
            .insert(&raw_record_with_tokens(
                "guest-reset-threshold-growth",
                chrono::Utc::now(),
                5_000_000,
            ))
            .unwrap();
        let reset_at = chrono::Utc::now() + chrono::Duration::hours(1);
        assert_eq!(ledger.reset_planet(reset_at).unwrap(), 5_000_000);
        assert_eq!(table_count(&ledger, "guest_provenance_reset_receipt"), 0);
        assert_eq!(table_count(&ledger, "shop_era_progress"), 1);
        assert_eq!(table_count(&ledger, "planet_wallet_credit"), 1);
        assert_eq!(table_count(&ledger, "shop_effect_history"), 1);
        assert_eq!(
            ledger
                .connection
                .query_row(
                    "SELECT eligible FROM guest_provenance_lineage WHERE singleton=1",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn purchased_guest_state_keeps_normal_reset_but_no_first_reset_receipt() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        ledger
            .insert(&raw_one_date_record(chrono::Utc::now()))
            .unwrap();
        ledger
            .connection
            .execute(
                "INSERT INTO cosmetic_purchase(account_id,purchase_id,sku,price,purchased_at_utc)
                 VALUES ('local','fixture-purchase','fixture-sku',0,'2026-10-03T00:00:00.000000Z')",
                [],
            )
            .unwrap();
        assert_eq!(
            ledger
                .reset_planet(chrono::Utc::now() + chrono::Duration::hours(1))
                .unwrap(),
            1_000_000
        );
        assert_eq!(table_count(&ledger, "guest_provenance_reset_receipt"), 0);
        assert_eq!(table_count(&ledger, "planet_wallet_credit"), 1);
        assert_eq!(table_count(&ledger, "shop_effect_history"), 1);
        assert_eq!(
            ledger
                .connection
                .query_row(
                    "SELECT eligible FROM guest_provenance_lineage WHERE singleton=1",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn non_uuid_direct_reset_keeps_ordinary_history_without_first_reset_receipt() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        ledger
            .insert(&raw_one_date_record(chrono::Utc::now()))
            .unwrap();
        let old_cycle = ledger.planet_cycle_id().unwrap();
        let legacy_request_id = format!("legacy-reset:{old_cycle}");
        let reset_at = chrono::DateTime::from_timestamp_micros(
            (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp_micros(),
        )
        .unwrap();

        let result = ledger
            .reset_guest_planet(&legacy_request_id, &old_cycle, reset_at)
            .unwrap();

        assert_eq!(
            result.status,
            crate::domain::cosmetic_shop::ShopActionStatus::Reset
        );
        assert_eq!(result.request_id, legacy_request_id);
        assert_eq!(table_count(&ledger, "guest_provenance_reset_receipt"), 0);
        assert_eq!(table_count(&ledger, "shop_effect_history"), 1);
        assert_eq!(table_count(&ledger, "planet_wallet_credit"), 1);
        assert_eq!(
            ledger
                .connection
                .query_row(
                    "SELECT eligible FROM guest_provenance_lineage WHERE singleton=1",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn expired_preexisting_reset_deadline_keeps_ordinary_history_without_receipt() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        ledger
            .insert(&raw_one_date_record(chrono::Utc::now()))
            .unwrap();
        let stale_deadline = (chrono::Utc::now() - chrono::Duration::hours(1))
            .to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
        ledger
            .connection
            .execute(
                "INSERT INTO setting(key,value) VALUES('planet_reset_available_at_utc',?1)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                [&stale_deadline],
            )
            .unwrap();

        assert_eq!(
            ledger
                .reset_planet(chrono::Utc::now() + chrono::Duration::hours(1))
                .unwrap(),
            1_000_000
        );

        assert_eq!(table_count(&ledger, "guest_provenance_reset_receipt"), 0);
        assert_eq!(table_count(&ledger, "shop_effect_history"), 1);
        assert_eq!(table_count(&ledger, "planet_wallet_credit"), 1);
        assert_eq!(
            ledger
                .connection
                .query_row(
                    "SELECT eligible FROM guest_provenance_lineage WHERE singleton=1",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn preexisting_effect_history_keeps_ordinary_reset_without_first_reset_receipt() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        ledger
            .insert(&raw_one_date_record(chrono::Utc::now()))
            .unwrap();
        let old_cycle = ledger.planet_cycle_id().unwrap();
        let activation = ledger.planet_activation_at().unwrap();
        let default_effects =
            serde_json::to_string(&crate::domain::cosmetic_shop::ActiveEffects::default()).unwrap();
        ledger
            .connection
            .execute(
                "INSERT INTO shop_effect_history(account_id,cycle_id,revision,started_at_utc,
                 active_instance_ids_json,effects_json) VALUES('local',?1,1,?2,'[]',?3)",
                rusqlite::params![
                    old_cycle,
                    activation.to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
                    default_effects
                ],
            )
            .unwrap();
        let reset_at = chrono::DateTime::from_timestamp_micros(
            (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp_micros(),
        )
        .unwrap();

        assert_eq!(ledger.reset_planet(reset_at).unwrap(), 1_000_000);

        assert_eq!(table_count(&ledger, "guest_provenance_reset_receipt"), 0);
        assert_eq!(table_count(&ledger, "shop_effect_history"), 2);
        assert_eq!(table_count(&ledger, "planet_wallet_credit"), 1);
        let (old_end, new_revision): (Option<String>, i64) = ledger
            .connection
            .query_row(
                "SELECT
                   (SELECT ended_at_utc FROM shop_effect_history
                    WHERE account_id='local' AND cycle_id=?1 AND revision=1),
                   (SELECT revision FROM shop_effect_history
                    WHERE account_id='local' AND cycle_id=(SELECT value FROM setting
                      WHERE key='planet_current_cycle_id'))",
                [&old_cycle],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            old_end.as_deref(),
            Some(
                reset_at
                    .to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
                    .as_str()
            )
        );
        assert_eq!(new_revision, 2);
        assert_eq!(
            ledger
                .connection
                .query_row(
                    "SELECT eligible FROM guest_provenance_lineage WHERE singleton=1",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn reset_provenance_failure_rolls_back_credit() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), chrono_tz::UTC).unwrap();
        let occurred_at = chrono::Utc::now();
        ledger.insert(&raw_one_date_record(occurred_at)).unwrap();

        ledger.prepare_growth_journal().unwrap();
        ledger
            .connection
            .execute_batch(
                "CREATE TRIGGER fail_guest_first_reset_receipt
             BEFORE INSERT ON guest_provenance_reset_receipt
             BEGIN SELECT RAISE(ABORT,'injected first-reset receipt failure'); END;",
            )
            .unwrap();

        let previous_cycle_id = ledger.planet_cycle_id().unwrap();
        let old_setting: (String, Option<String>) = ledger
            .connection
            .query_row(
                "SELECT (SELECT value FROM setting WHERE key='planet_current_cycle_id'),
                        (SELECT value FROM setting WHERE key='planet_reset_available_at_utc')",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let before = reset_state_dump(&ledger);
        let reset_at = chrono::DateTime::from_timestamp_micros(
            (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp_micros(),
        )
        .unwrap();
        assert!(ledger.reset_planet(reset_at).is_err());
        assert_eq!(reset_state_dump(&ledger), before);
        assert_eq!(ledger.planet_cycle_id().unwrap(), previous_cycle_id);
        let new_setting: (String, Option<String>) = ledger
            .connection
            .query_row(
                "SELECT (SELECT value FROM setting WHERE key='planet_current_cycle_id'),
                        (SELECT value FROM setting WHERE key='planet_reset_available_at_utc')",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(new_setting, old_setting);
        assert_eq!(
            ledger
                .daily_total(
                    crate::domain::usage::Agent::Codex,
                    &occurred_at.format("%Y-%m-%d").to_string()
                )
                .unwrap(),
            Some(1_000_000)
        );
        ledger
            .connection
            .execute_batch("DROP TRIGGER fail_guest_first_reset_receipt;")
            .unwrap();
        assert_eq!(ledger.reset_planet(reset_at).unwrap(), 1_000_000);
        ledger.prepare_growth_journal().unwrap();
        let (retry_end, retry_credit): (Option<String>, Option<i64>) = ledger
            .connection
            .query_row(
                "SELECT ended_at_utc,wallet_credit FROM growth_journal_cycle
                 WHERE account_id='local' AND cycle_id=?1",
                [&previous_cycle_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            retry_end.as_deref(),
            Some(
                reset_at
                    .to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
                    .as_str()
            )
        );
        assert_eq!(retry_credit, Some(1_000_000));
        assert_eq!(table_count(&ledger, "guest_provenance_reset_receipt"), 1);
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CanonicalOrdinalHistory {
    verified: bool,
    credits: Vec<PlanetWalletCredit>,
}

fn ordinal_chronology_is_valid(
    credits: &[PlanetWalletCredit],
    cycle_started_at: &str,
    last_reset_at: Option<&str>,
) -> bool {
    let Ok(cycle_start) = DateTime::parse_from_rfc3339(cycle_started_at) else {
        return false;
    };
    let last_reset = match last_reset_at.map(DateTime::parse_from_rfc3339).transpose() {
        Ok(value) => value,
        Err(_) => return false,
    };
    if last_reset.is_some_and(|reset| reset > cycle_start) {
        return false;
    }
    let mut matching_reset = last_reset.is_none();
    for credit in credits {
        let Ok(completed_at) = DateTime::parse_from_rfc3339(&credit.created_at_utc) else {
            return false;
        };
        if completed_at > cycle_start || last_reset.is_some_and(|reset| completed_at > reset) {
            return false;
        }
        matching_reset |= last_reset == Some(completed_at);
    }
    matching_reset
}

impl Ledger {
    pub fn planet_ordinal(
        &self,
    ) -> Result<crate::domain::planet_ordinal::PlanetOrdinal, ScanError> {
        use crate::domain::planet_ordinal::{PlanetOrdinal, PlanetOrdinalStatus};
        let account = self.cosmetic_account_id()?;
        let cycle = self.planet_cycle_id()?;
        let credits = if account == "local" || account == "legacy" {
            self.planet_wallet_credits()?
        } else {
            let key = format!("ordinal_history:{account}:{cycle}");
            let Some(raw) = setting_value(&self.connection, &key)? else {
                return Ok(PlanetOrdinal::unknown());
            };
            let history: CanonicalOrdinalHistory =
                serde_json::from_str(&raw).map_err(|_| ScanError::Database)?;
            if !history.verified {
                return Ok(PlanetOrdinal::unknown());
            }
            history.credits
        };
        let Some(cycle_started_at) =
            setting_value(&self.connection, "planet_cycle_started_at_utc")?
        else {
            return Ok(PlanetOrdinal::unknown());
        };
        let last_reset_at = setting_value(&self.connection, "planet_last_reset_at_utc")?;
        if !ordinal_chronology_is_valid(&credits, &cycle_started_at, last_reset_at.as_deref()) {
            return Ok(PlanetOrdinal::unknown());
        }
        let mut unique = std::collections::BTreeMap::new();
        for credit in credits {
            if credit.previous_cycle_id.trim().is_empty()
                || credit.previous_cycle_id == cycle
                || DateTime::parse_from_rfc3339(&credit.created_at_utc).is_err()
            {
                return Ok(PlanetOrdinal::unknown());
            }
            if let Some(previous) = unique.insert(credit.previous_cycle_id.clone(), credit.clone())
            {
                if previous != credit {
                    return Ok(PlanetOrdinal::unknown());
                }
            }
        }
        Ok(PlanetOrdinal {
            status: PlanetOrdinalStatus::Verified,
            current: Some(unique.len() as u64 + 1),
        })
    }

    pub fn record_canonical_planet_history(
        &mut self,
        account_id: &str,
        state: &PlanetState,
    ) -> Result<(), ScanError> {
        if self.cosmetic_account_id()? != account_id {
            return Err(ScanError::InvalidShopState);
        }
        record_ordinal_history_in(&self.connection, account_id, state)
    }
}

pub(crate) fn record_ordinal_history_in(
    connection: &Connection,
    account: &str,
    state: &PlanetState,
) -> Result<(), ScanError> {
    let mut unique = std::collections::BTreeMap::new();
    let mut verified = !state.current_cycle_id.trim().is_empty()
        && ordinal_chronology_is_valid(
            &state.wallet_credits,
            &state.cycle_started_at_utc,
            state.last_reset_at_utc.as_deref(),
        );
    for credit in &state.wallet_credits {
        verified &= !credit.previous_cycle_id.trim().is_empty()
            && credit.previous_cycle_id != state.current_cycle_id
            && DateTime::parse_from_rfc3339(&credit.created_at_utc).is_ok();
        if let Some(previous) = unique.insert(credit.previous_cycle_id.clone(), credit.clone()) {
            verified &= previous == *credit;
        }
    }
    let mut query = connection.prepare("SELECT previous_cycle_id FROM planet_wallet_credit")?;
    for id in query.query_map([], |row| row.get::<_, String>(0))? {
        verified &= unique.contains_key(&id?);
    }
    // Keep the known completion set across cycles, even if economic caches are replaced.
    let mut query = connection.prepare("SELECT value FROM setting WHERE key LIKE ?1")?;
    for raw in query.query_map([format!("ordinal_history:{account}:%")], |row| {
        row.get::<_, String>(0)
    })? {
        let history: CanonicalOrdinalHistory =
            serde_json::from_str(&raw?).map_err(|_| ScanError::Database)?;
        for credit in history.credits {
            verified &= unique.contains_key(&credit.previous_cycle_id);
        }
    }
    let history = CanonicalOrdinalHistory {
        verified,
        credits: unique.into_values().collect(),
    };
    connection.execute("INSERT INTO setting(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![format!("ordinal_history:{account}:{}",state.current_cycle_id),serde_json::to_string(&history).map_err(|_| ScanError::Database)?])?;
    Ok(())
}

#[cfg(test)]
mod cycle_ordinal_tests {
    use super::*;
    fn ledger() -> Ledger {
        Ledger::open(Path::new(":memory:"), chrono_tz::UTC).unwrap()
    }
    fn planet(ledger: &Ledger) -> PlanetState {
        let usage = TokenUsage {
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            total_tokens: None,
            coverage: UsageCoverage::Unavailable,
        };
        crate::growth::world_snapshot(
            ledger,
            crate::collectors::discovery::ScanSummary {
                codex: usage.clone(),
                claude_code: usage,
                codex_source: crate::collectors::discovery::SourceHealth::NotFound,
                claude_code_source: crate::collectors::discovery::SourceHealth::NotFound,
                confirmed_subtotal: None,
                complete_total: None,
                scanned_at_utc: Utc::now(),
            },
        )
        .unwrap()
        .planet
    }
    fn credit(id: &str, amount: u64) -> PlanetWalletCredit {
        PlanetWalletCredit {
            previous_cycle_id: id.into(),
            amount,
            created_at_utc: "2026-10-01T00:00:00Z".into(),
        }
    }
    #[test]
    fn cycle_ordinal_purchase_balance_and_journal_deletion_do_not_change_sequence() {
        let mut ledger = ledger();
        ledger.connection.execute("INSERT INTO planet_wallet_credit VALUES ('completed',500000,'2026-10-01T00:00:00Z')", []).unwrap();
        assert_eq!(ledger.planet_ordinal().unwrap().current, Some(2));
        let purchase = ledger
            .purchase_guest_cosmetic("11111111-1111-4111-8111-111111111111", "star_cluster_v2")
            .unwrap();
        assert_eq!(purchase.available_balance, 0);
        assert_eq!(ledger.planet_ordinal().unwrap().current, Some(2));
        let account = setting_value(&ledger.connection, "planet_account_id")
            .unwrap()
            .unwrap();
        for table in ["growth_journal_cycle", "growth_journal_remote_cycle"] {
            ledger.connection.execute(&format!("INSERT INTO {table}(account_id,cycle_id,wallet_credit) VALUES (?1,'journal-completed',500000)"), [&account]).unwrap();
            let count: i64 = ledger
                .connection
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE account_id=?1"),
                    [&account],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(count > 0);
        }
        ledger
            .apply_growth_journal_state(&crate::domain::growth_journal::GrowthJournal {
                generation: 1,
                deleted_at_utc: Some("2026-10-10T00:00:00Z".into()),
                timezone: Some("UTC".into()),
                cycles: vec![],
                entries: vec![],
            })
            .unwrap();
        for table in ["growth_journal_cycle", "growth_journal_remote_cycle"] {
            let count: i64 = ledger
                .connection
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE account_id=?1"),
                    [&account],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(count, 0);
        }
        assert_eq!(ledger.planet_ordinal().unwrap().current, Some(2));
    }
    #[test]
    fn cycle_ordinal_account_switch_restores_each_canonical_sequence() {
        let mut ledger = ledger();
        let accounts = [
            "11111111-1111-4111-8111-111111111111",
            "22222222-2222-4222-8222-222222222222",
        ];
        for (index, account) in accounts.iter().enumerate() {
            ledger.ensure_planet_account(account).unwrap();
            let mut remote = planet(&ledger);
            remote.wallet_credits = (0..=index)
                .map(|n| credit(&format!("completed-{n}"), 0))
                .collect();
            ledger.merge_remote_planet_state(&remote).unwrap();
            ledger
                .record_canonical_planet_history(&format!("account:{account}"), &remote)
                .unwrap();
            assert_eq!(
                ledger.planet_ordinal().unwrap().current,
                Some(index as u64 + 2)
            );
        }
        for (index, account) in accounts.iter().enumerate() {
            ledger.ensure_planet_account(account).unwrap();
            assert_eq!(
                ledger.planet_ordinal().unwrap().current,
                Some(index as u64 + 2)
            );
        }
    }
    #[test]
    fn cycle_ordinal_fresh_guest_starts_at_one_and_zero_credit_counts() {
        let ledger = ledger();
        assert_eq!(ledger.planet_ordinal().unwrap().current, Some(1));
        ledger.connection.execute("INSERT INTO planet_wallet_credit VALUES ('legacy-completed',0,'2026-10-01T00:00:00Z')",[]).unwrap();
        assert_eq!(ledger.planet_ordinal().unwrap().current, Some(2));
        ledger
            .connection
            .execute("DELETE FROM growth_journal_entry", [])
            .unwrap();
        assert_eq!(ledger.planet_ordinal().unwrap().current, Some(2));
    }
    #[test]
    fn cycle_ordinal_signed_cache_unknown_until_complete_canonical_history() {
        let mut ledger = ledger();
        ledger
            .ensure_planet_account("11111111-1111-4111-8111-111111111111")
            .unwrap();
        let remote = planet(&ledger);
        ledger.merge_remote_planet_state(&remote).unwrap();
        assert_eq!(ledger.planet_ordinal().unwrap().current, None);
        ledger
            .record_canonical_planet_history(
                "account:11111111-1111-4111-8111-111111111111",
                &remote,
            )
            .unwrap();
        assert_eq!(ledger.planet_ordinal().unwrap().current, Some(1));
    }
    #[test]
    fn cycle_ordinal_canonical_dedupes_exact_and_rejects_conflict_or_current_cycle() {
        let mut ledger = ledger();
        let account = "account:11111111-1111-4111-8111-111111111111";
        ledger
            .ensure_planet_account("11111111-1111-4111-8111-111111111111")
            .unwrap();
        let mut remote = planet(&ledger);
        remote.wallet_credits = vec![credit("legacy-completed", 0), credit("legacy-completed", 0)];
        ledger
            .record_canonical_planet_history(account, &remote)
            .unwrap();
        assert_eq!(ledger.planet_ordinal().unwrap().current, Some(2));
        remote.wallet_credits[1].amount = 1;
        ledger
            .record_canonical_planet_history(account, &remote)
            .unwrap();
        assert_eq!(ledger.planet_ordinal().unwrap().current, None);
        remote.wallet_credits = vec![credit(&remote.current_cycle_id, 0)];
        ledger
            .record_canonical_planet_history(account, &remote)
            .unwrap();
        assert_eq!(ledger.planet_ordinal().unwrap().current, None);
    }
    #[test]
    fn cycle_ordinal_valid_legacy_and_guest_retry_preserve_recorded_sequence() {
        let mut ledger = ledger();
        let at = Utc::now() + Duration::hours(1);
        let cycle = ledger.planet_cycle_id().unwrap();
        let request = uuid::Uuid::new_v4().to_string();
        ledger.reset_guest_planet(&request, &cycle, at).unwrap();
        assert_eq!(ledger.planet_ordinal().unwrap().current, Some(2));
        ledger.reset_guest_planet(&request, &cycle, at).unwrap();
        assert_eq!(ledger.planet_ordinal().unwrap().current, Some(2));
        ledger
            .connection
            .execute(
                "UPDATE setting SET value='legacy' WHERE key='planet_account_id'",
                [],
            )
            .unwrap();
        assert_eq!(ledger.planet_ordinal().unwrap().current, Some(2));
    }

    #[test]
    fn cycle_ordinal_chronology_canonical_future_completion_is_unknown() {
        let mut ledger = ledger();
        ledger
            .ensure_planet_account("11111111-1111-4111-8111-111111111111")
            .unwrap();
        let mut remote = planet(&ledger);
        remote.cycle_started_at_utc = "2026-10-02T00:00:00Z".into();
        remote.wallet_credits = vec![PlanetWalletCredit {
            previous_cycle_id: "future-completed".into(),
            amount: 0,
            created_at_utc: "2026-10-03T00:00:00Z".into(),
        }];
        ledger
            .record_canonical_planet_history(
                "account:11111111-1111-4111-8111-111111111111",
                &remote,
            )
            .unwrap();
        assert_eq!(ledger.planet_ordinal().unwrap().current, None);
    }
    #[test]
    fn cycle_ordinal_chronology_canonical_latest_reset_requires_matching_completion() {
        let mut ledger = ledger();
        ledger
            .ensure_planet_account("11111111-1111-4111-8111-111111111111")
            .unwrap();
        let mut remote = planet(&ledger);
        remote.cycle_started_at_utc = "2026-10-02T00:00:00Z".into();
        remote.last_reset_at_utc = Some(remote.cycle_started_at_utc.clone());
        remote.wallet_credits = vec![credit("older-completed", 0)];
        ledger
            .record_canonical_planet_history(
                "account:11111111-1111-4111-8111-111111111111",
                &remote,
            )
            .unwrap();
        assert_eq!(ledger.planet_ordinal().unwrap().current, None);
    }
    #[test]
    fn cycle_ordinal_chronology_future_completion_is_unknown() {
        let mut ledger = ledger();
        let start = "2026-10-02T00:00:00Z";
        set_setting_value(&ledger.connection, "planet_cycle_started_at_utc", start).unwrap();
        ledger.connection.execute("INSERT INTO planet_wallet_credit VALUES ('future-completed',0,'2026-10-03T00:00:00Z')", []).unwrap();
        assert_eq!(ledger.planet_ordinal().unwrap().current, None);
        ledger
            .ensure_planet_account("11111111-1111-4111-8111-111111111111")
            .unwrap();
        let mut remote = planet(&ledger);
        remote.cycle_started_at_utc = start.into();
        remote.wallet_credits = vec![PlanetWalletCredit {
            previous_cycle_id: "future-completed".into(),
            amount: 0,
            created_at_utc: "2026-10-03T00:00:00Z".into(),
        }];
        ledger
            .record_canonical_planet_history(
                "account:11111111-1111-4111-8111-111111111111",
                &remote,
            )
            .unwrap();
        assert_eq!(ledger.planet_ordinal().unwrap().current, None);
    }
    #[test]
    fn cycle_ordinal_chronology_latest_reset_requires_matching_completion() {
        let mut ledger = ledger();
        let start = "2026-10-02T00:00:00Z";
        set_setting_value(&ledger.connection, "planet_cycle_started_at_utc", start).unwrap();
        set_setting_value(&ledger.connection, "planet_last_reset_at_utc", start).unwrap();
        ledger.connection.execute("INSERT INTO planet_wallet_credit VALUES ('older-completed',0,'2026-10-01T00:00:00Z')", []).unwrap();
        assert_eq!(ledger.planet_ordinal().unwrap().current, None);
        ledger
            .ensure_planet_account("11111111-1111-4111-8111-111111111111")
            .unwrap();
        let mut remote = planet(&ledger);
        remote.cycle_started_at_utc = start.into();
        remote.last_reset_at_utc = Some(start.into());
        remote.wallet_credits = vec![credit("older-completed", 0)];
        ledger
            .record_canonical_planet_history(
                "account:11111111-1111-4111-8111-111111111111",
                &remote,
            )
            .unwrap();
        assert_eq!(ledger.planet_ordinal().unwrap().current, None);
        remote.wallet_credits.push(PlanetWalletCredit {
            previous_cycle_id: "latest-completed".into(),
            amount: 0,
            created_at_utc: "2026-10-02T09:00:00+09:00".into(),
        });
        ledger
            .record_canonical_planet_history(
                "account:11111111-1111-4111-8111-111111111111",
                &remote,
            )
            .unwrap();
        assert_eq!(ledger.planet_ordinal().unwrap().current, Some(3));
    }

    #[test]
    fn cycle_ordinal_reset_marker_without_completion_is_unknown() {
        let mut ledger = ledger();
        let account = "account:11111111-1111-4111-8111-111111111111";
        ledger
            .ensure_planet_account("11111111-1111-4111-8111-111111111111")
            .unwrap();
        let mut remote = planet(&ledger);
        remote.last_reset_at_utc = Some(remote.cycle_started_at_utc.clone());
        ledger
            .record_canonical_planet_history(account, &remote)
            .unwrap();
        assert_eq!(ledger.planet_ordinal().unwrap().current, None);
    }

    #[test]
    fn cycle_ordinal_known_missing_history_remains_unknown() {
        let mut ledger = ledger();
        let account = "account:11111111-1111-4111-8111-111111111111";
        ledger
            .ensure_planet_account("11111111-1111-4111-8111-111111111111")
            .unwrap();
        let remote = planet(&ledger);
        ledger.connection.execute("INSERT INTO planet_wallet_credit VALUES ('known-completed',0,'2026-10-01T00:00:00Z')",[]).unwrap();
        ledger
            .record_canonical_planet_history(account, &remote)
            .unwrap();
        assert_eq!(ledger.planet_ordinal().unwrap().current, None);
    }
}
