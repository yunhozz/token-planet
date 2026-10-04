-- The generated object array remains complete; removal is only a per-cycle tombstone.
create table private.shop_natural_removal (
  user_id uuid not null references auth.users(id) on delete cascade,
  cycle_id text not null check (char_length(cycle_id) between 1 and 80),
  stage smallint not null check (stage between 0 and 4),
  ordinal integer not null check (ordinal >= 0),
  version bigint not null default 1 check (version > 0),
  removed_at timestamptz not null default clock_timestamp(),
  primary key (user_id, cycle_id, stage, ordinal)
);

alter table private.shop_natural_removal enable row level security;
revoke all on private.shop_natural_removal from public, anon, authenticated, service_role;

alter function private.shop_quote_json(uuid, jsonb)
  rename to shop_quote_purchase_json;
revoke all on function private.shop_quote_purchase_json(uuid, jsonb)
  from public, anon, authenticated, service_role;

create function private.shop_natural_removal_price(
  p_base_price bigint, p_discount_bps integer
)
returns bigint
language plpgsql immutable set search_path = '' as $$
declare
  v_discount integer;
begin
  if p_base_price is null or p_base_price < 1 then
    raise exception 'natural removal base price must be positive' using errcode = '22023';
  end if;
  if p_discount_bps is null then
    raise exception 'natural removal discount must be an integer' using errcode = '22023';
  end if;

  v_discount := least(greatest(p_discount_bps, 0), 3000);
  return greatest(
    1::numeric,
    ceil(p_base_price::numeric * (10000 - v_discount)::numeric / 10000::numeric)
  )::bigint;
end;
$$;

create function private.shop_natural_removal_quote_json(
  p_user_id uuid, p_target jsonb
)
returns jsonb
language plpgsql set search_path = '' as $$
declare
  v_key jsonb;
  v_cycle_id text;
  v_current_cycle_id text;
  v_planet_objects jsonb;
  v_stage integer;
  v_ordinal integer;
  v_effects jsonb;
  v_discount integer;
  v_price bigint;
  v_effect_revision bigint;
  v_object_exists boolean;
begin
  if p_user_id is null
    or jsonb_typeof(p_target) is distinct from 'object'
    or not private.shop_has_exact_keys(p_target, array['key', 'kind'])
    or p_target->>'kind' is distinct from 'remove_natural'
    or jsonb_typeof(p_target->'key') is distinct from 'object'
    or not private.shop_has_exact_keys(p_target->'key', array['cycle_id', 'ordinal', 'stage'])
    or jsonb_typeof(p_target->'key'->'cycle_id') is distinct from 'string'
    or char_length(p_target->'key'->>'cycle_id') not between 1 and 80
    or btrim(p_target->'key'->>'cycle_id') = ''
    or jsonb_typeof(p_target->'key'->'stage') is distinct from 'number'
    or (p_target->'key'->>'stage') !~ '^[0-4]$'
    or jsonb_typeof(p_target->'key'->'ordinal') is distinct from 'number'
    or (p_target->'key'->>'ordinal') !~ '^(0|[1-9][0-9]{0,9})$'
  then
    raise exception 'natural removal quote target is invalid' using errcode = '22023';
  end if;

  v_key := p_target->'key';
  v_cycle_id := v_key->>'cycle_id';
  v_stage := (v_key->>'stage')::integer;
  if (v_key->>'ordinal')::numeric > 2147483647::numeric then
    raise exception 'natural removal ordinal is invalid' using errcode = '22023';
  end if;
  v_ordinal := (v_key->>'ordinal')::integer;

  select p.current_cycle_id, p.objects
  into v_current_cycle_id, v_planet_objects
  from public.planet_member_state p
  where p.user_id = p_user_id;
  if not found or v_cycle_id is distinct from v_current_cycle_id then
    raise exception 'natural removal key is not in the current cycle' using errcode = '22023';
  end if;
  if not exists (
    select 1 from private.shop_planet_object_generation_baseline b
    where b.user_id = p_user_id and b.cycle_id = v_cycle_id
  ) then
    raise exception 'natural object generation is not initialized' using errcode = '22023';
  end if;

  select exists (
    select 1
    from jsonb_array_elements(coalesce(v_planet_objects, '[]'::jsonb)) o(value)
    where o.value = private.shop_canonical_planet_object(v_cycle_id, v_stage, v_ordinal)
  ) into v_object_exists;
  if not v_object_exists or exists (
    select 1 from private.shop_natural_removal r
    where r.user_id = p_user_id and r.cycle_id = v_cycle_id
      and r.stage = v_stage and r.ordinal = v_ordinal
  ) then
    raise exception 'natural removal key is unavailable' using errcode = '22023';
  end if;

  v_effects := private.shop_active_effects(p_user_id, v_cycle_id);
  v_discount := least(greatest(
    coalesce((v_effects->>'natural_removal_discount_bps')::integer, 0), 0
  ), 3000);
  v_price := private.shop_natural_removal_price(case v_stage
    when 0 then 100000::bigint
    when 1 then 250000::bigint
    when 2 then 500000::bigint
    when 3 then 1000000::bigint
    else 2000000::bigint
  end, v_discount);

  select coalesce(max(h.revision), 0) into v_effect_revision
  from private.shop_effect_history h where h.user_id = p_user_id;

  return jsonb_build_object(
    'target', jsonb_build_object(
      'kind', 'remove_natural',
      'key', jsonb_build_object(
        'cycle_id', v_cycle_id, 'stage', v_stage, 'ordinal', v_ordinal
      )
    ),
    'catalog_revision', 1,
    'effect_revision', v_effect_revision,
    'price', v_price
  );
end;
$$;

create function private.shop_quote_json(p_user_id uuid, p_target jsonb)
returns jsonb
language plpgsql set search_path = '' as $$
begin
  if jsonb_typeof(p_target) = 'object'
    and p_target->>'kind' = 'remove_natural'
  then
    return private.shop_natural_removal_quote_json(p_user_id, p_target);
  end if;
  return private.shop_quote_purchase_json(p_user_id, p_target);
end;
$$;

revoke all on function private.shop_natural_removal_price(bigint, integer),
  private.shop_natural_removal_quote_json(uuid, jsonb),
  private.shop_quote_json(uuid, jsonb)
  from public, anon, authenticated, service_role;
