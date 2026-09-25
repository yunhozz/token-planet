begin;
create extension if not exists pgtap with schema extensions;
select plan(12);

insert into auth.users(id) values ('00000000-0000-0000-0000-000000000911');
insert into public.worlds(id, owner_id, name, timezone) values
  ('20000000-0000-0000-0000-000000000911', '00000000-0000-0000-0000-000000000911', 'cutoff', 'Asia/Seoul');
insert into public.daily_usage_snapshots
  (world_id, user_id, device_id, bucket_date, bucket_policy_version, agent, schema_version, revision, total_tokens, coverage, payload_hash)
values
  ('20000000-0000-0000-0000-000000000911', '00000000-0000-0000-0000-000000000911', '30000000-0000-0000-0000-000000000911', (now() at time zone 'Asia/Seoul')::date, 1, 'codex', 1, 1, 100, 'complete', 'a');

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000911', true);
select is((select public.delete_synced_usage('20000000-0000-0000-0000-000000000911')), 1,
  'deletion removes the requesting member snapshots');
select ok((select paused and deleted_through = (now() at time zone 'Asia/Seoul')::date
  from public.get_my_sync_policy('20000000-0000-0000-0000-000000000911')),
  'deletion records an account-wide pause and creator-timezone cutoff');
select is((select public.upload_daily_snapshot('20000000-0000-0000-0000-000000000911', jsonb_build_object(
  'device_id', '30000000-0000-0000-0000-000000000912',
  'bucket_date', (now() at time zone 'Asia/Seoul')::date::text,
  'bucket_policy_version', 1, 'agent', 'codex', 'schema_version', 1, 'revision', 1,
  'input_tokens', null, 'output_tokens', null, 'cache_read_tokens', null,
  'cache_write_tokens', null, 'total_tokens', 100, 'coverage', 'complete',
  'payload_hash', repeat('a', 64)
))), 1::bigint, 'a second device gets an acknowledgement for deleted history');
select is((select count(*)::integer from public.daily_usage_snapshots where world_id='20000000-0000-0000-0000-000000000911'), 0,
  'a second device cannot restore deleted history');
select throws_ok($$select public.upload_daily_snapshot('20000000-0000-0000-0000-000000000911', jsonb_build_object(
  'device_id', '30000000-0000-0000-0000-000000000912',
  'bucket_date', ((now() at time zone 'Asia/Seoul')::date + 1)::text,
  'bucket_policy_version', 1, 'agent', 'codex', 'schema_version', 1, 'revision', 1,
  'input_tokens', null, 'output_tokens', null, 'cache_read_tokens', null,
  'cache_write_tokens', null, 'total_tokens', 50, 'coverage', 'complete',
  'payload_hash', repeat('b', 64)
))$$, '42501', null, 'another device cannot upload while deletion pause is active');
select is((select public.resume_my_sync('20000000-0000-0000-0000-000000000911')), true,
  'a member can explicitly resume future sharing');
select is((select paused from public.get_my_sync_policy('20000000-0000-0000-0000-000000000911')), false,
  'resuming clears the account-wide pause');
select is((select public.upload_daily_snapshot('20000000-0000-0000-0000-000000000911', jsonb_build_object(
  'device_id', '30000000-0000-0000-0000-000000000912',
  'bucket_date', ((now() at time zone 'Asia/Seoul')::date + 1)::text,
  'bucket_policy_version', 1, 'agent', 'codex', 'schema_version', 1, 'revision', 1,
  'input_tokens', null, 'output_tokens', null, 'cache_read_tokens', null,
  'cache_write_tokens', null, 'total_tokens', 50, 'coverage', 'complete',
  'payload_hash', repeat('b', 64)
))), 1::bigint, 'the next day can be shared after deletion');
select is((select count(*)::integer from public.daily_usage_snapshots where world_id='20000000-0000-0000-0000-000000000911'), 1,
  'only a post-cutoff day appears after resuming');

reset role;
insert into auth.users(id) values ('00000000-0000-0000-0000-000000000912');
insert into public.world_members(world_id, user_id, role) values
  ('20000000-0000-0000-0000-000000000911', '00000000-0000-0000-0000-000000000912', 'member');
set local role authenticated;
select is((select public.transfer_world_owner('20000000-0000-0000-0000-000000000911', '00000000-0000-0000-0000-000000000912')), true,
  'the owner can transfer before leaving');
select is((select public.leave_world('20000000-0000-0000-0000-000000000911')), true,
  'leaving removes the member and their usage');
reset role;
insert into public.world_members(world_id, user_id, role) values
  ('20000000-0000-0000-0000-000000000911', '00000000-0000-0000-0000-000000000911', 'member');
set local role authenticated;
select ok((select paused and deleted_through = (now() at time zone 'Asia/Seoul')::date
  from public.get_my_sync_policy('20000000-0000-0000-0000-000000000911')),
  'rejoining preserves an account-wide cutoff for another device');

select * from finish();
rollback;
