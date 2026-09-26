begin;
create extension if not exists pgtap with schema extensions;
select plan(14);

select ok(to_regprocedure('public.upload_daily_snapshot(uuid,jsonb)') is not null,
  'aggregate upload API exists');

insert into auth.users(id) values
  ('00000000-0000-0000-0000-000000000301'),
  ('00000000-0000-0000-0000-000000000302');
insert into public.worlds(id, owner_id, name, timezone) values
  ('50000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000301', 'Upload', 'Asia/Seoul'),
  ('50000000-0000-0000-0000-000000000002', '00000000-0000-0000-0000-000000000302', 'Other', 'Asia/Seoul');

create temporary table payloads (name text primary key, body jsonb);
insert into payloads(name, body) values
  ('first', jsonb_build_object(
    'device_id', '60000000-0000-0000-0000-000000000001', 'bucket_date', '2026-09-25',
    'bucket_policy_version', 1, 'agent', 'codex', 'schema_version', 1, 'revision', 1,
    'input_tokens', null, 'output_tokens', null, 'cache_read_tokens', null,
    'cache_write_tokens', null, 'total_tokens', 100000, 'coverage', 'complete',
    'payload_hash', repeat('a', 64))),
  ('second_device', jsonb_build_object(
    'device_id', '60000000-0000-0000-0000-000000000002', 'bucket_date', '2026-09-25',
    'bucket_policy_version', 1, 'agent', 'claude_code', 'schema_version', 1, 'revision', 1,
    'input_tokens', null, 'output_tokens', null, 'cache_read_tokens', null,
    'cache_write_tokens', null, 'total_tokens', 100000, 'coverage', 'complete',
    'payload_hash', repeat('b', 64)));
grant select on payloads to authenticated;

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000301', true);
select is(public.upload_daily_snapshot('50000000-0000-0000-0000-000000000001',
  (select body from payloads where name = 'first')), 1::bigint, 'first upload acknowledges revision one');
select is(public.upload_daily_snapshot('50000000-0000-0000-0000-000000000001',
  (select body from payloads where name = 'first')), 1::bigint, 'identical retry acknowledges without adding usage');
select is((select sum(total_tokens)::numeric from public.daily_usage_snapshots where world_id='50000000-0000-0000-0000-000000000001'),
  100000::numeric, 'identical retry keeps one known total');
select throws_ok(
  $$select public.upload_daily_snapshot('50000000-0000-0000-0000-000000000001',
    (select jsonb_set(body, '{total_tokens}', '200000'::jsonb) from payloads where name = 'first'))$$,
  '23514', null, 'same revision with changed data is rejected');
select is(public.upload_daily_snapshot('50000000-0000-0000-0000-000000000001',
  (select jsonb_set(jsonb_set(body, '{revision}', '2'::jsonb), '{payload_hash}', to_jsonb(repeat('c', 64)))
   from payloads where name = 'first')), 2::bigint, 'newer revision replaces the older snapshot');
select is(public.upload_daily_snapshot('50000000-0000-0000-0000-000000000001',
  (select body from payloads where name = 'first')), 2::bigint, 'stale retry receives stored revision');
select is(public.upload_daily_snapshot('50000000-0000-0000-0000-000000000001',
  (select body from payloads where name = 'second_device')), 1::bigint,
  'second device is accepted as a separate aggregate');
select is((select sum(total_tokens)::numeric from public.daily_usage_snapshots where world_id='50000000-0000-0000-0000-000000000001'),
  200000::numeric, 'two devices contribute additively');
select is((select count(*)::integer from public.daily_usage_snapshots where world_id='50000000-0000-0000-0000-000000000001'), 2, 'device aggregates remain separate after retries');
select throws_ok(
  $$select public.upload_daily_snapshot('50000000-0000-0000-0000-000000000002',
    (select body from payloads where name = 'first'))$$,
  '42501', null, 'other world rejects upload');
select throws_ok(
  $$select public.upload_daily_snapshot('50000000-0000-0000-0000-000000000001',
    (select body || '{"source_path":"secret"}'::jsonb from payloads where name = 'first'))$$,
  '23514', null, 'raw source identity field is rejected');
select throws_ok(
  $$select public.upload_daily_snapshot('50000000-0000-0000-0000-000000000001', null::jsonb)$$,
  '23514', null, 'null payload is rejected');
select throws_ok(
  $$select public.upload_daily_snapshot('50000000-0000-0000-0000-000000000001',
    (select jsonb_set(body, '{device_id}', '"not-a-device"'::jsonb) from payloads where name = 'first'))$$,
  '23514', null, 'non-UUID device identity is rejected');

select * from finish();
rollback;
