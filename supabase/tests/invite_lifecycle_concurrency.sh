#!/usr/bin/env bash
# Source-only in the current handoff. Execute only after separate DB approval.
set -Eeuo pipefail
umask 077
if [[ $# != 4 || "$1" != --workdir || "$3" != --container ]]; then
  printf 'Usage: %s --workdir <approved-workdir> --container <approved-container>\n' "$0" >&2
  exit 2
fi
python3 - "$2" "$4" <<'PY'
import json, os, pathlib, select, subprocess, sys, time, uuid

workdir, container = sys.argv[1:]
expected = 'token-planet-multiplayer-qa-20261008'
root = pathlib.Path('/tmp/token-planet-multiplayer-qa-20261008')

def fail(message):
    raise RuntimeError(message)

def is_approved_workdir(workdir, approved_root):
    return pathlib.Path(workdir).resolve() == approved_root.resolve()

def docker(args):
    result = subprocess.run(['docker', *args], capture_output=True, text=True)
    if result.returncode: fail('Docker identity operation failed; details withheld')
    return result.stdout

# Fail closed before any SQL. Never infer another project or endpoint.
if not is_approved_workdir(workdir, root) or container != 'supabase_db_' + expected:
    fail('approved workdir/container required')
if any(os.environ.get(key) for key in ('DOCKER_HOST','DOCKER_CONTEXT','DATABASE_URL','PGHOST','PGPORT','PGPASSWORD','SUPABASE_ACCESS_TOKEN')):
    fail('connection overrides are not allowed')
config = (root/'supabase/config.toml').read_text()
import re
if not re.search(r'^project_id\s*=\s*"'+expected+r'"\s*$',config,re.M): fail('project identity mismatch')
context = json.loads(docker(['context','inspect']))[0]
endpoint = context.get('Endpoints',{}).get('docker',{}).get('Host','')
if not endpoint.startswith('unix://'): fail('local Docker socket required')
info = json.loads(docker(['container','inspect',container]))[0]
labels = info.get('Config',{}).get('Labels',{})
if labels.get('com.supabase.cli.project') != expected or labels.get('com.supabase.cli.workdir') != str(root):
    fail('container ownership mismatch')
if not info.get('State',{}).get('Running'): fail('approved database is not running')
ports = info.get('NetworkSettings',{}).get('Ports',{})
if any(binding.get('HostIp') != '127.0.0.1' for entries in ports.values() for binding in entries or []):
    fail('database must publish on loopback only; no SQL executed')
if not any(b.get('HostIp')=='127.0.0.1' and b.get('HostPort')=='56322' for b in ports.get('5432/tcp') or []):
    fail('approved loopback port required')
volume = 'supabase_db_' + expected
if not any(m.get('Type')=='volume' and m.get('Name')==volume and m.get('Destination')=='/var/lib/postgresql/data' for m in info.get('Mounts',[])):
    fail('database volume mismatch')
v = json.loads(docker(['volume','inspect',volume]))[0]
if v.get('Labels',{}).get('com.supabase.cli.project')!=expected: fail('volume ownership mismatch')
container_id = info['Id']
run = 'invite_' + uuid.uuid4().hex
users, worlds, processes = [], [], []

def connect():
    # Recheck immutable ID; do not fall back to a name if a container is replaced.
    current = json.loads(docker(['container','inspect',container]))[0]
    if current['Id'] != container_id: fail('container changed during test')
    p = subprocess.Popen(['docker','exec','-i','-e','PGAPPNAME='+run,container_id,'psql','-X','-qAt','-v','ON_ERROR_STOP=1','-U','postgres','-d','postgres'],
        stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
    processes.append(p)
    return p

def query(sql):
    p = connect()
    out, _ = p.communicate(sql,timeout=20)
    if p.returncode: fail('SQL operation failed; secret-bearing diagnostics withheld')
    return out.strip()

def user():
    value = str(uuid.uuid4()); users.append(value)
    query("insert into auth.users(id) values ('"+value+"');")
    return value

def world(owner):
    value=str(uuid.uuid4()); worlds.append(value)
    query("insert into public.worlds(id,owner_id,name,timezone) values ('"+value+"','"+owner+"','"+run+"','UTC');")
    return value

def auth(value):
    return "set request.jwt.claim.sub='"+value+"';"

def invite(owner,w):
    value=json.loads(query(auth(owner)+"select row_to_json(i) from public.create_world_invite('"+w+"') i;"))
    return value['invite_id'], value['code']

def accept(u,code):
    return auth(u)+"select status from public.accept_world_invite('"+code+"');"

def revoke(u,i):
    return auth(u)+"select status from public.revoke_world_invite('"+i+"');"

def holder(w):
    p=connect(); p.stdin.write("begin; select 1 from public.worlds where id='"+w+"' for update;\n\\echo LOCK_HELD\n");p.stdin.flush()
    deadline=time.monotonic()+10
    received=b''
    while time.monotonic()<deadline:
        if select.select([p.stdout],[],[],0.2)[0]:
            received+=os.read(p.stdout.fileno(),4096)
            if b'LOCK_HELD\n' in received: return p
    fail('barrier could not acquire world lock')

def start(sql,label):
    p=connect(); p.stdin.write("set application_name='"+run+'_'+label+"';"+sql+'\n');p.stdin.close();p.stdin=None
    return p

def blocked(label):
    deadline=time.monotonic()+10
    while time.monotonic()<deadline:
        if query("select count(*) from pg_stat_activity where application_name='"+run+'_'+label+"' and wait_event_type='Lock';")=='1': return
        time.sleep(0.02) # Poll interval only; pg_stat_activity establishes the barrier.
    fail('waiter did not reach a database lock barrier')

def release(p):
    p.stdin.write('commit;\n\\q\n');p.stdin.flush();p.stdin.close();p.stdin=None
    p.communicate(timeout=10)
    if p.returncode: fail('barrier release failed')

def result(p):
    out,_=p.communicate(timeout=20)
    if p.returncode: fail('concurrent SQL failed; details withheld')
    return out.strip()

def race(w,sqls):
    gate=holder(w)
    jobs=[start(sql,str(n)) for n,sql in enumerate(sqls)]
    for n in range(len(jobs)): blocked(str(n))
    release(gate)
    return [result(p) for p in jobs]

try:
    if query("select exists(select 1 from information_schema.columns where table_schema='public' and table_name='world_members' and column_name='joined_via_invite_id');")!='t':
        fail('invite migration must already be applied by an approved operator')
    owner=user(); a=user(); b=user(); w=world(owner); i,c=invite(owner,w)
    assert sorted(race(w,[accept(a,c),accept(b,c)]))==['accepted','unavailable']
    winner=a if query("select used_by from public.world_invites where id='"+i+"';")==a else b
    assert query(accept(winner,c))=='already_accepted'
    assert query("select count(*) from public.world_members where world_id='"+w+"';")=='2'
    print('PASS same-code competition and retry')

    owner2=user(); w2=world(owner2)
    for _ in range(8): query("insert into public.world_members(world_id,user_id,role) values ('"+w2+"','"+user()+"','member');")
    i1,c1=invite(owner2,w2); i2,c2=invite(owner2,w2)
    assert sorted(race(w2,[accept(user(),c1),accept(user(),c2)]))==['accepted','world_full']
    assert query("select count(*) from public.world_members where world_id='"+w2+"';")=='10'
    print('PASS last-slot competition')

    for operation in ('revoke','transfer'):
        o=user(); w3=world(o); n=user()
        query("insert into public.world_members(world_id,user_id,role) values ('"+w3+"','"+n+"','member');")
        ix,cx=invite(o,w3)
        other=revoke(o,ix) if operation=='revoke' else auth(o)+"select public.transfer_world_owner('"+w3+"','"+n+"');"
        statuses=race(w3,[accept(user(),cx),other])
        assert statuses[0] in ('accepted','unavailable')
        assert statuses[1] in (('revoked','used') if operation=='revoke' else ('t',))
        state=query("select (used_at is not null)::int+(revoked_at is not null)::int from public.world_invites where id='"+ix+"';")
        assert state=='1'
        print('PASS accept versus '+operation)

    o=user(); w4=world(o); ix,cx=invite(o,w4); gate=holder(w4)
    waiter=start(accept(user(),cx),'expiry'); blocked('expiry')
    query("update public.world_invites set expires_at=clock_timestamp() where id='"+ix+"';")
    release(gate); assert result(waiter)=='unavailable'
    print('PASS expiry during world-lock wait')

    o1=user(); o2=user(); wx=world(o1); wy=world(o2); _,cx=invite(o1,wx); _,cy=invite(o2,wy); u=user()
    gate=holder(wx); first=start(accept(u,cx),'same-user-1'); blocked('same-user-1')
    second=start(accept(u,cy),'same-user-2'); blocked('same-user-2')
    release(gate); assert result(first)=='accepted'; assert result(second)=='already_member'
    print('PASS same-user different-world competition')
    limited=user()
    for _ in range(5): assert query(accept(limited,'bad'))=='unavailable'
    assert query(accept(limited,'bad'))=='rate_limited'
    assert query("select failed_attempts from private.member_code_join_attempts where user_id='"+limited+"';")=='5'
    print('PASS independently committed rate-limit counter')
finally:
    # Close only sessions bearing this unpredictable run identifier; never other apps.
    query("select pg_terminate_backend(pid) from pg_stat_activity where left(application_name,"+str(len(run))+")='"+run+"' and pid<>pg_backend_pid();")
    for p in processes:
        if p.poll() is None:
            p.kill();p.communicate()
    # Exact generated UUIDs only. No reset, stop, truncate, or foreign fixture cleanup.
    if worlds: query("delete from public.worlds where id in ("+','.join("'"+x+"'" for x in worlds)+") and name='"+run+"';")
    if users: query("delete from auth.users where id in ("+','.join("'"+x+"'" for x in users)+");")
PY
