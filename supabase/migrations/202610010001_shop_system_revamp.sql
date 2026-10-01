create table private.shop_products (
  sku text primary key check (sku ~ '^[a-z][a-z0-9_]{0,63}$'),
  category text not null check (category in ('landscape', 'avatar')),
  display_name text not null check (char_length(display_name) between 1 and 80),
  price bigint not null check (price > 0),
  catalog_revision integer not null check (catalog_revision > 0),
  purchasable boolean not null,
  placement_zone text check (placement_zone in ('ground', 'sky')),
  avatar_slot text check (avatar_slot in ('head', 'outfit', 'face', 'back')),
  effect_type text check (effect_type in (
    'token_earning', 'civilization_growth', 'shop_discount', 'reset_cooldown',
    'natural_removal_discount', 'era_reward', 'streak_reward'
  )),
  effect_value bigint not null check (effect_value >= 0),
  sort_order smallint not null unique check (sort_order between 1 and 48),
  unique (sku, avatar_slot),
  check (
    (category = 'landscape' and placement_zone is not null and avatar_slot is null
      and effect_type is not null and effect_value > 0)
    or
    (category = 'avatar' and placement_zone is null and avatar_slot is not null
      and effect_type is null and effect_value = 0)
  )
);

create function private.prevent_shop_product_identity_change()
returns trigger
language plpgsql set search_path = '' as $$
begin
  if new.sku is distinct from old.sku
    or new.category is distinct from old.category
    or new.display_name is distinct from old.display_name
    or new.price is distinct from old.price
    or new.catalog_revision is distinct from old.catalog_revision
    or new.placement_zone is distinct from old.placement_zone
    or new.avatar_slot is distinct from old.avatar_slot
    or new.effect_type is distinct from old.effect_type
    or new.effect_value is distinct from old.effect_value
    or new.sort_order is distinct from old.sort_order
  then
    raise exception 'shop product identity is immutable' using errcode = '23514';
  end if;
  return new;
end;
$$;

create trigger shop_product_identity_is_immutable
before update on private.shop_products
for each row execute function private.prevent_shop_product_identity_change();

