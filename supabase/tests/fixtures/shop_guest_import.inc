create function pg_temp.shop_guest_import_empty_request(
  p_import_id uuid,
  p_user_id uuid,
  p_cycle_id text
)
returns jsonb
language sql
immutable
as $$
  select jsonb_build_object(
    'schema_version', 1,
    'snapshot', jsonb_build_object(
      'import_id', p_import_id,
      'target_account_id', 'account:' || p_user_id::text,
      'source_account_id', 'local',
      'source_fingerprint', repeat('a', 64),
      'disposition', 'local_integrity_validated',
      'data', jsonb_build_object(
        'world_timezone', 'UTC',
        'planet_timezone', 'UTC',
        'reward_timezone', 'UTC',
        'planet_device_id', '30000000-0000-4000-a000-000000000001',
        'shop_state_revision', 0,
        'profile', null,
        'activation_at_utc', '2026-10-01T00:00:00+00:00',
        'current_cycle', jsonb_build_object(
          'cycle_id', p_cycle_id,
          'started_at_utc', '2026-10-01T00:00:00+00:00',
          'ended_at_utc', null,
          'is_current', true,
          'settled_bonus_tokens', null
        ),
        'historical_cycles', '[]'::jsonb,
        'last_reset_at_utc', null,
        'reset_available_at_utc', null,
        'effect_timeline_state', null,
        'effect_cycle_bounds_authoritative', false,
        'contribution_canonical_version', null,
        'natural_objects', '[]'::jsonb,
        'landscape_instances', '[]'::jsonb,
        'placements', '[]'::jsonb,
        'landscape_edit_versions', '[]'::jsonb,
        'avatar_owned', '[]'::jsonb,
        'avatar_equipment', '[]'::jsonb,
        'cosmetic_equipment', '[]'::jsonb,
        'pending_purchases', '[]'::jsonb,
        'cosmetic_purchases', '[]'::jsonb,
        'purchases', '[]'::jsonb,
        'purchase_proofs', '[]'::jsonb,
        'natural_removals', '[]'::jsonb,
        'removal_debits', '[]'::jsonb,
        'removal_proofs', '[]'::jsonb,
        'effect_history', '[]'::jsonb,
        'effect_cycle_bounds', '[]'::jsonb,
        'effect_contributions', '[]'::jsonb,
        'activity_days', '[]'::jsonb,
        'game_rewards', '[]'::jsonb,
        'wallet_credits', '[]'::jsonb,
        'unverified_planet_wallet_claims', '[]'::jsonb,
        'cycle_settlements', '[]'::jsonb,
        'era_progress', '[]'::jsonb,
        'daily_agent_totals', '[]'::jsonb,
        'usage_aggregates', '[]'::jsonb,
        'lifetime_usage_tokens', null,
        'current_cycle_usage_tokens', null,
        'cycle_usage_totals', '[]'::jsonb,
        'growth_journal_state', null,
        'growth_journal_cycles', '[]'::jsonb,
        'growth_journal_entries', '[]'::jsonb,
        'reset_settlement_proofs', '[]'::jsonb,
        'integrity_issues', '[]'::jsonb,
        'reset_receipts_unverifiable', false,
        'legacy_partial_import_pending', false
      )
    )
  );
$$;

create function pg_temp.shop_guest_import_with_wallet_claim(
  p_import_id uuid,
  p_user_id uuid,
  p_cycle_id text
)
returns jsonb
language sql
immutable
as $$
  select jsonb_set(
    pg_temp.shop_guest_import_empty_request(p_import_id, p_user_id, p_cycle_id),
    '{snapshot,data,unverified_planet_wallet_claims}',
    jsonb_build_array(jsonb_build_object(
      'previous_cycle_id', 'previous-cycle',
      'claimed_amount', 9000000,
      'created_at_utc', '2026-10-01T00:00:00+00:00'
    )),
    false
  );
$$;

create function pg_temp.shop_guest_import_receipt_count(
  p_user_id uuid,
  p_import_id uuid
)
returns integer
language plpgsql
as $$
declare
  v_count integer;
begin
  execute 'select count(*)::integer from private.shop_guest_import_request where user_id = $1 and import_id = $2'
    into v_count using p_user_id, p_import_id;
  return v_count;
exception when undefined_table then
  return 0;
end;
$$;

create function pg_temp.shop_guest_import_receipt_result(
  p_user_id uuid,
  p_import_id uuid
)
returns jsonb
language plpgsql
security definer
as $$
declare
  v_result jsonb;
begin
  execute 'select result from private.shop_guest_import_request where user_id = $1 and import_id = $2'
    into v_result using p_user_id, p_import_id;
  return v_result;
exception when undefined_table then
  return null;
end;
$$;

create function pg_temp.shop_guest_import_no_select(p_role text)
returns boolean
language plpgsql
as $$
begin
  if to_regclass('private.shop_guest_import_request') is null then
    return false;
  end if;
  return not has_table_privilege(p_role, 'private.shop_guest_import_request', 'select');
exception when others then
  return false;
end;
$$;

create function pg_temp.shop_guest_import_purchase_removal_request(
  p_import_id uuid,
  p_user_id uuid,
  p_cycle_id text
)
returns jsonb
language sql
immutable
as $$
  with base as (
    select pg_temp.shop_guest_import_empty_request(p_import_id, p_user_id, p_cycle_id) as request
  ), data0 as (
    select request, request#>'{snapshot,data}' as data from base
  ), data1 as (
    select request, jsonb_set(data, '{landscape_instances}', jsonb_build_array(
      jsonb_build_object(
        'instance_id', '20000000-0000-4000-a000-000000000001',
        'sku', 'land_pond', 'variation_index', 0, 'seed', 'pond-seed',
        'variation_version', 1, 'acquired_at_utc', '2026-10-01T00:00:01+00:00'
      )
    )) as data from data0
  ), data2 as (
    select request, jsonb_set(data, '{landscape_edit_versions}', jsonb_build_array(
      jsonb_build_object('instance_id', '20000000-0000-4000-a000-000000000001', 'version', 0)
    )) as data from data1
  ), data3 as (
    select request, jsonb_set(data, '{purchases}', jsonb_build_array(
      jsonb_build_object(
        'purchase_id', 'purchase-pond-1', 'sku', 'land_pond', 'price', 5000000,
        'purchased_at_utc', '2026-10-01T00:00:01+00:00'
      )
    )) as data from data2
  ), data4 as (
    select request, jsonb_set(data, '{purchase_proofs}', jsonb_build_array(
      jsonb_build_object(
        'request_id', 'purchase-pond-1',
        'quote', jsonb_build_object(
          'target', jsonb_build_object('kind', 'purchase', 'sku', 'land_pond'),
          'catalog_revision', 1, 'effect_revision', 0, 'price', 5000000
        ),
        'status', 'purchased',
        'ownership', jsonb_build_object(
          'kind', 'landscape', 'instance_id', '20000000-0000-4000-a000-000000000001'
        )
      )
    )) as data from data3
  ), data5 as (
    select request, jsonb_set(data, '{natural_objects}', jsonb_build_array(
      jsonb_build_object(
        'cycle_id', p_cycle_id, 'stage', 0, 'ordinal', 0, 'kind', 'rock',
        'x', 1, 'y', 2, 'seed', 17
      )
    )) as data from data4
  ), data6 as (
    select request, jsonb_set(data, '{natural_removals}', jsonb_build_array(
      jsonb_build_object(
        'cycle_id', p_cycle_id, 'stage', 0, 'ordinal', 0, 'version', 1,
        'removed_at_utc', '2026-10-01T00:00:02+00:00'
      )
    )) as data from data5
  ), data7 as (
    select request, jsonb_set(data, '{removal_debits}', jsonb_build_array(
      jsonb_build_object(
        'request_id', 'remove-rock-1', 'amount', 100000,
        'created_at_utc', '2026-10-01T00:00:02+00:00'
      )
    )) as data from data6
  ), data8 as (
    select request, jsonb_set(data, '{removal_proofs}', jsonb_build_array(
      jsonb_build_object(
        'request_id', 'remove-rock-1',
        'target', jsonb_build_object(
          'cycle_id', p_cycle_id, 'stage', 0, 'ordinal', 0
        ),
        'quote', jsonb_build_object(
          'target', jsonb_build_object(
            'kind', 'remove_natural',
            'key', jsonb_build_object('cycle_id', p_cycle_id, 'stage', 0, 'ordinal', 0)
          ),
          'catalog_revision', 1, 'effect_revision', 0, 'price', 100000
        ),
        'status', 'removed'
      )
    )) as data from data7
  )
  select jsonb_set(request, '{snapshot,data}', data)
  from data8;
