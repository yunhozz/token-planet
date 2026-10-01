begin;
create extension if not exists pgtap with schema extensions;
select no_plan();
\ir fixtures/planet.inc

insert into auth.users(id) values
  ('00000000-0000-0000-0000-000000001207'),
  ('00000000-0000-0000-0000-000000001208');

create function pg_temp.shop_apply(p_request jsonb)
returns jsonb
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

create function pg_temp.try_planet_upsert(p_state jsonb, p_device jsonb)
returns jsonb
language plpgsql as $$
begin
  return public.upsert_my_planet_state(p_state, p_device);
exception when others then
  return jsonb_build_object('error', sqlstate);
end;
$$;

create function pg_temp.natural_new_cycle_upload()
returns jsonb
language plpgsql security definer set search_path = '' as $$
declare
  v_planet jsonb;
  v_cycle_id text;
  v_cycle_started_at timestamptz;
  v_effect_revision bigint;
  v_occurred_at timestamptz := clock_timestamp();
  v_day text;
begin
  v_planet := public.get_my_planet_state();
  v_cycle_id := v_planet->>'current_cycle_id';
  v_cycle_started_at := (v_planet->>'cycle_started_at_utc')::timestamptz;
  v_day := to_char(v_occurred_at at time zone 'Asia/Seoul', 'YYYY-MM-DD');
  select coalesce(max(h.revision), 0) into v_effect_revision
  from private.shop_effect_history h
  where h.user_id = (select auth.uid()) and h.cycle_id = v_cycle_id;

  return public.upsert_my_planet_state(
    pg_temp.planet_state('Removal Action', v_cycle_id, v_cycle_started_at),
    jsonb_build_object(
      'device_id', '30000000-0000-0000-0000-000000001207',
      'current_cycle_id', v_cycle_id,
      'lifetime_tokens', 200000, 'current_planet_tokens', 100000,
      'daily_tokens', jsonb_build_object(v_day, 100000), 'incomplete', false,
      'canonical_version', 5,
      'daily_segments', jsonb_build_array(jsonb_build_object(
        'cycle_id', v_cycle_id, 'date', v_day,
        'effect_revision', v_effect_revision, 'tokens', 100000
      )),
      'activity_days', jsonb_build_array(jsonb_build_object(
        'cycle_id', v_cycle_id, 'reward_date', v_day,
        'first_occurred_at_utc', v_occurred_at, 'tokens', 100000
      ))
    )
  );
end;
$$;

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000001207', true);
select lives_ok($$select public.upsert_my_planet_state(
  pg_temp.planet_state('Removal Action', 'natural-action-cycle', null,
    '[]'::jsonb),
  pg_temp.planet_device('30000000-0000-0000-0000-000000001207', 'natural-action-cycle', 0)
    || jsonb_build_object('canonical_version', 0, 'daily_segments', '[]'::jsonb, 'activity_days', '[]'::jsonb))$$,
  'removal owner starts a funded server-owned cycle');
select lives_ok($$select public.upsert_my_planet_state(
  pg_temp.planet_state('Removal Action', 'natural-action-cycle'),
  jsonb_build_object(
    'device_id', '30000000-0000-0000-0000-000000001207',
    'current_cycle_id', 'natural-action-cycle',
    'lifetime_tokens', 100000, 'current_planet_tokens', 100000,
    'daily_tokens', '{"2026-09-26":100000}'::jsonb, 'incomplete', false,
    'canonical_version', 1,
    'daily_segments', '[{"cycle_id":"natural-action-cycle","date":"2026-09-26","effect_revision":0,"tokens":100000}]'::jsonb,
    'activity_days', '[{"cycle_id":"natural-action-cycle","reward_date":"2026-09-26","first_occurred_at_utc":"2026-09-26T12:00:00Z","tokens":100000}]'::jsonb
  )
)$$, 'positive canonical usage creates a server-owned natural object');

reset role;
insert into private.planet_wallet_credits(user_id, previous_cycle_id, amount, created_at)
values ('00000000-0000-0000-0000-000000001207', 'natural-action-credit',
  1000000, '2026-09-30T00:00:00Z');
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000001207', true);
select is((public.get_my_shop_state()->>'available_balance')::bigint, 1000000::bigint,
  'trusted server wallet fixture funds the natural removal quote');

