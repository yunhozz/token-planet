-- Stage the guest import RPC behind a deliberately fail-closed boundary.
-- Ledger reconstruction, imported credits/state, and all game-table writes are
-- deferred until the server can independently verify the complete ledger.

create table if not exists private.shop_guest_import_request (
  user_id uuid not null references auth.users(id) on delete cascade,
  import_id uuid not null,
  payload jsonb not null check (jsonb_typeof(payload) = 'object'),
  source_fingerprint text not null check (source_fingerprint ~ '^[0-9a-f]{64}$'),
  status text not null check (status in ('source_unverifiable', 'active_account')),
  result jsonb not null check (jsonb_typeof(result) = 'object'),
  created_at timestamptz not null default now(),
  primary key (user_id, import_id)
);

alter table private.shop_guest_import_request enable row level security;
revoke all on private.shop_guest_import_request from public, anon, authenticated, service_role;

create table if not exists private.shop_guest_bootstrap_receipt (
  user_id uuid not null references auth.users(id) on delete cascade,
  import_id uuid not null,
  payload jsonb not null check (jsonb_typeof(payload) = 'object'),
  source_fingerprint text not null check (source_fingerprint ~ '^[0-9a-f]{64}$'),
  status text not null check (status in ('imported', 'source_unverifiable', 'active_account')),
  result jsonb not null check (jsonb_typeof(result) = 'object'),
  created_at timestamptz not null default now(),
  primary key (user_id, import_id)
);

alter table private.shop_guest_bootstrap_receipt enable row level security;
revoke all on private.shop_guest_bootstrap_receipt from public, anon, authenticated, service_role;

create or replace function private.shop_guest_import_has_keys(
  p_value jsonb,
  p_expected text[],
  p_optional text[] default '{}'::text[]
)
returns boolean
language sql
immutable
set search_path = ''
as $$
  select jsonb_typeof(p_value) = 'object'
    and not exists (
      select 1 from jsonb_object_keys(p_value) k(key)
      where not (k.key = any(p_expected))
    )
    and not exists (
      select 1 from unnest(p_expected) required(key)
      where not (required.key = any(p_optional))
        and not (p_value ? required.key)
    );
$$;

