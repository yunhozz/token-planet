-- Pure SQL validation contract for the native schema-2 first-reset fixture.
-- This suite intentionally starts RED until the v2 normalizer migration exists.
\set ON_ERROR_STOP on
begin;
create extension if not exists pgtap with schema extensions;
set local search_path = extensions, pg_catalog, pg_temp;
create table public.:"rollback_probe" (probe integer);
\ir fixtures/shop_guest_import_v2_first_reset.inc
\ir fixtures/shop_guest_import_native_first_reset.inc
create function pg_temp.shop_guest_import_v2_try_normalize(p_request jsonb, p_now timestamptz)
returns jsonb
language plpgsql
as $$
declare
  v_result jsonb;
begin
  execute 'select private.shop_guest_import_v2_normalize($1,$2)'
    into v_result using p_request,p_now;
  return v_result;
exception when undefined_function then
  return null;
end;
$$;
create function pg_temp.shop_guest_import_v2_try_normalize_unsealed(p_request jsonb, p_now timestamptz)
returns jsonb
language plpgsql
as $$
declare
  v_result jsonb;
begin
  execute 'select private.shop_guest_import_v2_normalize($1,$2)'
    into v_result using p_request,p_now;
  return v_result;
exception when undefined_function then
  return null;
end;
$$;
create function pg_temp.shop_guest_import_v2_reseal(p_request jsonb)
returns jsonb
language plpgsql
as $$
declare
  v_snapshot jsonb := p_request->'snapshot';
  v_provenance jsonb := p_request#>'{snapshot,provenance}';
  v_prefix jsonb;
  v_canonical text;
  v_hash text;
begin
  v_prefix := jsonb_build_object(
    'version',1,
    'lineage_id',v_provenance->>'lineage_id',
    'device_id',v_provenance->>'device_id',
    'ingest_watermark',v_provenance->'ingest_watermark',
    'occurrences',v_provenance->'occurrences'
  );
  execute 'select private.shop_guest_import_v2_canonical_json($1)'
    into v_canonical using v_prefix;
  v_hash := encode(extensions.digest(convert_to(v_canonical,'UTF8'),'sha256'),'hex');
  v_provenance := jsonb_set(v_provenance,'{prefix_fingerprint}',to_jsonb(v_hash),true);
  v_snapshot := jsonb_set(v_snapshot,'{provenance}',v_provenance,true);
  execute 'select private.shop_guest_import_v2_canonical_json($1)'
    into v_canonical using v_snapshot - 'source_fingerprint';
  v_hash := encode(extensions.digest(convert_to(v_canonical,'UTF8'),'sha256'),'hex');
  v_snapshot := jsonb_set(v_snapshot,'{source_fingerprint}',to_jsonb(v_hash),true);
  return jsonb_set(p_request,'{snapshot}',v_snapshot,true);
exception when undefined_function then
  -- Keep the pre-implementation RED useful when the v2 helper is absent.
  return p_request;
end;
$$;
create function pg_temp.shop_guest_import_v2_reseal_source_only(p_request jsonb)
returns jsonb
language plpgsql
as $$
declare
  v_snapshot jsonb := p_request->'snapshot';
  v_canonical text;
  v_hash text;
begin
  execute 'select private.shop_guest_import_v2_canonical_json($1)'
    into v_canonical using v_snapshot - 'source_fingerprint';
  v_hash := encode(extensions.digest(convert_to(v_canonical,'UTF8'),'sha256'),'hex');
  return jsonb_set(
    p_request,
    '{snapshot,source_fingerprint}',
    to_jsonb(v_hash),
    true
  );
exception when undefined_function then
  return p_request;
end;
$$;
create function pg_temp.shop_guest_import_v2_source_fingerprint_valid(p_request jsonb)
returns boolean
language plpgsql
as $$
declare
  v_snapshot jsonb := p_request->'snapshot';
  v_canonical text;
  v_hash text;
begin
  execute 'select private.shop_guest_import_v2_canonical_json($1)'
    into v_canonical using v_snapshot - 'source_fingerprint';
  v_hash := encode(extensions.digest(convert_to(v_canonical,'UTF8'),'sha256'),'hex');
  return v_snapshot->>'source_fingerprint' = v_hash;
exception when undefined_function then
  return false;
