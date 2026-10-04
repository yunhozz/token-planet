begin;
create extension if not exists pgtap with schema extensions;
select no_plan();
\ir fixtures/shop_guest_import.inc

create function pg_temp.shop_guest_import_ledger_empty_request(
  p_import_id uuid,
  p_user_id uuid,
  p_cycle_id text
)
returns jsonb
language plpgsql
immutable
as $$
declare
  v_request jsonb := pg_temp.shop_guest_import_empty_request(
    p_import_id, p_user_id, p_cycle_id
  );
  v_data jsonb := v_request#>'{snapshot,data}';
  v_zero_effects jsonb := jsonb_build_object(
    'token_earning_bps', 0,
    'civilization_growth_bps', 0,
    'shop_discount_bps', 0,
    'reset_cooldown_bps', 0,
    'natural_removal_discount_bps', 0,
    'era_reward_tokens', 0,
    'streak_reward_tokens', 0
  );
begin
  v_data := v_data || jsonb_build_object(
    'effect_cycle_bounds_authoritative', true,
    'effect_cycle_bounds', jsonb_build_array(jsonb_build_object(
      'cycle_id', p_cycle_id,
      'started_at_utc', '2026-10-01T00:00:00+00:00',
      'ended_at_utc', null
    )),
    'effect_timeline_state', jsonb_build_object(
      'current_cycle_id', p_cycle_id,
      'effect_revision', 1,
      'server_time_utc', '2026-10-01T00:00:04+00:00',
      'reward_timezone', 'UTC'
    ),
    'effect_history', jsonb_build_array(jsonb_build_object(
      'cycle_id', p_cycle_id,
      'revision', 1,
      'started_at_utc', '2026-10-01T00:00:00+00:00',
      'ended_at_utc', null,
      'active_instance_ids', '[]'::jsonb,
      'effects', v_zero_effects
    )),
    'contribution_canonical_version', 1,
    'growth_journal_cycles', jsonb_build_array(jsonb_build_object(
      'cycle_id', p_cycle_id,
      'started_at_utc', '2026-10-01T00:00:00+00:00',
      'ended_at_utc', null,
      'wallet_credit', null,
      'wallet_credit_at_utc', null
    )),
    'lifetime_usage_tokens', 0,
    'current_cycle_usage_tokens', 0,
    'growth_journal_state', null
  );
  return jsonb_set(v_request, '{snapshot,data}', v_data, true);
end;
$$;

