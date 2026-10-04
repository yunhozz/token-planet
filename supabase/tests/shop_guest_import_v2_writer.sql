-- Atomic schema-2 first-reset writer contract. The native fixture stays byte-for-byte frozen.
\set ON_ERROR_STOP on
begin;
create extension if not exists pgtap with schema extensions;
set local search_path = extensions, pg_catalog, pg_temp;
create table public.:"rollback_probe" (probe integer);
\ir fixtures/shop_guest_import_v2_first_reset.inc
\ir fixtures/shop_guest_import_native_first_reset.inc

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
    v_request,
    '{snapshot,target_account_id}',
    to_jsonb('account:' || p_user_id::text),
    true
  );
  return pg_temp.shop_guest_import_v2_reseal(v_request);
end;
$$;

create function pg_temp.shop_guest_import_v2_request_with_profile(
  p_user_id uuid,
  p_import_id uuid,
  p_profile jsonb
)
returns jsonb
language plpgsql
as $$
begin
  return pg_temp.shop_guest_import_v2_reseal(jsonb_set(
    pg_temp.shop_guest_import_v2_request(p_user_id, p_import_id),
    '{snapshot,data,profile}', p_profile, true
  ));
end;
$$;

create function pg_temp.shop_guest_import_v2_bad_final_proof(p_request jsonb)
returns jsonb
language sql
as $$
  select pg_temp.shop_guest_import_v2_reseal(jsonb_set(
    p_request,
    '{snapshot,canonical_payload,lifetime_tokens}',
    '999999'::jsonb,
    true
  ));
$$;

create function pg_temp.shop_guest_import_v2_try_bootstrap(
  p_user_id uuid,
  p_import_id uuid,
  p_request jsonb
)
returns jsonb
language plpgsql
as $$
declare
  v_result jsonb;
begin
  perform set_config(
    'request.jwt.claim.sub',
    coalesce(p_user_id::text, ''),
    true
  );
  execute 'select private.shop_guest_import_v2_bootstrap($1,$2)'
    into v_result using p_import_id, p_request;
  return v_result;
exception when undefined_function then
  -- Keep the intended pre-migration RED visible as TAP instead of a SQL parse error.
  return null;
end;
$$;

create function pg_temp.shop_guest_import_v2_counts(p_user_id uuid)
returns jsonb
language sql
stable
as $$
  select jsonb_build_object(
    'planet', (select count(*) from public.planet_member_state where user_id = p_user_id),
    'lock', (select count(*) from private.shop_account_lock where user_id = p_user_id),
    'receipt', (select count(*) from private.shop_guest_bootstrap_receipt where user_id = p_user_id),
    'device', (select count(*) from private.planet_device_state where user_id = p_user_id),
    'wallet', (select count(*) from private.planet_wallet_credits where user_id = p_user_id),
    'journal_state', (select count(*) from private.growth_journal_state where user_id = p_user_id),
    'journal_cycle', (select count(*) from private.growth_journal_cycles where user_id = p_user_id),
    'journal_day', (select count(*) from private.growth_journal_days where user_id = p_user_id),
    'contribution_state', (select count(*) from private.shop_device_contribution_state where user_id = p_user_id),
    'effect_contribution', (select count(*) from private.shop_effect_contribution where user_id = p_user_id),
    'device_activity', (select count(*) from private.shop_device_activity_day where user_id = p_user_id),
    'activity', (select count(*) from private.shop_activity_day where user_id = p_user_id),
    'baseline', (select count(*) from private.shop_cycle_effect_baseline where user_id = p_user_id),
    'reset_request', (select count(*) from private.shop_reset_request where user_id = p_user_id),
    'settlement', (select count(*) from private.shop_cycle_token_settlement where user_id = p_user_id)
  );
$$;

create function pg_temp.shop_guest_import_v2_no_game_writes(p_user_id uuid)
returns boolean
language sql
stable
as $$
  select not exists (
    select 1 from public.planet_member_state where user_id = p_user_id
    union all select 1 from private.shop_account_state where user_id = p_user_id
    union all select 1 from private.planet_device_state where user_id = p_user_id
    union all select 1 from private.planet_wallet_credits where user_id = p_user_id
    union all select 1 from private.growth_journal_state where user_id = p_user_id
    union all select 1 from private.growth_journal_cycles where user_id = p_user_id
    union all select 1 from private.growth_journal_days where user_id = p_user_id
    union all select 1 from private.shop_device_contribution_state where user_id = p_user_id
    union all select 1 from private.shop_effect_contribution where user_id = p_user_id
    union all select 1 from private.shop_device_activity_day where user_id = p_user_id
    union all select 1 from private.shop_activity_day where user_id = p_user_id
    union all select 1 from private.shop_cycle_effect_baseline where user_id = p_user_id
    union all select 1 from private.shop_planet_object_generation_baseline where user_id = p_user_id
    union all select 1 from private.shop_reset_request where user_id = p_user_id
    union all select 1 from private.shop_cycle_token_settlement where user_id = p_user_id
    union all select 1 from private.shop_purchase where user_id = p_user_id
    union all select 1 from private.shop_landscape_instance where user_id = p_user_id
    union all select 1 from private.shop_landscape_placement where user_id = p_user_id
    union all select 1 from private.shop_avatar_owned where user_id = p_user_id
    union all select 1 from private.shop_avatar_equipment where user_id = p_user_id
    union all select 1 from private.shop_game_reward where user_id = p_user_id
    union all select 1 from private.shop_effect_history where user_id = p_user_id
    union all select 1 from private.shop_action_request where user_id = p_user_id
    union all select 1 from private.shop_natural_removal where user_id = p_user_id
  );