end;
$$;
create function pg_temp.shop_guest_import_v2_bad_prefix_request()
returns jsonb
language sql
as $$
  select pg_temp.shop_guest_import_v2_reseal_source_only(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,provenance,prefix_fingerprint}',
      to_jsonb(repeat('0',64))
    )
  )
$$;
create function pg_temp.shop_guest_import_v2_reject(p_request jsonb)
returns boolean
language sql
as $$
  select pg_temp.shop_guest_import_v2_try_normalize(
    pg_temp.shop_guest_import_v2_reseal(p_request),
    '2026-10-04T00:00:00Z'::timestamptz
  ) is null
$$;
create function pg_temp.shop_guest_import_v2_append_occurrence(
  p_request jsonb,
  p_occurrence_id text,
  p_cycle_id text,
  p_tokens bigint,
  p_occurred_at text,
  p_ingested_at text
)
returns jsonb
language plpgsql
as $$
declare
  v_snapshot jsonb := p_request->'snapshot';
  v_provenance jsonb := v_snapshot->'provenance';
  v_occurrences jsonb := v_provenance->'occurrences';
  v_occurrence jsonb;
  v_next_seq bigint := (v_provenance->>'ingest_watermark')::bigint + 1;
begin
  v_occurrence := jsonb_build_object(
    'occurrence_id',p_occurrence_id,
    'record_version',1,
    'ingest_seq',v_next_seq,
    'device_id',v_provenance->>'device_id',
    'agent','codex',
    'occurred_at_utc',p_occurred_at,
    'ingested_at_utc',p_ingested_at,
    'cycle_id',p_cycle_id,
    'total_tokens',p_tokens,
    'coverage','complete'
  );
  v_provenance := jsonb_set(v_provenance,'{occurrences}',v_occurrences || jsonb_build_array(v_occurrence),true);
  v_provenance := jsonb_set(v_provenance,'{ingest_watermark}',to_jsonb(v_next_seq),true);
  v_provenance := jsonb_set(v_provenance,'{occurrence_count}',to_jsonb(v_next_seq),true);
  v_snapshot := jsonb_set(v_snapshot,'{provenance}',v_provenance,true);
  return pg_temp.shop_guest_import_v2_reseal(jsonb_set(p_request,'{snapshot}',v_snapshot,true));
end;
$$;

create function pg_temp.shop_guest_import_v2_rehash_journal(p_request jsonb, p_index integer)
returns jsonb
language plpgsql
as $$
declare
  v_snapshot jsonb := p_request->'snapshot';
  v_data jsonb := v_snapshot->'data';
  v_entries jsonb := v_data->'growth_journal_entries';
  v_entry jsonb := v_entries->p_index;
  v_hash text;
begin
  v_hash := encode(extensions.digest(convert_to(
    private.shop_guest_import_v2_canonical_json(jsonb_build_object(
      'agent',v_entry->'agent',
      'bucket_date',v_entry->'bucket_date',
      'confirmed_tokens',v_entry->'confirmed_tokens',
      'coverage',v_entry->'coverage',
      'cycle_id',v_entry->'cycle_id',
      'device_id',v_entry->'device_id',
      'generation',v_entry->'generation',
      'present',v_entry->'present',
      'revision',v_entry->'revision'
    )), 'UTF8'
  ), 'sha256'), 'hex');
  v_entry := jsonb_set(v_entry,'{payload_hash}',to_jsonb(v_hash),true);
  v_entries := jsonb_set(v_entries,array[p_index::text],v_entry,true);
  v_data := jsonb_set(v_data,'{growth_journal_entries}',v_entries,true);
  v_snapshot := jsonb_set(v_snapshot,'{data}',v_data,true);
  return pg_temp.shop_guest_import_v2_reseal(jsonb_set(p_request,'{snapshot}',v_snapshot,true));
end;
$$;

create function pg_temp.shop_guest_import_v2_zero_postreset_request()
returns jsonb
language plpgsql
as $$
declare
  v_request jsonb := pg_temp.shop_guest_import_v2_first_reset_request();
  v_snapshot jsonb := v_request->'snapshot';
  v_provenance jsonb := v_snapshot->'provenance';
  v_data jsonb := v_snapshot->'data';
  v_occurrence jsonb;
  v_journal jsonb;
  v_journal_hash text;
  v_device_id text := v_provenance->>'device_id';
  v_new_cycle_id text := v_data#>>'{current_cycle,cycle_id}';
  v_occurrences jsonb;
  v_journal_entries jsonb;
