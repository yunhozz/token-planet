-- Reuse the committed purchase path unchanged while adding placement actions.
alter function private.apply_my_shop_action(jsonb)
  rename to apply_my_shop_purchase_action;

create function private.shop_landscape_placement_valid(
  p_sku text,
  p_x numeric,
  p_y numeric
)
returns boolean
language plpgsql stable set search_path = '' as $$
declare
  v_zone text;
  v_row integer;
  v_column integer;
  v_cell_x numeric;
  v_cell_y numeric;
begin
  if p_x is null or p_y is null then
    return false;
  end if;

  select p.placement_zone into v_zone
  from private.shop_products p
  where p.sku = p_sku and p.category = 'landscape';
  if not found then
    return false;
  end if;

  if v_zone = 'ground' then
    if p_x < 0 or p_y < 0 or p_x + 64 > 1420 or p_y + 64 > 548 then
      return false;
    end if;

    -- The deterministic natural layout's maximum generated extent is smaller
    -- than the canonical 1420x548 terrain minimum. Bounds therefore remain the
    -- same when Task 7 hides a natural object; PlanetState.objects stays intact.
    for v_row in 0..9 loop
      for v_column in 0..23 loop
        if v_row in (2, 8)
          or (v_row between 3 and 7 and v_column in (0, 1, 19, 20))
          or (v_column >= 21 and v_row <= 6)
        then
          v_cell_x := 26 + v_column * 56;
          v_cell_y := 33 + v_row * 50;
          if p_x < v_cell_x + 56 and p_x + 64 > v_cell_x
            and p_y < v_cell_y + 50 and p_y + 64 > v_cell_y
          then
            return false;
          end if;
        end if;
      end loop;
    end loop;
    return true;
  elsif v_zone = 'sky' then
    return p_x >= 0 and p_x + 96 <= 1420
      and p_y >= -220 and p_y + 64 <= 0;
  end if;

  return false;
end;
$$;

create function private.shop_record_effect_change(
  p_user_id uuid,
  p_cycle_id text
)
returns void
language plpgsql set search_path = '' as $$
declare
  v_revision bigint;
  v_now timestamptz := clock_timestamp();
  v_active_ids jsonb;
  v_effects jsonb;
begin
  select coalesce(jsonb_agg(pl.instance_id::text order by pl.instance_id), '[]'::jsonb)
  into v_active_ids
  from private.shop_landscape_placement pl
  where pl.user_id = p_user_id and pl.cycle_id = p_cycle_id;

  select coalesce(max(h.revision), 0) + 1
  into v_revision
  from private.shop_effect_history h
  where h.user_id = p_user_id;

  v_effects := private.shop_active_effects(p_user_id, p_cycle_id);
  update private.shop_effect_history h
  set ended_at = v_now
  where h.user_id = p_user_id and h.cycle_id = p_cycle_id and h.ended_at is null;

  insert into private.shop_effect_history(
    user_id, cycle_id, revision, started_at, active_instance_ids, effects
  ) values (
    p_user_id, p_cycle_id, v_revision, v_now, v_active_ids, v_effects
  );
end;
$$;

create function private.apply_my_shop_action(p_request jsonb)
returns jsonb
language plpgsql set search_path = '' as $$
declare
  v_user_id uuid := (select auth.uid());
  v_request_id text;
  v_kind text;
  v_payload_sha256 text;
  v_current_cycle text := '';
  v_prior_payload jsonb;
  v_prior_hash text;
  v_prior_status text;
  v_prior_quote jsonb;
  v_status text := 'unavailable';
  v_confirmed_quote jsonb;
  v_instance_id uuid;
  v_instance_sku text;
  v_slot text;
  v_avatar_sku text;
  v_product_slot text;
  v_current_version bigint;
  v_expected_version bigint;
  v_next_version bigint;
  v_x numeric;
  v_y numeric;
  v_coordinates_valid boolean := true;
  v_before_active_ids jsonb;
  v_after_active_ids jsonb;
  v_state jsonb;
