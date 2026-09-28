begin;
create extension if not exists pgtap with schema extensions;
select plan(53);
\ir fixtures/planet.inc

select has_table('private', 'cosmetic_slots', 'private cosmetic slot catalog exists');
select has_table('private', 'cosmetic_products', 'private cosmetic product catalog exists');
select ok(to_regprocedure('public.get_my_cosmetic_state()') is not null, 'owner shop state RPC exists');
select ok(to_regprocedure('public.purchase_my_cosmetic(uuid,text,integer)') is not null, 'atomic purchase RPC exists');
select ok(to_regprocedure('public.equip_my_cosmetic(text,text,text,bigint)') is not null, 'versioned equipment RPC exists');

insert into auth.users(id) values
  ('00000000-0000-0000-0000-000000000801'),
  ('00000000-0000-0000-0000-000000000802'),
  ('00000000-0000-0000-0000-000000000803');

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000801', true);
select lives_ok($$select public.upsert_my_planet_state(pg_temp.planet_state('Alice', 'alice-cycle', null,
  '[{"previous_cycle_id":"alice-credit","amount":600000,"created_at_utc":"2026-09-28T00:00:00Z"}]'::jsonb),
  pg_temp.planet_device('30000000-0000-0000-0000-000000000801', 'alice-cycle', 0))$$, 'Alice has a funded current planet');
select is(jsonb_array_length(public.get_my_cosmetic_state()->'slots'), 3, 'state contains the catalog slot list');
select is(jsonb_array_length(public.get_my_cosmetic_state()->'products'), 6, 'state contains the six initial catalog products');
select is((public.get_my_cosmetic_state()->>'available_balance')::bigint, 600000::bigint, 'shop balance starts from reset credits');

create temporary table cosmetic_purchase_first as
select public.purchase_my_cosmetic('11111111-1111-4111-8111-111111111111', 'star_cluster', 1) as result;
select is((select result->>'status' from cosmetic_purchase_first), 'purchased', 'basic product purchase succeeds');
select is((select (result->>'price')::bigint from cosmetic_purchase_first), 100000::bigint, 'server catalog supplies purchase price');
select is((select (result->>'available_balance')::bigint from cosmetic_purchase_first), 500000::bigint, 'purchase response returns the net balance');
select is(public.purchase_my_cosmetic('11111111-1111-4111-8111-111111111111', 'star_cluster', 1),
  (select result from cosmetic_purchase_first), 'same request UUID replays the canonical success');
select is((select public.purchase_my_cosmetic('22222222-2222-4222-8222-222222222222', 'star_cluster', 1)->>'status'),
  'already_owned', 'a second request for the same SKU does not charge twice');
select is((public.get_my_planet_state()->>'wallet_balance')::bigint, 500000::bigint, 'planet state exposes the net wallet balance');
select is((select public.purchase_my_cosmetic('11111111-1111-4111-8111-111111111111', 'aurora', 1)->>'status'),
  'request_conflict', 'request UUID cannot be reused for a different SKU');
select is((select public.purchase_my_cosmetic('33333333-3333-4333-8333-333333333333', 'aurora', 2)->>'status'),
  'catalog_mismatch', 'stale catalog revision has a distinct result');

create temporary table cosmetic_equipment_first as
select public.equip_my_cosmetic('alice-cycle', 'sky', 'star_cluster', 0) as result;
select is((select result->>'status' from cosmetic_equipment_first), 'equipped', 'owned matching SKU can be equipped');
select is((select (result->>'version')::bigint from cosmetic_equipment_first), 1::bigint, 'equipment version advances from zero');
select is((select public.equip_my_cosmetic('alice-cycle', 'sky', null, 0)->>'status'),
  'version_conflict', 'stale slot version is rejected');
select is((select public.equip_my_cosmetic('alice-cycle', 'sky', null, 0)->>'sku'),
  'star_cluster', 'version conflict returns the current canonical slot');
select is((select public.equip_my_cosmetic('alice-cycle', 'ring', 'star_cluster', 0)->>'status'),
  'catalog_mismatch', 'SKU cannot be equipped into another slot');
select is((select public.equip_my_cosmetic('alice-cycle', 'sky', 'aurora', 1)->>'status'),
  'not_owned', 'unowned SKU cannot be equipped');
select is((select public.equip_my_cosmetic('old-cycle', 'sky', null, 1)->>'status'),
  'cycle_mismatch', 'old-cycle request cannot restore equipment');
select is((select public.equip_my_cosmetic('alice-cycle', 'sky', null, 1)->>'status'),
  'unequipped', 'slot can be cleared for free');
select is((public.equip_my_cosmetic('alice-cycle', 'sky', null, 1)->>'version')::bigint,
  2::bigint, 'unequip advances the slot version');

reset role;
select throws_ok($$update private.cosmetic_products set price = 1 where sku = 'star_cluster'$$,
  '23514', null, 'a registered SKU price is immutable');
