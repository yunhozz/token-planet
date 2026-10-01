begin;
create extension if not exists pgtap with schema extensions;
select no_plan();
\ir fixtures/planet.inc

insert into auth.users(id) values
  ('00000000-0000-0000-0000-000000000951'),
  ('00000000-0000-0000-0000-000000000952');
insert into private.planet_wallet_credits(user_id, previous_cycle_id, amount, created_at) values
  ('00000000-0000-0000-0000-000000000951', 'action-credit', 200000000, '2026-09-30T00:00:00Z'),
  ('00000000-0000-0000-0000-000000000952', 'discount-credit', 1000000000, '2026-09-30T00:00:00Z');
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

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000951', true);
select lives_ok($$select public.upsert_my_planet_state(
  pg_temp.planet_state('Action Buyer', 'action-cycle', null,
    '[{"previous_cycle_id":"action-credit","amount":200000000,"created_at_utc":"2026-09-30T00:00:00Z"}]'::jsonb),
  pg_temp.planet_device('30000000-0000-0000-0000-000000000951', 'action-cycle', 0)
    || jsonb_build_object('canonical_version', 0, 'daily_segments', '[]'::jsonb, 'activity_days', '[]'::jsonb))$$,
  'purchase fixture has a funded canonical account');

select ok(to_regprocedure('public.apply_shop_action(jsonb)') is not null,
  'atomic shop action RPC exists');
create temporary table action_quote as
select public.quote_shop_action('{"kind":"purchase","sku":"land_pond"}'::jsonb) as quote;
create temporary table action_first as
select pg_temp.shop_apply(jsonb_build_object(
  'kind', 'purchase',
  'request_id', 'shop-actions-purchase-first',
  'quote', quote
)) as result from action_quote;

select is((select result->>'status' from action_first), 'purchased',
  'purchase atomically creates the owned landscape instance');
select is((select result->'confirmed_quote' from action_first),
  (select quote from action_quote), 'purchase returns the server-confirmed quote');
select is((select (result->'state'->>'available_balance')::bigint from action_first),
  195000000::bigint, 'purchase atomically debits the quoted wallet price');
select is(jsonb_array_length((select result->'state'->'landscape_instances' from action_first)),
  1, 'purchase returns its new canonical landscape instance');
select is(((select result->'state'->'landscape_instances'->0->>'variation_index' from action_first)::integer),
  0, 'first purchase assigns variation index zero');

create temporary table action_variants as
select n, pg_temp.shop_apply(jsonb_build_object(
  'kind', 'purchase',
  'request_id', 'shop-actions-variant-' || n::text,
  'quote', (select quote from action_quote)
)) as result
from generate_series(1, 4) n;
select is((select array_agg(result->>'status' order by n) from action_variants),
  array['purchased','purchased','purchased','purchased']::text[],
  'landscape copies one through four are purchased');
select is((
  select array_agg((last_instance.value->>'variation_index')::integer order by a.n)
  from action_variants a
  cross join lateral (
    select e.value
    from jsonb_array_elements(a.result->'state'->'landscape_instances') e(value)
    order by (e.value->>'variation_index')::integer desc
    limit 1
  ) last_instance
), array[1,2,3,4]::integer[], 'purchases assign each unused variation index exactly once');
select is((select (result->'state'->>'available_balance')::bigint from action_variants order by n desc limit 1),
  175000000::bigint, 'five successful landscape purchases debit exactly five prices');
select is((select jsonb_array_length(result->'state'->'landscape_instances') from action_variants order by n desc limit 1),
  5, 'canonical state retains all five owned copies');

create temporary table action_sixth as
select pg_temp.shop_apply(jsonb_build_object(
  'kind', 'purchase',
  'request_id', 'shop-actions-land-pond-sixth',
  'quote', (select quote from action_quote)
)) as result;
select is((select result->>'status' from action_sixth), 'limit_reached',
  'sixth landscape purchase is rejected at the per-SKU cap');
select is((select (result->'state'->>'available_balance')::bigint from action_sixth),
  175000000::bigint, 'sixth landscape purchase does not debit the wallet');
select is(jsonb_array_length((select result->'state'->'landscape_instances' from action_sixth)),
  5, 'sixth landscape purchase does not create an instance');

select is((pg_temp.shop_apply(jsonb_build_object(
  'kind', 'purchase',
  'request_id', 'shop-actions-purchase-first',
  'quote', (select quote from action_quote)
))->>'status'), 'purchased', 'same request payload replays the original success');
select is((pg_temp.shop_apply(jsonb_build_object(
  'kind', 'purchase',
  'request_id', 'shop-actions-purchase-first',
  'quote', (select quote from action_quote)
))->'confirmed_quote'), (select quote from action_quote),
  'purchase replay preserves its original confirmed quote');
select is((pg_temp.shop_apply(jsonb_build_object(
  'kind', 'purchase',
  'request_id', 'shop-actions-purchase-first',
  'quote', (select quote from action_quote)
))->'state'->>'available_balance')::bigint,
  175000000::bigint, 'purchase replay returns latest canonical state without another debit');

