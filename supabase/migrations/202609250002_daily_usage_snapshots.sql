create table public.daily_usage_snapshots (
  world_id uuid not null,
  user_id uuid not null,
  device_id uuid not null,
  bucket_date date not null,
  bucket_policy_version smallint not null check (bucket_policy_version > 0),
  agent text not null check (agent in ('codex', 'claude_code')),
  schema_version smallint not null check (schema_version > 0),
  revision bigint not null check (revision > 0),
  input_tokens bigint check (input_tokens >= 0),
  output_tokens bigint check (output_tokens >= 0),
  cache_read_tokens bigint check (cache_read_tokens >= 0),
  cache_write_tokens bigint check (cache_write_tokens >= 0),
  total_tokens bigint check (total_tokens >= 0),
  coverage text not null check (coverage in ('complete', 'partial', 'unavailable', 'unsupported', 'user_disabled')),
  payload_hash text not null,
  updated_at timestamptz not null default now(),
  primary key (world_id, user_id, device_id, bucket_date, agent),
  foreign key (world_id, user_id) references public.world_members(world_id, user_id) on delete cascade,
  check (coverage not in ('complete') or total_tokens is not null),
  check (coverage not in ('unavailable', 'unsupported', 'user_disabled') or total_tokens is null)
);

alter table public.daily_usage_snapshots enable row level security;
create policy snapshots_read_self on public.daily_usage_snapshots for select to authenticated
  using (user_id = (select auth.uid()));
revoke all on public.daily_usage_snapshots from anon, authenticated;
grant select on public.daily_usage_snapshots to authenticated;

create function private.compute_world_summary(p_world_id uuid)
returns table (
  world_id uuid, member_count integer, known_tokens numeric, growth_credit numeric,
  stage smallint, progress_to_next numeric, incomplete boolean, last_update timestamptz
)
language plpgsql security definer set search_path = '' as $$
declare
  v_member_count integer;
  v_known_tokens numeric;
  v_growth_credit numeric;
  v_stage smallint;
  v_progress numeric;
  v_incomplete boolean;
  v_last_update timestamptz;
begin
  if (select auth.uid()) is null or not exists (
    select 1 from public.world_members m
    where m.world_id = p_world_id and m.user_id = (select auth.uid())
  ) then
    raise exception 'world access denied' using errcode = '42501';
  end if;
  select count(*)::integer into v_member_count
  from public.world_members m where m.world_id = p_world_id;

  select sum(s.total_tokens)::numeric, max(s.updated_at)
  into v_known_tokens, v_last_update
  from public.daily_usage_snapshots s where s.world_id = p_world_id;

  select coalesce(sum(log(2::numeric, 1::numeric + d.tokens / 100000::numeric)), 0)
  into v_growth_credit
  from (
    select s.user_id, s.bucket_date, sum(s.total_tokens)::numeric as tokens
    from public.daily_usage_snapshots s
    where s.world_id = p_world_id and s.total_tokens is not null
      and s.coverage <> 'user_disabled'
    group by s.user_id, s.bucket_date
  ) d;

  select exists (
    select 1 from public.world_members m
    cross join (values ('codex'), ('claude_code')) as a(agent)
    where m.world_id = p_world_id and not exists (
      select 1 from public.daily_usage_snapshots s
      where s.world_id = m.world_id and s.user_id = m.user_id and s.agent = a.agent
    )
  ) or exists (
    select 1 from public.daily_usage_snapshots s
    where s.world_id = p_world_id and s.coverage not in ('complete', 'user_disabled')
  ) into v_incomplete;

  v_stage := case
    when v_growth_credit >= 100 then 4
    when v_growth_credit >= 50 then 3
    when v_growth_credit >= 20 then 2
    when v_growth_credit >= 5 then 1
    else 0
  end;
  v_progress := case v_stage
    when 4 then 1
    when 3 then (v_growth_credit - 50) / 50
    when 2 then (v_growth_credit - 20) / 30
    when 1 then (v_growth_credit - 5) / 15
    else v_growth_credit / 5
  end;
  return query select p_world_id, v_member_count, v_known_tokens, v_growth_credit,
    v_stage, v_progress, v_incomplete, v_last_update;
end;
$$;

create function public.get_world_summary(p_world_id uuid)
returns table (
  world_id uuid, member_count integer, known_tokens numeric, growth_credit numeric,
  stage smallint, progress_to_next numeric, incomplete boolean, last_update timestamptz
)
language sql security invoker set search_path = '' as $$
  select * from private.compute_world_summary(p_world_id);
$$;

revoke all on function private.compute_world_summary(uuid) from public, anon;
revoke all on function public.get_world_summary(uuid) from public, anon;
grant usage on schema private to authenticated;
grant execute on function private.compute_world_summary(uuid) to authenticated;
grant execute on function public.get_world_summary(uuid) to authenticated;
