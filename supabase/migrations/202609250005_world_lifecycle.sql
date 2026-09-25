create function private.delete_synced_usage(p_world_id uuid) returns integer
language plpgsql security definer set search_path = '' as $$
declare
  v_deleted integer;
begin
  if (select auth.uid()) is null or not exists (
    select 1 from public.world_members m
    where m.world_id = p_world_id and m.user_id = (select auth.uid())
  ) then
    raise exception 'world access denied' using errcode = '42501';
  end if;
  delete from public.daily_usage_snapshots s
  where s.world_id = p_world_id and s.user_id = (select auth.uid());
  get diagnostics v_deleted = row_count;
  return v_deleted;
end;
$$;
create function public.delete_synced_usage(p_world_id uuid) returns integer
language sql security invoker set search_path = '' as $$
  select private.delete_synced_usage(p_world_id);
$$;

create function private.transfer_world_owner(p_world_id uuid, p_new_owner_id uuid) returns boolean
language plpgsql security definer set search_path = '' as $$
begin
  perform 1 from public.worlds w
  where w.id = p_world_id and w.owner_id = (select auth.uid()) for update;
  if not found then
    raise exception 'only the owner can transfer this world' using errcode = '42501';
  end if;
  if p_new_owner_id = (select auth.uid()) or not exists (
    select 1 from public.world_members m
    where m.world_id = p_world_id and m.user_id = p_new_owner_id and m.role = 'member'
  ) then
    raise exception 'new owner must be another member' using errcode = '23514';
  end if;
  update public.world_members m set role = 'member'
  where m.world_id = p_world_id and m.user_id = (select auth.uid());
  update public.world_members m set role = 'owner'
  where m.world_id = p_world_id and m.user_id = p_new_owner_id;
  update public.worlds w set owner_id = p_new_owner_id where w.id = p_world_id;
  return true;
end;
$$;
create function public.transfer_world_owner(p_world_id uuid, p_new_owner_id uuid) returns boolean
language sql security invoker set search_path = '' as $$
  select private.transfer_world_owner(p_world_id, p_new_owner_id);
$$;

create function private.leave_world(p_world_id uuid) returns boolean
language plpgsql security definer set search_path = '' as $$
declare
  v_role text;
begin
  perform 1 from public.worlds w where w.id = p_world_id for update;
  select m.role into v_role from public.world_members m
  where m.world_id = p_world_id and m.user_id = (select auth.uid());
  if v_role is null then
    raise exception 'world access denied' using errcode = '42501';
  end if;
  if v_role = 'owner' then
    if (select count(*) from public.world_members m where m.world_id = p_world_id) > 1 then
      raise exception 'transfer ownership before leaving' using errcode = '23514';
    end if;
    delete from public.worlds w where w.id = p_world_id;
  else
    delete from public.world_members m
    where m.world_id = p_world_id and m.user_id = (select auth.uid());
  end if;
  return true;
end;
$$;
create function public.leave_world(p_world_id uuid) returns boolean
language sql security invoker set search_path = '' as $$
  select private.leave_world(p_world_id);
$$;

create function private.list_world_invites(p_world_id uuid)
returns table (invite_id uuid, created_at timestamptz, expires_at timestamptz,
  revoked_at timestamptz, used_at timestamptz)
language plpgsql security definer set search_path = '' as $$
begin
  if (select auth.uid()) is null or not exists (
    select 1 from public.worlds w
    where w.id = p_world_id and w.owner_id = (select auth.uid())
  ) then
    raise exception 'only the owner can list invitations' using errcode = '42501';
  end if;
  return query select i.id, i.created_at, i.expires_at, i.revoked_at, i.used_at
  from public.world_invites i where i.world_id = p_world_id
  order by i.created_at desc;
end;
$$;
create function public.list_world_invites(p_world_id uuid)
returns table (invite_id uuid, created_at timestamptz, expires_at timestamptz,
  revoked_at timestamptz, used_at timestamptz)
language sql security invoker set search_path = '' as $$
  select * from private.list_world_invites(p_world_id);
$$;

revoke all on function private.delete_synced_usage(uuid), private.leave_world(uuid),
  private.transfer_world_owner(uuid, uuid), private.list_world_invites(uuid) from public, anon;
revoke all on function public.delete_synced_usage(uuid), public.leave_world(uuid),
  public.transfer_world_owner(uuid, uuid), public.list_world_invites(uuid) from public, anon;
grant execute on function private.delete_synced_usage(uuid), private.leave_world(uuid),
  private.transfer_world_owner(uuid, uuid), private.list_world_invites(uuid) to authenticated;
grant execute on function public.delete_synced_usage(uuid), public.leave_world(uuid),
  public.transfer_world_owner(uuid, uuid), public.list_world_invites(uuid) to authenticated;

create function private.list_world_members(p_world_id uuid)
returns table (user_id uuid, role text)
language plpgsql security definer set search_path = '' as $$
begin
  if (select auth.uid()) is null or not exists (
    select 1 from public.worlds w
    where w.id = p_world_id and w.owner_id = (select auth.uid())
  ) then
    raise exception 'only the owner can list transfer candidates' using errcode = '42501';
  end if;
  return query select m.user_id, m.role from public.world_members m
  where m.world_id = p_world_id order by m.joined_at;
end;
$$;
create function public.list_world_members(p_world_id uuid)
returns table (user_id uuid, role text)
language sql security invoker set search_path = '' as $$
  select * from private.list_world_members(p_world_id);
$$;
revoke all on function private.list_world_members(uuid), public.list_world_members(uuid) from public, anon;
grant execute on function private.list_world_members(uuid), public.list_world_members(uuid) to authenticated;
