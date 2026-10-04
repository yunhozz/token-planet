create function pg_temp.shop_guest_import_boundary_try_source_normalize(p_data jsonb)
returns jsonb
language plpgsql
as $$
declare
  v_owned jsonb;
  v_timeline jsonb;
  v_usage jsonb;
  v_resets jsonb;
  v_rewards jsonb;
begin
  v_owned := private.shop_guest_import_ownership_normalize(p_data);
  if v_owned is null then
    return jsonb_build_object('stage', 'ownership_rejected');
  end if;
  v_timeline := jsonb_build_object(
    'effect_cycle_bounds', v_owned->'effect_cycle_bounds',
    'effect_history', v_owned->'effect_history'
  );
  v_usage := private.shop_guest_import_usage_normalize(p_data, v_timeline);
  if v_usage is null then
    return jsonb_build_object('stage', 'usage_rejected');
  end if;
  v_resets := private.shop_guest_import_reset_normalize(p_data, v_usage, v_timeline);
  if v_resets is null then
    return jsonb_build_object('stage', 'reset_rejected');
  end if;
  v_rewards := private.shop_guest_import_rewards_normalize(
    p_data, v_usage, v_timeline, v_resets
  );
  return jsonb_build_object(
    'stage', 'ready', 'usage', v_usage, 'resets', v_resets,
    'rewards', v_rewards
  );
end;
$$;

create function pg_temp.shop_guest_import_streak_cross_cycle_request(
  p_import_id uuid,
  p_user_id uuid,
  p_current_cycle text
)
returns jsonb
language plpgsql
immutable
as $$
declare
  v_request jsonb := pg_temp.shop_guest_import_single_reset_streak_request(
    p_import_id, p_user_id, p_current_cycle
  );
  v_data jsonb := v_request#>'{snapshot,data}';
  v_lantern_id constant text := '20000000-0000-4000-a000-000000000002';
  v_lantern_effects constant jsonb := jsonb_build_object(
    'token_earning_bps', 0,
    'civilization_growth_bps', 0,
    'shop_discount_bps', 0,
    'reset_cooldown_bps', 0,
    'natural_removal_discount_bps', 0,
    'era_reward_tokens', 0,
    'streak_reward_tokens', 10000
  );
  v_old_cycle_effects constant jsonb := jsonb_build_object(
    'token_earning_bps', 100,
    'civilization_growth_bps', 0,
    'shop_discount_bps', 0,
    'reset_cooldown_bps', 0,
    'natural_removal_discount_bps', 0,
    'era_reward_tokens', 0,
    'streak_reward_tokens', 10000
  );
  v_old_cycle_instance_ids constant jsonb := jsonb_build_array(
    '20000000-0000-4000-a000-000000000001', v_lantern_id
  );
