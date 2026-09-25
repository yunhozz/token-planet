create table public.world_invites (
  id uuid primary key default gen_random_uuid(),
  world_id uuid not null references public.worlds(id) on delete cascade,
  code_hash text not null unique check (code_hash ~ '^[0-9a-f]{64}$'),
  created_by uuid references auth.users(id) on delete set null,
  created_at timestamptz not null default now(),
  expires_at timestamptz not null,
  revoked_at timestamptz,
  used_at timestamptz,
  used_by uuid references auth.users(id) on delete set null
);
alter table public.world_invites enable row level security;
revoke all on public.world_invites from anon, authenticated;

create function private.create_world_invite(p_world_id uuid)
returns table (invite_id uuid, code text, expires_at timestamptz)
language plpgsql security definer set search_path = '' as $$
declare
  v_code text;
begin
  perform 1 from public.worlds w
  where w.id = p_world_id and w.owner_id = (select auth.uid())
  for update;
  if not found then
    raise exception 'only the world owner can create an invitation' using errcode = '42501';
  end if;
  if (select count(*) from public.world_members m where m.world_id = p_world_id) >= 10 then
    raise exception 'world member limit reached' using errcode = '23514';
  end if;
  v_code := encode(extensions.gen_random_bytes(32), 'hex');
  return query
    insert into public.world_invites (world_id, code_hash, created_by, expires_at)
    values (p_world_id, encode(extensions.digest(convert_to(v_code, 'UTF8'), 'sha256'), 'hex'),
      (select auth.uid()), now() + interval '7 days')
    returning id, v_code, public.world_invites.expires_at;
end;
$$;
create function public.create_world_invite(p_world_id uuid)
returns table (invite_id uuid, code text, expires_at timestamptz)
language sql security invoker set search_path = '' as $$
  select * from private.create_world_invite(p_world_id);
$$;

create function private.accept_world_invite(p_code text)
returns table (
  world_id uuid, member_count integer, known_tokens numeric, growth_credit numeric,
  stage smallint, progress_to_next numeric, incomplete boolean, last_update timestamptz
)
language plpgsql security definer set search_path = '' as $$
declare
  v_invite_id uuid;
  v_world_id uuid;
  v_expires_at timestamptz;
  v_revoked_at timestamptz;
  v_used_at timestamptz;
begin
  if (select auth.uid()) is null or p_code !~ '^[0-9a-f]{64}$' then
    raise exception 'invitation is invalid' using errcode = '23514';
  end if;
  select i.id, i.world_id, i.expires_at, i.revoked_at, i.used_at
  into v_invite_id, v_world_id, v_expires_at, v_revoked_at, v_used_at
  from public.world_invites i
  where i.code_hash = encode(extensions.digest(convert_to(p_code, 'UTF8'), 'sha256'), 'hex')
  for update;
  if v_invite_id is null or v_expires_at <= now() or v_revoked_at is not null or v_used_at is not null then
    raise exception 'invitation is unavailable' using errcode = '23514';
  end if;
  perform 1 from public.worlds w where w.id = v_world_id for update;
  if exists (select 1 from public.world_members m where m.user_id = (select auth.uid())) then
    raise exception 'account already belongs to a shared world' using errcode = '23514';
  end if;
  if (select count(*) from public.world_members m where m.world_id = v_world_id) >= 10 then
    raise exception 'world member limit reached' using errcode = '23514';
  end if;
  insert into public.world_members(world_id, user_id, role)
  values (v_world_id, (select auth.uid()), 'member');
  update public.world_invites i set used_at = now(), used_by = (select auth.uid())
  where i.id = v_invite_id;
  return query select * from private.compute_world_summary(v_world_id);
end;
$$;
create function public.accept_world_invite(p_code text)
returns table (
  world_id uuid, member_count integer, known_tokens numeric, growth_credit numeric,
  stage smallint, progress_to_next numeric, incomplete boolean, last_update timestamptz
)
language sql security invoker set search_path = '' as $$
  select * from private.accept_world_invite(p_code);
$$;

create function private.revoke_world_invite(p_invite_id uuid) returns boolean
language plpgsql security definer set search_path = '' as $$
declare
  v_world_id uuid;
begin
  select i.world_id into v_world_id from public.world_invites i
  where i.id = p_invite_id for update;
  if (select auth.uid()) is null or v_world_id is null or not exists (
    select 1 from public.worlds w where w.id = v_world_id and w.owner_id = (select auth.uid())
  ) then
    raise exception 'only the world owner can revoke an invitation' using errcode = '42501';
  end if;
  update public.world_invites i set revoked_at = now()
  where i.id = p_invite_id and i.revoked_at is null and i.used_at is null;
  return found;
end;
$$;
create function public.revoke_world_invite(p_invite_id uuid) returns boolean
language sql security invoker set search_path = '' as $$
  select private.revoke_world_invite(p_invite_id);
$$;

revoke all on function private.create_world_invite(uuid), private.accept_world_invite(text), private.revoke_world_invite(uuid) from public, anon;
revoke all on function public.create_world_invite(uuid), public.accept_world_invite(text), public.revoke_world_invite(uuid) from public, anon;
grant execute on function private.create_world_invite(uuid), private.accept_world_invite(text), private.revoke_world_invite(uuid) to authenticated;
grant execute on function public.create_world_invite(uuid), public.accept_world_invite(text), public.revoke_world_invite(uuid) to authenticated;