create or replace function private.shop_guest_import_string_valid(
  p_value jsonb,
  p_min_length integer,
  p_max_length integer,
  p_nullable boolean default false
)
returns boolean
language sql
immutable
set search_path = ''
as $$
  select (p_nullable and p_value = 'null'::jsonb)
    or (jsonb_typeof(p_value) = 'string'
      and char_length(p_value #>> '{}') between p_min_length and p_max_length);
$$;

create or replace function private.shop_guest_import_uint_valid(
  p_value jsonb,
  p_max numeric,
  p_nullable boolean default false
)
returns boolean
language plpgsql
immutable
set search_path = ''
as $$
declare
  v_number numeric;
begin
  if p_nullable and p_value = 'null'::jsonb then
    return true;
  end if;
  if jsonb_typeof(p_value) is distinct from 'number' then
    return false;
  end if;
  v_number := (p_value #>> '{}')::numeric;
  return v_number >= 0 and v_number <= p_max and trunc(v_number) = v_number;
exception when others then
  return false;
end;
$$;

create or replace function private.shop_guest_import_timestamp_valid(
  p_value jsonb,
  p_nullable boolean default false
)
returns boolean
language plpgsql
immutable
set search_path = ''
as $$
declare
  v_timestamp text;
begin
  if p_nullable and p_value = 'null'::jsonb then
    return true;
  end if;
  if jsonb_typeof(p_value) is distinct from 'string' then
    return false;
  end if;
  v_timestamp := p_value #>> '{}';
  if v_timestamp !~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(\.[0-9]+)?(Z|\+00:00)$' then
    return false;
  end if;
  perform v_timestamp::timestamptz;
  return true;
exception when others then
  return false;
end;
$$;

create or replace function private.shop_guest_import_timezone_valid(p_value jsonb)
returns boolean
language sql
stable
set search_path = ''
as $$
  select jsonb_typeof(p_value) = 'string'
    and char_length(p_value #>> '{}') between 1 and 80
    and exists (
      select 1 from pg_catalog.pg_timezone_names z where z.name = p_value #>> '{}'
    );
$$;

create or replace function private.shop_guest_import_cycle_valid(p_cycle jsonb)
returns boolean
language sql
immutable
set search_path = ''
as $$
  select private.shop_guest_import_has_keys(
      p_cycle,
      array['cycle_id', 'started_at_utc', 'ended_at_utc', 'is_current', 'settled_bonus_tokens']
    )
    and private.shop_guest_import_string_valid(p_cycle->'cycle_id', 1, 80)
    and private.shop_guest_import_timestamp_valid(p_cycle->'started_at_utc', true)
    and private.shop_guest_import_timestamp_valid(p_cycle->'ended_at_utc', true)
    and jsonb_typeof(p_cycle->'is_current') = 'boolean'
    and private.shop_guest_import_uint_valid(
      p_cycle->'settled_bonus_tokens', 9223372036854775807::numeric, true
    );
$$;

create or replace function private.shop_guest_import_data_valid(p_data jsonb)
returns boolean
language plpgsql
stable
set search_path = ''
as $$
declare
  v_array_key text;
  v_row jsonb;
begin
  if not private.shop_guest_import_has_keys(p_data, array[
    'world_timezone', 'planet_timezone', 'reward_timezone', 'planet_device_id',
    'shop_state_revision', 'profile', 'activation_at_utc', 'current_cycle',
    'historical_cycles', 'last_reset_at_utc', 'reset_available_at_utc',
    'effect_timeline_state', 'effect_cycle_bounds_authoritative',
    'contribution_canonical_version', 'natural_objects', 'landscape_instances',
    'placements', 'landscape_edit_versions', 'avatar_owned', 'avatar_equipment',
    'cosmetic_equipment', 'pending_purchases', 'cosmetic_purchases', 'purchases',
    'purchase_proofs', 'natural_removals', 'removal_debits', 'removal_proofs',
    'effect_history', 'effect_cycle_bounds', 'effect_contributions', 'activity_days',
    'game_rewards', 'wallet_credits', 'unverified_planet_wallet_claims',
    'cycle_settlements', 'era_progress', 'daily_agent_totals', 'usage_aggregates',
    'lifetime_usage_tokens', 'current_cycle_usage_tokens', 'cycle_usage_totals',
    'growth_journal_state', 'growth_journal_cycles', 'growth_journal_entries',
    'reset_settlement_proofs', 'integrity_issues', 'reset_receipts_unverifiable',
    'legacy_partial_import_pending'
  ], array['purchase_proofs', 'removal_proofs']) then
    return false;
  end if;

  if not private.shop_guest_import_timezone_valid(p_data->'world_timezone')
    or not private.shop_guest_import_timezone_valid(p_data->'planet_timezone')
    or not private.shop_guest_import_timezone_valid(p_data->'reward_timezone')
    or jsonb_typeof(p_data->'planet_device_id') is distinct from 'string'
    or p_data->>'planet_device_id' !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
    or not private.shop_guest_import_uint_valid(
      p_data->'shop_state_revision', 9223372036854775807::numeric
    )
    or not private.shop_guest_import_timestamp_valid(p_data->'activation_at_utc')
    or not private.shop_guest_import_cycle_valid(p_data->'current_cycle')
    or not private.shop_guest_import_timestamp_valid(p_data->'last_reset_at_utc', true)
    or not private.shop_guest_import_timestamp_valid(p_data->'reset_available_at_utc', true)
    or not private.shop_guest_import_uint_valid(
      p_data->'contribution_canonical_version', 9223372036854775807::numeric, true
    )
    or not private.shop_guest_import_uint_valid(
      p_data->'lifetime_usage_tokens', 9223372036854775807::numeric, true
    )
    or not private.shop_guest_import_uint_valid(
      p_data->'current_cycle_usage_tokens', 9223372036854775807::numeric, true
    )
    or jsonb_typeof(p_data->'effect_cycle_bounds_authoritative') is distinct from 'boolean'
    or jsonb_typeof(p_data->'reset_receipts_unverifiable') is distinct from 'boolean'
    or jsonb_typeof(p_data->'legacy_partial_import_pending') is distinct from 'boolean'
  then
    return false;
  end if;

  if p_data->'profile' <> 'null'::jsonb then
    if not private.shop_guest_import_has_keys(p_data->'profile', array['nickname', 'avatar'])
      or not private.shop_guest_import_string_valid(p_data#>'{profile,nickname}', 1, 160)
      or jsonb_typeof(p_data#>'{profile,avatar}') is distinct from 'string'
      or p_data#>>'{profile,avatar}' not in ('masculine', 'feminine')
    then
      return false;
    end if;
  end if;

  if jsonb_typeof(p_data->'effect_timeline_state') <> 'null'::text then
    if not private.shop_guest_import_has_keys(p_data->'effect_timeline_state', array[
      'current_cycle_id', 'effect_revision', 'server_time_utc', 'reward_timezone'
    ])
      or not private.shop_guest_import_string_valid(
        p_data#>'{effect_timeline_state,current_cycle_id}', 1, 80
      )
      or not private.shop_guest_import_uint_valid(
        p_data#>'{effect_timeline_state,effect_revision}', 9223372036854775807::numeric
      )
      or not private.shop_guest_import_timestamp_valid(
        p_data#>'{effect_timeline_state,server_time_utc}'
      )
      or not private.shop_guest_import_timezone_valid(
        p_data#>'{effect_timeline_state,reward_timezone}'
      )
    then
      return false;
    end if;
  end if;

  if jsonb_typeof(p_data->'historical_cycles') is distinct from 'array' then
    return false;
  end if;
  for v_row in select value from jsonb_array_elements(p_data->'historical_cycles')
  loop
    if not private.shop_guest_import_cycle_valid(v_row) then
      return false;
    end if;
  end loop;

  -- Snapshot arrays are schema-checked as arrays and row objects here. Their
  -- nested contents remain untrusted and are never imported in this stage.
  foreach v_array_key in array array[
    'natural_objects', 'landscape_instances', 'placements', 'landscape_edit_versions',
    'avatar_owned', 'avatar_equipment', 'cosmetic_equipment', 'pending_purchases',
    'cosmetic_purchases', 'purchases', 'purchase_proofs', 'natural_removals',
    'removal_debits', 'removal_proofs', 'effect_history', 'effect_cycle_bounds',
    'effect_contributions', 'activity_days', 'game_rewards', 'wallet_credits',
    'unverified_planet_wallet_claims', 'cycle_settlements', 'era_progress',
    'daily_agent_totals', 'usage_aggregates', 'cycle_usage_totals',
    'growth_journal_cycles', 'growth_journal_entries', 'reset_settlement_proofs',
    'integrity_issues'
  ]
  loop
    if v_array_key in ('purchase_proofs', 'removal_proofs')
      and not (p_data ? v_array_key) then
      continue;
    end if;
    if jsonb_typeof(p_data->v_array_key) is distinct from 'array' then
      return false;
    end if;
    for v_row in select value from jsonb_array_elements(p_data->v_array_key)
    loop
      if jsonb_typeof(v_row) is distinct from 'object' then
        return false;
      end if;
    end loop;
  end loop;

  if jsonb_typeof(p_data->'growth_journal_state') <> 'null'::text
    and not private.shop_guest_import_has_keys(
      p_data->'growth_journal_state', array['generation','deleted_at_utc']
    ) then
    return false;
  end if;
  if jsonb_typeof(p_data->'growth_journal_state') <> 'null'::text
    and (not private.shop_guest_import_uint_valid(
        p_data#>'{growth_journal_state,generation}', 9223372036854775807::numeric
      )
      or not private.shop_guest_import_timestamp_valid(
        p_data#>'{growth_journal_state,deleted_at_utc}', true
      )) then
    return false;
  end if;

  return true;
exception when others then
  return false;
end;
$$;

create or replace function private.shop_guest_import_envelope_valid(
  p_import_id uuid,
  p_request jsonb
)
returns boolean
language plpgsql
stable
set search_path = ''
as $$
declare
  v_snapshot jsonb;
begin
  if p_import_id is null
    or not private.shop_guest_import_has_keys(p_request, array['schema_version','snapshot'])
    or not private.shop_guest_import_uint_valid(p_request->'schema_version', 1)
    or (p_request->>'schema_version')::numeric <> 1 then
    return false;
  end if;

  v_snapshot := p_request->'snapshot';
  if not private.shop_guest_import_has_keys(v_snapshot, array[
      'import_id','target_account_id','source_account_id','source_fingerprint',
      'disposition','data'
    ])
    or jsonb_typeof(v_snapshot->'import_id') is distinct from 'string'
    or v_snapshot->>'import_id' !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
    or (v_snapshot->>'import_id')::uuid is distinct from p_import_id
    or jsonb_typeof(v_snapshot->'target_account_id') is distinct from 'string'
    or v_snapshot->>'target_account_id' !~ '^account:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
    or jsonb_typeof(v_snapshot->'source_account_id') is distinct from 'string'
    or v_snapshot->>'source_account_id' <> 'local'
    or jsonb_typeof(v_snapshot->'source_fingerprint') is distinct from 'string'
    or v_snapshot->>'source_fingerprint' !~ '^[0-9a-f]{64}$'
    or jsonb_typeof(v_snapshot->'disposition') is distinct from 'string'
    or v_snapshot->>'disposition' not in ('local_integrity_validated','source_unverifiable')
    or not private.shop_guest_import_data_valid(v_snapshot->'data') then
    return false;
  end if;
  return true;
exception when others then
  return false;
end;
$$;

-- This is a partial, pure normalizer for ownership and action-proof rows. It
-- does not establish ledger completeness, grant credit, or authorize import.
create or replace function private.shop_guest_import_ownership_normalize(p_data jsonb)
returns jsonb
language plpgsql
stable
set search_path = ''
as $$
declare
  v_row jsonb;
  v_proof jsonb;
  v_quote jsonb;
  v_target jsonb;
  v_ownership jsonb;
  v_product private.shop_products%rowtype;
  v_instance jsonb;
  v_instance_id uuid;
  v_cycle_id text;
  v_request_id text;
  v_sku text;
  v_price bigint;
  v_revision bigint;
  v_variation smallint;
  v_seed text;
  v_x numeric;
  v_y numeric;
  v_match_count integer;
  v_purchase_count integer;
  v_removal_count integer;
  v_landscape_purchase_count integer := 0;
  v_avatar_purchase_count integer := 0;
  v_seen_instances jsonb := '{}'::jsonb;
  v_seen_landscape_ownership jsonb := '{}'::jsonb;
  v_seen_variations jsonb := '{}'::jsonb;
  v_seen_purchases jsonb := '{}'::jsonb;
  v_seen_avatar_skus jsonb := '{}'::jsonb;
  v_seen_proofs jsonb := '{}'::jsonb;
  v_seen_placements jsonb := '{}'::jsonb;
  v_seen_natural_keys jsonb := '{}'::jsonb;
  v_seen_removal_ids jsonb := '{}'::jsonb;
  v_seen_removal_keys jsonb := '{}'::jsonb;
  v_landscape_instances jsonb := '[]'::jsonb;
  v_avatar_owned jsonb := '[]'::jsonb;
  v_placements jsonb := '[]'::jsonb;
  v_purchases jsonb := '[]'::jsonb;
  v_natural_removals jsonb := '[]'::jsonb;
  v_normalized_timestamp text;
  v_effect_timeline jsonb;
  v_removal_key text;
  v_stage integer;
  v_ordinal integer;
  v_expected_removal_price bigint;
begin
  if jsonb_typeof(p_data) is distinct from 'object'
    or jsonb_typeof(p_data->'current_cycle') is distinct from 'object'
    or jsonb_typeof(p_data->'landscape_instances') is distinct from 'array'
    or jsonb_typeof(p_data->'landscape_edit_versions') is distinct from 'array'
    or jsonb_typeof(p_data->'placements') is distinct from 'array'
    or jsonb_typeof(p_data->'avatar_owned') is distinct from 'array'
    or jsonb_typeof(p_data->'avatar_equipment') is distinct from 'array'
    or jsonb_typeof(p_data->'purchases') is distinct from 'array'
    or jsonb_typeof(p_data->'natural_objects') is distinct from 'array'
    or jsonb_typeof(p_data->'natural_removals') is distinct from 'array'
    or jsonb_typeof(p_data->'removal_debits') is distinct from 'array'
    or jsonb_typeof(p_data->'effect_history') is distinct from 'array'
  then
    return null;
  end if;

  v_cycle_id := p_data#>>'{current_cycle,cycle_id}';
  if not private.shop_guest_import_string_valid(p_data#>'{current_cycle,cycle_id}', 1, 80) then
    return null;
  end if;

  v_purchase_count := jsonb_array_length(p_data->'purchases');
  if jsonb_array_length(coalesce(p_data->'purchase_proofs', '[]'::jsonb)) <> v_purchase_count
    or jsonb_array_length(p_data->'pending_purchases') <> 0
  then
    return null;
  end if;

  -- Validate and normalize landscape ownership before consulting any proofs.
  for v_row in select value from jsonb_array_elements(p_data->'landscape_instances')
  loop
    if not private.shop_guest_import_has_keys(v_row, array[
        'instance_id','sku','variation_index','seed','variation_version','acquired_at_utc'
      ])
      or jsonb_typeof(v_row->'instance_id') is distinct from 'string'
      or jsonb_typeof(v_row->'sku') is distinct from 'string'
      or not private.shop_guest_import_uint_valid(v_row->'variation_index', 4)
      or not private.shop_guest_import_string_valid(v_row->'seed', 1, 80)
      or not private.shop_guest_import_uint_valid(v_row->'variation_version', 2147483647)
      or (v_row->>'variation_version')::numeric < 1
      or not private.shop_guest_import_timestamp_valid(v_row->'acquired_at_utc')
    then
      return null;
    end if;

    v_instance_id := (v_row->>'instance_id')::uuid;
    if v_instance_id::text is distinct from v_row->>'instance_id'
      or v_seen_instances ? v_instance_id::text
    then
      return null;
    end if;
    v_sku := v_row->>'sku';
    select p.* into v_product from private.shop_products p
    where p.sku = v_sku and p.category = 'landscape' and p.purchasable;
    if not found then
      return null;
    end if;
    v_variation := (v_row->>'variation_index')::smallint;
    if v_seen_variations ? (v_sku || ':' || v_variation::text) then
      return null;
    end if;
    if (select count(*) from jsonb_array_elements(p_data->'landscape_instances') i(value)
        where i.value->>'sku' = v_sku) > 5 then
      return null;
    end if;
    v_seen_instances := v_seen_instances || jsonb_build_object(v_instance_id::text, v_row);
    v_seen_variations := v_seen_variations || jsonb_build_object(v_sku || ':' || v_variation::text, true);
    v_normalized_timestamp := pg_catalog.to_char(
      (v_row->>'acquired_at_utc')::timestamptz at time zone 'UTC',
      'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'
    );
    v_landscape_instances := v_landscape_instances || jsonb_build_array(
      jsonb_build_object(
        'instance_id', v_instance_id::text,
        'sku', v_sku,
        'variation_index', v_variation,
        'seed', v_row->>'seed',
        'variation_version', (v_row->>'variation_version')::integer,
        'acquired_at_utc', v_normalized_timestamp,
        'catalog_revision', v_product.catalog_revision,
        'catalog_price', v_product.price
      )
    );
  end loop;

  -- Purchase IDs, exact typed quotes, status and ownership must cover each row once.
  for v_row in select value from jsonb_array_elements(p_data->'purchases')
  loop
    if not private.shop_guest_import_has_keys(v_row, array[
        'purchase_id','sku','price','purchased_at_utc'
      ])
      or not private.shop_guest_import_string_valid(v_row->'purchase_id', 1, 160)
      or btrim(v_row->>'purchase_id') = ''
      or not private.shop_guest_import_string_valid(v_row->'sku', 1, 64)
      or not private.shop_guest_import_uint_valid(v_row->'price', 9223372036854775807::numeric)
      or (v_row->>'price')::numeric < 1
      or not private.shop_guest_import_timestamp_valid(v_row->'purchased_at_utc')
    then
      return null;
    end if;
    v_request_id := v_row->>'purchase_id';
    if v_seen_purchases ? v_request_id then
      return null;
    end if;
    v_seen_purchases := v_seen_purchases || jsonb_build_object(v_request_id, true);
    v_sku := v_row->>'sku';
    v_price := (v_row->>'price')::bigint;
    select p.* into v_product from private.shop_products p
    where p.sku = v_sku and p.purchasable;
    if not found or v_price > v_product.price then
      return null;
    end if;

    select count(*) into v_match_count
    from jsonb_array_elements(coalesce(p_data->'purchase_proofs', '[]'::jsonb)) q(value)
    where q.value->>'request_id' = v_request_id;
    if v_match_count <> 1 then
      return null;
    end if;
    select q.value into v_proof
    from jsonb_array_elements(coalesce(p_data->'purchase_proofs', '[]'::jsonb)) q(value)
    where q.value->>'request_id' = v_request_id;
    if not private.shop_guest_import_has_keys(v_proof, array[
        'request_id','quote','status','ownership'
      ])
      or jsonb_typeof(v_proof->'request_id') is distinct from 'string'
      or not private.shop_guest_import_string_valid(v_proof->'request_id', 1, 160)
      or v_proof->>'request_id' is distinct from v_request_id
      or v_proof->>'status' is distinct from 'purchased'
      or not private.shop_guest_import_has_keys(v_proof->'quote', array[
        'target','catalog_revision','effect_revision','price'
      ])
      or not private.shop_guest_import_has_keys(v_proof#>'{quote,target}', array['kind','sku'])
      or v_proof#>>'{quote,target,kind}' is distinct from 'purchase'
      or v_proof#>>'{quote,target,sku}' is distinct from v_sku
      or not private.shop_guest_import_uint_valid(v_proof#>'{quote,catalog_revision}', 2147483647)
      or (v_proof#>>'{quote,catalog_revision}')::integer is distinct from v_product.catalog_revision
      or not private.shop_guest_import_uint_valid(v_proof#>'{quote,effect_revision}', 9223372036854775807::numeric)
      or not private.shop_guest_import_uint_valid(v_proof#>'{quote,price}', 9223372036854775807::numeric)
      or (v_proof#>>'{quote,price}')::bigint is distinct from v_price
    then
      return null;
    end if;
    if v_product.category = 'landscape' then
      v_ownership := v_proof->'ownership';
      if not private.shop_guest_import_has_keys(v_ownership, array['kind','instance_id'])
        or v_ownership->>'kind' is distinct from 'landscape'
        or jsonb_typeof(v_ownership->'instance_id') is distinct from 'string'
      then
        return null;
      end if;
      v_instance_id := (v_ownership->>'instance_id')::uuid;
      if v_instance_id::text is distinct from v_ownership->>'instance_id'
        or not (v_seen_instances ? v_instance_id::text)
        or v_seen_landscape_ownership ? v_instance_id::text
      then
        return null;
      end if;
      v_instance := v_seen_instances->v_instance_id::text;
      if v_instance->>'sku' is distinct from v_sku then
        return null;
      end if;
      v_seen_landscape_ownership := v_seen_landscape_ownership
        || jsonb_build_object(v_instance_id::text, true);
      v_landscape_purchase_count := v_landscape_purchase_count + 1;
    else
      v_ownership := v_proof->'ownership';
      if not private.shop_guest_import_has_keys(v_ownership, array['kind','sku'])
        or v_ownership->>'kind' is distinct from 'avatar'
        or v_ownership->>'sku' is distinct from v_sku
      then
        return null;
      end if;
      select count(*) into v_match_count
      from jsonb_array_elements(p_data->'avatar_owned') a(value)
      where a.value->>'purchase_id' = v_request_id and a.value->>'sku' = v_sku;
      if v_match_count <> 1 then
        return null;
      end if;
      v_avatar_purchase_count := v_avatar_purchase_count + 1;
    end if;
    v_seen_proofs := v_seen_proofs || jsonb_build_object(v_request_id, true);
    v_normalized_timestamp := pg_catalog.to_char(
      (v_row->>'purchased_at_utc')::timestamptz at time zone 'UTC',
      'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'
    );
    v_purchases := v_purchases || jsonb_build_array(
      jsonb_build_object(
        'request_id', v_request_id,
        'sku', v_sku,
        'price', v_price,
        'catalog_revision', v_product.catalog_revision,
        'effect_revision', (v_proof#>>'{quote,effect_revision}')::bigint,
        'category', v_product.category,
        'purchased_at_utc', v_normalized_timestamp,
        'ownership', v_ownership
      )
    );
  end loop;

  if jsonb_array_length(p_data->'landscape_instances') <> v_landscape_purchase_count
    or (select count(*) from jsonb_object_keys(v_seen_landscape_ownership))
      <> jsonb_array_length(p_data->'landscape_instances')
    or jsonb_array_length(p_data->'avatar_owned') <> v_avatar_purchase_count
  then
    return null;
  end if;

  -- Validate avatar ownership rows independently, including slot and purchase binding.
  for v_row in select value from jsonb_array_elements(p_data->'avatar_owned')
  loop
    if not private.shop_guest_import_has_keys(v_row, array[
        'sku','purchase_id','price','acquired_at_utc'
      ])
      or not private.shop_guest_import_string_valid(v_row->'sku', 1, 64)
      or not private.shop_guest_import_string_valid(v_row->'purchase_id', 1, 160)
      or not private.shop_guest_import_uint_valid(v_row->'price', 9223372036854775807::numeric)
      or not private.shop_guest_import_timestamp_valid(v_row->'acquired_at_utc')
    then
      return null;
    end if;
    v_sku := v_row->>'sku';
    if v_seen_avatar_skus ? v_sku then
      return null;
    end if;
    v_seen_avatar_skus := v_seen_avatar_skus || jsonb_build_object(v_sku, true);
    select p.* into v_product from private.shop_products p
    where p.sku = v_sku and p.category = 'avatar' and p.purchasable;
    if not found or (v_row->>'price')::numeric > v_product.price then
      return null;
    end if;
    select count(*) into v_match_count
    from jsonb_array_elements(coalesce(p_data->'purchase_proofs', '[]'::jsonb)) q(value)
    where q.value->>'request_id' = v_row->>'purchase_id'
      and q.value#>>'{ownership,kind}' = 'avatar'
      and q.value#>>'{ownership,sku}' = v_sku
      and (q.value#>>'{quote,price}')::numeric = (v_row->>'price')::numeric;
    if v_match_count <> 1 then
      return null;
    end if;
    v_avatar_owned := v_avatar_owned || jsonb_build_array(jsonb_build_object(
      'sku', v_sku,
      'slot', v_product.avatar_slot,
      'purchase_id', v_row->>'purchase_id',
      'price', (v_row->>'price')::bigint,
      'catalog_revision', v_product.catalog_revision,
      'acquired_at_utc', pg_catalog.to_char(
        (v_row->>'acquired_at_utc')::timestamptz at time zone 'UTC',
        'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'
      )
    ));
  end loop;

  -- Edit-version rows cover every acquired landscape instance, placed or stored.
  if jsonb_array_length(p_data->'landscape_edit_versions') <> jsonb_array_length(p_data->'landscape_instances') then
    return null;
  end if;
  for v_row in select value from jsonb_array_elements(p_data->'landscape_edit_versions')
  loop
    if not private.shop_guest_import_has_keys(v_row, array['instance_id','version'])
      or jsonb_typeof(v_row->'instance_id') is distinct from 'string'
      or not private.shop_guest_import_uint_valid(v_row->'version', 9223372036854775807::numeric)
    then
      return null;
    end if;
    v_instance_id := (v_row->>'instance_id')::uuid;
    if v_instance_id::text is distinct from v_row->>'instance_id'
      or not (v_seen_instances ? v_instance_id::text)
      or v_seen_placements ? ('edit:' || v_instance_id::text)
    then
      return null;
    end if;
    v_seen_placements := v_seen_placements || jsonb_build_object('edit:' || v_instance_id::text, true);
  end loop;

  -- Placements must point at an owned instance in the current cycle and fit its catalog zone.
  for v_row in select value from jsonb_array_elements(p_data->'placements')
  loop
    if not private.shop_guest_import_has_keys(v_row, array[
        'instance_id','cycle_id','x','y','version'
      ])
      or jsonb_typeof(v_row->'instance_id') is distinct from 'string'
      or not private.shop_guest_import_string_valid(v_row->'cycle_id', 1, 80)
      or v_row->>'cycle_id' is distinct from v_cycle_id
      or jsonb_typeof(v_row->'x') is distinct from 'number'
      or jsonb_typeof(v_row->'y') is distinct from 'number'
      or not private.shop_guest_import_uint_valid(v_row->'version', 9223372036854775807::numeric)
    then
      return null;
    end if;
    v_instance_id := (v_row->>'instance_id')::uuid;
    if v_instance_id::text is distinct from v_row->>'instance_id'
      or not (v_seen_instances ? v_instance_id::text)
      or v_seen_placements ? v_instance_id::text
    then
      return null;
    end if;
    v_instance := v_seen_instances->v_instance_id::text;
    v_x := (v_row->>'x')::numeric;
    v_y := (v_row->>'y')::numeric;
    if not private.shop_landscape_placement_valid(v_instance->>'sku', v_x, v_y) then
      return null;
    end if;
    v_seen_placements := v_seen_placements || jsonb_build_object(v_instance_id::text, true);
    v_placements := v_placements || jsonb_build_array(jsonb_build_object(
      'instance_id', v_instance_id::text,
      'cycle_id', v_cycle_id,
      'x', v_x,
      'y', v_y,
      'version', (v_row->>'version')::bigint
    ));
  end loop;

  v_effect_timeline := private.shop_guest_import_effect_timeline_normalize(
    p_data, v_landscape_instances, v_placements
  );
  if v_effect_timeline is null then
    return null;
  end if;

  -- Equipment references only typed, owned avatar items with the catalog slot.
  if jsonb_array_length(p_data->'avatar_equipment') > 4 then
    return null;
  end if;
  for v_row in select value from jsonb_array_elements(p_data->'avatar_equipment')
  loop
    if not private.shop_guest_import_has_keys(v_row, array['slot','sku','version'])
      or v_row->>'slot' not in ('head','outfit','face','back')
      or jsonb_typeof(v_row->'sku') not in ('string','null')
      or not private.shop_guest_import_uint_valid(v_row->'version', 9223372036854775807::numeric)
      or v_seen_placements ? ('slot:' || (v_row->>'slot'))
    then
      return null;
    end if;
    v_seen_placements := v_seen_placements || jsonb_build_object('slot:' || (v_row->>'slot'), true);
    if v_row->'sku' <> 'null'::jsonb then
      select p.* into v_product from private.shop_products p
      where p.sku = v_row->>'sku' and p.category = 'avatar';
      if not found or v_product.avatar_slot is distinct from v_row->>'slot'
        or not exists (
          select 1 from jsonb_array_elements(p_data->'avatar_owned') a(value)
          where a.value->>'sku' = v_row->>'sku'
        )
      then
        return null;
      end if;
    end if;
  end loop;

  -- Tombstones, debit rows, and successful removal proofs are a one-to-one set.
  v_removal_count := jsonb_array_length(p_data->'natural_removals');
  if jsonb_array_length(p_data->'removal_debits') <> v_removal_count
    or jsonb_array_length(coalesce(p_data->'removal_proofs', '[]'::jsonb)) <> v_removal_count
  then
    return null;
  end if;
  for v_row in select value from jsonb_array_elements(p_data->'natural_objects')
  loop
    if not private.shop_guest_import_has_keys(v_row, array[
        'cycle_id','stage','ordinal','kind','x','y','seed'
      ])
      or not private.shop_guest_import_string_valid(v_row->'cycle_id', 1, 80)
      or not private.shop_guest_import_uint_valid(v_row->'stage', 4)
      or not private.shop_guest_import_uint_valid(v_row->'ordinal', 2147483647)
      or not private.shop_guest_import_string_valid(v_row->'kind', 1, 80)
      or not private.shop_guest_import_uint_valid(v_row->'x', 255)
      or not private.shop_guest_import_uint_valid(v_row->'y', 255)
      or not private.shop_guest_import_uint_valid(v_row->'seed', 9223372036854775807::numeric)
    then
      return null;
    end if;
    v_removal_key := jsonb_build_array(
      v_row->>'cycle_id', (v_row->>'stage')::integer, (v_row->>'ordinal')::integer
    )::text;
    if v_seen_natural_keys ? v_removal_key then
      return null;
    end if;
    v_seen_natural_keys := v_seen_natural_keys || jsonb_build_object(v_removal_key, true);
  end loop;

  for v_row in select value from jsonb_array_elements(p_data->'natural_removals')
  loop
    if not private.shop_guest_import_has_keys(v_row, array[
        'cycle_id','stage','ordinal','version','removed_at_utc'
      ])
      or not private.shop_guest_import_string_valid(v_row->'cycle_id', 1, 80)
      or not private.shop_guest_shop_import_known_cycle(p_data, v_row->>'cycle_id')
      or not private.shop_guest_import_uint_valid(v_row->'stage', 4)
      or not private.shop_guest_import_uint_valid(v_row->'ordinal', 2147483647)
      or not private.shop_guest_import_uint_valid(v_row->'version', 9223372036854775807::numeric)
      or (v_row->>'version')::numeric < 1
      or not private.shop_guest_import_timestamp_valid(v_row->'removed_at_utc')
    then
      return null;
    end if;
    v_removal_key := jsonb_build_array(
      v_row->>'cycle_id', (v_row->>'stage')::integer, (v_row->>'ordinal')::integer
    )::text;
    if not (v_seen_natural_keys ? v_removal_key)
      or v_seen_removal_keys ? v_removal_key
    then
      return null;
    end if;
    v_seen_removal_keys := v_seen_removal_keys || jsonb_build_object(v_removal_key, true);

    select count(*) into v_match_count
    from jsonb_array_elements(p_data->'removal_debits') d(value)
    join jsonb_array_elements(coalesce(p_data->'removal_proofs', '[]'::jsonb)) q(value)
      on q.value->>'request_id' = d.value->>'request_id'
    where q.value#>>'{target,cycle_id}' = v_row->>'cycle_id'
      and q.value#>>'{target,stage}' = v_row->>'stage'
      and q.value#>>'{target,ordinal}' = v_row->>'ordinal';
    if v_match_count <> 1 then
      return null;
    end if;

    select d.value, q.value into v_instance, v_proof
    from jsonb_array_elements(p_data->'removal_debits') d(value)
    join jsonb_array_elements(coalesce(p_data->'removal_proofs', '[]'::jsonb)) q(value)
      on q.value->>'request_id' = d.value->>'request_id'
    where q.value#>>'{target,cycle_id}' = v_row->>'cycle_id'
      and q.value#>>'{target,stage}' = v_row->>'stage'
      and q.value#>>'{target,ordinal}' = v_row->>'ordinal';
    if not private.shop_guest_import_has_keys(v_instance, array[
        'request_id','amount','created_at_utc'
      ])
      or not private.shop_guest_import_string_valid(v_instance->'request_id', 1, 160)
      or not private.shop_guest_import_uint_valid(v_instance->'amount', 9223372036854775807::numeric)
      or (v_instance->>'amount')::numeric < 1
      or not private.shop_guest_import_timestamp_valid(v_instance->'created_at_utc')
      or not private.shop_guest_import_has_keys(v_proof, array[
        'request_id','target','quote','status'
      ])
      or jsonb_typeof(v_proof->'request_id') is distinct from 'string'
      or not private.shop_guest_import_string_valid(v_proof->'request_id', 1, 160)
      or v_proof->>'status' is distinct from 'removed'
      or v_proof->>'request_id' is distinct from v_instance->>'request_id'
      or not private.shop_guest_import_has_keys(v_proof->'target', array[
        'cycle_id','stage','ordinal'
      ])
      or not private.shop_guest_import_string_valid(v_proof#>'{target,cycle_id}', 1, 80)
      or jsonb_typeof(v_proof#>'{target,stage}') is distinct from 'number'
      or not private.shop_guest_import_uint_valid(v_proof#>'{target,stage}', 4)
      or jsonb_typeof(v_proof#>'{target,ordinal}') is distinct from 'number'
      or not private.shop_guest_import_uint_valid(v_proof#>'{target,ordinal}', 2147483647)
      or (v_proof#>>'{target,stage}')::integer is distinct from (v_row->>'stage')::integer
      or (v_proof#>>'{target,ordinal}')::integer is distinct from (v_row->>'ordinal')::integer
      or not private.shop_guest_import_has_keys(v_proof->'quote', array[
        'target','catalog_revision','effect_revision','price'
      ])
      or not private.shop_guest_import_has_keys(v_proof#>'{quote,target}', array['kind','key'])
      or v_proof#>>'{quote,target,kind}' is distinct from 'remove_natural'
      or not private.shop_guest_import_has_keys(v_proof#>'{quote,target,key}', array[
        'cycle_id','stage','ordinal'
      ])
      or not private.shop_guest_import_string_valid(
        v_proof#>'{quote,target,key,cycle_id}', 1, 80
      )
      or jsonb_typeof(v_proof#>'{quote,target,key,stage}') is distinct from 'number'
      or not private.shop_guest_import_uint_valid(
        v_proof#>'{quote,target,key,stage}', 4
      )
      or jsonb_typeof(v_proof#>'{quote,target,key,ordinal}') is distinct from 'number'
      or not private.shop_guest_import_uint_valid(
        v_proof#>'{quote,target,key,ordinal}', 2147483647
      )
      or v_proof#>>'{quote,target,key,cycle_id}' is distinct from v_row->>'cycle_id'
      or (v_proof#>>'{quote,target,key,stage}')::integer is distinct from (v_row->>'stage')::integer
      or (v_proof#>>'{quote,target,key,ordinal}')::integer is distinct from (v_row->>'ordinal')::integer
      or not private.shop_guest_import_uint_valid(v_proof#>'{quote,catalog_revision}', 2147483647)
      or (v_proof#>>'{quote,catalog_revision}')::integer <> 1
      or not private.shop_guest_import_uint_valid(v_proof#>'{quote,effect_revision}', 9223372036854775807::numeric)
      or (v_proof#>>'{quote,effect_revision}')::numeric <> 0
      or not private.shop_guest_import_uint_valid(v_proof#>'{quote,price}', 9223372036854775807::numeric)
      or (v_proof#>>'{quote,price}')::numeric is distinct from (v_instance->>'amount')::numeric
    then
      return null;
    end if;

    v_stage := (v_row->>'stage')::integer;
    v_expected_removal_price := private.shop_natural_removal_price(
      case v_stage when 0 then 100000::bigint when 1 then 250000::bigint
        when 2 then 500000::bigint when 3 then 1000000::bigint else 2000000::bigint end,
      0
    );
    if (v_instance->>'amount')::bigint is distinct from v_expected_removal_price then
      return null;
    end if;
    if v_seen_removal_ids ? (v_instance->>'request_id') then
      return null;
    end if;
    v_seen_removal_ids := v_seen_removal_ids || jsonb_build_object(v_instance->>'request_id', true);
    v_normalized_timestamp := pg_catalog.to_char(
      (v_row->>'removed_at_utc')::timestamptz at time zone 'UTC',
      'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'
    );
    v_natural_removals := v_natural_removals || jsonb_build_array(jsonb_build_object(
      'cycle_id', v_row->>'cycle_id',
      'stage', v_stage,
      'ordinal', (v_row->>'ordinal')::integer,
      'version', (v_row->>'version')::bigint,
      'removed_at_utc', v_normalized_timestamp,
      'request_id', v_instance->>'request_id',
      'price', v_expected_removal_price
    ));
  end loop;

  if jsonb_array_length(p_data->'natural_objects') < v_removal_count
    or jsonb_array_length(v_natural_removals) <> v_removal_count
    or (select count(*) from jsonb_object_keys(v_seen_removal_ids)) <> v_removal_count
  then
    return null;
  end if;
  return jsonb_build_object(
    'landscape_instances', v_landscape_instances,
    'avatar_owned', v_avatar_owned,
    'placements', v_placements,
    'purchases', v_purchases,
    'natural_removals', v_natural_removals,
    'purchase_count', v_purchase_count,
    'removal_count', v_removal_count
  ) || v_effect_timeline;
exception when others then
  return null;
end;
$$;

create or replace function private.shop_guest_shop_import_known_cycle(p_data jsonb, p_cycle_id text)
returns boolean
language sql
immutable
set search_path = ''
as $$
  select p_cycle_id = p_data#>>'{current_cycle,cycle_id}'
    or exists (
      select 1 from jsonb_array_elements(coalesce(p_data->'historical_cycles', '[]'::jsonb)) c(value)
      where c.value->>'cycle_id' = p_cycle_id
  );
$$;

create or replace function private.shop_guest_import_effect_timeline_normalize(
  p_data jsonb,
  p_landscape_instances jsonb,
  p_placements jsonb
)
returns jsonb
language plpgsql
stable
set search_path = ''
as $$
declare
  v_row jsonb;
  v_cycle jsonb;
  v_interval jsonb;
  v_bound jsonb;
  v_instance jsonb;
  v_active_value jsonb;
  v_product private.shop_products%rowtype;
  v_current_cycle_id text;
  v_current_cycle_started_at timestamptz;
  v_cycle_id text;
  v_instance_id text;
  v_last_instance_id text;
  v_revision bigint;
  v_max_revision bigint := 0;
  v_started_at timestamptz;
  v_ended_at timestamptz;
  v_bound_started_at timestamptz;
  v_bound_ended_at timestamptz;
  v_server_time timestamptz;
  v_timeline jsonb;
  v_authoritative boolean;
  v_seen_bounds jsonb := '{}'::jsonb;
  v_seen_revisions jsonb := '{}'::jsonb;
  v_seen_active_instances jsonb := '{}'::jsonb;
  v_normalized_bounds jsonb := '[]'::jsonb;
  v_sorted_bounds jsonb := '[]'::jsonb;
  v_normalized_intervals jsonb := '[]'::jsonb;
  v_sorted_intervals jsonb := '[]'::jsonb;
  v_expected_effects jsonb;
  v_claimed_effects jsonb;
  v_open_current_ids jsonb;
  v_open_current_count integer := 0;
  v_token_earning numeric;
  v_civilization_growth numeric;
  v_shop_discount numeric;
  v_reset_cooldown numeric;
  v_natural_removal_discount numeric;
  v_era_reward numeric;
  v_streak_reward numeric;
  v_history_row jsonb;
  v_previous_start timestamptz;
  v_previous_end timestamptz;
  v_previous_revision bigint;
begin
  if jsonb_typeof(p_data) is distinct from 'object'
    or jsonb_typeof(p_data->'effect_cycle_bounds_authoritative') is distinct from 'boolean'
    or jsonb_typeof(p_data->'effect_cycle_bounds') is distinct from 'array'
    or jsonb_typeof(p_data->'effect_history') is distinct from 'array'
    or jsonb_typeof(p_landscape_instances) is distinct from 'array'
    or jsonb_typeof(p_placements) is distinct from 'array'
    or not private.shop_guest_import_string_valid(p_data#>'{current_cycle,cycle_id}', 1, 80)
    or not private.shop_guest_import_timestamp_valid(p_data#>'{current_cycle,started_at_utc}')
  then
    return null;
  end if;

  v_current_cycle_id := p_data#>>'{current_cycle,cycle_id}';
  v_current_cycle_started_at := (p_data#>>'{current_cycle,started_at_utc}')::timestamptz;
  v_authoritative := (p_data->>'effect_cycle_bounds_authoritative')::boolean;
  v_timeline := p_data->'effect_timeline_state';

  if not v_authoritative then
    if jsonb_array_length(p_data->'effect_cycle_bounds') <> 0
      or jsonb_array_length(p_data->'effect_history') <> 0
    then
      return null;
    end if;
  else
    for v_row in select value from jsonb_array_elements(p_data->'effect_cycle_bounds')
    loop
      if not private.shop_guest_import_has_keys(v_row, array[
          'cycle_id','started_at_utc','ended_at_utc'
        ])
        or not private.shop_guest_import_string_valid(v_row->'cycle_id', 1, 80)
        or not private.shop_guest_import_timestamp_valid(v_row->'started_at_utc')
        or not private.shop_guest_import_timestamp_valid(v_row->'ended_at_utc', true)
      then
        return null;
      end if;

      v_cycle_id := v_row->>'cycle_id';
      if v_seen_bounds ? v_cycle_id
        or not private.shop_guest_shop_import_known_cycle(p_data, v_cycle_id)
      then
        return null;
      end if;
      v_seen_bounds := v_seen_bounds || jsonb_build_object(v_cycle_id, true);
      v_bound_started_at := (v_row->>'started_at_utc')::timestamptz;
      v_bound_ended_at := case when v_row->'ended_at_utc' = 'null'::jsonb then null
        else (v_row->>'ended_at_utc')::timestamptz end;
      if v_bound_ended_at is not null and v_bound_ended_at <= v_bound_started_at then
        return null;
      end if;

      if v_cycle_id = v_current_cycle_id then
        if v_bound_started_at is distinct from v_current_cycle_started_at
          or v_bound_ended_at is not null
        then
          return null;
        end if;
      else
        if v_bound_ended_at is null then
          return null;
        end if;
        select c.value into v_cycle
        from jsonb_array_elements(coalesce(p_data->'historical_cycles', '[]'::jsonb)) c(value)
        where c.value->>'cycle_id' = v_cycle_id;
        if found then
          if v_cycle->'started_at_utc' <> 'null'::jsonb
            and (v_cycle->>'started_at_utc')::timestamptz is distinct from v_bound_started_at
          then
            return null;
          end if;
          if v_cycle->'ended_at_utc' <> 'null'::jsonb
            and (v_cycle->>'ended_at_utc')::timestamptz is distinct from v_bound_ended_at
          then
            return null;
          end if;
        end if;
      end if;

      v_normalized_bounds := v_normalized_bounds || jsonb_build_array(jsonb_build_object(
        'cycle_id', v_cycle_id,
        'started_at_utc', pg_catalog.to_char(
          v_bound_started_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'
        ),
        'ended_at_utc', case when v_bound_ended_at is null then null else pg_catalog.to_char(
          v_bound_ended_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'
        ) end
      ));
    end loop;

    if not (v_seen_bounds ? v_current_cycle_id) then
      return null;
    end if;
    if exists (
      select 1 from (
        select b.value->>'cycle_id' as cycle_id,
          (b.value->>'started_at_utc')::timestamptz as started_at,
          nullif(b.value->>'ended_at_utc', '')::timestamptz as ended_at,
          lag((b.value->>'started_at_utc')::timestamptz)
            over (order by (b.value->>'started_at_utc')::timestamptz, b.value->>'cycle_id') as previous_start,
          lag(nullif(b.value->>'ended_at_utc', '')::timestamptz)
            over (order by (b.value->>'started_at_utc')::timestamptz, b.value->>'cycle_id') as previous_end
        from jsonb_array_elements(v_normalized_bounds) b(value)
      ) ordered_bounds
      where previous_start is not null
        and (previous_start >= started_at or previous_end is null or previous_end > started_at)
    ) or exists (
      select 1 from jsonb_array_elements(v_normalized_bounds) b(value)
      where b.value->>'cycle_id' <> v_current_cycle_id
        and (b.value->>'started_at_utc')::timestamptz >= v_current_cycle_started_at
    ) then
      return null;
    end if;
  end if;

  for v_interval in select value from jsonb_array_elements(p_data->'effect_history')
  loop
    if not v_authoritative
      or not private.shop_guest_import_has_keys(v_interval, array[
        'cycle_id','revision','started_at_utc','ended_at_utc','active_instance_ids','effects'
      ])
      or not private.shop_guest_import_string_valid(v_interval->'cycle_id', 1, 80)
      or not private.shop_guest_import_uint_valid(v_interval->'revision', 9223372036854775807::numeric)
      or (v_interval->>'revision')::numeric < 1
      or not private.shop_guest_import_timestamp_valid(v_interval->'started_at_utc')
      or not private.shop_guest_import_timestamp_valid(v_interval->'ended_at_utc', true)
      or jsonb_typeof(v_interval->'active_instance_ids') is distinct from 'array'
    then
      return null;
    end if;

    v_cycle_id := v_interval->>'cycle_id';
    v_revision := (v_interval->>'revision')::bigint;
    v_started_at := (v_interval->>'started_at_utc')::timestamptz;
    v_ended_at := case when v_interval->'ended_at_utc' = 'null'::jsonb then null
      else (v_interval->>'ended_at_utc')::timestamptz end;
    select b.value into v_bound
    from jsonb_array_elements(v_normalized_bounds) b(value)
    where b.value->>'cycle_id' = v_cycle_id;
    if not found
      or v_seen_revisions ? v_revision::text
      or not private.shop_guest_shop_import_known_cycle(p_data, v_cycle_id)
      or v_started_at < (v_bound->>'started_at_utc')::timestamptz
      or (v_ended_at is not null and v_ended_at <= v_started_at)
      or ((v_bound->'ended_at_utc' <> 'null'::jsonb)
        and (v_ended_at is null or v_ended_at > (v_bound->>'ended_at_utc')::timestamptz))
    then
      return null;
    end if;
    v_seen_revisions := v_seen_revisions || jsonb_build_object(v_revision::text, true);
    v_max_revision := greatest(v_max_revision, v_revision);

    v_token_earning := 0;
    v_civilization_growth := 0;
    v_shop_discount := 0;
    v_reset_cooldown := 0;
    v_natural_removal_discount := 0;
    v_era_reward := 0;
    v_streak_reward := 0;
    v_seen_active_instances := '{}'::jsonb;
    v_last_instance_id := null;
    for v_active_value in select value from jsonb_array_elements(v_interval->'active_instance_ids')
    loop
      if jsonb_typeof(v_active_value) is distinct from 'string' then
        return null;
      end if;
      v_instance_id := v_active_value #>> '{}';
      if v_instance_id !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
        or (v_instance_id::uuid)::text is distinct from v_instance_id
        or (v_last_instance_id is not null
          and v_instance_id collate "C" <= v_last_instance_id collate "C")
        or v_seen_active_instances ? v_instance_id
      then
        return null;
      end if;
      v_last_instance_id := v_instance_id;
      v_seen_active_instances := v_seen_active_instances || jsonb_build_object(v_instance_id, true);

      select i.value into v_instance
      from jsonb_array_elements(p_landscape_instances) i(value)
      where i.value->>'instance_id' = v_instance_id;
      if not found
        or (v_instance->>'acquired_at_utc')::timestamptz > v_started_at
      then
        return null;
      end if;

      select p.* into v_product
      from private.shop_products p
      where p.sku = v_instance->>'sku'
        and p.category = 'landscape' and p.purchasable;
      if not found then
        return null;
      end if;
      case v_product.effect_type
        when 'token_earning' then v_token_earning := v_token_earning + v_product.effect_value::numeric;
        when 'civilization_growth' then v_civilization_growth := v_civilization_growth + v_product.effect_value::numeric;
        when 'shop_discount' then v_shop_discount := v_shop_discount + v_product.effect_value::numeric;
        when 'reset_cooldown' then v_reset_cooldown := v_reset_cooldown + v_product.effect_value::numeric;
        when 'natural_removal_discount' then v_natural_removal_discount := v_natural_removal_discount + v_product.effect_value::numeric;
        when 'era_reward' then v_era_reward := v_era_reward + v_product.effect_value::numeric;
        when 'streak_reward' then v_streak_reward := v_streak_reward + v_product.effect_value::numeric;
        else return null;
      end case;
    end loop;

    v_expected_effects := jsonb_build_object(
      'token_earning_bps', least(v_token_earning, 3000)::bigint,
      'civilization_growth_bps', least(v_civilization_growth, 2000)::bigint,
      'shop_discount_bps', least(v_shop_discount, 1500)::bigint,
      'reset_cooldown_bps', least(v_reset_cooldown, 2500)::bigint,
      'natural_removal_discount_bps', least(v_natural_removal_discount, 3000)::bigint,
      'era_reward_tokens', least(v_era_reward, 10000000)::bigint,
      'streak_reward_tokens', least(v_streak_reward, 500000)::bigint
    );
    v_claimed_effects := v_interval->'effects';
    if not private.shop_guest_import_has_keys(v_claimed_effects, array[
        'token_earning_bps','civilization_growth_bps','shop_discount_bps',
        'reset_cooldown_bps','natural_removal_discount_bps','era_reward_tokens',
        'streak_reward_tokens'
      ])
      or not private.shop_guest_import_uint_valid(v_claimed_effects->'token_earning_bps', 3000)
      or not private.shop_guest_import_uint_valid(v_claimed_effects->'civilization_growth_bps', 2000)
      or not private.shop_guest_import_uint_valid(v_claimed_effects->'shop_discount_bps', 1500)
      or not private.shop_guest_import_uint_valid(v_claimed_effects->'reset_cooldown_bps', 2500)
      or not private.shop_guest_import_uint_valid(v_claimed_effects->'natural_removal_discount_bps', 3000)
      or not private.shop_guest_import_uint_valid(v_claimed_effects->'era_reward_tokens', 10000000)
      or not private.shop_guest_import_uint_valid(v_claimed_effects->'streak_reward_tokens', 500000)
      or v_claimed_effects is distinct from v_expected_effects
    then
      return null;
    end if;

    if v_ended_at is null then
      if v_cycle_id <> v_current_cycle_id then
        return null;
      end if;
      v_open_current_count := v_open_current_count + 1;
      v_open_current_ids := v_interval->'active_instance_ids';
    end if;
    v_normalized_intervals := v_normalized_intervals || jsonb_build_array(jsonb_build_object(
      'cycle_id', v_cycle_id,
      'revision', v_revision,
      'started_at_utc', pg_catalog.to_char(
        v_started_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'
      ),
      'ended_at_utc', case when v_ended_at is null then null else pg_catalog.to_char(
        v_ended_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'
      ) end,
      'active_instance_ids', v_interval->'active_instance_ids',
      'effects', v_expected_effects
    ));
  end loop;

  if v_open_current_count > 1
    or (jsonb_array_length(p_placements) > 0 and v_open_current_count <> 1)
  then
    return null;
  end if;
  if v_open_current_count = 1 then
    if jsonb_array_length(v_open_current_ids) <> jsonb_array_length(p_placements)
      or exists (
        select 1 from jsonb_array_elements(p_placements) p(value)
        where not (v_open_current_ids ? (p.value->>'instance_id'))
      )
    then
      return null;
    end if;
  end if;

  select coalesce(jsonb_agg(i.value order by
    (i.value->>'started_at_utc')::timestamptz, (i.value->>'revision')::bigint
  ), '[]'::jsonb)
  into v_sorted_intervals
  from jsonb_array_elements(v_normalized_intervals) i(value);
  v_previous_start := null;
  v_previous_end := null;
  v_previous_revision := null;
  for v_history_row in select value from jsonb_array_elements(v_sorted_intervals)
  loop
    v_started_at := (v_history_row->>'started_at_utc')::timestamptz;
    v_ended_at := case when v_history_row->'ended_at_utc' = 'null'::jsonb then null
      else (v_history_row->>'ended_at_utc')::timestamptz end;
    v_revision := (v_history_row->>'revision')::bigint;
    if v_previous_start is not null
      and (v_previous_start >= v_started_at or v_previous_revision >= v_revision
        or v_previous_end is null or v_previous_end > v_started_at)
    then
      return null;
    end if;
    v_previous_start := v_started_at;
    v_previous_end := v_ended_at;
    v_previous_revision := v_revision;
  end loop;

  if v_timeline is not null and v_timeline <> 'null'::jsonb then
    if not private.shop_guest_import_has_keys(v_timeline, array[
        'current_cycle_id','effect_revision','server_time_utc','reward_timezone'
      ])
      or not private.shop_guest_import_string_valid(v_timeline->'current_cycle_id', 1, 80)
      or v_timeline->>'current_cycle_id' is distinct from v_current_cycle_id
      or not private.shop_guest_import_uint_valid(v_timeline->'effect_revision', 9223372036854775807::numeric)
      or not private.shop_guest_import_timestamp_valid(v_timeline->'server_time_utc')
      or not private.shop_guest_import_timezone_valid(v_timeline->'reward_timezone')
      or v_timeline->>'reward_timezone' is distinct from p_data->>'reward_timezone'
      or (v_timeline->>'effect_revision')::bigint is distinct from v_max_revision
    then
      return null;
    end if;
    v_server_time := (v_timeline->>'server_time_utc')::timestamptz;
  elsif jsonb_array_length(p_data->'effect_history') > 0
    or jsonb_array_length(p_placements) > 0
    or v_max_revision <> 0
  then
    return null;
  end if;

  if v_server_time is not null and (
    exists (
      select 1 from jsonb_array_elements(v_sorted_intervals) i(value)
      where (i.value->>'started_at_utc')::timestamptz > v_server_time
        or (i.value->'ended_at_utc' <> 'null'::jsonb
          and (i.value->>'ended_at_utc')::timestamptz > v_server_time)
    )
    or exists (
      select 1 from jsonb_array_elements(v_normalized_bounds) b(value)
      where (b.value->>'started_at_utc')::timestamptz > v_server_time
        or (b.value->'ended_at_utc' <> 'null'::jsonb
          and (b.value->>'ended_at_utc')::timestamptz > v_server_time)
    )
  ) then
    return null;
  end if;

  select coalesce(jsonb_agg(b.value order by
    (b.value->>'started_at_utc')::timestamptz, b.value->>'cycle_id'
  ), '[]'::jsonb)
  into v_sorted_bounds
  from jsonb_array_elements(v_normalized_bounds) b(value);

  return jsonb_build_object(
    'effect_cycle_bounds', v_sorted_bounds,
    'effect_history', v_sorted_intervals
  );
exception when others then
  return null;
end;
$$;

-- Reconcile the captured usage rows against their daily mirrors and the one
-- guest device's acknowledged growth-journal rows. This is a partial source
-- normalizer only; it does not validate effect contributions or authorize import.
create or replace function private.shop_guest_import_usage_core_normalize(p_data jsonb)
returns jsonb
language plpgsql
stable
set search_path = ''
as $$
declare
  v_row jsonb;
  v_current jsonb;
  v_state jsonb;
  v_cycle_row jsonb;
  v_journal_row jsonb;
  v_cycle_id text;
  v_current_cycle_id text;
  v_bucket_date text;
  v_agent text;
  v_key text;
  v_device_id text;
  v_timezone text;
  v_coverage text;
  v_present boolean;
  v_generation bigint;
  v_revision bigint;
  v_acknowledged_revision bigint;
  v_tokens numeric;
  v_event_count numeric;
  v_raw_count integer := 0;
  v_daily_count integer := 0;
  v_present_count integer := 0;
  v_max bigint := 9223372036854775807;
  v_raw_total numeric := 0;
  v_current_total numeric := 0;
  v_cycle_total numeric;
  v_expected_cycle_total numeric;
  v_raw_by_key jsonb := '{}'::jsonb;
  v_daily_by_key jsonb := '{}'::jsonb;
  v_cycle_totals jsonb := '{}'::jsonb;
  v_expected_cycle_totals jsonb := '{}'::jsonb;
  v_cycle_rows jsonb := '{}'::jsonb;
  v_journal_keys jsonb := '{}'::jsonb;
  v_known_cycles jsonb := '{}'::jsonb;
begin
  if jsonb_typeof(p_data) is distinct from 'object'
    or jsonb_typeof(p_data->'usage_aggregates') is distinct from 'array'
    or jsonb_typeof(p_data->'daily_agent_totals') is distinct from 'array'
    or jsonb_typeof(p_data->'cycle_usage_totals') is distinct from 'array'
    or jsonb_typeof(p_data->'growth_journal_cycles') is distinct from 'array'
    or jsonb_typeof(p_data->'growth_journal_entries') is distinct from 'array'
    or not (p_data ? 'growth_journal_state')
    or jsonb_typeof(p_data->'growth_journal_state') not in ('null','object')
    or not private.shop_guest_import_timezone_valid(p_data->'world_timezone')
    or not private.shop_guest_import_timezone_valid(p_data->'planet_timezone')
    or not private.shop_guest_import_timezone_valid(p_data->'reward_timezone')
    or p_data->>'world_timezone' is distinct from p_data->>'planet_timezone'
    or p_data->>'world_timezone' is distinct from p_data->>'reward_timezone'
    or jsonb_typeof(p_data->'planet_device_id') is distinct from 'string'
    or p_data->>'planet_device_id' !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
    or ((p_data->>'planet_device_id')::uuid)::text is distinct from p_data->>'planet_device_id'
    or not private.shop_guest_import_cycle_valid(p_data->'current_cycle')
    or jsonb_typeof(p_data->'historical_cycles') is distinct from 'array'
  then
    return null;
  end if;

  v_current := p_data->'current_cycle';
  v_current_cycle_id := v_current->>'cycle_id';
  if v_current->'is_current' is distinct from 'true'::jsonb
    or v_current->'ended_at_utc' is distinct from 'null'::jsonb
  then
    return null;
  end if;
  v_known_cycles := jsonb_build_object(v_current_cycle_id, true);
  for v_row in select value from jsonb_array_elements(p_data->'historical_cycles')
  loop
    if not private.shop_guest_import_cycle_valid(v_row)
      or v_row->'is_current' is distinct from 'false'::jsonb
      or v_known_cycles ? (v_row->>'cycle_id')
    then
      return null;
    end if;
    v_known_cycles := v_known_cycles || jsonb_build_object(v_row->>'cycle_id', true);
  end loop;

  v_state := p_data->'growth_journal_state';
  if v_state <> 'null'::jsonb then
    if not private.shop_guest_import_has_keys(v_state, array['generation','deleted_at_utc'])
      or not private.shop_guest_import_uint_valid(v_state->'generation', v_max::numeric)
      or not private.shop_guest_import_timestamp_valid(v_state->'deleted_at_utc', true)
      or v_state->'deleted_at_utc' <> 'null'::jsonb
    then
      return null;
    end if;
    v_generation := (v_state->>'generation')::bigint;
  end if;

  for v_row in select value from jsonb_array_elements(p_data->'growth_journal_cycles')
  loop
    if not private.shop_guest_import_has_keys(v_row, array[
        'cycle_id','started_at_utc','ended_at_utc','wallet_credit','wallet_credit_at_utc'
      ])
      or not private.shop_guest_import_string_valid(v_row->'cycle_id', 1, 80)
      or not (v_known_cycles ? (v_row->>'cycle_id'))
      or not private.shop_guest_import_timestamp_valid(v_row->'started_at_utc', true)
      or not private.shop_guest_import_timestamp_valid(v_row->'ended_at_utc', true)
      or not private.shop_guest_import_uint_valid(v_row->'wallet_credit', v_max::numeric, true)
      or not private.shop_guest_import_timestamp_valid(v_row->'wallet_credit_at_utc', true)
      or (v_row->'wallet_credit' = 'null'::jsonb)
        is distinct from (v_row->'wallet_credit_at_utc' = 'null'::jsonb)
      or v_cycle_rows ? (v_row->>'cycle_id')
    then
      return null;
    end if;
    if v_row->'started_at_utc' <> 'null'::jsonb
      and v_row->'ended_at_utc' <> 'null'::jsonb
      and (v_row->>'ended_at_utc')::timestamptz <= (v_row->>'started_at_utc')::timestamptz
    then
      return null;
    end if;
    if v_row->>'cycle_id' = v_current_cycle_id
      and (v_row->'started_at_utc' = 'null'::jsonb
        or (v_row->>'started_at_utc')::timestamptz
          is distinct from (v_current->>'started_at_utc')::timestamptz
        or v_row->'ended_at_utc' <> 'null'::jsonb)
    then
      return null;
    end if;
    v_cycle_rows := v_cycle_rows || jsonb_build_object(v_row->>'cycle_id', v_row);
  end loop;

  for v_row in select value from jsonb_array_elements(p_data->'usage_aggregates')
  loop
    if not private.shop_guest_import_has_keys(v_row, array[
        'cycle_id','bucket_date','agent','event_count','total_tokens','coverage'
      ])
      or not private.shop_guest_import_string_valid(v_row->'cycle_id', 1, 80)
      or not (v_known_cycles ? (v_row->>'cycle_id'))
      or not private.shop_guest_import_string_valid(v_row->'bucket_date', 10, 10)
      or (v_row->>'bucket_date') !~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'
      or (v_row->>'bucket_date')::date::text is distinct from v_row->>'bucket_date'
      or not private.shop_guest_import_string_valid(v_row->'agent', 1, 32)
      or v_row->>'agent' not in ('codex','claude_code')
      or not private.shop_guest_import_uint_valid(v_row->'event_count', v_max::numeric)
      or (v_row->>'event_count')::numeric = 0
      or not private.shop_guest_import_uint_valid(v_row->'total_tokens', v_max::numeric)
      or jsonb_typeof(v_row->'coverage') is distinct from 'string'
      or v_row->>'coverage' <> 'complete'
    then
      return null;
    end if;
    v_cycle_id := v_row->>'cycle_id';
    v_bucket_date := v_row->>'bucket_date';
    v_agent := v_row->>'agent';
    v_key := jsonb_build_array(v_bucket_date, v_agent)::text;
    v_tokens := (v_row->>'total_tokens')::numeric;
    if v_raw_by_key ? v_key or not (v_cycle_rows ? v_cycle_id)
      or v_cycle_rows#>>array[v_cycle_id,'started_at_utc'] is null
      or v_cycle_rows#>>array[v_cycle_id,'started_at_utc'] = 'null'
    then
      return null;
    end if;
    v_raw_by_key := v_raw_by_key || jsonb_build_object(v_key, v_row);
    v_raw_total := v_raw_total + v_tokens;
    if v_cycle_id = v_current_cycle_id then
      v_current_total := v_current_total + v_tokens;
    end if;
    if v_raw_total > v_max or v_current_total > v_max then
      return null;
    end if;
    v_raw_count := v_raw_count + 1;
  end loop;

  for v_row in select value from jsonb_array_elements(p_data->'daily_agent_totals')
  loop
    if not private.shop_guest_import_has_keys(v_row, array[
        'bucket_date','agent','total_tokens','coverage'
      ])
      or not private.shop_guest_import_string_valid(v_row->'bucket_date', 10, 10)
      or (v_row->>'bucket_date') !~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'
      or (v_row->>'bucket_date')::date::text is distinct from v_row->>'bucket_date'
      or not private.shop_guest_import_string_valid(v_row->'agent', 1, 32)
      or v_row->>'agent' not in ('codex','claude_code')
      or not private.shop_guest_import_uint_valid(v_row->'total_tokens', v_max::numeric)
      or jsonb_typeof(v_row->'coverage') is distinct from 'string'
      or v_row->>'coverage' <> 'complete'
    then
      return null;
    end if;
    v_key := jsonb_build_array(v_row->>'bucket_date', v_row->>'agent')::text;
    if v_daily_by_key ? v_key then
      return null;
    end if;
    v_daily_by_key := v_daily_by_key || jsonb_build_object(v_key, v_row);
    v_daily_count := v_daily_count + 1;
  end loop;

  if v_daily_count <> v_raw_count
    or exists (
      select 1 from jsonb_each(v_raw_by_key) r(key, value)
      where not (v_daily_by_key ? r.key)
        or (v_daily_by_key#>>array[r.key,'total_tokens']) is distinct from r.value->>'total_tokens'
        or (v_daily_by_key#>>array[r.key,'coverage']) is distinct from r.value->>'coverage'
    )
  then
    return null;
  end if;

  for v_row in select value from jsonb_array_elements(p_data->'growth_journal_entries')
  loop
    if not private.shop_guest_import_has_keys(v_row, array[
        'device_id','cycle_id','bucket_date','agent','revision',
        'acknowledged_revision','generation','present','confirmed_tokens','coverage','payload_hash'
      ])
      or jsonb_typeof(v_row->'device_id') is distinct from 'string'
      or v_row->>'device_id' is distinct from p_data->>'planet_device_id'
      or not private.shop_guest_import_string_valid(v_row->'cycle_id', 1, 80)
      or not (v_known_cycles ? (v_row->>'cycle_id'))
      or not private.shop_guest_import_string_valid(v_row->'bucket_date', 10, 10)
      or (v_row->>'bucket_date') !~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'
      or (v_row->>'bucket_date')::date::text is distinct from v_row->>'bucket_date'
      or jsonb_typeof(v_row->'agent') is distinct from 'string'
      or v_row->>'agent' not in ('codex','claude_code')
      or not private.shop_guest_import_uint_valid(v_row->'revision', v_max::numeric)
      or (v_row->>'revision')::numeric < 1
      or not private.shop_guest_import_uint_valid(v_row->'acknowledged_revision', v_max::numeric)
      or not private.shop_guest_import_uint_valid(v_row->'generation', v_max::numeric)
      or jsonb_typeof(v_row->'present') is distinct from 'boolean'
      or not private.shop_guest_import_uint_valid(v_row->'confirmed_tokens', v_max::numeric, true)
      or jsonb_typeof(v_row->'coverage') is distinct from 'string'
      or v_row->>'coverage' not in ('complete','partial','unavailable','unsupported','user_disabled')
      or jsonb_typeof(v_row->'payload_hash') is distinct from 'string'
      or v_row->>'payload_hash' !~ '^[0-9a-f]{64}$'
    then
      return null;
    end if;
    v_cycle_id := v_row->>'cycle_id';
    if not (v_cycle_rows ? v_cycle_id)
      or v_cycle_rows#>>array[v_cycle_id,'started_at_utc'] is null
      or v_cycle_rows#>>array[v_cycle_id,'started_at_utc'] = 'null'
    then
      return null;
    end if;
    v_bucket_date := v_row->>'bucket_date';
    v_agent := v_row->>'agent';
    v_key := jsonb_build_array(v_cycle_id, v_bucket_date, v_agent)::text;
    if v_journal_keys ? v_key then
      return null;
    end if;
    v_journal_keys := v_journal_keys || jsonb_build_object(v_key, true);
    v_present := (v_row->>'present')::boolean;
    v_revision := (v_row->>'revision')::bigint;
    v_acknowledged_revision := (v_row->>'acknowledged_revision')::bigint;
    if v_acknowledged_revision > v_revision
      or v_state = 'null'::jsonb
      or (v_row->>'generation')::bigint is distinct from v_generation
    then
      return null;
    end if;
    v_key := jsonb_build_array(v_bucket_date, v_agent)::text;
    if v_present then
      if v_row->'confirmed_tokens' = 'null'::jsonb or v_row->>'coverage' <> 'complete'
        or not (v_raw_by_key ? v_key)
        or v_raw_by_key#>>array[v_key,'cycle_id'] is distinct from v_cycle_id
        or v_raw_by_key#>>array[v_key,'total_tokens'] is distinct from v_row->>'confirmed_tokens'
        or v_raw_by_key#>>array[v_key,'coverage'] is distinct from v_row->>'coverage'
      then
        return null;
      end if;
      v_tokens := (v_row->>'confirmed_tokens')::numeric;
      v_expected_cycle_total := coalesce((v_expected_cycle_totals->>v_cycle_id)::numeric, 0);
      v_expected_cycle_total := v_expected_cycle_total + v_tokens;
      if v_expected_cycle_total > v_max then
        return null;
      end if;
      v_expected_cycle_totals := v_expected_cycle_totals
        || jsonb_build_object(v_cycle_id, v_expected_cycle_total);
      v_present_count := v_present_count + 1;
    elsif v_row->'confirmed_tokens' <> 'null'::jsonb then
      return null;
    end if;
  end loop;

  if v_present_count <> v_raw_count then
    return null;
  end if;

  for v_row in select value from jsonb_array_elements(p_data->'cycle_usage_totals')
  loop
    if not private.shop_guest_import_has_keys(v_row, array['cycle_id','total_tokens'])
      or not private.shop_guest_import_string_valid(v_row->'cycle_id', 1, 80)
      or not (v_known_cycles ? (v_row->>'cycle_id'))
      or not private.shop_guest_import_uint_valid(v_row->'total_tokens', v_max::numeric)
    then
      return null;
    end if;
    v_cycle_id := v_row->>'cycle_id';
    if v_cycle_totals ? v_cycle_id then
      return null;
    end if;
    v_cycle_totals := v_cycle_totals || jsonb_build_object(v_cycle_id, v_row->'total_tokens');
  end loop;

  if exists (
      select 1 from jsonb_each(v_expected_cycle_totals) e(key, value)
      where not (v_cycle_totals ? e.key)
        or (v_cycle_totals->>e.key)::numeric is distinct from (e.value#>>'{}')::numeric
    ) or exists (
      select 1 from jsonb_each(v_cycle_totals) e(key, value)
      where not (v_expected_cycle_totals ? e.key)
    )
  then
    return null;
  end if;

  if v_raw_count = 0 then
    if jsonb_array_length(p_data->'growth_journal_entries') <> 0
      or (
        p_data->'lifetime_usage_tokens' = 'null'::jsonb
        and p_data->'current_cycle_usage_tokens' <> 'null'::jsonb
      )
      or (
        p_data->'current_cycle_usage_tokens' = 'null'::jsonb
        and p_data->'lifetime_usage_tokens' <> 'null'::jsonb
      )
    then
      return null;
    end if;
    if p_data->'lifetime_usage_tokens' = 'null'::jsonb then
      v_raw_total := null;
      v_current_total := null;
    elsif private.shop_guest_import_uint_valid(p_data->'lifetime_usage_tokens', v_max::numeric)
      and private.shop_guest_import_uint_valid(p_data->'current_cycle_usage_tokens', v_max::numeric)
      and (p_data->>'lifetime_usage_tokens')::numeric = 0
      and (p_data->>'current_cycle_usage_tokens')::numeric = 0
    then
      v_raw_total := 0;
      v_current_total := 0;
    else
      return null;
    end if;
  else
    if v_state = 'null'::jsonb
      or not private.shop_guest_import_uint_valid(p_data->'lifetime_usage_tokens', v_max::numeric)
      or not private.shop_guest_import_uint_valid(p_data->'current_cycle_usage_tokens', v_max::numeric)
      or (p_data->>'lifetime_usage_tokens')::numeric is distinct from v_raw_total
      or (p_data->>'current_cycle_usage_tokens')::numeric is distinct from v_current_total
    then
      return null;
    end if;
  end if;

  return jsonb_build_object(
    'validation_scope', 'usage_journal_core',
    'usage_aggregates', p_data->'usage_aggregates',
    'daily_agent_totals', p_data->'daily_agent_totals',
    'cycle_usage_totals', p_data->'cycle_usage_totals',
    'lifetime_usage_tokens', v_raw_total,
    'current_cycle_usage_tokens', v_current_total,
    'growth_journal_state', p_data->'growth_journal_state',
    'growth_journal_cycles', p_data->'growth_journal_cycles',
    'growth_journal_entries', p_data->'growth_journal_entries'
  );
exception when others then
  return null;
end;
$$;

-- Reconcile normalized effect contribution segments and activity-day summaries
-- against the usage/journal core. This is not ledger authenticity or credit proof.
create or replace function private.shop_guest_import_usage_normalize(
  p_data jsonb,
  p_effect_timeline jsonb
)
returns jsonb
language plpgsql
stable
set search_path = ''
as $$
declare
  v_core jsonb;
  v_row jsonb;
  v_bound jsonb;
  v_interval jsonb;
  v_effects jsonb;
  v_contribution jsonb;
  v_activity jsonb;
  v_cycle_id text;
  v_current_cycle_id text;
  v_date_text text;
  v_key text;
  v_revision bigint;
  v_version bigint;
  v_tokens numeric;
  v_group_total numeric;
  v_date_total numeric;
  v_weighted_total numeric;
  v_max bigint := 9223372036854775807;
  v_timezone text;
  v_device_id text;
  v_date date;
  v_day_start timestamptz;
  v_day_end timestamptz;
  v_bound_start timestamptz;
  v_bound_end timestamptz;
  v_interval_start timestamptz;
  v_interval_end timestamptz;
  v_first_effect_start timestamptz;
  v_server_time timestamptz;
  v_segment_start timestamptz;
  v_segment_end timestamptz;
  v_timestamp timestamptz;
  v_actual_growth_bps bigint;
  v_actual_wallet_bps bigint;
  v_positive_dates integer := 0;
  v_activity_count integer := 0;
  v_bound_map jsonb := '{}'::jsonb;
  v_history_map jsonb := '{}'::jsonb;
  v_seen_revisions jsonb := '{}'::jsonb;
  v_seen_contributions jsonb := '{}'::jsonb;
  v_seen_activities jsonb := '{}'::jsonb;
  v_expected_groups jsonb := '{}'::jsonb;
  v_actual_groups jsonb := '{}'::jsonb;
  v_contribution_dates jsonb := '{}'::jsonb;
  v_weighted_growth jsonb := '{}'::jsonb;
  v_segments jsonb := '[]'::jsonb;
  v_growth_rows jsonb := '[]'::jsonb;
  v_normalized_bounds jsonb := '[]'::jsonb;
  v_normalized_history jsonb := '[]'::jsonb;
begin
  v_core := private.shop_guest_import_usage_core_normalize(p_data);
  if v_core is null
    or jsonb_typeof(p_effect_timeline) is distinct from 'object'
    or not private.shop_guest_import_has_keys(
      p_effect_timeline, array['effect_cycle_bounds','effect_history']
    )
    or jsonb_typeof(p_effect_timeline->'effect_cycle_bounds') is distinct from 'array'
    or jsonb_typeof(p_effect_timeline->'effect_history') is distinct from 'array'
    or jsonb_typeof(p_data->'effect_contributions') is distinct from 'array'
    or jsonb_typeof(p_data->'activity_days') is distinct from 'array'
    or not (p_data ? 'effect_timeline_state')
    or jsonb_typeof(p_data->'effect_timeline_state') not in ('null','object')
  then
    return null;
  end if;

  v_timezone := p_data->>'planet_timezone';
  v_device_id := p_data->>'planet_device_id';
  v_current_cycle_id := p_data#>>'{current_cycle,cycle_id}';
  if p_data->'effect_timeline_state' <> 'null'::jsonb then
    if not private.shop_guest_import_has_keys(p_data->'effect_timeline_state', array[
        'current_cycle_id','effect_revision','server_time_utc','reward_timezone'
      ])
      or not private.shop_guest_import_string_valid(
        p_data#>'{effect_timeline_state,current_cycle_id}', 1, 80
      )
      or p_data#>>'{effect_timeline_state,current_cycle_id}' is distinct from v_current_cycle_id
      or not private.shop_guest_import_uint_valid(
        p_data#>'{effect_timeline_state,effect_revision}', v_max::numeric
      )
      or not private.shop_guest_import_timestamp_valid(
        p_data#>'{effect_timeline_state,server_time_utc}'
      )
      or not private.shop_guest_import_timezone_valid(
        p_data#>'{effect_timeline_state,reward_timezone}'
      )
      or p_data#>>'{effect_timeline_state,reward_timezone}' is distinct from v_timezone
    then
      return null;
    end if;
    v_server_time := (p_data#>>'{effect_timeline_state,server_time_utc}')::timestamptz;
  elsif jsonb_array_length(p_effect_timeline->'effect_history') > 0 then
    return null;
  end if;
  if jsonb_array_length(p_data->'effect_contributions') > 0
      or jsonb_array_length(p_data->'activity_days') > 0 then
    if not private.shop_guest_import_uint_valid(
      p_data->'contribution_canonical_version', v_max::numeric
    ) or (p_data->>'contribution_canonical_version')::numeric < 1 then
      return null;
    end if;
    v_version := (p_data->>'contribution_canonical_version')::bigint;
  elsif p_data->'contribution_canonical_version' <> 'null'::jsonb then
    if not private.shop_guest_import_uint_valid(
      p_data->'contribution_canonical_version', v_max::numeric
    ) then
      return null;
    end if;
    v_version := (p_data->>'contribution_canonical_version')::bigint;
  end if;

  for v_row in select value from jsonb_array_elements(p_effect_timeline->'effect_cycle_bounds')
  loop
    if not private.shop_guest_import_has_keys(v_row, array[
        'cycle_id','started_at_utc','ended_at_utc'
      ])
      or not private.shop_guest_import_string_valid(v_row->'cycle_id', 1, 80)
      or not private.shop_guest_shop_import_known_cycle(p_data, v_row->>'cycle_id')
      or not private.shop_guest_import_timestamp_valid(v_row->'started_at_utc')
      or not private.shop_guest_import_timestamp_valid(v_row->'ended_at_utc', true)
      or v_bound_map ? (v_row->>'cycle_id')
    then
      return null;
    end if;
    v_cycle_id := v_row->>'cycle_id';
    v_bound_start := (v_row->>'started_at_utc')::timestamptz;
    v_bound_end := case when v_row->'ended_at_utc' = 'null'::jsonb then null
      else (v_row->>'ended_at_utc')::timestamptz end;
    if v_bound_end is not null and v_bound_end <= v_bound_start then
      return null;
    end if;
    if v_cycle_id = v_current_cycle_id then
      if v_bound_start is distinct from (p_data#>>'{current_cycle,started_at_utc}')::timestamptz
        or v_bound_end is not null
      then
        return null;
      end if;
    elsif v_bound_end is null then
      return null;
    end if;
    v_bound_map := v_bound_map || jsonb_build_object(v_cycle_id, v_row);
    v_normalized_bounds := v_normalized_bounds || jsonb_build_array(v_row);
  end loop;

  if (jsonb_array_length(p_data->'effect_contributions') > 0
      or jsonb_array_length(p_data->'activity_days') > 0)
    and not (v_bound_map ? v_current_cycle_id)
  then
    return null;
  end if;

  for v_interval in select value from jsonb_array_elements(p_effect_timeline->'effect_history')
  loop
    if not private.shop_guest_import_has_keys(v_interval, array[
        'cycle_id','revision','started_at_utc','ended_at_utc','active_instance_ids','effects'
      ])
      or not private.shop_guest_import_string_valid(v_interval->'cycle_id', 1, 80)
      or not private.shop_guest_import_uint_valid(v_interval->'revision', v_max::numeric)
      or (v_interval->>'revision')::numeric < 1
      or not private.shop_guest_import_timestamp_valid(v_interval->'started_at_utc')
      or not private.shop_guest_import_timestamp_valid(v_interval->'ended_at_utc', true)
      or jsonb_typeof(v_interval->'active_instance_ids') is distinct from 'array'
      or not (v_bound_map ? (v_interval->>'cycle_id'))
    then
      return null;
    end if;
    v_cycle_id := v_interval->>'cycle_id';
    v_revision := (v_interval->>'revision')::bigint;
    v_key := jsonb_build_array(v_cycle_id, v_revision)::text;
    if v_seen_revisions ? v_revision::text or v_history_map ? v_key then
      return null;
    end if;
    v_seen_revisions := v_seen_revisions || jsonb_build_object(v_revision::text, true);
    v_bound := v_bound_map->v_cycle_id;
    v_bound_start := (v_bound->>'started_at_utc')::timestamptz;
    v_bound_end := case when v_bound->'ended_at_utc' = 'null'::jsonb then null
      else (v_bound->>'ended_at_utc')::timestamptz end;
    v_interval_start := (v_interval->>'started_at_utc')::timestamptz;
    v_interval_end := case when v_interval->'ended_at_utc' = 'null'::jsonb then null
      else (v_interval->>'ended_at_utc')::timestamptz end;
    if v_interval_start < v_bound_start
      or (v_bound_end is not null and
        (v_interval_end is null or v_interval_end > v_bound_end))
      or (v_interval_end is not null and v_interval_end <= v_interval_start)
      or (v_interval_end is null and v_cycle_id <> v_current_cycle_id)
    then
      return null;
    end if;
    v_effects := v_interval->'effects';
    if not private.shop_guest_import_has_keys(v_effects, array[
        'token_earning_bps','civilization_growth_bps','shop_discount_bps',
        'reset_cooldown_bps','natural_removal_discount_bps','era_reward_tokens',
        'streak_reward_tokens'
      ])
      or not private.shop_guest_import_uint_valid(v_effects->'token_earning_bps', 3000)
      or not private.shop_guest_import_uint_valid(v_effects->'civilization_growth_bps', 2000)
      or not private.shop_guest_import_uint_valid(v_effects->'shop_discount_bps', 1500)
      or not private.shop_guest_import_uint_valid(v_effects->'reset_cooldown_bps', 2500)
      or not private.shop_guest_import_uint_valid(v_effects->'natural_removal_discount_bps', 3000)
      or not private.shop_guest_import_uint_valid(v_effects->'era_reward_tokens', 10000000)
      or not private.shop_guest_import_uint_valid(v_effects->'streak_reward_tokens', 500000)
    then
      return null;
    end if;
    v_history_map := v_history_map || jsonb_build_object(v_key, v_interval);
    v_normalized_history := v_normalized_history || jsonb_build_array(v_interval);
  end loop;

  if exists (
    select 1 from jsonb_array_elements(p_data->'growth_journal_entries') j(value)
    where j.value->'present' = 'true'::jsonb
  ) then
    for v_row in select value from jsonb_array_elements(p_data->'growth_journal_entries')
    loop
      if v_row->'present' <> 'true'::jsonb then continue; end if;
      v_cycle_id := v_row->>'cycle_id';
      v_date_text := v_row->>'bucket_date';
      v_key := jsonb_build_array(v_cycle_id, v_date_text)::text;
      v_group_total := coalesce((v_expected_groups->>v_key)::numeric, 0)
        + (v_row->>'confirmed_tokens')::numeric;
      if v_group_total > v_max then return null; end if;
      v_expected_groups := v_expected_groups || jsonb_build_object(v_key, v_group_total);
    end loop;
  end if;

  for v_contribution in select value from jsonb_array_elements(p_data->'effect_contributions')
  loop
    if not private.shop_guest_import_has_keys(v_contribution, array[
        'device_id','cycle_id','date','effect_revision','canonical_version',
        'tokens','growth_bps','wallet_bps'
      ])
      or jsonb_typeof(v_contribution->'device_id') is distinct from 'string'
      or v_contribution->>'device_id' is distinct from v_device_id
      or not private.shop_guest_import_string_valid(v_contribution->'cycle_id', 1, 80)
      or not private.shop_guest_shop_import_known_cycle(p_data, v_contribution->>'cycle_id')
      or not private.shop_guest_import_string_valid(v_contribution->'date', 10, 10)
      or (v_contribution->>'date') !~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'
      or (v_contribution->>'date')::date::text is distinct from v_contribution->>'date'
      or not private.shop_guest_import_uint_valid(v_contribution->'effect_revision', v_max::numeric)
      or not private.shop_guest_import_uint_valid(v_contribution->'canonical_version', v_max::numeric)
      or (v_contribution->>'canonical_version')::bigint is distinct from v_version
      or not private.shop_guest_import_uint_valid(v_contribution->'tokens', v_max::numeric)
      or not private.shop_guest_import_uint_valid(v_contribution->'growth_bps', 2000)
      or not private.shop_guest_import_uint_valid(v_contribution->'wallet_bps', 3000)
    then
      return null;
    end if;
    v_cycle_id := v_contribution->>'cycle_id';
    v_date_text := v_contribution->>'date';
    v_date := v_date_text::date;
    v_revision := (v_contribution->>'effect_revision')::bigint;
    v_tokens := (v_contribution->>'tokens')::numeric;
    v_key := jsonb_build_array(v_device_id, v_cycle_id, v_date_text, v_revision)::text;
    if v_seen_contributions ? v_key or not (v_bound_map ? v_cycle_id) then
      return null;
    end if;
    v_seen_contributions := v_seen_contributions || jsonb_build_object(v_key, true);
    v_bound := v_bound_map->v_cycle_id;
    v_bound_start := (v_bound->>'started_at_utc')::timestamptz;
    v_bound_end := case when v_bound->'ended_at_utc' = 'null'::jsonb then null
      else (v_bound->>'ended_at_utc')::timestamptz end;
    v_day_start := (v_date::timestamp) at time zone v_timezone;
    v_day_end := ((v_date + 1)::timestamp) at time zone v_timezone;
    v_segment_start := greatest(v_bound_start, v_day_start);
    v_segment_end := v_day_end;
    if v_bound_end is not null and v_bound_end < v_segment_end then
      v_segment_end := v_bound_end;
    end if;

    if v_revision = 0 then
      if (v_contribution->>'growth_bps')::bigint <> 0
        or (v_contribution->>'wallet_bps')::bigint <> 0
      then
        return null;
      end if;
      select min((h.value->>'started_at_utc')::timestamptz)
      into v_first_effect_start
      from jsonb_array_elements(v_normalized_history) h(value)
      where h.value->>'cycle_id' = v_cycle_id;
      if v_first_effect_start is not null
        and v_first_effect_start < v_segment_end
      then
        v_segment_end := v_first_effect_start;
      end if;
      if v_segment_start >= v_segment_end then
        return null;
      end if;
    else
      v_key := jsonb_build_array(v_cycle_id, v_revision)::text;
      if not (v_history_map ? v_key) then
        return null;
      end if;
      v_interval := v_history_map->v_key;
      v_interval_start := (v_interval->>'started_at_utc')::timestamptz;
      v_interval_end := case when v_interval->'ended_at_utc' = 'null'::jsonb then null
        else (v_interval->>'ended_at_utc')::timestamptz end;
      v_segment_start := greatest(v_segment_start, v_interval_start);
      if v_interval_end is not null and v_interval_end < v_segment_end then
        v_segment_end := v_interval_end;
      end if;
      if v_segment_start >= v_segment_end
        or (v_contribution->>'growth_bps')::bigint
          is distinct from (v_interval#>>'{effects,civilization_growth_bps}')::bigint
        or (v_contribution->>'wallet_bps')::bigint
          is distinct from (v_interval#>>'{effects,token_earning_bps}')::bigint
      then
        return null;
      end if;
    end if;

    v_key := jsonb_build_array(v_cycle_id, v_date_text)::text;
    v_group_total := coalesce((v_actual_groups->>v_key)::numeric, 0) + v_tokens;
    v_date_total := coalesce((v_contribution_dates->>v_date_text)::numeric, 0) + v_tokens;
    v_weighted_total := coalesce((v_weighted_growth->>v_key)::numeric, 0)
      + v_tokens * (v_contribution->>'growth_bps')::numeric;
    if v_group_total > v_max or v_date_total > v_max or v_weighted_total > v_max::numeric * 2000 then
      return null;
    end if;
    v_actual_groups := v_actual_groups || jsonb_build_object(v_key, v_group_total);
    v_contribution_dates := v_contribution_dates || jsonb_build_object(v_date_text, v_date_total);
    v_weighted_growth := v_weighted_growth || jsonb_build_object(v_key, v_weighted_total);
    v_segments := v_segments || jsonb_build_array(jsonb_build_object(
      'cycle_id', v_cycle_id, 'date', v_date_text, 'tokens', v_tokens,
      'started_at_utc', pg_catalog.to_char(v_segment_start at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
      'ended_at_utc', pg_catalog.to_char(v_segment_end at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
    ));
  end loop;

  if exists (
      select 1 from jsonb_each(v_expected_groups) e(key, value)
      where not (v_actual_groups ? e.key)
        or (v_actual_groups->>e.key)::numeric is distinct from (e.value#>>'{}')::numeric
    ) or exists (
      select 1 from jsonb_each(v_actual_groups) a(key, value)
      where not (v_expected_groups ? a.key)
    )
  then
    return null;
  end if;

  for v_key, v_group_total in
    select key, (value#>>'{}')::numeric from jsonb_each(v_contribution_dates)
  loop
    if v_group_total > 0 then v_positive_dates := v_positive_dates + 1; end if;
  end loop;

  for v_activity in select value from jsonb_array_elements(p_data->'activity_days')
  loop
    if not private.shop_guest_import_has_keys(v_activity, array[
        'reward_date','cycle_id','first_occurred_at_utc','canonical_version','tokens'
      ])
      or not private.shop_guest_import_string_valid(v_activity->'reward_date', 10, 10)
      or (v_activity->>'reward_date') !~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'
      or (v_activity->>'reward_date')::date::text is distinct from v_activity->>'reward_date'
      or not private.shop_guest_import_string_valid(v_activity->'cycle_id', 1, 80)
      or not private.shop_guest_shop_import_known_cycle(p_data, v_activity->>'cycle_id')
      or not private.shop_guest_import_timestamp_valid(v_activity->'first_occurred_at_utc')
      or not private.shop_guest_import_uint_valid(v_activity->'canonical_version', v_max::numeric)
      or (v_activity->>'canonical_version')::bigint is distinct from v_version
      or not private.shop_guest_import_uint_valid(v_activity->'tokens', v_max::numeric)
      or (v_activity->>'tokens')::numeric = 0
      or not (v_bound_map ? (v_activity->>'cycle_id'))
    then
      return null;
    end if;
    v_date_text := v_activity->>'reward_date';
    v_cycle_id := v_activity->>'cycle_id';
    v_timestamp := (v_activity->>'first_occurred_at_utc')::timestamptz;
    if (v_server_time is not null and v_timestamp > v_server_time)
      or (v_timestamp at time zone v_timezone)::date is distinct from v_date_text::date
      or v_seen_activities ? v_date_text
      or not (v_contribution_dates ? v_date_text)
      or (v_contribution_dates->>v_date_text)::numeric
        is distinct from (v_activity->>'tokens')::numeric
    then
      return null;
    end if;
    v_bound := v_bound_map->v_cycle_id;
    v_bound_start := (v_bound->>'started_at_utc')::timestamptz;
    v_bound_end := case when v_bound->'ended_at_utc' = 'null'::jsonb then null
      else (v_bound->>'ended_at_utc')::timestamptz end;
    if v_timestamp < v_bound_start or (v_bound_end is not null and v_timestamp >= v_bound_end)
      or exists (
        select 1 from jsonb_array_elements(v_segments) s(value)
        where s.value->>'date' = v_date_text
          and (s.value->>'tokens')::numeric > 0
          and (s.value->>'ended_at_utc')::timestamptz <= v_timestamp
      )
      or not exists (
        select 1 from jsonb_array_elements(v_segments) s(value)
        where s.value->>'cycle_id' = v_cycle_id
          and s.value->>'date' = v_date_text
          and (s.value->>'tokens')::numeric > 0
          and v_timestamp >= (s.value->>'started_at_utc')::timestamptz
          and v_timestamp < (s.value->>'ended_at_utc')::timestamptz
      )
    then
      return null;
    end if;
    v_seen_activities := v_seen_activities || jsonb_build_object(v_date_text, true);
    v_activity_count := v_activity_count + 1;
  end loop;

  if v_activity_count <> v_positive_dates
    or exists (
      select 1 from jsonb_each(v_contribution_dates) d(key, value)
      where (d.value#>>'{}')::numeric > 0
        and not (v_seen_activities ? d.key)
    )
  then
    return null;
  end if;

  select coalesce(jsonb_agg(jsonb_build_object(
      'cycle_id', (g.key::jsonb)->>0,
      'date', (g.key::jsonb)->>1,
      'tokens', (g.value#>>'{}')::numeric,
      'weighted_growth_bps', coalesce((v_weighted_growth->>g.key)::numeric, 0),
      'growth_credit', case when (g.value#>>'{}')::numeric = 0 then 0::numeric else
        pg_catalog.ln(1 + (g.value#>>'{}')::numeric / 100000)
          / pg_catalog.ln(2)
          * (1 + coalesce((v_weighted_growth->>g.key)::numeric, 0)
            / ((g.value#>>'{}')::numeric * 10000)) end
    ) order by (g.key::jsonb)->>1, (g.key::jsonb)->>0), '[]'::jsonb)
  into v_growth_rows
  from jsonb_each(v_actual_groups) g(key, value);

  return v_core || jsonb_build_object(
    'validation_scope', 'usage_effect_activity_consistency',
    'effect_cycle_bounds', v_normalized_bounds,
    'effect_history', v_normalized_history,
    'effect_contributions', p_data->'effect_contributions',
    'activity_days', p_data->'activity_days',
    'daily_growth', v_growth_rows
  );
exception when others then
  return null;
end;
$$;

-- Reconstruct a current-only source or one fully evidenced reset. Reset
-- credits here are consistency output only; this helper performs no writes and
-- does not authorize wallet or reward changes.
create or replace function private.shop_guest_import_reset_normalize(
  p_data jsonb,
  p_usage jsonb,
  p_effect_timeline jsonb
)
returns jsonb
language plpgsql
stable
set search_path = ''
as $$
declare
  v_current_cycle_id text;
  v_current_start timestamptz;
  v_server_time timestamptz;
  v_proof jsonb;
  v_old_cycle jsonb;
  v_old_bound jsonb;
  v_new_bound jsonb;
  v_old_journal jsonb;
  v_current_journal jsonb;
  v_settlement jsonb;
  v_claim jsonb;
  v_final_interval jsonb;
  v_contribution jsonb;
  v_contribution_cycle_id text;
  v_reset_at timestamptz;
  v_reset_available timestamptz;
  v_expected_available timestamptz;
  v_old_start timestamptz;
  v_old_end timestamptz;
  v_new_start timestamptz;
  v_server_max constant numeric := 9223372036854775807::numeric;
  v_raw_tokens numeric := 0;
  v_weighted_tokens numeric := 0;
  v_tokens numeric;
  v_wallet_bps numeric;
  v_bonus numeric;
  v_total numeric;
  v_delay_seconds bigint;
  v_settlement_amount numeric;
  v_declared_bonus numeric;
  v_raw_claim_amount numeric;
  v_cycle_credits jsonb := '[]'::jsonb;
  v_reset_chain jsonb := '[]'::jsonb;
begin
  if jsonb_typeof(p_data) is distinct from 'object'
    or jsonb_typeof(p_usage) is distinct from 'object'
    or p_usage->>'validation_scope' is distinct from 'usage_effect_activity_consistency'
    or jsonb_typeof(p_effect_timeline) is distinct from 'object'
    or jsonb_typeof(p_data->'historical_cycles') is distinct from 'array'
    or jsonb_typeof(p_data->'cycle_settlements') is distinct from 'array'
    or jsonb_typeof(p_data->'unverified_planet_wallet_claims') is distinct from 'array'
    or jsonb_typeof(p_data->'reset_settlement_proofs') is distinct from 'array'
    or jsonb_typeof(p_data->'growth_journal_cycles') is distinct from 'array'
    or jsonb_typeof(p_usage->'effect_contributions') is distinct from 'array'
    or jsonb_typeof(p_effect_timeline->'effect_cycle_bounds') is distinct from 'array'
    or jsonb_typeof(p_effect_timeline->'effect_history') is distinct from 'array'
    or p_usage->'effect_cycle_bounds' is distinct from p_effect_timeline->'effect_cycle_bounds'
    or p_usage->'effect_history' is distinct from p_effect_timeline->'effect_history'
    or not private.shop_guest_import_string_valid(p_data#>'{current_cycle,cycle_id}', 1, 80)
    or jsonb_typeof(p_data->'current_cycle') is distinct from 'object'
    or p_data#>'{current_cycle,is_current}' is distinct from 'true'::jsonb
    or not private.shop_guest_import_timestamp_valid(p_data#>'{current_cycle,started_at_utc}')
    or p_data#>'{current_cycle,ended_at_utc}' is distinct from 'null'::jsonb
    or not private.shop_guest_import_timestamp_valid(p_data->'last_reset_at_utc', true)
    or not private.shop_guest_import_timestamp_valid(p_data->'reset_available_at_utc', true)
    or jsonb_typeof(p_data->'reset_receipts_unverifiable') is distinct from 'boolean'
    or p_data->'reset_receipts_unverifiable' is distinct from 'false'::jsonb
    or jsonb_typeof(p_data->'legacy_partial_import_pending') is distinct from 'boolean'
    or p_data->'legacy_partial_import_pending' is distinct from 'false'::jsonb
  then
    return null;
  end if;

  v_current_cycle_id := p_data#>>'{current_cycle,cycle_id}';
  v_current_start := (p_data#>>'{current_cycle,started_at_utc}')::timestamptz;
  if jsonb_typeof(p_data->'effect_timeline_state') is distinct from 'object'
    or not private.shop_guest_import_timestamp_valid(p_data#>'{effect_timeline_state,server_time_utc}')
    or p_data#>>'{effect_timeline_state,current_cycle_id}' is distinct from v_current_cycle_id
  then
    return null;
  end if;
  v_server_time := (p_data#>>'{effect_timeline_state,server_time_utc}')::timestamptz;

  if jsonb_array_length(p_data->'reset_settlement_proofs') = 0 then
    if p_data->'last_reset_at_utc' is distinct from 'null'::jsonb
      or p_data->'reset_available_at_utc' is distinct from 'null'::jsonb
      or jsonb_array_length(p_data->'historical_cycles') <> 0
      or jsonb_array_length(p_data->'cycle_settlements') <> 0
      or jsonb_array_length(p_data->'unverified_planet_wallet_claims') <> 0
      or jsonb_array_length(p_effect_timeline->'effect_cycle_bounds') <> 1
      or jsonb_array_length(p_data->'growth_journal_cycles') <> 1
    then
      return null;
    end if;
    select value into v_new_bound
    from jsonb_array_elements(p_effect_timeline->'effect_cycle_bounds') b(value);
    v_current_journal := p_data#>'{growth_journal_cycles,0}';
    if v_new_bound->>'cycle_id' is distinct from v_current_cycle_id
      or (v_new_bound->>'started_at_utc')::timestamptz is distinct from v_current_start
      or v_new_bound->'ended_at_utc' is distinct from 'null'::jsonb
      or jsonb_typeof(v_current_journal) is distinct from 'object'
      or v_current_journal->>'cycle_id' is distinct from v_current_cycle_id
      or v_current_journal->'ended_at_utc' is distinct from 'null'::jsonb
      or v_current_journal->'wallet_credit' is distinct from 'null'::jsonb
      or v_current_journal->'wallet_credit_at_utc' is distinct from 'null'::jsonb
    then
      return null;
    end if;
    return jsonb_build_object(
      'validation_scope', 'reset_settlement_consistency',
      'reset_chain', v_reset_chain,
      'cycle_token_credits', v_cycle_credits
    );
  end if;

  -- The current capture only carries a frozen availability timestamp for the
  -- reset that created the current cycle. Older reset deadlines are missing,
  -- so multi-reset chains remain held instead of inferring their deadlines.
  if jsonb_array_length(p_data->'reset_settlement_proofs') <> 1
    or jsonb_array_length(p_data->'historical_cycles') <> 1
    or jsonb_array_length(p_data->'cycle_settlements') <> 1
    or jsonb_array_length(p_data->'unverified_planet_wallet_claims') <> 1
    or jsonb_array_length(p_data->'growth_journal_cycles') <> 2
    or jsonb_array_length(p_effect_timeline->'effect_cycle_bounds') <> 2
    or not private.shop_guest_import_timestamp_valid(p_data->'last_reset_at_utc')
    or not private.shop_guest_import_timestamp_valid(p_data->'reset_available_at_utc')
  then
    return null;
  end if;

  select value into v_proof
  from jsonb_array_elements(p_data->'reset_settlement_proofs') p(value);
  if not private.shop_guest_import_has_keys(v_proof, array[
      'request_id','previous_cycle_id','new_cycle_id','reset_at_utc','raw_wallet_claim',
      'settled_bonus_tokens','final_effect_revision','final_effects',
      'final_active_instance_ids','old_cycle_started_at_utc','new_cycle_started_at_utc',
      'reset_available_at_utc'
    ])
    or not private.shop_guest_import_string_valid(v_proof->'request_id', 1, 200)
    or not private.shop_guest_import_string_valid(v_proof->'previous_cycle_id', 1, 80)
    or not private.shop_guest_import_string_valid(v_proof->'new_cycle_id', 1, 80)
    or v_proof->>'previous_cycle_id' is not distinct from v_proof->>'new_cycle_id'
    or not private.shop_guest_import_timestamp_valid(v_proof->'reset_at_utc')
    or jsonb_typeof(v_proof->'raw_wallet_claim') is distinct from 'object'
    or not private.shop_guest_import_uint_valid(v_proof->'settled_bonus_tokens', v_server_max)
    or not private.shop_guest_import_uint_valid(v_proof->'final_effect_revision', v_server_max)
    or jsonb_typeof(v_proof->'final_effects') is distinct from 'object'
    or jsonb_typeof(v_proof->'final_active_instance_ids') is distinct from 'array'
    or not private.shop_guest_import_timestamp_valid(v_proof->'old_cycle_started_at_utc')
    or not private.shop_guest_import_timestamp_valid(v_proof->'new_cycle_started_at_utc')
    or not private.shop_guest_import_timestamp_valid(v_proof->'reset_available_at_utc')
  then
    return null;
  end if;

  v_current_start := (p_data#>>'{current_cycle,started_at_utc}')::timestamptz;
  v_reset_at := (v_proof->>'reset_at_utc')::timestamptz;
  v_reset_available := (v_proof->>'reset_available_at_utc')::timestamptz;
  if v_reset_at > v_server_time
    or (p_data->>'last_reset_at_utc')::timestamptz is distinct from v_reset_at
    or (p_data->>'reset_available_at_utc')::timestamptz is distinct from v_reset_available
    or v_proof->>'new_cycle_id' is distinct from v_current_cycle_id
    or (v_proof->>'new_cycle_started_at_utc')::timestamptz is distinct from v_current_start
  then
    return null;
  end if;

  select value into v_old_cycle
  from jsonb_array_elements(p_data->'historical_cycles') c(value)
  where c.value->'cycle_id' = to_jsonb(v_proof->>'previous_cycle_id');
  if not found
    or not private.shop_guest_import_has_keys(v_old_cycle, array[
      'cycle_id','started_at_utc','ended_at_utc','is_current','settled_bonus_tokens'
    ])
    or v_old_cycle->'is_current' is distinct from 'false'::jsonb
    or not private.shop_guest_import_string_valid(v_old_cycle->'cycle_id', 1, 80)
    or v_old_cycle->'cycle_id' is distinct from to_jsonb(v_proof->>'previous_cycle_id')
    or not private.shop_guest_import_timestamp_valid(v_old_cycle->'started_at_utc')
    or not private.shop_guest_import_timestamp_valid(v_old_cycle->'ended_at_utc')
    or not private.shop_guest_import_uint_valid(v_old_cycle->'settled_bonus_tokens', v_server_max)
  then
    return null;
  end if;
  v_old_start := (v_old_cycle->>'started_at_utc')::timestamptz;
  v_old_end := (v_old_cycle->>'ended_at_utc')::timestamptz;
  if v_old_end is distinct from v_reset_at
    or (v_proof->>'old_cycle_started_at_utc')::timestamptz is distinct from v_old_start
  then
    return null;
  end if;

  select value into v_old_bound
  from jsonb_array_elements(p_effect_timeline->'effect_cycle_bounds') b(value)
  where b.value->>'cycle_id' = v_proof->>'previous_cycle_id';
  select value into v_new_bound
  from jsonb_array_elements(p_effect_timeline->'effect_cycle_bounds') b(value)
  where b.value->>'cycle_id' = v_current_cycle_id;
  if not found
    or v_old_bound is null
    or v_new_bound is null
    or (v_old_bound->>'started_at_utc')::timestamptz is distinct from v_old_start
    or (v_old_bound->>'ended_at_utc')::timestamptz is distinct from v_reset_at
    or (v_new_bound->>'started_at_utc')::timestamptz is distinct from v_reset_at
    or v_new_bound->'ended_at_utc' is distinct from 'null'::jsonb
  then
    return null;
  end if;

  select value into v_old_journal
  from jsonb_array_elements(p_data->'growth_journal_cycles') j(value)
  where j.value->'cycle_id' = to_jsonb(v_proof->>'previous_cycle_id');
  select value into v_current_journal
  from jsonb_array_elements(p_data->'growth_journal_cycles') j(value)
  where j.value->'cycle_id' = to_jsonb(v_current_cycle_id);
  if not found
    or v_old_journal is null
    or not private.shop_guest_import_has_keys(v_old_journal, array[
      'cycle_id','started_at_utc','ended_at_utc','wallet_credit','wallet_credit_at_utc'
    ])
    or not private.shop_guest_import_has_keys(v_current_journal, array[
      'cycle_id','started_at_utc','ended_at_utc','wallet_credit','wallet_credit_at_utc'
    ])
    or not private.shop_guest_import_string_valid(v_old_journal->'cycle_id', 1, 80)
    or not private.shop_guest_import_string_valid(v_current_journal->'cycle_id', 1, 80)
    or v_old_journal->'cycle_id' is distinct from to_jsonb(v_proof->>'previous_cycle_id')
    or v_current_journal->'cycle_id' is distinct from to_jsonb(v_current_cycle_id)
    or not private.shop_guest_import_timestamp_valid(v_old_journal->'started_at_utc')
    or not private.shop_guest_import_timestamp_valid(v_old_journal->'ended_at_utc')
    or not private.shop_guest_import_timestamp_valid(v_current_journal->'started_at_utc')
    or not private.shop_guest_import_timestamp_valid(v_current_journal->'ended_at_utc', true)
    or (v_old_journal->>'started_at_utc')::timestamptz is distinct from v_old_start
    or (v_old_journal->>'ended_at_utc')::timestamptz is distinct from v_reset_at
    or (v_current_journal->>'started_at_utc')::timestamptz is distinct from v_reset_at
    or v_current_journal->'ended_at_utc' is distinct from 'null'::jsonb
    or v_current_journal->'wallet_credit' is distinct from 'null'::jsonb
    or v_current_journal->'wallet_credit_at_utc' is distinct from 'null'::jsonb
  then
    return null;
  end if;

  select value into v_claim
  from jsonb_array_elements(p_data->'unverified_planet_wallet_claims') c(value);
  if not private.shop_guest_import_has_keys(v_claim, array[
      'previous_cycle_id','claimed_amount','created_at_utc'
    ])
    or not private.shop_guest_import_has_keys(v_proof->'raw_wallet_claim', array[
      'previous_cycle_id','claimed_amount','created_at_utc'
    ])
    or not private.shop_guest_import_string_valid(v_claim->'previous_cycle_id', 1, 80)
    or not private.shop_guest_import_string_valid(v_proof#>'{raw_wallet_claim,previous_cycle_id}', 1, 80)
    or v_claim->'previous_cycle_id' is distinct from to_jsonb(v_proof->>'previous_cycle_id')
    or v_proof#>'{raw_wallet_claim,previous_cycle_id}' is distinct from to_jsonb(v_proof->>'previous_cycle_id')
    or not private.shop_guest_import_uint_valid(v_claim->'claimed_amount', v_server_max)
    or not private.shop_guest_import_uint_valid(v_proof#>'{raw_wallet_claim,claimed_amount}', v_server_max)
    or not private.shop_guest_import_timestamp_valid(v_claim->'created_at_utc')
    or not private.shop_guest_import_timestamp_valid(v_proof#>'{raw_wallet_claim,created_at_utc}')
    or (v_claim->>'created_at_utc')::timestamptz is distinct from v_reset_at
    or (v_proof#>>'{raw_wallet_claim,created_at_utc}')::timestamptz is distinct from v_reset_at
    or v_claim->'claimed_amount' is distinct from v_proof#>'{raw_wallet_claim,claimed_amount}'
    or v_claim->'claimed_amount' is distinct from v_old_journal->'wallet_credit'
    or (v_old_journal->>'wallet_credit_at_utc')::timestamptz is distinct from v_reset_at
  then
    return null;
  end if;

  select value into v_settlement
  from jsonb_array_elements(p_data->'cycle_settlements') s(value);
  if not private.shop_guest_import_has_keys(v_settlement, array[
      'cycle_id','amount','settled_at_utc'
    ])
    or not private.shop_guest_import_string_valid(v_settlement->'cycle_id', 1, 80)
    or v_settlement->'cycle_id' is distinct from to_jsonb(v_proof->>'previous_cycle_id')
    or not private.shop_guest_import_uint_valid(v_settlement->'amount', v_server_max)
    or not private.shop_guest_import_timestamp_valid(v_settlement->'settled_at_utc')
    or (v_settlement->>'settled_at_utc')::timestamptz is distinct from v_reset_at
    or (v_settlement->>'settled_at_utc')::timestamptz > v_server_time
  then
    return null;
  end if;

  v_raw_tokens := 0;
  v_weighted_tokens := 0;
  for v_contribution in
    select value from jsonb_array_elements(p_usage->'effect_contributions') c(value)
    where c.value->>'cycle_id' = v_proof->>'previous_cycle_id'
  loop
    if not private.shop_guest_import_uint_valid(v_contribution->'tokens', v_server_max)
      or not private.shop_guest_import_uint_valid(v_contribution->'wallet_bps', 3000)
    then
      return null;
    end if;
    v_tokens := (v_contribution->>'tokens')::numeric;
    v_wallet_bps := (v_contribution->>'wallet_bps')::numeric;
    v_raw_tokens := v_raw_tokens + v_tokens;
    v_weighted_tokens := v_weighted_tokens + v_tokens * v_wallet_bps;
    if v_raw_tokens > v_server_max or v_weighted_tokens > v_server_max * 3000 then
      return null;
    end if;
  end loop;
  v_raw_claim_amount := (v_claim->>'claimed_amount')::numeric;
  v_bonus := floor(v_weighted_tokens / 10000);
  v_settlement_amount := (v_settlement->>'amount')::numeric;
  v_declared_bonus := (v_old_cycle->>'settled_bonus_tokens')::numeric;
  v_total := v_raw_tokens + v_bonus;
  if v_raw_tokens is distinct from v_raw_claim_amount
    or v_bonus is distinct from v_settlement_amount
    or v_bonus is distinct from v_declared_bonus
    or v_bonus is distinct from (v_proof->>'settled_bonus_tokens')::numeric
    or v_total > v_server_max
  then
    return null;
  end if;

  select value into v_final_interval
  from jsonb_array_elements(p_effect_timeline->'effect_history') h(value)
  where h.value->>'cycle_id' = v_proof->>'previous_cycle_id'
  order by (h.value->>'revision')::numeric desc
  limit 1;
  if v_final_interval is null
    or (v_final_interval->>'ended_at_utc')::timestamptz is distinct from v_reset_at
    or not private.shop_guest_import_uint_valid(v_final_interval->'revision', v_server_max)
    or (v_final_interval->>'revision')::numeric
      is distinct from (v_proof->>'final_effect_revision')::numeric
    or v_final_interval->'effects' is distinct from v_proof->'final_effects'
    or v_final_interval->'active_instance_ids' is distinct from v_proof->'final_active_instance_ids'
  then
    return null;
  end if;

  v_delay_seconds := greatest(
    64800::bigint,
    floor(86400::numeric * (10000 - (v_final_interval#>>'{effects,reset_cooldown_bps}')::numeric) / 10000)::bigint
  );
  v_expected_available := v_reset_at + v_delay_seconds * interval '1 second';
  if v_reset_available is distinct from v_expected_available
    or (v_old_cycle->>'ended_at_utc')::timestamptz > v_server_time
    or (v_claim->>'created_at_utc')::timestamptz > v_server_time
  then
    return null;
  end if;

  v_reset_chain := jsonb_build_array(jsonb_build_object(
    'request_id', v_proof->>'request_id',
    'previous_cycle_id', v_proof->>'previous_cycle_id',
    'new_cycle_id', v_proof->>'new_cycle_id',
    'reset_at_utc', pg_catalog.to_char(v_reset_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
    'reset_available_at_utc', pg_catalog.to_char(v_expected_available at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
    'raw_tokens', v_raw_tokens::bigint,
    'bonus_tokens', v_bonus::bigint,
    'final_effect_revision', (v_final_interval->>'revision')::bigint,
    'final_effects', v_final_interval->'effects',
    'final_active_instance_ids', v_final_interval->'active_instance_ids'
  ));
  v_cycle_credits := jsonb_build_array(jsonb_build_object(
    'cycle_id', v_proof->>'previous_cycle_id',
    'raw_tokens', v_raw_tokens::bigint,
    'bonus_tokens', v_bonus::bigint,
    'total_tokens', v_total::bigint
  ));

  return jsonb_build_object(
    'validation_scope', 'reset_settlement_consistency',
    'reset_chain', v_reset_chain,
    'cycle_token_credits', v_cycle_credits
  );
exception when others then
  return null;
end;
$$;

create or replace function private.shop_guest_import_cycle_token_normalize(
  p_data jsonb,
  p_usage jsonb,
  p_effect_timeline jsonb,
  p_resets jsonb
)
returns jsonb
language plpgsql
immutable
set search_path = ''
as $$
declare
  v_reset_count integer;
  v_credit_count integer;
  v_reward_count integer;
  v_wallet_count integer;
  v_reset jsonb;
  v_credit jsonb;
  v_reward jsonb;
  v_wallet jsonb;
  v_previous_cycle_id text;
  v_trigger_key text;
  v_reset_at timestamptz;
  v_reward_at timestamptz;
  v_wallet_at timestamptz;
  v_raw_tokens numeric;
  v_bonus_tokens numeric;
  v_total_tokens numeric;
  v_max bigint := 9223372036854775807;
  v_mirrors jsonb := '[]'::jsonb;
begin
  if jsonb_typeof(p_data) is distinct from 'object'
    or jsonb_typeof(p_usage) is distinct from 'object'
    or p_usage->>'validation_scope' is distinct from 'usage_effect_activity_consistency'
    or jsonb_typeof(p_effect_timeline) is distinct from 'object'
    or p_usage->'effect_cycle_bounds' is distinct from p_effect_timeline->'effect_cycle_bounds'
    or p_usage->'effect_history' is distinct from p_effect_timeline->'effect_history'
    or jsonb_typeof(p_resets) is distinct from 'object'
    or not private.shop_guest_import_has_keys(
      p_resets, array['validation_scope','reset_chain','cycle_token_credits']
    )
    or p_resets->>'validation_scope' is distinct from 'reset_settlement_consistency'
    or jsonb_typeof(p_resets->'reset_chain') is distinct from 'array'
    or jsonb_typeof(p_resets->'cycle_token_credits') is distinct from 'array'
    or jsonb_typeof(p_data->'game_rewards') is distinct from 'array'
    or jsonb_typeof(p_data->'wallet_credits') is distinct from 'array'
    or jsonb_typeof(p_data->'era_progress') is distinct from 'array'
    or jsonb_array_length(p_data->'era_progress') <> 0
  then
    return null;
  end if;

  v_reset_count := jsonb_array_length(p_resets->'reset_chain');
  v_credit_count := jsonb_array_length(p_resets->'cycle_token_credits');
  v_reward_count := jsonb_array_length(p_data->'game_rewards');
  v_wallet_count := jsonb_array_length(p_data->'wallet_credits');

  if v_reset_count > 1 or v_credit_count <> v_reset_count then
    return null;
  end if;
  if v_reset_count = 0 then
    if v_reward_count <> 0 or v_wallet_count <> 0 then
      return null;
    end if;
    return jsonb_build_object(
      'validation_scope', 'reward_wallet_mirror_core',
      'cycle_token_mirrors', v_mirrors
    );
  end if;

  v_reset := p_resets#>'{reset_chain,0}';
  v_credit := p_resets#>'{cycle_token_credits,0}';
  if not private.shop_guest_import_has_keys(v_reset, array[
      'request_id','previous_cycle_id','new_cycle_id','reset_at_utc',
      'reset_available_at_utc','raw_tokens','bonus_tokens','final_effect_revision',
      'final_effects','final_active_instance_ids'
    ])
    or not private.shop_guest_import_string_valid(v_reset->'request_id', 1, 200)
    or not private.shop_guest_import_string_valid(v_reset->'previous_cycle_id', 1, 80)
    or not private.shop_guest_import_string_valid(v_reset->'new_cycle_id', 1, 80)
    or v_reset->'previous_cycle_id' is not distinct from v_reset->'new_cycle_id'
    or not private.shop_guest_import_timestamp_valid(v_reset->'reset_at_utc')
    or not private.shop_guest_import_timestamp_valid(v_reset->'reset_available_at_utc')
    or not private.shop_guest_import_uint_valid(v_reset->'raw_tokens', v_max::numeric)
    or not private.shop_guest_import_uint_valid(v_reset->'bonus_tokens', v_max::numeric)
    or not private.shop_guest_import_uint_valid(v_reset->'final_effect_revision', v_max::numeric)
    or jsonb_typeof(v_reset->'final_effects') is distinct from 'object'
    or jsonb_typeof(v_reset->'final_active_instance_ids') is distinct from 'array'
    or not private.shop_guest_import_has_keys(v_credit, array[
      'cycle_id','raw_tokens','bonus_tokens','total_tokens'
    ])
    or not private.shop_guest_import_string_valid(v_credit->'cycle_id', 1, 80)
    or v_credit->'cycle_id' is distinct from v_reset->'previous_cycle_id'
    or not private.shop_guest_import_uint_valid(v_credit->'raw_tokens', v_max::numeric)
    or not private.shop_guest_import_uint_valid(v_credit->'bonus_tokens', v_max::numeric)
    or not private.shop_guest_import_uint_valid(v_credit->'total_tokens', v_max::numeric)
    or v_credit->'raw_tokens' is distinct from v_reset->'raw_tokens'
    or v_credit->'bonus_tokens' is distinct from v_reset->'bonus_tokens'
  then
    return null;
  end if;

  v_previous_cycle_id := v_reset->>'previous_cycle_id';
  v_reset_at := (v_reset->>'reset_at_utc')::timestamptz;
  v_raw_tokens := (v_credit->>'raw_tokens')::numeric;
  v_bonus_tokens := (v_credit->>'bonus_tokens')::numeric;
  v_total_tokens := v_raw_tokens + v_bonus_tokens;
  if v_total_tokens > v_max::numeric
    or v_credit->'total_tokens' is distinct from to_jsonb(v_total_tokens)
  then
    return null;
  end if;

  -- Current capture semantics omit both rows when the reset bonus is zero.
  -- A zero-amount cycle-token reward row is held until that shape is proven.
  if v_bonus_tokens = 0 then
    if v_reward_count <> 0 or v_wallet_count <> 0 then
      return null;
    end if;
    return jsonb_build_object(
      'validation_scope', 'reward_wallet_mirror_core',
      'cycle_token_mirrors', v_mirrors
    );
  end if;

  if v_reward_count <> 1 or v_wallet_count <> 1 then
    return null;
  end if;
  v_reward := p_data#>'{game_rewards,0}';
  v_wallet := p_data#>'{wallet_credits,0}';
  if not private.shop_guest_import_has_keys(v_reward, array[
      'reward_id','trigger_key','kind','cycle_id','amount','effects','awarded_at_utc'
    ])
    or not private.shop_guest_import_has_keys(v_wallet, array[
      'credit_id','trigger_key','cycle_id','amount','created_at_utc'
    ])
    or not private.shop_guest_import_string_valid(v_reward->'reward_id', 36, 36)
    or not coalesce(v_reward->>'reward_id' ~ '^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$', false)
    or not private.shop_guest_import_string_valid(v_wallet->'credit_id', 36, 36)
    or not coalesce(v_wallet->>'credit_id' ~ '^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$', false)
    or not private.shop_guest_import_string_valid(v_reward->'trigger_key', 1, 160)
    or not private.shop_guest_import_string_valid(v_wallet->'trigger_key', 1, 160)
    or not private.shop_guest_import_string_valid(v_reward->'kind', 1, 32)
    or v_reward->'kind' is distinct from '"cycle_token"'::jsonb
    or not private.shop_guest_import_string_valid(v_reward->'cycle_id', 1, 80)
    or not private.shop_guest_import_string_valid(v_wallet->'cycle_id', 1, 80)
    or v_reward->'cycle_id' is distinct from to_jsonb(v_previous_cycle_id)
    or v_wallet->'cycle_id' is distinct from to_jsonb(v_previous_cycle_id)
    or not private.shop_guest_import_uint_valid(v_reward->'amount', v_max::numeric)
    or not private.shop_guest_import_uint_valid(v_wallet->'amount', v_max::numeric)
    or v_reward->'amount' is distinct from v_wallet->'amount'
    or (v_reward->>'amount')::numeric is distinct from v_bonus_tokens
    or not private.shop_guest_import_timestamp_valid(v_reward->'awarded_at_utc')
    or not private.shop_guest_import_timestamp_valid(v_wallet->'created_at_utc')
    or jsonb_typeof(v_reward->'effects') is distinct from 'object'
    or v_reward->'effects' is distinct from jsonb_build_object(
      'token_earning_bps', 0,
      'civilization_growth_bps', 0,
      'shop_discount_bps', 0,
      'reset_cooldown_bps', 0,
      'natural_removal_discount_bps', 0,
      'era_reward_tokens', 0,
      'streak_reward_tokens', 0
    )
  then
    return null;
  end if;

  v_trigger_key := 'cycle-token:' || v_previous_cycle_id;
  v_reward_at := (v_reward->>'awarded_at_utc')::timestamptz;
  v_wallet_at := (v_wallet->>'created_at_utc')::timestamptz;
  if v_reward->'trigger_key' is distinct from to_jsonb(v_trigger_key)
    or v_wallet->'trigger_key' is distinct from to_jsonb(v_trigger_key)
    or v_reward_at is distinct from v_reset_at
    or v_wallet_at is distinct from v_reset_at
    or v_reward_at is distinct from v_wallet_at
  then
    return null;
  end if;

  v_mirrors := jsonb_build_array(jsonb_build_object(
    'cycle_id', v_previous_cycle_id,
    'trigger_key', v_trigger_key,
    'amount', v_bonus_tokens::bigint,
    'reward_id', v_reward->>'reward_id',
    'credit_id', v_wallet->>'credit_id',
    'awarded_at_utc', v_reward->>'awarded_at_utc',
    'created_at_utc', v_wallet->>'created_at_utc'
  ));
  return jsonb_build_object(
    'validation_scope', 'reward_wallet_mirror_core',
    'cycle_token_mirrors', v_mirrors
  );
exception when others then
  return null;
end;
$$;


create or replace function private.shop_guest_import_streak_mirrors_normalize(
  p_data jsonb,
  p_usage jsonb,
  p_effect_timeline jsonb
)
returns jsonb
language plpgsql
stable
set search_path = ''
as $$
declare
  v_reward jsonb;
  v_wallet jsonb;
  v_day jsonb;
  v_previous_day jsonb;
  v_bound jsonb;
  v_interval jsonb;
  v_effects jsonb;
  v_streak_rewards jsonb[] := array[]::jsonb[];
  v_streak_mirrors jsonb := '[]'::jsonb;
  v_seen_reward_ids jsonb := '{}'::jsonb;
  v_seen_reward_triggers jsonb := '{}'::jsonb;
  v_seen_wallet_ids jsonb := '{}'::jsonb;
  v_seen_wallet_triggers jsonb := '{}'::jsonb;
  v_reward_id text;
  v_credit_id text;
  v_trigger_key text;
  v_kind text;
  v_reward_date_text text;
  v_previous_date_text text;
  v_cycle_id text;
  v_timezone text;
  v_first timestamptz;
  v_awarded timestamptz;
  v_server_time timestamptz;
  v_bound_start timestamptz;
  v_bound_end timestamptz;
  v_interval_start timestamptz;
  v_interval_end timestamptz;
  v_amount numeric;
  v_max bigint := 9223372036854775807;
  v_reward_count integer := 0;
  v_total_reward_count integer := 0;
  v_wallet_count integer := 0;
  v_match_count integer;
  v_date date;
  v_previous_date date;
begin
  if jsonb_typeof(p_data) is distinct from 'object'
    or jsonb_typeof(p_usage) is distinct from 'object'
    or p_usage->>'validation_scope' is distinct from 'usage_effect_activity_consistency'
    or jsonb_typeof(p_effect_timeline) is distinct from 'object'
    or p_usage->'effect_cycle_bounds' is distinct from p_effect_timeline->'effect_cycle_bounds'
    or p_usage->'effect_history' is distinct from p_effect_timeline->'effect_history'
    or jsonb_typeof(p_data->'game_rewards') is distinct from 'array'
    or jsonb_typeof(p_data->'wallet_credits') is distinct from 'array'
    or jsonb_typeof(p_usage->'activity_days') is distinct from 'array'
    or p_usage->'activity_days' is distinct from p_data->'activity_days'
  then
    return null;
  end if;

  for v_reward in select value from jsonb_array_elements(p_data->'game_rewards')
  loop
    if not private.shop_guest_import_has_keys(v_reward, array[
        'reward_id','trigger_key','kind','cycle_id','amount','effects','awarded_at_utc'
      ])
      or not private.shop_guest_import_string_valid(v_reward->'reward_id', 36, 36)
      or not coalesce(v_reward->>'reward_id' ~ '^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$', false)
      or not private.shop_guest_import_string_valid(v_reward->'trigger_key', 1, 160)
      or not private.shop_guest_import_string_valid(v_reward->'kind', 1, 32)
      or v_reward->>'kind' not in ('cycle_token','streak')
      or not private.shop_guest_import_string_valid(v_reward->'cycle_id', 1, 80)
      or not private.shop_guest_import_uint_valid(v_reward->'amount', v_max::numeric)
      or (v_reward->>'amount')::numeric < 1
      or jsonb_typeof(v_reward->'effects') is distinct from 'object'
      or not private.shop_guest_import_timestamp_valid(v_reward->'awarded_at_utc')
    then
      return null;
    end if;
    v_reward_id := v_reward->>'reward_id';
    v_trigger_key := v_reward->>'trigger_key';
    v_total_reward_count := v_total_reward_count + 1;
    v_kind := v_reward->>'kind';
    if v_seen_reward_ids ? v_reward_id or v_seen_reward_triggers ? v_trigger_key then
      return null;
    end if;
    v_seen_reward_ids := v_seen_reward_ids || jsonb_build_object(v_reward_id, true);
    v_seen_reward_triggers := v_seen_reward_triggers || jsonb_build_object(v_trigger_key, true);
    if v_kind = 'cycle_token'
      and v_trigger_key is distinct from ('cycle-token:' || (v_reward->>'cycle_id'))
    then
      return null;
    end if;
    if v_kind = 'streak' then
      v_reward_count := v_reward_count + 1;
      v_streak_rewards := array_append(v_streak_rewards, v_reward);
    end if;
  end loop;

  for v_wallet in select value from jsonb_array_elements(p_data->'wallet_credits')
  loop
    if not private.shop_guest_import_has_keys(v_wallet, array[
        'credit_id','trigger_key','cycle_id','amount','created_at_utc'
      ])
      or not private.shop_guest_import_string_valid(v_wallet->'credit_id', 36, 36)
      or not coalesce(v_wallet->>'credit_id' ~ '^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$', false)
      or not private.shop_guest_import_string_valid(v_wallet->'trigger_key', 1, 160)
      or not private.shop_guest_import_string_valid(v_wallet->'cycle_id', 1, 80)
      or not private.shop_guest_import_uint_valid(v_wallet->'amount', v_max::numeric)
      or (v_wallet->>'amount')::numeric < 1
      or not private.shop_guest_import_timestamp_valid(v_wallet->'created_at_utc')
    then
      return null;
    end if;
    v_credit_id := v_wallet->>'credit_id';
    v_trigger_key := v_wallet->>'trigger_key';
    if v_seen_wallet_ids ? v_credit_id or v_seen_wallet_triggers ? v_trigger_key then
      return null;
    end if;
    v_seen_wallet_ids := v_seen_wallet_ids || jsonb_build_object(v_credit_id, true);
    v_seen_wallet_triggers := v_seen_wallet_triggers || jsonb_build_object(v_trigger_key, true);
    if left(v_trigger_key, 7) <> 'streak:'
      and left(v_trigger_key, 12) <> 'cycle-token:' then
      return null;
    end if;
    v_wallet_count := v_wallet_count + 1;
  end loop;

  if v_total_reward_count <> v_wallet_count then
    return null;
  end if;

  for v_reward in select value from jsonb_array_elements(p_data->'game_rewards')
  loop
    select count(*) into v_match_count
    from jsonb_array_elements(p_data->'wallet_credits') w(value)
    where w.value->'trigger_key' = v_reward->'trigger_key';
    if v_match_count <> 1 then
      return null;
    end if;
    select w.value into v_wallet
    from jsonb_array_elements(p_data->'wallet_credits') w(value)
    where w.value->'trigger_key' = v_reward->'trigger_key';
    if v_wallet->'cycle_id' is distinct from v_reward->'cycle_id'
      or v_wallet->'amount' is distinct from v_reward->'amount'
      or (v_wallet->>'created_at_utc')::timestamptz
        is distinct from (v_reward->>'awarded_at_utc')::timestamptz
    then
      return null;
    end if;
  end loop;

  for v_wallet in select value from jsonb_array_elements(p_data->'wallet_credits')
  loop
    select count(*) into v_match_count
    from jsonb_array_elements(p_data->'game_rewards') r(value)
    where r.value->'trigger_key' = v_wallet->'trigger_key';
    if v_match_count <> 1 then
      return null;
    end if;
  end loop;

  if cardinality(v_streak_rewards) = 0 then
    return jsonb_build_object('streak_mirrors', v_streak_mirrors);
  end if;

  if jsonb_typeof(p_data->'effect_timeline_state') is distinct from 'object'
    or not private.shop_guest_import_timestamp_valid(
      p_data#>'{effect_timeline_state,server_time_utc}'
    )
    or not private.shop_guest_import_timezone_valid(p_data->'reward_timezone')
  then
    return null;
  end if;
  v_server_time := (p_data#>>'{effect_timeline_state,server_time_utc}')::timestamptz;
  v_timezone := p_data->>'reward_timezone';

  foreach v_reward in array v_streak_rewards
  loop
    v_trigger_key := v_reward->>'trigger_key';
    if v_trigger_key !~ '^streak:[0-9]{4}-[0-9]{2}-[0-9]{2}$' then
      return null;
    end if;
    v_reward_date_text := substring(v_trigger_key from 8);
    v_date := v_reward_date_text::date;
    if v_date::text is distinct from v_reward_date_text then
      return null;
    end if;
    v_previous_date := v_date - 1;
    v_previous_date_text := v_previous_date::text;
    v_cycle_id := v_reward->>'cycle_id';
    v_day := null;
    v_previous_day := null;
    v_match_count := 0;

    for v_interval in select value from jsonb_array_elements(p_usage->'activity_days')
    loop
      if v_interval->>'reward_date' = v_reward_date_text then
        v_day := v_interval;
        v_match_count := v_match_count + 1;
      elsif v_interval->>'reward_date' = v_previous_date_text then
        v_previous_day := v_interval;
      end if;
    end loop;
    if v_match_count <> 1
      or v_day->>'cycle_id' is distinct from v_cycle_id
      or v_previous_day is null
      or (v_day->>'tokens')::numeric <= 0
      or (v_previous_day->>'tokens')::numeric <= 0
    then
      return null;
    end if;

    v_first := (v_day->>'first_occurred_at_utc')::timestamptz;
    v_awarded := (v_reward->>'awarded_at_utc')::timestamptz;
    if v_first > v_awarded or v_awarded > v_server_time
      or (v_first at time zone v_timezone)::date is distinct from v_date
    then
      return null;
    end if;

    v_bound := null;
    v_match_count := 0;
    for v_interval in select value from jsonb_array_elements(p_usage->'effect_cycle_bounds')
    loop
      if v_interval->>'cycle_id' = v_cycle_id then
        v_bound := v_interval;
        v_match_count := v_match_count + 1;
      end if;
    end loop;
    if v_match_count <> 1 then
      return null;
    end if;
    v_bound_start := (v_bound->>'started_at_utc')::timestamptz;
    v_bound_end := case when v_bound->'ended_at_utc' = 'null'::jsonb then null
      else (v_bound->>'ended_at_utc')::timestamptz end;
    if v_first < v_bound_start
      or (v_bound_end is not null and (v_first >= v_bound_end or v_awarded > v_bound_end))
    then
      return null;
    end if;

    v_effects := null;
    v_match_count := 0;
    for v_interval in select value from jsonb_array_elements(p_usage->'effect_history')
    loop
      if v_interval->>'cycle_id' = v_cycle_id
        and (v_interval->>'started_at_utc')::timestamptz <= v_first
        and (v_interval->'ended_at_utc' = 'null'::jsonb
          or v_first < (v_interval->>'ended_at_utc')::timestamptz)
      then
        v_effects := v_interval->'effects';
        v_match_count := v_match_count + 1;
      end if;
    end loop;
    if v_match_count > 1 then
      return null;
    elsif v_match_count = 0 then
      v_effects := jsonb_build_object(
        'token_earning_bps', 0,
        'civilization_growth_bps', 0,
        'shop_discount_bps', 0,
        'reset_cooldown_bps', 0,
        'natural_removal_discount_bps', 0,
        'era_reward_tokens', 0,
        'streak_reward_tokens', 0
      );
    end if;

    v_amount := least((v_effects->>'streak_reward_tokens')::numeric, 500000::numeric);
    if v_amount < 1
      or v_reward->'effects' is distinct from v_effects
      or (v_reward->>'amount')::numeric is distinct from v_amount
    then
      return null;
    end if;

    v_streak_mirrors := v_streak_mirrors || jsonb_build_array(jsonb_build_object(
      'reward_date', v_reward_date_text,
      'cycle_id', v_cycle_id,
      'trigger_key', v_trigger_key,
      'amount', v_amount::bigint,
      'effects', v_effects,
      'reward_id', v_reward->>'reward_id',
      'credit_id', (
        select w.value->>'credit_id'
        from jsonb_array_elements(p_data->'wallet_credits') w(value)
        where w.value->'trigger_key' = v_reward->'trigger_key'
      ),
      'awarded_at_utc', v_reward->>'awarded_at_utc',
      'created_at_utc', (
        select w.value->>'created_at_utc'
        from jsonb_array_elements(p_data->'wallet_credits') w(value)
        where w.value->'trigger_key' = v_reward->'trigger_key'
      )
    ));
  end loop;

  return jsonb_build_object('streak_mirrors', v_streak_mirrors);
exception when others then
  return null;
end;
$$;

create or replace function private.shop_guest_import_rewards_normalize(
  p_data jsonb,
  p_usage jsonb,
  p_effect_timeline jsonb,
  p_resets jsonb
)
returns jsonb
language plpgsql
stable
set search_path = ''
as $$
declare
  v_owned jsonb;
  v_expected_timeline jsonb;
  v_verified_usage jsonb;
  v_verified_resets jsonb;
  v_streak_result jsonb;
  v_cycle_data jsonb;
  v_cycle_reward_result jsonb;
  v_cycle_rewards jsonb;
  v_cycle_wallets jsonb;
begin
  if jsonb_typeof(p_data) is distinct from 'object'
    or jsonb_typeof(p_usage) is distinct from 'object'
    or p_usage->>'validation_scope' is distinct from 'usage_effect_activity_consistency'
    or jsonb_typeof(p_effect_timeline) is distinct from 'object'
    or jsonb_typeof(p_resets) is distinct from 'object'
    or jsonb_typeof(p_data->'game_rewards') is distinct from 'array'
    or jsonb_typeof(p_data->'wallet_credits') is distinct from 'array'
    or jsonb_typeof(p_data->'era_progress') is distinct from 'array'
    or jsonb_array_length(p_data->'era_progress') <> 0
  then
    return null;
  end if;

  v_owned := private.shop_guest_import_ownership_normalize(p_data);
  if v_owned is null then
    return null;
  end if;
  v_expected_timeline := jsonb_build_object(
    'effect_cycle_bounds', v_owned->'effect_cycle_bounds',
    'effect_history', v_owned->'effect_history'
  );
  if p_effect_timeline is distinct from v_expected_timeline then
    return null;
  end if;
  v_verified_usage := private.shop_guest_import_usage_normalize(p_data, v_expected_timeline);
  if v_verified_usage is null or p_usage is distinct from v_verified_usage then
    return null;
  end if;
  v_verified_resets := private.shop_guest_import_reset_normalize(
    p_data, v_verified_usage, v_expected_timeline
  );
  if v_verified_resets is null or p_resets is distinct from v_verified_resets then
    return null;
  end if;

  v_streak_result := private.shop_guest_import_streak_mirrors_normalize(
    p_data, v_verified_usage, v_expected_timeline
  );
  if v_streak_result is null then
    return null;
  end if;

  select coalesce(jsonb_agg(r.value order by r.ordinality), '[]'::jsonb)
  into v_cycle_rewards
  from jsonb_array_elements(p_data->'game_rewards') with ordinality r(value, ordinality)
  where r.value->>'kind' = 'cycle_token';
  select coalesce(jsonb_agg(w.value order by w.ordinality), '[]'::jsonb)
  into v_cycle_wallets
  from jsonb_array_elements(p_data->'wallet_credits') with ordinality w(value, ordinality)
  where left(w.value->>'trigger_key', 12) = 'cycle-token:';

  v_cycle_data := jsonb_set(p_data, '{game_rewards}', v_cycle_rewards, true);
  v_cycle_data := jsonb_set(v_cycle_data, '{wallet_credits}', v_cycle_wallets, true);
  v_cycle_reward_result := private.shop_guest_import_cycle_token_normalize(
    v_cycle_data, v_verified_usage, v_expected_timeline, v_verified_resets
  );
  if v_cycle_reward_result is null then
    return null;
  end if;

  return jsonb_build_object(
    'validation_scope', 'reward_wallet_streak_consistency',
    'cycle_token_mirrors', v_cycle_reward_result->'cycle_token_mirrors',
    'streak_mirrors', v_streak_result->'streak_mirrors'
  );
exception when others then
  return null;
end;
$$;


create or replace function private.shop_guest_import_ledger_prefix_normalize(p_events jsonb)
returns jsonb
language plpgsql
stable
set search_path = ''
as $$
declare
  v_max constant numeric := 9223372036854775807::numeric;
  v_seen_ids jsonb := '{}'::jsonb;
  v_row jsonb;
  v_source_id text;
  v_prefix_valid boolean;
  v_groups jsonb;
  v_final_balance numeric;
  v_total_credits numeric;
  v_total_debits numeric;
begin
  if jsonb_typeof(p_events) is distinct from 'array' then
    return null;
  end if;

  for v_row in select value from jsonb_array_elements(p_events)
  loop
    if not private.shop_guest_import_has_keys(v_row, array[
        'side','kind','source_id','cycle_id','at_utc','amount',
        'trigger_key','sku','stage','ordinal'
      ], array['trigger_key','sku','stage','ordinal'])
      or jsonb_typeof(v_row->'side') is distinct from 'string'
      or v_row->>'side' not in ('credit','debit')
      or jsonb_typeof(v_row->'kind') is distinct from 'string'
      or v_row->>'kind' not in (
        'reset_credit','streak_credit','purchase_debit','natural_removal_debit'
      )
      or (v_row->>'side' = 'credit' and v_row->>'kind' not in ('reset_credit','streak_credit'))
      or (v_row->>'side' = 'debit' and v_row->>'kind' not in ('purchase_debit','natural_removal_debit'))
      or not private.shop_guest_import_string_valid(v_row->'source_id', 1, 160)
      or not private.shop_guest_import_string_valid(v_row->'cycle_id', 1, 80)
      or not private.shop_guest_import_timestamp_valid(v_row->'at_utc')
      or not private.shop_guest_import_uint_valid(v_row->'amount', v_max)
      or (v_row ? 'trigger_key' and not private.shop_guest_import_string_valid(v_row->'trigger_key', 1, 160))
      or (v_row ? 'sku' and not private.shop_guest_import_string_valid(v_row->'sku', 1, 80))
      or (v_row ? 'stage' and not private.shop_guest_import_uint_valid(v_row->'stage', 4))
      or (v_row ? 'ordinal' and not private.shop_guest_import_uint_valid(v_row->'ordinal', 2147483647))
      or (v_row->>'kind' = 'streak_credit' and not (v_row ? 'trigger_key'))
      or (v_row->>'kind' <> 'streak_credit' and v_row ? 'trigger_key')
      or (v_row->>'kind' = 'purchase_debit' and not (v_row ? 'sku'))
      or (v_row->>'kind' <> 'purchase_debit' and v_row ? 'sku')
      or (v_row->>'kind' = 'natural_removal_debit' and not (v_row ? 'stage' and v_row ? 'ordinal'))
      or (v_row->>'kind' <> 'natural_removal_debit' and (v_row ? 'stage' or v_row ? 'ordinal'))
    then
      return null;
    end if;
    v_source_id := v_row->>'source_id';
    if v_seen_ids ? v_source_id or (v_row->>'amount')::numeric <= 0 then
      return null;
    end if;
    v_seen_ids := v_seen_ids || jsonb_build_object(v_source_id, true);
  end loop;

  with event_rows as (
    select (e.value->>'at_utc')::timestamptz as occurred_at,
      e.value->>'side' as side,
      (e.value->>'amount')::numeric as amount
    from jsonb_array_elements(p_events) e(value)
  ), totals as (
    select coalesce(sum(amount) filter (where side = 'credit'), 0::numeric) as credits,
      coalesce(sum(amount) filter (where side = 'debit'), 0::numeric) as debits
    from event_rows
  ), grouped as (
    select occurred_at,
      coalesce(sum(amount) filter (where side = 'credit'), 0::numeric) as credits,
      coalesce(sum(amount) filter (where side = 'debit'), 0::numeric) as debits
    from event_rows
    group by occurred_at
  ), prefix as (
    select occurred_at, credits, debits,
      coalesce(sum(credits - debits) over (
        order by occurred_at rows between unbounded preceding and 1 preceding
      ), 0::numeric) as opening_balance,
      sum(credits - debits) over (
        order by occurred_at rows between unbounded preceding and current row
      ) as ending_balance
    from grouped
  )
  select coalesce(bool_and(
      opening_balance - debits >= 0
        and ending_balance >= 0 and ending_balance <= v_max
        and credits <= v_max and debits <= v_max
    ), true)
      and (select credits <= v_max and debits <= v_max from totals),
    coalesce(jsonb_agg(jsonb_build_object(
      'at_utc', pg_catalog.to_char(
        occurred_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'
      ),
      'opening_balance', opening_balance::bigint,
      'credits', credits::bigint,
      'debits', debits::bigint,
      'ending_balance', ending_balance::bigint
    ) order by occurred_at), '[]'::jsonb),
    coalesce((array_agg(ending_balance order by occurred_at desc))[1], 0::numeric),
    (select credits from totals),
    (select debits from totals)
  into v_prefix_valid, v_groups, v_final_balance, v_total_credits, v_total_debits
  from prefix;

  if not v_prefix_valid or v_total_credits > v_max or v_total_debits > v_max then
    return null;
  end if;
  return jsonb_build_object(
    'timestamp_groups', v_groups,
    'ending_balance', v_final_balance::bigint
  );
exception when others then
  return null;
end;
$$;

create or replace function private.shop_guest_import_ledger_normalize(p_data jsonb)
returns jsonb
language plpgsql
stable
set search_path = ''
as $$
declare
  v_max constant numeric := 9223372036854775807::numeric;
  v_owned jsonb;
  v_effect_timeline jsonb;
  v_usage jsonb;
  v_resets jsonb;
  v_rewards jsonb;
  v_events jsonb := '[]'::jsonb;
  v_prefixes jsonb;
  v_seen_ids jsonb := '{}'::jsonb;
  v_seen_triggers jsonb := '{}'::jsonb;
  v_reset_chain jsonb;
  v_cycle_credits jsonb;
  v_cycle_mirrors jsonb;
  v_streak_mirrors jsonb;
  v_purchase_rows jsonb;
  v_removal_rows jsonb;
  v_row jsonb;
  v_prefix_result jsonb;
  v_reset jsonb;
  v_credit jsonb;
  v_mirror jsonb;
  v_wallet jsonb;
  v_bounds_row jsonb;
  v_interval jsonb;
  v_effects jsonb;
  v_ownership jsonb;
  v_cycle_id text;
  v_request_id text;
  v_trigger_key text;
  v_sku text;
  v_bound_cycle_id text;
  v_effect_revision bigint;
  v_at timestamptz;
  v_server_time timestamptz;
  v_bound_count integer;
  v_effect_count integer;
  v_match_count integer;
  v_index integer;
  v_stage integer;
  v_base_price numeric;
  v_discount_bps numeric;
  v_expected_price numeric;
  v_price numeric;
  v_reset_raw_total numeric := 0;
  v_claim_total numeric := 0;
  v_wallet_total numeric := 0;
  v_cycle_mirror_total numeric := 0;
  v_streak_total numeric := 0;
  v_server_credit_total numeric := 0;
  v_expected_final_balance numeric := 0;
  v_purchase_total numeric := 0;
  v_removal_total numeric := 0;
  v_local_claim_balance numeric := 0;
  v_final_balance numeric := 0;
  v_prefix_valid boolean;
  v_acquired_at timestamptz;
  v_quote_effect_revision bigint;
begin
  if jsonb_typeof(p_data) is distinct from 'object'
    or p_data->'legacy_partial_import_pending' is distinct from 'false'::jsonb
    or p_data->'reset_receipts_unverifiable' is distinct from 'false'::jsonb
    or jsonb_typeof(p_data->'era_progress') is distinct from 'array'
    or jsonb_typeof(p_data->'purchases') is distinct from 'array'
    or ((p_data ? 'purchase_proofs')
      and jsonb_typeof(p_data->'purchase_proofs') is distinct from 'array')
    or jsonb_typeof(p_data->'natural_removals') is distinct from 'array'
    or jsonb_typeof(p_data->'removal_debits') is distinct from 'array'
    or ((p_data ? 'removal_proofs')
      and jsonb_typeof(p_data->'removal_proofs') is distinct from 'array')
    or jsonb_typeof(p_data->'pending_purchases') is distinct from 'array'
    or jsonb_typeof(p_data->'cosmetic_purchases') is distinct from 'array'
    or jsonb_typeof(p_data->'cosmetic_equipment') is distinct from 'array'
    or jsonb_typeof(p_data->'unverified_planet_wallet_claims') is distinct from 'array'
    or jsonb_typeof(p_data->'wallet_credits') is distinct from 'array'
    or jsonb_typeof(p_data->'game_rewards') is distinct from 'array'
  then
    return null;
  end if;
  if jsonb_array_length(p_data->'era_progress') <> 0
    or jsonb_array_length(p_data->'pending_purchases') <> 0
    or jsonb_array_length(p_data->'cosmetic_purchases') <> 0
    or jsonb_array_length(p_data->'cosmetic_equipment') <> 0
  then
    return null;
  end if;

  v_owned := private.shop_guest_import_ownership_normalize(p_data);
  if v_owned is null then
    return null;
  end if;
  v_effect_timeline := jsonb_build_object(
    'effect_cycle_bounds', v_owned->'effect_cycle_bounds',
    'effect_history', v_owned->'effect_history'
  );
  v_usage := private.shop_guest_import_usage_normalize(p_data, v_effect_timeline);
  if v_usage is null then
    return null;
  end if;
  v_resets := private.shop_guest_import_reset_normalize(p_data, v_usage, v_effect_timeline);
  if v_resets is null then
    return null;
  end if;
  v_rewards := private.shop_guest_import_rewards_normalize(
    p_data, v_usage, v_effect_timeline, v_resets
  );
  if v_rewards is null
    or v_rewards->>'validation_scope' is distinct from 'reward_wallet_streak_consistency'
    or jsonb_typeof(v_rewards->'cycle_token_mirrors') is distinct from 'array'
    or jsonb_typeof(v_rewards->'streak_mirrors') is distinct from 'array'
  then
    return null;
  end if;

  v_server_time := (p_data#>>'{effect_timeline_state,server_time_utc}')::timestamptz;
  v_reset_chain := v_resets->'reset_chain';
  v_cycle_credits := v_resets->'cycle_token_credits';
  v_cycle_mirrors := v_rewards->'cycle_token_mirrors';
  v_streak_mirrors := v_rewards->'streak_mirrors';
  v_purchase_rows := v_owned->'purchases';
  v_removal_rows := v_owned->'natural_removals';
  if jsonb_typeof(v_reset_chain) is distinct from 'array'
    or jsonb_typeof(v_cycle_credits) is distinct from 'array'
    or jsonb_typeof(v_purchase_rows) is distinct from 'array'
    or jsonb_typeof(v_removal_rows) is distinct from 'array'
    or jsonb_array_length(v_reset_chain) <> jsonb_array_length(v_cycle_credits)
  then
    return null;
  end if;

  -- This increment verifies credit-only source ledgers. Debit-bearing captures
  -- remain held until a native purchase/removal source is independently proven.
  if jsonb_array_length(v_purchase_rows) <> 0
    or jsonb_array_length(v_removal_rows) <> 0
  then
    return null;
  end if;

  -- Reconstruct reset principal and settlement once. The cycle-token mirror is
  -- a second record of the reset bonus, so it is never added as another credit.
  for v_index in 0..jsonb_array_length(v_reset_chain) - 1 loop
    v_reset := v_reset_chain->v_index;
    v_credit := v_cycle_credits->v_index;
    if not private.shop_guest_import_has_keys(v_reset, array[
        'request_id','previous_cycle_id','reset_at_utc','raw_tokens','bonus_tokens',
        'new_cycle_id','final_effects','final_effect_revision',
        'reset_available_at_utc','final_active_instance_ids'
      ])
      or not private.shop_guest_import_has_keys(v_credit, array[
        'cycle_id','raw_tokens','bonus_tokens','total_tokens'
      ])
      or v_credit->'cycle_id' is distinct from v_reset->'previous_cycle_id'
      or v_credit->'raw_tokens' is distinct from v_reset->'raw_tokens'
      or v_credit->'bonus_tokens' is distinct from v_reset->'bonus_tokens'
    then
      return null;
    end if;
    v_request_id := v_reset->>'request_id';
    if v_seen_ids ? v_request_id or v_seen_triggers ? v_request_id then
      return null;
    end if;
    v_seen_ids := v_seen_ids || jsonb_build_object(v_request_id, true);
    v_at := (v_reset->>'reset_at_utc')::timestamptz;
    v_price := (v_credit->>'total_tokens')::numeric;
    if v_at > v_server_time or v_price <> (v_credit->>'raw_tokens')::numeric
        + (v_credit->>'bonus_tokens')::numeric
      or v_price < 0 or v_price > v_max
    then
      return null;
    end if;
    v_reset_raw_total := v_reset_raw_total + (v_credit->>'raw_tokens')::numeric;
    v_server_credit_total := v_server_credit_total + v_price;
    if v_price > 0 then
      v_events := v_events || jsonb_build_array(jsonb_build_object(
        'side', 'credit', 'kind', 'reset_credit',
        'source_id', v_request_id,
        'cycle_id', v_reset->>'previous_cycle_id',
        'at_utc', v_reset->>'reset_at_utc',
        'amount', v_price::bigint
      ));
    end if;
  end loop;

  for v_row in select value from jsonb_array_elements(p_data->'unverified_planet_wallet_claims')
  loop
    v_claim_total := v_claim_total + (v_row->>'claimed_amount')::numeric;
  end loop;
  if v_claim_total <> coalesce((
      select sum((value->>'raw_tokens')::numeric)
      from jsonb_array_elements(v_cycle_credits)
    ), 0::numeric)
    or v_claim_total <> v_reset_raw_total
  then
    return null;
  end if;

  for v_row in select value from jsonb_array_elements(v_cycle_mirrors)
  loop
    v_cycle_mirror_total := v_cycle_mirror_total + (v_row->>'amount')::numeric;
  end loop;
  for v_row in select value from jsonb_array_elements(v_streak_mirrors)
  loop
    v_streak_total := v_streak_total + (v_row->>'amount')::numeric;
    v_trigger_key := v_row->>'trigger_key';
    v_request_id := v_row->>'reward_id';
    if v_seen_ids ? v_request_id or v_seen_triggers ? v_request_id
      or v_seen_ids ? v_trigger_key or v_seen_triggers ? v_trigger_key
    then
      return null;
    end if;
    v_seen_ids := v_seen_ids || jsonb_build_object(v_request_id, true);
    v_seen_triggers := v_seen_triggers || jsonb_build_object(v_trigger_key, true);
    v_request_id := v_row->>'credit_id';
    if v_seen_ids ? v_request_id or v_seen_triggers ? v_request_id then
      return null;
    end if;
    v_seen_ids := v_seen_ids || jsonb_build_object(v_request_id, true);
    v_at := (v_row->>'awarded_at_utc')::timestamptz;
    if v_at > v_server_time then
      return null;
    end if;
    v_events := v_events || jsonb_build_array(jsonb_build_object(
      'side', 'credit', 'kind', 'streak_credit',
      'source_id', v_row->>'reward_id',
      'trigger_key', v_trigger_key,
      'cycle_id', v_row->>'cycle_id',
      'at_utc', v_row->>'awarded_at_utc',
      'amount', (v_row->>'amount')::bigint
    ));
  end loop;

  for v_row in select value from jsonb_array_elements(p_data->'game_rewards')
  loop
    v_request_id := v_row->>'reward_id';
    v_trigger_key := v_row->>'trigger_key';
    if v_seen_ids ? v_request_id or v_seen_triggers ? v_request_id
      or v_seen_ids ? v_trigger_key or v_seen_triggers ? v_trigger_key
    then
      return null;
    end if;
    v_seen_ids := v_seen_ids || jsonb_build_object(v_request_id, true);
    v_seen_triggers := v_seen_triggers || jsonb_build_object(v_trigger_key, true);
  end loop;
  for v_wallet in select value from jsonb_array_elements(p_data->'wallet_credits')
  loop
    v_request_id := v_wallet->>'credit_id';
    if v_seen_ids ? v_request_id or v_seen_triggers ? v_request_id then
      return null;
    end if;
    v_seen_ids := v_seen_ids || jsonb_build_object(v_request_id, true);
    v_wallet_total := v_wallet_total + (v_wallet->>'amount')::numeric;
  end loop;
  if v_wallet_total <> v_cycle_mirror_total + v_streak_total then
    return null;
  end if;

  -- This path is unreachable while debit-bearing sources are explicitly held
  -- above. Keep the checks as a documented future candidate, not acceptance.
  for v_row in select value from jsonb_array_elements(v_purchase_rows)
  loop
    v_request_id := v_row->>'request_id';
    if v_seen_ids ? v_request_id or v_seen_triggers ? v_request_id then
      return null;
    end if;
    v_seen_ids := v_seen_ids || jsonb_build_object(v_request_id, true);
    v_sku := v_row->>'sku';
    v_at := (v_row->>'purchased_at_utc')::timestamptz;
    if v_at > v_server_time then
      return null;
    end if;
    v_bound_count := 0;
    v_bound_cycle_id := null;
    for v_bounds_row in select value from jsonb_array_elements(v_effect_timeline->'effect_cycle_bounds')
    loop
      if v_at >= (v_bounds_row->>'started_at_utc')::timestamptz
        and (v_bounds_row->'ended_at_utc' = 'null'::jsonb
          or v_at < (v_bounds_row->>'ended_at_utc')::timestamptz)
      then
        v_bound_count := v_bound_count + 1;
        v_bound_cycle_id := v_bounds_row->>'cycle_id';
      end if;
    end loop;
    if v_bound_count <> 1 then
      return null;
    end if;

    v_effect_count := 0;
    v_effects := jsonb_build_object(
      'token_earning_bps', 0, 'civilization_growth_bps', 0,
      'shop_discount_bps', 0, 'reset_cooldown_bps', 0,
      'natural_removal_discount_bps', 0, 'era_reward_tokens', 0,
      'streak_reward_tokens', 0
    );
    for v_interval in select value from jsonb_array_elements(v_effect_timeline->'effect_history')
    loop
      if v_interval->>'cycle_id' = v_bound_cycle_id
        and v_at >= (v_interval->>'started_at_utc')::timestamptz
        and (v_interval->'ended_at_utc' = 'null'::jsonb
          or v_at < (v_interval->>'ended_at_utc')::timestamptz)
      then
        v_effect_count := v_effect_count + 1;
        v_effects := v_interval->'effects';
        v_effect_revision := (v_interval->>'revision')::bigint;
      end if;
      if v_interval->>'cycle_id' = v_bound_cycle_id
        and (v_at = (v_interval->>'started_at_utc')::timestamptz
          or (v_interval->'ended_at_utc' <> 'null'::jsonb
            and v_at = (v_interval->>'ended_at_utc')::timestamptz))
      then
        return null;
      end if;
    end loop;
    if v_effect_count > 1
      or (v_effect_count = 0 and exists (
        select 1 from jsonb_array_elements(v_effect_timeline->'effect_history') h(value)
        where h.value->>'cycle_id' = v_bound_cycle_id
          and (h.value->>'started_at_utc')::timestamptz <= v_at
      ))
    then
      return null;
    end if;
    v_quote_effect_revision := (v_row->>'effect_revision')::bigint;
    if (v_effect_count = 0 and v_quote_effect_revision <> 0)
      or (v_effect_count = 1 and v_quote_effect_revision <> v_effect_revision)
    then
      return null;
    end if;
    v_discount_bps := (v_effects->>'shop_discount_bps')::numeric;
    if v_discount_bps < 0 or v_discount_bps > 10000 then
      return null;
    end if;
    select p.price::numeric into v_base_price
    from private.shop_products p
    where p.sku = v_sku and p.purchasable;
    if not found then
      return null;
    end if;
    v_expected_price := greatest(
      1::numeric,
      ceil(v_base_price * (10000::numeric - v_discount_bps) / 10000::numeric)
    );
    v_price := (v_row->>'price')::numeric;
    if v_price <> v_expected_price or v_price < 1 or v_price > v_max then
      return null;
    end if;

    v_ownership := v_row->'ownership';
    if v_row->>'category' = 'landscape' then
      select count(*), min((i.value->>'acquired_at_utc')::timestamptz)
      into v_match_count, v_acquired_at
      from jsonb_array_elements(p_data->'landscape_instances') i(value)
      where i.value->>'instance_id' = v_ownership->>'instance_id'
        and i.value->>'sku' = v_sku;
    elsif v_row->>'category' = 'avatar' then
      select count(*), min((a.value->>'acquired_at_utc')::timestamptz)
      into v_match_count, v_acquired_at
      from jsonb_array_elements(p_data->'avatar_owned') a(value)
      where a.value->>'purchase_id' = v_request_id and a.value->>'sku' = v_sku;
    else
      return null;
    end if;
    if v_match_count <> 1 or v_acquired_at is distinct from v_at then
      return null;
    end if;

    v_purchase_total := v_purchase_total + v_price;
    v_events := v_events || jsonb_build_array(jsonb_build_object(
      'side', 'debit', 'kind', 'purchase_debit',
      'source_id', v_request_id,
      'cycle_id', v_bound_cycle_id,
      'sku', v_sku,
      'at_utc', v_row->>'purchased_at_utc',
      'amount', v_price::bigint
    ));
  end loop;

  -- This path is also unreachable until native removal evidence is accepted.
  for v_row in select value from jsonb_array_elements(v_removal_rows)
  loop
    v_request_id := v_row->>'request_id';
    if v_seen_ids ? v_request_id or v_seen_triggers ? v_request_id then
      return null;
    end if;
    v_seen_ids := v_seen_ids || jsonb_build_object(v_request_id, true);
    v_cycle_id := v_row->>'cycle_id';
    v_at := (v_row->>'removed_at_utc')::timestamptz;
    if v_at > v_server_time then
      return null;
    end if;
    v_bound_count := 0;
    for v_bounds_row in select value from jsonb_array_elements(v_effect_timeline->'effect_cycle_bounds')
    loop
      if v_bounds_row->>'cycle_id' = v_cycle_id
        and v_at >= (v_bounds_row->>'started_at_utc')::timestamptz
        and (v_bounds_row->'ended_at_utc' = 'null'::jsonb
          or v_at < (v_bounds_row->>'ended_at_utc')::timestamptz)
      then
        v_bound_count := v_bound_count + 1;
      end if;
    end loop;
    if v_bound_count <> 1 then
      return null;
    end if;
    v_effect_count := 0;
    for v_interval in select value from jsonb_array_elements(v_effect_timeline->'effect_history')
    loop
      if v_interval->>'cycle_id' = v_cycle_id
        and v_at >= (v_interval->>'started_at_utc')::timestamptz
        and (v_interval->'ended_at_utc' = 'null'::jsonb
          or v_at < (v_interval->>'ended_at_utc')::timestamptz)
      then
        v_effect_count := v_effect_count + 1;
      end if;
      if v_interval->>'cycle_id' = v_cycle_id
        and (v_at = (v_interval->>'started_at_utc')::timestamptz
          or (v_interval->'ended_at_utc' <> 'null'::jsonb
            and v_at = (v_interval->>'ended_at_utc')::timestamptz))
      then
        return null;
      end if;
    end loop;
    if v_effect_count <> 0 or exists (
      select 1 from jsonb_array_elements(v_effect_timeline->'effect_history') h(value)
      where h.value->>'cycle_id' = v_cycle_id
        and (h.value->>'started_at_utc')::timestamptz <= v_at
    ) then
      return null;
    end if;
    select count(*), min((d.value->>'created_at_utc')::timestamptz)
    into v_match_count, v_acquired_at
    from jsonb_array_elements(p_data->'removal_debits') d(value)
    where d.value->>'request_id' = v_request_id;
    if v_match_count <> 1 or v_acquired_at is distinct from v_at then
      return null;
    end if;
    v_stage := (v_row->>'stage')::integer;
    v_price := (v_row->>'price')::numeric;
    if v_price is distinct from private.shop_natural_removal_price(
      case v_stage when 0 then 100000::bigint when 1 then 250000::bigint
        when 2 then 500000::bigint when 3 then 1000000::bigint else 2000000::bigint end,
      0
    )::numeric or v_price < 1 or v_price > v_max then
      return null;
    end if;
    v_removal_total := v_removal_total + v_price;
    v_events := v_events || jsonb_build_array(jsonb_build_object(
      'side', 'debit', 'kind', 'natural_removal_debit',
      'source_id', v_request_id,
      'cycle_id', v_cycle_id,
      'stage', v_stage,
      'ordinal', (v_row->>'ordinal')::integer,
      'at_utc', v_row->>'removed_at_utc',
      'amount', v_price::bigint
    ));
  end loop;

  v_server_credit_total := v_server_credit_total + v_streak_total;
  v_local_claim_balance := v_claim_total + v_wallet_total - v_purchase_total - v_removal_total;
  v_expected_final_balance := v_server_credit_total - v_purchase_total - v_removal_total;
  if v_server_credit_total > v_max or v_local_claim_balance < 0
    or v_local_claim_balance > v_max
    or v_expected_final_balance < 0 or v_expected_final_balance > v_max
    or v_local_claim_balance is distinct from v_expected_final_balance
    or v_purchase_total > v_max or v_removal_total > v_max
  then
    return null;
  end if;

  v_prefix_result := private.shop_guest_import_ledger_prefix_normalize(v_events);
  if v_prefix_result is null
    or (v_prefix_result->>'ending_balance')::numeric is distinct from v_expected_final_balance
    or (v_prefix_result->>'ending_balance')::numeric is distinct from v_local_claim_balance
  then
    return null;
  end if;
  v_prefixes := v_prefix_result->'timestamp_groups';
  v_final_balance := (v_prefix_result->>'ending_balance')::numeric;

  return jsonb_build_object(
    'validation_scope', 'no_era_credit_only_ledger_consistency',
    'events', v_events,
    'timestamp_groups', v_prefixes,
    'raw_claim_total', v_claim_total::bigint,
    'wallet_credit_total', v_wallet_total::bigint,
    'credits_total', v_server_credit_total::bigint,
    'debits_total', (v_purchase_total + v_removal_total)::bigint,
    'local_claim_balance', v_local_claim_balance::bigint,
    'final_balance', v_final_balance::bigint
  );
exception when others then
  return null;
end;
$$;

create or replace function private.shop_guest_import_target_is_fresh(p_user_id uuid)
returns boolean
language sql
stable
security definer
set search_path = ''
as $$
  select p_user_id is not null and not exists (
    select 1 from public.planet_member_state p where p.user_id = p_user_id
    union all select 1 from public.daily_usage_snapshots d where d.user_id = p_user_id
    union all select 1 from public.worlds w where w.owner_id = p_user_id
    union all select 1 from private.planet_device_state d where d.user_id = p_user_id
    union all select 1 from private.planet_wallet_credits w where w.user_id = p_user_id
    union all select 1 from private.growth_journal_state g where g.user_id = p_user_id
    union all select 1 from private.growth_journal_cycles g where g.user_id = p_user_id
    union all select 1 from private.growth_journal_days g where g.user_id = p_user_id
    union all select 1 from private.shop_account_state s where s.user_id = p_user_id
    union all select 1 from private.shop_purchase s where s.user_id = p_user_id
    union all select 1 from private.shop_landscape_instance s where s.user_id = p_user_id
    union all select 1 from private.shop_landscape_placement s where s.user_id = p_user_id
    union all select 1 from private.shop_avatar_owned s where s.user_id = p_user_id
    union all select 1 from private.shop_avatar_equipment s where s.user_id = p_user_id
    union all select 1 from private.shop_action_request s where s.user_id = p_user_id
    union all select 1 from private.shop_effect_history s where s.user_id = p_user_id
    union all select 1 from private.shop_natural_removal s where s.user_id = p_user_id
    union all select 1 from private.shop_game_reward s where s.user_id = p_user_id
    union all select 1 from private.shop_reset_request s where s.user_id = p_user_id
    union all select 1 from private.shop_cycle_token_settlement s where s.user_id = p_user_id
    union all select 1 from private.shop_device_contribution_state s where s.user_id = p_user_id
    union all select 1 from private.shop_effect_contribution s where s.user_id = p_user_id
    union all select 1 from private.shop_device_activity_day s where s.user_id = p_user_id
    union all select 1 from private.shop_activity_day s where s.user_id = p_user_id
    union all select 1 from private.shop_cycle_effect_baseline s where s.user_id = p_user_id
    union all select 1 from private.shop_planet_object_generation_baseline s where s.user_id = p_user_id
    union all select 1 from private.cosmetic_purchase s where s.user_id = p_user_id
    union all select 1 from private.cosmetic_purchase_request s where s.user_id = p_user_id
    union all select 1 from private.cosmetic_equipment s where s.user_id = p_user_id
    union all select 1 from private.cosmetic_guest_import_request s where s.user_id = p_user_id
    union all select 1 from private.shop_guest_bootstrap_receipt s
      where s.user_id = p_user_id and s.status = 'imported'
  );
$$;

create or replace function private.shop_guest_bootstrap(
  p_import_id uuid,
  p_request jsonb
)
returns jsonb
language plpgsql
security invoker
set search_path = ''
as $$
declare
  v_user_id uuid := (select auth.uid());
  v_snapshot jsonb;
  v_data jsonb;
  v_metadata jsonb;
  v_journal_cycle jsonb;
  v_now timestamptz;
  v_normalized jsonb;
  v_state jsonb;
  v_prior_payload jsonb;
  v_prior_result jsonb;
  v_held_payload jsonb;
  v_held_result jsonb;
  v_status text;
  v_result jsonb;
begin
  if v_user_id is null then
    raise exception 'authentication required' using errcode = '42501';
  end if;
  if p_request is null or jsonb_typeof(p_request) is distinct from 'object'
    or jsonb_typeof(p_request->'snapshot') is distinct from 'object'
    or jsonb_typeof(p_request#>'{snapshot,target_account_id}') is distinct from 'string'
  then
    raise exception 'guest shop bootstrap envelope is invalid' using errcode = '23514';
  end if;

  v_snapshot := p_request->'snapshot';
  if v_snapshot->>'target_account_id' is distinct from 'account:' || v_user_id::text then
    raise exception 'guest shop bootstrap target does not match authenticated account'
      using errcode = '42501';
  end if;
  if not private.shop_guest_import_envelope_valid(p_import_id, p_request) then
    raise exception 'guest shop bootstrap envelope is invalid' using errcode = '23514';
  end if;
  v_data := v_snapshot->'data';

  -- Serialize with normal account mutations without initializing game state.
  insert into private.shop_account_lock(user_id) values (v_user_id)
  on conflict (user_id) do nothing;
  perform 1 from private.shop_account_lock l
  where l.user_id = v_user_id
  for update;

  select r.payload, r.result
    into v_prior_payload, v_prior_result
  from private.shop_guest_bootstrap_receipt r
  where r.user_id = v_user_id and r.import_id = p_import_id;
  if found then
    if v_prior_payload = p_request then
      return v_prior_result;
    end if;
    return jsonb_build_object('import_id', p_import_id, 'status', 'request_conflict');
  end if;

  -- A previous public held receipt is immutable and must never be promoted.
  select r.payload, r.result
    into v_held_payload, v_held_result
  from private.shop_guest_import_request r
  where r.user_id = v_user_id and r.import_id = p_import_id;
  if found then
    if v_held_payload = p_request then
      return v_held_result;
    end if;
    return jsonb_build_object('import_id', p_import_id, 'status', 'request_conflict');
  end if;

  if not private.shop_guest_import_target_is_fresh(v_user_id) then
    v_status := 'active_account';
  else
    v_now := transaction_timestamp();
    v_normalized := private.shop_guest_import_bootstrap_source_normalize(v_data, v_now);
    if v_normalized is null then
      v_status := 'source_unverifiable';
    else
      v_metadata := v_normalized->'source_metadata';
      v_journal_cycle := v_metadata#>'{growth_journal_cycles,0}';

      insert into public.planet_member_state(
        user_id, state_version, nickname, avatar, timezone,
        current_cycle_id, cycle_started_at, last_reset_at,
        current_planet_tokens, lifetime_tokens, growth_credit,
        stage, progress_to_next, incomplete, objects, shared_visible,
        reset_available_at, reset_cooldown_bps, updated_at
      ) values (
        v_user_id, 1,
        v_metadata#>>'{profile,nickname}',
        v_metadata#>>'{profile,avatar}',
        v_metadata->>'planet_timezone',
        v_metadata#>>'{current_cycle,cycle_id}',
        (v_metadata#>>'{current_cycle,started_at_utc}')::timestamptz,
        null,
        0, 0, 0,
        0, 0, false, '[]'::jsonb, false,
        null, 0, v_now
      );

      insert into private.shop_account_state(
        user_id, state_revision, reward_timezone, updated_at, reward_timezone_initialized
      ) values (
        v_user_id, 0, v_metadata->>'reward_timezone', v_now, true
      );

      insert into private.growth_journal_state(user_id, generation, deleted_at, timezone)
      values (
        v_user_id,
        0,
        null,
        v_metadata->>'planet_timezone'
      );

      insert into private.growth_journal_cycles(
        user_id, cycle_id, started_at, ended_at, wallet_credit, wallet_credit_at
      ) values (
        v_user_id,
        v_journal_cycle->>'cycle_id',
        (v_journal_cycle->>'started_at_utc')::timestamptz,
        null,
        null,
        null
      );

      -- This canonical projection is read-only. In particular, do not call the
      -- timeline RPC, which creates a baseline row as a side effect.
      v_state := private.shop_state_json(v_user_id);
      v_result := jsonb_build_object(
        'import_id', p_import_id,
        'status', 'imported',
        'result', v_state,
        'source_metadata', v_metadata
      );
      insert into private.shop_guest_bootstrap_receipt(
        user_id, import_id, payload, source_fingerprint, status, result
      ) values (
        v_user_id, p_import_id, p_request,
        v_snapshot->>'source_fingerprint', 'imported', v_result
      );
      return v_result;
    end if;
  end if;

  v_result := jsonb_build_object('import_id', p_import_id, 'status', v_status);
  insert into private.shop_guest_bootstrap_receipt(
    user_id, import_id, payload, source_fingerprint, status, result
  ) values (
    v_user_id, p_import_id, p_request,
    v_snapshot->>'source_fingerprint', v_status, v_result
  );
  return v_result;
end;
$$;

create or replace function public.import_guest_shop(p_import_id uuid, p_request jsonb)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  v_user_id uuid := (select auth.uid());
  v_target_account_id text;
  v_prior_payload jsonb;
  v_prior_result jsonb;
  v_status text;
  v_result jsonb;
begin
  if v_user_id is null then
    raise exception 'authentication required' using errcode = '42501';
  end if;
  if p_request is null or jsonb_typeof(p_request) is distinct from 'object'
    or jsonb_typeof(p_request->'snapshot') is distinct from 'object'
    or jsonb_typeof(p_request#>'{snapshot,target_account_id}') is distinct from 'string' then
    raise exception 'guest shop import envelope is invalid' using errcode = '23514';
  end if;

  v_target_account_id := p_request#>>'{snapshot,target_account_id}';
  if v_target_account_id is distinct from 'account:' || v_user_id::text then
    raise exception 'guest shop import target does not match authenticated account'
      using errcode = '42501';
  end if;
  if not private.shop_guest_import_envelope_valid(p_import_id, p_request) then
    raise exception 'guest shop import envelope is invalid' using errcode = '23514';
  end if;

  -- Share the existing account lock contract without initializing any game state.
  insert into private.shop_account_lock(user_id) values (v_user_id) on conflict do nothing;
  perform 1 from private.shop_account_lock l where l.user_id = v_user_id for update;

  select r.payload, r.result into v_prior_payload, v_prior_result
  from private.shop_guest_import_request r
  where r.user_id = v_user_id and r.import_id = p_import_id;
  if found then
    if v_prior_payload = p_request then
      return v_prior_result;
    end if;
    return jsonb_build_object('import_id', p_import_id, 'status', 'request_conflict');
  end if;

  if not private.shop_guest_import_target_is_fresh(v_user_id) then
    v_status := 'active_account';
  else
    v_status := 'source_unverifiable';
  end if;

  -- The local fingerprint and disposition are correlation/integrity metadata only.
  -- This stage has no server proof path, so every fresh accepted envelope is held.
  v_result := jsonb_build_object('import_id', p_import_id, 'status', v_status);
  insert into private.shop_guest_import_request(
    user_id, import_id, payload, source_fingerprint, status, result
  ) values (
    v_user_id, p_import_id, p_request,
    p_request#>>'{snapshot,source_fingerprint}', v_status, v_result
  );
  return v_result;
end;
$$;

-- Validate the captured native empty bootstrap shape without reading or writing
-- account state. This is source consistency only; it does not prove provenance,
-- grant credits, or authorize public import success.
create or replace function private.shop_guest_import_bootstrap_source_normalize(
  p_data jsonb,
  p_now timestamptz
)
returns jsonb
language plpgsql
stable
set search_path = ''
as $$
declare
  v_cycle_id text;
  v_device_id text;
  v_nickname text;
  v_start timestamptz;
  v_activation timestamptz;
  v_journal_cycle jsonb;
  v_array_key text;
  v_rust_trim_whitespace text[] := array[
    chr(9), chr(10), chr(11), chr(12), chr(13), chr(32), chr(133), chr(160),
    chr(5760), chr(8192), chr(8193), chr(8194), chr(8195), chr(8196),
    chr(8197), chr(8198), chr(8199), chr(8200), chr(8201), chr(8202),
    chr(8232), chr(8233), chr(8239), chr(8287), chr(12288)
  ];
begin
  if p_now is null or not pg_catalog.isfinite(p_now)
    or not private.shop_guest_import_data_valid(p_data)
    or p_data->'profile' = 'null'::jsonb
    or p_data->'current_cycle'->'is_current' is distinct from 'true'::jsonb
    or p_data->'current_cycle'->'ended_at_utc' is distinct from 'null'::jsonb
    or p_data->'current_cycle'->'settled_bonus_tokens' is distinct from 'null'::jsonb
    or p_data->'effect_timeline_state' is distinct from 'null'::jsonb
    or p_data->'contribution_canonical_version' is distinct from 'null'::jsonb
    or p_data->'effect_cycle_bounds_authoritative' is distinct from 'false'::jsonb
    or p_data->'reset_receipts_unverifiable' is distinct from 'false'::jsonb
    or p_data->'legacy_partial_import_pending' is distinct from 'false'::jsonb
    or p_data->'last_reset_at_utc' is distinct from 'null'::jsonb
    or p_data->'reset_available_at_utc' is distinct from 'null'::jsonb
    or not private.shop_guest_import_uint_valid(p_data->'shop_state_revision', 0)
    or not private.shop_guest_import_uint_valid(p_data->'lifetime_usage_tokens', 0)
    or not private.shop_guest_import_uint_valid(p_data->'current_cycle_usage_tokens', 0)
    or jsonb_typeof(p_data->'growth_journal_state') is distinct from 'object'
    or not private.shop_guest_import_has_keys(
      p_data->'growth_journal_state', array['generation','deleted_at_utc']
    )
    or not private.shop_guest_import_uint_valid(p_data#>'{growth_journal_state,generation}', 0)
    or p_data#>'{growth_journal_state,deleted_at_utc}' is distinct from 'null'::jsonb
    or jsonb_typeof(p_data->'growth_journal_cycles') is distinct from 'array'
    or jsonb_array_length(p_data->'growth_journal_cycles') <> 1
    or jsonb_typeof(p_data->'world_timezone') is distinct from 'string'
    or p_data->'world_timezone' is distinct from p_data->'planet_timezone'
    or p_data->'world_timezone' is distinct from p_data->'reward_timezone'
  then
    return null;
  end if;

  v_nickname := p_data#>>'{profile,nickname}';
  if not private.shop_guest_import_has_keys(p_data->'profile', array['nickname','avatar'])
    or jsonb_typeof(p_data#>'{profile,nickname}') is distinct from 'string'
    or char_length(v_nickname) not between 1 and 24
    or left(v_nickname, 1) = any(v_rust_trim_whitespace)
    or right(v_nickname, 1) = any(v_rust_trim_whitespace)
    or jsonb_typeof(p_data#>'{profile,avatar}') is distinct from 'string'
    or p_data#>>'{profile,avatar}' not in ('masculine','feminine')
  then
    return null;
  end if;

  v_cycle_id := p_data#>>'{current_cycle,cycle_id}';
  v_device_id := p_data->>'planet_device_id';
  v_start := (p_data#>>'{current_cycle,started_at_utc}')::timestamptz;
  v_activation := (p_data->>'activation_at_utc')::timestamptz;
  if v_cycle_id is null or v_cycle_id !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
    or (v_cycle_id::uuid)::text is distinct from v_cycle_id
    or v_device_id is null
    or (v_device_id::uuid)::text is distinct from v_device_id
    or v_activation is distinct from v_start
    or v_activation > p_now
  then
    return null;
  end if;

  foreach v_array_key in array array[
    'historical_cycles', 'natural_objects', 'landscape_instances', 'placements',
    'landscape_edit_versions', 'avatar_owned', 'avatar_equipment',
    'cosmetic_equipment', 'pending_purchases', 'cosmetic_purchases', 'purchases',
    'natural_removals', 'removal_debits', 'effect_history', 'effect_cycle_bounds',
    'effect_contributions', 'activity_days', 'game_rewards', 'wallet_credits',
    'unverified_planet_wallet_claims', 'cycle_settlements', 'era_progress',
    'daily_agent_totals', 'usage_aggregates', 'cycle_usage_totals',
    'growth_journal_entries', 'reset_settlement_proofs', 'integrity_issues'
  ] loop
    if jsonb_array_length(p_data->v_array_key) <> 0 then
      return null;
    end if;
  end loop;

  if (p_data ? 'purchase_proofs' and jsonb_array_length(p_data->'purchase_proofs') <> 0)
    or (p_data ? 'removal_proofs' and jsonb_array_length(p_data->'removal_proofs') <> 0)
  then
    return null;
  end if;

  v_journal_cycle := p_data#>'{growth_journal_cycles,0}';
  if not private.shop_guest_import_has_keys(v_journal_cycle, array[
      'cycle_id','started_at_utc','ended_at_utc','wallet_credit','wallet_credit_at_utc'
    ])
    or jsonb_typeof(v_journal_cycle->'cycle_id') is distinct from 'string'
    or v_journal_cycle->>'cycle_id' is distinct from v_cycle_id
    or not private.shop_guest_import_timestamp_valid(v_journal_cycle->'started_at_utc')
    or (v_journal_cycle->>'started_at_utc')::timestamptz is distinct from v_start
    or v_journal_cycle->'ended_at_utc' is distinct from 'null'::jsonb
    or v_journal_cycle->'wallet_credit' is distinct from 'null'::jsonb
    or v_journal_cycle->'wallet_credit_at_utc' is distinct from 'null'::jsonb
  then
    return null;
  end if;

  return jsonb_build_object(
    'validation_scope', 'native_empty_source_consistency',
    'source_metadata', jsonb_build_object(
      'profile', p_data->'profile',
      'planet_device_id', p_data->'planet_device_id',
      'world_timezone', p_data->'world_timezone',
      'planet_timezone', p_data->'planet_timezone',
      'reward_timezone', p_data->'reward_timezone',
      'activation_at_utc', p_data->'activation_at_utc',
      'current_cycle', p_data->'current_cycle',
      'shop_state_revision', p_data->'shop_state_revision',
      'growth_journal_state', p_data->'growth_journal_state',
      'growth_journal_cycles', p_data->'growth_journal_cycles',
      'effect_timeline_state', p_data->'effect_timeline_state',
      'effect_cycle_bounds', p_data->'effect_cycle_bounds',
      'effect_cycle_bounds_authoritative', p_data->'effect_cycle_bounds_authoritative',
      'effect_history', p_data->'effect_history',
      'contribution_canonical_version', p_data->'contribution_canonical_version'
    ),
    'derived_zero_state', jsonb_build_object(
      'wallet_balance_tokens', 0,
      'current_cycle_usage_tokens', 0,
      'lifetime_usage_tokens', 0,
      'raw_cycle_tokens', 0,
      'bonus_cycle_tokens', 0,
      'growth_tokens', 0,
      'stage', 0,
      'progress', 0,
      'natural_objects', '[]'::jsonb,
      'landscape_instances', '[]'::jsonb,
      'placements', '[]'::jsonb,
      'avatar_owned', '[]'::jsonb,
      'avatar_equipment', '[]'::jsonb,
      'cosmetic_equipment', '[]'::jsonb
    ),
    'server_policy', jsonb_build_object('shared_visible', false)
  );
exception when others then
  return null;
end;
$$;

revoke all on function private.shop_guest_import_has_keys(jsonb, text[], text[]),
  private.shop_guest_import_string_valid(jsonb, integer, integer, boolean),
  private.shop_guest_import_uint_valid(jsonb, numeric, boolean),
  private.shop_guest_import_timestamp_valid(jsonb, boolean),
  private.shop_guest_import_timezone_valid(jsonb),
  private.shop_guest_import_cycle_valid(jsonb),
  private.shop_guest_import_data_valid(jsonb),
  private.shop_guest_import_envelope_valid(uuid, jsonb),
  private.shop_guest_import_ownership_normalize(jsonb),
  private.shop_guest_import_effect_timeline_normalize(jsonb, jsonb, jsonb),
  private.shop_guest_import_usage_core_normalize(jsonb),
  private.shop_guest_import_usage_normalize(jsonb, jsonb),
  private.shop_guest_import_reset_normalize(jsonb, jsonb, jsonb),
  private.shop_guest_import_cycle_token_normalize(jsonb, jsonb, jsonb, jsonb),
  private.shop_guest_import_streak_mirrors_normalize(jsonb, jsonb, jsonb),
  private.shop_guest_import_rewards_normalize(jsonb, jsonb, jsonb, jsonb),
  private.shop_guest_import_ledger_prefix_normalize(jsonb),
  private.shop_guest_import_ledger_normalize(jsonb),
  private.shop_guest_import_bootstrap_source_normalize(jsonb, timestamptz),
  private.shop_guest_bootstrap(uuid, jsonb),
  private.shop_guest_shop_import_known_cycle(jsonb, text),
  private.shop_guest_import_target_is_fresh(uuid)
  from public, anon, authenticated, service_role;
revoke all on function public.import_guest_shop(uuid, jsonb) from public, anon;
grant execute on function public.import_guest_shop(uuid, jsonb) to authenticated;