begin
  -- Move the old-cycle positive source on Oct 1 onto the previous local date,
  -- leaving Oct 1's first positive activity in the post-reset current cycle.
  v_data := jsonb_set(v_data, '{activity_days,0,first_occurred_at_utc}',
    '"2026-09-30T23:59:59.750000+00:00"'::jsonb, true);
  v_data := jsonb_set(v_data, '{activity_days,0,tokens}', '110'::jsonb, true);
  v_data := jsonb_set(v_data, '{activity_days,1,cycle_id}', to_jsonb(p_current_cycle), true);
  v_data := jsonb_set(v_data, '{activity_days,1,first_occurred_at_utc}',
    '"2026-10-01T12:00:01+00:00"'::jsonb, true);
  v_data := jsonb_set(v_data, '{activity_days,1,tokens}', '60'::jsonb, true);
  v_data := jsonb_set(v_data, '{effect_history,0,started_at_utc}',
    '"2026-09-30T23:59:59.500000+00:00"'::jsonb, true);
  v_data := jsonb_set(v_data, '{effect_history,0,ended_at_utc}',
    '"2026-10-01T06:00:00+00:00"'::jsonb, true);
  v_data := jsonb_set(v_data, '{effect_history,1,started_at_utc}',
    '"2026-10-01T06:00:00+00:00"'::jsonb, true);
  v_data := jsonb_set(v_data, '{effect_history,0,active_instance_ids}',
    v_old_cycle_instance_ids, true);
  v_data := jsonb_set(v_data, '{effect_history,0,effects}', v_old_cycle_effects, true);
  v_data := jsonb_set(v_data, '{effect_history,1,active_instance_ids}',
    v_old_cycle_instance_ids, true);
  v_data := jsonb_set(v_data, '{effect_history,1,effects}', v_old_cycle_effects, true);

  v_data := jsonb_set(v_data, '{effect_contributions}', jsonb_build_array(
    jsonb_build_object(
      'device_id', '30000000-0000-4000-a000-000000000001',
      'cycle_id', 'history-cycle', 'date', '2026-09-30', 'effect_revision', 1,
      'canonical_version', 1, 'tokens', 110, 'growth_bps', 0, 'wallet_bps', 100
    ),
    jsonb_build_object(
      'device_id', '30000000-0000-4000-a000-000000000001',
      'cycle_id', p_current_cycle, 'date', '2026-10-01', 'effect_revision', 3,
      'canonical_version', 1, 'tokens', 60, 'growth_bps', 0, 'wallet_bps', 0
    )
  ), true);
  v_data := jsonb_set(v_data, '{reset_settlement_proofs,0,final_active_instance_ids}',
    v_old_cycle_instance_ids, true);
  v_data := jsonb_set(v_data, '{reset_settlement_proofs,0,final_effects}',
    v_old_cycle_effects, true);

  v_data := jsonb_set(v_data, '{usage_aggregates}', jsonb_build_array(
    jsonb_build_object(
      'cycle_id', 'history-cycle', 'bucket_date', '2026-09-30', 'agent', 'codex',
      'event_count', 2, 'total_tokens', 110, 'coverage', 'complete'
    ),
    jsonb_build_object(
      'cycle_id', p_current_cycle, 'bucket_date', '2026-10-01', 'agent', 'claude_code',
      'event_count', 1, 'total_tokens', 60, 'coverage', 'complete'
    )
  ), true);
  v_data := jsonb_set(v_data, '{daily_agent_totals}', jsonb_build_array(
    jsonb_build_object(
      'bucket_date', '2026-09-30', 'agent', 'codex',
      'total_tokens', 110, 'coverage', 'complete'
    ),
    jsonb_build_object(
      'bucket_date', '2026-10-01', 'agent', 'claude_code',
      'total_tokens', 60, 'coverage', 'complete'
    )
  ), true);
  v_data := jsonb_set(v_data, '{growth_journal_entries}', jsonb_build_array(
    jsonb_build_object(
      'device_id', '30000000-0000-4000-a000-000000000001',
      'cycle_id', 'history-cycle', 'bucket_date', '2026-09-30', 'agent', 'codex',
      'revision', 1, 'acknowledged_revision', 0, 'generation', 1,
      'present', true, 'confirmed_tokens', 110, 'coverage', 'complete',
      'payload_hash', repeat('f', 64)
    ),
    jsonb_build_object(
      'device_id', '30000000-0000-4000-a000-000000000001',
      'cycle_id', p_current_cycle, 'bucket_date', '2026-10-01', 'agent', 'claude_code',
      'revision', 1, 'acknowledged_revision', 0, 'generation', 1,
      'present', true, 'confirmed_tokens', 60, 'coverage', 'complete',
      'payload_hash', repeat('e', 64)
    )
  ), true);

  v_data := jsonb_set(v_data, '{effect_history,2,active_instance_ids}',
    jsonb_build_array(v_lantern_id), true);
  v_data := jsonb_set(v_data, '{effect_history,2,effects}', v_lantern_effects, true);
  v_data := jsonb_set(v_data, '{placements}', jsonb_build_array(jsonb_build_object(
    'instance_id', v_lantern_id,
    'cycle_id', p_current_cycle,
    'x', 160,
    'y', 200,
    'version', 0
  )), true);
  v_data := jsonb_set(v_data, '{game_rewards,1,cycle_id}', to_jsonb(p_current_cycle), true);
  v_data := jsonb_set(v_data, '{game_rewards,1,effects}', v_lantern_effects, true);
  v_data := jsonb_set(v_data, '{game_rewards,1,awarded_at_utc}',
    '"2026-10-01T12:00:04+00:00"'::jsonb, true);
  v_data := jsonb_set(v_data, '{wallet_credits,1,cycle_id}', to_jsonb(p_current_cycle), true);
  v_data := jsonb_set(v_data, '{wallet_credits,1,created_at_utc}',
    '"2026-10-01T12:00:04+00:00"'::jsonb, true);
  return jsonb_set(v_request, '{snapshot,data}', v_data, true);