$$;

create function private.shop_guest_import_v2_writer_test_fail()
returns trigger
language plpgsql
set search_path = ''
as $$
begin
  if new.user_id::text = current_setting('test.shop_guest_import_v2_failure_user', true)
    and tg_table_name = current_setting('test.shop_guest_import_v2_failure_table', true)
  then
    raise exception 'synthetic writer failure at %', tg_table_name using errcode = 'P0001';
  end if;
  return new;
end;
$$;

insert into auth.users(id) values
  ('00000000-0000-4000-a000-000000000001'),
  ('00000000-0000-4000-a000-000000000002'),
  ('00000000-0000-4000-a000-000000000071'),
  ('00000000-0000-4000-a000-000000000072'),
  ('00000000-0000-4000-a000-000000000073'),
  ('00000000-0000-4000-a000-000000000074'),
  ('00000000-0000-4000-a000-000000000075'),
  ('00000000-0000-4000-a000-000000000076'),
  ('00000000-0000-4000-a000-000000000077'),
  ('00000000-0000-4000-a000-000000000078');

-- The source fixture has profile=null. Metadata is a deliberate decoy and must not supply it.
update auth.users
set raw_user_meta_data = '{"nickname":"Metadata Decoy","avatar":"feminine"}'::jsonb
where id = '00000000-0000-4000-a000-000000000071';

create temporary table shop_guest_import_v2_writer_main as
select
  '00000000-0000-4000-a000-000000000071'::uuid as user_id,
  '4793fe7a-ddea-406d-ae19-da0560f3936f'::uuid as import_id,
  pg_temp.shop_guest_import_v2_request(
    '00000000-0000-4000-a000-000000000071',
    '4793fe7a-ddea-406d-ae19-da0560f3936f'
  ) as request;

-- A prior schema-1 public held receipt is immutable but does not poison another import ID.
insert into private.shop_guest_import_request(
  user_id, import_id, payload, source_fingerprint, status, result
)
select
  '00000000-0000-4000-a000-000000000001',
  '7da19666-68ec-4a45-b1e4-a5f195e8d584',
  pg_temp.shop_guest_import_native_first_reset_request(),
  '9ff43e38fc26b744c5a9f98a7145d1f3c1831c55720dfcfcfc83468a9d33a451',
  'source_unverifiable',
  jsonb_build_object('import_id','7da19666-68ec-4a45-b1e4-a5f195e8d584','status','source_unverifiable');

-- Legacy private receipt replay is checked before freshness, even if its proof rows are absent.
insert into private.shop_guest_bootstrap_receipt(
  user_id, import_id, payload, source_fingerprint, status, result
)
select
  '00000000-0000-4000-a000-000000000002',
  '88888888-8888-4888-8888-888888888881',
  jsonb_set(
    jsonb_set(
      pg_temp.shop_guest_import_native_first_reset_request(),
      '{snapshot,import_id}', to_jsonb('88888888-8888-4888-8888-888888888881'::text), true
    ),
    '{snapshot,target_account_id}', to_jsonb('account:00000000-0000-4000-a000-000000000002'::text), true
  ),
  '9ff43e38fc26b744c5a9f98a7145d1f3c1831c55720dfcfcfc83468a9d33a451',
  'imported',
  '{"schema_version":1,"import_id":"88888888-8888-4888-8888-888888888881","account_id":"account:00000000-0000-4000-a000-000000000002","status":"imported"}'::jsonb;

-- An orphan imported receipt remains a freshness blocker for a different valid ID.
insert into private.shop_guest_bootstrap_receipt(
  user_id, import_id, payload, source_fingerprint, status, result
)
select
  '00000000-0000-4000-a000-000000000072',
  '99999999-9999-4999-8999-999999999991',
  pg_temp.shop_guest_import_v2_request(
    '00000000-0000-4000-a000-000000000072',
    '99999999-9999-4999-8999-999999999991'
  ),
  'da9fc489f199caf407dd84d48244bd85adc9bf9aeafedcd88dd77fb3573678ee',
  'imported',
  '{"schema_version":2,"import_id":"99999999-9999-4999-8999-999999999991","account_id":"account:00000000-0000-4000-a000-000000000072","status":"imported"}'::jsonb;

