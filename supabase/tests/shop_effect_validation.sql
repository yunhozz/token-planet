begin;
create extension if not exists pgtap with schema extensions;
select no_plan();
\ir fixtures/planet.inc

insert into auth.users(id) values
  ('00000000-0000-0000-0000-000000000971'),
  ('00000000-0000-0000-0000-000000000972'),
  ('00000000-0000-0000-0000-000000000973'),
  ('00000000-0000-0000-0000-000000000974');
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000971', true);
select lives_ok($$select public.upsert_my_planet_state(
  pg_temp.planet_state('Validation Owner', 'validation-cycle', '2026-09-29T18:00:00Z'),
  pg_temp.planet_device(
    '30000000-0000-0000-0000-000000000971', 'validation-cycle', 0
  ) || jsonb_build_object(
    'canonical_version', 0,
    'daily_segments', '[]'::jsonb,
    'activity_days', '[]'::jsonb
  )
)$$, 'raw state is initialized before direct private-validator checks');
reset role;

insert into private.shop_account_state(user_id, reward_timezone)
values ('00000000-0000-0000-0000-000000000971', 'UTC')
on conflict (user_id) do nothing;
insert into private.shop_cycle_effect_baseline(user_id, cycle_id, started_at)
values ('00000000-0000-0000-0000-000000000971', 'validation-cycle', '2026-09-29T18:00:00Z')
on conflict (user_id, cycle_id) do nothing;
insert into private.shop_effect_history(
  user_id, cycle_id, revision, started_at, ended_at, active_instance_ids, effects
) values (
  '00000000-0000-0000-0000-000000000971', 'validation-cycle', 1,
  '2026-09-29T16:00:00Z', '2026-09-29T20:00:00Z', '[]'::jsonb,
  '{"token_earning_bps":0,"civilization_growth_bps":2000,"shop_discount_bps":0,"reset_cooldown_bps":0,"natural_removal_discount_bps":0,"era_reward_tokens":0,"streak_reward_tokens":0}'::jsonb
);

select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000971', true);
select lives_ok($$select private.shop_validate_effect_upload(
  '00000000-0000-0000-0000-000000000971',
  pg_temp.planet_state('Validation Owner', 'validation-cycle', '2026-09-29T18:00:00Z'),
  jsonb_build_object(
    'device_id', '30000000-0000-0000-0000-000000000971',
    'current_cycle_id', 'validation-cycle', 'lifetime_tokens', 100000,
    'current_planet_tokens', 100000, 'daily_tokens', '{"2026-09-30":100000}'::jsonb,
    'incomplete', false, 'canonical_version', 1,
    'daily_segments', '[{"cycle_id":"validation-cycle","date":"2026-09-30","effect_revision":1,"tokens":100000}]'::jsonb,
    'activity_days', '[{"cycle_id":"validation-cycle","reward_date":"2026-09-29","first_occurred_at_utc":"2026-09-29T19:00:00Z","tokens":100000}]'::jsonb
  )
)$$, 'growth date uses mutable planet timezone while activity date uses frozen reward timezone');

select throws_ok($$select private.shop_validate_effect_upload(
  '00000000-0000-0000-0000-000000000971',
  pg_temp.planet_state('Validation Owner', 'validation-cycle', '2026-09-29T18:00:00Z'),
  jsonb_build_object(
    'device_id', '30000000-0000-0000-0000-000000000971',
    'current_cycle_id', 'validation-cycle', 'lifetime_tokens', 0,
    'current_planet_tokens', 0, 'daily_tokens', '{}'::jsonb,
    'incomplete', false, 'canonical_version', 2,
    'daily_segments', '[]'::jsonb,
    'activity_days', '[{"cycle_id":"validation-cycle","reward_date":"2026-09-29","first_occurred_at_utc":"2026-09-29T19:00:00Z","tokens":1}]'::jsonb
  )
)$$, '23514', null, 'positive activity cannot be uploaded without positive raw segments');

select throws_ok($$select private.shop_validate_effect_upload(
  '00000000-0000-0000-0000-000000000971',
  jsonb_set(
    pg_temp.planet_state('Validation Owner', 'validation-cycle', '2026-09-29T18:00:00Z'),
    '{timezone}', '"UTC"'::jsonb
  ),
  jsonb_build_object(
    'device_id', '30000000-0000-0000-0000-000000000971',
    'current_cycle_id', 'validation-cycle', 'lifetime_tokens', 100000,
    'current_planet_tokens', 100000, 'daily_tokens', '{"2026-09-29":100000}'::jsonb,
    'incomplete', false, 'canonical_version', 2,
    'daily_segments', '[{"cycle_id":"validation-cycle","date":"2026-09-29","effect_revision":1,"tokens":100000}]'::jsonb,
    'activity_days', '[{"cycle_id":"validation-cycle","reward_date":"2026-09-29","first_occurred_at_utc":"2026-09-29T17:00:00Z","tokens":100000}]'::jsonb
  )
)$$, '23514', null, 'first activity before the current cycle start is rejected');

select throws_ok($$select private.shop_validate_effect_upload(
  '00000000-0000-0000-0000-000000000972',
  pg_temp.planet_state('Other Account', 'validation-cycle'),
  jsonb_build_object(
    'device_id', '30000000-0000-0000-0000-000000000971',
    'current_cycle_id', 'validation-cycle', 'lifetime_tokens', 100000,
    'current_planet_tokens', 100000, 'daily_tokens', '{"2026-09-30":100000}'::jsonb,
    'incomplete', false, 'canonical_version', 1,
    'daily_segments', '[{"cycle_id":"validation-cycle","date":"2026-09-30","effect_revision":1,"tokens":100000}]'::jsonb,
    'activity_days', '[{"cycle_id":"validation-cycle","reward_date":"2026-09-29","first_occurred_at_utc":"2026-09-29T19:00:00Z","tokens":100000}]'::jsonb
  )
)$$, '42501', null, 'a caller cannot validate another account device');