create temporary table natural_basis_before as
select state->'objects' as objects
from (select public.get_my_planet_state() as state) snapshot;
create temporary table natural_removal_key as
select jsonb_build_object(
  'cycle_id', 'natural-action-cycle',
  'stage', (objects->0->>'stage')::integer,
  'ordinal', (objects->0->>'ordinal')::integer
) as value
from natural_basis_before;
create temporary table natural_removal_quote as
select public.quote_shop_action(jsonb_build_object(
  'kind', 'remove_natural', 'key', value
)) as value
from natural_removal_key;
select is((select value->>'price' from natural_removal_quote), '100000',
  'the public quote RPC supplies the existing stage-zero removal quote');

create temporary table natural_removal_request as
select jsonb_build_object(
  'kind', 'remove_natural',
  'request_id', 'natural-removal-action-first',
  'key', (select value from natural_removal_key),
  'expected_version', 0,
  'quote', (select value from natural_removal_quote)
) as value;

create temporary table natural_stale_version as
select pg_temp.shop_apply(jsonb_set(
  jsonb_set(value, '{request_id}', '"natural-removal-action-stale-version"'::jsonb),
  '{expected_version}', '1'::jsonb
)) as value
from natural_removal_request;
select is((select value->>'status' from natural_stale_version), 'version_conflict',
  'a stale natural-object version is rejected');
select ok(
  (select (value->'state'->>'available_balance')::bigint = 1000000
      and jsonb_array_length(value->'state'->'removed_natural_keys') = 0
   from natural_stale_version),
  'a stale version changes neither balance nor removal state');

create temporary table natural_changed_quote as
select pg_temp.shop_apply(jsonb_set(
  jsonb_set(value, '{request_id}', '"natural-removal-action-changed-quote"'::jsonb),
  '{quote,price}', '99999'::jsonb
)) as value
from natural_removal_request;
select is((select value->>'status' from natural_changed_quote), 'quote_changed',
  'a stale quoted price requires reconfirmation');
select is((select (value->'confirmed_quote'->>'price')::bigint from natural_changed_quote),
  100000::bigint, 'quote_changed returns the current server quote');
select ok(
  (select (value->'state'->>'available_balance')::bigint = 1000000
      and jsonb_array_length(value->'state'->'removed_natural_keys') = 0
   from natural_changed_quote),
  'a changed quote does not charge or remove the natural object');

reset role;
update private.planet_wallet_credits
set amount = 99999
where user_id = '00000000-0000-0000-0000-000000001207'
  and previous_cycle_id = 'natural-action-credit';
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000001207', true);
create temporary table natural_insufficient_balance as
select pg_temp.shop_apply(jsonb_set(
  value, '{request_id}', '"natural-removal-action-insufficient"'::jsonb
)) as value
from natural_removal_request;
select is((select value->>'status' from natural_insufficient_balance), 'insufficient_balance',
  'insufficient balance rejects natural removal');
select ok(
  (select (value->'state'->>'available_balance')::bigint = 99999
      and jsonb_array_length(value->'state'->'removed_natural_keys') = 0
   from natural_insufficient_balance),
  'insufficient balance leaves the wallet and natural object unchanged');
reset role;
update private.planet_wallet_credits
set amount = 1000000
where user_id = '00000000-0000-0000-0000-000000001207'
  and previous_cycle_id = 'natural-action-credit';
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000001207', true);

create temporary table natural_removal_result as
select pg_temp.shop_apply(value) as value from natural_removal_request;
select is((select value->>'status' from natural_removal_result), 'removed',
  'the public action atomically removes a canonical natural object');
select is((select (value->'state'->>'available_balance')::bigint from natural_removal_result),
  900000::bigint, 'removal debits the quoted price exactly once');
select ok(
  (select action_result.value->'state'->'removed_natural_keys' @> jsonb_build_array(natural_key.value)
   from natural_removal_result action_result cross join natural_removal_key natural_key)
  and public.get_my_planet_state()->'removed_natural_keys'
    @> jsonb_build_array((select value from natural_removal_key)),
  'the canonical shop and personal planet states both expose the removal tombstone');
select is(public.get_my_planet_state()->'objects',
  (select objects from natural_basis_before),
  'removal preserves the complete canonical natural-object generation basis');

