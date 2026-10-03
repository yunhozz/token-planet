-- Public schema dispatch preserves the legacy held contract and exposes only
-- the typed, authenticated schema-2 bootstrap response.
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
    v_request, '{snapshot,target_account_id}',
    to_jsonb('account:' || p_user_id::text), true
  );
  return pg_temp.shop_guest_import_v2_reseal(v_request);
end;
$$;

create function pg_temp.shop_guest_import_v2_call_public(p_import_id uuid, p_request jsonb)
returns jsonb
language plpgsql
as $$
declare
  v_result jsonb;
  v_code text;
  v_message text;
begin
  v_result := public.import_guest_shop(p_import_id, p_request);
  return v_result;
exception when others then
  get stacked diagnostics v_code = returned_sqlstate, v_message = message_text;
  return jsonb_build_object('error_code', v_code, 'error_message', v_message);
end;
$$;

insert into auth.users(id) values
  ('00000000-0000-4000-a000-000000000001'),
  ('00000000-0000-4000-a000-000000000002'),
  ('00000000-0000-4000-a000-000000000071'),
  ('00000000-0000-4000-a000-000000000072'),
  ('00000000-0000-4000-a000-000000000073'),
  ('00000000-0000-4000-a000-000000000074');

select no_plan();

-- Authentication and target checks precede lock or receipt writes.
select set_config('request.jwt.claim.sub', '', true);
select throws_ok(
  format(
    'select public.import_guest_shop(%L::uuid,%L::jsonb)',
    '4793fe7a-ddea-406d-ae19-da0560f3936f',
    pg_temp.shop_guest_import_v2_first_reset_request()::text
  ),
  '42501', 'authentication required',
  'anonymous callers are rejected before receipt lookup'
);
select set_config('request.jwt.claim.sub', '00000000-0000-4000-a000-000000000072', true);
select throws_ok(
  format(
    'select public.import_guest_shop(%L::uuid,%L::jsonb)',
    '4793fe7a-ddea-406d-ae19-da0560f3936f',
    pg_temp.shop_guest_import_v2_first_reset_request()::text
  ),
  '42501', 'guest shop import target does not match authenticated account',
  'a different account cannot dispatch the native schema-2 target'
);

-- Legacy schema 1 remains held with its original result shape and receipt table.
select set_config('request.jwt.claim.sub', '00000000-0000-4000-a000-000000000001', true);
create temporary table shop_guest_import_v2_public_legacy as
select pg_temp.shop_guest_import_native_first_reset_request() as request;
create temporary table shop_guest_import_v2_public_legacy_result as
select public.import_guest_shop(
  '7da19666-68ec-4a45-b1e4-a5f195e8d584', request
) as result from shop_guest_import_v2_public_legacy;
select is(
  (select result->>'status' from shop_guest_import_v2_public_legacy_result),
  'source_unverifiable',
  'schema-1 ordinary-reset proof remains held by the existing public contract'
);
select ok(
  (select result->>'import_id' = '7da19666-68ec-4a45-b1e4-a5f195e8d584'
      and not (result ? 'schema_version') and not (result ? 'ack')
   from shop_guest_import_v2_public_legacy_result)
    and exists (select 1 from private.shop_guest_import_request r
      where r.user_id = '00000000-0000-4000-a000-000000000001'
        and r.import_id = '7da19666-68ec-4a45-b1e4-a5f195e8d584'
        and r.payload = (select request from shop_guest_import_v2_public_legacy)
        and r.status = 'source_unverifiable'),
  'schema 1 keeps the original minimal response and immutable public held receipt'
);
select is(
  public.import_guest_shop(
    '7da19666-68ec-4a45-b1e4-a5f195e8d584',
    (select request from shop_guest_import_v2_public_legacy)
  ),
  (select result from shop_guest_import_v2_public_legacy_result),
  'an identical schema-1 request replays the original public held result'
);
create temporary table shop_guest_import_v2_public_legacy_conflict as
select pg_temp.shop_guest_import_v2_call_public(
  '7da19666-68ec-4a45-b1e4-a5f195e8d584',
  jsonb_set(
    (select request from shop_guest_import_v2_public_legacy),
    '{snapshot,disposition}', '"local_integrity_validated"'::jsonb, true
  )
) as result;
select is(
  (select result->>'status' from shop_guest_import_v2_public_legacy_conflict),
  'request_conflict',
  'a changed schema-1 payload conflicts before revalidation and never promotes a held receipt'
);
select ok(
  (select payload = request from private.shop_guest_import_request r
    cross join shop_guest_import_v2_public_legacy
    where r.user_id = '00000000-0000-4000-a000-000000000001'
      and r.import_id = '7da19666-68ec-4a45-b1e4-a5f195e8d584')
    and private.shop_guest_import_target_is_fresh('00000000-0000-4000-a000-000000000001'),
  'legacy held conflict preserves the receipt and leaves game state fresh'
);
select is(
  private.shop_guest_bootstrap(
    '7da19666-68ec-4a45-b1e4-a5f195e8d584',
    (select request from shop_guest_import_v2_public_legacy)
  ),
  (select result from shop_guest_import_v2_public_legacy_result),
  'the private schema-1 bootstrap compatibility route replays the public held receipt'
);

