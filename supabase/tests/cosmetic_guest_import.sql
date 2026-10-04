begin;
create extension if not exists pgtap with schema extensions;
select plan(18);
\ir fixtures/planet.inc

select ok(to_regprocedure('public.import_my_guest_cosmetics(uuid,jsonb,jsonb)') is not null,
  'legacy guest import RPC remains identifiable for compatibility');
select is(has_function_privilege('anon', 'public.import_my_guest_cosmetics(uuid,jsonb,jsonb)', 'EXECUTE'),
  false, 'anonymous role cannot execute the retired guest import RPC');
select is(has_function_privilege('authenticated', 'public.import_my_guest_cosmetics(uuid,jsonb,jsonb)', 'EXECUTE'),
  false, 'authenticated role cannot execute the retired guest import RPC');
select is(has_function_privilege('service_role', 'public.import_my_guest_cosmetics(uuid,jsonb,jsonb)', 'EXECUTE'),
  false, 'service role cannot execute the retired guest import RPC');

insert into auth.users(id) values ('00000000-0000-0000-0000-000000000901');
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000901', true);
select lives_ok($$select public.upsert_my_planet_state(
  pg_temp.planet_state('Alice', 'alice-import-cycle', null,
    '[{"previous_cycle_id":"untrusted-client-credit","amount":600000,"created_at_utc":"2026-09-28T00:00:00Z"}]'::jsonb),
  pg_temp.planet_device('30000000-0000-0000-0000-000000000901', 'alice-import-cycle', 0)
)$$, 'canonical planet upload accepts its contribution payload');
select is((public.get_my_planet_state()->>'wallet_balance')::bigint, 0::bigint,
  'client-supplied wallet credits do not fund the account');
select is(public.get_my_planet_state()->'wallet_credits', '[]'::jsonb,
  'wallet credit projection comes from server-issued credits');

reset role;
insert into private.cosmetic_purchase(user_id, purchase_id, sku, price, purchased_at)
values ('00000000-0000-0000-0000-000000000901',
  '11111111-1111-4111-8111-111111111111', 'star_cluster', 100000, '2026-09-28T00:00:00Z');
insert into private.cosmetic_guest_import_request(user_id, import_id, result)
values ('00000000-0000-0000-0000-000000000901',
  '22222222-2222-4222-8222-222222222222', '{"status":"imported"}'::jsonb);

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000901', true);
select throws_ok($$select public.import_my_guest_cosmetics(
  'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa', '[]'::jsonb, '[]'::jsonb)$$,
  '42501', null, 'authenticated callers cannot run the retired import');
select throws_ok($$select * from private.cosmetic_guest_import_request$$,
  '42501', null, 'import request results remain private');
select is((public.get_my_planet_state()->>'wallet_balance')::bigint, 0::bigint,
  'a rejected import cannot change the server wallet');
reset role;
select is((select count(*)::bigint from private.cosmetic_guest_import_request
  where user_id = '00000000-0000-0000-0000-000000000901'), 1::bigint,
  'a rejected import does not create another request result');
select is((select count(*)::bigint from private.cosmetic_purchase
  where user_id = '00000000-0000-0000-0000-000000000901'), 1::bigint,
  'a rejected import does not add a legacy cosmetic purchase');
select is((select count(*)::bigint from private.shop_purchase
  where user_id = '00000000-0000-0000-0000-000000000901'), 0::bigint,
  'a retired import does not create current shop purchases');
select throws_ok($$select public.import_my_guest_cosmetics(
  'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb', '[]'::jsonb, '[]'::jsonb)$$,
  '55000', null, 'owner invocation reports the import RPC as retired');
select is(private.shop_available_balance('00000000-0000-0000-0000-000000000901'),
  0::numeric, 'owner retirement response leaves the server wallet unchanged');
select is((select count(*)::bigint from private.cosmetic_guest_import_request
  where user_id = '00000000-0000-0000-0000-000000000901'), 1::bigint,
  'owner retirement response preserves historical import results');
select is((select count(*)::bigint from private.cosmetic_purchase
  where user_id = '00000000-0000-0000-0000-000000000901'), 1::bigint,
  'owner retirement response preserves legacy purchase history');
set local role anon;
select throws_ok($$select public.import_my_guest_cosmetics(
  'cccccccc-cccc-4ccc-8ccc-cccccccccccc', '[]'::jsonb, '[]'::jsonb)$$,
  '42501', null, 'anonymous users cannot invoke the retired import');
reset role;
select * from finish();
rollback;