insert into private.shop_products(
  sku, category, display_name, price, catalog_revision, purchasable,
  placement_zone, avatar_slot, effect_type, effect_value, sort_order
) values
  ('land_pond', 'landscape', '연못', 5000000, 1, true, 'ground', null, 'token_earning', 100, 1),
  ('land_well', 'landscape', '우물', 15000000, 1, true, 'ground', null, 'token_earning', 150, 2),
  ('land_greenhouse', 'landscape', '온실', 40000000, 1, true, 'ground', null, 'token_earning', 200, 3),
  ('land_reservoir', 'landscape', '저수지', 100000000, 1, true, 'ground', null, 'token_earning', 300, 4),
  ('land_crystal', 'landscape', '수정탑', 5000000, 1, true, 'ground', null, 'civilization_growth', 100, 5),
  ('land_school', 'landscape', '학교', 15000000, 1, true, 'ground', null, 'civilization_growth', 150, 6),
  ('land_observatory', 'landscape', '천문대', 40000000, 1, true, 'ground', null, 'civilization_growth', 200, 7),
  ('land_laboratory', 'landscape', '연구소', 100000000, 1, true, 'ground', null, 'civilization_growth', 300, 8),
  ('land_market', 'landscape', '시장', 5000000, 1, true, 'ground', null, 'shop_discount', 100, 9),
  ('land_trading_post', 'landscape', '교역소', 15000000, 1, true, 'ground', null, 'shop_discount', 150, 10),
  ('land_freight', 'landscape', '화물 터미널', 40000000, 1, true, 'ground', null, 'shop_discount', 200, 11),
  ('land_bazaar', 'landscape', '대형 상가', 100000000, 1, true, 'ground', null, 'shop_discount', 300, 12),
  ('land_rover', 'landscape', '탐사 로버', 5000000, 1, true, 'ground', null, 'reset_cooldown', 100, 13),
  ('land_clocktower', 'landscape', '시계탑', 15000000, 1, true, 'ground', null, 'reset_cooldown', 150, 14),
  ('land_launchpad', 'landscape', '발사대', 40000000, 1, true, 'ground', null, 'reset_cooldown', 200, 15),
  ('land_portal', 'landscape', '포털', 100000000, 1, true, 'ground', null, 'reset_cooldown', 300, 16),
  ('land_toolbox', 'landscape', '정리 도구함', 5000000, 1, true, 'ground', null, 'natural_removal_discount', 100, 17),
  ('land_excavator', 'landscape', '굴착기', 15000000, 1, true, 'ground', null, 'natural_removal_discount', 150, 18),
  ('land_cutter', 'landscape', '암석 절단기', 40000000, 1, true, 'ground', null, 'natural_removal_discount', 200, 19),
  ('land_recycler', 'landscape', '재활용 로봇', 100000000, 1, true, 'ground', null, 'natural_removal_discount', 300, 20),
  ('land_flag', 'landscape', '깃발', 5000000, 1, true, 'ground', null, 'era_reward', 500000, 21),
  ('land_thin_ring', 'landscape', '얇은 고리', 15000000, 1, true, 'sky', null, 'era_reward', 1500000, 22),
  ('land_double_ring', 'landscape', '이중 고리', 40000000, 1, true, 'sky', null, 'era_reward', 4000000, 23),
  ('land_moonlets', 'landscape', '작은 위성들', 100000000, 1, true, 'sky', null, 'era_reward', 10000000, 24),
  ('land_lantern', 'landscape', '등불', 5000000, 1, true, 'ground', null, 'streak_reward', 10000, 25),
  ('land_stars', 'landscape', '별무리', 15000000, 1, true, 'sky', null, 'streak_reward', 30000, 26),
  ('land_aurora', 'landscape', '오로라', 40000000, 1, true, 'sky', null, 'streak_reward', 80000, 27),
  ('land_meteors', 'landscape', '유성우', 100000000, 1, true, 'sky', null, 'streak_reward', 200000, 28),
  ('land_garden', 'landscape', '꽃 정원', 5000000, 1, true, 'ground', null, 'civilization_growth', 100, 29),
  ('land_tree', 'landscape', '장식 나무', 15000000, 1, true, 'ground', null, 'civilization_growth', 150, 30),
  ('land_bench', 'landscape', '벤치', 40000000, 1, true, 'ground', null, 'civilization_growth', 200, 31),
  ('land_fountain', 'landscape', '분수', 100000000, 1, true, 'ground', null, 'civilization_growth', 300, 32),
  ('avatar_explorer_hat', 'avatar', '탐험가 모자', 100000000, 1, true, null, 'head', null, 0, 33),
  ('avatar_crown', 'avatar', '왕관', 200000000, 1, true, null, 'head', null, 0, 34),
  ('avatar_space_helmet', 'avatar', '우주 헬멧', 350000000, 1, true, null, 'head', null, 0, 35),
  ('avatar_halo', 'avatar', '홀로그램 관', 500000000, 1, true, null, 'head', null, 0, 36),
  ('avatar_workwear', 'avatar', '작업복', 100000000, 1, true, null, 'outfit', null, 0, 37),
  ('avatar_labwear', 'avatar', '연구복', 200000000, 1, true, null, 'outfit', null, 0, 38),
  ('avatar_spacesuit', 'avatar', '우주복', 350000000, 1, true, null, 'outfit', null, 0, 39),
  ('avatar_nebula_suit', 'avatar', '성운 의상', 500000000, 1, true, null, 'outfit', null, 0, 40),
  ('avatar_glasses', 'avatar', '안경', 100000000, 1, true, null, 'face', null, 0, 41),
  ('avatar_sunglasses', 'avatar', '선글라스', 200000000, 1, true, null, 'face', null, 0, 42),
  ('avatar_goggles', 'avatar', '고글', 350000000, 1, true, null, 'face', null, 0, 43),
  ('avatar_hud', 'avatar', 'HUD 바이저', 500000000, 1, true, null, 'face', null, 0, 44),
  ('avatar_backpack', 'avatar', '배낭', 100000000, 1, true, null, 'back', null, 0, 45),
  ('avatar_cape', 'avatar', '망토', 200000000, 1, true, null, 'back', null, 0, 46),
  ('avatar_jetpack', 'avatar', '제트팩', 350000000, 1, true, null, 'back', null, 0, 47),
  ('avatar_wings', 'avatar', '에너지 날개', 500000000, 1, true, null, 'back', null, 0, 48);

