-- Game reward payouts remain separate from imported/base wallet credits.
alter table private.shop_account_state
  add column reward_timezone_initialized boolean not null default false;
update private.shop_account_state s
set reward_timezone_initialized = true
from public.planet_member_state p
where p.user_id = s.user_id;

alter table private.shop_game_reward
  drop constraint shop_game_reward_amount_check;
alter table private.shop_game_reward
  add constraint shop_game_reward_kind_amount_check check (
    (kind = 'era' and amount between 0 and 10000000)
    or (kind = 'streak' and amount between 0 and 500000)
  );

create or replace function private.shop_available_balance(p_user_id uuid)
returns numeric
language sql security definer set search_path = '' as $$
  select coalesce((
    select sum(w.amount::numeric)
    from private.planet_wallet_credits w where w.user_id = p_user_id
  ), 0::numeric) + coalesce((
    select sum(r.amount::numeric)
    from private.shop_game_reward r where r.user_id = p_user_id
  ), 0::numeric) - coalesce((
    select sum(p.price::numeric)
    from private.shop_purchase p where p.user_id = p_user_id
  ), 0::numeric);
$$;
revoke all on function private.shop_available_balance(uuid)
  from public, anon, authenticated, service_role;

alter function private.settle_shop_rewards(uuid, text)
  rename to settle_shop_era_rewards;
revoke all on function private.settle_shop_era_rewards(uuid, text)
  from public, anon, authenticated, service_role;

create function private.settle_shop_streak_rewards(p_user_id uuid, p_cycle_id text)
returns void
language plpgsql security definer set search_path = '' as $$
declare
  v_current_cycle_id text;
  v_reward_timezone text;
  v_reward_date date;
  v_first_occurred_at timestamptz;
  v_has_previous_day boolean;
  v_effect_snapshot jsonb;
  v_amount numeric;
begin
  if p_user_id is null or p_user_id is distinct from (select auth.uid()) then
    raise exception 'reward account access denied' using errcode = '42501';
  end if;
  if p_cycle_id is null or char_length(p_cycle_id) not between 1 and 80 then
    raise exception 'reward cycle is invalid' using errcode = '22023';
  end if;

  perform private.lock_shop_account(p_user_id);
  select p.current_cycle_id into v_current_cycle_id
  from public.planet_member_state p
  where p.user_id = p_user_id
  for update;
  if not found or v_current_cycle_id is distinct from p_cycle_id then
    raise exception 'streak reward cycle is not current' using errcode = '23514';
  end if;

  select s.reward_timezone into v_reward_timezone
  from private.shop_account_state s where s.user_id = p_user_id;
  if v_reward_timezone is null then
    raise exception 'reward timezone is unavailable' using errcode = '23514';
  end if;

  -- Use the canonical account date, not a per-device row: its earliest source
  -- determines whether this date belongs to the current cycle. Previous-day
  -- activity may be from a closed cycle, preserving streak continuity.
  for v_reward_date, v_first_occurred_at in
    select a.reward_date, min(a.first_occurred_at_utc)
    from private.shop_activity_day a
    where a.user_id = p_user_id and a.cycle_id = p_cycle_id and a.tokens > 0
    group by a.reward_date
    order by a.reward_date
  loop
    if exists (
      select 1 from private.shop_game_reward r
      where r.user_id = p_user_id and r.kind = 'streak'
        and r.reward_date = v_reward_date
    ) then
      continue;
    end if;

    select exists (
      select 1 from private.shop_activity_day a
      where a.user_id = p_user_id and a.reward_date = v_reward_date - 1
        and a.tokens > 0
    ) into v_has_previous_day;

    select h.effects into v_effect_snapshot
    from private.shop_effect_history h
    where h.user_id = p_user_id and h.cycle_id = p_cycle_id
      and h.started_at <= v_first_occurred_at
      and (h.ended_at is null or v_first_occurred_at < h.ended_at)
    order by h.started_at desc, h.revision desc
    limit 1;
    if not found then
      -- An unregistered revision-zero interval has no effects; never use current
      -- placements to retroactively price a historical activity timestamp.
      v_effect_snapshot := jsonb_build_object(
        'token_earning_bps', 0,
        'civilization_growth_bps', 0,
        'shop_discount_bps', 0,
        'reset_cooldown_bps', 0,
        'natural_removal_discount_bps', 0,
        'era_reward_tokens', 0,
        'streak_reward_tokens', 0
      );
    end if;

    v_amount := 0::numeric;
    if not v_has_previous_day then
      -- Do not write a permanent zero marker for an ineligible date: a late
      -- canonical upload may establish the previous positive date later.
      continue;
    end if;
    v_amount := least(greatest(
      coalesce((v_effect_snapshot->>'streak_reward_tokens')::numeric, 0::numeric),
      0::numeric
    ), 500000::numeric);
    if v_amount <> trunc(v_amount) then
      raise exception 'streak reward effect must be a whole token amount' using errcode = '23514';
    end if;

    insert into private.shop_game_reward(
      user_id, trigger_key, kind, cycle_id, era_stage, reward_date,
      amount, effect_snapshot
    ) values (
      p_user_id, 'streak:' || v_reward_date::text, 'streak', p_cycle_id,
      null, v_reward_date, v_amount::bigint, v_effect_snapshot
    ) on conflict do nothing;
  end loop;