update private.cosmetic_products set purchasable = false where sku = 'star_cluster';
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000801', true);
select is((select public.equip_my_cosmetic('alice-cycle', 'sky', 'star_cluster', 2)->>'status'),
  'equipped', 'retired product remains equippable by its owner');

select lives_ok($$select public.upsert_my_planet_state(pg_temp.planet_state('Alice', 'alice-reset', now(),
  '[{"previous_cycle_id":"alice-reset-credit","amount":600000,"created_at_utc":"2026-09-29T00:00:00Z"}]'::jsonb),
  pg_temp.planet_device('30000000-0000-0000-0000-000000000801', 'alice-reset', 0))$$, 'accepted reset changes the current cycle');
select is(public.get_my_cosmetic_state()->>'current_cycle_id', 'alice-reset', 'shop state follows the new cycle');
select is(jsonb_array_length(public.get_my_cosmetic_state()->'equipped'), 0, 'reset returns an empty equipped list');
create temporary table delayed_old_equipment as
select public.equip_my_cosmetic('alice-cycle', 'sky', 'star_cluster', 3) as result;
select is((select result->>'status' from delayed_old_equipment), 'cycle_mismatch',
  'delayed old-cycle equip after reset is rejected');
select is((select result->>'cycle_id' from delayed_old_equipment), 'alice-reset',
  'delayed equip response returns the latest cycle');
select is((select result->'sku' from delayed_old_equipment), 'null'::jsonb,
  'delayed equip response returns the empty current slot');
reset role;
select is((select count(*)::integer from private.cosmetic_equipment where user_id='00000000-0000-0000-0000-000000000801'),
  0, 'cycle-change trigger clears persisted equipment');

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000802', true);
select lives_ok($$select public.upsert_my_planet_state(pg_temp.planet_state('Bob', 'bob-cycle', null,
  '[{"previous_cycle_id":"bob-credit","amount":499999,"created_at_utc":"2026-09-28T00:00:00Z"}]'::jsonb),
  pg_temp.planet_device('30000000-0000-0000-0000-000000000802', 'bob-cycle', 0))$$, 'Bob has a separate wallet');
create temporary table cosmetic_failed_purchase as
select public.purchase_my_cosmetic('44444444-4444-4444-8444-444444444444', 'aurora', 1) as result;
select is((select result->>'status' from cosmetic_failed_purchase), 'insufficient_balance', 'advanced item rejects a 499999 balance');
select is((select (result->>'available_balance')::bigint from cosmetic_failed_purchase), 499999::bigint, 'insufficient response reports current balance');
select is(jsonb_array_length(public.get_my_cosmetic_state()->'owned_skus'), 0, 'failed purchase does not grant ownership');
select is((public.get_my_planet_state()->>'wallet_balance')::bigint, 499999::bigint, 'failed purchase leaves the wallet unchanged');
select lives_ok($$select public.upsert_my_planet_state(pg_temp.planet_state('Bob', 'bob-cycle', null,
  '[{"previous_cycle_id":"bob-credit","amount":499999,"created_at_utc":"2026-09-28T00:00:00Z"},
    {"previous_cycle_id":"bob-later-credit","amount":1,"created_at_utc":"2026-09-29T00:00:00Z"}]'::jsonb),
  pg_temp.planet_device('30000000-0000-0000-0000-000000000802', 'bob-cycle', 0))$$, 'a later credit may raise the current balance');
select is(public.purchase_my_cosmetic('44444444-4444-4444-8444-444444444444', 'aurora', 1),
  (select result from cosmetic_failed_purchase), 'failure replay returns the original canonical result');
select is((public.get_my_cosmetic_state()->>'available_balance')::bigint, 500000::bigint, 'new wallet balance is visible independently of the cached failure');

select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000803', true);
select is((select public.purchase_my_cosmetic('55555555-5555-4555-8555-555555555555', 'aurora', 1)->>'status'),
  'insufficient_balance', 'purchase lock and result exist before a planet profile');
select is(public.get_my_planet_state(), null::jsonb, 'pre-profile purchase does not create a planet');
reset role;
select is((select count(*)::integer from private.cosmetic_account_lock where user_id='00000000-0000-0000-0000-000000000803'),
  1, 'pre-profile purchase creates the account lock row');

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000801', true);
select throws_ok($$select * from private.cosmetic_slots$$, '42501', null, 'slot catalog is not directly readable');
select throws_ok($$select * from private.cosmetic_products$$, '42501', null, 'product catalog is not directly readable');
select throws_ok($$select * from private.cosmetic_account_lock$$, '42501', null, 'account lock table is not directly readable');
select throws_ok($$select * from private.cosmetic_purchase$$, '42501', null, 'purchase history is not directly readable');
select throws_ok($$select * from private.cosmetic_purchase_request$$, '42501', null, 'request results are not directly readable');
select throws_ok($$select * from private.cosmetic_equipment$$, '42501', null, 'equipment table is not directly readable');
reset role;
set local role anon;
select throws_ok($$select public.get_my_cosmetic_state()$$, '42501', null, 'anonymous users cannot read shop state');
select * from finish();
rollback;
