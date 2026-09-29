insert into private.cosmetic_slots(slot_id, display_name)
values ('forecourt', '앞마당');

update private.cosmetic_products
set purchasable = false
where sku in ('star_cluster', 'aurora', 'thin_ring', 'double_ring', 'flag', 'crystal_tower');

insert into private.cosmetic_products(sku, slot_id, display_name, price, catalog_revision) values
  ('star_cluster_v2', 'sky', '별무리', 500000, 1),
  ('aurora_v2', 'sky', '오로라', 2000000, 1),
  ('thin_ring_v2', 'ring', '얇은 고리', 500000, 1),
  ('double_ring_v2', 'ring', '이중 고리', 2000000, 1),
  ('flag_v2', 'surface', '깃발', 500000, 1),
  ('crystal_tower_v2', 'surface', '수정탑', 2000000, 1),
  ('meteor_shower', 'sky', '유성우', 1000000, 1),
  ('moonlets', 'ring', '작은 위성들', 3000000, 1),
  ('flower_garden', 'surface', '꽃 정원', 1000000, 1),
  ('observatory', 'surface', '천문대', 5000000, 1),
  ('pond', 'forecourt', '연못', 750000, 1),
  ('lantern', 'forecourt', '등불', 1500000, 1),
  ('rover', 'forecourt', '탐사 로버', 3000000, 1),
  ('greenhouse', 'forecourt', '온실', 5000000, 1);

create table private.cosmetic_style_equivalence (
  new_sku text primary key references private.cosmetic_products(sku) on delete restrict,
  legacy_sku text not null unique references private.cosmetic_products(sku) on delete restrict,
  check (new_sku <> legacy_sku)
);

insert into private.cosmetic_style_equivalence(new_sku, legacy_sku) values
  ('star_cluster_v2', 'star_cluster'),
  ('aurora_v2', 'aurora'),
  ('thin_ring_v2', 'thin_ring'),
  ('double_ring_v2', 'double_ring'),
  ('flag_v2', 'flag'),
  ('crystal_tower_v2', 'crystal_tower');

create function private.prevent_cosmetic_style_equivalence_change()
returns trigger
language plpgsql set search_path = '' as $$
begin
  raise exception 'cosmetic style equivalence is immutable' using errcode = '23514';
end;
$$;

create trigger cosmetic_style_equivalence_is_immutable
before update or delete on private.cosmetic_style_equivalence
for each row execute function private.prevent_cosmetic_style_equivalence_change();

alter table private.cosmetic_style_equivalence enable row level security;
revoke all on private.cosmetic_style_equivalence from public, anon, authenticated;

create or replace function public.purchase_my_cosmetic(
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
    select 1 from private.cosmetic_purchase owned
    where owned.user_id = v_user_id and owned.sku = p_sku
  ) then
    v_result := jsonb_build_object(
      'purchase_id', p_purchase_id, 'sku', p_sku, 'status', 'already_owned',
      'price', v_product.price, 'available_balance', v_balance
    );
  elsif exists (
    select 1
    from private.cosmetic_style_equivalence e
    join private.cosmetic_purchase owned on owned.sku = e.legacy_sku
    where owned.user_id = v_user_id and e.new_sku = p_sku
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

create or replace function public.import_my_guest_cosmetics(
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

  with imported as (
    select p.value, p.ordinality,
      coalesce(e.legacy_sku, p.value->>'sku') as style_key,
      row_number() over (
        partition by coalesce(e.legacy_sku, p.value->>'sku') order by p.ordinality
      ) as style_order
    from jsonb_array_elements(p_purchases) with ordinality as p(value, ordinality)
    left join private.cosmetic_style_equivalence e on e.new_sku = p.value->>'sku'
  )
  select coalesce(sum((p.value->>'price')::numeric), 0) into v_new_purchases
  from imported p
  where p.style_order = 1
    and not exists (
      select 1 from private.cosmetic_purchase owned
      where owned.user_id = v_user_id and owned.sku = p.value->>'sku'
    )
    and not exists (
      select 1
      from private.cosmetic_style_equivalence e
      join private.cosmetic_purchase owned on owned.user_id = v_user_id
        and ((e.new_sku = p.value->>'sku' and owned.sku = e.legacy_sku)
          or (e.legacy_sku = p.value->>'sku' and owned.sku = e.new_sku))
      where e.new_sku = p.value->>'sku' or e.legacy_sku = p.value->>'sku'
    )
    and not exists (
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
      select 1 from private.cosmetic_purchase owned
      where owned.user_id = v_user_id and (
        owned.sku = v_purchase->>'sku'
        or owned.purchase_id = (v_purchase->>'purchase_id')::uuid
      )
    ) and not exists (
      select 1
      from private.cosmetic_style_equivalence e
      join private.cosmetic_purchase owned on owned.user_id = v_user_id
        and ((e.new_sku = v_purchase->>'sku' and owned.sku = e.legacy_sku)
          or (e.legacy_sku = v_purchase->>'sku' and owned.sku = e.new_sku))
      where e.new_sku = v_purchase->>'sku' or e.legacy_sku = v_purchase->>'sku'
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
    where owned.user_id = v_user_id and owned.purchase_id <> (p.value->>'purchase_id')::uuid
      and (
        owned.sku = p.value->>'sku'
        or exists (
          select 1 from private.cosmetic_style_equivalence e
          where (e.new_sku = p.value->>'sku' and e.legacy_sku = owned.sku)
            or (e.legacy_sku = p.value->>'sku' and e.new_sku = owned.sku)
        )
      )
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

revoke all on function private.prevent_cosmetic_style_equivalence_change()
  from public, anon, authenticated;