begin
  v_occurrence := jsonb_build_object(
    'occurrence_id','b91f0df0-8b90-44bd-931f-0f1111111111',
    'record_version',1,
    'ingest_seq',2,
    'device_id',v_device_id,
    'agent','codex',
    'occurred_at_utc','2026-10-04T06:00:00.000000Z',
    'ingested_at_utc','2026-10-04T06:00:00.001000Z',
    'cycle_id',v_new_cycle_id,
    'total_tokens',0,
    'coverage','complete'
  );
  v_occurrences := v_provenance->'occurrences' || jsonb_build_array(v_occurrence);
  v_provenance := jsonb_set(v_provenance,'{occurrences}',v_occurrences,true);
  v_provenance := jsonb_set(v_provenance,'{ingest_watermark}','2'::jsonb,true);
  v_provenance := jsonb_set(v_provenance,'{occurrence_count}','2'::jsonb,true);

  v_data := jsonb_set(v_data,'{usage_aggregates}',v_data->'usage_aggregates' || jsonb_build_array(
    jsonb_build_object(
      'cycle_id',v_new_cycle_id,
      'bucket_date','2026-10-04',
      'agent','codex',
      'event_count',1,
      'total_tokens',0,
      'coverage','complete'
    )
  ),true);
  v_data := jsonb_set(v_data,'{daily_agent_totals}',v_data->'daily_agent_totals' || jsonb_build_array(
    jsonb_build_object(
      'bucket_date','2026-10-04',
      'agent','codex',
      'total_tokens',0,
      'coverage','complete'
    )
  ),true);
  v_data := jsonb_set(v_data,'{cycle_usage_totals}',v_data->'cycle_usage_totals' || jsonb_build_array(
    jsonb_build_object('cycle_id',v_new_cycle_id,'total_tokens',0)
  ),true);
  v_journal := jsonb_build_object(
    'device_id',v_device_id,
    'cycle_id',v_new_cycle_id,
    'bucket_date','2026-10-04',
    'agent','codex',
    'revision',1,
    'acknowledged_revision',0,
    'generation',0,
    'present',true,
    'confirmed_tokens',0,
    'coverage','complete'
  );
  v_journal_hash := encode(extensions.digest(convert_to(
    private.shop_guest_import_v2_canonical_json(jsonb_build_object(
      'agent',v_journal->'agent',
      'bucket_date',v_journal->'bucket_date',
      'confirmed_tokens',v_journal->'confirmed_tokens',
      'coverage',v_journal->'coverage',
      'cycle_id',v_journal->'cycle_id',
      'device_id',v_journal->'device_id',
      'generation',v_journal->'generation',
      'present',v_journal->'present',
      'revision',v_journal->'revision'
    )), 'UTF8'
  ), 'sha256'), 'hex');
  v_journal := jsonb_set(v_journal,'{payload_hash}',to_jsonb(v_journal_hash),true);
  v_journal_entries := v_data->'growth_journal_entries' || jsonb_build_array(v_journal);
  v_data := jsonb_set(v_data,'{growth_journal_entries}',v_journal_entries,true);
  v_data := jsonb_set(v_data,'{effect_contributions}',v_data->'effect_contributions' || jsonb_build_array(
    jsonb_build_object(
      'device_id',v_device_id,
      'cycle_id',v_new_cycle_id,
      'date','2026-10-04',
      'effect_revision',0,
      'canonical_version',1,
      'tokens',0,
      'growth_bps',0,
      'wallet_bps',0
    )
  ),true);
  v_snapshot := jsonb_set(
    v_snapshot,
    '{canonical_payload,daily_segments}',
    (v_snapshot#>'{canonical_payload,daily_segments}') || jsonb_build_array(
      jsonb_build_object(
        'cycle_id',v_new_cycle_id,
        'date','2026-10-04',
        'effect_revision',0,
        'tokens',0
      )
    ),
    true
  );
  v_snapshot := jsonb_set(v_snapshot,'{provenance}',v_provenance,true);
  v_snapshot := jsonb_set(v_snapshot,'{data}',v_data,true);
  v_snapshot := jsonb_set(v_snapshot,'{captured_at_utc}','"2026-10-04T06:00:00.002000Z"'::jsonb,true);
  return pg_temp.shop_guest_import_v2_reseal(jsonb_set(v_request,'{snapshot}',v_snapshot,true));
