begin;
create extension if not exists pgtap with schema extensions;
select no_plan();
\ir fixtures/planet.inc

insert into auth.users(id) values ('00000000-0000-0000-0000-000000001206');

create function pg_temp.try_shop_quote(p_target jsonb)
returns jsonb
language plpgsql as $$
begin
  return public.quote_shop_action(p_target);
exception when others then
  return jsonb_build_object('error', sqlstate);
end;
$$;

create function pg_temp.try_shop_natural_removal_price(
  p_base_price bigint, p_discount_bps integer
)
returns bigint
language plpgsql as $$
declare
  v_price bigint;
begin
  execute 'select private.shop_natural_removal_price($1, $2)'
    into v_price using p_base_price, p_discount_bps;
  return v_price;
exception when others then
  return null;
end;
$$;

create function pg_temp.place_natural_discount_instance(
  p_instance_id uuid, p_request_id text, p_slot integer
)
returns jsonb
language sql as $$
  select public.apply_shop_action(jsonb_build_object(
    'kind', 'place',
    'request_id', p_request_id,
    'cycle_id', 'natural-removal-cycle',
    'instance_id', p_instance_id::text,
    'expected_version', 0,
    'x', 160 + (p_slot % 8) * 80,
    'y', case when p_slot < 8 then 200 else 350 end
  ));
$$;

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000001206', true);
select lives_ok($$select public.upsert_my_planet_state(
  pg_temp.planet_state('Natural Removal', 'natural-removal-cycle'),
  jsonb_build_object(
    'device_id', '30000000-0000-0000-0000-000000001206',
    'current_cycle_id', 'natural-removal-cycle',
    'lifetime_tokens', 0, 'current_planet_tokens', 0,
    'daily_tokens', '{}'::jsonb, 'incomplete', false,
    'canonical_version', 0, 'daily_segments', '[]'::jsonb, 'activity_days', '[]'::jsonb
  )
)$$, 'natural-removal owner starts a server-owned cycle');
select lives_ok($$select public.upsert_my_planet_state(
  pg_temp.planet_state('Natural Removal', 'natural-removal-cycle'),
  jsonb_build_object(
    'device_id', '30000000-0000-0000-0000-000000001206',
    'current_cycle_id', 'natural-removal-cycle',
    'lifetime_tokens', 100000, 'current_planet_tokens', 100000,
    'daily_tokens', '{"2026-09-26":100000}'::jsonb, 'incomplete', false,
    'canonical_version', 1,
    'daily_segments', '[{"cycle_id":"natural-removal-cycle","date":"2026-09-26","effect_revision":0,"tokens":100000}]'::jsonb,
    'activity_days', '[{"cycle_id":"natural-removal-cycle","reward_date":"2026-09-26","first_occurred_at_utc":"2026-09-26T12:00:00Z","tokens":100000}]'::jsonb
  )
)$$, 'positive canonical usage generates the natural object on the server');

create temporary table natural_key as
select jsonb_build_object(
  'cycle_id', 'natural-removal-cycle',
  'stage', (p.state->'objects'->0->>'stage')::integer,
  'ordinal', (p.state->'objects'->0->>'ordinal')::integer
) as value
from (select public.get_my_planet_state() as state) p;
select is((select (value->>'stage')::integer from natural_key), 0,
  'the public planet contains a canonical stage-zero object');

create temporary table natural_quote as
select pg_temp.try_shop_quote(jsonb_build_object(
  'kind', 'remove_natural', 'key', value
)) as value
from natural_key;
select is((select value->>'price' from natural_quote), '100000',
  'the public quote RPC prices removal of a canonical stage-zero natural object');

reset role;
select is(pg_temp.try_shop_natural_removal_price(5000001, 1500), 4250001::bigint,
  'the private removal price helper rounds fractional discounts up to the next token');
select is(private.shop_natural_removal_price(5000001, 0), 5000001::bigint,
  'zero-basis-point discounts preserve the base price');
select is(private.shop_natural_removal_price(5000001, 1), 4999501::bigint,
  'one-basis-point discounts round fractional token prices up');
