create table private.growth_journal_state (
  user_id uuid primary key references auth.users(id) on delete cascade,
  generation bigint not null default 0 check (generation >= 0),
  deleted_at timestamptz,
  timezone text
);

create table private.growth_journal_cycles (
  user_id uuid not null references auth.users(id) on delete cascade,
  cycle_id text not null check (char_length(cycle_id) between 1 and 80),
  started_at timestamptz,
  ended_at timestamptz,
  wallet_credit bigint check (wallet_credit is null or wallet_credit >= 0),
  wallet_credit_at timestamptz,
  primary key (user_id, cycle_id),
  check (ended_at is null or started_at is null or ended_at > started_at),
  check ((wallet_credit is null) = (wallet_credit_at is null))
);

create table private.growth_journal_days (
  user_id uuid not null references auth.users(id) on delete cascade,
  device_id uuid not null,
  cycle_id text not null check (char_length(cycle_id) between 1 and 80),
  bucket_date date not null,
  agent text not null check (agent in ('codex', 'claude_code')),
  revision bigint not null check (revision > 0),
  generation bigint not null check (generation >= 0),
  present boolean not null,
  confirmed_tokens bigint check (confirmed_tokens is null or confirmed_tokens >= 0),
  coverage text not null check (coverage in ('complete', 'partial', 'unavailable', 'unsupported', 'user_disabled')),
  payload_hash text not null check (payload_hash ~ '^[0-9a-f]{64}$'),
  primary key (user_id, device_id, cycle_id, bucket_date, agent),
  check (present or confirmed_tokens is null)
);

revoke all on private.growth_journal_state, private.growth_journal_cycles,
  private.growth_journal_days from public, anon, authenticated;

create function private.growth_journal_json(p_user_id uuid)
returns jsonb
language sql security definer set search_path = '' as $$
  select jsonb_build_object(
    'generation', coalesce(s.generation, 0),
    'deleted_at_utc', case when s.deleted_at is null then null else
      to_char(s.deleted_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"') end,
    'timezone', coalesce(s.timezone, p.timezone),
    'cycles', coalesce((
      select jsonb_agg(jsonb_build_object(
        'cycle_id', c.cycle_id,
        'started_at_utc', to_char(c.started_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
        'ended_at_utc', case when c.ended_at is null then null else
          to_char(c.ended_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"') end,
        'wallet_credit', c.wallet_credit,
        'wallet_credit_at_utc', case when c.wallet_credit_at is null then null else
          to_char(c.wallet_credit_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"') end
      ) order by c.started_at, c.cycle_id)
      from private.growth_journal_cycles c where c.user_id = p_user_id
    ), '[]'::jsonb),
    'entries', coalesce((
      select jsonb_agg(jsonb_build_object(
        'device_id', d.device_id,
        'cycle_id', d.cycle_id,
        'bucket_date', to_char(d.bucket_date, 'YYYY-MM-DD'),
        'agent', d.agent,
        'revision', d.revision,
        'generation', d.generation,
        'present', d.present,
        'confirmed_tokens', d.confirmed_tokens,
        'coverage', d.coverage,
        'payload_hash', d.payload_hash
      ) order by d.cycle_id, d.bucket_date, d.agent, d.device_id)
      from private.growth_journal_days d where d.user_id = p_user_id
    ), '[]'::jsonb)
  )
  from (select 1) singleton
  left join private.growth_journal_state s on s.user_id = p_user_id
  left join public.planet_member_state p on p.user_id = p_user_id;
$$;

create function private.get_my_growth_journal()
returns jsonb
language plpgsql security definer set search_path = '' as $$
begin
  if (select auth.uid()) is null then
    raise exception 'authentication required' using errcode = '42501';
  end if;
  insert into private.growth_journal_state(user_id,timezone)
  select (select auth.uid()), p.timezone from public.planet_member_state p
  where p.user_id = (select auth.uid())
  on conflict (user_id) do nothing;
  return private.growth_journal_json((select auth.uid()));
end;
$$;

create function public.get_my_growth_journal()
returns jsonb
language sql security invoker set search_path = '' as $$
  select private.get_my_growth_journal();
$$;

create function private.upsert_my_growth_journal(
  p_generation bigint,
  p_timezone text,
  p_cycles jsonb,
  p_entries jsonb
)
returns jsonb
language plpgsql security definer set search_path = '' as $$
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
$$;

create function public.upsert_my_growth_journal(p_generation bigint, p_timezone text, p_cycles jsonb, p_entries jsonb)
returns jsonb
language sql security invoker set search_path = '' as $$
  select private.upsert_my_growth_journal(p_generation, p_timezone, p_cycles, p_entries);
$$;

create function private.delete_my_growth_journal()
returns jsonb
language plpgsql security definer set search_path = '' as $$
declare
  v_user_id uuid := (select auth.uid());
begin
  if v_user_id is null then
    raise exception 'authentication required' using errcode = '42501';
  end if;
  insert into private.growth_journal_state(user_id,generation,deleted_at)
  values (v_user_id,1,now())
  on conflict (user_id) do update set
    generation = private.growth_journal_state.generation + 1,
    deleted_at = now();
  delete from private.growth_journal_days where user_id = v_user_id;
  delete from private.growth_journal_cycles where user_id = v_user_id;
  return private.growth_journal_json(v_user_id);
end;
$$;

create function public.delete_my_growth_journal()
returns jsonb
language sql security invoker set search_path = '' as $$
  select private.delete_my_growth_journal();
$$;

revoke all on function private.growth_journal_json(uuid), private.get_my_growth_journal(),
  private.upsert_my_growth_journal(bigint,text,jsonb,jsonb), private.delete_my_growth_journal(),
  public.get_my_growth_journal(), public.upsert_my_growth_journal(bigint,text,jsonb,jsonb),
  public.delete_my_growth_journal() from public, anon;
grant execute on function private.get_my_growth_journal(),
  private.upsert_my_growth_journal(bigint,text,jsonb,jsonb), private.delete_my_growth_journal(),
  public.get_my_growth_journal(), public.upsert_my_growth_journal(bigint,text,jsonb,jsonb),
  public.delete_my_growth_journal() to authenticated;
