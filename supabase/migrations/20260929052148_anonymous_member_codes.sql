create table private.user_member_codes (
  user_id uuid primary key references auth.users(id) on delete cascade,
  code text not null unique check (code ~ '^[23456789ABCDEFGHJKLMNPQRSTUVWXYZ]{10}$'),
  updated_at timestamptz not null default now()
);

create table private.member_code_join_attempts (
  user_id uuid primary key references auth.users(id) on delete cascade,
  window_started_at timestamptz not null default now(),
  failed_attempts integer not null default 0 check (failed_attempts between 0 and 5),
  blocked_until timestamptz
);

alter table private.user_member_codes enable row level security;
alter table private.member_code_join_attempts enable row level security;
revoke all on private.user_member_codes, private.member_code_join_attempts
  from public, anon, authenticated;

create function private.issue_member_code(p_user_id uuid, p_rotate boolean)
returns text
language plpgsql security definer set search_path = '' as $$
declare
  v_code text;
  v_random_byte integer;
  v_index integer;
begin
  if p_user_id is null or p_rotate is null then
    raise exception 'user id and rotation flag are required' using errcode = '22004';
  end if;

  perform 1 from auth.users u where u.id = p_user_id for update;
  if not found then
    raise exception 'user does not exist' using errcode = '23503';
  end if;

  if not p_rotate then
    select c.code into v_code from private.user_member_codes c where c.user_id = p_user_id;
    if found then
      return v_code;
    end if;
  end if;

  loop
    v_code := '';
    for v_index in 1..10 loop
      v_random_byte := pg_catalog.get_byte(extensions.gen_random_bytes(1), 0);
      v_code := v_code || pg_catalog.substr(
        '23456789ABCDEFGHJKLMNPQRSTUVWXYZ', (v_random_byte % 32) + 1, 1
      );
    end loop;

    begin
      if p_rotate then
        update private.user_member_codes c
        set code = v_code, updated_at = pg_catalog.now()
        where c.user_id = p_user_id;
        if not found then
          insert into private.user_member_codes(user_id, code) values (p_user_id, v_code);
        end if;
      else
        insert into private.user_member_codes(user_id, code) values (p_user_id, v_code);
      end if;
      return v_code;
    exception when unique_violation then
      -- A code collision is extraordinarily unlikely; generate another code.
      null;
    end;
  end loop;
end;
$$;

create function private.assign_member_code_to_new_user()
returns trigger
language plpgsql security definer set search_path = '' as $$
begin
  perform private.issue_member_code(new.id, false);
  return new;
end;
$$;

create trigger assign_member_code_to_new_user
after insert on auth.users
for each row execute function private.assign_member_code_to_new_user();

do $$
declare
  v_user record;
begin
  for v_user in
    select u.id from auth.users u
    where not exists (
      select 1 from private.user_member_codes c where c.user_id = u.id
    )
    order by u.id
  loop
    perform private.issue_member_code(v_user.id, false);
  end loop;
end;
$$;

create function public.get_my_member_code()
returns text
language plpgsql security definer set search_path = '' as $$
declare
  v_user_id uuid := (select auth.uid());
  v_code text;
begin
  if v_user_id is null then
    raise exception 'authentication required' using errcode = '42501';
  end if;
  select c.code into v_code from private.user_member_codes c where c.user_id = v_user_id;
  if not found then
    raise exception 'member code is unavailable' using errcode = 'P0002';
  end if;
  return v_code;
end;
$$;

create function public.rotate_my_member_code()
returns text
language plpgsql security definer set search_path = '' as $$
declare
  v_user_id uuid := (select auth.uid());
begin
  if v_user_id is null then
    raise exception 'authentication required' using errcode = '42501';
  end if;
  return private.issue_member_code(v_user_id, true);
end;
$$;

