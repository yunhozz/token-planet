begin;
create extension if not exists pgtap with schema extensions;
select no_plan();
\ir fixtures/shop_guest_import.inc
insert into auth.users(id) values
  ('f0000000-0000-4000-a000-000000000001'),
  ('f0000000-0000-4000-a000-000000000002'),
  ('f0000000-0000-4000-a000-000000000003');

select ok(to_regprocedure('public.import_guest_shop(uuid,jsonb)') is not null,
  'the fail-closed guest shop import RPC exists');

set local role authenticated;
select set_config('request.jwt.claim.sub', '', true);
select throws_ok($$select public.import_guest_shop(
  'f1000000-0000-4000-a000-000000000001',
  pg_temp.shop_guest_import_empty_request(
    'f1000000-0000-4000-a000-000000000001',
    'f0000000-0000-4000-a000-000000000001', 'import-cycle'
  )
)$$, '42501', null, 'guest import requires an authenticated account');

select set_config('request.jwt.claim.sub', 'f0000000-0000-4000-a000-000000000001', true);
select throws_ok($$select public.import_guest_shop(
  'f1000000-0000-4000-a000-000000000002',
  pg_temp.shop_guest_import_empty_request(
    'f1000000-0000-4000-a000-000000000002',
    'f0000000-0000-4000-a000-000000000002', 'import-cycle'
  )
)$$, '42501', null, 'guest import cannot target another account');

select throws_ok($$select public.import_guest_shop(
  'f1000000-0000-4000-a000-000000000003',
  jsonb_set(pg_temp.shop_guest_import_empty_request(
    'f1000000-0000-4000-a000-000000000003',
    'f0000000-0000-4000-a000-000000000001', 'import-cycle'
  ), '{snapshot,data,shop_state_revision}', '"0"'::jsonb)
)$$, '23514', null, 'guest import rejects a typed-field mismatch');

select throws_ok($$select public.import_guest_shop(
  'f1000000-0000-4000-a000-000000000006',
  jsonb_set(pg_temp.shop_guest_import_empty_request(
    'f1000000-0000-4000-a000-000000000006',
    'f0000000-0000-4000-a000-000000000001', 'import-cycle'
  ), '{snapshot,data,activation_at_utc}', '"2026-02-30T00:00:00+00:00"'::jsonb)
)$$, '23514', null, 'guest import rejects invalid calendar timestamps');

select is(
  public.import_guest_shop(
    'f1000000-0000-4000-a000-000000000004',
    pg_temp.shop_guest_import_empty_request(
      'f1000000-0000-4000-a000-000000000004',
      'f0000000-0000-4000-a000-000000000001', 'import-cycle'
    )
  )->>'status',
  'source_unverifiable',
  'a well-formed local snapshot is held without server ledger proof'
);

select is(
  public.import_guest_shop(
    'f1000000-0000-4000-a000-000000000005',
    pg_temp.shop_guest_import_with_wallet_claim(
      'f1000000-0000-4000-a000-000000000005',
      'f0000000-0000-4000-a000-000000000001', 'import-cycle'
    )
  )->>'status',
  'source_unverifiable',
  'a local wallet claim is never accepted as server credit authority'
);

select is(
  public.import_guest_shop(
    'f1000000-0000-4000-a000-000000000007',
    jsonb_set(pg_temp.shop_guest_import_empty_request(
      'f1000000-0000-4000-a000-000000000007',
      'f0000000-0000-4000-a000-000000000001', 'import-cycle'
    ), '{snapshot,data,integrity_issues}',
    '[{"kind":"purchase_proof_unverifiable","purchase_id":"purchase-1"}]'::jsonb)
  )->>'status',
  'source_unverifiable',
  'a typed integrity issue stays held instead of being treated as credit proof'
);

select is(
  public.import_guest_shop(
    'f1000000-0000-4000-a000-000000000008',
    jsonb_set(pg_temp.shop_guest_import_empty_request(
      'f1000000-0000-4000-a000-000000000008',
      'f0000000-0000-4000-a000-000000000001', 'import-cycle'
    ), '{snapshot,data,landscape_instances}',
    '[{"instance_id":"bad-id","sku":"bad_sku","variation_index":"bad","seed":"seed","variation_version":-1,"acquired_at_utc":"not-a-time"}]'::jsonb)
  )->>'status',
  'source_unverifiable',
  'malformed source rows remain held and are never imported'
);
reset role;

select is((select count(*)::integer from public.planet_member_state
  where user_id = 'f0000000-0000-4000-a000-000000000001'), 0,
  'guest import does not create a target profile or planet state');
select is((select count(*)::integer from private.shop_account_state
  where user_id = 'f0000000-0000-4000-a000-000000000001'), 0,
  'guest import does not initialize shop game state');
select is((select count(*)::integer from private.planet_wallet_credits
  where user_id = 'f0000000-0000-4000-a000-000000000001'), 0,
  'guest import does not write wallet credits');
select is((select count(*)::integer from private.shop_purchase
  where user_id = 'f0000000-0000-4000-a000-000000000001'), 0,
  'guest import does not write purchases');
select is((select count(*)::integer from private.shop_avatar_owned
  where user_id = 'f0000000-0000-4000-a000-000000000001'), 0,
  'guest import does not write owned items');