end;
$$;

create function pg_temp.shop_guest_import_streak_dst_request(
  p_import_id uuid,
  p_user_id uuid,
  p_cycle_id text,
  p_timezone text,
  p_previous_date text,
  p_current_date text,
  p_cycle_start text,
  p_effect_start text,
  p_acquired_at text,
  p_previous_first text,
  p_current_first text,
  p_awarded_at text,
  p_server_time text
)
returns jsonb
language plpgsql
immutable
as $$
declare
  v_request jsonb := pg_temp.shop_guest_import_streak_request(
    p_import_id, p_user_id, p_cycle_id
  );
  v_data jsonb := v_request#>'{snapshot,data}';
  v_removal_time text := pg_catalog.to_char(
    (p_cycle_start::timestamptz + interval '2 seconds') at time zone 'UTC',
    'YYYY-MM-DD"T"HH24:MI:SS.US'
  ) || '+00:00';
begin
  v_data := v_data || jsonb_build_object(
    'world_timezone', p_timezone,
    'planet_timezone', p_timezone,
    'reward_timezone', p_timezone
  );
  v_data := jsonb_set(v_data, '{activation_at_utc}', to_jsonb(p_cycle_start), true);
  v_data := jsonb_set(v_data, '{current_cycle,started_at_utc}', to_jsonb(p_cycle_start), true);
  v_data := jsonb_set(v_data, '{effect_cycle_bounds,0,started_at_utc}',
    to_jsonb(p_cycle_start), true);
  v_data := jsonb_set(v_data, '{effect_history,0,started_at_utc}',
    to_jsonb(p_effect_start), true);
  v_data := jsonb_set(v_data, '{effect_timeline_state,server_time_utc}',
    to_jsonb(p_server_time), true);
  v_data := jsonb_set(v_data, '{effect_timeline_state,reward_timezone}',
    to_jsonb(p_timezone), true);
  v_data := jsonb_set(v_data, '{growth_journal_cycles,0,started_at_utc}',
    to_jsonb(p_cycle_start), true);
  v_data := jsonb_set(v_data, '{landscape_instances,0,acquired_at_utc}',
    to_jsonb(p_acquired_at), true);
  v_data := jsonb_set(v_data, '{purchases,0,purchased_at_utc}',
    to_jsonb(p_acquired_at), true);
  v_data := jsonb_set(v_data, '{natural_removals,0,removed_at_utc}',
    to_jsonb(v_removal_time), true);
  v_data := jsonb_set(v_data, '{removal_debits,0,created_at_utc}',
    to_jsonb(v_removal_time), true);

  v_data := jsonb_set(v_data, '{activity_days,0,reward_date}',
    to_jsonb(p_previous_date), true);
  v_data := jsonb_set(v_data, '{activity_days,0,first_occurred_at_utc}',
    to_jsonb(p_previous_first), true);
  v_data := jsonb_set(v_data, '{activity_days,1,reward_date}',
    to_jsonb(p_current_date), true);
  v_data := jsonb_set(v_data, '{activity_days,1,first_occurred_at_utc}',
    to_jsonb(p_current_first), true);

  v_data := jsonb_set(v_data, '{effect_contributions,0,date}',
    to_jsonb(p_previous_date), true);
  v_data := jsonb_set(v_data, '{effect_contributions,1,date}',
    to_jsonb(p_current_date), true);
  v_data := jsonb_set(v_data, '{usage_aggregates,0,bucket_date}',
    to_jsonb(p_previous_date), true);
  v_data := jsonb_set(v_data, '{usage_aggregates,1,bucket_date}',
    to_jsonb(p_current_date), true);
  v_data := jsonb_set(v_data, '{daily_agent_totals,0,bucket_date}',
    to_jsonb(p_previous_date), true);
  v_data := jsonb_set(v_data, '{daily_agent_totals,1,bucket_date}',
    to_jsonb(p_current_date), true);
  v_data := jsonb_set(v_data, '{growth_journal_entries,0,bucket_date}',
    to_jsonb(p_previous_date), true);
  v_data := jsonb_set(v_data, '{growth_journal_entries,1,bucket_date}',
    to_jsonb(p_current_date), true);
  v_data := jsonb_set(v_data, '{game_rewards,0,trigger_key}',
    to_jsonb('streak:' || p_current_date), true);
  v_data := jsonb_set(v_data, '{game_rewards,0,awarded_at_utc}',
    to_jsonb(p_awarded_at), true);
  v_data := jsonb_set(v_data, '{wallet_credits,0,trigger_key}',
    to_jsonb('streak:' || p_current_date), true);
  v_data := jsonb_set(v_data, '{wallet_credits,0,created_at_utc}',
    to_jsonb(p_awarded_at), true);
  return jsonb_set(v_request, '{snapshot,data}', v_data, true);