create table private.shop_account_lock (
  user_id uuid primary key references auth.users(id) on delete cascade
);

create table private.shop_account_state (
  user_id uuid primary key references auth.users(id) on delete cascade,
  state_revision bigint not null default 0 check (state_revision >= 0),
  reward_timezone text not null check (char_length(reward_timezone) between 1 and 80),
  updated_at timestamptz not null default now()
);

create table private.shop_purchase (
  user_id uuid not null references auth.users(id) on delete cascade,
  request_id text not null check (char_length(request_id) between 1 and 160),
  sku text not null references private.shop_products(sku) on delete restrict,
  price bigint not null check (price > 0),
  catalog_revision integer not null check (catalog_revision > 0),
  effect_revision bigint not null check (effect_revision >= 0),
  purchased_at timestamptz not null default now(),
  primary key (user_id, request_id)
);

create table private.shop_landscape_instance (
  user_id uuid not null references auth.users(id) on delete cascade,
  instance_id uuid not null,
  sku text not null references private.shop_products(sku) on delete restrict,
  variation_index smallint not null check (variation_index between 0 and 4),
  seed text not null check (char_length(seed) between 1 and 80),
  variation_version integer not null default 1 check (variation_version > 0),
  placement_version bigint not null default 0 check (placement_version >= 0),
  acquired_at timestamptz not null default now(),
  primary key (user_id, instance_id),
  unique (user_id, sku, variation_index)
);

create index shop_landscape_instance_by_sku
  on private.shop_landscape_instance(user_id, sku, variation_index);

create table private.shop_landscape_placement (
  user_id uuid not null,
  instance_id uuid not null,
  cycle_id text not null check (char_length(cycle_id) between 1 and 80),
  x double precision not null check (x > '-Infinity'::double precision and x < 'Infinity'::double precision),
  y double precision not null check (y > '-Infinity'::double precision and y < 'Infinity'::double precision),
  version bigint not null check (version >= 0),
  placed_at timestamptz not null default now(),
  primary key (user_id, instance_id),
  foreign key (user_id, instance_id)
    references private.shop_landscape_instance(user_id, instance_id) on delete cascade
);

create index shop_landscape_placement_by_cycle
  on private.shop_landscape_placement(user_id, cycle_id, instance_id);

create table private.shop_avatar_owned (
  user_id uuid not null references auth.users(id) on delete cascade,
  sku text not null,
  slot text not null check (slot in ('head', 'outfit', 'face', 'back')),
  acquired_at timestamptz not null default now(),
  primary key (user_id, sku),
  unique (user_id, sku, slot),
  foreign key (sku, slot) references private.shop_products(sku, avatar_slot) on delete restrict
);

create table private.shop_avatar_equipment (
  user_id uuid not null references auth.users(id) on delete cascade,
  slot text not null check (slot in ('head', 'outfit', 'face', 'back')),
  sku text,
  version bigint not null default 0 check (version >= 0),
  updated_at timestamptz not null default now(),
  primary key (user_id, slot),
  foreign key (user_id, sku, slot)
    references private.shop_avatar_owned(user_id, sku, slot) on delete restrict
);

create table private.shop_action_request (
  user_id uuid not null references auth.users(id) on delete cascade,
  request_id text not null check (char_length(request_id) between 1 and 160),
  payload jsonb not null check (jsonb_typeof(payload) = 'object'),
  status text not null check (status in (
    'purchased', 'placed', 'retrieved', 'equipped', 'unequipped', 'removed',
    'limit_reached', 'already_owned', 'insufficient_balance', 'quote_changed',
    'catalog_mismatch', 'version_conflict', 'cycle_mismatch', 'not_owned',
    'already_removed', 'request_conflict', 'invalid_placement', 'unavailable'
  )),
  confirmed_quote jsonb check (confirmed_quote is null or jsonb_typeof(confirmed_quote) = 'object'),
  created_at timestamptz not null default now(),
  primary key (user_id, request_id)
);

