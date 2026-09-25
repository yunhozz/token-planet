begin;
create extension if not exists pgtap with schema extensions;
select plan(2);

insert into auth.users(id) values ('00000000-0000-0000-0000-000000000901');
insert into public.worlds(id, owner_id, name, timezone) values
  ('20000000-0000-0000-0000-000000000901', '00000000-0000-0000-0000-000000000901', 'coverage', 'Asia/Seoul');
insert into public.daily_usage_snapshots
  (world_id, user_id, device_id, bucket_date, bucket_policy_version, agent, schema_version, revision, total_tokens, coverage, payload_hash)
values
  ('20000000-0000-0000-0000-000000000901', '00000000-0000-0000-0000-000000000901', '30000000-0000-0000-0000-000000000901', '2026-09-24', 1, 'codex', 1, 1, 100, 'complete', 'a'),
  ('20000000-0000-0000-0000-000000000901', '00000000-0000-0000-0000-000000000901', '30000000-0000-0000-0000-000000000901', '2026-09-25', 1, 'claude_code', 1, 1, 200, 'complete', 'b');

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000901', true);
select is((select incomplete from public.get_world_summary('20000000-0000-0000-0000-000000000901')), true,
  'a different missing agent on each day keeps group coverage incomplete');
reset role;

insert into public.daily_usage_snapshots
  (world_id, user_id, device_id, bucket_date, bucket_policy_version, agent, schema_version, revision, total_tokens, coverage, payload_hash)
values
  ('20000000-0000-0000-0000-000000000901', '00000000-0000-0000-0000-000000000901', '30000000-0000-0000-0000-000000000901', '2026-09-24', 1, 'claude_code', 1, 1, null, 'user_disabled', 'c'),
  ('20000000-0000-0000-0000-000000000901', '00000000-0000-0000-0000-000000000901', '30000000-0000-0000-0000-000000000901', '2026-09-25', 1, 'codex', 1, 1, null, 'user_disabled', 'd');
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000901', true);
select is((select incomplete from public.get_world_summary('20000000-0000-0000-0000-000000000901')), false,
  'explicitly disabled agents fill both day coverage gaps');

select * from finish();
rollback;
