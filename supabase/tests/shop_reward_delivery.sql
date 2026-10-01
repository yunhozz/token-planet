begin;
create extension if not exists pgtap with schema extensions;
select no_plan();
\ir fixtures/planet.inc

insert into auth.users(id) values
  ('00000000-0000-0000-0000-000000000990'),
  ('00000000-0000-0000-0000-000000000991'),
  ('00000000-0000-0000-0000-000000000992'),
  ('00000000-0000-0000-0000-000000000993');

-- This account has a server-established UTC reward timezone before its first
-- planet arrives; that value must remain frozen when the planet uses Seoul.
insert into private.shop_account_state(
  user_id, state_revision, reward_timezone, reward_timezone_initialized
) values ('00000000-0000-0000-0000-000000000990', 0, 'UTC', true);

create function pg_temp.streak_planet_state(p_timezone text)
returns jsonb language sql as $$
  select jsonb_set(
    pg_temp.planet_state('Streak Reward', 'streak-reward-cycle'),
    '{timezone}', to_jsonb(p_timezone), true
  );
$$;

create function pg_temp.upload_streak_zero()
returns jsonb language sql as $$
  select public.upsert_my_planet_state(
    pg_temp.streak_planet_state('Asia/Seoul'),
    jsonb_build_object(
      'device_id', '30000000-0000-0000-0000-000000000990',
      'current_cycle_id', 'streak-reward-cycle',
      'lifetime_tokens', 0,
      'current_planet_tokens', 0,
      'daily_tokens', '{}'::jsonb,
      'incomplete', false,
      'canonical_version', 0,
      'daily_segments', '[]'::jsonb,
      'activity_days', '[]'::jsonb
    )
  );
$$;

create function pg_temp.upload_first_reward_timezone()
returns jsonb language sql as $$
  select public.upsert_my_planet_state(
    jsonb_set(
      pg_temp.planet_state('First Reward Timezone', 'first-reward-timezone-cycle'),
      '{timezone}', '"Asia/Seoul"'::jsonb, true
    ),
    jsonb_build_object(
      'device_id', '30000000-0000-0000-0000-000000000996',
      'current_cycle_id', 'first-reward-timezone-cycle',
      'lifetime_tokens', 0,
      'current_planet_tokens', 0,
      'daily_tokens', '{}'::jsonb,
      'incomplete', false,
      'canonical_version', 0,
      'daily_segments', '[]'::jsonb,
      'activity_days', '[]'::jsonb
    )
  );
$$;

create function pg_temp.upload_streak_two_days(p_version bigint)
returns jsonb language sql as $$
  select public.upsert_my_planet_state(
    pg_temp.streak_planet_state('Asia/Seoul'),
    jsonb_build_object(
      'device_id', '30000000-0000-0000-0000-000000000990',
      'current_cycle_id', 'streak-reward-cycle',
      'lifetime_tokens', 200,
      'current_planet_tokens', 200,
      'daily_tokens', '{"2026-09-27":100,"2026-09-28":100}'::jsonb,
      'incomplete', false,
      'canonical_version', p_version,
      'daily_segments', jsonb_build_array(
        jsonb_build_object('cycle_id', 'streak-reward-cycle', 'date', '2026-09-27',
          'effect_revision', 0, 'tokens', 100),
        jsonb_build_object('cycle_id', 'streak-reward-cycle', 'date', '2026-09-28',
          'effect_revision', 1, 'tokens', 100)
      ),
      'activity_days', jsonb_build_array(
        jsonb_build_object('cycle_id', 'streak-reward-cycle', 'reward_date', '2026-09-27',
          'first_occurred_at_utc', '2026-09-27T13:00:00Z', 'tokens', 100),
        jsonb_build_object('cycle_id', 'streak-reward-cycle', 'reward_date', '2026-09-28',
          'first_occurred_at_utc', '2026-09-28T13:00:00Z', 'tokens', 100)
      )
    )
  );
$$;