-- Schema 2 is expected RED until the public dispatcher calls the private writer.
select set_config('request.jwt.claim.sub', '00000000-0000-4000-a000-000000000071', true);
create temporary table shop_guest_import_v2_public_request as
select pg_temp.shop_guest_import_v2_request(
  '00000000-0000-4000-a000-000000000071',
  '4793fe7a-ddea-406d-ae19-da0560f3936f'
) as request;
create temporary table shop_guest_import_v2_public_result as
select pg_temp.shop_guest_import_v2_call_public(
  '4793fe7a-ddea-406d-ae19-da0560f3936f', request
) as result from shop_guest_import_v2_public_request;

select is(
  (select result->>'status' from shop_guest_import_v2_public_result),
  'imported',
  'the public endpoint dispatches a valid native schema-2 first-reset import'
);
select ok(
  (select result->>'schema_version' = '2'
      and result->>'import_id' = '4793fe7a-ddea-406d-ae19-da0560f3936f'
      and result->>'account_id' = 'account:00000000-0000-4000-a000-000000000071'
      and result->>'source_fingerprint' = request#>>'{snapshot,source_fingerprint}'
      and jsonb_typeof(result->'shop_state') = 'object'
      and jsonb_typeof(result->'planet_state') = 'object'
      and jsonb_typeof(result->'effect_timeline') = 'object'
      and jsonb_typeof(result->'canonical_contribution') = 'object'
      and jsonb_typeof(result->'journal_confirmation') = 'object'
      and jsonb_typeof(result->'ack') = 'object'
   from shop_guest_import_v2_public_result cross join shop_guest_import_v2_public_request),
  'the public result carries exact correlation and every typed imported projection'
);
with replay as (
  select pg_temp.shop_guest_import_v2_call_public(
    '4793fe7a-ddea-406d-ae19-da0560f3936f',
    (select request from shop_guest_import_v2_public_request)
  ) as result
)
select ok(
  (select replay.result = stored.result and replay.result->>'status' = 'imported'
   from replay cross join shop_guest_import_v2_public_result stored),
  'same ID and identical schema-2 payload replay the full immutable receipt'
);
select is(
  (pg_temp.shop_guest_import_v2_call_public(
    '4793fe7a-ddea-406d-ae19-da0560f3936f',
    jsonb_set(
      (select request from shop_guest_import_v2_public_request),
      '{snapshot,source_fingerprint}', to_jsonb(repeat('0',64)), true
    )
  ))->>'status',
  'request_conflict',
  'same ID with changed whole payload conflicts before source fingerprint validation'
);
select ok(
  exists (select 1 from private.shop_guest_bootstrap_receipt r
    where r.user_id = '00000000-0000-4000-a000-000000000071'
      and r.import_id = '4793fe7a-ddea-406d-ae19-da0560f3936f'
      and r.status = 'imported'
      and r.payload = (select request from shop_guest_import_v2_public_request)
      and r.result = (select result from shop_guest_import_v2_public_result)
      and r.normalized_proof = private.shop_guest_import_v2_normalize(
        (select request from shop_guest_import_v2_public_request), transaction_timestamp()
      )),
  'the private success receipt stores the original payload, normalized proof, and exact result'
);

create temporary table shop_guest_import_v2_public_new_id as
select pg_temp.shop_guest_import_v2_request(
  '00000000-0000-4000-a000-000000000071',
  '77777777-7777-4777-8777-777777777771'
) as request;
create temporary table shop_guest_import_v2_public_active as
select pg_temp.shop_guest_import_v2_call_public(
  '77777777-7777-4777-8777-777777777771', request
) as result from shop_guest_import_v2_public_new_id;
select is(
  (select result->>'status' from shop_guest_import_v2_public_active),
  'active_account',
  'a new ID after import is held as active without replacing canonical state'
);
select ok(
  (select not (result ?| array[
          'shop_state','planet_state','effect_timeline','canonical_contribution',
          'journal_confirmation','ack'
        ])
   from shop_guest_import_v2_public_active)
    and (select count(*) = 1 from public.planet_member_state
      where user_id = '00000000-0000-4000-a000-000000000071'
        and lifetime_tokens = 1000000)
    and (select count(*) = 2 from private.shop_guest_bootstrap_receipt
      where user_id = '00000000-0000-4000-a000-000000000071'),
  'active-account dispatch omits success projections and preserves the first imported state'
);

