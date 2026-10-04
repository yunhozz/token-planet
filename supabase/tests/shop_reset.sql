begin;
create extension if not exists pgtap with schema extensions;
select no_plan();
\ir fixtures/planet.inc

select ok(to_regprocedure('public.reset_my_planet(uuid,text)') is not null,
  'the server reset RPC is exposed with its request and cycle keys');
select ok(has_schema_privilege('authenticated', 'private', 'USAGE'),
  'the authenticated role can resolve only explicitly granted private RPCs');
select ok(not has_function_privilege(
    'authenticated', 'private.upsert_my_planet_state_before_reward_delivery(jsonb,jsonb)', 'EXECUTE'
  ), 'the pre-reset reward wrapper is not directly executable by authenticated clients');
select ok(not has_function_privilege(
    'authenticated', 'private.upsert_my_planet_state_legacy(jsonb,jsonb)', 'EXECUTE'
  ), 'the raw legacy updater is not directly executable by authenticated clients');
select ok(has_function_privilege(
    'authenticated', 'public.upsert_my_planet_state(jsonb,jsonb)', 'EXECUTE'
  ), 'authenticated clients retain the guarded public upload RPC');

insert into auth.users(id) values
  ('00000000-0000-0000-0000-000000000995'),
  ('00000000-0000-0000-0000-000000000996');
insert into private.planet_wallet_credits(user_id, previous_cycle_id, amount, created_at)
values (
  '00000000-0000-0000-0000-000000000996', 'positive-accrual-funding',
  6000000, '2026-09-30T00:00:00Z'
);
create function pg_temp.reset_device(
  p_cycle text, p_version bigint,
  p_device uuid default '30000000-0000-0000-0000-000000000995'
)
returns jsonb language sql as $$
  select jsonb_build_object(
    'device_id', p_device,
    'current_cycle_id', p_cycle,
    'lifetime_tokens', 0,
    'current_planet_tokens', 0,
    'daily_tokens', '{}'::jsonb,
    'incomplete', false,
    'canonical_version', p_version,
    'daily_segments', '[]'::jsonb,
    'activity_days', '[]'::jsonb
  );
$$;
create function pg_temp.reset_closed_device(p_cycle text, p_lifetime bigint)
returns jsonb language sql as $$
  with contribution_day as (
    select to_char((statement_timestamp() at time zone 'Asia/Seoul')::date, 'YYYY-MM-DD') as day
  )
  select jsonb_build_object(
    'device_id', '30000000-0000-0000-0000-000000000995',
    'current_cycle_id', p_cycle,
    'lifetime_tokens', p_lifetime,
    'current_planet_tokens', 10,
    'daily_tokens', jsonb_build_object(d.day, 10),
    'incomplete', false,
    'canonical_version', 1,
    'daily_segments', jsonb_build_array(jsonb_build_object(
      'cycle_id', p_cycle, 'date', d.day, 'effect_revision', 1, 'tokens', 10
    )),
    'activity_days', '[]'::jsonb
  )
  from contribution_day d;
$$;
create function pg_temp.positive_accrual_upload(
  p_tokens bigint, p_cycle_id text, p_cycle_started_at timestamptz, p_effect_revision bigint
)
returns jsonb language plpgsql as $$
declare
  v_occurred_at timestamptz := clock_timestamp();
  v_day text;
begin
  v_day := to_char(v_occurred_at at time zone 'Asia/Seoul', 'YYYY-MM-DD');

  return public.upsert_my_planet_state(
    pg_temp.planet_state('Positive Accrual', p_cycle_id, p_cycle_started_at),
    jsonb_build_object(
      'device_id', '30000000-0000-0000-0000-000000000996',
      'current_cycle_id', p_cycle_id,
      'lifetime_tokens', p_tokens,
      'current_planet_tokens', p_tokens,
      'daily_tokens', jsonb_build_object(v_day, p_tokens),
      'incomplete', false,
      'canonical_version', 1,
      'daily_segments', jsonb_build_array(jsonb_build_object(
        'cycle_id', p_cycle_id, 'date', v_day,
        'effect_revision', p_effect_revision, 'tokens', p_tokens
      )),
      'activity_days', jsonb_build_array(jsonb_build_object(
        'cycle_id', p_cycle_id, 'reward_date', v_day,
        'first_occurred_at_utc', v_occurred_at, 'tokens', p_tokens
      ))
    )
  );