set local role authenticated;
select set_config('request.jwt.claim.sub', 'f0000000-0000-4000-a000-000000000001', true);
select is(
  public.import_guest_shop(
    'f1000000-0000-4000-a000-000000000009',
    pg_temp.shop_guest_import_empty_request(
      'f1000000-0000-4000-a000-000000000009',
      'f0000000-0000-4000-a000-000000000001', 'receipt-cycle'
    )
  )->>'status',
  'source_unverifiable',
  'a fresh account receives a held receipt before ledger proof exists'
);
select is(
  public.import_guest_shop(
    'f1000000-0000-4000-a000-000000000009',
    pg_temp.shop_guest_import_empty_request(
      'f1000000-0000-4000-a000-000000000009',
      'f0000000-0000-4000-a000-000000000001', 'receipt-cycle'
    )
  ),
  pg_temp.shop_guest_import_receipt_result(
    'f0000000-0000-4000-a000-000000000001',
    'f1000000-0000-4000-a000-000000000009'
  ),
  'same import ID and exact payload replay the stored result'
);
select is(
  public.import_guest_shop(
    'f1000000-0000-4000-a000-000000000009',
    jsonb_set(pg_temp.shop_guest_import_empty_request(
      'f1000000-0000-4000-a000-000000000009',
      'f0000000-0000-4000-a000-000000000001', 'receipt-cycle'
    ), '{snapshot,data,shop_state_revision}', '1'::jsonb)
  )->>'status',
  'request_conflict',
  'same import ID with a different exact JSONB payload conflicts'
);
select set_config('request.jwt.claim.sub', 'f0000000-0000-4000-a000-000000000002', true);
select is(
  public.import_guest_shop(
    'f1000000-0000-4000-a000-000000000009',
    pg_temp.shop_guest_import_empty_request(
      'f1000000-0000-4000-a000-000000000009',
      'f0000000-0000-4000-a000-000000000002', 'receipt-cycle'
    )
  )->>'status',
  'source_unverifiable',
  'another user can use the same import ID only in their own account scope'
);
reset role;
select is(pg_temp.shop_guest_import_receipt_count(
  'f0000000-0000-4000-a000-000000000001',
  'f1000000-0000-4000-a000-000000000009'
), 1, 'same-payload replay and conflict keep one private receipt');
select is(pg_temp.shop_guest_import_receipt_count(
  'f0000000-0000-4000-a000-000000000002',
  'f1000000-0000-4000-a000-000000000009'
), 1, 'import receipts are isolated by authenticated user');
select is((select count(*)::integer from private.shop_account_lock
  where user_id = 'f0000000-0000-4000-a000-000000000001'), 1,
  'guest import may persist only the shared account lock metadata');
select is((select count(*)::integer from private.shop_account_state
  where user_id = 'f0000000-0000-4000-a000-000000000001'), 0,
  'locking does not initialize game state');

insert into public.planet_member_state(
  user_id, nickname, avatar, timezone, current_cycle_id, cycle_started_at,
  current_planet_tokens, lifetime_tokens, growth_credit, stage, progress_to_next,
  incomplete, objects
) values (
  'f0000000-0000-4000-a000-000000000003', 'existing-profile', 'feminine', 'UTC',
  'active-cycle', '2026-10-01T00:00:00Z', 0, 0, 0, 0, 0, false, '[]'::jsonb
);
set local role authenticated;
select set_config('request.jwt.claim.sub', 'f0000000-0000-4000-a000-000000000003', true);
select is(
  public.import_guest_shop(
    'f1000000-0000-4000-a000-000000000010',
    pg_temp.shop_guest_import_empty_request(
      'f1000000-0000-4000-a000-000000000010',
      'f0000000-0000-4000-a000-000000000003', 'active-cycle'
    )
  )->>'status',
  'active_account',
  'any existing profile blocks import even when the wallet is zero'
);
reset role;
select is((select nickname from public.planet_member_state
  where user_id = 'f0000000-0000-4000-a000-000000000003'), 'existing-profile',
  'active account profile is preserved without overwrite');

select ok(to_regclass('private.shop_guest_import_request') is not null,
  'guest import results use a private receipt table');
select ok(pg_temp.shop_guest_import_no_select('anon'),
  'anonymous role cannot read guest import receipts');
select ok(pg_temp.shop_guest_import_no_select('authenticated'),
  'authenticated role cannot read guest import receipts');

with source as (
  select pg_temp.shop_guest_import_purchase_removal_request(
    'f1000000-0000-4000-a000-000000000011',
    'f0000000-0000-4000-a000-000000000001',
    'ownership-cycle'
  )#>'{snapshot,data}' as data
), normalized as (
  select pg_temp.shop_guest_import_try_ownership_normalize(data) as value from source
)
select ok(to_regprocedure('private.shop_guest_import_ownership_normalize(jsonb)') is not null,
  'the private ownership proof normalizer exists');

with source as (
  select pg_temp.shop_guest_import_purchase_removal_request(
    'f1000000-0000-4000-a000-000000000011',
    'f0000000-0000-4000-a000-000000000001',
    'ownership-cycle'
  )#>'{snapshot,data}' as data
), normalized as (
  select pg_temp.shop_guest_import_try_ownership_normalize(data) as value from source
)
select ok(
  (select value is not null
    and value->>'purchase_count' = '1'
    and value->>'removal_count' = '1'
    and value#>>'{landscape_instances,0,sku}' = 'land_pond'
    and value#>>'{landscape_instances,0,catalog_revision}' = '1'
    and value#>>'{natural_removals,0,price}' = '100000'
   from normalized),
  'a typed landscape purchase and natural-removal proof normalize to catalog-bound ownership rows'
);

with source as (
  select jsonb_set(
    pg_temp.shop_guest_import_purchase_removal_request(
      'f1000000-0000-4000-a000-000000000012',
      'f0000000-0000-4000-a000-000000000001',
      'ownership-cycle'
    ), '{snapshot,data,purchase_proofs,0,quote,price}', '1'::jsonb
  )#>'{snapshot,data}' as data
)
select ok(pg_temp.shop_guest_import_try_ownership_normalize(data) is null,
  'a mismatched final purchase proof rejects the whole ownership normalization')
from source;

with source as (
  select jsonb_set(
    pg_temp.shop_guest_import_purchase_removal_request(
      'f1000000-0000-4000-a000-000000000013',
      'f0000000-0000-4000-a000-000000000001',
      'ownership-cycle'
    ), '{snapshot,data,landscape_instances,0,variation_index}', '5'::jsonb
  )#>'{snapshot,data}' as data
)
select ok(pg_temp.shop_guest_import_try_ownership_normalize(data) is null,
  'out-of-catalog landscape variation identity rejects normalization')
from source;

with source as (
  select pg_temp.shop_guest_import_purchase_removal_request(
    'f1000000-0000-4000-a000-000000000014',
    'f0000000-0000-4000-a000-000000000001',
    'ownership-cycle'
  ) as request
), duplicated_binding as (
  select jsonb_set(
    jsonb_set(
      jsonb_set(
        jsonb_set(
          request,
          '{snapshot,data,landscape_instances}',
          request#>'{snapshot,data,landscape_instances}' || jsonb_build_array(
            jsonb_set(
              jsonb_set(request#>'{snapshot,data,landscape_instances,0}',
                '{instance_id}', to_jsonb('20000000-0000-4000-a000-000000000002'::text)),
              '{variation_index}', '1'::jsonb
            )
          )
        ),
        '{snapshot,data,landscape_edit_versions}',
        request#>'{snapshot,data,landscape_edit_versions}' || jsonb_build_array(
          jsonb_build_object(
            'instance_id', '20000000-0000-4000-a000-000000000002',
            'version', 0
          )
        )
      ),
      '{snapshot,data,purchases}',
      request#>'{snapshot,data,purchases}' || jsonb_build_array(
        jsonb_set(request#>'{snapshot,data,purchases,0}',
          '{purchase_id}', to_jsonb('purchase-pond-2'::text))
      )
    ),
    '{snapshot,data,purchase_proofs}',
    request#>'{snapshot,data,purchase_proofs}' || jsonb_build_array(
      jsonb_set(request#>'{snapshot,data,purchase_proofs,0}',
        '{request_id}', to_jsonb('purchase-pond-2'::text))
    )
  ) as request
  from source
)
select ok(pg_temp.shop_guest_import_try_ownership_normalize(
    request#>'{snapshot,data}'
  ) is null,
  'two purchases cannot bind to one landscape instance and leave another orphaned')
