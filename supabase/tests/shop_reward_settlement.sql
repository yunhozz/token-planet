begin;
create extension if not exists pgtap with schema extensions;
select no_plan();
\ir fixtures/planet.inc

insert into auth.users(id) values ('00000000-0000-0000-0000-000000000984');

-- Five server-owned copies exceed the cap; the active snapshot must be capped at 10m.
insert into private.shop_landscape_instance(
  user_id, instance_id, sku, variation_index, seed, variation_version
) values
  ('00000000-0000-0000-0000-000000000984', '30000000-0000-0000-0000-000000000984', 'land_moonlets', 0, 'era-reward-0', 1),
  ('00000000-0000-0000-0000-000000000984', '30000000-0000-0000-0000-000000000985', 'land_moonlets', 1, 'era-reward-1', 1),
  ('00000000-0000-0000-0000-000000000984', '30000000-0000-0000-0000-000000000986', 'land_moonlets', 2, 'era-reward-2', 1),
  ('00000000-0000-0000-0000-000000000984', '30000000-0000-0000-0000-000000000987', 'land_moonlets', 3, 'era-reward-3', 1),
  ('00000000-0000-0000-0000-000000000984', '30000000-0000-0000-0000-000000000988', 'land_moonlets', 4, 'era-reward-4', 1);
insert into private.shop_landscape_placement(
  user_id, instance_id, cycle_id, x, y, version
) values
  ('00000000-0000-0000-0000-000000000984', '30000000-0000-0000-0000-000000000984', 'era-reward-cycle', 20, 40, 1),
  ('00000000-0000-0000-0000-000000000984', '30000000-0000-0000-0000-000000000985', 'era-reward-cycle', 22, 40, 1),
  ('00000000-0000-0000-0000-000000000984', '30000000-0000-0000-0000-000000000986', 'era-reward-cycle', 24, 40, 1),
  ('00000000-0000-0000-0000-000000000984', '30000000-0000-0000-0000-000000000987', 'era-reward-cycle', 26, 40, 1),
  ('00000000-0000-0000-0000-000000000984', '30000000-0000-0000-0000-000000000988', 'era-reward-cycle', 28, 40, 1);

create function pg_temp.upload_era_tokens(p_version bigint, p_tokens bigint)
returns jsonb
language sql as $$
  select public.upsert_my_planet_state(
    pg_temp.planet_state('Era Reward', 'era-reward-cycle'),
    jsonb_build_object(
      'device_id', '30000000-0000-0000-0000-000000000984',
      'current_cycle_id', 'era-reward-cycle',
      'lifetime_tokens', p_tokens,
      'current_planet_tokens', p_tokens,
      'daily_tokens', case when p_tokens = 0 then '{}'::jsonb
        else jsonb_build_object('2026-09-26', p_tokens) end,
      'incomplete', false,
      'canonical_version', p_version,
      'daily_segments', case when p_tokens = 0 then '[]'::jsonb else jsonb_build_array(
        jsonb_build_object('cycle_id', 'era-reward-cycle', 'date', '2026-09-26',
          'effect_revision', 0, 'tokens', p_tokens)
      ) end,
      'activity_days', case when p_tokens = 0 then '[]'::jsonb else jsonb_build_array(
        jsonb_build_object('cycle_id', 'era-reward-cycle', 'reward_date', '2026-09-26',
          'first_occurred_at_utc', '2026-09-26T13:00:00Z', 'tokens', p_tokens)
      ) end
    )
  );
$$;

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000984', true);
select lives_ok($$select pg_temp.upload_era_tokens(1, 0)$$,
  'canonical upload starts the account in server stage zero');
select is((public.get_my_planet_state()->>'stage')::integer, 0,
  'the initial server stage is zero');
reset role;

-- Register the server-computed active effect interval before the stage crossing.
insert into private.shop_effect_history(
  user_id, cycle_id, revision, started_at, ended_at, active_instance_ids, effects
)
select
  '00000000-0000-0000-0000-000000000984', 'era-reward-cycle', 1,
  now(), null,
  coalesce((
    select jsonb_agg(pl.instance_id::text order by pl.instance_id)
    from private.shop_landscape_placement pl
    where pl.user_id = '00000000-0000-0000-0000-000000000984'
      and pl.cycle_id = 'era-reward-cycle'
  ), '[]'::jsonb),
  private.shop_active_effects('00000000-0000-0000-0000-000000000984', 'era-reward-cycle');

set local role authenticated;
select lives_ok($$select pg_temp.upload_era_tokens(2, 3100000)$$,
  'canonical raw contribution crosses the stage-one threshold');
select is((public.get_my_planet_state()->>'stage')::integer, 1,
  'the canonical server stage advances from zero to one');
reset role;

select is((select (h.effects->>'era_reward_tokens')::bigint
  from private.shop_effect_history h
  where h.user_id = '00000000-0000-0000-0000-000000000984'
    and h.cycle_id = 'era-reward-cycle' and h.ended_at is null),
  10000000::bigint, 'the active server era effect snapshot is capped at ten million');

