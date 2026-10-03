begin;
create extension if not exists pgtap with schema extensions;
\ir fixtures/planet.inc
select no_plan();

select ok(to_regprocedure('public.get_my_shop_effect_timeline()') is not null,
  'the authenticated server effect timeline RPC exists');
select ok(has_function_privilege(
    'authenticated', 'public.get_my_shop_effect_timeline()', 'EXECUTE'
  ), 'authenticated clients can execute the timeline RPC');
select ok(not has_function_privilege(
    'anon', 'public.get_my_shop_effect_timeline()', 'EXECUTE'
  ), 'anonymous clients cannot execute the timeline RPC');
select ok(not has_function_privilege(
    'service_role', 'public.get_my_shop_effect_timeline()', 'EXECUTE'
  ), 'service role has no direct timeline RPC grant');
select ok(not has_table_privilege(
    'authenticated', 'private.shop_effect_history', 'SELECT'
  ), 'authenticated clients cannot read private effect history directly');
set local role anon;
select throws_ok($$select public.get_my_shop_effect_timeline()$$,
  '42501', null, 'anonymous invocation of the timeline RPC is denied');
reset role;

insert into auth.users(id) values
  ('00000000-0000-0000-0000-000000001208'),
  ('00000000-0000-0000-0000-000000001209'),
  ('00000000-0000-0000-0000-000000001210');
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000001210', true);
select throws_ok($$select public.get_my_shop_effect_timeline()$$,
  '55000', 'planet state is unavailable',
  'an authenticated account without a planet receives an explicit state-unavailable error');
reset role;

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000001208', true);
select lives_ok($$select public.upsert_my_planet_state(
  pg_temp.planet_state('Timeline Owner', 'timeline-cycle'),
  pg_temp.planet_device('30000000-0000-0000-0000-000000001208', 'timeline-cycle', 0)
    || jsonb_build_object(
      'canonical_version', 0,
      'daily_segments', '[]'::jsonb,
      'activity_days', '[]'::jsonb
    )
)$$, 'the authenticated fixture creates a server-owned current cycle');
reset role;
delete from private.shop_cycle_effect_baseline
where user_id = '00000000-0000-0000-0000-000000001208'
  and cycle_id = 'timeline-cycle';
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000001208', true);
create temporary table empty_timeline as
select public.get_my_shop_effect_timeline() as value;
select is((select value->>'account_id' from empty_timeline),
  '00000000-0000-0000-0000-000000001208',
  'timeline account id is the authenticated raw UUID');
select is((select value->>'current_cycle_id' from empty_timeline), 'timeline-cycle',
  'timeline identifies the authenticated current cycle');
select is((select (value->>'effect_revision')::bigint from empty_timeline), 0::bigint,
  'an account without effect changes starts at global revision zero');
select is((select value->>'reward_timezone' from empty_timeline), 'Asia/Seoul',
  'timeline returns the account frozen reward timezone');
select is((select jsonb_array_length(value->'intervals') from empty_timeline), 0,
  'a zero-revision timeline does not invent an effect interval');
select is((select jsonb_array_length(value->'cycle_bounds') from empty_timeline), 1,
  'timeline bootstraps only the current server-owned cycle bound');
select is((select value->'cycle_bounds'->0->>'started_at_utc' from empty_timeline),
  public.get_my_planet_state()->>'cycle_started_at_utc',
  'the bootstrapped cycle bound uses the server cycle start');
select ok((select (value->>'server_time_utc')::timestamptz >=
    (value->'cycle_bounds'->0->>'started_at_utc')::timestamptz
  from empty_timeline), 'server time is returned in UTC after the lock');
reset role;
select is((select count(*)::integer from private.shop_cycle_effect_baseline b
  where b.user_id = '00000000-0000-0000-0000-000000001208'
    and b.cycle_id = 'timeline-cycle'), 1,
  'the current server cycle baseline is persisted exactly once');

create temporary table reset_cycle_fixture as
select clock_timestamp() - interval '48 hours' as cycle_started_at;
update public.planet_member_state p
set cycle_started_at = f.cycle_started_at,
    last_reset_at = f.cycle_started_at,
    reset_available_at = f.cycle_started_at + interval '18 hours'