create temporary table natural_removal_replay as
select pg_temp.shop_apply(value) as value from natural_removal_request;
select is((select value->>'status' from natural_removal_replay), 'removed',
  'replaying the same request returns the original removal result');
select ok(
  (select (value->'state'->>'available_balance')::bigint = 900000
      and jsonb_array_length(value->'state'->'removed_natural_keys') = 1
   from natural_removal_replay),
  'same-ID replay preserves one debit and one tombstone');

create temporary table natural_removal_request_conflict as
select pg_temp.shop_apply(jsonb_set(value, '{quote,price}', '99999'::jsonb)) as value
from natural_removal_request;
select is((select value->>'status' from natural_removal_request_conflict), 'request_conflict',
  'reusing a removal request ID with a different payload conflicts');
select ok(
  (select (value->'state'->>'available_balance')::bigint = 900000
      and jsonb_array_length(value->'state'->'removed_natural_keys') = 1
   from natural_removal_request_conflict),
  'request conflict preserves the original single debit and tombstone');

create temporary table natural_removal_duplicate as
select pg_temp.shop_apply(jsonb_set(
  value, '{request_id}', '"natural-removal-action-second-id"'::jsonb
)) as value
from natural_removal_request;
select is((select value->>'status' from natural_removal_duplicate), 'already_removed',
  'a new request ID cannot charge for an already removed natural object');
select ok(
  (select (value->'state'->>'available_balance')::bigint = 900000
      and jsonb_array_length(value->'state'->'removed_natural_keys') = 1
   from natural_removal_duplicate),
  'already-removed result preserves the original single debit and tombstone');

create temporary table natural_same_cycle_refresh as
select public.upsert_my_planet_state(
  pg_temp.planet_state('Removal Action', 'natural-action-cycle')
    || jsonb_build_object('objects', (select objects from natural_basis_before)),
  jsonb_build_object(
    'device_id', '30000000-0000-0000-0000-000000001207',
    'current_cycle_id', 'natural-action-cycle',
    'lifetime_tokens', 100000, 'current_planet_tokens', 100000,
    'daily_tokens', '{"2026-09-26":100000}'::jsonb, 'incomplete', false,
    'canonical_version', 2,
    'daily_segments', '[{"cycle_id":"natural-action-cycle","date":"2026-09-26","effect_revision":0,"tokens":100000}]'::jsonb,
    'activity_days', '[{"cycle_id":"natural-action-cycle","reward_date":"2026-09-26","first_occurred_at_utc":"2026-09-26T12:00:00Z","tokens":100000}]'::jsonb
  )
) as value;
select ok(
  (select refresh.value->'removed_natural_keys' @> jsonb_build_array(natural_key.value)
      and refresh.value->'objects' = basis.objects
   from natural_same_cycle_refresh refresh
   cross join natural_removal_key natural_key
   cross join natural_basis_before basis),
  'a same-cycle old-client upload preserves the tombstone and full generation basis');
create temporary table natural_full_planet_dto as
select public.get_my_planet_state() as value;
select ok((select value ? 'removed_natural_keys' from natural_full_planet_dto),
  'the canonical personal planet DTO carries server-projected tombstones');
create temporary table natural_roundtrip_refresh as
select pg_temp.try_planet_upsert(
  (select value from natural_full_planet_dto),
  jsonb_build_object(
    'device_id', '30000000-0000-0000-0000-000000001207',
    'current_cycle_id', 'natural-action-cycle',
    'lifetime_tokens', 100000, 'current_planet_tokens', 100000,
    'daily_tokens', '{"2026-09-26":100000}'::jsonb, 'incomplete', false,
    'canonical_version', 3,
    'daily_segments', '[{"cycle_id":"natural-action-cycle","date":"2026-09-26","effect_revision":0,"tokens":100000}]'::jsonb,
    'activity_days', '[{"cycle_id":"natural-action-cycle","reward_date":"2026-09-26","first_occurred_at_utc":"2026-09-26T12:00:00Z","tokens":100000}]'::jsonb
  )
) as value;
select ok((select value->>'error' is null from natural_roundtrip_refresh),
  'a public GET DTO round-trips through the current-cycle upsert');
