create table public.planet_member_state (
  user_id uuid primary key references auth.users(id) on delete cascade,
  state_version smallint not null default 1 check (state_version = 1),
  nickname text not null check (char_length(nickname) between 1 and 24),
  avatar text not null check (avatar in ('masculine', 'feminine')),
  timezone text not null,
  current_cycle_id text not null,
  cycle_started_at timestamptz not null,
  last_reset_at timestamptz,
  current_planet_tokens bigint not null check (current_planet_tokens >= 0),
  lifetime_tokens bigint not null check (lifetime_tokens >= 0),
  growth_credit numeric not null check (growth_credit >= 0),
  stage smallint not null check (stage between 0 and 4),
  progress_to_next numeric not null check (progress_to_next between 0 and 1),
  incomplete boolean not null,
  objects jsonb not null default '[]'::jsonb check (jsonb_typeof(objects) = 'array'),
  shared_visible boolean not null default true,
  updated_at timestamptz not null default now()
);

create table private.planet_wallet_credits (
  user_id uuid not null references auth.users(id) on delete cascade,
  previous_cycle_id text not null,
  amount bigint not null check (amount >= 0),
  created_at timestamptz not null,
  primary key (user_id, previous_cycle_id)
);

create table private.planet_device_state (
  user_id uuid not null references auth.users(id) on delete cascade,
  device_id uuid not null,
  current_cycle_id text not null,
  lifetime_tokens bigint not null check (lifetime_tokens >= 0),
  current_planet_tokens bigint not null check (current_planet_tokens >= 0),
  daily_tokens jsonb not null default '{}'::jsonb check (jsonb_typeof(daily_tokens) = 'object'),
  incomplete boolean not null,
  updated_at timestamptz not null default now(),
  primary key (user_id, device_id)
);

alter table public.planet_member_state enable row level security;
revoke all on public.planet_member_state from public, anon, authenticated;
revoke all on private.planet_wallet_credits from public, anon, authenticated;
revoke all on private.planet_device_state from public, anon, authenticated;