begin
  if jsonb_typeof(p_request) is distinct from 'object'
    or p_request->>'kind' not in ('place', 'retrieve', 'equip_avatar')
  then
    return private.apply_my_shop_purchase_action(p_request);
  end if;

  if v_user_id is null then
    raise exception 'authentication required' using errcode = '42501';
  end if;
  if jsonb_typeof(p_request->'request_id') is distinct from 'string'
    or char_length(p_request->>'request_id') not between 1 and 160
    or btrim(p_request->>'request_id') = ''
  then
    raise exception 'shop action request is invalid' using errcode = '22023';
  end if;

  v_request_id := p_request->>'request_id';
  v_kind := p_request->>'kind';
  if v_kind = 'place' then
    if not private.shop_has_exact_keys(p_request, array[
      'cycle_id', 'expected_version', 'instance_id', 'kind', 'request_id', 'x', 'y'
    ])
      or jsonb_typeof(p_request->'cycle_id') is distinct from 'string'
      or jsonb_typeof(p_request->'instance_id') is distinct from 'string'
      or jsonb_typeof(p_request->'expected_version') is distinct from 'number'
      or (p_request->>'expected_version') !~ '^[0-9]+$'
      or jsonb_typeof(p_request->'x') is distinct from 'number'
      or jsonb_typeof(p_request->'y') is distinct from 'number'
    then
      raise exception 'shop placement request fields are invalid' using errcode = '22023';
    end if;
  elsif v_kind = 'retrieve' then
    if not private.shop_has_exact_keys(p_request, array[
      'cycle_id', 'expected_version', 'instance_id', 'kind', 'request_id'
    ])
      or jsonb_typeof(p_request->'cycle_id') is distinct from 'string'
      or jsonb_typeof(p_request->'instance_id') is distinct from 'string'
      or jsonb_typeof(p_request->'expected_version') is distinct from 'number'
      or (p_request->>'expected_version') !~ '^[0-9]+$'
    then
      raise exception 'shop retrieval request fields are invalid' using errcode = '22023';
    end if;
  else
    if not private.shop_has_exact_keys(p_request, array[
      'expected_version', 'kind', 'request_id', 'sku', 'slot'
    ])
      or jsonb_typeof(p_request->'slot') is distinct from 'string'
      or jsonb_typeof(p_request->'sku') not in ('string', 'null')
      or jsonb_typeof(p_request->'expected_version') is distinct from 'number'
      or (p_request->>'expected_version') !~ '^[0-9]+$'
    then
      raise exception 'avatar equipment request fields are invalid' using errcode = '22023';
    end if;
    v_slot := p_request->>'slot';
    v_avatar_sku := p_request->>'sku';
  end if;

  begin
    v_expected_version := (p_request->>'expected_version')::bigint;
  exception when others then
    raise exception 'shop action version is invalid' using errcode = '22023';
  end;
  if v_kind = 'place' then
    begin
      v_x := (p_request->>'x')::numeric;
      v_y := (p_request->>'y')::numeric;
    exception when others then
      v_coordinates_valid := false;
    end;
  end if;

  v_payload_sha256 := encode(
    extensions.digest(convert_to(p_request::text, 'UTF8'), 'sha256'), 'hex'
  );

  -- Every shop mutation serializes on the account row, then the planet row.
  perform private.lock_shop_account(v_user_id);
  select p.current_cycle_id into v_current_cycle
  from public.planet_member_state p
  where p.user_id = v_user_id
  for update;
  v_current_cycle := coalesce(v_current_cycle, '');

  select r.payload, r.payload_sha256, r.status, r.confirmed_quote
  into v_prior_payload, v_prior_hash, v_prior_status, v_prior_quote
  from private.shop_action_request r
  where r.user_id = v_user_id and r.request_id = v_request_id
  for update;
  if found then
    if v_prior_hash is distinct from v_payload_sha256
      or v_prior_payload is distinct from p_request
    then
      return jsonb_build_object(
        'status', 'request_conflict', 'request_id', v_request_id,
        'confirmed_quote', null, 'state', private.shop_state_json(v_user_id)
      );
    end if;
    return jsonb_build_object(
      'status', v_prior_status, 'request_id', v_request_id,
      'confirmed_quote', v_prior_quote, 'state', private.shop_state_json(v_user_id)
    );
  end if;

  if v_kind = 'equip_avatar' then
    if v_slot not in ('head', 'outfit', 'face', 'back') then
      v_status := 'not_owned';
    elsif v_avatar_sku is not null then
      select p.avatar_slot into v_product_slot
      from private.shop_products p
      where p.sku = v_avatar_sku and p.category = 'avatar';
      if not found or v_product_slot is distinct from v_slot then
        v_status := 'not_owned';
      elsif not exists (
        select 1 from private.shop_avatar_owned o
        where o.user_id = v_user_id and o.sku = v_avatar_sku and o.slot = v_slot
      ) then
        v_status := 'not_owned';
      end if;
    end if;

    if v_status = 'unavailable' then
      select e.version into v_current_version
      from private.shop_avatar_equipment e
      where e.user_id = v_user_id and e.slot = v_slot
      for update;
      v_current_version := coalesce(v_current_version, 0);
      if v_expected_version <> v_current_version then
        v_status := 'version_conflict';
      else
        v_next_version := v_current_version + 1;
        insert into private.shop_avatar_equipment(user_id, slot, sku, version, updated_at)
        values (v_user_id, v_slot, v_avatar_sku, v_next_version, clock_timestamp())
        on conflict (user_id, slot) do update set
          sku = excluded.sku,
          version = excluded.version,
          updated_at = excluded.updated_at;
        update private.shop_account_state s
        set state_revision = s.state_revision + 1, updated_at = clock_timestamp()
        where s.user_id = v_user_id;
        v_status := case when v_avatar_sku is null then 'unequipped' else 'equipped' end;
      end if;
    end if;
  else
  if p_request->>'cycle_id' is distinct from v_current_cycle then
    v_status := 'cycle_mismatch';
  else
    select i.instance_id, i.sku, i.placement_version
    into v_instance_id, v_instance_sku, v_current_version
    from private.shop_landscape_instance i
    join private.shop_products product
      on product.sku = i.sku and product.category = 'landscape'
    where i.user_id = v_user_id and i.instance_id::text = p_request->>'instance_id'
    for update of i;

    if not found then
      v_status := 'not_owned';
    elsif v_expected_version <> v_current_version then
      v_status := 'version_conflict';
    elsif v_kind = 'place' and not v_coordinates_valid then
      v_status := 'invalid_placement';
    elsif v_kind = 'place'
      and not private.shop_landscape_placement_valid(v_instance_sku, v_x, v_y)
    then
      v_status := 'invalid_placement';
    elsif v_kind = 'retrieve' and not exists (
      select 1 from private.shop_landscape_placement pl
      where pl.user_id = v_user_id and pl.instance_id = v_instance_id
        and pl.cycle_id = v_current_cycle
    ) then
      v_status := 'version_conflict';
    else
      select coalesce(jsonb_agg(pl.instance_id::text order by pl.instance_id), '[]'::jsonb)
      into v_before_active_ids
      from private.shop_landscape_placement pl
      where pl.user_id = v_user_id and pl.cycle_id = v_current_cycle;

      v_next_version := v_current_version + 1;
      if v_kind = 'place' then
        insert into private.shop_landscape_placement(
          user_id, instance_id, cycle_id, x, y, version
        ) values (
          v_user_id, v_instance_id, v_current_cycle,
          v_x::double precision, v_y::double precision, v_next_version
        ) on conflict (user_id, instance_id) do update set
          cycle_id = excluded.cycle_id,
          x = excluded.x,
          y = excluded.y,
          version = excluded.version,
          placed_at = clock_timestamp();
        v_status := 'placed';
      else
        delete from private.shop_landscape_placement pl
        where pl.user_id = v_user_id and pl.instance_id = v_instance_id
          and pl.cycle_id = v_current_cycle;
        v_status := 'retrieved';
      end if;

      update private.shop_landscape_instance i
      set placement_version = v_next_version
      where i.user_id = v_user_id and i.instance_id = v_instance_id;

      select coalesce(jsonb_agg(pl.instance_id::text order by pl.instance_id), '[]'::jsonb)
      into v_after_active_ids
      from private.shop_landscape_placement pl
      where pl.user_id = v_user_id and pl.cycle_id = v_current_cycle;
      if v_before_active_ids is distinct from v_after_active_ids then
        perform private.shop_record_effect_change(v_user_id, v_current_cycle);
      end if;

      update private.shop_account_state s
      set state_revision = s.state_revision + 1, updated_at = clock_timestamp()
      where s.user_id = v_user_id;
    end if;
  end if;
  end if;

  insert into private.shop_action_request(
    user_id, request_id, payload, payload_sha256, status, confirmed_quote
  ) values (
    v_user_id, v_request_id, p_request, v_payload_sha256, v_status, v_confirmed_quote
  );
  v_state := private.shop_state_json(v_user_id);
  return jsonb_build_object(
    'status', v_status, 'request_id', v_request_id,
    'confirmed_quote', v_confirmed_quote, 'state', v_state
  );
end;
$$;

revoke all on function private.shop_landscape_placement_valid(text, numeric, numeric),
  private.shop_record_effect_change(uuid, text),
  private.apply_my_shop_purchase_action(jsonb), private.apply_my_shop_action(jsonb)
  from public, anon, authenticated;
