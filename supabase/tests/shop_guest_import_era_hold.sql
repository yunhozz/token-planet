begin;
create extension if not exists pgtap with schema extensions;
select no_plan();
\ir fixtures/shop_guest_import.inc

create function pg_temp.shop_guest_import_try_era_rewards_normalize(p_data jsonb)
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
  v_rewards := private.shop_guest_import_rewards_normalize(
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

create function pg_temp.shop_guest_import_era_variant(
  p_data jsonb,
  p_variant text,
  p_stage integer default 5
)
returns jsonb
language plpgsql
immutable
as $$
declare
  v_data jsonb := p_data;
  v_amount constant bigint := 5000000;
  v_trigger text := 'era:history-cycle:' || p_stage::text;
  v_zero_effects jsonb := jsonb_build_object(
    'token_earning_bps', 0,
    'civilization_growth_bps', 0,
    'shop_discount_bps', 0,
    'reset_cooldown_bps', 0,
    'natural_removal_discount_bps', 0,
    'era_reward_tokens', 0,
    'streak_reward_tokens', 0
  );
  v_positive_effects jsonb;
  v_marker jsonb;
  v_reward jsonb;
  v_wallet jsonb;
begin
  v_positive_effects := v_zero_effects || jsonb_build_object('era_reward_tokens', v_amount);
  v_marker := jsonb_build_object(
    'cycle_id', 'history-cycle',
    'stage', p_stage,
    'trigger_key', v_trigger,
    'effects', v_positive_effects,
    'awarded_at_utc', '2026-10-01T12:00:00+00:00'
  );
  v_reward := jsonb_build_object(
    'reward_id', 'a5000000-0000-4000-a000-000000000001',
    'trigger_key', v_trigger,
    'kind', 'era',
    'cycle_id', 'history-cycle',
    'amount', v_amount,
    'effects', v_positive_effects,
    'awarded_at_utc', '2026-10-01T12:00:00+00:00'
  );
  v_wallet := jsonb_build_object(
    'credit_id', 'a6000000-0000-4000-a000-000000000001',
    'trigger_key', v_trigger,
    'cycle_id', 'history-cycle',
    'amount', v_amount,
    'created_at_utc', '2026-10-01T12:00:00+00:00'
  );

  case p_variant
    when 'zero_effect_marker' then
      v_marker := jsonb_set(v_marker, '{effects}', v_zero_effects, true);
      v_data := jsonb_set(v_data, '{era_progress}', jsonb_build_array(v_marker), true);
    when 'positive_marker_pair' then
      v_data := jsonb_set(v_data, '{era_progress}', jsonb_build_array(v_marker), true);
      v_data := jsonb_set(v_data, '{game_rewards}',
        (v_data->'game_rewards') || jsonb_build_array(v_reward), true);
      v_data := jsonb_set(v_data, '{wallet_credits}',
        (v_data->'wallet_credits') || jsonb_build_array(v_wallet), true);
    when 'positive_marker_only' then
      v_data := jsonb_set(v_data, '{era_progress}', jsonb_build_array(v_marker), true);
    when 'reward_only' then
      v_data := jsonb_set(v_data, '{game_rewards}',
        (v_data->'game_rewards') || jsonb_build_array(v_reward), true);
    when 'wallet_only' then
      v_data := jsonb_set(v_data, '{wallet_credits}',
        (v_data->'wallet_credits') || jsonb_build_array(v_wallet), true);
    when 'threshold_marker' then
      v_data := jsonb_set(v_data, '{era_progress}', jsonb_build_array(v_marker), true);
    when 'unknown_stage_marker' then
      v_marker := jsonb_set(v_marker, '{stage}', '7'::jsonb, true);
      v_marker := jsonb_set(v_marker, '{trigger_key}', '"era:history-cycle:7"'::jsonb, true);
      v_data := jsonb_set(v_data, '{era_progress}', jsonb_build_array(v_marker), true);
    when 'malformed_marker' then
      v_data := jsonb_set(v_data, '{era_progress}', '[{}]'::jsonb, true);
    when 'ambiguous_historical_marker' then
      v_data := jsonb_set(v_data, '{era_progress}', jsonb_build_array(v_marker), true);
    else
      return null;
  end case;

  return v_data;
end;
$$;

with source as materialized (
  select pg_temp.shop_guest_import_single_reset_streak_request(
    'f1000000-0000-4000-a000-000000000073',
    'f0000000-0000-4000-a000-000000000001', 'current-after-era-hold'
  )#>'{snapshot,data}' as value
), normalized as (
  select value, pg_temp.shop_guest_import_try_era_rewards_normalize(value) as result
  from source
)
select ok(
  result->>'stage' = 'ready'
    and result#>>'{usage,validation_scope}' = 'usage_effect_activity_consistency'
    and result#>>'{resets,validation_scope}' = 'reset_settlement_consistency'
    and result#>>'{rewards,validation_scope}' = 'reward_wallet_streak_consistency'
    and jsonb_array_length(result#>'{rewards,cycle_token_mirrors}') = 1
    and jsonb_array_length(result#>'{rewards,streak_mirrors}') = 1
    and not (result->'rewards' ? 'server_game_rewards')
    and value->'era_progress' = '[]'::jsonb,
  'a no-era mixed reset/streak capture keeps its existing validated mirrors'
)
from normalized;

with source as materialized (
  select pg_temp.shop_guest_import_single_reset_streak_request(
    'f1000000-0000-4000-a000-000000000073',
    'f0000000-0000-4000-a000-000000000001', 'current-after-era-hold'
  )#>'{snapshot,data}' as value
), variants as (
  select * from (values
    ('zero_effect_marker', 5),
    ('positive_marker_only', 5),
    ('unknown_stage_marker', 7),
    ('malformed_marker', 5),
    ('ambiguous_historical_marker', 100)
  ) as cases(variant, stage)
), evaluated as (
  select variant, pg_temp.shop_guest_import_try_era_rewards_normalize(
    pg_temp.shop_guest_import_era_variant(source.value, variants.variant, variants.stage)
  ) as result
  from source cross join variants
)
select ok(
  result->>'stage' = 'ready'
    and result#>>'{usage,validation_scope}' = 'usage_effect_activity_consistency'
    and result#>>'{resets,validation_scope}' = 'reset_settlement_consistency'
    and result->'rewards' = 'null'::jsonb,
  variant || ' era progress presence is held after upstream source checks'
)
from evaluated
order by variant;

with source as materialized (
  select pg_temp.shop_guest_import_single_reset_streak_request(
    'f1000000-0000-4000-a000-000000000073',
    'f0000000-0000-4000-a000-000000000001', 'current-after-era-hold'
  )#>'{snapshot,data}' as value
), variants as (
  select * from (values (5), (20), (50), (100)) as stages(stage)
), evaluated as (
  select stage, pg_temp.shop_guest_import_try_era_rewards_normalize(
    pg_temp.shop_guest_import_era_variant(source.value, 'threshold_marker', variants.stage)
  ) as result
  from source cross join variants
)
select ok(
  bool_and(
    result->>'stage' = 'ready'
      and result->'rewards' = 'null'::jsonb
  ) and count(*) = 4,
  'era threshold stages 5, 20, 50 and 100 remain held without reconstructed crossing evidence'
)
from evaluated;

with source as materialized (
  select pg_temp.shop_guest_import_single_reset_streak_request(
    'f1000000-0000-4000-a000-000000000073',
    'f0000000-0000-4000-a000-000000000001', 'current-after-era-hold'
  )#>'{snapshot,data}' as value
), variants as (
  select unnest(array['reward_only', 'wallet_only']::text[]) as variant
), evaluated as (
  select variant, pg_temp.shop_guest_import_try_era_rewards_normalize(
    pg_temp.shop_guest_import_era_variant(source.value, variants.variant, 5)
  ) as result
  from source cross join variants
)
select ok(
  result->>'stage' = 'ready'
    and result#>>'{usage,validation_scope}' = 'usage_effect_activity_consistency'
    and result#>>'{resets,validation_scope}' = 'reset_settlement_consistency'
    and result->'rewards' = 'null'::jsonb,
  variant || ' orphan era reward or wallet row is held'
)
from evaluated
order by variant;

with source as materialized (
  select pg_temp.shop_guest_import_single_reset_streak_request(
    'f1000000-0000-4000-a000-000000000073',
    'f0000000-0000-4000-a000-000000000001', 'current-after-era-hold'
  )#>'{snapshot,data}' as value
), mutated as (
  select pg_temp.shop_guest_import_era_variant(value, 'positive_marker_pair', 5) as value
  from source
), normalized as (
  select value, pg_temp.shop_guest_import_try_era_rewards_normalize(value) as result
  from mutated
)
select ok(
  result->>'stage' = 'ready'
    and result#>>'{usage,validation_scope}' = 'usage_effect_activity_consistency'
    and result#>>'{resets,validation_scope}' = 'reset_settlement_consistency'
    and result->'rewards' = 'null'::jsonb
    and jsonb_array_length(value->'era_progress') = 1
    and value#>>'{era_progress,0,trigger_key}' = 'era:history-cycle:5'
    and value#>>'{game_rewards,2,kind}' = 'era'
    and value#>>'{wallet_credits,2,trigger_key}' = 'era:history-cycle:5',
  'matching-looking era marker, reward and wallet rows do not become an accepted credit mirror'
)
from normalized;

select * from finish();
rollback;
