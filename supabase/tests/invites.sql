begin;
create extension if not exists pgtap with schema extensions;
select plan(1);

select has_table('public', 'world_invites', 'historical invite metadata table remains');

select * from finish();
rollback;
