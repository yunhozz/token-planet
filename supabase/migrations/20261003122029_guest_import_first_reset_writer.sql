-- Persist a validated schema-2 first-reset import as one private atomic write.
-- The existing receipt stores the original source, exact result, and normalized proof.
alter table private.shop_guest_bootstrap_receipt
  add column normalized_proof jsonb,
  add constraint shop_guest_bootstrap_receipt_normalized_proof_object
    check (normalized_proof is null or jsonb_typeof(normalized_proof) = 'object');

create function private.shop_guest_import_v2_bootstrap(
  p_import_id uuid,
  p_request jsonb
)
returns jsonb
language plpgsql
security invoker
set search_path = ''
as $function$
declare
  v_user_id uuid := (select auth.uid());
  v_snapshot jsonb;
  v_data jsonb;
  v_prior_payload jsonb;
  v_prior_result jsonb;
  v_profile jsonb;
  v_normalized jsonb;
  v_result jsonb;
  v_status text;
  v_now timestamptz;
  v_old_cycle_id text;
  v_new_cycle_id text;
  v_device_id uuid;
  v_activated_at timestamptz;
  v_cycle_started_at timestamptz;
  v_reset_at timestamptz;
  v_reset_available_at timestamptz;
  v_request_id uuid;
  v_raw_tokens bigint;
  v_wallet_tokens bigint;
  v_canonical_version bigint;
  v_canonical jsonb;
  v_stored_canonical jsonb;
  v_journal_confirmation jsonb;
  v_shop_state jsonb;
  v_planet_state jsonb;
  v_timeline jsonb;
  v_cycle_bounds jsonb;
  v_intervals jsonb;
  v_effect_revision bigint;
  v_row jsonb;