create trigger shop_guest_import_v2_writer_test_fail_journal
before insert on private.growth_journal_days
for each row execute function private.shop_guest_import_v2_writer_test_fail();
create trigger shop_guest_import_v2_writer_test_fail_settlement
before insert on private.shop_cycle_token_settlement
for each row execute function private.shop_guest_import_v2_writer_test_fail();
create trigger shop_guest_import_v2_writer_test_fail_receipt
before insert on private.shop_guest_bootstrap_receipt
for each row execute function private.shop_guest_import_v2_writer_test_fail();

select plan(47);

select ok(
  private.shop_guest_import_v2_normalize(
    (select request from shop_guest_import_v2_writer_main), transaction_timestamp()
  ) is not null,
  'the unchanged native schema-2 first-reset fixture is accepted by the installed validator'
);
select ok(
  to_regprocedure('private.shop_guest_import_v2_bootstrap(uuid,jsonb)') is not null,
  'the private v2 atomic bootstrap writer is installed'
);

select set_config('request.jwt.claim.sub', '', true);
select throws_ok(
  format(
    'select private.shop_guest_import_v2_bootstrap(%L::uuid,%L::jsonb)',
    (select import_id from shop_guest_import_v2_writer_main),
    (select request::text from shop_guest_import_v2_writer_main)
  ),
  '42501', 'authentication required',
  'anonymous execution is rejected before any writer side effect'
);
select set_config('request.jwt.claim.sub', '00000000-0000-4000-a000-000000000072', true);
select throws_ok(
  format(
    'select private.shop_guest_import_v2_bootstrap(%L::uuid,%L::jsonb)',
    (select import_id from shop_guest_import_v2_writer_main),
    (select request::text from shop_guest_import_v2_writer_main)
  ),
  '42501', 'guest shop bootstrap target does not match authenticated account',
  'a different authenticated account cannot bootstrap the fixture target'
);
select ok(
  pg_temp.shop_guest_import_v2_no_game_writes('00000000-0000-4000-a000-000000000071')
    and not exists (select 1 from private.shop_account_lock
      where user_id = '00000000-0000-4000-a000-000000000071')
    and not exists (select 1 from private.shop_guest_bootstrap_receipt
      where user_id = '00000000-0000-4000-a000-000000000071'),
  'auth and target failures leave the target without a lock, game rows, or receipt'
);

create temporary table shop_guest_import_v2_writer_imported as
select pg_temp.shop_guest_import_v2_try_bootstrap(user_id, import_id, request) as result
from shop_guest_import_v2_writer_main;

