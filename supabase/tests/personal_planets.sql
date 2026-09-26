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
select is((public.get_my_planet_state()->>'wallet_balance')::bigint, 20::bigint, 'second-account wallet is independent');
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000801', true);
select is((public.get_my_planet_state()->>'lifetime_tokens')::bigint, 200000::bigint, 'returning to the first account restores its totals');
select is((public.get_my_planet_state()->>'wallet_balance')::bigint, 0::bigint, 'another account wallet credit is not visible');
select is(public.get_my_planet_state()->'profile'->>'nickname', 'Alice', 'another account cannot change the first profile');
select lives_ok($$select public.upsert_my_planet_state(pg_temp.planet_state('Alice', 'alice-reset', now(),
  jsonb_build_array(jsonb_build_object('previous_cycle_id','alice-cycle','amount',200000,'created_at_utc',now()))),
  pg_temp.planet_device('30000000-0000-0000-0000-000000000801', 'alice-reset', 0, false, 100000))$$, 'reset creates a new cycle with one wallet credit');
select is((public.get_my_planet_state()->>'wallet_balance')::bigint, 200000::bigint, 'reset credits the previous cycle once');
select lives_ok($$select public.upsert_my_planet_state(public.get_my_planet_state(),
  pg_temp.planet_device('30000000-0000-0000-0000-000000000803', 'alice-reset', 0, false, 100000))$$, 'downloaded canonical reset state can be uploaded by another device');
select is((public.get_my_planet_state()->>'wallet_balance')::bigint, 200000::bigint, 'canonical retry does not duplicate the wallet credit');
select throws_ok($$select public.upsert_my_planet_state(pg_temp.planet_state('Alice','too-soon',now()+interval '1 hour'),
  pg_temp.planet_device('30000000-0000-0000-0000-000000000801','too-soon',0,false,100000))$$, '23514', null, 'reset cooldown is enforced');
select throws_ok($$select public.upsert_my_planet_state(pg_temp.planet_state('Alice') || '{"source_path":"secret"}'::jsonb,
  pg_temp.planet_device('30000000-0000-0000-0000-000000000801','cycle-1',0))$$, '23514', null, 'raw source fields are rejected');
select throws_ok($$select public.upsert_my_planet_state(pg_temp.planet_state('Alice'),
  pg_temp.planet_device('30000000-0000-0000-0000-000000000801','cycle-1',5) || '{"daily_tokens":{"2026-09-26":6}}'::jsonb)$$, '23514', null, 'inconsistent daily contribution is rejected');
reset role;
set local role anon;
select throws_ok($$select public.get_my_planet_state()$$, '42501', null, 'anonymous users cannot read an account planet');
select throws_ok($$select public.upsert_my_planet_state('{}'::jsonb,'{}'::jsonb)$$, '42501', null, 'anonymous users cannot upload a planet');
select * from finish();
rollback;
