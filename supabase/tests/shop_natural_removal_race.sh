#!/usr/bin/env bash
set -euo pipefail
set +x

readonly TEST_PROJECT_ID='token-planet-shop-revamp-test'
readonly TEST_WORKDIR='/private/tmp/token-planet-shop-revamp-test'
readonly TEST_CONFIG="$TEST_WORKDIR/supabase/config.toml"
readonly TEST_DB_PORT='55432'
readonly TEST_DB_CONTAINER="supabase_db_$TEST_PROJECT_ID"
readonly TEST_DB_VOLUME="supabase_db_$TEST_PROJECT_ID"

fail() {
  printf 'natural removal race: %s\n' "$1" >&2
  exit 1
}

command -v docker >/dev/null 2>&1 || fail 'docker is unavailable'
command -v python3 >/dev/null 2>&1 || fail 'python3 is unavailable'
command -v awk >/dev/null 2>&1 || fail 'awk is unavailable'
command -v rg >/dev/null 2>&1 || fail 'rg is unavailable'
command -v tr >/dev/null 2>&1 || fail 'tr is unavailable'
command -v tail >/dev/null 2>&1 || fail 'tail is unavailable'
[[ -f "$TEST_CONFIG" ]] || fail 'the pinned disposable project config is missing'

docker_context="$(docker context show 2>/dev/null)" \
  || fail 'could not identify the Docker context'
docker_endpoint="$(docker context inspect "$docker_context" \
  --format '{{(index .Endpoints "docker").Host}}' 2>/dev/null)" \
  || fail 'could not verify the local Docker endpoint'