from duplicated_binding;

with source as (
  select pg_temp.shop_guest_import_empty_request(
    'f1000000-0000-4000-a000-000000000015',
    'f0000000-0000-4000-a000-000000000001',
    'ownership-cycle'
  ) as request
), duplicated_removal_ids as (
  select jsonb_set(
    jsonb_set(
      jsonb_set(
        jsonb_set(
          request,
          '{snapshot,data,natural_objects}',
          jsonb_build_array(
            jsonb_build_object(
              'cycle_id', 'ownership-cycle', 'stage', 0, 'ordinal', 0,
              'kind', 'rock', 'x', 1, 'y', 2, 'seed', 17
            ),
            jsonb_build_object(
              'cycle_id', 'ownership-cycle', 'stage', 0, 'ordinal', 1,
              'kind', 'tree', 'x', 5, 'y', 6, 'seed', 23
            )
          )
        ),
        '{snapshot,data,natural_removals}',
        jsonb_build_array(
          jsonb_build_object(
            'cycle_id', 'ownership-cycle', 'stage', 0, 'ordinal', 0,
            'version', 1, 'removed_at_utc', '2026-10-01T00:00:02+00:00'
          ),
          jsonb_build_object(
            'cycle_id', 'ownership-cycle', 'stage', 0, 'ordinal', 1,
            'version', 1, 'removed_at_utc', '2026-10-01T00:00:03+00:00'
          )
        )
      ),
      '{snapshot,data,removal_debits}',
      jsonb_build_array(
        jsonb_build_object(
          'request_id', 'duplicate-removal', 'amount', 100000,
          'created_at_utc', '2026-10-01T00:00:02+00:00'
        ),
        jsonb_build_object(
          'request_id', 'duplicate-removal', 'amount', 100000,
          'created_at_utc', '2026-10-01T00:00:03+00:00'
        )
      )
    ),
    '{snapshot,data,removal_proofs}',
    jsonb_build_array(
      jsonb_build_object(
        'request_id', 'duplicate-removal',
        'target', jsonb_build_object(
          'cycle_id', 'ownership-cycle', 'stage', 0, 'ordinal', 0
        ),
        'quote', jsonb_build_object(
          'target', jsonb_build_object(
            'kind', 'remove_natural',
            'key', jsonb_build_object(
              'cycle_id', 'ownership-cycle', 'stage', 0, 'ordinal', 0
            )
          ),
          'catalog_revision', 1, 'effect_revision', 0, 'price', 100000
        ),
        'status', 'removed'
      ),
      jsonb_build_object(
        'request_id', 'duplicate-removal',
        'target', jsonb_build_object(
          'cycle_id', 'ownership-cycle', 'stage', 0, 'ordinal', 1
        ),
        'quote', jsonb_build_object(
          'target', jsonb_build_object(
            'kind', 'remove_natural',
            'key', jsonb_build_object(
              'cycle_id', 'ownership-cycle', 'stage', 0, 'ordinal', 1
            )
          ),
          'catalog_revision', 1, 'effect_revision', 0, 'price', 100000
        ),
        'status', 'removed'
      )
    )
  ) as request
  from source
)
select ok(pg_temp.shop_guest_import_try_ownership_normalize(
    request#>'{snapshot,data}'
  ) is null,
  'a removal request ID can bind to only one debit and proof target')
from duplicated_removal_ids;

with source as (
  select jsonb_set(
    jsonb_set(
      pg_temp.shop_guest_import_purchase_removal_request(
        'f1000000-0000-4000-a000-000000000017',
        'f0000000-0000-4000-a000-000000000001',
        'ownership-cycle'
      ),
      '{snapshot,data,purchases,0,purchase_id}', to_jsonb('1'::text)
    ),
    '{snapshot,data,purchase_proofs,0,request_id}', '1'::jsonb
  )#>'{snapshot,data}' as data
)
select ok(pg_temp.shop_guest_import_try_ownership_normalize(data) is null,
  'purchase proof request IDs must be JSON strings even when numbers text-match')
from source;

with source as (
  select jsonb_set(
    jsonb_set(
      pg_temp.shop_guest_import_purchase_removal_request(
        'f1000000-0000-4000-a000-000000000018',
        'f0000000-0000-4000-a000-000000000001',
        'ownership-cycle'
      ),
      '{snapshot,data,removal_debits,0,request_id}', to_jsonb('1'::text)
    ),
    '{snapshot,data,removal_proofs,0,request_id}', '1'::jsonb
  )#>'{snapshot,data}' as data
)
select ok(pg_temp.shop_guest_import_try_ownership_normalize(data) is null,
  'removal proof request IDs must be JSON strings even when numbers text-match')
from source;

with source as (
  select jsonb_set(
    pg_temp.shop_guest_import_purchase_removal_request(
      'f1000000-0000-4000-a000-000000000019',
      'f0000000-0000-4000-a000-000000000001',
      'ownership-cycle'
    ),
    '{snapshot,data,removal_proofs,0,target,stage}', to_jsonb('0'::text)
  )#>'{snapshot,data}' as data
)
select ok(pg_temp.shop_guest_import_try_ownership_normalize(data) is null,
  'removal proof target stage must be a JSON unsigned integer')
from source;

with source as (
  select jsonb_set(
    pg_temp.shop_guest_import_purchase_removal_request(
      'f1000000-0000-4000-a000-000000000020',
      'f0000000-0000-4000-a000-000000000001',
      'ownership-cycle'
    ),
    '{snapshot,data,removal_proofs,0,target,ordinal}', to_jsonb('0'::text)
  )#>'{snapshot,data}' as data
)
select ok(pg_temp.shop_guest_import_try_ownership_normalize(data) is null,
  'removal proof target ordinal must be a JSON unsigned integer')
from source;

