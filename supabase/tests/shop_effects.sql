begin;
create extension if not exists pgtap with schema extensions;
select no_plan();
\ir fixtures/planet.inc

insert into auth.users(id) values
  ('00000000-0000-0000-0000-000000000966'),
  ('00000000-0000-0000-0000-000000000968'),
  ('00000000-0000-0000-0000-000000000970'),
  ('00000000-0000-0000-0000-000000000972'),
  ('00000000-0000-0000-0000-000000000974'),
  ('00000000-0000-0000-0000-000000000976'),
  ('00000000-0000-0000-0000-000000000978');
insert into private.shop_effect_history(
  user_id, cycle_id, revision, started_at, ended_at, active_instance_ids, effects
) values (
  '00000000-0000-0000-0000-000000000966', 'effects-cycle', 1,
  '2026-09-30T12:00:00Z', null, '[]'::jsonb,
  '{"token_earning_bps":0,"civilization_growth_bps":2000,"shop_discount_bps":0,"reset_cooldown_bps":0,"natural_removal_discount_bps":0,"era_reward_tokens":0,"streak_reward_tokens":0}'::jsonb
);
insert into private.shop_effect_history(
  user_id, cycle_id, revision, started_at, ended_at, active_instance_ids, effects
) values (
  '00000000-0000-0000-0000-000000000976', 'threshold-cycle', 1,
  '2026-09-30T12:00:00Z', null, '[]'::jsonb,
  '{"token_earning_bps":0,"civilization_growth_bps":2000,"shop_discount_bps":0,"reset_cooldown_bps":0,"natural_removal_discount_bps":0,"era_reward_tokens":0,"streak_reward_tokens":0}'::jsonb
);

create function pg_temp.upload_effect_contribution(
  p_version bigint,
  p_current_tokens bigint,
  p_lifetime_tokens bigint,
  p_daily_tokens jsonb,
  p_daily_segments jsonb,
  p_activity_days jsonb
) returns jsonb
language sql as $$
  select public.upsert_my_planet_state(
    pg_temp.planet_state('Effect Contributor', 'effects-cycle'),
    jsonb_build_object(
      'device_id', '30000000-0000-0000-0000-000000000966',
      'current_cycle_id', 'effects-cycle',
      'lifetime_tokens', p_lifetime_tokens,
      'current_planet_tokens', p_current_tokens,
      'daily_tokens', p_daily_tokens,
      'incomplete', false,
      'canonical_version', p_version,
      'daily_segments', p_daily_segments,
      'activity_days', p_activity_days
    )
  );
$$;

create function pg_temp.upload_untrusted_planet_objects(p_objects jsonb)
returns jsonb
language sql as $$
  select public.upsert_my_planet_state(
    jsonb_set(
      pg_temp.planet_state('Untrusted Objects', 'untrusted-objects-cycle'),
      '{objects}', p_objects, true
    ),
    jsonb_build_object(
      'device_id', '30000000-0000-0000-0000-000000000968',
      'current_cycle_id', 'untrusted-objects-cycle',
      'lifetime_tokens', 0,
      'current_planet_tokens', 0,
      'daily_tokens', '{}'::jsonb,
      'incomplete', false,
      'canonical_version', 1,
      'daily_segments', '[]'::jsonb,
      'activity_days', '[]'::jsonb
    )
  );
$$;

create function pg_temp.upload_threshold_contribution(
  p_device_id uuid, p_version bigint, p_tokens bigint
) returns jsonb
language sql as $$
  select public.upsert_my_planet_state(
    pg_temp.planet_state('Threshold Owner', 'threshold-cycle'),
    jsonb_build_object(
      'device_id', p_device_id,
      'current_cycle_id', 'threshold-cycle',
      'lifetime_tokens', p_tokens,
      'current_planet_tokens', p_tokens,
      'daily_tokens', case when p_tokens = 0 then '{}'::jsonb
        else jsonb_build_object('2026-09-30', p_tokens) end,
      'incomplete', false,
      'canonical_version', p_version,
      'daily_segments', case when p_tokens = 0 then '[]'::jsonb else jsonb_build_array(
        jsonb_build_object('cycle_id', 'threshold-cycle', 'date', '2026-09-30',
          'effect_revision', 1, 'tokens', p_tokens)
      ) end,
      'activity_days', case when p_tokens = 0 then '[]'::jsonb else jsonb_build_array(
        jsonb_build_object('cycle_id', 'threshold-cycle', 'reward_date', '2026-09-30',
          'first_occurred_at_utc', '2026-09-30T13:00:00Z', 'tokens', p_tokens)
      ) end
    )
  );
$$;

create function pg_temp.upload_transition_contribution(p_tokens bigint)
returns jsonb
language plpgsql as $$
declare
  v_cycle_id text := current_setting('shop_test.transition_cycle');
  v_effect_revision bigint := current_setting('shop_test.transition_revision')::bigint;
  v_version bigint := current_setting('shop_test.transition_version')::bigint;
  v_occurred_at timestamptz := clock_timestamp();
  v_day text;
