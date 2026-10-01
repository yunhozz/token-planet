use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use rusqlite::{params, Connection, OptionalExtension};

use super::ledger::{Ledger, ScanError};
use crate::domain::cosmetic_shop::{ActiveEffects, EffectContribution, RewardState};
use crate::domain::planet::{
    PlanetActivityDayContribution, PlanetDeviceContributionSnapshot,
    PlanetEffectContributionSegment,
};
use crate::domain::shop_effects::{cycle_token_bonus, weighted_growth};

#[derive(Clone)]
struct EffectHistory {
    cycle_id: String,
    revision: u64,
    started_at: DateTime<Utc>,
    ended_at: Option<DateTime<Utc>>,
    effects: ActiveEffects,
}

#[derive(Clone)]
struct CanonicalOccurrence {
    event_key: String,
    occurred_at: DateTime<Utc>,
    growth_date: String,
    reward_date: String,
    cycle_id: String,
    effect_revision: u64,
    tokens: u64,
    effects: ActiveEffects,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ContributionKey {
    device_id: String,
    cycle_id: String,
    date: String,
    effect_revision: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ActivitySnapshot {
    reward_date: String,
    cycle_id: String,
    first_occurred_at_utc: String,
    tokens: u64,
}

impl Ledger {
    pub fn rebuild_shop_contributions(&mut self) -> Result<(), ScanError> {
        let connection = &self.connection;
        let account_id = current_account_id(connection)?;
        if account_id.starts_with("account:") {
            return Ok(());
        }
        let occurrences = canonical_occurrences(connection)?;
        let device_id = setting(connection,"planet_device_id")?.ok_or(ScanError::Database)?;
        let mut next_contributions = BTreeMap::<ContributionKey,(u64,u16,u16)>::new();
        let mut next_activity = BTreeMap::<String,ActivitySnapshot>::new();
        for occurrence in &occurrences {
            let key = ContributionKey {
                device_id: device_id.clone(),
                cycle_id: occurrence.cycle_id.clone(),
                date: occurrence.growth_date.clone(),
                effect_revision: occurrence.effect_revision,
            };
            let values = next_contributions.entry(key).or_insert((
                0,occurrence.effects.civilization_growth_bps,occurrence.effects.token_earning_bps,
            ));
            if values.1 != occurrence.effects.civilization_growth_bps
                || values.2 != occurrence.effects.token_earning_bps {
                return Err(ScanError::InvalidShopState);
            }
            values.0 = values.0.checked_add(occurrence.tokens).ok_or(ScanError::InvalidCount)?;
            if occurrence.tokens == 0 {
                continue;
            }
            let activity = next_activity.entry(occurrence.reward_date.clone()).or_insert_with(|| ActivitySnapshot {
                reward_date: occurrence.reward_date.clone(),
                cycle_id: occurrence.cycle_id.clone(),
                first_occurred_at_utc: occurrence.occurred_at.to_rfc3339(),
                tokens: 0,
            });
            activity.tokens = activity.tokens.checked_add(occurrence.tokens).ok_or(ScanError::InvalidCount)?;
            let first = DateTime::parse_from_rfc3339(&activity.first_occurred_at_utc)
                .map_err(|_| ScanError::Database)?.with_timezone(&Utc);
            if occurrence.occurred_at < first {
                activity.first_occurred_at_utc = occurrence.occurred_at.to_rfc3339();
                activity.cycle_id = occurrence.cycle_id.clone();
            }
        }
        let old_contributions = contribution_snapshot(connection,&account_id)?;
        let new_contributions = next_contributions.iter().map(|(key,(tokens,growth,wallet))|
            (key.clone(),*tokens,*growth,*wallet)).collect::<Vec<_>>();
        let old_activity = activity_snapshot(connection,&account_id)?;
        let new_activity = next_activity.values().cloned().collect::<Vec<_>>();
        let changed = old_contributions != new_contributions || old_activity != new_activity;
        let stored_version: Option<i64> = connection.query_row(
            "SELECT canonical_version FROM shop_contribution_state WHERE account_id=?1",
            [&account_id],|row|row.get(0),
        ).optional()?;
        let current_version = stored_version.unwrap_or(0);
        let next_version = if changed {
            current_version.checked_add(1).ok_or(ScanError::InvalidCount)?
        } else {
            current_version
        };
        let transaction = self.connection.transaction()?;
        if changed {
            transaction.execute("DELETE FROM shop_effect_contribution WHERE account_id=?1",[&account_id])?;
            for (key,(tokens,growth_bps,wallet_bps)) in next_contributions {
                transaction.execute(
                    "INSERT INTO shop_effect_contribution(account_id,device_id,cycle_id,date,effect_revision,
                     canonical_version,tokens,growth_bps,wallet_bps) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                    params![account_id,key.device_id,key.cycle_id,key.date,
                        to_i64(key.effect_revision)?,next_version,to_i64(tokens)?,
                        growth_bps,wallet_bps],
                )?;
            }
            transaction.execute("DELETE FROM shop_activity_day WHERE account_id=?1",[&account_id])?;
            for activity in new_activity {
                transaction.execute(
                    "INSERT INTO shop_activity_day(account_id,reward_date,cycle_id,first_occurred_at_utc,
                     canonical_version,tokens) VALUES (?1,?2,?3,?4,?5,?6)",
                    params![account_id,activity.reward_date,activity.cycle_id,
                        activity.first_occurred_at_utc,next_version,to_i64(activity.tokens)?],
                )?;
            }
        }
        transaction.execute(
            "INSERT INTO shop_contribution_state(account_id,canonical_version) VALUES (?1,?2)
             ON CONFLICT(account_id) DO UPDATE SET canonical_version=excluded.canonical_version",
            params![account_id,next_version],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Reads the canonical contribution snapshot without rebuilding it from raw usage.
    /// Callers that upload after collecting new events must rebuild explicitly first.
    pub fn shop_device_contribution(
        &self,
        incomplete: bool,
    ) -> Result<PlanetDeviceContributionSnapshot, ScanError> {
        let transaction = self.connection.unchecked_transaction()?;
        let raw = self.planet_device_contribution(incomplete)?;
        let account_id = current_account_id(&transaction)?;
        let stored_version: Option<i64> = transaction.query_row(
            "SELECT canonical_version FROM shop_contribution_state WHERE account_id=?1",
            [&account_id],
            |row| row.get(0),
        ).optional()?;
        let canonical_version = to_u64(stored_version.unwrap_or(0))?;
        let device_id = raw.device_id.clone();
        let segments = {
            let mut statement = transaction.prepare(
                "SELECT cycle_id,date,effect_revision,tokens FROM shop_effect_contribution
                 WHERE account_id=?1 AND device_id=?2
                 ORDER BY cycle_id,date,effect_revision",
            )?;
            let rows = statement.query_map(params![account_id,device_id],|row|Ok((
                row.get::<_,String>(0)?,
                row.get::<_,String>(1)?,
                row.get::<_,i64>(2)?,
                row.get::<_,i64>(3)?,
            )))?.collect::<Result<Vec<_>,_>>()?;
            rows
        };
        let daily_segments = segments.into_iter().map(|(cycle_id,date,revision,tokens)|Ok(
            PlanetEffectContributionSegment {
                cycle_id,
                date,
                effect_revision: to_u64(revision)?,
                tokens: to_u64(tokens)?,
            }
        )).collect::<Result<Vec<_>,ScanError>>()?;
        let activity_days = activity_snapshot(&transaction,&account_id)?.into_iter().map(|activity|
            PlanetActivityDayContribution {
                reward_date: activity.reward_date,
                cycle_id: activity.cycle_id,
                first_occurred_at_utc: activity.first_occurred_at_utc,
                tokens: activity.tokens,
            }
        ).collect();
        transaction.commit()?;
        Ok(PlanetDeviceContributionSnapshot {
            raw,
            canonical_version,
            daily_segments,
            activity_days,
        })
    }

    pub fn settle_guest_rewards(
        &mut self,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), ScanError> {
        self.rebuild_shop_contributions()?;
        let account_id = current_account_id(&self.connection)?;
        if account_id.starts_with("account:") {
            return Ok(());
        }
        let current_cycle = current_cycle_id(&self.connection)?;
        let occurrences = canonical_occurrences(&self.connection)?;
        let histories = effect_histories(&self.connection,&account_id)?;
        let transaction = self.connection.transaction()?;

        let mut current_date: Option<&str> = None;
        let mut day_tokens = 0_u64;
        let mut day_segments = Vec::<EffectContribution>::new();
        let mut completed_growth = 0.0_f64;
        for occurrence in &occurrences {
            if occurrence.cycle_id != current_cycle {
                continue;
            }
            if current_date.is_some_and(|date| date != occurrence.growth_date) {
                completed_growth += weighted_growth(day_tokens,&day_segments)
                    .map_err(|_| ScanError::InvalidShopState)?;
                day_tokens = 0;
                day_segments.clear();
            }
            current_date = Some(&occurrence.growth_date);
            day_tokens = day_tokens.checked_add(occurrence.tokens).ok_or(ScanError::InvalidCount)?;
            day_segments.push(EffectContribution {
                device_id: setting(&transaction,"planet_device_id")?.ok_or(ScanError::Database)?,
                cycle_id: occurrence.cycle_id.clone(),
                date: occurrence.growth_date.clone(),
                effect_revision: occurrence.effect_revision,
                tokens: occurrence.tokens,
                growth_bps: occurrence.effects.civilization_growth_bps,
                wallet_bps: occurrence.effects.token_earning_bps,
            });
            let growth = completed_growth + weighted_growth(day_tokens,&day_segments)
                .map_err(|_| ScanError::InvalidShopState)?;
            for (index,threshold) in crate::growth::STAGE_THRESHOLDS.iter().enumerate() {
                let stage = (index+1) as u8;
                if growth < *threshold {
                    break;
                }
                let trigger_key = format!("era:{}:{stage}",occurrence.cycle_id);
                let existed = transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM shop_era_progress WHERE account_id=?1 AND cycle_id=?2 AND stage=?3)",
                    params![account_id,occurrence.cycle_id,stage],|row|row.get::<_,bool>(0),
                )?;
                if existed { continue; }
                let effect_snapshot = serde_json::to_string(&occurrence.effects).map_err(|_|ScanError::Database)?;
                let inserted = transaction.execute(
                    "INSERT OR IGNORE INTO shop_era_progress(account_id,cycle_id,stage,trigger_key,
                     effect_snapshot_json,awarded_at_utc) VALUES (?1,?2,?3,?4,?5,?6)",
                    params![account_id,occurrence.cycle_id,stage,trigger_key,effect_snapshot,now.to_rfc3339()],
                )?;
                if inserted == 0 { continue; }
                let amount = occurrence.effects.era_reward_tokens.min(10_000_000);
                if amount > 0 {
                    record_game_reward(&transaction,&account_id,&trigger_key,"era",
                        &occurrence.cycle_id,amount,&occurrence.effects,now)?;
                }
            }
        }