end;
$$;

create function pg_temp.shop_guest_import_v2_zero_before_positive_request()
returns jsonb
language plpgsql
as $$
declare
  v_request jsonb := pg_temp.shop_guest_import_v2_first_reset_request();
  v_snapshot jsonb := v_request->'snapshot';
  v_provenance jsonb := v_snapshot->'provenance';
  v_data jsonb := v_snapshot->'data';
  v_positive jsonb := v_provenance#>'{occurrences,0}';
  v_zero jsonb;
begin
  v_positive := jsonb_set(v_positive,'{ingest_seq}','2'::jsonb,true);
  v_zero := jsonb_build_object(
    'occurrence_id','d223f57a-25f5-4556-8a88-4c1111111111',
    'record_version',1,
    'ingest_seq',1,
    'device_id',v_provenance->>'device_id',
    'agent','codex',
    'occurred_at_utc','2026-10-03T05:26:25.940093Z',
    'ingested_at_utc','2026-10-03T05:26:25.940468Z',
    'cycle_id',v_positive->>'cycle_id',
    'total_tokens',0,
    'coverage','complete'
  );
  v_provenance := jsonb_set(
    v_provenance,
    '{occurrences}',
    jsonb_build_array(v_zero,v_positive),
    true
  );
  v_provenance := jsonb_set(v_provenance,'{ingest_watermark}','2'::jsonb,true);
  v_provenance := jsonb_set(v_provenance,'{occurrence_count}','2'::jsonb,true);
  v_data := jsonb_set(v_data,'{usage_aggregates,0,event_count}','2'::jsonb,true);
  v_snapshot := jsonb_set(v_snapshot,'{provenance}',v_provenance,true);
  v_snapshot := jsonb_set(v_snapshot,'{data}',v_data,true);
  return pg_temp.shop_guest_import_v2_reseal(jsonb_set(v_request,'{snapshot}',v_snapshot,true));
end;
$$;

select plan(53);

select is(
  pg_temp.shop_guest_import_v2_first_reset_request()->>'schema_version',
  '2',
  'native first-reset fixture is the schema-2 request'
);
select is(
  pg_temp.shop_guest_import_v2_first_reset_request()#>>'{snapshot,source_fingerprint}',
  'da9fc489f199caf407dd84d48244bd85adc9bf9aeafedcd88dd77fb3573678ee',
  'native schema-2 request identity matches the frozen source fingerprint'
);
select ok(
  pg_temp.shop_guest_import_v2_first_reset_request()#>>'{snapshot,provenance,domain}' = 'ordinary_first_reset_zero_effect_v1'
  and pg_temp.shop_guest_import_v2_first_reset_request()#>>'{snapshot,data,lifetime_usage_tokens}' = '1000000'
  and pg_temp.shop_guest_import_v2_first_reset_request()#>>'{snapshot,data,current_cycle_usage_tokens}' = '0',
  'native source records the one-million raw old-cycle first reset with zero current usage'
);
select is(
  private.shop_guest_import_bootstrap_source_normalize(
    pg_temp.shop_guest_import_native_first_reset_request()->'snapshot'->'data',
    '2026-10-04T00:00:00Z'::timestamptz
  ),
  null::jsonb,
  'schema-1 proofless first-reset control remains held by the existing validator'
);
select ok(
  to_regprocedure('private.shop_guest_import_v2_normalize(jsonb,timestamp with time zone)') is not null,
  'schema-2 pure validator is installed'
);
select ok(
  pg_temp.shop_guest_import_v2_try_normalize(
    pg_temp.shop_guest_import_v2_first_reset_request(),
    '2026-10-04T00:00:00Z'::timestamptz
  ) is not null,
  'native schema-2 first-reset proof is independently normalized'
);
select ok(
  pg_temp.shop_guest_import_v2_try_normalize(
    pg_temp.shop_guest_import_v2_reseal(
      jsonb_set(
        pg_temp.shop_guest_import_v2_first_reset_request(),
        '{snapshot,data,historical_cycles,0,ended_at_utc}',
        '"2026-10-03T05:26:25.944093Z"'::jsonb
      )
    ),
    '2026-10-04T00:00:00Z'::timestamptz
  ) is not null,
  'historical end and settlement compare as the same UTC instant across valid encodings'
);

