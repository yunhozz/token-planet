-- The first canonical upload per account/cycle ignores legacy client objects and
-- projects from server-confirmed growth; its marker is written only after success.
create table private.shop_planet_object_generation_baseline (
  user_id uuid not null references auth.users(id) on delete cascade,
  cycle_id text not null check (char_length(cycle_id) between 1 and 80),
  initialized_at timestamptz not null default now(),
  primary key (user_id, cycle_id)
);
alter table private.shop_planet_object_generation_baseline enable row level security;
revoke all on private.shop_planet_object_generation_baseline
  from public, anon, authenticated, service_role;

create function private.shop_canonical_planet_object(
  p_cycle_id text, p_stage integer, p_ordinal integer
)
returns jsonb
language plpgsql immutable set search_path = '' as $$
declare
  v_hash bytea;
  v_seed numeric := 0;
  v_i integer;
  v_kinds text[];
  v_kind text;
begin
  if p_cycle_id is null or p_stage not between 0 and 4 or p_ordinal < 0 then
    raise exception 'planet object key is invalid' using errcode = '23514';
  end if;
  v_hash := extensions.digest(convert_to(
    p_cycle_id || ':' || p_stage::text || ':' || p_ordinal::text, 'UTF8'
  ), 'sha256');
  for v_i in 0..7 loop
    v_seed := v_seed * 256 + get_byte(v_hash, v_i)::numeric;
  end loop;
  v_kinds := case p_stage
    when 0 then array['rock', 'water', 'tree', 'fern', 'creature']
    when 1 then array['camp', 'crops', 'cottage', 'path', 'well']
    when 2 then array['house', 'workshop', 'plaza', 'road', 'market']
    when 3 then array['factory', 'power', 'rail', 'tower', 'district']
    else array['laboratory', 'satellite', 'rocket', 'solar', 'habitat']
  end;
  v_kind := v_kinds[mod(v_seed, array_length(v_kinds, 1)::numeric)::integer + 1];
  return jsonb_build_object(
    'stage', p_stage,
    'ordinal', p_ordinal,
    'kind', v_kind,
    'x', mod(v_seed, 88)::integer + 6,
    'y', mod(div(v_seed, 256::numeric), 52)::integer + 28,
    'seed', v_seed
  );
end;
$$;

revoke all on function private.shop_canonical_planet_object(text, integer, integer)
  from public, anon, authenticated, service_role;

create function private.shop_normalize_planet_objects(p_cycle_id text, p_objects jsonb)
returns jsonb
language plpgsql immutable set search_path = '' as $$
declare
  v_stage integer;
  v_object jsonb;
  v_stage_number numeric;
  v_ordinal_number numeric;
  v_ordinal integer;
  v_seen integer[];
  v_sorted integer[];
  v_count integer;
  v_result jsonb := '[]'::jsonb;
begin
  if jsonb_typeof(p_objects) is distinct from 'array' then
    return '[]'::jsonb;
  end if;

  for v_stage in 0..4 loop
    v_seen := array[]::integer[];
    for v_object in select value from jsonb_array_elements(p_objects)
    loop
      if jsonb_typeof(v_object) is distinct from 'object'
        or jsonb_typeof(v_object->'stage') is distinct from 'number'
        or jsonb_typeof(v_object->'ordinal') is distinct from 'number'
      then
        continue;
      end if;
      v_stage_number := (v_object->>'stage')::numeric;
      v_ordinal_number := (v_object->>'ordinal')::numeric;
      if v_stage_number <> v_stage or v_stage_number <> trunc(v_stage_number)
        or v_ordinal_number < 0 or v_ordinal_number > 2147483647::numeric
        or v_ordinal_number <> trunc(v_ordinal_number)
      then
        continue;
      end if;
      v_ordinal := v_ordinal_number::integer;
      if v_object = private.shop_canonical_planet_object(p_cycle_id, v_stage, v_ordinal)
        and not (v_ordinal = any(v_seen))
      then
        v_seen := array_append(v_seen, v_ordinal);
      end if;
    end loop;

    select coalesce(array_agg(o.ordinal order by o.ordinal), array[]::integer[])
    into v_sorted
    from unnest(v_seen) o(ordinal);
    v_count := 0;
    foreach v_ordinal in array v_sorted
    loop
      if v_ordinal <> v_count then
        exit;
      end if;
      v_result := v_result || jsonb_build_array(
        private.shop_canonical_planet_object(p_cycle_id, v_stage, v_ordinal)
      );
      v_count := v_count + 1;
    end loop;
  end loop;
  return v_result;
