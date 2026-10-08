-- Forward-only cutover. Historical memberships and personal-code records are preserved.
alter table public.world_members add column joined_via_invite_id uuid
  references public.world_invites(id) on delete set null;
create index world_members_joined_via_invite_idx on public.world_members(joined_via_invite_id)
  where joined_via_invite_id is not null;
create index world_invites_world_created_idx on public.world_invites(world_id, created_at desc, id);
update public.world_invites set revoked_at = clock_timestamp()
  where used_at is null and revoked_at is null;

-- Return contracts change. Drop wrappers before helpers; never cascade dependencies.
drop function public.create_world_invite(uuid);
drop function public.list_world_invites(uuid);
drop function public.revoke_world_invite(uuid);
drop function public.accept_world_invite(text);
drop function private.create_world_invite(uuid);
drop function private.list_world_invites(uuid);
drop function private.revoke_world_invite(uuid);
drop function private.accept_world_invite(text);

create function private.create_world_invite(p_world_id uuid)
returns table(invite_id uuid, code text, created_at timestamptz, expires_at timestamptz)
language plpgsql security definer set search_path = '' as $$
declare
  v_user uuid := auth.uid();
  v_code text;
  v_created timestamptz;
begin
  if v_user is null then raise exception 'authentication required' using errcode='42501'; end if;
  perform 1 from public.worlds w where w.id=p_world_id and w.owner_id=v_user for update;
  if not found then raise exception 'owner access required' using errcode='42501'; end if;
  if (select count(*) from public.world_members m where m.world_id=p_world_id)>=10 then
    raise exception 'world member limit reached' using errcode='23514';
  end if;
  v_created := clock_timestamp();
  v_code := encode(extensions.gen_random_bytes(32),'hex');
  return query insert into public.world_invites(world_id,code_hash,created_by,created_at,expires_at)
    values(p_world_id,encode(extensions.digest(convert_to(v_code,'UTF8'),'sha256'),'hex'),
      v_user,v_created,v_created+interval '168 hours')
    returning id,v_code,public.world_invites.created_at,public.world_invites.expires_at;
end;
$$;
create function public.create_world_invite(p_world_id uuid)
returns table(invite_id uuid, code text, created_at timestamptz, expires_at timestamptz)
language sql security invoker set search_path = '' as $$ select * from private.create_world_invite(p_world_id); $$;

create function private.list_world_invites(p_world_id uuid)
returns table(invite_id uuid, created_at timestamptz, expires_at timestamptz,
  revoked_at timestamptz, used_at timestamptz, status text)
language plpgsql security definer set search_path = '' as $$
begin
  if auth.uid() is null or not exists(select 1 from public.worlds w where w.id=p_world_id and w.owner_id=auth.uid()) then
    raise exception 'owner access required' using errcode='42501';
  end if;
  return query select i.id,i.created_at,i.expires_at,i.revoked_at,i.used_at,
    case when i.used_at is not null then 'used' when i.revoked_at is not null then 'revoked'
      when i.expires_at<=clock_timestamp() then 'expired' else 'active' end
    from public.world_invites i where i.world_id=p_world_id order by i.created_at desc,i.id;
end;
$$;
create function public.list_world_invites(p_world_id uuid)
returns table(invite_id uuid, created_at timestamptz, expires_at timestamptz,
  revoked_at timestamptz, used_at timestamptz, status text)
language sql security invoker set search_path = '' as $$ select * from private.list_world_invites(p_world_id); $$;

create function private.revoke_world_invite(p_invite_id uuid)
returns table(status text)
language plpgsql security definer set search_path = '' as $$
declare v_world uuid; v_invite public.world_invites%rowtype;
begin
  if auth.uid() is null then raise exception 'authentication required' using errcode='42501'; end if;
  select i.world_id into v_world from public.world_invites i where i.id=p_invite_id;
  perform 1 from public.worlds w where w.id=v_world and w.owner_id=auth.uid() for update;
  if not found then raise exception 'owner access required' using errcode='42501'; end if;
  select * into v_invite from public.world_invites i where i.id=p_invite_id and i.world_id=v_world for update;
  if not found then raise exception 'owner access required' using errcode='42501'; end if;
  if v_invite.used_at is not null then return query select 'used'::text; return; end if;
  if v_invite.revoked_at is not null then return query select 'already_revoked'::text; return; end if;
  update public.world_invites i set revoked_at=clock_timestamp() where i.id=p_invite_id;
  return query select 'revoked'::text;
end;
$$;
create function public.revoke_world_invite(p_invite_id uuid) returns table(status text)
language sql security invoker set search_path = '' as $$ select * from private.revoke_world_invite(p_invite_id); $$;

create function private.accept_world_invite(p_code text)
returns table(status text, world_id uuid)
language plpgsql security definer set search_path = '' as $$
declare
  v_user uuid := auth.uid();
  v_code text;
  v_now timestamptz;
  v_attempt private.member_code_join_attempts%rowtype;
  v_invite public.world_invites%rowtype;
  v_id uuid;
  v_world uuid;
  v_owner uuid;
  v_inserted uuid;
