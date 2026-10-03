create function public.get_my_shop_effect_timeline()
returns jsonb
language plpgsql security definer set search_path = '' as $$
declare
  v_user_id uuid := (select auth.uid());
  v_current_cycle_id text;
  v_cycle_started_at timestamptz;
  v_reward_timezone text;
  v_server_time timestamptz;
  v_effect_revision bigint := 0;
  v_baseline_started_at timestamptz;
  v_baseline_ended_at timestamptz;
  v_cycle_bounds jsonb;
  v_intervals jsonb;
begin
  if v_user_id is null then
    raise exception 'authentication required' using errcode = '42501';
  end if;

  perform private.lock_shop_account(v_user_id);
  select p.current_cycle_id, p.cycle_started_at
  into v_current_cycle_id, v_cycle_started_at
  from public.planet_member_state p
  where p.user_id = v_user_id
  for update;
  if not found then
    raise exception 'planet state is unavailable' using errcode = '55000';
  end if;

  v_server_time := clock_timestamp();

  select s.reward_timezone into v_reward_timezone
  from private.shop_account_state s
  where s.user_id = v_user_id;
  if not found or v_reward_timezone is null then
    raise exception 'reward timezone is unavailable' using errcode = '23514';
  end if;

  -- Bootstrap only the current server-owned bound. Historical cycles without a
  -- recorded bound stay unknown; this read never reconstructs their dates.
  insert into private.shop_cycle_effect_baseline(user_id, cycle_id, started_at)
  values (v_user_id, v_current_cycle_id, v_cycle_started_at)
  on conflict (user_id, cycle_id) do nothing;
  select b.started_at, b.ended_at
  into v_baseline_started_at, v_baseline_ended_at
  from private.shop_cycle_effect_baseline b
  where b.user_id = v_user_id and b.cycle_id = v_current_cycle_id
  for update;
  if not found or v_baseline_started_at is distinct from v_cycle_started_at
    or v_baseline_ended_at is not null
  then
    raise exception 'current cycle effect bound is inconsistent' using errcode = '23514';
  end if;

  select coalesce(max(h.revision), 0)
  into v_effect_revision
  from private.shop_effect_history h
  where h.user_id = v_user_id;

  select coalesce(jsonb_agg(jsonb_build_object(
    'cycle_id', b.cycle_id,
    'started_at_utc', to_char(b.started_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
    'ended_at_utc', case when b.ended_at is null then null
      else to_char(b.ended_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"') end
  ) order by b.started_at, b.cycle_id), '[]'::jsonb)
  into v_cycle_bounds
  from private.shop_cycle_effect_baseline b
  where b.user_id = v_user_id
    -- Before the secure reset migration, uploads could switch cycles without
    -- recording the previous end. Keep those rows untouched and report only
    -- bounds with a server-recorded end plus the current server-owned bound.
    and (b.cycle_id = v_current_cycle_id or b.ended_at is not null);

  select coalesce(jsonb_agg(jsonb_build_object(
    'cycle_id', h.cycle_id,
    'revision', h.revision,
    'started_at_utc', to_char(h.started_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
    'ended_at_utc', case when h.ended_at is null then null
      else to_char(h.ended_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"') end,
    'active_instance_ids', h.active_instance_ids,
    'effects', h.effects
  ) order by h.revision, h.cycle_id), '[]'::jsonb)
  into v_intervals
  from private.shop_effect_history h
  where h.user_id = v_user_id
    -- A noncurrent open interval is ambiguous legacy state. Never let it act
    -- as current history or infer an end time; the database row is unchanged.
    and (h.cycle_id = v_current_cycle_id or h.ended_at is not null);

  return jsonb_build_object(
    'account_id', v_user_id::text,
    'current_cycle_id', v_current_cycle_id,
    'effect_revision', v_effect_revision,
    'server_time_utc', to_char(v_server_time at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
    'reward_timezone', v_reward_timezone,
    'cycle_bounds', v_cycle_bounds,
    'intervals', v_intervals
  );
end;
$$;
revoke all on function public.get_my_shop_effect_timeline()
  from public, anon, service_role;
grant execute on function public.get_my_shop_effect_timeline() to authenticated;