select is(private.shop_natural_removal_price(5000001, 3000), 3500001::bigint,
  'the maximum discount is capped at 3000 basis points and rounded up');
select is(private.shop_natural_removal_price(5000001, 5000), 3500001::bigint,
  'discounts above 3000 basis points use the 3000-basis-point cap');
select is(private.shop_natural_removal_price(5000001, -100), 5000001::bigint,
  'negative discounts clamp to zero basis points');
select is(private.shop_natural_removal_price(1, 3000), 1::bigint,
  'the minimum positive base price remains at least one token after discount');
select is(private.shop_natural_removal_price(9223372036854775807, 0),
  9223372036854775807::bigint,
  'numeric intermediate arithmetic safely handles the largest bigint base price');
select throws_ok($$select private.shop_natural_removal_price(null::bigint, 0)$$,
  '22023', null, 'the private price helper rejects a null base price');
select throws_ok($$select private.shop_natural_removal_price(100, null::integer)$$,
  '22023', null, 'the private price helper rejects a null discount');
select throws_ok($$select private.shop_natural_removal_price(0, 0)$$,
  '22023', null, 'the private price helper rejects a zero base price');
select throws_ok($$select private.shop_natural_removal_price(-1, 0)$$,
  '22023', null, 'the private price helper rejects a negative base price');
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000001206', true);

-- Stage-key fixtures are generated by the private server canonicalizer, not supplied by the client.
reset role;
update public.planet_member_state p
set objects = (
  select jsonb_agg(
    private.shop_canonical_planet_object(p.current_cycle_id, generated.stage, 0)
    order by generated.stage
  )
  from generate_series(0, 4) as generated(stage)
)
where p.user_id = '00000000-0000-0000-0000-000000001206';

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000001206', true);
select is((public.get_my_planet_state()->>'stage')::integer, 0,
  'the server-seeded canonical object fixture remains at era zero');
create temporary table natural_stage_quotes as
select generated.stage, public.quote_shop_action(jsonb_build_object(
  'kind', 'remove_natural',
  'key', jsonb_build_object(
    'cycle_id', 'natural-removal-cycle', 'stage', generated.stage, 'ordinal', 0
  )
)) as value
from generate_series(0, 4) as generated(stage);
select is((select array_agg((value->>'price')::bigint order by stage) from natural_stage_quotes),
  array[100000, 250000, 500000, 1000000, 2000000]::bigint[],
  'natural removal keeps fixed server prices for stages zero through four');
select is((public.get_my_shop_state()->'effects'->>'natural_removal_discount_bps')::integer,
  0, 'a cycle without active discount placements has zero natural removal discount');

reset role;
create temporary table natural_discount_instances as
select pg_catalog.gen_random_uuid() as instance_id, product.sku,
  variation.variation_index::smallint as variation_index
from (values ('land_toolbox'), ('land_cutter'), ('land_recycler'), ('land_excavator')) product(sku)
cross join generate_series(0, 4) variation(variation_index);
grant select on natural_discount_instances to authenticated;
insert into private.shop_purchase(user_id, request_id, sku, price, catalog_revision, effect_revision)
select '00000000-0000-0000-0000-000000001206',
  'natural-discount-owned-' || owned.sku || '-' || owned.variation_index::text,
  owned.sku, product.price, product.catalog_revision, 0
from natural_discount_instances owned
join private.shop_products product on product.sku = owned.sku;
insert into private.shop_landscape_instance(
  user_id, instance_id, sku, variation_index, seed, variation_version, placement_version
)
select '00000000-0000-0000-0000-000000001206', instance_id, sku, variation_index,
  'natural-discount-' || sku || '-' || variation_index::text, 1, 0
from natural_discount_instances;

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000001206', true);
create temporary table natural_discount_first_placement as
select pg_temp.place_natural_discount_instance(
  instance_id, 'natural-discount-place-land-toolbox-0', 0
) as value
from natural_discount_instances
where sku = 'land_toolbox' and variation_index = 0;
select is((select value->>'status' from natural_discount_first_placement), 'placed',
  'a trusted owned natural-discount product activates through the placement RPC');
