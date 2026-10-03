alter table private.planet_device_state
  add column canonical_version bigint not null default 0 check (canonical_version >= 0);

create table private.shop_device_contribution_state (
  user_id uuid not null references auth.users(id) on delete cascade,
  device_id uuid not null,
  canonical_version bigint not null check (canonical_version >= 0),
  canonical_payload jsonb not null check (jsonb_typeof(canonical_payload) = 'object'),
  updated_at timestamptz not null default now(),
  primary key (user_id, device_id)
);

create table private.shop_effect_contribution (
  user_id uuid not null,
  device_id uuid not null,
  cycle_id text not null check (char_length(cycle_id) between 1 and 80),
  date date not null,
  effect_revision bigint not null check (effect_revision >= 0),
  canonical_version bigint not null check (canonical_version >= 0),
  tokens bigint not null check (tokens >= 0),
  growth_bps smallint not null check (growth_bps between 0 and 2000),
  wallet_bps smallint not null check (wallet_bps between 0 and 3000),
  primary key (user_id, device_id, cycle_id, date, effect_revision),
  foreign key (user_id, device_id)
    references private.shop_device_contribution_state(user_id, device_id) on delete cascade
);

create index shop_effect_contribution_by_cycle_date
  on private.shop_effect_contribution(user_id, cycle_id, date, device_id);

create table private.shop_device_activity_day (
  user_id uuid not null,
  device_id uuid not null,
  reward_date date not null,
  cycle_id text not null check (char_length(cycle_id) between 1 and 80),
  first_occurred_at_utc timestamptz not null,
  canonical_version bigint not null check (canonical_version >= 0),
  tokens bigint not null check (tokens > 0),
  primary key (user_id, device_id, reward_date),
  foreign key (user_id, device_id)
    references private.shop_device_contribution_state(user_id, device_id) on delete cascade
);

create table private.shop_activity_day (
  user_id uuid not null references auth.users(id) on delete cascade,
  reward_date date not null,
  cycle_id text not null check (char_length(cycle_id) between 1 and 80),
  first_occurred_at_utc timestamptz not null,
  tokens bigint not null check (tokens > 0),
  primary key (user_id, reward_date)
);

create table private.shop_cycle_effect_baseline (
  user_id uuid not null references auth.users(id) on delete cascade,
  cycle_id text not null check (char_length(cycle_id) between 1 and 80),
  started_at timestamptz not null,
  primary key (user_id, cycle_id)
);

alter table private.shop_device_contribution_state enable row level security;
alter table private.shop_effect_contribution enable row level security;
alter table private.shop_device_activity_day enable row level security;
alter table private.shop_activity_day enable row level security;
alter table private.shop_cycle_effect_baseline enable row level security;

revoke all on private.shop_device_contribution_state, private.shop_effect_contribution,
  private.shop_device_activity_day, private.shop_activity_day, private.shop_cycle_effect_baseline
  from public, anon, authenticated, service_role;

create function private.shop_weighted_growth(p_user_id uuid, p_cycle_id text)
returns numeric
language sql set search_path = '' as $$
  with raw_daily as (
    select d.day::date as date, sum(d.tokens::numeric) as total_tokens
    from private.planet_device_state p
    cross join lateral jsonb_each_text(p.daily_tokens) d(day, tokens)
    where p.user_id = p_user_id and p.current_cycle_id = p_cycle_id
    group by d.day::date
  ), weighted_daily as (
    select c.date,
      sum(c.tokens::numeric * c.growth_bps::numeric) as weighted_growth_bps_tokens
    from private.shop_effect_contribution c
    where c.user_id = p_user_id and c.cycle_id = p_cycle_id
    group by c.date
  )
  select coalesce(sum(
    case when d.total_tokens = 0 then 0::numeric
      else log(2::numeric, 1::numeric + d.total_tokens / 100000::numeric)
        * (1::numeric + coalesce(w.weighted_growth_bps_tokens, 0::numeric)
          / (d.total_tokens * 10000::numeric))
    end
  ), 0::numeric)
  from raw_daily d
  left join weighted_daily w using (date);
$$;

