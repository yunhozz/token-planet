begin;
create extension if not exists pgtap with schema extensions;
select no_plan();
\ir fixtures/shop_guest_import.inc

with source as (
  select pg_temp.shop_guest_import_usage_request(
    'f1000000-0000-4000-a000-000000000054',
    'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
  ) as request
), normalized as (
  select pg_temp.shop_guest_import_try_usage_core_normalize(
    request#>'{snapshot,data}'
  ) as value from source
)
select ok(value is not null
    and value->>'validation_scope' = 'usage_journal_core'
    and value->>'lifetime_usage_tokens' = '100'
    and value->>'current_cycle_usage_tokens' = '100'
    and value#>>'{cycle_usage_totals,0,total_tokens}' = '100',
  'core usage normalizer reconciles a nonzero raw and journal source')
from normalized;

select ok(pg_temp.shop_guest_import_try_usage_core_normalize(jsonb_set(
    pg_temp.shop_guest_import_usage_request(
      'f1000000-0000-4000-a000-000000000055',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    )#>'{snapshot,data}', '{daily_agent_totals,0,total_tokens}', '99'::jsonb
  )) is null,
  'core usage rejects daily mirror total mismatches');

select ok(pg_temp.shop_guest_import_try_usage_core_normalize(jsonb_set(
    pg_temp.shop_guest_import_usage_request(
      'f1000000-0000-4000-a000-000000000056',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    )#>'{snapshot,data}', '{daily_agent_totals}', '[]'::jsonb
  )) is null,
  'core usage rejects an omitted daily mirror');

select ok(pg_temp.shop_guest_import_try_usage_core_normalize(jsonb_set(
    pg_temp.shop_guest_import_usage_request(
      'f1000000-0000-4000-a000-000000000057',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    )#>'{snapshot,data}', '{lifetime_usage_tokens}', '99'::jsonb
  )) is null,
  'core usage rejects a lifetime total mismatch');

select ok(pg_temp.shop_guest_import_try_usage_core_normalize(jsonb_set(
    pg_temp.shop_guest_import_usage_request(
      'f1000000-0000-4000-a000-000000000058',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    )#>'{snapshot,data}', '{current_cycle_usage_tokens}', '99'::jsonb
  )) is null,
  'core usage rejects a current-cycle total mismatch');

select ok(pg_temp.shop_guest_import_try_usage_core_normalize(jsonb_set(
    pg_temp.shop_guest_import_usage_request(
      'f1000000-0000-4000-a000-000000000059',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    )#>'{snapshot,data}', '{cycle_usage_totals,0,total_tokens}', '99'::jsonb
  )) is null,
  'core usage rejects cycle totals that disagree with present journal rows');

select ok(pg_temp.shop_guest_import_try_usage_core_normalize(jsonb_set(
    pg_temp.shop_guest_import_usage_request(
      'f1000000-0000-4000-a000-000000000060',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    )#>'{snapshot,data}', '{growth_journal_entries,0,confirmed_tokens}', '99'::jsonb
  )) is null,
  'core usage rejects journal totals that disagree with raw aggregates');

with source as (
  select pg_temp.shop_guest_import_usage_request(
    'f1000000-0000-4000-a000-000000000061',
    'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
  )#>'{snapshot,data}' as data
), altered as (
  select jsonb_set(
    jsonb_set(
      jsonb_set(
        jsonb_set(data, '{historical_cycles}', jsonb_build_array(jsonb_build_object(
          'cycle_id', 'historical-cycle',
          'started_at_utc', '2026-09-30T00:00:00+00:00',
          'ended_at_utc', '2026-10-01T00:00:00+00:00',
          'is_current', false, 'settled_bonus_tokens', null
        ))),
        '{growth_journal_cycles}', (data->'growth_journal_cycles') || jsonb_build_array(
          jsonb_build_object(
            'cycle_id', 'historical-cycle',
            'started_at_utc', '2026-09-30T00:00:00+00:00',
            'ended_at_utc', '2026-10-01T00:00:00+00:00',
            'wallet_credit', null, 'wallet_credit_at_utc', null
          )
        )
      ),
      '{growth_journal_entries,0,cycle_id}', to_jsonb('historical-cycle'::text)
    ),
    '{cycle_usage_totals,0,cycle_id}', to_jsonb('historical-cycle'::text)
  ) as value from source
)
select ok(pg_temp.shop_guest_import_try_usage_core_normalize(value) is null,
  'core usage rejects a known journal cycle that differs from the raw mapping')
from altered;

select ok(pg_temp.shop_guest_import_try_usage_core_normalize(jsonb_set(
    pg_temp.shop_guest_import_usage_request(
      'f1000000-0000-4000-a000-000000000062',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    )#>'{snapshot,data}', '{usage_aggregates,0,coverage}', to_jsonb('partial'::text)
  )) is null,
  'core usage rejects partial coverage');

select ok(pg_temp.shop_guest_import_try_usage_core_normalize(jsonb_set(
    pg_temp.shop_guest_import_usage_request(
      'f1000000-0000-4000-a000-000000000063',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    )#>'{snapshot,data}', '{growth_journal_entries,0,device_id}',
    to_jsonb('30000000-0000-4000-a000-000000000002'::text)
  )) is null,
  'core usage rejects a journal row from a foreign guest device');