        let days = activity_snapshot(&transaction,&account_id)?;
        let active_dates = days.iter().map(|day|day.reward_date.clone()).collect::<std::collections::BTreeSet<_>>();
        for day in days {
            if day.cycle_id != current_cycle {
                continue;
            }
            let date = NaiveDate::parse_from_str(&day.reward_date,"%Y-%m-%d")
                .map_err(|_|ScanError::Database)?;
            let previous_date = (date - chrono::Duration::days(1)).format("%Y-%m-%d").to_string();
            if !active_dates.contains(&previous_date) {
                continue;
            }
            let first = DateTime::parse_from_rfc3339(&day.first_occurred_at_utc)
                .map_err(|_|ScanError::Database)?.with_timezone(&Utc);
            let snapshot = effects_at(&histories,first);
            let amount = snapshot.streak_reward_tokens.min(500_000);
            if amount == 0 { continue; }
            let trigger_key = format!("streak:{}",day.reward_date);
            record_game_reward(&transaction,&account_id,&trigger_key,"streak",
                &day.cycle_id,amount,&snapshot,now)?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn shop_growth_credit_by_date(
        &self,
    ) -> Result<std::collections::BTreeMap<String, f64>, ScanError> {
        let (daily,_,_) = self.planet_usage_totals()?;
        let account_id = current_account_id(&self.connection)?;
        if account_id.starts_with("account:") {
            return daily.into_iter().map(|(date,tokens)| {
                Ok((date,weighted_growth(tokens,&[]).map_err(|_|ScanError::InvalidShopState)?))
            }).collect();
        }
        let current_cycle = current_cycle_id(&self.connection)?;
        let mut totals = BTreeMap::new();
        let mut statement = self.connection.prepare(
            "SELECT device_id,cycle_id,date,effect_revision,tokens,growth_bps,wallet_bps
             FROM shop_effect_contribution WHERE account_id=?1 AND cycle_id=?2 AND date=?3
             ORDER BY device_id,cycle_id,effect_revision",
        )?;
        for (date,total_tokens) in daily {
            let rows = statement.query_map(params![account_id,current_cycle,date],|row|Ok(EffectContribution {
                device_id:row.get(0)?,cycle_id:row.get(1)?,date:row.get(2)?,
                effect_revision:row.get(3)?,tokens:row.get(4)?,
                growth_bps:row.get(5)?,wallet_bps:row.get(6)?,
            }))?.collect::<Result<Vec<_>,_>>()?;
            let contributed = rows.iter().try_fold(0_u64,|sum,segment|
                sum.checked_add(segment.tokens).ok_or(ScanError::InvalidCount))?;
            if contributed != 0 && contributed != total_tokens {
                return Err(ScanError::InvalidShopState);
            }
            let credit = weighted_growth(total_tokens,&rows).map_err(|_|ScanError::InvalidShopState)?;
            totals.insert(date,credit);
        }
        Ok(totals)
    }

    pub fn settle_guest_cycle_tokens(
        &mut self,
        cycle_id: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64,ScanError> {
        self.rebuild_shop_contributions()?;
        let account_id = current_account_id(&self.connection)?;
        if account_id.starts_with("account:") { return Ok(0); }
        let transaction = self.connection.transaction()?;
        let prior: Option<i64> = transaction.query_row(
            "SELECT amount FROM shop_cycle_settlement WHERE account_id=?1 AND cycle_id=?2",
            params![account_id,cycle_id],|row|row.get(0),
        ).optional()?;
        if let Some(amount)=prior {
            return u64::try_from(amount).map_err(|_|ScanError::InvalidCount);
        }
        let segments = {
            let mut statement = transaction.prepare(
                "SELECT device_id,cycle_id,date,effect_revision,tokens,growth_bps,wallet_bps
                 FROM shop_effect_contribution WHERE account_id=?1 AND cycle_id=?2
                 ORDER BY device_id,date,effect_revision",
            )?;
            let rows=statement.query_map(params![account_id,cycle_id],|row|Ok(EffectContribution {
                device_id:row.get(0)?,cycle_id:row.get(1)?,date:row.get(2)?,
                effect_revision:row.get(3)?,tokens:row.get(4)?,
                growth_bps:row.get(5)?,wallet_bps:row.get(6)?,
            }))?.collect::<Result<Vec<_>,_>>()?;
            rows
        };
        let amount=cycle_token_bonus(&segments).map_err(|_|ScanError::InvalidShopState)?;
        transaction.execute(
            "INSERT INTO shop_cycle_settlement(account_id,cycle_id,amount,settled_at_utc) VALUES (?1,?2,?3,?4)",
            params![account_id,cycle_id,to_i64(amount)?,now.to_rfc3339()],
        )?;
        if amount>0 {
            let effects=ActiveEffects::default();
            record_game_reward(&transaction,&account_id,&format!("cycle-token:{cycle_id}"),
                "cycle_token",cycle_id,amount,&effects,now)?;
        }
        transaction.commit()?;
        Ok(amount)
    }

    pub(crate) fn initialize_shop_effect_storage(&mut self) -> Result<(), ScanError> {
        self.connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS shop_effect_history (
                account_id TEXT NOT NULL,
                cycle_id TEXT NOT NULL,
                revision INTEGER NOT NULL CHECK(revision > 0),
                started_at_utc TEXT NOT NULL,
                ended_at_utc TEXT,
                active_instance_ids_json TEXT NOT NULL,
                effects_json TEXT NOT NULL,
                PRIMARY KEY(account_id,cycle_id,revision)
             );
             CREATE INDEX IF NOT EXISTS shop_effect_history_by_time
                ON shop_effect_history(account_id,cycle_id,started_at_utc,ended_at_utc);
             CREATE TABLE IF NOT EXISTS shop_effect_contribution (
                account_id TEXT NOT NULL,
                device_id TEXT NOT NULL,
                cycle_id TEXT NOT NULL,
                date TEXT NOT NULL,
                effect_revision INTEGER NOT NULL,
                canonical_version INTEGER NOT NULL CHECK(canonical_version > 0),
                tokens INTEGER NOT NULL CHECK(tokens >= 0),
                growth_bps INTEGER NOT NULL CHECK(growth_bps BETWEEN 0 AND 2000),
                wallet_bps INTEGER NOT NULL CHECK(wallet_bps BETWEEN 0 AND 3000),
                PRIMARY KEY(account_id,device_id,cycle_id,date,effect_revision)
             );
             CREATE TABLE IF NOT EXISTS shop_contribution_state (
                account_id TEXT PRIMARY KEY,
                canonical_version INTEGER NOT NULL DEFAULT 0 CHECK(canonical_version >= 0)
             );
             CREATE TABLE IF NOT EXISTS shop_activity_day (
                account_id TEXT NOT NULL,
                reward_date TEXT NOT NULL,
                cycle_id TEXT NOT NULL,
                first_occurred_at_utc TEXT NOT NULL,
                canonical_version INTEGER NOT NULL DEFAULT 1 CHECK(canonical_version > 0),
                tokens INTEGER NOT NULL CHECK(tokens >= 0),
                PRIMARY KEY(account_id,reward_date)
             );
             CREATE TABLE IF NOT EXISTS shop_game_reward (
                account_id TEXT NOT NULL,
                reward_id TEXT NOT NULL,
                trigger_key TEXT NOT NULL,
                kind TEXT NOT NULL CHECK(kind IN ('cycle_token','era','streak')),
                cycle_id TEXT NOT NULL,
                amount INTEGER NOT NULL CHECK(amount >= 0),
                effect_snapshot_json TEXT NOT NULL,
                awarded_at_utc TEXT NOT NULL,
                PRIMARY KEY(account_id,reward_id),
                UNIQUE(account_id,trigger_key)
             );
             CREATE TABLE IF NOT EXISTS shop_wallet_credit (
                account_id TEXT NOT NULL,
                credit_id TEXT NOT NULL,
                trigger_key TEXT NOT NULL,
                cycle_id TEXT NOT NULL,
                amount INTEGER NOT NULL CHECK(amount >= 0),
                created_at_utc TEXT NOT NULL,
                PRIMARY KEY(account_id,credit_id),
                UNIQUE(account_id,trigger_key)
             );
             CREATE TABLE IF NOT EXISTS shop_cycle_settlement (
                account_id TEXT NOT NULL,
                cycle_id TEXT NOT NULL,
                amount INTEGER NOT NULL CHECK(amount >= 0),
                settled_at_utc TEXT NOT NULL,
                PRIMARY KEY(account_id,cycle_id)
             );
             CREATE TABLE IF NOT EXISTS shop_era_progress (
                account_id TEXT NOT NULL,
                cycle_id TEXT NOT NULL,
                stage INTEGER NOT NULL CHECK(stage BETWEEN 1 AND 4),
                trigger_key TEXT NOT NULL,
                effect_snapshot_json TEXT NOT NULL,
                awarded_at_utc TEXT NOT NULL,
                PRIMARY KEY(account_id,cycle_id,stage),
                UNIQUE(account_id,trigger_key)
             );",
        )?;
        Ok(())
    }

    pub(crate) fn shop_reward_state(
        &self,
        account_id: &str,
        cycle_id: &str,
    ) -> Result<RewardState, ScanError> {
        load_shop_reward_state(&self.connection, account_id, cycle_id)
    }
}

pub(crate) fn load_shop_reward_state(
    connection: &Connection,
    account_id: &str,
    cycle_id: &str,
) -> Result<RewardState, ScanError> {
        let reward_timezone: Option<String> = connection.query_row(
            "SELECT reward_timezone FROM shop_account_state WHERE account_id=?1",
            [account_id],
            |row| row.get(0),
        ).optional()?;
        let reward_timezone = match reward_timezone {
            Some(value) => value,
            None => connection.query_row(
                "SELECT value FROM setting WHERE key='planet_timezone'", [], |row| row.get(0),
            )?,
        };
        let settled_cycle_tokens: i64 = connection.query_row(
            "SELECT amount FROM shop_cycle_settlement WHERE account_id=?1 AND cycle_id=?2",
            params![account_id, cycle_id],
            |row| row.get(0),
        ).optional()?.unwrap_or(0);
        let era_reward_tokens: i64 = connection.query_row(
            "SELECT coalesce(sum(amount),0) FROM shop_game_reward
             WHERE account_id=?1 AND cycle_id=?2 AND kind='era'",
            params![account_id, cycle_id],
            |row| row.get(0),
        )?;
        let streak_reward_tokens: i64 = connection.query_row(
            "SELECT coalesce(sum(amount),0) FROM shop_game_reward
             WHERE account_id=?1 AND kind='streak'",
            [account_id],
            |row| row.get(0),
        )?;
        Ok(RewardState {
            reward_timezone,
            settled_cycle_tokens: to_u64(settled_cycle_tokens)?,
            era_reward_tokens: to_u64(era_reward_tokens)?,
            streak_reward_tokens: to_u64(streak_reward_tokens)?,
        })
}

fn to_u64(value: i64) -> Result<u64, ScanError> {
    u64::try_from(value).map_err(|_| ScanError::InvalidCount)
}

fn to_i64(value: u64) -> Result<i64, ScanError> {
    i64::try_from(value).map_err(|_|ScanError::InvalidCount)
}

fn setting(connection:&Connection,key:&str)->Result<Option<String>,ScanError> {
    connection.query_row("SELECT value FROM setting WHERE key=?1",[key],|row|row.get(0))
        .optional().map_err(Into::into)
}

fn current_account_id(connection:&Connection)->Result<String,ScanError> {
    setting(connection,"planet_account_id")?.ok_or(ScanError::Database)
}

fn current_cycle_id(connection:&Connection)->Result<String,ScanError> {
    setting(connection,"planet_current_cycle_id")?.ok_or(ScanError::Database)
}

fn effect_histories(connection:&Connection,account_id:&str)->Result<Vec<EffectHistory>,ScanError> {
    let mut statement=connection.prepare(
        "SELECT cycle_id,revision,started_at_utc,ended_at_utc,effects_json
         FROM shop_effect_history WHERE account_id=?1 ORDER BY started_at_utc,revision",
    )?;
    let rows=statement.query_map([account_id],|row|{
        let started:String=row.get(2)?;
        let ended:Option<String>=row.get(3)?;
        let effects:String=row.get(4)?;
        Ok((row.get::<_,String>(0)?,row.get::<_,i64>(1)?,started,ended,effects))
    })?;
    rows.map(|row|{
        let(cycle_id,revision,started,ended,effects)=row?;
        Ok(EffectHistory{
            cycle_id,
            revision:to_u64(revision)?,
            started_at:DateTime::parse_from_rfc3339(&started)
                .map_err(|_|ScanError::Database)?.with_timezone(&Utc),
            ended_at:ended.map(|value|DateTime::parse_from_rfc3339(&value)
                .map(|time|time.with_timezone(&Utc)).map_err(|_|ScanError::Database)).transpose()?,
            effects:serde_json::from_str(&effects).map_err(|_|ScanError::Database)?,
        })
    }).collect()
}

fn effects_at(histories:&[EffectHistory],at:DateTime<Utc>)->ActiveEffects {
    histories.iter()
        .filter(|history| history.started_at<=at && history.ended_at.is_none_or(|ended|at<ended))
        .max_by_key(|history|(history.started_at,history.revision))
        .map(|history|history.effects.clone())
        .unwrap_or_default()
}

fn canonical_occurrences(connection:&Connection)->Result<Vec<CanonicalOccurrence>,ScanError> {
    let account_id=current_account_id(connection)?;
    let current_cycle=current_cycle_id(connection)?;
    let activation=setting(connection,"planet_activation_at_utc")?
        .ok_or(ScanError::Database)?;
    let activation=DateTime::parse_from_rfc3339(&activation)
        .map_err(|_|ScanError::Database)?.with_timezone(&Utc);
    let cycle_started=setting(connection,"planet_last_reset_at_utc")?
        .or(setting(connection,"planet_cycle_started_at_utc")?)
        .ok_or(ScanError::Database)?;
    let cycle_started=DateTime::parse_from_rfc3339(&cycle_started)
        .map_err(|_|ScanError::Database)?.with_timezone(&Utc).max(activation);
    let device_id=setting(connection,"planet_device_id")?.ok_or(ScanError::Database)?;
    let growth_timezone=setting(connection,"planet_timezone")?
        .ok_or(ScanError::Database)?.parse::<Tz>().map_err(|_|ScanError::TimezoneMismatch)?;
    let reward_timezone=load_shop_reward_state(connection,&account_id,&current_cycle)?
        .reward_timezone.parse::<Tz>().map_err(|_|ScanError::TimezoneMismatch)?;
    let histories=effect_histories(connection,&account_id)?;
    let mut statement=connection.prepare(
        "SELECT r.event_key,r.occurred_at_utc,r.total_tokens
         FROM usage_record r
         WHERE r.total_tokens IS NOT NULL
           AND EXISTS(SELECT 1 FROM planet_usage_owner o
             WHERE o.event_key=r.event_key AND o.account_id=?1)
           AND ((r.agent='codex' AND coalesce((SELECT value FROM setting WHERE key='codex_enabled'),'true')!='false')
             OR (r.agent='claude_code' AND coalesce((SELECT value FROM setting WHERE key='claude_code_enabled'),'true')!='false'))
           AND (r.kind='response' OR NOT EXISTS(
             SELECT 1 FROM usage_record other
             WHERE other.source_id=r.source_id AND other.kind='response' AND other.agent=r.agent
               AND EXISTS(SELECT 1 FROM planet_usage_owner owner
                 WHERE owner.event_key=other.event_key AND owner.account_id=?1)))
         ORDER BY r.occurred_at_utc,r.event_key",
    )?;
    let raw=statement.query_map([&account_id],|row|{
        Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,i64>(2)?))
    })?.collect::<Result<Vec<_>,_>>()?;
    let mut occurrences=Vec::new();
    for(event_key,timestamp,tokens)in raw{
        let occurred_at=DateTime::parse_from_rfc3339(&timestamp)
            .map_err(|_|ScanError::Database)?.with_timezone(&Utc);
        if occurred_at<=activation{continue}
        let tokens=to_u64(tokens)?;
        let history=histories.iter()
            .filter(|history|history.started_at<=occurred_at
                && history.ended_at.is_none_or(|ended|occurred_at<ended))
            .max_by_key(|history|(history.started_at,history.revision));
        let(cycle_id,effect_revision,effects)=match history{
            Some(history)=>(history.cycle_id.clone(),history.revision,history.effects.clone()),
            None if occurred_at>cycle_started=>(current_cycle.clone(),0,ActiveEffects::default()),
            None=>("baseline".into(),0,ActiveEffects::default()),
        };
        occurrences.push(CanonicalOccurrence{
            event_key,
            occurred_at,
            growth_date:occurred_at.with_timezone(&growth_timezone).format("%Y-%m-%d").to_string(),
            reward_date:occurred_at.with_timezone(&reward_timezone).format("%Y-%m-%d").to_string(),
            cycle_id,
            effect_revision,
            tokens,
            effects,
        });
    }
    occurrences.sort_by(|left,right|left.occurred_at.cmp(&right.occurred_at)
        .then_with(||left.event_key.cmp(&right.event_key)));
    let _=device_id;
    Ok(occurrences)
}