begin
  v_day := to_char(v_occurred_at at time zone 'Asia/Seoul', 'YYYY-MM-DD');
  return public.upsert_my_planet_state(
    pg_temp.planet_state('Transition Owner', v_cycle_id, v_occurred_at),
    jsonb_build_object(
      'device_id', '30000000-0000-0000-0000-000000000970',
      'current_cycle_id', v_cycle_id,
      'lifetime_tokens', p_tokens,
      'current_planet_tokens', p_tokens,
      'daily_tokens', jsonb_build_object(v_day, p_tokens),
      'incomplete', false,
      'canonical_version', v_version,
      'daily_segments', jsonb_build_array(jsonb_build_object(
        'cycle_id', v_cycle_id, 'date', v_day,
        'effect_revision', v_effect_revision, 'tokens', p_tokens
      )),
      'activity_days', jsonb_build_array(jsonb_build_object(
        'cycle_id', v_cycle_id, 'reward_date', v_day,
        'first_occurred_at_utc', v_occurred_at, 'tokens', p_tokens
      ))
    )
  );
end;
$$;

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000966', true);
select lives_ok($$select pg_temp.upload_effect_contribution(
  1, 100000, 100000, '{"2026-09-30":100000}'::jsonb,
  '[
    {"cycle_id":"effects-cycle","date":"2026-09-30","effect_revision":0,"tokens":50000},
    {"cycle_id":"effects-cycle","date":"2026-09-30","effect_revision":1,"tokens":50000}
  ]'::jsonb,
  '[{"cycle_id":"effects-cycle","reward_date":"2026-09-30","first_occurred_at_utc":"2026-09-30T13:00:00Z","tokens":100000}]'::jsonb)$$,
  'canonical contribution uploads raw totals, effect segments, and activity day');
reset role;

select ok(abs((public.get_my_planet_state()->>'growth_credit')::numeric - 1.1) < 0.000001,
  'server weights same-day growth by the effect active for each segment');
select is((public.get_my_planet_state()->>'current_planet_tokens')::bigint, 100000::bigint,
  'effect bonuses do not inflate raw token totals');
select is((select array_agg(growth_bps order by effect_revision)
  from private.shop_effect_contribution
  where user_id = '00000000-0000-0000-0000-000000000966'),
  array[0,2000]::smallint[], 'growth rates are resolved from server effect history');
select is((select count(*)::bigint from private.shop_device_activity_day
  where user_id = '00000000-0000-0000-0000-000000000966'), 1::bigint,
  'first activity day is recorded with the canonical device snapshot');
select is((select reward_timezone from private.shop_account_state
  where user_id = '00000000-0000-0000-0000-000000000966'), 'Asia/Seoul',
  'first planet timezone is frozen for reward-date validation');

select lives_ok($$select pg_temp.upload_effect_contribution(
  1, 100000, 100000, '{"2026-09-30":100000}'::jsonb,
  '[
    {"cycle_id":"effects-cycle","date":"2026-09-30","effect_revision":0,"tokens":50000},
    {"cycle_id":"effects-cycle","date":"2026-09-30","effect_revision":1,"tokens":50000}
  ]'::jsonb,
  '[{"cycle_id":"effects-cycle","reward_date":"2026-09-30","first_occurred_at_utc":"2026-09-30T13:00:00Z","tokens":100000}]'::jsonb)$$,
  'same contribution version and payload safely replays');
select is((select count(*)::bigint from private.shop_effect_contribution
  where user_id = '00000000-0000-0000-0000-000000000966'), 2::bigint,
  'replay does not duplicate effect segments');

select throws_ok($$select pg_temp.upload_effect_contribution(
  0, 100000, 100000, '{"2026-09-30":100000}'::jsonb,
  '[{"cycle_id":"effects-cycle","date":"2026-09-30","effect_revision":0,"tokens":100000}]'::jsonb,
  '[{"cycle_id":"effects-cycle","reward_date":"2026-09-30","first_occurred_at_utc":"2026-09-30T13:00:00Z","tokens":100000}]'::jsonb)$$,
  '23514', null, 'regressing canonical contribution version is rejected');
select throws_ok($$select pg_temp.upload_effect_contribution(
  1, 100000, 100000, '{"2026-09-30":100000}'::jsonb,
  '[{"cycle_id":"effects-cycle","date":"2026-09-30","effect_revision":0,"tokens":100000}]'::jsonb,
  '[{"cycle_id":"effects-cycle","reward_date":"2026-09-30","first_occurred_at_utc":"2026-09-30T13:00:00Z","tokens":100000}]'::jsonb)$$,
  '23514', null, 'same canonical version cannot replace a different payload');
select throws_ok($$select pg_temp.upload_effect_contribution(
  2, 100000, 100000, '{"2026-09-30":100000}'::jsonb,
  '[{"cycle_id":"effects-cycle","date":"2026-09-30","effect_revision":1,"tokens":99999}]'::jsonb,
  '[{"cycle_id":"effects-cycle","reward_date":"2026-09-30","first_occurred_at_utc":"2026-09-30T13:00:00Z","tokens":100000}]'::jsonb)$$,
  '23514', null, 'effect segments must sum to the raw daily tokens');
select throws_ok($$select pg_temp.upload_effect_contribution(
  2, 100000, 100000, '{"2026-09-30":100000}'::jsonb,
  '[{"cycle_id":"effects-cycle","date":"2026-09-30","effect_revision":99,"tokens":100000}]'::jsonb,
  '[{"cycle_id":"effects-cycle","reward_date":"2026-09-30","first_occurred_at_utc":"2026-09-30T13:00:00Z","tokens":100000}]'::jsonb)$$,
  '23514', null, 'unregistered effect revision is rejected');