create temporary table natural_forged_tombstone_refresh as
select pg_temp.try_planet_upsert(
  jsonb_set(
    (select value from natural_full_planet_dto), '{removed_natural_keys}',
    '[{"cycle_id":"natural-action-cycle","stage":0,"ordinal":2147483647}]'::jsonb
  ),
  jsonb_build_object(
    'device_id', '30000000-0000-0000-0000-000000001207',
    'current_cycle_id', 'natural-action-cycle',
    'lifetime_tokens', 100000, 'current_planet_tokens', 100000,
    'daily_tokens', '{"2026-09-26":100000}'::jsonb, 'incomplete', false,
    'canonical_version', 4,
    'daily_segments', '[{"cycle_id":"natural-action-cycle","date":"2026-09-26","effect_revision":0,"tokens":100000}]'::jsonb,
    'activity_days', '[{"cycle_id":"natural-action-cycle","reward_date":"2026-09-26","first_occurred_at_utc":"2026-09-26T12:00:00Z","tokens":100000}]'::jsonb
  )
) as value;
select ok(
  (select value->>'error' is null
      and value->'removed_natural_keys' @> jsonb_build_array((select value from natural_removal_key))
      and not (value->'removed_natural_keys' @> '[{"cycle_id":"natural-action-cycle","stage":0,"ordinal":2147483647}]'::jsonb)
   from natural_forged_tombstone_refresh),
  'the upsert ignores forged client tombstones and returns only server-owned removals');
select ok(
  (public.get_my_planet_state()->'removed_natural_keys'
    @> jsonb_build_array((select value from natural_removal_key)))
  and public.get_my_planet_state()->'objects' = (select objects from natural_basis_before),
  'the personal projection still filters the removed object after same-cycle refresh');

reset role;
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000001208', true);
select is(coalesce(
  public.get_my_planet_state()->'removed_natural_keys', '[]'::jsonb
), '[]'::jsonb,
  'a second account does not receive another account natural-removal tombstones');
select is((public.get_my_shop_state()->>'available_balance')::bigint, 0::bigint,
  'a second account receives only its own empty shop balance');
reset role;
insert into private.shop_game_reward(
  user_id, trigger_key, kind, cycle_id, era_stage, reward_date, amount, effect_snapshot
) values (
  '00000000-0000-0000-0000-000000001208', 'era:foreign-reward-cycle:1', 'era',
  'foreign-reward-cycle', 1, null, 321, '{}'::jsonb
);
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000001208', true);
select is((public.get_my_shop_state()->>'available_balance')::bigint, 321::bigint,
  'the removal balance calculation preserves canonical game reward credits');
reset role;

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000001207', true);
select set_config('shop_test.natural_reset_result', public.reset_my_planet(
  '40000000-0000-0000-0000-000000001207', 'natural-action-cycle'
)::text, true);
select is(current_setting('shop_test.natural_reset_result')::jsonb->'action'->>'status',
  'reset', 'reset advances to a new cycle after a natural removal');
select ok((current_setting('shop_test.natural_reset_result')::jsonb
  ->'planet_state'->>'current_cycle_id') <> 'natural-action-cycle',
  'the reset response selects a new natural-object generation cycle');
select is(current_setting('shop_test.natural_reset_result')::jsonb
  ->'planet_state'->'removed_natural_keys', '[]'::jsonb,
  'the new cycle starts with no current natural-removal tombstones');
reset role;
select is((select sum(r.price)::bigint from private.shop_natural_removal r
  where r.user_id = '00000000-0000-0000-0000-000000001207'
    and r.cycle_id = 'natural-action-cycle'), 100000::bigint,
  'the prior-cycle removal charge remains recorded after reset');
select is((select sum(w.amount)::bigint from private.planet_wallet_credits w
  where w.user_id = '00000000-0000-0000-0000-000000001207'), 1100000::bigint,
  'reset credits prior-cycle raw tokens without erasing the original wallet funding');
insert into private.shop_game_reward(
  user_id, trigger_key, kind, cycle_id, era_stage, reward_date, amount, effect_snapshot
) values (
  '00000000-0000-0000-0000-000000001207', 'era:natural-action-cycle:1', 'era',
  'natural-action-cycle', 1, null, 321, '{}'::jsonb
);

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000001207', true);
create temporary table natural_stale_cycle as
select pg_temp.shop_apply(jsonb_set(
  value, '{request_id}', '"natural-removal-action-stale-cycle"'::jsonb
)) as value
from natural_removal_request;
select is((select value->>'status' from natural_stale_cycle), 'cycle_mismatch',
  'a removal request from the closed cycle is rejected after reset');
