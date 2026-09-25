begin;
create extension if not exists pgtap with schema extensions;
select plan(4);

select has_table('public', 'worlds', 'worlds table exists');
select has_table('public', 'world_members', 'memberships table exists');
select ok(coalesce((select relrowsecurity from pg_class where oid = to_regclass('public.worlds')), false), 'worlds has RLS');
select ok(coalesce((select relrowsecurity from pg_class where oid = to_regclass('public.world_members')), false), 'memberships have RLS');

select * from finish();
rollback;
