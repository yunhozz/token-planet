-- Secure reset state is kept separate from client-provided planet snapshots.
alter table public.planet_member_state
  add column reset_available_at timestamptz,
  add column reset_cooldown_bps smallint not null default 0
    check (reset_cooldown_bps between 0 and 2500);

update public.planet_member_state p
set reset_available_at = p.last_reset_at + interval '24 hours'
where p.last_reset_at is not null;

alter table private.shop_cycle_effect_baseline
  add column ended_at timestamptz,
  add constraint shop_cycle_effect_baseline_bounds_check
    check (ended_at is null or ended_at >= started_at);

create table private.shop_reset_request (
  user_id uuid not null references auth.users(id) on delete cascade,
  request_id uuid not null,
  expected_cycle_id text not null check (char_length(expected_cycle_id) between 1 and 80),
  status text not null check (status in ('reset', 'cycle_mismatch')),
  created_at timestamptz not null default now(),
  primary key (user_id, request_id)
);

create table private.shop_cycle_token_settlement (
  user_id uuid not null references auth.users(id) on delete cascade,
  cycle_id text not null check (char_length(cycle_id) between 1 and 80),
  next_cycle_id text not null check (char_length(next_cycle_id) between 1 and 80),
  request_id uuid not null,
  cycle_started_at timestamptz not null,
  last_reset_at timestamptz,
  raw_tokens bigint not null check (raw_tokens >= 0),
  bonus_tokens bigint not null check (bonus_tokens >= 0),
  effect_revision bigint not null check (effect_revision >= 0),
  reset_cooldown_bps smallint not null check (reset_cooldown_bps between 0 and 2500),
  reset_available_at timestamptz not null,
  effect_snapshot jsonb not null check (jsonb_typeof(effect_snapshot) = 'object'),
  settled_at timestamptz not null,
  primary key (user_id, cycle_id),
  unique (user_id, next_cycle_id),
  unique (user_id, request_id)
);

alter table private.shop_reset_request enable row level security;
alter table private.shop_cycle_token_settlement enable row level security;
revoke all on private.shop_reset_request, private.shop_cycle_token_settlement
  from public, anon, authenticated, service_role;

create function private.record_shop_cycle_token_settlement(
  p_user_id uuid,
  p_cycle_id text,
  p_next_cycle_id text,
  p_request_id uuid,
  p_settled_at timestamptz,
  p_reset_cooldown_bps smallint,
  p_reset_available_at timestamptz,
  p_effect_snapshot jsonb
)
returns bigint
language plpgsql security definer set search_path = '' as $$
declare
  v_server_cycle_id text;
  v_cycle_started_at timestamptz;
  v_last_reset_at timestamptz;
  v_raw_tokens bigint;
  v_bonus_tokens bigint;
  v_effect_revision bigint;
begin
  if p_user_id is null or p_user_id is distinct from (select auth.uid()) then
    raise exception 'cycle settlement account access denied' using errcode = '42501';
  end if;
  if p_cycle_id is null or char_length(p_cycle_id) not between 1 and 80
    or p_next_cycle_id is null or char_length(p_next_cycle_id) not between 1 and 80
    or p_request_id is null or p_settled_at is null
    or p_reset_cooldown_bps not between 0 and 2500
    or p_reset_available_at is null
    or jsonb_typeof(p_effect_snapshot) is distinct from 'object'
  then
    raise exception 'cycle settlement scope is invalid' using errcode = '22023';
  end if;

  perform private.lock_shop_account(p_user_id);
  select p.current_cycle_id, p.cycle_started_at, p.last_reset_at,
    p.current_planet_tokens
  into v_server_cycle_id, v_cycle_started_at, v_last_reset_at, v_raw_tokens
  from public.planet_member_state p
  where p.user_id = p_user_id
  for update;
  if not found or v_server_cycle_id is distinct from p_cycle_id then
    raise exception 'cycle settlement is not current' using errcode = '23514';
  end if;
  if exists (
    select 1 from private.planet_wallet_credits w
    where w.user_id = p_user_id and w.previous_cycle_id = p_cycle_id
  ) or exists (
    select 1 from private.shop_cycle_token_settlement s
    where s.user_id = p_user_id and s.cycle_id = p_cycle_id
  ) then
    raise exception 'current cycle already has a settlement record' using errcode = '23514';
  end if;

  v_bonus_tokens := private.shop_cycle_token_bonus(p_user_id, p_cycle_id);
  select coalesce(max(h.revision), 0)
  into v_effect_revision
  from private.shop_effect_history h
  where h.user_id = p_user_id and h.cycle_id = p_cycle_id;

  insert into private.shop_cycle_token_settlement(
    user_id, cycle_id, next_cycle_id, request_id, cycle_started_at,
    last_reset_at, raw_tokens, bonus_tokens, effect_revision,
    reset_cooldown_bps, reset_available_at, effect_snapshot, settled_at
  ) values (
    p_user_id, p_cycle_id, p_next_cycle_id, p_request_id,
    v_cycle_started_at, v_last_reset_at, v_raw_tokens, v_bonus_tokens,
    v_effect_revision, p_reset_cooldown_bps, p_reset_available_at,
    p_effect_snapshot, p_settled_at
  );
  return v_bonus_tokens;
