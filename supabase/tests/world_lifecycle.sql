begin;
create extension if not exists pgtap with schema extensions;
select plan(20);

select ok(to_regprocedure('public.delete_synced_usage(uuid)') is not null, 'usage deletion API exists');
select ok(to_regprocedure('public.leave_world(uuid)') is not null, 'leave API exists');
select ok(to_regprocedure('public.transfer_world_owner(uuid,uuid)') is not null, 'ownership transfer API exists');
select ok(to_regprocedure('public.list_world_invites(uuid)') is not null, 'invite list API exists');
select ok(to_regprocedure('public.list_world_members(uuid)') is not null, 'owner transfer member list API exists');

insert into auth.users(id) values
  ('00000000-0000-0000-0000-000000000401'),
  ('00000000-0000-0000-0000-000000000402'),
  ('00000000-0000-0000-0000-000000000403');
insert into public.worlds(id, owner_id, name, timezone) values
  ('70000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000401', 'Life', 'Asia/Seoul'),
  ('70000000-0000-0000-0000-000000000002', '00000000-0000-0000-0000-000000000403', 'Solo', 'Asia/Seoul');
insert into public.world_members(world_id, user_id, role)
values ('70000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000402', 'member');
insert into public.daily_usage_snapshots
  (world_id, user_id, device_id, bucket_date, bucket_policy_version, agent, schema_version, revision, total_tokens, coverage, payload_hash)
values
  ('70000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000401', '80000000-0000-0000-0000-000000000001', '2026-09-25', 1, 'codex', 1, 1, 100000, 'complete', repeat('a',64)),
  ('70000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000402', '80000000-0000-0000-0000-000000000002', '2026-09-25', 1, 'codex', 1, 1, 100000, 'complete', repeat('b',64));

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000401', true);
select throws_ok(
  $$select public.leave_world('70000000-0000-0000-0000-000000000001')$$,
  '23514', null, 'owner must transfer before leaving a populated world');
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000402', true);
select throws_ok(
  $$select public.transfer_world_owner('70000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000402')$$,
  '42501', null, 'member cannot transfer ownership');
select throws_ok(
  $$select * from public.list_world_members('70000000-0000-0000-0000-000000000001')$$,
  '42501', null, 'member cannot read transfer candidates');
select throws_ok(
  $$select * from public.list_world_invites('70000000-0000-0000-0000-000000000001')$$,
  '42501', null, 'member cannot list owner invitations');
select is(public.delete_synced_usage('70000000-0000-0000-0000-000000000001'), 1::integer,
  'member deletes only their own aggregate');
select is((select known_tokens from public.get_world_summary('70000000-0000-0000-0000-000000000001')),
  100000::numeric, 'deleted member aggregate leaves owner aggregate intact');
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000401', true);
select lives_ok(
  $$select * from public.create_world_invite('70000000-0000-0000-0000-000000000001')$$,
  'owner can create an invitation for listing');
select ok(not exists (
  select 1 from public.list_world_invites('70000000-0000-0000-0000-000000000001') i
  where to_jsonb(i) ?| array['code', 'code_hash', 'user_id', 'total_tokens']
), 'invitation list exposes metadata only');
select is((select count(*)::integer from public.list_world_members('70000000-0000-0000-0000-000000000001')),
  2, 'owner can see transfer candidates without usage');
select ok(public.transfer_world_owner('70000000-0000-0000-0000-000000000001',
  '00000000-0000-0000-0000-000000000402'), 'owner transfers to existing member');
select ok(public.leave_world('70000000-0000-0000-0000-000000000001'), 'former owner can leave');
reset role;
select is((select count(*)::integer from public.daily_usage_snapshots where world_id = '70000000-0000-0000-0000-000000000001'),
  0, 'leaving removes former owner aggregate');
select is((select owner_id from public.worlds where id = '70000000-0000-0000-0000-000000000001'),
  '00000000-0000-0000-0000-000000000402'::uuid, 'new owner remains');
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000403', true);
select ok(public.leave_world('70000000-0000-0000-0000-000000000002'), 'solo owner dissolves world');
reset role;
select is((select count(*)::integer from public.worlds where id = '70000000-0000-0000-0000-000000000002'),
  0, 'solo world is removed');

select * from finish();
rollback;