from reset_cycle_fixture f
where p.user_id = '00000000-0000-0000-0000-000000001208';
update private.shop_cycle_effect_baseline b
set started_at = f.cycle_started_at
from reset_cycle_fixture f
where b.user_id = '00000000-0000-0000-0000-000000001208'
  and b.cycle_id = 'timeline-cycle';
insert into private.shop_cycle_effect_baseline(user_id, cycle_id, started_at)
select '00000000-0000-0000-0000-000000001208', 'pre005-open-cycle',
  f.cycle_started_at - interval '24 hours'
from reset_cycle_fixture f;
insert into private.shop_effect_history(
  user_id, cycle_id, revision, started_at, active_instance_ids, effects
)
select '00000000-0000-0000-0000-000000001208', 'pre005-open-cycle', 1,
  f.cycle_started_at - interval '23 hours', '[]'::jsonb,
  jsonb_build_object(
    'token_earning_bps', 0, 'civilization_growth_bps', 0,
    'shop_discount_bps', 0, 'reset_cooldown_bps', 0,
    'natural_removal_discount_bps', 0, 'era_reward_tokens', 0,
    'streak_reward_tokens', 0
  )
from reset_cycle_fixture f;

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000001208', true);
create temporary table timeline_reset_result as
select public.reset_my_planet(
  '40000000-0000-0000-0000-000000001208', 'timeline-cycle'
) as value;
create temporary table timeline_after_reset as
select public.get_my_shop_effect_timeline() as value;
select is((select value->'action'->>'status' from timeline_reset_result), 'reset',
  'the authenticated reset closes and advances the server cycle');
select is((select value->>'current_cycle_id' from timeline_after_reset),
  (select value->'planet_state'->>'current_cycle_id' from timeline_reset_result),
  'timeline follows the server-created current cycle');
select is((select (value->>'effect_revision')::bigint from timeline_after_reset), 2::bigint,
  'reset advances the account-global effect revision');
select is((select jsonb_array_length(value->'cycle_bounds') from timeline_after_reset), 2,
  'timeline omits a pre-005 open bound and returns the closed old and open current bounds');
select ok((select
    (value->'cycle_bounds'->0->>'ended_at_utc') is not null
    and (value->'cycle_bounds'->1->>'ended_at_utc') is null
    and (value->'cycle_bounds'->0->>'ended_at_utc')::timestamptz =
      (value->'cycle_bounds'->1->>'started_at_utc')::timestamptz
  from timeline_after_reset), 'reset closes the old bound at the new bound start');
select is((select jsonb_array_length(value->'intervals') from timeline_after_reset), 1,
  'timeline omits the pre-005 open effect interval and returns the reset snapshot');
select is((select (value->'intervals'->0->>'revision')::bigint from timeline_after_reset), 2::bigint,
  'reset effect history carries the global revision');
select is((select value->'intervals'->0->>'cycle_id' from timeline_after_reset),
  (select value->>'current_cycle_id' from timeline_after_reset),
  'the reset effect snapshot belongs to the current cycle');
select is((select count(*)::integer
  from timeline_after_reset t,
    jsonb_object_keys(t.value->'intervals'->0->'effects') as effect_key), 7,
  'effect snapshots return all seven server-calculated effect fields');
select is((select value->>'reward_timezone' from timeline_after_reset), 'Asia/Seoul',
  'reset preserves the account frozen reward timezone');
reset role;
select ok((select b.ended_at is null from private.shop_cycle_effect_baseline b
  where b.user_id = '00000000-0000-0000-0000-000000001208'
    and b.cycle_id = 'pre005-open-cycle'),
  'reading the timeline does not invent a legacy cycle end time');
