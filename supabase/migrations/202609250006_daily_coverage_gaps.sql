create or replace function private.compute_world_summary(p_world_id uuid)
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
    where m.world_id = p_world_id and not exists (
      select 1 from public.daily_usage_snapshots s
      where s.world_id = m.world_id and s.user_id = m.user_id
    )
  ) or exists (
    select 1 from (
      select distinct s.user_id, s.bucket_date
      from public.daily_usage_snapshots s where s.world_id = p_world_id
    ) d cross join (values ('codex'), ('claude_code')) as a(agent)
    where not exists (
      select 1 from public.daily_usage_snapshots s
      where s.world_id = p_world_id and s.user_id = d.user_id
        and s.bucket_date = d.bucket_date and s.agent = a.agent
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