select throws_ok($$select pg_temp.upload_effect_contribution(
  2, 100000, 100000, '{"2026-09-30":100000}'::jsonb,
  '[{"cycle_id":"another-cycle","date":"2026-09-30","effect_revision":1,"tokens":100000}]'::jsonb,
  '[{"cycle_id":"effects-cycle","reward_date":"2026-09-30","first_occurred_at_utc":"2026-09-30T13:00:00Z","tokens":100000}]'::jsonb)$$,
  '23514', null, 'effect revision cannot be borrowed from another cycle');
select throws_ok($$select pg_temp.upload_effect_contribution(
  2, 100000, 100000, '{"2026-09-30":100000}'::jsonb,
  '[{"cycle_id":"effects-cycle","date":"2026-10-01","effect_revision":1,"tokens":100000}]'::jsonb,
  '[{"cycle_id":"effects-cycle","reward_date":"2026-10-01","first_occurred_at_utc":"2026-09-30T13:00:00Z","tokens":100000}]'::jsonb)$$,
  '23514', null, 'contribution date and reward timestamp must match the frozen timezone');
select throws_ok($$select pg_temp.upload_effect_contribution(
  2, 100000, 100000, '{"2026-09-30":100000}'::jsonb,
  '[{"cycle_id":"effects-cycle","date":"2026-09-30","effect_revision":1,"tokens":100000,"growth_bps":3000}]'::jsonb,
  '[{"cycle_id":"effects-cycle","reward_date":"2026-09-30","first_occurred_at_utc":"2026-09-30T13:00:00Z","tokens":100000}]'::jsonb)$$,
  '23514', null, 'client cannot submit effect basis-point values');

select lives_ok($$select pg_temp.upload_effect_contribution(
  2, 100000, 100000, '{"2026-09-30":100000}'::jsonb,
  '[{"cycle_id":"effects-cycle","date":"2026-09-30","effect_revision":0,"tokens":100000}]'::jsonb,
  '[{"cycle_id":"effects-cycle","reward_date":"2026-09-30","first_occurred_at_utc":"2026-09-30T13:00:00Z","tokens":100000}]'::jsonb)$$,
  'higher canonical version replaces the prior contribution snapshot');
select ok(abs((public.get_my_planet_state()->>'growth_credit')::numeric - 1.0) < 0.000001,
  'replacement removes the prior effect-weighted growth bonus');
select is((select count(*)::bigint from private.shop_effect_contribution
  where user_id = '00000000-0000-0000-0000-000000000966'), 1::bigint,
  'replacement removes stale effect segments instead of appending');

select lives_ok($$select pg_temp.upload_effect_contribution(
  3, 0, 100000, '{}'::jsonb, '[]'::jsonb, '[]'::jsonb)$$,
  'higher version can replace contributions with an empty disabled-source snapshot');
select ok(abs((public.get_my_planet_state()->>'growth_credit')::numeric) < 0.000001,
  'disabled source removes its old prospective growth bonus');
select is((public.get_my_planet_state()->>'current_planet_tokens')::bigint, 0::bigint,
  'disabled source leaves no current raw tokens');
select is((select count(*)::bigint from private.shop_effect_contribution
  where user_id = '00000000-0000-0000-0000-000000000966'), 0::bigint,
  'disabled-source replacement removes previous contribution rows');

insert into private.planet_device_state(
  user_id, device_id, current_cycle_id, lifetime_tokens, current_planet_tokens,
  daily_tokens, incomplete
) values (
  '00000000-0000-0000-0000-000000000966',
  '30000000-0000-0000-0000-000000000967', 'effects-cycle', 100000, 100000,
  '{"2026-09-30":100000}'::jsonb, false
);
select lives_ok($$select pg_temp.upload_effect_contribution(
  3, 0, 100000, '{}'::jsonb, '[]'::jsonb, '[]'::jsonb)$$,
  'replaying one device still re-aggregates other raw device snapshots');
select is((public.get_my_planet_state()->>'current_planet_tokens')::bigint, 100000::bigint,
  'unsegmented device tokens remain in raw current-cycle totals');
select ok(abs((public.get_my_planet_state()->>'growth_credit')::numeric - 1.0) < 0.000001,
  'unsegmented raw tokens receive base growth with no speculative effect bonus');
select is((select count(*)::bigint from private.shop_effect_contribution
  where user_id = '00000000-0000-0000-0000-000000000966'), 0::bigint,
  'unsegmented raw device does not receive a fabricated effect contribution');

reset role;

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000968', true);
select lives_ok($$select pg_temp.upload_untrusted_planet_objects(
  '[
    {"stage":4,"ordinal":99,"kind":"rocket","seed":42,"x":99,"y":99},
    {"stage":0,"ordinal":0,"kind":"tree","seed":123,"x":10,"y":20},
    {"stage":0,"ordinal":0,"kind":"fern","seed":456,"x":11,"y":21}
  ]'::jsonb)$$,
  'canonical upload ignores forged high-stage and duplicate natural-object identities');
