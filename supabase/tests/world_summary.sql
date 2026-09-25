begin;
create extension if not exists pgtap with schema extensions;
select plan(14);

select has_table('public', 'daily_usage_snapshots', 'daily snapshots table exists');
select ok(to_regprocedure('public.get_world_summary(uuid)') is not null, 'world summary API exists');

insert into auth.users(id) values
  ('00000000-0000-0000-0000-000000000101'),
  ('00000000-0000-0000-0000-000000000102'),
  ('00000000-0000-0000-0000-000000000103');
insert into public.worlds(id, owner_id, name, timezone) values
  ('20000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000101', 'A', 'Asia/Seoul'),
  ('20000000-0000-0000-0000-000000000002', '00000000-0000-0000-0000-000000000103', 'B', 'Asia/Seoul');
insert into public.world_members(world_id, user_id, role)
values ('20000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000102', 'member');
insert into public.daily_usage_snapshots
  (world_id, user_id, device_id, bucket_date, bucket_policy_version, agent, schema_version, revision, total_tokens, coverage, payload_hash)
values
  ('20000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000101', '30000000-0000-0000-0000-000000000001', '2026-09-25', 1, 'codex', 1, 1, 50000, 'complete', 'a'),
  ('20000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000101', '30000000-0000-0000-0000-000000000002', '2026-09-25', 1, 'claude_code', 1, 1, 50000, 'complete', 'b'),
  ('20000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000102', '30000000-0000-0000-0000-000000000003', '2026-09-25', 1, 'codex', 1, 1, 100000, 'complete', 'c'),
  ('20000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000102', '30000000-0000-0000-0000-000000000003', '2026-09-25', 1, 'claude_code', 1, 1, null, 'user_disabled', 'd'),
  ('20000000-0000-0000-0000-000000000002', '00000000-0000-0000-0000-000000000103', '30000000-0000-0000-0000-000000000004', '2026-09-25', 1, 'codex', 1, 1, 3000000, 'complete', 'e');

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000101', true);
select is((select member_count from public.get_world_summary('20000000-0000-0000-0000-000000000001')), 2, 'summary counts members');
select is((select known_tokens from public.get_world_summary('20000000-0000-0000-0000-000000000001')), 200000::numeric, 'summary includes only this world known tokens');
select ok(abs((select growth_credit from public.get_world_summary('20000000-0000-0000-0000-000000000001')) - 2) < 0.000001, 'both members earn one credit after sources and devices combine');
select is((select stage from public.get_world_summary('20000000-0000-0000-0000-000000000001')), 0::smallint, 'world remains at stage zero');
select ok(abs((select progress_to_next from public.get_world_summary('20000000-0000-0000-0000-000000000001')) - 0.4) < 0.000001, 'progress reflects two credits toward five');
select is((select incomplete from public.get_world_summary('20000000-0000-0000-0000-000000000001')), false, 'disabled source does not make world incomplete');
select is((select count(*)::int from public.daily_usage_snapshots where world_id = '20000000-0000-0000-0000-000000000001'), 2, 'member cannot read another member snapshot');
select ok(not exists (
  select 1 from public.get_world_summary('20000000-0000-0000-0000-000000000001') s
  where to_jsonb(s) ?| array['user_id', 'device_id', 'member_totals']
), 'summary exposes no member or device totals');
select throws_ok(
  $$select * from public.get_world_summary('20000000-0000-0000-0000-000000000002')$$,
  '42501', null, 'other world summary is denied'
);
reset role;
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000103', true);
select is((select incomplete from public.get_world_summary('20000000-0000-0000-0000-000000000002')), true, 'missing Claude source remains incomplete');
reset role;
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000102', true);
select is((select member_count from public.get_world_summary('20000000-0000-0000-0000-000000000001')), 2, 'non-owner member can read shared summary');
reset role;
set local role anon;
select throws_ok(
  $$select * from public.get_world_summary('20000000-0000-0000-0000-000000000001')$$,
  '42501', null, 'signed-out user cannot read shared summary'
);

select * from finish();
rollback;
