begin;
create extension if not exists pgtap with schema extensions;
select no_plan();
\ir fixtures/planet.inc

insert into auth.users(id) values
  ('00000000-0000-0000-0000-000000000953'),
  ('00000000-0000-0000-0000-000000000954');
insert into private.planet_wallet_credits(user_id, previous_cycle_id, amount, created_at) values
  ('00000000-0000-0000-0000-000000000953', 'placement-credit', 500000000, '2026-09-30T00:00:00Z'),
  ('00000000-0000-0000-0000-000000000954', 'other-credit', 100000000, '2026-09-30T00:00:00Z');
create function pg_temp.shop_apply(p_request jsonb) returns jsonb
language plpgsql as $$
declare
  v_result jsonb;
begin
  execute 'select public.apply_shop_action($1)' into v_result using p_request;
  return v_result;
exception when others then
  return jsonb_build_object('error', sqlstate);
end;
$$;

set local role anon;
select set_config('request.jwt.claim.sub', '', true);
select throws_ok($$select public.apply_shop_action('{}'::jsonb)$$,
  '42501', null, 'anonymous callers cannot apply a shop action');
reset role;

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000953', true);
select lives_ok($$select public.upsert_my_planet_state(
  jsonb_set(pg_temp.planet_state('Placement Owner', 'placement-cycle', null,
    '[{"previous_cycle_id":"placement-credit","amount":500000000,"created_at_utc":"2026-09-30T00:00:00Z"}]'::jsonb),
    '{objects}', '[{"stage":4,"ordinal":37,"kind":"rocket","x":100,"y":100,"seed":18446744073709551615}]'::jsonb),
  pg_temp.planet_device('30000000-0000-0000-0000-000000000953', 'placement-cycle', 0)
    || jsonb_build_object('canonical_version', 0, 'daily_segments', '[]'::jsonb, 'activity_days', '[]'::jsonb))$$,
  'funded account has a current planet cycle');
select lives_ok($$select public.upsert_my_planet_state(
  pg_temp.planet_state('Placement Owner', 'placement-cycle', null,
    '[{"previous_cycle_id":"placement-credit","amount":500000000,"created_at_utc":"2026-09-30T00:00:00Z"}]'::jsonb),
  jsonb_build_object(
    'device_id', '30000000-0000-0000-0000-000000000953',
    'current_cycle_id', 'placement-cycle',
    'lifetime_tokens', 100000, 'current_planet_tokens', 100000,
    'daily_tokens', '{"2026-09-26":100000}'::jsonb,
    'incomplete', false, 'canonical_version', 1,
    'daily_segments', '[{"cycle_id":"placement-cycle","date":"2026-09-26","effect_revision":0,"tokens":100000}]'::jsonb,
    'activity_days', '[{"cycle_id":"placement-cycle","reward_date":"2026-09-26","first_occurred_at_utc":"2026-09-26T12:00:00Z","tokens":100000}]'::jsonb
  )
)$$, 'positive canonical growth creates a server-owned natural-object basis');
create temporary table natural_basis_before as
select public.get_my_planet_state()->'objects' as objects;
reset role;
select is((select jsonb_array_length(objects) from natural_basis_before), 1,
  'placement fixture starts with a nonempty canonical natural-object basis');
select is((select objects->0 from natural_basis_before),
  private.shop_canonical_planet_object('placement-cycle', 0, 0),
  'placement fixture basis contains the Rust-compatible generated natural object');
select ok((select objects @> jsonb_build_array(
  private.shop_canonical_planet_object('placement-cycle', 0, 0)
) from natural_basis_before),
  'the current-cycle server-generated stage-zero object exists');
select is((select count(*)::bigint from natural_basis_before n
  cross join lateral jsonb_array_elements(n.objects) o(value)
  where o.value->>'stage' = '4' and o.value->>'ordinal' = '37'), 0::bigint,
  'the forged legacy stage-four ordinal is not persisted');