[[ "$docker_endpoint" == unix://* ]] || fail 'refusing a non-local Docker endpoint'

python3 - "$TEST_CONFIG" "$TEST_PROJECT_ID" "$TEST_DB_PORT" <<'PY'
import sys
import tomllib
from pathlib import Path

config_path, expected_project, expected_port = sys.argv[1:]
config = tomllib.loads(Path(config_path).read_text())
if config.get("project_id") != expected_project:
    raise SystemExit("natural removal race: disposable project ID does not match")
if str(config.get("db", {}).get("port")) != expected_port:
    raise SystemExit("natural removal race: disposable project database port does not match")
PY

container_id="$(docker inspect --format '{{.Id}}' "$TEST_DB_CONTAINER" 2>/dev/null)" \
  || fail 'the pinned disposable database container is unavailable'
[[ -n "$container_id" ]] || fail 'the pinned disposable database container ID is empty'
container_running="$(docker inspect --format '{{.State.Running}}' "$TEST_DB_CONTAINER")"
[[ "$container_running" == 'true' ]] || fail 'the pinned disposable database container is not running'
container_mounts="$(docker inspect --format '{{json .Mounts}}' "$TEST_DB_CONTAINER")" \
  || fail 'could not inspect the pinned database volume'
container_ports="$(docker inspect --format '{{json .NetworkSettings.Ports}}' "$TEST_DB_CONTAINER")" \
  || fail 'could not inspect the pinned database port'
python3 - "$container_mounts" "$container_ports" "$TEST_DB_VOLUME" "$TEST_DB_PORT" <<'PY'
import json
import sys

mounts = json.loads(sys.argv[1])
ports = json.loads(sys.argv[2])
expected_volume, expected_port = sys.argv[3:]
db_mounts = [mount for mount in mounts if mount.get("Destination") == "/var/lib/postgresql/data"]
if len(db_mounts) != 1 or db_mounts[0].get("Type") != "volume" or db_mounts[0].get("Name") != expected_volume:
    raise SystemExit("natural removal race: pinned database volume does not match")
bindings = ports.get("5432/tcp") or []
if not any(binding.get("HostPort") == expected_port for binding in bindings):
    raise SystemExit("natural removal race: pinned container is not bound to the expected DB port")
PY

psql_test() {
  local application_name="$1"
  shift
  docker exec --env "PGAPPNAME=$application_name" -i "$TEST_DB_CONTAINER" \
    psql -X -q -A -t "$@" -U postgres -d postgres
}

server_identity="$(psql_test natural-removal-preflight \
  -c "select current_database() || '|' || current_user || '|' || current_setting('port')")" \
  || fail 'could not connect to the pinned disposable database'
[[ "$server_identity" == 'postgres|postgres|5432' ]] \
  || fail 'connected database identity does not match the expected local Supabase database'

safe_target='yes'
race_user_id="$(python3 -c 'import uuid; print(uuid.uuid4())')"
race_device_id="$(python3 -c 'import uuid; print(uuid.uuid4())')"
race_run_id="$(python3 -c 'import uuid; print(uuid.uuid4())')"
race_cycle_id="natural-removal-race-$race_run_id"
race_credit_cycle_id="$race_cycle_id-credit"
request_a="$(python3 -c 'import uuid; print(uuid.uuid4())')"
request_b="$(python3 -c 'import uuid; print(uuid.uuid4())')"
blocker_app="natural-removal-blocker-$race_run_id"
racer_a_app="natural-removal-racer-a-$race_run_id"
racer_b_app="natural-removal-racer-b-$race_run_id"
run_dir=''
blocker_client_pid=''
racer_a_pid=''
racer_b_pid=''
blocker_backend_pid=''

wait_client() {
  local client_pid="$1"
  local label="$2"
  local state=''
  for _ in {1..300}; do
    state="$(ps -o stat= -p "$client_pid" 2>/dev/null | tr -d '[:space:]')"
    if [[ -z "$state" || "$state" == Z* ]]; then
      wait "$client_pid"
      return $?
    fi
    sleep 0.1
  done
  printf 'natural removal race: timed out waiting for %s\n' "$label" >&2
  return 1
}

report_racer_output() {
  local label="$1"
  local output_path="$2"
  if [[ -s "$output_path" ]]; then
    printf 'Natural removal %s client output:\n' "$label" >&2
    tail -n 40 "$output_path" >&2
  else
    printf 'Natural removal %s client output: empty or not created\n' "$label" >&2
  fi
}

report_racer_diagnostics() {
  report_racer_output 'racer A' "$run_dir/a.status"
  report_racer_output 'racer B' "$run_dir/b.status"
}

cleanup() {
  local exit_code=$?
  local cleanup_output=''
  local cleanup_summary=''
  local cleanup_failed='no'
  trap - EXIT INT TERM

  if [[ "${safe_target:-no}" == 'yes' ]]; then
    exec 3>&- 2>/dev/null || true
    cleanup_output="$(psql_test natural-removal-cleanup \
      -v ON_ERROR_STOP=1 \
      -v user_id="$race_user_id" \
      -v blocker_app="$blocker_app" \
      -v racer_a_app="$racer_a_app" \
      -v racer_b_app="$racer_b_app" <<'SQL'
select pg_terminate_backend(pid)
from pg_stat_activity
where application_name = any(array[:'blocker_app', :'racer_a_app', :'racer_b_app']::text[])
  and pid <> pg_backend_pid();
delete from auth.users where id = :'user_id'::uuid;
select (select count(*)::text from pg_stat_activity
  where application_name = any(array[:'blocker_app', :'racer_a_app', :'racer_b_app']::text[]))
  || '|' || (select count(*)::text from auth.users where id = :'user_id'::uuid);
SQL
    )" || cleanup_failed='yes'

    for client_pid in "${blocker_client_pid:-}" "${racer_a_pid:-}" "${racer_b_pid:-}"; do
      if [[ -n "$client_pid" ]] && kill -0 "$client_pid" 2>/dev/null; then
        kill "$client_pid" >/dev/null 2>&1 || true
      fi
    done
    for client_pid in "${blocker_client_pid:-}" "${racer_a_pid:-}" "${racer_b_pid:-}"; do
      if [[ -n "$client_pid" ]]; then
        wait "$client_pid" >/dev/null 2>&1 || true
      fi
    done

    cleanup_summary="$(printf '%s\n' "$cleanup_output" | awk 'NF {last = $0} END {print last}')"
    if [[ "$cleanup_failed" != 'no' || "$cleanup_summary" != '0|0' ]]; then
      printf 'natural removal race: cleanup could not verify zero sessions and zero test users (%s)\n' \
        "${cleanup_summary:-no result}" >&2
      exit_code=1
    fi
  fi

  if [[ -n "${run_dir:-}" && -d "$run_dir" ]]; then
    rm -rf -- "$run_dir"
  fi
  exit "$exit_code"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

run_dir="$(mktemp -d "${TMPDIR:-/tmp}/natural-removal-race.XXXXXXXX")" \
  || fail 'could not create a private result directory'

psql_test natural-removal-fixture \
  -v ON_ERROR_STOP=1 \
  -v user_id="$race_user_id" \
  -v device_id="$race_device_id" \
  -v cycle_id="$race_cycle_id" \
  -v credit_cycle_id="$race_credit_cycle_id" \
  >/dev/null <<'SQL'
insert into auth.users(id) values (:'user_id'::uuid);
set role authenticated;
select set_config('request.jwt.claim.sub', :'user_id', false) \gset
select public.upsert_my_planet_state(
  jsonb_build_object(
    'version', 1,
    'profile', jsonb_build_object('nickname', 'natural-removal-race', 'avatar', 'feminine'),
    'timezone', 'UTC',
    'current_cycle_id', :'cycle_id',
    'cycle_started_at_utc', '2026-09-30T00:00:00Z',
    'last_reset_at_utc', null,
    'wallet_balance', 0,
    'wallet_credits', '[]'::jsonb,
    'current_planet_tokens', 0,
    'lifetime_tokens', 0,
    'growth_credit', 0,
    'stage', 0,
    'progress_to_next', 0,
    'incomplete', false,
    'can_reset', true,
    'reset_available_at_utc', null,
    'objects', '[]'::jsonb
  ),
  jsonb_build_object(
    'device_id', :'device_id'::uuid,
    'current_cycle_id', :'cycle_id',
    'current_planet_tokens', 0,
    'lifetime_tokens', 0,
    'daily_tokens', '{}'::jsonb,
    'incomplete', false,
    'canonical_version', 0,
    'daily_segments', '[]'::jsonb,
    'activity_days', '[]'::jsonb
  )
);
with tick as (
  select clock_timestamp() as occurred_at
), keyed_tick as (
  select occurred_at,
    to_char(occurred_at at time zone 'UTC', 'YYYY-MM-DD') as occurred_day
  from tick
)
select public.upsert_my_planet_state(
  public.get_my_planet_state(),
  jsonb_build_object(
    'device_id', :'device_id'::uuid,
    'current_cycle_id', :'cycle_id',
    'current_planet_tokens', 100000,
    'lifetime_tokens', 100000,
    'daily_tokens', jsonb_build_object(keyed_tick.occurred_day, 100000),
    'incomplete', false,
    'canonical_version', 1,
    'daily_segments', jsonb_build_array(jsonb_build_object(
      'cycle_id', :'cycle_id', 'date', keyed_tick.occurred_day,
      'effect_revision', 0, 'tokens', 100000
    )),
    'activity_days', jsonb_build_array(jsonb_build_object(
      'cycle_id', :'cycle_id', 'reward_date', keyed_tick.occurred_day,
      'first_occurred_at_utc', keyed_tick.occurred_at, 'tokens', 100000
    ))
  )
)
from keyed_tick;
reset role;
insert into private.planet_wallet_credits(user_id, previous_cycle_id, amount, created_at)
values (:'user_id'::uuid, :'credit_cycle_id', 1000000, clock_timestamp());
SQL

fixture="$(psql_test natural-removal-quote \
  -v ON_ERROR_STOP=1 -v user_id="$race_user_id" <<'SQL'
set role authenticated;
select set_config('request.jwt.claim.sub', :'user_id', false) \gset
with planet as (
  select public.get_my_planet_state() as state
), first_object as (
  select jsonb_build_object(
    'cycle_id', planet.state->>'current_cycle_id',
    'stage', (object.value->>'stage')::integer,
    'ordinal', (object.value->>'ordinal')::integer
  ) as natural_key,
    planet.state->'objects' as original_objects
  from planet
  cross join lateral jsonb_array_elements(planet.state->'objects') object(value)
  order by (object.value->>'stage')::integer, (object.value->>'ordinal')::integer
  limit 1
)
select jsonb_build_object(
  'key', first_object.natural_key,
  'quote', public.quote_shop_action(jsonb_build_object(
    'kind', 'remove_natural', 'key', first_object.natural_key
  )),
  'basis', first_object.original_objects
)
from first_object;
SQL
)" || fail 'could not quote the server-generated natural object'
[[ -n "$fixture" ]] || fail 'the new account did not generate a natural object'
race_natural_key="$(python3 - "$fixture" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
if not payload.get("basis"):
    raise SystemExit("natural removal race: server generated no natural objects")
print(json.dumps(payload["key"], separators=(",", ":")))
PY
)" || fail 'could not extract the generated natural key'
race_quote="$(python3 - "$fixture" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
quote = payload.get("quote") or {}
if quote.get("price") != 100000 or quote.get("target", {}).get("key") != payload.get("key"):
    raise SystemExit("natural removal race: generated key did not receive the stage-zero quote")