with source as (
  select jsonb_set(
    pg_temp.shop_guest_import_purchase_removal_request(
      'f1000000-0000-4000-a000-000000000021',
      'f0000000-0000-4000-a000-000000000001',
      'ownership-cycle'
    ),
    '{snapshot,data,removal_proofs,0,quote,target,key,stage}', to_jsonb('0'::text)
  )#>'{snapshot,data}' as data
)
select ok(pg_temp.shop_guest_import_try_ownership_normalize(data) is null,
  'removal quote target key stage must be a JSON unsigned integer')
from source;

with source as (
  select jsonb_set(
    pg_temp.shop_guest_import_purchase_removal_request(
      'f1000000-0000-4000-a000-000000000022',
      'f0000000-0000-4000-a000-000000000001',
      'ownership-cycle'
    ),
    '{snapshot,data,removal_proofs,0,quote,target,key,ordinal}', to_jsonb('0'::text)
  )#>'{snapshot,data}' as data
)
select ok(pg_temp.shop_guest_import_try_ownership_normalize(data) is null,
  'removal quote target key ordinal must be a JSON unsigned integer')
from source;

with source as (
  select pg_temp.shop_guest_import_effect_request(
    'f1000000-0000-4000-a000-000000000023',
    'f0000000-0000-4000-a000-000000000001',
    'ownership-cycle'
  )#>'{snapshot,data}' as data
), normalized as (
  select pg_temp.shop_guest_import_try_ownership_normalize(data) as value from source
)
select ok(
  value is not null
    and value#>>'{effect_cycle_bounds,0,cycle_id}' = 'ownership-cycle'
    and value#>>'{effect_cycle_bounds,0,ended_at_utc}' is null
    and value#>>'{effect_history,0,revision}' = '1'
    and value#>>'{effect_history,0,active_instance_ids,0}' = '20000000-0000-4000-a000-000000000001'
    and value#>>'{effect_history,0,effects,token_earning_bps}' = '100',
  'valid current-cycle effect history normalizes against the owned catalog instance'
)
from normalized;

with source as (
  select jsonb_set(
    pg_temp.shop_guest_import_effect_request(
      'f1000000-0000-4000-a000-000000000024',
      'f0000000-0000-4000-a000-000000000001',
      'ownership-cycle'
    ),
    '{snapshot,data,effect_history,0,effects,token_earning_bps}', '99'::jsonb
  )#>'{snapshot,data}' as data
)
select ok(pg_temp.shop_guest_import_try_ownership_normalize(data) is null,
  'effect history rejects effects that do not match the active catalog instance')
from source;

with source as (
  select jsonb_set(
    jsonb_set(
      pg_temp.shop_guest_import_effect_request(
        'f1000000-0000-4000-a000-000000000025',
        'f0000000-0000-4000-a000-000000000001',
        'ownership-cycle'
      ),
      '{snapshot,data,effect_history,0,active_instance_ids}',
      jsonb_build_array('30000000-0000-4000-a000-000000000001')
    ),
    '{snapshot,data,effect_history,0,effects,token_earning_bps}', '0'::jsonb
  )#>'{snapshot,data}' as data
)
select ok(pg_temp.shop_guest_import_try_ownership_normalize(data) is null,
  'an active instance UUID absent from normalized owned landscape rows is rejected')
from source;

with source as (
  select jsonb_set(
    pg_temp.shop_guest_import_effect_request(
      'f1000000-0000-4000-a000-000000000026',
      'f0000000-0000-4000-a000-000000000001',
      'ownership-cycle'
    ),
    '{snapshot,data,landscape_instances,0,acquired_at_utc}',
    to_jsonb('2026-10-01T00:00:04+00:00'::text)
  )#>'{snapshot,data}' as data
)
select ok(pg_temp.shop_guest_import_try_ownership_normalize(data) is null,
  'an active instance acquired after its interval start is rejected')
from source;

with source as (
  select pg_temp.shop_guest_import_effect_request(
    'f1000000-0000-4000-a000-000000000027',
    'f0000000-0000-4000-a000-000000000001',
    'ownership-cycle'
  ) as request
), with_historical as (
  select jsonb_set(
    jsonb_set(
      jsonb_set(
        request,
        '{snapshot,data,historical_cycles}',
        jsonb_build_array(jsonb_build_object(
          'cycle_id', 'old-cycle',
          'started_at_utc', '2026-09-30T00:00:00+00:00',
          'ended_at_utc', '2026-10-01T00:00:00+00:00',
          'is_current', false,
          'settled_bonus_tokens', null
        ))
      ),
      '{snapshot,data,effect_cycle_bounds}',
      request#>'{snapshot,data,effect_cycle_bounds}' || jsonb_build_array(
        jsonb_build_object(
          'cycle_id', 'old-cycle',
          'started_at_utc', '2026-09-30T00:00:00+00:00',
          'ended_at_utc', '2026-10-01T00:00:00+00:00'
        )
      )
    ),
    '{snapshot,data,effect_history}',
    jsonb_build_array(
      jsonb_build_object(
        'cycle_id', 'old-cycle', 'revision', 1,
        'started_at_utc', '2026-09-30T00:00:00+00:00',
        'ended_at_utc', '2026-10-01T00:00:00+00:00',
        'active_instance_ids', '[]'::jsonb,
        'effects', jsonb_build_object(
          'token_earning_bps', 0, 'civilization_growth_bps', 0,
          'shop_discount_bps', 0, 'reset_cooldown_bps', 0,
          'natural_removal_discount_bps', 0, 'era_reward_tokens', 0,
          'streak_reward_tokens', 0
        )
      ),
      request#>'{snapshot,data,effect_history,0}'
    )
  ) as request
  from source
)
select ok(pg_temp.shop_guest_import_try_ownership_normalize(
    request#>'{snapshot,data}'
  ) is null,
  'effect revisions must be unique across all account cycles')
from with_historical;