-- The unchanged journal RPC surface must still serialize journal writes
-- against imports and preserve its read/upsert/delete behavior.
select set_config('request.jwt.claim.sub', '00000000-0000-4000-a000-000000000073', true);
create temporary table shop_guest_import_v2_journal_first_read as
select public.get_my_growth_journal() as result;
select ok(
  (select result->>'generation' = '0'
      and result->'deleted_at_utc' = 'null'::jsonb
      and result->'timezone' = 'null'::jsonb
      and result->'cycles' = '[]'::jsonb
      and result->'entries' = '[]'::jsonb
   from shop_guest_import_v2_journal_first_read)
    and not exists (select 1 from private.growth_journal_state
      where user_id = '00000000-0000-4000-a000-000000000073'),
  'an initial journal read on an empty account keeps the original empty DTO without creating state'
);
create temporary table shop_guest_import_v2_journal_seed as
select public.upsert_my_growth_journal(0, 'UTC', '[]'::jsonb, '[]'::jsonb) as result;
select ok(
  (select result->>'generation' = '0'
      and result->>'timezone' = 'UTC'
      and result->'cycles' = '[]'::jsonb
      and result->'entries' = '[]'::jsonb
   from shop_guest_import_v2_journal_seed)
    and exists (select 1 from private.growth_journal_state
      where user_id = '00000000-0000-4000-a000-000000000073'
        and generation = 0 and deleted_at is null)
    and exists (select 1 from private.shop_account_lock
      where user_id = '00000000-0000-4000-a000-000000000073')
    and not exists (select 1 from public.planet_member_state
      where user_id = '00000000-0000-4000-a000-000000000073'),
  'an initial journal write keeps the existing empty DTO and creates only the journal footprint'
);
create temporary table shop_guest_import_v2_journal_then_import as
select pg_temp.shop_guest_import_v2_request(
  '00000000-0000-4000-a000-000000000073',
  '77777777-7777-4777-8777-777777777772'
) as request;
create temporary table shop_guest_import_v2_journal_then_import_result as
select pg_temp.shop_guest_import_v2_call_public(
  '77777777-7777-4777-8777-777777777772', request
) as result from shop_guest_import_v2_journal_then_import;
select is(
  (select result->>'status' from shop_guest_import_v2_journal_then_import_result),
  'active_account',
  'the journal-only first write makes a later native bootstrap active-account held'
);
select ok(
  (select not (result ?| array[
          'shop_state','planet_state','effect_timeline','canonical_contribution',
          'journal_confirmation','ack'
        ])
   from shop_guest_import_v2_journal_then_import_result)
    and (select count(*) = 1 from private.shop_guest_bootstrap_receipt
      where user_id = '00000000-0000-4000-a000-000000000073'
        and import_id = '77777777-7777-4777-8777-777777777772'
        and status = 'active_account')
    and not exists (select 1 from public.planet_member_state
      where user_id = '00000000-0000-4000-a000-000000000073'),
  'journal-first bootstrap persists only its active receipt and never imports game state'
);

select set_config('request.jwt.claim.sub', '00000000-0000-4000-a000-000000000074', true);
create temporary table shop_guest_import_v2_journal_upsert as
select public.upsert_my_growth_journal(
  0,
  'UTC',
  jsonb_build_array(jsonb_build_object(
    'cycle_id','journal-cycle',
    'started_at_utc','2026-10-03T00:00:00Z',
    'ended_at_utc',null,
    'wallet_credit',null,
    'wallet_credit_at_utc',null
  )),
  jsonb_build_array(jsonb_build_object(
    'device_id','00000000-0000-4000-a000-000000000074',
    'cycle_id','journal-cycle',
    'bucket_date','2026-10-03',
    'agent','codex',
    'revision',1,
    'generation',0,
    'present',true,
    'confirmed_tokens',7,
    'coverage','complete',
    'payload_hash',repeat('a',64)
  ))
) as result;
select ok(
  (select result->>'generation' = '0'
      and result->>'timezone' = 'UTC'
      and jsonb_array_length(result->'cycles') = 1
      and jsonb_array_length(result->'entries') = 1
      and result#>>'{entries,0,confirmed_tokens}' = '7'
      and result#>>'{entries,0,coverage}' = 'complete'
   from shop_guest_import_v2_journal_upsert),
  'journal upsert keeps the existing generation, timezone, cycle, and entry DTO'
);
select throws_ok(
  format(
    'select public.upsert_my_growth_journal(0,%L,%L::jsonb,%L::jsonb)',
    'Pacific/Auckland','[]','[]'
  ),
  '23514', 'growth journal timezone differs from the account setting',
  'an invalid journal update fails without committing partial changes'
);
select is(
  public.get_my_growth_journal(),
  (select result from shop_guest_import_v2_journal_upsert),
  'a rejected journal update rolls back and leaves the previous read DTO intact'
);
create temporary table shop_guest_import_v2_journal_delete as
select public.delete_my_growth_journal() as result;
select ok(
  (select result->>'generation' = '1'
      and result->'deleted_at_utc' <> 'null'::jsonb
      and result->'cycles' = '[]'::jsonb
      and result->'entries' = '[]'::jsonb
   from shop_guest_import_v2_journal_delete),
  'journal delete keeps its tombstone generation and clears cycle and entry DTOs'
);
select is(
  public.get_my_growth_journal(),
  (select result from shop_guest_import_v2_journal_delete),
  'a journal read after deletion returns the unchanged tombstone DTO'
);