select is(
  (select result->>'status' from shop_guest_import_v2_writer_imported),
  'imported',
  'the unchanged native schema-2 first-reset source imports atomically'
);
select ok(
  (select result->>'schema_version' = '2'
    and result->>'import_id' = import_id::text
    and result->>'account_id' = 'account:' || user_id::text
    and result->>'source_fingerprint' = request#>>'{snapshot,source_fingerprint}'
    and result->>'status' = 'imported'
   from shop_guest_import_v2_writer_imported cross join shop_guest_import_v2_writer_main),
  'the imported result preserves schema, account, import, and source correlation'
);
select ok(
  (select jsonb_typeof(result->'shop_state') = 'object'
      and jsonb_typeof(result->'planet_state') = 'object'
      and jsonb_typeof(result->'effect_timeline') = 'object'
      and jsonb_typeof(result->'canonical_contribution') = 'object'
      and jsonb_typeof(result->'journal_confirmation') = 'object'
      and jsonb_typeof(result->'ack') = 'object'
   from shop_guest_import_v2_writer_imported),
  'success returns every typed state, timeline, journal, canonical, and ACK projection'
);
select is(
  (select result->'ack' from shop_guest_import_v2_writer_imported),
  (select private.shop_guest_import_v2_normalize(request, transaction_timestamp())->'ack'
   from shop_guest_import_v2_writer_main),
  'the receipt ACK exactly confirms the normalized immutable prefix and journal rows'
);
select is(
  (select result->'canonical_contribution' from shop_guest_import_v2_writer_imported),
  (select private.shop_guest_import_v2_normalize(request, transaction_timestamp())->'canonical_payload'
   from shop_guest_import_v2_writer_main),
  'the result canonical contribution equals the independently normalized payload'
);
select ok(
  (select result#>>'{planet_state,current_cycle_id}' = 'eafa93c3-1c6b-4687-aa60-ce6011a27f9b'
      and result#>>'{planet_state,current_planet_tokens}' = '0'
      and result#>>'{planet_state,lifetime_tokens}' = '1000000'
      and result#>>'{planet_state,last_reset_at_utc}' = '2026-10-03T05:26:26.192093Z'
      and result#>>'{planet_state,reset_available_at_utc}' = '2026-10-04T05:26:26.192093Z'
      and result#>'{planet_state,wallet_balance}' = '1000000'::jsonb
      and result#>'{planet_state,objects}' = '[]'::jsonb
      and result->'planet_state'->'profile' =
        '{"nickname":"행성 동기화 대기","avatar":"masculine"}'::jsonb
   from shop_guest_import_v2_writer_imported),
  'the hidden planet projection adopts reset state, wallet, empty objects, and the canonical null-profile placeholder'
);
select ok(
  (select result#>>'{shop_state,account_id}' = 'account:00000000-0000-4000-a000-000000000071'
      and result#>>'{shop_state,current_cycle_id}' = 'eafa93c3-1c6b-4687-aa60-ce6011a27f9b'
      and result#>>'{shop_state,reward_state,reward_timezone}' = 'UTC'
      and result#>'{shop_state,available_balance}' = '1000000'::jsonb
      and result#>'{shop_state,landscape_instances}' = '[]'::jsonb
      and result#>'{shop_state,placements}' = '[]'::jsonb
      and result#>'{shop_state,avatar_owned_skus}' = '[]'::jsonb
   from shop_guest_import_v2_writer_imported),
  'shop state contains the adopted wallet and timezone with no item ownership or placement'
);
select ok(
  (select result#>>'{effect_timeline,current_cycle_id}' = 'eafa93c3-1c6b-4687-aa60-ce6011a27f9b'
      and result#>>'{effect_timeline,reward_timezone}' = 'UTC'
      and result#>'{effect_timeline,cycle_bounds}' =
        private.shop_guest_import_v2_normalize(request, transaction_timestamp())->'cycle_bounds'
      and result#>'{effect_timeline,intervals}' = '[]'::jsonb
   from shop_guest_import_v2_writer_imported cross join shop_guest_import_v2_writer_main),
  'timeline confirms the old and current baselines without a positive effect interval'
);
select ok(
  exists (
    select 1 from private.planet_device_state d
    where d.user_id = '00000000-0000-4000-a000-000000000071'
      and d.device_id = '574b76bb-7e3a-4089-8f8b-f76b7f6df927'
      and d.current_cycle_id = 'eafa93c3-1c6b-4687-aa60-ce6011a27f9b'
      and d.lifetime_tokens = 1000000 and d.current_planet_tokens = 0
      and d.canonical_version = 1
  ) and exists (
    select 1 from private.shop_device_contribution_state c
    where c.user_id = '00000000-0000-4000-a000-000000000071'
      and c.device_id = '574b76bb-7e3a-4089-8f8b-f76b7f6df927'
      and c.canonical_version = 1
      and c.canonical_payload = (select result->'canonical_contribution'
        from shop_guest_import_v2_writer_imported)
  ),
  'one device and its canonical contribution are adopted with the exact current version'
);
select ok(
  (select count(*) = 1 from private.shop_effect_contribution c
    where c.user_id = '00000000-0000-4000-a000-000000000071'
      and c.device_id = '574b76bb-7e3a-4089-8f8b-f76b7f6df927'
      and c.cycle_id = '1fc7f5db-fc65-49ae-a20e-55da8ea7b6d2'
      and c.date = '2026-10-03' and c.effect_revision = 0
      and c.canonical_version = 1 and c.tokens = 1000000
      and c.growth_bps = 0 and c.wallet_bps = 0)
    and (select count(*) = 1 from private.shop_device_activity_day d
      where d.user_id = '00000000-0000-4000-a000-000000000071'
        and d.reward_date = '2026-10-03' and d.tokens = 1000000)
    and (select count(*) = 1 from private.shop_activity_day d
      where d.user_id = '00000000-0000-4000-a000-000000000071'
        and d.reward_date = '2026-10-03' and d.tokens = 1000000),
  'the old-cycle zero-effect contribution and activity are persisted exactly once'
);
select ok(
  (select count(*) = 2 from private.shop_cycle_effect_baseline b
    where b.user_id = '00000000-0000-4000-a000-000000000071')
    and exists (select 1 from private.shop_cycle_effect_baseline b
      where b.user_id = '00000000-0000-4000-a000-000000000071'
        and b.cycle_id = '1fc7f5db-fc65-49ae-a20e-55da8ea7b6d2'
        and b.started_at = '2026-10-03T05:26:25.938450Z'::timestamptz
        and b.ended_at = '2026-10-03T05:26:26.192093Z'::timestamptz)
    and exists (select 1 from private.shop_cycle_effect_baseline b
      where b.user_id = '00000000-0000-4000-a000-000000000071'
        and b.cycle_id = 'eafa93c3-1c6b-4687-aa60-ce6011a27f9b'
        and b.started_at = '2026-10-03T05:26:26.192093Z'::timestamptz
        and b.ended_at is null),
  'both exact old and new effect baselines are adopted'
);
select ok(
  exists (select 1 from private.shop_reset_request r
    where r.user_id = '00000000-0000-4000-a000-000000000071'
      and r.request_id = '8a161fbb-2b79-458b-a751-a6388fd3cc68'
      and r.expected_cycle_id = '1fc7f5db-fc65-49ae-a20e-55da8ea7b6d2'
      and r.status = 'reset')
    and exists (select 1 from private.shop_cycle_token_settlement s
      where s.user_id = '00000000-0000-4000-a000-000000000071'
        and s.cycle_id = '1fc7f5db-fc65-49ae-a20e-55da8ea7b6d2'
        and s.next_cycle_id = 'eafa93c3-1c6b-4687-aa60-ce6011a27f9b'
        and s.raw_tokens = 1000000 and s.bonus_tokens = 0
        and s.effect_revision = 0 and s.reset_cooldown_bps = 0)
    and exists (select 1 from private.planet_wallet_credits w
      where w.user_id = '00000000-0000-4000-a000-000000000071'
        and w.previous_cycle_id = '1fc7f5db-fc65-49ae-a20e-55da8ea7b6d2'
        and w.amount = 1000000
        and w.created_at = '2026-10-03T05:26:26.192093Z'::timestamptz),
  'the reset request, raw-only settlement, and wallet credit use the captured reset proof'
);
select ok(
  exists (select 1 from private.growth_journal_state s
    where s.user_id = '00000000-0000-4000-a000-000000000071'
      and s.generation = 0 and s.deleted_at is null and s.timezone = 'UTC')
    and (select count(*) = 2 from private.growth_journal_cycles c
      where c.user_id = '00000000-0000-4000-a000-000000000071')
    and exists (select 1 from private.growth_journal_days d
      where d.user_id = '00000000-0000-4000-a000-000000000071'
        and d.device_id = '574b76bb-7e3a-4089-8f8b-f76b7f6df927'
        and d.cycle_id = '1fc7f5db-fc65-49ae-a20e-55da8ea7b6d2'
        and d.bucket_date = '2026-10-03' and d.agent = 'codex'
        and d.generation = 0 and d.revision = 1 and d.confirmed_tokens = 1000000
        and d.coverage = 'complete' and d.present
        and d.payload_hash = '6f73aabb80e7253ddfb9cc7a5a85af1bf285c8e869e12252081ca4adfe66fb63')
    and (select result->'journal_confirmation'->'entries'->0->>'payload_hash'
      from shop_guest_import_v2_writer_imported) =
      '6f73aabb80e7253ddfb9cc7a5a85af1bf285c8e869e12252081ca4adfe66fb63',
  'the exact two-cycle journal and its acknowledged native entry are persisted'
);
select ok(
  exists (select 1 from public.planet_member_state p
    where p.user_id = '00000000-0000-4000-a000-000000000071'
      and not p.shared_visible and p.objects = '[]'::jsonb)
    and not exists (select 1 from public.worlds w
      where w.owner_id = '00000000-0000-4000-a000-000000000071')
    and not exists (select 1 from private.shop_game_reward r
      where r.user_id = '00000000-0000-4000-a000-000000000071')
    and not exists (select 1 from private.shop_purchase p
      where p.user_id = '00000000-0000-4000-a000-000000000071')
    and not exists (select 1 from private.shop_landscape_instance i
      where i.user_id = '00000000-0000-4000-a000-000000000071')
    and not exists (select 1 from private.shop_effect_history e
      where e.user_id = '00000000-0000-4000-a000-000000000071'),
  'bootstrap remains hidden and creates no world, reward, item, or positive effect'
);
select ok(
  exists (select 1 from private.shop_guest_bootstrap_receipt r
    where r.user_id = '00000000-0000-4000-a000-000000000071'
      and r.import_id = '4793fe7a-ddea-406d-ae19-da0560f3936f'
      and r.payload = (select request from shop_guest_import_v2_writer_main)
      and r.source_fingerprint = 'da9fc489f199caf407dd84d48244bd85adc9bf9aeafedcd88dd77fb3573678ee'
      and r.status = 'imported'
      and r.result = (select result from shop_guest_import_v2_writer_imported)
      and r.created_at = transaction_timestamp()),
  'the exact request, normalized provenance through ACK, and success result are stored in the final receipt'
);