end;
$$;

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000995', true);
select lives_ok($$select public.upsert_my_planet_state(
  pg_temp.planet_state('Reset Contract', 'reset-original-cycle', now() - interval '72 hours'),
  pg_temp.reset_device('reset-original-cycle', 0)
)$$, 'initial canonical upload creates the original planet cycle');
reset role;

update public.planet_member_state
set last_reset_at = now() - interval '48 hours',
    cycle_started_at = now() - interval '48 hours',
    reset_available_at = now() - interval '24 hours'
where user_id = '00000000-0000-0000-0000-000000000995';
update private.shop_cycle_effect_baseline
set started_at = now() - interval '48 hours'
where user_id = '00000000-0000-0000-0000-000000000995'
  and cycle_id = 'reset-original-cycle';

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000995', true);
select throws_ok($$select public.upsert_my_planet_state(
  pg_temp.planet_state('Reset Contract', 'client-selected-cycle', now() - interval '24 hours'),
  pg_temp.reset_device('client-selected-cycle', 1)
)$$, '23514', 'planet cycle can only change through reset_my_planet',
  'an elapsed client-supplied reset timestamp cannot change the cycle');
reset role;
select is((select p.current_cycle_id from public.planet_member_state p
  where p.user_id = '00000000-0000-0000-0000-000000000995'), 'reset-original-cycle',
  'rejected legacy upload leaves the server-owned cycle unchanged');

create temporary table reset_metadata_before as
select p.current_cycle_id, p.cycle_started_at, p.last_reset_at, p.reset_available_at
from public.planet_member_state p
where p.user_id = '00000000-0000-0000-0000-000000000995';
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000995', true);
select lives_ok($$select public.upsert_my_planet_state(
  jsonb_set(
    pg_temp.planet_state('Reset Contract', 'reset-original-cycle', now() + interval '10 years'),
    '{reset_available_at_utc}', '"9999-12-31T00:00:00Z"'::jsonb, true
  ),
  pg_temp.reset_device('reset-original-cycle', 0)
)$$, 'same-cycle uploads accept data while ignoring forged reset timestamps');
reset role;
select ok((select p.current_cycle_id is not distinct from b.current_cycle_id
    and p.cycle_started_at is not distinct from b.cycle_started_at
    and p.last_reset_at is not distinct from b.last_reset_at
    and p.reset_available_at is not distinct from b.reset_available_at
  from public.planet_member_state p cross join reset_metadata_before b
  where p.user_id = '00000000-0000-0000-0000-000000000995'),
  'client cycle start, last reset, and cooldown metadata cannot rewrite server values');

update public.planet_member_state
set current_planet_tokens = 123, lifetime_tokens = 456
where user_id = '00000000-0000-0000-0000-000000000995';
update private.planet_device_state d
set current_planet_tokens = 123,
    lifetime_tokens = 456,
    daily_tokens = jsonb_build_object(
      to_char((statement_timestamp() at time zone 'Asia/Seoul')::date, 'YYYY-MM-DD'), 123
    )
where d.user_id = '00000000-0000-0000-0000-000000000995'
  and d.device_id = '30000000-0000-0000-0000-000000000995'
  and d.current_cycle_id = 'reset-original-cycle';