with source as (
  select pg_temp.shop_guest_import_usage_request(
    'f1000000-0000-4000-a000-000000000064',
    'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
  )#>'{snapshot,data}' as data
)
select ok(pg_temp.shop_guest_import_try_usage_core_normalize(jsonb_set(
    data, '{usage_aggregates}',
    (data->'usage_aggregates') || (data->'usage_aggregates')
  )) is null,
  'core usage rejects duplicate raw date-agent keys')
from source;

select ok(pg_temp.shop_guest_import_try_usage_core_normalize(jsonb_set(
    pg_temp.shop_guest_import_usage_request(
      'f1000000-0000-4000-a000-000000000065',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    )#>'{snapshot,data}', '{usage_aggregates,0,total_tokens}',
    '9223372036854775808'::jsonb
  )) is null,
  'core usage rejects values outside signed bigint range');

select ok(pg_temp.shop_guest_import_try_usage_core_normalize(jsonb_set(
    pg_temp.shop_guest_import_usage_request(
      'f1000000-0000-4000-a000-000000000066',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    )#>'{snapshot,data}', '{usage_aggregates,0,event_count}', to_jsonb('2'::text)
  )) is null,
  'core usage rejects numeric fields with JSON string types');

with source as (
  select pg_temp.shop_guest_import_empty_request(
    'f1000000-0000-4000-a000-000000000067',
    'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
  )#>'{snapshot,data}' as data
), empty_capture as (
  select jsonb_set(
    jsonb_set(
      jsonb_set(data, '{growth_journal_cycles}', jsonb_build_array(jsonb_build_object(
        'cycle_id', 'ownership-cycle',
        'started_at_utc', '2026-10-01T00:00:00+00:00',
        'ended_at_utc', null,
        'wallet_credit', null,
        'wallet_credit_at_utc', null
      ))),
      '{lifetime_usage_tokens}', '0'::jsonb
    ),
    '{current_cycle_usage_tokens}', '0'::jsonb
  ) as data from source
)
select ok(
  result is not null
    and result->>'lifetime_usage_tokens' = '0'
    and result->>'current_cycle_usage_tokens' = '0'
    and jsonb_array_length(result->'usage_aggregates') = 0
    and jsonb_array_length(result->'growth_journal_entries') = 0,
  'core usage accepts the typed empty capture with current cycle metadata and zero sums'
)
from (
  select pg_temp.shop_guest_import_try_usage_core_normalize(data) as result
  from empty_capture
) checked;

select ok(pg_temp.shop_guest_import_try_usage_core_normalize(jsonb_set(
    pg_temp.shop_guest_import_usage_request(
      'f1000000-0000-4000-a000-000000000068',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    )#>'{snapshot,data}', '{growth_journal_entries,0,revision}', '0'::jsonb
  )) is null,
  'core usage rejects journal revision zero');

select ok(pg_temp.shop_guest_import_try_usage_core_normalize(jsonb_set(
    pg_temp.shop_guest_import_usage_request(
      'f1000000-0000-4000-a000-000000000069',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    )#>'{snapshot,data}', '{growth_journal_cycles,0,wallet_credit}', '50'::jsonb
  )) is null,
  'core usage rejects a wallet credit without its timestamp');

select ok(pg_temp.shop_guest_import_try_usage_core_normalize(jsonb_set(
    pg_temp.shop_guest_import_usage_request(
      'f1000000-0000-4000-a000-000000000070',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    )#>'{snapshot,data}', '{growth_journal_cycles,0,wallet_credit_at_utc}',
    to_jsonb('2026-10-01T00:00:01+00:00'::text)
  )) is null,
  'core usage rejects a wallet-credit timestamp without its credit amount');

with source as (
  select pg_temp.shop_guest_import_empty_request(
    'f1000000-0000-4000-a000-000000000071',
    'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
  )#>'{snapshot,data}' as data
), empty_capture as (
  select jsonb_set(
    jsonb_set(
      jsonb_set(data, '{growth_journal_cycles}', jsonb_build_array(jsonb_build_object(
        'cycle_id', 'ownership-cycle',
        'started_at_utc', '2026-10-01T00:00:00+00:00',
        'ended_at_utc', null,
        'wallet_credit', null,
        'wallet_credit_at_utc', null
      ))),
      '{lifetime_usage_tokens}', '0'::jsonb
    ),
    '{current_cycle_usage_tokens}', '0'::jsonb
  ) as data from source
)
select ok(pg_temp.shop_guest_import_try_usage_core_normalize(data - 'growth_journal_state') is null,
  'core usage requires the nullable growth-journal state key even for an empty capture')
from empty_capture;

with source as (
  select pg_temp.shop_guest_import_usage_request(
    'f1000000-0000-4000-a000-000000000072',
    'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
  )#>'{snapshot,data}' as data
)
select ok(
  pg_temp.shop_guest_import_try_usage_core_normalize(jsonb_set(
    data, '{growth_journal_state}', to_jsonb('invalid-state'::text)
  )) is null
  and pg_temp.shop_guest_import_try_usage_core_normalize(jsonb_set(
    data, '{growth_journal_state}', '7'::jsonb
  )) is null,
  'core usage rejects scalar growth-journal state values')
from source;

select * from finish();
rollback;