select is(public.get_my_planet_state()->'objects', '[]'::jsonb,
  'zero growth cannot persist client-supplied natural kinds, seeds, coordinates, or ordinals');
reset role;

insert into public.planet_member_state(
  user_id, nickname, avatar, timezone, current_cycle_id, cycle_started_at,
  current_planet_tokens, lifetime_tokens, growth_credit, stage, progress_to_next,
  incomplete, objects
) select
  '00000000-0000-0000-0000-000000000970', 'Transition Owner', 'feminine', 'Asia/Seoul',
  'transition-cycle', '2026-09-26T00:00:00Z', 0, 0, 99, 4, 1, false,
  jsonb_build_array(private.shop_canonical_planet_object('transition-cycle', 4, 0));
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000970', true);
select lives_ok($$select public.upsert_my_planet_state(
  jsonb_set(
    jsonb_set(pg_temp.planet_state('Transition Owner', 'transition-cycle'),
      '{growth_credit}', '99'::jsonb, true),
    '{stage}', '4'::jsonb, true
  ),
  jsonb_build_object(
    'device_id', '30000000-0000-0000-0000-000000000970',
    'current_cycle_id', 'transition-cycle',
    'lifetime_tokens', 0, 'current_planet_tokens', 0,
    'daily_tokens', '{}'::jsonb, 'incomplete', false,
    'canonical_version', 1, 'daily_segments', '[]'::jsonb, 'activity_days', '[]'::jsonb
  )
)$$, 'first canonical upload transitions a legacy planet row');
select is((public.get_my_planet_state()->>'growth_credit')::numeric, 0::numeric,
  'first canonical projection derives growth from server raw daily tokens');
select is((public.get_my_planet_state()->>'stage')::integer, 0,
  'first canonical projection ignores client-inflated stage');
select is(public.get_my_planet_state()->'objects', '[]'::jsonb,
  'first canonical projection does not trust hash-shaped legacy natural objects');
reset role;

select is(private.shop_project_planet_objects('hash-cycle', '[]'::jsonb, 1),
  '[{"stage":0,"ordinal":0,"kind":"rock","x":47,"y":30,"seed":14954763954862538265}]'::jsonb,
  'generated object hash attributes match Rust SHA256 and integer-shift coordinates');
select is(private.shop_canonical_planet_object('transition-cycle', 4, 0),
  '{"stage":4,"ordinal":0,"kind":"rocket","x":48,"y":41,"seed":8600109018655062522}'::jsonb,
  'canonical legacy identity attributes match Rust seed coordinate derivation');
select is(private.shop_project_planet_objects('transition-cycle', '[]'::jsonb, 116)->25,
  '{"stage":4,"ordinal":0,"kind":"rocket","x":48,"y":41,"seed":8600109018655062522}'::jsonb,
  'appended stage-four object uses Rust SHA256-derived coordinates');

insert into public.planet_member_state(
  user_id, nickname, avatar, timezone, current_cycle_id, cycle_started_at,
  current_planet_tokens, lifetime_tokens, growth_credit, stage, progress_to_next,
  incomplete, objects
) values (
  '00000000-0000-0000-0000-000000000972', 'Rollback Owner', 'feminine', 'Asia/Seoul',
  'rollback-cycle', '2026-09-26T00:00:00Z', 100000, 100000, 1, 0, 0.2, false, '[]'::jsonb
);
insert into private.planet_device_state(
  user_id, device_id, current_cycle_id, lifetime_tokens, current_planet_tokens,
  daily_tokens, incomplete, canonical_version
) values (
  '00000000-0000-0000-0000-000000000972',
  '30000000-0000-0000-0000-000000000972', 'rollback-cycle', 100000, 100000,
  '{"2026-09-30":100000}'::jsonb, false, 0
);
insert into private.shop_account_state(user_id, reward_timezone)
values ('00000000-0000-0000-0000-000000000972', 'UTC');
insert into private.shop_cycle_effect_baseline(user_id, cycle_id, started_at)
values ('00000000-0000-0000-0000-000000000972', 'rollback-cycle', '2026-09-26T00:00:00Z');
create temporary table pg_temp.failed_first_upload_before as
select
  (select to_jsonb(d) from private.planet_device_state d
    where d.user_id = '00000000-0000-0000-0000-000000000972'
      and d.device_id = '30000000-0000-0000-0000-000000000972') as raw_device,
  (select to_jsonb(p) from public.planet_member_state p
    where p.user_id = '00000000-0000-0000-0000-000000000972') as planet_state;
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000972', true);
select throws_ok($$select public.upsert_my_planet_state(
  pg_temp.planet_state('Rollback Owner', 'rollback-cycle'),
  jsonb_build_object(
    'device_id', '30000000-0000-0000-0000-000000000972',
    'current_cycle_id', 'rollback-cycle',
    'lifetime_tokens', 200000, 'current_planet_tokens', 200000,
    'daily_tokens', '{"2026-09-30":200000}'::jsonb, 'incomplete', false,
    'canonical_version', 1,
    'daily_segments', '[{"cycle_id":"rollback-cycle","date":"2026-09-30","effect_revision":0,"tokens":199999}]'::jsonb,
    'activity_days', '[]'::jsonb
  )
)$$, '23514', 'effect segments do not sum to raw daily tokens',
  'invalid first canonical upload fails after its raw candidate has been written');
