create schema if not exists private;

create table private.cosmetic_slots (
  slot_id text primary key check (slot_id ~ '^[a-z][a-z0-9_]{0,63}$'),
  display_name text not null check (char_length(display_name) between 1 and 80)
);

create table private.cosmetic_products (
  sku text primary key check (sku ~ '^[a-z][a-z0-9_]{0,63}$'),
  slot_id text not null references private.cosmetic_slots(slot_id) on delete restrict,
  display_name text not null check (char_length(display_name) between 1 and 80),
  price bigint not null check (price > 0),
  catalog_revision integer not null check (catalog_revision > 0),
  purchasable boolean not null default true,
  unique (sku, slot_id)
);

insert into private.cosmetic_slots(slot_id, display_name) values
  ('sky', '하늘'), ('ring', '고리'), ('surface', '지표');

insert into private.cosmetic_products(sku, slot_id, display_name, price, catalog_revision) values
  ('star_cluster', 'sky', '별무리', 100000, 1),
  ('aurora', 'sky', '오로라', 500000, 1),
  ('thin_ring', 'ring', '얇은 고리', 100000, 1),
  ('double_ring', 'ring', '이중 고리', 500000, 1),
  ('flag', 'surface', '깃발', 100000, 1),
  ('crystal_tower', 'surface', '수정탑', 500000, 1);

create function private.prevent_cosmetic_product_identity_change()
returns trigger
language plpgsql set search_path = '' as $$
begin
  if new.sku is distinct from old.sku
    or new.slot_id is distinct from old.slot_id
    or new.price is distinct from old.price
    or new.catalog_revision is distinct from old.catalog_revision
  then
    raise exception 'cosmetic SKU, slot, price, and revision are immutable' using errcode = '23514';
  end if;
  return new;
end;
$$;

create trigger cosmetic_product_identity_is_immutable
before update on private.cosmetic_products
for each row execute function private.prevent_cosmetic_product_identity_change();

create table private.cosmetic_account_lock (
  user_id uuid primary key references auth.users(id) on delete cascade
);

create table private.cosmetic_purchase (
  user_id uuid not null references auth.users(id) on delete cascade,
  purchase_id uuid not null,
  sku text not null references private.cosmetic_products(sku) on delete restrict,
  price bigint not null check (price > 0),
  purchased_at timestamptz not null default now(),
  primary key (user_id, purchase_id),
  unique (user_id, sku)
);

create table private.cosmetic_purchase_request (
  user_id uuid not null references auth.users(id) on delete cascade,
  purchase_id uuid not null,
  sku text not null check (sku ~ '^[a-z][a-z0-9_]{0,63}$'),
  result jsonb not null check (jsonb_typeof(result) = 'object'),
  created_at timestamptz not null default now(),
  primary key (user_id, purchase_id)
);

create table private.cosmetic_equipment (
  user_id uuid not null references auth.users(id) on delete cascade,
  cycle_id text not null check (char_length(cycle_id) between 1 and 80),
  slot_id text not null references private.cosmetic_slots(slot_id) on delete restrict,
  sku text,
  version bigint not null default 0 check (version >= 0),
  primary key (user_id, cycle_id, slot_id),
  foreign key (sku, slot_id) references private.cosmetic_products(sku, slot_id) on delete restrict
);

create index cosmetic_equipment_current_cycle_idx
  on private.cosmetic_equipment(user_id, cycle_id);

alter table private.cosmetic_slots enable row level security;
alter table private.cosmetic_products enable row level security;
alter table private.cosmetic_account_lock enable row level security;
alter table private.cosmetic_purchase enable row level security;
alter table private.cosmetic_purchase_request enable row level security;
alter table private.cosmetic_equipment enable row level security;
revoke all on private.cosmetic_slots, private.cosmetic_products, private.cosmetic_account_lock,
  private.cosmetic_purchase, private.cosmetic_purchase_request, private.cosmetic_equipment
  from public, anon, authenticated;