select ok(
  (select (value->'state'->>'available_balance')::bigint = 1000321
      and value->'state'->'removed_natural_keys' = '[]'::jsonb
   from natural_stale_cycle),
  'a stale-cycle request leaves the new-cycle balance and tombstones unchanged');
select is((public.get_my_shop_state()->>'available_balance')::bigint, 1000321::bigint,
  'the canonical balance retains the removal expense and reward after reset settlement');
select lives_ok($$select pg_temp.natural_new_cycle_upload()$$,
  'a new cycle accepts canonical raw usage and generates fresh natural objects');
create temporary table natural_new_cycle_state as
select public.get_my_planet_state() as value;
select is((select value->'removed_natural_keys' from natural_new_cycle_state),
  '[]'::jsonb, 'new natural objects are not blocked by the previous cycle tombstone');
select ok((select jsonb_array_length(value->'objects') > 0
  from natural_new_cycle_state),
  'the reset cycle generates its natural-object basis normally');
create temporary table natural_new_cycle_key as
select jsonb_build_object(
  'cycle_id', value->>'current_cycle_id',
  'stage', (value->'objects'->0->>'stage')::integer,
  'ordinal', (value->'objects'->0->>'ordinal')::integer
) as value
from natural_new_cycle_state;
create temporary table natural_new_cycle_quote as
select public.quote_shop_action(jsonb_build_object(
  'kind', 'remove_natural', 'key', value
)) as value
from natural_new_cycle_key;
select is((select value->>'price' from natural_new_cycle_quote), '100000',
  'the new cycle natural object receives its own server quote');
select ok((select value->>'price' = '100000' from natural_new_cycle_quote)
  and (public.get_my_shop_state()->>'available_balance')::bigint >= 100000,
  'a canonical game reward contributes to the balance available for the quoted removal');
create temporary table natural_new_cycle_request as
select jsonb_build_object(
  'kind', 'remove_natural',
  'request_id', 'natural-removal-action-newcycle',
  'key', (select value from natural_new_cycle_key),
  'expected_version', 0,
  'quote', (select value from natural_new_cycle_quote)
) as value;

reset role;
create function pg_temp.fail_natural_removal_receipt()
returns trigger
language plpgsql as $$
begin
  if new.request_id = 'natural-removal-action-newcycle' then
    raise exception 'injected receipt failure for rollback test' using errcode = 'P0001';
  end if;
  return new;
end;
$$;
create trigger shop_test_fail_natural_removal_receipt
before insert on private.shop_action_request
for each row execute function pg_temp.fail_natural_removal_receipt();
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000001207', true);
create temporary table natural_new_cycle_rollback as
select pg_temp.shop_apply(value) as value from natural_new_cycle_request;
select is((select value->>'error' from natural_new_cycle_rollback), 'P0001',
  'a failure after tombstone insertion aborts the removal transaction');
select ok(
  (public.get_my_shop_state()->>'available_balance')::bigint = 1000321
  and public.get_my_planet_state()->'removed_natural_keys' = '[]'::jsonb,
  'failed removal rolls back both the debit and tombstone');
reset role;
select is((select count(*)::integer from private.shop_action_request r
  where r.user_id = '00000000-0000-0000-0000-000000001207'
    and r.request_id = 'natural-removal-action-newcycle'), 0,
  'failed removal leaves no replay receipt');
drop trigger shop_test_fail_natural_removal_receipt on private.shop_action_request;

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000001207', true);
create temporary table natural_new_cycle_removal as
select pg_temp.shop_apply(value) as value from natural_new_cycle_request;
select is((select value->>'status' from natural_new_cycle_removal), 'removed',
  'the same request succeeds after the injected transaction failure is removed');
select ok(
  (select (value->'state'->>'available_balance')::bigint = 900321
      and jsonb_array_length(value->'state'->'removed_natural_keys') = 1
   from natural_new_cycle_removal),
  'a new-cycle removal charges once and creates only its current-cycle tombstone');

select * from finish();
rollback;
