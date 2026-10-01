-- Keep the existing canonical object array intact. A removal is a charged,
-- per-cycle tombstone that affects personal display projections only.
alter table private.shop_natural_removal
  add column price bigint not null check (price > 0);

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
  ), 0::numeric) - coalesce((
    select sum(r.price::numeric)
    from private.shop_natural_removal r where r.user_id = p_user_id
  ), 0::numeric);
$$;
revoke all on function private.shop_available_balance(uuid)
  from public, anon, authenticated, service_role;

create function private.shop_current_removed_natural_keys(p_user_id uuid)
returns jsonb
language sql stable set search_path = '' as $$
  select coalesce(jsonb_agg(jsonb_build_object(
    'cycle_id', r.cycle_id, 'stage', r.stage, 'ordinal', r.ordinal
  ) order by r.stage, r.ordinal), '[]'::jsonb)
  from private.shop_natural_removal r
  join public.planet_member_state p
    on p.user_id = r.user_id and p.current_cycle_id = r.cycle_id
  where r.user_id = p_user_id;
$$;
revoke all on function private.shop_current_removed_natural_keys(uuid)
  from public, anon, authenticated, service_role;

alter function private.shop_state_json(uuid)
  rename to shop_state_json_before_natural_removal;
create function private.shop_state_json(p_user_id uuid)
returns jsonb
language sql security definer set search_path = '' as $$
  select private.shop_state_json_before_natural_removal(p_user_id)
    || jsonb_build_object(
      'removed_natural_keys', private.shop_current_removed_natural_keys(p_user_id)
    );
$$;
revoke all on function private.shop_state_json_before_natural_removal(uuid),
  private.shop_state_json(uuid)
  from public, anon, authenticated, service_role;

create or replace function public.get_my_shop_state()
returns jsonb
language plpgsql security definer set search_path = '' as $$
declare
  v_user_id uuid := (select auth.uid());
begin
  if v_user_id is null then
    raise exception 'authentication required' using errcode = '42501';
  end if;
  perform private.lock_shop_account(v_user_id);
  return private.shop_state_json(v_user_id);
end;
$$;
revoke all on function public.get_my_shop_state()
  from public, anon, service_role;
grant execute on function public.get_my_shop_state() to authenticated;

-- Migration 00205 supplies the secure canonical planet projection. Wrap that
-- function so every personal snapshot carries the current cycle's tombstones.
alter function private.planet_state_json(uuid)
  rename to planet_state_json_before_natural_removal;
create function private.planet_state_json(p_user_id uuid)
returns jsonb
language sql security definer set search_path = '' as $$
  select state.value || jsonb_build_object(
    'removed_natural_keys', private.shop_current_removed_natural_keys(p_user_id)
  )
  from (select private.planet_state_json_before_natural_removal(p_user_id) as value) state
  where state.value is not null;
$$;
revoke all on function private.planet_state_json_before_natural_removal(uuid),
  private.planet_state_json(uuid)
  from public, anon, authenticated, service_role;

alter function private.upsert_my_planet_state(jsonb, jsonb)
  rename to upsert_my_planet_state_before_natural_removal;
create function private.upsert_my_planet_state(
  p_state jsonb, p_device_contribution jsonb
)
returns jsonb
language plpgsql security definer set search_path = '' as $$
declare
  v_user_id uuid := (select auth.uid());
  v_state jsonb;
begin
  v_state := private.upsert_my_planet_state_before_natural_removal(
    case when jsonb_typeof(p_state) = 'object'
      then p_state - 'removed_natural_keys'
      else p_state
    end,
    p_device_contribution
  );
  if v_state is null then
    return null;
  end if;
  return v_state || jsonb_build_object(
    'removed_natural_keys', private.shop_current_removed_natural_keys(v_user_id)
  );
end;
$$;
revoke all on function private.upsert_my_planet_state_before_natural_removal(jsonb, jsonb)
  from public, anon, authenticated, service_role;
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

