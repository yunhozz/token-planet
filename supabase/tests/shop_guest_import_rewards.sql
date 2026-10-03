begin;
create extension if not exists pgtap with schema extensions;
select no_plan();
\ir fixtures/shop_guest_import.inc

create function pg_temp.shop_guest_import_try_rewards_normalize(
  p_data jsonb,
  p_usage jsonb,
  p_effect_timeline jsonb,
  p_resets jsonb
)
returns jsonb
language plpgsql
as $$
declare
  v_result jsonb;
begin
  execute 'select private.shop_guest_import_rewards_normalize($1, $2, $3, $4)'
    into v_result using p_data, p_usage, p_effect_timeline, p_resets;
  return v_result;
exception when undefined_function then
  return '{"missing_rewards_normalizer":true}'::jsonb;
end;
$$;

create function pg_temp.shop_guest_import_try_rewards_source_normalize(p_data jsonb)
returns jsonb
language plpgsql
as $$
declare
  v_owned jsonb;
  v_effect_timeline jsonb;
  v_usage jsonb;
  v_resets jsonb;
  v_rewards jsonb;
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
  v_rewards := pg_temp.shop_guest_import_try_rewards_normalize(
    p_data, v_usage, v_effect_timeline, v_resets
  );
  return jsonb_build_object(
    'stage', 'ready',
    'usage', v_usage,
    'resets', v_resets,
    'rewards', v_rewards
  );
end;
$$;

with source as (
  select pg_temp.shop_guest_import_usage_request(
    'f1000000-0000-4000-a000-000000000062',
    'f0000000-0000-4000-a000-000000000001', 'no-reward-cycle'
  )#>'{snapshot,data}' as value
), owned as (
  select value, private.shop_guest_import_ownership_normalize(value) as normalized
  from source
), timeline as (
  select value, jsonb_build_object(
    'effect_cycle_bounds', normalized->'effect_cycle_bounds',
    'effect_history', normalized->'effect_history'
  ) as normalized_timeline
  from owned
), usage as (
  select value, normalized_timeline,
    private.shop_guest_import_usage_normalize(value, normalized_timeline) as normalized_usage
  from timeline
), resets as (
  select value, normalized_usage, normalized_timeline,
    private.shop_guest_import_reset_normalize(value, normalized_usage, normalized_timeline)
      as normalized_resets
  from usage
), rewards as (
  select value, normalized_usage, normalized_timeline, normalized_resets,
    pg_temp.shop_guest_import_try_rewards_normalize(
      value, normalized_usage, normalized_timeline, normalized_resets
    ) as normalized_rewards
  from resets
)
select ok(
  normalized_usage is not null
    and normalized_resets->>'validation_scope' = 'reset_settlement_consistency'
    and value->'game_rewards' = '[]'::jsonb
    and value->'wallet_credits' = '[]'::jsonb,
  'current-only no-reward/no-wallet source passes ownership, usage and reset normalization'
)
from rewards;

with source as (
  select pg_temp.shop_guest_import_usage_request(
    'f1000000-0000-4000-a000-000000000062',
    'f0000000-0000-4000-a000-000000000001', 'no-reward-cycle'
  )#>'{snapshot,data}' as value
), owned as (
  select value, private.shop_guest_import_ownership_normalize(value) as normalized
  from source
), timeline as (
  select value, jsonb_build_object(
    'effect_cycle_bounds', normalized->'effect_cycle_bounds',
    'effect_history', normalized->'effect_history'
  ) as normalized_timeline
  from owned
), usage as (
  select value, normalized_timeline,
    private.shop_guest_import_usage_normalize(value, normalized_timeline) as normalized_usage
  from timeline
), resets as (
  select value, normalized_usage, normalized_timeline,
    private.shop_guest_import_reset_normalize(value, normalized_usage, normalized_timeline)
      as normalized_resets
  from usage
), rewards as (
  select pg_temp.shop_guest_import_try_rewards_normalize(
      value, normalized_usage, normalized_timeline, normalized_resets
    ) as normalized_rewards
  from resets
)
select ok(
  normalized_rewards->>'validation_scope' = 'reward_wallet_streak_consistency'
    and normalized_rewards->'cycle_token_mirrors' = '[]'::jsonb
    and normalized_rewards->'streak_mirrors' = '[]'::jsonb
    and not (normalized_rewards ? 'server_game_rewards'),
  'no-reward source yields an empty mirror set without separate server rewards'
)
from rewards;

