begin;
create extension if not exists pgtap with schema extensions;
select no_plan();
\ir fixtures/shop_guest_import.inc

with source as (
  select pg_temp.shop_guest_import_usage_request(
    'f1000000-0000-4000-a000-000000000054',
    'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
  ) as request
), data as (
  select request#>'{snapshot,data}' as value from source
), owned as (
  select value, private.shop_guest_import_ownership_normalize(value) as normalized
  from data
), timeline as (
  select value, jsonb_build_object(
    'effect_cycle_bounds', normalized->'effect_cycle_bounds',
    'effect_history', normalized->'effect_history'
  ) as normalized_timeline
  from owned
), usage as (
  select value, normalized_timeline,
    private.shop_guest_import_usage_normalize(value, normalized_timeline) as normalized_usage
  from timeline
), reset as (
  select normalized_usage, pg_temp.shop_guest_import_try_reset_normalize(
    value, normalized_usage, normalized_timeline
  ) as normalized_reset
  from usage
)
select ok(normalized_usage is not null,
  'the current-only no-reset source passes the accepted ownership, effect and usage normalizers')
from reset;

with source as (
  select pg_temp.shop_guest_import_single_reset_floor_once_request(
    'f1000000-0000-4000-a000-000000000056',
    'f0000000-0000-4000-a000-000000000001', 'current-after-floor-once'
  )#>'{snapshot,data}' as value
), owned as (
  select value, private.shop_guest_import_ownership_normalize(value) as normalized
  from source
), timeline as (
  select value, jsonb_build_object(
    'effect_cycle_bounds', normalized->'effect_cycle_bounds',
    'effect_history', normalized->'effect_history'
  ) as normalized_timeline
  from owned
), usage as (
  select private.shop_guest_import_usage_normalize(value, normalized_timeline) as normalized_usage
  from timeline
)
select ok(
  normalized_usage is not null
    and normalized_usage->>'validation_scope' = 'usage_effect_activity_consistency',
  'the nonzero multi-segment reset source passes ownership, effect and usage normalization'
)
from usage;