reset role;
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000954', true);
select lives_ok($$select public.upsert_my_planet_state(
  pg_temp.planet_state('Other Owner', 'placement-cycle', null,
    '[{"previous_cycle_id":"other-credit","amount":100000000,"created_at_utc":"2026-09-30T00:00:00Z"}]'::jsonb),
  pg_temp.planet_device('30000000-0000-0000-0000-000000000954', 'placement-cycle', 0)
    || jsonb_build_object('canonical_version', 0, 'daily_segments', '[]'::jsonb, 'activity_days', '[]'::jsonb))$$,
  'second account has an independent current cycle');

reset role;
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000953', true);
create temporary table market_quote as
select public.quote_shop_action('{"kind":"purchase","sku":"land_market"}'::jsonb) as quote;
create temporary table stale_pond_quote as
select public.quote_shop_action('{"kind":"purchase","sku":"land_pond"}'::jsonb) as quote;
create temporary table market_purchase as
select pg_temp.shop_apply(jsonb_build_object(
  'kind', 'purchase', 'request_id', 'placement-buy-market',
  'quote', (select quote from market_quote)
)) as result;
select is((select result->>'status' from market_purchase), 'purchased',
  'funded account purchases a landscape instance before placement');
create temporary table market_instance as
select result->'state'->'landscape_instances'->0->>'instance_id' as instance_id
from market_purchase;

create temporary table initial_shop_state as
select public.get_my_shop_state() as state;
create temporary table unowned_place as
select pg_temp.shop_apply(jsonb_build_object(
  'kind', 'place', 'request_id', 'placement-unowned-instance',
  'cycle_id', 'placement-cycle',
  'instance_id', '10000000-0000-0000-0000-000000000999',
  'expected_version', 0, 'x', 160, 'y', 200
)) as result;
select is((select result->>'status' from unowned_place), 'not_owned',
  'an unknown instance cannot be placed');
select is((select result->'state' from unowned_place), (select state from initial_shop_state),
  'rejected ownership action preserves the full canonical state');

reset role;
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000954', true);
create temporary table other_owner_state as
select public.get_my_shop_state() as state;
create temporary table cross_account_place as
select pg_temp.shop_apply(jsonb_build_object(
  'kind', 'place', 'request_id', 'placement-cross-account-instance',
  'cycle_id', 'placement-cycle', 'instance_id', (select instance_id from market_instance),
  'expected_version', 0, 'x', 160, 'y', 200
)) as result;
select is((select result->>'status' from cross_account_place), 'not_owned',
  'an account cannot place another account instance');
select is((select result->'state' from cross_account_place), (select state from other_owner_state),
  'cross-account rejection preserves the caller canonical state');

reset role;
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000953', true);
create temporary table wrong_cycle_place as
select pg_temp.shop_apply(jsonb_build_object(
  'kind', 'place', 'request_id', 'placement-wrong-cycle',
  'cycle_id', 'stale-cycle', 'instance_id', (select instance_id from market_instance),
  'expected_version', 0, 'x', 160, 'y', 200
)) as result;
select is((select result->>'status' from wrong_cycle_place), 'cycle_mismatch',
  'cycle mismatch is checked before instance edits');
select is((select result->'state' from wrong_cycle_place), (select state from initial_shop_state),
  'cycle rejection leaves all canonical state unchanged');

create temporary table market_placed as
select pg_temp.shop_apply(jsonb_build_object(
  'kind', 'place', 'request_id', 'placement-market-first',
  'cycle_id', 'placement-cycle', 'instance_id', (select instance_id from market_instance),
  'expected_version', 0, 'x', 160, 'y', 200
)) as result;
select is((select result->>'status' from market_placed), 'placed',
  'owned ground instance can be placed inside its zone');
select is(((select result->'state'->'landscape_instances'->0->>'placement_version'
  from market_placed)::bigint), 1::bigint,
  'first placement advances the persistent instance version');
select is((public.quote_shop_action('{"kind":"purchase","sku":"land_pond"}'::jsonb)->>'effect_revision')::bigint,
  1::bigint, 'activating the first landscape effect opens revision one');

create temporary table stale_effect_purchase as
select pg_temp.shop_apply(jsonb_build_object(
  'kind', 'purchase', 'request_id', 'placement-stale-effect-quote',
  'quote', (select quote from stale_pond_quote)
)) as result;
select is((select result->>'status' from stale_effect_purchase), 'quote_changed',
  'purchase rejects a quote from the prior effect revision');