end;
$$;
revoke all on function private.record_shop_cycle_token_settlement(
  uuid, text, text, uuid, timestamptz, smallint, timestamptz, jsonb
)
  from public, anon, authenticated, service_role;

-- The caller holds this per-account lock before it reads the planet row or a
-- reset receipt. Capture the reset clock only after both locks are acquired.
create function private.lock_shop_reset_request(
  p_request_id uuid,
  p_expected_cycle_id text
)
returns table (
  current_cycle_id text,
  cycle_started_at timestamptz,
  last_reset_at timestamptz,
  current_planet_tokens bigint,
  lifetime_tokens bigint,
  reset_available_at timestamptz,
  prior_status text,
  prior_expected_cycle_id text,
  server_time timestamptz
)
language plpgsql security definer set search_path = '' as $$
declare
  v_user_id uuid := (select auth.uid());
begin
  if v_user_id is null then
    raise exception 'authentication required' using errcode = '42501';
  end if;
  if p_request_id is null or p_expected_cycle_id is null
    or char_length(p_expected_cycle_id) not between 1 and 80
    or btrim(p_expected_cycle_id) = ''
  then
    raise exception 'reset request scope is invalid' using errcode = '22023';
  end if;

  perform private.lock_shop_account(v_user_id);
  select p.current_cycle_id, p.cycle_started_at, p.last_reset_at,
    p.current_planet_tokens, p.lifetime_tokens, p.reset_available_at
  into current_cycle_id, cycle_started_at, last_reset_at,
    current_planet_tokens, lifetime_tokens, reset_available_at
  from public.planet_member_state p
  where p.user_id = v_user_id
  for update;
  if not found then
    raise exception 'planet state is unavailable' using errcode = 'P0002';
  end if;

  select r.status, r.expected_cycle_id
  into prior_status, prior_expected_cycle_id
  from private.shop_reset_request r
  where r.user_id = v_user_id and r.request_id = p_request_id;
  server_time := clock_timestamp();
  return next;
end;
$$;
revoke all on function private.lock_shop_reset_request(uuid, text)
  from public, anon, authenticated, service_role;