create table private.shop_effect_history (
  user_id uuid not null references auth.users(id) on delete cascade,
  cycle_id text not null check (char_length(cycle_id) between 1 and 80),
  revision bigint not null check (revision > 0),
  started_at timestamptz not null,
  ended_at timestamptz,
  active_instance_ids jsonb not null check (jsonb_typeof(active_instance_ids) = 'array'),
  effects jsonb not null check (jsonb_typeof(effects) = 'object'),
  primary key (user_id, cycle_id, revision),
  check (ended_at is null or ended_at >= started_at)
);

create unique index shop_effect_history_one_open_interval
  on private.shop_effect_history(user_id, cycle_id) where ended_at is null;

alter table private.shop_products enable row level security;
alter table private.shop_account_lock enable row level security;
alter table private.shop_account_state enable row level security;
alter table private.shop_purchase enable row level security;
alter table private.shop_landscape_instance enable row level security;
alter table private.shop_landscape_placement enable row level security;
alter table private.shop_avatar_owned enable row level security;
alter table private.shop_avatar_equipment enable row level security;
alter table private.shop_action_request enable row level security;
alter table private.shop_effect_history enable row level security;

revoke all on private.shop_products, private.shop_account_lock, private.shop_account_state,
  private.shop_purchase, private.shop_landscape_instance, private.shop_landscape_placement,
  private.shop_avatar_owned, private.shop_avatar_equipment, private.shop_action_request,
  private.shop_effect_history from public, anon, authenticated;

create function private.shop_available_balance(p_user_id uuid)
returns numeric
language sql set search_path = '' as $$
  select coalesce((
    select sum(w.amount::numeric)
    from private.planet_wallet_credits w where w.user_id = p_user_id
  ), 0) - coalesce((
    select sum(p.price::numeric)
    from private.shop_purchase p where p.user_id = p_user_id
  ), 0);
$$;

create function private.shop_active_effects(p_user_id uuid, p_cycle_id text)
returns jsonb
language sql set search_path = '' as $$
  with totals as (
    select
      coalesce(sum(p.effect_value::numeric) filter (where p.effect_type = 'token_earning'), 0) as token_earning,
      coalesce(sum(p.effect_value::numeric) filter (where p.effect_type = 'civilization_growth'), 0) as civilization_growth,
      coalesce(sum(p.effect_value::numeric) filter (where p.effect_type = 'shop_discount'), 0) as shop_discount,
      coalesce(sum(p.effect_value::numeric) filter (where p.effect_type = 'reset_cooldown'), 0) as reset_cooldown,
      coalesce(sum(p.effect_value::numeric) filter (where p.effect_type = 'natural_removal_discount'), 0) as natural_removal_discount,
      coalesce(sum(p.effect_value::numeric) filter (where p.effect_type = 'era_reward'), 0) as era_reward,
      coalesce(sum(p.effect_value::numeric) filter (where p.effect_type = 'streak_reward'), 0) as streak_reward
    from private.shop_landscape_placement pl
    join private.shop_landscape_instance i
      on i.user_id = pl.user_id and i.instance_id = pl.instance_id
    join private.shop_products p on p.sku = i.sku and p.category = 'landscape'
    where pl.user_id = p_user_id and pl.cycle_id = p_cycle_id
  )
  select jsonb_build_object(
    'token_earning_bps', least(t.token_earning, 3000)::bigint,
    'civilization_growth_bps', least(t.civilization_growth, 2000)::bigint,
    'shop_discount_bps', least(t.shop_discount, 1500)::bigint,
    'reset_cooldown_bps', least(t.reset_cooldown, 2500)::bigint,
    'natural_removal_discount_bps', least(t.natural_removal_discount, 3000)::bigint,
    'era_reward_tokens', least(t.era_reward, 10000000)::bigint,
    'streak_reward_tokens', least(t.streak_reward, 500000)::bigint
  ) from totals t;