with source as (
  select pg_temp.shop_guest_import_single_reset_cycle_token_request(
    'f1000000-0000-4000-a000-000000000063',
    'f0000000-0000-4000-a000-000000000001', 'current-after-reset'
  )#>'{snapshot,data}' as value
), normalized as (
  select value, pg_temp.shop_guest_import_try_rewards_source_normalize(value) as result
  from source
)
select ok(
  result->>'stage' = 'ready'
    and result#>>'{usage,validation_scope}' = 'usage_effect_activity_consistency'
    and result#>>'{resets,validation_scope}' = 'reset_settlement_consistency'
    and (result#>>'{resets,cycle_token_credits,0,raw_tokens}')::bigint = 100
    and (result#>>'{resets,cycle_token_credits,0,bonus_tokens}')::bigint = 1
    and (result#>>'{resets,cycle_token_credits,0,total_tokens}')::bigint = 101
    and jsonb_array_length(value->'game_rewards') = 1
    and jsonb_array_length(value->'wallet_credits') = 1
    and value#>>'{game_rewards,0,trigger_key}' = value#>>'{wallet_credits,0,trigger_key}'
    and value#>>'{game_rewards,0,cycle_id}' = value#>>'{wallet_credits,0,cycle_id}'
    and value#>'{game_rewards,0,amount}' = value#>'{wallet_credits,0,amount}'
    and value#>>'{game_rewards,0,awarded_at_utc}' = value#>>'{wallet_credits,0,created_at_utc}',
  'single-reset positive cycle-token fixture passes all upstream normalizers and mirrors'
)
from normalized;

with source as (
  select pg_temp.shop_guest_import_single_reset_cycle_token_request(
    'f1000000-0000-4000-a000-000000000063',
    'f0000000-0000-4000-a000-000000000001', 'current-after-reset'
  )#>'{snapshot,data}' as value
), normalized as (
  select pg_temp.shop_guest_import_try_rewards_source_normalize(value) as result
  from source
)
select ok(
  result#>>'{rewards,validation_scope}' = 'reward_wallet_streak_consistency'
    and jsonb_array_length(result#>'{rewards,cycle_token_mirrors}') = 1
    and result#>>'{rewards,cycle_token_mirrors,0,reward_id}' = 'a1000000-0000-4000-a000-000000000001'
    and result#>>'{rewards,cycle_token_mirrors,0,credit_id}' = 'a2000000-0000-4000-a000-000000000001'
    and result#>>'{rewards,cycle_token_mirrors,0,trigger_key}' = 'cycle-token:history-cycle'
    and result#>>'{rewards,cycle_token_mirrors,0,cycle_id}' = 'history-cycle'
    and (result#>>'{rewards,cycle_token_mirrors,0,amount}')::bigint = 1
    and result#>>'{rewards,cycle_token_mirrors,0,awarded_at_utc}' = '2026-10-01T12:00:00+00:00'
    and result#>>'{rewards,cycle_token_mirrors,0,created_at_utc}' = '2026-10-01T12:00:00+00:00'
    and result#>'{rewards,streak_mirrors}' = '[]'::jsonb
    and not (result->'rewards' ? 'server_game_rewards'),
  'positive reset bonus returns exactly its reward-wallet mirror and no separate server reward'
)
from normalized;

with source as (
  select pg_temp.shop_guest_import_single_reset_request(
    'f1000000-0000-4000-a000-000000000064',
    'f0000000-0000-4000-a000-000000000001', 'current-after-zero-reset'
  )#>'{snapshot,data}' as value
), normalized as (
  select value, pg_temp.shop_guest_import_try_rewards_source_normalize(value) as result
  from source
)
select ok(
  result->>'stage' = 'ready'
    and (result#>>'{resets,cycle_token_credits,0,bonus_tokens}')::bigint = 0
    and value->'game_rewards' = '[]'::jsonb
    and value->'wallet_credits' = '[]'::jsonb
    and result#>>'{rewards,validation_scope}' = 'reward_wallet_streak_consistency'
    and result#>'{rewards,cycle_token_mirrors}' = '[]'::jsonb
    and result#>'{rewards,streak_mirrors}' = '[]'::jsonb,
  'zero reset bonus with no serialized reward or wallet row yields no mirror'
)
from normalized;


create function pg_temp.shop_guest_import_reward_variant(p_data jsonb, p_variant text)
returns jsonb
language plpgsql
immutable
as $$
declare
  v_data jsonb := p_data;
begin
  case p_variant
    when 'missing_reward_array' then v_data := v_data - 'game_rewards';
    when 'missing_wallet_array' then v_data := v_data - 'wallet_credits';
    when 'missing_reward_row' then v_data := jsonb_set(v_data, '{game_rewards}', '[]'::jsonb, true);
    when 'missing_wallet_row' then v_data := jsonb_set(v_data, '{wallet_credits}', '[]'::jsonb, true);
    when 'reward_amount_mismatch' then v_data := jsonb_set(v_data, '{game_rewards,0,amount}', '2'::jsonb, true);
    when 'wallet_amount_mismatch' then v_data := jsonb_set(v_data, '{wallet_credits,0,amount}', '2'::jsonb, true);
    when 'reward_amount_string' then v_data := jsonb_set(v_data, '{game_rewards,0,amount}', '"1"'::jsonb, true);
    when 'reward_id_number' then v_data := jsonb_set(v_data, '{game_rewards,0,reward_id}', '1'::jsonb, true);
    when 'wallet_id_invalid' then v_data := jsonb_set(v_data, '{wallet_credits,0,credit_id}', '"not-a-uuid"'::jsonb, true);
    when 'reward_trigger_mismatch' then v_data := jsonb_set(v_data, '{game_rewards,0,trigger_key}', '"cycle-token:other"'::jsonb, true);
    when 'wallet_trigger_mismatch' then v_data := jsonb_set(v_data, '{wallet_credits,0,trigger_key}', '"cycle-token:other"'::jsonb, true);
    when 'reward_cycle_mismatch' then v_data := jsonb_set(v_data, '{game_rewards,0,cycle_id}', '"other-cycle"'::jsonb, true);
    when 'wallet_cycle_mismatch' then v_data := jsonb_set(v_data, '{wallet_credits,0,cycle_id}', '"other-cycle"'::jsonb, true);
    when 'reward_time_mismatch' then v_data := jsonb_set(v_data, '{game_rewards,0,awarded_at_utc}', '"2026-10-01T12:00:01+00:00"'::jsonb, true);
    when 'wallet_time_mismatch' then v_data := jsonb_set(v_data, '{wallet_credits,0,created_at_utc}', '"2026-10-01T12:00:01+00:00"'::jsonb, true);
    when 'duplicate_reward' then v_data := jsonb_set(v_data, '{game_rewards}', (v_data->'game_rewards') || (v_data->'game_rewards'), true);
    when 'duplicate_wallet' then v_data := jsonb_set(v_data, '{wallet_credits}', (v_data->'wallet_credits') || (v_data->'wallet_credits'), true);
    when 'streak_kind' then v_data := jsonb_set(v_data, '{game_rewards,0,kind}', '"streak"'::jsonb, true);
    when 'effect_value_string' then v_data := jsonb_set(v_data, '{game_rewards,0,effects,token_earning_bps}', '"0"'::jsonb, true);
    when 'effect_key_missing' then v_data := jsonb_set(v_data, '{game_rewards,0,effects}', (v_data#>'{game_rewards,0,effects}') - 'streak_reward_tokens', true);
    when 'effect_key_extra' then v_data := jsonb_set(v_data, '{game_rewards,0,effects}', (v_data#>'{game_rewards,0,effects}') || '{"extra":0}'::jsonb, true);
    when 'era_progress_present' then v_data := jsonb_set(v_data, '{era_progress}', '[{}]'::jsonb, true);
    else return null;
  end case;
  return v_data;
end;
$$;

with source as materialized (
  select pg_temp.shop_guest_import_single_reset_cycle_token_request(
    'f1000000-0000-4000-a000-000000000063',
    'f0000000-0000-4000-a000-000000000001', 'current-after-reset'
  )#>'{snapshot,data}' as value
), owned as materialized (
  select value, private.shop_guest_import_ownership_normalize(value) as normalized
  from source
), timeline as materialized (
  select value, jsonb_build_object(
    'effect_cycle_bounds', normalized->'effect_cycle_bounds',
    'effect_history', normalized->'effect_history'
  ) as normalized_timeline
  from owned
), usage as materialized (
  select value, normalized_timeline,
    private.shop_guest_import_usage_normalize(value, normalized_timeline) as normalized_usage
  from timeline
), resets as materialized (
  select value, normalized_timeline, normalized_usage,
    private.shop_guest_import_reset_normalize(value, normalized_usage, normalized_timeline)
      as normalized_resets
  from usage
), variants as (
  select unnest(array[
    'missing_reward_array', 'missing_wallet_array',
    'missing_reward_row', 'missing_wallet_row',
    'reward_amount_mismatch', 'wallet_amount_mismatch', 'reward_amount_string',
    'reward_id_number', 'wallet_id_invalid',
    'reward_trigger_mismatch', 'wallet_trigger_mismatch',
    'reward_cycle_mismatch', 'wallet_cycle_mismatch',
    'reward_time_mismatch', 'wallet_time_mismatch',
    'duplicate_reward', 'duplicate_wallet', 'streak_kind',
    'effect_value_string', 'effect_key_missing', 'effect_key_extra',
    'era_progress_present'
  ]::text[]) as variant
), evaluated as (
  select variants.variant,
    private.shop_guest_import_rewards_normalize(
      pg_temp.shop_guest_import_reward_variant(resets.value, variants.variant),
      resets.normalized_usage,
      resets.normalized_timeline,
      resets.normalized_resets
    ) as normalized_rewards
  from resets cross join variants
)
select ok(normalized_rewards is null, variant || ' mismatch is held')
from evaluated
order by variant;

with source as (
  select pg_temp.shop_guest_import_single_reset_request(
    'f1000000-0000-4000-a000-000000000064',
    'f0000000-0000-4000-a000-000000000001', 'current-after-zero-reset'
  )#>'{snapshot,data}' as value
), mutated as (
  select jsonb_set(value, '{game_rewards}', jsonb_build_array(jsonb_build_object(
    'reward_id', 'a1000000-0000-4000-a000-000000000002',
    'trigger_key', 'cycle-token:history-cycle',
    'kind', 'cycle_token',
    'cycle_id', 'history-cycle',
    'amount', 0,
    'effects', jsonb_build_object(
      'token_earning_bps', 0,
      'civilization_growth_bps', 0,
      'shop_discount_bps', 0,
      'reset_cooldown_bps', 0,
      'natural_removal_discount_bps', 0,
      'era_reward_tokens', 0,
      'streak_reward_tokens', 0
    ),
    'awarded_at_utc', '2026-10-01T12:00:00+00:00'
  )), true) as value
  from source
), normalized as (
  select value, pg_temp.shop_guest_import_try_rewards_source_normalize(value) as result
  from mutated
)
select ok(
  result->>'stage' = 'ready'
    and result ? 'rewards'
    and result->'rewards' = 'null'::jsonb,
  'an explicit zero-amount cycle-token reward on a zero-bonus reset is held'
)
from normalized;


with source as materialized (
  select pg_temp.shop_guest_import_streak_request(
    'f1000000-0000-4000-a000-000000000071',
    'f0000000-0000-4000-a000-000000000001', 'streak-current'
  )#>'{snapshot,data}' as value
), owned as materialized (
  select value, private.shop_guest_import_ownership_normalize(value) as normalized
  from source
), timeline as materialized (
  select value, jsonb_build_object(
    'effect_cycle_bounds', normalized->'effect_cycle_bounds',
    'effect_history', normalized->'effect_history'
  ) as normalized_timeline
  from owned
), usage as materialized (
  select value, normalized_timeline,
    private.shop_guest_import_usage_normalize(value, normalized_timeline) as normalized_usage
  from timeline
), resets as materialized (
  select value, normalized_timeline, normalized_usage,
    private.shop_guest_import_reset_normalize(value, normalized_usage, normalized_timeline)
      as normalized_resets
  from usage
), normalized as (
  select *, pg_temp.shop_guest_import_try_rewards_source_normalize(value) as result
  from resets
)
select ok(
  normalized is not null
    and result->>'stage' = 'ready'
    and normalized_usage->>'validation_scope' = 'usage_effect_activity_consistency'
    and normalized_resets->>'validation_scope' = 'reset_settlement_consistency'
    and result#>>'{usage,daily_growth,0,tokens}' is not null
    and value#>>'{effect_history,0,effects,streak_reward_tokens}' = '10000'
    and value#>>'{game_rewards,0,trigger_key}' = 'streak:2026-10-01'
    and value#>>'{game_rewards,0,kind}' = 'streak',
  'catalog-backed streak fixture passes ownership, effect, usage and reset controls'
)
from normalized;

with source as materialized (
  select pg_temp.shop_guest_import_streak_request(
    'f1000000-0000-4000-a000-000000000071',
    'f0000000-0000-4000-a000-000000000001', 'streak-current'
  )#>'{snapshot,data}' as value
), normalized as (
  select pg_temp.shop_guest_import_try_rewards_source_normalize(value) as result
  from source
)
select ok(
  result#>>'{rewards,validation_scope}' = 'reward_wallet_streak_consistency'
    and jsonb_array_length(result#>'{rewards,streak_mirrors}') = 1
    and result#>>'{rewards,streak_mirrors,0,reward_id}' = 'a3000000-0000-4000-a000-000000000001'
    and result#>>'{rewards,streak_mirrors,0,credit_id}' = 'a4000000-0000-4000-a000-000000000001'
    and result#>>'{rewards,streak_mirrors,0,trigger_key}' = 'streak:2026-10-01'
    and result#>>'{rewards,streak_mirrors,0,cycle_id}' = 'streak-current'
    and (result#>>'{rewards,streak_mirrors,0,amount}')::bigint = 10000,
  'streak reward and wallet pair normalizes to the first-day effect amount'
)
from normalized;

create function pg_temp.shop_guest_import_streak_variant(p_data jsonb, p_variant text)
returns jsonb
language plpgsql
immutable
as $$
declare
  v_data jsonb := p_data;
begin
  case p_variant
    when 'missing_wallet_row' then
      v_data := jsonb_set(v_data, '{wallet_credits}', '[]'::jsonb, true);
    when 'reward_trigger_mismatch' then
      v_data := jsonb_set(v_data, '{game_rewards,0,trigger_key}', '"streak:2026-10-02"'::jsonb, true);
    when 'reward_cycle_mismatch' then
      v_data := jsonb_set(v_data, '{game_rewards,0,cycle_id}', '"other-cycle"'::jsonb, true);
    when 'reward_amount_mismatch' then
      v_data := jsonb_set(v_data, '{game_rewards,0,amount}', '10001'::jsonb, true);
    when 'reward_effect_mismatch' then
      v_data := jsonb_set(v_data, '{game_rewards,0,effects,streak_reward_tokens}', '9999'::jsonb, true);
    when 'reward_time_mismatch' then
      v_data := jsonb_set(v_data, '{game_rewards,0,awarded_at_utc}', '"2026-10-01T00:00:05+00:00"'::jsonb, true);
      v_data := jsonb_set(v_data, '{effect_timeline_state,server_time_utc}', '"2026-10-01T00:00:06+00:00"'::jsonb, true);
    when 'duplicate_streak_trigger' then
      v_data := jsonb_set(v_data, '{game_rewards}', (v_data->'game_rewards') || jsonb_build_array(
        (v_data#>'{game_rewards,1}') || '{"reward_id":"a3000000-0000-4000-a000-000000000002"}'::jsonb
      ), true);
      v_data := jsonb_set(v_data, '{wallet_credits}', (v_data->'wallet_credits') || jsonb_build_array(
        (v_data#>'{wallet_credits,1}') || '{"credit_id":"a4000000-0000-4000-a000-000000000002"}'::jsonb
      ), true);
    when 'streak_award_before_first' then
      v_data := jsonb_set(v_data, '{game_rewards,1,awarded_at_utc}', '"2026-10-01T00:00:03+00:00"'::jsonb, true);
      v_data := jsonb_set(v_data, '{wallet_credits,1,created_at_utc}', '"2026-10-01T00:00:03+00:00"'::jsonb, true);
    when 'streak_award_after_old_cycle_end' then
      v_data := jsonb_set(v_data, '{game_rewards,1,awarded_at_utc}', '"2026-10-01T12:00:01+00:00"'::jsonb, true);
      v_data := jsonb_set(v_data, '{wallet_credits,1,created_at_utc}', '"2026-10-01T12:00:01+00:00"'::jsonb, true);
    when 'streak_award_after_server' then
      v_data := jsonb_set(v_data, '{game_rewards,0,awarded_at_utc}', '"2026-10-01T00:00:05+00:00"'::jsonb, true);
      v_data := jsonb_set(v_data, '{wallet_credits,0,created_at_utc}', '"2026-10-01T00:00:05+00:00"'::jsonb, true);
    else
      return null;
  end case;
  return v_data;
end;
$$;

with source as materialized (
  select pg_temp.shop_guest_import_streak_request(
    'f1000000-0000-4000-a000-000000000071',
    'f0000000-0000-4000-a000-000000000001', 'streak-current'
  )#>'{snapshot,data}' as value
), variants as (
  select unnest(array[
    'missing_wallet_row', 'reward_trigger_mismatch', 'reward_cycle_mismatch',
    'reward_amount_mismatch', 'reward_effect_mismatch', 'reward_time_mismatch'
  ]::text[]) as variant
), evaluated as (
  select variants.variant,
    pg_temp.shop_guest_import_try_rewards_source_normalize(
      pg_temp.shop_guest_import_streak_variant(source.value, variants.variant)
    ) as result
  from source cross join variants
)
select ok(
  result->>'stage' = 'ready'
    and result ? 'rewards'
    and result->'rewards' = 'null'::jsonb,
  variant || ' streak reward/wallet mismatch is held'
)
from evaluated
order by variant;

with source as materialized (
  select pg_temp.shop_guest_import_streak_request(
    'f1000000-0000-4000-a000-000000000071',
    'f0000000-0000-4000-a000-000000000001', 'streak-current'
  )#>'{snapshot,data}' as value
), delayed as (
  select jsonb_set(
    jsonb_set(
      jsonb_set(value, '{effect_timeline_state,server_time_utc}',
        '"2026-10-03T00:00:00+00:00"'::jsonb, true),
      '{game_rewards,0,awarded_at_utc}',
      '"2026-10-02T12:00:00+00:00"'::jsonb, true),
    '{wallet_credits,0,created_at_utc}',
    '"2026-10-02T12:00:00+00:00"'::jsonb, true
  ) as value
  from source
), normalized as (
  select value, pg_temp.shop_guest_import_try_rewards_source_normalize(value) as result
  from delayed
)
select ok(
  result->>'stage' = 'ready'
    and result#>>'{rewards,validation_scope}' = 'reward_wallet_streak_consistency'
    and result#>>'{rewards,streak_mirrors,0,reward_date}' = '2026-10-01'
    and result#>>'{rewards,streak_mirrors,0,awarded_at_utc}' = '2026-10-02T12:00:00+00:00'
    and result#>>'{rewards,streak_mirrors,0,created_at_utc}' = '2026-10-02T12:00:00+00:00'
    and value#>>'{activity_days,1,reward_date}' = '2026-10-01'
    and value#>>'{activity_days,1,first_occurred_at_utc}' = '2026-10-01T00:00:01+00:00',
  'a later local-date settlement preserves the original activity date and mirror time'
)
from normalized;

with source as materialized (
  select pg_temp.shop_guest_import_single_reset_streak_request(
    'f1000000-0000-4000-a000-000000000072',
    'f0000000-0000-4000-a000-000000000001', 'current-after-streak-reset'
  )#>'{snapshot,data}' as value
), normalized as (
  select value, pg_temp.shop_guest_import_try_rewards_source_normalize(value) as result
  from source
)
select ok(
  result->>'stage' = 'ready'
    and result#>>'{usage,validation_scope}' = 'usage_effect_activity_consistency'
    and result#>>'{resets,validation_scope}' = 'reset_settlement_consistency'
    and (result#>>'{resets,cycle_token_credits,0,raw_tokens}')::bigint = 110
    and (result#>>'{resets,cycle_token_credits,0,bonus_tokens}')::bigint = 1
    and (result#>>'{resets,cycle_token_credits,0,total_tokens}')::bigint = 111
    and jsonb_array_length(value->'game_rewards') = 2
    and jsonb_array_length(value->'wallet_credits') = 2,
  'mixed cycle-token and streak fixture passes all upstream source controls'
)
from normalized;

with source as materialized (
  select pg_temp.shop_guest_import_single_reset_streak_request(
    'f1000000-0000-4000-a000-000000000072',
    'f0000000-0000-4000-a000-000000000001', 'current-after-streak-reset'
  )#>'{snapshot,data}' as value
), normalized as (
  select pg_temp.shop_guest_import_try_rewards_source_normalize(value) as result
  from source
)
select ok(
  result->>'stage' = 'ready'
    and result#>>'{rewards,validation_scope}' = 'reward_wallet_streak_consistency'
    and jsonb_array_length(result#>'{rewards,cycle_token_mirrors}') = 1
    and result#>>'{rewards,cycle_token_mirrors,0,trigger_key}' = 'cycle-token:history-cycle'
    and (result#>>'{rewards,cycle_token_mirrors,0,amount}')::bigint = 1
    and jsonb_array_length(result#>'{rewards,streak_mirrors}') = 1
    and result#>>'{rewards,streak_mirrors,0,trigger_key}' = 'streak:2026-10-01'
    and result#>>'{rewards,streak_mirrors,0,cycle_id}' = 'history-cycle'
    and (result#>>'{rewards,streak_mirrors,0,amount}')::bigint = 10000
    and not (result->'rewards' ? 'server_game_rewards'),
  'mixed source returns exactly one reset mirror and one streak mirror without double-credit output'
)
from normalized;

with source as materialized (
  select pg_temp.shop_guest_import_streak_request(
    'f1000000-0000-4000-a000-000000000071',
    'f0000000-0000-4000-a000-000000000001', 'streak-current'
  )#>'{snapshot,data}' as value
), eligible_without_pair as (
  select jsonb_set(
    jsonb_set(value, '{game_rewards}', '[]'::jsonb, true),
    '{wallet_credits}', '[]'::jsonb, true
  ) as value from source
), normalized as (
  select value, pg_temp.shop_guest_import_try_rewards_source_normalize(value) as result
  from eligible_without_pair
)
select ok(
  result->>'stage' = 'ready'
    and result#>>'{rewards,validation_scope}' = 'reward_wallet_streak_consistency'
    and result#>'{rewards,streak_mirrors}' = '[]'::jsonb
    and jsonb_array_length(value->'activity_days') = 2
    and value#>>'{effect_history,0,effects,streak_reward_tokens}' = '10000'
    and not (result->'rewards' ? 'server_game_rewards'),
  'eligible streak activity without serialized rows yields no synthesized reward'
)
from normalized;

with source as materialized (
  select pg_temp.shop_guest_import_single_reset_streak_request(
    'f1000000-0000-4000-a000-000000000072',
    'f0000000-0000-4000-a000-000000000001', 'current-after-streak-reset'
  )#>'{snapshot,data}' as value
), variants as (
  select unnest(array[
    'duplicate_streak_trigger', 'streak_award_before_first',
    'streak_award_after_old_cycle_end'
  ]::text[]) as variant
), evaluated as (
  select variants.variant,
    pg_temp.shop_guest_import_try_rewards_source_normalize(
      pg_temp.shop_guest_import_streak_variant(source.value, variants.variant)
    ) as result
  from source cross join variants
)
select ok(
  result->>'stage' = 'ready'
    and result ? 'rewards'
    and result->'rewards' = 'null'::jsonb,
  variant || ' streak eligibility boundary is held'
)
from evaluated
order by variant;

with source as materialized (
  select pg_temp.shop_guest_import_streak_request(
    'f1000000-0000-4000-a000-000000000071',
    'f0000000-0000-4000-a000-000000000001', 'streak-current'
  )#>'{snapshot,data}' as value
), mutated as (
  select pg_temp.shop_guest_import_streak_variant(value, 'streak_award_after_server') as value
  from source
), normalized as (
  select value, pg_temp.shop_guest_import_try_rewards_source_normalize(value) as result
  from mutated
)
select ok(
  result->>'stage' = 'ready'
    and result ? 'rewards'
    and result->'rewards' = 'null'::jsonb,
  'streak mirror awarded after the accepted server time is held'
)
from normalized;

select * from finish();
rollback;
