create or replace function private.shop_validate_effect_upload(
  p_user_id uuid, p_state jsonb, p_device jsonb
)
returns void
language plpgsql security definer set search_path = '' as $$
declare
  v_cycle_id text;
  v_growth_timezone text;
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
  v_cycle_started_at timestamptz;
  v_cycle_ended_at timestamptz;
  v_reward_day_start timestamptz;
  v_reward_day_end timestamptz;
  v_source_tokens numeric;
  v_activity_tokens numeric;
  v_total_segment_tokens numeric;
  v_total_activity_tokens numeric;
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
  v_growth_timezone := p_state->>'timezone';
  if v_growth_timezone is null or not exists (
    select 1 from pg_catalog.pg_timezone_names z where z.name = v_growth_timezone
  ) then
    raise exception 'planet growth timezone is unavailable' using errcode = '23514';
  end if;
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

    v_day_start := v_date::timestamp at time zone v_growth_timezone;
    v_day_end := (v_date + 1)::timestamp at time zone v_growth_timezone;
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
    v_activity_tokens := (v_row->>'tokens')::numeric;
    if to_char(v_reward_date, 'YYYY-MM-DD') <> v_row->>'reward_date'
      or (v_first_occurred at time zone v_timezone)::date <> v_reward_date
    then
      raise exception 'activity reward date is invalid' using errcode = '23514';
    end if;

    if v_row_cycle = v_cycle_id then
      select p.cycle_started_at, null::timestamptz
      into v_cycle_started_at, v_cycle_ended_at
      from public.planet_member_state p
      where p.user_id = p_user_id and p.current_cycle_id = v_row_cycle;
    else
      select b.started_at, w.created_at
      into v_cycle_started_at, v_cycle_ended_at
      from private.shop_cycle_effect_baseline b
      join private.planet_wallet_credits w
        on w.user_id = b.user_id and w.previous_cycle_id = b.cycle_id
      where b.user_id = p_user_id and b.cycle_id = v_row_cycle;
    end if;
    if not found or v_cycle_started_at is null
      or v_first_occurred < v_cycle_started_at
      or (v_cycle_ended_at is not null and v_first_occurred >= v_cycle_ended_at)
    then
      raise exception 'activity first occurrence is outside its known cycle interval' using errcode = '23514';
    end if;

    v_reward_day_start := v_reward_date::timestamp at time zone v_timezone;
    v_reward_day_end := (v_reward_date + 1)::timestamp at time zone v_timezone;
    if not exists (
      select 1
      from jsonb_array_elements(p_device->'daily_segments') s(value)
      where s.value->>'cycle_id' = v_row_cycle
        and (s.value->>'tokens')::numeric > 0
        and v_first_occurred >= ((s.value->>'date')::date::timestamp at time zone v_growth_timezone)
        and v_first_occurred < (((s.value->>'date')::date + 1)::timestamp at time zone v_growth_timezone)
    ) then
      raise exception 'activity first occurrence has no positive raw source segment' using errcode = '23514';
    end if;

    select coalesce(sum((s.value->>'tokens')::numeric), 0)
    into v_source_tokens
    from jsonb_array_elements(p_device->'daily_segments') s(value)
    where (s.value->>'tokens')::numeric > 0
      and greatest(
        (s.value->>'date')::date::timestamp at time zone v_growth_timezone,
        v_reward_day_start
      ) < least(
        ((s.value->>'date')::date + 1)::timestamp at time zone v_growth_timezone,
        v_reward_day_end
      );
    if v_source_tokens <= 0 or v_activity_tokens > v_source_tokens then
      raise exception 'activity tokens exceed positive source tokens for the reward date' using errcode = '23514';
    end if;
  end loop;
  if (select count(*) from jsonb_array_elements(p_device->'activity_days'))
    <> (select count(distinct value->>'reward_date') from jsonb_array_elements(p_device->'activity_days'))
  then
    raise exception 'activity reward dates must be unique per device' using errcode = '23514';
  end if;

  select coalesce(sum((s.value->>'tokens')::numeric), 0::numeric)
  into v_total_segment_tokens
  from jsonb_array_elements(p_device->'daily_segments') s(value);
  select coalesce(sum((a.value->>'tokens')::numeric), 0::numeric)
  into v_total_activity_tokens
  from jsonb_array_elements(p_device->'activity_days') a(value);
  if v_total_activity_tokens > v_total_segment_tokens then
    raise exception 'activity tokens exceed canonical raw source tokens' using errcode = '23514';
  end if;
exception
  when invalid_text_representation or invalid_datetime_format or datetime_field_overflow
    or numeric_value_out_of_range then
    raise exception 'planet contribution values are invalid' using errcode = '23514';
end;
$$;


revoke all on function private.shop_validate_effect_upload(uuid, jsonb, jsonb)
  from public, anon, authenticated, service_role;
