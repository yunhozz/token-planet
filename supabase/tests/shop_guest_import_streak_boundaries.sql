begin;
create extension if not exists pgtap with schema extensions;
select no_plan();
\ir fixtures/shop_guest_import.inc
\ir fixtures/shop_guest_import_streak_boundaries.inc

with source as materialized (
  select pg_temp.shop_guest_import_streak_cross_cycle_request(
    'f1000000-0000-4000-a000-000000000081',
    'f0000000-0000-4000-a000-000000000001', 'current-after-streak-reset'
  )#>'{snapshot,data}' as data
), normalized as (
  select data, pg_temp.shop_guest_import_boundary_try_source_normalize(data) as result
  from source
)
select ok(
  result->>'stage' = 'ready'
    and result#>>'{usage,validation_scope}' = 'usage_effect_activity_consistency'
    and result#>>'{resets,validation_scope}' = 'reset_settlement_consistency'
    and data#>>'{activity_days,0,cycle_id}' = 'history-cycle'
    and data#>>'{activity_days,0,reward_date}' = '2026-09-30'
    and data#>>'{activity_days,1,cycle_id}' = 'current-after-streak-reset'
    and data#>>'{activity_days,1,reward_date}' = '2026-10-01'
    and result#>>'{rewards,validation_scope}' = 'reward_wallet_streak_consistency'
    and result#>>'{rewards,streak_mirrors,0,cycle_id}' = 'current-after-streak-reset'
    and (result#>>'{rewards,streak_mirrors,0,amount}')::bigint = 10000,
  'a positive prior local date in the preceding cycle supports a current-cycle streak mirror'
)
from normalized;

with fixtures as materialized (
  select 'spring-forward-23h'::text as boundary,
    pg_temp.shop_guest_import_streak_dst_request(
      'f1000000-0000-4000-a000-000000000091',
      'f0000000-0000-4000-a000-000000000001', 'dst-spring-cycle',
      'America/New_York', '2026-03-07', '2026-03-08',
      '2026-03-07T05:00:00+00:00', '2026-03-07T05:00:03+00:00',
      '2026-03-07T05:00:01+00:00', '2026-03-07T09:30:00+00:00',
      '2026-03-08T08:30:00+00:00', '2026-03-08T08:30:04+00:00',
      '2026-03-08T08:30:05+00:00'
    )#>'{snapshot,data}' as data, 82800::bigint as expected_gap
  union all
  select 'fall-back-25h'::text,
    pg_temp.shop_guest_import_streak_dst_request(
      'f1000000-0000-4000-a000-000000000092',
      'f0000000-0000-4000-a000-000000000001', 'dst-fall-cycle',
      'America/New_York', '2026-10-31', '2026-11-01',
      '2026-10-31T04:00:00+00:00', '2026-10-31T04:00:03+00:00',
      '2026-10-31T04:00:01+00:00', '2026-10-31T08:30:00+00:00',
      '2026-11-01T09:30:00+00:00', '2026-11-01T09:30:04+00:00',
      '2026-11-01T09:30:05+00:00'
    )#>'{snapshot,data}' as data, 90000::bigint
), normalized as (
  select boundary, expected_gap, data,
    pg_temp.shop_guest_import_boundary_try_source_normalize(data) as result
  from fixtures
)
select ok(
  result->>'stage' = 'ready'
    and result#>>'{usage,validation_scope}' = 'usage_effect_activity_consistency'
    and result#>>'{resets,validation_scope}' = 'reset_settlement_consistency'
    and result#>>'{rewards,validation_scope}' = 'reward_wallet_streak_consistency'
    and data->>'reward_timezone' = 'America/New_York'
    and extract(epoch from (
      (data#>>'{activity_days,1,first_occurred_at_utc}')::timestamptz
      - (data#>>'{activity_days,0,first_occurred_at_utc}')::timestamptz
    ))::bigint = expected_gap
    and (result#>>'{rewards,streak_mirrors,0,amount}')::bigint = 10000,
  boundary || ' consecutive reward dates normalize at the expected elapsed hours'
)
from normalized
order by boundary;

with source as materialized (
  select pg_temp.shop_guest_import_streak_half_open_request(
    'f1000000-0000-4000-a000-000000000093',
    'f0000000-0000-4000-a000-000000000001', 'current-half-open-cycle'
  )#>'{snapshot,data}' as data
), normalized as (
  select data, pg_temp.shop_guest_import_boundary_try_source_normalize(data) as result
  from source
)
select ok(
  result->>'stage' = 'ready'
    and data#>>'{activity_days,1,first_occurred_at_utc}'
      = data#>>'{effect_history,2,ended_at_utc}'
    and data#>>'{activity_days,1,first_occurred_at_utc}'
      = data#>>'{effect_history,3,started_at_utc}'
    and result#>>'{rewards,validation_scope}' = 'reward_wallet_streak_consistency'
    and (result#>>'{rewards,streak_mirrors,0,amount}')::bigint = 10000,
  'activity exactly at the next effect interval start uses the new interval snapshot'
)
from normalized;

with source as materialized (
  select pg_temp.shop_guest_import_streak_half_open_request(
    'f1000000-0000-4000-a000-000000000094',
    'f0000000-0000-4000-a000-000000000001', 'current-half-open-cycle'
  )#>'{snapshot,data}' as data
), wrong_snapshot as (
  select jsonb_set(data, '{game_rewards,1,effects}', jsonb_build_object(
    'token_earning_bps', 0,
    'civilization_growth_bps', 0,
    'shop_discount_bps', 0,
    'reset_cooldown_bps', 0,
    'natural_removal_discount_bps', 0,
    'era_reward_tokens', 0,
    'streak_reward_tokens', 0
  ), true) as data
  from source
), normalized as (
  select data, pg_temp.shop_guest_import_boundary_try_source_normalize(data) as result
  from wrong_snapshot
)
select ok(
  result->>'stage' = 'ready'
    and result#>>'{resets,validation_scope}' = 'reset_settlement_consistency'
    and data#>>'{game_rewards,1,amount}' = '10000'
    and data#>>'{wallet_credits,1,amount}' = '10000'
    and result->'rewards' = 'null'::jsonb,
  'the old effect snapshot cannot support a positive streak award at the half-open boundary'
)
from normalized;

with source as materialized (
  select pg_temp.shop_guest_import_single_reset_streak_request(
    'f1000000-0000-4000-a000-000000000082',
    'f0000000-0000-4000-a000-000000000001', 'current-after-streak-reset'
  )#>'{snapshot,data}' as data
), normalized as (
  select data, pg_temp.shop_guest_import_boundary_try_source_normalize(data) as result
  from source
)
select ok(
  result->>'stage' = 'ready'
    and result#>>'{resets,validation_scope}' = 'reset_settlement_consistency'
    and data#>>'{effect_cycle_bounds,0,ended_at_utc}' = '2026-10-01T12:00:00+00:00'
    and data#>>'{game_rewards,1,awarded_at_utc}' = '2026-10-01T12:00:00+00:00'
    and result#>>'{rewards,validation_scope}' = 'reward_wallet_streak_consistency'
    and result#>>'{rewards,streak_mirrors,0,cycle_id}' = 'history-cycle'
    and (result#>>'{rewards,streak_mirrors,0,amount}')::bigint = 10000,
  'an old-cycle streak award exactly at cycle end is included in the mirror'
)
from normalized;

with source as materialized (
  select pg_temp.shop_guest_import_single_reset_streak_request(
    'f1000000-0000-4000-a000-000000000083',
    'f0000000-0000-4000-a000-000000000001', 'current-after-streak-reset'
  )#>'{snapshot,data}' as data
), after_end as (
  select jsonb_set(
    jsonb_set(data, '{game_rewards,1,awarded_at_utc}',
      '"2026-10-01T12:00:00.000001+00:00"'::jsonb, true),
    '{wallet_credits,1,created_at_utc}',
    '"2026-10-01T12:00:00.000001+00:00"'::jsonb, true
  ) as data
  from source
), normalized as (
  select data, pg_temp.shop_guest_import_boundary_try_source_normalize(data) as result
  from after_end
)
select ok(
  result->>'stage' = 'ready'
    and result#>>'{resets,validation_scope}' = 'reset_settlement_consistency'
    and result->'rewards' = 'null'::jsonb,
  'an old-cycle streak award one microsecond after cycle end is rejected after upstream validation'
)
from normalized;

select * from finish();
rollback;