select ok(
  pg_temp.shop_guest_import_v2_reject(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,data,historical_cycles,0,ended_at_utc}',
      '"2026-10-03T05:26:26.192093Z"'::jsonb
    )
  ),
  'historical cycle end must match its independently recorded settlement time'
);
with native as (
  select pg_temp.shop_guest_import_v2_first_reset_request() as request
)
select ok(
  pg_temp.shop_guest_import_v2_reject(
    jsonb_set(
      jsonb_set(
        request,
        '{snapshot,data,cycle_settlements,0,settled_at_utc}',
        '"2026-10-03T05:26:26.192094Z"'::jsonb
      ),
      '{snapshot,data,historical_cycles,0,ended_at_utc}',
      '"2026-10-03T05:26:26.192094Z"'::jsonb
    )
  ),
  'settlement later than reset remains invalid even when historical end matches it'
)
from native;
select ok(
  pg_temp.shop_guest_import_v2_reject(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,data,cycle_settlements,0,cycle_id}',
      to_jsonb('eafa93c3-1c6b-4687-aa60-ce6011a27f9b'::text)
    )
  ),
  'cycle settlement must remain attached to the old cycle'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,provenance,reset_receipt,result,bonus_tokens}',
      '1'::jsonb
    )
  ),
  'positive reset bonus remains outside the first-reset zero-effect domain'
);