print(json.dumps(quote, separators=(",", ":")))
PY
)" || fail 'server quote did not match the generated stage-zero natural object'
original_basis="$(python3 - "$fixture" <<'PY'
import json
import sys

print(json.dumps(json.loads(sys.argv[1])["basis"], separators=(",", ":")))
PY
)" || fail 'could not capture the original natural-object generation basis'
natural_stage="$(python3 - "$race_natural_key" <<'PY'
import json
import sys

print(json.loads(sys.argv[1])["stage"])
PY
)"
natural_ordinal="$(python3 - "$race_natural_key" <<'PY'
import json
import sys

print(json.loads(sys.argv[1])["ordinal"])
PY
)"

wallet_balance="$(psql_test natural-removal-funded-check \
  -v ON_ERROR_STOP=1 -v user_id="$race_user_id" <<'SQL'
select set_config('request.jwt.claim.sub', :'user_id', false) \gset
select (public.get_my_shop_state()->>'available_balance')::bigint;
SQL
)" || fail 'could not verify the server-seeded wallet'
[[ "$wallet_balance" == '1000000' ]] || fail 'the server-seeded wallet did not expose exactly 1,000,000'

blocker_app="natural-removal-blocker-$race_run_id"
racer_a_app="natural-removal-racer-a-$race_run_id"
racer_b_app="natural-removal-racer-b-$race_run_id"
mkfifo "$run_dir/blocker.in" || fail 'could not create the account-lock input pipe'
docker exec --env "PGAPPNAME=$blocker_app" -i "$TEST_DB_CONTAINER" \
  psql -X -q -A -t -v ON_ERROR_STOP=1 -v user_id="$race_user_id" \
    -U postgres -d postgres \
  <"$run_dir/blocker.in" >"$run_dir/blocker.out" 2>&1 &