with source as (
  select pg_temp.shop_guest_import_single_reset_floor_once_request(
    'f1000000-0000-4000-a000-000000000056',
    'f0000000-0000-4000-a000-000000000001', 'current-after-floor-once'
  )#>'{snapshot,data}' as value
), normalized as (
  select pg_temp.shop_guest_import_try_reset_source_normalize(value) as result
  from source
)
select ok(
  coalesce(
    result->>'validation_scope' = 'reset_settlement_consistency'
      and result#>>'{cycle_token_credits,0,cycle_id}' = 'history-cycle'
      and (result#>>'{cycle_token_credits,0,raw_tokens}')::bigint = 100
      and (result#>>'{cycle_token_credits,0,bonus_tokens}')::bigint = 1
      and (result#>>'{cycle_token_credits,0,total_tokens}')::bigint = 101
      and floor(60::numeric * 100 / 10000) + floor(40::numeric * 100 / 10000) = 0,
    false
  ),
  'reset bonus floors the weighted sum once: raw 100 plus bonus 1, while per-segment floors would be zero'
)
from normalized;

with source as (
  select pg_temp.shop_guest_import_single_reset_request(
    'f1000000-0000-4000-a000-000000000055',
    'f0000000-0000-4000-a000-000000000001', 'current-after-reset'
  ) as request
), data as (
  select request#>'{snapshot,data}' as value from source
), owned as (
  select value, private.shop_guest_import_ownership_normalize(value) as normalized
  from data
), timeline as (
  select value, normalized, jsonb_build_object(
    'effect_cycle_bounds', normalized->'effect_cycle_bounds',
    'effect_history', normalized->'effect_history'
  ) as normalized_timeline
  from owned
), usage as (
  select value, normalized, normalized_timeline,
    private.shop_guest_import_usage_normalize(value, normalized_timeline) as normalized_usage
  from timeline
)
select ok(normalized is not null,
  'the single-reset source passes ownership and effect normalization')
from usage;

with source as (
  select pg_temp.shop_guest_import_single_reset_request(
    'f1000000-0000-4000-a000-000000000055',
    'f0000000-0000-4000-a000-000000000001', 'current-after-reset'
  ) as request
), data as (
  select request#>'{snapshot,data}' as value from source
), owned as (
  select value, private.shop_guest_import_ownership_normalize(value) as normalized
  from data
), timeline as (
  select value, jsonb_build_object(
    'effect_cycle_bounds', normalized->'effect_cycle_bounds',
    'effect_history', normalized->'effect_history'
  ) as normalized_timeline
  from owned
), usage as (
  select value, normalized_timeline,
    private.shop_guest_import_usage_normalize(value, normalized_timeline) as normalized_usage
  from timeline
)
select ok(
  normalized_usage is not null
    and normalized_usage->>'validation_scope' = 'usage_effect_activity_consistency',
  'the single-reset source passes the accepted usage and effect-activity normalizers'
)
from usage;

with source as (
  select pg_temp.shop_guest_import_single_reset_request(
    'f1000000-0000-4000-a000-000000000055',
    'f0000000-0000-4000-a000-000000000001', 'current-after-reset'
  ) as request
), data as (
  select request#>'{snapshot,data}' as value from source
)
select ok(
  jsonb_array_length(value->'reset_settlement_proofs') = 1
    and jsonb_array_length(value->'cycle_settlements') = 1
    and jsonb_array_length(value->'unverified_planet_wallet_claims') = 1
    and jsonb_array_length(value->'growth_journal_cycles') = 2
    and value#>'{reset_settlement_proofs,0,raw_wallet_claim}'
      = value#>'{unverified_planet_wallet_claims,0}',
  'the single-reset fixture pairs its proof, settlement, raw claim and journal rows'
)
from data;

with source as (
  select pg_temp.shop_guest_import_single_reset_request(
    'f1000000-0000-4000-a000-000000000055',
    'f0000000-0000-4000-a000-000000000001', 'current-after-reset'
  ) as request
), data as (
  select request#>'{snapshot,data}' as value from source
), owned as (
  select value, private.shop_guest_import_ownership_normalize(value) as normalized
  from data
), timeline as (
  select value, jsonb_build_object(
    'effect_cycle_bounds', normalized->'effect_cycle_bounds',
    'effect_history', normalized->'effect_history'
  ) as normalized_timeline
  from owned
), usage as (
  select value, normalized_timeline,
    private.shop_guest_import_usage_normalize(value, normalized_timeline) as normalized_usage
  from timeline
), reset as (
  select normalized_usage, pg_temp.shop_guest_import_try_reset_normalize(
    value, normalized_usage, normalized_timeline
  ) as normalized_reset
  from usage
)
select ok(
  coalesce(
    normalized_usage->>'validation_scope' = 'usage_effect_activity_consistency'
      and normalized_reset->>'validation_scope' = 'reset_settlement_consistency'
      and jsonb_array_length(normalized_reset->'reset_chain') = 1
      and normalized_reset#>>'{reset_chain,0,request_id}' = 'reset-history-cycle'
      and normalized_reset#>>'{reset_chain,0,previous_cycle_id}' = 'history-cycle'
      and normalized_reset#>>'{reset_chain,0,new_cycle_id}' = 'current-after-reset'
      and (normalized_reset#>>'{reset_chain,0,reset_at_utc}')::timestamptz
        = '2026-10-01T12:00:00+00:00'::timestamptz
      and (normalized_reset#>>'{reset_chain,0,reset_available_at_utc}')::timestamptz
        = '2026-10-02T12:00:00+00:00'::timestamptz
      and jsonb_array_length(normalized_reset->'cycle_token_credits') = 1
      and normalized_reset#>>'{cycle_token_credits,0,cycle_id}' = 'history-cycle'
      and (normalized_reset#>>'{cycle_token_credits,0,raw_tokens}')::bigint = 40
      and (normalized_reset#>>'{cycle_token_credits,0,bonus_tokens}')::bigint = 0
      and (normalized_reset#>>'{cycle_token_credits,0,total_tokens}')::bigint = 40,
    false
  ),
  'one reset reconstructs exact raw 40, bonus 0 and the frozen 24-hour deadline'
)
from reset;

with source as (
  select pg_temp.shop_guest_import_single_reset_request(
    'f1000000-0000-4000-a000-000000000055',
    'f0000000-0000-4000-a000-000000000001', 'current-after-reset'
  )#>'{snapshot,data}' as source_data
), variants as (
  select cases.variant, cases.candidate_data
  from source s
  cross join lateral (
    values
      ('proof raw claim disagrees with captured claim', jsonb_set(
        s.source_data, '{reset_settlement_proofs,0,raw_wallet_claim,claimed_amount}', '41'::jsonb, true
      )),
      ('claim, proof and journal agree on raw 41 while contributions sum to 40', jsonb_set(
        jsonb_set(
          jsonb_set(s.source_data, '{unverified_planet_wallet_claims,0,claimed_amount}', '41'::jsonb, true),
          '{reset_settlement_proofs,0,raw_wallet_claim,claimed_amount}', '41'::jsonb, true
        ), '{growth_journal_cycles,0,wallet_credit}', '41'::jsonb, true
      )),
      ('proof, settlement and declared cycle bonus disagree with computed zero', jsonb_set(
        jsonb_set(
          jsonb_set(s.source_data, '{reset_settlement_proofs,0,settled_bonus_tokens}', '1'::jsonb, true),
          '{cycle_settlements,0,amount}', '1'::jsonb, true
        ), '{historical_cycles,0,settled_bonus_tokens}', '1'::jsonb, true
      )),
      ('proof final effect revision differs from the old cycle last interval', jsonb_set(
        s.source_data, '{reset_settlement_proofs,0,final_effect_revision}', '2'::jsonb, true
      )),
      ('proof final effects differ from the old cycle last interval', jsonb_set(
        s.source_data, '{reset_settlement_proofs,0,final_effects,token_earning_bps}', '0'::jsonb, true
      )),
      ('proof and account advertise 18 hours when the authoritative cooldown is 24 hours', jsonb_set(
        jsonb_set(s.source_data, '{reset_available_at_utc}', '"2026-10-02T06:00:00+00:00"'::jsonb, true),
        '{reset_settlement_proofs,0,reset_available_at_utc}',
        '"2026-10-02T06:00:00+00:00"'::jsonb, true
      )),
      ('duplicate proof request is rejected', jsonb_set(
        s.source_data, '{reset_settlement_proofs}',
        (s.source_data->'reset_settlement_proofs') || (s.source_data->'reset_settlement_proofs'), true
      )),
      ('orphan settlement row is rejected', jsonb_set(
        s.source_data, '{cycle_settlements}',
        (s.source_data->'cycle_settlements') || jsonb_build_array(jsonb_build_object(
          'cycle_id', 'orphan-cycle', 'amount', 0, 'settled_at_utc', '2026-10-01T12:00:00+00:00'
        )), true
      )),
      ('orphan wallet claim is rejected', jsonb_set(
        s.source_data, '{unverified_planet_wallet_claims}',
        (s.source_data->'unverified_planet_wallet_claims') || jsonb_build_array(jsonb_build_object(
          'previous_cycle_id', 'orphan-cycle', 'claimed_amount', 0,
          'created_at_utc', '2026-10-01T12:00:00+00:00'
        )), true
      )),
      ('journal cycle set with an extra row is rejected', jsonb_set(
        s.source_data, '{growth_journal_cycles}',
        (s.source_data->'growth_journal_cycles') || jsonb_build_array(s.source_data#>'{growth_journal_cycles,1}'), true
      )),
      ('settlement timestamp differs from reset time', jsonb_set(
        s.source_data, '{cycle_settlements,0,settled_at_utc}',
        '"2026-10-01T12:00:01+00:00"'::jsonb, true
      )),
      ('reset event is later than the authoritative server time', jsonb_set(
        jsonb_set(s.source_data, '{last_reset_at_utc}', '"2026-10-01T14:00:00+00:00"'::jsonb, true),
        '{reset_settlement_proofs,0,reset_at_utc}', '"2026-10-01T14:00:00+00:00"'::jsonb, true
      )),
      ('required frozen availability timestamp is missing', jsonb_set(
        jsonb_set(s.source_data, '{reset_available_at_utc}', 'null'::jsonb, true),
        '{reset_settlement_proofs,0,reset_available_at_utc}', 'null'::jsonb, true
      )),
      ('raw claim proof exceeds signed bigint range', jsonb_set(
        s.source_data, '{reset_settlement_proofs,0,raw_wallet_claim,claimed_amount}',
        '9223372036854775808'::jsonb, true
      ))
  ) cases(variant, candidate_data)
)
select ok(
  pg_temp.shop_guest_import_try_reset_source_normalize(candidate_data) is null,
  'held reset evidence: ' || variant
)
from variants;

with source as (
  select pg_temp.shop_guest_import_usage_request(
    'f1000000-0000-4000-a000-000000000054',
    'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
  ) as request
), data as (
  select request#>'{snapshot,data}' as value from source
), owned as (
  select value, private.shop_guest_import_ownership_normalize(value) as normalized
  from data
), timeline as (
  select value, jsonb_build_object(
    'effect_cycle_bounds', normalized->'effect_cycle_bounds',
    'effect_history', normalized->'effect_history'
  ) as normalized_timeline
  from owned
), usage as (
  select value, normalized_timeline,
    private.shop_guest_import_usage_normalize(value, normalized_timeline) as normalized_usage
  from timeline
), reset as (
  select pg_temp.shop_guest_import_try_reset_normalize(
    value, normalized_usage, normalized_timeline
  ) as normalized_reset
  from usage
)
select ok(
  coalesce(
    normalized_reset->>'validation_scope' = 'reset_settlement_consistency'
      and normalized_reset->'reset_chain' = '[]'::jsonb
      and normalized_reset->'cycle_token_credits' = '[]'::jsonb,
    false
  ),
  'a no-reset current-cycle capture normalizes to empty reset and cycle-credit arrays'
)
from reset;

with source as (
  select pg_temp.shop_guest_import_usage_request(
    'f1000000-0000-4000-a000-000000000054',
    'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
  ) as request
), data as (
  select value, variant
  from source s
  cross join lateral (
    values
      (jsonb_set(s.request#>'{snapshot,data}', '{reset_receipts_unverifiable}', '"false"'::jsonb, true), 'string false'),
      (jsonb_set(s.request#>'{snapshot,data}', '{reset_receipts_unverifiable}', '0'::jsonb, true), 'number'),
      (jsonb_set(s.request#>'{snapshot,data}', '{reset_receipts_unverifiable}', 'null'::jsonb, true), 'null'),
      ((s.request#>'{snapshot,data}') - 'reset_receipts_unverifiable', 'missing')
  ) bad(value, variant)
), owned as (
  select value, variant, private.shop_guest_import_ownership_normalize(value) as normalized
  from data
), timeline as (
  select value, variant, jsonb_build_object(
    'effect_cycle_bounds', normalized->'effect_cycle_bounds',
    'effect_history', normalized->'effect_history'
  ) as normalized_timeline
  from owned
), usage as (
  select value, variant, normalized_timeline,
    private.shop_guest_import_usage_normalize(value, normalized_timeline) as normalized_usage
  from timeline
), reset as (
  select variant, normalized_usage, pg_temp.shop_guest_import_try_reset_normalize(
    value, normalized_usage, normalized_timeline
  ) as normalized_reset
  from usage
)
select ok(
  normalized_usage is not null and normalized_reset is null,
  'a ' || variant || ' reset uncertainty flag is rejected after usage normalization'
)
from reset;

with source as (
  select pg_temp.shop_guest_import_single_reset_request(
    'f1000000-0000-4000-a000-000000000058',
    'f0000000-0000-4000-a000-000000000001', 'current-after-reset'
  )#>'{snapshot,data}' as value
), owned as (
  select value, private.shop_guest_import_ownership_normalize(value) as normalized
  from source
), timeline as (
  select value, jsonb_build_object(
    'effect_cycle_bounds', normalized->'effect_cycle_bounds',
    'effect_history', normalized->'effect_history'
  ) as normalized_timeline
  from owned
), core as (
  select value, normalized_timeline,
    private.shop_guest_import_usage_core_normalize(value) as normalized_usage
  from timeline
), reset as (
  select normalized_usage, pg_temp.shop_guest_import_try_reset_normalize(
    value, normalized_usage, normalized_timeline
  ) as normalized_reset
  from core
)
select ok(
  normalized_usage is not null
    and normalized_usage->>'validation_scope' = 'usage_journal_core'
    and normalized_reset is null,
  'reset reconstruction refuses core totals without effect and activity consistency'
)
from reset;

with source as (
  select pg_temp.shop_guest_import_single_reset_request(
    'f1000000-0000-4000-a000-000000000059',
    'f0000000-0000-4000-a000-000000000001', 'current-after-reset'
  )#>'{snapshot,data}' as value
), owned as (
  select value, private.shop_guest_import_ownership_normalize(value) as normalized
  from source
), timeline as (
  select value, jsonb_build_object(
    'effect_cycle_bounds', normalized->'effect_cycle_bounds',
    'effect_history', normalized->'effect_history'
  ) as normalized_timeline
  from owned
), usage as (
  select value, normalized_timeline,
    private.shop_guest_import_usage_normalize(value, normalized_timeline) as normalized_usage
  from timeline
), candidates as (
  select value, normalized_usage, candidate.variant, candidate.timeline,
    jsonb_set(normalized_usage, '{effect_cycle_bounds}', candidate.timeline->'effect_cycle_bounds', true)
      as candidate_usage
  from usage
  cross join lateral (
    values
      ('old cycle bound missing', jsonb_set(
        normalized_timeline, '{effect_cycle_bounds}',
        jsonb_build_array(normalized_timeline#>'{effect_cycle_bounds,1}'), true
      )),
      ('current cycle bound missing', jsonb_set(
        normalized_timeline, '{effect_cycle_bounds}',
        jsonb_build_array(normalized_timeline#>'{effect_cycle_bounds,0}'), true
      ))
  ) candidate(variant, timeline)
)
select ok(
  normalized_usage is not null
    and pg_temp.shop_guest_import_try_reset_normalize(value, candidate_usage, timeline) is null,
  'reset reconstruction holds when the ' || variant || ' is omitted from normalized bounds'
)
from candidates;

with source as (
  select pg_temp.shop_guest_import_single_reset_overflow_request(
    'f1000000-0000-4000-a000-000000000060',
    'f0000000-0000-4000-a000-000000000001', 'current-after-overflow'
  )#>'{snapshot,data}' as value
), owned as (
  select value, private.shop_guest_import_ownership_normalize(value) as normalized
  from source
), timeline as (
  select value, jsonb_build_object(
    'effect_cycle_bounds', normalized->'effect_cycle_bounds',
    'effect_history', normalized->'effect_history'
  ) as normalized_timeline
  from owned
), usage as (
  select value, normalized_timeline,
    private.shop_guest_import_usage_normalize(value, normalized_timeline) as normalized_usage
  from timeline
), credits as (
  select value, normalized_usage, pg_temp.shop_guest_import_try_reset_normalize(
      value, normalized_usage, normalized_timeline
    ) as normalized_reset,
    sum((contribution.row->>'tokens')::numeric) as raw_tokens,
    floor(sum((contribution.row->>'tokens')::numeric * (contribution.row->>'wallet_bps')::numeric) / 10000)
      as computed_bonus
  from usage
  cross join lateral jsonb_array_elements(normalized_usage->'effect_contributions') as contribution(row)
  where contribution.row->>'cycle_id' = 'history-cycle'
  group by value, normalized_usage, normalized_timeline
)
select ok(
  normalized_usage is not null
    and normalized_usage->>'validation_scope' = 'usage_effect_activity_consistency'
    and raw_tokens = 9223372036854775807::numeric
    and computed_bonus > 0
    and raw_tokens + computed_bonus > 9223372036854775807::numeric
    and normalized_reset is null,
  'verified raw plus weighted reset bonus above signed bigint range is held'
)
from credits;

with source as (
  select replace(
    (pg_temp.shop_guest_import_single_reset_request(
      'f1000000-0000-4000-a000-000000000061',
      'f0000000-0000-4000-a000-000000000001', '2'
    )#>'{snapshot,data}')::text,
    '"history-cycle"', '"1"'
  )::jsonb as value
), owned as (
  select value, private.shop_guest_import_ownership_normalize(value) as normalized
  from source
), timeline as (
  select value, jsonb_build_object(
    'effect_cycle_bounds', normalized->'effect_cycle_bounds',
    'effect_history', normalized->'effect_history'
  ) as normalized_timeline
  from owned
), usage as (
  select value, normalized_timeline,
    private.shop_guest_import_usage_normalize(value, normalized_timeline) as normalized_usage
  from timeline
), reset as (
  select normalized_usage, pg_temp.shop_guest_import_try_reset_normalize(
    value, normalized_usage, normalized_timeline
  ) as normalized_reset
  from usage
)
select ok(
  normalized_usage is not null
    and normalized_reset->>'validation_scope' = 'reset_settlement_consistency'
    and normalized_reset#>>'{reset_chain,0,previous_cycle_id}' = '1',
  'a fully consistent source with old cycle ID string "1" is accepted'
)
from reset;

with source as (
  select replace(
    (pg_temp.shop_guest_import_single_reset_request(
      'f1000000-0000-4000-a000-000000000061',
      'f0000000-0000-4000-a000-000000000001', '2'
    )#>'{snapshot,data}')::text,
    '"history-cycle"', '"1"'
  )::jsonb as value
), variants as (
  select cases.variant, jsonb_set(source.value, cases.path, cases.bad_value, true) as candidate_data
  from source
  cross join lateral (
    values
      ('captured claim previous cycle ID', array['unverified_planet_wallet_claims','0','previous_cycle_id']::text[], '1'::jsonb),
      ('proof nested raw claim previous cycle ID', array['reset_settlement_proofs','0','raw_wallet_claim','previous_cycle_id']::text[], '1'::jsonb),
      ('settlement cycle ID', array['cycle_settlements','0','cycle_id']::text[], '1'::jsonb),
      ('historical cycle ID', array['historical_cycles','0','cycle_id']::text[], '1'::jsonb),
      ('old journal cycle ID', array['growth_journal_cycles','0','cycle_id']::text[], '1'::jsonb),
      ('current journal cycle ID', array['growth_journal_cycles','1','cycle_id']::text[], '2'::jsonb)
  ) cases(variant, path, bad_value)
), owned as (
  select variant, candidate_data,
    private.shop_guest_import_ownership_normalize(candidate_data) as normalized
  from variants
), timeline as (
  select variant, candidate_data, jsonb_build_object(
    'effect_cycle_bounds', normalized->'effect_cycle_bounds',
    'effect_history', normalized->'effect_history'
  ) as normalized_timeline
  from owned
), usage as (
  select variant, candidate_data, normalized_timeline,
    private.shop_guest_import_usage_normalize(candidate_data, normalized_timeline) as normalized_usage
  from timeline
), reset as (
  select variant, normalized_usage, pg_temp.shop_guest_import_try_reset_normalize(
    candidate_data, normalized_usage, normalized_timeline
  ) as normalized_reset
  from usage
)
select ok(
  case
    when variant in ('historical cycle ID', 'old journal cycle ID', 'current journal cycle ID')
      then normalized_usage is null and normalized_reset is null
    else normalized_usage is not null and normalized_reset is null
  end,
  case
    when variant in ('historical cycle ID', 'old journal cycle ID', 'current journal cycle ID')
      then 'upstream usage normalization rejects non-string ' || variant
    else 'reset normalization rejects non-string ' || variant
  end
)
from reset;

select * from finish();
rollback;
