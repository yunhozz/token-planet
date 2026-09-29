begin;
create extension if not exists pgtap with schema extensions;
select plan(22);
\ir fixtures/planet.inc
select ok(to_regprocedure('public.import_my_guest_cosmetics(uuid,jsonb,jsonb)') is not null,
  'atomic guest cosmetic import RPC exists');

insert into auth.users(id) values
  ('00000000-0000-0000-0000-000000000901'),
  ('00000000-0000-0000-0000-000000000902');
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000901', true);
select lives_ok($$select public.upsert_my_planet_state(pg_temp.planet_state('Alice', 'alice-import-cycle', null,
  '[{"previous_cycle_id":"alice-existing-credit","amount":600000,"created_at_utc":"2026-09-28T00:00:00Z"}]'::jsonb),
  pg_temp.planet_device('30000000-0000-0000-0000-000000000901', 'alice-import-cycle', 0))$$, 'Alice starts with a reset credit');
reset role;
insert into private.cosmetic_purchase(user_id,purchase_id,sku,price,purchased_at)
values ('00000000-0000-0000-0000-000000000901', '11111111-1111-4111-8111-111111111111',
  'star_cluster_v2', 500000, '2026-09-28T00:00:00Z');
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000901', true);
select is(public.get_my_cosmetic_state()->'owned_skus', '["star_cluster_v2"]'::jsonb,
  'target account already owns the new-price replacement');
create temporary table guest_import_first as
select public.import_my_guest_cosmetics('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa',
  '[{"previous_cycle_id":"alice-existing-credit","amount":600000,"created_at_utc":"2026-09-28T00:00:00Z"}]'::jsonb,
  '[{"purchase_id":"22222222-2222-4222-8222-222222222222","sku":"star_cluster","price":100000,"purchased_at_utc":"2026-09-28T00:00:00Z"}]'::jsonb) as result;
select is((select result->>'status' from guest_import_first), 'imported', 'valid guest history imports atomically');
select is((select result->'skipped_skus' from guest_import_first), '["star_cluster"]'::jsonb,
  'legacy import reports the equivalent already-owned SKU as skipped');
select is((select (result->>'new_purchase_total')::bigint from guest_import_first), 0::bigint,
  'legacy purchase price is not debited when its replacement is owned');
select is(public.import_my_guest_cosmetics('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa',
  '[{"previous_cycle_id":"alice-existing-credit","amount":600000,"created_at_utc":"2026-09-28T00:00:00Z"}]'::jsonb,
  '[{"purchase_id":"22222222-2222-4222-8222-222222222222","sku":"star_cluster","price":100000,"purchased_at_utc":"2026-09-28T00:00:00Z"}]'::jsonb),
  (select result from guest_import_first), 'same import UUID replays the canonical result');
select is((public.get_my_cosmetic_state()->>'available_balance')::bigint, 100000::bigint, 'existing reset credit is not duplicated or repriced');
select is(public.get_my_cosmetic_state()->'owned_skus', '["star_cluster_v2"]'::jsonb, 'equivalent imported legacy SKU does not grant or debit a duplicate appearance');
select is((select public.import_my_guest_cosmetics('bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb',
  '[{"previous_cycle_id":"alice-new-credit","amount":100000,"created_at_utc":"2026-09-29T00:00:00Z"}]'::jsonb,
  '[{"purchase_id":"33333333-3333-4333-8333-333333333333","sku":"star_cluster","price":100000,"purchased_at_utc":"2026-09-29T00:00:00Z"}]'::jsonb)->>'status'),
  'imported', 'existing account ownership skips the imported purchase charge');
select is((public.get_my_cosmetic_state()->>'available_balance')::bigint, 200000::bigint, 'skipped equivalent purchase leaves imported credit available');
select is((public.get_my_planet_state()->>'wallet_balance')::bigint, 200000::bigint, 'planet wallet uses imported credit minus historical purchases');

select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000902', true);
select lives_ok($$select public.upsert_my_planet_state(pg_temp.planet_state('Bob', 'bob-import-cycle'),
  pg_temp.planet_device('30000000-0000-0000-0000-000000000902', 'bob-import-cycle', 0))$$, 'Bob starts with no wallet credit');
create temporary table guest_import_failure as
select public.import_my_guest_cosmetics('cccccccc-cccc-4ccc-8ccc-cccccccccccc', '[]'::jsonb,
  '[{"purchase_id":"44444444-4444-4444-8444-444444444444","sku":"aurora","price":500000,"purchased_at_utc":"2026-09-28T00:00:00Z"}]'::jsonb) as result;
select is((select result->>'status' from guest_import_failure), 'insufficient_balance', 'underfunded import is rejected');
select is((public.get_my_cosmetic_state()->>'available_balance')::bigint, 0::bigint, 'failed import preserves the target wallet');
select is(jsonb_array_length(public.get_my_cosmetic_state()->'owned_skus'), 0, 'failed import grants no account ownership');
select is(jsonb_array_length(public.get_my_planet_state()->'wallet_credits'), 0, 'failed import does not add guest credits');
select lives_ok($$select public.upsert_my_planet_state(pg_temp.planet_state('Bob', 'bob-import-cycle', null,
  '[{"previous_cycle_id":"bob-delayed-credit","amount":500000,"created_at_utc":"2026-09-29T00:00:00Z"}]'::jsonb),
  pg_temp.planet_device('30000000-0000-0000-0000-000000000902', 'bob-import-cycle', 0))$$,
  'a later planet sync uploads the account credit');
select is((public.import_my_guest_cosmetics('cccccccc-cccc-4ccc-8ccc-cccccccccccc', '[]'::jsonb,
  '[{"purchase_id":"44444444-4444-4444-8444-444444444444","sku":"aurora","price":500000,"purchased_at_utc":"2026-09-28T00:00:00Z"}]'::jsonb)->>'status'),
  'imported', 'the same import ID can retry after the account wallet changes');
select is((public.get_my_cosmetic_state()->>'available_balance')::bigint, 0::bigint,
  'retried import charges the newly available balance exactly once');
select throws_ok($$select * from private.cosmetic_guest_import_request$$, '42501', null, 'import request results remain private');
reset role;
set local role anon;
select throws_ok($$select public.import_my_guest_cosmetics('dddddddd-dddd-4ddd-8ddd-dddddddddddd', '[]'::jsonb, '[]'::jsonb)$$,
  '42501', null, 'anonymous users cannot import guest cosmetics');
select * from finish();
rollback;