reset role;
select is((select to_jsonb(d) from private.planet_device_state d
  where d.user_id = '00000000-0000-0000-0000-000000000972'
    and d.device_id = '30000000-0000-0000-0000-000000000972'),
  (select raw_device from pg_temp.failed_first_upload_before),
  'failed first upload rolls back the raw device snapshot');
select is((select to_jsonb(p) from public.planet_member_state p
  where p.user_id = '00000000-0000-0000-0000-000000000972'),
  (select planet_state from pg_temp.failed_first_upload_before),
  'failed first upload rolls back the planet projection');
select is((select count(*)::bigint from private.shop_device_contribution_state s
  where s.user_id = '00000000-0000-0000-0000-000000000972'), 0::bigint,
  'failed first upload does not leave a canonical snapshot');
select is((select count(*)::bigint from private.shop_planet_object_generation_baseline b
  where b.user_id = '00000000-0000-0000-0000-000000000972'
    and b.cycle_id = 'rollback-cycle'), 0::bigint,
  'failed first upload does not mark object generation initialized');

insert into private.shop_effect_history(
  user_id, cycle_id, revision, started_at, ended_at, active_instance_ids, effects
) values (
  '00000000-0000-0000-0000-000000000974', 'threshold-cycle', 1,
  '2026-09-30T12:00:00Z', null, '[]'::jsonb,
  '{"token_earning_bps":0,"civilization_growth_bps":2000,"shop_discount_bps":0,"reset_cooldown_bps":0,"natural_removal_discount_bps":0,"era_reward_tokens":0,"streak_reward_tokens":0}'::jsonb
);
select is((select count(*)::bigint from public.planet_member_state p
  where p.user_id = '00000000-0000-0000-0000-000000000974'), 0::bigint,
  'new-account first projection starts without a planet row');
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000974', true);
select lives_ok($$select public.upsert_my_planet_state(
  jsonb_set(
    jsonb_set(
      jsonb_set(pg_temp.planet_state('Threshold Owner', 'threshold-cycle'),
        '{growth_credit}', '99'::jsonb, true),
      '{stage}', '4'::jsonb, true
    ),
    '{objects}', '[{"stage":4,"ordinal":99,"kind":"rocket","seed":42,"x":99,"y":99}]'::jsonb,
    true
  ),
  jsonb_build_object(
    'device_id', '30000000-0000-0000-0000-000000000974',
    'current_cycle_id', 'threshold-cycle',
    'lifetime_tokens', 100000, 'current_planet_tokens', 100000,
    'daily_tokens', '{"2026-09-30":100000}'::jsonb, 'incomplete', false,
    'canonical_version', 1,
    'daily_segments', '[{"cycle_id":"threshold-cycle","date":"2026-09-30","effect_revision":1,"tokens":100000}]'::jsonb,
    'activity_days', '[{"cycle_id":"threshold-cycle","reward_date":"2026-09-30","first_occurred_at_utc":"2026-09-30T13:00:00Z","tokens":100000}]'::jsonb
  )
)$$, 'first account upload projects from its server contribution snapshot');
reset role;
select ok(abs((public.get_my_planet_state()->>'growth_credit')::numeric - 1.2) < 0.000001,
  'new-account generation uses weighted server raw daily growth, not client growth_credit');
select is(public.get_my_planet_state()->'objects',
  jsonb_build_array(private.shop_canonical_planet_object('threshold-cycle', 0, 0)),
  'new account creates the first deterministic stage-zero object and ignores client objects');
select is((select count(*)::bigint from private.shop_planet_object_generation_baseline b
  where b.user_id = '00000000-0000-0000-0000-000000000974'
    and b.cycle_id = 'threshold-cycle'), 1::bigint,
  'successful first projection records an account-and-cycle marker');

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000974', true);
select lives_ok($$select public.upsert_my_planet_state(
  pg_temp.planet_state('Threshold Owner', 'threshold-cycle'),
  jsonb_build_object(
    'device_id', '30000000-0000-0000-0000-000000000974',
    'current_cycle_id', 'threshold-cycle',
    'lifetime_tokens', 2685762, 'current_planet_tokens', 2685762,
    'daily_tokens', '{"2026-09-30":2685762}'::jsonb, 'incomplete', false,
    'canonical_version', 2,
    'daily_segments', '[{"cycle_id":"threshold-cycle","date":"2026-09-30","effect_revision":1,"tokens":2685762}]'::jsonb,
    'activity_days', '[{"cycle_id":"threshold-cycle","reward_date":"2026-09-30","first_occurred_at_utc":"2026-09-30T13:00:00Z","tokens":2685762}]'::jsonb
  )
)$$, 'threshold contribution replaces the prior canonical device snapshot');
reset role;
select ok(log(2::numeric, 1 + 2685762::numeric / 100000) < 5,
  'raw-only growth at the fixture threshold remains below stage one');
select ok((public.get_my_planet_state()->>'growth_credit')::numeric > 5
    and (public.get_my_planet_state()->>'growth_credit')::numeric < 6,
  'server effect weighting moves the threshold contribution above stage one');
select is((public.get_my_planet_state()->>'stage')::integer, 1,
  'weighted server growth advances the canonical planet stage');
