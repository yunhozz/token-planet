-- Test payloads use the real public RPC; server totals must be derived from
-- device contributions, not the deliberately zero-valued client metrics.
create function pg_temp.planet_state(
  p_nickname text, p_cycle text default 'cycle-1', p_reset timestamptz default null,
  p_credits jsonb default '[]'::jsonb
) returns jsonb language sql as $$
  select jsonb_build_object(
    'version', 1, 'profile', jsonb_build_object('nickname', p_nickname, 'avatar', 'feminine'),
    'timezone', 'Asia/Seoul', 'current_cycle_id', p_cycle,
    'cycle_started_at_utc', coalesce(p_reset, '2026-09-26T00:00:00Z'::timestamptz),
    'last_reset_at_utc', p_reset, 'wallet_balance', 0, 'wallet_credits', p_credits,
    'current_planet_tokens', 0, 'lifetime_tokens', 0, 'growth_credit', 0,
    'stage', 0, 'progress_to_next', 0, 'incomplete', false,
    'can_reset', true, 'reset_available_at_utc', null, 'objects', '[]'::jsonb
  );
$$;
create function pg_temp.planet_device(
  p_device uuid, p_cycle text, p_tokens bigint, p_incomplete boolean default false,
  p_lifetime bigint default null
) returns jsonb language sql as $$
  select jsonb_build_object(
    'device_id', p_device, 'current_cycle_id', p_cycle,
    'current_planet_tokens', p_tokens, 'lifetime_tokens', coalesce(p_lifetime, p_tokens),
    'daily_tokens', case when p_tokens = 0 then '{}'::jsonb
      else jsonb_build_object('2026-09-26', p_tokens) end,
    'incomplete', p_incomplete
  );
$$;