fn contribution_snapshot(
    connection:&Connection,
    account_id:&str,
)->Result<Vec<(ContributionKey,u64,u16,u16)>,ScanError>{
    let mut statement=connection.prepare(
        "SELECT device_id,cycle_id,date,effect_revision,tokens,growth_bps,wallet_bps
         FROM shop_effect_contribution WHERE account_id=?1
         ORDER BY device_id,cycle_id,date,effect_revision",
    )?;
    let rows=statement.query_map([account_id],|row|{
        Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,
            row.get::<_,i64>(3)?,row.get::<_,i64>(4)?,row.get::<_,u16>(5)?,row.get::<_,u16>(6)?))
    })?;
    rows.map(|row|{
        let(device,cycle,date,revision,tokens,growth,wallet)=row?;
        Ok((ContributionKey{device_id:device,cycle_id:cycle,date,effect_revision:to_u64(revision)?},
            to_u64(tokens)?,growth,wallet))
    }).collect()
}

fn activity_snapshot(connection:&Connection,account_id:&str)->Result<Vec<ActivitySnapshot>,ScanError>{
    let mut statement=connection.prepare(
        "SELECT reward_date,cycle_id,first_occurred_at_utc,tokens FROM shop_activity_day
         WHERE account_id=?1 ORDER BY reward_date",
    )?;
    let rows=statement.query_map([account_id],|row|Ok(ActivitySnapshot{
        reward_date:row.get(0)?,cycle_id:row.get(1)?,first_occurred_at_utc:row.get(2)?,
        tokens:row.get(3)?,
    }))?.collect::<Result<Vec<_>,_>>().map_err(Into::into);
    rows
}

