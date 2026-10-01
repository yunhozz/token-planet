begin;
create extension if not exists pgtap with schema extensions;
select no_plan();
\ir fixtures/planet.inc

insert into auth.users(id) values
  ('00000000-0000-0000-0000-000000000957');

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000957', true);
select lives_ok($$select public.upsert_my_planet_state(
  pg_temp.planet_state('Legacy Retention', 'legacy-cycle', null,
    '[{"previous_cycle_id":"legacy-credit","amount":2000000,"created_at_utc":"2026-09-30T00:00:00Z"}]'::jsonb),
  pg_temp.planet_device('30000000-0000-0000-0000-000000000957', 'legacy-cycle', 0))$$,
  'legacy account fixture retains its base wallet credit');
reset role;

insert into public.worlds(id, owner_id, name, timezone)
values ('40000000-0000-0000-0000-000000000957', '00000000-0000-0000-0000-000000000957', 'Legacy fixture', 'UTC');
insert into public.daily_usage_snapshots(
  world_id, user_id, device_id, bucket_date, bucket_policy_version, agent,
  schema_version, revision, input_tokens, output_tokens, cache_read_tokens,
  cache_write_tokens, total_tokens, coverage, payload_hash
) values (
  '40000000-0000-0000-0000-000000000957', '00000000-0000-0000-0000-000000000957',
  '30000000-0000-0000-0000-000000000957', '2026-09-30', 1, 'codex',
  1, 1, 1000, 200, 50, 50, 1300, 'complete', repeat('a', 64)
);
insert into private.cosmetic_account_lock(user_id)
values ('00000000-0000-0000-0000-000000000957');
insert into private.cosmetic_purchase(user_id, purchase_id, sku, price)
values ('00000000-0000-0000-0000-000000000957', '50000000-0000-0000-0000-000000000957', 'aurora_v2', 900000);
insert into private.cosmetic_purchase_request(user_id, purchase_id, sku, result)
values ('00000000-0000-0000-0000-000000000957', '50000000-0000-0000-0000-000000000957',
  'aurora_v2', '{"status":"purchased"}'::jsonb);
insert into private.cosmetic_equipment(user_id, cycle_id, slot_id, sku, version)
values ('00000000-0000-0000-0000-000000000957', 'legacy-cycle', 'sky', 'aurora_v2', 1);
insert into private.cosmetic_guest_import_request(user_id, import_id, result)
values ('00000000-0000-0000-0000-000000000957',
  '60000000-0000-0000-0000-000000000957', '{"status":"imported"}'::jsonb);
insert into private.shop_purchase(user_id, request_id, sku, price, catalog_revision, effect_revision)
values ('00000000-0000-0000-0000-000000000957', 'legacy-balance-new-shop-spend', 'land_market', 300000, 1, 0);

select is((select count(*)::bigint from auth.users
  where id = '00000000-0000-0000-0000-000000000957'), 1::bigint,
  'legacy retirement preserves the auth account');
select is((select count(*)::bigint from public.planet_member_state
  where user_id = '00000000-0000-0000-0000-000000000957'), 1::bigint,
  'legacy retirement preserves canonical account state');
select is((select sum(amount)::bigint from private.planet_wallet_credits
  where user_id = '00000000-0000-0000-0000-000000000957'), 2000000::bigint,
  'legacy retirement preserves base wallet credits');
select is((select count(*)::bigint from public.daily_usage_snapshots
  where user_id = '00000000-0000-0000-0000-000000000957'), 1::bigint,
  'legacy retirement preserves raw usage');

update public.planet_member_state
set current_cycle_id = 'legacy-cycle-next'
where user_id = '00000000-0000-0000-0000-000000000957';
select ok(not exists (
  select 1 from pg_trigger
  where tgrelid = 'public.planet_member_state'::regclass
    and tgname = 'clear_cosmetic_equipment_after_cycle_change'
    and not tgisinternal
), 'the old cycle-change equipment deletion trigger is retired');
select is((select count(*)::bigint from private.cosmetic_equipment
  where user_id = '00000000-0000-0000-0000-000000000957'), 1::bigint,
  'legacy equipment rows survive a cycle change');
select is((select count(*)::bigint from private.cosmetic_purchase
  where user_id = '00000000-0000-0000-0000-000000000957'), 1::bigint,
  'legacy purchase rows remain available for compatibility reads');
select is((select count(*)::bigint from private.cosmetic_purchase_request
  where user_id = '00000000-0000-0000-0000-000000000957'), 1::bigint,
  'legacy purchase request rows are retained');