select is(jsonb_array_length(public.get_my_planet_state()->'objects'), 5,
  'weighted growth projects five stage-zero natural objects at this threshold');
select is(public.get_my_planet_state()->'objects'->1,
  '{"stage":0,"ordinal":1,"kind":"fern","x":19,"y":60,"seed":5165125686793361533}'::jsonb,
  'new natural-object seed and coordinates match Rust SHA256 generation');

create temporary table pg_temp.threshold_objects_before_replay as
select public.get_my_planet_state()->'objects' as objects;
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000974', true);
select lives_ok($$select pg_temp.upload_threshold_contribution(
  '30000000-0000-0000-0000-000000000974', 2, 2685762)$$,
  'same-version replay returns the existing canonical contribution');
reset role;
select is(public.get_my_planet_state()->'objects',
  (select objects from pg_temp.threshold_objects_before_replay),
  'same-version replay preserves generated object IDs and seed attributes');
select is((select count(*)::bigint from private.shop_planet_object_generation_baseline b
  where b.user_id = '00000000-0000-0000-0000-000000000974'
    and b.cycle_id = 'threshold-cycle'), 1::bigint,
  'same-cycle replay does not duplicate the projection marker');

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000974', true);
select lives_ok($$select pg_temp.upload_threshold_contribution(
  '30000000-0000-0000-0000-000000000975', 1, 100000)$$,
  'second device contributes an independent canonical snapshot');
reset role;
select is(public.get_my_planet_state()->'objects',
  (select objects from pg_temp.threshold_objects_before_replay),
  'second-device aggregation preserves existing generated IDs and seeds');

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000974', true);
select lives_ok($$select pg_temp.upload_threshold_contribution(
  '30000000-0000-0000-0000-000000000974', 3, 100000)$$,
  'first lower-growth snapshot replaces the high-growth device contribution');
reset role;
select ok((public.get_my_planet_state()->>'growth_credit')::numeric < 5
    and (public.get_my_planet_state()->>'stage')::integer = 0,
  'first growth decrease updates stage from server weighted raw device totals');
select is(public.get_my_planet_state()->'objects',
  (select objects from pg_temp.threshold_objects_before_replay),
  'first growth decrease preserves the already-generated natural-object IDs');

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000974', true);
select lives_ok($$select pg_temp.upload_threshold_contribution(
  '30000000-0000-0000-0000-000000000974', 4, 0)$$,
  'second lower-growth snapshot disables the first device contribution');
reset role;
select is((public.get_my_planet_state()->>'current_planet_tokens')::bigint, 100000::bigint,
  'second device remains in the raw current-cycle total after first device is disabled');
select is(public.get_my_planet_state()->'objects',
  (select objects from pg_temp.threshold_objects_before_replay),
  'second growth decrease also preserves all generated natural-object IDs and seeds');
select is((select count(*)::bigint from private.shop_planet_object_generation_baseline b
  where b.user_id = '00000000-0000-0000-0000-000000000974'
    and b.cycle_id = 'threshold-cycle'), 1::bigint,
  'replay and growth decreases retain one account-cycle projection marker');

select is((select count(*)::bigint from private.shop_planet_object_generation_baseline b
  where b.user_id = '00000000-0000-0000-0000-000000000976'
    and b.cycle_id = 'threshold-cycle'), 0::bigint,
  'another account has no marker for the same cycle key');
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000976', true);
select lives_ok($$select public.upsert_my_planet_state(
  pg_temp.planet_state('Other Account', 'threshold-cycle'),
  jsonb_build_object(
    'device_id', '30000000-0000-0000-0000-000000000976',
    'current_cycle_id', 'threshold-cycle',
    'lifetime_tokens', 100000, 'current_planet_tokens', 100000,
    'daily_tokens', '{"2026-09-30":100000}'::jsonb, 'incomplete', false,
    'canonical_version', 1,
    'daily_segments', '[{"cycle_id":"threshold-cycle","date":"2026-09-30","effect_revision":1,"tokens":100000}]'::jsonb,
    'activity_days', '[{"cycle_id":"threshold-cycle","reward_date":"2026-09-30","first_occurred_at_utc":"2026-09-30T13:00:00Z","tokens":100000}]'::jsonb
  )
)$$, 'same cycle key uploads as a separate account');
reset role;
select is(jsonb_array_length(public.get_my_planet_state()->'objects'), 1,
  'same-cycle marker from another account cannot preserve its five generated objects');
select is((select count(*)::bigint from private.shop_planet_object_generation_baseline b
  where b.user_id = '00000000-0000-0000-0000-000000000976'
    and b.cycle_id = 'threshold-cycle'), 1::bigint,
  'first upload creates a marker scoped to its authenticated account');
select is((select count(*)::bigint from private.shop_planet_object_generation_baseline b
  where b.user_id = '00000000-0000-0000-0000-000000000974'
    and b.cycle_id = 'threshold-cycle'), 1::bigint,
  'separate account upload leaves the original account-cycle marker intact');

update public.planet_member_state p
set last_reset_at = now() - interval '10 hours',
    reset_available_at = now() + interval '14 hours'