create or replace function private.get_my_planet_state()
returns jsonb
language plpgsql security definer set search_path = '' as $$
begin
  if (select auth.uid()) is null then
    raise exception 'authentication required' using errcode = '42501';
  end if;
  return private.planet_state_json((select auth.uid()));
end;
$$;
revoke all on function private.get_my_planet_state()
  from public, anon, service_role;
grant execute on function private.get_my_planet_state() to authenticated;

create or replace function public.get_my_planet_state()
returns jsonb
language sql security invoker set search_path = '' as $$
  select private.get_my_planet_state();
$$;
revoke all on function public.get_my_planet_state()
  from public, anon, service_role;
grant execute on function public.get_my_planet_state() to authenticated;

alter function private.apply_my_shop_action(jsonb)
  rename to apply_my_shop_action_before_natural_removal;

create function private.apply_my_natural_removal_action(p_request jsonb)
returns jsonb
language plpgsql set search_path = '' as $$
declare
  v_user_id uuid := (select auth.uid());
  v_request_id text;
  v_cycle_id text;
  v_current_cycle_id text := '';
  v_stage integer;
  v_ordinal integer;
  v_expected_version bigint;
  v_key jsonb;
  v_target jsonb;
  v_payload_sha256 text;
  v_prior_payload jsonb;
  v_prior_hash text;
  v_prior_status text;
  v_prior_quote jsonb;
  v_submitted_quote jsonb;
  v_server_quote jsonb;
  v_status text := 'unavailable';
  v_confirmed_quote jsonb;
  v_balance numeric;
  v_state jsonb;