create temporary table shop_guest_import_v2_writer_after_import as
select pg_temp.shop_guest_import_v2_counts('00000000-0000-4000-a000-000000000071') as counts;
select is(
  pg_temp.shop_guest_import_v2_try_bootstrap(
    '00000000-0000-4000-a000-000000000071',
    '4793fe7a-ddea-406d-ae19-da0560f3936f',
    (select request from shop_guest_import_v2_writer_main)
  ),
  (select result from shop_guest_import_v2_writer_imported),
  'same ID and identical payload replay the immutable imported DTO'
);
select is(
  pg_temp.shop_guest_import_v2_counts('00000000-0000-4000-a000-000000000071'),
  (select counts from shop_guest_import_v2_writer_after_import),
  'replay leaves every canonical row and receipt count unchanged'
);
select is(
  (pg_temp.shop_guest_import_v2_try_bootstrap(
    '00000000-0000-4000-a000-000000000071',
    '4793fe7a-ddea-406d-ae19-da0560f3936f',
    pg_temp.shop_guest_import_v2_reseal(jsonb_set(
      (select request from shop_guest_import_v2_writer_main),
      '{snapshot,captured_at_utc}', '"2026-10-03T05:26:26.215074Z"'::jsonb, true
    ))
  ))->>'status',
  'request_conflict',
  'same ID with a different correctly resealed payload conflicts before freshness'
);