create function pg_temp.shop_guest_import_credit_only_reset_request(
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
  v_zero_effects jsonb := jsonb_build_object(
    'token_earning_bps', 0,
    'civilization_growth_bps', 0,
    'shop_discount_bps', 0,
    'reset_cooldown_bps', 0,
    'natural_removal_discount_bps', 0,
    'era_reward_tokens', 0,
    'streak_reward_tokens', 0
  );
  v_raw constant bigint := 1000000;
begin
  v_data := v_data || jsonb_build_object(
    'landscape_instances', '[]'::jsonb,
    'landscape_edit_versions', '[]'::jsonb,
    'placements', '[]'::jsonb,
    'purchases', '[]'::jsonb,
    'purchase_proofs', '[]'::jsonb,
    'natural_removals', '[]'::jsonb,
    'removal_debits', '[]'::jsonb,
    'removal_proofs', '[]'::jsonb,
    'effect_history', jsonb_build_array(
      jsonb_build_object(
        'cycle_id', 'history-cycle', 'revision', 1,
        'started_at_utc', '2026-10-01T00:00:03+00:00',
        'ended_at_utc', '2026-10-01T12:00:00+00:00',
        'active_instance_ids', '[]'::jsonb, 'effects', v_zero_effects
      ),
      jsonb_build_object(
        'cycle_id', p_current_cycle, 'revision', 2,
        'started_at_utc', '2026-10-01T12:00:00+00:00', 'ended_at_utc', null,
        'active_instance_ids', '[]'::jsonb, 'effects', v_zero_effects
      )
    ),
    'effect_contributions', jsonb_build_array(
      jsonb_build_object(
        'device_id', '30000000-0000-4000-a000-000000000001',
        'cycle_id', 'history-cycle', 'date', '2026-10-01', 'effect_revision', 0,
        'canonical_version', 1, 'tokens', v_raw, 'growth_bps', 0, 'wallet_bps', 0
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
      'canonical_version', 1, 'tokens', v_raw + 60
    )),
    'usage_aggregates', jsonb_build_array(
      jsonb_build_object(
        'cycle_id', 'history-cycle', 'bucket_date', '2026-10-01', 'agent', 'codex',
        'event_count', 1, 'total_tokens', v_raw, 'coverage', 'complete'
      ),
      jsonb_build_object(
        'cycle_id', p_current_cycle, 'bucket_date', '2026-10-01', 'agent', 'claude_code',
        'event_count', 1, 'total_tokens', 60, 'coverage', 'complete'
      )
    ),
    'daily_agent_totals', jsonb_build_array(
      jsonb_build_object(
        'bucket_date', '2026-10-01', 'agent', 'codex',
        'total_tokens', v_raw, 'coverage', 'complete'
      ),
      jsonb_build_object(
        'bucket_date', '2026-10-01', 'agent', 'claude_code',
        'total_tokens', 60, 'coverage', 'complete'
      )
    ),
    'lifetime_usage_tokens', v_raw + 60,
    'current_cycle_usage_tokens', 60,
    'cycle_usage_totals', jsonb_build_array(
      jsonb_build_object('cycle_id', 'history-cycle', 'total_tokens', v_raw),
      jsonb_build_object('cycle_id', p_current_cycle, 'total_tokens', 60)
    ),
    'growth_journal_entries', jsonb_build_array(
      jsonb_build_object(
        'device_id', '30000000-0000-4000-a000-000000000001',
        'cycle_id', 'history-cycle', 'bucket_date', '2026-10-01', 'agent', 'codex',
        'revision', 1, 'acknowledged_revision', 0, 'generation', 1,
        'present', true, 'confirmed_tokens', v_raw, 'coverage', 'complete',
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
    'growth_journal_cycles', jsonb_build_array(
      jsonb_build_object(
        'cycle_id', 'history-cycle',
        'started_at_utc', '2026-10-01T00:00:00+00:00',
        'ended_at_utc', '2026-10-01T12:00:00+00:00',
        'wallet_credit', v_raw, 'wallet_credit_at_utc', '2026-10-01T12:00:00+00:00'
      ),
      jsonb_build_object(
        'cycle_id', p_current_cycle,
        'started_at_utc', '2026-10-01T12:00:00+00:00',
        'ended_at_utc', null, 'wallet_credit', null, 'wallet_credit_at_utc', null
      )
    ),
    'unverified_planet_wallet_claims', jsonb_build_array(jsonb_build_object(
      'previous_cycle_id', 'history-cycle', 'claimed_amount', v_raw,
      'created_at_utc', '2026-10-01T12:00:00+00:00'
    )),
    'cycle_settlements', jsonb_build_array(jsonb_build_object(
      'cycle_id', 'history-cycle', 'amount', 0,
      'settled_at_utc', '2026-10-01T12:00:00+00:00'
    )),
    'game_rewards', '[]'::jsonb,
    'wallet_credits', '[]'::jsonb,
    'era_progress', '[]'::jsonb
  );
  v_data := jsonb_set(v_data, '{historical_cycles,0,settled_bonus_tokens}', '0'::jsonb, true);
  v_data := jsonb_set(v_data, '{growth_journal_cycles,0,wallet_credit}', to_jsonb(v_raw), true);
  v_data := jsonb_set(v_data, '{unverified_planet_wallet_claims,0,claimed_amount}', to_jsonb(v_raw), true);
  v_data := jsonb_set(v_data, '{reset_settlement_proofs,0,raw_wallet_claim,claimed_amount}', to_jsonb(v_raw), true);
  v_data := jsonb_set(v_data, '{reset_settlement_proofs,0,settled_bonus_tokens}', '0'::jsonb, true);
  v_data := jsonb_set(v_data, '{reset_settlement_proofs,0,final_effect_revision}', '1'::jsonb, true);
  v_data := jsonb_set(v_data, '{reset_settlement_proofs,0,final_effects}', v_zero_effects, true);
  v_data := jsonb_set(v_data, '{reset_settlement_proofs,0,final_active_instance_ids}', '[]'::jsonb, true);
  return jsonb_set(v_request, '{snapshot,data}', v_data, true);
end;
$$;

create function pg_temp.shop_guest_import_try_ledger_normalize(p_data jsonb)
returns jsonb
language plpgsql
as $$
declare
  v_result jsonb;
begin
  execute 'select private.shop_guest_import_ledger_normalize($1)'
    into v_result using p_data;
  return v_result;
exception when undefined_function then
  return '{"missing_ledger_normalizer":true}'::jsonb;
end;
$$;

create function pg_temp.shop_guest_import_try_ledger_prefix_normalize(p_events jsonb)
returns jsonb
language plpgsql
as $$
declare
  v_result jsonb;
begin
  execute 'select private.shop_guest_import_ledger_prefix_normalize($1)'
    into v_result using p_events;
  return v_result;
exception when undefined_function then
  return '{"missing_prefix_normalizer":true}'::jsonb;
end;
$$;

create function pg_temp.shop_guest_import_try_ledger_source_normalize(p_data jsonb)
returns jsonb
language plpgsql
as $$
declare
  v_owned jsonb;
  v_effect_timeline jsonb;
  v_usage jsonb;
  v_resets jsonb;
  v_rewards jsonb;
  v_ledger jsonb;
begin
  v_owned := private.shop_guest_import_ownership_normalize(p_data);
  if v_owned is null then
    return jsonb_build_object('stage', 'ownership_rejected');
  end if;
  v_effect_timeline := jsonb_build_object(
    'effect_cycle_bounds', v_owned->'effect_cycle_bounds',
    'effect_history', v_owned->'effect_history'
  );
  v_usage := private.shop_guest_import_usage_normalize(p_data, v_effect_timeline);
  if v_usage is null then
    return jsonb_build_object('stage', 'usage_rejected');
  end if;
  v_resets := private.shop_guest_import_reset_normalize(p_data, v_usage, v_effect_timeline);
  if v_resets is null then
    return jsonb_build_object('stage', 'reset_rejected');
  end if;
  v_rewards := private.shop_guest_import_rewards_normalize(
    p_data, v_usage, v_effect_timeline, v_resets
  );
  if v_rewards is null then
    return jsonb_build_object('stage', 'rewards_rejected');
  end if;
  v_ledger := pg_temp.shop_guest_import_try_ledger_normalize(p_data);
  return jsonb_build_object(
    'stage', 'ready',
    'ownership', v_owned,
    'effect_timeline', v_effect_timeline,
    'usage', v_usage,
    'resets', v_resets,
    'rewards', v_rewards,
    'ledger', v_ledger
  );
end;
$$;

with source as (
  select pg_temp.shop_guest_import_ledger_empty_request(
    'f1000000-0000-4000-a000-000000000081',
    'f0000000-0000-4000-a000-000000000001', 'ledger-empty-current'
  )#>'{snapshot,data}' as value
), normalized as (
  select value, pg_temp.shop_guest_import_try_ledger_source_normalize(value) as result
  from source
)
select ok(
  result->>'stage' = 'ready'
    and result#>>'{usage,validation_scope}' = 'usage_effect_activity_consistency'
    and result#>>'{resets,validation_scope}' = 'reset_settlement_consistency'
    and result#>>'{rewards,validation_scope}' = 'reward_wallet_streak_consistency'
    and result#>'{resets,reset_chain}' = '[]'::jsonb
    and result#>'{resets,cycle_token_credits}' = '[]'::jsonb
    and result#>'{rewards,cycle_token_mirrors}' = '[]'::jsonb
    and result#>'{rewards,streak_mirrors}' = '[]'::jsonb
    and jsonb_array_length(value->'purchases') = 0
    and jsonb_array_length(value->'removal_debits') = 0
    and jsonb_array_length(value->'historical_cycles') = 0
    and jsonb_array_length(value->'growth_journal_cycles') = 1
    and jsonb_array_length(value->'cycle_usage_totals') = 0
    and value->'lifetime_usage_tokens' = '0'::jsonb
    and value->'current_cycle_usage_tokens' = '0'::jsonb
    and value->'growth_journal_state' = 'null'::jsonb
    and value->'era_progress' = '[]'::jsonb,
  'empty no-era ledger passes the existing source validators with no credits or debits'
)
from normalized;

with source as (
  select pg_temp.shop_guest_import_ledger_empty_request(
    'f1000000-0000-4000-a000-000000000081',
    'f0000000-0000-4000-a000-000000000001', 'ledger-empty-current'
  )#>'{snapshot,data}' as value
), normalized as (
  select pg_temp.shop_guest_import_try_ledger_source_normalize(value) as result
  from source
)
select ok(
  result#>>'{ledger,validation_scope}' = 'no_era_credit_only_ledger_consistency'
    and result#>'{ledger,events}' = '[]'::jsonb
    and (result#>>'{ledger,final_balance}')::bigint = 0,
  'the ledger normalizer returns a verified empty ledger'
)
from normalized;

with source as (
  select pg_temp.shop_guest_import_credit_only_reset_request(
    'f1000000-0000-4000-a000-000000000082',
    'f0000000-0000-4000-a000-000000000001', 'ledger-current-after-credit'
  )#>'{snapshot,data}' as value
), normalized as (
  select value, pg_temp.shop_guest_import_try_ledger_source_normalize(value) as result
  from source
)
select ok(
  result->>'stage' = 'ready'
    and result#>>'{usage,validation_scope}' = 'usage_effect_activity_consistency'
    and result#>>'{resets,validation_scope}' = 'reset_settlement_consistency'
    and result#>>'{resets,reset_chain,0,previous_cycle_id}' = 'history-cycle'
    and (result#>>'{resets,cycle_token_credits,0,raw_tokens}')::bigint = 1000000
    and (result#>>'{resets,cycle_token_credits,0,bonus_tokens}')::bigint = 0
    and (result#>>'{resets,cycle_token_credits,0,total_tokens}')::bigint = 1000000
    and result#>>'{rewards,validation_scope}' = 'reward_wallet_streak_consistency'
    and result#>'{rewards,cycle_token_mirrors}' = '[]'::jsonb
    and result#>'{rewards,streak_mirrors}' = '[]'::jsonb
    and result#>'{ownership,purchases}' = '[]'::jsonb
    and result#>'{ownership,natural_removals}' = '[]'::jsonb
    and jsonb_array_length(value->'purchases') = 0
    and jsonb_array_length(value->'removal_debits') = 0
    and value->'game_rewards' = '[]'::jsonb
    and value->'wallet_credits' = '[]'::jsonb
    and value->'era_progress' = '[]'::jsonb,
  'first-reset credit-only source independently reconstructs raw 1,000,000 with no bonus, purchase, removal or era'
)
from normalized;

with source as (
  select pg_temp.shop_guest_import_credit_only_reset_request(
    'f1000000-0000-4000-a000-000000000082',
    'f0000000-0000-4000-a000-000000000001', 'ledger-current-after-credit'
  )#>'{snapshot,data}' as value
), normalized as (
  select pg_temp.shop_guest_import_try_ledger_source_normalize(value) as result
  from source
)
select ok(
  result#>>'{ledger,validation_scope}' = 'no_era_credit_only_ledger_consistency'
    and (result#>>'{ledger,final_balance}')::bigint = 1000000,
  'the ledger normalizer derives the exact first reset credit'
)
from normalized;

with source as (
  select pg_temp.shop_guest_import_single_reset_floor_once_request(
    'f1000000-0000-4000-a000-000000000083',
    'f0000000-0000-4000-a000-000000000001', 'ledger-current-after-bonus'
  )#>'{snapshot,data}' as value
), normalized as (
  select value, pg_temp.shop_guest_import_try_reset_source_normalize(value) as result
  from source
)
select ok(
  result->>'validation_scope' = 'reset_settlement_consistency'
    and result#>>'{cycle_token_credits,0,raw_tokens}' = '100'
    and result#>>'{cycle_token_credits,0,bonus_tokens}' = '1'
    and result#>>'{cycle_token_credits,0,total_tokens}' = '101',
  'accepted reset normalization derives raw 100 plus one floor-once bonus as 101'
)
from normalized;

with events as (
  select jsonb_build_array(
    jsonb_build_object(
      'side', 'credit', 'kind', 'streak_credit', 'source_id', 'reward-1',
      'cycle_id', 'current-cycle', 'trigger_key', 'streak:2026-10-01',
      'at_utc', '2026-10-01T12:01:00Z', 'amount', 50000
    ),
    jsonb_build_object(
      'side', 'debit', 'kind', 'purchase_debit', 'source_id', 'purchase-1',
      'cycle_id', 'current-cycle', 'sku', 'land_pond',
      'at_utc', '2026-10-01T12:01:00Z', 'amount', 100000
    ),
    jsonb_build_object(
      'side', 'credit', 'kind', 'reset_credit', 'source_id', 'reset-1',
      'cycle_id', 'history-cycle', 'at_utc', '2026-10-01T12:00:00Z', 'amount', 1000000
    )
  ) as value
), normalized as (
  select pg_temp.shop_guest_import_try_ledger_prefix_normalize(value) as result
  from events
)
select ok(
  result->>'ending_balance' = '950000'
    and (result#>>'{timestamp_groups,1,opening_balance}')::bigint = 1000000
    and (result#>>'{timestamp_groups,1,debits}')::bigint = 100000
    and (result#>>'{timestamp_groups,1,credits}')::bigint = 50000
    and (result#>>'{timestamp_groups,1,ending_balance}')::bigint = 950000,
  'synthetic same-instant debit-before-credit group yields the chronological 950000 ending balance'
)
from normalized;

with events as (
  select jsonb_build_array(
    jsonb_build_object(
      'side', 'credit', 'kind', 'reset_credit', 'source_id', 'reset-1',
      'cycle_id', 'history-cycle', 'at_utc', '2026-10-01T12:00:00Z', 'amount', 10
    ),
    jsonb_build_object(
      'side', 'debit', 'kind', 'purchase_debit', 'source_id', 'purchase-1',
      'cycle_id', 'current-cycle', 'sku', 'land_pond',
      'at_utc', '2026-10-01T12:01:00Z', 'amount', 12
    ),
    jsonb_build_object(
      'side', 'credit', 'kind', 'streak_credit', 'source_id', 'reward-1',
      'cycle_id', 'current-cycle', 'trigger_key', 'streak:2026-10-01',
      'at_utc', '2026-10-01T12:01:00Z', 'amount', 5
    )
  ) as value
), normalized as (
  select pg_temp.shop_guest_import_try_ledger_prefix_normalize(value) as result
  from events
)
select ok(result is null, 'same-instant debits are tested before credits and cannot borrow from them')
from normalized;

with events as (
  select jsonb_build_array(
    jsonb_build_object(
      'side', 'credit', 'kind', 'reset_credit', 'source_id', 'reset-1',
      'cycle_id', 'history-cycle', 'at_utc', '2026-10-01T12:00:00Z', 'amount', 9223372036854775808::numeric
    )
  ) as value
), normalized as (
  select pg_temp.shop_guest_import_try_ledger_prefix_normalize(value) as result
  from events
)
select ok(result is null, 'prefix normalization rejects an event amount above bigint range')
from normalized;

with source as (
  select pg_temp.shop_guest_import_single_reset_request(
    'f1000000-0000-4000-a000-000000000084',
    'f0000000-0000-4000-a000-000000000001', 'ledger-current-native-debits'
  )#>'{snapshot,data}' as value
), normalized as (
  select value, pg_temp.shop_guest_import_try_ledger_source_normalize(value) as result
  from source
)
select ok(
  result->>'stage' = 'ready'
    and jsonb_array_length(value->'purchases') = 1
    and jsonb_array_length(value->'removal_debits') = 1
    and result->'ledger' = 'null'::jsonb,
  'source-validated debit-bearing fixture remains held until purchase and removal ledger verification is complete'
)
from normalized;

with source as (
  select pg_temp.shop_guest_import_ledger_empty_request(
    'f1000000-0000-4000-a000-000000000085',
    'f0000000-0000-4000-a000-000000000001', 'ledger-era-held'
  )#>'{snapshot,data}' || jsonb_build_object(
    'era_progress', jsonb_build_array(jsonb_build_object('cycle_id', 'ledger-era-held', 'stage', 1))
  ) as value
), normalized as (
  select value, pg_temp.shop_guest_import_try_ledger_source_normalize(value) as result
  from source
)
select ok(
  result->>'stage' = 'rewards_rejected',
  'era marker is held by the accepted source verification pipeline'
)
from normalized;

with source as (
  select pg_temp.shop_guest_import_ledger_empty_request(
    'f1000000-0000-4000-a000-000000000086',
    'f0000000-0000-4000-a000-000000000001', 'ledger-legacy-held'
  )#>'{snapshot,data}' || jsonb_build_object('legacy_partial_import_pending', true) as value
), normalized as (
  select value, pg_temp.shop_guest_import_try_ledger_source_normalize(value) as result
  from source
)
select ok(
  result->>'stage' = 'reset_rejected',
  'legacy partial-import marker is held by the accepted source verification pipeline'
)
from normalized;

with cases(events) as (
  values
    (jsonb_build_array(jsonb_build_object(
      'side', 'credit', 'kind', 'reset_credit', 'source_id', 'bad-string',
      'cycle_id', 'cycle', 'at_utc', '2026-10-01T12:00:00Z', 'amount', '1'
    ))),
    (jsonb_build_array(jsonb_build_object(
      'side', 'credit', 'kind', 'reset_credit', 'source_id', 'bad-negative',
      'cycle_id', 'cycle', 'at_utc', '2026-10-01T12:00:00Z', 'amount', -1
    ))),
    (jsonb_build_array(jsonb_build_object(
      'side', 'credit', 'kind', 'reset_credit', 'source_id', 'bad-fraction',
      'cycle_id', 'cycle', 'at_utc', '2026-10-01T12:00:00Z', 'amount', 1.5
    ))),
    (jsonb_build_array(jsonb_build_object(
      'side', 'credit', 'kind', 'reset_credit', 'source_id', 'bad-null',
      'cycle_id', 'cycle', 'at_utc', '2026-10-01T12:00:00Z', 'amount', null
    ))),
    (jsonb_build_array(jsonb_build_object(
      'side', 'credit', 'kind', 'reset_credit', 'source_id', 'bad-missing',
      'cycle_id', 'cycle', 'at_utc', '2026-10-01T12:00:00Z'
    ))),
    (jsonb_build_array(jsonb_build_object(
      'side', 'credit', 'kind', 'purchase_debit', 'source_id', 'bad-pair',
      'cycle_id', 'cycle', 'at_utc', '2026-10-01T12:00:00Z', 'amount', 1, 'sku', 'land_pond'
    ))),
    (jsonb_build_array(jsonb_build_object(
      'side', 'other', 'kind', 'reset_credit', 'source_id', 'bad-side',
      'cycle_id', 'cycle', 'at_utc', '2026-10-01T12:00:00Z', 'amount', 1
    ))),
    (jsonb_build_array(jsonb_build_object(
      'side', 'credit', 'kind', 'streak_credit', 'source_id', 'bad-trigger',
      'cycle_id', 'cycle', 'at_utc', '2026-10-01T12:00:00Z', 'amount', 1, 'trigger_key', 7
    ))),
    (jsonb_build_array(jsonb_build_object(
      'side', 'debit', 'kind', 'purchase_debit', 'source_id', 'bad-sku',
      'cycle_id', 'cycle', 'at_utc', '2026-10-01T12:00:00Z', 'amount', 1, 'sku', 7
    ))),
    (jsonb_build_array(jsonb_build_object(
      'side', 'debit', 'kind', 'natural_removal_debit', 'source_id', 'bad-stage',
      'cycle_id', 'cycle', 'at_utc', '2026-10-01T12:00:00Z', 'amount', 1,
      'stage', 0.5, 'ordinal', 0
    ))),
    (jsonb_build_array(jsonb_build_object(
      'side', 'debit', 'kind', 'natural_removal_debit', 'source_id', 'bad-ordinal',
      'cycle_id', 'cycle', 'at_utc', '2026-10-01T12:00:00Z', 'amount', 1,
      'stage', 0, 'ordinal', '0'
    )))
), normalized as (
  select pg_temp.shop_guest_import_try_ledger_prefix_normalize(events) as result
  from cases
)
select ok(bool_and(result is null),
  'prefix rejects string, negative, fractional, null, missing and mismatched side/type rows')
from normalized;

with events as (
  select jsonb_build_array(
    jsonb_build_object(
      'side', 'credit', 'kind', 'reset_credit', 'source_id', 'shared-source',
      'cycle_id', 'old-cycle', 'at_utc', '2026-10-01T12:00:00Z', 'amount', 10
    ),
    jsonb_build_object(
      'side', 'debit', 'kind', 'purchase_debit', 'source_id', 'shared-source',
      'cycle_id', 'current-cycle', 'at_utc', '2026-10-01T12:01:00Z', 'amount', 1, 'sku', 'land_pond'
    )
  ) as value
), normalized as (
  select pg_temp.shop_guest_import_try_ledger_prefix_normalize(value) as result
  from events
)
select ok(result is null, 'prefix rejects a source ID reused across credit and debit event kinds')
from normalized;

with events as (
  select jsonb_build_array(
    jsonb_build_object(
      'side', 'credit', 'kind', 'reset_credit', 'source_id', 'large-credit-a',
      'cycle_id', 'cycle-a', 'at_utc', '2026-10-01T12:00:00Z', 'amount', 5000000000000000000::numeric
    ),
    jsonb_build_object(
      'side', 'credit', 'kind', 'reset_credit', 'source_id', 'large-credit-b',
      'cycle_id', 'cycle-b', 'at_utc', '2026-10-01T12:00:00Z', 'amount', 5000000000000000000::numeric
    )
  ) as value
), normalized as (
  select pg_temp.shop_guest_import_try_ledger_prefix_normalize(value) as result
  from events
)
select ok(result is null, 'prefix rejects cumulative credit totals above bigint range')
from normalized;

with events as (
  select jsonb_build_array(
    jsonb_build_object(
      'side', 'credit', 'kind', 'reset_credit', 'source_id', 'group-credit-a',
      'cycle_id', 'cycle-a', 'at_utc', '2026-10-01T12:00:00Z', 'amount', 5000000000000000000::numeric
    ),
    jsonb_build_object(
      'side', 'credit', 'kind', 'reset_credit', 'source_id', 'group-credit-b',
      'cycle_id', 'cycle-b', 'at_utc', '2026-10-01T12:00:00Z', 'amount', 5000000000000000000::numeric
    )
  ) as value
), normalized as (
  select pg_temp.shop_guest_import_try_ledger_prefix_normalize(value) as result
  from events
)
select ok(result is null, 'prefix rejects a same-timestamp credit group above bigint range')
from normalized;

with source as (
  select (
    (
      pg_temp.shop_guest_import_ledger_empty_request(
        'f1000000-0000-4000-a000-000000000091',
        'f0000000-0000-4000-a000-000000000001', 'ledger-empty-omitted-proofs'
      ) #> array['snapshot', 'data']::text[]
    ) - 'purchase_proofs'::text - 'removal_proofs'::text
  ) as value
), normalized as (
  select pg_temp.shop_guest_import_try_ledger_source_normalize(value) as result
  from source
)
select is(result->>'stage', 'ready', 'empty omitted-proof capture passes ownership, usage, reset and reward source checks')
from normalized;

with source as (
  select (
    (
      pg_temp.shop_guest_import_ledger_empty_request(
        'f1000000-0000-4000-a000-000000000091',
        'f0000000-0000-4000-a000-000000000001', 'ledger-empty-omitted-proofs'
      ) #> array['snapshot', 'data']::text[]
    ) - 'purchase_proofs'::text - 'removal_proofs'::text
  ) as value
), normalized as (
  select pg_temp.shop_guest_import_try_ledger_source_normalize(value) as result
  from source
)
select ok(
  result->>'stage' = 'ready'
    and result#>>'{ledger,validation_scope}' = 'no_era_credit_only_ledger_consistency'
    and (result#>>'{ledger,final_balance}')::bigint = 0,
  'empty proof arrays may be omitted from a valid empty capture'
)
from normalized;

with source as (
  select (
    (
      pg_temp.shop_guest_import_credit_only_reset_request(
        'f1000000-0000-4000-a000-000000000092',
        'f0000000-0000-4000-a000-000000000001', 'ledger-credit-omitted-proofs'
      ) #> array['snapshot', 'data']::text[]
    ) - 'purchase_proofs'::text - 'removal_proofs'::text
  ) as value
), normalized as (
  select pg_temp.shop_guest_import_try_ledger_source_normalize(value) as result
  from source
)
select is(result->>'stage', 'ready', 'raw-credit omitted-proof capture passes ownership, usage, reset and reward source checks')
from normalized;

with source as (
  select (
    (
      pg_temp.shop_guest_import_credit_only_reset_request(
        'f1000000-0000-4000-a000-000000000092',
        'f0000000-0000-4000-a000-000000000001', 'ledger-credit-omitted-proofs'
      ) #> array['snapshot', 'data']::text[]
    ) - 'purchase_proofs'::text - 'removal_proofs'::text
  ) as value
), normalized as (
  select pg_temp.shop_guest_import_try_ledger_source_normalize(value) as result
  from source
)
select ok(
  result->>'stage' = 'ready'
    and result#>>'{ledger,validation_scope}' = 'no_era_credit_only_ledger_consistency'
    and (result#>>'{ledger,final_balance}')::bigint = 1000000,
  'empty proof arrays may be omitted from a valid credit-only reset capture'
)
from normalized;

with source as (
  select jsonb_set(
    jsonb_set(
      pg_temp.shop_guest_import_ledger_empty_request(
        'f1000000-0000-4000-a000-000000000093',
        'f0000000-0000-4000-a000-000000000001', 'ledger-null-proofs'
      )#>'{snapshot,data}',
      '{purchase_proofs}', 'null'::jsonb, true
    ),
    '{removal_proofs}', 'null'::jsonb, true
  ) as value
), normalized as (
  select pg_temp.shop_guest_import_try_ledger_normalize(value) as result
  from source
)
select ok(result is null, 'explicit null proof fields remain held')
from normalized;

with source as (
  select jsonb_set(
    jsonb_set(
      pg_temp.shop_guest_import_ledger_empty_request(
        'f1000000-0000-4000-a000-000000000094',
        'f0000000-0000-4000-a000-000000000001', 'ledger-scalar-proofs'
      )#>'{snapshot,data}',
      '{purchase_proofs}', '"not-an-array"'::jsonb, true
    ),
    '{removal_proofs}', '7'::jsonb, true
  ) as value
), normalized as (
  select pg_temp.shop_guest_import_try_ledger_normalize(value) as result
  from source
)
select ok(result is null, 'scalar proof fields remain held')
from normalized;

select * from finish();
rollback;
