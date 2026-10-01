-- Game rewards stay separate from imported/base wallet credits.
create table private.shop_game_reward (
  user_id uuid not null references auth.users(id) on delete cascade,
  trigger_key text not null check (char_length(trigger_key) between 1 and 160),
  kind text not null check (kind in ('era', 'streak')),
  cycle_id text,
  era_stage smallint,
  reward_date date,
  amount bigint not null check (amount between 0 and 10000000),
  effect_snapshot jsonb not null check (jsonb_typeof(effect_snapshot) = 'object'),
  created_at timestamptz not null default now(),
  primary key (user_id, trigger_key),
  check (cycle_id is null or char_length(cycle_id) between 1 and 80),
  check (
    (kind = 'era' and cycle_id is not null and era_stage is not null
      and era_stage between 1 and 4 and reward_date is null)
    or (kind = 'streak' and cycle_id is not null and era_stage is null
      and reward_date is not null)
  )
);
create unique index shop_game_reward_era_key
  on private.shop_game_reward(user_id, cycle_id, era_stage) where kind = 'era';
create unique index shop_game_reward_streak_key
  on private.shop_game_reward(user_id, reward_date) where kind = 'streak';
alter table private.shop_game_reward enable row level security;
revoke all on private.shop_game_reward from public, anon, authenticated, service_role;

-- Accounts already beyond an era at rollout keep that achievement without retroactive payout.
insert into private.shop_game_reward(
  user_id, trigger_key, kind, cycle_id, era_stage, reward_date,
  amount, effect_snapshot
)
select p.user_id,
  'era:' || p.current_cycle_id || ':' || s.stage::text,
  'era', p.current_cycle_id, s.stage::smallint, null,
  0, '{}'::jsonb
from public.planet_member_state p
cross join lateral pg_catalog.generate_series(1, p.stage) as s(stage)
where p.current_cycle_id is not null and p.stage between 1 and 4;

create function private.settle_shop_rewards(p_user_id uuid, p_cycle_id text)
returns void
language plpgsql security definer set search_path = '' as $$
declare
  v_current_cycle_id text;
  v_current_stage integer;
  v_last_stage integer;
  v_stage integer;
  v_server_time timestamptz;
  v_effect_snapshot jsonb;
  v_amount numeric;
begin
  if p_user_id is null or p_user_id is distinct from (select auth.uid()) then
    raise exception 'reward account access denied' using errcode = '42501';
  end if;
  if p_cycle_id is null or char_length(p_cycle_id) not between 1 and 80 then
    raise exception 'reward cycle is invalid' using errcode = '22023';
  end if;

  perform private.lock_shop_account(p_user_id);
  select p.current_cycle_id, p.stage::integer
  into v_current_cycle_id, v_current_stage
  from public.planet_member_state p
  where p.user_id = p_user_id
  for update;
  if not found or v_current_cycle_id is distinct from p_cycle_id
    or v_current_stage not between 0 and 4
  then
    raise exception 'reward cycle or server stage is invalid' using errcode = '23514';
  end if;

  v_server_time := clock_timestamp();

  select coalesce(max(r.era_stage), 0)::integer
  into v_last_stage
  from private.shop_game_reward r
  where r.user_id = p_user_id and r.kind = 'era' and r.cycle_id = p_cycle_id;

  if v_current_stage <= v_last_stage then
    return;
  end if;

  select h.effects into v_effect_snapshot
  from private.shop_effect_history h
  where h.user_id = p_user_id and h.cycle_id = p_cycle_id
    and h.started_at <= v_server_time
    and (h.ended_at is null or v_server_time < h.ended_at)
  order by h.revision desc
  limit 1;
  if not found then
    v_effect_snapshot := private.shop_active_effects(p_user_id, p_cycle_id);
  end if;
  v_amount := least(greatest(
    coalesce((v_effect_snapshot->>'era_reward_tokens')::numeric, 0::numeric), 0::numeric
  ), 10000000::numeric);
  if v_amount <> trunc(v_amount) then
    raise exception 'era reward effect must be a whole token amount' using errcode = '23514';
  end if;

  for v_stage in (v_last_stage + 1)..v_current_stage loop
    insert into private.shop_game_reward(
      user_id, trigger_key, kind, cycle_id, era_stage, reward_date,
      amount, effect_snapshot
    ) values (
      p_user_id, 'era:' || p_cycle_id || ':' || v_stage::text, 'era',
      p_cycle_id, v_stage::smallint, null, v_amount::bigint, v_effect_snapshot
    ) on conflict (user_id, trigger_key) do nothing;
  end loop;
end;
$$;
revoke all on function private.settle_shop_rewards(uuid, text)
  from public, anon, authenticated, service_role;