select ok(
  pg_temp.shop_guest_import_v2_reject(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,provenance,occurrences}',
      '[]'::jsonb
    )
  ),
  'missing immutable-prefix occurrence remains held after resealing'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    pg_temp.shop_guest_import_v2_append_occurrence(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      pg_temp.shop_guest_import_v2_first_reset_request()#>>'{snapshot,provenance,occurrences,0,occurrence_id}',
      pg_temp.shop_guest_import_v2_first_reset_request()#>>'{snapshot,provenance,occurrences,0,cycle_id}',
      0,
      '2026-10-03T05:26:25.943093Z',
      '2026-10-03T05:26:25.944468Z'
    )
  ),
  'duplicate occurrence identity remains held after resealing the valid prefix'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,provenance,occurrences,0,device_id}',
      to_jsonb('34d4ac55-4444-4e44-8444-444444444444'::text)
    )
  ),
  'occurrence from another device remains held after resealing'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    pg_temp.shop_guest_import_v2_first_reset_request() #- '{snapshot,provenance,occurrences,0,ingested_at_utc}'
  ),
  'occurrence with a missing required key remains held after resealing'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,provenance,occurrences,0,record_version}',
      '2'::jsonb
    )
  ),
  'unsupported occurrence record version remains held after resealing'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,provenance,occurrences,0,ingest_seq}',
      '2'::jsonb
    )
  ),
  'noncontiguous occurrence sequence remains held after resealing'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,provenance,cycle_bounds,0,ended_at_utc}',
      '"2026-10-03T05:26:26.192092Z"'::jsonb
    )
  ),
  'gap before the reset boundary remains held'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,provenance,cycle_bounds,1,started_at_utc}',
      '"2026-10-03T05:26:26.192092Z"'::jsonb
    )
  ),
  'overlap after the reset boundary remains held'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,provenance,cycle_bounds,0,cycle_id}',
      '"eafa93c3-1c6b-4687-aa60-ce6011a27f9b"'::jsonb
    )
  ),
  'old-cycle reset boundary cannot be relabeled as the new cycle'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,provenance,baselines,1,revision}',
      '1'::jsonb
    )
  ),
  'new-cycle final baseline must remain revision zero'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,provenance,baselines,1,effects,token_earning_bps}',
      '1'::jsonb
    )
  ),
  'nonzero final baseline effect remains held'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,provenance,reset_receipt,request,request_id}',
      '"not-a-uuid"'::jsonb
    )
  ),
  'malformed reset request UUID remains held'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,data,cycle_settlements}',
      (pg_temp.shop_guest_import_v2_first_reset_request()#>'{snapshot,data,cycle_settlements}') ||
      (pg_temp.shop_guest_import_v2_first_reset_request()#>'{snapshot,data,cycle_settlements}')
    )
  ),
  'multiple settlement rows remain held'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,provenance,reset_receipt,result,reset_available_at_utc}',
      '"2026-10-04T05:26:27.192093Z"'::jsonb
    )
  ),
  'reset deadline must equal reset time plus one day'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,captured_at_utc}',
      '"2026-10-05T00:00:00.000000Z"'::jsonb
    )
  ),
  'future capture time remains held'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,provenance,occurrences,0,occurred_at_utc}',
      '"2026-10-03T05:26:25.943093Z"'::jsonb
    )
  ),
  'occurrence cannot occur after its ingestion time'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    pg_temp.shop_guest_import_v2_append_occurrence(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      'f4aa137e-3235-438b-8762-23456789abcd',
      pg_temp.shop_guest_import_v2_first_reset_request()#>>'{snapshot,provenance,occurrences,0,cycle_id}',
      0,
      '2026-10-03T05:26:25.941093Z',
      '2026-10-03T05:26:25.943468Z'
    )
  ),
  'occurrence time cannot move backwards within the immutable prefix'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,provenance,occurrences,0,coverage}',
      '"partial"'::jsonb
    )
  ),
  'incomplete occurrence coverage remains held'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,data,usage_aggregates,0,total_tokens}',
      '999999'::jsonb
    )
  ),
  'usage aggregate total must be independently rebuilt from occurrences'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,canonical_payload,lifetime_tokens}',
      '999999'::jsonb
    )
  ),
  'canonical lifetime sum must match the occurrence prefix'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,data,contribution_canonical_version}',
      '2'::jsonb
    )
  ),
  'contribution canonical version must match the source canonical version'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,canonical_payload,daily_segments,0,tokens}',
      '999999'::jsonb
    )
  ),
  'canonical daily segment must match independently grouped usage'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,data,activity_days,0,first_occurred_at_utc}',
      '"2026-10-03T05:26:25.943093+00:00"'::jsonb
    )
  ),
  'activity day timestamp must be rebuilt from occurrence instants'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,data,growth_journal_entries,0,payload_hash}',
      to_jsonb(repeat('0',64))
    )
  ),
  'journal payload hash must match the native nine-field encoding'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    pg_temp.shop_guest_import_v2_rehash_journal(
      jsonb_set(
        pg_temp.shop_guest_import_v2_first_reset_request(),
        '{snapshot,data,growth_journal_entries,0,revision}',
        '0'::jsonb
      ),
      0
    )
  ),
  'zero journal revision remains invalid with its payload hash recomputed'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    pg_temp.shop_guest_import_v2_rehash_journal(
      jsonb_set(
        pg_temp.shop_guest_import_v2_first_reset_request(),
        '{snapshot,data,growth_journal_entries,0,generation}',
        '1'::jsonb
      ),
      0
    )
  ),
  'journal generation must match the undeleted generation-zero state'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,data,growth_journal_cycles,0,wallet_credit}',
      '999999'::jsonb
    )
  ),
  'old-cycle journal credit must match independently reconstructed raw usage'
);
select ok(
  pg_temp.shop_guest_import_v2_source_fingerprint_valid(
    pg_temp.shop_guest_import_v2_bad_prefix_request()
  )
  and pg_temp.shop_guest_import_v2_try_normalize_unsealed(
    pg_temp.shop_guest_import_v2_bad_prefix_request(),
    '2026-10-04T00:00:00Z'::timestamptz
  ) is null,
  'incorrect immutable-prefix fingerprint remains held with a valid enclosing source fingerprint'
);
select ok(
  pg_temp.shop_guest_import_v2_try_normalize_unsealed(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,source_fingerprint}',
      to_jsonb(repeat('0',64))
    ),
    '2026-10-04T00:00:00Z'::timestamptz
  ) is null,
  'incorrect snapshot source fingerprint remains held'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,data,unverified_planet_wallet_claims,0,claimed_amount}',
      '999999'::jsonb
    )
  ),
  'wallet claim amount must match the independently derived raw reset credit'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,data,game_rewards}',
      '[{}]'::jsonb
    )
  ),
  'any reward row remains outside the zero-effect first-reset domain'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,data,wallet_credits}',
      '[{}]'::jsonb
    )
  ),
  'any separate wallet credit row remains outside the first-reset domain'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,data,removal_debits}',
      '[{}]'::jsonb
    )
  ),
  'any removal debit row remains outside the first-reset domain'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    pg_temp.shop_guest_import_v2_append_occurrence(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      'faac137e-3235-438b-8762-23456789abcd',
      pg_temp.shop_guest_import_v2_first_reset_request()#>>'{snapshot,provenance,occurrences,0,cycle_id}',
      9223372036854775807,
      '2026-10-03T05:26:25.943093Z',
      '2026-10-03T05:26:25.944468Z'
    )
  ),
  'raw occurrence sum above signed bigint remains held before casts'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,data,landscape_instances}',
      '[{}]'::jsonb
    )
  ),
  'item instances remain outside the first-reset domain'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,data,effect_history}',
      '[{}]'::jsonb
    )
  ),
  'effect history remains outside the zero-effect domain'
);
select ok(
  pg_temp.shop_guest_import_v2_reject(
    jsonb_set(
      pg_temp.shop_guest_import_v2_first_reset_request(),
      '{snapshot,data,era_progress}',
      '[{}]'::jsonb
    )
  ),
  'era progress remains outside the first-reset domain'
);
select is(
  pg_temp.shop_guest_import_v2_try_normalize(
    pg_temp.shop_guest_import_v2_zero_postreset_request(),
    '2026-10-05T00:00:00Z'::timestamptz
  )#>'{canonical_payload,daily_segments}',
  pg_temp.shop_guest_import_v2_zero_postreset_request()#>'{snapshot,canonical_payload,daily_segments}',
  'complete zero-token postreset occurrence preserves its zero-token daily segment'
);
select is(
  pg_temp.shop_guest_import_v2_try_normalize(
    pg_temp.shop_guest_import_v2_zero_before_positive_request(),
    '2026-10-04T00:00:00Z'::timestamptz
  )#>'{canonical_payload,activity_days}',
  pg_temp.shop_guest_import_v2_first_reset_request()#>'{snapshot,canonical_payload,activity_days}',
  'zero-token occurrence does not move the first positive activity timestamp'
);
select is(
  pg_temp.shop_guest_import_v2_try_normalize(
    pg_temp.shop_guest_import_v2_first_reset_request(),
    '2026-10-04T00:00:00Z'::timestamptz
  ),
  pg_temp.shop_guest_import_v2_try_normalize(
    pg_temp.shop_guest_import_v2_first_reset_request(),
    '2026-10-04T00:00:00Z'::timestamptz
  ),
  'same source and trusted time deterministically produce the same validation result'
);
select ok(
  exists (
    select 1 from pg_proc p
    where p.oid = 'private.shop_guest_import_v2_normalize(jsonb,timestamp with time zone)'::regprocedure
      and p.provolatile = 's'
      and not p.prosecdef
      and coalesce(p.proconfig,array[]::text[]) @> array['search_path=""']
      and not exists (
        select 1 from aclexplode(p.proacl) a
        where a.grantee = 0 and a.privilege_type = 'EXECUTE'
      )
  ),
  'normalizer stays stable, invoker-only, empty-search-path, and private'
);
create temp table pg_temp.shop_guest_import_v2_expected_result(value jsonb);
insert into pg_temp.shop_guest_import_v2_expected_result
select pg_temp.shop_guest_import_v2_try_normalize(
  pg_temp.shop_guest_import_v2_first_reset_request(),
  '2026-10-04T00:00:00Z'::timestamptz
);
set local time zone 'Pacific/Kiritimati';
select is(
  pg_temp.shop_guest_import_v2_try_normalize(
    pg_temp.shop_guest_import_v2_first_reset_request(),
    '2026-10-04T00:00:00Z'::timestamptz
  ),
  (select value from pg_temp.shop_guest_import_v2_expected_result),
  'normalization does not depend on the session time zone'
);
select * from finish();
rollback;
\echo SHOP_GUEST_IMPORT_V2_ROLLBACK_COMPLETED
