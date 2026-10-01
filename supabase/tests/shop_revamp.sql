begin;
create extension if not exists pgtap with schema extensions;
select no_plan();
\ir fixtures/planet.inc

select has_table('private', 'shop_products', 'the new authoritative shop catalog exists');
select ok(to_regprocedure('public.get_my_shop_state()') is not null, 'canonical shop state RPC exists');
select ok(to_regprocedure('public.quote_shop_action(jsonb)') is not null, 'server quote RPC exists');

create function pg_temp.shop_catalog_snapshot() returns jsonb
language plpgsql as $$
declare
  v_catalog jsonb;
begin
  execute $sql$
    select jsonb_build_object(
      'total', count(*)::integer,
      'landscape', count(*) filter (where category = 'landscape')::integer,
      'avatar', count(*) filter (where category = 'avatar')::integer,
      'sku_prices', coalesce(jsonb_agg(sku || ':' || price order by sku), '[]'::jsonb)
    )
    from private.shop_products
  $sql$ into v_catalog;
  return v_catalog;
exception when others then
  return jsonb_build_object('error', sqlstate);
end;
$$;

create function pg_temp.shop_state() returns jsonb
language plpgsql as $$
declare
  v_state jsonb;
begin
  execute 'select public.get_my_shop_state()' into v_state;
  return v_state;
exception when others then
  return jsonb_build_object('error', sqlstate);
end;
$$;

create function pg_temp.shop_quote(p_target jsonb) returns jsonb
language plpgsql as $$
declare
  v_quote jsonb;
begin
  execute 'select public.quote_shop_action($1)' into v_quote using p_target;
  return v_quote;
exception when others then
  return jsonb_build_object('error', sqlstate);
end;
$$;

select is(pg_temp.shop_catalog_snapshot(),
  '{"total":48,"landscape":32,"avatar":16,"sku_prices":[
    "avatar_backpack:100000000","avatar_cape:200000000","avatar_crown:200000000",
    "avatar_explorer_hat:100000000","avatar_glasses:100000000","avatar_goggles:350000000",
    "avatar_halo:500000000","avatar_hud:500000000","avatar_jetpack:350000000",
    "avatar_labwear:200000000","avatar_nebula_suit:500000000",
    "avatar_space_helmet:350000000","avatar_spacesuit:350000000",
    "avatar_sunglasses:200000000","avatar_wings:500000000","avatar_workwear:100000000",
    "land_aurora:40000000","land_bazaar:100000000","land_bench:40000000",
    "land_clocktower:15000000","land_crystal:5000000","land_cutter:40000000",
    "land_double_ring:40000000","land_excavator:15000000","land_flag:5000000",
    "land_fountain:100000000","land_freight:40000000","land_garden:5000000",
    "land_greenhouse:40000000","land_laboratory:100000000","land_lantern:5000000",
    "land_launchpad:40000000","land_market:5000000","land_meteors:100000000","land_moonlets:100000000",
    "land_observatory:40000000","land_pond:5000000","land_portal:100000000",
    "land_recycler:100000000","land_reservoir:100000000","land_rover:5000000",
    "land_school:15000000","land_stars:15000000","land_thin_ring:15000000",
    "land_toolbox:5000000","land_trading_post:15000000","land_tree:15000000",
    "land_well:15000000"
  ]}'::jsonb,
  'catalog has the 32 landscape and 16 avatar SKU prices from the approved contract');

insert into auth.users(id) values
  ('00000000-0000-0000-0000-000000000901'),
  ('00000000-0000-0000-0000-000000000902');
insert into public.planet_member_state(
  user_id, nickname, avatar, timezone, current_cycle_id, cycle_started_at,
  current_planet_tokens, lifetime_tokens, growth_credit, stage,
  progress_to_next, incomplete, objects
) values
  ('00000000-0000-0000-0000-000000000901', 'Shop Alice', 'feminine', 'Asia/Seoul', 'shop-cycle', now(), 0, 0, 0, 0, 0, false, '[]'::jsonb),
  ('00000000-0000-0000-0000-000000000902', 'Shop Bob', 'masculine', 'UTC', 'bob-cycle', now(), 0, 0, 0, 0, 0, false, '[]'::jsonb);
insert into private.planet_wallet_credits(user_id, previous_cycle_id, amount, created_at) values
  ('00000000-0000-0000-0000-000000000901', 'shop-wallet', 205000000, now()),
  ('00000000-0000-0000-0000-000000000902', 'bob-wallet', 30000000, now());