select is((pg_temp.shop_apply(jsonb_build_object(
  'kind', 'purchase',
  'request_id', 'shop-actions-purchase-first',
  'quote', jsonb_set((select quote from action_quote), '{price}', '5000001'::jsonb)
))->>'status'), 'request_conflict', 'request ID cannot be reused with a different canonical payload');
select is((pg_temp.shop_apply(jsonb_build_object(
  'kind', 'purchase',
  'request_id', 'shop-actions-stale-price',
  'quote', jsonb_set((select quote from action_quote), '{price}', '5000001'::jsonb)
))->>'status'), 'quote_changed', 'stale quoted price requires user reconfirmation');
select is((pg_temp.shop_apply(jsonb_build_object(
  'kind', 'purchase',
  'request_id', 'shop-actions-stale-catalog',
  'quote', jsonb_set((select quote from action_quote), '{catalog_revision}', '0'::jsonb)
))->>'status'), 'catalog_mismatch', 'stale catalog revision is rejected');
create temporary table action_insufficient_quote as
select public.quote_shop_action('{"kind":"purchase","sku":"avatar_halo"}'::jsonb) as quote;
select is((pg_temp.shop_apply(jsonb_build_object(
  'kind', 'purchase',
  'request_id', 'shop-actions-insufficient',
  'quote', (select quote from action_insufficient_quote)
))->>'status'), 'insufficient_balance', 'insufficient wallet balance rejects purchase');
select is((public.get_my_shop_state()->>'available_balance')::bigint, 175000000::bigint,
  'request conflicts and rejected quotes leave the wallet unchanged');

reset role;
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000952', true);
select lives_ok($$select public.upsert_my_planet_state(
  pg_temp.planet_state('Discount Buyer', 'discount-cycle', null,
    '[{"previous_cycle_id":"discount-credit","amount":1000000000,"created_at_utc":"2026-09-30T00:00:00Z"}]'::jsonb),
  pg_temp.planet_device('30000000-0000-0000-0000-000000000952', 'discount-cycle', 0)
    || jsonb_build_object('canonical_version', 0, 'daily_segments', '[]'::jsonb, 'activity_days', '[]'::jsonb))$$,
  'discount fixture has an independent funded account');
reset role;

do $$
declare
  v_index integer;
  v_instance_id uuid;
  v_active_ids jsonb := '[]'::jsonb;
  v_sku text;
  v_request_id text;
  v_price bigint;
  v_x double precision;
begin
  for v_index in 0..5 loop
    if v_index < 5 then
      v_sku := 'land_bazaar';
      v_request_id := 'discount-fixture-bazaar-' || v_index::text;
      v_price := 100000000;
    else
      v_sku := 'land_market';
      v_request_id := 'discount-fixture-market';
      v_price := 5000000;
    end if;
    v_instance_id := pg_catalog.gen_random_uuid();
    v_x := 160 + v_index * 80;
    insert into private.shop_purchase(
      user_id, request_id, sku, price, catalog_revision, effect_revision
    ) values (
      '00000000-0000-0000-0000-000000000952', v_request_id, v_sku, v_price, 1, 0
    );
    insert into private.shop_landscape_instance(
      user_id, instance_id, sku, variation_index, seed, placement_version
    ) values (
      '00000000-0000-0000-0000-000000000952', v_instance_id, v_sku,
      case when v_index < 5 then v_index else 0 end,
      'discount-fixture-' || v_index::text, 1
    );
    insert into private.shop_landscape_placement(
      user_id, instance_id, cycle_id, x, y, version
    ) values (
      '00000000-0000-0000-0000-000000000952', v_instance_id,
      'discount-cycle', v_x, 200, 1
    );
    v_active_ids := v_active_ids || jsonb_build_array(v_instance_id::text);
  end loop;
  insert into private.shop_effect_history(
    user_id, cycle_id, revision, started_at, active_instance_ids, effects
  ) values (
    '00000000-0000-0000-0000-000000000952', 'discount-cycle', 11, now(), v_active_ids,
    '{"token_earning_bps":0,"civilization_growth_bps":0,"shop_discount_bps":1500,"reset_cooldown_bps":0,"natural_removal_discount_bps":0,"era_reward_tokens":0,"streak_reward_tokens":0}'::jsonb
  );
end;
$$;

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000952', true);
create temporary table action_discount_quotes as
select
  public.quote_shop_action('{"kind":"purchase","sku":"land_pond"}'::jsonb) as landscape,
  public.quote_shop_action('{"kind":"purchase","sku":"avatar_explorer_hat"}'::jsonb) as avatar;
select is((public.get_my_shop_state()->'effects'->>'shop_discount_bps')::integer,
  1500, 'active discount effects are capped at 15 percent');
select is((select (landscape->>'price')::bigint from action_discount_quotes),
  4250000::bigint, 'landscape quote receives the capped 15 percent discount');
select is((select (avatar->>'price')::bigint from action_discount_quotes),
  85000000::bigint, 'avatar quote receives the same capped 15 percent discount');

create temporary table action_avatar_first as
select pg_temp.shop_apply(jsonb_build_object(
  'kind', 'purchase',
  'request_id', 'shop-actions-discount-avatar-first',
  'quote', (select avatar from action_discount_quotes)
)) as result;
select is((select result->>'status' from action_avatar_first), 'purchased',
  'discounted avatar purchase succeeds');
select is((select (result->'confirmed_quote'->>'price')::bigint from action_avatar_first),
  85000000::bigint, 'avatar purchase confirms its discounted server price');
create temporary table action_avatar_duplicate as
select pg_temp.shop_apply(jsonb_build_object(
  'kind', 'purchase',
  'request_id', 'shop-actions-discount-avatar-duplicate',
  'quote', (select avatar from action_discount_quotes)
)) as result;
select is((select result->>'status' from action_avatar_duplicate), 'already_owned',
  'avatar SKU can be purchased only once');
select is((select (result->'state'->>'available_balance')::bigint from action_avatar_duplicate),
  410000000::bigint, 'duplicate avatar request does not debit its discounted price');
select is(jsonb_array_length((select result->'state'->'avatar_owned_skus' from action_avatar_duplicate)),
  1, 'avatar purchase maintains one owned SKU');

select * from finish();
rollback;