end;
$$;

create function pg_temp.shop_guest_import_streak_half_open_request(
  p_import_id uuid,
  p_user_id uuid,
  p_current_cycle text
)
returns jsonb
language plpgsql
immutable
as $$
declare
  v_request jsonb := pg_temp.shop_guest_import_streak_cross_cycle_request(
    p_import_id, p_user_id, p_current_cycle
  );
  v_data jsonb := v_request#>'{snapshot,data}';
  v_lantern_id constant text := '20000000-0000-4000-a000-000000000002';
  v_baseline_effects constant jsonb := jsonb_build_object(
    'token_earning_bps', 0,
    'civilization_growth_bps', 0,
    'shop_discount_bps', 0,
    'reset_cooldown_bps', 0,
    'natural_removal_discount_bps', 0,
    'era_reward_tokens', 0,
    'streak_reward_tokens', 0
  );
  v_lantern_effects constant jsonb := jsonb_build_object(
    'token_earning_bps', 0,
    'civilization_growth_bps', 0,
    'shop_discount_bps', 0,
    'reset_cooldown_bps', 0,
    'natural_removal_discount_bps', 0,
    'era_reward_tokens', 0,
    'streak_reward_tokens', 10000
  );
begin
  -- The first current-cycle activity lands exactly on the next interval's start.
  v_data := jsonb_set(v_data, '{effect_history,2,ended_at_utc}',
    '"2026-10-01T12:00:01+00:00"'::jsonb, true);
  v_data := jsonb_set(v_data, '{effect_history,2,active_instance_ids}',
    '[]'::jsonb, true);
  v_data := jsonb_set(v_data, '{effect_history,2,effects}', v_baseline_effects, true);
  v_data := jsonb_set(v_data, '{effect_history}', v_data->'effect_history' || jsonb_build_array(
    jsonb_build_object(
      'cycle_id', p_current_cycle,
      'revision', 4,
      'started_at_utc', '2026-10-01T12:00:01+00:00',
      'ended_at_utc', null,
      'active_instance_ids', jsonb_build_array(v_lantern_id),
      'effects', v_lantern_effects
    )
  ), true);
  v_data := jsonb_set(v_data, '{effect_timeline_state,effect_revision}', '4'::jsonb, true);
  v_data := jsonb_set(v_data, '{effect_contributions,1,effect_revision}', '4'::jsonb, true);
  v_data := jsonb_set(v_data, '{game_rewards,1,effects}', v_lantern_effects, true);
  return jsonb_set(v_request, '{snapshot,data}', v_data, true);
end;
$$;
