do $$
begin
  if not exists (
    select 1
    from pg_catalog.pg_trigger t
    where t.tgrelid = 'public.planet_member_state'::pg_catalog.regclass
      and t.tgname = 'clear_cosmetic_equipment_after_cycle_change'
      and not t.tgisinternal
  ) then
    raise exception 'expected legacy cycle-change trigger is missing' using errcode = '55000';
  end if;

  execute 'drop trigger clear_cosmetic_equipment_after_cycle_change on public.planet_member_state';
end;
$$;

create or replace function public.purchase_my_cosmetic(
  p_purchase_id uuid,
  p_sku text,
  p_catalog_revision integer
)
returns jsonb
language plpgsql security definer set search_path = '' as $$
begin
  raise exception 'legacy cosmetic purchase RPC is retired; use apply_shop_action' using errcode = '55000';
end;
$$;

create or replace function public.equip_my_cosmetic(
  p_cycle_id text,
  p_slot_id text,
  p_sku text,
  p_expected_version bigint
)
returns jsonb
language plpgsql security definer set search_path = '' as $$
begin
  raise exception 'legacy cosmetic equipment RPC is retired; use apply_shop_action' using errcode = '55000';
end;
$$;

create or replace function public.import_my_guest_cosmetics(
  p_import_id uuid,
  p_wallet_credits jsonb,
  p_purchases jsonb
)
returns jsonb
language plpgsql security definer set search_path = '' as $$
begin
  raise exception 'legacy cosmetic import RPC is retired; use apply_shop_action' using errcode = '55000';
end;
$$;

revoke all on function public.purchase_my_cosmetic(uuid, text, integer),
  public.equip_my_cosmetic(text, text, text, bigint),
  public.import_my_guest_cosmetics(uuid, jsonb, jsonb)
  from public, anon, authenticated, service_role;

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
    'wallet_balance', private.shop_available_balance(p_user_id),
    'wallet_credits', coalesce((
      select jsonb_agg(jsonb_build_object(
        'previous_cycle_id', w.previous_cycle_id,
        'amount', w.amount,
        'created_at_utc', to_char(w.created_at at time zone 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
      ) order by w.created_at, w.previous_cycle_id)
      from private.planet_wallet_credits w where w.user_id = p.user_id
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