with source as (
  select pg_temp.shop_guest_import_effect_request(
    'f1000000-0000-4000-a000-000000000028',
    'f0000000-0000-4000-a000-000000000001',
    'ownership-cycle'
  ) as request
), with_duplicate_active_ids as (
  select jsonb_set(
    jsonb_set(
      request,
      '{snapshot,data,effect_history}',
      jsonb_build_array(
        jsonb_build_object(
          'cycle_id', 'ownership-cycle', 'revision', 1,
          'started_at_utc', '2026-10-01T00:00:01.500000+00:00',
          'ended_at_utc', '2026-10-01T00:00:02.500000+00:00',
          'active_instance_ids', jsonb_build_array(
            '20000000-0000-4000-a000-000000000001',
            '20000000-0000-4000-a000-000000000001'
          ),
          'effects', jsonb_build_object(
            'token_earning_bps', 200, 'civilization_growth_bps', 0,
            'shop_discount_bps', 0, 'reset_cooldown_bps', 0,
            'natural_removal_discount_bps', 0, 'era_reward_tokens', 0,
            'streak_reward_tokens', 0
          )
        ),
        jsonb_set(request#>'{snapshot,data,effect_history,0}', '{revision}', '2'::jsonb)
      )
    ),
    '{snapshot,data,effect_timeline_state,effect_revision}', '2'::jsonb
  )#>'{snapshot,data}' as data
  from source
)
select ok(pg_temp.shop_guest_import_try_ownership_normalize(data) is null,
  'effect intervals reject duplicate active instance IDs')
from with_duplicate_active_ids;

with source as (
  select jsonb_set(
    pg_temp.shop_guest_import_effect_request(
      'f1000000-0000-4000-a000-000000000029',
      'f0000000-0000-4000-a000-000000000001',
      'ownership-cycle'
    ),
    '{snapshot,data,effect_cycle_bounds,0,started_at_utc}',
    to_jsonb('2026-10-01T00:00:01+00:00'::text)
  )#>'{snapshot,data}' as data
)
select ok(pg_temp.shop_guest_import_try_ownership_normalize(data) is null,
  'the authoritative current-cycle bound must match the captured cycle start')
from source;

with source as (
  select pg_temp.shop_guest_import_effect_request(
    'f1000000-0000-4000-a000-000000000030',
    'f0000000-0000-4000-a000-000000000001',
    'ownership-cycle'
  ) as request
)
select ok(pg_temp.shop_guest_import_try_ownership_normalize(
    jsonb_set(
      request,
      '{snapshot,data,effect_cycle_bounds}',
      request#>'{snapshot,data,effect_cycle_bounds}' || request#>'{snapshot,data,effect_cycle_bounds}'
    )#>'{snapshot,data}'
  ) is null,
  'authoritative cycle bounds reject duplicate cycle IDs')
from source;

with source as (
  select pg_temp.shop_guest_import_effect_request(
    'f1000000-0000-4000-a000-000000000031',
    'f0000000-0000-4000-a000-000000000001',
    'ownership-cycle'
  ) as request
), overlap as (
  select jsonb_set(
    jsonb_set(
      request,
      '{snapshot,data,effect_history}',
      jsonb_build_array(
        jsonb_build_object(
          'cycle_id', 'ownership-cycle', 'revision', 1,
          'started_at_utc', '2026-10-01T00:00:02+00:00',
          'ended_at_utc', '2026-10-01T00:00:04+00:00',
          'active_instance_ids', '[]'::jsonb,
          'effects', jsonb_build_object(
            'token_earning_bps', 0, 'civilization_growth_bps', 0,
            'shop_discount_bps', 0, 'reset_cooldown_bps', 0,
            'natural_removal_discount_bps', 0, 'era_reward_tokens', 0,
            'streak_reward_tokens', 0
          )
        ),
        jsonb_set(request#>'{snapshot,data,effect_history,0}', '{revision}', '2'::jsonb)
      )
    ),
    '{snapshot,data,effect_timeline_state,effect_revision}', '2'::jsonb
  )#>'{snapshot,data}' as data
  from source
)
select ok(pg_temp.shop_guest_import_try_ownership_normalize(data) is null,
  'effect intervals with overlapping time ranges are rejected')
from overlap;

with source as (
  select pg_temp.shop_guest_import_effect_request(
    'f1000000-0000-4000-a000-000000000032',
    'f0000000-0000-4000-a000-000000000001',
    'ownership-cycle'
  ) as request
), historical_open_bound as (
  select jsonb_set(
    jsonb_set(
      request,
      '{snapshot,data,historical_cycles}',
      jsonb_build_array(jsonb_build_object(
        'cycle_id', 'old-cycle',
        'started_at_utc', '2026-09-30T00:00:00+00:00',
        'ended_at_utc', null,
        'is_current', false,
        'settled_bonus_tokens', null
      ))
    ),
    '{snapshot,data,effect_cycle_bounds}',
    request#>'{snapshot,data,effect_cycle_bounds}' || jsonb_build_array(
      jsonb_build_object(
        'cycle_id', 'old-cycle',
        'started_at_utc', '2026-09-30T00:00:00+00:00',
        'ended_at_utc', null
      )
    )
  )#>'{snapshot,data}' as data
  from source
)
select ok(pg_temp.shop_guest_import_try_ownership_normalize(data) is null,
  'authoritative historical cycle bounds must be closed')
from historical_open_bound;

with source as (
  select pg_temp.shop_guest_import_effect_request(
    'f1000000-0000-4000-a000-000000000033',
    'f0000000-0000-4000-a000-000000000001',
    'ownership-cycle'
  ) as request
), historical_open_interval as (
  select jsonb_set(
    jsonb_set(
      jsonb_set(
        jsonb_set(
          request,
          '{snapshot,data,placements}', '[]'::jsonb
        ),
        '{snapshot,data,historical_cycles}',
        jsonb_build_array(jsonb_build_object(
          'cycle_id', 'old-cycle',
          'started_at_utc', '2026-09-30T00:00:00+00:00',
          'ended_at_utc', '2026-10-01T00:00:00+00:00',
          'is_current', false,
          'settled_bonus_tokens', null
        ))
      ),
      '{snapshot,data,effect_cycle_bounds}',
      request#>'{snapshot,data,effect_cycle_bounds}' || jsonb_build_array(
        jsonb_build_object(
          'cycle_id', 'old-cycle',
          'started_at_utc', '2026-09-30T00:00:00+00:00',
          'ended_at_utc', '2026-10-01T00:00:00+00:00'
        )
      )
    ),
    '{snapshot,data,effect_history}',
    jsonb_build_array(jsonb_build_object(
      'cycle_id', 'old-cycle', 'revision', 1,
      'started_at_utc', '2026-09-30T00:00:00+00:00',
      'ended_at_utc', null,
      'active_instance_ids', '[]'::jsonb,
      'effects', jsonb_build_object(
        'token_earning_bps', 0, 'civilization_growth_bps', 0,
        'shop_discount_bps', 0, 'reset_cooldown_bps', 0,
        'natural_removal_discount_bps', 0, 'era_reward_tokens', 0,
        'streak_reward_tokens', 0
      )
    ))
  )#>'{snapshot,data}' as data
  from source
)
select ok(pg_temp.shop_guest_import_try_ownership_normalize(data) is null,
  'an open effect interval cannot belong to a historical cycle')
from historical_open_interval;