select is((select result->'state' from stale_effect_purchase),
  (select result->'state' from market_placed),
  'stale effect quote rejection leaves the full canonical state unchanged');

create temporary table market_moved as
select pg_temp.shop_apply(jsonb_build_object(
  'kind', 'place', 'request_id', 'placement-market-move',
  'cycle_id', 'placement-cycle', 'instance_id', (select instance_id from market_instance),
  'expected_version', 1, 'x', 240, 'y', 200
)) as result;
select is((select result->>'status' from market_moved), 'placed',
  'an active landscape instance can move');
select is(((select result->'state'->'landscape_instances'->0->>'placement_version'
  from market_moved)::bigint), 2::bigint, 'a move advances its persistent version');
select is((public.quote_shop_action('{"kind":"purchase","sku":"land_pond"}'::jsonb)->>'effect_revision')::bigint,
  1::bigint, 'moving the same active instance does not create another effect revision');
select is((public.get_my_shop_state()->'effects'->>'shop_discount_bps')::integer,
  100, 'moving an active instance does not duplicate its effect');

create temporary table invalid_bounds_move as
select pg_temp.shop_apply(jsonb_build_object(
  'kind', 'place', 'request_id', 'placement-out-of-bounds',
  'cycle_id', 'placement-cycle', 'instance_id', (select instance_id from market_instance),
  'expected_version', 2, 'x', 1390, 'y', 500
)) as result;
select is((select result->>'status' from invalid_bounds_move), 'invalid_placement',
  'the whole ground footprint must remain within terrain bounds');
select is((select result->'state' from invalid_bounds_move),
  (select result from market_moved)->'state', 'invalid bounds do not mutate canonical state');
create temporary table invalid_walkway_move as
select pg_temp.shop_apply(jsonb_build_object(
  'kind', 'place', 'request_id', 'placement-walkway-crossing',
  'cycle_id', 'placement-cycle', 'instance_id', (select instance_id from market_instance),
  'expected_version', 2, 'x', 50, 'y', 200
)) as result;
select is((select result->>'status' from invalid_walkway_move), 'invalid_placement',
  'ground footprint cannot intersect a reserved walkway');
create temporary table invalid_huge_coordinate as
select pg_temp.shop_apply(jsonb_build_object(
  'kind', 'place', 'request_id', 'placement-nonfinite-coordinate',
  'cycle_id', 'placement-cycle', 'instance_id', (select instance_id from market_instance),
  'expected_version', 2, 'x', '1e1000'::numeric, 'y', 200
)) as result;
select is((select result->>'status' from invalid_huge_coordinate), 'invalid_placement',
  'coordinates outside finite terrain bounds are rejected');

create temporary table market_retrieved as
select pg_temp.shop_apply(jsonb_build_object(
  'kind', 'retrieve', 'request_id', 'placement-market-retrieve',
  'cycle_id', 'placement-cycle', 'instance_id', (select instance_id from market_instance),
  'expected_version', 2
)) as result;
select is((select result->>'status' from market_retrieved), 'retrieved',
  'a placed landscape instance can be retrieved');
select is(((select result->'state'->'landscape_instances'->0->>'placement_version'
  from market_retrieved)::bigint), 3::bigint,
  'retrieve advances the persistent version without deleting ownership');
select is(jsonb_array_length((select result->'state'->'placements' from market_retrieved)),
  0, 'retrieved instance leaves the active placements');
select is((public.quote_shop_action('{"kind":"purchase","sku":"land_pond"}'::jsonb)->>'effect_revision')::bigint,
  2::bigint, 'retrieval ends the active effect interval exactly once');

create temporary table stale_replacement_place as
select pg_temp.shop_apply(jsonb_build_object(
  'kind', 'place', 'request_id', 'placement-stale-version',
  'cycle_id', 'placement-cycle', 'instance_id', (select instance_id from market_instance),
  'expected_version', 2, 'x', 160, 'y', 200
)) as result;
select is((select result->>'status' from stale_replacement_place), 'version_conflict',
  'a stale pre-retrieve version cannot place the instance');
select is((select result->'state' from stale_replacement_place),
  (select result from market_retrieved)->'state',
  'stale placement rejection preserves the complete state');