insert into private.shop_landscape_instance(
  user_id, instance_id, sku, variation_index, seed, placement_version
) values (
  '00000000-0000-0000-0000-000000000995',
  '50000000-0000-0000-0000-000000000995', 'land_rover', 0, 'reset-test-seed', 0
);
insert into private.shop_landscape_placement(
  user_id, instance_id, cycle_id, x, y, version
) values (
  '00000000-0000-0000-0000-000000000995',
  '50000000-0000-0000-0000-000000000995', 'reset-original-cycle', 100, 100, 0
);
insert into private.shop_avatar_owned(user_id, sku, slot)
values ('00000000-0000-0000-0000-000000000995', 'avatar_explorer_hat', 'head');
update private.shop_avatar_equipment
set sku = 'avatar_explorer_hat', version = 3
where user_id = '00000000-0000-0000-0000-000000000995' and slot = 'head';
select private.shop_record_effect_change(
  '00000000-0000-0000-0000-000000000995', 'reset-original-cycle'
);

create temporary table reset_failure_before as
select p.current_cycle_id, p.current_planet_tokens, p.lifetime_tokens,
  p.cycle_started_at, p.last_reset_at, p.reset_available_at, p.reset_cooldown_bps,
  s.state_revision,
  (select count(*)::integer from private.planet_wallet_credits w
    where w.user_id = p.user_id and w.previous_cycle_id = p.current_cycle_id) as wallet_rows,
  (select count(*)::integer from private.shop_cycle_token_settlement st
    where st.user_id = p.user_id and st.cycle_id = p.current_cycle_id) as settlement_rows,
  (select count(*)::integer from private.shop_reset_request r
    where r.user_id = p.user_id) as reset_receipts,
  (select count(*)::integer from private.shop_landscape_placement pl
    where pl.user_id = p.user_id and pl.cycle_id = p.current_cycle_id) as placement_rows,
  (select coalesce(jsonb_agg(jsonb_build_object(
      'instance_id', pl.instance_id, 'cycle_id', pl.cycle_id,
      'x', pl.x, 'y', pl.y, 'version', pl.version
    ) order by pl.instance_id), '[]'::jsonb)
    from private.shop_landscape_placement pl where pl.user_id = p.user_id) as placements,
  (select i.placement_version from private.shop_landscape_instance i
    where i.user_id = p.user_id
      and i.instance_id = '50000000-0000-0000-0000-000000000995') as placement_version,
  (select count(*)::integer from private.shop_effect_history h
    where h.user_id = p.user_id and h.ended_at is null) as open_effect_rows,
  (select coalesce(max(h.revision), 0) from private.shop_effect_history h
    where h.user_id = p.user_id) as max_effect_revision,
  (select b.ended_at from private.shop_cycle_effect_baseline b
    where b.user_id = p.user_id and b.cycle_id = p.current_cycle_id) as old_baseline_ended_at,
  (select count(*)::integer from private.shop_planet_object_generation_baseline b
    where b.user_id = p.user_id) as generation_baselines
from public.planet_member_state p
join private.shop_account_state s on s.user_id = p.user_id
where p.user_id = '00000000-0000-0000-0000-000000000995';
create function pg_temp.fail_new_reset_effect_history()
returns trigger language plpgsql as $$
begin
  if new.user_id = '00000000-0000-0000-0000-000000000995'
    and new.cycle_id <> 'reset-original-cycle'
  then
    raise exception 'injected reset failure' using errcode = 'P0001';
  end if;
  return new;
end;
$$;
create trigger shop_reset_test_fail_new_effect_history
before insert on private.shop_effect_history
for each row execute function pg_temp.fail_new_reset_effect_history();
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000995', true);
select throws_ok($$select public.reset_my_planet(
  '40000000-0000-0000-0000-000000000997', 'reset-original-cycle'
)$$, 'P0001', 'injected reset failure',
  'a failure after reset mutations aborts the entire reset transaction');