with source as (
  select jsonb_set(
    pg_temp.shop_guest_import_effect_request(
      'f1000000-0000-4000-a000-000000000034',
      'f0000000-0000-4000-a000-000000000001',
      'ownership-cycle'
    ),
    '{snapshot,data,effect_history,0,active_instance_ids}', '[]'::jsonb
  ) as request
), normalized as (
  select jsonb_set(request, '{snapshot,data,effect_history,0,effects,token_earning_bps}', '0'::jsonb)
    #>'{snapshot,data}' as data from source
)
select ok(pg_temp.shop_guest_import_try_ownership_normalize(data) is null,
  'the current open interval active IDs must match current placements')
from normalized;

with source as (
  select jsonb_set(
    pg_temp.shop_guest_import_effect_request(
      'f1000000-0000-4000-a000-000000000035',
      'f0000000-0000-4000-a000-000000000001',
      'ownership-cycle'
    ),
    '{snapshot,data,effect_history,0,effects,token_earning_bps}', to_jsonb('100'::text)
  )#>'{snapshot,data}' as data
)
select ok(pg_temp.shop_guest_import_try_ownership_normalize(data) is null,
  'effect fields must keep their strict numeric JSON types')
from source;

with source as (
  select jsonb_set(
    pg_temp.shop_guest_import_effect_request(
      'f1000000-0000-4000-a000-000000000036',
      'f0000000-0000-4000-a000-000000000001',
      'ownership-cycle'
    ),
    '{snapshot,data,effect_timeline_state,current_cycle_id}', to_jsonb('other-cycle'::text)
  )#>'{snapshot,data}' as data
)
select ok(pg_temp.shop_guest_import_try_ownership_normalize(data) is null,
  'timeline state must name the captured current cycle')
from source;

with source as (
  select jsonb_set(
    pg_temp.shop_guest_import_effect_request(
      'f1000000-0000-4000-a000-000000000037',
      'f0000000-0000-4000-a000-000000000001',
      'ownership-cycle'
    ),
    '{snapshot,data,effect_timeline_state,effect_revision}', '2'::jsonb
  )#>'{snapshot,data}' as data
)
select ok(pg_temp.shop_guest_import_try_ownership_normalize(data) is null,
  'timeline revision must match the complete captured account-global history')
from source;

with source as (
  select pg_temp.shop_guest_import_empty_request(
    'f1000000-0000-4000-a000-000000000038',
    'f0000000-0000-4000-a000-000000000001',
    'future-bound-cycle'
  ) as request
), future_bound as (
  select jsonb_set(
    jsonb_set(
      jsonb_set(
        jsonb_set(
          request,
          '{snapshot,data,current_cycle,started_at_utc}',
          to_jsonb('2030-01-01T00:00:00+00:00'::text)
        ),
        '{snapshot,data,effect_cycle_bounds_authoritative}', 'true'::jsonb
      ),
      '{snapshot,data,effect_cycle_bounds}', jsonb_build_array(jsonb_build_object(
        'cycle_id', 'future-bound-cycle',
        'started_at_utc', '2030-01-01T00:00:00+00:00',
        'ended_at_utc', null
      ))
    ),
    '{snapshot,data,effect_timeline_state}', jsonb_build_object(
      'current_cycle_id', 'future-bound-cycle',
      'effect_revision', 0,
      'server_time_utc', '2026-10-01T00:00:04+00:00',
      'reward_timezone', 'UTC'
    )
  ) as request
  from source
)
select ok(pg_temp.shop_guest_import_try_ownership_normalize(
    request#>'{snapshot,data}'
  ) is null,
  'authoritative cycle bounds cannot start after timeline server time')
from future_bound;