$$;

create function private.lock_shop_account(p_user_id uuid)
returns void
language plpgsql set search_path = '' as $$
begin
  if p_user_id is null or p_user_id is distinct from (select auth.uid()) then
    raise exception 'shop account access denied' using errcode = '42501';
  end if;

  insert into private.shop_account_lock(user_id) values (p_user_id) on conflict do nothing;
  perform 1 from private.shop_account_lock l where l.user_id = p_user_id for update;

  insert into private.shop_account_state(user_id, reward_timezone)
  values (
    p_user_id,
    coalesce((select p.timezone from public.planet_member_state p where p.user_id = p_user_id), 'UTC')
  ) on conflict (user_id) do nothing;

  insert into private.shop_avatar_equipment(user_id, slot)
  values
    (p_user_id, 'head'), (p_user_id, 'outfit'), (p_user_id, 'face'), (p_user_id, 'back')
  on conflict (user_id, slot) do nothing;
end;
$$;

create function private.shop_state_json(p_user_id uuid)
returns jsonb
language sql set search_path = '' as $$
  with account as (
    select p.current_cycle_id
    from public.planet_member_state p where p.user_id = p_user_id
  ), current_cycle as (
    select coalesce((select a.current_cycle_id from account a), '') as cycle_id
  )
  select jsonb_build_object(
    'account_id', 'account:' || p_user_id::text,
    'current_cycle_id', c.cycle_id,
    'catalog_revision', 1,
    'state_revision', coalesce((
      select s.state_revision from private.shop_account_state s where s.user_id = p_user_id
    ), 0),
    'available_balance', private.shop_available_balance(p_user_id),
    'products', coalesce((
      select jsonb_agg(jsonb_build_object(
        'sku', p.sku,
        'category', p.category,
        'display_name', p.display_name,
        'price', p.price,
        'catalog_revision', p.catalog_revision,
        'purchasable', p.purchasable,
        'placement_zone', p.placement_zone,
        'avatar_slot', p.avatar_slot,
        'effect_type', p.effect_type,
        'effect_value', p.effect_value
      ) order by p.sort_order)
      from private.shop_products p
    ), '[]'::jsonb),
    'landscape_instances', coalesce((
      select jsonb_agg(jsonb_build_object(
        'instance_id', i.instance_id::text,
        'sku', i.sku,
        'variation_index', i.variation_index,
        'seed', i.seed,
        'variation_version', i.variation_version,
        'placement_version', i.placement_version
      ) order by i.sku, i.variation_index)
      from private.shop_landscape_instance i where i.user_id = p_user_id
    ), '[]'::jsonb),
    'placements', coalesce((
      select jsonb_agg(jsonb_build_object(
        'instance_id', pl.instance_id::text,
        'cycle_id', pl.cycle_id,
        'x', pl.x,
        'y', pl.y,
        'version', pl.version
      ) order by pl.instance_id)
      from private.shop_landscape_placement pl
      where pl.user_id = p_user_id and pl.cycle_id = c.cycle_id
    ), '[]'::jsonb),
    'avatar_owned_skus', coalesce((
      select jsonb_agg(o.sku order by o.sku)
      from private.shop_avatar_owned o where o.user_id = p_user_id
    ), '[]'::jsonb),
    'avatar_equipment', jsonb_build_object(
      'head', coalesce((
        select jsonb_build_object('sku', e.sku, 'version', e.version)
        from private.shop_avatar_equipment e where e.user_id = p_user_id and e.slot = 'head'
      ), '{"sku":null,"version":0}'::jsonb),
      'outfit', coalesce((
        select jsonb_build_object('sku', e.sku, 'version', e.version)
        from private.shop_avatar_equipment e where e.user_id = p_user_id and e.slot = 'outfit'
      ), '{"sku":null,"version":0}'::jsonb),
      'face', coalesce((
        select jsonb_build_object('sku', e.sku, 'version', e.version)
        from private.shop_avatar_equipment e where e.user_id = p_user_id and e.slot = 'face'
      ), '{"sku":null,"version":0}'::jsonb),
      'back', coalesce((
        select jsonb_build_object('sku', e.sku, 'version', e.version)
        from private.shop_avatar_equipment e where e.user_id = p_user_id and e.slot = 'back'
      ), '{"sku":null,"version":0}'::jsonb)
    ),
    'effects', private.shop_active_effects(p_user_id, c.cycle_id),
    'reward_state', jsonb_build_object(
      'reward_timezone', coalesce((
        select s.reward_timezone from private.shop_account_state s where s.user_id = p_user_id
      ), 'UTC'),
      'settled_cycle_tokens', 0,
      'era_reward_tokens', 0,
      'streak_reward_tokens', 0
    ),
    'action_unavailable_reason', null,
    'guest_import_pending', false,
    'guest_import_error', null
  )
  from current_cycle c;