revoke all on function private.shop_weighted_growth(uuid, text)
  from public, anon, authenticated, service_role;

create function private.shop_validate_effect_upload(
  p_user_id uuid, p_state jsonb, p_device jsonb
)
returns void
language plpgsql security definer set search_path = '' as $$
declare
  v_cycle_id text;
  v_timezone text;
  v_device_id uuid;
  v_version bigint;
  v_lifetime bigint;
  v_current bigint;
  v_closed_tokens numeric;
  v_row jsonb;
  v_row_cycle text;
  v_date date;
  v_revision bigint;
  v_tokens bigint;
  v_day_start timestamptz;
  v_day_end timestamptz;
  v_interval_start timestamptz;
  v_interval_end timestamptz;
  v_reward_date date;
  v_first_occurred timestamptz;
begin
  if p_user_id is null or p_user_id is distinct from (select auth.uid()) then
    raise exception 'planet contribution access denied' using errcode = '42501';
  end if;
  if jsonb_typeof(p_state) is distinct from 'object'
    or jsonb_typeof(p_device) is distinct from 'object'
    or (select array_agg(k.key order by k.key) from jsonb_object_keys(p_device) k(key))
      is distinct from array[
        'activity_days', 'canonical_version', 'current_cycle_id', 'current_planet_tokens',
        'daily_segments', 'daily_tokens', 'device_id', 'incomplete', 'lifetime_tokens'
      ]::text[]
    or jsonb_typeof(p_state->'current_cycle_id') is distinct from 'string'
    or char_length(p_state->>'current_cycle_id') not between 1 and 80
    or jsonb_typeof(p_device->'current_cycle_id') is distinct from 'string'
    or p_device->>'current_cycle_id' is distinct from p_state->>'current_cycle_id'
    or jsonb_typeof(p_device->'device_id') is distinct from 'string'
    or jsonb_typeof(p_device->'canonical_version') is distinct from 'number'
    or (p_device->>'canonical_version')::numeric < 0
    or (p_device->>'canonical_version')::numeric > 9223372036854775807::numeric
    or (p_device->>'canonical_version')::numeric <> trunc((p_device->>'canonical_version')::numeric)
    or jsonb_typeof(p_device->'current_planet_tokens') is distinct from 'number'
    or jsonb_typeof(p_device->'lifetime_tokens') is distinct from 'number'
    or jsonb_typeof(p_device->'daily_tokens') is distinct from 'object'
    or jsonb_typeof(p_device->'daily_segments') is distinct from 'array'
    or jsonb_typeof(p_device->'activity_days') is distinct from 'array'
  then
    raise exception 'planet contribution fields are invalid' using errcode = '23514';
  end if;

  v_cycle_id := p_state->>'current_cycle_id';
  v_timezone := (select s.reward_timezone from private.shop_account_state s where s.user_id = p_user_id);
  if v_timezone is null or not exists (
    select 1 from pg_catalog.pg_timezone_names z where z.name = v_timezone
  ) then
    raise exception 'account reward timezone is unavailable' using errcode = '23514';
  end if;

  if jsonb_typeof(p_device->'incomplete') is distinct from 'boolean'
    or (p_device->>'current_planet_tokens')::numeric < 0
    or (p_device->>'current_planet_tokens')::numeric > 9223372036854775807::numeric
    or (p_device->>'current_planet_tokens')::numeric <> trunc((p_device->>'current_planet_tokens')::numeric)
    or (p_device->>'lifetime_tokens')::numeric < (p_device->>'current_planet_tokens')::numeric
    or (p_device->>'lifetime_tokens')::numeric > 9223372036854775807::numeric
    or (p_device->>'lifetime_tokens')::numeric <> trunc((p_device->>'lifetime_tokens')::numeric)
    or exists (
      select 1 from jsonb_each(p_device->'daily_tokens') d(key, value)
      where d.key !~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'
        or jsonb_typeof(d.value) is distinct from 'number'
        or (d.value #>> '{}')::numeric < 0
        or (d.value #>> '{}')::numeric > 9223372036854775807::numeric
        or (d.value #>> '{}')::numeric <> trunc((d.value #>> '{}')::numeric)
    )
    or (select coalesce(sum(d.value::numeric), 0)
        from jsonb_each_text(p_device->'daily_tokens') d(key, value))
      <> (p_device->>'current_planet_tokens')::numeric
  then
    raise exception 'planet raw contribution is invalid' using errcode = '23514';
  end if;

  v_device_id := (p_device->>'device_id')::uuid;
  v_version := (p_device->>'canonical_version')::bigint;
  v_lifetime := (p_device->>'lifetime_tokens')::bigint;
  v_current := (p_device->>'current_planet_tokens')::bigint;

  for v_row in select value from jsonb_array_elements(p_device->'daily_segments')
  loop
    if jsonb_typeof(v_row) is distinct from 'object'
      or (select array_agg(k.key order by k.key) from jsonb_object_keys(v_row) k(key))
        is distinct from array['cycle_id', 'date', 'effect_revision', 'tokens']::text[]
      or jsonb_typeof(v_row->'cycle_id') is distinct from 'string'
      or char_length(v_row->>'cycle_id') not between 1 and 80
      or jsonb_typeof(v_row->'date') is distinct from 'string'
      or (v_row->>'date') !~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'
      or jsonb_typeof(v_row->'effect_revision') is distinct from 'number'
      or (v_row->>'effect_revision')::numeric < 0
      or (v_row->>'effect_revision')::numeric > 9223372036854775807::numeric
      or (v_row->>'effect_revision')::numeric <> trunc((v_row->>'effect_revision')::numeric)
      or jsonb_typeof(v_row->'tokens') is distinct from 'number'
      or (v_row->>'tokens')::numeric < 0
      or (v_row->>'tokens')::numeric > 9223372036854775807::numeric
      or (v_row->>'tokens')::numeric <> trunc((v_row->>'tokens')::numeric)
    then
      raise exception 'effect contribution segment is invalid' using errcode = '23514';
    end if;

    v_row_cycle := v_row->>'cycle_id';
    v_date := (v_row->>'date')::date;
    v_revision := (v_row->>'effect_revision')::bigint;
    v_tokens := (v_row->>'tokens')::bigint;
    if to_char(v_date, 'YYYY-MM-DD') <> v_row->>'date' then
      raise exception 'effect contribution date is invalid' using errcode = '23514';
    end if;
    if v_row_cycle <> v_cycle_id and not exists (
      select 1 from private.planet_wallet_credits w
      where w.user_id = p_user_id and w.previous_cycle_id = v_row_cycle
    ) then
      raise exception 'effect contribution cycle is unknown' using errcode = '23514';
    end if;

    v_day_start := v_date::timestamp at time zone v_timezone;
    v_day_end := (v_date + 1)::timestamp at time zone v_timezone;
    if v_revision = 0 then
      select b.started_at, min(h.started_at)
      into v_interval_start, v_interval_end
      from private.shop_cycle_effect_baseline b
      left join private.shop_effect_history h
        on h.user_id = b.user_id and h.cycle_id = b.cycle_id
      where b.user_id = p_user_id and b.cycle_id = v_row_cycle
      group by b.started_at;
      if not found or v_interval_start is null
        or greatest(v_interval_start, v_day_start)
          >= least(coalesce(v_interval_end, 'infinity'::timestamptz), v_day_end)
      then
        raise exception 'effect baseline revision is not active on contribution date' using errcode = '23514';
      end if;
    else
      select h.started_at, h.ended_at
      into v_interval_start, v_interval_end
      from private.shop_effect_history h
      where h.user_id = p_user_id and h.cycle_id = v_row_cycle and h.revision = v_revision;
      if not found or greatest(v_interval_start, v_day_start)
        >= least(coalesce(v_interval_end, 'infinity'::timestamptz), v_day_end)
      then
        raise exception 'effect revision is not active on contribution date' using errcode = '23514';
      end if;
    end if;
  end loop;

  if exists (
    with raw_daily as (
      select d.key::date as date, sum(d.value::numeric) as tokens
      from jsonb_each(p_device->'daily_tokens') d(key, value)
      group by d.key::date
    ), segment_daily as (
      select (s.value->>'date')::date as date,
        sum((s.value->>'tokens')::numeric) as tokens
      from jsonb_array_elements(p_device->'daily_segments') s(value)
      where s.value->>'cycle_id' = v_cycle_id
      group by (s.value->>'date')::date
    )
    select 1 from raw_daily r full join segment_daily s using (date)
    where coalesce(r.tokens, 0) <> coalesce(s.tokens, 0)
  ) then
    raise exception 'effect segments do not sum to raw daily tokens' using errcode = '23514';
  end if;

  select coalesce(sum((s.value->>'tokens')::numeric), 0)
  into v_closed_tokens
  from jsonb_array_elements(p_device->'daily_segments') s(value)
  where s.value->>'cycle_id' <> v_cycle_id;
  if v_closed_tokens > v_lifetime - v_current then
    raise exception 'closed-cycle segments exceed raw lifetime tokens' using errcode = '23514';
  end if;

  for v_row in select value from jsonb_array_elements(p_device->'activity_days')
  loop
    if jsonb_typeof(v_row) is distinct from 'object'
      or (select array_agg(k.key order by k.key) from jsonb_object_keys(v_row) k(key))
        is distinct from array['cycle_id', 'first_occurred_at_utc', 'reward_date', 'tokens']::text[]
      or jsonb_typeof(v_row->'cycle_id') is distinct from 'string'
      or char_length(v_row->>'cycle_id') not between 1 and 80
      or jsonb_typeof(v_row->'first_occurred_at_utc') is distinct from 'string'
      or jsonb_typeof(v_row->'reward_date') is distinct from 'string'
      or (v_row->>'reward_date') !~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'
      or jsonb_typeof(v_row->'tokens') is distinct from 'number'
      or (v_row->>'tokens')::numeric <= 0
      or (v_row->>'tokens')::numeric > 9223372036854775807::numeric
      or (v_row->>'tokens')::numeric <> trunc((v_row->>'tokens')::numeric)
    then
      raise exception 'activity day is invalid' using errcode = '23514';
    end if;
    v_row_cycle := v_row->>'cycle_id';
    v_reward_date := (v_row->>'reward_date')::date;
    v_first_occurred := (v_row->>'first_occurred_at_utc')::timestamptz;
    if to_char(v_reward_date, 'YYYY-MM-DD') <> v_row->>'reward_date'
      or (v_first_occurred at time zone v_timezone)::date <> v_reward_date
      or (v_row_cycle <> v_cycle_id and not exists (
        select 1 from private.planet_wallet_credits w
        where w.user_id = p_user_id and w.previous_cycle_id = v_row_cycle
      ))
    then
      raise exception 'activity date or cycle is invalid' using errcode = '23514';
    end if;
  end loop;
  if (select count(*) from jsonb_array_elements(p_device->'activity_days'))
    <> (select count(distinct value->>'reward_date') from jsonb_array_elements(p_device->'activity_days'))
  then
    raise exception 'activity reward dates must be unique per device' using errcode = '23514';
  end if;
exception
  when invalid_text_representation or invalid_datetime_format or datetime_field_overflow
    or numeric_value_out_of_range then
    raise exception 'planet contribution values are invalid' using errcode = '23514';
end;
$$;

revoke all on function private.shop_validate_effect_upload(uuid, jsonb, jsonb)
  from public, anon, authenticated, service_role;

create function private.shop_replace_effect_snapshot(
  p_user_id uuid, p_state jsonb, p_device jsonb
)
returns boolean
language plpgsql security definer set search_path = '' as $$
declare
  v_cycle_id text;
  v_cycle_started_at timestamptz;
  v_device_id uuid;
  v_version bigint;
  v_existing private.shop_device_contribution_state%rowtype;
begin
  if p_user_id is null or p_user_id is distinct from (select auth.uid()) then
    raise exception 'planet contribution access denied' using errcode = '42501';
  end if;
  perform private.lock_shop_account(p_user_id);

  select p.current_cycle_id, p.cycle_started_at
  into v_cycle_id, v_cycle_started_at
  from public.planet_member_state p where p.user_id = p_user_id for update;
  if not found or p_state->>'current_cycle_id' is distinct from v_cycle_id then
    raise exception 'planet contribution cycle is stale' using errcode = '23514';
  end if;
  insert into private.shop_cycle_effect_baseline(user_id, cycle_id, started_at)
  values (p_user_id, v_cycle_id, v_cycle_started_at)
  on conflict (user_id, cycle_id) do nothing;

  perform private.shop_validate_effect_upload(p_user_id, p_state, p_device);
  v_device_id := (p_device->>'device_id')::uuid;
  v_version := (p_device->>'canonical_version')::bigint;

  select s.* into v_existing
  from private.shop_device_contribution_state s
  where s.user_id = p_user_id and s.device_id = v_device_id
  for update;
  if found and v_version < v_existing.canonical_version then
    raise exception 'canonical contribution version regressed' using errcode = '23514';
  end if;
  if found and v_version = v_existing.canonical_version then
    if p_device is distinct from v_existing.canonical_payload then
      raise exception 'canonical contribution version conflicts with saved payload' using errcode = '23514';
    end if;
    update private.planet_device_state d set canonical_version = v_version
    where d.user_id = p_user_id and d.device_id = v_device_id;
    return false;
  end if;

  insert into private.shop_device_contribution_state(
    user_id, device_id, canonical_version, canonical_payload, updated_at
  ) values (p_user_id, v_device_id, v_version, p_device, now())
  on conflict (user_id, device_id) do update set
    canonical_version = excluded.canonical_version,
    canonical_payload = excluded.canonical_payload,
    updated_at = excluded.updated_at;

  delete from private.shop_effect_contribution c
  where c.user_id = p_user_id and c.device_id = v_device_id;
  delete from private.shop_device_activity_day a
  where a.user_id = p_user_id and a.device_id = v_device_id;

  insert into private.shop_effect_contribution(
    user_id, device_id, cycle_id, date, effect_revision, canonical_version,
    tokens, growth_bps, wallet_bps
  )
  with segments as (
    select s.value->>'cycle_id' as cycle_id,
      (s.value->>'date')::date as date,
      (s.value->>'effect_revision')::bigint as effect_revision,
      sum((s.value->>'tokens')::numeric)::bigint as tokens
    from jsonb_array_elements(p_device->'daily_segments') s(value)
    group by s.value->>'cycle_id', (s.value->>'date')::date,
      (s.value->>'effect_revision')::bigint
  )
  select p_user_id, v_device_id, s.cycle_id, s.date, s.effect_revision, v_version,
    s.tokens,
    case when s.effect_revision = 0 then 0
      else (h.effects->>'civilization_growth_bps')::smallint end,
    case when s.effect_revision = 0 then 0
      else (h.effects->>'token_earning_bps')::smallint end
  from segments s
  left join private.shop_effect_history h
    on h.user_id = p_user_id and h.cycle_id = s.cycle_id
      and h.revision = s.effect_revision;

  insert into private.shop_device_activity_day(
    user_id, device_id, reward_date, cycle_id, first_occurred_at_utc,
    canonical_version, tokens
  )
  select p_user_id, v_device_id, (a.value->>'reward_date')::date,
    a.value->>'cycle_id', (a.value->>'first_occurred_at_utc')::timestamptz,
    v_version, (a.value->>'tokens')::bigint
  from jsonb_array_elements(p_device->'activity_days') a(value);

  update private.planet_device_state d set canonical_version = v_version
  where d.user_id = p_user_id and d.device_id = v_device_id;

  delete from private.shop_activity_day a where a.user_id = p_user_id;
  insert into private.shop_activity_day(
    user_id, reward_date, cycle_id, first_occurred_at_utc, tokens
  )
  select a.user_id, a.reward_date,
    (array_agg(a.cycle_id order by a.first_occurred_at_utc))[1],
    min(a.first_occurred_at_utc), sum(a.tokens::numeric)::bigint
  from private.shop_device_activity_day a
  where a.user_id = p_user_id
  group by a.user_id, a.reward_date;

  return true;
end;
$$;

revoke all on function private.shop_replace_effect_snapshot(uuid, jsonb, jsonb)
  from public, anon, authenticated, service_role;
