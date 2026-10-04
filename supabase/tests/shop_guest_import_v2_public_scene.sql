-- The imported account begins with a private, hidden public-scene placeholder.
\set ON_ERROR_STOP on
begin;
create extension if not exists pgtap with schema extensions;
set local search_path = extensions, pg_catalog, pg_temp;
create table public.:"rollback_probe" (probe integer);
\ir fixtures/shop_guest_import_v2_first_reset.inc

create function pg_temp.shop_guest_import_v2_reseal(p_request jsonb)
returns jsonb
language plpgsql
as $$
declare
  v_snapshot jsonb := p_request->'snapshot';
  v_canonical text;
  v_fingerprint text;
begin
  select private.shop_guest_import_v2_canonical_json(v_snapshot - 'source_fingerprint')
    into v_canonical;
  v_fingerprint := encode(extensions.digest(convert_to(v_canonical, 'UTF8'), 'sha256'), 'hex');
  return jsonb_set(
    p_request,
    '{snapshot,source_fingerprint}',
    to_jsonb(v_fingerprint),
    true
  );
end;
$$;

create function pg_temp.shop_guest_import_v2_request(p_user_id uuid, p_import_id uuid)
returns jsonb
language plpgsql
as $$
declare
  v_request jsonb := pg_temp.shop_guest_import_v2_first_reset_request();
begin
  v_request := jsonb_set(v_request, '{snapshot,import_id}', to_jsonb(p_import_id::text), true);
  v_request := jsonb_set(
    v_request, '{snapshot,target_account_id}',
    to_jsonb('account:' || p_user_id::text), true
  );
  return pg_temp.shop_guest_import_v2_reseal(v_request);
end;
$$;

create function pg_temp.shop_guest_import_v2_scene_fingerprint()
returns table(table_name text, row_count bigint, row_digest text)
language sql
as $$
  select 'public.planet_member_state'::text, count(*)::bigint,
    md5(coalesce(string_agg(to_jsonb(t)::text, E'\n' order by to_jsonb(t)::text), ''))
  from public.planet_member_state t
  union all
  select 'public.daily_usage_snapshots', count(*)::bigint,
    md5(coalesce(string_agg(to_jsonb(t)::text, E'\n' order by to_jsonb(t)::text), ''))
  from public.daily_usage_snapshots t
  union all
  select 'private.member_sync_policy', count(*)::bigint,
    md5(coalesce(string_agg(to_jsonb(t)::text, E'\n' order by to_jsonb(t)::text), ''))
  from private.member_sync_policy t
  union all
  select 'private.planet_wallet_credits', count(*)::bigint,
    md5(coalesce(string_agg(to_jsonb(t)::text, E'\n' order by to_jsonb(t)::text), ''))
  from private.planet_wallet_credits t
  union all
  select 'private.planet_device_state', count(*)::bigint,
    md5(coalesce(string_agg(to_jsonb(t)::text, E'\n' order by to_jsonb(t)::text), ''))
  from private.planet_device_state t
  union all
  select 'private.shop_guest_import_request', count(*)::bigint,
    md5(coalesce(string_agg(to_jsonb(t)::text, E'\n' order by to_jsonb(t)::text), ''))
  from private.shop_guest_import_request t
  union all
  select 'private.shop_guest_bootstrap_receipt', count(*)::bigint,
    md5(coalesce(string_agg(to_jsonb(t)::text, E'\n' order by to_jsonb(t)::text), ''))
  from private.shop_guest_bootstrap_receipt t
  union all
  select 'private.shop_account_lock', count(*)::bigint,
    md5(coalesce(string_agg(to_jsonb(t)::text, E'\n' order by to_jsonb(t)::text), ''))
  from private.shop_account_lock t
  union all
  select 'private.shop_account_state', count(*)::bigint,
    md5(coalesce(string_agg(to_jsonb(t)::text, E'\n' order by to_jsonb(t)::text), ''))
  from private.shop_account_state t
  union all
  select 'private.shop_effect_history', count(*)::bigint,
    md5(coalesce(string_agg(to_jsonb(t)::text, E'\n' order by to_jsonb(t)::text), ''))
  from private.shop_effect_history t
  union all
  select 'private.shop_effect_contribution', count(*)::bigint,
    md5(coalesce(string_agg(to_jsonb(t)::text, E'\n' order by to_jsonb(t)::text), ''))
  from private.shop_effect_contribution t
  union all
  select 'private.shop_cycle_effect_baseline', count(*)::bigint,
    md5(coalesce(string_agg(to_jsonb(t)::text, E'\n' order by to_jsonb(t)::text), ''))
  from private.shop_cycle_effect_baseline t
  union all
  select 'private.shop_cycle_token_settlement', count(*)::bigint,
    md5(coalesce(string_agg(to_jsonb(t)::text, E'\n' order by to_jsonb(t)::text), ''))
  from private.shop_cycle_token_settlement t
  union all
  select 'private.shop_reset_request', count(*)::bigint,
    md5(coalesce(string_agg(to_jsonb(t)::text, E'\n' order by to_jsonb(t)::text), ''))
  from private.shop_reset_request t
  union all
  select 'private.shop_device_contribution_state', count(*)::bigint,
    md5(coalesce(string_agg(to_jsonb(t)::text, E'\n' order by to_jsonb(t)::text), ''))
  from private.shop_device_contribution_state t
  union all
  select 'private.shop_device_activity_day', count(*)::bigint,
    md5(coalesce(string_agg(to_jsonb(t)::text, E'\n' order by to_jsonb(t)::text), ''))
  from private.shop_device_activity_day t
  union all
  select 'private.shop_activity_day', count(*)::bigint,
    md5(coalesce(string_agg(to_jsonb(t)::text, E'\n' order by to_jsonb(t)::text), ''))
  from private.shop_activity_day t
  union all
  select 'private.growth_journal_state', count(*)::bigint,
    md5(coalesce(string_agg(to_jsonb(t)::text, E'\n' order by to_jsonb(t)::text), ''))
  from private.growth_journal_state t
  union all
  select 'private.growth_journal_cycles', count(*)::bigint,
    md5(coalesce(string_agg(to_jsonb(t)::text, E'\n' order by to_jsonb(t)::text), ''))
  from private.growth_journal_cycles t
  union all
  select 'private.growth_journal_days', count(*)::bigint,
    md5(coalesce(string_agg(to_jsonb(t)::text, E'\n' order by to_jsonb(t)::text), ''))
  from private.growth_journal_days t;
