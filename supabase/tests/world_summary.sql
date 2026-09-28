begin;
create extension if not exists pgtap with schema extensions;
select plan(23);
\ir fixtures/planet.inc
select has_table('public', 'planet_member_state', 'member planets table exists');
select ok(to_regprocedure('public.get_world_planets(uuid)') is not null, 'member planets API exists');
select ok(to_regprocedure('public.get_world_summary(uuid)') is null, 'old single world API is retired');
insert into auth.users(id) values
  ('00000000-0000-0000-0000-000000000101'),
  ('00000000-0000-0000-0000-000000000102'),
  ('00000000-0000-0000-0000-000000000103');
insert into public.worlds(id, owner_id, name, timezone) values
  ('20000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000101', 'A', 'Asia/Seoul'),
  ('20000000-0000-0000-0000-000000000002', '00000000-0000-0000-0000-000000000103', 'B', 'Asia/Seoul');
insert into public.world_members(world_id, user_id, role)
values ('20000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000102', 'member');
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000101', true);
do $$ begin perform public.upsert_my_planet_state(pg_temp.planet_state('Alice', 'cycle-1', null,
  '[{"previous_cycle_id":"alice-credit","amount":100000,"created_at_utc":"2026-09-28T00:00:00Z"}]'::jsonb),
  pg_temp.planet_device('30000000-0000-0000-0000-000000000001', 'cycle-1', 100000)); end $$;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000102', true);
do $$ begin perform public.upsert_my_planet_state(pg_temp.planet_state('Bob', 'cycle-1', null,
  '[{"previous_cycle_id":"bob-credit","amount":100000,"created_at_utc":"2026-09-28T00:00:00Z"}]'::jsonb),
  pg_temp.planet_device('30000000-0000-0000-0000-000000000002', 'cycle-1', 100000)); end $$;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000101', true);
select is((select count(*)::int from public.get_world_planets('20000000-0000-0000-0000-000000000001')), 2, 'one independent planet per member');
select is((select sum(lifetime_tokens) from public.get_world_planets('20000000-0000-0000-0000-000000000001')), 200000::numeric, 'only this world member totals are returned');
select ok((select bool_and(stage=0) from public.get_world_planets('20000000-0000-0000-0000-000000000001')), 'both members remain in their first stage');
select ok((select bool_and(abs(growth_credit-1)<0.000001) from public.get_world_planets('20000000-0000-0000-0000-000000000001')), 'each member earns their own daily credit');
select ok((select bool_and(abs(progress_to_next-0.2)<0.000001) from public.get_world_planets('20000000-0000-0000-0000-000000000001')), 'progress is per member');
select ok((select bool_and(not incomplete) from public.get_world_planets('20000000-0000-0000-0000-000000000001')), 'complete contributions remain complete');
select ok(not exists (select 1 from public.get_world_planets('20000000-0000-0000-0000-000000000001') p
  where to_jsonb(p) ?| array['user_id', 'device_id', 'wallet_balance', 'wallet_credits', 'source_path']), 'group response excludes private wallet and source identity');
select throws_ok($$select * from public.get_world_planets('20000000-0000-0000-0000-000000000002')$$, '42501', null, 'other world access is denied');
select throws_ok($$select * from public.planet_member_state$$, '42501', null, 'members cannot bypass the planet RPC');
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000102', true);
select is((select count(*)::int from public.get_world_planets('20000000-0000-0000-0000-000000000001')), 2, 'non-owner can read the group planets');
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000101', true);
select is((public.purchase_my_cosmetic('11111111-1111-4111-8111-111111111111', 'star_cluster', 1)->>'status'), 'purchased', 'Alice can buy a private cosmetic');
select is((public.equip_my_cosmetic('cycle-1', 'sky', 'star_cluster', 0)->>'status'), 'equipped', 'Alice can equip the purchased cosmetic');
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000102', true);
select is((public.purchase_my_cosmetic('22222222-2222-4222-8222-222222222222', 'thin_ring', 1)->>'status'), 'purchased', 'Bob can buy a private cosmetic');
select is((public.equip_my_cosmetic('cycle-1', 'ring', 'thin_ring', 0)->>'status'), 'equipped', 'Bob can equip the purchased cosmetic');
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000101', true);
select is((select equipped_cosmetics from public.get_world_planets('20000000-0000-0000-0000-000000000001') where nickname='Alice'),
  '[{"slot_id":"sky","sku":"star_cluster"}]'::jsonb, 'Alice group row exposes only her current equipped keys');
select is((select equipped_cosmetics from public.get_world_planets('20000000-0000-0000-0000-000000000001') where nickname='Bob'),
  '[{"slot_id":"ring","sku":"thin_ring"}]'::jsonb, 'Bob group row exposes only his current equipped keys');
select ok(not exists (select 1 from public.get_world_planets('20000000-0000-0000-0000-000000000001') p
  where to_jsonb(p) ?| array['user_id', 'device_id', 'wallet_balance', 'wallet_credits', 'source_path',
    'purchase_id', 'price', 'available_balance', 'owned_skus']), 'group response excludes cosmetic ownership and wallet data');
select lives_ok($$select public.delete_synced_usage('20000000-0000-0000-0000-000000000001')$$, 'Alice can hide her group planet');
select is((select equipped_cosmetics from public.get_world_planets('20000000-0000-0000-0000-000000000001') where nickname='행성 동기화 대기'),
  '[]'::jsonb, 'a hidden planet exposes no equipped cosmetics');
reset role;
set local role anon;
select throws_ok($$select * from public.get_world_planets('20000000-0000-0000-0000-000000000001')$$, '42501', null, 'signed-out users cannot read group planets');
select * from finish();
rollback;