select throws_ok($$select private.shop_validate_effect_upload(
  '00000000-0000-0000-0000-000000000971',
  pg_temp.planet_state('Validation Owner', 'validation-cycle', '2026-09-29T18:00:00Z'),
  jsonb_build_object(
    'device_id', '30000000-0000-0000-0000-000000000971',
    'current_cycle_id', 'validation-cycle', 'lifetime_tokens', 100000,
    'current_planet_tokens', 100000, 'daily_tokens', '{"2026-09-30":100000}'::jsonb,
    'incomplete', false, 'canonical_version', 2,
    'daily_segments', '[{"cycle_id":"unknown-cycle","date":"2026-09-30","effect_revision":1,"tokens":100000}]'::jsonb,
    'activity_days', '[]'::jsonb
  )
)$$, '23514', null, 'contribution cannot use a revision from an unknown cycle');

insert into public.planet_member_state(
  user_id, nickname, avatar, timezone, current_cycle_id, cycle_started_at,
  current_planet_tokens, lifetime_tokens, growth_credit, stage, progress_to_next,
  incomplete, objects
) values (
  '00000000-0000-0000-0000-000000000973', 'Multi-day Test', 'feminine', 'Asia/Seoul',
  'multi-day-cycle', '2026-09-29T15:00:00Z', 100, 100, 0, 0, 0, false, '[]'::jsonb
);
insert into private.shop_account_state(user_id, reward_timezone)
values ('00000000-0000-0000-0000-000000000973', 'UTC');
insert into private.shop_cycle_effect_baseline(user_id, cycle_id, started_at)
values ('00000000-0000-0000-0000-000000000973', 'multi-day-cycle', '2026-09-29T15:00:00Z');
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000973', true);
select throws_ok($$select private.shop_validate_effect_upload(
  '00000000-0000-0000-0000-000000000973',
  pg_temp.planet_state('Multi-day Test', 'multi-day-cycle', '2026-09-29T15:00:00Z'),
  jsonb_build_object(
    'device_id', '30000000-0000-0000-0000-000000000973',
    'current_cycle_id', 'multi-day-cycle', 'lifetime_tokens', 100,
    'current_planet_tokens', 100, 'daily_tokens', '{"2026-09-30":100}'::jsonb,
    'incomplete', false, 'canonical_version', 1,
    'daily_segments', '[{"cycle_id":"multi-day-cycle","date":"2026-09-30","effect_revision":0,"tokens":100}]'::jsonb,
    'activity_days', '[{"cycle_id":"multi-day-cycle","reward_date":"2026-09-29","first_occurred_at_utc":"2026-09-29T16:00:00Z","tokens":100},{"cycle_id":"multi-day-cycle","reward_date":"2026-09-30","first_occurred_at_utc":"2026-09-30T10:00:00Z","tokens":100}]'::jsonb
  )
)$$, '23514', null, 'one growth-day raw total cannot be counted in two UTC reward dates');

insert into public.planet_member_state(
  user_id, nickname, avatar, timezone, current_cycle_id, cycle_started_at, last_reset_at,
  current_planet_tokens, lifetime_tokens, growth_credit, stage, progress_to_next,
  incomplete, objects
) values (
  '00000000-0000-0000-0000-000000000974', 'Reset Day Test', 'feminine', 'Asia/Seoul',
  'new-cycle', '2026-09-30T12:00:00Z', '2026-09-30T12:00:00Z',
  100, 200, 0, 0, 0, false, '[]'::jsonb
);
insert into private.shop_account_state(user_id, reward_timezone)
values ('00000000-0000-0000-0000-000000000974', 'UTC');
insert into private.planet_wallet_credits(user_id, previous_cycle_id, amount, created_at)
values ('00000000-0000-0000-0000-000000000974', 'old-cycle', 0, '2026-09-30T12:00:00Z');
insert into private.shop_cycle_effect_baseline(user_id, cycle_id, started_at) values
  ('00000000-0000-0000-0000-000000000974', 'old-cycle', '2026-09-29T15:00:00Z'),
  ('00000000-0000-0000-0000-000000000974', 'new-cycle', '2026-09-30T12:00:00Z');
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000974', true);
select lives_ok($$select private.shop_validate_effect_upload(
  '00000000-0000-0000-0000-000000000974',
  pg_temp.planet_state('Reset Day Test', 'new-cycle', '2026-09-30T12:00:00Z'),
  jsonb_build_object(
    'device_id', '30000000-0000-0000-0000-000000000974',
    'current_cycle_id', 'new-cycle', 'lifetime_tokens', 200,
    'current_planet_tokens', 100, 'daily_tokens', '{"2026-09-30":100}'::jsonb,
    'incomplete', false, 'canonical_version', 1,
    'daily_segments', '[{"cycle_id":"old-cycle","date":"2026-09-30","effect_revision":0,"tokens":100},{"cycle_id":"new-cycle","date":"2026-09-30","effect_revision":0,"tokens":100}]'::jsonb,
    'activity_days', '[{"cycle_id":"old-cycle","reward_date":"2026-09-30","first_occurred_at_utc":"2026-09-30T10:00:00Z","tokens":200}]'::jsonb
  )
)$$, 'one reward-day activity aggregate may span a reset across two known cycles');

select * from finish();
rollback;
