-- Internal helpers are not callable by API roles. All callers lock world before chat state.
create function private.lock_group_chat(p_world_id uuid) returns bigint
language plpgsql security definer set search_path='' as $$
declare v_cutoff bigint;
begin
  if auth.uid() is null then raise exception 'authentication required' using errcode='42501'; end if;
  perform 1 from public.worlds where id=p_world_id for update;
  if not found then raise exception 'world access denied' using errcode='42501'; end if;
  select joined_after_seq into v_cutoff from public.world_members where world_id=p_world_id and user_id=auth.uid();
  if not found then raise exception 'world access denied' using errcode='42501'; end if;
  perform 1 from private.group_chat_state where world_id=p_world_id for update;
  insert into private.group_chat_authors(world_id,user_id) values(p_world_id,auth.uid()) on conflict do nothing;
  insert into private.group_chat_reads(world_id,user_id,last_read_seq) values(p_world_id,auth.uid(),v_cutoff) on conflict do nothing;
  return v_cutoff;
end;
$$;
create function private.group_chat_message_json(p public.group_chat_messages) returns jsonb
language sql immutable set search_path='' as $$
select jsonb_build_object('id',p.id,'world_id',p.world_id,'message_seq',p.message_seq::text,'change_seq',p.change_seq::text,
 'author_key',p.author_key,'nickname',p.nickname,'avatar',p.avatar,'body',p.body,'created_at',p.created_at,'deleted_at',p.deleted_at);
$$;
create function private.group_chat_context_json(p_world_id uuid) returns jsonb
language sql stable set search_path='' as $$
select jsonb_build_object('world_id',s.world_id,'joined_after_seq',m.joined_after_seq::text,'author_key',a.author_key,
 'last_read_seq',r.last_read_seq::text,'last_message_seq',s.last_message_seq::text,'last_change_seq',s.last_change_seq::text,
 'unread_count',(select count(*) from public.group_chat_messages x where x.world_id=s.world_id
  and x.message_seq>greatest(m.joined_after_seq,r.last_read_seq) and x.author_key<>a.author_key and x.deleted_at is null)::text)
from private.group_chat_state s join public.world_members m on m.world_id=s.world_id and m.user_id=auth.uid()
join private.group_chat_authors a on a.world_id=s.world_id and a.user_id=auth.uid()
join private.group_chat_reads r on r.world_id=s.world_id and r.user_id=auth.uid() where s.world_id=p_world_id;
$$;
create function public.get_group_chat_context(p_world_id uuid) returns jsonb
language plpgsql security definer set search_path='' as $$
begin
  perform private.lock_group_chat(p_world_id);
  return private.group_chat_context_json(p_world_id);
end;
$$;
create function public.send_group_chat_message(p_world_id uuid,p_request_id uuid,p_body text) returns jsonb
language plpgsql security definer set search_path='' as $$
declare v_digest bytea; v_request private.group_chat_requests%rowtype; v_message public.group_chat_messages%rowtype;
 v_key uuid; v_nickname text; v_avatar text; v_message_seq bigint; v_change_seq bigint;
begin
  perform private.lock_group_chat(p_world_id);
  if p_request_id is null or p_body is null or char_length(p_body) not between 1 and 2000 or p_body !~ '[^[:space:]]' then
    raise exception 'invalid chat message' using errcode='23514';
  end if;
  v_digest:=extensions.digest(convert_to(p_body,'UTF8'),'sha256');
  select * into v_request from private.group_chat_requests where world_id=p_world_id and user_id=auth.uid() and request_id=p_request_id;
  if found then
    if v_request.body_digest<>v_digest then raise exception 'request body mismatch' using errcode='23514'; end if;
    select * into v_message from public.group_chat_messages where id=v_request.message_id;
    -- A retry from an earlier membership must not expose a prejoin message.
    if v_message.message_seq<=(select joined_after_seq from public.world_members where world_id=p_world_id and user_id=auth.uid()) then
      raise exception 'request predates membership' using errcode='42501';
    end if;
    return private.group_chat_message_json(v_message);
  end if;
  select author_key into v_key from private.group_chat_authors where world_id=p_world_id and user_id=auth.uid();
  select nickname,avatar into v_nickname,v_avatar from public.planet_member_state where user_id=auth.uid();
  v_nickname:=coalesce(v_nickname,'행성 동기화 대기'); v_avatar:=coalesce(v_avatar,'masculine');
  update private.group_chat_state set last_message_seq=last_message_seq+1,last_change_seq=last_change_seq+1 where world_id=p_world_id
    returning last_message_seq,last_change_seq into v_message_seq,v_change_seq;
  insert into public.group_chat_messages(world_id,message_seq,change_seq,author_key,nickname,avatar,body)
    values(p_world_id,v_message_seq,v_change_seq,v_key,v_nickname,v_avatar,p_body) returning * into v_message;
  insert into private.group_chat_requests(world_id,user_id,request_id,body_digest,message_id)
    values(p_world_id,auth.uid(),p_request_id,v_digest,v_message.id);
  return private.group_chat_message_json(v_message);