$$;

select plan(9);
insert into auth.users(id) values
  ('00000000-0000-4000-a000-000000000091'),
  ('00000000-0000-4000-a000-000000000092'),
  ('00000000-0000-4000-a000-000000000093'),
  ('00000000-0000-4000-a000-000000000094');
select set_config('request.jwt.claim.sub', '00000000-0000-4000-a000-000000000091', true);

create temporary table shop_guest_import_v2_public_scene_result as
select public.import_guest_shop(
  '9971c7b8-472c-48e6-83d5-5c6ad73abc21',
  pg_temp.shop_guest_import_v2_request(
    '00000000-0000-4000-a000-000000000091',
    '9971c7b8-472c-48e6-83d5-5c6ad73abc21'
  )
) as result;

select ok(
  (select result->>'status' = 'imported'
      and exists (
        select 1 from public.planet_member_state p
        where p.user_id = '00000000-0000-4000-a000-000000000091'
          and p.shared_visible = false
          and p.current_planet_tokens = 0
          and p.growth_credit = 0
          and p.stage = 0
          and p.progress_to_next = 0
          and p.objects = '[]'::jsonb
      )
   from shop_guest_import_v2_public_scene_result),
  'an imported account starts with a hidden zero-state scene placeholder'
);

-- Create sharing membership only after the import, so it cannot affect freshness.
insert into public.worlds(id, owner_id, name, timezone) values
  ('20000000-0000-0000-0000-000000000091',
   '00000000-0000-4000-a000-000000000092', 'Task 9 scene', 'UTC');
insert into public.world_members(world_id, user_id, role) values
  ('20000000-0000-0000-0000-000000000091',
   '00000000-0000-4000-a000-000000000091', 'member');
insert into public.planet_member_state(
  user_id, nickname, avatar, timezone, current_cycle_id, cycle_started_at,
  current_planet_tokens, lifetime_tokens, growth_credit, stage, progress_to_next,
  incomplete, objects, shared_visible
) values (
  '00000000-0000-4000-a000-000000000092', 'Task 9 Viewer', 'masculine', 'UTC',
  'task9-viewer-cycle', transaction_timestamp(), 0, 0, 0, 0, 0,
  false, '[]'::jsonb, true
);
insert into public.worlds(id, owner_id, name, timezone) values
  ('20000000-0000-0000-0000-000000000092',
   '00000000-0000-4000-a000-000000000093', 'Task 9 other world', 'UTC');

-- Fixture and membership writes are complete before the hidden-scene read phase.
create temporary table shop_guest_import_v2_hidden_before as
select * from pg_temp.shop_guest_import_v2_scene_fingerprint();

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-4000-a000-000000000092', true);
select ok(
  (select count(*) = 1
      and bool_and(nickname = '행성 동기화 대기'
        and avatar = 'masculine'
        and stage = 0
        and current_planet_tokens = 0
        and lifetime_tokens = 0
        and growth_credit = 0
        and progress_to_next = 0
        and objects = '[]'::jsonb)
   from public.get_world_planets('20000000-0000-0000-0000-000000000091')
   where nickname = '행성 동기화 대기'),
  'an allowed viewer sees only the zero-state placeholder while sharing is disabled'
);
reset role;
create temporary table shop_guest_import_v2_hidden_after as
select * from pg_temp.shop_guest_import_v2_scene_fingerprint();
select ok(not exists (
  select 1
  from shop_guest_import_v2_hidden_before before_read
  full join shop_guest_import_v2_hidden_after after_read using (table_name)
  where before_read.row_count is distinct from after_read.row_count
     or before_read.row_digest is distinct from after_read.row_digest
), 'the first hidden-scene read leaves public and private game state unchanged');

