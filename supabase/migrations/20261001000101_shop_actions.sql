alter table private.shop_action_request add column payload_sha256 text;
update private.shop_action_request
set payload_sha256 = encode(
  extensions.digest(convert_to(payload::text, 'UTF8'), 'sha256'), 'hex'
)
where payload_sha256 is null;
alter table private.shop_action_request
  alter column payload_sha256 set not null;

create function private.shop_has_exact_keys(p_value jsonb, p_keys text[])
returns boolean
language sql immutable set search_path = '' as $$
  select jsonb_typeof(p_value) = 'object'
    and (select array_agg(k.key order by k.key) from jsonb_object_keys(p_value) k(key))
      = (select array_agg(expected_key order by expected_key) from unnest(p_keys) expected_key);
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
  v_submitted_quote jsonb;
  v_server_quote jsonb;
  v_product private.shop_products%rowtype;
  v_balance numeric;
  v_instance_id uuid;
  v_seed text;
  v_variation_index integer;
  v_owned_count integer;
  v_state jsonb;
begin
  if v_user_id is null then
    raise exception 'authentication required' using errcode = '42501';
  end if;
  if jsonb_typeof(p_request) is distinct from 'object'
    or jsonb_typeof(p_request->'request_id') is distinct from 'string'
    or char_length(p_request->>'request_id') not between 1 and 160
    or btrim(p_request->>'request_id') = ''
    or jsonb_typeof(p_request->'kind') is distinct from 'string'
  then
    raise exception 'shop action request is invalid' using errcode = '22023';
  end if;

  v_request_id := p_request->>'request_id';
  v_kind := p_request->>'kind';
  if v_kind = 'purchase'
    and not private.shop_has_exact_keys(p_request, array['kind', 'quote', 'request_id'])
  then
    raise exception 'shop purchase request fields are invalid' using errcode = '22023';
  end if;
  v_payload_sha256 := encode(
    extensions.digest(convert_to(p_request::text, 'UTF8'), 'sha256'), 'hex'
  );

  -- Lock order is account serialization row, then the wallet/profile row, before
  -- reading balance, ownership counts, quote revisions, or placement state.
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

  if v_kind = 'purchase'
    and private.shop_has_exact_keys(p_request->'quote', array[
      'catalog_revision', 'effect_revision', 'price', 'target'
    ])
    and jsonb_typeof(p_request->'quote'->'catalog_revision') = 'number'
    and (p_request->'quote'->>'catalog_revision') ~ '^[0-9]+$'
    and jsonb_typeof(p_request->'quote'->'effect_revision') = 'number'
    and (p_request->'quote'->>'effect_revision') ~ '^[0-9]+$'
    and jsonb_typeof(p_request->'quote'->'price') = 'number'
    and (p_request->'quote'->>'price') ~ '^[0-9]+$'
  then
    v_submitted_quote := p_request->'quote';
    begin
      v_server_quote := private.shop_quote_json(v_user_id, v_submitted_quote->'target');
    exception when sqlstate '22023' then
      v_server_quote := null;
    end;

    if v_server_quote is not null then
      v_confirmed_quote := v_server_quote;
      if v_submitted_quote->'catalog_revision'
        is distinct from v_server_quote->'catalog_revision'
      then
        v_status := 'catalog_mismatch';
      elsif v_submitted_quote->'effect_revision'
        is distinct from v_server_quote->'effect_revision'
        or v_submitted_quote->'price' is distinct from v_server_quote->'price'
      then
        v_status := 'quote_changed';
      else
        select p.* into v_product
        from private.shop_products p
        where p.sku = v_server_quote->'target'->>'sku'
          and p.purchasable;

        if v_product.category = 'landscape' then
          select count(*)::integer into v_owned_count
          from private.shop_landscape_instance i
          where i.user_id = v_user_id and i.sku = v_product.sku;
          if v_owned_count >= 5 then
            v_status := 'limit_reached';
          else
            select candidate into v_variation_index
            from generate_series(0, 4) candidate
            where not exists (
              select 1 from private.shop_landscape_instance i
              where i.user_id = v_user_id and i.sku = v_product.sku
                and i.variation_index = candidate
            )
            order by candidate
            limit 1;
          end if;
        elsif exists (
          select 1 from private.shop_avatar_owned o
          where o.user_id = v_user_id and o.sku = v_product.sku
        ) then
          v_status := 'already_owned';
        end if;

        if v_status = 'unavailable' then
          v_balance := private.shop_available_balance(v_user_id);
          if v_balance < (v_server_quote->>'price')::numeric then
            v_status := 'insufficient_balance';
          else
            insert into private.shop_purchase(
              user_id, request_id, sku, price, catalog_revision, effect_revision
            ) values (
              v_user_id, v_request_id, v_product.sku,
              (v_server_quote->>'price')::bigint,
              (v_server_quote->>'catalog_revision')::integer,
              (v_server_quote->>'effect_revision')::bigint
            );

            if v_product.category = 'landscape' then
              v_instance_id := pg_catalog.gen_random_uuid();
              v_seed := pg_catalog.gen_random_uuid()::text;
              insert into private.shop_landscape_instance(
                user_id, instance_id, sku, variation_index, seed, variation_version,
                placement_version
              ) values (
                v_user_id, v_instance_id, v_product.sku, v_variation_index,
                v_seed, 1, 0
              );
            else
              insert into private.shop_avatar_owned(user_id, sku, slot)
              values (v_user_id, v_product.sku, v_product.avatar_slot);
            end if;

            update private.shop_account_state
            set state_revision = state_revision + 1, updated_at = now()
            where user_id = v_user_id;
            v_status := 'purchased';
          end if;
        end if;
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

create function public.apply_shop_action(p_request jsonb)
returns jsonb
language plpgsql security definer set search_path = '' as $$
begin
  return private.apply_my_shop_action(p_request);
end;
$$;

revoke all on function private.shop_has_exact_keys(jsonb, text[]),
  private.apply_my_shop_action(jsonb) from public, anon, authenticated;
revoke all on function public.apply_shop_action(jsonb) from public, anon, authenticated;
grant execute on function public.apply_shop_action(jsonb) to authenticated;