end;
$$;
create function public.delete_group_chat_message(p_world_id uuid,p_message_id uuid) returns jsonb
language plpgsql security definer set search_path='' as $$
declare v_cutoff bigint; v_message public.group_chat_messages%rowtype; v_change bigint;
begin
  v_cutoff:=private.lock_group_chat(p_world_id);
  select * into v_message from public.group_chat_messages where world_id=p_world_id and id=p_message_id and message_seq>v_cutoff;
  if not found or v_message.author_key<>(select author_key from private.group_chat_authors where world_id=p_world_id and user_id=auth.uid()) then
    raise exception 'message access denied' using errcode='42501';
  end if;
  if v_message.deleted_at is null then
    update private.group_chat_state set last_change_seq=last_change_seq+1 where world_id=p_world_id returning last_change_seq into v_change;
    update public.group_chat_messages set body=null,deleted_at=clock_timestamp(),change_seq=v_change where id=p_message_id returning * into v_message;
  end if;
  return private.group_chat_message_json(v_message);
end;
$$;
create function public.mark_group_chat_read(p_world_id uuid,p_message_seq bigint) returns jsonb
language plpgsql security definer set search_path='' as $$
declare v_cutoff bigint; v_context jsonb;
begin
  v_cutoff:=private.lock_group_chat(p_world_id);
  if p_message_seq is null or p_message_seq<v_cutoff or p_message_seq<(select last_read_seq from private.group_chat_reads where world_id=p_world_id and user_id=auth.uid())
   or p_message_seq>(select last_message_seq from private.group_chat_state where world_id=p_world_id) then
    raise exception 'invalid read cursor' using errcode='23514';
  end if;
  update private.group_chat_reads set last_read_seq=p_message_seq where world_id=p_world_id and user_id=auth.uid();
  v_context:=private.group_chat_context_json(p_world_id);
  return jsonb_build_object('last_read_seq',v_context->'last_read_seq','unread_count',v_context->'unread_count');
end;
$$;
create function public.list_group_chat_messages(p_world_id uuid,p_before_seq bigint default null,p_limit integer default 50) returns jsonb
language plpgsql security definer set search_path='' as $$
declare v_cutoff bigint; v_messages jsonb; v_next bigint; v_more boolean;
begin
  v_cutoff:=private.lock_group_chat(p_world_id);
  if p_limit is null or p_limit not between 1 and 50 or p_before_seq<0 then raise exception 'invalid page cursor' using errcode='23514'; end if;
  select coalesce(jsonb_agg(private.group_chat_message_json(q) order by q.message_seq),'[]'::jsonb),min(q.message_seq)
    into v_messages,v_next from (select * from public.group_chat_messages where world_id=p_world_id and message_seq>v_cutoff
      and (p_before_seq is null or message_seq<p_before_seq) order by message_seq desc limit p_limit) q;
  select exists(select 1 from public.group_chat_messages where world_id=p_world_id and message_seq>v_cutoff and message_seq<v_next) into v_more;
  return jsonb_build_object('messages',v_messages,'next_cursor',case when v_more then v_next::text else null end,'has_more',v_more);
end;
$$;
create function public.sync_group_chat_changes(p_world_id uuid,p_after_change_seq bigint,p_until_change_seq bigint,p_limit integer default 50) returns jsonb
language plpgsql security definer set search_path='' as $$
declare v_cutoff bigint; v_messages jsonb; v_next bigint; v_more boolean; v_last bigint;
begin
  v_cutoff:=private.lock_group_chat(p_world_id);
  select last_change_seq into v_last from private.group_chat_state where world_id=p_world_id;
  if p_limit is null or p_limit not between 1 and 50 or p_after_change_seq is null or p_until_change_seq is null or p_after_change_seq<0
    or p_until_change_seq<p_after_change_seq or p_until_change_seq>v_last then raise exception 'invalid change cursor' using errcode='23514'; end if;
  select coalesce(jsonb_agg(private.group_chat_message_json(q) order by q.change_seq),'[]'::jsonb),max(q.change_seq)
    into v_messages,v_next from (select * from public.group_chat_messages where world_id=p_world_id and message_seq>v_cutoff
      and change_seq>p_after_change_seq and change_seq<=p_until_change_seq order by change_seq limit p_limit) q;
  select exists(select 1 from public.group_chat_messages where world_id=p_world_id and message_seq>v_cutoff and change_seq>v_next and change_seq<=p_until_change_seq) into v_more;
  return jsonb_build_object('messages',v_messages,'next_cursor',(case when v_more then v_next else p_until_change_seq end)::text,'has_more',v_more);
end;
$$;
revoke all on function private.lock_group_chat(uuid),private.group_chat_message_json(public.group_chat_messages),private.group_chat_context_json(uuid) from public,anon,authenticated;
revoke all on function public.get_group_chat_context(uuid),public.send_group_chat_message(uuid,uuid,text),public.delete_group_chat_message(uuid,uuid),
 public.mark_group_chat_read(uuid,bigint),public.list_group_chat_messages(uuid,bigint,integer),public.sync_group_chat_changes(uuid,bigint,bigint,integer) from public,anon;
grant execute on function public.get_group_chat_context(uuid),public.send_group_chat_message(uuid,uuid,text),public.delete_group_chat_message(uuid,uuid),
 public.mark_group_chat_read(uuid,bigint),public.list_group_chat_messages(uuid,bigint,integer),public.sync_group_chat_changes(uuid,bigint,bigint,integer) to authenticated;