reset role;
drop trigger shop_reset_test_fail_new_effect_history on private.shop_effect_history;
drop function pg_temp.fail_new_reset_effect_history();
select ok((select p.current_cycle_id is not distinct from b.current_cycle_id
    and p.current_planet_tokens is not distinct from b.current_planet_tokens
    and p.lifetime_tokens is not distinct from b.lifetime_tokens
    and p.cycle_started_at is not distinct from b.cycle_started_at
    and p.last_reset_at is not distinct from b.last_reset_at
    and p.reset_available_at is not distinct from b.reset_available_at
    and p.reset_cooldown_bps is not distinct from b.reset_cooldown_bps
  from public.planet_member_state p cross join reset_failure_before b
  where p.user_id = '00000000-0000-0000-0000-000000000995'),
  'failed reset preserves cycle, token totals, and cooldown metadata');
select is((select count(*)::integer from private.planet_wallet_credits w
  where w.user_id = '00000000-0000-0000-0000-000000000995'
    and w.previous_cycle_id = 'reset-original-cycle'),
  (select wallet_rows from reset_failure_before),
  'failed reset rolls back the wallet credit');
select is((select count(*)::integer from private.shop_cycle_token_settlement st
  where st.user_id = '00000000-0000-0000-0000-000000000995'
    and st.cycle_id = 'reset-original-cycle'),
  (select settlement_rows from reset_failure_before),
  'failed reset rolls back the cycle settlement');
select is((select count(*)::integer from private.shop_reset_request r
  where r.user_id = '00000000-0000-0000-0000-000000000995'),
  (select reset_receipts from reset_failure_before),
  'failed reset leaves no request receipt');
select ok((select count(*)::integer = b.placement_rows
    and coalesce(jsonb_agg(jsonb_build_object(
      'instance_id', pl.instance_id, 'cycle_id', pl.cycle_id,
      'x', pl.x, 'y', pl.y, 'version', pl.version
    ) order by pl.instance_id), '[]'::jsonb) = b.placements
    and (select i.placement_version from private.shop_landscape_instance i
      where i.user_id = '00000000-0000-0000-0000-000000000995'
        and i.instance_id = '50000000-0000-0000-0000-000000000995') = b.placement_version
  from private.shop_landscape_placement pl
  cross join reset_failure_before b
  where pl.user_id = '00000000-0000-0000-0000-000000000995'
  group by b.placement_rows, b.placements, b.placement_version),
  'failed reset restores every placement and persistent instance version');
select ok((select count(*)::integer = b.open_effect_rows
    and (select coalesce(max(h.revision), 0) from private.shop_effect_history h
      where h.user_id = '00000000-0000-0000-0000-000000000995') = b.max_effect_revision
    and (select ended_at from private.shop_cycle_effect_baseline cb
      where cb.user_id = '00000000-0000-0000-0000-000000000995'
        and cb.cycle_id = 'reset-original-cycle') is not distinct from b.old_baseline_ended_at
    and (select count(*)::integer from private.shop_planet_object_generation_baseline gb
      where gb.user_id = '00000000-0000-0000-0000-000000000995') = b.generation_baselines
  from private.shop_effect_history h cross join reset_failure_before b
  where h.user_id = '00000000-0000-0000-0000-000000000995' and h.ended_at is null
  group by b.open_effect_rows, b.max_effect_revision,
    b.old_baseline_ended_at, b.generation_baselines),
  'failed reset restores effect intervals, global revision, and generation baselines');
select is((select s.state_revision from private.shop_account_state s
  where s.user_id = '00000000-0000-0000-0000-000000000995'),
  (select b.state_revision from reset_failure_before b),
  'failed reset leaves the shop state revision unchanged');

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000995', true);
select set_config('shop_test.reset_result', public.reset_my_planet(
  '40000000-0000-0000-0000-000000000995', 'reset-original-cycle'
)::text, true);
select is((current_setting('shop_test.reset_result')::jsonb->'action'->>'status'),
  'reset', 'a valid reset creates the next server cycle');
select ok((current_setting('shop_test.reset_result')::jsonb
  ->'planet_state'->>'current_cycle_id') <> 'reset-original-cycle',
  'the server chooses a new cycle id');
select set_config('shop_test.reset_replay', public.reset_my_planet(
  '40000000-0000-0000-0000-000000000995', 'reset-original-cycle'
)::text, true);
select is((current_setting('shop_test.reset_replay')::jsonb->'action'->>'status'),
  'reset', 'replaying the request returns its original status');