-- Explicit sharing is an intentional write, so the second read-only phase has
-- its own baseline after that write.
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-4000-a000-000000000091', true);
select public.resume_my_sync('20000000-0000-0000-0000-000000000091');
reset role;
create temporary table shop_guest_import_v2_shared_before as
select * from pg_temp.shop_guest_import_v2_scene_fingerprint();

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-4000-a000-000000000092', true);
select ok(
  exists (
    select 1 from public.get_world_planets('20000000-0000-0000-0000-000000000091') p
    where p.nickname = '행성 동기화 대기'
      and p.avatar = 'masculine'
      and p.stage = 0
      and p.current_planet_tokens = 0
      and p.lifetime_tokens = 1000000
      and p.growth_credit = 0
      and p.progress_to_next = 0
      and p.incomplete = false
      and p.objects = '[]'::jsonb
      and p.token_rank is not null
      and p.civilization_rank is not null
  ),
  'explicit sharing exposes the imported profile, lifetime, empty scene, and existing ranks'
);
select ok(not exists (
  with recursive scene_nodes(value) as (
    select to_jsonb(p)
    from public.get_world_planets('20000000-0000-0000-0000-000000000091') p
    union all
    select child.value
    from scene_nodes parent
    cross join lateral (
      select member.value
      from jsonb_each(case when jsonb_typeof(parent.value) = 'object'
        then parent.value else '{}'::jsonb end) member
      union all
      select element.value
      from jsonb_array_elements(case when jsonb_typeof(parent.value) = 'array'
        then parent.value else '[]'::jsonb end) element
    ) child
  )
  select 1
  from scene_nodes node
  cross join lateral jsonb_object_keys(case when jsonb_typeof(node.value) = 'object'
    then node.value else '{}'::jsonb end) scene_key(key)
  where scene_key.key = any (array[
    'wallet_balance', 'wallet_credits', 'available_balance', 'removal_debits',
    'removal_proofs', 'purchase_proofs', 'purchase_id', 'purchases',
    'reward_timezone', 'era_progress', 'game_rewards', 'last_reset_at_utc',
    'reset_available_at_utc', 'reset_receipt', 'reset_settlement_proofs',
    'import_id', 'request_id', 'expected_cycle_id', 'source_fingerprint',
    'source_account_id', 'source_path',
    'prefix_fingerprint', 'lineage_id', 'occurrence_id', 'occurrences',
    'planet_device_id', 'device_id', 'canonical_version', 'canonical_payload',
    'canonical_contribution', 'ack', 'journal_confirmation', 'effect_revision',
    'cycle_id', 'current_cycle_id', 'old_cycle_id', 'previous_cycle_id',
    'new_cycle_id', 'next_cycle_id',
    'effect_history', 'effect_timeline', 'effect_contributions', 'provenance',
    'agent', 'reward_date', 'raw_tokens', 'prompt', 'log'
  ]::text[])
), 'the recursively inspected public scene contains no private import or reward fields');

select count(*) from public.get_world_planets('20000000-0000-0000-0000-000000000091');
select count(*) from public.get_world_planets('20000000-0000-0000-0000-000000000091');
select count(*) from public.get_world_planets('20000000-0000-0000-0000-000000000091');
reset role;
create temporary table shop_guest_import_v2_shared_after as
select * from pg_temp.shop_guest_import_v2_scene_fingerprint();
select ok(not exists (
  select 1
  from shop_guest_import_v2_shared_before before_read
  full join shop_guest_import_v2_shared_after after_read using (table_name)
  where before_read.row_count is distinct from after_read.row_count
     or before_read.row_digest is distinct from after_read.row_digest
), 'the first and repeated shared-scene reads do not write settlement, receipt, canonical revision, or ACK state');

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-4000-a000-000000000094', true);
select throws_ok(
  $$select * from public.get_world_planets('20000000-0000-0000-0000-000000000091')$$,
  '42501', null, 'an authenticated outsider cannot read a world scene'
);
select set_config('request.jwt.claim.sub', '00000000-0000-4000-a000-000000000093', true);
select throws_ok(
  $$select * from public.get_world_planets('20000000-0000-0000-0000-000000000091')$$,
  '42501', null, 'a member of another world cannot read this world scene'
);
reset role;
set local role anon;
select set_config('request.jwt.claim.sub', '', true);
select throws_ok(
  $$select * from public.get_world_planets('20000000-0000-0000-0000-000000000091')$$,
  '42501', null, 'anonymous callers cannot read a public scene'
);
reset role;

select * from finish();
rollback;
\echo SHOP_GUEST_IMPORT_V2_ROLLBACK_COMPLETED