$$;

create function private.shop_quote_json(p_user_id uuid, p_target jsonb)
returns jsonb
language plpgsql set search_path = '' as $$
declare
  v_product private.shop_products%rowtype;
  v_sku text;
  v_cycle_id text;
  v_effects jsonb;
  v_effect_revision bigint;
  v_discount integer;
  v_price bigint;
begin
  if jsonb_typeof(p_target) is distinct from 'object'
    or (select array_agg(k.key order by k.key) from jsonb_object_keys(p_target) k(key))
      is distinct from array['kind', 'sku']::text[]
    or jsonb_typeof(p_target->'kind') is distinct from 'string'
    or p_target->>'kind' <> 'purchase'
    or jsonb_typeof(p_target->'sku') is distinct from 'string'
  then
    raise exception 'shop quote target is invalid or unavailable' using errcode = '22023';
  end if;

  v_sku := p_target->>'sku';
  if v_sku !~ '^[a-z][a-z0-9_]{0,63}$' then
    raise exception 'shop quote SKU is invalid' using errcode = '22023';
  end if;

  select p.* into v_product from private.shop_products p
  where p.sku = v_sku and p.purchasable;
  if not found then
    raise exception 'shop catalog product is unavailable' using errcode = '22023';
  end if;

  v_cycle_id := coalesce((
    select p.current_cycle_id from public.planet_member_state p where p.user_id = p_user_id
  ), '');
  v_effects := private.shop_active_effects(p_user_id, v_cycle_id);
  v_discount := (v_effects->>'shop_discount_bps')::integer;
  select coalesce(max(h.revision), 0) into v_effect_revision
  from private.shop_effect_history h
  where h.user_id = p_user_id and h.cycle_id = v_cycle_id;

  v_price := greatest(1, floor((v_product.price::numeric * (10000 - v_discount) + 9999) / 10000)::bigint);
  return jsonb_build_object(
    'target', p_target,
    'catalog_revision', v_product.catalog_revision,
    'effect_revision', v_effect_revision,
    'price', v_price
  );
end;
$$;

create function public.get_my_shop_state()
returns jsonb
language plpgsql security definer set search_path = '' as $$
declare
  v_user_id uuid := (select auth.uid());
begin
  if v_user_id is null then
    raise exception 'authentication required' using errcode = '42501';
  end if;
  perform private.lock_shop_account(v_user_id);
  return private.shop_state_json(v_user_id);
end;
$$;

create function public.quote_shop_action(p_target jsonb)
returns jsonb
language plpgsql security definer set search_path = '' as $$
declare
  v_user_id uuid := (select auth.uid());
begin
  if v_user_id is null then
    raise exception 'authentication required' using errcode = '42501';
  end if;
  perform private.lock_shop_account(v_user_id);
  return private.shop_quote_json(v_user_id, p_target);
end;
$$;

revoke all on function private.prevent_shop_product_identity_change(),
  private.shop_available_balance(uuid), private.shop_active_effects(uuid, text),
  private.lock_shop_account(uuid), private.shop_state_json(uuid), private.shop_quote_json(uuid, jsonb)
  from public, anon, authenticated;
revoke all on function public.get_my_shop_state(), public.quote_shop_action(jsonb)
  from public, anon, authenticated;
grant execute on function public.get_my_shop_state(), public.quote_shop_action(jsonb)
  to authenticated;