create function pg_temp.upload_streak_late_dates(
  p_version bigint, p_include_previous boolean
)
returns jsonb language sql as $$
  select public.upsert_my_planet_state(
    pg_temp.streak_planet_state('Asia/Seoul'),
    jsonb_build_object(
      'device_id', '30000000-0000-0000-0000-000000000990',
      'current_cycle_id', 'streak-reward-cycle',
      'lifetime_tokens', case when p_include_previous then 400 else 300 end,
      'current_planet_tokens', case when p_include_previous then 400 else 300 end,
      'daily_tokens', case when p_include_previous then
        '{"2026-09-27":100,"2026-09-28":100,"2026-09-29":100,"2026-09-30":100}'::jsonb
        else '{"2026-09-27":100,"2026-09-28":100,"2026-09-30":100}'::jsonb end,
      'incomplete', false,
      'canonical_version', p_version,
      'daily_segments',
        jsonb_build_array(
          jsonb_build_object('cycle_id', 'streak-reward-cycle', 'date', '2026-09-27',
            'effect_revision', 0, 'tokens', 100),
          jsonb_build_object('cycle_id', 'streak-reward-cycle', 'date', '2026-09-28',
            'effect_revision', 1, 'tokens', 100)
        )
        || case when p_include_previous then jsonb_build_array(
          jsonb_build_object('cycle_id', 'streak-reward-cycle', 'date', '2026-09-29',
            'effect_revision', 1, 'tokens', 100)
        ) else '[]'::jsonb end
        || jsonb_build_array(
          jsonb_build_object('cycle_id', 'streak-reward-cycle', 'date', '2026-09-30',
            'effect_revision', 1, 'tokens', 100)
        ),
      'activity_days',
        jsonb_build_array(
          jsonb_build_object('cycle_id', 'streak-reward-cycle', 'reward_date', '2026-09-27',
            'first_occurred_at_utc', '2026-09-27T13:00:00Z', 'tokens', 100),
          jsonb_build_object('cycle_id', 'streak-reward-cycle', 'reward_date', '2026-09-28',
            'first_occurred_at_utc', '2026-09-28T13:00:00Z', 'tokens', 100)
        )
        || case when p_include_previous then jsonb_build_array(
          jsonb_build_object('cycle_id', 'streak-reward-cycle', 'reward_date', '2026-09-29',
            'first_occurred_at_utc', '2026-09-29T13:00:00Z', 'tokens', 100)
        ) else '[]'::jsonb end
        || jsonb_build_array(
          jsonb_build_object('cycle_id', 'streak-reward-cycle', 'reward_date', '2026-09-30',
            'first_occurred_at_utc', '2026-09-30T13:00:00Z', 'tokens', 100)
        )
    )
  );
$$;

create function pg_temp.upload_mixed_cycle_activity()
returns jsonb language sql as $$
  select public.upsert_my_planet_state(
    pg_temp.planet_state('Mixed Cycle Activity', 'delivery-new-cycle'),
    jsonb_build_object(
      'device_id', '30000000-0000-0000-0000-000000000999',
      'current_cycle_id', 'delivery-new-cycle',
      'lifetime_tokens', 100,
      'current_planet_tokens', 100,
      'daily_tokens', '{"2026-09-30":100}'::jsonb,
      'incomplete', false,
      'canonical_version', 1,
      'daily_segments', '[{"cycle_id":"delivery-new-cycle","date":"2026-09-30","effect_revision":1,"tokens":100}]'::jsonb,
      'activity_days', '[{"cycle_id":"delivery-new-cycle","reward_date":"2026-09-30","first_occurred_at_utc":"2026-09-30T12:00:00Z","tokens":100}]'::jsonb
    )
  );
$$;

create function pg_temp.upload_reward_summary(p_version bigint, p_tokens bigint)
returns jsonb language sql as $$
  select public.upsert_my_planet_state(
    pg_temp.planet_state('Reward Summary', 'reward-summary-cycle'),
    jsonb_build_object(
      'device_id', '30000000-0000-0000-0000-000000000993',
      'current_cycle_id', 'reward-summary-cycle',
      'lifetime_tokens', p_tokens,
      'current_planet_tokens', p_tokens,
      'daily_tokens', case when p_tokens = 0 then '{}'::jsonb
        else '{"2026-09-30":10000}'::jsonb end,
      'incomplete', false,
      'canonical_version', p_version,
      'daily_segments', case when p_tokens = 0 then '[]'::jsonb else
        '[{"cycle_id":"reward-summary-cycle","date":"2026-09-30","effect_revision":1,"tokens":10000}]'::jsonb end,
      'activity_days', case when p_tokens = 0 then '[]'::jsonb else
        '[{"cycle_id":"reward-summary-cycle","reward_date":"2026-09-30","first_occurred_at_utc":"2026-09-30T13:00:00Z","tokens":10000}]'::jsonb end
    )
  );
$$;

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000991', true);
select is((public.get_my_shop_state()->'reward_state'->>'reward_timezone'), 'UTC',
  'pre-planet shop initialization keeps its timezone provisional');
select lives_ok($$select pg_temp.upload_first_reward_timezone()$$,
  'first canonical planet upload sets the provisional reward timezone');