create function pg_temp.settle_era_twice(p_user_id uuid, p_cycle_id text)
returns jsonb
language plpgsql as $$
declare
  v_function_exists boolean := true;
  v_count bigint := 0;
  v_amount bigint := 0;
  v_snapshot_amount bigint := 0;
begin
  begin
    execute 'select private.settle_shop_rewards($1, $2)' using p_user_id, p_cycle_id;
    execute 'select private.settle_shop_rewards($1, $2)' using p_user_id, p_cycle_id;
  exception when undefined_function then
    v_function_exists := false;
  end;

  if v_function_exists then
    begin
      execute 'select count(*)::bigint, coalesce(sum(amount::numeric), 0)::bigint,
          coalesce(max((effect_snapshot->>''era_reward_tokens'')::bigint), 0)::bigint
        from private.shop_game_reward
        where user_id = $1 and trigger_key = $2'
      into v_count, v_amount, v_snapshot_amount
      using p_user_id, 'era:' || p_cycle_id || ':1';
    exception when undefined_table or undefined_column then
      v_count := 0;
      v_amount := 0;
      v_snapshot_amount := 0;
    end;
  end if;

  return jsonb_build_object(
    'settlement_function_exists', v_function_exists,
    'ledger_rows', v_count,
    'amount', v_amount,
    'snapshot_era_reward_tokens', v_snapshot_amount
  );
end;
$$;

select is(pg_temp.settle_era_twice(
  '00000000-0000-0000-0000-000000000984', 'era-reward-cycle'),
  '{"settlement_function_exists":true,"ledger_rows":1,"amount":10000000,"snapshot_era_reward_tokens":10000000}'::jsonb,
  'the first server stage crossing stores one capped era payout and replay preserves its key and amount');

select throws_ok($$select private.settle_shop_rewards(
  '00000000-0000-0000-0000-000000000983', 'era-reward-cycle')$$,
  '42501', null, 'settlement rejects a different account id');
select throws_ok($$select private.settle_shop_rewards(
  '00000000-0000-0000-0000-000000000984', 'stale-era-cycle')$$,
  '23514', null, 'settlement rejects a noncurrent cycle');
select ok(not has_function_privilege(
  'authenticated', 'private.settle_shop_rewards(uuid,text)', 'EXECUTE'),
  'authenticated clients cannot execute the settlement helper directly');
select ok(not has_function_privilege(
  'service_role', 'private.settle_shop_rewards(uuid,text)', 'EXECUTE'),
  'service role cannot execute the settlement helper directly');

create function pg_temp.refresh_era_effect_interval()
returns void
language plpgsql as $$
declare
  v_ended_at timestamptz := clock_timestamp();
  v_revision bigint;
  v_active_ids jsonb;
begin
  update private.shop_effect_history h
  set ended_at = v_ended_at
  where h.user_id = '00000000-0000-0000-0000-000000000984'
    and h.cycle_id = 'era-reward-cycle' and h.ended_at is null;
  select coalesce(max(h.revision), 0) + 1
  into v_revision
  from private.shop_effect_history h
  where h.user_id = '00000000-0000-0000-0000-000000000984'
    and h.cycle_id = 'era-reward-cycle';
  select coalesce(jsonb_agg(pl.instance_id::text order by pl.instance_id), '[]'::jsonb)
  into v_active_ids
  from private.shop_landscape_placement pl
  where pl.user_id = '00000000-0000-0000-0000-000000000984'
    and pl.cycle_id = 'era-reward-cycle';
  insert into private.shop_effect_history(
    user_id, cycle_id, revision, started_at, ended_at, active_instance_ids, effects
  ) values (
    '00000000-0000-0000-0000-000000000984', 'era-reward-cycle', v_revision,
    v_ended_at + interval '1 microsecond', null, v_active_ids,
    private.shop_active_effects('00000000-0000-0000-0000-000000000984', 'era-reward-cycle')
  );
end;
$$;

-- Lowering and restoring the server stage, or changing effects later, cannot rewrite a paid key.
update public.planet_member_state set stage = 0
where user_id = '00000000-0000-0000-0000-000000000984';
select private.settle_shop_rewards(
  '00000000-0000-0000-0000-000000000984', 'era-reward-cycle');
update public.planet_member_state set stage = 1
where user_id = '00000000-0000-0000-0000-000000000984';
select private.settle_shop_rewards(
  '00000000-0000-0000-0000-000000000984', 'era-reward-cycle');
delete from private.shop_landscape_placement
where user_id = '00000000-0000-0000-0000-000000000984'
  and cycle_id = 'era-reward-cycle';
select pg_temp.refresh_era_effect_interval();
select private.settle_shop_rewards(
  '00000000-0000-0000-0000-000000000984', 'era-reward-cycle');
select is((
  select jsonb_build_object(
    'count', count(*),
    'amount', coalesce(sum(r.amount::numeric), 0)::bigint,
    'snapshot_era_reward_tokens', coalesce(max((r.effect_snapshot->>'era_reward_tokens')::bigint), 0)
  )
  from private.shop_game_reward r
  where r.user_id = '00000000-0000-0000-0000-000000000984'
    and r.trigger_key = 'era:era-reward-cycle:1'
), '{"count":1,"amount":10000000,"snapshot_era_reward_tokens":10000000}'::jsonb,
  'stage decline, rebound, and later effect removal preserve the original award and snapshot');