select is((current_setting('shop_test.reset_replay')::jsonb->'action'->'state'->>'current_cycle_id'),
  (current_setting('shop_test.reset_replay')::jsonb->'planet_state'->>'current_cycle_id'),
  'the replay returns the latest canonical cycle in both states');
select set_config('shop_test.reset_conflict', public.reset_my_planet(
  '40000000-0000-0000-0000-000000000995', 'different-original-cycle'
)::text, true);
select is(current_setting('shop_test.reset_conflict')::jsonb->'action'->>'status',
  'request_conflict', 'reusing a reset request id with a different old cycle conflicts');
select is(current_setting('shop_test.reset_conflict')::jsonb
    ->'planet_state'->>'current_cycle_id',
  current_setting('shop_test.reset_result')::jsonb->'planet_state'->>'current_cycle_id',
  'a conflicting reset replay returns the latest canonical cycle without changing it');
select ok((current_setting('shop_test.reset_result')::jsonb
  ->'action'->'confirmed_quote') = 'null'::jsonb,
  'reset has no purchase quote');
reset role;
select is((select p.current_planet_tokens from public.planet_member_state p
  where p.user_id = '00000000-0000-0000-0000-000000000995'), 0::bigint,
  'reset clears only current-cycle raw tokens');
select is((select p.lifetime_tokens from public.planet_member_state p
  where p.user_id = '00000000-0000-0000-0000-000000000995'), 456::bigint,
  'reset preserves lifetime raw tokens');
select is((select d.current_planet_tokens from private.planet_device_state d
  where d.user_id = '00000000-0000-0000-0000-000000000995'
    and d.device_id = '30000000-0000-0000-0000-000000000995'), 0::bigint,
  'reset clears the device current-cycle raw total');
select is((select d.lifetime_tokens from private.planet_device_state d
  where d.user_id = '00000000-0000-0000-0000-000000000995'
    and d.device_id = '30000000-0000-0000-0000-000000000995'), 456::bigint,
  'reset preserves the device lifetime total');

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000995', true);
select lives_ok($$select public.upsert_my_planet_state(
  pg_temp.planet_state('Reset Contract', 'reset-original-cycle', now() - interval '48 hours'),
  pg_temp.reset_closed_device('reset-original-cycle', 600)
)$$, 'a known closed-cycle upload can advance raw lifetime history');
reset role;
select ok((select p.current_cycle_id <> 'reset-original-cycle'
  from public.planet_member_state p
  where p.user_id = '00000000-0000-0000-0000-000000000995'),
  'a late closed-cycle upload cannot reopen the current planet');
select is((select p.current_planet_tokens from public.planet_member_state p
  where p.user_id = '00000000-0000-0000-0000-000000000995'), 0::bigint,
  'a late closed-cycle upload does not restore current-cycle raw tokens');
select is((select p.lifetime_tokens from public.planet_member_state p
  where p.user_id = '00000000-0000-0000-0000-000000000995'), 600::bigint,
  'a late closed-cycle upload preserves only the larger lifetime total');
select is((select count(*)::integer from private.shop_activity_day a
  where a.user_id = '00000000-0000-0000-0000-000000000995'), 0,
  'closed-cycle raw recovery does not add current activity rewards');

select is((select w.amount from private.planet_wallet_credits w
  where w.user_id = '00000000-0000-0000-0000-000000000995'
    and w.previous_cycle_id = 'reset-original-cycle'), 123::bigint,
  'reset credits the original cycle raw tokens exactly once');
select is((select s.raw_tokens from private.shop_cycle_token_settlement s
  where s.user_id = '00000000-0000-0000-0000-000000000995'
    and s.cycle_id = 'reset-original-cycle'), 123::bigint,
  'the settlement records its authoritative raw-token amount');
