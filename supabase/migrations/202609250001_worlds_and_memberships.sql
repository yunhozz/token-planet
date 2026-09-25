create schema if not exists private;

create table public.worlds (
  id uuid primary key default gen_random_uuid(),
  owner_id uuid not null references auth.users(id) on delete restrict,
  name text not null check (char_length(name) between 1 and 80),
  timezone text not null,
  created_at timestamptz not null default now()
);

create table public.world_members (
  world_id uuid not null references public.worlds(id) on delete cascade,
  user_id uuid not null references auth.users(id) on delete cascade,
  role text not null check (role in ('owner', 'member')),
  joined_at timestamptz not null default now(),
  primary key (world_id, user_id),
  unique (user_id)
);

create unique index one_owner_per_world on public.world_members(world_id) where role = 'owner';

create function private.validate_world_timezone() returns trigger
language plpgsql set search_path = '' as $$
begin
  if tg_op = 'UPDATE' and new.timezone <> old.timezone then
    raise exception 'world timezone is fixed' using errcode = '23514';
  end if;
  if not exists (select 1 from pg_catalog.pg_timezone_names where name = new.timezone) then
    raise exception 'world timezone is not recognized' using errcode = '23514';
  end if;
  return new;
end;
$$;
create trigger validate_world_timezone before insert or update of timezone on public.worlds
for each row execute function private.validate_world_timezone();

create function private.limit_world_members() returns trigger
language plpgsql security definer set search_path = '' as $$
begin
  perform 1 from public.worlds where id = new.world_id for update;
  if (select count(*) from public.world_members where world_id = new.world_id) >= 10 then
    raise exception 'world member limit reached' using errcode = '23514';
  end if;
  return new;
end;
$$;
create trigger limit_world_members before insert on public.world_members
for each row execute function private.limit_world_members();

create function private.add_world_owner() returns trigger
language plpgsql security definer set search_path = '' as $$
begin
  insert into public.world_members(world_id, user_id, role)
  values (new.id, new.owner_id, 'owner');
  return new;
end;
$$;
create trigger add_world_owner after insert on public.worlds
for each row execute function private.add_world_owner();

alter table public.worlds enable row level security;
alter table public.world_members enable row level security;

create policy worlds_read_member on public.worlds for select to authenticated
  using (exists (
    select 1 from public.world_members m
    where m.world_id = id and m.user_id = (select auth.uid())
  ));
create policy worlds_create_self on public.worlds for insert to authenticated
  with check (owner_id = (select auth.uid()));
create policy world_members_read_self on public.world_members for select to authenticated
  using (user_id = (select auth.uid()));

revoke all on public.worlds, public.world_members from anon;
revoke insert, update, delete on public.world_members from authenticated;
revoke update, delete on public.worlds from authenticated;
grant select, insert on public.worlds to authenticated;
grant select on public.world_members to authenticated;