create function private.cosmetic_available_balance(p_user_id uuid)
returns numeric
language sql security definer set search_path = '' as $$
  select coalesce((
    select sum(w.amount::numeric) from private.planet_wallet_credits w where w.user_id = p_user_id
  ), 0) - coalesce((
    select sum(p.price::numeric) from private.cosmetic_purchase p where p.user_id = p_user_id
  ), 0);
$$;

create function private.cosmetic_shop_state_json(p_user_id uuid)
returns jsonb
language sql security definer set search_path = '' as $$
  select jsonb_build_object(
    'slots', coalesce((
      select jsonb_agg(jsonb_build_object('slot_id', s.slot_id, 'display_name', s.display_name)
        order by s.slot_id)
      from private.cosmetic_slots s
    ), '[]'::jsonb),
    'products', coalesce((
      select jsonb_agg(jsonb_build_object(
        'sku', p.sku, 'slot_id', p.slot_id, 'display_name', p.display_name,
        'price', p.price, 'catalog_revision', p.catalog_revision, 'purchasable', p.purchasable
      ) order by p.slot_id, p.price, p.sku)
      from private.cosmetic_products p
    ), '[]'::jsonb),
    'current_cycle_id', coalesce((
      select p.current_cycle_id from public.planet_member_state p where p.user_id = p_user_id
    ), ''),
    'available_balance', private.cosmetic_available_balance(p_user_id),
    'owned_skus', coalesce((
      select jsonb_agg(p.sku order by p.sku)
      from private.cosmetic_purchase p where p.user_id = p_user_id
    ), '[]'::jsonb),
    'equipped', coalesce((
      select jsonb_agg(jsonb_build_object('slot_id', e.slot_id, 'sku', e.sku, 'version', e.version)
        order by e.slot_id)
      from private.cosmetic_equipment e
      join public.planet_member_state p on p.user_id = e.user_id and p.current_cycle_id = e.cycle_id
      where e.user_id = p_user_id and e.sku is not null
    ), '[]'::jsonb),
    'slot_versions', coalesce((
      select jsonb_object_agg(e.slot_id, e.version)
      from private.cosmetic_equipment e
      join public.planet_member_state p on p.user_id = e.user_id and p.current_cycle_id = e.cycle_id
      where e.user_id = p_user_id
    ), '{}'::jsonb),
    'actions_require_online', true
  );
$$;

create function public.get_my_cosmetic_state()
returns jsonb
language plpgsql security definer set search_path = '' as $$
declare
  v_user_id uuid := (select auth.uid());
begin
  if v_user_id is null then
    raise exception 'authentication required' using errcode = '42501';
  end if;
  return private.cosmetic_shop_state_json(v_user_id);
end;
$$;

create function public.purchase_my_cosmetic(
  p_purchase_id uuid,
  p_sku text,
  p_catalog_revision integer
)
returns jsonb
language plpgsql security definer set search_path = '' as $$
declare
  v_user_id uuid := (select auth.uid());
  v_prior_sku text;
  v_prior_result jsonb;
  v_product private.cosmetic_products%rowtype;
  v_balance numeric;
  v_result jsonb;