select is((select count(*)::integer from private.planet_wallet_credits w
  where w.user_id = '00000000-0000-0000-0000-000000000995'
    and w.previous_cycle_id = 'reset-original-cycle'), 1,
  'replay does not duplicate the wallet credit');
select is((select count(*)::integer from private.shop_reset_request r
  where r.user_id = '00000000-0000-0000-0000-000000000995'
    and r.request_id = '40000000-0000-0000-0000-000000000995'), 1,
  'reset keeps one durable request receipt');
select is((select p.reset_available_at - p.last_reset_at
  from public.planet_member_state p
  where p.user_id = '00000000-0000-0000-0000-000000000995'), interval '23 hours 45 minutes 36 seconds',
  'reset freezes the server-placed cooldown effect');
select is((select p.reset_cooldown_bps from public.planet_member_state p
  where p.user_id = '00000000-0000-0000-0000-000000000995'), 100::smallint,
  'reset snapshots the active cooldown effect');
select is((select i.placement_version from private.shop_landscape_instance i
  where i.user_id = '00000000-0000-0000-0000-000000000995'
    and i.instance_id = '50000000-0000-0000-0000-000000000995'), 1::bigint,
  'reset retrieves the placed item and advances its persistent edit version');
select is((select count(*)::integer from private.shop_landscape_instance i
  where i.user_id = '00000000-0000-0000-0000-000000000995'
    and i.instance_id = '50000000-0000-0000-0000-000000000995'), 1,
  'reset preserves the landscape inventory instance');
select is((select count(*)::integer from private.shop_landscape_placement pl
  where pl.user_id = '00000000-0000-0000-0000-000000000995'), 0,
  'reset removes active placements');
select is((select e.sku from private.shop_avatar_equipment e
  where e.user_id = '00000000-0000-0000-0000-000000000995' and e.slot = 'head'),
  'avatar_explorer_hat', 'reset preserves avatar equipment');
select is((select e.version from private.shop_avatar_equipment e
  where e.user_id = '00000000-0000-0000-0000-000000000995' and e.slot = 'head'), 3::bigint,
  'reset preserves the avatar equipment version');
select is((select count(*)::integer from private.shop_avatar_owned a
  where a.user_id = '00000000-0000-0000-0000-000000000995'
    and a.sku = 'avatar_explorer_hat'), 1,
  'reset preserves avatar ownership');
select ok((select h.revision from private.shop_effect_history h
  join public.planet_member_state p on p.user_id = h.user_id
    and p.current_cycle_id = h.cycle_id
  where h.user_id = '00000000-0000-0000-0000-000000000995' and h.ended_at is null)
  > (select coalesce(max(h.revision), 0) from private.shop_effect_history h
    where h.user_id = '00000000-0000-0000-0000-000000000995'
      and h.cycle_id = 'reset-original-cycle'),
  'the next cycle effect revision is globally monotonic');

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000996', true);
select lives_ok($$select public.upsert_my_planet_state(
  pg_temp.planet_state('Positive Accrual', 'positive-accrual-cycle', now() - interval '72 hours'),
  pg_temp.reset_device('positive-accrual-cycle', 0,
    '30000000-0000-0000-0000-000000000996')
)$$, 'the positive-accrual account starts with a server-owned cycle');
select set_config('shop_test.positive_quote',
  public.quote_shop_action('{"kind":"purchase","sku":"land_pond"}'::jsonb)::text, true);
select set_config('shop_test.positive_purchase',
  public.apply_shop_action(jsonb_build_object(
    'kind', 'purchase', 'request_id', 'reset-positive-pond',
    'quote', current_setting('shop_test.positive_quote')::jsonb
  ))::text, true);
select is(current_setting('shop_test.positive_purchase')::jsonb->>'status',
  'purchased', 'the canonical 100-basis-point product is purchased through the shop RPC');
select set_config('shop_test.positive_instance',
  current_setting('shop_test.positive_purchase')::jsonb
    ->'state'->'landscape_instances'->0->>'instance_id', true);
