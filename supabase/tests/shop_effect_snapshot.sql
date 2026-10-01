begin;
create extension if not exists pgtap with schema extensions;
select no_plan();
\ir fixtures/planet.inc

insert into auth.users(id) values ('00000000-0000-0000-0000-000000000968');
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000968', true);
select lives_ok($$select public.upsert_my_planet_state(
  pg_temp.planet_state('Snapshot Owner', 'snapshot-cycle'),
  pg_temp.planet_device('30000000-0000-0000-0000-000000000968', 'snapshot-cycle', 100000)
)$$, 'raw device state is initialized through the existing authenticated RPC');
reset role;

insert into private.shop_effect_history(
  user_id, cycle_id, revision, started_at, ended_at, active_instance_ids, effects
) values (
  '00000000-0000-0000-0000-000000000968', 'snapshot-cycle', 1,
  '2026-09-30T12:00:00Z', null, '[]'::jsonb,
  '{"token_earning_bps":3000,"civilization_growth_bps":2000,"shop_discount_bps":0,"reset_cooldown_bps":0,"natural_removal_discount_bps":0,"era_reward_tokens":0,"streak_reward_tokens":0}'::jsonb
);

select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000968', true);
select ok(private.shop_replace_effect_snapshot(
  '00000000-0000-0000-0000-000000000968',
  pg_temp.planet_state('Snapshot Owner', 'snapshot-cycle'),
  jsonb_build_object(
    'device_id', '30000000-0000-0000-0000-000000000968',
    'current_cycle_id', 'snapshot-cycle',
    'lifetime_tokens', 100000,
    'current_planet_tokens', 100000,
    'daily_tokens', '{"2026-09-30":100000}'::jsonb,
    'incomplete', false,
    'canonical_version', 1,
    'daily_segments', '[{"cycle_id":"snapshot-cycle","date":"2026-09-30","effect_revision":0,"tokens":50000},{"cycle_id":"snapshot-cycle","date":"2026-09-30","effect_revision":1,"tokens":50000}]'::jsonb,
    'activity_days', '[{"cycle_id":"snapshot-cycle","reward_date":"2026-09-30","first_occurred_at_utc":"2026-09-30T13:00:00Z","tokens":100000}]'::jsonb
  )
), 'first valid canonical version atomically replaces the device snapshot');
select is((select array_agg(growth_bps order by effect_revision)
  from private.shop_effect_contribution
  where user_id = '00000000-0000-0000-0000-000000000968'),
  array[0,2000]::smallint[], 'growth rates come from the account cycle effect history');
select is((select array_agg(wallet_bps order by effect_revision)
  from private.shop_effect_contribution
  where user_id = '00000000-0000-0000-0000-000000000968'),
  array[0,3000]::smallint[], 'wallet rates come from the account cycle effect history');
select is((select count(*)::bigint from private.shop_device_activity_day
  where user_id = '00000000-0000-0000-0000-000000000968'), 1::bigint,
  'activity days are stored with the canonical device version');
select is((select canonical_version from private.planet_device_state
  where user_id = '00000000-0000-0000-0000-000000000968'
    and device_id = '30000000-0000-0000-0000-000000000968'), 1::bigint,
  'the raw device row records its accepted canonical version');

select is(private.shop_replace_effect_snapshot(
  '00000000-0000-0000-0000-000000000968',
  pg_temp.planet_state('Snapshot Owner', 'snapshot-cycle'),
  jsonb_build_object(
    'device_id', '30000000-0000-0000-0000-000000000968',
    'current_cycle_id', 'snapshot-cycle', 'lifetime_tokens', 100000,
    'current_planet_tokens', 100000, 'daily_tokens', '{"2026-09-30":100000}'::jsonb,
    'incomplete', false, 'canonical_version', 1,
    'daily_segments', '[{"cycle_id":"snapshot-cycle","date":"2026-09-30","effect_revision":0,"tokens":50000},{"cycle_id":"snapshot-cycle","date":"2026-09-30","effect_revision":1,"tokens":50000}]'::jsonb,
    'activity_days', '[{"cycle_id":"snapshot-cycle","reward_date":"2026-09-30","first_occurred_at_utc":"2026-09-30T13:00:00Z","tokens":100000}]'::jsonb
  )
), false, 'identical canonical version is an idempotent replay');
select throws_ok($$select private.shop_replace_effect_snapshot(
  '00000000-0000-0000-0000-000000000968',
  pg_temp.planet_state('Snapshot Owner', 'snapshot-cycle'),
  jsonb_build_object(
    'device_id', '30000000-0000-0000-0000-000000000968',
    'current_cycle_id', 'snapshot-cycle', 'lifetime_tokens', 100000,
    'current_planet_tokens', 100000, 'daily_tokens', '{"2026-09-30":100000}'::jsonb,
    'incomplete', false, 'canonical_version', 0,
    'daily_segments', '[{"cycle_id":"snapshot-cycle","date":"2026-09-30","effect_revision":0,"tokens":100000}]'::jsonb,
    'activity_days', '[]'::jsonb
  )
)$$, '23514', null, 'lower canonical versions are rejected');
select throws_ok($$select private.shop_replace_effect_snapshot(
  '00000000-0000-0000-0000-000000000968',
  pg_temp.planet_state('Snapshot Owner', 'snapshot-cycle'),
  jsonb_build_object(
    'device_id', '30000000-0000-0000-0000-000000000968',
    'current_cycle_id', 'snapshot-cycle', 'lifetime_tokens', 100000,
    'current_planet_tokens', 100000, 'daily_tokens', '{"2026-09-30":100000}'::jsonb,
    'incomplete', false, 'canonical_version', 1,
    'daily_segments', '[{"cycle_id":"snapshot-cycle","date":"2026-09-30","effect_revision":0,"tokens":100000}]'::jsonb,
    'activity_days', '[]'::jsonb
  )
)$$, '23514', null, 'same version with a different payload is rejected');

select ok(private.shop_replace_effect_snapshot(
  '00000000-0000-0000-0000-000000000968',
  pg_temp.planet_state('Snapshot Owner', 'snapshot-cycle'),
  jsonb_build_object(
    'device_id', '30000000-0000-0000-0000-000000000968',
    'current_cycle_id', 'snapshot-cycle', 'lifetime_tokens', 100000,
    'current_planet_tokens', 100000, 'daily_tokens', '{"2026-09-30":100000}'::jsonb,
    'incomplete', false, 'canonical_version', 2,
    'daily_segments', '[{"cycle_id":"snapshot-cycle","date":"2026-09-30","effect_revision":0,"tokens":100000}]'::jsonb,
    'activity_days', '[]'::jsonb
  )
), 'higher version replaces rather than appends contributions and activity');
select is((select count(*)::bigint from private.shop_effect_contribution
  where user_id = '00000000-0000-0000-0000-000000000968'), 1::bigint,
  'replacement leaves exactly the current segment set');
select is((select sum(growth_bps)::integer from private.shop_effect_contribution
  where user_id = '00000000-0000-0000-0000-000000000968'), 0,
  'replacement removes the prior growth bonus');
select is((select count(*)::bigint from private.shop_device_activity_day
  where user_id = '00000000-0000-0000-0000-000000000968'), 0::bigint,
  'replacement removes prior activity rows');
select ok(not has_function_privilege(
  'authenticated', 'private.shop_replace_effect_snapshot(uuid,jsonb,jsonb)', 'execute'
), 'snapshot helper is not executable by API roles');

select * from finish();
rollback;
