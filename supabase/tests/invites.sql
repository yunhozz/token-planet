begin;
create extension if not exists pgtap with schema extensions;
select plan(20);

select has_table('public', 'world_invites', 'invite metadata table exists');
select ok(to_regprocedure('public.create_world_invite(uuid)') is not null, 'owner can create invite through API');
select ok(to_regprocedure('public.accept_world_invite(text)') is not null, 'invite acceptance API exists');
select ok(to_regprocedure('public.revoke_world_invite(uuid)') is not null, 'invite revocation API exists');

insert into auth.users(id) values
  ('00000000-0000-0000-0000-000000000201'),
  ('00000000-0000-0000-0000-000000000202');
insert into public.worlds(id, owner_id, name, timezone)
values ('40000000-0000-0000-0000-000000000001', '00000000-0000-0000-0000-000000000201', 'Invites', 'Asia/Seoul');
create temporary table captured_invites (invite_id uuid, code text, expires_at timestamptz);
grant insert, select on captured_invites to authenticated;
create temporary table expired_invite (invite_id uuid, code text, expires_at timestamptz);
create temporary table revoked_invite (invite_id uuid, code text, expires_at timestamptz);
create temporary table remaining_invite (invite_id uuid, code text, expires_at timestamptz);
create temporary table full_invite (invite_id uuid, code text, expires_at timestamptz);
grant insert, select on expired_invite, revoked_invite, remaining_invite, full_invite to authenticated;

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000201', true);
select lives_ok(
  $$insert into captured_invites select * from public.create_world_invite('40000000-0000-0000-0000-000000000001')$$,
  'owner can create an invitation'
);
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000202', true);
select throws_ok(
  $$select * from public.create_world_invite('40000000-0000-0000-0000-000000000001')$$,
  '42501', null, 'non-owner cannot create an invitation'
);
reset role;
select is(length(code), 64, 'invite code has 256 bits of hex entropy') from captured_invites;
select is(
  (select code_hash from public.world_invites where id = c.invite_id),
  encode(extensions.digest(convert_to(c.code, 'UTF8'), 'sha256'), 'hex'),
  'database stores the code hash'
) from captured_invites c;
select is(
  (select expires_at from public.world_invites where id = c.invite_id),
  c.expires_at,
  'returned expiry matches stored expiry'
) from captured_invites c;
select ok(expires_at between now() + interval '6 days 23 hours' and now() + interval '7 days 1 hour',
  'invite expires in seven days') from captured_invites;

set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000202', true);
select lives_ok(
  (select format('select * from public.accept_world_invite(%L)', code) from captured_invites),
  'valid code admits a new member'
);
select throws_ok(
  (select format('select * from public.accept_world_invite(%L)', code) from captured_invites),
  '23514', null, 'used code cannot be replayed'
);
reset role;
select is((select count(*)::integer from public.world_members where world_id = '40000000-0000-0000-0000-000000000001'), 2,
  'accepted member is added once');

insert into auth.users(id) values
  ('00000000-0000-0000-0000-000000000203'),
  ('00000000-0000-0000-0000-000000000204'),
  ('00000000-0000-0000-0000-000000000205'),
  ('00000000-0000-0000-0000-000000000206');
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000201', true);
insert into expired_invite select * from public.create_world_invite('40000000-0000-0000-0000-000000000001');
insert into revoked_invite select * from public.create_world_invite('40000000-0000-0000-0000-000000000001');
insert into remaining_invite select * from public.create_world_invite('40000000-0000-0000-0000-000000000001');
reset role;
update public.world_invites set expires_at = now() - interval '1 second'
where id = (select invite_id from expired_invite);
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000203', true);
select throws_ok(
  (select format('select * from public.accept_world_invite(%L)', code) from expired_invite),
  '23514', null, 'expired code is rejected'
);
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000201', true);
select ok(public.revoke_world_invite((select invite_id from revoked_invite)),
  'owner can revoke an unused code');
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000204', true);
select throws_ok(
  (select format('select * from public.accept_world_invite(%L)', code) from revoked_invite),
  '23514', null, 'revoked code is rejected'
);
select throws_ok(
  $$select * from public.accept_world_invite('unknown-code')$$,
  '23514', null, 'unknown code is rejected'
);
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000202', true);
select throws_ok(
  (select format('select public.revoke_world_invite(%L)', invite_id) from remaining_invite),
  '42501', null, 'non-owner cannot revoke a code'
);

reset role;
insert into auth.users(id)
select ('00000000-0000-0000-0000-' || lpad(n::text, 12, '0'))::uuid
from generate_series(210, 219) n;
insert into public.worlds(id, owner_id, name, timezone)
values ('40000000-0000-0000-0000-000000000002', '00000000-0000-0000-0000-000000000210', 'Full', 'Asia/Seoul');
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000210', true);
insert into full_invite select * from public.create_world_invite('40000000-0000-0000-0000-000000000002');
reset role;
insert into public.world_members(world_id, user_id, role)
select '40000000-0000-0000-0000-000000000002',
  ('00000000-0000-0000-0000-' || lpad(n::text, 12, '0'))::uuid, 'member'
from generate_series(211, 219) n;
set local role authenticated;
select set_config('request.jwt.claim.sub', '00000000-0000-0000-0000-000000000205', true);
select throws_ok(
  (select format('select * from public.accept_world_invite(%L)', code) from full_invite),
  '23514', null, 'full world rejects an invite'
);
reset role;
select is((select count(*)::integer from public.world_members where world_id = '40000000-0000-0000-0000-000000000002'), 10,
  'full world retains exactly ten members');

select * from finish();
rollback;