select set_config('shop_test.positive_place',
  public.apply_shop_action(jsonb_build_object(
    'kind', 'place', 'request_id', 'reset-positive-place',
    'cycle_id', 'positive-accrual-cycle',
    'instance_id', current_setting('shop_test.positive_instance'),
    'expected_version', 0, 'x', 160, 'y', 200
  ))::text, true);
select is(current_setting('shop_test.positive_place')::jsonb->>'status',
  'placed', 'the purchased token-earning landscape instance is placed through the shop RPC');
reset role;
select is((select (h.effects->>'token_earning_bps')::integer
  from private.shop_effect_history h
  where h.user_id = '00000000-0000-0000-0000-000000000996'
    and h.cycle_id = 'positive-accrual-cycle' and h.ended_at is null),
  100, 'the server effect snapshot records the catalog 100-basis-point rate');
select set_config('shop_test.positive_cycle_started_at',
  (select p.cycle_started_at::text from public.planet_member_state p
    where p.user_id = '00000000-0000-0000-0000-000000000996'), true);
select set_config('shop_test.positive_effect_revision',
  (select h.revision::text from private.shop_effect_history h
    where h.user_id = '00000000-0000-0000-0000-000000000996'
      and h.cycle_id = 'positive-accrual-cycle' and h.ended_at is null), true);
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000996', true);
select lives_ok($$select pg_temp.positive_accrual_upload(
  100000, 'positive-accrual-cycle',
  current_setting('shop_test.positive_cycle_started_at')::timestamptz,
  current_setting('shop_test.positive_effect_revision')::bigint
)$$,
  'canonical raw usage uploads against the actual active server effect revision and clock');
reset role;
select is((select private.shop_cycle_token_bonus(
    '00000000-0000-0000-0000-000000000996', 'positive-accrual-cycle'
  )), 1000::bigint, '100000 raw tokens accrue exactly 1000 bonus tokens at 100 basis points');
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000996', true);
select set_config('shop_test.positive_reset', public.reset_my_planet(
  '40000000-0000-0000-0000-000000000996', 'positive-accrual-cycle'
)::text, true);
select is(current_setting('shop_test.positive_reset')::jsonb->'action'->>'status',
  'reset', 'reset settles the positively accrued cycle');
reset role;
select is((select w.amount from private.planet_wallet_credits w
  where w.user_id = '00000000-0000-0000-0000-000000000996'
    and w.previous_cycle_id = 'positive-accrual-cycle'),
  101000::bigint, 'reset credits 100000 base tokens plus the 1000-token server bonus');
select is((select s.raw_tokens from private.shop_cycle_token_settlement s
  where s.user_id = '00000000-0000-0000-0000-000000000996'
    and s.cycle_id = 'positive-accrual-cycle'), 100000::bigint,
  'the settlement records the canonical raw usage separately from its bonus');
select is((select s.bonus_tokens from private.shop_cycle_token_settlement s
  where s.user_id = '00000000-0000-0000-0000-000000000996'
    and s.cycle_id = 'positive-accrual-cycle'), 1000::bigint,
  'the settlement records the actual effect-derived bonus');
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000996', true);
select set_config('shop_test.positive_reset_replay', public.reset_my_planet(
  '40000000-0000-0000-0000-000000000996', 'positive-accrual-cycle'
)::text, true);
select is(current_setting('shop_test.positive_reset_replay')::jsonb->'action'->>'status',
  'reset', 'positive settlement replay returns the original reset status');
reset role;
select is((select count(*)::integer from private.planet_wallet_credits w
  where w.user_id = '00000000-0000-0000-0000-000000000996'
    and w.previous_cycle_id = 'positive-accrual-cycle'), 1,
  'reset receipt replay cannot issue the positive cycle credit twice');
select is((select count(*)::integer from private.shop_cycle_token_settlement s
  where s.user_id = '00000000-0000-0000-0000-000000000996'
    and s.cycle_id = 'positive-accrual-cycle'), 1,
  'reset receipt replay retains one immutable bonus settlement');

select * from finish();
rollback;
