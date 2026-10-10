#!/usr/bin/env bash
set -Eeuo pipefail
umask 077
python3 - "$@" <<'PY'
import json, os, pathlib, re, select, subprocess, sys, time, uuid

def fail(message): raise RuntimeError(message)

def validate_target(workdir, config, context, info):
    root=pathlib.Path('/tmp/token-planet-group-chat-supabase')
    project='token-planet-group-chat-qa'
    if pathlib.Path(workdir).resolve()!=root.resolve() or not re.search(r'^project_id\s*=\s*"'+project+r'"\s*$',config,re.M): fail('approved scratch target required')
    if not context.get('Endpoints',{}).get('docker',{}).get('Host','').startswith('unix:///'): fail('local Docker required')
    labels=info.get('Config',{}).get('Labels') or {}
    if info.get('Name')!='/supabase_db_'+project or not info.get('State',{}).get('Running') or labels.get('com.supabase.cli.project')!=project or pathlib.Path(labels.get('com.supabase.cli.workdir','/')).resolve()!=root.resolve() or not re.fullmatch('[0-9a-f]{64}',info.get('Id','')): fail('container identity mismatch')
    ports=info.get('NetworkSettings',{}).get('Ports',{}).get('5432/tcp')
    if not ports or any(p.get('HostPort')!='54322' or p.get('HostIp') not in ('127.0.0.1','0.0.0.0','::') for p in ports): fail('database port mismatch')
    return info['Id']
def docker(args):
    result = subprocess.run(['docker', *args], capture_output=True, text=True, timeout=20)
    if result.returncode: fail('Docker identity operation failed; details withheld')
    return result.stdout

root=pathlib.Path('/tmp/token-planet-group-chat-supabase')
container='supabase_db_token-planet-group-chat-qa'
if len(sys.argv)!=1: fail('this harness accepts no target overrides')
if any(os.environ.get(key) for key in ('DOCKER_HOST','DOCKER_CONTEXT','DOCKER_TLS_VERIFY','DOCKER_CERT_PATH','DATABASE_URL','PGHOST','PGHOSTADDR','PGPORT','PGDATABASE','PGUSER','PGPASSWORD','PGSERVICE','PGSERVICEFILE','SUPABASE_ACCESS_TOKEN')):
    fail('connection overrides forbidden')
container_id=validate_target(str(root),(root/'supabase/config.toml').read_text(),json.loads(docker(['context','inspect']))[0],json.loads(docker(['inspect',container]))[0])
run = 'chat_' + uuid.uuid4().hex
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
    o=user(); a=user(); w=world(o); request=str(uuid.uuid4())
    send=lambda u,r,b: "set role authenticated;"+auth(u)+"select public.send_group_chat_message('"+w+"','"+r+"','"+b+"')->>'message_seq';"
    results=race(w,[send(o,request,'same'),send(o,request,'same')])
    assert results==['1','1']
    assert query("select count(*) from public.group_chat_messages where world_id='"+w+"';")=='1'
    print('PASS identical concurrent requests save exactly one message')
    join="insert into public.world_members(world_id,user_id,role) values ('"+w+"','"+a+"','member'); select joined_after_seq from public.world_members where user_id='"+a+"';"
    results=race(w,[join,send(o,str(uuid.uuid4()),'join race')])
    cutoff=int(results[0]); assert cutoff in (1,2)
    visible=query("set role authenticated;"+auth(a)+"select count(*) from public.group_chat_messages where world_id='"+w+"';")
    assert int(visible)==2-cutoff
    print('PASS join versus send respects the serialized cutoff')
    query(send(a,str(uuid.uuid4()),'member message'))
    leave="set role authenticated;"+auth(a)+"select public.leave_world('"+w+"');"
    gate=holder(w); exiting=start(leave,'leave'); blocked('leave')
    sending=start(send(a,str(uuid.uuid4()),'late'),'send'); blocked('send')
    release(gate); assert result(exiting)=='t'
    _,_=sending.communicate(timeout=20); assert sending.returncode!=0
    assert query("select count(*) from public.group_chat_messages where world_id='"+w+"';")=='3'
    print('PASS send after concurrent leave is denied and old messages survive')
finally:
    query("select pg_terminate_backend(pid) from pg_stat_activity where left(application_name,"+str(len(run))+")='"+run+"' and pid<>pg_backend_pid();")
    for p in processes:
        if p.poll() is None: p.kill();p.communicate()
    if worlds: query("delete from public.worlds where id in ("+','.join("'"+x+"'" for x in worlds)+") and name='"+run+"';")
    if users: query("delete from auth.users where id in ("+','.join("'"+x+"'" for x in users)+");")
PY