begin
  if v_user_id is null then
    raise exception 'authentication required' using errcode = '42501';
  end if;
  if p_purchase_id is null or p_sku is null or p_sku !~ '^[a-z][a-z0-9_]{0,63}$'
    or p_catalog_revision is null or p_catalog_revision < 1
  then
    raise exception 'cosmetic purchase request is invalid' using errcode = '23514';
  end if;

  insert into private.cosmetic_account_lock(user_id) values (v_user_id) on conflict do nothing;
  perform 1 from private.cosmetic_account_lock l where l.user_id = v_user_id for update;

  select r.sku, r.result into v_prior_sku, v_prior_result
  from private.cosmetic_purchase_request r
  where r.user_id = v_user_id and r.purchase_id = p_purchase_id;
  if found then
    if v_prior_sku = p_sku then
      return v_prior_result;
    end if;
    return jsonb_build_object(
      'purchase_id', p_purchase_id, 'sku', p_sku, 'status', 'request_conflict',
      'price', 0, 'available_balance', private.cosmetic_available_balance(v_user_id)
    );
  end if;

  v_balance := private.cosmetic_available_balance(v_user_id);
  select p.* into v_product from private.cosmetic_products p where p.sku = p_sku;
  if not found then
    v_result := jsonb_build_object(
      'purchase_id', p_purchase_id, 'sku', p_sku, 'status', 'catalog_mismatch',
      'price', 0, 'available_balance', v_balance
    );
  elsif exists (
    select 1 from private.cosmetic_purchase p where p.user_id = v_user_id and p.sku = p_sku
  ) then
    v_result := jsonb_build_object(
      'purchase_id', p_purchase_id, 'sku', p_sku, 'status', 'already_owned',
      'price', v_product.price, 'available_balance', v_balance
    );
  elsif v_product.catalog_revision <> p_catalog_revision or not v_product.purchasable then
    v_result := jsonb_build_object(
      'purchase_id', p_purchase_id, 'sku', p_sku, 'status', 'catalog_mismatch',
      'price', v_product.price, 'available_balance', v_balance
    );
  elsif v_balance < v_product.price then
    v_result := jsonb_build_object(
      'purchase_id', p_purchase_id, 'sku', p_sku, 'status', 'insufficient_balance',
      'price', v_product.price, 'available_balance', v_balance
    );
  else
    insert into private.cosmetic_purchase(user_id, purchase_id, sku, price)
    values (v_user_id, p_purchase_id, p_sku, v_product.price);
    v_result := jsonb_build_object(
      'purchase_id', p_purchase_id, 'sku', p_sku, 'status', 'purchased',
      'price', v_product.price, 'available_balance', v_balance - v_product.price
    );
  end if;

  insert into private.cosmetic_purchase_request(user_id, purchase_id, sku, result)
  values (v_user_id, p_purchase_id, p_sku, v_result);
  return v_result;
end;
$$;

create function public.equip_my_cosmetic(
  p_cycle_id text,
  p_slot_id text,
  p_sku text,
  p_expected_version bigint
)
returns jsonb
language plpgsql security definer set search_path = '' as $$
declare
  v_user_id uuid := (select auth.uid());
  v_cycle_id text;
  v_sku text;
  v_version bigint := 0;
  v_status text;
begin
  if v_user_id is null then
    raise exception 'authentication required' using errcode = '42501';
  end if;
  if p_cycle_id is null or char_length(p_cycle_id) not between 1 and 80
    or p_slot_id is null or p_slot_id !~ '^[a-z][a-z0-9_]{0,63}$'
    or p_expected_version is null or p_expected_version < 0
    or (p_sku is not null and p_sku !~ '^[a-z][a-z0-9_]{0,63}$')
  then
    raise exception 'cosmetic equipment request is invalid' using errcode = '23514';
  end if;

  insert into private.cosmetic_account_lock(user_id) values (v_user_id) on conflict do nothing;
  perform 1 from private.cosmetic_account_lock l where l.user_id = v_user_id for update;
  select p.current_cycle_id into v_cycle_id
  from public.planet_member_state p where p.user_id = v_user_id for update;
  if not found then
    return jsonb_build_object('status', 'cycle_mismatch', 'cycle_id', '', 'slot_id', p_slot_id,
      'sku', null, 'version', 0);
  end if;
  select e.sku, e.version into v_sku, v_version
  from private.cosmetic_equipment e
  where e.user_id = v_user_id and e.cycle_id = v_cycle_id and e.slot_id = p_slot_id;
  if not found then
    v_sku := null;
    v_version := 0;
  end if;

  if p_cycle_id <> v_cycle_id then
    v_status := 'cycle_mismatch';
  elsif not exists (select 1 from private.cosmetic_slots s where s.slot_id = p_slot_id) then
    v_status := 'catalog_mismatch';
  elsif p_sku is not null and not exists (
    select 1 from private.cosmetic_products p where p.sku = p_sku and p.slot_id = p_slot_id
  ) then
    v_status := 'catalog_mismatch';
  elsif p_sku is not null and not exists (
    select 1 from private.cosmetic_purchase p where p.user_id = v_user_id and p.sku = p_sku
  ) then
    v_status := 'not_owned';
  elsif p_expected_version <> v_version then
    v_status := 'version_conflict';
  else
    insert into private.cosmetic_equipment(user_id, cycle_id, slot_id, sku, version)
    values (v_user_id, v_cycle_id, p_slot_id, p_sku, v_version + 1)
    on conflict (user_id, cycle_id, slot_id) do update set
      sku = excluded.sku, version = excluded.version;
    v_sku := p_sku;
    v_version := v_version + 1;
    v_status := case when p_sku is null then 'unequipped' else 'equipped' end;
  end if;

  return jsonb_build_object(
    'status', v_status, 'cycle_id', v_cycle_id, 'slot_id', p_slot_id,
    'sku', v_sku, 'version', v_version
  );