create temporary table shop_guest_import_v2_writer_new_id as
select pg_temp.shop_guest_import_v2_try_bootstrap(
  '00000000-0000-4000-a000-000000000071',
  '77777777-7777-4777-8777-777777777771',
  pg_temp.shop_guest_import_v2_request(
    '00000000-0000-4000-a000-000000000071',
    '77777777-7777-4777-8777-777777777771'
  )
) as result;
select is((select result->>'status' from shop_guest_import_v2_writer_new_id),
  'active_account', 'a new import ID cannot bootstrap an already imported target');
select ok(
  (select count(*) = 1 from public.planet_member_state
    where user_id = '00000000-0000-4000-a000-000000000071'
      and lifetime_tokens = 1000000)
    and (select count(*) = 2 from private.shop_guest_bootstrap_receipt
      where user_id = '00000000-0000-4000-a000-000000000071'),
  'the active-account result preserves the existing planet and writes only its new receipt'
);

select is(
  pg_temp.shop_guest_import_v2_try_bootstrap(
    '00000000-0000-4000-a000-000000000001',
    '7da19666-68ec-4a45-b1e4-a5f195e8d584',
    pg_temp.shop_guest_import_native_first_reset_request()
  ),
  (select result from private.shop_guest_import_request
   where user_id = '00000000-0000-4000-a000-000000000001'
     and import_id = '7da19666-68ec-4a45-b1e4-a5f195e8d584'),
  'the existing public schema-1 held receipt remains immutable at the private writer boundary'
);
select is(
  (pg_temp.shop_guest_import_v2_try_bootstrap(
    '00000000-0000-4000-a000-000000000001',
    '7da19666-68ec-4a45-b1e4-a5f195e8d584',
    jsonb_set(pg_temp.shop_guest_import_native_first_reset_request(),
      '{snapshot,source_fingerprint}', to_jsonb(repeat('0',64)), true)
  ))->>'status',
  'request_conflict',
  'a changed payload cannot promote or rewrite an existing schema-1 held receipt'
);
select ok(
  private.shop_guest_import_target_is_fresh('00000000-0000-4000-a000-000000000001')
    and not exists (select 1 from private.shop_guest_bootstrap_receipt
      where user_id = '00000000-0000-4000-a000-000000000001'),
  'a held receipt alone leaves another valid import ID eligible'
);
select is(
  (pg_temp.shop_guest_import_v2_try_bootstrap(
    '00000000-0000-4000-a000-000000000001',
    '77777777-7777-4777-8777-777777777772',
    pg_temp.shop_guest_import_v2_request(
      '00000000-0000-4000-a000-000000000001',
      '77777777-7777-4777-8777-777777777772'
    )
  ))->>'status',
  'imported',
  'a prior public held receipt does not block a different valid schema-2 import ID'
);

select is(
  pg_temp.shop_guest_import_v2_try_bootstrap(
    '00000000-0000-4000-a000-000000000002',
    '88888888-8888-4888-8888-888888888881',
    (select payload from private.shop_guest_bootstrap_receipt
      where user_id = '00000000-0000-4000-a000-000000000002'
        and import_id = '88888888-8888-4888-8888-888888888881')
  ),
  (select result from private.shop_guest_bootstrap_receipt
   where user_id = '00000000-0000-4000-a000-000000000002'
     and import_id = '88888888-8888-4888-8888-888888888881'),
  'an existing private empty-bootstrap success receipt replays before freshness or schema dispatch'
);
select is(
  (pg_temp.shop_guest_import_v2_try_bootstrap(
    '00000000-0000-4000-a000-000000000002',
    '77777777-7777-4777-8777-777777777773',
    pg_temp.shop_guest_import_v2_request(
      '00000000-0000-4000-a000-000000000002',
      '77777777-7777-4777-8777-777777777773'
    )
  ))->>'status',
  'active_account',
  'a legacy private empty success receipt prevents a second bootstrap under a new ID'
);
select ok(
  pg_temp.shop_guest_import_v2_no_game_writes('00000000-0000-4000-a000-000000000002')
    and (select count(*) = 1 from private.shop_guest_bootstrap_receipt
      where user_id = '00000000-0000-4000-a000-000000000002'
        and import_id = '88888888-8888-4888-8888-888888888881'),
  'private empty receipt compatibility does not require or fabricate orphan game rows'
);
select is(
  (pg_temp.shop_guest_import_v2_try_bootstrap(
    '00000000-0000-4000-a000-000000000072',
    '77777777-7777-4777-8777-777777777774',
    pg_temp.shop_guest_import_v2_request(
      '00000000-0000-4000-a000-000000000072',
      '77777777-7777-4777-8777-777777777774'
    )
  ))->>'status',
  'active_account',
  'an orphan imported success receipt blocks a fresh-looking target under another ID'
);
select ok(
  pg_temp.shop_guest_import_v2_no_game_writes('00000000-0000-4000-a000-000000000072')
    and (select count(*) = 2 from private.shop_guest_bootstrap_receipt
      where user_id = '00000000-0000-4000-a000-000000000072'),
  'an orphan receipt is preserved and does not cause partial game writes'
);

