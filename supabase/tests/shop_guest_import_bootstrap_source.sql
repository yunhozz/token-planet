begin;
create extension if not exists pgtap with schema extensions;
select no_plan();
\ir fixtures/shop_guest_import_native_empty.inc

create function pg_temp.shop_guest_import_native_source_readiness(p_data jsonb)
returns jsonb
language plpgsql
stable
as $$
declare
  v_owned jsonb;
  v_timeline jsonb;
  v_usage jsonb;
  v_resets jsonb;
  v_rewards jsonb;
  v_ledger jsonb;
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

  v_rewards := private.shop_guest_import_rewards_normalize(p_data, v_usage, v_timeline, v_resets);
  if v_rewards is null then
    return jsonb_build_object('stage', 'rewards_rejected');
  end if;

  v_ledger := private.shop_guest_import_ledger_normalize(p_data);
  if v_ledger is null then
    return jsonb_build_object('stage', 'ledger_rejected');
  end if;

  return jsonb_build_object(
    'stage', 'ready',
    'usage_scope', v_usage->>'validation_scope',
    'reset_scope', v_resets->>'validation_scope',
    'rewards_scope', v_rewards->>'validation_scope',
    'ledger_scope', v_ledger->>'validation_scope',
    'final_balance', v_ledger->'final_balance'
  );
end;
$$;

create function pg_temp.shop_guest_import_try_bootstrap_source_normalize(
  p_data jsonb,
  p_now timestamptz
)
returns jsonb
language plpgsql
as $$
declare
  v_result jsonb;
begin
  execute 'select private.shop_guest_import_bootstrap_source_normalize($1, $2)'
    into v_result using p_data, p_now;
  return v_result;
exception when undefined_function then
  return null;
end;
$$;