create or replace function private.planet_state_json(p_user_id uuid)
returns jsonb
language sql security definer set search_path = '' as $$
  select jsonb_build_object(
    'version', p.state_version,
    'profile', jsonb_build_object('nickname', p.nickname, 'avatar', p.avatar),
    'timezone', p.timezone,
    'current_cycle_id', p.current_cycle_id,
    'cycle_started_at_utc', to_char(p.cycle_started_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
    'last_reset_at_utc', case when p.last_reset_at is null then null else to_char(p.last_reset_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"') end,
    'wallet_balance', private.shop_available_balance(p_user_id),
    'wallet_credits', coalesce((
      select jsonb_agg(jsonb_build_object(
        'previous_cycle_id', w.previous_cycle_id,
        'amount', w.amount,
        'created_at_utc', to_char(w.created_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
      ) order by w.created_at, w.previous_cycle_id)
      from private.planet_wallet_credits w where w.user_id = p.user_id
    ), '[]'::jsonb),
    'current_planet_tokens', p.current_planet_tokens,
    'lifetime_tokens', p.lifetime_tokens,
    'growth_credit', p.growth_credit,
    'stage', p.stage,
    'progress_to_next', p.progress_to_next,
    'incomplete', p.incomplete,
    'can_reset', p.reset_available_at is null or statement_timestamp() >= p.reset_available_at,
    'reset_available_at_utc', case when p.reset_available_at is null then null else to_char(p.reset_available_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"') end,
    'objects', p.objects
  )
  from public.planet_member_state p where p.user_id = p_user_id;
$$;
revoke all on function private.planet_state_json(uuid)
  from public, anon, authenticated, service_role;

create or replace function private.shop_reward_state_json(p_user_id uuid)
returns jsonb
language plpgsql security definer set search_path = '' as $$
declare
  v_reward_timezone text;
  v_cycle_id text;
  v_settled_cycle_tokens numeric := 0;
  v_era_reward_tokens numeric := 0;
  v_streak_reward_tokens numeric := 0;
begin
  select s.reward_timezone into v_reward_timezone
  from private.shop_account_state s where s.user_id = p_user_id;
  select p.current_cycle_id into v_cycle_id
  from public.planet_member_state p where p.user_id = p_user_id;

  select coalesce(sum(s.raw_tokens::numeric + s.bonus_tokens::numeric), 0::numeric)
  into v_settled_cycle_tokens
  from private.shop_cycle_token_settlement s
  where s.user_id = p_user_id and s.cycle_id = v_cycle_id;
  select coalesce(sum(r.amount::numeric) filter (
      where r.kind = 'era' and r.cycle_id = v_cycle_id
    ), 0::numeric),
    coalesce(sum(r.amount::numeric) filter (where r.kind = 'streak'), 0::numeric)
  into v_era_reward_tokens, v_streak_reward_tokens
  from private.shop_game_reward r where r.user_id = p_user_id;

  if greatest(v_settled_cycle_tokens, v_era_reward_tokens, v_streak_reward_tokens)
    > 18446744073709551615::numeric
  then
    raise exception 'reward state exceeds client integer range' using errcode = '22003';
  end if;
  return jsonb_build_object(
    'reward_timezone', coalesce(v_reward_timezone, 'UTC'),
    'settled_cycle_tokens', v_settled_cycle_tokens,
    'era_reward_tokens', v_era_reward_tokens,
    'streak_reward_tokens', v_streak_reward_tokens
  );
end;
$$;
revoke all on function private.shop_reward_state_json(uuid)
  from public, anon, authenticated, service_role;

create function public.reset_my_planet(p_request_id uuid, p_cycle_id text)
returns jsonb
language plpgsql security definer set search_path = '' as $$
declare
  v_user_id uuid := (select auth.uid());
  v_lock record;
  v_status text;
  v_server_time timestamptz;
  v_new_cycle_id text;
  v_effects jsonb;
  v_new_effects jsonb;
  v_reset_cooldown_bps smallint;
  v_cooldown_seconds integer;
  v_reset_available_at timestamptz;
  v_bonus_tokens bigint;
  v_credit_amount numeric;
  v_effect_revision bigint;
  v_state jsonb;
  v_action jsonb;
begin
  if v_user_id is null then
    raise exception 'authentication required' using errcode = '42501';
  end if;

  select * into v_lock
  from private.lock_shop_reset_request(p_request_id, p_cycle_id);
  if v_lock.prior_status is not null then
    v_status := case
      when v_lock.prior_expected_cycle_id is distinct from p_cycle_id then 'request_conflict'
      else v_lock.prior_status
    end;
    v_action := jsonb_build_object(
      'status', v_status,
      'request_id', p_request_id::text,
      'confirmed_quote', null,
      'state', private.shop_state_json(v_user_id)
    );
    return jsonb_build_object(
      'action', v_action,
      'planet_state', private.planet_state_json(v_user_id)
    );
  end if;

  if v_lock.current_cycle_id is distinct from p_cycle_id then
    insert into private.shop_reset_request(user_id, request_id, expected_cycle_id, status)
    values (v_user_id, p_request_id, p_cycle_id, 'cycle_mismatch');
    v_action := jsonb_build_object(
      'status', 'cycle_mismatch',
      'request_id', p_request_id::text,
      'confirmed_quote', null,
      'state', private.shop_state_json(v_user_id)
    );
    return jsonb_build_object(
      'action', v_action,
      'planet_state', private.planet_state_json(v_user_id)
    );
  end if;

  if v_lock.reset_available_at is not null
    and v_lock.server_time < v_lock.reset_available_at
  then
    raise exception 'planet reset cooldown is active' using errcode = '55000';
  end if;

  perform private.settle_shop_rewards(v_user_id, p_cycle_id);
  v_server_time := clock_timestamp();
  v_new_cycle_id := pg_catalog.gen_random_uuid()::text;
  v_effects := private.shop_active_effects(v_user_id, p_cycle_id);
  v_reset_cooldown_bps := least(greatest(
    coalesce((v_effects->>'reset_cooldown_bps')::integer, 0), 0
  ), 2500)::smallint;
  v_cooldown_seconds := greatest(
    64800,
    floor((86400::numeric * (10000 - v_reset_cooldown_bps)) / 10000)::integer
  );
  v_reset_available_at := v_server_time + pg_catalog.make_interval(secs => v_cooldown_seconds);
  v_bonus_tokens := private.record_shop_cycle_token_settlement(
    v_user_id, p_cycle_id, v_new_cycle_id, p_request_id,
    v_server_time, v_reset_cooldown_bps, v_reset_available_at, v_effects
  );
  v_credit_amount := v_lock.current_planet_tokens::numeric + v_bonus_tokens::numeric;
  if v_credit_amount > 9223372036854775807::numeric then
    raise exception 'cycle wallet credit exceeds bigint range' using errcode = '22003';
  end if;

  insert into private.planet_wallet_credits(user_id, previous_cycle_id, amount, created_at)
  values (v_user_id, p_cycle_id, v_credit_amount::bigint, v_server_time);

  update private.shop_landscape_instance i
  set placement_version = i.placement_version + 1
  where i.user_id = v_user_id and exists (
    select 1 from private.shop_landscape_placement pl
    where pl.user_id = i.user_id and pl.instance_id = i.instance_id
      and pl.cycle_id = p_cycle_id
  );
  delete from private.shop_landscape_placement pl
  where pl.user_id = v_user_id and pl.cycle_id = p_cycle_id;

  insert into private.shop_cycle_effect_baseline as prior_baseline(
    user_id, cycle_id, started_at, ended_at
  )
  values (v_user_id, p_cycle_id, v_lock.cycle_started_at, v_server_time)
  on conflict (user_id, cycle_id) do update
    set ended_at = coalesce(prior_baseline.ended_at, excluded.ended_at);
  update private.shop_cycle_effect_baseline b
  set ended_at = coalesce(b.ended_at, v_server_time)
  where b.user_id = v_user_id and b.cycle_id = p_cycle_id;
  update private.shop_effect_history h
  set ended_at = v_server_time
  where h.user_id = v_user_id and h.cycle_id = p_cycle_id and h.ended_at is null;

  select coalesce(max(h.revision), 0) into v_effect_revision
  from private.shop_effect_history h where h.user_id = v_user_id;
  if v_effect_revision = 9223372036854775807 then
    raise exception 'effect revision exhausted' using errcode = '22003';
  end if;
  v_effect_revision := v_effect_revision + 1;
  update public.planet_member_state p
  set current_cycle_id = v_new_cycle_id,
      cycle_started_at = v_server_time,
      last_reset_at = v_server_time,
      reset_available_at = v_reset_available_at,
      reset_cooldown_bps = v_reset_cooldown_bps,
      current_planet_tokens = 0,
      growth_credit = 0,
      stage = 0,
      progress_to_next = 0,
      incomplete = false,
      objects = '[]'::jsonb,
      updated_at = v_server_time
  where p.user_id = v_user_id;
  update private.planet_device_state d
  set current_cycle_id = v_new_cycle_id,
      current_planet_tokens = 0,
      daily_tokens = '{}'::jsonb,
      incomplete = false,
      updated_at = v_server_time
  where d.user_id = v_user_id;

  insert into private.shop_cycle_effect_baseline(user_id, cycle_id, started_at)
  values (v_user_id, v_new_cycle_id, v_server_time);
  insert into private.shop_planet_object_generation_baseline(user_id, cycle_id, initialized_at)
  values (v_user_id, v_new_cycle_id, v_server_time);
  v_new_effects := private.shop_active_effects(v_user_id, v_new_cycle_id);
  insert into private.shop_effect_history(
    user_id, cycle_id, revision, started_at, ended_at, active_instance_ids, effects
  ) values (
    v_user_id, v_new_cycle_id, v_effect_revision, v_server_time, null, '[]'::jsonb, v_new_effects
  );

  update private.shop_account_state s
  set state_revision = s.state_revision + 1, updated_at = v_server_time
  where s.user_id = v_user_id;
  if not found then
    raise exception 'shop account state is unavailable' using errcode = '23503';
  end if;

  insert into private.shop_reset_request(user_id, request_id, expected_cycle_id, status)
  values (v_user_id, p_request_id, p_cycle_id, 'reset');
  v_action := jsonb_build_object(
    'status', 'reset',
    'request_id', p_request_id::text,
    'confirmed_quote', null,
    'state', private.shop_state_json(v_user_id)
  );
  v_state := private.planet_state_json(v_user_id);
  return jsonb_build_object('action', v_action, 'planet_state', v_state);
end;
$$;
revoke all on function public.reset_my_planet(uuid, text)
  from public, anon, service_role;
grant execute on function public.reset_my_planet(uuid, text) to authenticated;

alter function private.upsert_my_planet_state(jsonb, jsonb)
  rename to upsert_my_planet_state_before_secure_reset;
revoke all on function private.upsert_my_planet_state_before_secure_reset(jsonb, jsonb)
  from public, anon, authenticated, service_role;
-- These renamed entry points are still needed by the guarded definer wrapper,
-- but authenticated clients must not call them around the cycle guard.
revoke all on function private.upsert_my_planet_state_before_reward_delivery(jsonb, jsonb)
  from public, anon, authenticated, service_role;
revoke all on function private.upsert_my_planet_state_legacy(jsonb, jsonb)
  from public, anon, authenticated, service_role;

create function private.upsert_my_planet_state(p_state jsonb, p_device_contribution jsonb)
returns jsonb
language plpgsql security definer set search_path = '' as $$
declare
  v_user_id uuid := (select auth.uid());
  v_current_cycle_id text;
  v_incoming_cycle_id text;
  v_device_id uuid;
  v_state jsonb;
  v_device jsonb;
  v_device_lifetime bigint;
  v_device_current bigint := 0;
  v_device_daily jsonb := '{}'::jsonb;
  v_device_incomplete boolean := false;
  v_growth numeric;
  v_stage smallint;
  v_progress numeric;
  v_objects jsonb;
begin
  if v_user_id is null then
    raise exception 'authentication required' using errcode = '42501';
  end if;
  perform private.lock_shop_account(v_user_id);

  select p.current_cycle_id into v_current_cycle_id
  from public.planet_member_state p where p.user_id = v_user_id for update;
  if not found then
    return private.upsert_my_planet_state_before_secure_reset(p_state, p_device_contribution);
  end if;
  if jsonb_typeof(p_state) is distinct from 'object'
    or jsonb_typeof(p_device_contribution) is distinct from 'object'
    or jsonb_typeof(p_state->'current_cycle_id') is distinct from 'string'
  then
    raise exception 'planet cycle is invalid' using errcode = '23514';
  end if;
  v_incoming_cycle_id := p_state->>'current_cycle_id';

  if v_incoming_cycle_id = v_current_cycle_id then
    select jsonb_set(
      jsonb_set(
        jsonb_set(
          jsonb_set(p_state, '{cycle_started_at_utc}',
            to_jsonb(to_char(p.cycle_started_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')), true),
          '{last_reset_at_utc}', coalesce(
            to_jsonb(to_char(p.last_reset_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')),
            'null'::jsonb
          ), true
        ),
        '{reset_available_at_utc}', coalesce(
          to_jsonb(to_char(p.reset_available_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')),
          'null'::jsonb
        ), true
      ),
      '{current_cycle_id}', to_jsonb(p.current_cycle_id), true
    ) into v_state
    from public.planet_member_state p where p.user_id = v_user_id;
    return private.upsert_my_planet_state_before_secure_reset(v_state, p_device_contribution);
  end if;

  if not exists (
    select 1 from private.shop_cycle_token_settlement s
    where s.user_id = v_user_id and s.cycle_id = v_incoming_cycle_id
  ) then
    raise exception 'planet cycle can only change through reset_my_planet' using errcode = '23514';
  end if;

  -- Closed-cycle uploads can advance lifetime raw usage, but never replace the
  -- current planet, contribution snapshot, effects, or rewards.
  perform private.shop_validate_effect_upload(v_user_id, p_state, p_device_contribution);
  v_device_id := (p_device_contribution->>'device_id')::uuid;
  v_device_lifetime := (p_device_contribution->>'lifetime_tokens')::bigint;
  select d.lifetime_tokens, d.current_planet_tokens, d.daily_tokens, d.incomplete
  into v_device_lifetime, v_device_current, v_device_daily, v_device_incomplete
  from private.planet_device_state d
  where d.user_id = v_user_id and d.device_id = v_device_id
    and d.current_cycle_id = v_current_cycle_id;
  if not found then
    v_device_current := 0;
    v_device_daily := '{}'::jsonb;
    v_device_incomplete := false;
  end if;
  v_device_lifetime := greatest(
    coalesce(v_device_lifetime, 0),
    (p_device_contribution->>'lifetime_tokens')::bigint
  );
  v_device := jsonb_build_object(
    'device_id', v_device_id::text,
    'current_cycle_id', v_current_cycle_id,
    'lifetime_tokens', v_device_lifetime,
    'current_planet_tokens', v_device_current,
    'daily_tokens', v_device_daily,
    'incomplete', v_device_incomplete
  );
  -- Later projection wrappers may add server-only fields that the legacy
  -- updater does not accept as input. Keep the persisted DTO intact while
  -- passing only its legacy contract through this internal compatibility path.
  v_state := private.planet_state_json(v_user_id) - 'removed_natural_keys';
  perform private.upsert_my_planet_state_legacy(v_state, v_device);

  v_growth := private.shop_weighted_growth(v_user_id, v_current_cycle_id);
  v_stage := case
    when v_growth >= 100 then 4
    when v_growth >= 50 then 3
    when v_growth >= 20 then 2
    when v_growth >= 5 then 1
    else 0
  end;
  v_progress := case v_stage
    when 0 then v_growth / 5
    when 1 then (v_growth - 5) / 15
    when 2 then (v_growth - 20) / 30
    when 3 then (v_growth - 50) / 50
    else 1
  end;
  select p.objects into v_objects
  from public.planet_member_state p where p.user_id = v_user_id;
  v_objects := private.shop_project_planet_objects(v_current_cycle_id, v_objects, v_growth);
  update public.planet_member_state p set
    growth_credit = v_growth,
    stage = v_stage,
    progress_to_next = v_progress,
    objects = v_objects,
    updated_at = clock_timestamp()
  where p.user_id = v_user_id and p.current_cycle_id = v_current_cycle_id;
  return private.planet_state_json(v_user_id);
end;
$$;
revoke all on function private.upsert_my_planet_state(jsonb, jsonb)
  from public, anon, service_role;
grant execute on function private.upsert_my_planet_state(jsonb, jsonb) to authenticated;

create or replace function public.upsert_my_planet_state(
  p_state jsonb, p_device_contribution jsonb
)
returns jsonb
language sql security invoker set search_path = '' as $$
  select private.upsert_my_planet_state(p_state, p_device_contribution);
$$;
revoke all on function public.upsert_my_planet_state(jsonb, jsonb)
  from public, anon, service_role;
grant execute on function public.upsert_my_planet_state(jsonb, jsonb) to authenticated;
