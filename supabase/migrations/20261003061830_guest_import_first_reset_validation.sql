-- Schema-2 first-reset proof normalization is private, pure, and fail-closed.
-- It returns derived proof material only; it performs no account/game writes.
create or replace function private.shop_guest_import_v2_canonical_json(p_value jsonb)
returns text
language plpgsql
immutable
security invoker
set search_path = ''
as $$
declare
  v_kind text;
  v_result text;
begin
  if p_value is null then
    return null;
  end if;
  v_kind := jsonb_typeof(p_value);
  if v_kind = 'object' then
    select '{' || coalesce(string_agg(
      pg_catalog.to_json(e.key)::text || ':' || private.shop_guest_import_v2_canonical_json(e.value),
      ',' order by e.key collate "C"
    ), '') || '}'
    into v_result
    from jsonb_each(p_value) e;
    return v_result;
  elsif v_kind = 'array' then
    select '[' || coalesce(string_agg(
      private.shop_guest_import_v2_canonical_json(e.value),
      ',' order by e.ordinality
    ), '') || ']'
    into v_result
    from jsonb_array_elements(p_value) with ordinality e(value, ordinality);
    return v_result;
  end if;
  return p_value::text;
end;
$$;

create or replace function private.shop_guest_import_v2_integer_tree_valid(p_value jsonb)
returns boolean
language plpgsql
immutable
security invoker
set search_path = ''
as $$
declare
  v_child jsonb;
  v_number numeric;
