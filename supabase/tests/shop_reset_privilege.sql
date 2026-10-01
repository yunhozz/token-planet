begin;
create extension if not exists pgtap with schema extensions;
select plan(4);

select ok(has_schema_privilege('authenticated', 'private', 'USAGE'),
  'authenticated can resolve explicitly granted private functions');
select ok(not has_function_privilege(
    'authenticated', 'private.upsert_my_planet_state_before_reward_delivery(jsonb,jsonb)', 'EXECUTE'
  ), 'authenticated cannot call the pre-reset reward upload path');
select ok(not has_function_privilege(
    'authenticated', 'private.upsert_my_planet_state_legacy(jsonb,jsonb)', 'EXECUTE'
  ), 'authenticated cannot call the raw legacy upload path');
select ok(has_function_privilege(
    'authenticated', 'public.upsert_my_planet_state(jsonb,jsonb)', 'EXECUTE'
  ), 'authenticated can call the guarded public upload RPC');

select * from finish();
rollback;