create function private.planet_state_json(p_user_id uuid)
returns jsonb
language sql security definer set search_path = '' as $$
  select jsonb_build_object(
    'version', p.state_version,
    'profile', jsonb_build_object('nickname', p.nickname, 'avatar', p.avatar),
    'timezone', p.timezone,
    'current_cycle_id', p.current_cycle_id,
    'cycle_started_at_utc', to_char(p.cycle_started_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'),
    'last_reset_at_utc', case when p.last_reset_at is null then null else to_char(p.last_reset_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') end,
    'wallet_balance', coalesce((select sum(w.amount) from private.planet_wallet_credits w where w.user_id=p.user_id), 0),
    'wallet_credits', coalesce((
      select jsonb_agg(jsonb_build_object(
        'previous_cycle_id', w.previous_cycle_id,
        'amount', w.amount,
        'created_at_utc', to_char(w.created_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')
      ) order by w.created_at, w.previous_cycle_id)
      from private.planet_wallet_credits w where w.user_id=p.user_id
    ), '[]'::jsonb),
    'current_planet_tokens', p.current_planet_tokens,
    'lifetime_tokens', p.lifetime_tokens,
    'growth_credit', p.growth_credit,
    'stage', p.stage,
    'progress_to_next', p.progress_to_next,
    'incomplete', p.incomplete,
    'can_reset', p.last_reset_at is null or now() >= p.last_reset_at + interval '24 hours',
    'reset_available_at_utc', case when p.last_reset_at is null then null else to_char((p.last_reset_at + interval '24 hours') at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') end,
    'objects', p.objects
  )
  from public.planet_member_state p where p.user_id = p_user_id;
$$;

create function private.get_my_planet_state()
returns jsonb
language plpgsql security definer set search_path = '' as $$
begin
  if (select auth.uid()) is null then
    raise exception 'authentication required' using errcode = '42501';
  end if;
  return private.planet_state_json((select auth.uid()));
end;
$$;

create function public.get_my_planet_state()
returns jsonb
language sql security invoker set search_path = '' as $$
  select private.get_my_planet_state();
$$;

create function private.upsert_my_planet_state(p_state jsonb, p_device_contribution jsonb)
returns jsonb
language plpgsql security definer set search_path = '' as $$
declare
  v_user_id uuid := (select auth.uid());
  v_keys text[] := array[
    'version', 'profile', 'timezone', 'current_cycle_id', 'cycle_started_at_utc', 'last_reset_at_utc',
    'wallet_balance', 'wallet_credits', 'current_planet_tokens', 'lifetime_tokens',
    'growth_credit', 'stage', 'progress_to_next', 'incomplete', 'can_reset',
    'reset_available_at_utc', 'objects'
  ];
  v_profile jsonb;
  v_device jsonb;
  v_object jsonb;
  v_credit jsonb;
  v_old public.planet_member_state%rowtype;
  v_cycle_id text;
  v_cycle_started_at timestamptz;
  v_last_reset_at timestamptz;
  v_nickname text;
  v_avatar text;
  v_timezone text;
  v_device_id uuid;
  v_device_cycle_id text;
  v_server_cycle_id text;
  v_daily_growth numeric;
  v_stage smallint;
  v_progress numeric;
begin
  if v_user_id is null then
    raise exception 'authentication required' using errcode = '42501';
  end if;
  if p_state is null or jsonb_typeof(p_state) <> 'object'
    or not p_state ?& v_keys
    or exists (select 1 from jsonb_object_keys(p_state) k(key) where k.key <> all(v_keys))
  then
    raise exception 'planet state fields are invalid' using errcode = '23514';
  end if;
  v_profile := p_state->'profile';
  if jsonb_typeof(v_profile) <> 'object'
    or not v_profile ?& array['nickname', 'avatar']
    or exists (select 1 from jsonb_object_keys(v_profile) k(key) where k.key not in ('nickname', 'avatar'))
    or jsonb_typeof(v_profile->'nickname') <> 'string'
    or char_length(v_profile->>'nickname') not between 1 and 24
    or p_state->'profile' = 'null'::jsonb
    or jsonb_typeof(p_state->'version') <> 'number'
    or (p_state->>'version')::integer <> 1
    or jsonb_typeof(p_state->'timezone') <> 'string'
    or char_length(p_state->>'timezone') not between 1 and 64
    or not exists (select 1 from pg_catalog.pg_timezone_names z where z.name = p_state->>'timezone')
    or char_length(p_state->>'current_cycle_id') not between 1 and 80
    or jsonb_typeof(p_state->'wallet_credits') <> 'array'
    or jsonb_typeof(p_state->'objects') <> 'array'
    or jsonb_typeof(p_state->'current_planet_tokens') <> 'number'
    or jsonb_typeof(p_state->'lifetime_tokens') <> 'number'
    or jsonb_typeof(p_state->'growth_credit') <> 'number'
    or jsonb_typeof(p_state->'stage') <> 'number'
    or jsonb_typeof(p_state->'progress_to_next') <> 'number'
    or jsonb_typeof(p_state->'incomplete') <> 'boolean'
    or jsonb_typeof(p_state->'can_reset') <> 'boolean'
  then
    raise exception 'planet state values are invalid' using errcode = '23514';
  end if;
  v_nickname := v_profile->>'nickname';
  v_avatar := v_profile->>'avatar';
  v_timezone := p_state->>'timezone';
  if v_avatar not in ('masculine', 'feminine')
    or (p_state->>'current_planet_tokens')::numeric < 0
    or (p_state->>'lifetime_tokens')::numeric < 0
    or (p_state->>'growth_credit')::numeric < 0
    or (p_state->>'stage')::integer not between 0 and 4
    or (p_state->>'progress_to_next')::numeric not between 0 and 1
  then
    raise exception 'planet state values are invalid' using errcode = '23514';
  end if;
  v_cycle_id := p_state->>'current_cycle_id';
  v_cycle_started_at := (p_state->>'cycle_started_at_utc')::timestamptz;
  v_last_reset_at := nullif(p_state->>'last_reset_at_utc', '')::timestamptz;

  v_device := p_device_contribution;
  if jsonb_typeof(v_device) <> 'object'
    or (select array_agg(k.key order by k.key) from jsonb_object_keys(v_device) k(key))
      is distinct from array['current_cycle_id', 'current_planet_tokens', 'daily_tokens', 'device_id', 'incomplete', 'lifetime_tokens']
    or jsonb_typeof(v_device->'device_id') <> 'string'
    or jsonb_typeof(v_device->'current_cycle_id') <> 'string'
    or v_device->>'current_cycle_id' <> v_cycle_id
    or jsonb_typeof(v_device->'lifetime_tokens') <> 'number'
    or (v_device->>'lifetime_tokens')::numeric < 0
    or jsonb_typeof(v_device->'current_planet_tokens') <> 'number'
    or (v_device->>'current_planet_tokens')::numeric < 0
    or (v_device->>'lifetime_tokens')::numeric < (v_device->>'current_planet_tokens')::numeric
    or jsonb_typeof(v_device->'daily_tokens') <> 'object'
    or jsonb_typeof(v_device->'incomplete') <> 'boolean'
    or exists (
      select 1 from jsonb_each(v_device->'daily_tokens') d(key, value)
      where d.key !~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'
        or jsonb_typeof(d.value) <> 'number'
        or (d.value #>> '{}')::numeric < 0
    )
    or (select coalesce(sum(d.value::numeric), 0) from jsonb_each_text(v_device->'daily_tokens') d(key, value))
      <> (v_device->>'current_planet_tokens')::numeric
  then
    raise exception 'planet device contribution is invalid' using errcode = '23514';
  end if;
  v_device_id := (v_device->>'device_id')::uuid;
  v_device_cycle_id := v_device->>'current_cycle_id';

  for v_credit in select value from jsonb_array_elements(p_state->'wallet_credits')
  loop
    if jsonb_typeof(v_credit) <> 'object'
      or (select array_agg(k.key order by k.key) from jsonb_object_keys(v_credit) k(key))
        is distinct from array['amount', 'created_at_utc', 'previous_cycle_id']
      or jsonb_typeof(v_credit->'previous_cycle_id') <> 'string'
      or jsonb_typeof(v_credit->'amount') <> 'number'
      or (v_credit->>'amount')::numeric < 0
      or jsonb_typeof(v_credit->'created_at_utc') <> 'string'
    then
      raise exception 'wallet credit fields are invalid' using errcode = '23514';
    end if;
    insert into private.planet_wallet_credits(user_id, previous_cycle_id, amount, created_at)
    values (v_user_id, v_credit->>'previous_cycle_id', (v_credit->>'amount')::bigint,
      (v_credit->>'created_at_utc')::timestamptz)
    on conflict (user_id, previous_cycle_id) do nothing;
  end loop;

  for v_object in select value from jsonb_array_elements(p_state->'objects')
  loop
    if jsonb_typeof(v_object) <> 'object'
      or (select array_agg(k.key order by k.key) from jsonb_object_keys(v_object) k(key))
        is distinct from array['kind', 'ordinal', 'seed', 'stage', 'x', 'y']
      or jsonb_typeof(v_object->'stage') <> 'number'
      or (v_object->>'stage')::integer not between 0 and 4
      or jsonb_typeof(v_object->'ordinal') <> 'number'
      or (v_object->>'ordinal')::integer < 0
      or jsonb_typeof(v_object->'kind') <> 'string'
      or v_object->>'kind' not in (
        'rock', 'water', 'tree', 'fern', 'creature', 'camp', 'crops', 'cottage', 'path', 'well',
        'house', 'workshop', 'plaza', 'road', 'market', 'factory', 'power', 'rail', 'tower',
        'district', 'laboratory', 'satellite', 'rocket', 'solar', 'habitat'
      )
      or not case (v_object->>'stage')::integer
        when 0 then v_object->>'kind' in ('rock', 'water', 'tree', 'fern', 'creature')
        when 1 then v_object->>'kind' in ('camp', 'crops', 'cottage', 'path', 'well')
        when 2 then v_object->>'kind' in ('house', 'workshop', 'plaza', 'road', 'market')
        when 3 then v_object->>'kind' in ('factory', 'power', 'rail', 'tower', 'district')
        when 4 then v_object->>'kind' in ('laboratory', 'satellite', 'rocket', 'solar', 'habitat')
        else false
      end
      or jsonb_typeof(v_object->'x') <> 'number'
      or jsonb_typeof(v_object->'y') <> 'number'
      or jsonb_typeof(v_object->'seed') <> 'number'
      or (v_object->>'x')::integer not between 0 and 100
      or (v_object->>'y')::integer not between 0 and 100
    then
      raise exception 'planet object fields are invalid' using errcode = '23514';
    end if;
  end loop;

  select * into v_old from public.planet_member_state p where p.user_id = v_user_id for update;
  if found and v_timezone <> v_old.timezone then
    raise exception 'planet timezone differs from the account setting' using errcode = '23514';
  end if;
  if found and v_old.last_reset_at is not null
    and v_last_reset_at is distinct from v_old.last_reset_at
    and (v_last_reset_at is null or v_last_reset_at < v_old.last_reset_at + interval '24 hours')
  then
    raise exception 'planet reset cooldown has not elapsed' using errcode = '23514';
  end if;

  insert into public.planet_member_state(
    user_id, state_version, nickname, avatar, timezone, current_cycle_id, cycle_started_at, last_reset_at,
    current_planet_tokens, lifetime_tokens, growth_credit, stage, progress_to_next,
    incomplete, objects, updated_at
  ) values (
    v_user_id, 1, v_nickname, v_avatar, v_timezone, v_cycle_id, v_cycle_started_at, v_last_reset_at,
    (p_state->>'current_planet_tokens')::bigint, (p_state->>'lifetime_tokens')::bigint,
    (p_state->>'growth_credit')::numeric, (p_state->>'stage')::smallint,
    (p_state->>'progress_to_next')::numeric, (p_state->>'incomplete')::boolean,
    p_state->'objects', now()
  )
  on conflict (user_id) do update set
    nickname = excluded.nickname,
    avatar = excluded.avatar,
    timezone = planet_member_state.timezone,
    current_cycle_id = excluded.current_cycle_id,
    cycle_started_at = excluded.cycle_started_at,
    last_reset_at = excluded.last_reset_at,
    current_planet_tokens = case when planet_member_state.current_cycle_id = excluded.current_cycle_id
      then greatest(planet_member_state.current_planet_tokens, excluded.current_planet_tokens)
      else excluded.current_planet_tokens end,
    lifetime_tokens = greatest(planet_member_state.lifetime_tokens, excluded.lifetime_tokens),
    growth_credit = case when planet_member_state.current_cycle_id = excluded.current_cycle_id
      then greatest(planet_member_state.growth_credit, excluded.growth_credit)
      else excluded.growth_credit end,
    stage = case when planet_member_state.current_cycle_id = excluded.current_cycle_id
      then greatest(planet_member_state.stage, excluded.stage)
      else excluded.stage end,
    progress_to_next = case when planet_member_state.current_cycle_id = excluded.current_cycle_id
      then greatest(planet_member_state.progress_to_next, excluded.progress_to_next)
      else excluded.progress_to_next end,
    incomplete = planet_member_state.incomplete or excluded.incomplete,
    shared_visible = planet_member_state.shared_visible,
    objects = case when planet_member_state.current_cycle_id = excluded.current_cycle_id then (
      select coalesce(jsonb_agg(objects.value order by (objects.value->>'stage')::integer, (objects.value->>'ordinal')::integer), '[]'::jsonb)
      from (select distinct value from jsonb_array_elements(planet_member_state.objects || excluded.objects)) objects
    ) else excluded.objects end,
    updated_at = now()
  where planet_member_state.last_reset_at is null
    or excluded.last_reset_at >= planet_member_state.last_reset_at + interval '24 hours'
    or (planet_member_state.current_cycle_id = excluded.current_cycle_id
      and planet_member_state.last_reset_at is not distinct from excluded.last_reset_at);

  select p.current_cycle_id into v_server_cycle_id
  from public.planet_member_state p where p.user_id = v_user_id;

  insert into private.planet_device_state(
    user_id, device_id, current_cycle_id, lifetime_tokens, current_planet_tokens,
    daily_tokens, incomplete, updated_at
  ) values (
    v_user_id, v_device_id, v_device_cycle_id,
    (v_device->>'lifetime_tokens')::bigint,
    case when v_device_cycle_id = v_server_cycle_id then (v_device->>'current_planet_tokens')::bigint else 0 end,
    case when v_device_cycle_id = v_server_cycle_id then v_device->'daily_tokens' else '{}'::jsonb end,
    (v_device->>'incomplete')::boolean,
    now()
  )
  on conflict (user_id, device_id) do update set
    lifetime_tokens = greatest(planet_device_state.lifetime_tokens, excluded.lifetime_tokens),
    current_cycle_id = case when excluded.current_cycle_id = v_server_cycle_id
      then excluded.current_cycle_id else planet_device_state.current_cycle_id end,
    current_planet_tokens = case when excluded.current_cycle_id = v_server_cycle_id
      then excluded.current_planet_tokens else planet_device_state.current_planet_tokens end,
    daily_tokens = case when excluded.current_cycle_id = v_server_cycle_id
      then excluded.daily_tokens else planet_device_state.daily_tokens end,
    incomplete = case when excluded.current_cycle_id = v_server_cycle_id
      then excluded.incomplete else planet_device_state.incomplete end,
    updated_at = now();

  select coalesce(sum(log(2::numeric, 1::numeric + daily.total_tokens / 100000::numeric)), 0)
  into v_daily_growth
  from (
    select daily.day, sum(daily.tokens::numeric) as total_tokens
    from private.planet_device_state d
    cross join lateral jsonb_each_text(d.daily_tokens) daily(day, tokens)
    where d.user_id = v_user_id and d.current_cycle_id = v_server_cycle_id
    group by daily.day
  ) daily;
  v_stage := case
    when v_daily_growth >= 100 then 4
    when v_daily_growth >= 50 then 3
    when v_daily_growth >= 20 then 2
    when v_daily_growth >= 5 then 1
    else 0
  end;
  v_progress := case v_stage
    when 0 then v_daily_growth / 5
    when 1 then (v_daily_growth - 5) / 15
    when 2 then (v_daily_growth - 20) / 30
    when 3 then (v_daily_growth - 50) / 50
    else 1
  end;
  update public.planet_member_state p set
    lifetime_tokens = coalesce((
      select sum(d.lifetime_tokens) from private.planet_device_state d where d.user_id = v_user_id
    ), 0),
    current_planet_tokens = coalesce((
      select sum(d.current_planet_tokens) from private.planet_device_state d
      where d.user_id = v_user_id and d.current_cycle_id = v_server_cycle_id
    ), 0),
    growth_credit = v_daily_growth,
    stage = v_stage,
    progress_to_next = v_progress,
    incomplete = coalesce((
      select bool_or(d.incomplete) from private.planet_device_state d
      where d.user_id = v_user_id and d.current_cycle_id = v_server_cycle_id
    ), true),
    updated_at = now()
  where p.user_id = v_user_id;

  return private.planet_state_json(v_user_id);
end;
$$;

create function public.upsert_my_planet_state(p_state jsonb, p_device_contribution jsonb)
returns jsonb
language sql security invoker set search_path = '' as $$
  select private.upsert_my_planet_state(p_state, p_device_contribution);
$$;

create function private.get_world_planets(p_world_id uuid)
returns table (
  nickname text, avatar text, stage smallint, current_planet_tokens bigint,
  lifetime_tokens bigint, growth_credit numeric, progress_to_next numeric,
  incomplete boolean, objects jsonb, token_rank smallint, civilization_rank smallint
)
language plpgsql security definer set search_path = '' as $$
begin
  if (select auth.uid()) is null or not exists (
    select 1 from public.world_members m
    where m.world_id = p_world_id and m.user_id = (select auth.uid())
  ) then
    raise exception 'world access denied' using errcode = '42501';
  end if;
  return query
    with members as (
      select m.joined_at,
        coalesce(p.nickname, '행성 동기화 대기') as nickname,
        coalesce(p.avatar, 'masculine') as avatar,
        coalesce(p.stage, 0)::smallint as stage,
        coalesce(p.current_planet_tokens, 0)::bigint as current_planet_tokens,
        coalesce(p.lifetime_tokens, 0)::bigint as lifetime_tokens,
        coalesce(p.growth_credit, 0)::numeric as growth_credit,
        coalesce(p.progress_to_next, 0)::numeric as progress_to_next,
        coalesce(p.incomplete, true) as incomplete,
        coalesce(p.objects, '[]'::jsonb) as objects
      from public.world_members m
      left join public.planet_member_state p on p.user_id = m.user_id and p.shared_visible
      where m.world_id = p_world_id
    ), ranked as (
      select members.*,
        rank() over (order by lifetime_tokens desc)::smallint as token_rank,
        rank() over (order by growth_credit desc)::smallint as civilization_rank
      from members
    )
    select ranked.nickname, ranked.avatar, ranked.stage, ranked.current_planet_tokens,
      ranked.lifetime_tokens, ranked.growth_credit, ranked.progress_to_next,
      ranked.incomplete, ranked.objects, ranked.token_rank, ranked.civilization_rank
    from ranked order by ranked.joined_at, ranked.nickname;
end;
$$;

create or replace function private.delete_synced_usage(p_world_id uuid) returns integer
language plpgsql security definer set search_path = '' as $$
declare
  v_deleted integer;
  v_timezone text;
  v_cutoff date;
begin
  select w.timezone into v_timezone from public.worlds w
  where w.id = p_world_id for update;
  if (select auth.uid()) is null or v_timezone is null or not exists (
    select 1 from public.world_members m
    where m.world_id = p_world_id and m.user_id = (select auth.uid())
  ) then
    raise exception 'world access denied' using errcode = '42501';
  end if;
  v_cutoff := (now() at time zone v_timezone)::date;
  insert into private.member_sync_policy(world_id, user_id, deleted_through, paused)
  values (p_world_id, (select auth.uid()), v_cutoff, true)
  on conflict (world_id, user_id) do update set
    deleted_through = greatest(private.member_sync_policy.deleted_through, excluded.deleted_through),
    paused = true;
  update public.planet_member_state set shared_visible = false, updated_at = now()
  where user_id = (select auth.uid());
  delete from public.daily_usage_snapshots s
  where s.world_id = p_world_id and s.user_id = (select auth.uid());
  get diagnostics v_deleted = row_count;
  return v_deleted;
end;
$$;

create or replace function private.resume_my_sync(p_world_id uuid) returns boolean
language plpgsql security definer set search_path = '' as $$
begin
  if (select auth.uid()) is null or not exists (
    select 1 from public.world_members m
    where m.world_id = p_world_id and m.user_id = (select auth.uid())
  ) then
    raise exception 'world access denied' using errcode = '42501';
  end if;
  update private.member_sync_policy p set paused = false
  where p.world_id = p_world_id and p.user_id = (select auth.uid());
  update public.planet_member_state set shared_visible = true, updated_at = now()
  where user_id = (select auth.uid());
  return true;
end;
$$;

create function public.get_world_planets(p_world_id uuid)
returns table (
  nickname text, avatar text, stage smallint, current_planet_tokens bigint,
  lifetime_tokens bigint, growth_credit numeric, progress_to_next numeric,
  incomplete boolean, objects jsonb, token_rank smallint, civilization_rank smallint
)
language sql security invoker set search_path = '' as $$
  select * from private.get_world_planets(p_world_id);
$$;

create or replace function private.compute_world_summary(p_world_id uuid)
returns table (
  world_id uuid, member_count integer, known_tokens numeric, growth_credit numeric,
  stage smallint, progress_to_next numeric, incomplete boolean, last_update timestamptz
)
language plpgsql security definer set search_path = '' as $$
begin
  if (select auth.uid()) is null or not exists (
    select 1 from public.world_members m
    where m.world_id = p_world_id and m.user_id = (select auth.uid())
  ) then
    raise exception 'world access denied' using errcode = '42501';
  end if;
  return query
    select p_world_id,
      count(m.user_id)::integer,
      sum(s.lifetime_tokens)::numeric,
      coalesce(sum(s.growth_credit), 0)::numeric,
      coalesce(max(s.stage), 0)::smallint,
      coalesce(max(s.progress_to_next), 0)::numeric,
      bool_or(s.user_id is null or s.incomplete),
      max(s.updated_at)
    from public.world_members m
    left join public.planet_member_state s on s.user_id = m.user_id
    where m.world_id = p_world_id;
end;
$$;

drop function public.get_world_summary(uuid);

revoke all on function private.planet_state_json(uuid), private.get_my_planet_state(),
  private.upsert_my_planet_state(jsonb, jsonb), private.get_world_planets(uuid),
  public.get_my_planet_state(), public.upsert_my_planet_state(jsonb, jsonb), public.get_world_planets(uuid)
  from public, anon;
grant execute on function private.get_my_planet_state(), private.upsert_my_planet_state(jsonb, jsonb),
  private.get_world_planets(uuid), public.get_my_planet_state(),
  public.upsert_my_planet_state(jsonb, jsonb), public.get_world_planets(uuid) to authenticated;
