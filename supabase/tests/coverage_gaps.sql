begin;
create extension if not exists pgtap with schema extensions;
select plan(4);
\ir fixtures/planet.inc
insert into auth.users(id) values ('00000000-0000-0000-0000-000000000901');
insert into public.worlds(id, owner_id, name, timezone) values
  ('20000000-0000-0000-0000-000000000901', '00000000-0000-0000-0000-000000000901', 'coverage', 'Asia/Seoul');
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000901', true);
do $$ begin
  perform public.upsert_my_planet_state(pg_temp.planet_state('Coverage'),
    pg_temp.planet_device('30000000-0000-0000-0000-000000000901', 'cycle-1', 100));
  perform public.upsert_my_planet_state(pg_temp.planet_state('Coverage'),
    pg_temp.planet_device('30000000-0000-0000-0000-000000000902', 'cycle-1', 200, true));
end $$;
select is((public.get_my_planet_state()->>'incomplete')::boolean, true, 'an incomplete device keeps the account incomplete');
select is((select incomplete from public.get_world_planets('20000000-0000-0000-0000-000000000901')), true, 'group members see the coverage gap');
do $$ begin
  perform public.upsert_my_planet_state(pg_temp.planet_state('Coverage'),
    pg_temp.planet_device('30000000-0000-0000-0000-000000000902', 'cycle-1', 200, false));
end $$;
select is((public.get_my_planet_state()->>'incomplete')::boolean, false, 'a resolved device gap clears the account coverage warning');
select is((select incomplete from public.get_world_planets('20000000-0000-0000-0000-000000000901')), false, 'resolved coverage is reflected in the group');
select * from finish();
rollback;