end;
$$;
revoke all on function private.settle_shop_streak_rewards(uuid, text)
  from public, anon, authenticated, service_role;

create function private.shop_reward_state_json(p_user_id uuid)
returns jsonb
language plpgsql security definer set search_path = '' as $$
declare
  v_reward_timezone text;
  v_cycle_id text;
  v_settled_cycle_tokens bigint := 0;
  v_era_reward_tokens numeric := 0;
  v_streak_reward_tokens numeric := 0;
begin
  select s.reward_timezone into v_reward_timezone
  from private.shop_account_state s where s.user_id = p_user_id;
  select p.current_cycle_id into v_cycle_id
  from public.planet_member_state p where p.user_id = p_user_id;

  -- Token earning is only a forecast here. Reset writes the authoritative
  -- cycle settlement in a later migration; until then the settled amount is 0.
  select coalesce(sum(r.amount::numeric) filter (
      where r.kind = 'era' and r.cycle_id = v_cycle_id
    ), 0::numeric),
    coalesce(sum(r.amount::numeric) filter (where r.kind = 'streak'), 0::numeric)
  into v_era_reward_tokens, v_streak_reward_tokens
  from private.shop_game_reward r where r.user_id = p_user_id;

  if v_era_reward_tokens > 18446744073709551615::numeric
    or v_streak_reward_tokens > 18446744073709551615::numeric
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

create or replace function private.shop_state_json(p_user_id uuid)
returns jsonb
language sql security definer set search_path = '' as $$
  with account as (
    select p.current_cycle_id
    from public.planet_member_state p where p.user_id = p_user_id
  ), current_cycle as (
    select coalesce((select a.current_cycle_id from account a), '') as cycle_id
  )
  select jsonb_build_object(
    'account_id', 'account:' || p_user_id::text,
    'current_cycle_id', c.cycle_id,
    'catalog_revision', 1,
    'state_revision', coalesce((
      select s.state_revision from private.shop_account_state s where s.user_id = p_user_id
    ), 0),
    'available_balance', private.shop_available_balance(p_user_id),
    'products', coalesce((
      select jsonb_agg(jsonb_build_object(
        'sku', p.sku,
        'category', p.category,
        'display_name', p.display_name,
        'price', p.price,
        'catalog_revision', p.catalog_revision,
        'purchasable', p.purchasable,
        'placement_zone', p.placement_zone,
        'avatar_slot', p.avatar_slot,
        'effect_type', p.effect_type,
        'effect_value', p.effect_value
      ) order by p.sort_order)
      from private.shop_products p
    ), '[]'::jsonb),
    'landscape_instances', coalesce((
      select jsonb_agg(jsonb_build_object(
        'instance_id', i.instance_id::text,
        'sku', i.sku,
        'variation_index', i.variation_index,
        'seed', i.seed,
        'variation_version', i.variation_version,
        'placement_version', i.placement_version
      ) order by i.sku, i.variation_index)
      from private.shop_landscape_instance i where i.user_id = p_user_id
    ), '[]'::jsonb),
    'placements', coalesce((
      select jsonb_agg(jsonb_build_object(
        'instance_id', pl.instance_id::text,
        'cycle_id', pl.cycle_id,
        'x', pl.x,
        'y', pl.y,
        'version', pl.version
      ) order by pl.instance_id)
      from private.shop_landscape_placement pl
      where pl.user_id = p_user_id and pl.cycle_id = c.cycle_id
    ), '[]'::jsonb),
    'avatar_owned_skus', coalesce((
      select jsonb_agg(o.sku order by o.sku)
      from private.shop_avatar_owned o where o.user_id = p_user_id
    ), '[]'::jsonb),
    'avatar_equipment', jsonb_build_object(
      'head', coalesce((
        select jsonb_build_object('sku', e.sku, 'version', e.version)
        from private.shop_avatar_equipment e where e.user_id = p_user_id and e.slot = 'head'
      ), '{"sku":null,"version":0}'::jsonb),
      'outfit', coalesce((
        select jsonb_build_object('sku', e.sku, 'version', e.version)
        from private.shop_avatar_equipment e where e.user_id = p_user_id and e.slot = 'outfit'
      ), '{"sku":null,"version":0}'::jsonb),
      'face', coalesce((
        select jsonb_build_object('sku', e.sku, 'version', e.version)
        from private.shop_avatar_equipment e where e.user_id = p_user_id and e.slot = 'face'
      ), '{"sku":null,"version":0}'::jsonb),
      'back', coalesce((
        select jsonb_build_object('sku', e.sku, 'version', e.version)
        from private.shop_avatar_equipment e where e.user_id = p_user_id and e.slot = 'back'
      ), '{"sku":null,"version":0}'::jsonb)
    ),
    'effects', private.shop_active_effects(p_user_id, c.cycle_id),
    'reward_state', private.shop_reward_state_json(p_user_id),
    'action_unavailable_reason', null,
    'guest_import_pending', false,
    'guest_import_error', null
  ) from current_cycle c;