reset role;
select is((select s.reward_timezone from private.shop_account_state s
  where s.user_id = '00000000-0000-0000-0000-000000000991'), 'Asia/Seoul',
  'first planet timezone is frozen as the account reward timezone');

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000990', true);
select lives_ok($$select pg_temp.upload_streak_zero()$$,
  'canonical upload accepts a planet timezone distinct from the frozen reward timezone');
reset role;

-- Five server-owned meteor placements exceed the 500k streak cap.
insert into private.shop_landscape_instance(
  user_id, instance_id, sku, variation_index, seed, variation_version
) values
  ('00000000-0000-0000-0000-000000000990', '30000000-0000-0000-0000-000000000991', 'land_meteors', 0, 'streak-0', 1),
  ('00000000-0000-0000-0000-000000000990', '30000000-0000-0000-0000-000000000992', 'land_meteors', 1, 'streak-1', 1),
  ('00000000-0000-0000-0000-000000000990', '30000000-0000-0000-0000-000000000993', 'land_meteors', 2, 'streak-2', 1),
  ('00000000-0000-0000-0000-000000000990', '30000000-0000-0000-0000-000000000994', 'land_meteors', 3, 'streak-3', 1),
  ('00000000-0000-0000-0000-000000000990', '30000000-0000-0000-0000-000000000995', 'land_meteors', 4, 'streak-4', 1);
insert into private.shop_landscape_placement(
  user_id, instance_id, cycle_id, x, y, version
) values
  ('00000000-0000-0000-0000-000000000990', '30000000-0000-0000-0000-000000000991', 'streak-reward-cycle', 20, 70, 1),
  ('00000000-0000-0000-0000-000000000990', '30000000-0000-0000-0000-000000000992', 'streak-reward-cycle', 24, 70, 1),
  ('00000000-0000-0000-0000-000000000990', '30000000-0000-0000-0000-000000000993', 'streak-reward-cycle', 28, 70, 1),
  ('00000000-0000-0000-0000-000000000990', '30000000-0000-0000-0000-000000000994', 'streak-reward-cycle', 32, 70, 1),
  ('00000000-0000-0000-0000-000000000990', '30000000-0000-0000-0000-000000000995', 'streak-reward-cycle', 36, 70, 1);
insert into private.shop_effect_history(
  user_id, cycle_id, revision, started_at, ended_at, active_instance_ids, effects
)
select '00000000-0000-0000-0000-000000000990', 'streak-reward-cycle', 1,
  '2026-09-28T12:00:00Z', null,
  coalesce((
    select jsonb_agg(pl.instance_id::text order by pl.instance_id)
    from private.shop_landscape_placement pl
    where pl.user_id = '00000000-0000-0000-0000-000000000990'
      and pl.cycle_id = 'streak-reward-cycle'
  ), '[]'::jsonb),
  private.shop_active_effects('00000000-0000-0000-0000-000000000990', 'streak-reward-cycle');
select is((private.shop_active_effects(
  '00000000-0000-0000-0000-000000000990', 'streak-reward-cycle'
)->>'streak_reward_tokens')::bigint, 500000::bigint,
  'fixture has five active streak items and the server effect is capped at 500k');

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000990', true);
select pg_temp.upload_streak_two_days(1) as first_upload \gset
select is((:'first_upload'::jsonb->>'wallet_balance')::bigint, 500000::bigint,
  'canonical planet response includes available game reward credits');
select is((:'first_upload'::jsonb->>'current_planet_tokens')::bigint, 200::bigint,
  'streak rewards do not inflate raw current-cycle token totals');
select is(jsonb_array_length(:'first_upload'::jsonb->'wallet_credits'), 0,
  'game reward credits do not rewrite the separate base wallet-credit rows');
select is((public.get_my_shop_state()->>'available_balance')::bigint, 500000::bigint,
  'available shop balance includes the separate game reward ledger');
select is(public.get_my_shop_state()->'reward_state',
  '{"reward_timezone":"UTC","settled_cycle_tokens":0,"era_reward_tokens":0,"streak_reward_tokens":500000}'::jsonb,
  'shop reward state reports live timezone and streak totals');
select (public.get_my_shop_state()->>'state_revision')::bigint as first_state_revision \gset
select is(:'first_state_revision'::bigint, 1::bigint,
  'new streak payout advances financial state revision once');
reset role;

select is((select s.reward_timezone from private.shop_account_state s
  where s.user_id = '00000000-0000-0000-0000-000000000990'), 'UTC',
  'planet growth timezone differs from the frozen account reward timezone');
