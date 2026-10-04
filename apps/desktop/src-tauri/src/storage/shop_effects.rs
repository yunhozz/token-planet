use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use rusqlite::{params, Connection, OptionalExtension, Transaction};

use super::ledger::{Ledger, ScanError};
use crate::domain::cosmetic_shop::{
    ActiveEffects, EffectContribution, RewardState, ShopEffectTimeline,
};
use crate::domain::planet::{
    PlanetActivityDayContribution, PlanetDeviceContribution, PlanetDeviceContributionSnapshot,
    PlanetEffectContributionSegment,
};
use crate::domain::shop_effects::{cycle_token_bonus, weighted_growth};

#[derive(Clone)]
struct EffectHistory {
    cycle_id: String,
    revision: u64,
    started_at: DateTime<Utc>,
    ended_at: Option<DateTime<Utc>>,
    active_instance_ids: Vec<String>,
    effects: ActiveEffects,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CycleBound {
    cycle_id: String,
    started_at: DateTime<Utc>,
    ended_at: Option<DateTime<Utc>>,
}

#[derive(Clone)]
struct GuestProvenanceBound {
    cycle_id: String,
    started_at: DateTime<Utc>,
    ended_at: Option<DateTime<Utc>>,
    revision: u64,
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

struct ConfirmedEffectTimelineState {
    current_cycle_id: String,
    effect_revision: u64,
    server_time: DateTime<Utc>,
    reward_timezone: String,
    // False only for timelines persisted before the server supplied cycle bounds.
    cycle_bounds_initialized: bool,
    cycle_bounds: Vec<CycleBound>,
    intervals: Vec<EffectHistory>,
}

fn rebuild_shop_contributions_in_transaction(connection: &Connection) -> Result<(), ScanError> {
    let account_id = current_account_id(connection)?;
    if account_id.starts_with("account:") {
        let confirmed: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM shop_effect_timeline_state WHERE account_id=?1)",
            [&account_id],
            |row| row.get(0),
        )?;
        if !confirmed {
            return Ok(());
        }
    }
    let occurrences = canonical_occurrences(connection)?;
    let device_id = setting(connection, "planet_device_id")?.ok_or(ScanError::Database)?;
    let mut next_contributions = BTreeMap::<ContributionKey, (u64, u16, u16)>::new();
    let mut next_activity = BTreeMap::<String, ActivitySnapshot>::new();
    for occurrence in &occurrences {
        let key = ContributionKey {
            device_id: device_id.clone(),
            cycle_id: occurrence.cycle_id.clone(),
            date: occurrence.growth_date.clone(),
            effect_revision: occurrence.effect_revision,
        };
        let values = next_contributions.entry(key).or_insert((
            0,
            occurrence.effects.civilization_growth_bps,
            occurrence.effects.token_earning_bps,
        ));
        if values.1 != occurrence.effects.civilization_growth_bps
            || values.2 != occurrence.effects.token_earning_bps
        {
            return Err(ScanError::InvalidShopState);
        }
        values.0 = values
            .0
            .checked_add(occurrence.tokens)
            .ok_or(ScanError::InvalidCount)?;
        if occurrence.tokens == 0 {
            continue;
        }
        let activity = next_activity
            .entry(occurrence.reward_date.clone())
            .or_insert_with(|| ActivitySnapshot {
                reward_date: occurrence.reward_date.clone(),
                cycle_id: occurrence.cycle_id.clone(),
                first_occurred_at_utc: occurrence.occurred_at.to_rfc3339(),
                tokens: 0,
            });
        activity.tokens = activity
            .tokens
            .checked_add(occurrence.tokens)
            .ok_or(ScanError::InvalidCount)?;
        let first = DateTime::parse_from_rfc3339(&activity.first_occurred_at_utc)
            .map_err(|_| ScanError::Database)?
            .with_timezone(&Utc);
        if occurrence.occurred_at < first {
            activity.first_occurred_at_utc = occurrence.occurred_at.to_rfc3339();
            activity.cycle_id = occurrence.cycle_id.clone();
        }
    }
    let old_contributions = contribution_snapshot(connection, &account_id)?;
    let new_contributions = next_contributions
        .iter()
        .map(|(key, (tokens, growth, wallet))| (key.clone(), *tokens, *growth, *wallet))
        .collect::<Vec<_>>();
    let old_activity = activity_snapshot(connection, &account_id)?;
    let new_activity = next_activity.values().cloned().collect::<Vec<_>>();
    let changed = old_contributions != new_contributions || old_activity != new_activity;
    let stored_version: Option<i64> = connection
        .query_row(
            "SELECT canonical_version FROM shop_contribution_state WHERE account_id=?1",
            [&account_id],
            |row| row.get(0),
        )
        .optional()?;
    let current_version = stored_version.unwrap_or(0);
    let next_version = if changed {
        current_version
            .checked_add(1)
            .ok_or(ScanError::InvalidCount)?
    } else {
        current_version
    };
    if changed {
        connection.execute(
            "DELETE FROM shop_effect_contribution WHERE account_id=?1",
            [&account_id],
        )?;
        for (key, (tokens, growth_bps, wallet_bps)) in next_contributions {
            connection.execute(
                "INSERT INTO shop_effect_contribution(account_id,device_id,cycle_id,date,effect_revision,
                 canonical_version,tokens,growth_bps,wallet_bps) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![
                    account_id,
                    key.device_id,
                    key.cycle_id,
                    key.date,
                    to_i64(key.effect_revision)?,
                    next_version,
                    to_i64(tokens)?,
                    growth_bps,
                    wallet_bps
                ],
            )?;
        }
        connection.execute(
            "DELETE FROM shop_activity_day WHERE account_id=?1",
            [&account_id],
        )?;
        for activity in new_activity {
            connection.execute(
                "INSERT INTO shop_activity_day(account_id,reward_date,cycle_id,first_occurred_at_utc,
                 canonical_version,tokens) VALUES (?1,?2,?3,?4,?5,?6)",
                params![
                    account_id,
                    activity.reward_date,
                    activity.cycle_id,
                    activity.first_occurred_at_utc,
                    next_version,
                    to_i64(activity.tokens)?
                ],
            )?;
        }
    }
    connection.execute(
        "INSERT INTO shop_contribution_state(account_id,canonical_version) VALUES (?1,?2)
         ON CONFLICT(account_id) DO UPDATE SET canonical_version=excluded.canonical_version",
        params![account_id, next_version],
    )?;
    Ok(())
}

pub(crate) fn seed_guest_import_contribution_baseline_in_transaction(
    connection: &Connection,
    account_id: &str,
    canonical_version: u64,
    contributions: &[crate::domain::cosmetic_shop::GuestEffectContribution],
    activity_days: &[crate::domain::cosmetic_shop::GuestActivityDay],
) -> Result<(), ScanError> {
    let canonical_version = to_i64(canonical_version)?;
    connection.execute(
        "DELETE FROM shop_effect_contribution WHERE account_id=?1",
        [account_id],
    )?;
    for contribution in contributions {
        if to_i64(contribution.canonical_version)? != canonical_version {
            return Err(ScanError::InvalidShopState);
        }
        connection.execute(
            "INSERT INTO shop_effect_contribution(account_id,device_id,cycle_id,date,effect_revision,
             canonical_version,tokens,growth_bps,wallet_bps) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                account_id,
                contribution.device_id,
                contribution.cycle_id,
                contribution.date,
                to_i64(contribution.effect_revision)?,
                canonical_version,
                to_i64(contribution.tokens)?,
                contribution.growth_bps,
                contribution.wallet_bps,
            ],
        )?;
    }

    connection.execute(
        "DELETE FROM shop_activity_day WHERE account_id=?1",
        [account_id],
    )?;
    for activity_day in activity_days {
        if to_i64(activity_day.canonical_version)? != canonical_version {
            return Err(ScanError::InvalidShopState);
        }
        connection.execute(
            "INSERT INTO shop_activity_day(account_id,reward_date,cycle_id,first_occurred_at_utc,
             canonical_version,tokens) VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                account_id,
                activity_day.reward_date,
                activity_day.cycle_id,
                activity_day.first_occurred_at_utc,
                canonical_version,
                to_i64(activity_day.tokens)?,
            ],
        )?;
    }
    connection.execute(
        "INSERT INTO shop_contribution_state(account_id,canonical_version) VALUES (?1,?2)
         ON CONFLICT(account_id) DO UPDATE SET canonical_version=excluded.canonical_version",
        params![account_id, canonical_version],
    )?;
    Ok(())
}

fn normalize_account_uuid(value: &str) -> Result<String, ScanError> {
    let value = value.strip_prefix("account:").unwrap_or(value);
    uuid::Uuid::parse_str(value)
        .map(|value| value.to_string())
        .map_err(|_| ScanError::InvalidShopState)
}

fn parse_utc(value: &str) -> Result<DateTime<Utc>, ScanError> {
    let parsed = DateTime::parse_from_rfc3339(value).map_err(|_| ScanError::InvalidShopState)?;
    if parsed.offset().local_minus_utc() != 0 {
        return Err(ScanError::InvalidShopState);
    }
    Ok(parsed.with_timezone(&Utc))
}

fn validate_effect_caps(effects: &ActiveEffects) -> Result<(), ScanError> {
    if effects.token_earning_bps > 3_000
        || effects.civilization_growth_bps > 2_000
        || effects.shop_discount_bps > 1_500
        || effects.reset_cooldown_bps > 2_500
        || effects.natural_removal_discount_bps > 3_000
        || effects.era_reward_tokens > 10_000_000
        || effects.streak_reward_tokens > 500_000
    {
        return Err(ScanError::InvalidShopState);
    }
    Ok(())
}

fn validate_shop_effect_timeline(
    timeline: &ShopEffectTimeline,
) -> Result<(Vec<CycleBound>, Vec<EffectHistory>), ScanError> {
    if timeline.account_id.starts_with("account:") {
        return Err(ScanError::InvalidShopState);
    }
    normalize_account_uuid(&timeline.account_id)?;
    if timeline.current_cycle_id.trim().is_empty()
        || to_i64(timeline.effect_revision).is_err()
        || timeline.reward_timezone.parse::<Tz>().is_err()
    {
        return Err(ScanError::InvalidShopState);
    }
    let server_time = parse_utc(&timeline.server_time_utc)?;
    let mut cycle_bounds = Vec::with_capacity(timeline.cycle_bounds.len());
    for (index, bound) in timeline.cycle_bounds.iter().enumerate() {
        if bound.cycle_id.trim().is_empty()
            || cycle_bounds
                .iter()
                .any(|previous: &CycleBound| previous.cycle_id == bound.cycle_id)
        {
            return Err(ScanError::InvalidShopState);
        }
        let started_at = parse_utc(&bound.started_at_utc)?;
        let ended_at = bound.ended_at_utc.as_deref().map(parse_utc).transpose()?;
        if started_at > server_time
            || ended_at.is_some_and(|ended| ended <= started_at || ended > server_time)
            || (index + 1 < timeline.cycle_bounds.len() && ended_at.is_none())
            || (index + 1 == timeline.cycle_bounds.len() && ended_at.is_some())
        {
            return Err(ScanError::InvalidShopState);
        }
        if let Some(previous) = cycle_bounds.last() {
            if previous.started_at >= started_at
                || previous.ended_at.is_none_or(|ended| ended > started_at)
            {
                return Err(ScanError::InvalidShopState);
            }
        }
        cycle_bounds.push(CycleBound {
            cycle_id: bound.cycle_id.clone(),
            started_at,
            ended_at,
        });
    }
    if cycle_bounds
        .last()
        .is_some_and(|bound| bound.cycle_id != timeline.current_cycle_id)
    {
        return Err(ScanError::InvalidShopState);
    }

    let mut histories = Vec::with_capacity(timeline.intervals.len());
    let mut previous_started = None;
    let mut previous_ended = None;
    let mut previous_revision = 0;
    for (index, interval) in timeline.intervals.iter().enumerate() {
        if interval.cycle_id.trim().is_empty()
            || interval.revision == 0
            || interval.revision <= previous_revision
            || interval.revision > timeline.effect_revision
        {
            return Err(ScanError::InvalidShopState);
        }
        let started_at = parse_utc(&interval.started_at_utc)?;
        let ended_at = interval
            .ended_at_utc
            .as_deref()
            .map(parse_utc)
            .transpose()?;
        let bound = cycle_bounds
            .iter()
            .find(|bound| bound.cycle_id == interval.cycle_id);
        if started_at > server_time
            || previous_started.is_some_and(|previous| started_at <= previous)
            || (index > 0 && previous_ended.is_none())
            || previous_ended.is_some_and(|previous| started_at < previous)
            || ended_at.is_some_and(|ended| ended <= started_at || ended > server_time)
            || (index + 1 < timeline.intervals.len() && ended_at.is_none())
            || (ended_at.is_none() && interval.cycle_id != timeline.current_cycle_id)
            || interval
                .active_instance_ids
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            || bound.is_some_and(|bound| {
                started_at < bound.started_at
                    || ended_at.is_some_and(|ended| {
                        bound.ended_at.is_some_and(|bound_end| ended > bound_end)
                    })
                    || (bound.ended_at.is_some() && ended_at.is_none())
            })
        {
            return Err(ScanError::InvalidShopState);
        }
        validate_effect_caps(&interval.effects)?;
        histories.push(EffectHistory {
            cycle_id: interval.cycle_id.clone(),
            revision: interval.revision,
            started_at,
            ended_at,
            active_instance_ids: interval.active_instance_ids.clone(),
            effects: interval.effects.clone(),
        });
        previous_started = Some(started_at);
        previous_ended = ended_at;
        previous_revision = interval.revision;
    }
    if histories
        .iter()
        .any(|history| history.revision > timeline.effect_revision)
    {
        return Err(ScanError::InvalidShopState);
    }
    Ok((cycle_bounds, histories))
}

fn load_cycle_bounds(
    connection: &Connection,
    account_id: &str,
) -> Result<Vec<CycleBound>, ScanError> {
    let mut statement = connection.prepare(
        "SELECT cycle_id,started_at_utc,ended_at_utc FROM shop_effect_cycle_bound
         WHERE account_id=?1 ORDER BY started_at_utc,cycle_id",
    )?;
    let rows = statement.query_map([account_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
        ))
    })?;
    rows.map(|row| {
        let (cycle_id, started_at, ended_at) = row?;
        Ok(CycleBound {
            cycle_id,
            started_at: parse_utc(&started_at)?,
            ended_at: ended_at.as_deref().map(parse_utc).transpose()?,
        })
    })
    .collect()
}

fn load_confirmed_timeline(
    connection: &Connection,
    account_id: &str,
) -> Result<Option<ConfirmedEffectTimelineState>, ScanError> {
    let stored: Option<(String, i64, String, String)> = connection
        .query_row(
            "SELECT current_cycle_id,effect_revision,server_time_utc,reward_timezone
             FROM shop_effect_timeline_state WHERE account_id=?1",
            [account_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let Some((current_cycle_id, effect_revision, server_time, reward_timezone)) = stored else {
        return Ok(None);
    };
    let cycle_bounds_initialized: bool = connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM shop_effect_cycle_bounds_state WHERE account_id=?1
         ) OR EXISTS(
             SELECT 1 FROM shop_effect_cycle_bound WHERE account_id=?1
         )",
        [account_id],
        |row| row.get(0),
    )?;
    Ok(Some(ConfirmedEffectTimelineState {
        current_cycle_id,
        effect_revision: to_u64(effect_revision)?,
        server_time: parse_utc(&server_time)?,
        reward_timezone,
        cycle_bounds_initialized,
        cycle_bounds: load_cycle_bounds(connection, account_id)?,
        intervals: effect_histories(connection, account_id)?,
    }))
}

fn validate_cycle_bounds_successor(
    prior_initialized: bool,
    prior: &[CycleBound],
    incoming: &[CycleBound],
    prior_server_time: DateTime<Utc>,
) -> Result<(), ScanError> {
    // A legacy cache gets one authoritative bootstrap; timeline revision, clock,
    // timezone, and positive-interval checks still run in the successor validator.
    if !prior_initialized {
        return Ok(());
    }
    if incoming.len() < prior.len() {
        return Err(ScanError::InvalidShopState);
    }
    if prior.is_empty() {
        if incoming
            .iter()
            .any(|bound| bound.started_at < prior_server_time)
        {
            return Err(ScanError::InvalidShopState);
        }
        return Ok(());
    }
    for (old, new) in prior.iter().take(prior.len() - 1).zip(incoming) {
        if old != new {
            return Err(ScanError::InvalidShopState);
        }
    }
    let old_last = prior.last().ok_or(ScanError::InvalidShopState)?;
    let new_last_known = &incoming[prior.len() - 1];
    if old_last.cycle_id != new_last_known.cycle_id
        || old_last.started_at != new_last_known.started_at
    {
        return Err(ScanError::InvalidShopState);
    }
    match (old_last.ended_at, new_last_known.ended_at) {
        (Some(old_end), Some(new_end)) if old_end == new_end && incoming.len() == prior.len() => {
            Ok(())
        }
        (None, None) if incoming.len() == prior.len() => Ok(()),
        (None, Some(new_end))
            if new_end >= prior_server_time
                && incoming.len() > prior.len()
                && incoming[prior.len()..]
                    .iter()
                    .all(|bound| bound.started_at >= prior_server_time) =>
        {
            Ok(())
        }
        _ => Err(ScanError::InvalidShopState),
    }
}

fn same_interval_content(left: &EffectHistory, right: &EffectHistory) -> bool {
    left.cycle_id == right.cycle_id
        && left.revision == right.revision
        && left.started_at == right.started_at
        && left.ended_at == right.ended_at
        && left.active_instance_ids == right.active_instance_ids
        && left.effects == right.effects
}

fn validate_timeline_successor(
    prior: &ConfirmedEffectTimelineState,
    incoming_timeline: &ShopEffectTimeline,
    incoming_cycle_bounds: &[CycleBound],
    incoming: &[EffectHistory],
) -> Result<(), ScanError> {
    let incoming_server_time = parse_utc(&incoming_timeline.server_time_utc)?;
    if incoming_server_time < prior.server_time
        || incoming_timeline.reward_timezone != prior.reward_timezone
        || incoming_timeline.effect_revision < prior.effect_revision
        || (incoming_timeline.current_cycle_id != prior.current_cycle_id
            && incoming_cycle_bounds.len() <= prior.cycle_bounds.len())
    {
        return Err(ScanError::InvalidShopState);
    }
    if incoming_timeline.effect_revision == prior.effect_revision {
        if incoming.len() != prior.intervals.len()
            || !incoming
                .iter()
                .zip(&prior.intervals)
                .all(|(new, old)| same_interval_content(new, old))
        {
            return Err(ScanError::InvalidShopState);
        }
        return Ok(());
    }
    if incoming.len() < prior.intervals.len() {
        return Err(ScanError::InvalidShopState);
    }
    for (index, old) in prior.intervals.iter().enumerate() {
        let new = &incoming[index];
        if old.cycle_id != new.cycle_id
            || old.revision != new.revision
            || old.started_at != new.started_at
            || old.active_instance_ids != new.active_instance_ids
            || old.effects != new.effects
        {
            return Err(ScanError::InvalidShopState);
        }
        match old.ended_at {
            Some(old_end) if new.ended_at != Some(old_end) => {
                return Err(ScanError::InvalidShopState);
            }
            None => {
                let Some(new_end) = new.ended_at else {
                    return Err(ScanError::InvalidShopState);
                };
                if new_end < prior.server_time {
                    return Err(ScanError::InvalidShopState);
                }
            }
            _ => {}
        }
    }
    for added in &incoming[prior.intervals.len()..] {
        if added.revision <= prior.effect_revision || added.started_at < prior.server_time {
            return Err(ScanError::InvalidShopState);
        }
    }
    if incoming
        .iter()
        .any(|interval| interval.revision > incoming_timeline.effect_revision)
    {
        return Err(ScanError::InvalidShopState);
    }
    Ok(())
}