create temporary table market_replaced as
select pg_temp.shop_apply(jsonb_build_object(
  'kind', 'place', 'request_id', 'placement-market-replace',
  'cycle_id', 'placement-cycle', 'instance_id', (select instance_id from market_instance),
  'expected_version', 3, 'x', 160, 'y', 200
)) as result;
select is((select result->>'status' from market_replaced), 'placed',
  'retrieved instance can be placed again');
select is(((select result->'state'->'landscape_instances'->0->>'placement_version'
  from market_replaced)::bigint), 4::bigint,
  're-placement continues the persistent version sequence');
select is((public.quote_shop_action('{"kind":"purchase","sku":"land_pond"}'::jsonb)->>'effect_revision')::bigint,
  3::bigint, 're-placement opens one new active effect interval');
select is((pg_temp.shop_apply(jsonb_build_object(
  'kind', 'place', 'request_id', 'placement-market-first',
  'cycle_id', 'placement-cycle', 'instance_id', (select instance_id from market_instance),
  'expected_version', 0, 'x', 160, 'y', 200
))->'state'->'landscape_instances'->0->>'placement_version')::bigint,
  4::bigint, 'place replay returns original status and latest canonical state');

create temporary table ground_boundary_place as
select pg_temp.shop_apply(jsonb_build_object(
  'kind', 'place', 'request_id', 'placement-ground-boundary',
  'cycle_id', 'placement-cycle', 'instance_id', (select instance_id from market_instance),
  'expected_version', 4, 'x', 1356, 'y', 484
)) as result;
select is((select result->>'status' from ground_boundary_place), 'placed',
  'ground footprint may touch the right and bottom terrain edges');
create temporary table beyond_ground_boundary as
select pg_temp.shop_apply(jsonb_build_object(
  'kind', 'place', 'request_id', 'placement-ground-beyond-boundary',
  'cycle_id', 'placement-cycle', 'instance_id', (select instance_id from market_instance),
  'expected_version', 5, 'x', 1357, 'y', 484
)) as result;
select is((select result->>'status' from beyond_ground_boundary), 'invalid_placement',
  'one unit beyond the ground footprint boundary is rejected');

create temporary table sky_quote as
select public.quote_shop_action('{"kind":"purchase","sku":"land_stars"}'::jsonb) as quote;
create temporary table sky_purchase as
select pg_temp.shop_apply(jsonb_build_object(
  'kind', 'purchase', 'request_id', 'placement-buy-sky',
  'quote', (select quote from sky_quote)
)) as result;
select is((select result->>'status' from sky_purchase), 'purchased',
  'sky landscape product can be purchased');
create temporary table sky_instance as
select result->'state'->'landscape_instances'->1->>'instance_id' as instance_id
from sky_purchase;
create temporary table sky_placed as
select pg_temp.shop_apply(jsonb_build_object(
  'kind', 'place', 'request_id', 'placement-sky-first',
  'cycle_id', 'placement-cycle', 'instance_id', (select instance_id from sky_instance),
  'expected_version', 0, 'x', 100, 'y', -100
)) as result;
select is((select result->>'status' from sky_placed), 'placed',
  'sky footprint can be placed within its zone');
create temporary table sky_boundary_move as
select pg_temp.shop_apply(jsonb_build_object(
  'kind', 'place', 'request_id', 'placement-sky-boundary',
  'cycle_id', 'placement-cycle', 'instance_id', (select instance_id from sky_instance),
  'expected_version', 1, 'x', 1324, 'y', -220
)) as result;
select is((select result->>'status' from sky_boundary_move), 'placed',
  'sky footprint may touch the full horizontal and vertical boundaries');
create temporary table sky_beyond_boundary as
select pg_temp.shop_apply(jsonb_build_object(
  'kind', 'place', 'request_id', 'placement-sky-beyond-boundary',
  'cycle_id', 'placement-cycle', 'instance_id', (select instance_id from sky_instance),
  'expected_version', 2, 'x', 1325, 'y', -220
)) as result;
select is((select result->>'status' from sky_beyond_boundary), 'invalid_placement',
  'sky footprint beyond the horizontal boundary is rejected');

