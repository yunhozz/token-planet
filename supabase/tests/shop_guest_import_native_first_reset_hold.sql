begin;
create extension if not exists pgtap with schema extensions;
select no_plan();
\ir fixtures/shop_guest_import_native_first_reset.inc

insert into auth.users(id) values ('00000000-0000-4000-a000-000000000001');
select set_config('request.jwt.claim.sub', '00000000-0000-4000-a000-000000000001', true);

create function pg_temp.shop_guest_import_native_first_reset_readiness(p_data jsonb)
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

  return jsonb_build_object('stage', 'ready');
end;
$$;

with native as (
  select pg_temp.shop_guest_import_native_first_reset_request() as request
), data as (
  select request#>'{snapshot,data}' as value from native
)
  select ok(
  value->>'current_cycle_usage_tokens' = '0'
    and value->>'lifetime_usage_tokens' = '1000000'
    and value#>>'{historical_cycles,0,settled_bonus_tokens}' = '0'
    and value#>>'{cycle_settlements,0,amount}' = '0'
    and value#>>'{unverified_planet_wallet_claims,0,claimed_amount}' = '1000000'
    and value->>'contribution_canonical_version' = '1'
    and jsonb_array_length(value->'effect_history') = 1
    and value#>>'{effect_history,0,cycle_id}' = value#>>'{current_cycle,cycle_id}'
    and value->'effect_cycle_bounds' = '[]'::jsonb
    and value->'effect_cycle_bounds_authoritative' = 'false'::jsonb
    and value->'effect_timeline_state' = 'null'::jsonb
    and value->'reset_receipts_unverifiable' = 'true'::jsonb
    and value#>'{reset_settlement_proofs,0,final_effect_revision}' = 'null'::jsonb
    and value#>'{reset_settlement_proofs,0,final_effects}' = 'null'::jsonb
    and (select request#>>'{snapshot,disposition}' from native) = 'source_unverifiable',
  'the native first-reset fixture preserves raw 1m, zero bonus/current, and missing old-cycle proof metadata'
)
from data;

with native as (
  select pg_temp.shop_guest_import_native_first_reset_request() as request
), data as (
  select request#>'{snapshot,data}' as value from native
), readiness as (
  select pg_temp.shop_guest_import_native_first_reset_readiness(value) as result from data
)
select ok(
  result->>'stage' <> 'ready'
    and private.shop_guest_import_ledger_normalize(data.value) is null
    and private.shop_guest_import_bootstrap_source_normalize(
      data.value, '2026-10-03T00:00:00+00:00'::timestamptz
    ) is null,
  'strict reset/ledger and native-empty source normalizers keep the ordinary nonempty reset held'
)
from data cross join readiness;

select ok(
  private.shop_guest_import_target_is_fresh('00000000-0000-4000-a000-000000000001'),
  'the synthetic target has no game state before either held import path'
);

with native as (
  select pg_temp.shop_guest_import_native_first_reset_request() as request
), result as (
  select public.import_guest_shop(
    (request#>>'{snapshot,import_id}')::uuid, request
  ) as value, request#>>'{snapshot,import_id}' as expected_import_id
  from native
)
select ok(
  value->>'status' = 'source_unverifiable'
    and value->>'import_id' = expected_import_id,
  'the public import endpoint returns only a held result for the native ordinary reset'
)
from result;

select is((select status from private.shop_guest_import_request
  where user_id = '00000000-0000-4000-a000-000000000001'
    and import_id = '7da19666-68ec-4a45-b1e4-a5f195e8d584'),
  'source_unverifiable', 'the public receipt remains held');
select ok(
  private.shop_guest_import_target_is_fresh('00000000-0000-4000-a000-000000000001'),
  'the public held call creates no game-state rows'
);

with native as (
  select pg_temp.shop_guest_import_native_first_reset_request() as request
), result as (
  select private.shop_guest_bootstrap(
    (request#>>'{snapshot,import_id}')::uuid, request
  ) as value
  from native
)
select ok(
  value->>'status' = 'source_unverifiable',
  'the private bootstrap writer holds the native ordinary reset'
)
from result;

select ok(
  private.shop_guest_import_target_is_fresh('00000000-0000-4000-a000-000000000001'),
  'the private route does not promote the public held receipt or create game-state rows'
);

insert into auth.users(id) values ('00000000-0000-4000-a000-000000000002');
select set_config('request.jwt.claim.sub', '00000000-0000-4000-a000-000000000002', true);

with native as (
  select jsonb_set(
    pg_temp.shop_guest_import_native_first_reset_request(),
    '{snapshot,target_account_id}',
    to_jsonb('account:00000000-0000-4000-a000-000000000002'::text),
    false
  ) as request
), result as (
  select private.shop_guest_bootstrap(
    (request#>>'{snapshot,import_id}')::uuid, request
  ) as value
  from native
)
select ok(
  value->>'status' = 'source_unverifiable',
  'the private bootstrap writer independently holds the native ordinary reset for a fresh account'
)
from result;

select is((select status from private.shop_guest_bootstrap_receipt
  where user_id = '00000000-0000-4000-a000-000000000002'
    and import_id = '7da19666-68ec-4a45-b1e4-a5f195e8d584'),
  'source_unverifiable', 'the private writer stores only its held receipt');
select ok(
  private.shop_guest_import_target_is_fresh('00000000-0000-4000-a000-000000000002'),
  'the private writer creates no game-state rows for the fresh account'
);

select set_config('request.jwt.claim.sub', '00000000-0000-4000-a000-000000000001', true);
select is((select count(*)::integer from public.planet_member_state
  where user_id = '00000000-0000-4000-a000-000000000001'), 0,
  'held ordinary reset does not initialize a planet row');
select is((select count(*)::integer from private.shop_account_state
  where user_id = '00000000-0000-4000-a000-000000000001'), 0,
  'held ordinary reset does not initialize shop state');
select is((select count(*)::integer from private.growth_journal_state
  where user_id = '00000000-0000-4000-a000-000000000001'), 0,
  'held ordinary reset does not initialize journal state');
select is((select count(*)::integer from private.growth_journal_cycles
  where user_id = '00000000-0000-4000-a000-000000000001'), 0,
  'held ordinary reset does not initialize journal cycle metadata');

select * from finish();
rollback;