begin
  if v_user_id is null then
    raise exception 'authentication required' using errcode = '42501';
  end if;
  if p_import_id is null
    or not private.shop_guest_import_v2_uuid_valid(to_jsonb(p_import_id::text))
    or jsonb_typeof(p_request) is distinct from 'object'
    or jsonb_typeof(p_request->'snapshot') is distinct from 'object'
    or jsonb_typeof(p_request#>'{snapshot,import_id}') is distinct from 'string'
    or not private.shop_guest_import_v2_uuid_valid(p_request#>'{snapshot,import_id}')
    or p_request#>>'{snapshot,import_id}' is distinct from p_import_id::text
    or jsonb_typeof(p_request#>'{snapshot,target_account_id}') is distinct from 'string'
  then
    raise exception 'guest shop bootstrap envelope is invalid' using errcode = '23514';
  end if;

  v_snapshot := p_request->'snapshot';
  if v_snapshot->>'target_account_id' is distinct from 'account:' || v_user_id::text then
    raise exception 'guest shop bootstrap target does not match authenticated account'
      using errcode = '42501';
  end if;

  -- Use the shared serialization row without the legacy helper's game-state
  -- initialization. The lock survives only on commit and is rolled back on error.
  insert into private.shop_account_lock(user_id) values (v_user_id)
  on conflict (user_id) do nothing;
  perform 1 from private.shop_account_lock l
  where l.user_id = v_user_id
  for update;

  -- Immutable receipts precede schema dispatch, freshness, and source validation.
  select r.payload, r.result into v_prior_payload, v_prior_result
  from private.shop_guest_bootstrap_receipt r
  where r.user_id = v_user_id and r.import_id = p_import_id;
  if found then
    if v_prior_payload = p_request then
      return v_prior_result;
    end if;
    return jsonb_build_object(
      'schema_version', 2,
      'import_id', p_import_id::text,
      'account_id', 'account:' || v_user_id::text,
      'source_fingerprint', v_snapshot->>'source_fingerprint',
      'status', 'request_conflict'
    );
  end if;

  select r.payload, r.result into v_prior_payload, v_prior_result
  from private.shop_guest_import_request r
  where r.user_id = v_user_id and r.import_id = p_import_id;
  if found then
    if v_prior_payload = p_request then
      return v_prior_result;
    end if;
    return jsonb_build_object(
      'schema_version', 2,
      'import_id', p_import_id::text,
      'account_id', 'account:' || v_user_id::text,
      'source_fingerprint', v_snapshot->>'source_fingerprint',
      'status', 'request_conflict'
    );
  end if;

  if not private.shop_guest_import_has_keys(p_request, array['schema_version','snapshot'])
    or p_request->'schema_version' is distinct from '2'::jsonb
    or not private.shop_guest_import_has_keys(v_snapshot, array[
      'import_id','target_account_id','source_account_id','source_fingerprint',
      'disposition','captured_at_utc','provenance','canonical_payload','data'
    ])
    or v_snapshot->'source_account_id' is distinct from '"local"'::jsonb
    or jsonb_typeof(v_snapshot->'source_fingerprint') is distinct from 'string'
    or v_snapshot->>'source_fingerprint' !~ '^[0-9a-f]{64}$'
  then
    raise exception 'guest shop bootstrap envelope is invalid' using errcode = '23514';
  end if;

  if not private.shop_guest_import_target_is_fresh(v_user_id) then
    v_status := 'active_account';
  else
    v_now := transaction_timestamp();
    v_normalized := private.shop_guest_import_v2_normalize(p_request, v_now);
    if v_normalized is null then
      v_status := 'source_unverifiable';
    else
      v_data := v_snapshot->'data';
      v_profile := v_data->'profile';
      if v_profile = 'null'::jsonb then
        v_profile := '{"nickname":"행성 동기화 대기","avatar":"masculine"}'::jsonb;
      elsif jsonb_typeof(v_profile) = 'object'
        and private.shop_guest_import_has_keys(v_profile, array['nickname','avatar'])
        and jsonb_typeof(v_profile->'nickname') = 'string'
        and char_length(v_profile->>'nickname') between 1 and 24
        and v_profile->>'avatar' in ('masculine','feminine')
      then
        null;
      else
        v_normalized := null;
        v_status := 'source_unverifiable';
      end if;

      if v_normalized is not null then
        v_status := 'imported';
        v_old_cycle_id := v_normalized#>>'{raw_credit,cycle_id}';
        v_new_cycle_id := v_normalized#>>'{canonical_payload,current_cycle_id}';
        v_device_id := (v_normalized#>>'{prefix_identity,device_id}')::uuid;
        v_activated_at := (v_snapshot#>>'{provenance,activated_at_utc}')::timestamptz;
        v_cycle_started_at := (v_data#>>'{current_cycle,started_at_utc}')::timestamptz;
        v_reset_at := (v_normalized#>>'{raw_credit,reset_at_utc}')::timestamptz;
        v_reset_available_at := (v_normalized#>>'{raw_credit,reset_available_at_utc}')::timestamptz;
        v_request_id := (v_snapshot#>>'{provenance,reset_receipt,request,request_id}')::uuid;
        v_raw_tokens := (v_normalized#>>'{raw_credit,raw_tokens}')::bigint;
        v_wallet_tokens := (v_normalized->>'wallet_balance_tokens')::bigint;
        v_canonical := v_normalized->'canonical_payload';
        v_canonical_version := (v_canonical->>'canonical_version')::bigint;

        insert into public.planet_member_state(
          user_id, state_version, nickname, avatar, timezone,
          current_cycle_id, cycle_started_at, last_reset_at,
          current_planet_tokens, lifetime_tokens, growth_credit, stage,
          progress_to_next, incomplete, objects, shared_visible,
          reset_available_at, reset_cooldown_bps, updated_at
        ) values (
          v_user_id, 1, v_profile->>'nickname', v_profile->>'avatar',
          v_data->>'planet_timezone', v_new_cycle_id, v_cycle_started_at,
          v_reset_at, (v_canonical->>'current_planet_tokens')::bigint,
          (v_canonical->>'lifetime_tokens')::bigint,
          (v_normalized->>'growth_credit')::numeric,
          (v_normalized->>'stage')::smallint,
          (v_normalized->>'progress_to_next')::numeric,
          (v_canonical->>'incomplete')::boolean, '[]'::jsonb, false,
          v_reset_available_at, 0, v_now
        );

        insert into private.shop_account_state(
          user_id, state_revision, reward_timezone, updated_at,
          reward_timezone_initialized
        ) values (
          v_user_id, (v_data->>'shop_state_revision')::bigint,
          v_data->>'reward_timezone', v_now, true
        );

        insert into private.planet_device_state(
          user_id, device_id, current_cycle_id, lifetime_tokens,
          current_planet_tokens, daily_tokens, incomplete,
          canonical_version, updated_at
        ) values (
          v_user_id, v_device_id, v_new_cycle_id,
          (v_canonical->>'lifetime_tokens')::bigint,
          (v_canonical->>'current_planet_tokens')::bigint,
          v_canonical->'daily_tokens', (v_canonical->>'incomplete')::boolean,
          v_canonical_version, v_now
        );

        insert into private.shop_device_contribution_state(
          user_id, device_id, canonical_version, canonical_payload, updated_at
        ) values (v_user_id, v_device_id, v_canonical_version, v_canonical, v_now);

        for v_row in
          select e.value from jsonb_array_elements(v_canonical->'daily_segments') as e(value)
        loop
          insert into private.shop_effect_contribution(
            user_id, device_id, cycle_id, date, effect_revision,
            canonical_version, tokens, growth_bps, wallet_bps
          ) values (
            v_user_id, v_device_id, v_row->>'cycle_id',
            (v_row->>'date')::date, (v_row->>'effect_revision')::bigint,
            v_canonical_version, (v_row->>'tokens')::bigint, 0, 0
          );
        end loop;

        for v_row in
          select e.value from jsonb_array_elements(v_canonical->'activity_days') as e(value)
        loop
          insert into private.shop_device_activity_day(
            user_id, device_id, reward_date, cycle_id,
            first_occurred_at_utc, canonical_version, tokens
          ) values (
            v_user_id, v_device_id, (v_row->>'reward_date')::date,
            v_row->>'cycle_id', (v_row->>'first_occurred_at_utc')::timestamptz,
            v_canonical_version, (v_row->>'tokens')::bigint
          );
          insert into private.shop_activity_day(
            user_id, reward_date, cycle_id, first_occurred_at_utc, tokens
          ) values (
            v_user_id, (v_row->>'reward_date')::date, v_row->>'cycle_id',
            (v_row->>'first_occurred_at_utc')::timestamptz,
            (v_row->>'tokens')::bigint
          );
        end loop;

        for v_row in
          select e.value from jsonb_array_elements(v_normalized->'baselines') as e(value)
        loop
          insert into private.shop_cycle_effect_baseline(
            user_id, cycle_id, started_at, ended_at
          ) values (
            v_user_id, v_row->>'cycle_id',
            (v_row->>'started_at_utc')::timestamptz,
            case when v_row->'ended_at_utc' = 'null'::jsonb then null
              else (v_row->>'ended_at_utc')::timestamptz end
          );
        end loop;

        insert into private.shop_reset_request(
          user_id, request_id, expected_cycle_id, status, created_at
        ) values (v_user_id, v_request_id, v_old_cycle_id, 'reset', v_reset_at);

        insert into private.shop_cycle_token_settlement(
          user_id, cycle_id, next_cycle_id, request_id, cycle_started_at,
          last_reset_at, raw_tokens, bonus_tokens, effect_revision,
          reset_cooldown_bps, reset_available_at, effect_snapshot, settled_at
        ) values (
          v_user_id, v_old_cycle_id, v_new_cycle_id, v_request_id,
          v_activated_at, null, v_raw_tokens, 0, 0, 0,
          v_reset_available_at,
          v_snapshot#>'{provenance,reset_receipt,result,final_effects}',
          v_reset_at
        );

        insert into private.planet_wallet_credits(
          user_id, previous_cycle_id, amount, created_at
        ) values (v_user_id, v_old_cycle_id, v_raw_tokens, v_reset_at);

        insert into private.growth_journal_state(
          user_id, generation, deleted_at, timezone
        ) values (
          v_user_id,
          (v_normalized#>>'{journal_state,generation}')::bigint,
          case when v_normalized#>'{journal_state,deleted_at_utc}' = 'null'::jsonb
            then null else (v_normalized#>>'{journal_state,deleted_at_utc}')::timestamptz end,
          v_data->>'planet_timezone'
        );

        for v_row in
          select e.value from jsonb_array_elements(v_normalized->'journal_cycles') as e(value)
        loop
          insert into private.growth_journal_cycles(
            user_id, cycle_id, started_at, ended_at, wallet_credit, wallet_credit_at
          ) values (
            v_user_id, v_row->>'cycle_id',
            (v_row->>'started_at_utc')::timestamptz,
            case when v_row->'ended_at_utc' = 'null'::jsonb then null
              else (v_row->>'ended_at_utc')::timestamptz end,
            case when v_row->'wallet_credit' = 'null'::jsonb then null
              else (v_row->>'wallet_credit')::bigint end,
            case when v_row->'wallet_credit_at_utc' = 'null'::jsonb then null
              else (v_row->>'wallet_credit_at_utc')::timestamptz end
          );
        end loop;

        for v_row in
          select e.value from jsonb_array_elements(v_normalized->'journal_entries') as e(value)
        loop
          insert into private.growth_journal_days(
            user_id, device_id, cycle_id, bucket_date, agent, revision,
            generation, present, confirmed_tokens, coverage, payload_hash
          ) values (
            v_user_id, (v_row->>'device_id')::uuid, v_row->>'cycle_id',
            (v_row->>'bucket_date')::date, v_row->>'agent',
            (v_row->>'revision')::bigint, (v_row->>'generation')::bigint,
            (v_row->>'present')::boolean,
            (v_row->>'confirmed_tokens')::bigint, v_row->>'coverage',
            v_row->>'payload_hash'
          );
        end loop;

        select c.canonical_payload into v_stored_canonical
        from private.shop_device_contribution_state c
        where c.user_id = v_user_id and c.device_id = v_device_id;
        v_planet_state := private.planet_state_json(v_user_id);
        v_shop_state := private.shop_state_json(v_user_id);
        v_journal_confirmation := private.growth_journal_json(v_user_id);

        select coalesce(jsonb_agg(jsonb_build_object(
          'cycle_id', b.cycle_id,
          'started_at_utc', to_char(b.started_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
          'ended_at_utc', case when b.ended_at is null then null
            else to_char(b.ended_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"') end
        ) order by b.started_at, b.cycle_id), '[]'::jsonb)
        into v_cycle_bounds
        from private.shop_cycle_effect_baseline b
        where b.user_id = v_user_id
          and (b.cycle_id = v_new_cycle_id or b.ended_at is not null);

        select coalesce(max(h.revision), 0) into v_effect_revision
        from private.shop_effect_history h where h.user_id = v_user_id;

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
          and (h.cycle_id = v_new_cycle_id or h.ended_at is not null);

        v_timeline := jsonb_build_object(
          'account_id', v_user_id::text,
          'current_cycle_id', v_new_cycle_id,
          'effect_revision', v_effect_revision,
          'server_time_utc', to_char(v_now at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
          'reward_timezone', v_data->>'reward_timezone',
          'cycle_bounds', v_cycle_bounds,
          'intervals', v_intervals
        );

        if v_stored_canonical is distinct from v_canonical
          or v_planet_state is null
          or v_planet_state->'profile' is distinct from v_profile
          or v_planet_state->>'timezone' is distinct from v_data->>'planet_timezone'
          or v_planet_state->>'current_cycle_id' is distinct from v_new_cycle_id
          or v_planet_state->>'cycle_started_at_utc' is distinct from
            to_char(v_cycle_started_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
          or v_planet_state->>'last_reset_at_utc' is distinct from
            to_char(v_reset_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
          or v_planet_state->'current_planet_tokens' is distinct from v_canonical->'current_planet_tokens'
          or v_planet_state->'lifetime_tokens' is distinct from v_canonical->'lifetime_tokens'
          or v_planet_state->'wallet_balance' is distinct from to_jsonb(v_wallet_tokens)
          or v_planet_state->'wallet_credits' is distinct from jsonb_build_array(jsonb_build_object(
            'previous_cycle_id', v_old_cycle_id,
            'amount', v_raw_tokens,
            'created_at_utc', to_char(v_reset_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
          ))
          or v_planet_state->'objects' is distinct from '[]'::jsonb
          or v_planet_state->'removed_natural_keys' is distinct from '[]'::jsonb
          or v_shop_state->>'account_id' is distinct from 'account:' || v_user_id::text
          or v_shop_state->>'current_cycle_id' is distinct from v_new_cycle_id
          or v_shop_state->>'state_revision' is distinct from v_data->>'shop_state_revision'
          or v_shop_state#>>'{reward_state,reward_timezone}' is distinct from v_data->>'reward_timezone'
          or v_shop_state->'available_balance' is distinct from to_jsonb(v_wallet_tokens)
          or v_shop_state->'landscape_instances' is distinct from '[]'::jsonb
          or v_shop_state->'placements' is distinct from '[]'::jsonb
          or v_shop_state->'avatar_owned_skus' is distinct from '[]'::jsonb
          or v_cycle_bounds is distinct from v_normalized->'cycle_bounds'
          or v_intervals is distinct from '[]'::jsonb
          or v_journal_confirmation->>'generation' is distinct from '0'
          or v_journal_confirmation->'deleted_at_utc' is distinct from 'null'::jsonb
          or v_journal_confirmation->>'timezone' is distinct from v_data->>'planet_timezone'
          or jsonb_array_length(v_journal_confirmation->'cycles') <>
            jsonb_array_length(v_normalized->'journal_cycles')
          or jsonb_array_length(v_journal_confirmation->'entries') <>
            jsonb_array_length(v_normalized->'journal_entries')
          or not exists (
            select 1 from public.planet_member_state p
            where p.user_id = v_user_id and not p.shared_visible
          )
        then
          raise exception 'guest shop bootstrap persisted state mismatch' using errcode = '23514';
        end if;

        for v_row in
          select e.value from jsonb_array_elements(v_normalized->'journal_entries') as e(value)
        loop
          if not exists (
            select 1 from private.growth_journal_days d
            where d.user_id = v_user_id
              and d.device_id = (v_row->>'device_id')::uuid
              and d.cycle_id = v_row->>'cycle_id'
              and d.bucket_date = (v_row->>'bucket_date')::date
              and d.agent = v_row->>'agent'
              and d.revision = (v_row->>'revision')::bigint
              and d.generation = (v_row->>'generation')::bigint
              and d.present = (v_row->>'present')::boolean
              and d.confirmed_tokens = (v_row->>'confirmed_tokens')::bigint
              and d.coverage = v_row->>'coverage'
              and d.payload_hash = v_row->>'payload_hash'
          ) then
            raise exception 'guest shop bootstrap journal projection mismatch' using errcode = '23514';
          end if;
        end loop;

        v_result := jsonb_build_object(
          'schema_version', 2,
          'import_id', p_import_id::text,
          'account_id', 'account:' || v_user_id::text,
          'source_fingerprint', v_snapshot->>'source_fingerprint',
          'status', 'imported',
          'shop_state', v_shop_state,
          'planet_state', v_planet_state,
          'effect_timeline', v_timeline,
          'canonical_contribution', v_canonical,
          'journal_confirmation', v_journal_confirmation,
          'ack', v_normalized->'ack'
        );

        -- Last write: source, normalized proof/ACK, and exact success DTO commit together.
        insert into private.shop_guest_bootstrap_receipt(
          user_id, import_id, payload, source_fingerprint, status,
          result, normalized_proof, created_at
        ) values (
          v_user_id, p_import_id, p_request,
          v_snapshot->>'source_fingerprint', 'imported',
          v_result, v_normalized, v_now
        );
        return v_result;
      end if;
    end if;
  end if;

  v_now := coalesce(v_now, transaction_timestamp());
  v_result := jsonb_build_object(
    'schema_version', 2,
    'import_id', p_import_id::text,
    'account_id', 'account:' || v_user_id::text,
    'source_fingerprint', v_snapshot->>'source_fingerprint',
    'status', v_status
  );
  insert into private.shop_guest_bootstrap_receipt(
    user_id, import_id, payload, source_fingerprint, status,
    result, normalized_proof, created_at
  ) values (
    v_user_id, p_import_id, p_request,
    v_snapshot->>'source_fingerprint', v_status,
    v_result, null, v_now
  );
  return v_result;
end;
$function$;

revoke all on function private.shop_guest_import_v2_bootstrap(uuid,jsonb)
  from public, anon, authenticated, service_role;