$$;
revoke all on function private.shop_state_json(uuid)
  from public, anon, authenticated, service_role;

create function private.settle_shop_rewards(p_user_id uuid, p_cycle_id text)
returns void
language plpgsql security definer set search_path = '' as $$
declare
  v_reward_count_before bigint;
  v_reward_count_after bigint;
begin
  if p_user_id is null or p_user_id is distinct from (select auth.uid()) then
    raise exception 'reward account access denied' using errcode = '42501';
  end if;
  perform private.lock_shop_account(p_user_id);

  select count(*) into v_reward_count_before
  from private.shop_game_reward r where r.user_id = p_user_id;
  perform private.settle_shop_era_rewards(p_user_id, p_cycle_id);
  perform private.settle_shop_streak_rewards(p_user_id, p_cycle_id);
  select count(*) into v_reward_count_after
  from private.shop_game_reward r where r.user_id = p_user_id;

  if v_reward_count_after > v_reward_count_before then
    update private.shop_account_state s
    set state_revision = s.state_revision + 1, updated_at = now()
    where s.user_id = p_user_id;
  end if;
end;
$$;
revoke all on function private.settle_shop_rewards(uuid, text)
  from public, anon, authenticated, service_role;

alter function private.upsert_my_planet_state(jsonb, jsonb)
  rename to upsert_my_planet_state_before_reward_delivery;
revoke all on function private.upsert_my_planet_state_before_reward_delivery(jsonb, jsonb)
  from public, anon, authenticated, service_role;

create function private.upsert_my_planet_state(p_state jsonb, p_device_contribution jsonb)
returns jsonb
language plpgsql security definer set search_path = '' as $$
declare
  v_user_id uuid := (select auth.uid());
  v_previous_cycle_id text;
  v_cycle_id text;
  v_previous_bonus bigint := 0;
  v_current_bonus bigint := 0;
  v_revision_before bigint;
  v_revision_after bigint;
  v_has_planet boolean := false;
begin
  if v_user_id is null then
    raise exception 'authentication required' using errcode = '42501';
  end if;

  perform private.lock_shop_account(v_user_id);
  select p.current_cycle_id into v_previous_cycle_id
  from public.planet_member_state p where p.user_id = v_user_id for update;
  v_has_planet := found;
  if v_has_planet then
    update private.shop_account_state s
    set reward_timezone_initialized = true
    where s.user_id = v_user_id and not s.reward_timezone_initialized;
  else
    update private.shop_account_state s
    set reward_timezone = coalesce(nullif(p_state->>'timezone', ''), 'UTC'),
      reward_timezone_initialized = true, updated_at = now()
    where s.user_id = v_user_id and not s.reward_timezone_initialized;
  end if;
  if v_previous_cycle_id is not null then
    v_previous_bonus := private.shop_cycle_token_bonus(v_user_id, v_previous_cycle_id);
  end if;
  select s.state_revision into v_revision_before
  from private.shop_account_state s where s.user_id = v_user_id;

  perform private.upsert_my_planet_state_before_reward_delivery(
    p_state, p_device_contribution
  );

  select p.current_cycle_id into v_cycle_id
  from public.planet_member_state p where p.user_id = v_user_id for update;
  if v_cycle_id is not null then
    perform private.settle_shop_rewards(v_user_id, v_cycle_id);
    v_current_bonus := private.shop_cycle_token_bonus(v_user_id, v_cycle_id);
  end if;

  select s.state_revision into v_revision_after
  from private.shop_account_state s where s.user_id = v_user_id;
  if v_current_bonus is distinct from v_previous_bonus
    and v_revision_after = v_revision_before
  then
    update private.shop_account_state s
    set state_revision = s.state_revision + 1, updated_at = now()
    where s.user_id = v_user_id;
  end if;

  return private.planet_state_json(v_user_id);
end;
$$;
revoke all on function private.upsert_my_planet_state(jsonb, jsonb)
  from public, anon, service_role;
grant execute on function private.upsert_my_planet_state(jsonb, jsonb)
  to authenticated;

create or replace function public.upsert_my_planet_state(
  p_state jsonb, p_device_contribution jsonb
)
returns jsonb
language sql security invoker set search_path = '' as $$
  select private.upsert_my_planet_state(p_state, p_device_contribution);
$$;
revoke all on function public.upsert_my_planet_state(jsonb, jsonb)
  from public, anon, service_role;
grant execute on function public.upsert_my_planet_state(jsonb, jsonb)
  to authenticated;