select is((select count(*)::bigint from private.shop_game_reward
  where user_id = '00000000-0000-0000-0000-000000000990'
    and kind = 'streak' and reward_date = '2026-09-28'), 1::bigint,
  'the second of two consecutive positive reward dates creates one streak key');
select is((select amount from private.shop_game_reward
  where user_id = '00000000-0000-0000-0000-000000000990'
    and kind = 'streak' and reward_date = '2026-09-28'), 500000::bigint,
  'the server historical effect is capped to a 500k daily streak reward');

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000990', true);
select pg_temp.upload_streak_two_days(1) as replay_upload \gset
reset role;
select is((select count(*)::bigint from private.shop_game_reward
  where user_id = '00000000-0000-0000-0000-000000000990'
    and kind = 'streak' and reward_date = '2026-09-28'), 1::bigint,
  'same canonical upload replay does not add a second date-keyed reward');
select is((public.get_my_shop_state()->>'state_revision')::bigint, :'first_state_revision'::bigint,
  'same canonical upload replay leaves the financial state revision unchanged');
select is((:'replay_upload'::jsonb->>'wallet_balance')::bigint, 500000::bigint,
  'same canonical upload replay returns the refreshed wallet total');

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000990', true);
select pg_temp.upload_streak_late_dates(2, false) as day_before_previous \gset
select is((:'day_before_previous'::jsonb->>'wallet_balance')::bigint, 500000::bigint,
  'a positive day without its previous day does not pay a streak reward');
select pg_temp.upload_streak_late_dates(3, true) as late_previous_day \gset
reset role;
select is((select count(*)::bigint from private.shop_game_reward
  where user_id = '00000000-0000-0000-0000-000000000990'
    and kind = 'streak' and reward_date = '2026-09-30'), 1::bigint,
  'late canonical previous-day activity settles one reward for the date');
select is((select amount from private.shop_game_reward
  where user_id = '00000000-0000-0000-0000-000000000990'
    and kind = 'streak' and reward_date = '2026-09-30'), 500000::bigint,
  'late previous-day activity upgrades the date from zero to its historical payout');
select is((:'late_previous_day'::jsonb->>'wallet_balance')::bigint, 1500000::bigint,
  'late activity returns the combined current wallet balance');

-- The account-level canonical activity day can belong to a closed cycle when
-- another device reports the same reward date later in the current cycle.
insert into private.shop_account_state(
  user_id, reward_timezone, reward_timezone_initialized
) values ('00000000-0000-0000-0000-000000000992', 'UTC', true);
insert into public.planet_member_state(
  user_id, nickname, avatar, timezone, current_cycle_id, cycle_started_at,
  current_planet_tokens, lifetime_tokens, growth_credit, stage, progress_to_next,
  incomplete, objects
) values (
  '00000000-0000-0000-0000-000000000992', 'Mixed Cycle Activity', 'feminine', 'Asia/Seoul',
  'delivery-new-cycle', '2026-09-26T00:00:00Z', 0, 0, 0, 0, 0, false, '[]'::jsonb
);
insert into private.shop_cycle_effect_baseline(user_id, cycle_id, started_at) values
  ('00000000-0000-0000-0000-000000000992', 'delivery-old-cycle', '2026-09-26T00:00:00Z'),
  ('00000000-0000-0000-0000-000000000992', 'delivery-new-cycle', '2026-09-26T00:00:00Z');
insert into private.shop_landscape_instance(
  user_id, instance_id, sku, variation_index, seed, variation_version
) values (
  '00000000-0000-0000-0000-000000000992',
  '30000000-0000-0000-0000-000000000997', 'land_lantern', 0, 'mixed-cycle-lantern', 1
);
insert into private.shop_landscape_placement(
  user_id, instance_id, cycle_id, x, y, version
) values (
  '00000000-0000-0000-0000-000000000992',
  '30000000-0000-0000-0000-000000000997', 'delivery-new-cycle', 20, 70, 1
);
insert into private.shop_effect_history(
  user_id, cycle_id, revision, started_at, ended_at, active_instance_ids, effects
)
select '00000000-0000-0000-0000-000000000992', 'delivery-new-cycle', 1,
  '2026-09-26T00:00:00Z', null,
  '["30000000-0000-0000-0000-000000000997"]'::jsonb,
  private.shop_active_effects('00000000-0000-0000-0000-000000000992', 'delivery-new-cycle');