-- A first jump across several server stages records one durable key for each stage.
delete from private.shop_game_reward
where user_id = '00000000-0000-0000-0000-000000000984';
insert into private.shop_landscape_placement(
  user_id, instance_id, cycle_id, x, y, version
) values
  ('00000000-0000-0000-0000-000000000984', '30000000-0000-0000-0000-000000000984', 'era-reward-cycle', 20, 40, 2),
  ('00000000-0000-0000-0000-000000000984', '30000000-0000-0000-0000-000000000985', 'era-reward-cycle', 22, 40, 2),
  ('00000000-0000-0000-0000-000000000984', '30000000-0000-0000-0000-000000000986', 'era-reward-cycle', 24, 40, 2),
  ('00000000-0000-0000-0000-000000000984', '30000000-0000-0000-0000-000000000987', 'era-reward-cycle', 26, 40, 2),
  ('00000000-0000-0000-0000-000000000984', '30000000-0000-0000-0000-000000000988', 'era-reward-cycle', 28, 40, 2);
select pg_temp.refresh_era_effect_interval();
update public.planet_member_state set stage = 0
where user_id = '00000000-0000-0000-0000-000000000984';
update public.planet_member_state set stage = 3
where user_id = '00000000-0000-0000-0000-000000000984';
select private.settle_shop_rewards(
  '00000000-0000-0000-0000-000000000984', 'era-reward-cycle');
select is((select count(*)::bigint from private.shop_game_reward
  where user_id = '00000000-0000-0000-0000-000000000984'
    and kind = 'era' and cycle_id = 'era-reward-cycle'), 3::bigint,
  'a first server jump to stage three records three era keys');
select is((select array_agg(era_stage order by era_stage)
  from private.shop_game_reward
  where user_id = '00000000-0000-0000-0000-000000000984'
    and kind = 'era' and cycle_id = 'era-reward-cycle'), array[1,2,3]::smallint[],
  'the multi-stage jump writes each stage marker');
select ok((select bool_and(amount = 10000000
      and (effect_snapshot->>'era_reward_tokens')::bigint = 10000000)
  from private.shop_game_reward
  where user_id = '00000000-0000-0000-0000-000000000984'
    and kind = 'era' and cycle_id = 'era-reward-cycle'),
  'each stage in the jump uses the same capped crossing-time effect snapshot');
update public.planet_member_state set stage = 4
where user_id = '00000000-0000-0000-0000-000000000984';
select private.settle_shop_rewards(
  '00000000-0000-0000-0000-000000000984', 'era-reward-cycle');
select is((select count(*)::bigint from private.shop_game_reward
  where user_id = '00000000-0000-0000-0000-000000000984'
    and kind = 'era' and cycle_id = 'era-reward-cycle'), 4::bigint,
  'the stage-three-to-four crossing adds one new key');

-- A zero-effect crossing is still final after a later placement.
delete from private.shop_game_reward
where user_id = '00000000-0000-0000-0000-000000000984';
delete from private.shop_landscape_placement
where user_id = '00000000-0000-0000-0000-000000000984'
  and cycle_id = 'era-reward-cycle';
select pg_temp.refresh_era_effect_interval();
update public.planet_member_state set stage = 0
where user_id = '00000000-0000-0000-0000-000000000984';
update public.planet_member_state set stage = 1
where user_id = '00000000-0000-0000-0000-000000000984';
select private.settle_shop_rewards(
  '00000000-0000-0000-0000-000000000984', 'era-reward-cycle');
select is((select amount from private.shop_game_reward
  where user_id = '00000000-0000-0000-0000-000000000984'
    and trigger_key = 'era:era-reward-cycle:1'), 0::bigint,
  'crossing without an active era effect stores a zero-value marker');
insert into private.shop_landscape_placement(
  user_id, instance_id, cycle_id, x, y, version
) values
  ('00000000-0000-0000-0000-000000000984', '30000000-0000-0000-0000-000000000984', 'era-reward-cycle', 20, 40, 3);
select pg_temp.refresh_era_effect_interval();
select private.settle_shop_rewards(
  '00000000-0000-0000-0000-000000000984', 'era-reward-cycle');
select is((select jsonb_build_object(
      'count', count(*), 'amount', coalesce(sum(amount::numeric), 0)::bigint,
      'snapshot_era_reward_tokens', coalesce(max((effect_snapshot->>'era_reward_tokens')::bigint), 0)
    )
  from private.shop_game_reward
  where user_id = '00000000-0000-0000-0000-000000000984'
    and trigger_key = 'era:era-reward-cycle:1'),
  '{"count":1,"amount":0,"snapshot_era_reward_tokens":0}'::jsonb,
  'a later placement cannot retroactively change the zero-value era marker');

select * from finish();
rollback;
