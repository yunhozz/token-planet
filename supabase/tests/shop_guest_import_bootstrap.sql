begin;
create extension if not exists pgtap with schema extensions;
select no_plan();
\ir fixtures/shop_guest_import_native_empty.inc

insert into auth.users(id) values ('00000000-0000-4000-a000-000000000001');
select set_config('request.jwt.claim.sub', '00000000-0000-4000-a000-000000000001', true);

create function pg_temp.try_shop_guest_bootstrap(p_import_id uuid, p_request jsonb)
returns jsonb
language plpgsql
as $$
declare
  v_result jsonb;
begin
  execute 'select private.shop_guest_bootstrap($1, $2)'
    into v_result using p_import_id, p_request;
  return v_result;
exception when undefined_function then
  return null;
end;
$$;

create function pg_temp.shop_guest_bootstrap_test_request(p_user_id uuid, p_import_id uuid)
returns jsonb
language sql
immutable
as $$
  select jsonb_set(
    jsonb_set(
      pg_temp.shop_guest_import_native_empty_request(),
      '{snapshot,import_id}', to_jsonb(p_import_id::text), true
    ),
    '{snapshot,target_account_id}', to_jsonb('account:' || p_user_id::text), true
  );
$$;

create temp table pg_temp.shop_guest_bootstrap_test_accounts (
  case_name text primary key,
  user_id uuid not null default gen_random_uuid(),
  import_id uuid not null default gen_random_uuid(),
  request jsonb
);
create temp table pg_temp.shop_guest_bootstrap_test_results (
  case_name text primary key,
  user_id uuid not null,
  request jsonb not null,
  value jsonb not null
);
insert into pg_temp.shop_guest_bootstrap_test_accounts(case_name) values
  ('public_held'), ('invalid_source'), ('active_account'), ('journal_failure'),
  ('receipt_failure'), ('imported_receipt'), ('unverifiable_receipt');
update pg_temp.shop_guest_bootstrap_test_accounts a
set request = pg_temp.shop_guest_bootstrap_test_request(a.user_id, a.import_id);
insert into auth.users(id)
select a.user_id from pg_temp.shop_guest_bootstrap_test_accounts a;