with native as (
  select pg_temp.shop_guest_import_native_empty_request() as request
), data as (
  select request#>'{snapshot,data}' as value from native
)
select ok(
  jsonb_typeof(value) = 'object'
    and value->>'shop_state_revision' = '0'
    and value#>>'{profile,nickname}' = 'Synthetic Native'
    and jsonb_typeof(value->'planet_device_id') = 'string'
    and jsonb_typeof(value#>'{current_cycle,cycle_id}') = 'string'
    and value#>'{current_cycle,is_current}' = 'true'::jsonb
    and value#>'{current_cycle,started_at_utc}' = value->'activation_at_utc'
    and value->'current_cycle_usage_tokens' = '0'::jsonb
    and value->'lifetime_usage_tokens' = '0'::jsonb
    and value->'effect_timeline_state' = 'null'::jsonb
    and value->'contribution_canonical_version' = 'null'::jsonb,
  'the unmodified Rust capture has the native zero-use source shape'
)
from data;

with native as (
  select pg_temp.shop_guest_import_native_empty_request() as request
), readiness as (
  select pg_temp.shop_guest_import_native_source_readiness(request#>'{snapshot,data}') as result
  from native
)
select is(
  result->>'stage',
  'reset_rejected',
  'the existing strict ownership and usage chain holds the native capture at reset validation'
)
from readiness;

with native as (
  select pg_temp.shop_guest_import_native_empty_request() as request
)
select ok(
  private.shop_guest_import_ledger_normalize(request#>'{snapshot,data}') is null,
  'the existing strict whole-source ledger validator keeps the native capture held'
)
from native;

with native as (
  select pg_temp.shop_guest_import_native_empty_request() as request
), readiness as (
  select
    request#>'{snapshot,data}' as data,
    pg_temp.shop_guest_import_native_source_readiness(request#>'{snapshot,data}') as source
  from native
), normalized as (
  select
    data,
    source,
    pg_temp.shop_guest_import_try_bootstrap_source_normalize(
      data, '2026-10-03T00:00:00+00:00'::timestamptz
    ) as result
  from readiness
)
select ok(
  source->>'stage' = 'reset_rejected'
    and result->>'validation_scope' = 'native_empty_source_consistency'
    and result#>'{source_metadata,profile}' = data->'profile'
    and result#>'{source_metadata,current_cycle}' = data->'current_cycle'
    and result#>'{source_metadata,effect_timeline_state}' = 'null'::jsonb
    and result#>'{source_metadata,contribution_canonical_version}' = 'null'::jsonb
    and result->'derived_zero_state' = jsonb_build_object(
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
    )
    and result#>'{server_policy,shared_visible}' = 'false'::jsonb,
  'native zero output preserves source metadata and keeps server visibility policy separate'
)
from normalized;

with native as (
  select pg_temp.shop_guest_import_native_empty_request()#>'{snapshot,data}' as data
), variants(data) as (
  select data - 'world_timezone' from native
  union all select data || '{"unrecognized":true}'::jsonb from native
  union all select jsonb_set(data, '{purchases}', 'null'::jsonb, true) from native
  union all select jsonb_set(data, '{shop_state_revision}', '"0"'::jsonb, true) from native
)
select ok(
  bool_and(pg_temp.shop_guest_import_try_bootstrap_source_normalize(
    data, '2026-10-03T00:00:00+00:00'::timestamptz
  ) is null),
  'unknown or missing data keys and malformed top-level field types stay held'
)
from variants;

with native as (
  select pg_temp.shop_guest_import_native_empty_request()#>'{snapshot,data}' as data
), variants(data) as (
  select jsonb_set(data, '{profile}', 'null'::jsonb, true) from native
  union all select jsonb_set(data, '{profile,nickname}', to_jsonb(repeat('N', 25)), true) from native
  union all select jsonb_set(data, '{profile,nickname}', to_jsonb(chr(160) || 'Synthetic Native'), true) from native
  union all select jsonb_set(data, '{profile,nickname}', to_jsonb('Synthetic Native' || chr(8195)), true) from native
  union all select jsonb_set(data, '{profile,avatar}', '"unknown"'::jsonb, true) from native
)
select ok(
  bool_and(pg_temp.shop_guest_import_try_bootstrap_source_normalize(
    data, '2026-10-03T00:00:00+00:00'::timestamptz
  ) is null),
  'profile presence, nickname length and Rust Unicode trim, and avatar enum are strict'
)
from variants;

with native as (
  select pg_temp.shop_guest_import_native_empty_request()#>'{snapshot,data}' as data
), variants(data) as (
  select jsonb_set(data, '{planet_device_id}', to_jsonb('not-a-uuid'::text), true) from native
  union all select jsonb_set(data, '{current_cycle,cycle_id}', to_jsonb('not-a-uuid'::text), true) from native
  union all select jsonb_set(data, '{planet_timezone}', to_jsonb('Asia/Seoul'::text), true) from native
  union all select jsonb_set(data, '{reward_timezone}', to_jsonb('Not/AZone'::text), true) from native
)
select ok(
  bool_and(pg_temp.shop_guest_import_try_bootstrap_source_normalize(
    data, '2026-10-03T00:00:00+00:00'::timestamptz
  ) is null),
  'device and cycle UUIDs and equal valid source timezones are required'
)
from variants;

with native as (
  select pg_temp.shop_guest_import_native_empty_request()#>'{snapshot,data}' as data
), variants(data, now_value) as (
  select jsonb_set(data, '{activation_at_utc}', to_jsonb('2026-10-02T14:54:14Z'::text), true),
         '2026-10-03T00:00:00Z'::timestamptz from native
  union all select data, null::timestamptz from native
  union all select data, 'infinity'::timestamptz from native
  union all select data, '2026-10-02T14:54:13Z'::timestamptz from native
)
select ok(
  bool_and(pg_temp.shop_guest_import_try_bootstrap_source_normalize(data, now_value) is null),
  'activation must equal the current-cycle start and not exceed a finite trusted clock'
)
from variants;

with native as (
  select pg_temp.shop_guest_import_native_empty_request()#>'{snapshot,data}' as data
), variants(data) as (
  select jsonb_set(data, '{current_cycle,is_current}', 'false'::jsonb, true) from native
  union all select jsonb_set(data, '{current_cycle,ended_at_utc}', to_jsonb('2026-10-03T00:00:00Z'::text), true) from native
  union all select jsonb_set(data, '{current_cycle,settled_bonus_tokens}', '0'::jsonb, true) from native
  union all select jsonb_set(data, '{last_reset_at_utc}', to_jsonb('2026-10-02T20:00:00Z'::text), true) from native
  union all select jsonb_set(data, '{reset_available_at_utc}', to_jsonb('2026-10-03T00:00:00Z'::text), true) from native
  union all select jsonb_set(data, '{reset_receipts_unverifiable}', 'true'::jsonb, true) from native
)
select ok(
  bool_and(pg_temp.shop_guest_import_try_bootstrap_source_normalize(
    data, '2026-10-03T00:00:00+00:00'::timestamptz
  ) is null),
  'bootstrap accepts only an unsettled current cycle with no reset metadata'
)
from variants;

with native as (
  select pg_temp.shop_guest_import_native_empty_request()#>'{snapshot,data}' as data
), variants(data) as (
  select jsonb_set(data, '{growth_journal_state,generation}', '1'::jsonb, true) from native
  union all select jsonb_set(data, '{growth_journal_state,deleted_at_utc}', to_jsonb('2026-10-02T20:00:00Z'::text), true) from native
  union all select jsonb_set(data, '{growth_journal_state,unexpected}', 'true'::jsonb, true) from native
  union all select jsonb_set(data, '{growth_journal_cycles}', '[]'::jsonb, true) from native
  union all select jsonb_set(data, '{growth_journal_cycles,0,cycle_id}', to_jsonb('00000000-0000-4000-a000-000000000002'::text), true) from native
  union all select jsonb_set(data, '{growth_journal_cycles,0,wallet_credit}', '0'::jsonb, true) from native
)
select ok(
  bool_and(pg_temp.shop_guest_import_try_bootstrap_source_normalize(
    data, '2026-10-03T00:00:00+00:00'::timestamptz
  ) is null),
  'journal state and its single current-cycle row must match the native zero shape exactly'
)
from variants;

with native as (
  select pg_temp.shop_guest_import_native_empty_request()#>'{snapshot,data}' as data
), variants(data) as (
  select jsonb_set(data, '{shop_state_revision}', '1'::jsonb, true) from native
  union all select jsonb_set(data, '{lifetime_usage_tokens}', '1'::jsonb, true) from native
  union all select jsonb_set(data, '{current_cycle_usage_tokens}', '1'::jsonb, true) from native
  union all select jsonb_set(data, '{natural_objects}', '[{}]'::jsonb, true) from native
  union all select jsonb_set(data, '{avatar_owned}', '[{}]'::jsonb, true) from native
  union all select jsonb_set(data, '{purchase_proofs}', '[{}]'::jsonb, true) from native
  union all select jsonb_set(data, '{removal_proofs}', '[{}]'::jsonb, true) from native
)
select ok(
  bool_and(pg_temp.shop_guest_import_try_bootstrap_source_normalize(
    data, '2026-10-03T00:00:00+00:00'::timestamptz
  ) is null),
  'nonzero totals, shop revision, natural objects, ownership, and action proofs stay held'
)
from variants;

with native as (
  select pg_temp.shop_guest_import_native_empty_request()#>'{snapshot,data}' as data
), variants(data) as (
  select jsonb_set(data, '{effect_timeline_state}', '{}'::jsonb, true) from native
  union all select jsonb_set(data, '{effect_cycle_bounds}', '[{}]'::jsonb, true) from native
  union all select jsonb_set(data, '{effect_history}', '[{}]'::jsonb, true) from native
  union all select jsonb_set(data, '{contribution_canonical_version}', '1'::jsonb, true) from native
  union all select jsonb_set(data, '{effect_cycle_bounds_authoritative}', 'true'::jsonb, true) from native
)
select ok(
  bool_and(pg_temp.shop_guest_import_try_bootstrap_source_normalize(
    data, '2026-10-03T00:00:00+00:00'::timestamptz
  ) is null),
  'the empty validator does not manufacture effect bounds, history, or canonical metadata'
)
from variants;

select * from finish();
rollback;