blocker_client_pid=$!
exec 3>"$run_dir/blocker.in"
cat <<'SQL' >&3
set statement_timeout = '30s';
set idle_in_transaction_session_timeout = '90s';
begin;
select user_id from private.shop_account_lock where user_id = :'user_id'::uuid for update;
select 'NATURAL_REMOVAL_BLOCKER_READY';
SQL

blocker_ready='no'
for _ in {1..100}; do
  if rg -q 'NATURAL_REMOVAL_BLOCKER_READY' "$run_dir/blocker.out"; then
    blocker_ready='yes'
    break
  fi
  sleep 0.1
done
[[ "$blocker_ready" == 'yes' ]] || fail 'the blocker session did not acquire the account row lock'
blocker_backend_pid="$(psql_test natural-removal-observer \
  -c "select pid::text from pg_stat_activity where application_name = '$blocker_app' and state = 'idle in transaction' limit 1")" \
  || fail 'could not identify the account-lock blocker backend'
[[ -n "$blocker_backend_pid" ]] || fail 'the account-lock blocker backend PID is empty'

run_removal() {
  local application_name="$1"
  local request_id="$2"
  local output_path="$3"
  psql_test "$application_name" \
    -v ON_ERROR_STOP=1 \
    -v user_id="$race_user_id" \
    -v request_id="$request_id" \
    -v natural_key="$race_natural_key" \
    -v quote_json="$race_quote" \
    >"$output_path" 2>&1 <<'SQL'
set role authenticated;
select set_config('request.jwt.claim.sub', :'user_id', false) \gset
set statement_timeout = '45s';
select public.apply_shop_action(jsonb_build_object(
  'kind', 'remove_natural',
  'request_id', :'request_id',
  'key', :'natural_key'::jsonb,
  'expected_version', 0,
  'quote', :'quote_json'::jsonb
))->>'status';
SQL
}