create function public.join_world_by_member_code(p_code text)
returns table (
  world_id uuid, member_count integer, known_tokens numeric, growth_credit numeric,
  stage smallint, progress_to_next numeric, incomplete boolean, last_update timestamptz
)
language plpgsql security definer set search_path = '' as $$
declare
  v_user_id uuid := (select auth.uid());
  v_now timestamptz := pg_catalog.now();
  v_code text := pg_catalog.upper(pg_catalog.btrim(coalesce(p_code, '')));
  v_window_started_at timestamptz;
  v_failed_attempts integer;
  v_blocked_until timestamptz;
  v_owner_id uuid;
  v_current_owner_id uuid;
  v_world_id uuid;
begin
  if v_user_id is null then
    raise exception 'authentication required' using errcode = '42501';
  end if;

  insert into private.member_code_join_attempts(user_id)
  values (v_user_id) on conflict (user_id) do nothing;
  select a.window_started_at, a.failed_attempts, a.blocked_until
  into v_window_started_at, v_failed_attempts, v_blocked_until
  from private.member_code_join_attempts a where a.user_id = v_user_id for update;

  if v_blocked_until > v_now then
    return;
  end if;
  if v_window_started_at <= v_now - interval '15 minutes' then
    update private.member_code_join_attempts a
    set window_started_at = v_now, failed_attempts = 0, blocked_until = null
    where a.user_id = v_user_id;
    v_failed_attempts := 0;
  end if;

  if v_code !~ '^[23456789ABCDEFGHJKLMNPQRSTUVWXYZ]{10}$' then
    update private.member_code_join_attempts a
    set failed_attempts = v_failed_attempts + 1,
        blocked_until = case when v_failed_attempts + 1 >= 5
          then v_now + interval '15 minutes' else null end
    where a.user_id = v_user_id;
    return;
  end if;

  select c.user_id, w.id into v_owner_id, v_world_id
  from private.user_member_codes c
  join public.worlds w on w.owner_id = c.user_id
  where c.code = v_code
  for update of c;
  if not found then
    update private.member_code_join_attempts a
    set failed_attempts = v_failed_attempts + 1,
        blocked_until = case when v_failed_attempts + 1 >= 5
          then v_now + interval '15 minutes' else null end
    where a.user_id = v_user_id;
    return;
  end if;

  if exists (select 1 from public.world_members m where m.user_id = v_user_id) then
    raise exception 'account already belongs to a shared world' using errcode = '23514';
  end if;

  select w.owner_id into v_current_owner_id
  from public.worlds w where w.id = v_world_id for update;
  if not found or v_current_owner_id is distinct from v_owner_id then
    update private.member_code_join_attempts a
    set failed_attempts = v_failed_attempts + 1,
        blocked_until = case when v_failed_attempts + 1 >= 5
          then v_now + interval '15 minutes' else null end
    where a.user_id = v_user_id;
    return;
  end if;
  if (select count(*) from public.world_members m where m.world_id = v_world_id) >= 10 then
    raise exception 'world member limit reached' using errcode = '23514';
  end if;

  insert into public.world_members(world_id, user_id, role)
  values (v_world_id, v_user_id, 'member');
  return query select * from private.compute_world_summary(v_world_id);
end;
$$;

revoke all on function private.issue_member_code(uuid, boolean),
  private.assign_member_code_to_new_user() from public, anon, authenticated;
revoke all on function public.get_my_member_code(), public.rotate_my_member_code(),
  public.join_world_by_member_code(text) from public, anon;
grant execute on function public.get_my_member_code(), public.rotate_my_member_code(),
  public.join_world_by_member_code(text) to authenticated;

revoke all on function private.create_world_invite(uuid), private.accept_world_invite(text),
  private.revoke_world_invite(uuid), private.list_world_invites(uuid),
  public.create_world_invite(uuid), public.accept_world_invite(text),
  public.revoke_world_invite(uuid), public.list_world_invites(uuid)
  from public, anon, authenticated;
