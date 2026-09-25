create table private.member_sync_policy (
  world_id uuid not null references public.worlds(id) on delete cascade,
  user_id uuid not null references auth.users(id) on delete cascade,
  deleted_through date not null,
  paused boolean not null default true,
  primary key (world_id, user_id)
);
revoke all on private.member_sync_policy from public, anon, authenticated;

create or replace function private.delete_synced_usage(p_world_id uuid) returns integer
language plpgsql security definer set search_path = '' as $$
declare
  v_deleted integer;
  v_timezone text;
  v_cutoff date;
begin
  select w.timezone into v_timezone from public.worlds w
  where w.id = p_world_id for update;
  if (select auth.uid()) is null or v_timezone is null or not exists (
    select 1 from public.world_members m
    where m.world_id = p_world_id and m.user_id = (select auth.uid())
  ) then
    raise exception 'world access denied' using errcode = '42501';
  end if;
  v_cutoff := (now() at time zone v_timezone)::date;
  insert into private.member_sync_policy(world_id, user_id, deleted_through, paused)
  values (p_world_id, (select auth.uid()), v_cutoff, true)
  on conflict (world_id, user_id) do update set
    deleted_through = greatest(private.member_sync_policy.deleted_through, excluded.deleted_through),
    paused = true;
  delete from public.daily_usage_snapshots s
  where s.world_id = p_world_id and s.user_id = (select auth.uid());
  get diagnostics v_deleted = row_count;
  return v_deleted;
end;
$$;

create function private.get_my_sync_policy(p_world_id uuid)
returns table (deleted_through date, paused boolean)
language plpgsql security definer set search_path = '' as $$
begin
  if (select auth.uid()) is null or not exists (
    select 1 from public.world_members m
    where m.world_id = p_world_id and m.user_id = (select auth.uid())
  ) then
    raise exception 'world access denied' using errcode = '42501';
  end if;
  return query select p.deleted_through, p.paused from private.member_sync_policy p
    where p.world_id = p_world_id and p.user_id = (select auth.uid());
  if not found then
    return query select null::date, false;
  end if;
end;
$$;
create function public.get_my_sync_policy(p_world_id uuid)
returns table (deleted_through date, paused boolean)
language sql security invoker set search_path = '' as $$
  select * from private.get_my_sync_policy(p_world_id);
$$;

create function private.resume_my_sync(p_world_id uuid) returns boolean
language plpgsql security definer set search_path = '' as $$
begin
  if (select auth.uid()) is null or not exists (
    select 1 from public.world_members m
    where m.world_id = p_world_id and m.user_id = (select auth.uid())
  ) then
    raise exception 'world access denied' using errcode = '42501';
  end if;
  update private.member_sync_policy p set paused = false
  where p.world_id = p_world_id and p.user_id = (select auth.uid());
  return true;
end;
$$;
create function public.resume_my_sync(p_world_id uuid) returns boolean
language sql security invoker set search_path = '' as $$
  select private.resume_my_sync(p_world_id);
$$;

create function private.guard_deleted_usage() returns trigger
language plpgsql security definer set search_path = '' as $$
declare
  v_cutoff date;
  v_paused boolean;
begin
  select p.deleted_through, p.paused into v_cutoff, v_paused
  from private.member_sync_policy p
  where p.world_id = new.world_id and p.user_id = new.user_id;
  if new.bucket_date <= v_cutoff then
    return null;
  end if;
  if v_paused then
    raise exception 'sync paused after deletion' using errcode = '42501';
  end if;
  return new;
end;
$$;
create trigger guard_deleted_usage before insert or update on public.daily_usage_snapshots
for each row execute function private.guard_deleted_usage();

create or replace function private.leave_world(p_world_id uuid) returns boolean
language plpgsql security definer set search_path = '' as $$
declare
  v_role text;
  v_timezone text;
begin
  select w.timezone into v_timezone from public.worlds w where w.id = p_world_id for update;
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
    insert into private.member_sync_policy(world_id, user_id, deleted_through, paused)
    values (p_world_id, (select auth.uid()), (now() at time zone v_timezone)::date, true)
    on conflict (world_id, user_id) do update set
      deleted_through = greatest(private.member_sync_policy.deleted_through, excluded.deleted_through),
      paused = true;
    delete from public.world_members m
    where m.world_id = p_world_id and m.user_id = (select auth.uid());
  end if;
  return true;
end;
$$;

revoke all on function private.get_my_sync_policy(uuid), private.resume_my_sync(uuid),
  private.guard_deleted_usage(), public.get_my_sync_policy(uuid), public.resume_my_sync(uuid)
  from public, anon;
grant execute on function private.get_my_sync_policy(uuid), private.resume_my_sync(uuid),
  public.get_my_sync_policy(uuid), public.resume_my_sync(uuid) to authenticated;