fn record_game_reward(
    connection:&Connection,
    account_id:&str,
    trigger_key:&str,
    kind:&str,
    cycle_id:&str,
    amount:u64,
    effects:&ActiveEffects,
    now:DateTime<Utc>,
)->Result<(),ScanError>{
    let effect_snapshot=serde_json::to_string(effects).map_err(|_|ScanError::Database)?;
    let inserted=connection.execute(
        "INSERT OR IGNORE INTO shop_game_reward(account_id,reward_id,trigger_key,kind,cycle_id,
         amount,effect_snapshot_json,awarded_at_utc) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![account_id,uuid::Uuid::new_v4().to_string(),trigger_key,kind,cycle_id,
            to_i64(amount)?,effect_snapshot,now.to_rfc3339()],
    )?;
    if inserted>0 && amount>0 {
        connection.execute(
            "INSERT OR IGNORE INTO shop_wallet_credit(account_id,credit_id,trigger_key,cycle_id,amount,created_at_utc)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![account_id,uuid::Uuid::new_v4().to_string(),trigger_key,cycle_id,
                to_i64(amount)?,now.to_rfc3339()],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Ledger;
    use crate::collectors::{ParsedRecord, RecordKind};
    use crate::domain::cosmetic_shop::{ActiveEffects, EffectContribution};
    use crate::domain::shop_effects::weighted_growth;
    use crate::domain::usage::{Agent, TokenUsage, UsageCoverage};
    use chrono::{DateTime, Utc};
    use chrono_tz::UTC;
    use rusqlite::params;
    use std::path::Path;

    fn ledger() -> Ledger {
        let ledger = Ledger::open(Path::new(":memory:"), UTC).unwrap();
        ledger.connection.execute(
            "UPDATE setting SET value='2026-09-24T00:00:00+00:00'
             WHERE key IN ('planet_activation_at_utc','planet_cycle_started_at_utc')",
            [],
        ).unwrap();
        ledger
    }

    fn add_event(ledger: &mut Ledger, key: &str, at: &str, tokens: u64) {
        ledger.insert(&ParsedRecord {
            agent: Agent::Codex,
            kind: RecordKind::Response,
            event_key: key.into(),
            occurred_at_utc: DateTime::parse_from_rfc3339(at).unwrap().with_timezone(&Utc),
            usage: TokenUsage {
                input_tokens: None,
                output_tokens: None,
                cache_read_tokens: None,
                cache_write_tokens: None,
                total_tokens: Some(tokens),
                coverage: UsageCoverage::Complete,
            },
        }).unwrap();
    }

    fn add_effect_history(ledger: &Ledger, revision: i64, started: &str, effects: ActiveEffects) {
        let account: String = ledger.connection.query_row(
            "SELECT value FROM setting WHERE key='planet_account_id'", [], |row| row.get(0),
        ).unwrap();
        let cycle: String = ledger.connection.query_row(
            "SELECT value FROM setting WHERE key='planet_current_cycle_id'", [], |row| row.get(0),
        ).unwrap();
        ledger.connection.execute(
            "INSERT INTO shop_effect_history(account_id,cycle_id,revision,started_at_utc,
             active_instance_ids_json,effects_json) VALUES (?1,?2,?3,?4,'[\"fixture\"]',?5)",
            params![account,cycle,revision,started,serde_json::to_string(&effects).unwrap()],
        ).unwrap();
    }

    fn contribution_rows(ledger: &Ledger, date: &str) -> Vec<EffectContribution> {
        let account: String = ledger.connection.query_row(
            "SELECT value FROM setting WHERE key='planet_account_id'", [], |row| row.get(0),
        ).unwrap();
        let mut statement = ledger.connection.prepare(
            "SELECT device_id,cycle_id,date,effect_revision,tokens,growth_bps,wallet_bps
             FROM shop_effect_contribution WHERE account_id=?1 AND date=?2 ORDER BY effect_revision",
        ).unwrap();
        statement.query_map(params![account,date], |row| Ok(EffectContribution {
            device_id: row.get(0)?, cycle_id: row.get(1)?, date: row.get(2)?,
            effect_revision: row.get(3)?, tokens: row.get(4)?,
            growth_bps: row.get(5)?, wallet_bps: row.get(6)?,
        })).unwrap().collect::<Result<Vec<_>,_>>().unwrap()
    }

    #[test]
    fn rebuild_uses_occurrence_time_and_replaces_late_or_corrected_canonical_rows() {
        let mut ledger = ledger();
        add_event(&mut ledger,"before-effect","2026-09-25T06:00:00Z",50_000);
        add_effect_history(&ledger,1,"2026-09-25T12:00:00+00:00",ActiveEffects {
            civilization_growth_bps: 2_000,
            token_earning_bps: 100,
            ..ActiveEffects::default()
        });
        add_event(&mut ledger,"after-effect","2026-09-25T18:00:00Z",50_000);

        ledger.rebuild_shop_contributions().unwrap();
        let segments = contribution_rows(&ledger,"2026-09-25");
        assert_eq!(segments.len(),2);
        assert_eq!(segments[0].growth_bps,0,"the effect must not apply before activation");
        assert_eq!(segments[1].growth_bps,2_000);
        assert!((weighted_growth(100_000,&segments).unwrap()-1.1).abs()<1e-12);

        add_event(&mut ledger,"late-before-effect","2026-09-25T09:00:00Z",1_000);
        ledger.rebuild_shop_contributions().unwrap();
        let segments = contribution_rows(&ledger,"2026-09-25");
        assert_eq!(segments.iter().map(|segment| segment.tokens).sum::<u64>(),101_000);
        assert_eq!(segments.iter().find(|segment| segment.effect_revision==0).unwrap().tokens,51_000);
        assert_eq!(segments.iter().find(|segment| segment.effect_revision==1).unwrap().tokens,50_000);

        add_event(&mut ledger,"after-effect","2026-09-25T18:00:00Z",25_000);
        ledger.rebuild_shop_contributions().unwrap();
        let corrected = contribution_rows(&ledger,"2026-09-25");
        assert_eq!(corrected.iter().map(|segment| segment.tokens).sum::<u64>(),76_000);
        assert_eq!(corrected.iter().find(|segment| segment.effect_revision==1).unwrap().tokens,25_000);
        let contribution_versions: i64 = ledger.connection.query_row(
            "SELECT count(DISTINCT canonical_version) FROM shop_effect_contribution",
            [], |row| row.get(0),
        ).unwrap();
        assert_eq!(contribution_versions,1,"replacement keeps one canonical set, not correction duplicates");

        ledger.set_agent_enabled(Agent::Codex,false).unwrap();
        ledger.rebuild_shop_contributions().unwrap();
        let remaining: i64 = ledger.connection.query_row(
            "SELECT count(*) FROM shop_effect_contribution", [], |row| row.get(0),
        ).unwrap();
        assert_eq!(remaining,0,"disabled source rows must be removed from canonical effects");
    }

    #[test]
    fn same_day_reset_growth_uses_only_the_current_cycle_contributions() {
        let mut ledger = ledger();
        add_effect_history(&ledger,1,"2026-09-24T00:00:00Z",ActiveEffects {
            civilization_growth_bps: 2_000,
            ..ActiveEffects::default()
        });
        add_event(&mut ledger,"same-day-old-cycle","2026-09-25T08:00:00Z",50_000);
        let old_cycle = ledger.planet_cycle_id().unwrap();
        let reset_at = DateTime::parse_from_rfc3339("2026-09-25T12:00:00Z").unwrap().with_timezone(&Utc);
        let reset = ledger.reset_guest_planet("same-day-cycle-reset",&old_cycle,reset_at).unwrap();
        assert_eq!(reset.status,crate::domain::cosmetic_shop::ShopActionStatus::Reset);

        add_event(&mut ledger,"same-day-new-cycle","2026-09-25T18:00:00Z",25_000);
        ledger.rebuild_shop_contributions().unwrap();
        let current_cycle = ledger.planet_cycle_id().unwrap();
        let same_date = contribution_rows(&ledger,"2026-09-25");
        assert!(same_date.iter().any(|segment| segment.cycle_id == old_cycle));
        assert!(same_date.iter().any(|segment| segment.cycle_id == current_cycle));
        assert_eq!(ledger.planet_usage_totals().unwrap().0["2026-09-25"],25_000);

        let growth = ledger.shop_growth_credit_by_date().unwrap();
        let expected = weighted_growth(25_000,&[]).unwrap();
        assert!((growth["2026-09-25"]-expected).abs()<1e-12);
    }

    #[test]
    fn shop_device_contribution_reads_stored_all_cycle_segments_and_activity_without_mutation() {
        let mut ledger = ledger();
        add_effect_history(&ledger,1,"2026-09-24T00:00:00Z",ActiveEffects::default());
        add_event(&mut ledger,"snapshot-old-cycle","2026-09-25T08:00:00Z",50_000);
        let old_cycle = ledger.planet_cycle_id().unwrap();
        let reset_at = DateTime::parse_from_rfc3339("2026-09-25T12:00:00Z").unwrap().with_timezone(&Utc);
        ledger.reset_guest_planet("snapshot-cycle-reset",&old_cycle,reset_at).unwrap();
        let current_cycle = ledger.planet_cycle_id().unwrap();
        add_effect_history(&ledger,1,"2026-09-25T12:00:00Z",ActiveEffects::default());
        add_event(&mut ledger,"snapshot-current-cycle","2026-09-25T18:00:00Z",25_000);
        ledger.rebuild_shop_contributions().unwrap();

        let account: String = ledger.connection.query_row(
            "SELECT value FROM setting WHERE key='planet_account_id'",[],|row|row.get(0),
        ).unwrap();
        let device: String = ledger.connection.query_row(
            "SELECT value FROM setting WHERE key='planet_device_id'",[],|row|row.get(0),
        ).unwrap();
        ledger.connection.execute(
            "INSERT INTO shop_contribution_state(account_id,canonical_version) VALUES ('account:bob',77)",
            [],
        ).unwrap();
        ledger.connection.execute(
            "INSERT INTO shop_effect_contribution(account_id,device_id,cycle_id,date,effect_revision,
             canonical_version,tokens,growth_bps,wallet_bps) VALUES (?1,'another-device','ignored-device',
             '2026-09-25',1,77,9,0,0)",
            [&account],
        ).unwrap();
        ledger.connection.execute(
            "INSERT INTO shop_effect_contribution(account_id,device_id,cycle_id,date,effect_revision,
             canonical_version,tokens,growth_bps,wallet_bps) VALUES ('account:bob',?1,'ignored-account',
             '2026-09-25',1,77,11,0,0)",
            [&device],
        ).unwrap();
        ledger.connection.execute(
            "INSERT INTO shop_activity_day(account_id,reward_date,cycle_id,first_occurred_at_utc,
             canonical_version,tokens) VALUES ('account:bob','2026-09-26','ignored-account',
             '2026-09-26T08:00:00+00:00',77,11)",
            [],
        ).unwrap();

        let stored_version: i64 = ledger.connection.query_row(
            "SELECT canonical_version FROM shop_contribution_state WHERE account_id=?1",
            [&account],|row|row.get(0),
        ).unwrap();
        let before_rows: (i64,i64) = ledger.connection.query_row(
            "SELECT (SELECT count(*) FROM shop_effect_contribution WHERE account_id=?1),
                    (SELECT count(*) FROM shop_activity_day WHERE account_id=?1)",
            [&account],|row|Ok((row.get(0)?,row.get(1)?)),
        ).unwrap();

        let contribution = ledger.shop_device_contribution(false).unwrap();
        let repeated = ledger.shop_device_contribution(false).unwrap();
        let value = serde_json::to_value(&contribution).unwrap();
        assert_eq!(value,serde_json::to_value(&repeated).unwrap());
        assert_eq!(contribution.canonical_version,stored_version as u64);
        assert_eq!(contribution.raw.device_id,device);
        assert_eq!(contribution.raw.current_cycle_id,current_cycle);
        assert_eq!(contribution.raw.current_planet_tokens,25_000);
        assert_eq!(contribution.raw.lifetime_tokens,75_000);
        assert_eq!(contribution.raw.daily_tokens.get("2026-09-25"),Some(&25_000));

        let mut expected_segments = vec![
            (old_cycle.clone(),serde_json::json!({
                "cycle_id": old_cycle,
                "date": "2026-09-25",
                "effect_revision": 1,
                "tokens": 50_000,
            })),
            (current_cycle.clone(),serde_json::json!({
                "cycle_id": current_cycle,
                "date": "2026-09-25",
                "effect_revision": 2,
                "tokens": 25_000,
            })),
        ];
        expected_segments.sort_by(|left,right|left.0.cmp(&right.0));
        let expected_segments: Vec<_> = expected_segments.into_iter().map(|(_,segment)|segment).collect();
        assert_eq!(value["daily_segments"],serde_json::json!(expected_segments));
        assert_eq!(value["activity_days"],serde_json::json!([{
            "reward_date": "2026-09-25",
            "cycle_id": old_cycle,
            "first_occurred_at_utc": "2026-09-25T08:00:00+00:00",
            "tokens": 75_000,
        }]));
        let mut top_level_keys = value.as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>();
        top_level_keys.sort_unstable();
        assert_eq!(top_level_keys,vec![
            "activity_days","canonical_version","current_cycle_id","current_planet_tokens",
            "daily_segments","daily_tokens","device_id","incomplete","lifetime_tokens",
        ]);
        for segment in value["daily_segments"].as_array().unwrap() {
            let mut keys = segment.as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>();
            keys.sort_unstable();
            assert_eq!(keys,vec!["cycle_id","date","effect_revision","tokens"]);
        }
        for activity in value["activity_days"].as_array().unwrap() {
            let mut keys = activity.as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>();
            keys.sort_unstable();
            assert_eq!(keys,vec!["cycle_id","first_occurred_at_utc","reward_date","tokens"]);
        }
        let after_version: i64 = ledger.connection.query_row(
            "SELECT canonical_version FROM shop_contribution_state WHERE account_id=?1",
            [&account],|row|row.get(0),
        ).unwrap();
        let after_rows: (i64,i64) = ledger.connection.query_row(
            "SELECT (SELECT count(*) FROM shop_effect_contribution WHERE account_id=?1),
                    (SELECT count(*) FROM shop_activity_day WHERE account_id=?1)",
            [&account],|row|Ok((row.get(0)?,row.get(1)?)),
        ).unwrap();
        assert_eq!(after_version,stored_version);
        assert_eq!(after_rows,before_rows);

        // New raw events do not silently rewrite the persisted canonical version during a read.
        add_event(&mut ledger,"snapshot-not-yet-rebuilt","2026-09-25T20:00:00Z",5_000);
        let pending = ledger.shop_device_contribution(false).unwrap();
        assert_eq!(pending.canonical_version,stored_version as u64);
        assert_eq!(pending.raw.current_planet_tokens,30_000);
        assert_eq!(pending.daily_segments,contribution.daily_segments);
        let still_stored_version: i64 = ledger.connection.query_row(
            "SELECT canonical_version FROM shop_contribution_state WHERE account_id=?1",
            [&account],|row|row.get(0),
        ).unwrap();
        assert_eq!(still_stored_version,stored_version);
    }

    #[test]
    fn reward_settlement_uses_frozen_timezone_consecutive_dates_and_deduped_era_keys() {
        let mut ledger = ledger();
        add_effect_history(&ledger,1,"2026-09-24T00:00:00+00:00",ActiveEffects {
            era_reward_tokens: 500_000,
            streak_reward_tokens: 10_000,
            ..ActiveEffects::default()
        });
        add_event(&mut ledger,"era-crossing","2026-09-25T23:30:00Z",3_500_000);
        add_event(&mut ledger,"day-two","2026-09-26T00:30:00Z",50_000);
        add_effect_history(&ledger,2,"2026-09-26T00:45:00Z",ActiveEffects {
            streak_reward_tokens: 70_000,
            ..ActiveEffects::default()
        });
        add_event(&mut ledger,"day-two-later","2026-09-26T10:30:00Z",50_000);
        add_event(&mut ledger,"gap-day","2026-09-28T00:30:00Z",50_000);
        ledger.set_planet_timezone("Pacific/Kiritimati").unwrap();
        let now = DateTime::parse_from_rfc3339("2026-09-29T00:00:00Z").unwrap().with_timezone(&Utc);

        ledger.rebuild_shop_contributions().unwrap();
        ledger.settle_guest_rewards(now).unwrap();
        ledger.settle_guest_rewards(now).unwrap();
        let activity_dates: Vec<String> = {
            let mut statement = ledger.connection.prepare(
                "SELECT reward_date FROM shop_activity_day ORDER BY reward_date",
            ).unwrap();
            statement.query_map([],|row|row.get(0)).unwrap()
                .collect::<Result<Vec<_>,_>>().unwrap()
        };
        assert_eq!(activity_dates,vec!["2026-09-25","2026-09-26","2026-09-28"]);
        assert_eq!(ledger.shop_state().unwrap().reward_state.reward_timezone,"UTC");
        let (era_count,era_amount):(i64,i64)=ledger.connection.query_row(
            "SELECT count(*),coalesce(sum(amount),0) FROM shop_game_reward WHERE kind='era'",
            [],|row|Ok((row.get(0)?,row.get(1)?)),
        ).unwrap();
        assert_eq!((era_count,era_amount),(1,500_000));
        let (streak_count,streak_date,streak_amount):(i64,Option<String>,i64)=ledger.connection.query_row(
            "SELECT count(*),max(trigger_key),coalesce(sum(amount),0)
             FROM shop_game_reward WHERE kind='streak'",
            [],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
        ).unwrap();
        assert_eq!((streak_count,streak_date.as_deref(),streak_amount),(1,Some("streak:2026-09-26"),10_000));
        let wallet_total:i64=ledger.connection.query_row(
            "SELECT coalesce(sum(amount),0) FROM shop_wallet_credit",[],|row|row.get(0),
        ).unwrap();
        assert_eq!(wallet_total,510_000);

        add_event(&mut ledger,"era-crossing","2026-09-25T23:30:00Z",0);
        ledger.rebuild_shop_contributions().unwrap();
        ledger.settle_guest_rewards(now).unwrap();
        let persisted_rewards:i64=ledger.connection.query_row(
            "SELECT count(*) FROM shop_game_reward WHERE kind IN ('era','streak')",[],|row|row.get(0),
        ).unwrap();
        assert_eq!(persisted_rewards,2,"canonical correction cannot claw back a confirmed reward or issue it twice");
        let reduced_growth: f64 = ledger.shop_growth_credit_by_date().unwrap().values().sum();
        assert!(reduced_growth < crate::growth::STAGE_THRESHOLDS[0]);

        add_event(&mut ledger,"era-crossing","2026-09-25T23:30:00Z",3_500_000);
        ledger.rebuild_shop_contributions().unwrap();
        ledger.settle_guest_rewards(now).unwrap();
        let rebound_growth: f64 = ledger.shop_growth_credit_by_date().unwrap().values().sum();
        assert!(rebound_growth >= crate::growth::STAGE_THRESHOLDS[0]);
        let reward_rows: i64 = ledger.connection.query_row(
            "SELECT count(*) FROM shop_game_reward WHERE kind='era'",[],|row|row.get(0),
        ).unwrap();
        assert_eq!(reward_rows,1,"a score drop and later recrossing reuses the original era key");
        assert_eq!(ledger.shop_state().unwrap().reward_state.era_reward_tokens,500_000);
    }

    #[test]
    fn reward_settlement_awards_each_era_threshold_once() {
        let mut ledger = ledger();
        add_effect_history(&ledger,1,"2026-09-24T00:00:00Z",ActiveEffects {
            era_reward_tokens: 500_000,
            ..ActiveEffects::default()
        });
        add_event(&mut ledger,"threshold-day-one","2026-09-25T12:00:00Z",1_000_000_000_000_000_000);
        add_event(&mut ledger,"threshold-day-two","2026-09-26T12:00:00Z",1_000_000_000_000_000_000);
        add_event(&mut ledger,"threshold-day-three","2026-09-27T12:00:00Z",1_000_000_000_000_000_000);
        let now = DateTime::parse_from_rfc3339("2026-09-29T00:00:00Z").unwrap().with_timezone(&Utc);

        ledger.settle_guest_rewards(now).unwrap();
        ledger.settle_guest_rewards(now).unwrap();

        let stages: Vec<i64> = {
            let mut statement = ledger.connection.prepare(
                "SELECT stage FROM shop_era_progress ORDER BY stage",
            ).unwrap();
            statement.query_map([],|row|row.get(0)).unwrap()
                .collect::<Result<Vec<_>,_>>().unwrap()
        };
        assert_eq!(stages,vec![1,2,3,4]);
        let (count,amount):(i64,i64)=ledger.connection.query_row(
            "SELECT count(*),coalesce(sum(amount),0) FROM shop_game_reward WHERE kind='era'",
            [],|row|Ok((row.get(0)?,row.get(1)?)),
        ).unwrap();
        assert_eq!((count,amount),(4,2_000_000));
        assert_eq!(ledger.shop_state().unwrap().reward_state.era_reward_tokens,2_000_000);
    }

    #[test]
    fn old_cycle_growth_does_not_unlock_era_rewards_in_the_new_cycle() {
        let mut ledger = ledger();
        add_effect_history(&ledger,1,"2026-09-24T00:00:00Z",ActiveEffects {
            era_reward_tokens: 500_000,
            ..ActiveEffects::default()
        });
        add_event(&mut ledger,"old-cycle-growth","2026-09-25T12:00:00Z",1_000_000_000_000_000_000);
        ledger.connection.execute(
            "UPDATE setting SET value='new-cycle' WHERE key='planet_current_cycle_id'",
            [],
        ).unwrap();
        ledger.connection.execute(
            "UPDATE setting SET value='2026-09-26T00:00:00Z'
             WHERE key IN ('planet_last_reset_at_utc','planet_cycle_started_at_utc')",
            [],
        ).unwrap();

        ledger.settle_guest_rewards(DateTime::parse_from_rfc3339("2026-09-29T00:00:00Z").unwrap().with_timezone(&Utc)).unwrap();

        let rewards: i64 = ledger.connection.query_row(
            "SELECT count(*) FROM shop_era_progress",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(rewards,0);
    }

    #[test]
    fn cycle_token_settlement_carries_fractional_bonus_and_is_idempotent() {
        let mut ledger = ledger();
        add_effect_history(&ledger,1,"2026-09-24T00:00:00Z",ActiveEffects {
            token_earning_bps: 100,
            ..ActiveEffects::default()
        });
        for ordinal in 0..100 {
            add_event(&mut ledger,&format!("wallet-event-{ordinal}"),"2026-09-25T12:00:00Z",1);
        }
        let cycle: String = ledger.connection.query_row(
            "SELECT value FROM setting WHERE key='planet_current_cycle_id'",[],|row|row.get(0),
        ).unwrap();
        let now = DateTime::parse_from_rfc3339("2026-09-29T00:00:00Z").unwrap().with_timezone(&Utc);

        assert_eq!(ledger.settle_guest_cycle_tokens(&cycle,now).unwrap(),1);
        assert_eq!(ledger.settle_guest_cycle_tokens(&cycle,now).unwrap(),1);
        assert_eq!(ledger.planet_usage_totals().unwrap().1,100);
        let (settlements,rewards,credits):(i64,i64,i64)=ledger.connection.query_row(
            "SELECT (SELECT count(*) FROM shop_cycle_settlement WHERE account_id='local'),
             (SELECT count(*) FROM shop_game_reward WHERE account_id='local' AND kind='cycle_token'),
             (SELECT coalesce(sum(amount),0) FROM shop_wallet_credit WHERE account_id='local')",
            [],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
        ).unwrap();
        assert_eq!((settlements,rewards,credits),(1,1,1));
    }

    #[test]
    fn late_closed_cycle_activity_does_not_award_streak_but_can_precede_a_current_cycle_day() {
        let mut ledger = ledger();
        add_effect_history(&ledger,1,"2026-09-24T00:00:00Z",ActiveEffects {
            streak_reward_tokens: 70_000,
            ..ActiveEffects::default()
        });
        add_event(&mut ledger,"old-cycle-day-one","2026-09-30T10:00:00Z",10);
        ledger.settle_guest_rewards(DateTime::parse_from_rfc3339("2026-10-01T00:00:00Z").unwrap().with_timezone(&Utc)).unwrap();
        assert_eq!(ledger.connection.query_row::<i64,_,_>(
            "SELECT count(*) FROM shop_game_reward WHERE kind='streak'",[],|row|row.get(0),
        ).unwrap(),0);

        let old_cycle = ledger.planet_cycle_id().unwrap();
        ledger.reset_planet(DateTime::parse_from_rfc3339("2026-10-02T00:00:00Z").unwrap().with_timezone(&Utc)).unwrap();
        let new_cycle = ledger.planet_cycle_id().unwrap();
        assert_ne!(old_cycle,new_cycle);

        // A new raw event arrives after reset but its occurrence belongs to yesterday in the old cycle.
        add_event(&mut ledger,"late-old-cycle-day-two","2026-10-01T10:00:00Z",10);
        ledger.rebuild_shop_contributions().unwrap();
        let late_cycle: String = ledger.connection.query_row(
            "SELECT cycle_id FROM shop_activity_day WHERE reward_date='2026-10-01'",[],|row|row.get(0),
        ).unwrap();
        assert_eq!(late_cycle,old_cycle);
        ledger.settle_guest_rewards(DateTime::parse_from_rfc3339("2026-10-02T00:01:00Z").unwrap().with_timezone(&Utc)).unwrap();
        assert_eq!(ledger.connection.query_row::<i64,_,_>(
            "SELECT count(*) FROM shop_game_reward WHERE kind='streak'",[],|row|row.get(0),
        ).unwrap(),0);

        // The previous old-cycle day may establish continuity for an active day in the new cycle.
        add_effect_history(&ledger,3,"2026-10-02T00:00:00Z",ActiveEffects {
            streak_reward_tokens: 70_000,
            ..ActiveEffects::default()
        });
        add_event(&mut ledger,"new-cycle-day-three","2026-10-02T10:00:00Z",10);
        ledger.settle_guest_rewards(DateTime::parse_from_rfc3339("2026-10-03T00:00:00Z").unwrap().with_timezone(&Utc)).unwrap();
        assert_eq!(ledger.connection.query_row::<i64,_,_>(
            "SELECT coalesce(sum(amount),0) FROM shop_game_reward WHERE kind='streak'",[],|row|row.get(0),
        ).unwrap(),70_000);
        assert_eq!(ledger.connection.query_row::<i64,_,_>(
            "SELECT count(*) FROM shop_game_reward WHERE kind='streak' AND cycle_id=?1",
            [&new_cycle],|row|row.get(0),
        ).unwrap(),1);
    }

    #[test]
    fn logged_in_shop_effects_are_not_settled_from_local_usage() {
        let mut ledger = ledger();
        add_event(&mut ledger,"logged-event","2026-09-25T18:00:00Z",100_000);
        ledger.ensure_planet_account("alice").unwrap();
        let account: String = ledger.connection.query_row(
            "SELECT value FROM setting WHERE key='planet_account_id'",[],|row|row.get(0),
        ).unwrap();
        let cycle: String = ledger.connection.query_row(
            "SELECT value FROM setting WHERE key='planet_current_cycle_id'",[],|row|row.get(0),
        ).unwrap();
        let effects = ActiveEffects {
            civilization_growth_bps: 2_000,
            era_reward_tokens: 500_000,
            ..ActiveEffects::default()
        };
        ledger.connection.execute(
            "INSERT INTO shop_effect_history(account_id,cycle_id,revision,started_at_utc,
             active_instance_ids_json,effects_json) VALUES (?1,?2,1,'2026-09-24T00:00:00Z','[]',?3)",
            params![account,cycle,serde_json::to_string(&effects).unwrap()],
        ).unwrap();
        ledger.connection.execute(
            "INSERT INTO shop_effect_contribution(account_id,device_id,cycle_id,date,effect_revision,
             canonical_version,tokens,growth_bps,wallet_bps)
             VALUES (?1,'local-device',?2,'2026-09-25',1,1,100000,2000,0)",
            params![account,cycle],
        ).unwrap();

        let growth = ledger.shop_growth_credit_by_date().unwrap();
        assert!((growth["2026-09-25"] - crate::growth::contribution_credit(100_000,100_000.0)).abs() < 1e-12);
        ledger.rebuild_shop_contributions().unwrap();
        ledger.settle_guest_rewards(DateTime::parse_from_rfc3339("2026-09-29T00:00:00Z").unwrap().with_timezone(&Utc)).unwrap();
        assert_eq!(ledger.settle_guest_cycle_tokens(&cycle,DateTime::parse_from_rfc3339("2026-09-29T00:00:00Z").unwrap().with_timezone(&Utc)).unwrap(),0);
        let (contributions,rewards,credits):(i64,i64,i64)=ledger.connection.query_row(
            "SELECT (SELECT count(*) FROM shop_effect_contribution WHERE account_id=?1),
             (SELECT count(*) FROM shop_game_reward WHERE account_id=?1),
             (SELECT count(*) FROM shop_wallet_credit WHERE account_id=?1)",
            [&account],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
        ).unwrap();
        assert_eq!((contributions,rewards,credits),(1,0,0));
    }
}