where p.user_id = '00000000-0000-0000-0000-000000000970';
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000970', true);
select throws_ok($$select public.upsert_my_planet_state(
  pg_temp.planet_state('Transition Owner', 'transition-cycle-next', now() - interval '1 hour'),
  jsonb_build_object(
    'device_id', '30000000-0000-0000-0000-000000000971',
    'current_cycle_id', 'transition-cycle-next',
    'lifetime_tokens', 100000, 'current_planet_tokens', 100000,
    'daily_tokens', jsonb_build_object(to_char(now() at time zone 'Asia/Seoul', 'YYYY-MM-DD'), 100000),
    'incomplete', false, 'canonical_version', 1,
    'daily_segments', jsonb_build_array(jsonb_build_object(
      'cycle_id', 'transition-cycle-next',
      'date', to_char(now() at time zone 'Asia/Seoul', 'YYYY-MM-DD'),
      'effect_revision', 1, 'tokens', 100000)),
    'activity_days', jsonb_build_array(jsonb_build_object(
      'cycle_id', 'transition-cycle-next',
      'reward_date', to_char(now() at time zone 'Asia/Seoul', 'YYYY-MM-DD'),
      'first_occurred_at_utc', now(), 'tokens', 100000))
  )
)$$, '23514', 'planet cycle can only change through reset_my_planet',
  'client cycle changes are rejected while the server reset is unavailable');
reset role;
select is((select p.current_cycle_id from public.planet_member_state p
  where p.user_id = '00000000-0000-0000-0000-000000000970'), 'transition-cycle',
  'rejected early cycle change leaves the current cycle unchanged');
select is((select count(*)::bigint from private.shop_planet_object_generation_baseline b
  where b.user_id = '00000000-0000-0000-0000-000000000970'), 1::bigint,
  'rejected early cycle change does not create a second generation marker');

update public.planet_member_state p
set last_reset_at = now() - interval '48 hours',
    reset_available_at = now() - interval '24 hours'
where p.user_id = '00000000-0000-0000-0000-000000000970';
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000970', true);
select throws_ok($$select public.upsert_my_planet_state(
  pg_temp.planet_state('Transition Owner', 'transition-cycle-next', now() - interval '24 hours'),
  jsonb_build_object(
    'device_id', '30000000-0000-0000-0000-000000000971',
    'current_cycle_id', 'transition-cycle-next',
    'lifetime_tokens', 100000, 'current_planet_tokens', 100000,
    'daily_tokens', jsonb_build_object(to_char(now() at time zone 'Asia/Seoul', 'YYYY-MM-DD'), 100000),
    'incomplete', false, 'canonical_version', 1,
    'daily_segments', jsonb_build_array(jsonb_build_object(
      'cycle_id', 'transition-cycle-next',
      'date', to_char(now() at time zone 'Asia/Seoul', 'YYYY-MM-DD'),
      'effect_revision', 1, 'tokens', 100000)),
    'activity_days', jsonb_build_array(jsonb_build_object(
      'cycle_id', 'transition-cycle-next',
      'reward_date', to_char(now() at time zone 'Asia/Seoul', 'YYYY-MM-DD'),
      'first_occurred_at_utc', now(), 'tokens', 100000))
  )
)$$, '23514', 'planet cycle can only change through reset_my_planet',
  'client cycle changes remain rejected after the old cooldown would have elapsed');
reset role;
select is((select p.current_cycle_id from public.planet_member_state p
  where p.user_id = '00000000-0000-0000-0000-000000000970'), 'transition-cycle',
  'elapsed client timestamps still leave the server-owned cycle unchanged');
select is((select count(*)::bigint from private.shop_planet_object_generation_baseline b
  where b.user_id = '00000000-0000-0000-0000-000000000970'), 1::bigint,
  'legacy upload attempts do not create reset-cycle generation markers');
select is((select count(*)::bigint from private.shop_planet_object_generation_baseline b
  where b.user_id = '00000000-0000-0000-0000-000000000970'
    and b.cycle_id = 'transition-cycle'), 1::bigint,
  'rejected transition preserves the old cycle generation marker');

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000970', true);
select set_config('shop_test.transition_reset', public.reset_my_planet(
  '40000000-0000-0000-0000-000000000970', 'transition-cycle'
)::text, true);
reset role;
select is(current_setting('shop_test.transition_reset')::jsonb->'action'->>'status',
  'reset', 'the authenticated reset RPC performs the valid server cycle transition');
select set_config('shop_test.transition_cycle',
  (select p.current_cycle_id from public.planet_member_state p
    where p.user_id = '00000000-0000-0000-0000-000000000970'), true);
select set_config('shop_test.transition_revision',
  (select h.revision::text
    from private.shop_effect_history h
    join public.planet_member_state p on p.user_id = h.user_id
      and p.current_cycle_id = h.cycle_id
    where h.user_id = '00000000-0000-0000-0000-000000000970'
      and h.ended_at is null), true);
select set_config('shop_test.transition_version',
  (select (d.canonical_version + 1)::text
    from private.planet_device_state d
    where d.user_id = '00000000-0000-0000-0000-000000000970'
      and d.device_id = '30000000-0000-0000-0000-000000000970'), true);
select is(current_setting('shop_test.transition_cycle'),
  current_setting('shop_test.transition_reset')::jsonb->'planet_state'->>'current_cycle_id',
  'the reset response and stored server-selected cycle agree');

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000970', true);
select lives_ok($$select pg_temp.upload_transition_contribution(100000)$$,
  'the first contribution uploads against the server-selected new cycle');
