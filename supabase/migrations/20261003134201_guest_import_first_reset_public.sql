-- Dispatch authenticated schema-2 imports to the private atomic writer while
-- retaining the schema-1 held response and immutable public receipt contract.
create or replace function public.import_guest_shop(p_import_id uuid, p_request jsonb)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $function$
declare
  v_user_id uuid := (select auth.uid());
  v_target_account_id text;
  v_prior_payload jsonb;
  v_prior_result jsonb;
  v_status text;
  v_result jsonb;
  v_schema_version jsonb;
begin
  if v_user_id is null then
    raise exception 'authentication required' using errcode = '42501';
  end if;
  if p_import_id is null
    or p_request is null
    or jsonb_typeof(p_request) is distinct from 'object'
    or jsonb_typeof(p_request->'snapshot') is distinct from 'object'
    or jsonb_typeof(p_request#>'{snapshot,import_id}') is distinct from 'string'
    or p_request#>>'{snapshot,import_id}' is distinct from p_import_id::text
    or jsonb_typeof(p_request#>'{snapshot,target_account_id}') is distinct from 'string'
    or jsonb_typeof(p_request#>'{snapshot,source_fingerprint}') is distinct from 'string'
    or p_request#>>'{snapshot,source_fingerprint}' !~ '^[0-9a-f]{64}$'
  then
    raise exception 'guest shop import envelope is invalid' using errcode = '23514';
  end if;

  v_target_account_id := p_request#>>'{snapshot,target_account_id}';
  if v_target_account_id is distinct from 'account:' || v_user_id::text then
    raise exception 'guest shop import target does not match authenticated account'
      using errcode = '42501';
  end if;

  -- Serialize with the existing game writers without initializing account state.
  insert into private.shop_account_lock(user_id) values (v_user_id)
  on conflict (user_id) do nothing;
  perform 1 from private.shop_account_lock l
  where l.user_id = v_user_id
  for update;

  -- Whole-payload equality is decided before schema dispatch or source validation.
  select r.payload, r.result into v_prior_payload, v_prior_result
  from private.shop_guest_bootstrap_receipt r
  where r.user_id = v_user_id and r.import_id = p_import_id;
  if found then
    if v_prior_payload = p_request then
      return v_prior_result;
    end if;
    if p_request->'schema_version' is not distinct from '2'::jsonb then
      return jsonb_build_object(
        'schema_version', 2,
        'import_id', p_import_id::text,
        'account_id', 'account:' || v_user_id::text,
        'source_fingerprint', p_request#>>'{snapshot,source_fingerprint}',
        'status', 'request_conflict'
      );
    end if;
    return jsonb_build_object('import_id', p_import_id, 'status', 'request_conflict');
  end if;

  select r.payload, r.result into v_prior_payload, v_prior_result
  from private.shop_guest_import_request r
  where r.user_id = v_user_id and r.import_id = p_import_id;
  if found then
    if v_prior_payload = p_request then
      return v_prior_result;
    end if;
    if p_request->'schema_version' is not distinct from '2'::jsonb then
      return jsonb_build_object(
        'schema_version', 2,
        'import_id', p_import_id::text,
        'account_id', 'account:' || v_user_id::text,
        'source_fingerprint', p_request#>>'{snapshot,source_fingerprint}',
        'status', 'request_conflict'
      );
    end if;
    return jsonb_build_object('import_id', p_import_id, 'status', 'request_conflict');
  end if;

  v_schema_version := p_request->'schema_version';
  if v_schema_version is not distinct from '2'::jsonb then
    return private.shop_guest_import_v2_bootstrap(p_import_id, p_request);
  end if;
  if v_schema_version is distinct from '1'::jsonb then
    raise exception 'guest shop import envelope is invalid' using errcode = '23514';
  end if;

  -- Keep schema 1 strict and held-only; its existing validator and response shape
  -- remain unchanged after the pre-validation receipt check.
  if not private.shop_guest_import_envelope_valid(p_import_id, p_request) then
    raise exception 'guest shop import envelope is invalid' using errcode = '23514';
  end if;
  if not private.shop_guest_import_target_is_fresh(v_user_id) then
    v_status := 'active_account';
  else
    v_status := 'source_unverifiable';
  end if;
  v_result := jsonb_build_object('import_id', p_import_id, 'status', v_status);
  insert into private.shop_guest_import_request(
    user_id, import_id, payload, source_fingerprint, status, result
  ) values (
    v_user_id, p_import_id, p_request,
    p_request#>>'{snapshot,source_fingerprint}', v_status, v_result
  );
  return v_result;
end;
$function$;

revoke all on function public.import_guest_shop(uuid,jsonb) from public, anon;
grant execute on function public.import_guest_shop(uuid,jsonb) to authenticated;

-- Journal RPCs participate in the same account-lock-before-game-row-lock order.
create or replace function private.get_my_growth_journal()
returns jsonb
language plpgsql
security definer
set search_path = ''
as $function$
declare
  v_user_id uuid := (select auth.uid());
begin
  if v_user_id is null then
    raise exception 'authentication required' using errcode = '42501';
  end if;
  insert into private.shop_account_lock(user_id) values (v_user_id)
  on conflict (user_id) do nothing;
  perform 1 from private.shop_account_lock l
  where l.user_id = v_user_id
  for update;
  insert into private.growth_journal_state(user_id,timezone)
  select v_user_id, p.timezone from public.planet_member_state p
  where p.user_id = v_user_id
  on conflict (user_id) do nothing;
  return private.growth_journal_json(v_user_id);
end;
$function$;

create or replace function private.upsert_my_growth_journal(
  p_generation bigint,
  p_timezone text,
  p_cycles jsonb,
  p_entries jsonb
)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $function$
declare
  v_user_id uuid := (select auth.uid());
  v_generation bigint;
  v_cycle jsonb;
  v_entry jsonb;
  v_cycle_keys text[] := array['cycle_id', 'ended_at_utc', 'started_at_utc', 'wallet_credit', 'wallet_credit_at_utc'];
  v_entry_keys text[] := array['agent', 'bucket_date', 'confirmed_tokens', 'coverage', 'cycle_id', 'device_id', 'generation', 'payload_hash', 'present', 'revision'];
  v_revision bigint;
  v_row private.growth_journal_days%rowtype;
begin
  if v_user_id is null then
    raise exception 'authentication required' using errcode = '42501';
  end if;
  insert into private.shop_account_lock(user_id) values (v_user_id)
  on conflict (user_id) do nothing;
  perform 1 from private.shop_account_lock l
  where l.user_id = v_user_id
  for update;
  if p_generation is null or p_generation < 0
    or p_timezone is null or char_length(p_timezone) not between 1 and 64
    or not exists (select 1 from pg_catalog.pg_timezone_names z where z.name = p_timezone)
    or p_cycles is null
    or jsonb_typeof(p_cycles) <> 'array'
    or p_entries is null
    or jsonb_typeof(p_entries) <> 'array'
  then
    raise exception 'growth journal payload is invalid' using errcode = '23514';
  end if;
  if exists (select 1 from public.planet_member_state p where p.user_id = v_user_id and p.timezone <> p_timezone) then
    raise exception 'growth journal timezone differs from the account setting' using errcode = '23514';
  end if;
  insert into private.growth_journal_state(user_id,timezone) values (v_user_id,p_timezone)
  on conflict (user_id) do nothing;
  select s.generation into v_generation
  from private.growth_journal_state s where s.user_id = v_user_id for update;
  if p_generation > v_generation then
    raise exception 'growth journal generation is invalid' using errcode = '23514';
  end if;

  if p_generation = v_generation then
    update private.growth_journal_state s set timezone = p_timezone
    where s.user_id = v_user_id and (s.timezone is null or s.timezone = p_timezone);
    if exists (select 1 from private.growth_journal_state s where s.user_id = v_user_id and s.timezone <> p_timezone) then
      raise exception 'growth journal timezone differs from the account setting' using errcode = '23514';
    end if;
    for v_cycle in select value from jsonb_array_elements(p_cycles)
    loop
      if jsonb_typeof(v_cycle) <> 'object'
        or (select array_agg(k.key order by k.key) from jsonb_object_keys(v_cycle) k(key))
          is distinct from v_cycle_keys
        or jsonb_typeof(v_cycle->'cycle_id') <> 'string'
        or char_length(v_cycle->>'cycle_id') not between 1 and 80
        or (v_cycle->'started_at_utc' <> 'null'::jsonb and jsonb_typeof(v_cycle->'started_at_utc') <> 'string')
        or (v_cycle->'ended_at_utc' <> 'null'::jsonb and jsonb_typeof(v_cycle->'ended_at_utc') <> 'string')
        or (v_cycle->'wallet_credit' <> 'null'::jsonb and jsonb_typeof(v_cycle->'wallet_credit') <> 'number')
        or (v_cycle->'wallet_credit' <> 'null'::jsonb and trunc((v_cycle->>'wallet_credit')::numeric) <> (v_cycle->>'wallet_credit')::numeric)
        or (v_cycle->'wallet_credit_at_utc' <> 'null'::jsonb and jsonb_typeof(v_cycle->'wallet_credit_at_utc') <> 'string')
        or ((v_cycle->'wallet_credit' = 'null'::jsonb) <> (v_cycle->'wallet_credit_at_utc' = 'null'::jsonb))
      then
        raise exception 'growth journal cycle is invalid' using errcode = '23514';
      end if;
      insert into private.growth_journal_cycles(
        user_id,cycle_id,started_at,ended_at,wallet_credit,wallet_credit_at
      ) values (
        v_user_id, v_cycle->>'cycle_id', nullif(v_cycle->>'started_at_utc', 'null')::timestamptz,
        nullif(v_cycle->>'ended_at_utc', 'null')::timestamptz,
        nullif(v_cycle->>'wallet_credit', 'null')::bigint,
        nullif(v_cycle->>'wallet_credit_at_utc', 'null')::timestamptz
      )
      on conflict (user_id,cycle_id) do update set
        started_at = coalesce(least(private.growth_journal_cycles.started_at, excluded.started_at),
          private.growth_journal_cycles.started_at, excluded.started_at),
        ended_at = coalesce(private.growth_journal_cycles.ended_at, excluded.ended_at),
        wallet_credit = coalesce(private.growth_journal_cycles.wallet_credit, excluded.wallet_credit),
        wallet_credit_at = coalesce(private.growth_journal_cycles.wallet_credit_at, excluded.wallet_credit_at);
    end loop;

    for v_entry in select value from jsonb_array_elements(p_entries)
    loop
      if jsonb_typeof(v_entry) <> 'object'
        or (select array_agg(k.key order by k.key) from jsonb_object_keys(v_entry) k(key))
          is distinct from v_entry_keys
        or jsonb_typeof(v_entry->'device_id') <> 'string'
        or jsonb_typeof(v_entry->'cycle_id') <> 'string'
        or jsonb_typeof(v_entry->'bucket_date') <> 'string'
        or jsonb_typeof(v_entry->'agent') <> 'string'
        or jsonb_typeof(v_entry->'revision') <> 'number'
        or trunc((v_entry->>'revision')::numeric) <> (v_entry->>'revision')::numeric
        or jsonb_typeof(v_entry->'generation') <> 'number'
        or trunc((v_entry->>'generation')::numeric) <> (v_entry->>'generation')::numeric
        or jsonb_typeof(v_entry->'present') <> 'boolean'
        or (v_entry->'confirmed_tokens' <> 'null'::jsonb and jsonb_typeof(v_entry->'confirmed_tokens') <> 'number')
        or (v_entry->'confirmed_tokens' <> 'null'::jsonb and trunc((v_entry->>'confirmed_tokens')::numeric) <> (v_entry->>'confirmed_tokens')::numeric)
        or jsonb_typeof(v_entry->'coverage') <> 'string'
        or jsonb_typeof(v_entry->'payload_hash') <> 'string'
      then
        raise exception 'growth journal entry is invalid' using errcode = '23514';
      end if;
      if (v_entry->>'generation')::bigint > v_generation then
        raise exception 'growth journal entry generation is invalid' using errcode = '23514';
      end if;
      if (v_entry->>'generation')::bigint < v_generation then
        continue;
      end if;
      if (v_entry->>'revision')::bigint <= 0
        or char_length(v_entry->>'cycle_id') not between 1 and 80
        or (v_entry->>'agent') not in ('codex', 'claude_code')
        or (v_entry->>'coverage') not in ('complete', 'partial', 'unavailable', 'unsupported', 'user_disabled')
        or (v_entry->>'payload_hash') !~ '^[0-9a-f]{64}$'
        or ((v_entry->>'present')::boolean = false and v_entry->'confirmed_tokens' <> 'null'::jsonb)
        or (v_entry->'confirmed_tokens' <> 'null'::jsonb and (v_entry->>'confirmed_tokens')::numeric < 0)
      then
        raise exception 'growth journal entry values are invalid' using errcode = '23514';
      end if;
      v_revision := (v_entry->>'revision')::bigint;
      select * into v_row from private.growth_journal_days d
      where d.user_id = v_user_id
        and d.device_id = (v_entry->>'device_id')::uuid
        and d.cycle_id = v_entry->>'cycle_id'
        and d.bucket_date = (v_entry->>'bucket_date')::date
        and d.agent = v_entry->>'agent'
      for update;
      if found and v_row.generation = v_generation and v_row.revision = v_revision
        and v_row.payload_hash <> v_entry->>'payload_hash'
      then
        raise exception 'growth journal revision conflicts' using errcode = '23514';
      end if;
      insert into private.growth_journal_days(
        user_id,device_id,cycle_id,bucket_date,agent,revision,generation,present,
        confirmed_tokens,coverage,payload_hash
      ) values (
        v_user_id,(v_entry->>'device_id')::uuid,v_entry->>'cycle_id',
        (v_entry->>'bucket_date')::date,v_entry->>'agent',v_revision,v_generation,
        (v_entry->>'present')::boolean,nullif(v_entry->>'confirmed_tokens', 'null')::bigint,
        v_entry->>'coverage',v_entry->>'payload_hash'
      )
      on conflict (user_id,device_id,cycle_id,bucket_date,agent) do update set
        revision = excluded.revision,
        generation = excluded.generation,
        present = excluded.present,
        confirmed_tokens = excluded.confirmed_tokens,
        coverage = excluded.coverage,
        payload_hash = excluded.payload_hash
      where private.growth_journal_days.generation < excluded.generation
        or private.growth_journal_days.revision < excluded.revision;
    end loop;
  end if;
  return private.growth_journal_json(v_user_id);
end;
$function$;

create or replace function private.delete_my_growth_journal()
returns jsonb
language plpgsql
security definer
set search_path = ''
as $function$
declare
  v_user_id uuid := (select auth.uid());
begin
  if v_user_id is null then
    raise exception 'authentication required' using errcode = '42501';
  end if;
  insert into private.shop_account_lock(user_id) values (v_user_id)
  on conflict (user_id) do nothing;
  perform 1 from private.shop_account_lock l
  where l.user_id = v_user_id
  for update;
  insert into private.growth_journal_state(user_id,generation,deleted_at)
  values (v_user_id,1,now())
  on conflict (user_id) do update set
    generation = private.growth_journal_state.generation + 1,
    deleted_at = now();
  delete from private.growth_journal_days where user_id = v_user_id;
  delete from private.growth_journal_cycles where user_id = v_user_id;
  return private.growth_journal_json(v_user_id);
end;
$function$;