impl Ledger {
    pub(super) fn confirmed_current_cycle_start_in_connection(
        connection: &Connection,
    ) -> Result<Option<DateTime<Utc>>, ScanError> {
        let account_id = current_account_id(connection)?;
        if !account_id.starts_with("account:") {
            return Ok(None);
        }
        let cycle_id = current_cycle_id(connection)?;
        let started_at: Option<String> = connection
            .query_row(
                "SELECT b.started_at_utc
                 FROM shop_effect_cycle_bounds_state initialized
                 JOIN shop_effect_timeline_state timeline USING(account_id)
                 JOIN shop_effect_cycle_bound b USING(account_id)
                 WHERE initialized.account_id=?1
                   AND timeline.current_cycle_id=?2
                   AND b.cycle_id=?2",
                params![account_id, cycle_id],
                |row| row.get(0),
            )
            .optional()?;
        started_at.as_deref().map(parse_utc).transpose()
    }

    pub fn rebuild_shop_contributions(&mut self) -> Result<(), ScanError> {
        self.require_guest_import_game_mutations_allowed()?;
        let transaction = self.connection.transaction()?;
        rebuild_shop_contributions_in_transaction(&transaction)?;
        transaction.commit()?;
        Ok(())
    }

    /// Applies a server-confirmed personal effect timeline and rebuilds this
    /// device's canonical contribution snapshot in one transaction.
    pub fn apply_confirmed_shop_effect_timeline(
        &mut self,
        timeline: &ShopEffectTimeline,
        expected_account_id: &str,
        expected_cycle_id: &str,
    ) -> Result<(), ScanError> {
        self.require_guest_import_game_mutations_allowed()?;
        let transaction = self.connection.transaction()?;
        apply_confirmed_shop_effect_timeline_in_transaction(
            &transaction,
            timeline,
            expected_account_id,
            expected_cycle_id,
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
        let snapshot = shop_device_contribution_from_connection(&transaction, raw)?;
        transaction.commit()?;
        Ok(snapshot)
    }

    pub fn settle_guest_rewards(
        &mut self,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), ScanError> {
        if !self.guest_import_game_mutations_allowed()? {
            return Ok(());
        }
        let transaction = self.connection.transaction()?;
        Self::settle_guest_rewards_in_transaction(&transaction, now)?;
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn prepare_guest_import_source_in_transaction(
        transaction: &Transaction<'_>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), ScanError> {
        Self::settle_guest_rewards_in_transaction(transaction, now)?;
        let current_cycle = current_cycle_id(transaction)?;
        Self::settle_guest_cycle_tokens_in_transaction(transaction, &current_cycle, now)?;
        Ok(())
    }

    pub(crate) fn settle_guest_rewards_in_transaction(
        transaction: &Transaction<'_>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), ScanError> {
        rebuild_shop_contributions_in_transaction(transaction)?;
        let account_id = current_account_id(transaction)?;
        if account_id.starts_with("account:") {
            return Ok(());
        }
        let current_cycle = current_cycle_id(transaction)?;
        let occurrences = canonical_occurrences(transaction)?;
        let histories = effect_histories(transaction, &account_id)?;

        let mut current_date: Option<&str> = None;
        let mut day_tokens = 0_u64;
        let mut day_segments = Vec::<EffectContribution>::new();
        let mut completed_growth = 0.0_f64;
        for occurrence in &occurrences {
            if occurrence.cycle_id != current_cycle {
                continue;
            }
            if current_date.is_some_and(|date| date != occurrence.growth_date) {
                completed_growth += weighted_growth(day_tokens, &day_segments)
                    .map_err(|_| ScanError::InvalidShopState)?;
                day_tokens = 0;
                day_segments.clear();
            }
            current_date = Some(&occurrence.growth_date);
            day_tokens = day_tokens
                .checked_add(occurrence.tokens)
                .ok_or(ScanError::InvalidCount)?;
            day_segments.push(EffectContribution {
                device_id: setting(transaction, "planet_device_id")?.ok_or(ScanError::Database)?,
                cycle_id: occurrence.cycle_id.clone(),
                date: occurrence.growth_date.clone(),
                effect_revision: occurrence.effect_revision,
                tokens: occurrence.tokens,
                growth_bps: occurrence.effects.civilization_growth_bps,
                wallet_bps: occurrence.effects.token_earning_bps,
            });
            let growth = completed_growth
                + weighted_growth(day_tokens, &day_segments)
                    .map_err(|_| ScanError::InvalidShopState)?;
            for (index, threshold) in crate::growth::STAGE_THRESHOLDS.iter().enumerate() {
                let stage = (index + 1) as u8;
                if growth < *threshold {
                    break;
                }
                let trigger_key = format!("era:{}:{stage}", occurrence.cycle_id);
                let existed: bool = transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM shop_era_progress WHERE account_id=?1 AND cycle_id=?2 AND stage=?3)",
                    params![account_id,occurrence.cycle_id,stage],|row|row.get(0),
                )?;
                if existed {
                    continue;
                }
                let effect_snapshot =
                    serde_json::to_string(&occurrence.effects).map_err(|_| ScanError::Database)?;
                let inserted = transaction.execute(
                    "INSERT OR IGNORE INTO shop_era_progress(account_id,cycle_id,stage,trigger_key,
                     effect_snapshot_json,awarded_at_utc) VALUES (?1,?2,?3,?4,?5,?6)",
                    params![
                        account_id,
                        occurrence.cycle_id,
                        stage,
                        trigger_key,
                        effect_snapshot,
                        now.to_rfc3339()
                    ],
                )?;
                if inserted == 0 {
                    continue;
                }
                let amount = occurrence.effects.era_reward_tokens.min(10_000_000);
                if amount > 0 {
                    record_game_reward(
                        transaction,
                        &account_id,
                        &trigger_key,
                        "era",
                        &occurrence.cycle_id,
                        amount,
                        &occurrence.effects,
                        now,
                    )?;
                }
            }
        }

