create function private.upload_daily_snapshot(p_world_id uuid, p_snapshot jsonb)
returns bigint
language plpgsql security definer set search_path = '' as $$
declare
  v_keys text[] := array[
    'device_id', 'bucket_date', 'bucket_policy_version', 'agent', 'schema_version',
    'revision', 'input_tokens', 'output_tokens', 'cache_read_tokens',
    'cache_write_tokens', 'total_tokens', 'coverage', 'payload_hash'
  ];
  v_user_id uuid := (select auth.uid());
  v_device_id uuid;
  v_bucket_date date;
  v_policy smallint;
  v_agent text;
  v_schema smallint;
  v_revision bigint;
  v_input bigint;
  v_output bigint;
  v_cache_read bigint;
  v_cache_write bigint;
  v_total bigint;
  v_coverage text;
  v_hash text;
  v_old public.daily_usage_snapshots%rowtype;
begin
  if v_user_id is null or not exists (
    select 1 from public.world_members m
    where m.world_id = p_world_id and m.user_id = v_user_id
  ) then
    raise exception 'world access denied' using errcode = '42501';
  end if;
  if p_snapshot is null or jsonb_typeof(p_snapshot) <> 'object'
    or not p_snapshot ?& v_keys
    or exists (select 1 from jsonb_object_keys(p_snapshot) k(key) where k.key <> all(v_keys))
  then
    raise exception 'aggregate payload fields are invalid' using errcode = '23514';
  end if;
  if jsonb_typeof(p_snapshot->'device_id') <> 'string'
    or p_snapshot->>'device_id' !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
    or p_snapshot->>'bucket_date' !~ '^\d{4}-\d{2}-\d{2}$'
    or p_snapshot->>'agent' not in ('codex', 'claude_code')
    or p_snapshot->>'coverage' not in ('complete', 'partial', 'unavailable', 'unsupported', 'user_disabled')
    or p_snapshot->>'payload_hash' !~ '^[0-9a-f]{64}$'
    or exists (
      select 1 from unnest(array[
        'bucket_policy_version', 'schema_version', 'revision', 'input_tokens',
        'output_tokens', 'cache_read_tokens', 'cache_write_tokens', 'total_tokens'
      ]) k(key)
      where jsonb_typeof(p_snapshot->k.key) not in ('number', 'null')
        or (k.key in ('bucket_policy_version', 'schema_version', 'revision')
          and jsonb_typeof(p_snapshot->k.key) <> 'number')
    )
  then
    raise exception 'aggregate payload values are invalid' using errcode = '23514';
  end if;
  v_device_id := (p_snapshot->>'device_id')::uuid;
  v_bucket_date := (p_snapshot->>'bucket_date')::date;
  v_policy := (p_snapshot->>'bucket_policy_version')::smallint;
  v_agent := p_snapshot->>'agent';
  v_schema := (p_snapshot->>'schema_version')::smallint;
  v_revision := (p_snapshot->>'revision')::bigint;
  v_input := (p_snapshot->>'input_tokens')::bigint;
  v_output := (p_snapshot->>'output_tokens')::bigint;
  v_cache_read := (p_snapshot->>'cache_read_tokens')::bigint;
  v_cache_write := (p_snapshot->>'cache_write_tokens')::bigint;
  v_total := (p_snapshot->>'total_tokens')::bigint;
  v_coverage := p_snapshot->>'coverage';
  v_hash := p_snapshot->>'payload_hash';
  if v_policy <> 1 or v_schema <> 1 or v_revision < 1
    or coalesce(v_input < 0 or v_output < 0 or v_cache_read < 0 or v_cache_write < 0 or v_total < 0, false)
    or (v_coverage = 'complete' and v_total is null)
    or (v_coverage in ('unavailable', 'unsupported', 'user_disabled') and v_total is not null)
  then
    raise exception 'aggregate payload values are invalid' using errcode = '23514';
  end if;

  perform 1 from public.worlds w where w.id = p_world_id for update;
  select * into v_old from public.daily_usage_snapshots s
  where s.world_id = p_world_id and s.user_id = v_user_id and s.device_id = v_device_id
    and s.bucket_date = v_bucket_date and s.agent = v_agent
  for update;
  if found then
    if v_revision < v_old.revision then
      return v_old.revision;
    end if;
    if v_revision = v_old.revision then
      if row(v_old.bucket_policy_version, v_old.schema_version, v_old.input_tokens,
        v_old.output_tokens, v_old.cache_read_tokens, v_old.cache_write_tokens,
        v_old.total_tokens, v_old.coverage, v_old.payload_hash)
        is distinct from row(v_policy, v_schema, v_input, v_output, v_cache_read,
          v_cache_write, v_total, v_coverage, v_hash)
      then
        raise exception 'revision conflicts with stored aggregate' using errcode = '23514';
      end if;
      return v_old.revision;
    end if;
    update public.daily_usage_snapshots s set
      bucket_policy_version = v_policy, schema_version = v_schema, revision = v_revision,
      input_tokens = v_input, output_tokens = v_output, cache_read_tokens = v_cache_read,
      cache_write_tokens = v_cache_write, total_tokens = v_total, coverage = v_coverage,
      payload_hash = v_hash, updated_at = now()
    where s.world_id = p_world_id and s.user_id = v_user_id and s.device_id = v_device_id
      and s.bucket_date = v_bucket_date and s.agent = v_agent;
  else
    insert into public.daily_usage_snapshots (
      world_id, user_id, device_id, bucket_date, bucket_policy_version, agent,
      schema_version, revision, input_tokens, output_tokens, cache_read_tokens,
      cache_write_tokens, total_tokens, coverage, payload_hash
    ) values (
      p_world_id, v_user_id, v_device_id, v_bucket_date, v_policy, v_agent,
      v_schema, v_revision, v_input, v_output, v_cache_read, v_cache_write,
      v_total, v_coverage, v_hash
    );
  end if;
  return v_revision;
end;
$$;

create function public.upload_daily_snapshot(p_world_id uuid, p_snapshot jsonb)
returns bigint
language sql security invoker set search_path = '' as $$
  select private.upload_daily_snapshot(p_world_id, p_snapshot);
$$;

revoke all on function private.upload_daily_snapshot(uuid, jsonb) from public, anon;
revoke all on function public.upload_daily_snapshot(uuid, jsonb) from public, anon;
grant execute on function private.upload_daily_snapshot(uuid, jsonb) to authenticated;
grant execute on function public.upload_daily_snapshot(uuid, jsonb) to authenticated;