run_removal "$racer_a_app" "$request_a" "$run_dir/a.status" &
racer_a_pid=$!
run_removal "$racer_b_app" "$request_b" "$run_dir/b.status" &
racer_b_pid=$!

wait_count='0'
wait_count="$(psql_test natural-removal-observer \
  -v ON_ERROR_STOP=1 \
  -v racer_a_app="$racer_a_app" \
  -v racer_b_app="$racer_b_app" \
  -v blocker_backend_pid="$blocker_backend_pid" <<'SQL'
set statement_timeout = '45s';
create function pg_temp.wait_for_racer_lock_chains(
  p_racer_apps text[], p_blocker_pid integer
)
returns integer
language plpgsql as $$
declare
  v_attempt integer;
  v_waiting_roots integer := 0;
begin
  for v_attempt in 1..200 loop
    perform pg_stat_clear_snapshot();
    with recursive racer_roots as (
      select a.pid
      from pg_stat_activity a
      where a.application_name = any(p_racer_apps)
        and a.state = 'active'
        and a.wait_event_type = 'Lock'
    ), wait_chain(root_pid, current_pid, path, depth) as (
      select r.pid, r.pid, array[r.pid], 0
      from racer_roots r
      union all
      select chain.root_pid, blockers.pid,
        chain.path || blockers.pid, chain.depth + 1
      from wait_chain chain
      cross join lateral unnest(pg_blocking_pids(chain.current_pid)) blockers(pid)
      where chain.depth < 16 and not blockers.pid = any(chain.path)
    )
    select count(distinct chain.root_pid)::integer
    into v_waiting_roots
    from wait_chain chain
    where chain.current_pid = p_blocker_pid;

    if v_waiting_roots = cardinality(p_racer_apps) then
      return v_waiting_roots;
    end if;
    perform pg_sleep(0.1);
  end loop;
  return v_waiting_roots;
end;
$$;
select pg_temp.wait_for_racer_lock_chains(
  array[:'racer_a_app', :'racer_b_app']::text[],
  :'blocker_backend_pid'::integer
);
SQL
)" || {
  report_racer_diagnostics
  fail 'could not inspect the natural-removal blocking chains'
}
if [[ "$wait_count" != '2' ]]; then
  wait_graph="$(psql_test natural-removal-observer \
    -c "select coalesce(jsonb_agg(jsonb_build_object(
      'application_name', application_name, 'pid', pid,
      'wait_event_type', wait_event_type, 'wait_event', wait_event,
      'blocking_pids', pg_blocking_pids(pid)
    )), '[]'::jsonb) from pg_stat_activity
    where application_name in ('$blocker_app', '$racer_a_app', '$racer_b_app')")" \
    || wait_graph='unavailable'
  printf 'Natural removal wait proof found %s independent blocking chains; sessions: %s\n' \
    "$wait_count" "$wait_graph" >&2
  report_racer_diagnostics
  fail 'both independent removal requests were not observed with wait chains reaching the account-lock holder'
fi
printf 'Observed %s independent removal wait chains reaching the account-lock holder.\n' "$wait_count"