-- Seed an active market only when the Task5 schema is present. This keeps the same
-- behavior test executable against the pre-Task5 schema for an observable RED run.
do $$
begin
  if to_regclass('private.shop_landscape_instance') is not null
    and to_regclass('private.shop_landscape_placement') is not null
    and to_regclass('private.shop_purchase') is not null
    and to_regclass('private.shop_effect_history') is not null
  then
    insert into private.shop_purchase(
      user_id, request_id, sku, price, catalog_revision, effect_revision
    ) values (
      '00000000-0000-0000-0000-000000000901', 'fixture-market-purchase',
      'land_market', 5000000, 1, 0
    );
    insert into private.shop_landscape_instance(
      user_id, instance_id, sku, variation_index, seed, placement_version
    ) values (
      '00000000-0000-0000-0000-000000000901',
      '10000000-0000-0000-0000-000000000901', 'land_market', 0, 'shop-test', 4
    );
    insert into private.shop_landscape_placement(
      user_id, instance_id, cycle_id, x, y, version
    ) values (
      '00000000-0000-0000-0000-000000000901',
      '10000000-0000-0000-0000-000000000901', 'shop-cycle', 1, 2, 4
    );
    insert into private.shop_effect_history(
      user_id, cycle_id, revision, started_at, active_instance_ids, effects
    ) values (
      '00000000-0000-0000-0000-000000000901', 'shop-cycle', 7, now(),
      '["10000000-0000-0000-0000-000000000901"]'::jsonb,
      '{"shop_discount_bps":100}'::jsonb
    );
  end if;
end;
$$;

set local role anon;
select set_config('request.jwt.claim.sub', '', true);
select throws_ok($$select public.get_my_shop_state()$$, '42501', null, 'anonymous callers cannot read shop state');
select throws_ok($$select public.quote_shop_action('{"kind":"purchase","sku":"avatar_explorer_hat"}'::jsonb)$$,
  '42501', null, 'anonymous callers cannot obtain a shop quote');
reset role;

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000901', true);
select throws_ok($$select * from private.shop_products limit 1$$, '42501', null, 'authenticated callers cannot read the private catalog directly');
select throws_ok($$select * from private.shop_landscape_instance limit 1$$, '42501', null, 'authenticated callers cannot read private owned instances directly');

select is(pg_temp.shop_state()->>'account_id', 'account:00000000-0000-0000-0000-000000000901', 'state RPC binds data to the authenticated account');
select is(pg_temp.shop_state()->>'current_cycle_id', 'shop-cycle', 'state RPC returns the current canonical cycle');
select is((pg_temp.shop_state()->>'available_balance')::bigint, 200000000::bigint, 'state RPC returns wallet credits less recorded purchases');
select is(jsonb_array_length(pg_temp.shop_state()->'products'), 48, 'state RPC returns the authoritative 48-product catalog');
select is((pg_temp.shop_state()->'landscape_instances'->0->>'placement_version')::bigint, 4::bigint, 'state RPC includes persistent placement version for an owned instance');
select is((pg_temp.shop_state()->'effects'->>'shop_discount_bps')::bigint, 100::bigint, 'state RPC derives active effects from placed landscape');

select is(pg_temp.shop_quote('{"kind":"purchase","sku":"avatar_explorer_hat"}'::jsonb)->'target',
  '{"kind":"purchase","sku":"avatar_explorer_hat"}'::jsonb, 'quote RPC returns the confirmed target');
select is((pg_temp.shop_quote('{"kind":"purchase","sku":"avatar_explorer_hat"}'::jsonb)->>'catalog_revision')::integer,
  1, 'quote RPC uses the authoritative catalog revision');
select is((pg_temp.shop_quote('{"kind":"purchase","sku":"avatar_explorer_hat"}'::jsonb)->>'effect_revision')::bigint,
  7::bigint, 'quote RPC returns the current active-effect revision');
select is((pg_temp.shop_quote('{"kind":"purchase","sku":"avatar_explorer_hat"}'::jsonb)->>'price')::bigint,
  99000000::bigint, 'quote RPC applies the active server-calculated avatar discount');
select is(pg_temp.shop_quote('{"kind":"purchase","sku":"not_in_catalog"}'::jsonb)->>'error',
  '22023', 'quote RPC rejects an unavailable SKU');

select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000902', true);
select is(pg_temp.shop_state()->>'account_id', 'account:00000000-0000-0000-0000-000000000902', 'switching identity returns only the second account state');
select is((pg_temp.shop_state()->>'available_balance')::bigint, 30000000::bigint, 'second account receives only its own wallet credits');
select is((pg_temp.shop_quote('{"kind":"purchase","sku":"avatar_explorer_hat"}'::jsonb)->>'price')::bigint,
  100000000::bigint, 'another account quote does not inherit the first account discount');

select * from finish();
rollback;