begin
  if v_user is null then raise exception 'authentication required' using errcode='42501'; end if;
  insert into private.member_code_join_attempts(user_id,window_started_at)
    values(v_user,clock_timestamp()) on conflict(user_id) do nothing;
  select * into v_attempt from private.member_code_join_attempts a where a.user_id=v_user for update;
  v_now := clock_timestamp();
  if v_attempt.blocked_until>v_now then return query select 'rate_limited'::text,null::uuid; return; end if;
  if v_attempt.window_started_at<=v_now-interval '15 minutes' then
    update private.member_code_join_attempts a set window_started_at=v_now,failed_attempts=0,blocked_until=null where a.user_id=v_user;
    v_attempt.failed_attempts := 0;
  end if;
  <<decision>>
  begin
    if p_code is null or char_length(p_code)>256 then exit decision; end if;
    v_code := lower(btrim(p_code));
    if v_code !~ '^[0-9a-f]{64}$' then exit decision; end if;
    select i.id,i.world_id into v_id,v_world from public.world_invites i
      where i.code_hash=encode(extensions.digest(convert_to(v_code,'UTF8'),'sha256'),'hex');
    if not found then exit decision; end if;
    select w.owner_id into v_owner from public.worlds w where w.id=v_world for update;
    if not found then exit decision; end if;
    select * into v_invite from public.world_invites i where i.id=v_id and i.world_id=v_world for update;
    if not found then exit decision; end if;
    -- Retry confirms the current membership, even after expiry or owner transfer.
    if v_invite.used_at is not null then
      if v_invite.used_by=v_user and exists(select 1 from public.world_members m
        where m.user_id=v_user and m.world_id=v_world and m.joined_via_invite_id=v_id) then
        return query select 'already_accepted'::text,v_world; return;
      end if;
      exit decision;
    end if;
    v_now := clock_timestamp();
    if v_invite.revoked_at is not null or v_invite.expires_at<=v_now
      or v_invite.created_by is distinct from v_owner then exit decision; end if;
    if exists(select 1 from public.world_members m where m.user_id=v_user) then
      return query select 'already_member'::text,null::uuid; return;
    end if;
    if (select count(*) from public.world_members m where m.world_id=v_world)>=10 then
      return query select 'world_full'::text,null::uuid; return;
    end if;
    insert into public.world_members(world_id,user_id,role,joined_via_invite_id)
      values(v_world,v_user,'member',v_id) on conflict(user_id) do nothing returning user_id into v_inserted;
    if v_inserted is null then return query select 'already_member'::text,null::uuid; return; end if;
    update public.world_invites i set used_at=v_now,used_by=v_user where i.id=v_id;
    return query select 'accepted'::text,v_world; return;
  end decision;
  -- Normal failure return commits the counter; no exception after this write.
  v_now := clock_timestamp();
  update private.member_code_join_attempts a set failed_attempts=v_attempt.failed_attempts+1,
    blocked_until=case when v_attempt.failed_attempts+1>=5 then v_now+interval '15 minutes' else null end
    where a.user_id=v_user;
  return query select 'unavailable'::text,null::uuid;
end;
$$;
create function public.accept_world_invite(p_code text) returns table(status text, world_id uuid)
language sql security invoker set search_path = '' as $$ select * from private.accept_world_invite(p_code); $$;

create or replace function private.transfer_world_owner(p_world_id uuid,p_new_owner_id uuid) returns boolean
language plpgsql security definer set search_path = '' as $$
begin
  if auth.uid() is null then raise exception 'authentication required' using errcode='42501'; end if;
  perform 1 from public.worlds w where w.id=p_world_id and w.owner_id=auth.uid() for update;
  if not found then raise exception 'owner access required' using errcode='42501'; end if;
  if p_new_owner_id=auth.uid() or not exists(select 1 from public.world_members m
    where m.world_id=p_world_id and m.user_id=p_new_owner_id and m.role='member') then
    raise exception 'new owner must be another member' using errcode='23514';
  end if;
  perform 1 from public.world_invites i where i.world_id=p_world_id and i.used_at is null and i.revoked_at is null order by i.id for update;
  update public.world_invites i set revoked_at=clock_timestamp() where i.world_id=p_world_id and i.used_at is null and i.revoked_at is null;
  update public.world_members m set role='member' where m.world_id=p_world_id and m.user_id=auth.uid();
  update public.world_members m set role='owner' where m.world_id=p_world_id and m.user_id=p_new_owner_id;
  update public.worlds w set owner_id=p_new_owner_id where w.id=p_world_id;
  return true;
end;
$$;
-- Deny every caller, including stale clients. Keep the data and signature intact.
create or replace function public.join_world_by_member_code(p_code text)
returns table(world_id uuid,member_count integer,known_tokens numeric,growth_credit numeric,
  stage smallint,progress_to_next numeric,incomplete boolean,last_update timestamptz)
language plpgsql security invoker set search_path = '' as $$
begin raise exception 'client update required' using errcode='42501'; end;
$$;
revoke all on function public.join_world_by_member_code(text) from public,anon,authenticated;
revoke all on public.world_invites from public,anon,authenticated;
revoke all on function public.create_world_invite(uuid),public.list_world_invites(uuid),
  public.revoke_world_invite(uuid),public.accept_world_invite(text),
  private.create_world_invite(uuid),private.list_world_invites(uuid),
  private.revoke_world_invite(uuid),private.accept_world_invite(text) from public,anon;
grant execute on function public.create_world_invite(uuid),public.list_world_invites(uuid),
  public.revoke_world_invite(uuid),public.accept_world_invite(text),
  private.create_world_invite(uuid),private.list_world_invites(uuid),
  private.revoke_world_invite(uuid),private.accept_world_invite(text) to authenticated;