select is((select count(*)::bigint from private.cosmetic_guest_import_request
  where user_id = '00000000-0000-0000-0000-000000000957'), 1::bigint,
  'legacy guest import request rows are retained');
select is(private.shop_available_balance('00000000-0000-0000-0000-000000000957'),
  1700000::numeric,
  'new balance subtracts new shop spend but excludes legacy cosmetic spend');
select is((private.planet_state_json('00000000-0000-0000-0000-000000000957')->>'wallet_balance')::numeric,
  private.shop_available_balance('00000000-0000-0000-0000-000000000957'),
  'planet wallet projection uses the canonical shop balance helper');

select is(has_function_privilege('anon', 'public.purchase_my_cosmetic(uuid,text,integer)', 'EXECUTE'), false,
  'anonymous role cannot execute the retired purchase RPC');
select is(has_function_privilege('authenticated', 'public.purchase_my_cosmetic(uuid,text,integer)', 'EXECUTE'), false,
  'authenticated role cannot execute the retired purchase RPC');
select is(has_function_privilege('service_role', 'public.purchase_my_cosmetic(uuid,text,integer)', 'EXECUTE'), false,
  'service role cannot execute the retired purchase RPC');
select is(has_function_privilege('anon', 'public.equip_my_cosmetic(text,text,text,bigint)', 'EXECUTE'), false,
  'anonymous role cannot execute the retired equipment RPC');
select is(has_function_privilege('authenticated', 'public.equip_my_cosmetic(text,text,text,bigint)', 'EXECUTE'), false,
  'authenticated role cannot execute the retired equipment RPC');
select is(has_function_privilege('service_role', 'public.equip_my_cosmetic(text,text,text,bigint)', 'EXECUTE'), false,
  'service role cannot execute the retired equipment RPC');
select is(has_function_privilege('anon', 'public.import_my_guest_cosmetics(uuid,jsonb,jsonb)', 'EXECUTE'), false,
  'anonymous role cannot execute the retired guest import RPC');
select is(has_function_privilege('authenticated', 'public.import_my_guest_cosmetics(uuid,jsonb,jsonb)', 'EXECUTE'), false,
  'authenticated role cannot execute the retired guest import RPC');
select is(has_function_privilege('service_role', 'public.import_my_guest_cosmetics(uuid,jsonb,jsonb)', 'EXECUTE'), false,
  'service role cannot execute the retired guest import RPC');
select ok(not exists (
  select 1
  from pg_proc p
  cross join lateral aclexplode(p.proacl) acl
  where p.oid in (
    to_regprocedure('public.purchase_my_cosmetic(uuid,text,integer)'),
    to_regprocedure('public.equip_my_cosmetic(text,text,text,bigint)'),
    to_regprocedure('public.import_my_guest_cosmetics(uuid,jsonb,jsonb)')
  ) and acl.grantee = 0 and acl.privilege_type = 'EXECUTE'
), 'PUBLIC has no execute grant on the retired RPCs');

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000957', true);
select throws_ok($$select public.purchase_my_cosmetic(
  '50000000-0000-0000-0000-000000000958', 'flag_v2', 1)$$,
  '42501', null, 'authenticated legacy purchase call is denied');
select throws_ok($$select public.equip_my_cosmetic('legacy-cycle-next', 'sky', 'aurora_v2', 1)$$,
  '42501', null, 'authenticated legacy equipment call is denied');
select throws_ok($$select public.import_my_guest_cosmetics(
  '60000000-0000-0000-0000-000000000958', '[]'::jsonb, '[]'::jsonb)$$,
  '42501', null, 'authenticated legacy import call is denied');
reset role;

select set_config('request.jwt.claim.sub', '', true);
select throws_ok($$select public.purchase_my_cosmetic(
  '50000000-0000-0000-0000-000000000959', 'flag_v2', 1)$$,
  '55000', null, 'owner invocation reports the legacy purchase RPC as retired');
select throws_ok($$select public.equip_my_cosmetic('legacy-cycle-next', 'sky', 'aurora_v2', 1)$$,
  '55000', null, 'owner invocation reports the legacy equipment RPC as retired');
select throws_ok($$select public.import_my_guest_cosmetics(
  '60000000-0000-0000-0000-000000000959', '[]'::jsonb, '[]'::jsonb)$$,
  '55000', null, 'owner invocation reports the legacy import RPC as retired');

select * from finish();
rollback;
