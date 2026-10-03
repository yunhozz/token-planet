begin;
create extension if not exists pgtap with schema extensions;
select no_plan();

insert into auth.users(id) values ('00000000-0000-0000-0000-000000000982');
insert into auth.users(id) values ('00000000-0000-0000-0000-000000000983');
insert into private.shop_device_contribution_state(
  user_id, device_id, canonical_version, canonical_payload
) values (
  '00000000-0000-0000-0000-000000000982',
  '30000000-0000-0000-0000-000000000982', 1, '{}'::jsonb
), (
  '00000000-0000-0000-0000-000000000983',
  '30000000-0000-0000-0000-000000000983', 1, '{}'::jsonb
);
insert into private.shop_effect_contribution(
  user_id, device_id, cycle_id, date, effect_revision, canonical_version,
  tokens, growth_bps, wallet_bps
)
select
  '00000000-0000-0000-0000-000000000982',
  '30000000-0000-0000-0000-000000000982', 'cycle-bonus',
  date '2026-01-01' + n - 1, 1, 1, 1, 0, 100
from generate_series(1, 100) as g(n);

-- Four fractional rows must accumulate before the single cycle-level floor.
insert into private.shop_effect_contribution(
  user_id, device_id, cycle_id, date, effect_revision, canonical_version,
  tokens, growth_bps, wallet_bps
)
select
  '00000000-0000-0000-0000-000000000983',
  '30000000-0000-0000-0000-000000000983', 'cycle-fraction',
  date '2026-03-01' + n - 1, 1, 1, 1, 0, 2500
from generate_series(1, 4) as g(n);

-- Values near bigint's limit exercise numeric aggregation and the checked cast.
insert into private.shop_effect_contribution(
  user_id, device_id, cycle_id, date, effect_revision, canonical_version,
  tokens, growth_bps, wallet_bps
)
select
  '00000000-0000-0000-0000-000000000983',
  '30000000-0000-0000-0000-000000000983', 'overflow-cycle',
  date '2026-04-01' + n - 1, 1, 1, 9223372036854775807, 0, 3000
from generate_series(1, 4) as g(n);

create function pg_temp.cycle_token_bonus(p_user_id uuid, p_cycle_id text)
returns bigint
language plpgsql as $$
declare
  v_bonus bigint;
begin
  execute 'select private.shop_cycle_token_bonus($1, $2)'
    into v_bonus using p_user_id, p_cycle_id;
  return v_bonus;
exception when undefined_function then
  return -1;
end;
$$;

select is(pg_temp.cycle_token_bonus(
  '00000000-0000-0000-0000-000000000982', 'cycle-bonus'), 1::bigint,
  'cycle token bonus floors the sum of one hundred server-confirmed one-token one-percent segments once');

select is(pg_temp.cycle_token_bonus(
  '00000000-0000-0000-0000-000000000982', 'missing-cycle'), 0::bigint,
  'an account with no matching cycle contributions receives zero bonus');
select is(pg_temp.cycle_token_bonus(
  '00000000-0000-0000-0000-000000000983', 'cycle-bonus'), 0::bigint,
  'another account cannot read a matching cycle identifier’s contributions');
select is(pg_temp.cycle_token_bonus(
  '00000000-0000-0000-0000-000000000982', 'cycle-fraction'), 0::bigint,
  'an account cannot read another account’s cycle contributions');
select is(pg_temp.cycle_token_bonus(
  '00000000-0000-0000-0000-000000000983', 'cycle-fraction'), 1::bigint,
  'four quarter-token contributions accumulate before the cycle-level floor');

select throws_ok($$select private.shop_cycle_token_bonus(
  '00000000-0000-0000-0000-000000000983', 'overflow-cycle')$$,
  '22003', null, 'a bonus above bigint range is rejected before casting');
select throws_ok($$select private.shop_cycle_token_bonus(
  null, 'cycle-bonus')$$,
  '22023', null, 'a null account scope is rejected');
select throws_ok($$select private.shop_cycle_token_bonus(
  '00000000-0000-0000-0000-000000000982', null)$$,
  '22023', null, 'a null cycle scope is rejected');

select ok(not has_function_privilege(
  'anon', 'private.shop_cycle_token_bonus(uuid,text)', 'EXECUTE'),
  'anonymous clients cannot execute the private cycle bonus helper');
select ok(not has_function_privilege(
  'authenticated', 'private.shop_cycle_token_bonus(uuid,text)', 'EXECUTE'),
  'authenticated clients cannot execute the private cycle bonus helper');
select ok(not has_function_privilege(
  'service_role', 'private.shop_cycle_token_bonus(uuid,text)', 'EXECUTE'),
  'service role cannot execute the private cycle bonus helper directly');

select * from finish();
rollback;