select is(
  (pg_temp.shop_guest_import_v2_try_bootstrap(
    '00000000-0000-4000-a000-000000000076',
    '77777777-7777-4777-8777-777777777776',
    pg_temp.shop_guest_import_v2_request_with_profile(
      '00000000-0000-4000-a000-000000000076',
      '77777777-7777-4777-8777-777777777776',
      '{"nickname":"Source Profile","avatar":"feminine"}'::jsonb
    )
  ))#>'{planet_state,profile}',
  '{"nickname":"Source Profile","avatar":"feminine"}'::jsonb,
  'a valid non-null source profile is persisted and returned unchanged'
);
select is(
  (pg_temp.shop_guest_import_v2_try_bootstrap(
    '00000000-0000-4000-a000-000000000077',
    '77777777-7777-4777-8777-777777777777',
    pg_temp.shop_guest_import_v2_request_with_profile(
      '00000000-0000-4000-a000-000000000077',
      '77777777-7777-4777-8777-777777777777',
      jsonb_build_object('nickname', repeat('N',25), 'avatar', 'masculine')
    )
  ))->>'status',
  'source_unverifiable',
  'a malformed profile is held instead of being rescued by the null-profile fallback'
);
select ok(
  not exists (select 1 from public.planet_member_state
    where user_id = '00000000-0000-4000-a000-000000000077')
    and not exists (select 1 from private.shop_device_contribution_state
      where user_id = '00000000-0000-4000-a000-000000000077')
    and exists (select 1 from private.shop_guest_bootstrap_receipt
      where user_id = '00000000-0000-4000-a000-000000000077'
        and status = 'source_unverifiable'),
  'malformed profile creates only a held receipt and no canonical state'
);
create temporary table shop_guest_import_v2_writer_bad_final_proof as
select
  pg_temp.shop_guest_import_v2_request(
    '00000000-0000-4000-a000-000000000078',
    '77777777-7777-4777-8777-777777777778'
  ) as valid_request;
select ok(
  private.shop_guest_import_v2_normalize(valid_request, transaction_timestamp()) is not null
    and private.shop_guest_import_v2_normalize(
      pg_temp.shop_guest_import_v2_bad_final_proof(valid_request),
      transaction_timestamp()
    ) is null,
  'a correctly resealed source still fails when only its final canonical proof is inconsistent'
)
from shop_guest_import_v2_writer_bad_final_proof;
create temporary table shop_guest_import_v2_writer_bad_final_proof_result as
select pg_temp.shop_guest_import_v2_try_bootstrap(
  '00000000-0000-4000-a000-000000000078',
  '77777777-7777-4777-8777-777777777778',
  pg_temp.shop_guest_import_v2_bad_final_proof(valid_request)
) as result
from shop_guest_import_v2_writer_bad_final_proof;
select is(
  (select result->>'status' from shop_guest_import_v2_writer_bad_final_proof_result),
  'source_unverifiable',
  'a late canonical-proof defect is held without importing otherwise valid source data'
);
select ok(
  pg_temp.shop_guest_import_v2_no_game_writes('00000000-0000-4000-a000-000000000078')
    and exists (select 1 from private.shop_guest_bootstrap_receipt
      where user_id = '00000000-0000-4000-a000-000000000078'
        and import_id = '77777777-7777-4777-8777-777777777778'
        and status = 'source_unverifiable')
    and not exists (select 1 from private.shop_guest_bootstrap_receipt
      where user_id = '00000000-0000-4000-a000-000000000078'
        and status = 'imported'),
  'final-proof rejection writes only a held receipt and no game state or success receipt'
);
select ok(
  case when to_regprocedure('private.shop_guest_import_v2_bootstrap(uuid,jsonb)') is null
    then false
    else exists (
      select 1 from pg_proc p
      where p.oid = to_regprocedure('private.shop_guest_import_v2_bootstrap(uuid,jsonb)')
        and p.proowner = 'postgres'::regrole
        and not p.prosecdef
        and coalesce(p.proconfig, array[]::text[]) @> array['search_path=""']
        and not exists (
          select 1 from aclexplode(p.proacl) a
          where a.grantee = 0 and a.privilege_type = 'EXECUTE'
        )
    )
      and not has_function_privilege('anon',
        to_regprocedure('private.shop_guest_import_v2_bootstrap(uuid,jsonb)'), 'EXECUTE')
      and not has_function_privilege('authenticated',
        to_regprocedure('private.shop_guest_import_v2_bootstrap(uuid,jsonb)'), 'EXECUTE')
      and not has_function_privilege('service_role',
        to_regprocedure('private.shop_guest_import_v2_bootstrap(uuid,jsonb)'), 'EXECUTE')
  end,
  'the writer is postgres-owned, invoker-only, empty-search-path, and inaccessible to clients'
);