select ok((select h.ended_at is null from private.shop_effect_history h
  where h.user_id = '00000000-0000-0000-0000-000000001208'
    and h.cycle_id = 'pre005-open-cycle' and h.revision = 1),
  'reading the timeline leaves ambiguous legacy effect history untouched');

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000001209', true);
select lives_ok($$select public.upsert_my_planet_state(
  pg_temp.planet_state('Timeline Other', 'other-timeline-cycle'),
  pg_temp.planet_device('30000000-0000-0000-0000-000000001209', 'other-timeline-cycle', 0)
    || jsonb_build_object(
      'canonical_version', 0,
      'daily_segments', '[]'::jsonb,
      'activity_days', '[]'::jsonb
    )
)$$, 'a second account creates an independent timeline');
create temporary table foreign_timeline as
select public.get_my_shop_effect_timeline() as value;
select is((select value->>'account_id' from foreign_timeline),
  '00000000-0000-0000-0000-000000001209', 'timeline identity follows auth.uid');
select is((select value->>'current_cycle_id' from foreign_timeline), 'other-timeline-cycle',
  'a second account receives only its own current cycle');
select is((select (value->>'effect_revision')::bigint from foreign_timeline), 0::bigint,
  'a second account cannot observe another account revision');
select is((select jsonb_array_length(value->'intervals') from foreign_timeline), 0,
  'a second account cannot observe another account effect history');
select is((select jsonb_array_length(value->'cycle_bounds') from foreign_timeline), 1,
  'a second account receives only its own baseline');
reset role;

insert into private.shop_cycle_effect_baseline(user_id, cycle_id, started_at)
select '00000000-0000-0000-0000-000000001209', 'pre005-orphan-cycle',
  p.cycle_started_at - interval '24 hours'
from public.planet_member_state p
where p.user_id = '00000000-0000-0000-0000-000000001209';
insert into private.shop_effect_history(
  user_id, cycle_id, revision, started_at, active_instance_ids, effects
)
select '00000000-0000-0000-0000-000000001209', 'pre005-orphan-cycle', 7,
  p.cycle_started_at - interval '23 hours', '[]'::jsonb,
  jsonb_build_object(
    'token_earning_bps', 0, 'civilization_growth_bps', 0,
    'shop_discount_bps', 0, 'reset_cooldown_bps', 0,
    'natural_removal_discount_bps', 0, 'era_reward_tokens', 0,
    'streak_reward_tokens', 0
  )
from public.planet_member_state p
where p.user_id = '00000000-0000-0000-0000-000000001209';
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000001209', true);
create temporary table timeline_orphan_only as
select public.get_my_shop_effect_timeline() as value;
select is((select (value->>'effect_revision')::bigint from timeline_orphan_only), 7::bigint,
  'timeline preserves the account-global highwater of hidden legacy history');
select is((select jsonb_array_length(value->'intervals') from timeline_orphan_only), 0,
  'a noncurrent open legacy interval is omitted when no current interval exists');
select is((select jsonb_array_length(value->'cycle_bounds') from timeline_orphan_only), 1,
  'a noncurrent open legacy bound is omitted without altering the current bound');
reset role;
select ok((select b.ended_at is null from private.shop_cycle_effect_baseline b
  where b.user_id = '00000000-0000-0000-0000-000000001209'
    and b.cycle_id = 'pre005-orphan-cycle'),
  'timeline does not infer an end time for an unknown historical bound');
select ok((select h.ended_at is null from private.shop_effect_history h
  where h.user_id = '00000000-0000-0000-0000-000000001209'
    and h.cycle_id = 'pre005-orphan-cycle' and h.revision = 7),
  'timeline does not mutate hidden historical effect rows');

delete from private.shop_cycle_effect_baseline
where user_id = '00000000-0000-0000-0000-000000001208'
  and cycle_id in ('timeline-cycle', 'pre005-open-cycle');
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000001208', true);
create temporary table timeline_unknown_old_bound as
select public.get_my_shop_effect_timeline() as value;
select is((select jsonb_array_length(value->'cycle_bounds') from timeline_unknown_old_bound), 1,
  'timeline does not invent a missing historical cycle bound');
select is((select value->'cycle_bounds'->0->>'cycle_id' from timeline_unknown_old_bound),
  (select value->>'current_cycle_id' from timeline_unknown_old_bound),
  'only the current server-owned cycle is bootstrapped');
select is((select (value->>'effect_revision')::bigint from timeline_unknown_old_bound), 2::bigint,
  'omitting an unknown old bound preserves recorded global effect history');
reset role;

select * from finish();
rollback;