cat <<'SQL' >&3
commit;
select 'NATURAL_REMOVAL_BLOCKER_RELEASED';
SQL
exec 3>&-
wait_client "$blocker_client_pid" 'blocker session' \
  || fail 'blocker session did not exit after lock release'
blocker_client_pid=''
wait_client "$racer_a_pid" 'first removal request' \
  || { report_racer_diagnostics; fail 'first removal request failed or timed out'; }
racer_a_pid=''
wait_client "$racer_b_pid" 'second removal request' \
  || { report_racer_diagnostics; fail 'second removal request failed or timed out'; }
racer_b_pid=''

status_a="$(awk 'NF {value = $0} END {print value}' "$run_dir/a.status")"
status_b="$(awk 'NF {value = $0} END {print value}' "$run_dir/b.status")"
if ! { [[ "$status_a" == 'removed' && "$status_b" == 'already_removed' ]] \
    || [[ "$status_b" == 'removed' && "$status_a" == 'already_removed' ]]; }; then
  report_racer_diagnostics
  fail 'the two distinct requests must return exactly one removed and one already_removed'
fi

summary="$(psql_test natural-removal-summary \
  -v ON_ERROR_STOP=1 \
  -v user_id="$race_user_id" \
  -v cycle_id="$race_cycle_id" \
  -v stage="$natural_stage" \
  -v ordinal="$natural_ordinal" \
  -v request_a="$request_a" \
  -v request_b="$request_b" \
  -v basis_json="$original_basis" <<'SQL'
select jsonb_build_object(
  'removed_receipts', (select count(*) from private.shop_action_request r
    where r.user_id = :'user_id'::uuid
      and r.request_id = any(array[:'request_a', :'request_b']::text[])
      and r.status = 'removed'),
  'already_removed_receipts', (select count(*) from private.shop_action_request r
    where r.user_id = :'user_id'::uuid
      and r.request_id = any(array[:'request_a', :'request_b']::text[])
      and r.status = 'already_removed'),
  'receipt_count', (select count(*) from private.shop_action_request r
    where r.user_id = :'user_id'::uuid
      and r.request_id = any(array[:'request_a', :'request_b']::text[])),
  'removal_count', (select count(*) from private.shop_natural_removal r
    where r.user_id = :'user_id'::uuid and r.cycle_id = :'cycle_id'),
  'tombstone_count', (select count(*) from private.shop_natural_removal r
    where r.user_id = :'user_id'::uuid and r.cycle_id = :'cycle_id'
      and r.stage = :'stage'::integer and r.ordinal = :'ordinal'::integer),
  'removal_price_total', coalesce((select sum(r.price::numeric)::bigint
    from private.shop_natural_removal r
    where r.user_id = :'user_id'::uuid and r.cycle_id = :'cycle_id'), 0),
  'available_balance', private.shop_available_balance(:'user_id'::uuid),
  'wallet_credit_total', coalesce((select sum(w.amount::numeric)::bigint
    from private.planet_wallet_credits w where w.user_id = :'user_id'::uuid), 0),
  'purchase_count', (select count(*) from private.shop_purchase p
    where p.user_id = :'user_id'::uuid),
  'objects_unchanged', (select p.objects = :'basis_json'::jsonb
    from public.planet_member_state p where p.user_id = :'user_id'::uuid)
);
SQL
)" || fail 'could not inspect post-race canonical state'
python3 - "$summary" <<'PY'
import json
import sys

result = json.loads(sys.argv[1])
expected = {
    "removed_receipts": 1,
    "already_removed_receipts": 1,
    "receipt_count": 2,
    "removal_count": 1,
    "tombstone_count": 1,
    "removal_price_total": 100000,
    "available_balance": 900000,
    "wallet_credit_total": 1000000,
    "purchase_count": 0,
    "objects_unchanged": True,
}
if result != expected:
    print(f"natural removal race: expected {expected}, observed {result}", file=sys.stderr)
    raise SystemExit(1)
PY

printf 'Race statuses: %s, %s\n' "$status_a" "$status_b"
printf 'Verified one 100,000 removal, one current tombstone, a 900,000 balance, and unchanged natural-object basis.\n'