with source as (
  select pg_temp.shop_guest_import_native_empty_request() as request
)
select ok(
  private.shop_guest_import_envelope_valid(
    (request#>>'{snapshot,import_id}')::uuid, request
  ),
  'the unmodified Rust native-empty envelope passes the existing envelope validator'
)
from source;

select ok(
  private.shop_guest_import_target_is_fresh('00000000-0000-4000-a000-000000000001'),
  'the synthetic target is fresh before private bootstrap'
);

select is((select count(*)::integer from public.planet_member_state
  where user_id = '00000000-0000-4000-a000-000000000001'), 0,
  'the target starts without a planet profile');
select is((select count(*)::integer from private.shop_account_state
  where user_id = '00000000-0000-4000-a000-000000000001'), 0,
  'the target starts without shop state');
select is((select count(*)::integer from private.growth_journal_state
  where user_id = '00000000-0000-4000-a000-000000000001'), 0,
  'the target starts without journal state');
select is((select count(*)::integer from private.growth_journal_cycles
  where user_id = '00000000-0000-4000-a000-000000000001'), 0,
  'the target starts without journal cycle metadata');

with source as (
  select pg_temp.shop_guest_import_native_empty_request() as request
), result as (
  select pg_temp.try_shop_guest_bootstrap(
    (request#>>'{snapshot,import_id}')::uuid, request
  ) as value, request
  from source
)
select ok(
  value->>'status' = 'imported'
    and value#>>'{result,account_id}' = 'account:00000000-0000-4000-a000-000000000001'
    and value#>>'{result,current_cycle_id}' = request#>>'{snapshot,data,current_cycle,cycle_id}'
    and value#>>'{result,state_revision}' = '0'
    and value#>'{result,available_balance}' = '0'::jsonb
    and value#>'{result,landscape_instances}' = '[]'::jsonb
    and value#>'{result,placements}' = '[]'::jsonb
    and value#>'{result,avatar_owned_skus}' = '[]'::jsonb
    and value#>'{result,avatar_equipment}' = jsonb_build_object(
      'head', '{"sku":null,"version":0}'::jsonb,
      'outfit', '{"sku":null,"version":0}'::jsonb,
      'face', '{"sku":null,"version":0}'::jsonb,
      'back', '{"sku":null,"version":0}'::jsonb
    )
    and value->'source_metadata' = private.shop_guest_import_bootstrap_source_normalize(
      request#>'{snapshot,data}', transaction_timestamp()
    )->'source_metadata',
  'the private native-empty bootstrap returns its canonical imported result'
)
from result;

select ok(exists (
  select 1 from public.planet_member_state p
  where p.user_id = '00000000-0000-4000-a000-000000000001'
    and p.state_version = 1
    and p.nickname = 'Synthetic Native'
    and p.avatar = 'masculine'
    and p.timezone = 'UTC'
    and p.current_planet_tokens = 0
    and p.lifetime_tokens = 0
    and p.growth_credit = 0
    and p.stage = 0
    and p.progress_to_next = 0
    and not p.incomplete
    and p.objects = '[]'::jsonb
    and not p.shared_visible
    and p.last_reset_at is null
    and p.reset_available_at is null
    and p.reset_cooldown_bps = 0
    and p.updated_at = transaction_timestamp()
), 'one hidden planet member row is initialized from the native profile with zero state');

select ok(exists (
  select 1 from private.shop_account_state s
  where s.user_id = '00000000-0000-4000-a000-000000000001'
    and s.state_revision = 0
    and s.reward_timezone = 'UTC'
    and s.reward_timezone_initialized
    and s.updated_at = transaction_timestamp()
), 'one shop account row stores the source reward timezone at the transaction time');

select ok(exists (
  select 1 from private.growth_journal_state s
  where s.user_id = '00000000-0000-4000-a000-000000000001'
    and s.generation = 0 and s.deleted_at is null and s.timezone = 'UTC'
), 'one live generation-zero journal state is created');

select ok(
  (select count(*) = 1 from private.growth_journal_cycles c
   where c.user_id = '00000000-0000-4000-a000-000000000001')
  and exists (
    select 1 from private.growth_journal_cycles c
    where c.user_id = '00000000-0000-4000-a000-000000000001'
      and c.cycle_id = '253be5a5-ffa4-48ea-9a90-f3b13840404b'
      and c.started_at = '2026-10-02T14:54:13.376221Z'::timestamptz
      and c.ended_at is null and c.wallet_credit is null and c.wallet_credit_at is null
  ), 'one empty current journal cycle is created from the captured cycle metadata'
);

select ok(exists (
  select 1 from private.shop_guest_bootstrap_receipt r
  where r.user_id = '00000000-0000-4000-a000-000000000001'
    and r.import_id = '59cc031d-948c-4c6e-bc77-8de58b519f99'
    and r.status = 'imported'
    and r.payload = pg_temp.shop_guest_import_native_empty_request()
    and r.source_fingerprint = '5e611b03b1f9425c4a4758db57f2c664ae87afc06887b4b4c27763e2d29bb110'
    and r.result->>'status' = 'imported'
    and r.result->'source_metadata' is not null
), 'the immutable private receipt stores the exact payload, correlation fingerprint, result, and normalized metadata');

select ok(not exists (
  select 1 from private.planet_device_state d where d.user_id = '00000000-0000-4000-a000-000000000001'
  union all select 1 from private.planet_wallet_credits w where w.user_id = '00000000-0000-4000-a000-000000000001'
  union all select 1 from private.growth_journal_days g where g.user_id = '00000000-0000-4000-a000-000000000001'
  union all select 1 from private.shop_purchase s where s.user_id = '00000000-0000-4000-a000-000000000001'
  union all select 1 from private.shop_landscape_instance s where s.user_id = '00000000-0000-4000-a000-000000000001'
  union all select 1 from private.shop_landscape_placement s where s.user_id = '00000000-0000-4000-a000-000000000001'
  union all select 1 from private.shop_avatar_owned s where s.user_id = '00000000-0000-4000-a000-000000000001'
  union all select 1 from private.shop_avatar_equipment s where s.user_id = '00000000-0000-4000-a000-000000000001'
  union all select 1 from private.shop_action_request s where s.user_id = '00000000-0000-4000-a000-000000000001'
  union all select 1 from private.shop_effect_history s where s.user_id = '00000000-0000-4000-a000-000000000001'
  union all select 1 from private.shop_natural_removal s where s.user_id = '00000000-0000-4000-a000-000000000001'
  union all select 1 from private.shop_game_reward s where s.user_id = '00000000-0000-4000-a000-000000000001'
  union all select 1 from private.shop_reset_request s where s.user_id = '00000000-0000-4000-a000-000000000001'
  union all select 1 from private.shop_cycle_token_settlement s where s.user_id = '00000000-0000-4000-a000-000000000001'
  union all select 1 from private.shop_device_contribution_state s where s.user_id = '00000000-0000-4000-a000-000000000001'
  union all select 1 from private.shop_effect_contribution s where s.user_id = '00000000-0000-4000-a000-000000000001'
  union all select 1 from private.shop_device_activity_day s where s.user_id = '00000000-0000-4000-a000-000000000001'
  union all select 1 from private.shop_activity_day s where s.user_id = '00000000-0000-4000-a000-000000000001'
  union all select 1 from private.shop_cycle_effect_baseline s where s.user_id = '00000000-0000-4000-a000-000000000001'
  union all select 1 from private.shop_planet_object_generation_baseline s where s.user_id = '00000000-0000-4000-a000-000000000001'
  union all select 1 from private.cosmetic_purchase s where s.user_id = '00000000-0000-4000-a000-000000000001'
  union all select 1 from private.cosmetic_purchase_request s where s.user_id = '00000000-0000-4000-a000-000000000001'
  union all select 1 from private.cosmetic_equipment s where s.user_id = '00000000-0000-4000-a000-000000000001'
), 'bootstrap creates only the four planned game rows and separate import metadata');

with source as (
  select pg_temp.shop_guest_import_native_empty_request() as request
), replay as (
  select pg_temp.try_shop_guest_bootstrap(
    (request#>>'{snapshot,import_id}')::uuid, request
  ) as value
  from source
), stored as (
  select r.result from private.shop_guest_bootstrap_receipt r
  where r.user_id = '00000000-0000-4000-a000-000000000001'
    and r.import_id = '59cc031d-948c-4c6e-bc77-8de58b519f99'
)
select is((select value from replay), (select result from stored),
  'same import ID and exact payload replay the immutable private result');

with source as (
  select jsonb_set(
    pg_temp.shop_guest_import_native_empty_request(),
    '{snapshot,data,profile,nickname}', '"Different Name"'::jsonb, true
  ) as request
), conflict as (
  select pg_temp.try_shop_guest_bootstrap(
    (request#>>'{snapshot,import_id}')::uuid, request
  ) as value
  from source
)
select is((select value->>'status' from conflict), 'request_conflict',
  'same private import ID with a changed JSONB payload conflicts');
select is((select count(*)::integer from private.shop_guest_bootstrap_receipt
  where user_id = '00000000-0000-4000-a000-000000000001'
    and import_id = '59cc031d-948c-4c6e-bc77-8de58b519f99'), 1,
  'replay and conflict leave one immutable imported receipt');

select set_config('request.jwt.claim.sub', '00000000-0000-4000-a000-000000000001', true);
create temporary table shop_guest_bootstrap_public_result as
with source as (
  select pg_temp.shop_guest_import_native_empty_request() as request
)
select public.import_guest_shop(
  (request#>>'{snapshot,import_id}')::uuid, request
) as value
from source;
select 'SCHEMA1_PUBLIC_RPC_DIAGNOSTIC=' || jsonb_build_object(
  'status', value->>'status',
  'import_id', value->>'import_id',
  'has_result', value ? 'result',
  'has_source_metadata', value ? 'source_metadata'
)::text
from shop_guest_bootstrap_public_result;
select ok(
  (select value->>'status' = 'active_account'
      and not (value ? 'result')
      and not (value ? 'source_metadata')
   from shop_guest_bootstrap_public_result),
  'the public held-only RPC does not expose a private imported result'
);

select ok(
  not has_table_privilege('anon', 'private.shop_guest_bootstrap_receipt', 'SELECT')
    and not has_table_privilege('authenticated', 'private.shop_guest_bootstrap_receipt', 'SELECT')
    and not has_table_privilege('service_role', 'private.shop_guest_bootstrap_receipt', 'SELECT')
    and not has_function_privilege('anon', 'private.shop_guest_bootstrap(uuid,jsonb)', 'EXECUTE')
    and not has_function_privilege('authenticated', 'private.shop_guest_bootstrap(uuid,jsonb)', 'EXECUTE')
    and not has_function_privilege('service_role', 'private.shop_guest_bootstrap(uuid,jsonb)', 'EXECUTE')
    and (select c.relrowsecurity from pg_catalog.pg_class c
         where c.oid = 'private.shop_guest_bootstrap_receipt'::regclass),
  'private receipt and writer have RLS with no client table or function access'
);

select set_config('request.jwt.claim.sub', (
  select user_id::text from pg_temp.shop_guest_bootstrap_test_accounts
  where case_name = 'public_held'
), true);
with source as (
  select request from pg_temp.shop_guest_bootstrap_test_accounts
  where case_name = 'public_held'
)
select is((public.import_guest_shop(
  (request#>>'{snapshot,import_id}')::uuid, request
)->>'status'), 'source_unverifiable',
  'a public held receipt can predate the private writer')
from source;

with source as (
  select request from pg_temp.shop_guest_bootstrap_test_accounts
  where case_name = 'public_held'
), private_result as (
  select pg_temp.try_shop_guest_bootstrap(
    (request#>>'{snapshot,import_id}')::uuid, request
  ) as value
  from source
), held as (
  select r.result from private.shop_guest_import_request r
  join pg_temp.shop_guest_bootstrap_test_accounts a on a.user_id = r.user_id
  where a.case_name = 'public_held'
)
select is((select value from private_result), (select result from held),
  'the private writer preserves an existing same-payload public held result');

with source as (
  select jsonb_set(
    request, '{snapshot,data,profile,nickname}', '"Different Source"'::jsonb, true
  ) as request, user_id, import_id
  from pg_temp.shop_guest_bootstrap_test_accounts
  where case_name = 'public_held'
), private_result as (
  select pg_temp.try_shop_guest_bootstrap(import_id, request) as value
  from source
)
select is((select value->>'status' from private_result), 'request_conflict',
  'the private writer conflicts rather than promoting a different public-held payload');

select ok(
  (select count(*) = 1
   from private.shop_guest_import_request r
   join pg_temp.shop_guest_bootstrap_test_accounts a on a.user_id = r.user_id
   where a.case_name = 'public_held' and r.status = 'source_unverifiable')
  and not exists (
    select 1 from private.shop_guest_bootstrap_receipt r
    join pg_temp.shop_guest_bootstrap_test_accounts a on a.user_id = r.user_id
    where a.case_name = 'public_held'
  )
  and private.shop_guest_import_target_is_fresh((
    select user_id from pg_temp.shop_guest_bootstrap_test_accounts
    where case_name = 'public_held'
  )),
  'public held replay creates no private receipt and does not make a fresh account active'
);

insert into private.shop_guest_bootstrap_receipt(
  user_id, import_id, payload, source_fingerprint, status, result
)
select user_id, import_id, request, request#>>'{snapshot,source_fingerprint}',
       'imported', '{"status":"imported"}'::jsonb
from pg_temp.shop_guest_bootstrap_test_accounts
where case_name = 'imported_receipt';

select ok(
  not private.shop_guest_import_target_is_fresh((
    select user_id from pg_temp.shop_guest_bootstrap_test_accounts
    where case_name = 'imported_receipt'
  ))
  and not exists (
    select 1 from public.planet_member_state p
    join pg_temp.shop_guest_bootstrap_test_accounts a on a.user_id = p.user_id
    where a.case_name = 'imported_receipt'
  ),
  'an imported private receipt alone is included in account freshness'
);

insert into public.planet_member_state(
  user_id, nickname, avatar, timezone, current_cycle_id, cycle_started_at,
  current_planet_tokens, lifetime_tokens, growth_credit, stage, progress_to_next,
  incomplete, objects, shared_visible
)
select user_id, 'preexisting-profile', 'feminine', 'UTC', 'existing-cycle',
       '2026-09-01T00:00:00Z'::timestamptz, 0, 0, 0, 0, 0, false, '[]'::jsonb, true
from pg_temp.shop_guest_bootstrap_test_accounts
where case_name = 'active_account';
select set_config('request.jwt.claim.sub', (
  select user_id::text from pg_temp.shop_guest_bootstrap_test_accounts
  where case_name = 'active_account'
), true);
with source as (
  select user_id, request from pg_temp.shop_guest_bootstrap_test_accounts
  where case_name = 'active_account'
), result as (
  select pg_temp.try_shop_guest_bootstrap(
    (request#>>'{snapshot,import_id}')::uuid, request
  ) as value, user_id
  from source
)
select ok(
  value->>'status' = 'active_account'
    and exists (
      select 1 from public.planet_member_state p
      where p.user_id = result.user_id and p.nickname = 'preexisting-profile'
    )
    and not exists (select 1 from private.shop_account_state s where s.user_id = result.user_id)
    and not exists (select 1 from private.growth_journal_state s where s.user_id = result.user_id),
  'an existing account is held without overwriting or initializing game state'
)
from result;

select set_config('request.jwt.claim.sub', (
  select user_id::text from pg_temp.shop_guest_bootstrap_test_accounts
  where case_name = 'invalid_source'
), true);
with source as (
  select user_id, import_id,
    jsonb_set(request, '{snapshot,data,profile,nickname}', to_jsonb(repeat('N', 25)), true) as request
  from pg_temp.shop_guest_bootstrap_test_accounts
  where case_name = 'invalid_source'
)
insert into pg_temp.shop_guest_bootstrap_test_results(case_name, user_id, request, value)
select 'invalid_source_first', user_id, request,
       pg_temp.try_shop_guest_bootstrap(import_id, request)
from source;

select ok(
  (select value->>'status' from pg_temp.shop_guest_bootstrap_test_results
   where case_name = 'invalid_source_first') = 'source_unverifiable'
    and exists (
      select 1 from private.shop_guest_bootstrap_receipt r
      join pg_temp.shop_guest_bootstrap_test_results result
        on result.user_id = r.user_id and result.case_name = 'invalid_source_first'
        and r.status = 'source_unverifiable'
        and r.payload = result.request
    )
    and private.shop_guest_import_target_is_fresh((
      select user_id from pg_temp.shop_guest_bootstrap_test_results
      where case_name = 'invalid_source_first'
    ))
    and not exists (
      select 1 from public.planet_member_state p
      join pg_temp.shop_guest_bootstrap_test_results result on result.user_id = p.user_id
      where result.case_name = 'invalid_source_first'
    )
    and not exists (
      select 1 from private.shop_account_state s
      join pg_temp.shop_guest_bootstrap_test_results result on result.user_id = s.user_id
      where result.case_name = 'invalid_source_first'
    )
    and not exists (
      select 1 from private.growth_journal_state s
      join pg_temp.shop_guest_bootstrap_test_results result on result.user_id = s.user_id
      where result.case_name = 'invalid_source_first'
    ),
  'last-stage source rejection stores only a private held receipt and leaves a fresh account usable'
);

with source as (
  select user_id, import_id,
    jsonb_set(request, '{snapshot,data,profile,nickname}', to_jsonb(repeat('N', 25)), true) as request
  from pg_temp.shop_guest_bootstrap_test_accounts
  where case_name = 'invalid_source'
), replay as (
  select pg_temp.try_shop_guest_bootstrap(import_id, request) as value, user_id, import_id
  from source
), stored as (
  select r.result
  from private.shop_guest_bootstrap_receipt r
  join replay on replay.user_id = r.user_id and replay.import_id = r.import_id
)
select is((select value from replay), (select result from stored),
  'same invalid private payload replays its immutable held result');

with account as (
  select user_id from pg_temp.shop_guest_bootstrap_test_accounts
  where case_name = 'invalid_source'
), source as (
  select a.user_id,
    pg_temp.shop_guest_bootstrap_test_request(a.user_id, gen_random_uuid()) as request
  from account a
)
insert into pg_temp.shop_guest_bootstrap_test_results(case_name, user_id, request, value)
select 'invalid_source_retry', user_id, request,
       pg_temp.try_shop_guest_bootstrap(
         (request#>>'{snapshot,import_id}')::uuid, request
       )
from source;

select ok(
  (select value->>'status' from pg_temp.shop_guest_bootstrap_test_results
   where case_name = 'invalid_source_retry') = 'imported'
    and not private.shop_guest_import_target_is_fresh((
      select user_id from pg_temp.shop_guest_bootstrap_test_results
      where case_name = 'invalid_source_retry'
    ))
    and (select count(*) = 2 from private.shop_guest_bootstrap_receipt r
         where r.user_id = (
           select user_id from pg_temp.shop_guest_bootstrap_test_results
           where case_name = 'invalid_source_retry'
         )),
  'a held receipt does not block a new valid import ID for the still-fresh account'
);

create function private.shop_guest_bootstrap_test_fail()
returns trigger
language plpgsql
set search_path = ''
as $$
begin
  if new.user_id::text = current_setting('test.shop_guest_bootstrap_failure_user', true)
    and tg_table_name = current_setting('test.shop_guest_bootstrap_failure_table', true)
  then
    raise exception 'synthetic bootstrap failure at %', tg_table_name
      using errcode = 'P0001';
  end if;
  return new;
end;
$$;

create trigger shop_guest_bootstrap_test_fail_journal_cycle
before insert on private.growth_journal_cycles
for each row execute function private.shop_guest_bootstrap_test_fail();

create trigger shop_guest_bootstrap_test_fail_receipt
before insert on private.shop_guest_bootstrap_receipt
for each row execute function private.shop_guest_bootstrap_test_fail();

select set_config('request.jwt.claim.sub', (
  select user_id::text from pg_temp.shop_guest_bootstrap_test_accounts
  where case_name = 'journal_failure'
), true);
select set_config('test.shop_guest_bootstrap_failure_user', (
  select user_id::text from pg_temp.shop_guest_bootstrap_test_accounts
  where case_name = 'journal_failure'
), true);
select set_config('test.shop_guest_bootstrap_failure_table', 'growth_journal_cycles', true);
select throws_ok(
  format(
    'select private.shop_guest_bootstrap(%L::uuid, %L::jsonb)',
    (select import_id from pg_temp.shop_guest_bootstrap_test_accounts
     where case_name = 'journal_failure'),
    (select request::text from pg_temp.shop_guest_bootstrap_test_accounts
     where case_name = 'journal_failure')
  ),
  'P0001', 'synthetic bootstrap failure at growth_journal_cycles',
  'a failure after the journal-cycle write aborts the bootstrap transaction'
);
select ok(
  private.shop_guest_import_target_is_fresh((
    select user_id from pg_temp.shop_guest_bootstrap_test_accounts
    where case_name = 'journal_failure'
  ))
  and not exists (
    select 1 from public.planet_member_state p
    join pg_temp.shop_guest_bootstrap_test_accounts a on a.user_id = p.user_id
    where a.case_name = 'journal_failure'
    union all
    select 1 from private.shop_account_state s
    join pg_temp.shop_guest_bootstrap_test_accounts a on a.user_id = s.user_id
    where a.case_name = 'journal_failure'
    union all
    select 1 from private.growth_journal_state s
    join pg_temp.shop_guest_bootstrap_test_accounts a on a.user_id = s.user_id
    where a.case_name = 'journal_failure'
    union all
    select 1 from private.growth_journal_cycles c
    join pg_temp.shop_guest_bootstrap_test_accounts a on a.user_id = c.user_id
    where a.case_name = 'journal_failure'
    union all
    select 1 from private.shop_guest_bootstrap_receipt r
    join pg_temp.shop_guest_bootstrap_test_accounts a on a.user_id = r.user_id
    where a.case_name = 'journal_failure'
    union all
    select 1 from private.shop_account_lock l
    join pg_temp.shop_guest_bootstrap_test_accounts a on a.user_id = l.user_id
    where a.case_name = 'journal_failure'
  ),
  'journal-cycle failure rolls back the four game rows, receipt, and lock metadata'
);

select set_config('request.jwt.claim.sub', (
  select user_id::text from pg_temp.shop_guest_bootstrap_test_accounts
  where case_name = 'receipt_failure'
), true);
select set_config('test.shop_guest_bootstrap_failure_user', (
  select user_id::text from pg_temp.shop_guest_bootstrap_test_accounts
  where case_name = 'receipt_failure'
), true);
select set_config('test.shop_guest_bootstrap_failure_table', 'shop_guest_bootstrap_receipt', true);
select throws_ok(
  format(
    'select private.shop_guest_bootstrap(%L::uuid, %L::jsonb)',
    (select import_id from pg_temp.shop_guest_bootstrap_test_accounts
     where case_name = 'receipt_failure'),
    (select request::text from pg_temp.shop_guest_bootstrap_test_accounts
     where case_name = 'receipt_failure')
  ),
  'P0001', 'synthetic bootstrap failure at shop_guest_bootstrap_receipt',
  'a failure at the final imported receipt aborts all earlier bootstrap writes'
);
select ok(
  private.shop_guest_import_target_is_fresh((
    select user_id from pg_temp.shop_guest_bootstrap_test_accounts
    where case_name = 'receipt_failure'
  ))
  and not exists (
    select 1 from public.planet_member_state p
    join pg_temp.shop_guest_bootstrap_test_accounts a on a.user_id = p.user_id
    where a.case_name = 'receipt_failure'
    union all
    select 1 from private.shop_account_state s
    join pg_temp.shop_guest_bootstrap_test_accounts a on a.user_id = s.user_id
    where a.case_name = 'receipt_failure'
    union all
    select 1 from private.growth_journal_state s
    join pg_temp.shop_guest_bootstrap_test_accounts a on a.user_id = s.user_id
    where a.case_name = 'receipt_failure'
    union all
    select 1 from private.growth_journal_cycles c
    join pg_temp.shop_guest_bootstrap_test_accounts a on a.user_id = c.user_id
    where a.case_name = 'receipt_failure'
    union all
    select 1 from private.shop_guest_bootstrap_receipt r
    join pg_temp.shop_guest_bootstrap_test_accounts a on a.user_id = r.user_id
    where a.case_name = 'receipt_failure'
    union all
    select 1 from private.shop_account_lock l
    join pg_temp.shop_guest_bootstrap_test_accounts a on a.user_id = l.user_id
    where a.case_name = 'receipt_failure'
  ),
  'receipt failure rolls back all four game rows, the receipt, and lock metadata'
);

select * from finish();
rollback;
\echo SHOP_GUEST_IMPORT_V2_ROLLBACK_COMPLETED