select set_config('test.shop_guest_import_v2_failure_user', '00000000-0000-4000-a000-000000000073', true);
select set_config('test.shop_guest_import_v2_failure_table', 'growth_journal_days', true);
select throws_ok(
  format(
    'select pg_temp.shop_guest_import_v2_try_bootstrap(%L::uuid,%L::uuid,%L::jsonb)',
    '00000000-0000-4000-a000-000000000073',
    '77777777-7777-4777-8777-777777777773',
    pg_temp.shop_guest_import_v2_request(
      '00000000-0000-4000-a000-000000000073',
      '77777777-7777-4777-8777-777777777773'
    )::text
  ),
  'P0001', 'synthetic writer failure at growth_journal_days',
  'a journal insert failure aborts the full successful bootstrap statement'
);
select ok(
  private.shop_guest_import_target_is_fresh('00000000-0000-4000-a000-000000000073')
    and pg_temp.shop_guest_import_v2_no_game_writes('00000000-0000-4000-a000-000000000073')
    and not exists (select 1 from private.shop_account_lock
      where user_id = '00000000-0000-4000-a000-000000000073')
    and not exists (select 1 from private.shop_guest_bootstrap_receipt
      where user_id = '00000000-0000-4000-a000-000000000073'),
  'journal failure rolls back game rows, success receipt, and newly created lock'
);
select set_config('test.shop_guest_import_v2_failure_user', '00000000-0000-4000-a000-000000000074', true);
select set_config('test.shop_guest_import_v2_failure_table', 'shop_cycle_token_settlement', true);
select throws_ok(
  format(
    'select pg_temp.shop_guest_import_v2_try_bootstrap(%L::uuid,%L::uuid,%L::jsonb)',
    '00000000-0000-4000-a000-000000000074',
    '77777777-7777-4777-8777-777777777774',
    pg_temp.shop_guest_import_v2_request(
      '00000000-0000-4000-a000-000000000074',
      '77777777-7777-4777-8777-777777777774'
    )::text
  ),
  'P0001', 'synthetic writer failure at shop_cycle_token_settlement',
  'a settlement insert failure aborts the full successful bootstrap statement'
);
select ok(
  private.shop_guest_import_target_is_fresh('00000000-0000-4000-a000-000000000074')
    and pg_temp.shop_guest_import_v2_no_game_writes('00000000-0000-4000-a000-000000000074')
    and not exists (select 1 from private.shop_account_lock
      where user_id = '00000000-0000-4000-a000-000000000074')
    and not exists (select 1 from private.shop_guest_bootstrap_receipt
      where user_id = '00000000-0000-4000-a000-000000000074'),
  'settlement failure rolls back game rows, success receipt, and newly created lock'
);
select set_config('test.shop_guest_import_v2_failure_user', '00000000-0000-4000-a000-000000000075', true);
select set_config('test.shop_guest_import_v2_failure_table', 'shop_guest_bootstrap_receipt', true);
select throws_ok(
  format(
    'select pg_temp.shop_guest_import_v2_try_bootstrap(%L::uuid,%L::uuid,%L::jsonb)',
    '00000000-0000-4000-a000-000000000075',
    '77777777-7777-4777-8777-777777777775',
    pg_temp.shop_guest_import_v2_request(
      '00000000-0000-4000-a000-000000000075',
      '77777777-7777-4777-8777-777777777775'
    )::text
  ),
  'P0001', 'synthetic writer failure at shop_guest_bootstrap_receipt',
  'a final receipt insert failure aborts all canonical game writes'
);
select ok(
  private.shop_guest_import_target_is_fresh('00000000-0000-4000-a000-000000000075')
    and pg_temp.shop_guest_import_v2_no_game_writes('00000000-0000-4000-a000-000000000075')
    and not exists (select 1 from private.shop_account_lock
      where user_id = '00000000-0000-4000-a000-000000000075')
    and not exists (select 1 from private.shop_guest_bootstrap_receipt
      where user_id = '00000000-0000-4000-a000-000000000075'),
  'final receipt failure rolls back game rows, success receipt, and newly created lock'
);

drop trigger shop_guest_import_v2_writer_test_fail_journal on private.growth_journal_days;
drop trigger shop_guest_import_v2_writer_test_fail_settlement on private.shop_cycle_token_settlement;
drop trigger shop_guest_import_v2_writer_test_fail_receipt on private.shop_guest_bootstrap_receipt;
drop function private.shop_guest_import_v2_writer_test_fail();
select * from finish();
rollback;
\echo SHOP_GUEST_IMPORT_V2_ROLLBACK_COMPLETED