select ok(
  case when to_regprocedure('public.import_guest_shop(uuid,jsonb)') is null then false
    else exists (
      select 1 from pg_proc p
      where p.oid = to_regprocedure('public.import_guest_shop(uuid,jsonb)')
        and p.proowner = 'postgres'::regrole
        and p.prosecdef
        and coalesce(p.proconfig, array[]::text[]) @> array['search_path=""']
    )
      and has_function_privilege('authenticated',
        to_regprocedure('public.import_guest_shop(uuid,jsonb)'), 'EXECUTE')
      and not has_function_privilege('anon',
        to_regprocedure('public.import_guest_shop(uuid,jsonb)'), 'EXECUTE')
      and has_function_privilege('service_role',
        to_regprocedure('public.import_guest_shop(uuid,jsonb)'), 'EXECUTE')
      and not exists (
        select 1 from pg_proc p
        cross join lateral aclexplode(p.proacl) a
        where p.oid = to_regprocedure('public.import_guest_shop(uuid,jsonb)')
          and a.grantee = 0 and a.privilege_type = 'EXECUTE'
      )
      and not has_table_privilege('authenticated',
        'private.shop_guest_bootstrap_receipt', 'SELECT')
      and not has_table_privilege('authenticated',
        'private.shop_guest_import_request', 'SELECT')
  end,
  'public dispatch is narrowly authenticated and private receipt tables remain inaccessible'
);
select ok(
  case when to_regprocedure('private.shop_guest_import_v2_bootstrap(uuid,jsonb)') is null then false
    else exists (
      select 1 from pg_proc p
      where p.oid = to_regprocedure('private.shop_guest_import_v2_bootstrap(uuid,jsonb)')
        and p.proowner = 'postgres'::regrole
        and not p.prosecdef
        and coalesce(p.proconfig, array[]::text[]) @> array['search_path=""']
    )
      and not has_function_privilege('authenticated',
        to_regprocedure('private.shop_guest_import_v2_bootstrap(uuid,jsonb)'), 'EXECUTE')
      and not has_function_privilege('anon',
        to_regprocedure('private.shop_guest_import_v2_bootstrap(uuid,jsonb)'), 'EXECUTE')
      and not has_function_privilege('service_role',
        to_regprocedure('private.shop_guest_import_v2_bootstrap(uuid,jsonb)'), 'EXECUTE')
  end,
  'the v2 private writer stays invoker-only and inaccessible to clients'
);
with signatures(signature, service_role_execute) as (
  values
    ('private.get_my_growth_journal()', false),
    ('private.upsert_my_growth_journal(bigint,text,jsonb,jsonb)', false),
    ('private.delete_my_growth_journal()', false),
    ('public.get_my_growth_journal()', true),
    ('public.upsert_my_growth_journal(bigint,text,jsonb,jsonb)', true),
    ('public.delete_my_growth_journal()', true)
), resolved as (
  select to_regprocedure(signature) as oid, service_role_execute
  from signatures
)
select ok(
  (select count(*) = 6
      and bool_and(has_function_privilege('authenticated', oid, 'EXECUTE'))
      and bool_and(not has_function_privilege('anon', oid, 'EXECUTE'))
      and bool_and(has_function_privilege('service_role', oid, 'EXECUTE') = service_role_execute)
      and bool_and(not exists (
        select 1 from pg_proc p
        cross join lateral aclexplode(coalesce(p.proacl, acldefault('f', p.proowner))) a
        where p.oid = resolved.oid and a.grantee = 0 and a.privilege_type = 'EXECUTE'
      ))
   from resolved where oid is not null),
  'the existing journal RPC ACLs remain authenticated-only'
);

select * from finish();
rollback;
\echo SHOP_GUEST_IMPORT_V2_ROLLBACK_COMPLETED