$$;

create function pg_temp.shop_guest_import_effect_request(
  p_import_id uuid,
  p_user_id uuid,
  p_cycle_id text
)
returns jsonb
language sql
immutable
as $$
  with base as (
    select pg_temp.shop_guest_import_purchase_removal_request(
      p_import_id, p_user_id, p_cycle_id
    ) as request
  ), data0 as (
    select request, jsonb_set(request#>'{snapshot,data}', '{placements}',
      jsonb_build_array(jsonb_build_object(
        'instance_id', '20000000-0000-4000-a000-000000000001',
        'cycle_id', p_cycle_id, 'x', 1, 'y', 2, 'version', 1
      )), true) as data from base
  ), data1 as (
    select request, jsonb_set(data, '{effect_cycle_bounds}', jsonb_build_array(
      jsonb_build_object(
        'cycle_id', p_cycle_id,
        'started_at_utc', '2026-10-01T00:00:00+00:00',
        'ended_at_utc', null
      )
    ), true) as data from data0
  ), data2 as (
    select request, jsonb_set(data, '{effect_cycle_bounds_authoritative}', 'true'::jsonb, true)
      as data from data1
  ), data3 as (
    select request, jsonb_set(data, '{effect_timeline_state}', jsonb_build_object(
      'current_cycle_id', p_cycle_id,
      'effect_revision', 1,
      'server_time_utc', '2026-10-01T00:00:04+00:00',
      'reward_timezone', 'UTC'
    ), true) as data from data2
  ), data4 as (
    select request, jsonb_set(data, '{effect_history}', jsonb_build_array(
      jsonb_build_object(
        'cycle_id', p_cycle_id,
        'revision', 1,
        'started_at_utc', '2026-10-01T00:00:03+00:00',
        'ended_at_utc', null,
        'active_instance_ids', jsonb_build_array(
          '20000000-0000-4000-a000-000000000001'
        ),
        'effects', jsonb_build_object(
          'token_earning_bps', 100,
          'civilization_growth_bps', 0,
          'shop_discount_bps', 0,
          'reset_cooldown_bps', 0,
          'natural_removal_discount_bps', 0,
          'era_reward_tokens', 0,
          'streak_reward_tokens', 0
        )
      )
    ), true) as data from data3
  )
  select jsonb_set(request, '{snapshot,data}', data, true)
  from data4;
$$;

create function pg_temp.shop_guest_import_usage_request(
  p_import_id uuid,
  p_user_id uuid,
  p_cycle_id text
)
returns jsonb
language sql
immutable
as $$
  with base as (
    select pg_temp.shop_guest_import_effect_request(
      p_import_id, p_user_id, p_cycle_id
    ) as request
  ), data as (
    select request, request#>'{snapshot,data}' as source from base
  )
  select jsonb_set(request, '{snapshot,data}', source || jsonb_build_object(
    'contribution_canonical_version', 1,
    'effect_contributions', jsonb_build_array(
      jsonb_build_object(
        'device_id', '30000000-0000-4000-a000-000000000001',
        'cycle_id', p_cycle_id, 'date', '2026-10-01', 'effect_revision', 0,
        'canonical_version', 1, 'tokens', 40, 'growth_bps', 0, 'wallet_bps', 0
      ),
      jsonb_build_object(
        'device_id', '30000000-0000-4000-a000-000000000001',
        'cycle_id', p_cycle_id, 'date', '2026-10-01', 'effect_revision', 1,
        'canonical_version', 1, 'tokens', 60, 'growth_bps', 0, 'wallet_bps', 100
      )
    ),
    'activity_days', jsonb_build_array(jsonb_build_object(
      'reward_date', '2026-10-01', 'cycle_id', p_cycle_id,
      'first_occurred_at_utc', '2026-10-01T00:00:01+00:00',
      'canonical_version', 1, 'tokens', 100
    )),
    'usage_aggregates', jsonb_build_array(jsonb_build_object(
      'cycle_id', p_cycle_id, 'bucket_date', '2026-10-01', 'agent', 'codex',
      'event_count', 2, 'total_tokens', 100, 'coverage', 'complete'
    )),
    'daily_agent_totals', jsonb_build_array(jsonb_build_object(
      'bucket_date', '2026-10-01', 'agent', 'codex',
      'total_tokens', 100, 'coverage', 'complete'
    )),
    'lifetime_usage_tokens', 100,
    'current_cycle_usage_tokens', 100,
    'cycle_usage_totals', jsonb_build_array(jsonb_build_object(
      'cycle_id', p_cycle_id, 'total_tokens', 100
    )),
    'growth_journal_state', jsonb_build_object(
      'generation', 1, 'deleted_at_utc', null
    ),
    'growth_journal_cycles', jsonb_build_array(jsonb_build_object(
      'cycle_id', p_cycle_id,
      'started_at_utc', '2026-10-01T00:00:00+00:00',
      'ended_at_utc', null, 'wallet_credit', null, 'wallet_credit_at_utc', null
    )),
    'growth_journal_entries', jsonb_build_array(jsonb_build_object(
      'device_id', '30000000-0000-4000-a000-000000000001',
      'cycle_id', p_cycle_id, 'bucket_date', '2026-10-01', 'agent', 'codex',
      'revision', 1, 'acknowledged_revision', 0, 'generation', 1,
      'present', true, 'confirmed_tokens', 100, 'coverage', 'complete',
      'payload_hash', repeat('b', 64)
    ))
  ))
  from data;
$$;