        let days = activity_snapshot(transaction, &account_id)?;
        let active_dates = days
            .iter()
            .map(|day| day.reward_date.clone())
            .collect::<std::collections::BTreeSet<_>>();
        for day in days {
            if day.cycle_id != current_cycle {
                continue;
            }
            let date = NaiveDate::parse_from_str(&day.reward_date, "%Y-%m-%d")
                .map_err(|_| ScanError::Database)?;
            let previous_date = (date - chrono::Duration::days(1))
                .format("%Y-%m-%d")
                .to_string();
            if !active_dates.contains(&previous_date) {
                continue;
            }
            let first = DateTime::parse_from_rfc3339(&day.first_occurred_at_utc)
                .map_err(|_| ScanError::Database)?
                .with_timezone(&Utc);
            let snapshot = effects_at(&histories, first);
            let amount = snapshot.streak_reward_tokens.min(500_000);
            if amount == 0 {
                continue;
            }
            let trigger_key = format!("streak:{}", day.reward_date);
            record_game_reward(
                transaction,
                &account_id,
                &trigger_key,
                "streak",
                &day.cycle_id,
                amount,
                &snapshot,
                now,
            )?;
        }
        Ok(())
    }

    pub fn shop_growth_credit_by_date(
        &self,
    ) -> Result<std::collections::BTreeMap<String, f64>, ScanError> {
        let (daily, _, _) = self.planet_usage_totals()?;
        if !self.guest_import_game_mutations_allowed()? {
            return daily
                .into_iter()
                .map(|(date, tokens)| {
                    Ok((
                        date,
                        weighted_growth(tokens, &[]).map_err(|_| ScanError::InvalidShopState)?,
                    ))
                })
                .collect();
        }
        let account_id = current_account_id(&self.connection)?;
        if account_id.starts_with("account:") {
            return daily
                .into_iter()
                .map(|(date, tokens)| {
                    Ok((
                        date,
                        weighted_growth(tokens, &[]).map_err(|_| ScanError::InvalidShopState)?,
                    ))
                })
                .collect();
        }
        let current_cycle = current_cycle_id(&self.connection)?;
        let mut totals = BTreeMap::new();
        let mut statement = self.connection.prepare(
            "SELECT device_id,cycle_id,date,effect_revision,tokens,growth_bps,wallet_bps
             FROM shop_effect_contribution WHERE account_id=?1 AND cycle_id=?2 AND date=?3
             ORDER BY device_id,cycle_id,effect_revision",
        )?;
        for (date, total_tokens) in daily {
            let rows = statement
                .query_map(params![account_id, current_cycle, date], |row| {
                    Ok(EffectContribution {
                        device_id: row.get(0)?,
                        cycle_id: row.get(1)?,
                        date: row.get(2)?,
                        effect_revision: row.get(3)?,
                        tokens: row.get(4)?,
                        growth_bps: row.get(5)?,
                        wallet_bps: row.get(6)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let contributed = rows.iter().try_fold(0_u64, |sum, segment| {
                sum.checked_add(segment.tokens)
                    .ok_or(ScanError::InvalidCount)
            })?;
            if contributed != 0 && contributed != total_tokens {
                return Err(ScanError::InvalidShopState);
            }
            let credit =
                weighted_growth(total_tokens, &rows).map_err(|_| ScanError::InvalidShopState)?;
            totals.insert(date, credit);
        }
        Ok(totals)
    }

    pub fn settle_guest_cycle_tokens(
        &mut self,
        cycle_id: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, ScanError> {
        self.require_guest_import_game_mutations_allowed()?;
        let transaction = self.connection.transaction()?;
        let amount = Self::settle_guest_cycle_tokens_in_transaction(&transaction, cycle_id, now)?;
        transaction.commit()?;
        Ok(amount)
    }

    pub(crate) fn settle_guest_cycle_tokens_in_transaction(
        transaction: &Transaction<'_>,
        cycle_id: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, ScanError> {
        rebuild_shop_contributions_in_transaction(transaction)?;
        let account_id = current_account_id(transaction)?;
        if account_id.starts_with("account:") {
            return Ok(0);
        }
        let prior: Option<i64> = transaction
            .query_row(
                "SELECT amount FROM shop_cycle_settlement WHERE account_id=?1 AND cycle_id=?2",
                params![account_id, cycle_id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(amount) = prior {
            return u64::try_from(amount).map_err(|_| ScanError::InvalidCount);
        }
        let segments = {
            let mut statement = transaction.prepare(
                "SELECT device_id,cycle_id,date,effect_revision,tokens,growth_bps,wallet_bps
                 FROM shop_effect_contribution WHERE account_id=?1 AND cycle_id=?2
                 ORDER BY device_id,date,effect_revision",
            )?;
            let rows = statement
                .query_map(params![account_id, cycle_id], |row| {
                    Ok(EffectContribution {
                        device_id: row.get(0)?,
                        cycle_id: row.get(1)?,
                        date: row.get(2)?,
                        effect_revision: row.get(3)?,
                        tokens: row.get(4)?,
                        growth_bps: row.get(5)?,
                        wallet_bps: row.get(6)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        let amount = cycle_token_bonus(&segments).map_err(|_| ScanError::InvalidShopState)?;
        transaction.execute(
            "INSERT INTO shop_cycle_settlement(account_id,cycle_id,amount,settled_at_utc) VALUES (?1,?2,?3,?4)",
            params![account_id,cycle_id,to_i64(amount)?,now.to_rfc3339()],
        )?;
        if amount > 0 {
            let effects = ActiveEffects::default();
            record_game_reward(
                transaction,
                &account_id,
                &format!("cycle-token:{cycle_id}"),
                "cycle_token",
                cycle_id,
                amount,
                &effects,
                now,
            )?;
        }
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
             CREATE TABLE IF NOT EXISTS shop_effect_cycle_bound (
                account_id TEXT NOT NULL,
                cycle_id TEXT NOT NULL,
                started_at_utc TEXT NOT NULL,
                ended_at_utc TEXT,
                PRIMARY KEY(account_id,cycle_id)
             );
             CREATE INDEX IF NOT EXISTS shop_effect_cycle_bound_by_time
                ON shop_effect_cycle_bound(account_id,started_at_utc,ended_at_utc);
             CREATE TABLE IF NOT EXISTS shop_effect_cycle_bounds_state (
                account_id TEXT PRIMARY KEY
             );
             CREATE TABLE IF NOT EXISTS shop_effect_timeline_state (
                account_id TEXT PRIMARY KEY,
                current_cycle_id TEXT NOT NULL,
                effect_revision INTEGER NOT NULL CHECK(effect_revision >= 0),
                server_time_utc TEXT NOT NULL,
                reward_timezone TEXT NOT NULL
             );
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

pub(crate) fn shop_device_contribution_from_connection(
    connection: &Connection,
    raw: PlanetDeviceContribution,
) -> Result<PlanetDeviceContributionSnapshot, ScanError> {
    let account_id = current_account_id(connection)?;
    let stored_version: Option<i64> = connection
        .query_row(
            "SELECT canonical_version FROM shop_contribution_state WHERE account_id=?1",
            [&account_id],
            |row| row.get(0),
        )
        .optional()?;
    let canonical_version = to_u64(stored_version.unwrap_or(0))?;
    let device_id = raw.device_id.clone();
    let segments = {
        let mut statement = connection.prepare(
            "SELECT cycle_id,date,effect_revision,tokens FROM shop_effect_contribution
             WHERE account_id=?1 AND device_id=?2
             ORDER BY cycle_id,date,effect_revision",
        )?;
        let rows = statement
            .query_map(params![account_id, device_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let daily_segments = segments
        .into_iter()
        .map(|(cycle_id, date, revision, tokens)| {
            Ok(PlanetEffectContributionSegment {
                cycle_id,
                date,
                effect_revision: to_u64(revision)?,
                tokens: to_u64(tokens)?,
            })
        })
        .collect::<Result<Vec<_>, ScanError>>()?;
    let activity_days = activity_snapshot(connection, &account_id)?
        .into_iter()
        .map(|activity| PlanetActivityDayContribution {
            reward_date: activity.reward_date,
            cycle_id: activity.cycle_id,
            first_occurred_at_utc: activity.first_occurred_at_utc,
            tokens: activity.tokens,
        })
        .collect();
    Ok(PlanetDeviceContributionSnapshot {
        raw,
        canonical_version,
        daily_segments,
        activity_days,
    })
}

pub(crate) fn capture_shop_device_contribution_from_connection(
    connection: &Connection,
    raw: PlanetDeviceContribution,
) -> Result<PlanetDeviceContributionSnapshot, ScanError> {
    rebuild_shop_contributions_in_transaction(connection)?;
    shop_device_contribution_from_connection(connection, raw)
}

pub(crate) fn load_shop_reward_state(
    connection: &Connection,
    account_id: &str,
    cycle_id: &str,
) -> Result<RewardState, ScanError> {
    let reward_timezone: Option<String> = connection
        .query_row(
            "SELECT reward_timezone FROM shop_account_state WHERE account_id=?1",
            [account_id],
            |row| row.get(0),
        )
        .optional()?;
    let reward_timezone = match reward_timezone {
        Some(value) => value,
        None => connection.query_row(
            "SELECT value FROM setting WHERE key='planet_timezone'",
            [],
            |row| row.get(0),
        )?,
    };
    let settled_cycle_tokens: i64 = connection
        .query_row(
            "SELECT amount FROM shop_cycle_settlement WHERE account_id=?1 AND cycle_id=?2",
            params![account_id, cycle_id],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(0);
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
    i64::try_from(value).map_err(|_| ScanError::InvalidCount)
}

fn setting(connection: &Connection, key: &str) -> Result<Option<String>, ScanError> {
    connection
        .query_row("SELECT value FROM setting WHERE key=?1", [key], |row| {
            row.get(0)
        })
        .optional()
        .map_err(Into::into)
}

fn current_account_id(connection: &Connection) -> Result<String, ScanError> {
    setting(connection, "planet_account_id")?.ok_or(ScanError::Database)
}

fn current_cycle_id(connection: &Connection) -> Result<String, ScanError> {
    setting(connection, "planet_current_cycle_id")?.ok_or(ScanError::Database)
}

fn effect_histories(
    connection: &Connection,
    account_id: &str,
) -> Result<Vec<EffectHistory>, ScanError> {
    let mut statement = connection.prepare(
        "SELECT cycle_id,revision,started_at_utc,ended_at_utc,active_instance_ids_json,effects_json
         FROM shop_effect_history WHERE account_id=?1 ORDER BY started_at_utc,revision",
    )?;
    let rows = statement.query_map([account_id], |row| {
        let started: String = row.get(2)?;
        let ended: Option<String> = row.get(3)?;
        let active_instance_ids: String = row.get(4)?;
        let effects: String = row.get(5)?;
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            started,
            ended,
            active_instance_ids,
            effects,
        ))
    })?;
    rows.map(|row| {
        let (cycle_id, revision, started, ended, active_instance_ids, effects) = row?;
        Ok(EffectHistory {
            cycle_id,
            revision: to_u64(revision)?,
            started_at: DateTime::parse_from_rfc3339(&started)
                .map_err(|_| ScanError::Database)?
                .with_timezone(&Utc),
            ended_at: ended
                .map(|value| {
                    DateTime::parse_from_rfc3339(&value)
                        .map(|time| time.with_timezone(&Utc))
                        .map_err(|_| ScanError::Database)
                })
                .transpose()?,
            active_instance_ids: serde_json::from_str(&active_instance_ids)
                .map_err(|_| ScanError::Database)?,
            effects: serde_json::from_str(&effects).map_err(|_| ScanError::Database)?,
        })
    })
    .collect()
}

fn effects_at(histories: &[EffectHistory], at: DateTime<Utc>) -> ActiveEffects {
    histories
        .iter()
        .filter(|history| {
            history.started_at <= at && history.ended_at.is_none_or(|ended| at < ended)
        })
        .max_by_key(|history| (history.started_at, history.revision))
        .map(|history| history.effects.clone())
        .unwrap_or_default()
}

fn canonical_occurrences(connection: &Connection) -> Result<Vec<CanonicalOccurrence>, ScanError> {
    let account_id = current_account_id(connection)?;
    let current_cycle = current_cycle_id(connection)?;
    let activation = setting(connection, "planet_activation_at_utc")?.ok_or(ScanError::Database)?;
    let activation = DateTime::parse_from_rfc3339(&activation)
        .map_err(|_| ScanError::Database)?
        .with_timezone(&Utc);
    let cycle_started = setting(connection, "planet_last_reset_at_utc")?
        .or(setting(connection, "planet_cycle_started_at_utc")?)
        .ok_or(ScanError::Database)?;
    let cycle_started = DateTime::parse_from_rfc3339(&cycle_started)
        .map_err(|_| ScanError::Database)?
        .with_timezone(&Utc)
        .max(activation);
    let device_id = setting(connection, "planet_device_id")?.ok_or(ScanError::Database)?;
    let growth_timezone = setting(connection, "planet_timezone")?
        .ok_or(ScanError::Database)?
        .parse::<Tz>()
        .map_err(|_| ScanError::TimezoneMismatch)?;
    let confirmed_timeline = load_confirmed_timeline(connection, &account_id)?;
    let reward_timezone = match confirmed_timeline.as_ref() {
        Some(state) => state.reward_timezone.clone(),
        None => load_shop_reward_state(connection, &account_id, &current_cycle)?.reward_timezone,
    }
    .parse::<Tz>()
    .map_err(|_| ScanError::TimezoneMismatch)?;
    let legacy_histories;
    let (histories, cycle_bounds) = if let Some(state) = confirmed_timeline.as_ref() {
        (state.intervals.as_slice(), state.cycle_bounds.as_slice())
    } else {
        legacy_histories = effect_histories(connection, &account_id)?;
        (legacy_histories.as_slice(), &[][..])
    };
    let eligible_lineage: Option<String> = connection
        .query_row(
            "SELECT lineage_id FROM guest_provenance_lineage
             WHERE singleton=1 AND eligible=1
               AND device_id=(SELECT value FROM setting WHERE key='planet_device_id')
               AND (SELECT value FROM setting WHERE key='planet_account_id')='local'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let guest_provenance_bounds = if let Some(lineage_id) = eligible_lineage {
        let mut statement = connection.prepare(
            "SELECT cycle_id,started_at_utc,ended_at_utc,baseline_revision
             FROM guest_provenance_cycle WHERE lineage_id=?1 ORDER BY started_at_utc",
        )?;
        let rows = statement.query_map([lineage_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;
        rows.map(|row| {
            let (cycle_id, started_at, ended_at, revision) = row?;
            Ok(GuestProvenanceBound {
                cycle_id,
                started_at: parse_utc(&started_at)?,
                ended_at: ended_at.as_deref().map(parse_utc).transpose()?,
                revision: to_u64(revision)?,
            })
        })
        .collect::<Result<Vec<_>, ScanError>>()?
    } else {
        Vec::new()
    };
    let mut statement = connection.prepare(
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
    let raw = statement
        .query_map([&account_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut occurrences = Vec::new();
    for (event_key, timestamp, tokens) in raw {
        let occurred_at = DateTime::parse_from_rfc3339(&timestamp)
            .map_err(|_| ScanError::Database)?
            .with_timezone(&Utc);
        if occurred_at < activation {
            continue;
        }
        let tokens = to_u64(tokens)?;
        let bound = cycle_bounds.iter().find(|bound| {
            bound.started_at <= occurred_at
                && bound.ended_at.is_none_or(|ended| occurred_at < ended)
        });
        let history = histories
            .iter()
            .filter(|history| {
                history.started_at <= occurred_at
                    && history.ended_at.is_none_or(|ended| occurred_at < ended)
            })
            .max_by_key(|history| (history.started_at, history.revision));
        let guest_bound = guest_provenance_bounds
            .iter()
            .filter(|bound| {
                bound.started_at <= occurred_at
                    && bound.ended_at.is_none_or(|ended| occurred_at < ended)
            })
            .max_by_key(|bound| bound.started_at);
        if occurred_at == activation && guest_bound.is_none() {
            continue;
        }
        let (cycle_id, effect_revision, effects) = match history {
            Some(history) => (
                history.cycle_id.clone(),
                history.revision,
                history.effects.clone(),
            ),
            None if confirmed_timeline.is_some() => match bound {
                Some(bound) => (bound.cycle_id.clone(), 0, ActiveEffects::default()),
                None => continue,
            },
            None if guest_bound.is_some() => {
                let bound = guest_bound.expect("checked guest provenance bound");
                (
                    bound.cycle_id.clone(),
                    bound.revision,
                    ActiveEffects::default(),
                )
            }
            None if occurred_at > cycle_started => {
                (current_cycle.clone(), 0, ActiveEffects::default())
            }
            None => ("baseline".into(), 0, ActiveEffects::default()),
        };
        occurrences.push(CanonicalOccurrence {
            event_key,
            occurred_at,
            growth_date: occurred_at
                .with_timezone(&growth_timezone)
                .format("%Y-%m-%d")
                .to_string(),
            reward_date: occurred_at
                .with_timezone(&reward_timezone)
                .format("%Y-%m-%d")
                .to_string(),
            cycle_id,
            effect_revision,
            tokens,
            effects,
        });
    }
    occurrences.sort_by(|left, right| {
        left.occurred_at
            .cmp(&right.occurred_at)
            .then_with(|| left.event_key.cmp(&right.event_key))
    });
    let _ = device_id;
    Ok(occurrences)
}

fn contribution_snapshot(
    connection: &Connection,
    account_id: &str,
) -> Result<Vec<(ContributionKey, u64, u16, u16)>, ScanError> {
    let mut statement = connection.prepare(
        "SELECT device_id,cycle_id,date,effect_revision,tokens,growth_bps,wallet_bps
         FROM shop_effect_contribution WHERE account_id=?1
         ORDER BY device_id,cycle_id,date,effect_revision",
    )?;
    let rows = statement.query_map([account_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, u16>(5)?,
            row.get::<_, u16>(6)?,
        ))
    })?;
    rows.map(|row| {
        let (device, cycle, date, revision, tokens, growth, wallet) = row?;
        Ok((
            ContributionKey {
                device_id: device,
                cycle_id: cycle,
                date,
                effect_revision: to_u64(revision)?,
            },
            to_u64(tokens)?,
            growth,
            wallet,
        ))
    })
    .collect()
}

fn activity_snapshot(
    connection: &Connection,
    account_id: &str,
) -> Result<Vec<ActivitySnapshot>, ScanError> {
    let mut statement = connection.prepare(
        "SELECT reward_date,cycle_id,first_occurred_at_utc,tokens FROM shop_activity_day
         WHERE account_id=?1 ORDER BY reward_date",
    )?;
    let rows = statement
        .query_map([account_id], |row| {
            Ok(ActivitySnapshot {
                reward_date: row.get(0)?,
                cycle_id: row.get(1)?,
                first_occurred_at_utc: row.get(2)?,
                tokens: row.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into);
    rows
}

fn record_game_reward(
    connection: &Connection,
    account_id: &str,
    trigger_key: &str,
    kind: &str,
    cycle_id: &str,
    amount: u64,
    effects: &ActiveEffects,
    now: DateTime<Utc>,
) -> Result<(), ScanError> {
    let effect_snapshot = serde_json::to_string(effects).map_err(|_| ScanError::Database)?;
    let inserted = connection.execute(
        "INSERT OR IGNORE INTO shop_game_reward(account_id,reward_id,trigger_key,kind,cycle_id,
         amount,effect_snapshot_json,awarded_at_utc) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![
            account_id,
            uuid::Uuid::new_v4().to_string(),
            trigger_key,
            kind,
            cycle_id,
            to_i64(amount)?,
            effect_snapshot,
            now.to_rfc3339()
        ],
    )?;
    if inserted > 0 && amount > 0 {
        connection.execute(
            "INSERT OR IGNORE INTO shop_wallet_credit(account_id,credit_id,trigger_key,cycle_id,amount,created_at_utc)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![account_id,uuid::Uuid::new_v4().to_string(),trigger_key,cycle_id,
                to_i64(amount)?,now.to_rfc3339()],
        )?;
    }
    Ok(())
}

pub(crate) fn apply_confirmed_shop_effect_timeline_in_transaction(
    connection: &Connection,
    timeline: &ShopEffectTimeline,
    expected_account_id: &str,
    expected_cycle_id: &str,
) -> Result<(), ScanError> {
    let actual_account = current_account_id(connection)?;
    let actual_cycle = current_cycle_id(connection)?;
    let expected_account = normalize_account_uuid(expected_account_id)?;
    let timeline_account = normalize_account_uuid(&timeline.account_id)?;
    if !actual_account.starts_with("account:")
        || normalize_account_uuid(&actual_account)? != expected_account
        || timeline_account != expected_account
        || actual_cycle != expected_cycle_id
        || timeline.current_cycle_id != expected_cycle_id
    {
        return Err(ScanError::InvalidShopState);
    }

    let (incoming_cycle_bounds, incoming) = validate_shop_effect_timeline(timeline)?;
    let prior = load_confirmed_timeline(connection, &actual_account)?;
    if let Some(prior) = &prior {
        validate_cycle_bounds_successor(
            prior.cycle_bounds_initialized,
            &prior.cycle_bounds,
            &incoming_cycle_bounds,
            prior.server_time,
        )?;
        validate_timeline_successor(prior, timeline, &incoming_cycle_bounds, &incoming)?;
    }

    if prior
        .as_ref()
        .is_none_or(|prior| prior.effect_revision != timeline.effect_revision)
    {
        connection.execute(
            "DELETE FROM shop_effect_history WHERE account_id=?1",
            [&actual_account],
        )?;
        for interval in &incoming {
            connection.execute(
                "INSERT INTO shop_effect_history(account_id,cycle_id,revision,started_at_utc,
                 ended_at_utc,active_instance_ids_json,effects_json)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![
                    actual_account,
                    interval.cycle_id,
                    to_i64(interval.revision)?,
                    interval.started_at.to_rfc3339(),
                    interval.ended_at.map(|value| value.to_rfc3339()),
                    serde_json::to_string(&interval.active_instance_ids)
                        .map_err(|_| ScanError::Database)?,
                    serde_json::to_string(&interval.effects).map_err(|_| ScanError::Database)?,
                ],
            )?;
        }
    }
    if prior
        .as_ref()
        .is_none_or(|prior| prior.cycle_bounds != incoming_cycle_bounds)
    {
        connection.execute(
            "DELETE FROM shop_effect_cycle_bound WHERE account_id=?1",
            [&actual_account],
        )?;
        for bound in &incoming_cycle_bounds {
            connection.execute(
                "INSERT INTO shop_effect_cycle_bound(account_id,cycle_id,started_at_utc,ended_at_utc)
                 VALUES (?1,?2,?3,?4)",
                params![
                    actual_account,
                    bound.cycle_id,
                    bound.started_at.to_rfc3339(),
                    bound.ended_at.map(|value| value.to_rfc3339()),
                ],
            )?;
        }
    }
    // Row presence records a confirmed server response, including an empty list.
    connection.execute(
        "INSERT OR IGNORE INTO shop_effect_cycle_bounds_state(account_id) VALUES (?1)",
        [&actual_account],
    )?;
    connection.execute(
        "INSERT INTO shop_effect_timeline_state(account_id,current_cycle_id,effect_revision,
         server_time_utc,reward_timezone) VALUES (?1,?2,?3,?4,?5)
         ON CONFLICT(account_id) DO UPDATE SET current_cycle_id=excluded.current_cycle_id,
         effect_revision=excluded.effect_revision,server_time_utc=excluded.server_time_utc,
         reward_timezone=excluded.reward_timezone",
        params![
            actual_account,
            timeline.current_cycle_id,
            to_i64(timeline.effect_revision)?,
            parse_utc(&timeline.server_time_utc)?.to_rfc3339(),
            timeline.reward_timezone
        ],
    )?;
    connection.execute(
        "UPDATE shop_account_state SET reward_timezone=?2 WHERE account_id=?1",
        params![actual_account, timeline.reward_timezone],
    )?;
    rebuild_shop_contributions_in_transaction(connection)?;
    Ok(())
}

pub(crate) fn validate_reset_timeline_current_bound(
    timeline: &ShopEffectTimeline,
    expected_cycle_id: &str,
    expected_started_at_utc: &str,
) -> Result<(), ScanError> {
    let (cycle_bounds, _) = validate_shop_effect_timeline(timeline)?;
    let expected_started_at = DateTime::parse_from_rfc3339(expected_started_at_utc)
        .map_err(|_| ScanError::InvalidShopState)?
        .with_timezone(&Utc);
    let current_bounds = cycle_bounds
        .iter()
        .filter(|bound| bound.cycle_id == expected_cycle_id)
        .collect::<Vec<_>>();
    let Some(current_bound) = current_bounds.first() else {
        return Err(ScanError::InvalidShopState);
    };
    if timeline.current_cycle_id != expected_cycle_id
        || current_bounds.len() != 1
        || cycle_bounds.last().map(|bound| bound.cycle_id.as_str()) != Some(expected_cycle_id)
        || current_bound.ended_at.is_some()
        || current_bound.started_at != expected_started_at
    {
        return Err(ScanError::InvalidShopState);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Ledger;
    use crate::collectors::{ParsedRecord, RecordKind};
    use crate::domain::cosmetic_shop::{
        ActiveEffects, EffectContribution, ResetShopResult, ShopActionResult, ShopActionStatus,
        ShopCycleBound, ShopEffectInterval, ShopEffectTimeline,
    };
    use crate::domain::planet::{
        PlanetAvatar, PlanetObject, PlanetProfile, PlanetState, PlanetWalletCredit,
    };
    use crate::domain::shop_effects::weighted_growth;
    use crate::domain::usage::{Agent, TokenUsage, UsageCoverage};
    use chrono::{DateTime, Utc};
    use chrono_tz::UTC;
    use rusqlite::params;
    use std::collections::BTreeMap;
    use std::path::Path;

    fn ledger() -> Ledger {
        let ledger = Ledger::open(Path::new(":memory:"), UTC).unwrap();
        ledger
            .connection
            .execute(
                "UPDATE setting SET value='2026-09-24T00:00:00+00:00'
             WHERE key IN ('planet_activation_at_utc','planet_cycle_started_at_utc')",
                [],
            )
            .unwrap();
        ledger
    }

    fn signed_timeline(
        account_id: &str,
        cycle_id: &str,
        effect_revision: u64,
        server_time_utc: &str,
        intervals: Vec<ShopEffectInterval>,
    ) -> ShopEffectTimeline {
        ShopEffectTimeline {
            account_id: account_id.into(),
            current_cycle_id: cycle_id.into(),
            effect_revision,
            server_time_utc: server_time_utc.into(),
            reward_timezone: "UTC".into(),
            cycle_bounds: vec![ShopCycleBound {
                cycle_id: cycle_id.into(),
                started_at_utc: "2026-09-24T00:00:00Z".into(),
                ended_at_utc: None,
            }],
            intervals,
        }
    }

    fn effect_interval(
        cycle_id: &str,
        revision: u64,
        started_at_utc: &str,
        ended_at_utc: Option<&str>,
        active_instance_ids: &[&str],
        effects: ActiveEffects,
    ) -> ShopEffectInterval {
        ShopEffectInterval {
            cycle_id: cycle_id.into(),
            revision,
            started_at_utc: started_at_utc.into(),
            ended_at_utc: ended_at_utc.map(str::to_owned),
            active_instance_ids: active_instance_ids.iter().map(|id| (*id).into()).collect(),
            effects,
        }
    }

    fn signed_account(ledger: &mut Ledger) -> (String, String) {
        let account_id = "00000000-0000-0000-0000-000000000031";
        ledger.ensure_planet_account(account_id).unwrap();
        (account_id.into(), ledger.planet_cycle_id().unwrap())
    }

    // Model a database that was already signed in before this fixture's guest
    // provenance was established. Calling ensure_planet_account here would
    // exercise the ownership transition, which intentionally captures and
    // holds an eligible first-reset guest instead.
    fn mark_existing_signed_owner_for_test(ledger: &mut Ledger, user_id: &str) {
        let account_id = format!("account:{user_id}");
        let tx = ledger.connection.transaction().unwrap();
        tx.execute(
            "UPDATE planet_usage_owner SET account_id=?1 WHERE account_id='local'",
            [&account_id],
        )
        .unwrap();
        tx.execute(
            "UPDATE setting SET value=?1 WHERE key='planet_account_id'",
            [&account_id],
        )
        .unwrap();
        tx.execute(
            "INSERT OR IGNORE INTO shop_account_state(account_id,state_revision,reward_timezone)
             VALUES (?1,0,(SELECT value FROM setting WHERE key='planet_timezone'))",
            [&account_id],
        )
        .unwrap();
        tx.commit().unwrap();
    }

    fn one_interval_timeline(
        account_id: &str,
        cycle_id: &str,
        server_time_utc: &str,
        effects: ActiveEffects,
        active_instance_ids: &[&str],
    ) -> ShopEffectTimeline {
        signed_timeline(
            account_id,
            cycle_id,
            1,
            server_time_utc,
            vec![effect_interval(
                cycle_id,
                1,
                "2026-09-24T00:00:00Z",
                None,
                active_instance_ids,
                effects,
            )],
        )
    }

    fn contribution_version(ledger: &Ledger, account_id: &str) -> i64 {
        ledger
            .connection
            .query_row(
                "SELECT canonical_version FROM shop_contribution_state WHERE account_id=?1",
                [format!("account:{account_id}")],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn reset_planet_state(cycle_id: &str, reset_at: &str, old_cycle_id: &str) -> PlanetState {
        PlanetState {
            version: 1,
            profile: Some(PlanetProfile {
                nickname: "Reset account".into(),
                avatar: PlanetAvatar::Feminine,
            }),
            timezone: "UTC".into(),
            current_cycle_id: cycle_id.into(),
            cycle_started_at_utc: reset_at.into(),
            last_reset_at_utc: Some(reset_at.into()),
            wallet_balance: 50_007,
            wallet_credits: vec![
                PlanetWalletCredit {
                    previous_cycle_id: "previous-credit-cycle".into(),
                    amount: 7,
                    created_at_utc: "2026-09-25T00:00:00Z".into(),
                },
                PlanetWalletCredit {
                    previous_cycle_id: old_cycle_id.into(),
                    amount: 50_000,
                    created_at_utc: reset_at.into(),
                },
            ],
            current_planet_tokens: 0,
            lifetime_tokens: 50_000,
            growth_credit: 0.0,
            stage: 0,
            progress_to_next: 0.0,
            incomplete: false,
            can_reset: false,
            reset_available_at_utc: Some("2026-10-02T18:00:00Z".into()),
            objects: vec![PlanetObject {
                stage: 0,
                ordinal: 0,
                kind: "forest".into(),
                x: 8,
                y: 8,
                seed: 42,
            }],
            removed_natural_keys: vec![],
        }
    }

    fn reset_shop_state(
        ledger: &Ledger,
        account_id: &str,
        new_cycle_id: &str,
    ) -> crate::domain::cosmetic_shop::ShopState {
        let mut state = ledger.shop_state().unwrap();
        state.account_id = format!("account:{account_id}");
        state.current_cycle_id = new_cycle_id.into();
        state.state_revision = 2;
        state.available_balance = 50_007;
        state.placements.clear();
        state.effects = ActiveEffects::default();
        state.removed_natural_keys.clear();
        state
    }

    fn reset_timeline(
        account_id: &str,
        old_cycle_id: &str,
        new_cycle_id: &str,
    ) -> ShopEffectTimeline {
        ShopEffectTimeline {
            account_id: account_id.into(),
            current_cycle_id: new_cycle_id.into(),
            effect_revision: 2,
            server_time_utc: "2026-10-02T00:05:00Z".into(),
            reward_timezone: "UTC".into(),
            cycle_bounds: vec![
                ShopCycleBound {
                    cycle_id: old_cycle_id.into(),
                    started_at_utc: "2026-09-24T00:00:00Z".into(),
                    ended_at_utc: Some("2026-10-02T00:00:00Z".into()),
                },
                ShopCycleBound {
                    cycle_id: new_cycle_id.into(),
                    started_at_utc: "2026-10-02T00:00:00Z".into(),
                    ended_at_utc: None,
                },
            ],
            intervals: vec![
                effect_interval(
                    old_cycle_id,
                    1,
                    "2026-09-24T00:00:00Z",
                    Some("2026-10-02T00:00:00Z"),
                    &[],
                    ActiveEffects::default(),
                ),
                effect_interval(
                    new_cycle_id,
                    2,
                    "2026-10-02T00:00:00Z",
                    None,
                    &[],
                    ActiveEffects::default(),
                ),
            ],
        }
    }

    #[test]
    fn confirmed_reset_rejects_a_timeline_without_the_new_cycle_bound() {
        let mut ledger = ledger();
        let (account_id, old_cycle_id) = signed_account(&mut ledger);
        ledger
            .set_planet_profile("Keep source profile", PlanetAvatar::Masculine)
            .unwrap();
        add_event(
            &mut ledger,
            "unbounded-reset-source",
            "2026-10-01T12:00:00Z",
            12,
        );

        let new_cycle_id = "reset-cycle-without-bound";
        let reset_at = "2026-10-02T00:00:00Z";
        let state = reset_shop_state(&ledger, &account_id, new_cycle_id);
        let result = ResetShopResult {
            action: ShopActionResult {
                status: ShopActionStatus::Reset,
                request_id: "reset-missing-bound".into(),
                confirmed_quote: None,
                state,
            },
            planet_state: reset_planet_state(new_cycle_id, reset_at, &old_cycle_id),
        };
        let timeline = ShopEffectTimeline {
            account_id: account_id.clone(),
            current_cycle_id: new_cycle_id.into(),
            effect_revision: 1,
            server_time_utc: "2026-10-02T00:05:00Z".into(),
            reward_timezone: "UTC".into(),
            cycle_bounds: vec![],
            intervals: vec![],
        };

        assert_eq!(
            ledger.apply_confirmed_reset_result(&result, &timeline, &account_id, &old_cycle_id),
            Err(crate::storage::ledger::ScanError::InvalidShopState),
        );
        assert_eq!(ledger.planet_cycle_id().unwrap(), old_cycle_id);
        assert_eq!(
            ledger.planet_profile().unwrap().unwrap().nickname,
            "Keep source profile"
        );
        assert_eq!(
            ledger
                .connection
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM usage_record WHERE event_key=?1",
                    ["unbounded-reset-source"],
                    |row| row.get(0),
                )
                .unwrap(),
            1
        );
        assert_eq!(
            ledger
                .connection
                .query_row::<i64, _, _>("SELECT count(*) FROM shop_remote_state", [], |row| row
                    .get(0),)
                .unwrap(),
            0
        );
    }

    #[test]
    fn confirmed_reset_cache_failure_rolls_back_planet_shop_and_timeline_together() {
        let mut ledger = ledger();
        let (account_id, old_cycle_id) = signed_account(&mut ledger);
        ledger
            .set_planet_profile("Keep my avatar", PlanetAvatar::Masculine)
            .unwrap();
        add_event(
            &mut ledger,
            "reset-source-event",
            "2026-09-25T12:00:00Z",
            50_000,
        );
        ledger
            .connection
            .execute(
                "INSERT INTO planet_object(cycle_id,stage,ordinal,kind,x,y,seed)
             VALUES (?1,0,0,'old-natural',1,2,'1')",
                [&old_cycle_id],
            )
            .unwrap();
        ledger
            .connection
            .execute(
                "INSERT INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
             VALUES ('previous-credit-cycle',7,'2026-09-25T00:00:00Z')",
                [],
            )
            .unwrap();
        let account_key = format!("account:{account_id}");
        ledger.connection.execute(
            "INSERT INTO shop_landscape_instance(account_id,instance_id,sku,variation_index,seed,
             variation_version,acquired_at_utc) VALUES (?1,'owned-tree','land_tree',0,'seed-tree',1,
             '2026-09-25T00:00:00Z')",
            [&account_key],
        ).unwrap();
        ledger
            .connection
            .execute(
                "INSERT INTO shop_landscape_placement(account_id,instance_id,cycle_id,x,y,version)
             VALUES (?1,'owned-tree',?2,12,34,1)",
                params![account_key, old_cycle_id],
            )
            .unwrap();
        ledger
            .connection
            .execute(
                "INSERT INTO shop_avatar_owned(account_id,sku,purchase_id,price,acquired_at_utc)
             VALUES (?1,'avatar_crown','avatar-crown-purchase',200000000,'2026-09-25T00:00:00Z')",
                [&account_key],
            )
            .unwrap();
        ledger
            .connection
            .execute(
                "INSERT INTO shop_avatar_equipment(account_id,slot,sku,version)
             VALUES (?1,'head','avatar_crown',3)",
                [&account_key],
            )
            .unwrap();

        let mut old_shop_state = ledger.shop_state().unwrap();
        old_shop_state.state_revision = 1;
        ledger.store_confirmed_shop_state(&old_shop_state).unwrap();
        let old_timeline = one_interval_timeline(
            &account_id,
            &old_cycle_id,
            "2026-10-01T00:00:00Z",
            ActiveEffects::default(),
            &[],
        );
        ledger
            .apply_confirmed_shop_effect_timeline(&old_timeline, &account_id, &old_cycle_id)
            .unwrap();

        let new_cycle_id = "server-generated-reset-cycle";
        let reset_at = "2026-10-02T00:00:00+00:00";
        let mut new_shop_state = reset_shop_state(&ledger, &account_id, new_cycle_id);
        new_shop_state.landscape_instances = old_shop_state.landscape_instances.clone();
        let reset_result = ResetShopResult {
            action: ShopActionResult {
                status: ShopActionStatus::Reset,
                request_id: "reset-cache-atomicity".into(),
                confirmed_quote: None,
                state: new_shop_state,
            },
            planet_state: reset_planet_state(new_cycle_id, reset_at, &old_cycle_id),
        };
        let timeline = reset_timeline(&account_id, &old_cycle_id, new_cycle_id);

        let before_planet_settings: Vec<(String, String)> = ledger.connection.prepare(
            "SELECT key,value FROM setting WHERE key IN ('planet_current_cycle_id',
             'planet_cycle_started_at_utc','planet_last_reset_at_utc','planet_reset_available_at_utc',
             'planet_remote_cycle_id','planet_remote_current_tokens','planet_remote_lifetime_tokens')
             ORDER BY key",
        ).unwrap().query_map([], |row| Ok((row.get(0)?,row.get(1)?))).unwrap()
            .collect::<Result<_,_>>().unwrap();
        let before_shop_state: String = ledger
            .connection
            .query_row(
                "SELECT state_json FROM shop_remote_state WHERE account_id=?1",
                [&account_key],
                |row| row.get(0),
            )
            .unwrap();
        let before_timeline: (String, i64, String) = ledger
            .connection
            .query_row(
                "SELECT current_cycle_id,effect_revision,server_time_utc
             FROM shop_effect_timeline_state WHERE account_id=?1",
                [&account_key],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let before_wallet: Vec<(String, i64, String)> = ledger.connection.prepare(
            "SELECT previous_cycle_id,amount,created_at_utc FROM planet_wallet_credit ORDER BY previous_cycle_id",
        ).unwrap().query_map([], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).unwrap()
            .collect::<Result<_,_>>().unwrap();
        let before_objects: Vec<(String, i64, i64, String)> = ledger.connection.prepare(
            "SELECT cycle_id,stage,ordinal,kind FROM planet_object ORDER BY cycle_id,stage,ordinal",
        ).unwrap().query_map([], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).unwrap()
            .collect::<Result<_,_>>().unwrap();
        let before_owned: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM shop_landscape_instance WHERE account_id=?1",
                [&account_key],
                |row| row.get(0),
            )
            .unwrap();
        let before_avatar: (i64, Option<String>, i64) = ledger
            .connection
            .query_row(
                "SELECT (SELECT count(*) FROM shop_avatar_owned WHERE account_id=?1),
             (SELECT sku FROM shop_avatar_equipment WHERE account_id=?1 AND slot='head'),
             (SELECT version FROM shop_avatar_equipment WHERE account_id=?1 AND slot='head')",
                [&account_key],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let before_source_count: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM usage_record WHERE event_key='reset-source-event'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(
            ledger.apply_confirmed_reset_result(
                &reset_result,
                &timeline,
                "00000000-0000-0000-0000-000000000032",
                &old_cycle_id,
            ),
            Err(crate::storage::ledger::ScanError::InvalidShopState),
        );
        assert_eq!(
            ledger.apply_confirmed_reset_result(
                &reset_result,
                &timeline,
                &account_id,
                "stale-old-cycle",
            ),
            Err(crate::storage::ledger::ScanError::InvalidShopState),
        );

        let mut shifted_cycle_start = timeline.clone();
        shifted_cycle_start
            .cycle_bounds
            .last_mut()
            .unwrap()
            .started_at_utc = "2026-10-02T00:01:00Z".into();
        shifted_cycle_start
            .intervals
            .last_mut()
            .unwrap()
            .started_at_utc = "2026-10-02T00:01:00Z".into();
        assert_eq!(
            ledger.apply_confirmed_reset_result(
                &reset_result,
                &shifted_cycle_start,
                &account_id,
                &old_cycle_id,
            ),
            Err(crate::storage::ledger::ScanError::InvalidShopState),
            "effect bounds must begin at the authoritative reset instant",
        );

        let mut missing_current_bound = timeline.clone();
        missing_current_bound.cycle_bounds.clear();
        assert_eq!(
            ledger.apply_confirmed_reset_result(
                &reset_result,
                &missing_current_bound,
                &account_id,
                &old_cycle_id,
            ),
            Err(crate::storage::ledger::ScanError::InvalidShopState),
            "a reset response must include its new cycle bound",
        );
        assert_eq!(ledger.planet_cycle_id().unwrap(), old_cycle_id);
        let unchanged_shop_state: String = ledger
            .connection
            .query_row(
                "SELECT state_json FROM shop_remote_state WHERE account_id=?1",
                [&account_key],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(unchanged_shop_state, before_shop_state);
        let unchanged_timeline: (String, i64, String) = ledger
            .connection
            .query_row(
                "SELECT current_cycle_id,effect_revision,server_time_utc
             FROM shop_effect_timeline_state WHERE account_id=?1",
                [&account_key],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(unchanged_timeline, before_timeline);
        let unchanged_source_count: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM usage_record WHERE event_key=?1",
                ["reset-source-event"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(unchanged_source_count, before_source_count);

        ledger
            .connection
            .execute_batch(
                "CREATE TRIGGER fail_confirmed_reset_timeline
             BEFORE INSERT ON shop_effect_history
             WHEN NEW.cycle_id='server-generated-reset-cycle'
             BEGIN SELECT RAISE(ABORT, 'injected timeline failure'); END;",
            )
            .unwrap();
        assert_eq!(
            ledger.apply_confirmed_reset_result(
                &reset_result,
                &timeline,
                &account_id,
                &old_cycle_id,
            ),
            Err(crate::storage::ledger::ScanError::Database),
        );

        let after_planet_settings: Vec<(String, String)> = ledger.connection.prepare(
            "SELECT key,value FROM setting WHERE key IN ('planet_current_cycle_id',
             'planet_cycle_started_at_utc','planet_last_reset_at_utc','planet_reset_available_at_utc',
             'planet_remote_cycle_id','planet_remote_current_tokens','planet_remote_lifetime_tokens')
             ORDER BY key",
        ).unwrap().query_map([], |row| Ok((row.get(0)?,row.get(1)?))).unwrap()
            .collect::<Result<_,_>>().unwrap();
        let after_shop_state: String = ledger
            .connection
            .query_row(
                "SELECT state_json FROM shop_remote_state WHERE account_id=?1",
                [&account_key],
                |row| row.get(0),
            )
            .unwrap();
        let after_timeline: (String, i64, String) = ledger
            .connection
            .query_row(
                "SELECT current_cycle_id,effect_revision,server_time_utc
             FROM shop_effect_timeline_state WHERE account_id=?1",
                [&account_key],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let after_wallet: Vec<(String, i64, String)> = ledger.connection.prepare(
            "SELECT previous_cycle_id,amount,created_at_utc FROM planet_wallet_credit ORDER BY previous_cycle_id",
        ).unwrap().query_map([], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).unwrap()
            .collect::<Result<_,_>>().unwrap();
        let after_objects: Vec<(String, i64, i64, String)> = ledger.connection.prepare(
            "SELECT cycle_id,stage,ordinal,kind FROM planet_object ORDER BY cycle_id,stage,ordinal",
        ).unwrap().query_map([], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).unwrap()
            .collect::<Result<_,_>>().unwrap();
        let after_owned: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM shop_landscape_instance WHERE account_id=?1",
                [&account_key],
                |row| row.get(0),
            )
            .unwrap();
        let after_avatar: (i64, Option<String>, i64) = ledger
            .connection
            .query_row(
                "SELECT (SELECT count(*) FROM shop_avatar_owned WHERE account_id=?1),
             (SELECT sku FROM shop_avatar_equipment WHERE account_id=?1 AND slot='head'),
             (SELECT version FROM shop_avatar_equipment WHERE account_id=?1 AND slot='head')",
                [&account_key],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let after_source_count: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM usage_record WHERE event_key='reset-source-event'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(ledger.planet_cycle_id().unwrap(), old_cycle_id);
        assert_eq!(after_planet_settings, before_planet_settings);
        assert_eq!(after_shop_state, before_shop_state);
        assert_eq!(after_timeline, before_timeline);
        assert_eq!(after_wallet, before_wallet);
        assert_eq!(after_objects, before_objects);
        assert_eq!(after_owned, before_owned);
        assert_eq!(after_avatar, before_avatar);
        assert_eq!(after_source_count, before_source_count);

        ledger
            .connection
            .execute_batch("DROP TRIGGER fail_confirmed_reset_timeline;")
            .unwrap();
        ledger
            .apply_confirmed_reset_result(&reset_result, &timeline, &account_id, &old_cycle_id)
            .unwrap();

        assert_eq!(ledger.planet_cycle_id().unwrap(), new_cycle_id);
        assert_eq!(ledger.planet_cycle_started_at().unwrap(), reset_at);
        assert_eq!(
            ledger.last_reset_at().unwrap().unwrap().to_rfc3339(),
            "2026-10-02T00:00:00+00:00"
        );
        assert_eq!(
            ledger.reset_available_at().unwrap().unwrap().to_rfc3339(),
            "2026-10-02T18:00:00+00:00"
        );
        assert_eq!(
            ledger.planet_profile().unwrap().unwrap().nickname,
            "Keep my avatar"
        );
        assert_eq!(
            ledger.planet_profile().unwrap().unwrap().avatar,
            PlanetAvatar::Masculine
        );
        assert_eq!(
            ledger.planet_usage_totals().unwrap(),
            (BTreeMap::new(), 0, 50_000)
        );
        assert_eq!(
            ledger.planet_wallet_credits().unwrap(),
            reset_result.planet_state.wallet_credits
        );
        assert_eq!(ledger.shop_state().unwrap().current_cycle_id, new_cycle_id);
        assert!(ledger.shop_state().unwrap().placements.is_empty());
        assert_eq!(ledger.shop_state().unwrap().landscape_instances.len(), 1);
        assert_eq!(
            ledger.shop_state().unwrap().avatar_owned_skus,
            vec!["avatar_crown"]
        );
        assert_eq!(
            ledger
                .shop_state()
                .unwrap()
                .avatar_equipment
                .head
                .sku
                .as_deref(),
            Some("avatar_crown")
        );
        let object_rows: Vec<(String, i64, String)> = ledger
            .connection
            .prepare("SELECT cycle_id,ordinal,kind FROM planet_object ORDER BY cycle_id,ordinal")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(object_rows, vec![(new_cycle_id.into(), 0, "forest".into())]);
        let current_timeline: (String, i64) = ledger.connection.query_row(
            "SELECT current_cycle_id,effect_revision FROM shop_effect_timeline_state WHERE account_id=?1",
            [&account_key], |row| Ok((row.get(0)?,row.get(1)?)),
        ).unwrap();
        assert_eq!(current_timeline, (new_cycle_id.into(), 2));
        let contribution = ledger.shop_device_contribution(false).unwrap();
        assert_eq!(contribution.raw.current_planet_tokens, 0);
        assert_eq!(contribution.raw.lifetime_tokens, 50_000);
        assert_eq!(contribution.daily_segments.len(), 1);
        assert_eq!(contribution.daily_segments[0].cycle_id, old_cycle_id);
        assert_eq!(contribution.daily_segments[0].tokens, 50_000);
        assert_eq!(
            ledger
                .connection
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM usage_record WHERE event_key='reset-source-event'",
                    [],
                    |row| row.get(0),
                )
                .unwrap(),
            1
        );
    }

    fn add_event(ledger: &mut Ledger, key: &str, at: &str, tokens: u64) {
        ledger
            .insert(&ParsedRecord {
                agent: Agent::Codex,
                kind: RecordKind::Response,
                event_key: key.into(),
                occurred_at_utc: DateTime::parse_from_rfc3339(at)
                    .unwrap()
                    .with_timezone(&Utc),
                usage: TokenUsage {
                    input_tokens: None,
                    output_tokens: None,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    total_tokens: Some(tokens),
                    coverage: UsageCoverage::Complete,
                },
            })
            .unwrap();
    }

    fn add_effect_history(ledger: &Ledger, revision: i64, started: &str, effects: ActiveEffects) {
        let account: String = ledger
            .connection
            .query_row(
                "SELECT value FROM setting WHERE key='planet_account_id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let cycle: String = ledger
            .connection
            .query_row(
                "SELECT value FROM setting WHERE key='planet_current_cycle_id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        ledger
            .connection
            .execute(
                "INSERT INTO shop_effect_history(account_id,cycle_id,revision,started_at_utc,
             active_instance_ids_json,effects_json) VALUES (?1,?2,?3,?4,'[\"fixture\"]',?5)",
                params![
                    account,
                    cycle,
                    revision,
                    started,
                    serde_json::to_string(&effects).unwrap()
                ],
            )
            .unwrap();
    }

    fn contribution_rows(ledger: &Ledger, date: &str) -> Vec<EffectContribution> {
        let account: String = ledger
            .connection
            .query_row(
                "SELECT value FROM setting WHERE key='planet_account_id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let mut statement = ledger.connection.prepare(
            "SELECT device_id,cycle_id,date,effect_revision,tokens,growth_bps,wallet_bps
             FROM shop_effect_contribution WHERE account_id=?1 AND date=?2 ORDER BY effect_revision",
        ).unwrap();
        statement
            .query_map(params![account, date], |row| {
                Ok(EffectContribution {
                    device_id: row.get(0)?,
                    cycle_id: row.get(1)?,
                    date: row.get(2)?,
                    effect_revision: row.get(3)?,
                    tokens: row.get(4)?,
                    growth_bps: row.get(5)?,
                    wallet_bps: row.get(6)?,
                })
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }

    #[test]
    fn rebuild_uses_occurrence_time_and_replaces_late_or_corrected_canonical_rows() {
        let mut ledger = ledger();
        let current_cycle = ledger.planet_cycle_id().unwrap();
        add_event(&mut ledger, "before-effect", "2026-09-25T06:00:00Z", 50_000);
        add_effect_history(
            &ledger,
            1,
            "2026-09-25T12:00:00+00:00",
            ActiveEffects {
                civilization_growth_bps: 2_000,
                token_earning_bps: 100,
                ..ActiveEffects::default()
            },
        );
        add_event(
            &mut ledger,
            "at-effect-start",
            "2026-09-25T12:00:00Z",
            50_000,
        );
        add_event(&mut ledger, "after-effect", "2026-09-25T18:00:00Z", 50_000);

        let occurrences = super::canonical_occurrences(&ledger.connection).unwrap();
        let before_effect = occurrences
            .iter()
            .find(|item| item.event_key == "before-effect")
            .unwrap();
        assert_eq!(before_effect.cycle_id, current_cycle);
        assert_eq!(before_effect.effect_revision, 0);
        assert_eq!(before_effect.effects, ActiveEffects::default());
        let at_effect_start = occurrences
            .iter()
            .find(|item| item.event_key == "at-effect-start")
            .unwrap();
        assert_eq!(at_effect_start.cycle_id, current_cycle);
        assert_eq!(at_effect_start.effect_revision, 1);
        assert_eq!(
            at_effect_start.effects,
            ActiveEffects {
                civilization_growth_bps: 2_000,
                token_earning_bps: 100,
                ..ActiveEffects::default()
            }
        );
        let after_effect = occurrences
            .iter()
            .find(|item| item.event_key == "after-effect")
            .unwrap();
        assert_eq!(after_effect.cycle_id, current_cycle);
        assert_eq!(after_effect.effect_revision, 1);
        assert_eq!(
            after_effect.effects,
            ActiveEffects {
                civilization_growth_bps: 2_000,
                token_earning_bps: 100,
                ..ActiveEffects::default()
            }
        );

        ledger.rebuild_shop_contributions().unwrap();
        let segments = contribution_rows(&ledger, "2026-09-25");
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].tokens, 50_000);
        assert_eq!(
            segments[1].tokens, 100_000,
            "usage at the interval start belongs to revision 1"
        );
        assert_eq!(
            segments[0].growth_bps, 0,
            "the effect must not apply before activation"
        );
        assert_eq!(segments[1].growth_bps, 2_000);
        assert!((weighted_growth(150_000, &segments).unwrap() - 1.4981851742056773).abs() < 1e-12);

        add_event(
            &mut ledger,
            "late-before-effect",
            "2026-09-25T09:00:00Z",
            1_000,
        );
        ledger.rebuild_shop_contributions().unwrap();
        let segments = contribution_rows(&ledger, "2026-09-25");
        assert_eq!(
            segments.iter().map(|segment| segment.tokens).sum::<u64>(),
            151_000
        );
        assert_eq!(
            segments
                .iter()
                .find(|segment| segment.effect_revision == 0)
                .unwrap()
                .tokens,
            51_000
        );
        assert_eq!(
            segments
                .iter()
                .find(|segment| segment.effect_revision == 1)
                .unwrap()
                .tokens,
            100_000
        );

        add_event(&mut ledger, "after-effect", "2026-09-25T18:00:00Z", 25_000);
        ledger.rebuild_shop_contributions().unwrap();
        let corrected = contribution_rows(&ledger, "2026-09-25");
        assert_eq!(
            corrected.iter().map(|segment| segment.tokens).sum::<u64>(),
            126_000
        );
        assert_eq!(
            corrected
                .iter()
                .find(|segment| segment.effect_revision == 1)
                .unwrap()
                .tokens,
            75_000
        );
        let contribution_versions: i64 = ledger
            .connection
            .query_row(
                "SELECT count(DISTINCT canonical_version) FROM shop_effect_contribution",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            contribution_versions, 1,
            "replacement keeps one canonical set, not correction duplicates"
        );

        ledger.set_agent_enabled(Agent::Codex, false).unwrap();
        ledger.rebuild_shop_contributions().unwrap();
        let remaining: i64 = ledger
            .connection
            .query_row("SELECT count(*) FROM shop_effect_contribution", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(
            remaining, 0,
            "disabled source rows must be removed from canonical effects"
        );
    }

    #[test]
    fn same_day_reset_growth_uses_only_the_current_cycle_contributions() {
        let mut ledger = ledger();
        add_effect_history(
            &ledger,
            1,
            "2026-09-24T00:00:00Z",
            ActiveEffects {
                civilization_growth_bps: 2_000,
                ..ActiveEffects::default()
            },
        );
        add_event(
            &mut ledger,
            "same-day-old-cycle",
            "2026-09-25T08:00:00Z",
            50_000,
        );
        let old_cycle = ledger.planet_cycle_id().unwrap();
        let reset_at = DateTime::parse_from_rfc3339("2026-09-25T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let reset = ledger
            .reset_guest_planet("same-day-cycle-reset", &old_cycle, reset_at)
            .unwrap();
        assert_eq!(
            reset.status,
            crate::domain::cosmetic_shop::ShopActionStatus::Reset
        );

        add_event(
            &mut ledger,
            "same-day-new-cycle",
            "2026-09-25T18:00:00Z",
            25_000,
        );
        ledger.rebuild_shop_contributions().unwrap();
        let current_cycle = ledger.planet_cycle_id().unwrap();
        let same_date = contribution_rows(&ledger, "2026-09-25");
        assert!(same_date
            .iter()
            .any(|segment| segment.cycle_id == old_cycle));
        assert!(same_date
            .iter()
            .any(|segment| segment.cycle_id == current_cycle));
        assert_eq!(
            ledger.planet_usage_totals().unwrap().0["2026-09-25"],
            25_000
        );

        let growth = ledger.shop_growth_credit_by_date().unwrap();
        let expected = weighted_growth(25_000, &[]).unwrap();
        assert!((growth["2026-09-25"] - expected).abs() < 1e-12);
    }

    #[test]
    fn raw_usage_without_effect_history_before_cycle_start_uses_baseline_revision_zero() {
        let mut ledger = ledger();
        ledger
            .connection
            .execute(
                "UPDATE setting SET value='2026-09-26T00:00:00Z'
             WHERE key IN ('planet_last_reset_at_utc','planet_cycle_started_at_utc')",
                [],
            )
            .unwrap();
        let current_cycle = ledger.planet_cycle_id().unwrap();
        add_event(
            &mut ledger,
            "untracked-old-cycle",
            "2026-09-25T12:00:00Z",
            1_000,
        );
        add_event(
            &mut ledger,
            "untracked-current-cycle",
            "2026-09-27T12:00:00Z",
            2_000,
        );

        let occurrences = super::canonical_occurrences(&ledger.connection).unwrap();
        assert_eq!(occurrences.len(), 2);
        let old = occurrences
            .iter()
            .find(|item| item.event_key == "untracked-old-cycle")
            .unwrap();
        assert_eq!(old.cycle_id, "baseline");
        assert_eq!(old.effect_revision, 0);
        assert_eq!(old.effects, ActiveEffects::default());
        let current = occurrences
            .iter()
            .find(|item| item.event_key == "untracked-current-cycle")
            .unwrap();
        assert_eq!(current.cycle_id, current_cycle);
        assert_eq!(current.effect_revision, 0);
        assert_eq!(current.effects, ActiveEffects::default());
    }

    #[test]
    fn shop_device_contribution_reads_stored_all_cycle_segments_and_activity_without_mutation() {
        let mut ledger = ledger();
        add_effect_history(&ledger, 1, "2026-09-24T00:00:00Z", ActiveEffects::default());
        add_event(
            &mut ledger,
            "snapshot-old-cycle",
            "2026-09-25T08:00:00Z",
            50_000,
        );
        let old_cycle = ledger.planet_cycle_id().unwrap();
        let reset_at = DateTime::parse_from_rfc3339("2026-09-25T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        ledger
            .reset_guest_planet("snapshot-cycle-reset", &old_cycle, reset_at)
            .unwrap();
        let current_cycle = ledger.planet_cycle_id().unwrap();
        add_effect_history(&ledger, 1, "2026-09-25T12:00:00Z", ActiveEffects::default());
        add_event(
            &mut ledger,
            "snapshot-current-cycle",
            "2026-09-25T18:00:00Z",
            25_000,
        );
        ledger.rebuild_shop_contributions().unwrap();

        let account: String = ledger
            .connection
            .query_row(
                "SELECT value FROM setting WHERE key='planet_account_id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let device: String = ledger
            .connection
            .query_row(
                "SELECT value FROM setting WHERE key='planet_device_id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
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

        let stored_version: i64 = ledger
            .connection
            .query_row(
                "SELECT canonical_version FROM shop_contribution_state WHERE account_id=?1",
                [&account],
                |row| row.get(0),
            )
            .unwrap();
        let before_rows: (i64, i64) = ledger
            .connection
            .query_row(
                "SELECT (SELECT count(*) FROM shop_effect_contribution WHERE account_id=?1),
                    (SELECT count(*) FROM shop_activity_day WHERE account_id=?1)",
                [&account],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        let contribution = ledger.shop_device_contribution(false).unwrap();
        let repeated = ledger.shop_device_contribution(false).unwrap();
        let value = serde_json::to_value(&contribution).unwrap();
        assert_eq!(value, serde_json::to_value(&repeated).unwrap());
        assert_eq!(contribution.canonical_version, stored_version as u64);
        assert_eq!(contribution.raw.device_id, device);
        assert_eq!(contribution.raw.current_cycle_id, current_cycle);
        assert_eq!(contribution.raw.current_planet_tokens, 25_000);
        assert_eq!(contribution.raw.lifetime_tokens, 75_000);
        assert_eq!(
            contribution.raw.daily_tokens.get("2026-09-25"),
            Some(&25_000)
        );

        let mut expected_segments = vec![
            (
                old_cycle.clone(),
                serde_json::json!({
                    "cycle_id": old_cycle,
                    "date": "2026-09-25",
                    "effect_revision": 1,
                    "tokens": 50_000,
                }),
            ),
            (
                current_cycle.clone(),
                serde_json::json!({
                    "cycle_id": current_cycle,
                    "date": "2026-09-25",
                    "effect_revision": 2,
                    "tokens": 25_000,
                }),
            ),
        ];
        expected_segments.sort_by(|left, right| left.0.cmp(&right.0));
        let expected_segments: Vec<_> = expected_segments
            .into_iter()
            .map(|(_, segment)| segment)
            .collect();
        assert_eq!(
            value["daily_segments"],
            serde_json::json!(expected_segments)
        );
        assert_eq!(
            value["activity_days"],
            serde_json::json!([{
                "reward_date": "2026-09-25",
                "cycle_id": old_cycle,
                "first_occurred_at_utc": "2026-09-25T08:00:00+00:00",
                "tokens": 75_000,
            }])
        );
        let mut top_level_keys = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        top_level_keys.sort_unstable();
        assert_eq!(
            top_level_keys,
            vec![
                "activity_days",
                "canonical_version",
                "current_cycle_id",
                "current_planet_tokens",
                "daily_segments",
                "daily_tokens",
                "device_id",
                "incomplete",
                "lifetime_tokens",
            ]
        );
        for segment in value["daily_segments"].as_array().unwrap() {
            let mut keys = segment
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>();
            keys.sort_unstable();
            assert_eq!(keys, vec!["cycle_id", "date", "effect_revision", "tokens"]);
        }
        for activity in value["activity_days"].as_array().unwrap() {
            let mut keys = activity
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>();
            keys.sort_unstable();
            assert_eq!(
                keys,
                vec!["cycle_id", "first_occurred_at_utc", "reward_date", "tokens"]
            );
        }
        let after_version: i64 = ledger
            .connection
            .query_row(
                "SELECT canonical_version FROM shop_contribution_state WHERE account_id=?1",
                [&account],
                |row| row.get(0),
            )
            .unwrap();
        let after_rows: (i64, i64) = ledger
            .connection
            .query_row(
                "SELECT (SELECT count(*) FROM shop_effect_contribution WHERE account_id=?1),
                    (SELECT count(*) FROM shop_activity_day WHERE account_id=?1)",
                [&account],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(after_version, stored_version);
        assert_eq!(after_rows, before_rows);

        // New raw events do not silently rewrite the persisted canonical version during a read.
        add_event(
            &mut ledger,
            "snapshot-not-yet-rebuilt",
            "2026-09-25T20:00:00Z",
            5_000,
        );
        let pending = ledger.shop_device_contribution(false).unwrap();
        assert_eq!(pending.canonical_version, stored_version as u64);
        assert_eq!(pending.raw.current_planet_tokens, 30_000);
        assert_eq!(pending.daily_segments, contribution.daily_segments);
        let still_stored_version: i64 = ledger
            .connection
            .query_row(
                "SELECT canonical_version FROM shop_contribution_state WHERE account_id=?1",
                [&account],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(still_stored_version, stored_version);
    }

    #[test]
    fn confirmed_effect_timeline_rebuilds_signed_usage_from_server_intervals_without_rewards() {
        let mut ledger = ledger();
        let user_id = "00000000-0000-0000-0000-000000000031";
        ledger.ensure_planet_account(user_id).unwrap();
        let cycle_id = ledger.planet_cycle_id().unwrap();
        add_event(
            &mut ledger,
            "signed-timeline-event",
            "2026-09-25T12:00:00Z",
            100_000,
        );
        let timeline: ShopEffectTimeline = serde_json::from_value(serde_json::json!({
            "account_id": user_id,
            "current_cycle_id": cycle_id,
            "effect_revision": 1,
            "server_time_utc": "2026-10-01T00:00:00Z",
            "reward_timezone": "UTC",
            "cycle_bounds": [{
                "cycle_id": cycle_id,
                "started_at_utc": "2026-09-24T00:00:00Z",
                "ended_at_utc": null
            }],
            "intervals": [{
                "cycle_id": cycle_id,
                "revision": 1,
                "started_at_utc": "2026-09-24T00:00:00Z",
                "ended_at_utc": null,
                "active_instance_ids": [],
                "effects": {
                    "token_earning_bps": 100,
                    "civilization_growth_bps": 1_000,
                    "shop_discount_bps": 0,
                    "reset_cooldown_bps": 0,
                    "natural_removal_discount_bps": 0,
                    "era_reward_tokens": 0,
                    "streak_reward_tokens": 0,
                },
            }],
        }))
        .unwrap();

        ledger
            .apply_confirmed_shop_effect_timeline(&timeline, user_id, &cycle_id)
            .unwrap();

        let account_id = format!("account:{user_id}");
        let (tokens, growth_bps, wallet_bps): (i64, i64, i64) = ledger
            .connection
            .query_row(
                "SELECT tokens,growth_bps,wallet_bps FROM shop_effect_contribution
             WHERE account_id=?1 AND cycle_id=?2",
                rusqlite::params![account_id, cycle_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!((tokens, growth_bps, wallet_bps), (100_000, 1_000, 100));
        assert_eq!(
            ledger
                .connection
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM shop_game_reward WHERE account_id=?1",
                    [&account_id],
                    |row| row.get(0),
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn guest_import_v2_bounds_signed_account_uses_normal_baseline_fallback() {
        let mut ledger = Ledger::open(Path::new(":memory:"), UTC).unwrap();
        let activation = ledger.planet_activation_at().unwrap();
        let old_guest_cycle = ledger.planet_cycle_id().unwrap();
        add_event(
            &mut ledger,
            "guest-activation-equality-after-sign-in",
            &activation.to_rfc3339(),
            10,
        );

        let guest_event_at = Utc::now();
        assert!(guest_event_at > activation);
        add_event(
            &mut ledger,
            "guest-old-cycle-after-sign-in",
            &guest_event_at.to_rfc3339(),
            23,
        );
        ledger
            .reset_planet(guest_event_at + chrono::Duration::seconds(1))
            .unwrap();
        let signed_cycle = ledger.planet_cycle_id().unwrap();
        mark_existing_signed_owner_for_test(&mut ledger, "00000000-0000-0000-0000-000000000091");

        let occurrences = super::canonical_occurrences(&ledger.connection).unwrap();
        let retained = occurrences
            .iter()
            .find(|item| item.event_key == "guest-old-cycle-after-sign-in")
            .expect("signed raw usage after activation remains canonical");
        assert_eq!(
            (retained.cycle_id.as_str(), retained.tokens),
            ("baseline", 23)
        );
        assert_ne!(retained.cycle_id, old_guest_cycle);
        assert_eq!(ledger.planet_cycle_id().unwrap(), signed_cycle);
        assert!(occurrences
            .iter()
            .all(|item| item.event_key != "guest-activation-equality-after-sign-in"));
    }

    #[test]
    fn guest_import_v2_bounds_require_a_matching_device_for_local_cycles() {
        let mut ledger = Ledger::open(Path::new(":memory:"), UTC).unwrap();
        let activation = ledger.planet_activation_at().unwrap();
        let old_guest_cycle = ledger.planet_cycle_id().unwrap();
        let guest_event_at = Utc::now();
        assert!(guest_event_at > activation);
        add_event(
            &mut ledger,
            "guest-device-bound-event",
            &guest_event_at.to_rfc3339(),
            31,
        );
        ledger
            .reset_planet(guest_event_at + chrono::Duration::seconds(1))
            .unwrap();

        let before_mismatch = super::canonical_occurrences(&ledger.connection).unwrap();
        let before_mismatch = before_mismatch
            .iter()
            .find(|item| item.event_key == "guest-device-bound-event")
            .unwrap();
        assert_eq!(before_mismatch.cycle_id, old_guest_cycle);

        let lineage_state: (i64, String) = ledger
            .connection
            .query_row(
                "SELECT eligible,device_id FROM guest_provenance_lineage WHERE singleton=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(lineage_state.0, 1);
        let changed_device = "device-mismatch-fixture";
        assert_ne!(lineage_state.1, changed_device);
        ledger
            .connection
            .execute(
                "UPDATE setting SET value=?1 WHERE key='planet_device_id'",
                [changed_device],
            )
            .unwrap();

        let after_mismatch = super::canonical_occurrences(&ledger.connection).unwrap();
        let after_mismatch = after_mismatch
            .iter()
            .find(|item| item.event_key == "guest-device-bound-event")
            .unwrap();
        assert_eq!(after_mismatch.cycle_id, "baseline");
        assert_ne!(after_mismatch.cycle_id, old_guest_cycle);
    }

    #[test]
    fn guest_import_v2_bounds_confirmed_timeline_uses_server_cycle_over_guest_cycle() {
        let mut ledger = Ledger::open(Path::new(":memory:"), UTC).unwrap();
        let activation = ledger.planet_activation_at().unwrap();
        let old_guest_cycle = ledger.planet_cycle_id().unwrap();
        let guest_event_at = Utc::now();
        assert!(guest_event_at > activation);
        add_event(
            &mut ledger,
            "guest-timeline-bound-event",
            &guest_event_at.to_rfc3339(),
            47,
        );
        let reset_at = guest_event_at + chrono::Duration::seconds(1);
        ledger.reset_planet(reset_at).unwrap();
        let current_cycle = ledger.planet_cycle_id().unwrap();
        assert_ne!(current_cycle, old_guest_cycle);

        let before_sign_in = super::canonical_occurrences(&ledger.connection).unwrap();
        let before_sign_in = before_sign_in
            .iter()
            .find(|item| item.event_key == "guest-timeline-bound-event")
            .unwrap();
        assert_eq!(before_sign_in.cycle_id, old_guest_cycle);

        let account_id = "00000000-0000-0000-0000-000000000092";
        mark_existing_signed_owner_for_test(&mut ledger, account_id);
        let timeline = ShopEffectTimeline {
            account_id: account_id.into(),
            current_cycle_id: current_cycle.clone(),
            effect_revision: 0,
            server_time_utc: (reset_at + chrono::Duration::seconds(1)).to_rfc3339(),
            reward_timezone: "UTC".into(),
            cycle_bounds: vec![
                ShopCycleBound {
                    cycle_id: "server-confirmed-history".into(),
                    started_at_utc: activation.to_rfc3339(),
                    ended_at_utc: Some(reset_at.to_rfc3339()),
                },
                ShopCycleBound {
                    cycle_id: current_cycle.clone(),
                    started_at_utc: reset_at.to_rfc3339(),
                    ended_at_utc: None,
                },
            ],
            intervals: vec![],
        };
        ledger
            .apply_confirmed_shop_effect_timeline(&timeline, account_id, &current_cycle)
            .unwrap();

        let after_timeline = super::canonical_occurrences(&ledger.connection).unwrap();
        let occurrence = after_timeline
            .iter()
            .find(|item| item.event_key == "guest-timeline-bound-event")
            .unwrap();
        assert_eq!(occurrence.cycle_id, "server-confirmed-history");
        assert_eq!(occurrence.effect_revision, 0);
        assert_eq!(occurrence.effects, ActiveEffects::default());
    }

    #[test]
    fn confirmed_cycle_bounds_attribute_pre_effect_usage_and_omit_unknown_legacy_events() {
        let mut ledger = ledger();
        let (account_id, cycle_id) = signed_account(&mut ledger);
        ledger
            .connection
            .execute(
                "UPDATE setting SET value='2026-10-01T00:00:00Z'
             WHERE key IN ('planet_last_reset_at_utc','planet_cycle_started_at_utc')",
                [],
            )
            .unwrap();
        add_event(
            &mut ledger,
            "unknown-before-server-bounds",
            "2026-09-25T12:00:00Z",
            100,
        );
        add_event(
            &mut ledger,
            "known-before-first-effect",
            "2026-09-30T12:00:00Z",
            200,
        );
        add_event(
            &mut ledger,
            "inside-positive-interval",
            "2026-10-01T12:00:00Z",
            300,
        );

        let timeline: ShopEffectTimeline = serde_json::from_value(serde_json::json!({
            "account_id": account_id,
            "current_cycle_id": cycle_id,
            "effect_revision": 1,
            "server_time_utc": "2026-10-02T00:00:00Z",
            "reward_timezone": "UTC",
            "cycle_bounds": [
                {
                    "cycle_id": "known-prior-cycle",
                    "started_at_utc": "2026-09-26T00:00:00Z",
                    "ended_at_utc": "2026-09-29T00:00:00Z"
                },
                {
                    "cycle_id": cycle_id,
                    "started_at_utc": "2026-09-29T00:00:00Z",
                    "ended_at_utc": null
                }
            ],
            "intervals": [{
                "cycle_id": cycle_id,
                "revision": 1,
                "started_at_utc": "2026-10-01T00:00:00Z",
                "ended_at_utc": null,
                "active_instance_ids": [],
                "effects": {
                    "token_earning_bps": 100,
                    "civilization_growth_bps": 0,
                    "shop_discount_bps": 0,
                    "reset_cooldown_bps": 0,
                    "natural_removal_discount_bps": 0,
                    "era_reward_tokens": 0,
                    "streak_reward_tokens": 0
                }
            }]
        }))
        .unwrap();
        ledger
            .apply_confirmed_shop_effect_timeline(&timeline, &account_id, &cycle_id)
            .unwrap();

        let occurrences = super::canonical_occurrences(&ledger.connection).unwrap();
        assert!(
            occurrences
                .iter()
                .all(|item| item.event_key != "unknown-before-server-bounds"),
            "raw lifetime usage outside server-confirmed cycles must not claim effect attribution"
        );
        let known = occurrences
            .iter()
            .find(|item| item.event_key == "known-before-first-effect")
            .unwrap();
        assert_eq!(known.cycle_id, cycle_id);
        assert_eq!(known.effect_revision, 0);
        assert_eq!(known.effects, ActiveEffects::default());
        assert_eq!(
            ledger
                .planet_device_contribution(false)
                .unwrap()
                .lifetime_tokens,
            600
        );

        ledger.rebuild_shop_contributions().unwrap();
        let unknown_activity_days: i64 = ledger.connection.query_row(
            "SELECT count(*) FROM shop_activity_day WHERE account_id=?1 AND reward_date='2026-09-25'",
            [format!("account:{account_id}")],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(unknown_activity_days, 0);
    }

    #[test]
    fn same_revision_clock_refresh_preserves_history_and_canonical_version() {
        let mut ledger = ledger();
        let (account_id, cycle_id) = signed_account(&mut ledger);
        add_event(
            &mut ledger,
            "timeline-clock-event",
            "2026-09-25T12:00:00Z",
            100_000,
        );
        let effects = ActiveEffects {
            token_earning_bps: 100,
            civilization_growth_bps: 1_000,
            ..ActiveEffects::default()
        };
        let initial = one_interval_timeline(
            &account_id,
            &cycle_id,
            "2026-10-01T00:00:00Z",
            effects.clone(),
            &["instance-1"],
        );
        ledger
            .apply_confirmed_shop_effect_timeline(&initial, &account_id, &cycle_id)
            .unwrap();
        let version = contribution_version(&ledger, &account_id);
        let history_before: (String, String) = ledger.connection.query_row(
            "SELECT active_instance_ids_json,effects_json FROM shop_effect_history WHERE account_id=?1",
            [format!("account:{account_id}")],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap();

        let refreshed = one_interval_timeline(
            &account_id,
            &cycle_id,
            "2026-10-01T01:00:00Z",
            effects,
            &["instance-1"],
        );
        ledger
            .apply_confirmed_shop_effect_timeline(&refreshed, &account_id, &cycle_id)
            .unwrap();

        assert_eq!(contribution_version(&ledger, &account_id), version);
        let history_after: (String, String) = ledger.connection.query_row(
            "SELECT active_instance_ids_json,effects_json FROM shop_effect_history WHERE account_id=?1",
            [format!("account:{account_id}")],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap();
        assert_eq!(history_after, history_before);
        let (revision, server_time): (i64, String) = ledger.connection.query_row(
            "SELECT effect_revision,server_time_utc FROM shop_effect_timeline_state WHERE account_id=?1",
            [format!("account:{account_id}")],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap();
        assert_eq!(
            (revision, server_time.as_str()),
            (1, "2026-10-01T01:00:00+00:00")
        );
    }

    #[test]
    fn first_confirmed_timezone_replaces_local_placeholder_and_is_then_immutable() {
        let mut ledger = ledger();
        ledger.set_planet_timezone("Pacific/Kiritimati").unwrap();
        let (account_id, cycle_id) = signed_account(&mut ledger);
        add_event(
            &mut ledger,
            "timeline-timezone-event",
            "2026-10-01T23:30:00Z",
            100_000,
        );
        let mut timeline = one_interval_timeline(
            &account_id,
            &cycle_id,
            "2026-10-02T00:00:00Z",
            ActiveEffects::default(),
            &[],
        );
        timeline.reward_timezone = "UTC".into();
        ledger
            .apply_confirmed_shop_effect_timeline(&timeline, &account_id, &cycle_id)
            .unwrap();

        let reward_date: String = ledger
            .connection
            .query_row(
                "SELECT reward_date FROM shop_activity_day WHERE account_id=?1",
                [format!("account:{account_id}")],
                |row| row.get(0),
            )
            .unwrap();
        let account_timezone: String = ledger
            .connection
            .query_row(
                "SELECT reward_timezone FROM shop_account_state WHERE account_id=?1",
                [format!("account:{account_id}")],
                |row| row.get(0),
            )
            .unwrap();
        let planet_timezone = ledger.planet_timezone().unwrap().to_string();
        assert_eq!(reward_date, "2026-10-01");
        assert_eq!(account_timezone, "UTC");
        assert_eq!(planet_timezone, "Pacific/Kiritimati");

        timeline.reward_timezone = "Asia/Seoul".into();
        timeline.server_time_utc = "2026-10-02T01:00:00Z".into();
        assert_eq!(
            ledger.apply_confirmed_shop_effect_timeline(&timeline, &account_id, &cycle_id),
            Err(crate::storage::ledger::ScanError::InvalidShopState),
        );
        let stored_timezone: String = ledger
            .connection
            .query_row(
                "SELECT reward_timezone FROM shop_effect_timeline_state WHERE account_id=?1",
                [format!("account:{account_id}")],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored_timezone, "UTC");
    }

    #[test]
    fn timeline_rejects_stale_account_cycle_revision_and_same_revision_content_conflicts() {
        let mut ledger = ledger();
        let (account_id, cycle_id) = signed_account(&mut ledger);
        add_event(
            &mut ledger,
            "timeline-conflict-event",
            "2026-09-25T12:00:00Z",
            100_000,
        );
        let initial = one_interval_timeline(
            &account_id,
            &cycle_id,
            "2026-10-01T00:00:00Z",
            ActiveEffects::default(),
            &[],
        );
        ledger
            .apply_confirmed_shop_effect_timeline(&initial, &account_id, &cycle_id)
            .unwrap();
        let before_version = contribution_version(&ledger, &account_id);

        assert_eq!(
            ledger.apply_confirmed_shop_effect_timeline(
                &initial,
                "00000000-0000-0000-0000-000000000032",
                &cycle_id
            ),
            Err(crate::storage::ledger::ScanError::InvalidShopState),
        );
        assert_eq!(
            ledger.apply_confirmed_shop_effect_timeline(&initial, &account_id, "stale-cycle"),
            Err(crate::storage::ledger::ScanError::InvalidShopState),
        );

        let mut conflicting = initial.clone();
        conflicting.intervals[0].effects.token_earning_bps = 100;
        assert_eq!(
            ledger.apply_confirmed_shop_effect_timeline(&conflicting, &account_id, &cycle_id),
            Err(crate::storage::ledger::ScanError::InvalidShopState),
        );
        let stale_revision =
            signed_timeline(&account_id, &cycle_id, 0, "2026-10-01T01:00:00Z", vec![]);
        assert_eq!(
            ledger.apply_confirmed_shop_effect_timeline(&stale_revision, &account_id, &cycle_id),
            Err(crate::storage::ledger::ScanError::InvalidShopState),
        );

        assert_eq!(contribution_version(&ledger, &account_id), before_version);
        let (revision, effect_json): (i64, String) = ledger
            .connection
            .query_row(
                "SELECT t.effect_revision,h.effects_json FROM shop_effect_timeline_state t
             JOIN shop_effect_history h USING(account_id) WHERE t.account_id=?1",
                [format!("account:{account_id}")],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(revision, 1);
        assert_eq!(
            effect_json,
            serde_json::to_string(&ActiveEffects::default()).unwrap()
        );
    }

    #[test]
    fn account_global_effect_revision_accepts_sparse_visible_timeline() {
        let mut ledger = ledger();
        let (account_id, cycle_id) = signed_account(&mut ledger);
        let initial = signed_timeline(&account_id, &cycle_id, 7, "2026-10-02T00:00:00Z", vec![]);

        ledger
            .apply_confirmed_shop_effect_timeline(&initial, &account_id, &cycle_id)
            .unwrap();

        let mut stale_visible = initial.clone();
        stale_visible.effect_revision = 8;
        stale_visible.server_time_utc = "2026-10-03T00:00:00Z".into();
        stale_visible.intervals = vec![effect_interval(
            &cycle_id,
            6,
            "2026-10-02T00:00:00Z",
            None,
            &[],
            ActiveEffects::default(),
        )];
        assert_eq!(
            ledger.apply_confirmed_shop_effect_timeline(&stale_visible, &account_id, &cycle_id),
            Err(crate::storage::ledger::ScanError::InvalidShopState),
        );

        let mut successor = initial;
        successor.effect_revision = 8;
        successor.server_time_utc = "2026-10-03T00:00:00Z".into();
        successor.intervals = vec![effect_interval(
            &cycle_id,
            8,
            "2026-10-02T00:00:00Z",
            None,
            &[],
            ActiveEffects::default(),
        )];
        ledger
            .apply_confirmed_shop_effect_timeline(&successor, &account_id, &cycle_id)
            .unwrap();

        let stored_revision: i64 = ledger
            .connection
            .query_row(
                "SELECT effect_revision FROM shop_effect_timeline_state WHERE account_id=?1",
                [format!("account:{account_id}")],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored_revision, 8);
    }

    #[test]
    fn timeline_rejects_open_interval_for_noncurrent_cycle() {
        let mut ledger = ledger();
        let (account_id, cycle_id) = signed_account(&mut ledger);
        let timeline = ShopEffectTimeline {
            account_id: account_id.clone(),
            current_cycle_id: cycle_id.clone(),
            effect_revision: 1,
            server_time_utc: "2026-10-02T00:00:00Z".into(),
            reward_timezone: "UTC".into(),
            cycle_bounds: vec![ShopCycleBound {
                cycle_id: cycle_id.clone(),
                started_at_utc: "2026-09-24T00:00:00Z".into(),
                ended_at_utc: None,
            }],
            intervals: vec![effect_interval(
                "legacy-open-cycle",
                1,
                "2026-09-25T00:00:00Z",
                None,
                &[],
                ActiveEffects::default(),
            )],
        };

        assert_eq!(
            ledger.apply_confirmed_shop_effect_timeline(&timeline, &account_id, &cycle_id),
            Err(crate::storage::ledger::ScanError::InvalidShopState),
        );
    }

    #[test]
    fn timeline_rejects_uncapped_effects_noncanonical_ids_non_utc_and_overlaps() {
        let mut ledger = ledger();
        let (account_id, cycle_id) = signed_account(&mut ledger);
        let over_cap = one_interval_timeline(
            &account_id,
            &cycle_id,
            "2026-10-01T00:00:00Z",
            ActiveEffects {
                token_earning_bps: 3_001,
                ..ActiveEffects::default()
            },
            &[],
        );
        let unsorted_ids = one_interval_timeline(
            &account_id,
            &cycle_id,
            "2026-10-01T00:00:00Z",
            ActiveEffects::default(),
            &["instance-b", "instance-a"],
        );
        let mut non_utc = one_interval_timeline(
            &account_id,
            &cycle_id,
            "2026-10-01T00:00:00Z",
            ActiveEffects::default(),
            &[],
        );
        let mut local_account_key = one_interval_timeline(
            &account_id,
            &cycle_id,
            "2026-10-01T00:00:00Z",
            ActiveEffects::default(),
            &[],
        );
        let overlapping = signed_timeline(
            &account_id,
            &cycle_id,
            2,
            "2026-10-02T00:00:00Z",
            vec![
                effect_interval(
                    &cycle_id,
                    1,
                    "2026-09-24T00:00:00Z",
                    Some("2026-10-01T12:00:00Z"),
                    &[],
                    ActiveEffects::default(),
                ),
                effect_interval(
                    &cycle_id,
                    2,
                    "2026-10-01T11:00:00Z",
                    None,
                    &[],
                    ActiveEffects::default(),
                ),
            ],
        );
        non_utc.intervals[0].started_at_utc = "2026-09-24T00:00:00+01:00".into();
        local_account_key.account_id = format!("account:{account_id}");

        for invalid in [
            over_cap,
            unsorted_ids,
            non_utc,
            local_account_key,
            overlapping,
        ] {
            assert_eq!(
                ledger.apply_confirmed_shop_effect_timeline(&invalid, &account_id, &cycle_id),
                Err(crate::storage::ledger::ScanError::InvalidShopState),
            );
        }
        let stored_count: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM shop_effect_timeline_state WHERE account_id=?1",
                [format!("account:{account_id}")],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored_count, 0);
    }

    #[test]
    fn confirmed_effect_interval_gap_uses_default_revision_inside_known_cycle() {
        let mut ledger = ledger();
        let (account_id, cycle_id) = signed_account(&mut ledger);
        add_event(
            &mut ledger,
            "usage-in-effect-gap",
            "2026-10-01T12:30:00Z",
            200,
        );
        let timeline = signed_timeline(
            &account_id,
            &cycle_id,
            2,
            "2026-10-02T00:00:00Z",
            vec![
                effect_interval(
                    &cycle_id,
                    1,
                    "2026-09-24T00:00:00Z",
                    Some("2026-10-01T12:00:00Z"),
                    &[],
                    ActiveEffects {
                        token_earning_bps: 100,
                        ..ActiveEffects::default()
                    },
                ),
                effect_interval(
                    &cycle_id,
                    2,
                    "2026-10-01T13:00:00Z",
                    None,
                    &[],
                    ActiveEffects {
                        token_earning_bps: 200,
                        ..ActiveEffects::default()
                    },
                ),
            ],
        );
        ledger
            .apply_confirmed_shop_effect_timeline(&timeline, &account_id, &cycle_id)
            .unwrap();

        let occurrence = super::canonical_occurrences(&ledger.connection)
            .unwrap()
            .into_iter()
            .find(|item| item.event_key == "usage-in-effect-gap")
            .unwrap();
        assert_eq!(occurrence.cycle_id, cycle_id);
        assert_eq!(occurrence.effect_revision, 0);
        assert_eq!(occurrence.effects, ActiveEffects::default());
    }

    #[test]
    fn confirmed_cycle_bounds_close_and_append_across_reset() {
        let mut ledger = ledger();
        let (account_id, old_cycle_id) = signed_account(&mut ledger);
        add_event(&mut ledger, "old-cycle-event", "2026-10-01T12:00:00Z", 100);

        let initial = one_interval_timeline(
            &account_id,
            &old_cycle_id,
            "2026-10-02T00:00:00Z",
            ActiveEffects {
                token_earning_bps: 100,
                ..ActiveEffects::default()
            },
            &[],
        );
        ledger
            .apply_confirmed_shop_effect_timeline(&initial, &account_id, &old_cycle_id)
            .unwrap();

        let new_cycle_id = "cycle-after-reset";
        ledger
            .connection
            .execute(
                "UPDATE setting SET value=?1 WHERE key='planet_current_cycle_id'",
                [new_cycle_id],
            )
            .unwrap();
        ledger
            .connection
            .execute(
                "UPDATE setting SET value='2026-10-02T00:00:00Z'
             WHERE key IN ('planet_last_reset_at_utc','planet_cycle_started_at_utc')",
                [],
            )
            .unwrap();
        add_event(
            &mut ledger,
            "new-cycle-before-effect",
            "2026-10-02T12:00:00Z",
            200,
        );
        add_event(
            &mut ledger,
            "new-cycle-after-effect",
            "2026-10-03T12:00:00Z",
            300,
        );
        let reset_timeline = ShopEffectTimeline {
            account_id: account_id.clone(),
            current_cycle_id: new_cycle_id.into(),
            effect_revision: 2,
            server_time_utc: "2026-10-04T00:00:00Z".into(),
            reward_timezone: "UTC".into(),
            cycle_bounds: vec![
                ShopCycleBound {
                    cycle_id: old_cycle_id.clone(),
                    started_at_utc: "2026-09-24T00:00:00Z".into(),
                    ended_at_utc: Some("2026-10-02T00:00:00Z".into()),
                },
                ShopCycleBound {
                    cycle_id: new_cycle_id.into(),
                    started_at_utc: "2026-10-02T00:00:00Z".into(),
                    ended_at_utc: None,
                },
            ],
            intervals: vec![
                effect_interval(
                    &old_cycle_id,
                    1,
                    "2026-09-24T00:00:00Z",
                    Some("2026-10-02T00:00:00Z"),
                    &[],
                    ActiveEffects {
                        token_earning_bps: 100,
                        ..ActiveEffects::default()
                    },
                ),
                effect_interval(
                    new_cycle_id,
                    2,
                    "2026-10-03T00:00:00Z",
                    None,
                    &[],
                    ActiveEffects {
                        token_earning_bps: 300,
                        ..ActiveEffects::default()
                    },
                ),
            ],
        };
        ledger
            .apply_confirmed_shop_effect_timeline(&reset_timeline, &account_id, new_cycle_id)
            .unwrap();

        let occurrences = super::canonical_occurrences(&ledger.connection).unwrap();
        let old_event = occurrences
            .iter()
            .find(|item| item.event_key == "old-cycle-event")
            .unwrap();
        assert_eq!(
            (old_event.cycle_id.as_str(), old_event.effect_revision),
            (old_cycle_id.as_str(), 1)
        );
        let before_effect = occurrences
            .iter()
            .find(|item| item.event_key == "new-cycle-before-effect")
            .unwrap();
        assert_eq!(
            (
                before_effect.cycle_id.as_str(),
                before_effect.effect_revision
            ),
            (new_cycle_id, 0)
        );
        assert_eq!(before_effect.effects, ActiveEffects::default());
        let after_effect = occurrences
            .iter()
            .find(|item| item.event_key == "new-cycle-after-effect")
            .unwrap();
        assert_eq!(
            (after_effect.cycle_id.as_str(), after_effect.effect_revision),
            (new_cycle_id, 2)
        );
        assert_eq!(after_effect.effects.token_earning_bps, 300);

        let stored_bounds: Vec<(String, Option<String>)> = {
            let mut statement = ledger
                .connection
                .prepare(
                    "SELECT cycle_id,ended_at_utc FROM shop_effect_cycle_bound
                 WHERE account_id=?1 ORDER BY started_at_utc",
                )
                .unwrap();
            statement
                .query_map([format!("account:{account_id}")], |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert_eq!(stored_bounds.len(), 2);
        assert_eq!(
            stored_bounds[0],
            (old_cycle_id, Some("2026-10-02T00:00:00+00:00".into()))
        );
        assert_eq!(stored_bounds[1], (new_cycle_id.into(), None));
        assert_eq!(
            ledger
                .planet_device_contribution(false)
                .unwrap()
                .lifetime_tokens,
            600
        );
    }

    #[test]
    fn confirmed_cycle_bounds_reject_closed_mutation_and_retroactive_prepend() {
        let mut ledger = ledger();
        let (account_id, old_cycle_id) = signed_account(&mut ledger);
        let initial = signed_timeline(
            &account_id,
            &old_cycle_id,
            0,
            "2026-10-01T00:00:00Z",
            vec![],
        );
        ledger
            .apply_confirmed_shop_effect_timeline(&initial, &account_id, &old_cycle_id)
            .unwrap();

        let new_cycle_id = "cycle-after-reset";
        ledger
            .connection
            .execute(
                "UPDATE setting SET value=?1 WHERE key='planet_current_cycle_id'",
                [new_cycle_id],
            )
            .unwrap();
        let successor = ShopEffectTimeline {
            account_id: account_id.clone(),
            current_cycle_id: new_cycle_id.into(),
            effect_revision: 0,
            server_time_utc: "2026-10-03T00:00:00Z".into(),
            reward_timezone: "UTC".into(),
            cycle_bounds: vec![
                ShopCycleBound {
                    cycle_id: old_cycle_id.clone(),
                    started_at_utc: "2026-09-24T00:00:00Z".into(),
                    ended_at_utc: Some("2026-10-02T00:00:00Z".into()),
                },
                ShopCycleBound {
                    cycle_id: new_cycle_id.into(),
                    started_at_utc: "2026-10-02T00:00:00Z".into(),
                    ended_at_utc: None,
                },
            ],
            intervals: vec![],
        };
        ledger
            .apply_confirmed_shop_effect_timeline(&successor, &account_id, new_cycle_id)
            .unwrap();
        let before_version = contribution_version(&ledger, &account_id);

        let mut mutated_closed_cycle = successor.clone();
        mutated_closed_cycle.server_time_utc = "2026-10-04T00:00:00Z".into();
        mutated_closed_cycle.cycle_bounds[0].started_at_utc = "2026-09-23T00:00:00Z".into();
        assert_eq!(
            ledger.apply_confirmed_shop_effect_timeline(
                &mutated_closed_cycle,
                &account_id,
                new_cycle_id,
            ),
            Err(crate::storage::ledger::ScanError::InvalidShopState),
        );

        let mut prepended = successor.clone();
        prepended.server_time_utc = "2026-10-04T00:00:00Z".into();
        prepended.cycle_bounds.insert(
            0,
            ShopCycleBound {
                cycle_id: "previously-unknown-cycle".into(),
                started_at_utc: "2026-09-23T00:00:00Z".into(),
                ended_at_utc: Some("2026-09-24T00:00:00Z".into()),
            },
        );
        assert_eq!(
            ledger.apply_confirmed_shop_effect_timeline(&prepended, &account_id, new_cycle_id),
            Err(crate::storage::ledger::ScanError::InvalidShopState),
        );

        assert_eq!(contribution_version(&ledger, &account_id), before_version);
        let stored_bounds: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM shop_effect_cycle_bound WHERE account_id=?1",
                [format!("account:{account_id}")],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored_bounds, 2);
    }

    #[test]
    fn reopened_legacy_timeline_bootstraps_bounds_once_without_rewriting_history() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut ledger = Ledger::open(file.path(), UTC).unwrap();
        ledger
            .connection
            .execute(
                "UPDATE setting SET value='2026-09-24T00:00:00Z'
             WHERE key IN ('planet_activation_at_utc','planet_cycle_started_at_utc')",
                [],
            )
            .unwrap();
        let (account_id, cycle_id) = signed_account(&mut ledger);
        let old_timeline = ShopEffectTimeline {
            account_id: account_id.clone(),
            current_cycle_id: cycle_id.clone(),
            effect_revision: 1,
            server_time_utc: "2026-10-03T00:00:00Z".into(),
            reward_timezone: "UTC".into(),
            cycle_bounds: vec![],
            intervals: vec![effect_interval(
                &cycle_id,
                1,
                "2026-09-24T00:00:00Z",
                None,
                &[],
                ActiveEffects::default(),
            )],
        };
        ledger
            .apply_confirmed_shop_effect_timeline(&old_timeline, &account_id, &cycle_id)
            .unwrap();
        ledger
            .connection
            .execute_batch(
                "DROP TABLE shop_effect_cycle_bounds_state;
             DROP TABLE shop_effect_cycle_bound;",
            )
            .unwrap();
        drop(ledger);

        let mut ledger = Ledger::open(file.path(), UTC).unwrap();
        let retained_history: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM shop_effect_history WHERE account_id=?1",
                [format!("account:{account_id}")],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(retained_history, 1);

        let known_bounds = vec![ShopCycleBound {
            cycle_id: cycle_id.clone(),
            started_at_utc: "2026-09-24T00:00:00Z".into(),
            ended_at_utc: None,
        }];
        let mut timezone_conflict = old_timeline.clone();
        timezone_conflict.server_time_utc = "2026-10-04T00:00:00Z".into();
        timezone_conflict.reward_timezone = "Asia/Seoul".into();
        timezone_conflict.cycle_bounds = known_bounds.clone();
        assert_eq!(
            ledger
                .apply_confirmed_shop_effect_timeline(&timezone_conflict, &account_id, &cycle_id,),
            Err(crate::storage::ledger::ScanError::InvalidShopState),
        );
        let stale_revision = ShopEffectTimeline {
            account_id: account_id.clone(),
            current_cycle_id: cycle_id.clone(),
            effect_revision: 0,
            server_time_utc: "2026-10-04T00:00:00Z".into(),
            reward_timezone: "UTC".into(),
            cycle_bounds: known_bounds.clone(),
            intervals: vec![],
        };
        assert_eq!(
            ledger.apply_confirmed_shop_effect_timeline(&stale_revision, &account_id, &cycle_id,),
            Err(crate::storage::ledger::ScanError::InvalidShopState),
        );
        let stale_clock = ShopEffectTimeline {
            server_time_utc: "2026-10-02T00:00:00Z".into(),
            ..old_timeline.clone()
        };
        assert_eq!(
            ledger.apply_confirmed_shop_effect_timeline(&stale_clock, &account_id, &cycle_id),
            Err(crate::storage::ledger::ScanError::InvalidShopState),
        );

        let (prior_server_time, prior_bounds, initialized_marker): (String, i64, i64) = ledger
            .connection
            .query_row(
                "SELECT server_time_utc,
                        (SELECT count(*) FROM shop_effect_cycle_bound WHERE account_id=?1),
                        (SELECT count(*) FROM shop_effect_cycle_bounds_state WHERE account_id=?1)
                 FROM shop_effect_timeline_state WHERE account_id=?1",
                [format!("account:{account_id}")],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            (prior_server_time.as_str(), prior_bounds, initialized_marker),
            ("2026-10-03T00:00:00+00:00", 0, 0)
        );

        let mut bootstrapped = old_timeline.clone();
        bootstrapped.server_time_utc = "2026-10-04T00:00:00Z".into();
        bootstrapped.cycle_bounds = known_bounds;
        ledger
            .apply_confirmed_shop_effect_timeline(&bootstrapped, &account_id, &cycle_id)
            .unwrap();

        let mut retroactive_prepend = bootstrapped.clone();
        retroactive_prepend.server_time_utc = "2026-10-05T00:00:00Z".into();
        retroactive_prepend.cycle_bounds.insert(
            0,
            ShopCycleBound {
                cycle_id: "too-old-to-bootstrap-again".into(),
                started_at_utc: "2026-09-23T00:00:00Z".into(),
                ended_at_utc: Some("2026-09-24T00:00:00Z".into()),
            },
        );
        assert_eq!(
            ledger.apply_confirmed_shop_effect_timeline(
                &retroactive_prepend,
                &account_id,
                &cycle_id,
            ),
            Err(crate::storage::ledger::ScanError::InvalidShopState),
        );
        let (stored_time, stored_bounds): (String, i64) = ledger
            .connection
            .query_row(
                "SELECT server_time_utc,
                    (SELECT count(*) FROM shop_effect_cycle_bound WHERE account_id=?1)
             FROM shop_effect_timeline_state WHERE account_id=?1",
                [format!("account:{account_id}")],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(stored_time, "2026-10-04T00:00:00+00:00");
        assert_eq!(stored_bounds, 1);
    }

    #[test]
    fn known_positive_history_outside_current_bounds_still_attributes_events() {
        let mut ledger = ledger();
        let (account_id, current_cycle_id) = signed_account(&mut ledger);
        add_event(
            &mut ledger,
            "known-old-positive",
            "2026-09-26T12:00:00Z",
            100,
        );
        add_event(
            &mut ledger,
            "current-revision-zero",
            "2026-10-01T12:00:00Z",
            200,
        );
        add_event(
            &mut ledger,
            "unknown-old-raw-only",
            "2026-09-24T12:00:00Z",
            300,
        );
        let old_cycle_id = "confirmed-older-cycle";
        let timeline = ShopEffectTimeline {
            account_id: account_id.clone(),
            current_cycle_id: current_cycle_id.clone(),
            effect_revision: 2,
            server_time_utc: "2026-10-03T00:00:00Z".into(),
            reward_timezone: "UTC".into(),
            cycle_bounds: vec![ShopCycleBound {
                cycle_id: current_cycle_id.clone(),
                started_at_utc: "2026-09-30T00:00:00Z".into(),
                ended_at_utc: None,
            }],
            intervals: vec![
                effect_interval(
                    old_cycle_id,
                    1,
                    "2026-09-25T00:00:00Z",
                    Some("2026-09-30T00:00:00Z"),
                    &[],
                    ActiveEffects {
                        token_earning_bps: 100,
                        ..ActiveEffects::default()
                    },
                ),
                effect_interval(
                    &current_cycle_id,
                    2,
                    "2026-10-02T00:00:00Z",
                    None,
                    &[],
                    ActiveEffects {
                        token_earning_bps: 200,
                        ..ActiveEffects::default()
                    },
                ),
            ],
        };
        ledger
            .apply_confirmed_shop_effect_timeline(&timeline, &account_id, &current_cycle_id)
            .unwrap();

        let occurrences = super::canonical_occurrences(&ledger.connection).unwrap();
        let old = occurrences
            .iter()
            .find(|item| item.event_key == "known-old-positive");
        assert!(
            old.is_some(),
            "a confirmed positive interval attributes its known older-cycle usage"
        );
        let old = old.unwrap();
        assert_eq!(
            (old.cycle_id.as_str(), old.effect_revision),
            (old_cycle_id, 1)
        );
        assert_eq!(old.effects.token_earning_bps, 100);
        let current = occurrences
            .iter()
            .find(|item| item.event_key == "current-revision-zero")
            .unwrap();
        assert_eq!(
            (current.cycle_id.as_str(), current.effect_revision),
            (current_cycle_id.as_str(), 0)
        );
        assert_eq!(current.effects, ActiveEffects::default());
        assert!(occurrences
            .iter()
            .all(|item| item.event_key != "unknown-old-raw-only"));
        assert_eq!(
            ledger
                .planet_device_contribution(false)
                .unwrap()
                .lifetime_tokens,
            600
        );
    }

    #[test]
    fn higher_timeline_revision_rebuilds_late_usage_from_occurrence_intervals() {
        let mut ledger = ledger();
        let (account_id, cycle_id) = signed_account(&mut ledger);
        let old_effects = ActiveEffects {
            token_earning_bps: 100,
            civilization_growth_bps: 1_000,
            ..ActiveEffects::default()
        };
        let initial = one_interval_timeline(
            &account_id,
            &cycle_id,
            "2026-10-01T00:00:00Z",
            old_effects.clone(),
            &["instance-1"],
        );
        ledger
            .apply_confirmed_shop_effect_timeline(&initial, &account_id, &cycle_id)
            .unwrap();
        let before_version = contribution_version(&ledger, &account_id);

        add_event(
            &mut ledger,
            "late-before-effect-change",
            "2026-10-01T06:00:00Z",
            5_000,
        );
        add_event(
            &mut ledger,
            "late-after-effect-change",
            "2026-10-01T18:00:00Z",
            7_000,
        );
        let new_effects = ActiveEffects {
            token_earning_bps: 200,
            ..ActiveEffects::default()
        };
        let updated = signed_timeline(
            &account_id,
            &cycle_id,
            2,
            "2026-10-02T00:00:00Z",
            vec![
                effect_interval(
                    &cycle_id,
                    1,
                    "2026-09-24T00:00:00Z",
                    Some("2026-10-01T12:00:00Z"),
                    &["instance-1"],
                    old_effects,
                ),
                effect_interval(
                    &cycle_id,
                    2,
                    "2026-10-01T12:00:00Z",
                    None,
                    &["instance-1", "instance-2"],
                    new_effects,
                ),
            ],
        );
        ledger
            .apply_confirmed_shop_effect_timeline(&updated, &account_id, &cycle_id)
            .unwrap();

        let rows: Vec<(i64, i64, i64, i64)> = {
            let mut statement = ledger.connection.prepare(
                "SELECT effect_revision,tokens,growth_bps,wallet_bps FROM shop_effect_contribution
                 WHERE account_id=?1 AND date='2026-10-01' ORDER BY effect_revision",
            ).unwrap();
            statement
                .query_map([format!("account:{account_id}")], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert_eq!(rows, vec![(1, 5_000, 1_000, 100), (2, 7_000, 0, 200)]);
        let updated_version = contribution_version(&ledger, &account_id);
        assert!(updated_version > before_version);
        let rewritten_closed_interval = signed_timeline(
            &account_id,
            &cycle_id,
            3,
            "2026-10-03T00:00:00Z",
            vec![
                effect_interval(
                    &cycle_id,
                    1,
                    "2026-09-24T00:00:00Z",
                    Some("2026-10-01T12:00:00Z"),
                    &["instance-1"],
                    ActiveEffects {
                        token_earning_bps: 100,
                        civilization_growth_bps: 999,
                        ..ActiveEffects::default()
                    },
                ),
                effect_interval(
                    &cycle_id,
                    2,
                    "2026-10-01T12:00:00Z",
                    Some("2026-10-02T12:00:00Z"),
                    &["instance-1", "instance-2"],
                    ActiveEffects {
                        token_earning_bps: 200,
                        ..ActiveEffects::default()
                    },
                ),
                effect_interval(
                    &cycle_id,
                    3,
                    "2026-10-02T12:00:00Z",
                    None,
                    &["instance-1", "instance-2"],
                    ActiveEffects {
                        token_earning_bps: 200,
                        ..ActiveEffects::default()
                    },
                ),
            ],
        );
        assert_eq!(
            ledger.apply_confirmed_shop_effect_timeline(
                &rewritten_closed_interval,
                &account_id,
                &cycle_id
            ),
            Err(crate::storage::ledger::ScanError::InvalidShopState),
        );
        assert_eq!(contribution_version(&ledger, &account_id), updated_version);
        ledger.settle_guest_rewards(Utc::now()).unwrap();
        ledger
            .settle_guest_cycle_tokens(&cycle_id, Utc::now())
            .unwrap();
        let local_rewards: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM shop_game_reward WHERE account_id=?1",
                [format!("account:{account_id}")],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(local_rewards, 0);
    }

    #[test]
    fn higher_revision_with_unchanged_event_mapping_advances_timeline_not_contributions() {
        let mut ledger = ledger();
        let (account_id, cycle_id) = signed_account(&mut ledger);
        add_event(
            &mut ledger,
            "timeline-revision-event",
            "2026-09-25T12:00:00Z",
            100_000,
        );
        let effects = ActiveEffects {
            token_earning_bps: 100,
            civilization_growth_bps: 1_000,
            ..ActiveEffects::default()
        };
        let initial = one_interval_timeline(
            &account_id,
            &cycle_id,
            "2026-10-01T00:00:00Z",
            effects.clone(),
            &["instance-1"],
        );
        ledger
            .apply_confirmed_shop_effect_timeline(&initial, &account_id, &cycle_id)
            .unwrap();
        let version = contribution_version(&ledger, &account_id);
        let updated = signed_timeline(
            &account_id,
            &cycle_id,
            2,
            "2026-10-02T00:00:00Z",
            vec![
                effect_interval(
                    &cycle_id,
                    1,
                    "2026-09-24T00:00:00Z",
                    Some("2026-10-01T12:00:00Z"),
                    &["instance-1"],
                    effects.clone(),
                ),
                effect_interval(
                    &cycle_id,
                    2,
                    "2026-10-01T12:00:00Z",
                    None,
                    &["instance-1"],
                    effects,
                ),
            ],
        );
        ledger
            .apply_confirmed_shop_effect_timeline(&updated, &account_id, &cycle_id)
            .unwrap();

        assert_eq!(contribution_version(&ledger, &account_id), version);
        let timeline_revision: i64 = ledger
            .connection
            .query_row(
                "SELECT effect_revision FROM shop_effect_timeline_state WHERE account_id=?1",
                [format!("account:{account_id}")],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(timeline_revision, 2);
    }

    #[test]
    fn failed_contribution_rebuild_rolls_back_timeline_history_and_metadata() {
        let mut ledger = ledger();
        let (account_id, cycle_id) = signed_account(&mut ledger);
        add_event(
            &mut ledger,
            "atomic-baseline-event",
            "2026-10-03T01:00:00Z",
            100_000,
        );
        let initial_effects = ActiveEffects {
            token_earning_bps: 100,
            ..ActiveEffects::default()
        };
        let initial = one_interval_timeline(
            &account_id,
            &cycle_id,
            "2026-10-03T00:00:00Z",
            initial_effects.clone(),
            &["instance-1"],
        );
        ledger
            .apply_confirmed_shop_effect_timeline(&initial, &account_id, &cycle_id)
            .unwrap();
        let before_version = contribution_version(&ledger, &account_id);
        let account_key = format!("account:{account_id}");
        ledger.connection.execute(
            "INSERT INTO usage_record(event_key,source_id,agent,kind,bucket_date,occurred_at_utc,
             total_tokens,coverage,parser_version)
             VALUES ('atomic-overflow-event','test:atomic-overflow','codex','response',
             '2026-10-03','2026-10-03T01:30:00+00:00',?1,'complete',1)",
            [i64::MAX],
        ).unwrap();
        ledger.connection.execute(
            "INSERT INTO planet_usage_owner(event_key,account_id) VALUES ('atomic-overflow-event',?1)",
            [&account_key],
        ).unwrap();
        let updated = signed_timeline(
            &account_id,
            &cycle_id,
            2,
            "2026-10-04T00:00:00Z",
            vec![
                effect_interval(
                    &cycle_id,
                    1,
                    "2026-09-24T00:00:00Z",
                    Some("2026-10-03T02:00:00Z"),
                    &["instance-1"],
                    initial_effects,
                ),
                effect_interval(
                    &cycle_id,
                    2,
                    "2026-10-03T02:00:00Z",
                    None,
                    &["instance-2"],
                    ActiveEffects::default(),
                ),
            ],
        );

        assert_eq!(
            ledger.apply_confirmed_shop_effect_timeline(&updated, &account_id, &cycle_id),
            Err(crate::storage::ledger::ScanError::InvalidCount),
        );

        let (revision, server_time): (i64, String) = ledger.connection.query_row(
            "SELECT effect_revision,server_time_utc FROM shop_effect_timeline_state WHERE account_id=?1",
            [&account_key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap();
        assert_eq!(
            (revision, server_time.as_str()),
            (1, "2026-10-03T00:00:00+00:00")
        );
        let (history_count, open_revision): (i64, i64) = ledger
            .connection
            .query_row(
                "SELECT count(*),max(revision) FROM shop_effect_history
             WHERE account_id=?1 AND ended_at_utc IS NULL",
                [&account_key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!((history_count, open_revision), (1, 1));
        assert_eq!(contribution_version(&ledger, &account_id), before_version);
        let stored_tokens: i64 = ledger
            .connection
            .query_row(
                "SELECT tokens FROM shop_effect_contribution WHERE account_id=?1",
                [&account_key],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored_tokens, 100_000);
    }

    #[test]
    fn account_switch_cannot_apply_a_previous_account_timeline() {
        let mut ledger = ledger();
        let (account_id, cycle_id) = signed_account(&mut ledger);
        let timeline = one_interval_timeline(
            &account_id,
            &cycle_id,
            "2026-10-01T00:00:00Z",
            ActiveEffects::default(),
            &[],
        );
        ledger
            .apply_confirmed_shop_effect_timeline(&timeline, &account_id, &cycle_id)
            .unwrap();

        let next_account = "00000000-0000-0000-0000-000000000032";
        ledger.ensure_planet_account(next_account).unwrap();
        let next_cycle = ledger.planet_cycle_id().unwrap();
        assert_ne!(cycle_id, next_cycle);
        assert_eq!(
            ledger.apply_confirmed_shop_effect_timeline(&timeline, &account_id, &cycle_id),
            Err(crate::storage::ledger::ScanError::InvalidShopState),
        );
        let old_account_timeline: i64 = ledger
            .connection
            .query_row(
                "SELECT effect_revision FROM shop_effect_timeline_state WHERE account_id=?1",
                [format!("account:{account_id}")],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(old_account_timeline, 1);
        let old_account_bounds: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM shop_effect_cycle_bound WHERE account_id=?1",
                [format!("account:{account_id}")],
                |row| row.get(0),
            )
            .unwrap();
        let next_account_bounds: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM shop_effect_cycle_bound WHERE account_id=?1",
                [format!("account:{next_account}")],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!((old_account_bounds, next_account_bounds), (1, 0));
    }

    #[test]
    fn reward_settlement_uses_frozen_timezone_consecutive_dates_and_deduped_era_keys() {
        let mut ledger = ledger();
        add_effect_history(
            &ledger,
            1,
            "2026-09-24T00:00:00+00:00",
            ActiveEffects {
                era_reward_tokens: 500_000,
                streak_reward_tokens: 10_000,
                ..ActiveEffects::default()
            },
        );
        add_event(
            &mut ledger,
            "era-crossing",
            "2026-09-25T23:30:00Z",
            3_500_000,
        );
        add_event(&mut ledger, "day-two", "2026-09-26T00:30:00Z", 50_000);
        add_effect_history(
            &ledger,
            2,
            "2026-09-26T00:45:00Z",
            ActiveEffects {
                streak_reward_tokens: 70_000,
                ..ActiveEffects::default()
            },
        );
        add_event(&mut ledger, "day-two-later", "2026-09-26T10:30:00Z", 50_000);
        add_event(&mut ledger, "gap-day", "2026-09-28T00:30:00Z", 50_000);
        ledger.set_planet_timezone("Pacific/Kiritimati").unwrap();
        let now = DateTime::parse_from_rfc3339("2026-09-29T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        ledger.rebuild_shop_contributions().unwrap();
        ledger.settle_guest_rewards(now).unwrap();
        ledger.settle_guest_rewards(now).unwrap();
        let activity_dates: Vec<String> = {
            let mut statement = ledger
                .connection
                .prepare("SELECT reward_date FROM shop_activity_day ORDER BY reward_date")
                .unwrap();
            statement
                .query_map([], |row| row.get(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert_eq!(
            activity_dates,
            vec!["2026-09-25", "2026-09-26", "2026-09-28"]
        );
        assert_eq!(
            ledger.shop_state().unwrap().reward_state.reward_timezone,
            "UTC"
        );
        let (era_count, era_amount): (i64, i64) = ledger
            .connection
            .query_row(
                "SELECT count(*),coalesce(sum(amount),0) FROM shop_game_reward WHERE kind='era'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!((era_count, era_amount), (1, 500_000));
        let (streak_count, streak_date, streak_amount): (i64, Option<String>, i64) = ledger
            .connection
            .query_row(
                "SELECT count(*),max(trigger_key),coalesce(sum(amount),0)
             FROM shop_game_reward WHERE kind='streak'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            (streak_count, streak_date.as_deref(), streak_amount),
            (1, Some("streak:2026-09-26"), 10_000)
        );
        let wallet_total: i64 = ledger
            .connection
            .query_row(
                "SELECT coalesce(sum(amount),0) FROM shop_wallet_credit",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(wallet_total, 510_000);

        add_event(&mut ledger, "era-crossing", "2026-09-25T23:30:00Z", 0);
        ledger.rebuild_shop_contributions().unwrap();
        ledger.settle_guest_rewards(now).unwrap();
        let persisted_rewards: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM shop_game_reward WHERE kind IN ('era','streak')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            persisted_rewards, 2,
            "canonical correction cannot claw back a confirmed reward or issue it twice"
        );
        let reduced_growth: f64 = ledger.shop_growth_credit_by_date().unwrap().values().sum();
        assert!(reduced_growth < crate::growth::STAGE_THRESHOLDS[0]);

        add_event(
            &mut ledger,
            "era-crossing",
            "2026-09-25T23:30:00Z",
            3_500_000,
        );
        ledger.rebuild_shop_contributions().unwrap();
        ledger.settle_guest_rewards(now).unwrap();
        let rebound_growth: f64 = ledger.shop_growth_credit_by_date().unwrap().values().sum();
        assert!(rebound_growth >= crate::growth::STAGE_THRESHOLDS[0]);
        let reward_rows: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM shop_game_reward WHERE kind='era'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            reward_rows, 1,
            "a score drop and later recrossing reuses the original era key"
        );
        assert_eq!(
            ledger.shop_state().unwrap().reward_state.era_reward_tokens,
            500_000
        );
    }

    #[test]
    fn reward_settlement_awards_each_era_threshold_once() {
        let mut ledger = ledger();
        add_effect_history(
            &ledger,
            1,
            "2026-09-24T00:00:00Z",
            ActiveEffects {
                era_reward_tokens: 500_000,
                ..ActiveEffects::default()
            },
        );
        add_event(
            &mut ledger,
            "threshold-day-one",
            "2026-09-25T12:00:00Z",
            1_000_000_000_000_000_000,
        );
        add_event(
            &mut ledger,
            "threshold-day-two",
            "2026-09-26T12:00:00Z",
            1_000_000_000_000_000_000,
        );
        add_event(
            &mut ledger,
            "threshold-day-three",
            "2026-09-27T12:00:00Z",
            1_000_000_000_000_000_000,
        );
        let now = DateTime::parse_from_rfc3339("2026-09-29T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        ledger.settle_guest_rewards(now).unwrap();
        ledger.settle_guest_rewards(now).unwrap();

        let stages: Vec<i64> = {
            let mut statement = ledger
                .connection
                .prepare("SELECT stage FROM shop_era_progress ORDER BY stage")
                .unwrap();
            statement
                .query_map([], |row| row.get(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert_eq!(stages, vec![1, 2, 3, 4]);
        let (count, amount): (i64, i64) = ledger
            .connection
            .query_row(
                "SELECT count(*),coalesce(sum(amount),0) FROM shop_game_reward WHERE kind='era'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!((count, amount), (4, 2_000_000));
        assert_eq!(
            ledger.shop_state().unwrap().reward_state.era_reward_tokens,
            2_000_000
        );
    }

    #[test]
    fn old_cycle_growth_does_not_unlock_era_rewards_in_the_new_cycle() {
        let mut ledger = ledger();
        add_effect_history(
            &ledger,
            1,
            "2026-09-24T00:00:00Z",
            ActiveEffects {
                era_reward_tokens: 500_000,
                ..ActiveEffects::default()
            },
        );
        add_event(
            &mut ledger,
            "old-cycle-growth",
            "2026-09-25T12:00:00Z",
            1_000_000_000_000_000_000,
        );
        ledger
            .connection
            .execute(
                "UPDATE setting SET value='new-cycle' WHERE key='planet_current_cycle_id'",
                [],
            )
            .unwrap();
        ledger
            .connection
            .execute(
                "UPDATE setting SET value='2026-09-26T00:00:00Z'
             WHERE key IN ('planet_last_reset_at_utc','planet_cycle_started_at_utc')",
                [],
            )
            .unwrap();

        ledger
            .settle_guest_rewards(
                DateTime::parse_from_rfc3339("2026-09-29T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            )
            .unwrap();

        let rewards: i64 = ledger
            .connection
            .query_row("SELECT count(*) FROM shop_era_progress", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(rewards, 0);
    }

    #[test]
    fn cycle_token_settlement_carries_fractional_bonus_and_is_idempotent() {
        let mut ledger = ledger();
        add_effect_history(
            &ledger,
            1,
            "2026-09-24T00:00:00Z",
            ActiveEffects {
                token_earning_bps: 100,
                ..ActiveEffects::default()
            },
        );
        for ordinal in 0..100 {
            add_event(
                &mut ledger,
                &format!("wallet-event-{ordinal}"),
                "2026-09-25T12:00:00Z",
                1,
            );
        }
        let cycle: String = ledger
            .connection
            .query_row(
                "SELECT value FROM setting WHERE key='planet_current_cycle_id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let now = DateTime::parse_from_rfc3339("2026-09-29T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        assert_eq!(ledger.settle_guest_cycle_tokens(&cycle, now).unwrap(), 1);
        assert_eq!(ledger.settle_guest_cycle_tokens(&cycle, now).unwrap(), 1);
        assert_eq!(ledger.planet_usage_totals().unwrap().1, 100);
        let (settlements,rewards,credits):(i64,i64,i64)=ledger.connection.query_row(
            "SELECT (SELECT count(*) FROM shop_cycle_settlement WHERE account_id='local'),
             (SELECT count(*) FROM shop_game_reward WHERE account_id='local' AND kind='cycle_token'),
             (SELECT coalesce(sum(amount),0) FROM shop_wallet_credit WHERE account_id='local')",
            [],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
        ).unwrap();
        assert_eq!((settlements, rewards, credits), (1, 1, 1));
    }

    #[test]
    fn late_closed_cycle_activity_does_not_award_streak_but_can_precede_a_current_cycle_day() {
        let mut ledger = ledger();
        add_effect_history(
            &ledger,
            1,
            "2026-09-24T00:00:00Z",
            ActiveEffects {
                streak_reward_tokens: 70_000,
                ..ActiveEffects::default()
            },
        );
        add_event(&mut ledger, "old-cycle-day-one", "2026-09-30T10:00:00Z", 10);
        ledger
            .settle_guest_rewards(
                DateTime::parse_from_rfc3339("2026-10-01T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            )
            .unwrap();
        assert_eq!(
            ledger
                .connection
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM shop_game_reward WHERE kind='streak'",
                    [],
                    |row| row.get(0),
                )
                .unwrap(),
            0
        );

        let old_cycle = ledger.planet_cycle_id().unwrap();
        ledger
            .reset_planet(
                DateTime::parse_from_rfc3339("2026-10-02T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            )
            .unwrap();
        let new_cycle = ledger.planet_cycle_id().unwrap();
        assert_ne!(old_cycle, new_cycle);

        // A new raw event arrives after reset but its occurrence belongs to yesterday in the old cycle.
        add_event(
            &mut ledger,
            "late-old-cycle-day-two",
            "2026-10-01T10:00:00Z",
            10,
        );
        ledger.rebuild_shop_contributions().unwrap();
        let late_cycle: String = ledger
            .connection
            .query_row(
                "SELECT cycle_id FROM shop_activity_day WHERE reward_date='2026-10-01'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(late_cycle, old_cycle);
        ledger
            .settle_guest_rewards(
                DateTime::parse_from_rfc3339("2026-10-02T00:01:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            )
            .unwrap();
        assert_eq!(
            ledger
                .connection
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM shop_game_reward WHERE kind='streak'",
                    [],
                    |row| row.get(0),
                )
                .unwrap(),
            0
        );

        // The previous old-cycle day may establish continuity for an active day in the new cycle.
        add_effect_history(
            &ledger,
            3,
            "2026-10-02T00:00:00Z",
            ActiveEffects {
                streak_reward_tokens: 70_000,
                ..ActiveEffects::default()
            },
        );
        add_event(
            &mut ledger,
            "new-cycle-day-three",
            "2026-10-02T10:00:00Z",
            10,
        );
        ledger
            .settle_guest_rewards(
                DateTime::parse_from_rfc3339("2026-10-03T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            )
            .unwrap();
        assert_eq!(
            ledger
                .connection
                .query_row::<i64, _, _>(
                    "SELECT coalesce(sum(amount),0) FROM shop_game_reward WHERE kind='streak'",
                    [],
                    |row| row.get(0),
                )
                .unwrap(),
            70_000
        );
        assert_eq!(
            ledger
                .connection
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM shop_game_reward WHERE kind='streak' AND cycle_id=?1",
                    [&new_cycle],
                    |row| row.get(0),
                )
                .unwrap(),
            1
        );
    }

    #[test]
    fn logged_in_shop_effects_are_not_settled_from_local_usage() {
        let mut ledger = ledger();
        add_event(&mut ledger, "logged-event", "2026-09-25T18:00:00Z", 100_000);
        ledger.ensure_planet_account("alice").unwrap();
        let account: String = ledger
            .connection
            .query_row(
                "SELECT value FROM setting WHERE key='planet_account_id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let cycle: String = ledger
            .connection
            .query_row(
                "SELECT value FROM setting WHERE key='planet_current_cycle_id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
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
        assert!(
            (growth["2026-09-25"] - crate::growth::contribution_credit(100_000, 100_000.0)).abs()
                < 1e-12
        );
        ledger.rebuild_shop_contributions().unwrap();
        ledger
            .settle_guest_rewards(
                DateTime::parse_from_rfc3339("2026-09-29T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            )
            .unwrap();
        assert_eq!(
            ledger
                .settle_guest_cycle_tokens(
                    &cycle,
                    DateTime::parse_from_rfc3339("2026-09-29T00:00:00Z")
                        .unwrap()
                        .with_timezone(&Utc)
                )
                .unwrap(),
            0
        );
        let (contributions, rewards, credits): (i64, i64, i64) = ledger
            .connection
            .query_row(
                "SELECT (SELECT count(*) FROM shop_effect_contribution WHERE account_id=?1),
             (SELECT count(*) FROM shop_game_reward WHERE account_id=?1),
             (SELECT count(*) FROM shop_wallet_credit WHERE account_id=?1)",
                [&account],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!((contributions, rewards, credits), (1, 0, 0));
    }
}