reset role;
select is((public.get_my_planet_state()->>'current_cycle_id'),
  current_setting('shop_test.transition_cycle'),
  'new canonical upload retains the actual server-selected cycle');
select is((select min(c.effect_revision)
  from private.shop_effect_contribution c
  where c.user_id = '00000000-0000-0000-0000-000000000970'
    and c.cycle_id = current_setting('shop_test.transition_cycle')),
  current_setting('shop_test.transition_revision')::bigint,
  'new-cycle contribution uses the server-created global effect revision');
select is((public.get_my_planet_state()->>'growth_credit')::numeric, 1::numeric,
  'new-cycle growth uses its server-created zero-effect baseline');
select is(public.get_my_planet_state()->'objects',
  jsonb_build_array(private.shop_canonical_planet_object(
    current_setting('shop_test.transition_cycle'), 0, 0)),
  'new cycle projects natural objects from its own server-selected identity');
select is((select count(*)::bigint from private.shop_planet_object_generation_baseline b
  where b.user_id = '00000000-0000-0000-0000-000000000970'), 2::bigint,
  'old and reset-created cycle generation markers remain separate');
select is((select count(*)::bigint from private.shop_planet_object_generation_baseline b
  where b.user_id = '00000000-0000-0000-0000-000000000970'
    and b.cycle_id = 'transition-cycle'), 1::bigint,
  'new-cycle projection preserves the previous cycle marker');

insert into private.planet_wallet_credits(user_id, previous_cycle_id, amount, created_at)
values ('00000000-0000-0000-0000-000000000978', 'trusted-credit', 2000000,
  '2026-09-30T00:00:00Z');
insert into private.planet_wallet_credits(user_id, previous_cycle_id, amount, created_at)
values ('00000000-0000-0000-0000-000000000976', 'other-owner-credit', 123456,
  '2026-09-29T00:00:00Z');
create temporary table trusted_wallet_before as
select to_jsonb(w) as wallet_row
from private.planet_wallet_credits w
where w.user_id = '00000000-0000-0000-0000-000000000978';
create function pg_temp.upload_forged_wallet_snapshot() returns jsonb
language sql as $$
  select public.upsert_my_planet_state(
    pg_temp.planet_state('Wallet Guard', 'wallet-guard-cycle', null,
      '[{"previous_cycle_id":"trusted-credit","amount":99999999,"created_at_utc":"2026-09-30T00:00:00Z"},{"previous_cycle_id":"unused-cycle","amount":50000000,"created_at_utc":"2026-09-30T00:00:00Z"}]'::jsonb),
    pg_temp.planet_device('30000000-0000-0000-0000-000000000978', 'wallet-guard-cycle', 0)
      || jsonb_build_object('canonical_version', 0, 'daily_segments', '[]'::jsonb, 'activity_days', '[]'::jsonb)
  );
$$;
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000978', true);
select lives_ok($$select pg_temp.upload_forged_wallet_snapshot()$$,
  'wallet projection accepts a raw snapshot while preserving server-owned credits');
reset role;
select is((select count(*)::bigint from private.planet_wallet_credits w
  where w.user_id = '00000000-0000-0000-0000-000000000978'), 1::bigint,
  'client cannot mint an unused-cycle wallet credit');
select is((select w.amount from private.planet_wallet_credits w
  where w.user_id = '00000000-0000-0000-0000-000000000978'
    and w.previous_cycle_id = 'trusted-credit'), 2000000::bigint,
  'client snapshot cannot alter an existing server wallet credit');
select is((select to_jsonb(w) from private.planet_wallet_credits w
  where w.user_id = '00000000-0000-0000-0000-000000000978'
    and w.previous_cycle_id = 'trusted-credit'),
  (select wallet_row from trusted_wallet_before),
  'full server credit row and provenance fields remain unchanged');
select is((public.get_my_planet_state()->>'wallet_balance')::bigint, 2000000::bigint,
  'wallet balance remains equal to retained server credits after client forgery');
select is(jsonb_array_length(public.get_my_planet_state()->'wallet_credits'), 1,
  'canonical wallet response excludes credits owned by another account');
select is(public.get_my_planet_state()->'wallet_credits'->0->>'previous_cycle_id',
  'trusted-credit', 'canonical wallet response derives only from this account rows');
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000978', true);
select lives_ok($$select pg_temp.upload_forged_wallet_snapshot()$$,
  'replaying the forged wallet snapshot remains safe');
reset role;
select is((select count(*)::bigint from private.planet_wallet_credits w
  where w.user_id = '00000000-0000-0000-0000-000000000978'), 1::bigint,
  'replay cannot add another wallet credit');
select is((select to_jsonb(w) from private.planet_wallet_credits w
  where w.user_id = '00000000-0000-0000-0000-000000000978'
    and w.previous_cycle_id = 'trusted-credit'),
  (select wallet_row from trusted_wallet_before),
  'replay retains the exact server credit row');

set local role anon;
select throws_ok($$select * from private.shop_planet_object_generation_baseline$$,
  '42501', null, 'anonymous role cannot read generation markers');
reset role;

select * from finish();
rollback;
