begin;
create extension if not exists pgtap with schema extensions;
select plan(37);
\ir fixtures/planet.inc

select has_table('private', 'cosmetic_slots', 'legacy cosmetic slots remain available for compatibility reads');
select has_table('private', 'cosmetic_products', 'legacy cosmetic product rows remain stored');
select has_table('private', 'cosmetic_style_equivalence', 'legacy appearance mappings remain stored');
select ok(to_regprocedure('public.get_my_cosmetic_state()') is not null,
  'legacy cosmetic state read remains identifiable');
select ok(to_regprocedure('public.purchase_my_cosmetic(uuid,text,integer)') is not null,
  'legacy purchase RPC remains identifiable for retirement checks');
select ok(to_regprocedure('public.equip_my_cosmetic(text,text,text,bigint)') is not null,
  'legacy equipment RPC remains identifiable for retirement checks');
select is(has_function_privilege('anon', 'public.purchase_my_cosmetic(uuid,text,integer)', 'EXECUTE'),
  false, 'anonymous role cannot execute legacy purchases');
select is(has_function_privilege('authenticated', 'public.purchase_my_cosmetic(uuid,text,integer)', 'EXECUTE'),
  false, 'authenticated role cannot execute legacy purchases');
select is(has_function_privilege('service_role', 'public.purchase_my_cosmetic(uuid,text,integer)', 'EXECUTE'),
  false, 'service role cannot execute legacy purchases');
select is(has_function_privilege('anon', 'public.equip_my_cosmetic(text,text,text,bigint)', 'EXECUTE'),
  false, 'anonymous role cannot execute legacy equipment changes');
select is(has_function_privilege('authenticated', 'public.equip_my_cosmetic(text,text,text,bigint)', 'EXECUTE'),
  false, 'authenticated role cannot execute legacy equipment changes');
select is(has_function_privilege('service_role', 'public.equip_my_cosmetic(text,text,text,bigint)', 'EXECUTE'),
  false, 'service role cannot execute legacy equipment changes');

insert into auth.users(id) values ('00000000-0000-0000-0000-000000000801');
insert into private.planet_wallet_credits(user_id, previous_cycle_id, amount, created_at)
values ('00000000-0000-0000-0000-000000000801',
  'server-issued-credit', 600000, '2026-09-28T00:00:00Z');
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000801', true);
select lives_ok($$select public.upsert_my_planet_state(
  pg_temp.planet_state('Alice', 'alice-cycle'),
  pg_temp.planet_device('30000000-0000-0000-0000-000000000801', 'alice-cycle', 0)
)$$, 'owner can upload a canonical current planet');
select is((public.get_my_planet_state()->>'wallet_balance')::bigint, 600000::bigint,
  'wallet projection uses server-issued credits');

reset role;
insert into private.cosmetic_purchase(user_id, purchase_id, sku, price, purchased_at)
values ('00000000-0000-0000-0000-000000000801',
  '11111111-1111-4111-8111-111111111111', 'star_cluster', 100000, '2026-09-28T00:00:00Z');
insert into private.cosmetic_purchase_request(user_id, purchase_id, sku, result)
values ('00000000-0000-0000-0000-000000000801',
  '11111111-1111-4111-8111-111111111111', 'star_cluster', '{"status":"purchased"}'::jsonb);
insert into private.cosmetic_equipment(user_id, cycle_id, slot_id, sku, version)
values ('00000000-0000-0000-0000-000000000801',
  'alice-cycle', 'sky', 'star_cluster', 1);

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000801', true);
select throws_ok($$select public.purchase_my_cosmetic(
  '22222222-2222-4222-8222-222222222222', 'flag_v2', 1)$$,
  '42501', null, 'authenticated legacy purchase call is denied');
select throws_ok($$select public.equip_my_cosmetic(
  'alice-cycle', 'sky', 'star_cluster', 1)$$,
  '42501', null, 'authenticated legacy equipment call is denied');
select throws_ok($$select * from private.cosmetic_purchase$$,
  '42501', null, 'legacy purchase rows remain private');
select is((public.get_my_planet_state()->>'wallet_balance')::bigint, 600000::bigint,
  'denied legacy calls do not debit the current wallet');

reset role;
select throws_ok($$select public.purchase_my_cosmetic(
  '33333333-3333-4333-8333-333333333333', 'flag_v2', 1)$$,
  '55000', null, 'owner invocation reports the purchase RPC as retired');
select throws_ok($$select public.equip_my_cosmetic(
  'alice-cycle', 'sky', 'star_cluster', 1)$$,
  '55000', null, 'owner invocation reports the equipment RPC as retired');
select throws_ok($$update private.cosmetic_products set price = 1 where sku = 'star_cluster'$$,
  '23514', null, 'a registered legacy SKU price remains immutable');
select throws_ok($$update private.cosmetic_style_equivalence
  set legacy_sku = 'aurora' where new_sku = 'star_cluster_v2'$$,
  '23514', null, 'appearance equivalence remains immutable');
select is((select count(*)::bigint from private.cosmetic_purchase
  where user_id = '00000000-0000-0000-0000-000000000801'), 1::bigint,
  'retired purchase calls preserve legacy purchase history');
select is((select count(*)::bigint from private.cosmetic_purchase_request
  where user_id = '00000000-0000-0000-0000-000000000801'), 1::bigint,
  'retired purchase calls preserve legacy request history');
select is((select count(*)::bigint from private.cosmetic_equipment
  where user_id = '00000000-0000-0000-0000-000000000801'), 1::bigint,
  'retired equipment calls preserve the existing appearance');
select is((select count(*)::bigint from private.shop_purchase
  where user_id = '00000000-0000-0000-0000-000000000801'), 0::bigint,
  'retired cosmetic calls do not create current shop purchases');

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000801', true);
select is((public.reset_my_planet(
  '44444444-4444-4444-8444-444444444444', 'alice-cycle')->'action'->>'status'),
  'reset', 'the current reset API advances the server-owned cycle');
reset role;
select ok((select current_cycle_id <> 'alice-cycle' from public.planet_member_state
  where user_id = '00000000-0000-0000-0000-000000000801'),
  'the server chooses the next planet cycle');
select is((select count(*)::bigint from private.cosmetic_equipment
  where user_id = '00000000-0000-0000-0000-000000000801'
    and cycle_id = 'alice-cycle'), 1::bigint,
  'legacy equipment rows survive the server reset');
select is((private.shop_available_balance('00000000-0000-0000-0000-000000000801'))::bigint,
  600000::bigint, 'reset preserves the canonical wallet balance');

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000801', true);
select throws_ok($$select * from private.cosmetic_slots$$, '42501', null,
  'slot catalog is not directly readable');
select throws_ok($$select * from private.cosmetic_products$$, '42501', null,
  'product catalog is not directly readable');
select throws_ok($$select * from private.cosmetic_account_lock$$, '42501', null,
  'account lock table is not directly readable');
select throws_ok($$select * from private.cosmetic_purchase_request$$, '42501', null,
  'purchase request results are not directly readable');
select throws_ok($$select * from private.cosmetic_equipment$$, '42501', null,
  'equipment table is not directly readable');
select throws_ok($$select * from private.cosmetic_style_equivalence$$, '42501', null,
  'style equivalence table is not directly readable');
reset role;
set local role anon;
select throws_ok($$select public.get_my_cosmetic_state()$$, '42501', null,
  'anonymous users cannot read legacy shop state');
reset role;

select * from finish();
rollback;