end;
$$;

create function private.clear_cosmetic_equipment_on_cycle_change()
returns trigger
language plpgsql security definer set search_path = '' as $$
begin
  delete from private.cosmetic_equipment where user_id = new.user_id;
  return new;
end;
$$;

create trigger clear_cosmetic_equipment_after_cycle_change
after update of current_cycle_id on public.planet_member_state
for each row when (old.current_cycle_id is distinct from new.current_cycle_id)
execute function private.clear_cosmetic_equipment_on_cycle_change();

create or replace function private.planet_state_json(p_user_id uuid)
returns jsonb
language sql security definer set search_path = '' as $$
  select jsonb_build_object(
    'version', p.state_version,
    'profile', jsonb_build_object('nickname', p.nickname, 'avatar', p.avatar),
    'timezone', p.timezone,
    'current_cycle_id', p.current_cycle_id,
    'cycle_started_at_utc', to_char(p.cycle_started_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
    'last_reset_at_utc', case when p.last_reset_at is null then null else to_char(p.last_reset_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"') end,
    'wallet_balance', coalesce((select sum(w.amount::numeric) from private.planet_wallet_credits w where w.user_id=p.user_id), 0)
      - coalesce((select sum(c.price::numeric) from private.cosmetic_purchase c where c.user_id=p.user_id), 0),
    'wallet_credits', coalesce((
      select jsonb_agg(jsonb_build_object(
        'previous_cycle_id', w.previous_cycle_id,
        'amount', w.amount,
        'created_at_utc', to_char(w.created_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
      ) order by w.created_at, w.previous_cycle_id)
      from private.planet_wallet_credits w where w.user_id=p.user_id
    ), '[]'::jsonb),
    'current_planet_tokens', p.current_planet_tokens,
    'lifetime_tokens', p.lifetime_tokens,
    'growth_credit', p.growth_credit,
    'stage', p.stage,
    'progress_to_next', p.progress_to_next,
    'incomplete', p.incomplete,
    'can_reset', p.last_reset_at is null or now() >= p.last_reset_at + interval '24 hours',
    'reset_available_at_utc', case when p.last_reset_at is null then null else to_char((p.last_reset_at + interval '24 hours') at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"') end,
    'objects', p.objects
  )
  from public.planet_member_state p where p.user_id = p_user_id;
$$;

revoke all on function private.prevent_cosmetic_product_identity_change(),
  private.cosmetic_available_balance(uuid), private.cosmetic_shop_state_json(uuid),
  private.clear_cosmetic_equipment_on_cycle_change(), public.get_my_cosmetic_state(),
  public.purchase_my_cosmetic(uuid, text, integer), public.equip_my_cosmetic(text, text, text, bigint),
  private.planet_state_json(uuid) from public, anon, authenticated;
grant execute on function public.get_my_cosmetic_state(),
  public.purchase_my_cosmetic(uuid, text, integer), public.equip_my_cosmetic(text, text, text, bigint)
  to authenticated;

create table private.cosmetic_guest_import_request (
  user_id uuid not null references auth.users(id) on delete cascade,
  import_id uuid not null,
  result jsonb not null check (jsonb_typeof(result) = 'object'),
  created_at timestamptz not null default now(),
  primary key (user_id, import_id)
);

alter table private.cosmetic_guest_import_request enable row level security;
revoke all on private.cosmetic_guest_import_request from public, anon, authenticated;

create function public.import_my_guest_cosmetics(
  p_import_id uuid,
  p_wallet_credits jsonb,
  p_purchases jsonb
)
returns jsonb
language plpgsql security definer set search_path = '' as $$
declare
  v_user_id uuid := (select auth.uid());
  v_prior_result jsonb;
  v_credit jsonb;
  v_purchase jsonb;
  v_product private.cosmetic_products%rowtype;
  v_balance numeric;
  v_new_credits numeric;
  v_new_purchases numeric;
  v_result jsonb;
  v_skipped_skus jsonb;
begin
  if v_user_id is null then
    raise exception 'authentication required' using errcode = '42501';
  end if;
  if p_import_id is null or p_wallet_credits is null or jsonb_typeof(p_wallet_credits) <> 'array'
    or p_purchases is null or jsonb_typeof(p_purchases) <> 'array'
  then
    raise exception 'guest cosmetic import is invalid' using errcode = '23514';
  end if;

  insert into private.cosmetic_account_lock(user_id) values (v_user_id) on conflict do nothing;
  perform 1 from private.cosmetic_account_lock l where l.user_id = v_user_id for update;
  select r.result into v_prior_result from private.cosmetic_guest_import_request r
  where r.user_id = v_user_id and r.import_id = p_import_id;
  if found then
    return v_prior_result;
  end if;

  if exists (
    select 1 from jsonb_array_elements(p_wallet_credits) c(value)
    group by c.value->>'previous_cycle_id' having count(*) > 1
  ) or exists (
    select 1 from jsonb_array_elements(p_purchases) p(value)
    group by p.value->>'sku' having count(*) > 1
  ) or exists (
    select 1 from jsonb_array_elements(p_purchases) p(value)
    group by p.value->>'purchase_id' having count(*) > 1
  )
  then
    raise exception 'guest cosmetic import contains duplicate keys' using errcode = '23514';
  end if;

  for v_credit in select value from jsonb_array_elements(p_wallet_credits)
  loop
    if jsonb_typeof(v_credit) <> 'object'
      or (select array_agg(k.key order by k.key) from jsonb_object_keys(v_credit) k(key))
        is distinct from array['amount', 'created_at_utc', 'previous_cycle_id']
      or jsonb_typeof(v_credit->'previous_cycle_id') <> 'string'
      or char_length(v_credit->>'previous_cycle_id') not between 1 and 80
      or jsonb_typeof(v_credit->'amount') <> 'number'
      or trunc((v_credit->>'amount')::numeric) <> (v_credit->>'amount')::numeric
      or (v_credit->>'amount')::numeric not between 0 and 9223372036854775807
      or jsonb_typeof(v_credit->'created_at_utc') <> 'string'
    then
      raise exception 'guest wallet credit is invalid' using errcode = '23514';
    end if;
    perform (v_credit->>'created_at_utc')::timestamptz;
  end loop;

  for v_purchase in select value from jsonb_array_elements(p_purchases)
  loop
    if jsonb_typeof(v_purchase) <> 'object'
      or (select array_agg(k.key order by k.key) from jsonb_object_keys(v_purchase) k(key))
        is distinct from array['price', 'purchase_id', 'purchased_at_utc', 'sku']
      or jsonb_typeof(v_purchase->'purchase_id') <> 'string'
      or jsonb_typeof(v_purchase->'sku') <> 'string'
      or (v_purchase->>'sku') !~ '^[a-z][a-z0-9_]{0,63}$'
      or jsonb_typeof(v_purchase->'price') <> 'number'
      or trunc((v_purchase->>'price')::numeric) <> (v_purchase->>'price')::numeric
      or (v_purchase->>'price')::numeric <= 0
      or (v_purchase->>'price')::numeric > 9223372036854775807
      or jsonb_typeof(v_purchase->'purchased_at_utc') <> 'string'
    then
      raise exception 'guest purchase is invalid' using errcode = '23514';
    end if;
    perform (v_purchase->>'purchase_id')::uuid;
    perform (v_purchase->>'purchased_at_utc')::timestamptz;
    select p.* into v_product from private.cosmetic_products p where p.sku = v_purchase->>'sku';
    if not found or v_product.price <> (v_purchase->>'price')::bigint then
      raise exception 'guest purchase does not match the registered SKU price' using errcode = '23514';
    end if;
    if exists (
      select 1 from private.cosmetic_purchase p
      where p.user_id = v_user_id and p.purchase_id = (v_purchase->>'purchase_id')::uuid
        and p.sku <> v_purchase->>'sku'
    ) then
      raise exception 'guest purchase ID conflicts with an existing purchase' using errcode = '23514';
    end if;
  end loop;

  v_balance := private.cosmetic_available_balance(v_user_id);
  select coalesce(sum((c.value->>'amount')::numeric), 0) into v_new_credits
  from jsonb_array_elements(p_wallet_credits) c(value)
  where not exists (
    select 1 from private.planet_wallet_credits w
    where w.user_id = v_user_id and w.previous_cycle_id = c.value->>'previous_cycle_id'
  );
  select coalesce(sum((p.value->>'price')::numeric), 0) into v_new_purchases
  from jsonb_array_elements(p_purchases) p(value)
  where not exists (
    select 1 from private.cosmetic_purchase owned
    where owned.user_id = v_user_id and owned.sku = p.value->>'sku'
  ) and not exists (
    select 1 from private.cosmetic_purchase existing
    where existing.user_id = v_user_id
      and existing.purchase_id = (p.value->>'purchase_id')::uuid
  );

  if v_balance + v_new_credits - v_new_purchases < 0 then
    v_result := jsonb_build_object(
      'import_id', p_import_id, 'status', 'insufficient_balance',
      'available_balance', v_balance, 'guest_credit_total', v_new_credits,
      'guest_purchase_total', v_new_purchases
    );
    return v_result;
  end if;

  for v_credit in select value from jsonb_array_elements(p_wallet_credits)
  loop
    insert into private.planet_wallet_credits(user_id, previous_cycle_id, amount, created_at)
    values (v_user_id, v_credit->>'previous_cycle_id', (v_credit->>'amount')::bigint,
      (v_credit->>'created_at_utc')::timestamptz)
    on conflict (user_id, previous_cycle_id) do nothing;
  end loop;

  for v_purchase in select value from jsonb_array_elements(p_purchases)
  loop
    if not exists (
      select 1 from private.cosmetic_purchase p
      where p.user_id = v_user_id and (p.sku = v_purchase->>'sku'
        or p.purchase_id = (v_purchase->>'purchase_id')::uuid)
    ) then
      insert into private.cosmetic_purchase(user_id, purchase_id, sku, price, purchased_at)
      values (v_user_id, (v_purchase->>'purchase_id')::uuid, v_purchase->>'sku',
        (v_purchase->>'price')::bigint, (v_purchase->>'purchased_at_utc')::timestamptz);
    end if;
  end loop;

  select coalesce(jsonb_agg(distinct p.value->>'sku' order by p.value->>'sku'), '[]'::jsonb)
  into v_skipped_skus
  from jsonb_array_elements(p_purchases) p(value)
  where exists (
    select 1 from private.cosmetic_purchase owned
    where owned.user_id = v_user_id and owned.sku = p.value->>'sku'
      and owned.purchase_id <> (p.value->>'purchase_id')::uuid
  );
  v_result := jsonb_build_object(
    'import_id', p_import_id, 'status', 'imported',
    'available_balance', private.cosmetic_available_balance(v_user_id),
    'new_credit_total', v_new_credits, 'new_purchase_total', v_new_purchases,
    'skipped_skus', v_skipped_skus
  );
  insert into private.cosmetic_guest_import_request(user_id, import_id, result)
  values (v_user_id, p_import_id, v_result);
  return v_result;
end;
$$;

revoke all on function public.import_my_guest_cosmetics(uuid, jsonb, jsonb) from public, anon;
grant execute on function public.import_my_guest_cosmetics(uuid, jsonb, jsonb) to authenticated;

drop function public.get_world_planets(uuid);
drop function private.get_world_planets(uuid);

create function private.get_world_planets(p_world_id uuid)
returns table (
  nickname text, avatar text, stage smallint, current_planet_tokens bigint,
  lifetime_tokens bigint, growth_credit numeric, progress_to_next numeric,
  incomplete boolean, objects jsonb, equipped_cosmetics jsonb,
  token_rank smallint, civilization_rank smallint
)
language plpgsql security definer set search_path = '' as $$
begin
  if (select auth.uid()) is null or not exists (
    select 1 from public.world_members m
    where m.world_id = p_world_id and m.user_id = (select auth.uid())
  ) then
    raise exception 'world access denied' using errcode = '42501';
  end if;
  return query
    with members as (
      select m.joined_at,
        coalesce(p.nickname, '행성 동기화 대기') as nickname,
        coalesce(p.avatar, 'masculine') as avatar,
        coalesce(p.stage, 0)::smallint as stage,
        coalesce(p.current_planet_tokens, 0)::bigint as current_planet_tokens,
        coalesce(p.lifetime_tokens, 0)::bigint as lifetime_tokens,
        coalesce(p.growth_credit, 0)::numeric as growth_credit,
        coalesce(p.progress_to_next, 0)::numeric as progress_to_next,
        coalesce(p.incomplete, true) as incomplete,
        coalesce(p.objects, '[]'::jsonb) as objects,
        coalesce((
          select jsonb_agg(jsonb_build_object('slot_id', e.slot_id, 'sku', e.sku)
            order by e.slot_id)
          from private.cosmetic_equipment e
          where e.user_id = p.user_id and e.cycle_id = p.current_cycle_id and e.sku is not null
        ), '[]'::jsonb) as equipped_cosmetics
      from public.world_members m
      left join public.planet_member_state p on p.user_id = m.user_id and p.shared_visible
      where m.world_id = p_world_id
    ), ranked as (
      select members.*,
        rank() over (order by members.lifetime_tokens desc)::smallint as token_rank,
        rank() over (order by members.growth_credit desc)::smallint as civilization_rank
      from members
    )
    select ranked.nickname, ranked.avatar, ranked.stage, ranked.current_planet_tokens,
      ranked.lifetime_tokens, ranked.growth_credit, ranked.progress_to_next,
      ranked.incomplete, ranked.objects, ranked.equipped_cosmetics,
      ranked.token_rank, ranked.civilization_rank
    from ranked order by ranked.joined_at, ranked.nickname;
end;
$$;

create function public.get_world_planets(p_world_id uuid)
returns table (
  nickname text, avatar text, stage smallint, current_planet_tokens bigint,
  lifetime_tokens bigint, growth_credit numeric, progress_to_next numeric,
  incomplete boolean, objects jsonb, equipped_cosmetics jsonb,
  token_rank smallint, civilization_rank smallint
)
language sql security invoker set search_path = '' as $$
  select * from private.get_world_planets(p_world_id);
$$;

revoke all on function private.get_world_planets(uuid), public.get_world_planets(uuid)
  from public, anon, authenticated;
grant execute on function private.get_world_planets(uuid), public.get_world_planets(uuid)
  to authenticated;
