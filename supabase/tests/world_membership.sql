begin;
create extension if not exists pgtap with schema extensions;
select plan(9);

insert into auth.users(id)
select ('00000000-0000-0000-0000-' || lpad(n::text, 12, '0'))::uuid
from generate_series(1, 15) as n;

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000001', true);
insert into public.worlds(id, owner_id, name, timezone)
values ('10000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000001', 'World A', 'Asia/Seoul');
reset role;
select is((select count(*)::int from public.world_members where world_id = '10000000-0000-0000-0000-000000000001' and role = 'owner'), 1, 'world creation also inserts owner membership');

insert into public.worlds(id, owner_id, name, timezone)
values ('10000000-0000-0000-0000-000000000002', '00000000-0000-0000-0000-000000000013', 'World B', 'Asia/Seoul');
insert into public.world_members(world_id, user_id, role)
values ('10000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000002', 'member');
select is((select count(*)::int from public.world_members where world_id = '10000000-0000-0000-0000-000000000001'), 2, 'second member may join');
select throws_ok(
  $$insert into public.world_members(world_id, user_id, role) values ('10000000-0000-0000-0000-000000000002', '00000000-0000-0000-0000-000000000002', 'member')$$,
  '23505', null, 'one account cannot join two worlds'
);

insert into public.world_members(world_id, user_id, role)
select '10000000-0000-0000-0000-000000000001',
  ('00000000-0000-0000-0000-' || lpad(n::text, 12, '0'))::uuid, 'member'
from generate_series(3, 10) as n;
select throws_ok(
  $$insert into public.world_members(world_id, user_id, role) values ('10000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000011', 'member')$$,
  '23514', null, 'eleventh member is rejected'
);

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000001', true);
select is((select count(*)::int from public.worlds where id = '10000000-0000-0000-0000-000000000001'), 1, 'owner can read own world');
select is((select count(*)::int from public.worlds where id = '10000000-0000-0000-0000-000000000002'), 0, 'other world is hidden');
select throws_ok(
  $$delete from public.worlds where id = '10000000-0000-0000-0000-000000000002'$$,
  '42501', null, 'member cannot mutate another world'
);
reset role;

select throws_ok(
  $$insert into public.worlds(id, owner_id, name, timezone) values ('10000000-0000-0000-0000-000000000004', '00000000-0000-0000-0000-000000000012', 'Invalid time', 'Not/AZone')$$,
  '23514', null, 'creator timezone must be recognized'
);
insert into public.worlds(id, owner_id, name, timezone)
values ('10000000-0000-0000-0000-000000000003', '00000000-0000-0000-0000-000000000014', 'World C', 'Asia/Seoul');
insert into public.world_members(world_id, user_id, role)
values ('10000000-0000-0000-0000-000000000003', '00000000-0000-0000-0000-000000000015', 'member');
select throws_ok(
  $$delete from auth.users where id = '00000000-0000-0000-0000-000000000014'$$,
  '23503', null, 'owner account cannot vanish before transfer or dissolution'
);

select * from finish();
rollback;