begin
  if p_value is null then
    return false;
  end if;
  case jsonb_typeof(p_value)
    when 'number' then
      if p_value::text !~ '^(0|[1-9][0-9]*)$' then
        return false;
      end if;
      v_number := (p_value #>> '{}')::numeric;
      return v_number <= 9223372036854775807::numeric;
    when 'array' then
      for v_child in select value from jsonb_array_elements(p_value)
      loop
        if not private.shop_guest_import_v2_integer_tree_valid(v_child) then
          return false;
        end if;
      end loop;
    when 'object' then
      for v_child in select value from jsonb_each(p_value)
      loop
        if not private.shop_guest_import_v2_integer_tree_valid(v_child) then
          return false;
        end if;
      end loop;
    else
      return true;
  end case;
  return true;
exception when others then
  return false;
end;
$$;

create or replace function private.shop_guest_import_v2_uuid_valid(p_value jsonb)
returns boolean
language plpgsql
immutable
security invoker
set search_path = ''
as $$
declare
  v_text text;
begin
  if jsonb_typeof(p_value) is distinct from 'string' then
    return false;
  end if;
  v_text := p_value #>> '{}';
  if v_text !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
    or v_text = '00000000-0000-0000-0000-000000000000'
  then
    return false;
  end if;
  return v_text::uuid::text = v_text;
exception when others then
  return false;
end;
$$;

create or replace function private.shop_guest_import_v2_timestamp_valid(
  p_value jsonb,
  p_nullable boolean default false
)
returns boolean
language plpgsql
immutable
security invoker
set search_path = ''
as $$
declare
  v_text text;
begin
  if p_nullable and p_value = 'null'::jsonb then
    return true;
  end if;
  if jsonb_typeof(p_value) is distinct from 'string' then
    return false;
  end if;
  v_text := p_value #>> '{}';
  if v_text !~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{6}Z$'
    or not private.shop_guest_import_timestamp_valid(p_value)
  then
    return false;
  end if;
  return true;
exception when others then
  return false;
end;
$$;

create or replace function private.shop_guest_import_v2_normalize(
  p_request jsonb,
  p_now timestamptz
)
returns jsonb
language plpgsql
stable
security invoker
set search_path = ''
as $$
declare
  v_snapshot jsonb;
  v_provenance jsonb;
  v_data jsonb;
  v_reset_receipt jsonb;
  v_reset_request jsonb;
  v_reset_result jsonb;
  v_old_id text;
  v_new_id text;
  v_device_id text;
  v_lineage_id text;
  v_account_id text;
  v_timezone_world text;
  v_timezone_planet text;
  v_timezone_reward text;
  v_captured_at timestamptz;
  v_activated_at timestamptz;
  v_reset_at timestamptz;
  v_reset_available timestamptz;
  v_occurrences jsonb;
  v_bounds jsonb;
  v_baselines jsonb;
  v_effects jsonb;
  v_row jsonb;
  v_other jsonb;
  v_expected_prefix jsonb;
  v_source_hash text;
  v_prefix_hash text;
  v_canonical_hash text;
  v_key_hash text;
  v_group_key text;
  v_previous_key text := null;
  v_seen_occurrences jsonb := '{}'::jsonb;
  v_seen_journal jsonb := '{}'::jsonb;
  v_occurrence_count bigint;
  v_ordinal bigint;
  v_watermark numeric;
  v_declared_count numeric;
  v_record_version numeric;
  v_ingest_seq numeric;
  v_tokens numeric;
  v_total_tokens numeric := 0;
  v_old_tokens numeric := 0;
  v_current_tokens numeric := 0;
  v_growth numeric := 0;
  v_canonical_version numeric;
  v_journal_count integer := 0;
  v_expected_journal_count integer := 0;
  v_bound_count integer := 0;
  v_baseline_count integer := 0;
  v_first_occurred timestamptz;
  v_last_occurred timestamptz;
  v_last_ingested timestamptz;
  v_occurred timestamptz;
  v_ingested timestamptz;
  v_numeric numeric;
  v_expected_usage jsonb;
  v_expected_daily jsonb;
  v_expected_cycle_totals jsonb;
  v_expected_segments jsonb;
  v_expected_activity jsonb;
  v_expected_contributions jsonb;
  v_expected_canonical jsonb;
  v_expected_journal jsonb := '[]'::jsonb;
  v_ack_journal jsonb := '[]'::jsonb;
  v_reset_proof jsonb;
  v_historical_cycle jsonb;
  v_settlement jsonb;
  v_claim jsonb;
  v_old_journal_cycle jsonb;
  v_new_journal_cycle jsonb;
  v_old_bound jsonb;
  v_new_bound jsonb;
  v_old_baseline jsonb;
  v_new_baseline jsonb;
  v_old_raw numeric;
  v_available_at timestamptz;
  v_growth_row record;
  v_max constant numeric := 9223372036854775807::numeric;
begin
  if p_now is null or not pg_catalog.isfinite(p_now)
    or jsonb_typeof(p_request) is distinct from 'object'
    or not private.shop_guest_import_v2_integer_tree_valid(p_request)
    or not private.shop_guest_import_has_keys(p_request, array['schema_version','snapshot'])
    or p_request->'schema_version' is distinct from '2'::jsonb
  then
    return null;
  end if;

  v_snapshot := p_request->'snapshot';
  if not private.shop_guest_import_has_keys(v_snapshot, array[
      'import_id','target_account_id','source_account_id','source_fingerprint',
      'disposition','captured_at_utc','provenance','canonical_payload','data'
    ])
    or not private.shop_guest_import_v2_uuid_valid(v_snapshot->'import_id')
    or jsonb_typeof(v_snapshot->'target_account_id') is distinct from 'string'
    or v_snapshot->>'target_account_id' !~ '^account:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
    or not private.shop_guest_import_v2_uuid_valid(to_jsonb(substr(v_snapshot->>'target_account_id', 9)))
    or v_snapshot->'source_account_id' is distinct from '"local"'::jsonb
    or v_snapshot->'disposition' is distinct from '"local_integrity_validated"'::jsonb
    or not private.shop_guest_import_v2_timestamp_valid(v_snapshot->'captured_at_utc')
    or jsonb_typeof(v_snapshot->'source_fingerprint') is distinct from 'string'
    or v_snapshot->>'source_fingerprint' !~ '^[0-9a-f]{64}$'
  then
    return null;
  end if;

  v_source_hash := encode(extensions.digest(convert_to(
    private.shop_guest_import_v2_canonical_json(v_snapshot - 'source_fingerprint'), 'UTF8'
  ), 'sha256'), 'hex');
  if v_source_hash is distinct from v_snapshot->>'source_fingerprint' then
    return null;
  end if;

  v_provenance := v_snapshot->'provenance';
  v_data := v_snapshot->'data';
  if not private.shop_guest_import_has_keys(v_provenance, array[
      'version','domain','authority','lineage_id','device_id','activated_at_utc',
      'clock_policy','ingest_watermark','occurrence_count','prefix_fingerprint',
      'occurrences','cycle_bounds','baselines','reset_receipt'
    ])
    or v_provenance->'version' is distinct from '1'::jsonb
    or v_provenance->'domain' is distinct from '"ordinary_first_reset_zero_effect_v1"'::jsonb
    or v_provenance->'authority' is distinct from '"client_self_reported"'::jsonb
    or v_provenance->'clock_policy' is distinct from '"guest_utc_monotonic_v1"'::jsonb
    or not private.shop_guest_import_v2_uuid_valid(v_provenance->'lineage_id')
    or not private.shop_guest_import_v2_uuid_valid(v_provenance->'device_id')
    or not private.shop_guest_import_v2_timestamp_valid(v_provenance->'activated_at_utc')
    or not private.shop_guest_import_uint_valid(v_provenance->'ingest_watermark', v_max)
    or not private.shop_guest_import_uint_valid(v_provenance->'occurrence_count', v_max)
    or jsonb_typeof(v_provenance->'prefix_fingerprint') is distinct from 'string'
    or v_provenance->>'prefix_fingerprint' !~ '^[0-9a-f]{64}$'
    or jsonb_typeof(v_provenance->'occurrences') is distinct from 'array'
    or jsonb_typeof(v_provenance->'cycle_bounds') is distinct from 'array'
    or jsonb_typeof(v_provenance->'baselines') is distinct from 'array'
    or jsonb_typeof(v_provenance->'reset_receipt') is distinct from 'object'
    or not private.shop_guest_import_data_valid(v_data)
  then
    return null;
  end if;

  v_device_id := v_provenance->>'device_id';
  v_lineage_id := v_provenance->>'lineage_id';
  v_account_id := v_snapshot->>'target_account_id';
  v_occurrences := v_provenance->'occurrences';
  v_bounds := v_provenance->'cycle_bounds';
  v_baselines := v_provenance->'baselines';
  v_occurrence_count := jsonb_array_length(v_occurrences);
  v_watermark := (v_provenance->>'ingest_watermark')::numeric;
  v_declared_count := (v_provenance->>'occurrence_count')::numeric;
  if v_occurrence_count < 1
    or v_watermark <> v_occurrence_count::numeric
    or v_declared_count <> v_occurrence_count::numeric
    or jsonb_array_length(v_bounds) <> 2
    or jsonb_array_length(v_baselines) <> 2
  then
    return null;
  end if;

  v_reset_receipt := v_provenance->'reset_receipt';
  if not private.shop_guest_import_has_keys(v_reset_receipt, array['request','result'])
    or not private.shop_guest_import_has_keys(v_reset_receipt->'request', array[
      'kind','request_id','cycle_id'
    ])
    or not private.shop_guest_import_has_keys(v_reset_receipt->'result', array[
      'status','request_id','previous_cycle_id','new_cycle_id','reset_at_utc',
      'final_effect_revision','final_active_instance_ids','final_effects',
      'frozen_deadline_before_reset_utc','reset_available_at_utc','raw_tokens',
      'bonus_tokens','credited_tokens','shop_state_revision'
    ])
  then
    return null;
  end if;
  v_reset_request := v_reset_receipt->'request';
  v_reset_result := v_reset_receipt->'result';
  if v_reset_request->'kind' is distinct from '"reset_planet"'::jsonb
    or not private.shop_guest_import_v2_uuid_valid(v_reset_request->'request_id')
    or not private.shop_guest_import_v2_uuid_valid(v_reset_request->'cycle_id')
    or v_reset_result->'status' is distinct from '"reset"'::jsonb
    or not private.shop_guest_import_v2_uuid_valid(v_reset_result->'request_id')
    or not private.shop_guest_import_v2_uuid_valid(v_reset_result->'previous_cycle_id')
    or not private.shop_guest_import_v2_uuid_valid(v_reset_result->'new_cycle_id')
    or not private.shop_guest_import_v2_timestamp_valid(v_reset_result->'reset_at_utc')
    or not private.shop_guest_import_uint_valid(v_reset_result->'final_effect_revision', v_max)
    or v_reset_result->'final_effect_revision' <> '0'::jsonb
    or jsonb_typeof(v_reset_result->'final_active_instance_ids') is distinct from 'array'
    or jsonb_array_length(v_reset_result->'final_active_instance_ids') <> 0
    or not private.shop_guest_import_has_keys(v_reset_result->'final_effects', array[
      'token_earning_bps','civilization_growth_bps','shop_discount_bps',
      'reset_cooldown_bps','natural_removal_discount_bps','era_reward_tokens',
      'streak_reward_tokens'
    ])
    or exists (
      select 1 from jsonb_each(v_reset_result->'final_effects') e(key, value)
      where e.value is distinct from '0'::jsonb
    )
    or v_reset_result->'frozen_deadline_before_reset_utc' is distinct from 'null'::jsonb
    or not private.shop_guest_import_v2_timestamp_valid(v_reset_result->'reset_available_at_utc')
    or not private.shop_guest_import_uint_valid(v_reset_result->'raw_tokens', v_max)
    or not private.shop_guest_import_uint_valid(v_reset_result->'bonus_tokens', v_max)
    or v_reset_result->'bonus_tokens' is distinct from '0'::jsonb
    or not private.shop_guest_import_uint_valid(v_reset_result->'credited_tokens', v_max)
    or not private.shop_guest_import_uint_valid(v_reset_result->'shop_state_revision', v_max)
    or (v_reset_result->>'shop_state_revision')::numeric < 1
  then
    return null;
  end if;

  v_old_id := v_reset_request->>'cycle_id';
  v_new_id := v_reset_result->>'new_cycle_id';
  v_reset_at := (v_reset_result->>'reset_at_utc')::timestamptz;
  v_activated_at := (v_provenance->>'activated_at_utc')::timestamptz;
  v_captured_at := (v_snapshot->>'captured_at_utc')::timestamptz;
  v_reset_available := (v_reset_result->>'reset_available_at_utc')::timestamptz;
  if v_old_id = v_new_id
    or v_reset_result->>'previous_cycle_id' is distinct from v_old_id
    or v_reset_result->>'request_id' is distinct from v_reset_request->>'request_id'
    or v_activated_at >= v_reset_at
    or v_reset_at > v_captured_at
    or v_captured_at > p_now
    or v_reset_available is distinct from v_reset_at + interval '86400 seconds'
    or v_snapshot->'source_fingerprint' is distinct from to_jsonb(v_source_hash)
    or v_data->>'planet_device_id' is distinct from v_device_id
    or (v_data->>'activation_at_utc')::timestamptz is distinct from v_activated_at
    or v_provenance->'reset_receipt'->'request'->>'request_id'
      is distinct from v_reset_result->>'request_id'
  then
    return null;
  end if;

  -- Provenance bounds and zero-effect revision-0 baselines are the source of cycle authority.
  v_bound_count := 0;
  for v_row in
    select value from jsonb_array_elements(v_bounds) with ordinality as b(value, ordinality)
    order by b.ordinality
  loop
    v_bound_count := v_bound_count + 1;
    if not private.shop_guest_import_has_keys(v_row, array[
        'cycle_id','started_at_utc','ended_at_utc'
      ])
      or not private.shop_guest_import_v2_uuid_valid(v_row->'cycle_id')
      or not private.shop_guest_import_v2_timestamp_valid(v_row->'started_at_utc')
      or not private.shop_guest_import_v2_timestamp_valid(v_row->'ended_at_utc', true)
    then
      return null;
    end if;
  end loop;
  v_old_bound := v_bounds->0;
  v_new_bound := v_bounds->1;
  if v_old_bound->>'cycle_id' is distinct from v_old_id
    or v_new_bound->>'cycle_id' is distinct from v_new_id
    or (v_old_bound->>'started_at_utc')::timestamptz is distinct from v_activated_at
    or (v_old_bound->>'ended_at_utc')::timestamptz is distinct from v_reset_at
    or (v_new_bound->>'started_at_utc')::timestamptz is distinct from v_reset_at
    or v_new_bound->'ended_at_utc' is distinct from 'null'::jsonb
  then
    return null;
  end if;

  v_baseline_count := 0;
  for v_row in
    select value from jsonb_array_elements(v_baselines) with ordinality as b(value, ordinality)
    order by b.ordinality
  loop
    v_baseline_count := v_baseline_count + 1;
    if not private.shop_guest_import_has_keys(v_row, array[
        'cycle_id','revision','started_at_utc','ended_at_utc','active_instance_ids','effects'
      ])
      or not private.shop_guest_import_v2_uuid_valid(v_row->'cycle_id')
      or not private.shop_guest_import_uint_valid(v_row->'revision', v_max)
      or v_row->'revision' is distinct from '0'::jsonb
      or not private.shop_guest_import_v2_timestamp_valid(v_row->'started_at_utc')
      or not private.shop_guest_import_v2_timestamp_valid(v_row->'ended_at_utc', true)
      or jsonb_typeof(v_row->'active_instance_ids') is distinct from 'array'
      or jsonb_array_length(v_row->'active_instance_ids') <> 0
      or not private.shop_guest_import_has_keys(v_row->'effects', array[
        'token_earning_bps','civilization_growth_bps','shop_discount_bps',
        'reset_cooldown_bps','natural_removal_discount_bps','era_reward_tokens',
        'streak_reward_tokens'
      ])
      or exists (
        select 1 from jsonb_each(v_row->'effects') e(key, value)
        where e.value is distinct from '0'::jsonb
      )
    then
      return null;
    end if;
    v_other := v_bounds->(v_baseline_count - 1);
    if v_row->'cycle_id' is distinct from v_other->'cycle_id'
      or v_row->'started_at_utc' is distinct from v_other->'started_at_utc'
      or v_row->'ended_at_utc' is distinct from v_other->'ended_at_utc'
    then
      return null;
    end if;
  end loop;
  v_old_baseline := v_baselines->0;
  v_new_baseline := v_baselines->1;

  if not private.shop_guest_import_has_keys(v_data->'current_cycle', array[
      'cycle_id','started_at_utc','ended_at_utc','is_current','settled_bonus_tokens'
    ])
    or v_data#>'{current_cycle,cycle_id}' is distinct from to_jsonb(v_new_id)
    or v_data#>'{current_cycle,is_current}' is distinct from 'true'::jsonb
    or v_data#>'{current_cycle,ended_at_utc}' is distinct from 'null'::jsonb
    or v_data#>'{current_cycle,settled_bonus_tokens}' is distinct from 'null'::jsonb
    or (v_data#>>'{current_cycle,started_at_utc}')::timestamptz is distinct from v_reset_at
    or jsonb_array_length(v_data->'historical_cycles') <> 1
    or jsonb_array_length(v_data->'effect_cycle_bounds') <> 2
    or v_data->'effect_cycle_bounds' is distinct from v_bounds
    or v_data->'effect_cycle_bounds_authoritative' is distinct from 'false'::jsonb
    or v_data->'effect_timeline_state' is distinct from 'null'::jsonb
    or jsonb_array_length(v_data->'effect_history') <> 0
    or v_data->'reset_receipts_unverifiable' is distinct from 'false'::jsonb
    or v_data->'legacy_partial_import_pending' is distinct from 'false'::jsonb
    or jsonb_array_length(v_data->'reset_settlement_proofs') <> 1
    or jsonb_array_length(v_data->'cycle_settlements') <> 1
    or jsonb_array_length(v_data->'unverified_planet_wallet_claims') <> 1
    or jsonb_array_length(v_data->'growth_journal_cycles') <> 2
    or v_data->'growth_journal_state' is distinct from
       '{"generation":0,"deleted_at_utc":null}'::jsonb
  then
    return null;
  end if;

  v_settlement := v_data->'cycle_settlements'->0;
  v_historical_cycle := v_data->'historical_cycles'->0;
  if not private.shop_guest_import_timestamp_valid(v_settlement->'settled_at_utc', false) then
    return null;
  end if;
  if not private.shop_guest_import_has_keys(v_historical_cycle, array[
      'cycle_id','started_at_utc','ended_at_utc','is_current','settled_bonus_tokens'
    ])
    or v_historical_cycle->'cycle_id' is distinct from to_jsonb(v_old_id)
    or v_historical_cycle->'is_current' is distinct from 'false'::jsonb
    or v_historical_cycle->'settled_bonus_tokens' is distinct from '0'::jsonb
    or (v_historical_cycle->>'started_at_utc')::timestamptz is distinct from v_activated_at
    or (v_historical_cycle->>'ended_at_utc')::timestamptz
      is distinct from (v_settlement->>'settled_at_utc')::timestamptz
    or (v_data->>'last_reset_at_utc')::timestamptz is distinct from v_reset_at
    or (v_data->>'reset_available_at_utc')::timestamptz is distinct from v_reset_available
    or v_reset_result->>'previous_cycle_id' is distinct from v_historical_cycle->>'cycle_id'
    or (v_old_baseline->>'started_at_utc')::timestamptz is distinct from v_activated_at
    or (v_old_baseline->>'ended_at_utc')::timestamptz is distinct from v_reset_at
    or (v_new_baseline->>'started_at_utc')::timestamptz is distinct from v_reset_at
    or v_new_baseline->'ended_at_utc' is distinct from 'null'::jsonb
  then
    return null;
  end if;

  -- All item, reward, purchase, removal, bonus, and era paths are outside this domain.
  foreach v_group_key in array array[
    'natural_objects','landscape_instances','placements','landscape_edit_versions',
    'avatar_owned','avatar_equipment','cosmetic_equipment','pending_purchases',
    'cosmetic_purchases','purchases','purchase_proofs','natural_removals',
    'removal_debits','removal_proofs','game_rewards','wallet_credits','era_progress',
    'integrity_issues'
  ] loop
    if jsonb_typeof(v_data->v_group_key) is distinct from 'array'
      or jsonb_array_length(v_data->v_group_key) <> 0
    then
      return null;
    end if;
  end loop;

  v_timezone_world := v_data->>'world_timezone';
  v_timezone_planet := v_data->>'planet_timezone';
  v_timezone_reward := v_data->>'reward_timezone';
  if not private.shop_guest_import_timezone_valid(v_data->'world_timezone')
    or not private.shop_guest_import_timezone_valid(v_data->'planet_timezone')
    or not private.shop_guest_import_timezone_valid(v_data->'reward_timezone')
    or v_data->>'planet_device_id' is distinct from v_device_id
    or v_provenance->>'device_id' is distinct from v_data->>'planet_device_id'
    or not private.shop_guest_import_v2_uuid_valid(v_data->'planet_device_id')
  then
    return null;
  end if;

  -- Occurrences are a complete immutable prefix with contiguous sequence and no correction.
  v_expected_prefix := jsonb_build_object(
    'version', 1,
    'lineage_id', v_lineage_id,
    'device_id', v_device_id,
    'ingest_watermark', v_watermark::bigint,
    'occurrences', v_occurrences
  );
  v_prefix_hash := encode(extensions.digest(convert_to(
    private.shop_guest_import_v2_canonical_json(v_expected_prefix), 'UTF8'
  ), 'sha256'), 'hex');
  if v_prefix_hash is distinct from v_provenance->>'prefix_fingerprint' then
    return null;
  end if;

  v_ordinal := 0;
  v_last_occurred := null;
  v_last_ingested := null;
  for v_row in
    select value from jsonb_array_elements(v_occurrences) with ordinality as o(value, ordinality)
    order by o.ordinality
  loop
    v_ordinal := v_ordinal + 1;
    if not private.shop_guest_import_has_keys(v_row, array[
        'occurrence_id','record_version','ingest_seq','device_id','agent',
        'occurred_at_utc','ingested_at_utc','cycle_id','total_tokens','coverage'
      ])
      or not private.shop_guest_import_v2_uuid_valid(v_row->'occurrence_id')
      or not private.shop_guest_import_uint_valid(v_row->'record_version', v_max)
      or v_row->'record_version' is distinct from '1'::jsonb
      or not private.shop_guest_import_uint_valid(v_row->'ingest_seq', v_max)
      or (v_row->>'ingest_seq')::numeric is distinct from v_ordinal::numeric
      or not private.shop_guest_import_v2_uuid_valid(v_row->'device_id')
      or v_row->>'device_id' is distinct from v_device_id
      or v_row->>'agent' not in ('codex','claude_code')
      or jsonb_typeof(v_row->'agent') is distinct from 'string'
      or not private.shop_guest_import_v2_timestamp_valid(v_row->'occurred_at_utc')
      or not private.shop_guest_import_v2_timestamp_valid(v_row->'ingested_at_utc')
      or not private.shop_guest_import_v2_uuid_valid(v_row->'cycle_id')
      or not private.shop_guest_import_uint_valid(v_row->'total_tokens', v_max)
      or v_row->'coverage' is distinct from '"complete"'::jsonb
      or (v_seen_occurrences ? (v_row->>'occurrence_id'))
    then
      return null;
    end if;
    v_occurred := (v_row->>'occurred_at_utc')::timestamptz;
    v_ingested := (v_row->>'ingested_at_utc')::timestamptz;
    v_tokens := (v_row->>'total_tokens')::numeric;
    if v_occurred < v_activated_at
      or v_occurred > v_ingested
      or v_ingested > v_captured_at
      or (v_last_occurred is not null and v_occurred < v_last_occurred)
      or (v_last_ingested is not null and v_ingested < v_last_ingested)
      or (v_occurred < v_reset_at and v_row->>'cycle_id' is distinct from v_old_id)
      or (v_occurred >= v_reset_at and v_row->>'cycle_id' is distinct from v_new_id)
      or (v_row->>'cycle_id' = v_new_id and v_tokens <> 0)
    then
      return null;
    end if;
    v_seen_occurrences := v_seen_occurrences || jsonb_build_object(v_row->>'occurrence_id', true);
    v_last_occurred := v_occurred;
    v_last_ingested := v_ingested;
    v_total_tokens := v_total_tokens + v_tokens;
    if v_row->>'cycle_id' = v_old_id then
      v_old_tokens := v_old_tokens + v_tokens;
    else
      v_current_tokens := v_current_tokens + v_tokens;
    end if;
    if v_total_tokens > v_max or v_old_tokens > v_max or v_current_tokens > v_max then
      return null;
    end if;
  end loop;
  if v_ordinal::numeric <> v_watermark or v_ordinal::numeric <> v_declared_count
    or v_old_tokens <= 0 or v_current_tokens <> 0
  then
    return null;
  end if;

  v_reset_proof := v_data->'reset_settlement_proofs'->0;
  v_claim := v_data->'unverified_planet_wallet_claims'->0;
  if not private.shop_guest_import_has_keys(v_reset_proof, array[
      'request_id','previous_cycle_id','new_cycle_id','reset_at_utc','raw_wallet_claim',
      'settled_bonus_tokens','final_effect_revision','final_effects',
      'final_active_instance_ids','old_cycle_started_at_utc','new_cycle_started_at_utc',
      'reset_available_at_utc'
    ])
    or v_reset_proof->>'request_id' is distinct from v_reset_request->>'request_id'
    or v_reset_proof->>'previous_cycle_id' is distinct from v_old_id
    or v_reset_proof->>'new_cycle_id' is distinct from v_new_id
    or (v_reset_proof->>'reset_at_utc')::timestamptz is distinct from v_reset_at
    or v_reset_proof->>'settled_bonus_tokens' is distinct from '0'
    or v_reset_proof->>'final_effect_revision' is distinct from '0'
    or v_reset_proof->'final_effects' is distinct from v_reset_result->'final_effects'
    or v_reset_proof->'final_active_instance_ids' is distinct from '[]'::jsonb
    or (v_reset_proof->>'old_cycle_started_at_utc')::timestamptz is distinct from v_activated_at
    or (v_reset_proof->>'new_cycle_started_at_utc')::timestamptz is distinct from v_reset_at
    or (v_reset_proof->>'reset_available_at_utc')::timestamptz is distinct from v_reset_available
    or v_reset_proof->'raw_wallet_claim' is distinct from v_claim
    or v_reset_result->>'request_id' is distinct from v_reset_request->>'request_id'
    or v_reset_result->>'previous_cycle_id' is distinct from v_old_id
    or (v_reset_result->>'raw_tokens')::numeric is distinct from v_old_tokens
    or v_reset_result->>'credited_tokens' is distinct from v_reset_result->>'raw_tokens'
    or v_reset_result->>'shop_state_revision' is distinct from v_data->>'shop_state_revision'
    or v_data->>'contribution_canonical_version' is null
  then
    return null;
  end if;

  if not private.shop_guest_import_has_keys(v_settlement, array[
      'cycle_id','amount','settled_at_utc'
    ])
    or v_settlement->>'cycle_id' is distinct from v_old_id
    or v_settlement->'amount' is distinct from '0'::jsonb
    or not private.shop_guest_import_timestamp_valid(v_settlement->'settled_at_utc', false)
    or (v_settlement->>'settled_at_utc')::timestamptz < v_activated_at
    or (v_settlement->>'settled_at_utc')::timestamptz > v_reset_at
    or not private.shop_guest_import_has_keys(v_claim, array[
      'previous_cycle_id','claimed_amount','created_at_utc'
    ])
    or v_claim->>'previous_cycle_id' is distinct from v_old_id
    or (v_claim->>'claimed_amount')::numeric is distinct from v_old_tokens
    or (v_claim->>'created_at_utc')::timestamptz is distinct from v_reset_at
    or v_reset_result->>'raw_tokens' is distinct from v_claim->>'claimed_amount'
    or v_reset_result->'bonus_tokens' is distinct from '0'::jsonb
  then
    return null;
  end if;

  v_old_journal_cycle := null;
  v_new_journal_cycle := null;
  for v_row in select value from jsonb_array_elements(v_data->'growth_journal_cycles')
  loop
    if v_row->>'cycle_id' = v_old_id then
      v_old_journal_cycle := v_row;
    elsif v_row->>'cycle_id' = v_new_id then
      v_new_journal_cycle := v_row;
    else
      return null;
    end if;
  end loop;
  if not private.shop_guest_import_has_keys(v_old_journal_cycle, array[
      'cycle_id','started_at_utc','ended_at_utc','wallet_credit','wallet_credit_at_utc'
    ])
    or not private.shop_guest_import_has_keys(v_new_journal_cycle, array[
      'cycle_id','started_at_utc','ended_at_utc','wallet_credit','wallet_credit_at_utc'
    ])
    or (v_old_journal_cycle->>'started_at_utc')::timestamptz is distinct from v_activated_at
    or (v_old_journal_cycle->>'ended_at_utc')::timestamptz is distinct from v_reset_at
    or (v_old_journal_cycle->>'wallet_credit')::numeric is distinct from v_old_tokens
    or (v_old_journal_cycle->>'wallet_credit_at_utc')::timestamptz is distinct from v_reset_at
    or (v_new_journal_cycle->>'started_at_utc')::timestamptz is distinct from v_reset_at
    or v_new_journal_cycle->'ended_at_utc' is distinct from 'null'::jsonb
    or v_new_journal_cycle->'wallet_credit' is distinct from 'null'::jsonb
    or v_new_journal_cycle->'wallet_credit_at_utc' is distinct from 'null'::jsonb
  then
    return null;
  end if;

  -- Independently rebuild world-day usage aggregates and cycle/daily totals.
  select coalesce(jsonb_agg(jsonb_build_object(
      'cycle_id', g.cycle_id,
      'bucket_date', g.bucket_date,
      'agent', g.agent,
      'event_count', g.event_count,
      'total_tokens', g.total_tokens,
      'coverage', 'complete'
    ) order by g.cycle_id, g.bucket_date, g.agent), '[]'::jsonb)
  into v_expected_usage
  from (
    select o.value->>'cycle_id' as cycle_id,
      ((o.value->>'occurred_at_utc')::timestamptz at time zone v_timezone_world)::date::text as bucket_date,
      o.value->>'agent' as agent,
      count(*)::bigint as event_count,
      sum((o.value->>'total_tokens')::numeric)::bigint as total_tokens
    from jsonb_array_elements(v_occurrences) o(value)
    group by o.value->>'cycle_id',
      ((o.value->>'occurred_at_utc')::timestamptz at time zone v_timezone_world)::date,
      o.value->>'agent'
  ) g;
  select coalesce(jsonb_agg(jsonb_build_object(
      'bucket_date', g.bucket_date,
      'agent', g.agent,
      'total_tokens', g.total_tokens,
      'coverage', 'complete'
    ) order by g.bucket_date, g.agent), '[]'::jsonb)
  into v_expected_daily
  from (
    select ((o.value->>'occurred_at_utc')::timestamptz at time zone v_timezone_world)::date::text as bucket_date,
      o.value->>'agent' as agent,
      sum((o.value->>'total_tokens')::numeric)::bigint as total_tokens
    from jsonb_array_elements(v_occurrences) o(value)
    group by ((o.value->>'occurred_at_utc')::timestamptz at time zone v_timezone_world)::date,
      o.value->>'agent'
  ) g;
  select coalesce(jsonb_agg(jsonb_build_object(
      'cycle_id', g.cycle_id,
      'total_tokens', g.total_tokens
    ) order by g.cycle_id), '[]'::jsonb)
  into v_expected_cycle_totals
  from (
    select o.value->>'cycle_id' as cycle_id,
      sum((o.value->>'total_tokens')::numeric)::bigint as total_tokens
    from jsonb_array_elements(v_occurrences) o(value)
    group by o.value->>'cycle_id'
  ) g;
  if v_data->'usage_aggregates' is distinct from v_expected_usage
    or v_data->'daily_agent_totals' is distinct from v_expected_daily
    or v_data->'cycle_usage_totals' is distinct from v_expected_cycle_totals
    or (v_data->>'lifetime_usage_tokens')::numeric is distinct from v_total_tokens
    or (v_data->>'current_cycle_usage_tokens')::numeric is distinct from 0::numeric
  then
    return null;
  end if;

  -- Rebuild planet-day segments and reward-day evidence from occurrence instants.
  select coalesce(jsonb_agg(jsonb_build_object(
      'cycle_id', g.cycle_id,
      'date', g.day,
      'effect_revision', 0,
      'tokens', g.tokens
    ) order by g.cycle_id, g.day, 0), '[]'::jsonb)
  into v_expected_segments
  from (
    select o.value->>'cycle_id' as cycle_id,
      ((o.value->>'occurred_at_utc')::timestamptz at time zone v_timezone_planet)::date::text as day,
      sum((o.value->>'total_tokens')::numeric)::bigint as tokens
    from jsonb_array_elements(v_occurrences) o(value)
    group by o.value->>'cycle_id',
      ((o.value->>'occurred_at_utc')::timestamptz at time zone v_timezone_planet)::date
  ) g;
  select coalesce(jsonb_agg(jsonb_build_object(
      'cycle_id', g.cycle_id,
      'reward_date', g.reward_date,
      'first_occurred_at_utc', g.first_occurred_at_utc,
      'tokens', g.tokens
    ) order by g.reward_date, g.cycle_id), '[]'::jsonb)
  into v_expected_activity
  from (
    select o.value->>'cycle_id' as cycle_id,
      ((o.value->>'occurred_at_utc')::timestamptz at time zone v_timezone_reward)::date::text as reward_date,
      pg_catalog.to_char(
        min((o.value->>'occurred_at_utc')::timestamptz) at time zone 'UTC',
        'YYYY-MM-DD"T"HH24:MI:SS.US'
      ) || '+00:00' as first_occurred_at_utc,
      sum((o.value->>'total_tokens')::numeric)::bigint as tokens
    from jsonb_array_elements(v_occurrences) o(value)
    where (o.value->>'total_tokens')::numeric > 0
    group by o.value->>'cycle_id',
      ((o.value->>'occurred_at_utc')::timestamptz at time zone v_timezone_reward)::date
    having sum((o.value->>'total_tokens')::numeric) > 0
  ) g;
  select coalesce(jsonb_agg(jsonb_build_object(
      'device_id', v_device_id,
      'cycle_id', s.value->>'cycle_id',
      'date', s.value->>'date',
      'effect_revision', s.value->'effect_revision',
      'canonical_version', (v_snapshot->'canonical_payload'->>'canonical_version')::bigint,
      'tokens', s.value->'tokens',
      'growth_bps', 0,
      'wallet_bps', 0
    ) order by s.value->>'cycle_id', s.value->>'date', (s.value->>'effect_revision')::numeric), '[]'::jsonb)
  into v_expected_contributions
  from jsonb_array_elements(v_expected_segments) s(value);

  if v_data->'effect_contributions' is distinct from v_expected_contributions
    or v_data->'activity_days' is distinct from (
      select coalesce(jsonb_agg(a.value || jsonb_build_object(
        'canonical_version', (v_snapshot->'canonical_payload'->>'canonical_version')::bigint
      ) order by a.value->>'reward_date', a.value->>'cycle_id'), '[]'::jsonb)
      from jsonb_array_elements(v_expected_activity) a(value)
    )
  then
    return null;
  end if;

  if not private.shop_guest_import_has_keys(v_snapshot->'canonical_payload', array[
      'activity_days','canonical_version','current_cycle_id','current_planet_tokens',
      'daily_segments','daily_tokens','device_id','incomplete','lifetime_tokens'
    ])
    or not private.shop_guest_import_uint_valid(v_snapshot#>'{canonical_payload,canonical_version}', v_max)
    or (v_snapshot#>>'{canonical_payload,canonical_version}')::numeric < 1
  then
    return null;
  end if;
  v_canonical_version := (v_snapshot#>>'{canonical_payload,canonical_version}')::numeric;
  v_expected_canonical := jsonb_build_object(
    'activity_days', v_expected_activity,
    'canonical_version', v_canonical_version::bigint,
    'current_cycle_id', v_new_id,
    'current_planet_tokens', 0,
    'daily_segments', v_expected_segments,
    'daily_tokens', '{}'::jsonb,
    'device_id', v_device_id,
    'incomplete', false,
    'lifetime_tokens', v_total_tokens::bigint
  );
  if v_canonical_version is distinct from (v_data->>'contribution_canonical_version')::numeric
    or v_snapshot->'canonical_payload' is distinct from v_expected_canonical
  then
    return null;
  end if;

  -- Journal rows are source evidence only after independently checking their logical key,
  -- tokens, reset generation, ACK watermark, revision, and native nine-field SHA-256.
  v_expected_journal_count := jsonb_array_length(v_expected_usage);
  v_journal_count := jsonb_array_length(v_data->'growth_journal_entries');
  if v_journal_count <> v_expected_journal_count then
    return null;
  end if;
  for v_row in select value from jsonb_array_elements(v_data->'growth_journal_entries')
  loop
    if not private.shop_guest_import_has_keys(v_row, array[
        'device_id','cycle_id','bucket_date','agent','revision','acknowledged_revision',
        'generation','present','confirmed_tokens','coverage','payload_hash'
      ])
      or v_row->>'device_id' is distinct from v_device_id
      or not private.shop_guest_import_v2_uuid_valid(v_row->'cycle_id')
      or not private.shop_guest_import_uint_valid(v_row->'revision', v_max)
      or (v_row->>'revision')::numeric < 1
      or v_row->'acknowledged_revision' is distinct from '0'::jsonb
      or v_row->'generation' is distinct from '0'::jsonb
      or v_row->'present' is distinct from 'true'::jsonb
      or v_row->>'coverage' is distinct from 'complete'
      or not private.shop_guest_import_uint_valid(v_row->'confirmed_tokens', v_max)
      or v_row->>'agent' not in ('codex','claude_code')
      or jsonb_typeof(v_row->'bucket_date') is distinct from 'string'
      or (v_row->>'bucket_date') !~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'
      or (v_row->>'bucket_date')::date::text is distinct from v_row->>'bucket_date'
      or jsonb_typeof(v_row->'payload_hash') is distinct from 'string'
      or v_row->>'payload_hash' !~ '^[0-9a-f]{64}$'
    then
      return null;
    end if;
    if not exists (
      select 1 from jsonb_array_elements(v_expected_usage) u(value)
      where u.value->>'cycle_id' = v_row->>'cycle_id'
        and u.value->>'bucket_date' = v_row->>'bucket_date'
        and u.value->>'agent' = v_row->>'agent'
        and u.value->'total_tokens' = v_row->'confirmed_tokens'
        and u.value->'coverage' = v_row->'coverage'
    ) then
      return null;
    end if;
    v_group_key := jsonb_build_array(
      v_row->>'cycle_id',v_row->>'bucket_date',v_row->>'agent'
    )::text;
    if v_seen_journal ? v_group_key then
      return null;
    end if;
    v_seen_journal := v_seen_journal || jsonb_build_object(v_group_key, true);
    v_key_hash := encode(extensions.digest(convert_to(
      private.shop_guest_import_v2_canonical_json(jsonb_build_object(
        'agent',v_row->'agent',
        'bucket_date',v_row->'bucket_date',
        'confirmed_tokens',v_row->'confirmed_tokens',
        'coverage',v_row->'coverage',
        'cycle_id',v_row->'cycle_id',
        'device_id',v_row->'device_id',
        'generation',v_row->'generation',
        'present',v_row->'present',
        'revision',v_row->'revision'
      )), 'UTF8'
    ), 'sha256'), 'hex');
    if v_key_hash is distinct from v_row->>'payload_hash' then
      return null;
    end if;
    v_expected_journal := v_expected_journal || jsonb_build_array(v_row);
    v_ack_journal := v_ack_journal || jsonb_build_array(jsonb_build_object(
      'logical_key',jsonb_build_object(
        'device_id',v_device_id,
        'cycle_id',v_row->>'cycle_id',
        'bucket_date',v_row->>'bucket_date',
        'agent',v_row->>'agent',
        'generation',0
      ),
      'revision',(v_row->>'revision')::bigint,
      'payload_hash',v_key_hash
    ));
  end loop;
  select count(*)::integer into v_expected_journal_count
  from jsonb_array_elements(v_expected_usage);
  if (select count(*) from jsonb_object_keys(v_seen_journal)) <> v_expected_journal_count then
    return null;
  end if;
  select coalesce(jsonb_agg(e.value order by e.value->>'cycle_id',e.value->>'bucket_date',e.value->>'agent'),'[]'::jsonb)
  into v_other from jsonb_array_elements(v_expected_journal) e(value);
  if v_data->'growth_journal_entries' is distinct from v_other then
    return null;
  end if;

  -- First-reset credit is raw old-cycle usage only. Growth is checked in SQL numeric
  -- against the same log2 threshold and the empty era proof remains mandatory.
  select coalesce(sum(pg_catalog.ln(1 + g.tokens / 100000::numeric) / pg_catalog.ln(2)), 0)
  into v_growth
  from (
    select ((o.value->>'occurred_at_utc')::timestamptz at time zone v_timezone_planet)::date as day,
      sum((o.value->>'total_tokens')::numeric) as tokens
    from jsonb_array_elements(v_occurrences) o(value)
    where o.value->>'cycle_id' = v_old_id
    group by ((o.value->>'occurred_at_utc')::timestamptz at time zone v_timezone_planet)::date
  ) g;
  if v_growth >= 5 then
    return null;
  end if;

  return jsonb_build_object(
    'validation_scope','guest_import_v2_first_reset',
    'source_metadata',jsonb_build_object(
      'import_id',v_snapshot->>'import_id',
      'target_account_id',v_account_id,
      'source_account_id','local',
      'source_fingerprint',v_source_hash,
      'disposition',v_snapshot->>'disposition',
      'captured_at_utc',v_snapshot->>'captured_at_utc',
      'source_relation','exact'
    ),
    'prefix_identity',jsonb_build_object(
      'lineage_id',v_lineage_id,
      'device_id',v_device_id,
      'ingest_watermark',v_watermark::bigint,
      'occurrence_count',v_declared_count::bigint,
      'prefix_fingerprint',v_prefix_hash
    ),
    'cycle_bounds',v_bounds,
    'baselines',v_baselines,
    'canonical_payload',v_expected_canonical,
    'canonical_payload_fingerprint',encode(extensions.digest(convert_to(
      private.shop_guest_import_v2_canonical_json(v_expected_canonical),'UTF8'
    ),'sha256'),'hex'),
    'journal_state',v_data->'growth_journal_state',
    'journal_cycles',v_data->'growth_journal_cycles',
    'journal_entries',v_expected_journal,
    'reset_receipt',v_reset_receipt,
    'raw_credit',jsonb_build_object(
      'cycle_id',v_old_id,
      'raw_tokens',v_old_tokens::bigint,
      'bonus_tokens',0,
      'credited_tokens',v_old_tokens::bigint,
      'reset_at_utc',v_reset_result->>'reset_at_utc',
      'reset_available_at_utc',v_reset_result->>'reset_available_at_utc'
    ),
    'wallet_balance_tokens',v_old_tokens::bigint,
    'current_cycle_tokens',0,
    'growth_credit',0,
    'stage',0,
    'progress_to_next',0,
    'ack',jsonb_build_object(
      'lineage_id',v_lineage_id,
      'device_id',v_device_id,
      'ingest_watermark',v_watermark::bigint,
      'occurrence_count',v_declared_count::bigint,
      'prefix_fingerprint',v_prefix_hash,
      'canonical_version',v_canonical_version::bigint,
      'canonical_payload_fingerprint',encode(extensions.digest(convert_to(
        private.shop_guest_import_v2_canonical_json(v_expected_canonical),'UTF8'
      ),'sha256'),'hex'),
      'journal_entries',v_ack_journal
    )
  );
exception when others then
  return null;
end;
$$;

revoke all on function private.shop_guest_import_v2_canonical_json(jsonb),
  private.shop_guest_import_v2_integer_tree_valid(jsonb),
  private.shop_guest_import_v2_uuid_valid(jsonb),
  private.shop_guest_import_v2_timestamp_valid(jsonb,boolean),
  private.shop_guest_import_v2_normalize(jsonb,timestamptz)
  from public, anon, authenticated, service_role;