end;
$$;

revoke all on function private.shop_normalize_planet_objects(text, jsonb)
  from public, anon, authenticated, service_role;

create function private.shop_project_planet_objects(
  p_cycle_id text, p_objects jsonb, p_growth numeric
)
returns jsonb
language plpgsql security definer set search_path = '' as $$
declare
  v_thresholds numeric[] := array[5, 20, 50, 100]::numeric[];
  v_intervals numeric[] := array[1, 2, 4, 8, 16]::numeric[];
  v_targets integer[] := array[0, 0, 0, 0, 0];
  v_start numeric := 0;
  v_end numeric;
  v_segment numeric;
  v_available numeric;
  v_remainder numeric := 0;
  v_count integer;
  v_target integer;
  v_existing integer;
  v_stage integer;
  v_ordinal integer;
  v_hash bytea;
  v_seed numeric;
  v_i integer;
  v_x integer;
  v_y integer;
  v_kinds text[];
  v_kind text;
  v_objects jsonb := coalesce(p_objects, '[]'::jsonb);
begin
  for v_stage in 0..4 loop
    v_end := case when v_stage < 4 then v_thresholds[v_stage + 1] else p_growth end;
    v_segment := greatest(least(p_growth, v_end) - v_start, 0::numeric);
    v_available := v_segment + v_remainder;
    v_count := floor(v_available / v_intervals[v_stage + 1] + 0.000000001)::integer;
    v_targets[v_stage + 1] := v_count;
    v_remainder := greatest(v_available - v_count * v_intervals[v_stage + 1], 0::numeric);
    v_start := v_end;
  end loop;

  for v_stage in 0..4 loop
    select count(distinct (o.value->>'ordinal')::integer)::integer
    into v_existing
    from jsonb_array_elements(v_objects) o(value)
    where (o.value->>'stage')::integer = v_stage;
    v_existing := coalesce(v_existing, 0);
    v_target := greatest(v_targets[v_stage + 1], v_existing);
    v_ordinal := 0;
    while v_existing < v_target loop
      if not exists (
        select 1 from jsonb_array_elements(v_objects) o(value)
        where (o.value->>'stage')::integer = v_stage
          and (o.value->>'ordinal')::integer = v_ordinal
      ) then
        v_hash := extensions.digest(convert_to(
          p_cycle_id || ':' || v_stage::text || ':' || v_ordinal::text, 'UTF8'
        ), 'sha256');
        v_seed := 0;
        for v_i in 0..7 loop
          v_seed := v_seed * 256 + get_byte(v_hash, v_i)::numeric;
        end loop;
        v_kinds := case v_stage
          when 0 then array['rock', 'water', 'tree', 'fern', 'creature']
          when 1 then array['camp', 'crops', 'cottage', 'path', 'well']
          when 2 then array['house', 'workshop', 'plaza', 'road', 'market']
          when 3 then array['factory', 'power', 'rail', 'tower', 'district']
          else array['laboratory', 'satellite', 'rocket', 'solar', 'habitat']
        end;
        v_kind := v_kinds[mod(v_seed, array_length(v_kinds, 1)::numeric)::integer + 1];
        v_x := mod(v_seed, 88)::integer + 6;
        v_y := mod(div(v_seed, 256::numeric), 52)::integer + 28;
        v_objects := v_objects || jsonb_build_array(jsonb_build_object(
          'stage', v_stage, 'ordinal', v_ordinal, 'kind', v_kind,
          'x', v_x, 'y', v_y, 'seed', v_seed
        ));
        v_existing := v_existing + 1;
      end if;
      v_ordinal := v_ordinal + 1;
    end loop;
  end loop;

  select coalesce(jsonb_agg(o.value order by
      (o.value->>'stage')::integer, (o.value->>'ordinal')::integer), '[]'::jsonb)
  into v_objects
  from jsonb_array_elements(v_objects) o(value);
  return v_objects;
end;
$$;

revoke all on function private.shop_project_planet_objects(text, jsonb, numeric)
  from public, anon, authenticated, service_role;