select is((public.get_my_shop_state()->'effects'->>'natural_removal_discount_bps')::integer,
  100, 'the active catalog placement supplies its server-owned natural removal effect');
select is((public.quote_shop_action(jsonb_build_object(
  'kind', 'remove_natural', 'key', (select value from natural_key)
))->>'price')::bigint, 99000::bigint,
  'the first available 100-bps catalog discount reduces the stage-zero quote');

create temporary table natural_discount_exact_placements as
select owned.sku, owned.variation_index,
  pg_temp.place_natural_discount_instance(
    owned.instance_id,
    'natural-discount-place-' || owned.sku || '-' || owned.variation_index::text,
    row_number() over (order by owned.sku, owned.variation_index)::integer
  ) as value
from natural_discount_instances owned
where (owned.sku = 'land_toolbox' and owned.variation_index > 0)
  or owned.sku in ('land_cutter', 'land_recycler');
select is((select count(*)::integer from natural_discount_exact_placements
  where value->>'status' = 'placed'), 14,
  'all remaining fixtures needed for the exact 3000-bps discount are placed');
select is((public.get_my_shop_state()->'effects'->>'natural_removal_discount_bps')::integer,
  3000, 'server-confirmed natural removal effects reach the exact 30-percent cap');
select is((public.quote_shop_action(jsonb_build_object(
  'kind', 'remove_natural', 'key', (select value from natural_key)
))->>'price')::bigint, 70000::bigint,
  'an exact 3000-bps natural removal discount charges 70 percent of base price');

create temporary table natural_discount_overcap_placement as
select pg_temp.place_natural_discount_instance(
  instance_id, 'natural-discount-place-land-excavator-0', 15
) as value
from natural_discount_instances
where sku = 'land_excavator' and variation_index = 0;
select is((select value->>'status' from natural_discount_overcap_placement), 'placed',
  'an additional trusted discount product can be placed beyond the cap');
select is((public.get_my_shop_state()->'effects'->>'natural_removal_discount_bps')::integer,
  3000, 'natural removal effect aggregation remains capped above 3000 bps');
select is((public.quote_shop_action(jsonb_build_object(
  'kind', 'remove_natural', 'key', (select value from natural_key)
))->>'price')::bigint, 70000::bigint,
  'an over-cap effect sum uses the same capped natural removal quote');
select is(public.quote_shop_action('{"kind":"purchase","sku":"land_pond"}'::jsonb)
    ->'target'->>'sku', 'land_pond',
  'existing purchase quote targets remain eligible after natural removal pricing is added');
select is((public.quote_shop_action('{"kind":"purchase","sku":"land_pond"}'::jsonb)
    ->>'price')::bigint, 5000000::bigint,
  'natural removal effects do not change existing purchase prices');

