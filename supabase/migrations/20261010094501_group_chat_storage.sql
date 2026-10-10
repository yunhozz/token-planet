create table private.group_chat_state (
  world_id uuid primary key references public.worlds(id) on delete cascade,
  last_message_seq bigint not null default 0 check(last_message_seq>=0),
  last_change_seq bigint not null default 0 check(last_change_seq>=last_message_seq)
);
insert into private.group_chat_state(world_id) select id from public.worlds;
alter table public.world_members add column joined_after_seq bigint not null default 0 check(joined_after_seq>=0);
create table private.group_chat_authors (
  world_id uuid not null references public.worlds(id) on delete cascade,
  user_id uuid not null references auth.users(id) on delete cascade,
  author_key uuid not null default gen_random_uuid(),
  primary key(world_id,user_id), unique(world_id,author_key)
);
create table public.group_chat_messages (
  id uuid primary key default gen_random_uuid(),
  world_id uuid not null references public.worlds(id) on delete cascade,
  message_seq bigint not null check(message_seq>0),
  change_seq bigint not null check(change_seq>0),
  author_key uuid not null,
  nickname text not null check(char_length(nickname) between 1 and 24),
  avatar text not null check(avatar in ('masculine','feminine')),
  body text,
  created_at timestamptz not null default clock_timestamp(),
  deleted_at timestamptz,
  check((deleted_at is null and body is not null and char_length(body) between 1 and 2000 and body ~ '[^[:space:]]') or (deleted_at is not null and body is null)),
  unique(world_id,message_seq), unique(world_id,change_seq)
);
create index group_chat_changes_idx on public.group_chat_messages(world_id,change_seq);
create table private.group_chat_requests (
  world_id uuid not null references public.worlds(id) on delete cascade,
  user_id uuid not null references auth.users(id) on delete cascade,
  request_id uuid not null,
  body_digest bytea not null,
  message_id uuid not null references public.group_chat_messages(id) on delete cascade,
  primary key(world_id,user_id,request_id)
);
create table private.group_chat_reads (
  world_id uuid not null,
  user_id uuid not null,
  last_read_seq bigint not null check(last_read_seq>=0),
  primary key(world_id,user_id),
  foreign key(world_id,user_id) references public.world_members(world_id,user_id) on delete cascade
);
create function private.initialize_group_chat() returns trigger
language plpgsql security definer set search_path='' as $$
begin
  insert into private.group_chat_state(world_id) values(new.id) on conflict do nothing;
  return new;
end;
$$;
-- Alphabetically before add_world_owner, whose INSERT invokes the cutoff trigger.
create trigger aa_initialize_group_chat after insert on public.worlds
for each row execute function private.initialize_group_chat();
create function private.capture_group_chat_join() returns trigger
language plpgsql security definer set search_path='' as $$
begin
  perform 1 from public.worlds where id=new.world_id for update;
  insert into private.group_chat_state(world_id) values(new.world_id) on conflict do nothing;
  select last_message_seq into new.joined_after_seq from private.group_chat_state where world_id=new.world_id for update;
  return new;
end;
$$;
create trigger capture_group_chat_join before insert on public.world_members
for each row execute function private.capture_group_chat_join();
alter table public.group_chat_messages enable row level security;
alter table private.group_chat_state enable row level security;
alter table private.group_chat_authors enable row level security;
alter table private.group_chat_requests enable row level security;
alter table private.group_chat_reads enable row level security;
create policy group_chat_read_member on public.group_chat_messages for select to authenticated
using(exists(select 1 from public.world_members m where m.world_id=group_chat_messages.world_id
  and m.user_id=(select auth.uid()) and group_chat_messages.message_seq>m.joined_after_seq));
revoke all on public.group_chat_messages from public,anon,authenticated;
grant select on public.group_chat_messages to authenticated;
revoke all on private.group_chat_state,private.group_chat_authors,private.group_chat_requests,private.group_chat_reads from public,anon,authenticated;
revoke all on function private.initialize_group_chat(),private.capture_group_chat_join() from public,anon,authenticated;
alter publication supabase_realtime add table public.group_chat_messages;