create function pg_temp.shop_guest_import_dated_usage_request(
  p_import_id uuid,
  p_user_id uuid,
  p_cycle_id text,
  p_date text,
  p_timezone text,
  p_cycle_start text,
  p_effect_start text,
  p_server_time text,
  p_first_timestamp text
)
returns jsonb
language sql
immutable
as $$
  with base as (
    select pg_temp.shop_guest_import_usage_request(
      p_import_id, p_user_id, p_cycle_id
    ) as request
  ), source as (
    select request, request#>'{snapshot,data}' as data from base
  ), changed as (
    select request, data || jsonb_build_object(
      'world_timezone', p_timezone,
      'planet_timezone', p_timezone,
      'reward_timezone', p_timezone,
      'current_cycle', jsonb_set(data->'current_cycle', '{started_at_utc}', to_jsonb(p_cycle_start)),
      'effect_cycle_bounds', jsonb_build_array(jsonb_set(
        data#>'{effect_cycle_bounds,0}', '{started_at_utc}', to_jsonb(p_cycle_start)
      )),
      'effect_timeline_state', (data->'effect_timeline_state') || jsonb_build_object(
        'server_time_utc', p_server_time, 'reward_timezone', p_timezone
      ),
      'effect_history', jsonb_build_array(jsonb_set(
        data#>'{effect_history,0}', '{started_at_utc}', to_jsonb(p_effect_start)
      )),
      'effect_contributions', jsonb_build_array(
        jsonb_set(data#>'{effect_contributions,0}', '{date}', to_jsonb(p_date)),
        jsonb_set(data#>'{effect_contributions,1}', '{date}', to_jsonb(p_date))
      ),
      'activity_days', jsonb_build_array(
        jsonb_set(
          jsonb_set(data#>'{activity_days,0}', '{reward_date}', to_jsonb(p_date)),
          '{first_occurred_at_utc}', to_jsonb(p_first_timestamp)
        )
      ),
      'usage_aggregates', jsonb_build_array(jsonb_set(
        data#>'{usage_aggregates,0}', '{bucket_date}', to_jsonb(p_date)
      )),
      'daily_agent_totals', jsonb_build_array(jsonb_set(
        data#>'{daily_agent_totals,0}', '{bucket_date}', to_jsonb(p_date)
      )),
      'growth_journal_cycles', jsonb_build_array(jsonb_set(
        data#>'{growth_journal_cycles,0}', '{started_at_utc}', to_jsonb(p_cycle_start)
      )),
      'growth_journal_entries', jsonb_build_array(jsonb_set(
        data#>'{growth_journal_entries,0}', '{bucket_date}', to_jsonb(p_date)
      ))
    ) as data
    from source
  )
  select jsonb_set(request, '{snapshot,data}', data, true)
  from changed;
$$;

create function pg_temp.shop_guest_import_multicycle_usage_request(
  p_import_id uuid,
  p_user_id uuid,
  p_cycle_id text
)
returns jsonb
language plpgsql
immutable
as $$
declare
  v_request jsonb := pg_temp.shop_guest_import_usage_request(
    p_import_id, p_user_id, p_cycle_id
  );
  v_data jsonb := v_request#>'{snapshot,data}';
begin
  v_data := v_data || jsonb_build_object(
    'current_cycle', jsonb_set(
      v_data->'current_cycle', '{started_at_utc}', '"2026-10-01T12:00:00+00:00"'::jsonb
    ),
    'historical_cycles', jsonb_build_array(jsonb_build_object(
      'cycle_id', 'history-cycle',
      'started_at_utc', '2026-10-01T00:00:00+00:00',
      'ended_at_utc', '2026-10-01T12:00:00+00:00',
      'is_current', false, 'settled_bonus_tokens', null
    )),
    'effect_cycle_bounds', jsonb_build_array(
      jsonb_build_object(
        'cycle_id', 'history-cycle',
        'started_at_utc', '2026-10-01T00:00:00+00:00',
        'ended_at_utc', '2026-10-01T12:00:00+00:00'
      ),
      jsonb_build_object(
        'cycle_id', p_cycle_id,
        'started_at_utc', '2026-10-01T12:00:00+00:00',
        'ended_at_utc', null
      )
    ),
    'effect_timeline_state', (v_data->'effect_timeline_state') || jsonb_build_object(
      'server_time_utc', '2026-10-01T13:00:04+00:00'
    ),
    'effect_history', jsonb_build_array(jsonb_set(
      v_data#>'{effect_history,0}', '{started_at_utc}',
      '"2026-10-01T12:00:03+00:00"'::jsonb
    )),
    'effect_contributions', jsonb_build_array(
      jsonb_build_object(
        'device_id', '30000000-0000-4000-a000-000000000001',
        'cycle_id', 'history-cycle', 'date', '2026-10-01', 'effect_revision', 0,
        'canonical_version', 1, 'tokens', 40, 'growth_bps', 0, 'wallet_bps', 0
      ),
      jsonb_build_object(
        'device_id', '30000000-0000-4000-a000-000000000001',
        'cycle_id', p_cycle_id, 'date', '2026-10-01', 'effect_revision', 1,
        'canonical_version', 1, 'tokens', 60, 'growth_bps', 0, 'wallet_bps', 100
      )
    ),
    'activity_days', jsonb_build_array(jsonb_build_object(
      'reward_date', '2026-10-01', 'cycle_id', 'history-cycle',
      'first_occurred_at_utc', '2026-10-01T00:00:01+00:00',
      'canonical_version', 1, 'tokens', 100
    )),
    'usage_aggregates', jsonb_build_array(
      jsonb_build_object(
        'cycle_id', 'history-cycle', 'bucket_date', '2026-10-01', 'agent', 'codex',
        'event_count', 1, 'total_tokens', 40, 'coverage', 'complete'
      ),
      jsonb_build_object(
        'cycle_id', p_cycle_id, 'bucket_date', '2026-10-01', 'agent', 'claude_code',
        'event_count', 1, 'total_tokens', 60, 'coverage', 'complete'
      )
    ),
    'daily_agent_totals', jsonb_build_array(
      jsonb_build_object(
        'bucket_date', '2026-10-01', 'agent', 'codex',
        'total_tokens', 40, 'coverage', 'complete'
      ),
      jsonb_build_object(
        'bucket_date', '2026-10-01', 'agent', 'claude_code',
        'total_tokens', 60, 'coverage', 'complete'
      )
    ),
    'lifetime_usage_tokens', 100,
    'current_cycle_usage_tokens', 60,
    'cycle_usage_totals', jsonb_build_array(
      jsonb_build_object('cycle_id', 'history-cycle', 'total_tokens', 40),
      jsonb_build_object('cycle_id', p_cycle_id, 'total_tokens', 60)
    ),
    'growth_journal_cycles', jsonb_build_array(
      jsonb_build_object(
        'cycle_id', 'history-cycle',
        'started_at_utc', '2026-10-01T00:00:00+00:00',
        'ended_at_utc', '2026-10-01T12:00:00+00:00',
        'wallet_credit', null, 'wallet_credit_at_utc', null
      ),
      jsonb_build_object(
        'cycle_id', p_cycle_id,
        'started_at_utc', '2026-10-01T12:00:00+00:00',
        'ended_at_utc', null,
        'wallet_credit', null, 'wallet_credit_at_utc', null
      )
    ),
    'growth_journal_entries', jsonb_build_array(
      jsonb_build_object(
        'device_id', '30000000-0000-4000-a000-000000000001',
        'cycle_id', 'history-cycle', 'bucket_date', '2026-10-01', 'agent', 'codex',
        'revision', 1, 'acknowledged_revision', 0, 'generation', 1,
        'present', true, 'confirmed_tokens', 40, 'coverage', 'complete',
        'payload_hash', repeat('b', 64)
      ),
      jsonb_build_object(
        'device_id', '30000000-0000-4000-a000-000000000001',
        'cycle_id', p_cycle_id, 'bucket_date', '2026-10-01', 'agent', 'claude_code',
        'revision', 1, 'acknowledged_revision', 0, 'generation', 1,
        'present', true, 'confirmed_tokens', 60, 'coverage', 'complete',
        'payload_hash', repeat('c', 64)
      )
    )
  );
  return jsonb_set(v_request, '{snapshot,data}', v_data, true);
end;
$$;

create function pg_temp.shop_guest_import_single_reset_request(
  p_import_id uuid,
  p_user_id uuid,
  p_current_cycle text
)
returns jsonb
language plpgsql
immutable
as $$
declare
  v_request jsonb := pg_temp.shop_guest_import_multicycle_usage_request(
    p_import_id, p_user_id, p_current_cycle
  );
  v_data jsonb := v_request#>'{snapshot,data}';
  v_reset_at constant text := '2026-10-01T12:00:00+00:00';
  v_reset_available constant text := '2026-10-02T12:00:00+00:00';
  v_instance_id constant text := '20000000-0000-4000-a000-000000000001';
  v_final_effects constant jsonb := jsonb_build_object(
    'token_earning_bps', 100,
    'civilization_growth_bps', 0,
    'shop_discount_bps', 0,
    'reset_cooldown_bps', 0,
    'natural_removal_discount_bps', 0,
    'era_reward_tokens', 0,
    'streak_reward_tokens', 0
  );
begin
  v_data := v_data || jsonb_build_object(
    'current_cycle', jsonb_build_object(
      'cycle_id', p_current_cycle,
      'started_at_utc', v_reset_at,
      'ended_at_utc', null,
      'is_current', true,
      'settled_bonus_tokens', null
    ),
    'historical_cycles', jsonb_build_array(jsonb_build_object(
      'cycle_id', 'history-cycle',
      'started_at_utc', '2026-10-01T00:00:00+00:00',
      'ended_at_utc', v_reset_at,
      'is_current', false,
      'settled_bonus_tokens', 0
    )),
    'last_reset_at_utc', v_reset_at,
    'reset_available_at_utc', v_reset_available,
    'effect_timeline_state', jsonb_build_object(
      'current_cycle_id', p_current_cycle,
      'effect_revision', 2,
      'server_time_utc', '2026-10-01T13:00:00+00:00',
      'reward_timezone', 'UTC'
    ),
    'effect_cycle_bounds_authoritative', true,
    'effect_cycle_bounds', jsonb_build_array(
      jsonb_build_object(
        'cycle_id', 'history-cycle',
        'started_at_utc', '2026-10-01T00:00:00+00:00',
        'ended_at_utc', v_reset_at
      ),
      jsonb_build_object(
        'cycle_id', p_current_cycle,
        'started_at_utc', v_reset_at,
        'ended_at_utc', null
      )
    ),
    'effect_history', jsonb_build_array(
      jsonb_build_object(
        'cycle_id', 'history-cycle', 'revision', 1,
        'started_at_utc', '2026-10-01T00:00:03+00:00',
        'ended_at_utc', v_reset_at,
        'active_instance_ids', jsonb_build_array(v_instance_id),
        'effects', v_final_effects
      ),
      jsonb_build_object(
        'cycle_id', p_current_cycle, 'revision', 2,
        'started_at_utc', v_reset_at,
        'ended_at_utc', null,
        'active_instance_ids', '[]'::jsonb,
        'effects', jsonb_build_object(
          'token_earning_bps', 0,
          'civilization_growth_bps', 0,
          'shop_discount_bps', 0,
          'reset_cooldown_bps', 0,
          'natural_removal_discount_bps', 0,
          'era_reward_tokens', 0,
          'streak_reward_tokens', 0
        )
      )
    ),
    'placements', '[]'::jsonb,
    'contribution_canonical_version', 1,
    'effect_contributions', jsonb_build_array(
      jsonb_build_object(
        'device_id', '30000000-0000-4000-a000-000000000001',
        'cycle_id', 'history-cycle', 'date', '2026-10-01', 'effect_revision', 0,
        'canonical_version', 1, 'tokens', 40, 'growth_bps', 0, 'wallet_bps', 0
      ),
      jsonb_build_object(
        'device_id', '30000000-0000-4000-a000-000000000001',
        'cycle_id', p_current_cycle, 'date', '2026-10-01', 'effect_revision', 2,
        'canonical_version', 1, 'tokens', 60, 'growth_bps', 0, 'wallet_bps', 0
      )
    ),
    'activity_days', jsonb_build_array(jsonb_build_object(
      'reward_date', '2026-10-01', 'cycle_id', 'history-cycle',
      'first_occurred_at_utc', '2026-10-01T00:00:01+00:00',
      'canonical_version', 1, 'tokens', 100
    )),
    'usage_aggregates', jsonb_build_array(
      jsonb_build_object(
        'cycle_id', 'history-cycle', 'bucket_date', '2026-10-01', 'agent', 'codex',
        'event_count', 1, 'total_tokens', 40, 'coverage', 'complete'
      ),
      jsonb_build_object(
        'cycle_id', p_current_cycle, 'bucket_date', '2026-10-01', 'agent', 'claude_code',
        'event_count', 1, 'total_tokens', 60, 'coverage', 'complete'
      )
    ),
    'daily_agent_totals', jsonb_build_array(
      jsonb_build_object(
        'bucket_date', '2026-10-01', 'agent', 'codex',
        'total_tokens', 40, 'coverage', 'complete'
      ),
      jsonb_build_object(
        'bucket_date', '2026-10-01', 'agent', 'claude_code',
        'total_tokens', 60, 'coverage', 'complete'
      )
    ),
    'lifetime_usage_tokens', 100,
    'current_cycle_usage_tokens', 60,
    'cycle_usage_totals', jsonb_build_array(
      jsonb_build_object('cycle_id', 'history-cycle', 'total_tokens', 40),
      jsonb_build_object('cycle_id', p_current_cycle, 'total_tokens', 60)
    ),
    'growth_journal_cycles', jsonb_build_array(
      jsonb_build_object(
        'cycle_id', 'history-cycle',
        'started_at_utc', '2026-10-01T00:00:00+00:00',
        'ended_at_utc', v_reset_at,
        'wallet_credit', 40, 'wallet_credit_at_utc', v_reset_at
      ),
      jsonb_build_object(
        'cycle_id', p_current_cycle,
        'started_at_utc', v_reset_at,
        'ended_at_utc', null,
        'wallet_credit', null, 'wallet_credit_at_utc', null
      )
    ),
    'growth_journal_entries', jsonb_build_array(
      jsonb_build_object(
        'device_id', '30000000-0000-4000-a000-000000000001',
        'cycle_id', 'history-cycle', 'bucket_date', '2026-10-01', 'agent', 'codex',
        'revision', 1, 'acknowledged_revision', 0, 'generation', 1,
        'present', true, 'confirmed_tokens', 40, 'coverage', 'complete',
        'payload_hash', repeat('d', 64)
      ),
      jsonb_build_object(
        'device_id', '30000000-0000-4000-a000-000000000001',
        'cycle_id', p_current_cycle, 'bucket_date', '2026-10-01', 'agent', 'claude_code',
        'revision', 1, 'acknowledged_revision', 0, 'generation', 1,
        'present', true, 'confirmed_tokens', 60, 'coverage', 'complete',
        'payload_hash', repeat('e', 64)
      )
    ),
    'cycle_settlements', jsonb_build_array(jsonb_build_object(
      'cycle_id', 'history-cycle', 'amount', 0,
      'settled_at_utc', v_reset_at
    )),
    'unverified_planet_wallet_claims', jsonb_build_array(jsonb_build_object(
      'previous_cycle_id', 'history-cycle', 'claimed_amount', 40,
      'created_at_utc', v_reset_at
    )),
    'reset_settlement_proofs', jsonb_build_array(jsonb_build_object(
      'request_id', 'reset-history-cycle',
      'previous_cycle_id', 'history-cycle',
      'new_cycle_id', p_current_cycle,
      'reset_at_utc', v_reset_at,
      'raw_wallet_claim', jsonb_build_object(
        'previous_cycle_id', 'history-cycle', 'claimed_amount', 40,
        'created_at_utc', v_reset_at
      ),
      'settled_bonus_tokens', 0,
      'final_effect_revision', 1,
      'final_effects', v_final_effects,
      'final_active_instance_ids', jsonb_build_array(v_instance_id),
      'old_cycle_started_at_utc', '2026-10-01T00:00:00+00:00',
      'new_cycle_started_at_utc', v_reset_at,
      'reset_available_at_utc', v_reset_available
    ))
  );
  v_data := jsonb_set(v_data, '{natural_objects,0,cycle_id}', '"history-cycle"'::jsonb, true);
  v_data := jsonb_set(v_data, '{natural_removals,0,cycle_id}', '"history-cycle"'::jsonb, true);
  v_data := jsonb_set(v_data, '{removal_proofs,0,target,cycle_id}', '"history-cycle"'::jsonb, true);
  v_data := jsonb_set(v_data, '{removal_proofs,0,quote,target,key,cycle_id}', '"history-cycle"'::jsonb, true);
  v_data := jsonb_set(v_data, '{landscape_instances,0,acquired_at_utc}',
    '"2026-09-30T23:59:59+00:00"'::jsonb, true);
  return jsonb_set(v_request, '{snapshot,data}', v_data, true);
end;
$$;

create function pg_temp.shop_guest_import_single_reset_floor_once_request(
  p_import_id uuid,
  p_user_id uuid,
  p_current_cycle text
)
returns jsonb
language plpgsql
immutable
as $$
declare
  v_request jsonb := pg_temp.shop_guest_import_single_reset_request(
    p_import_id, p_user_id, p_current_cycle
  );
  v_data jsonb := v_request#>'{snapshot,data}';
  v_final_effects jsonb := v_data#>'{effect_history,0,effects}';
  v_instance_ids jsonb := v_data#>'{effect_history,0,active_instance_ids}';
  v_current_effects jsonb := v_data#>'{effect_history,1,effects}';
begin
  v_data := v_data || jsonb_build_object(
    'effect_timeline_state', jsonb_set(v_data->'effect_timeline_state', '{effect_revision}', '3'::jsonb, true),
    'effect_history', jsonb_build_array(
      jsonb_build_object(
        'cycle_id', 'history-cycle', 'revision', 1,
        'started_at_utc', '2026-10-01T00:00:03+00:00',
        'ended_at_utc', '2026-10-01T06:00:00+00:00',
        'active_instance_ids', v_instance_ids, 'effects', v_final_effects
      ),
      jsonb_build_object(
        'cycle_id', 'history-cycle', 'revision', 2,
        'started_at_utc', '2026-10-01T06:00:00+00:00',
        'ended_at_utc', '2026-10-01T12:00:00+00:00',
        'active_instance_ids', v_instance_ids, 'effects', v_final_effects
      ),
      jsonb_build_object(
        'cycle_id', p_current_cycle, 'revision', 3,
        'started_at_utc', '2026-10-01T12:00:00+00:00', 'ended_at_utc', null,
        'active_instance_ids', '[]'::jsonb, 'effects', v_current_effects
      )
    ),
    'effect_contributions', jsonb_build_array(
      jsonb_build_object(
        'device_id', '30000000-0000-4000-a000-000000000001',
        'cycle_id', 'history-cycle', 'date', '2026-10-01', 'effect_revision', 1,
        'canonical_version', 1, 'tokens', 60, 'growth_bps', 0, 'wallet_bps', 100
      ),
      jsonb_build_object(
        'device_id', '30000000-0000-4000-a000-000000000001',
        'cycle_id', 'history-cycle', 'date', '2026-10-01', 'effect_revision', 2,
        'canonical_version', 1, 'tokens', 40, 'growth_bps', 0, 'wallet_bps', 100
      ),
      jsonb_build_object(
        'device_id', '30000000-0000-4000-a000-000000000001',
        'cycle_id', p_current_cycle, 'date', '2026-10-01', 'effect_revision', 3,
        'canonical_version', 1, 'tokens', 60, 'growth_bps', 0, 'wallet_bps', 0
      )
    ),
    'activity_days', jsonb_build_array(jsonb_build_object(
      'reward_date', '2026-10-01', 'cycle_id', 'history-cycle',
      'first_occurred_at_utc', '2026-10-01T00:00:04+00:00',
      'canonical_version', 1, 'tokens', 160
    )),
    'usage_aggregates', jsonb_build_array(
      jsonb_build_object(
        'cycle_id', 'history-cycle', 'bucket_date', '2026-10-01', 'agent', 'codex',
        'event_count', 1, 'total_tokens', 100, 'coverage', 'complete'
      ),
      jsonb_build_object(
        'cycle_id', p_current_cycle, 'bucket_date', '2026-10-01', 'agent', 'claude_code',
        'event_count', 1, 'total_tokens', 60, 'coverage', 'complete'
      )
    ),
    'daily_agent_totals', jsonb_build_array(
      jsonb_build_object('bucket_date', '2026-10-01', 'agent', 'codex', 'total_tokens', 100, 'coverage', 'complete'),
      jsonb_build_object('bucket_date', '2026-10-01', 'agent', 'claude_code', 'total_tokens', 60, 'coverage', 'complete')
    ),
    'lifetime_usage_tokens', 160,
    'cycle_usage_totals', jsonb_build_array(
      jsonb_build_object('cycle_id', 'history-cycle', 'total_tokens', 100),
      jsonb_build_object('cycle_id', p_current_cycle, 'total_tokens', 60)
    ),
    'growth_journal_entries', jsonb_build_array(
      jsonb_build_object(
        'device_id', '30000000-0000-4000-a000-000000000001',
        'cycle_id', 'history-cycle', 'bucket_date', '2026-10-01', 'agent', 'codex',
        'revision', 1, 'acknowledged_revision', 0, 'generation', 1,
        'present', true, 'confirmed_tokens', 100, 'coverage', 'complete', 'payload_hash', repeat('d', 64)
      ),
      jsonb_build_object(
        'device_id', '30000000-0000-4000-a000-000000000001',
        'cycle_id', p_current_cycle, 'bucket_date', '2026-10-01', 'agent', 'claude_code',
        'revision', 1, 'acknowledged_revision', 0, 'generation', 1,
        'present', true, 'confirmed_tokens', 60, 'coverage', 'complete', 'payload_hash', repeat('e', 64)
      )
    ),
    'cycle_settlements', jsonb_build_array(jsonb_build_object(
      'cycle_id', 'history-cycle', 'amount', 1, 'settled_at_utc', '2026-10-01T12:00:00+00:00'
    )),
    'unverified_planet_wallet_claims', jsonb_build_array(jsonb_build_object(
      'previous_cycle_id', 'history-cycle', 'claimed_amount', 100,
      'created_at_utc', '2026-10-01T12:00:00+00:00'
    ))
  );
  v_data := jsonb_set(v_data, '{historical_cycles,0,settled_bonus_tokens}', '1'::jsonb, true);
  v_data := jsonb_set(v_data, '{growth_journal_cycles,0,wallet_credit}', '100'::jsonb, true);
  v_data := jsonb_set(v_data, '{reset_settlement_proofs,0,raw_wallet_claim,claimed_amount}', '100'::jsonb, true);
  v_data := jsonb_set(v_data, '{reset_settlement_proofs,0,settled_bonus_tokens}', '1'::jsonb, true);
  v_data := jsonb_set(v_data, '{reset_settlement_proofs,0,final_effect_revision}', '2'::jsonb, true);
  return jsonb_set(v_request, '{snapshot,data}', v_data, true);
end;
$$;

create function pg_temp.shop_guest_import_single_reset_cycle_token_request(
  p_import_id uuid,
  p_user_id uuid,
  p_current_cycle text
)
returns jsonb
language plpgsql
immutable
as $$
declare
  v_request jsonb := pg_temp.shop_guest_import_single_reset_floor_once_request(
    p_import_id, p_user_id, p_current_cycle
  );
  v_data jsonb := v_request#>'{snapshot,data}';
  v_effects constant jsonb := jsonb_build_object(
    'token_earning_bps', 0,
    'civilization_growth_bps', 0,
    'shop_discount_bps', 0,
    'reset_cooldown_bps', 0,
    'natural_removal_discount_bps', 0,
    'era_reward_tokens', 0,
    'streak_reward_tokens', 0
  );
begin
  v_data := jsonb_set(v_data, '{game_rewards}', jsonb_build_array(jsonb_build_object(
    'reward_id', 'a1000000-0000-4000-a000-000000000001',
    'trigger_key', 'cycle-token:history-cycle',
    'kind', 'cycle_token',
    'cycle_id', 'history-cycle',
    'amount', 1,
    'effects', v_effects,
    'awarded_at_utc', '2026-10-01T12:00:00+00:00'
  )), true);
  v_data := jsonb_set(v_data, '{wallet_credits}', jsonb_build_array(jsonb_build_object(
    'credit_id', 'a2000000-0000-4000-a000-000000000001',
    'trigger_key', 'cycle-token:history-cycle',
    'cycle_id', 'history-cycle',
    'amount', 1,
    'created_at_utc', '2026-10-01T12:00:00+00:00'
  )), true);
  return jsonb_set(v_request, '{snapshot,data}', v_data, true);
end;
$$;

create function pg_temp.shop_guest_import_single_reset_overflow_request(
  p_import_id uuid,
  p_user_id uuid,
  p_current_cycle text
)
returns jsonb
language plpgsql
immutable
as $$
declare
  v_request jsonb := pg_temp.shop_guest_import_single_reset_floor_once_request(
    p_import_id, p_user_id, p_current_cycle
  );
  v_data jsonb := v_request#>'{snapshot,data}';
  v_max constant bigint := 9223372036854775807;
  v_bonus bigint := floor(9223372036854775807::numeric * 100 / 10000)::bigint;
begin
  v_data := v_data || jsonb_build_object(
    'effect_contributions', jsonb_build_array(
      jsonb_build_object(
        'device_id', '30000000-0000-4000-a000-000000000001',
        'cycle_id', 'history-cycle', 'date', '2026-10-01', 'effect_revision', 1,
        'canonical_version', 1, 'tokens', v_max, 'growth_bps', 0, 'wallet_bps', 100
      ),
      jsonb_build_object(
        'device_id', '30000000-0000-4000-a000-000000000001',
        'cycle_id', 'history-cycle', 'date', '2026-10-01', 'effect_revision', 2,
        'canonical_version', 1, 'tokens', 0, 'growth_bps', 0, 'wallet_bps', 100
      ),
      jsonb_build_object(
        'device_id', '30000000-0000-4000-a000-000000000001',
        'cycle_id', p_current_cycle, 'date', '2026-10-01', 'effect_revision', 3,
        'canonical_version', 1, 'tokens', 0, 'growth_bps', 0, 'wallet_bps', 0
      )
    ),
    'activity_days', jsonb_build_array(jsonb_build_object(
      'reward_date', '2026-10-01', 'cycle_id', 'history-cycle',
      'first_occurred_at_utc', '2026-10-01T00:00:04+00:00',
      'canonical_version', 1, 'tokens', v_max
    )),
    'usage_aggregates', jsonb_build_array(
      jsonb_build_object(
        'cycle_id', 'history-cycle', 'bucket_date', '2026-10-01', 'agent', 'codex',
        'event_count', 1, 'total_tokens', v_max, 'coverage', 'complete'
      ),
      jsonb_build_object(
        'cycle_id', p_current_cycle, 'bucket_date', '2026-10-01', 'agent', 'claude_code',
        'event_count', 1, 'total_tokens', 0, 'coverage', 'complete'
      )
    ),
    'daily_agent_totals', jsonb_build_array(
      jsonb_build_object('bucket_date', '2026-10-01', 'agent', 'codex', 'total_tokens', v_max, 'coverage', 'complete'),
      jsonb_build_object('bucket_date', '2026-10-01', 'agent', 'claude_code', 'total_tokens', 0, 'coverage', 'complete')
    ),
    'lifetime_usage_tokens', v_max,
    'current_cycle_usage_tokens', 0,
    'cycle_usage_totals', jsonb_build_array(
      jsonb_build_object('cycle_id', 'history-cycle', 'total_tokens', v_max),
      jsonb_build_object('cycle_id', p_current_cycle, 'total_tokens', 0)
    ),
    'growth_journal_entries', jsonb_build_array(
      jsonb_build_object(
        'device_id', '30000000-0000-4000-a000-000000000001',
        'cycle_id', 'history-cycle', 'bucket_date', '2026-10-01', 'agent', 'codex',
        'revision', 1, 'acknowledged_revision', 0, 'generation', 1,
        'present', true, 'confirmed_tokens', v_max, 'coverage', 'complete', 'payload_hash', repeat('d', 64)
      ),
      jsonb_build_object(
        'device_id', '30000000-0000-4000-a000-000000000001',
        'cycle_id', p_current_cycle, 'bucket_date', '2026-10-01', 'agent', 'claude_code',
        'revision', 1, 'acknowledged_revision', 0, 'generation', 1,
        'present', true, 'confirmed_tokens', 0, 'coverage', 'complete', 'payload_hash', repeat('e', 64)
      )
    ),
    'unverified_planet_wallet_claims', jsonb_build_array(jsonb_build_object(
      'previous_cycle_id', 'history-cycle', 'claimed_amount', v_max,
      'created_at_utc', '2026-10-01T12:00:00+00:00'
    )),
    'cycle_settlements', jsonb_build_array(jsonb_build_object(
      'cycle_id', 'history-cycle', 'amount', v_bonus,
      'settled_at_utc', '2026-10-01T12:00:00+00:00'
    ))
  );
  v_data := jsonb_set(v_data, '{growth_journal_cycles,0,wallet_credit}', to_jsonb(v_max), true);
  v_data := jsonb_set(v_data, '{historical_cycles,0,settled_bonus_tokens}', to_jsonb(v_bonus), true);
  v_data := jsonb_set(v_data, '{reset_settlement_proofs,0,raw_wallet_claim,claimed_amount}', to_jsonb(v_max), true);
  v_data := jsonb_set(v_data, '{reset_settlement_proofs,0,settled_bonus_tokens}', to_jsonb(v_bonus), true);
  return jsonb_set(v_request, '{snapshot,data}', v_data, true);
end;
$$;

create function pg_temp.shop_guest_import_growth_usage_request(
  p_import_id uuid,
  p_user_id uuid,
  p_cycle_id text
)
returns jsonb
language plpgsql
immutable
as $$
declare
  v_request jsonb := pg_temp.shop_guest_import_usage_request(
    p_import_id, p_user_id, p_cycle_id
  );
  v_data jsonb := v_request#>'{snapshot,data}';
begin
  v_data := jsonb_set(v_data, '{landscape_instances,0,sku}', '"land_crystal"'::jsonb);
  v_data := jsonb_set(v_data, '{landscape_instances,0,seed}', '"crystal-seed"'::jsonb);
  v_data := jsonb_set(v_data, '{purchases,0,sku}', '"land_crystal"'::jsonb);
  v_data := jsonb_set(v_data, '{purchase_proofs,0,quote,target,sku}', '"land_crystal"'::jsonb);
  v_data := jsonb_set(v_data, '{effect_history,0,effects,token_earning_bps}', '0'::jsonb);
  v_data := jsonb_set(v_data, '{effect_history,0,effects,civilization_growth_bps}', '100'::jsonb);
  v_data := jsonb_set(v_data, '{effect_contributions,1,growth_bps}', '100'::jsonb);
  v_data := jsonb_set(v_data, '{effect_contributions,1,wallet_bps}', '0'::jsonb);
  return jsonb_set(v_request, '{snapshot,data}', v_data, true);
end;
$$;

create function pg_temp.shop_guest_import_try_ownership_normalize(p_data jsonb)
returns jsonb
language plpgsql
as $$
declare
  v_result jsonb;
begin
  execute 'select private.shop_guest_import_ownership_normalize($1)'
    into v_result using p_data;
  return v_result;
exception when undefined_function then
  return '{"missing_normalizer":true}'::jsonb;
end;
$$;

create function pg_temp.shop_guest_import_try_usage_normalize(
  p_data jsonb,
  p_effect_timeline jsonb
)
returns jsonb
language plpgsql
as $$
declare
  v_result jsonb;
begin
  execute 'select private.shop_guest_import_usage_normalize($1, $2)'
    into v_result using p_data, p_effect_timeline;
  return v_result;
exception when undefined_function then
  return '{"missing_usage_normalizer":true}'::jsonb;
end;
$$;

create function pg_temp.shop_guest_import_try_reset_normalize(
  p_data jsonb,
  p_usage jsonb,
  p_effect_timeline jsonb
)
returns jsonb
language plpgsql
as $$
declare
  v_result jsonb;
begin
  execute 'select private.shop_guest_import_reset_normalize($1, $2, $3)'
    into v_result using p_data, p_usage, p_effect_timeline;
  return v_result;
exception when undefined_function then
  return '{"missing_reset_normalizer":true}'::jsonb;
end;
$$;

create function pg_temp.shop_guest_import_try_reset_source_normalize(p_data jsonb)
returns jsonb
language plpgsql
as $$
declare
  v_owned jsonb;
  v_timeline jsonb;
  v_usage jsonb;
begin
  v_owned := private.shop_guest_import_ownership_normalize(p_data);
  if v_owned is null then
    return null;
  end if;
  v_timeline := jsonb_build_object(
    'effect_cycle_bounds', v_owned->'effect_cycle_bounds',
    'effect_history', v_owned->'effect_history'
  );
  v_usage := private.shop_guest_import_usage_normalize(p_data, v_timeline);
  if v_usage is null then
    return null;
  end if;
  return pg_temp.shop_guest_import_try_reset_normalize(p_data, v_usage, v_timeline);
end;
$$;

create function pg_temp.shop_guest_import_try_usage_core_normalize(p_data jsonb)
returns jsonb
language plpgsql
as $$
declare
  v_result jsonb;
begin
  execute 'select private.shop_guest_import_usage_core_normalize($1)'
    into v_result using p_data;
  return v_result;
exception when undefined_function then
  return '{"missing_usage_core_normalizer":true}'::jsonb;
end;
$$;

create function pg_temp.shop_guest_import_try_usage_request(p_request jsonb)
returns jsonb
language plpgsql
as $$
declare
  v_data jsonb := p_request#>'{snapshot,data}';
  v_owned jsonb;
begin
  v_owned := private.shop_guest_import_ownership_normalize(v_data);
  if v_owned is null then
    return null;
  end if;
  return pg_temp.shop_guest_import_try_usage_normalize(v_data, jsonb_build_object(
    'effect_cycle_bounds', v_owned->'effect_cycle_bounds',
    'effect_history', v_owned->'effect_history'
  ));
end;
$$;


create function pg_temp.shop_guest_import_streak_request(
  p_import_id uuid,
  p_user_id uuid,
  p_cycle_id text
)
returns jsonb
language plpgsql
immutable
as $$
declare
  v_request jsonb := pg_temp.shop_guest_import_usage_request(
    p_import_id, p_user_id, p_cycle_id
  );
  v_data jsonb := v_request#>'{snapshot,data}';
  v_effects constant jsonb := jsonb_build_object(
    'token_earning_bps', 0,
    'civilization_growth_bps', 0,
    'shop_discount_bps', 0,
    'reset_cooldown_bps', 0,
    'natural_removal_discount_bps', 0,
    'era_reward_tokens', 0,
    'streak_reward_tokens', 10000
  );
begin
  v_data := jsonb_set(v_data, '{activation_at_utc}', '"2026-09-30T00:00:00+00:00"', true);
  v_data := jsonb_set(v_data, '{current_cycle,started_at_utc}', '"2026-09-30T00:00:00+00:00"', true);
  v_data := jsonb_set(v_data, '{landscape_instances,0,sku}', '"land_lantern"', true);
  v_data := jsonb_set(v_data, '{landscape_instances,0,seed}', '"lantern-seed"', true);
  v_data := jsonb_set(v_data, '{landscape_instances,0,acquired_at_utc}', '"2026-09-30T00:00:01+00:00"', true);
  v_data := jsonb_set(v_data, '{purchases,0,sku}', '"land_lantern"', true);
  v_data := jsonb_set(v_data, '{purchases,0,purchased_at_utc}', '"2026-09-30T00:00:01+00:00"', true);
  v_data := jsonb_set(v_data, '{purchase_proofs,0,quote,target,sku}', '"land_lantern"', true);
  v_data := jsonb_set(v_data, '{effect_cycle_bounds,0,started_at_utc}', '"2026-09-30T00:00:00+00:00"', true);
  v_data := jsonb_set(v_data, '{effect_timeline_state,server_time_utc}', '"2026-10-01T00:00:04+00:00"', true);
  v_data := jsonb_set(v_data, '{effect_history,0,started_at_utc}', '"2026-09-30T00:00:03+00:00"', true);
  v_data := jsonb_set(v_data, '{effect_history,0,effects}', v_effects, true);
  v_data := jsonb_set(v_data, '{growth_journal_cycles,0,started_at_utc}', '"2026-09-30T00:00:00+00:00"', true);
  v_data := v_data || jsonb_build_object(
    'effect_contributions', jsonb_build_array(
      jsonb_build_object(
        'device_id', '30000000-0000-4000-a000-000000000001',
        'cycle_id', p_cycle_id, 'date', '2026-09-30', 'effect_revision', 1,
        'canonical_version', 1, 'tokens', 40, 'growth_bps', 0, 'wallet_bps', 0
      ),
      jsonb_build_object(
        'device_id', '30000000-0000-4000-a000-000000000001',
        'cycle_id', p_cycle_id, 'date', '2026-10-01', 'effect_revision', 1,
        'canonical_version', 1, 'tokens', 60, 'growth_bps', 0, 'wallet_bps', 0
      )
    ),
    'activity_days', jsonb_build_array(
      jsonb_build_object(
        'reward_date', '2026-09-30', 'cycle_id', p_cycle_id,
        'first_occurred_at_utc', '2026-09-30T00:00:04+00:00',
        'canonical_version', 1, 'tokens', 40
      ),
      jsonb_build_object(
        'reward_date', '2026-10-01', 'cycle_id', p_cycle_id,
        'first_occurred_at_utc', '2026-10-01T00:00:01+00:00',
        'canonical_version', 1, 'tokens', 60
      )
    ),
    'usage_aggregates', jsonb_build_array(
      jsonb_build_object(
        'cycle_id', p_cycle_id, 'bucket_date', '2026-09-30', 'agent', 'codex',
        'event_count', 1, 'total_tokens', 40, 'coverage', 'complete'
      ),
      jsonb_build_object(
        'cycle_id', p_cycle_id, 'bucket_date', '2026-10-01', 'agent', 'codex',
        'event_count', 1, 'total_tokens', 60, 'coverage', 'complete'
      )
    ),
    'daily_agent_totals', jsonb_build_array(
      jsonb_build_object(
        'bucket_date', '2026-09-30', 'agent', 'codex',
        'total_tokens', 40, 'coverage', 'complete'
      ),
      jsonb_build_object(
        'bucket_date', '2026-10-01', 'agent', 'codex',
        'total_tokens', 60, 'coverage', 'complete'
      )
    ),
    'growth_journal_entries', jsonb_build_array(
      jsonb_build_object(
        'device_id', '30000000-0000-4000-a000-000000000001',
        'cycle_id', p_cycle_id, 'bucket_date', '2026-09-30', 'agent', 'codex',
        'revision', 1, 'acknowledged_revision', 0, 'generation', 1,
        'present', true, 'confirmed_tokens', 40, 'coverage', 'complete',
        'payload_hash', repeat('c', 64)
      ),
      jsonb_build_object(
        'device_id', '30000000-0000-4000-a000-000000000001',
        'cycle_id', p_cycle_id, 'bucket_date', '2026-10-01', 'agent', 'codex',
        'revision', 1, 'acknowledged_revision', 0, 'generation', 1,
        'present', true, 'confirmed_tokens', 60, 'coverage', 'complete',
        'payload_hash', repeat('d', 64)
      )
    ),
    'game_rewards', jsonb_build_array(jsonb_build_object(
      'reward_id', 'a3000000-0000-4000-a000-000000000001',
      'trigger_key', 'streak:2026-10-01',
      'kind', 'streak',
      'cycle_id', p_cycle_id,
      'amount', 10000,
      'effects', v_effects,
      'awarded_at_utc', '2026-10-01T00:00:04+00:00'
    )),
    'wallet_credits', jsonb_build_array(jsonb_build_object(
      'credit_id', 'a4000000-0000-4000-a000-000000000001',
      'trigger_key', 'streak:2026-10-01',
      'cycle_id', p_cycle_id,
      'amount', 10000,
      'created_at_utc', '2026-10-01T00:00:04+00:00'
    ))
  );
  return jsonb_set(v_request, '{snapshot,data}', v_data, true);
end;
$$;


create function pg_temp.shop_guest_import_single_reset_streak_request(
  p_import_id uuid,
  p_user_id uuid,
  p_current_cycle text
)
returns jsonb
language plpgsql
immutable
as $$
declare
  v_request jsonb := pg_temp.shop_guest_import_single_reset_cycle_token_request(
    p_import_id, p_user_id, p_current_cycle
  );
  v_data jsonb := v_request#>'{snapshot,data}';
  v_lantern_id constant text := '20000000-0000-4000-a000-000000000002';
  v_reward_effects constant jsonb := jsonb_build_object(
    'token_earning_bps', 100,
    'civilization_growth_bps', 0,
    'shop_discount_bps', 0,
    'reset_cooldown_bps', 0,
    'natural_removal_discount_bps', 0,
    'era_reward_tokens', 0,
    'streak_reward_tokens', 10000
  );
begin
  v_data := jsonb_set(v_data, '{activation_at_utc}', '"2026-09-30T00:00:00+00:00"'::jsonb, true);
  v_data := jsonb_set(v_data, '{historical_cycles,0,started_at_utc}', '"2026-09-30T00:00:00+00:00"'::jsonb, true);
  v_data := jsonb_set(v_data, '{effect_cycle_bounds,0,started_at_utc}', '"2026-09-30T00:00:00+00:00"'::jsonb, true);
  v_data := jsonb_set(v_data, '{growth_journal_cycles,0,started_at_utc}', '"2026-09-30T00:00:00+00:00"'::jsonb, true);
  v_data := jsonb_set(v_data, '{reset_settlement_proofs,0,old_cycle_started_at_utc}', '"2026-09-30T00:00:00+00:00"'::jsonb, true);
  v_data := jsonb_set(v_data, '{landscape_instances}', (v_data->'landscape_instances') || jsonb_build_array(
    jsonb_build_object(
      'instance_id', v_lantern_id, 'sku', 'land_lantern', 'variation_index', 0,
      'seed', 'streak-lantern-seed', 'variation_version', 1,
      'acquired_at_utc', '2026-09-30T23:59:59+00:00'
    )
  ), true);
  v_data := jsonb_set(v_data, '{landscape_edit_versions}', (v_data->'landscape_edit_versions') || jsonb_build_array(
    jsonb_build_object('instance_id', v_lantern_id, 'version', 0)
  ), true);
  v_data := jsonb_set(v_data, '{purchases}', (v_data->'purchases') || jsonb_build_array(
    jsonb_build_object(
      'purchase_id', 'purchase-streak-lantern', 'sku', 'land_lantern', 'price', 5000000,
      'purchased_at_utc', '2026-09-30T23:59:59+00:00'
    )
  ), true);
  v_data := jsonb_set(v_data, '{purchase_proofs}', (v_data->'purchase_proofs') || jsonb_build_array(
    jsonb_build_object(
      'request_id', 'purchase-streak-lantern',
      'quote', jsonb_build_object(
        'target', jsonb_build_object('kind', 'purchase', 'sku', 'land_lantern'),
        'catalog_revision', 1, 'effect_revision', 0, 'price', 5000000
      ),
      'status', 'purchased',
      'ownership', jsonb_build_object('kind', 'landscape', 'instance_id', v_lantern_id)
    )
  ), true);

  v_data := jsonb_set(v_data, '{effect_history,0,active_instance_ids}',
    (v_data#>'{effect_history,0,active_instance_ids}') || jsonb_build_array(v_lantern_id), true);
  v_data := jsonb_set(v_data, '{effect_history,0,effects}',
    (v_data#>'{effect_history,0,effects}') || jsonb_build_object('streak_reward_tokens', 10000), true);
  v_data := jsonb_set(v_data, '{effect_history,1,active_instance_ids}',
    (v_data#>'{effect_history,1,active_instance_ids}') || jsonb_build_array(v_lantern_id), true);
  v_data := jsonb_set(v_data, '{effect_history,1,effects}',
    (v_data#>'{effect_history,1,effects}') || jsonb_build_object('streak_reward_tokens', 10000), true);
  v_data := jsonb_set(v_data, '{reset_settlement_proofs,0,final_active_instance_ids}',
    (v_data#>'{reset_settlement_proofs,0,final_active_instance_ids}') || jsonb_build_array(v_lantern_id), true);
  v_data := jsonb_set(v_data, '{reset_settlement_proofs,0,final_effects}',
    (v_data#>'{reset_settlement_proofs,0,final_effects}') || jsonb_build_object('streak_reward_tokens', 10000), true);

  v_data := jsonb_set(v_data, '{activity_days}', jsonb_build_array(
    jsonb_build_object(
      'reward_date', '2026-09-30', 'cycle_id', 'history-cycle',
      'first_occurred_at_utc', '2026-09-30T23:59:59+00:00',
      'canonical_version', 1, 'tokens', 10
    ),
    jsonb_set(v_data#>'{activity_days,0}', '{first_occurred_at_utc}', '"2026-10-01T00:00:04+00:00"'::jsonb, true)
  ), true);
  v_data := jsonb_set(v_data, '{effect_contributions}', (v_data->'effect_contributions') || jsonb_build_array(
    jsonb_build_object(
      'device_id', '30000000-0000-4000-a000-000000000001',
      'cycle_id', 'history-cycle', 'date', '2026-09-30', 'effect_revision', 0,
      'canonical_version', 1, 'tokens', 10, 'growth_bps', 0, 'wallet_bps', 0
    )
  ), true);
  v_data := jsonb_set(v_data, '{usage_aggregates}', (v_data->'usage_aggregates') || jsonb_build_array(
    jsonb_build_object(
      'cycle_id', 'history-cycle', 'bucket_date', '2026-09-30', 'agent', 'codex',
      'event_count', 1, 'total_tokens', 10, 'coverage', 'complete'
    )
  ), true);
  v_data := jsonb_set(v_data, '{daily_agent_totals}', (v_data->'daily_agent_totals') || jsonb_build_array(
    jsonb_build_object(
      'bucket_date', '2026-09-30', 'agent', 'codex', 'total_tokens', 10, 'coverage', 'complete'
    )
  ), true);
  v_data := jsonb_set(v_data, '{growth_journal_entries}', (v_data->'growth_journal_entries') || jsonb_build_array(
    jsonb_build_object(
      'device_id', '30000000-0000-4000-a000-000000000001',
      'cycle_id', 'history-cycle', 'bucket_date', '2026-09-30', 'agent', 'codex',
      'revision', 1, 'acknowledged_revision', 0, 'generation', 1,
      'present', true, 'confirmed_tokens', 10, 'coverage', 'complete',
      'payload_hash', repeat('f', 64)
    )
  ), true);
  v_data := jsonb_set(v_data, '{lifetime_usage_tokens}', '170'::jsonb, true);
  v_data := jsonb_set(v_data, '{cycle_usage_totals,0,total_tokens}', '110'::jsonb, true);
  v_data := jsonb_set(v_data, '{growth_journal_cycles,0,wallet_credit}', '110'::jsonb, true);
  v_data := jsonb_set(v_data, '{unverified_planet_wallet_claims,0,claimed_amount}', '110'::jsonb, true);
  v_data := jsonb_set(v_data, '{reset_settlement_proofs,0,raw_wallet_claim,claimed_amount}', '110'::jsonb, true);
  v_data := jsonb_set(v_data, '{game_rewards}', (v_data->'game_rewards') || jsonb_build_array(
    jsonb_build_object(
      'reward_id', 'a3000000-0000-4000-a000-000000000001',
      'trigger_key', 'streak:2026-10-01', 'kind', 'streak', 'cycle_id', 'history-cycle',
      'amount', 10000, 'effects', v_reward_effects,
      'awarded_at_utc', '2026-10-01T12:00:00+00:00'
    )
  ), true);
  v_data := jsonb_set(v_data, '{wallet_credits}', (v_data->'wallet_credits') || jsonb_build_array(
    jsonb_build_object(
      'credit_id', 'a4000000-0000-4000-a000-000000000001',
      'trigger_key', 'streak:2026-10-01', 'cycle_id', 'history-cycle',
      'amount', 10000, 'created_at_utc', '2026-10-01T12:00:00+00:00'
    )
  ), true);
  return jsonb_set(v_request, '{snapshot,data}', v_data, true);
end;
$$;