begin
  if v_user_id is null then
    raise exception 'authentication required' using errcode = '42501';
  end if;
  if jsonb_typeof(p_request) is distinct from 'object'
    or not private.shop_has_exact_keys(p_request, array[
      'expected_version', 'key', 'kind', 'quote', 'request_id'
    ])
    or p_request->>'kind' is distinct from 'remove_natural'
    or jsonb_typeof(p_request->'request_id') is distinct from 'string'
    or char_length(p_request->>'request_id') not between 1 and 160
    or btrim(p_request->>'request_id') = ''
    or jsonb_typeof(p_request->'key') is distinct from 'object'
    or not private.shop_has_exact_keys(p_request->'key', array[
      'cycle_id', 'ordinal', 'stage'
    ])
    or jsonb_typeof(p_request->'key'->'cycle_id') is distinct from 'string'
    or char_length(p_request->'key'->>'cycle_id') not between 1 and 80
    or btrim(p_request->'key'->>'cycle_id') = ''
    or jsonb_typeof(p_request->'key'->'stage') is distinct from 'number'
    or (p_request->'key'->>'stage') !~ '^[0-4]$'
    or jsonb_typeof(p_request->'key'->'ordinal') is distinct from 'number'
    or (p_request->'key'->>'ordinal') !~ '^(0|[1-9][0-9]{0,9})$'
    or jsonb_typeof(p_request->'expected_version') is distinct from 'number'
    or (p_request->>'expected_version') !~ '^(0|[1-9][0-9]{0,18})$'
    or jsonb_typeof(p_request->'quote') is distinct from 'object'
    or not private.shop_has_exact_keys(p_request->'quote', array[
      'catalog_revision', 'effect_revision', 'price', 'target'
    ])
    or jsonb_typeof(p_request->'quote'->'catalog_revision') is distinct from 'number'
    or (p_request->'quote'->>'catalog_revision') !~ '^(0|[1-9][0-9]*)$'
    or jsonb_typeof(p_request->'quote'->'effect_revision') is distinct from 'number'
    or (p_request->'quote'->>'effect_revision') !~ '^(0|[1-9][0-9]*)$'
    or jsonb_typeof(p_request->'quote'->'price') is distinct from 'number'
    or (p_request->'quote'->>'price') !~ '^(0|[1-9][0-9]*)$'
  then
    raise exception 'natural removal action request is invalid' using errcode = '22023';
  end if;

  if (p_request->'key'->>'ordinal')::numeric > 2147483647::numeric
    or (p_request->>'expected_version')::numeric > 9223372036854775807::numeric
  then
    raise exception 'natural removal action version or key is out of range'
      using errcode = '22023';
  end if;

  v_request_id := p_request->>'request_id';
  v_cycle_id := p_request->'key'->>'cycle_id';
  v_stage := (p_request->'key'->>'stage')::integer;
  v_ordinal := (p_request->'key'->>'ordinal')::integer;
  v_expected_version := (p_request->>'expected_version')::bigint;
  v_key := jsonb_build_object(
    'cycle_id', v_cycle_id, 'stage', v_stage, 'ordinal', v_ordinal
  );
  v_target := jsonb_build_object('kind', 'remove_natural', 'key', v_key);
  v_submitted_quote := p_request->'quote';
  if v_submitted_quote->'target' is distinct from v_target then
    raise exception 'natural removal quote does not match its request key'
      using errcode = '22023';
  end if;

  v_payload_sha256 := encode(
    extensions.digest(convert_to(p_request::text, 'UTF8'), 'sha256'), 'hex'
  );

  -- Serialize all account mutations, then lock the canonical cycle row.
  perform private.lock_shop_account(v_user_id);
  select p.current_cycle_id
  into v_current_cycle_id
  from public.planet_member_state p
  where p.user_id = v_user_id
  for update;
  v_current_cycle_id := coalesce(v_current_cycle_id, '');

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

  if v_current_cycle_id is distinct from v_cycle_id then
    v_status := 'cycle_mismatch';
  elsif exists (
    select 1 from private.shop_natural_removal r
    where r.user_id = v_user_id and r.cycle_id = v_cycle_id
      and r.stage = v_stage and r.ordinal = v_ordinal
  ) then
    v_status := 'already_removed';
  elsif v_expected_version <> 0 then
    v_status := 'version_conflict';
  else
    begin
      v_server_quote := private.shop_quote_json(v_user_id, v_target);
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
        v_balance := private.shop_available_balance(v_user_id);
        if v_balance < (v_server_quote->>'price')::numeric then
          v_status := 'insufficient_balance';
        else
          insert into private.shop_natural_removal(
            user_id, cycle_id, stage, ordinal, version, removed_at, price
          ) values (
            v_user_id, v_cycle_id, v_stage, v_ordinal, 1, clock_timestamp(),
            (v_server_quote->>'price')::bigint
          );
          update private.shop_account_state s
          set state_revision = s.state_revision + 1, updated_at = clock_timestamp()
          where s.user_id = v_user_id;
          if not found then
            raise exception 'shop account state is unavailable' using errcode = '23503';
          end if;
          v_status := 'removed';
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

create function private.apply_my_shop_action(p_request jsonb)
returns jsonb
language plpgsql set search_path = '' as $$
begin
  if jsonb_typeof(p_request) = 'object'
    and p_request->>'kind' = 'remove_natural'
  then
    return private.apply_my_natural_removal_action(p_request);
  end if;
  return private.apply_my_shop_action_before_natural_removal(p_request);
end;
$$;

revoke all on function private.apply_my_shop_action_before_natural_removal(jsonb),
  private.apply_my_natural_removal_action(jsonb),
  private.apply_my_shop_action(jsonb)
  from public, anon, authenticated, service_role;

create or replace function public.apply_shop_action(p_request jsonb)
returns jsonb
language plpgsql security definer set search_path = '' as $$
begin
  return private.apply_my_shop_action(p_request);
end;
$$;
revoke all on function public.apply_shop_action(jsonb)
  from public, anon, service_role;
grant execute on function public.apply_shop_action(jsonb) to authenticated;
