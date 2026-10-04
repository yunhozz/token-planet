create or replace function public.import_guest_shop(p_import_id uuid, p_request jsonb)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $function$
declare
  v_user_id uuid := (select auth.uid());
  v_target_account_id text;
  v_prior_payload jsonb;
  v_prior_result jsonb;
  v_status text;
  v_result jsonb;
  v_schema_version jsonb;
begin
  if v_user_id is null then
    raise exception 'authentication required' using errcode = '42501';
  end if;
  if p_import_id is null
    or p_request is null
    or jsonb_typeof(p_request) is distinct from 'object'
    or jsonb_typeof(p_request->'snapshot') is distinct from 'object'
    or jsonb_typeof(p_request#>'{snapshot,import_id}') is distinct from 'string'
    or p_request#>>'{snapshot,import_id}' is distinct from p_import_id::text
    or jsonb_typeof(p_request#>'{snapshot,target_account_id}') is distinct from 'string'
    or jsonb_typeof(p_request#>'{snapshot,source_fingerprint}') is distinct from 'string'
    or p_request#>>'{snapshot,source_fingerprint}' !~ '^[0-9a-f]{64}$'
  then
    raise exception 'guest shop import envelope is invalid' using errcode = '23514';
  end if;

  v_target_account_id := p_request#>>'{snapshot,target_account_id}';
  if v_target_account_id is distinct from 'account:' || v_user_id::text then
    raise exception 'guest shop import target does not match authenticated account'
      using errcode = '42501';
  end if;

  -- Serialize with the existing game writers without initializing account state.
  insert into private.shop_account_lock(user_id) values (v_user_id)
  on conflict (user_id) do nothing;
  perform 1 from private.shop_account_lock l
  where l.user_id = v_user_id
  for update;

  -- The private imported receipt is readable only through schema 2.
  if p_request->'schema_version' is not distinct from '2'::jsonb then
    select r.payload, r.result into v_prior_payload, v_prior_result
    from private.shop_guest_bootstrap_receipt r
    where r.user_id = v_user_id and r.import_id = p_import_id;
    if found then
      if v_prior_payload = p_request then
        return v_prior_result;
      end if;
      if p_request->'schema_version' is not distinct from '2'::jsonb then
        return jsonb_build_object(
          'schema_version', 2,
          'import_id', p_import_id::text,
          'account_id', 'account:' || v_user_id::text,
          'source_fingerprint', p_request#>>'{snapshot,source_fingerprint}',
          'status', 'request_conflict'
        );
      end if;
      return jsonb_build_object('import_id', p_import_id, 'status', 'request_conflict');
    end if;
  end if;

  select r.payload, r.result into v_prior_payload, v_prior_result
  from private.shop_guest_import_request r
  where r.user_id = v_user_id and r.import_id = p_import_id;
  if found then
    if v_prior_payload = p_request then
      return v_prior_result;
    end if;
    if p_request->'schema_version' is not distinct from '2'::jsonb then
      return jsonb_build_object(
        'schema_version', 2,
        'import_id', p_import_id::text,
        'account_id', 'account:' || v_user_id::text,
        'source_fingerprint', p_request#>>'{snapshot,source_fingerprint}',
        'status', 'request_conflict'
      );
    end if;
    return jsonb_build_object('import_id', p_import_id, 'status', 'request_conflict');
  end if;

  v_schema_version := p_request->'schema_version';
  if v_schema_version is not distinct from '2'::jsonb then
    return private.shop_guest_import_v2_bootstrap(p_import_id, p_request);
  end if;
  if v_schema_version is distinct from '1'::jsonb then
    raise exception 'guest shop import envelope is invalid' using errcode = '23514';
  end if;

  -- Keep schema 1 strict and held-only; its existing validator and response shape
  -- remain unchanged after the pre-validation receipt check.
  if not private.shop_guest_import_envelope_valid(p_import_id, p_request) then
    raise exception 'guest shop import envelope is invalid' using errcode = '23514';
  end if;
  if not private.shop_guest_import_target_is_fresh(v_user_id) then
    v_status := 'active_account';
  else
    v_status := 'source_unverifiable';
  end if;
  v_result := jsonb_build_object('import_id', p_import_id, 'status', v_status);
  insert into private.shop_guest_import_request(
    user_id, import_id, payload, source_fingerprint, status, result
  ) values (
    v_user_id, p_import_id, p_request,
    p_request#>>'{snapshot,source_fingerprint}', v_status, v_result
  );
  return v_result;
end;
$function$;