insert into private.shop_device_contribution_state(
  user_id, device_id, canonical_version, canonical_payload
) values
  ('00000000-0000-0000-0000-000000000992', '30000000-0000-0000-0000-000000000997', 0, '{}'::jsonb),
  ('00000000-0000-0000-0000-000000000992', '30000000-0000-0000-0000-000000000998', 0, '{}'::jsonb);
insert into private.shop_device_activity_day(
  user_id, device_id, reward_date, cycle_id, first_occurred_at_utc, canonical_version, tokens
) values
  ('00000000-0000-0000-0000-000000000992', '30000000-0000-0000-0000-000000000997', '2026-09-30',
    'delivery-old-cycle', '2026-09-30T10:00:00Z', 0, 100),
  ('00000000-0000-0000-0000-000000000992', '30000000-0000-0000-0000-000000000998', '2026-09-29',
    'delivery-new-cycle', '2026-09-29T12:00:00Z', 0, 100);

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000992', true);
select pg_temp.upload_mixed_cycle_activity() as mixed_cycle_response \gset
reset role;
select is((select cycle_id from private.shop_activity_day
  where user_id = '00000000-0000-0000-0000-000000000992' and reward_date = '2026-09-30'),
  'delivery-old-cycle', 'same-date account activity keeps its earliest closed-cycle source');
select is((select count(*)::bigint from private.shop_game_reward
  where user_id = '00000000-0000-0000-0000-000000000992'
    and kind = 'streak' and reward_date = '2026-09-30'), 0::bigint,
  'a later current-cycle device cannot pay from an earlier closed-cycle first event');
select is((:'mixed_cycle_response'::jsonb->>'wallet_balance')::bigint, 0::bigint,
  'closed-cycle first activity does not add wallet credit');

-- A positive earning rate is only a forecast until reset records a settlement;
-- era reward summaries are scoped to the active cycle, while balance is account-wide.
insert into private.shop_account_state(
  user_id, reward_timezone, reward_timezone_initialized
) values ('00000000-0000-0000-0000-000000000993', 'UTC', true);
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000993', true);
select lives_ok($$select pg_temp.upload_reward_summary(0, 0)$$,
  'reward summary account starts with an empty current cycle');
reset role;
insert into private.shop_landscape_instance(
  user_id, instance_id, sku, variation_index, seed, variation_version
) values (
  '00000000-0000-0000-0000-000000000993',
  '30000000-0000-0000-0000-000000000994', 'land_pond', 0, 'summary-pond', 1
);
insert into private.shop_landscape_placement(
  user_id, instance_id, cycle_id, x, y, version
) values (
  '00000000-0000-0000-0000-000000000993',
  '30000000-0000-0000-0000-000000000994', 'reward-summary-cycle', 20, 70, 1
);
insert into private.shop_effect_history(
  user_id, cycle_id, revision, started_at, ended_at, active_instance_ids, effects
)
select '00000000-0000-0000-0000-000000000993', 'reward-summary-cycle', 1,
  '2026-09-26T00:00:00Z', null,
  '["30000000-0000-0000-0000-000000000994"]'::jsonb,
  private.shop_active_effects('00000000-0000-0000-0000-000000000993', 'reward-summary-cycle');
insert into private.shop_game_reward(
  user_id, trigger_key, kind, cycle_id, era_stage, reward_date, amount, effect_snapshot
) values
  ('00000000-0000-0000-0000-000000000993', 'era:reward-old-cycle:1', 'era',
    'reward-old-cycle', 1, null, 11, '{}'::jsonb),
  ('00000000-0000-0000-0000-000000000993', 'era:reward-summary-cycle:1', 'era',
    'reward-summary-cycle', 1, null, 7, '{}'::jsonb);
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000993', true);
select pg_temp.upload_reward_summary(1, 10000) as reward_summary_planet \gset
reset role;
select is((private.shop_cycle_token_bonus(
  '00000000-0000-0000-0000-000000000993', 'reward-summary-cycle'
)), 100::bigint, 'fixture has a positive but not yet settled cycle earning amount');
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000993', true);
select is((:'reward_summary_planet'::jsonb->>'wallet_balance')::bigint, 18::bigint,
  'planet response retains account-wide current and past era wallet rewards');
select public.get_my_shop_state() as reward_summary_state \gset
select is((:'reward_summary_state'::jsonb->'reward_state'->>'settled_cycle_tokens')::bigint,
  0::bigint, 'shop state reports zero until the cycle amount is actually settled');
select is((:'reward_summary_state'::jsonb->'reward_state'->>'era_reward_tokens')::bigint,
  7::bigint, 'shop era total includes only rewards from the current cycle');
reset role;

select * from finish();
rollback;