select is(pg_temp.try_shop_quote(jsonb_build_object(
  'kind', 'remove_natural', 'key', (select value from natural_key), 'extra', 1
))->>'error', '22023', 'natural removal targets reject extra top-level fields');
select is(pg_temp.try_shop_quote(jsonb_build_object(
  'kind', 'remove_natural',
  'key', jsonb_set((select value from natural_key), '{unexpected}', 'true'::jsonb, true)
))->>'error', '22023', 'natural removal keys reject additional fields');
select is(pg_temp.try_shop_quote(jsonb_build_object(
  'kind', 'remove_natural',
  'key', jsonb_set((select value from natural_key), '{cycle_id}', '1'::jsonb)
))->>'error', '22023', 'natural removal keys require a string cycle ID');
select is(pg_temp.try_shop_quote(jsonb_build_object(
  'kind', 'remove_natural',
  'key', jsonb_set((select value from natural_key), '{cycle_id}', '""'::jsonb)
))->>'error', '22023', 'natural removal keys reject an empty cycle ID');
select is(pg_temp.try_shop_quote(jsonb_build_object(
  'kind', 'remove_natural',
  'key', jsonb_set((select value from natural_key), '{cycle_id}', '"another-cycle"'::jsonb)
))->>'error', '22023', 'natural removal keys from another cycle are rejected');
select is(pg_temp.try_shop_quote(jsonb_build_object(
  'kind', 'remove_natural',
  'key', jsonb_set((select value from natural_key), '{stage}', '5'::jsonb)
))->>'error', '22023', 'natural removal stages above four are rejected');
select is(pg_temp.try_shop_quote(jsonb_build_object(
  'kind', 'remove_natural',
  'key', jsonb_set((select value from natural_key), '{stage}', '0.5'::jsonb)
))->>'error', '22023', 'natural removal stages must be whole numbers');
select is(pg_temp.try_shop_quote(jsonb_build_object(
  'kind', 'remove_natural',
  'key', jsonb_set((select value from natural_key), '{ordinal}', '-1'::jsonb)
))->>'error', '22023', 'natural removal ordinals cannot be negative');
select is(pg_temp.try_shop_quote(jsonb_build_object(
  'kind', 'remove_natural',
  'key', jsonb_set((select value from natural_key), '{ordinal}', '0.5'::jsonb)
))->>'error', '22023', 'natural removal ordinals must be whole numbers');
select is(pg_temp.try_shop_quote(jsonb_build_object(
  'kind', 'remove_natural',
  'key', jsonb_set((select value from natural_key), '{ordinal}', '2147483648'::jsonb)
))->>'error', '22023', 'natural removal ordinals above the integer range are rejected');
select is(pg_temp.try_shop_quote(jsonb_build_object(
  'kind', 'remove_natural',
  'key', jsonb_set((select value from natural_key), '{ordinal}', '1'::jsonb)
))->>'error', '22023', 'canonical-shaped objects that were never generated are rejected');

reset role;
insert into private.shop_natural_removal(user_id, cycle_id, stage, ordinal, price)
values ('00000000-0000-0000-0000-000000001206', 'natural-removal-cycle', 0, 0, 100000);
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000001206', true);
select is(pg_temp.try_shop_quote(jsonb_build_object(
  'kind', 'remove_natural', 'key', (select value from natural_key)
))->>'error', '22023', 'an already tombstoned canonical object cannot be quoted again');
select throws_ok($$select * from private.shop_natural_removal limit 1$$,
  '42501', null, 'authenticated callers cannot read the private removal table directly');
select throws_ok($$select private.shop_natural_removal_quote_json(
  '00000000-0000-0000-0000-000000001206'::uuid, '{}'::jsonb)$$,
  '42501', null, 'authenticated callers cannot execute the private removal quote helper');
select throws_ok($$select private.shop_natural_removal_price(100000, 100)$$,
  '42501', null, 'authenticated callers cannot execute the private price helper');
select ok(not has_table_privilege('anon', 'private.shop_natural_removal', 'SELECT'),
  'anonymous callers have no direct removal table access');
select ok(not has_table_privilege('authenticated', 'private.shop_natural_removal', 'SELECT'),
  'authenticated callers have no direct removal table access');
select ok(not has_table_privilege('service_role', 'private.shop_natural_removal', 'SELECT'),
  'service role has no direct removal table grant');
select ok(not has_function_privilege(
  'anon', 'private.shop_natural_removal_quote_json(uuid,jsonb)', 'EXECUTE'),
  'anonymous callers cannot execute the private removal quote helper');
select ok(not has_function_privilege(
  'authenticated', 'private.shop_natural_removal_quote_json(uuid,jsonb)', 'EXECUTE'),
  'authenticated callers cannot execute the private removal quote helper');
select ok(not has_function_privilege(
  'service_role', 'private.shop_natural_removal_quote_json(uuid,jsonb)', 'EXECUTE'),
  'service role cannot execute the private removal quote helper directly');
select ok(not has_function_privilege(
  'anon', 'private.shop_natural_removal_price(bigint,integer)', 'EXECUTE'),
  'anonymous callers cannot execute the private price helper');
select ok(not has_function_privilege(
  'authenticated', 'private.shop_natural_removal_price(bigint,integer)', 'EXECUTE'),
  'authenticated callers cannot execute the private price helper');
select ok(not has_function_privilege(
  'service_role', 'private.shop_natural_removal_price(bigint,integer)', 'EXECUTE'),
  'service role cannot execute the private price helper directly');

select * from finish();
rollback;
