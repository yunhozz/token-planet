begin;
create extension if not exists pgtap with schema extensions;
select plan(24);
\ir fixtures/planet.inc
insert into auth.users(id) values
  ('00000000-0000-0000-0000-000000000801'),
  ('00000000-0000-0000-0000-000000000802');
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000801', true);
select is(public.get_my_planet_state(), null::jsonb, 'a new account has no remote planet');
select lives_ok($$select public.upsert_my_planet_state(pg_temp.planet_state('Alice', 'alice-cycle'),
  pg_temp.planet_device('30000000-0000-0000-0000-000000000801', 'alice-cycle', 100000))$$, 'a solo signed-in account can upload a planet');
select is((public.get_my_planet_state()->>'current_planet_tokens')::bigint, 100000::bigint, 'server derives totals from the device contribution');
select lives_ok($$select public.upsert_my_planet_state(pg_temp.planet_state('Alice', 'alice-cycle'),
  pg_temp.planet_device('30000000-0000-0000-0000-000000000801', 'alice-cycle', 100000))$$, 'same device retry succeeds');
select is((public.get_my_planet_state()->>'lifetime_tokens')::bigint, 100000::bigint, 'retry does not duplicate usage');
select lives_ok($$select public.upsert_my_planet_state(pg_temp.planet_state('Alice', 'alice-cycle'),
  pg_temp.planet_device('30000000-0000-0000-0000-000000000803', 'alice-cycle', 100000))$$, 'second device contribution succeeds');
select is((public.get_my_planet_state()->>'current_planet_tokens')::bigint, 200000::bigint, 'two devices add within the same account');
select ok(abs((public.get_my_planet_state()->>'growth_credit')::numeric-log(2::numeric,3::numeric))<0.000001, 'daily curve is applied after devices combine');
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000802', true);
select is(public.get_my_planet_state(), null::jsonb, 'switching accounts cannot read the first account planet');
select lives_ok($$select public.upsert_my_planet_state(pg_temp.planet_state('Bob', 'bob-cycle', null,
  '[{"previous_cycle_id":"bob-old","amount":20,"created_at_utc":"2026-09-26T00:00:00Z"}]'::jsonb),
  pg_temp.planet_device('30000000-0000-0000-0000-000000000801', 'bob-cycle', 7))$$, 'same installation can upload a separate second-account planet');
select is((public.get_my_planet_state()->>'lifetime_tokens')::bigint, 7::bigint, 'second account does not inherit first-account totals');
select is((public.get_my_planet_state()->>'wallet_balance')::bigint, 0::bigint,
  'client-supplied wallet credits do not fund the second account');
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000801', true);
select is((public.get_my_planet_state()->>'lifetime_tokens')::bigint, 200000::bigint, 'returning to the first account restores its totals');
select is((public.get_my_planet_state()->>'wallet_balance')::bigint, 0::bigint, 'another account wallet credit is not visible');
select is(public.get_my_planet_state()->'profile'->>'nickname', 'Alice', 'another account cannot change the first profile');
select is((public.reset_my_planet(
  '40000000-0000-0000-0000-000000000801', 'alice-cycle')->'action'->>'status'),
  'reset', 'the server reset action creates the next cycle');
select is((public.get_my_planet_state()->>'wallet_balance')::bigint, 200000::bigint,
  'reset credits the previous cycle raw tokens once');
select lives_ok($$select public.upsert_my_planet_state(public.get_my_planet_state(),
  pg_temp.planet_device('30000000-0000-0000-0000-000000000803',
    public.get_my_planet_state()->>'current_cycle_id', 0, false, 100000, 2))$$,
  'another device can upload the canonical server reset state');
select is((public.get_my_planet_state()->>'wallet_balance')::bigint, 200000::bigint,
  'canonical retry does not duplicate the server reset credit');
select throws_ok($$select public.reset_my_planet(
  '40000000-0000-0000-0000-000000000802',
  public.get_my_planet_state()->>'current_cycle_id')$$,
  '55000', 'planet reset cooldown is active', 'reset cooldown is enforced');
select throws_ok($$select public.upsert_my_planet_state(pg_temp.planet_state('Alice') || '{"source_path":"secret"}'::jsonb,
  pg_temp.planet_device('30000000-0000-0000-0000-000000000801','cycle-1',0))$$, '23514', null, 'raw source fields are rejected');
select throws_ok($$select public.upsert_my_planet_state(public.get_my_planet_state(),
  pg_temp.planet_device('30000000-0000-0000-0000-000000000801',
    public.get_my_planet_state()->>'current_cycle_id', 5, false, 100000, 2)
    || '{"daily_tokens":{"2026-09-26":6}}'::jsonb)$$,
  '23514', 'planet device contribution is invalid',
  'inconsistent daily contribution is rejected');
reset role;
set local role anon;
select throws_ok($$select public.get_my_planet_state()$$, '42501', null, 'anonymous users cannot read an account planet');
select throws_ok($$select public.upsert_my_planet_state('{}'::jsonb,'{}'::jsonb)$$, '42501', null, 'anonymous users cannot upload a planet');
select * from finish();
rollback;