select is((pg_temp.shop_apply(jsonb_build_object(
  'kind', 'place', 'request_id', 'placement-natural-key',
  'cycle_id', 'placement-cycle', 'instance_id', 'stage0:0',
  'expected_version', 0, 'x', 160, 'y', 200
))->>'status'), 'not_owned', 'generated natural keys cannot be moved as purchases');
select is((public.get_my_planet_state()->'objects'), (select objects from natural_basis_before),
  'shop placement preserves the complete natural-object generation basis');

create temporary table avatar_quote as
select public.quote_shop_action('{"kind":"purchase","sku":"avatar_explorer_hat"}'::jsonb) as quote;
create temporary table avatar_purchase as
select pg_temp.shop_apply(jsonb_build_object(
  'kind', 'purchase', 'request_id', 'placement-buy-avatar',
  'quote', (select quote from avatar_quote)
)) as result;
select is((select result->>'status' from avatar_purchase), 'purchased',
  'avatar SKU can be purchased before equipment');
create temporary table avatar_equipped as
select pg_temp.shop_apply(jsonb_build_object(
  'kind', 'equip_avatar', 'request_id', 'placement-equip-avatar',
  'slot', 'head', 'sku', 'avatar_explorer_hat', 'expected_version', 0
)) as result;
select is((select result->>'status' from avatar_equipped), 'equipped',
  'owned avatar SKU can be equipped in its catalog slot');
select is((select result->'state'->'avatar_equipment'->'head'->>'sku' from avatar_equipped),
  'avatar_explorer_hat', 'equipped avatar is returned in canonical state');
select is(((select result->'state'->'avatar_equipment'->'head'->>'version'
  from avatar_equipped)::bigint), 1::bigint,
  'equipment version advances independently of landscape versions');
create temporary table wrong_avatar_slot as
select pg_temp.shop_apply(jsonb_build_object(
  'kind', 'equip_avatar', 'request_id', 'placement-avatar-wrong-slot',
  'slot', 'outfit', 'sku', 'avatar_explorer_hat', 'expected_version', 0
)) as result;
select is((select result->>'status' from wrong_avatar_slot), 'not_owned',
  'avatar SKU cannot be equipped in another catalog slot');
create temporary table stale_avatar_equip as
select pg_temp.shop_apply(jsonb_build_object(
  'kind', 'equip_avatar', 'request_id', 'placement-avatar-stale-version',
  'slot', 'head', 'sku', null, 'expected_version', 0
)) as result;
select is((select result->>'status' from stale_avatar_equip), 'version_conflict',
  'stale avatar equipment version is rejected');
select is((select result->'state' from stale_avatar_equip),
  (select result from avatar_equipped)->'state',
  'stale avatar request preserves complete canonical state');
create temporary table avatar_unequipped as
select pg_temp.shop_apply(jsonb_build_object(
  'kind', 'equip_avatar', 'request_id', 'placement-unequip-avatar',
  'slot', 'head', 'sku', null, 'expected_version', 1
)) as result;
select is((select result->>'status' from avatar_unequipped), 'unequipped',
  'avatar slot can be unequipped');
select is((select result->'state'->'avatar_equipment'->'head'->>'sku' from avatar_unequipped),
  null, 'unequip clears the equipped SKU');
select is(((select result->'state'->'avatar_equipment'->'head'->>'version'
  from avatar_unequipped)::bigint), 2::bigint,
  'unequip increments its slot version');
select is((public.quote_shop_action('{"kind":"purchase","sku":"land_pond"}'::jsonb)->>'effect_revision')::bigint,
  4::bigint, 'avatar equipment never changes landscape effect revisions');

reset role;
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000953', true);
select throws_ok($$select count(*) from private.shop_landscape_instance$$,
  '42501', null, 'authenticated users cannot read private instance rows directly');
reset role;
select is((select count(*)::bigint from private.shop_effect_history
  where user_id = '00000000-0000-0000-0000-000000000953'
    and cycle_id = 'placement-cycle'), 4::bigint,
  'effect history records only four active composition changes');
select is((select count(*)::bigint from private.shop_effect_history
  where user_id = '00000000-0000-0000-0000-000000000953'
    and cycle_id = 'placement-cycle' and ended_at is null), 1::bigint,
  'active composition has one open revision interval');
select * from finish();
rollback;