with source as (
  select pg_temp.shop_guest_import_usage_request(
    'f1000000-0000-4000-a000-000000000038',
    'f0000000-0000-4000-a000-000000000001',
    'ownership-cycle'
  ) as request
), data as (
  select request#>'{snapshot,data}' as value from source
), ownership as (
  select value, private.shop_guest_import_ownership_normalize(value) as normalized
  from data
), timeline as (
  select value, jsonb_build_object(
    'effect_cycle_bounds', normalized->'effect_cycle_bounds',
    'effect_history', normalized->'effect_history'
  ) as normalized_timeline
  from ownership
)
select ok(
  result is not null
    and result#>>'{lifetime_usage_tokens}' = '100'
    and result#>>'{current_cycle_usage_tokens}' = '100'
    and result#>>'{cycle_usage_totals,0,total_tokens}' = '100'
    and jsonb_array_length(result->'effect_contributions') = 2
    and jsonb_array_length(result->'activity_days') = 1
    and result#>>'{daily_growth,0,tokens}' = '100'
    and result#>>'{daily_growth,0,weighted_growth_bps}' = '0'
    and abs(
      (result#>>'{daily_growth,0,growth_credit}')::numeric
        - pg_catalog.ln(1 + 100::numeric / 100000) / pg_catalog.ln(2)
    ) < 0.000000000000000001::numeric,
  'usage normalization reconciles raw, journal, baseline and positive-effect segments'
)
from (
  select pg_temp.shop_guest_import_try_usage_normalize(value, normalized_timeline) as result
  from timeline
) checked;

select ok(pg_temp.shop_guest_import_try_usage_request(jsonb_set(
    pg_temp.shop_guest_import_usage_request(
      'f1000000-0000-4000-a000-000000000039',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    ), '{snapshot,data,daily_agent_totals,0,total_tokens}', '99'::jsonb
  )) is null,
  'daily usage mirror totals must exactly match raw date-agent aggregates');

select ok(pg_temp.shop_guest_import_try_usage_request(jsonb_set(
    pg_temp.shop_guest_import_usage_request(
      'f1000000-0000-4000-a000-000000000040',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    ), '{snapshot,data,daily_agent_totals}', '[]'::jsonb
  )) is null,
  'raw usage rows require a matching daily-agent mirror');

select ok(pg_temp.shop_guest_import_try_usage_request(jsonb_set(
    pg_temp.shop_guest_import_usage_request(
      'f1000000-0000-4000-a000-000000000041',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    ), '{snapshot,data,lifetime_usage_tokens}', '99'::jsonb
  )) is null,
  'lifetime usage total must equal the checked raw source sum');

select ok(pg_temp.shop_guest_import_try_usage_request(jsonb_set(
    pg_temp.shop_guest_import_usage_request(
      'f1000000-0000-4000-a000-000000000042',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    ), '{snapshot,data,current_cycle_usage_tokens}', '99'::jsonb
  )) is null,
  'current-cycle usage total must equal its checked raw source sum');

select ok(pg_temp.shop_guest_import_try_usage_request(jsonb_set(
    pg_temp.shop_guest_import_usage_request(
      'f1000000-0000-4000-a000-000000000043',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    ), '{snapshot,data,effect_contributions,1,wallet_bps}', '99'::jsonb
  )) is null,
  'contribution basis points must match the normalized effect interval');

select ok(pg_temp.shop_guest_import_try_usage_request(jsonb_set(
    pg_temp.shop_guest_import_usage_request(
      'f1000000-0000-4000-a000-000000000044',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    ), '{snapshot,data,contribution_canonical_version}', '2'::jsonb
  )) is null,
  'contribution and activity canonical versions must match the source version');

select ok(pg_temp.shop_guest_import_try_usage_request(jsonb_set(
    pg_temp.shop_guest_import_usage_request(
      'f1000000-0000-4000-a000-000000000045',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    ), '{snapshot,data,activity_days,0,tokens}', '99'::jsonb
  )) is null,
  'activity tokens must match the all-cycle reward-date source total');

select ok(pg_temp.shop_guest_import_try_usage_request(jsonb_set(
    pg_temp.shop_guest_import_usage_request(
      'f1000000-0000-4000-a000-000000000046',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    ), '{snapshot,data,growth_journal_entries,0,device_id}',
    to_jsonb('30000000-0000-4000-a000-000000000002'::text)
  )) is null,
  'journal rows from a foreign device cannot support this single-device source');

with source as (
  select pg_temp.shop_guest_import_usage_request(
    'f1000000-0000-4000-a000-000000000047',
    'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
  ) as request
)
select ok(pg_temp.shop_guest_import_try_usage_request(jsonb_set(
    request, '{snapshot,data,usage_aggregates}',
    (request#>'{snapshot,data,usage_aggregates}') || (request#>'{snapshot,data,usage_aggregates}')
  )) is null,
  'duplicate raw date-agent keys are rejected instead of summed')
from source;

select ok(pg_temp.shop_guest_import_try_usage_request(jsonb_set(
    pg_temp.shop_guest_import_usage_request(
      'f1000000-0000-4000-a000-000000000048',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    ), '{snapshot,data,planet_timezone}', to_jsonb('America/Los_Angeles'::text)
  )) is null,
  'different world, planet and reward timezones are held without rebucketing');

select ok(pg_temp.shop_guest_import_try_usage_request(jsonb_set(
    pg_temp.shop_guest_import_usage_request(
      'f1000000-0000-4000-a000-000000000049',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    ), '{snapshot,data,usage_aggregates,0,total_tokens}', '9223372036854775808'::jsonb
  )) is null,
  'usage source sums outside the signed bigint range are held');

select ok(pg_temp.shop_guest_import_try_usage_request(jsonb_set(
    pg_temp.shop_guest_import_usage_request(
      'f1000000-0000-4000-a000-000000000050',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    ), '{snapshot,data,cycle_usage_totals,0,total_tokens}', '99'::jsonb
  )) is null,
  'cycle usage totals must match the present journal entries');

select ok(pg_temp.shop_guest_import_try_usage_request(jsonb_set(
    pg_temp.shop_guest_import_usage_request(
      'f1000000-0000-4000-a000-000000000051',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    ), '{snapshot,data,growth_journal_entries,0,confirmed_tokens}', '99'::jsonb
  )) is null,
  'present journal totals must match the raw aggregate and cycle mapping');

select ok(pg_temp.shop_guest_import_try_usage_request(jsonb_set(
    pg_temp.shop_guest_import_usage_request(
      'f1000000-0000-4000-a000-000000000052',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    ), '{snapshot,data,usage_aggregates,0,coverage}', to_jsonb('partial'::text)
  )) is null,
  'partial coverage is independently held by usage normalization');

with source as (
  select pg_temp.shop_guest_import_usage_request(
    'f1000000-0000-4000-a000-000000000053',
    'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
  ) as request
), data as (
  select request#>'{snapshot,data}' as value from source
), normalized as (
  select value, private.shop_guest_import_ownership_normalize(value) as owned
  from data
), altered as (
  select value, jsonb_set(value, '{growth_journal_entries,0,cycle_id}',
      to_jsonb('different-cycle'::text)) as mismatched,
    jsonb_build_object('effect_cycle_bounds', owned->'effect_cycle_bounds',
      'effect_history', owned->'effect_history') as timeline
  from normalized
)
select ok(pg_temp.shop_guest_import_try_usage_normalize(mismatched, timeline) is null,
  'journal cycle mapping must match the raw aggregate cycle')
from altered;

select ok(pg_temp.shop_guest_import_try_usage_request(jsonb_set(
    pg_temp.shop_guest_import_usage_request(
      'f1000000-0000-4000-a000-000000000073',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    ), '{snapshot,data,effect_contributions,1,effect_revision}', '2'::jsonb
  )) is null,
  'contributions with an unknown effect revision are held');

select ok(pg_temp.shop_guest_import_try_usage_request(jsonb_set(
    pg_temp.shop_guest_import_usage_request(
      'f1000000-0000-4000-a000-000000000074',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    ), '{snapshot,data,effect_contributions}',
    jsonb_build_array(jsonb_build_object(
      'device_id', '30000000-0000-4000-a000-000000000001',
      'cycle_id', 'ownership-cycle', 'date', '2026-10-01', 'effect_revision', 1,
      'canonical_version', 1, 'tokens', 60, 'growth_bps', 0, 'wallet_bps', 100
    ))
  )) is null,
  'a missing contribution segment cannot cover present journal tokens');

with source as (
  select pg_temp.shop_guest_import_usage_request(
    'f1000000-0000-4000-a000-000000000075',
    'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
  ) as request
)
select ok(pg_temp.shop_guest_import_try_usage_request(jsonb_set(
    request, '{snapshot,data,effect_contributions}',
    (request#>'{snapshot,data,effect_contributions}') ||
      jsonb_build_array(request#>'{snapshot,data,effect_contributions,0}')
  )) is null,
  'duplicate contribution cycle-date-revision keys are rejected')
from source;

select ok(pg_temp.shop_guest_import_try_usage_request(jsonb_set(
    pg_temp.shop_guest_import_usage_request(
      'f1000000-0000-4000-a000-000000000076',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    ), '{snapshot,data,activity_days}', '[]'::jsonb
  )) is null,
  'positive contribution dates require an activity-day row');

select ok(pg_temp.shop_guest_import_try_usage_request(jsonb_set(
    pg_temp.shop_guest_import_usage_request(
      'f1000000-0000-4000-a000-000000000077',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    ), '{snapshot,data,activity_days,0,first_occurred_at_utc}',
    to_jsonb('2026-10-02T00:00:01+00:00'::text)
  )) is null,
  'activity first timestamp must map to its declared reward date');

select ok(pg_temp.shop_guest_import_try_usage_request(jsonb_set(
    jsonb_set(
      jsonb_set(
        pg_temp.shop_guest_import_usage_request(
          'f1000000-0000-4000-a000-000000000078',
          'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
        ), '{snapshot,data,effect_contributions,0,tokens}', '0'::jsonb
      ), '{snapshot,data,effect_contributions,1,tokens}', '100'::jsonb
    ), '{snapshot,data,activity_days,0,first_occurred_at_utc}',
    to_jsonb('2026-10-01T00:00:02+00:00'::text)
  )) is null,
  'activity first timestamp must lie in a contribution segment with positive tokens');

with source as (
  select pg_temp.shop_guest_import_usage_request(
    'f1000000-0000-4000-a000-000000000079',
    'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
  ) as request
), data as (
  select request#>'{snapshot,data}' as value from source
), ownership as (
  select value, private.shop_guest_import_ownership_normalize(value) as normalized
  from data
)
select ok(pg_temp.shop_guest_import_try_usage_normalize(
    value, jsonb_build_object(
      'effect_cycle_bounds', '[]'::jsonb,
      'effect_history', normalized->'effect_history'
    )
  ) is null,
  'contributions without their authoritative cycle bound are held')
from ownership;

select ok(pg_temp.shop_guest_import_try_usage_request(jsonb_set(
    pg_temp.shop_guest_import_usage_request(
      'f1000000-0000-4000-a000-000000000080',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    ), '{snapshot,data,activity_days,0,first_occurred_at_utc}',
    to_jsonb('2026-10-01T00:00:05+00:00'::text)
  )) is null,
  'activity first timestamp cannot exceed the captured effect timeline server time');

select ok(pg_temp.shop_guest_import_try_usage_request(jsonb_set(
    pg_temp.shop_guest_import_usage_request(
      'f1000000-0000-4000-a000-000000000081',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    ), '{snapshot,data,activity_days,0,first_occurred_at_utc}',
    to_jsonb('2026-10-01T00:00:04+00:00'::text)
  )) is null,
  'first activity cannot follow an earlier positive baseline segment on the same reward date');

select ok(pg_temp.shop_guest_import_try_usage_request(
    pg_temp.shop_guest_import_dated_usage_request(
      'f1000000-0000-4000-a000-000000000082',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle',
      '2026-03-08', 'America/New_York',
      '2026-03-08T05:00:00+00:00', '2026-03-09T04:00:00+00:00',
      '2026-03-09T05:00:00+00:00', '2026-03-08T05:00:01+00:00'
    )
  ) is null,
  'spring-forward local-day end is half-open at 2026-03-09 04:00 UTC');

with checked as (
  select pg_temp.shop_guest_import_try_usage_request(
    pg_temp.shop_guest_import_dated_usage_request(
      'f1000000-0000-4000-a000-000000000083',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle',
      '2026-11-01', 'America/New_York',
      '2026-11-01T04:00:00+00:00', '2026-11-02T04:00:00+00:00',
      '2026-11-02T05:00:00+00:00', '2026-11-01T04:00:01+00:00'
    )
  ) as result
)
select ok(
  result is not null
    and result#>>'{daily_growth,0,tokens}' = '100'
    and jsonb_array_length(result->'effect_contributions') = 2,
  'fall-back contribution in the 25th local-day hour remains inside Nov 1'
)
from checked;

with checked as (
  select pg_temp.shop_guest_import_try_usage_request(
    pg_temp.shop_guest_import_multicycle_usage_request(
      'f1000000-0000-4000-a000-000000000084',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    )
  ) as result
)
select ok(
  result is not null
    and result#>>'{lifetime_usage_tokens}' = '100'
    and result#>>'{current_cycle_usage_tokens}' = '60'
    and result#>>'{activity_days,0,cycle_id}' = 'history-cycle'
    and result#>>'{activity_days,0,tokens}' = '100'
    and jsonb_array_length(result->'daily_growth') = 2
    and exists (
      select 1 from jsonb_array_elements(result->'daily_growth') d(value)
      where d.value->>'cycle_id' = 'history-cycle' and d.value->>'tokens' = '40'
    )
    and exists (
      select 1 from jsonb_array_elements(result->'daily_growth') d(value)
      where d.value->>'cycle_id' = 'ownership-cycle' and d.value->>'tokens' = '60'
    ),
  'one reward date sums its first historical and current-cycle contributions'
)
from checked;

with source as (
  select pg_temp.shop_guest_import_multicycle_usage_request(
    'f1000000-0000-4000-a000-000000000085',
    'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
  ) as request
), claimed_current_first as (
  select jsonb_set(
    jsonb_set(request,
      '{snapshot,data,activity_days,0,cycle_id}', '"ownership-cycle"'::jsonb),
    '{snapshot,data,activity_days,0,first_occurred_at_utc}',
    '"2026-10-01T13:00:01+00:00"'::jsonb
  ) as request
  from source
)
select ok(pg_temp.shop_guest_import_try_usage_request(request) is null,
  'a current-cycle first claim cannot follow positive historical tokens for the same date')
from claimed_current_first;

select ok(pg_temp.shop_guest_import_try_usage_request(jsonb_set(
    pg_temp.shop_guest_import_multicycle_usage_request(
      'f1000000-0000-4000-a000-000000000086',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    ), '{snapshot,data,activity_days,0,tokens}', '99'::jsonb
  )) is null,
  'same-date activity cannot omit tokens from a historical or current cycle');

with checked as (
  select pg_temp.shop_guest_import_try_usage_request(
    pg_temp.shop_guest_import_growth_usage_request(
      'f1000000-0000-4000-a000-000000000087',
      'f0000000-0000-4000-a000-000000000001', 'ownership-cycle'
    )
  ) as result
)
select ok(
  result is not null
    and result#>>'{daily_growth,0,tokens}' = '100'
    and result#>>'{daily_growth,0,weighted_growth_bps}' = '6000'
    and abs(
      (result#>>'{daily_growth,0,growth_credit}')::numeric
        - pg_catalog.ln(1 + 100::numeric / 100000) / pg_catalog.ln(2) * 1.006::numeric
    ) < 0.000000000000000001::numeric,
  'catalog-derived 100-bps growth weights T=100 as W=6000 in the daily formula'
)
from checked;

select * from finish();
rollback;