alter function private.upsert_my_planet_state(jsonb, jsonb)
  rename to upsert_my_planet_state_legacy;
revoke all on function private.upsert_my_planet_state_legacy(jsonb, jsonb)
  from public, anon, authenticated, service_role;

create function private.upsert_my_planet_state(p_state jsonb, p_device_contribution jsonb)
returns jsonb
language plpgsql security definer set search_path = '' as $$
declare
  v_user_id uuid := (select auth.uid());
  v_raw_device jsonb;
  v_server_state jsonb;
  v_cycle_id text;
  v_existing_cycle_id text;
  v_trusted_objects jsonb := '[]'::jsonb;
  v_server_wallet_credits jsonb := '[]'::jsonb;
  v_projection_initialized boolean := false;
  v_growth numeric;
  v_stage smallint;
  v_progress numeric;
  v_objects jsonb;
begin
  if v_user_id is null then
    raise exception 'authentication required' using errcode = '42501';
  end if;
  if jsonb_typeof(p_device_contribution) is distinct from 'object' then
    raise exception 'planet device contribution is invalid' using errcode = '23514';
  end if;

  -- Serialize account state before the legacy planet row and effect snapshot.
  perform private.lock_shop_account(v_user_id);
  select p.current_cycle_id, p.objects
  into v_existing_cycle_id, v_trusted_objects
  from public.planet_member_state p
  where p.user_id = v_user_id
  for update;
  if found then
    select exists (
      select 1 from private.shop_planet_object_generation_baseline b
      where b.user_id = v_user_id and b.cycle_id = p_state->>'current_cycle_id'
    ) into v_projection_initialized;
  end if;
  if not found or not v_projection_initialized
    or v_existing_cycle_id is distinct from p_state->>'current_cycle_id'
  then
    v_trusted_objects := '[]'::jsonb;
  else
    v_trusted_objects := private.shop_normalize_planet_objects(
      v_existing_cycle_id, v_trusted_objects
    );
  end if;
  select coalesce(jsonb_agg(jsonb_build_object(
      'previous_cycle_id', w.previous_cycle_id,
      'amount', w.amount,
      'created_at_utc', to_char(w.created_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
    ) order by w.created_at, w.previous_cycle_id), '[]'::jsonb)
  into v_server_wallet_credits
  from private.planet_wallet_credits w
  where w.user_id = v_user_id;
  v_server_state := jsonb_set(
    jsonb_set(p_state, '{objects}', v_trusted_objects, true),
    '{wallet_credits}', v_server_wallet_credits, true
  );
  v_raw_device := p_device_contribution
    - 'canonical_version' - 'daily_segments' - 'activity_days';
  perform private.upsert_my_planet_state_legacy(v_server_state, v_raw_device);
  update public.planet_member_state p
  set objects = v_trusted_objects
  where p.user_id = v_user_id;
  perform private.shop_replace_effect_snapshot(v_user_id, v_server_state, p_device_contribution);

  select p.current_cycle_id into v_cycle_id
  from public.planet_member_state p where p.user_id = v_user_id;
  v_growth := private.shop_weighted_growth(v_user_id, v_cycle_id);
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
  v_objects := private.shop_project_planet_objects(v_cycle_id, v_objects, v_growth);
  update public.planet_member_state p set
    growth_credit = v_growth,
    stage = v_stage,
    progress_to_next = v_progress,
    objects = v_objects,
    updated_at = now()
  where p.user_id = v_user_id and p.current_cycle_id = v_cycle_id;

  insert into private.shop_planet_object_generation_baseline(user_id, cycle_id)
  values (v_user_id, v_cycle_id)
  on conflict (user_id, cycle_id) do nothing;

  return private.planet_state_json(v_user_id);
end;
$$;

revoke all on function private.upsert_my_planet_state(jsonb, jsonb)
  from public, anon, service_role;
grant execute on function private.upsert_my_planet_state(jsonb, jsonb) to authenticated;

create or replace function public.upsert_my_planet_state(p_state jsonb, p_device_contribution jsonb)
returns jsonb
language sql security invoker set search_path = '' as $$
  select private.upsert_my_planet_state(p_state, p_device_contribution);
$$;

revoke all on function public.upsert_my_planet_state(jsonb, jsonb)
  from public, anon, service_role;
grant execute on function public.upsert_my_planet_state(jsonb, jsonb) to authenticated;
