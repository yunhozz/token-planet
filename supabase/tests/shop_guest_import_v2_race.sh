#!/usr/bin/env bash
set -euo pipefail
set +x

# Schema-2 public import races against synthetic accounts in the pinned local
# Supabase test database. This harness never starts or resets a project.
readonly TEST_PROJECT_ID='token-planet-shop-revamp-test'
readonly TEST_WORKDIR='/private/tmp/token-planet-shop-revamp-test'
readonly TEST_CONFIG="$TEST_WORKDIR/supabase/config.toml"
readonly TEST_DB_PORT='55432'
readonly TEST_DB_CONTAINER_NAME="supabase_db_$TEST_PROJECT_ID"
readonly TEST_DB_VOLUME="supabase_db_$TEST_PROJECT_ID"
readonly V2_FIRST_RESET_FIXTURE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/fixtures/shop_guest_import_v2_first_reset.json"
readonly EXPECTED_V2_FIXTURE_SHA256='c5bbc955848e86d389c4d4071026b940f1f8837617cb9ed11f138a610d4e3a62'

fail() {
  printf 'shop guest import v2 race: %s\n' "$1" >&2
  exit 1
}

command -v docker >/dev/null 2>&1 || fail 'docker is unavailable'
command -v python3 >/dev/null 2>&1 || fail 'python3 is unavailable'
command -v rg >/dev/null 2>&1 || fail 'rg is unavailable'
command -v shasum >/dev/null 2>&1 || fail 'shasum is unavailable'
[[ -f "$TEST_CONFIG" ]] || fail 'the pinned disposable project config is missing'
[[ -f "$V2_FIRST_RESET_FIXTURE" ]] || fail 'the frozen schema-2 first-reset fixture is missing'
fixture_sha256="$(shasum -a 256 "$V2_FIRST_RESET_FIXTURE" | awk '{print $1}')" \
  || fail 'could not hash the frozen schema-2 fixture'
[[ "$fixture_sha256" == "$EXPECTED_V2_FIXTURE_SHA256" ]] \
  || fail 'the frozen schema-2 fixture hash does not match'

docker_context="$(docker context show 2>/dev/null)" \
  || fail 'could not identify the Docker context'
docker_endpoint="$(docker context inspect "$docker_context" \
  --format '{{(index .Endpoints "docker").Host}}' 2>/dev/null)" \
  || fail 'could not verify the local Docker endpoint'
[[ "$docker_endpoint" == unix://* ]] || fail 'refusing a non-local Docker endpoint'
[[ "$docker_context" == 'desktop-linux' ]] || fail 'refusing a Docker context other than the pinned desktop-linux context'

python3 - "$TEST_CONFIG" "$TEST_PROJECT_ID" "$TEST_DB_PORT" <<'PY'
import sys
import tomllib
from pathlib import Path

config_path, expected_project, expected_port = sys.argv[1:]
config = tomllib.loads(Path(config_path).read_text())
if config.get("project_id") != expected_project:
    raise SystemExit("shop guest import v2 race: disposable project ID does not match")
if str(config.get("db", {}).get("port")) != expected_port:
    raise SystemExit("shop guest import v2 race: disposable database port does not match")
PY

container_id="$(docker inspect --format '{{.Id}}' "$TEST_DB_CONTAINER_NAME" 2>/dev/null)" \
  || fail 'the pinned disposable database container is unavailable'
[[ -n "$container_id" ]] || fail 'the pinned database container ID is empty'
readonly TEST_DB_CONTAINER="$container_id"
container_running="$(docker inspect --format '{{.State.Running}}' "$TEST_DB_CONTAINER")"
[[ "$container_running" == 'true' ]] || fail 'the pinned database container is not running'
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
    raise SystemExit("shop guest import v2 race: pinned database volume does not match")
bindings = ports.get("5432/tcp") or []
if not any(binding.get("HostPort") == expected_port for binding in bindings):
    raise SystemExit("shop guest import v2 race: pinned database port does not match")
PY

psql_test() {
  local application_name="$1"
  shift
  docker exec --env "PGAPPNAME=$application_name" -i "$TEST_DB_CONTAINER" \
    psql -X -q -A -t "$@" -U postgres -d postgres
}

database_digest() {
  local application_name="$1"
  psql_test "$application_name" -v ON_ERROR_STOP=1 <<'SQL'
create function pg_temp.shop_guest_import_v2_race_digest() returns text
language plpgsql
as $digest$
declare
  v_table record;
  v_count bigint;
  v_rows_hash text;
  v_contents text := '';
begin
  for v_table in
    select schemaname, tablename
    from pg_catalog.pg_tables
    where schemaname in ('public', 'private', 'auth')
    order by schemaname, tablename
  loop
    execute pg_catalog.format(
      'select count(*)::bigint, pg_catalog.md5(coalesce(pg_catalog.string_agg(pg_catalog.to_jsonb(t)::text, pg_catalog.chr(10) order by pg_catalog.to_jsonb(t)::text), %L)) from %I.%I as t',
      '', v_table.schemaname, v_table.tablename
    ) into v_count, v_rows_hash;
    v_contents := v_contents || v_table.schemaname || '.' || v_table.tablename || ':' ||
      v_count::text || ':' || v_rows_hash || pg_catalog.chr(10);
  end loop;
  return pg_catalog.md5(v_contents);
end;
$digest$;
select pg_temp.shop_guest_import_v2_race_digest();
SQL
}

observer_app="shop-bootstrap-observer-$(python3 -c 'import uuid; print(uuid.uuid4())')"
server_identity="$(psql_test "$observer_app" \
  -c "select current_database() || '|' || current_user || '|' || current_setting('port')")" \
  || fail 'could not connect to the pinned local database'
[[ "$server_identity" == 'postgres|postgres|5432' ]] \
  || fail 'connected database identity does not match the pinned local Supabase database'
printf 'shop guest import v2 race: pinned container=%s fixture_sha256=%s database=%s\n' \
  "$TEST_DB_CONTAINER" "$fixture_sha256" "$server_identity"

safe_target='no'
run_id="$(python3 -c 'import uuid; print(uuid.uuid4())')"
user_same="$(python3 -c 'import uuid; print(uuid.uuid4())')"
user_conflict="$(python3 -c 'import uuid; print(uuid.uuid4())')"
user_different="$(python3 -c 'import uuid; print(uuid.uuid4())')"
user_usage="$(python3 -c 'import uuid; print(uuid.uuid4())')"
same_import_id="$(python3 -c 'import uuid; print(uuid.uuid4())')"
conflict_import_id="$(python3 -c 'import uuid; print(uuid.uuid4())')"
different_import_a="$(python3 -c 'import uuid; print(uuid.uuid4())')"
different_import_b="$(python3 -c 'import uuid; print(uuid.uuid4())')"
usage_import_id="$(python3 -c 'import uuid; print(uuid.uuid4())')"
device_same="$(python3 -c 'import uuid; print(uuid.uuid4())')"
device_conflict="$(python3 -c 'import uuid; print(uuid.uuid4())')"
device_different="$(python3 -c 'import uuid; print(uuid.uuid4())')"
device_usage="$(python3 -c 'import uuid; print(uuid.uuid4())')"
test_users="$user_same $user_conflict $user_different $user_usage"
known_apps="$observer_app"
run_dir=''
active_client_pids=()
blocker_client_pid=''
cleanup_failed='no'

cleanup() {
  local exit_code=$?
  trap - EXIT INT TERM
  exec 3>&- 2>/dev/null || true
  if [[ "$safe_target" == 'yes' ]]; then
    for app_name in $known_apps; do
      psql_test shop-bootstrap-cleanup \
        -c "select pg_terminate_backend(pid) from pg_stat_activity where application_name = '$app_name' and pid <> pg_backend_pid()" \
        >/dev/null 2>&1 || cleanup_failed='yes'
    done
    for app_name in $known_apps; do
      active_count="$(psql_test shop-bootstrap-cleanup \
        -c "select count(*)::text from pg_stat_activity where application_name = '$app_name'")" \
        || cleanup_failed='yes'
      [[ "$active_count" == '0' ]] || cleanup_failed='yes'
    done
    if [[ -n "${active_client_pids[*]-}" ]]; then
      for client_pid in "${active_client_pids[@]}"; do
        [[ -n "$client_pid" ]] && kill "$client_pid" >/dev/null 2>&1 || true
      done
    fi
    if [[ -n "$blocker_client_pid" ]]; then
      kill "$blocker_client_pid" >/dev/null 2>&1 || true
    fi
    if [[ -n "${active_client_pids[*]-}" ]]; then
      for client_pid in "${active_client_pids[@]}"; do
        [[ -n "$client_pid" ]] && wait "$client_pid" >/dev/null 2>&1 || true
      done
    fi
    if [[ -n "$blocker_client_pid" ]]; then
      wait "$blocker_client_pid" >/dev/null 2>&1 || true
    fi
    for user_id in $test_users; do
      psql_test shop-bootstrap-cleanup -v ON_ERROR_STOP=1 \
        -c "delete from auth.users where id = '$user_id'::uuid" \
        >/dev/null 2>&1 || cleanup_failed='yes'
      remaining_users="$(psql_test shop-bootstrap-cleanup \
        -c "select count(*)::text from auth.users where id = '$user_id'::uuid")" \
        || cleanup_failed='yes'
      [[ "$remaining_users" == '0' ]] || cleanup_failed='yes'
      remaining_receipts="$(psql_test shop-bootstrap-cleanup -v user_id="$user_id" \
        -c "select count(*)::text from private.shop_guest_bootstrap_receipt where user_id = '$user_id'::uuid")" \
        || cleanup_failed='yes'
      [[ "$remaining_receipts" == '0' ]] || cleanup_failed='yes'
      remaining_requests="$(psql_test shop-bootstrap-cleanup -v user_id="$user_id" \
        -c "select count(*)::text from private.shop_guest_import_request where user_id = '$user_id'::uuid")" \
        || cleanup_failed='yes'
      [[ "$remaining_requests" == '0' ]] || cleanup_failed='yes'
      remaining_locks="$(psql_test shop-bootstrap-cleanup -v user_id="$user_id" \
        -c "select count(*)::text from private.shop_account_lock where user_id = '$user_id'::uuid")" \
        || cleanup_failed='yes'
      [[ "$remaining_locks" == '0' ]] || cleanup_failed='yes'
      remaining_game_rows="$(psql_test shop-bootstrap-cleanup -v user_id="$user_id" \
        -c "select (select count(*) from public.planet_member_state where user_id = '$user_id'::uuid) + (select count(*) from private.shop_account_state where user_id = '$user_id'::uuid) + (select count(*) from private.growth_journal_state where user_id = '$user_id'::uuid) + (select count(*) from private.growth_journal_cycles where user_id = '$user_id'::uuid) + (select count(*) from private.growth_journal_days where user_id = '$user_id'::uuid)")" \
        || cleanup_failed='yes'
      [[ "$remaining_game_rows" == '0' ]] || cleanup_failed='yes'
    done
  fi
  if [[ -n "$run_dir" && -d "$run_dir" ]]; then
    rm -rf -- "$run_dir"
  fi
  if [[ "$safe_target" == 'yes' && -n "${before_digest:-}" ]]; then
    after_digest="$(database_digest "sgbr-cleanup-$run_id")" \
      || cleanup_failed='yes'
    if [[ "$after_digest" != "$before_digest" ]]; then
      printf 'shop guest import v2 race: protected-table digest changed (before=%s after=%s)\n' \
        "$before_digest" "$after_digest" >&2
      cleanup_failed='yes'
    fi
  fi
  if [[ "$cleanup_failed" != 'no' ]]; then
    printf 'shop guest import v2 race: cleanup or protected-table digest verification failed\n' >&2
    exit_code=1
  elif [[ "$safe_target" == 'yes' ]]; then
    printf 'shop guest import v2 race: cleanup verified zero owned users, receipts, game rows, and unchanged protected-table digest\n'
  fi
  exit "$exit_code"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

run_dir="$(mktemp -d '/private/tmp/shop-guest-import-v2-race.XXXXXXXX')" \
  || fail 'could not create a private result directory'

bootstrap_request() {
  local raw_request
  raw_request="$(python3 - "$V2_FIRST_RESET_FIXTURE" "$1" "$2" "$4" <<'PY'
import json
import sys
import uuid
from pathlib import Path

fixture_path, user_id, import_id, nickname = sys.argv[1:]
uuid.UUID(user_id)
uuid.UUID(import_id)
request = json.loads(Path(fixture_path).read_text())
snapshot = request.get("snapshot")
if request.get("schema_version") != 2 or not isinstance(snapshot, dict):
    raise SystemExit("shop guest import v2 race: schema-2 fixture envelope is malformed")
snapshot["import_id"] = import_id
snapshot["target_account_id"] = "account:" + user_id
if snapshot.get("source_account_id") != "local" or snapshot.get("disposition") != "local_integrity_validated":
    raise SystemExit("shop guest import v2 race: frozen fixture source contract changed")
if not isinstance(snapshot.get("data"), dict) or snapshot["data"].get("profile") is not None:
    raise SystemExit("shop guest import v2 race: frozen null-profile fixture changed")
snapshot["data"]["profile"] = {"nickname": nickname, "avatar": "masculine"}
print(json.dumps(request, separators=(",", ":")))
PY
  )" || fail 'could not construct a schema-2 request from the frozen fixture'
  psql_test "sgbr-reseal-$run_id" -v ON_ERROR_STOP=1 -v request_json="$raw_request" <<'SQL'
with request as (
  select :'request_json'::jsonb as value
), canonical as (
  select private.shop_guest_import_v2_canonical_json(
    (value->'snapshot') - 'source_fingerprint'
  ) as value
  from request
)
select jsonb_set(
  request.value,
  '{snapshot,source_fingerprint}',
  to_jsonb(encode(extensions.digest(convert_to(canonical.value, 'UTF8'), 'sha256'), 'hex')),
  true
)::text
from request cross join canonical;
SQL
}

insert_test_users() {
  local existing_users
  existing_users="$(psql_test shop-bootstrap-preflight \
    -v user_same="$user_same" -v user_conflict="$user_conflict" \
    -v user_different="$user_different" -v user_usage="$user_usage" <<'SQL'
select count(*)::text from auth.users
where id in (:'user_same'::uuid, :'user_conflict'::uuid,
             :'user_different'::uuid, :'user_usage'::uuid);
SQL
  )" || fail 'could not verify generated synthetic auth IDs are unused'
  [[ "$existing_users" == '0' ]] || fail 'a generated synthetic auth ID already exists'
  psql_test shop-bootstrap-fixture -v ON_ERROR_STOP=1 \
    -v user_same="$user_same" -v user_conflict="$user_conflict" \
    -v user_different="$user_different" -v user_usage="$user_usage" \
    >/dev/null <<'SQL' || fail 'could not create synthetic auth accounts'
insert into auth.users(id) values
  (:'user_same'::uuid), (:'user_conflict'::uuid),
  (:'user_different'::uuid), (:'user_usage'::uuid);
SQL
  safe_target='yes'
}

seed_account_lock() {
  psql_test shop-bootstrap-lock-seed -v ON_ERROR_STOP=1 -v user_id="$1" \
    >/dev/null <<'SQL' || fail 'could not prepare a synthetic account lock row'
insert into private.shop_account_lock(user_id) values (:'user_id'::uuid)
on conflict (user_id) do nothing;
SQL
}

start_lock_blocker() {
  local user_id="$1"
  local label="$2"
  local fifo_path="$run_dir/$label-blocker.in"
  blocker_app="sgbr-lock-$label-$run_id"
  known_apps="$known_apps $blocker_app"
  mkfifo "$fifo_path" || fail 'could not create the account-lock input pipe'
  docker exec --env "PGAPPNAME=$blocker_app" -i "$TEST_DB_CONTAINER" \
    psql -X -q -A -t -v ON_ERROR_STOP=1 -v user_id="$user_id" \
      -U postgres -d postgres \
    <"$fifo_path" >"$run_dir/$label-blocker.out" 2>&1 &
  blocker_client_pid=$!
  active_client_pids+=("$blocker_client_pid")
  exec 3>"$fifo_path"
  cat <<'SQL' >&3
set statement_timeout = '45s';
set idle_in_transaction_session_timeout = '90s';
begin;
select user_id from private.shop_account_lock where user_id = :'user_id'::uuid for update;
select 'BOOTSTRAP_LOCK_READY';
SQL
  for _ in {1..100}; do
    if rg -q 'BOOTSTRAP_LOCK_READY' "$run_dir/$label-blocker.out"; then
      return
    fi
    sleep 0.1
  done
  fail 'the synthetic account-lock blocker did not become ready'
}

untrack_client_pid() {
  local target_pid="$1"
  local client_pid
  local remaining_pids=()
  if [[ -n "${active_client_pids[*]-}" ]]; then
    for client_pid in "${active_client_pids[@]}"; do
      if [[ "$client_pid" != "$target_pid" && -n "$client_pid" ]]; then
        remaining_pids+=("$client_pid")
      fi
    done
  fi
  active_client_pids=()
  if [[ -n "${remaining_pids[*]-}" ]]; then
    active_client_pids=("${remaining_pids[@]}")
  fi
}

release_lock_blocker() {
  [[ -n "$blocker_client_pid" ]] || fail 'the account-lock blocker PID is empty'
  cat <<'SQL' >&3
commit;
\q
SQL
  exec 3>&-
  if ! wait "$blocker_client_pid"; then
    fail 'the synthetic account-lock blocker failed'
  fi
  untrack_client_pid "$blocker_client_pid"
  blocker_client_pid=''
}

run_bootstrap() {
  local application_name="$1"
  local user_id="$2"
  local import_id="$3"
  local request_json="$4"
  psql_test "$application_name" -v ON_ERROR_STOP=1 \
    -v user_id="$user_id" -v import_id="$import_id" \
    -v request_json="$request_json" <<'SQL'
set statement_timeout = '45s';
  set role authenticated;
  select set_config('request.jwt.claim.sub', :'user_id', false);
  select public.import_guest_shop(:'import_id'::uuid, :'request_json'::jsonb);
SQL
}

wait_for_lock_waiters() {
  local expected_count="$1"
  shift
  local app_names=''
  local app_name
  local waiting_count=''
  for app_name in "$@"; do
    app_names="${app_names:+$app_names,}'$app_name'"
  done
  for _ in {1..100}; do
    waiting_count="$(psql_test shop-bootstrap-observer \
      -c "select count(*)::text from pg_stat_activity where application_name in ($app_names) and wait_event_type = 'Lock'")" \
      || fail 'could not observe bootstrap lock waiters'
    if [[ "$waiting_count" == "$expected_count" ]]; then
      return
    fi
    sleep 0.1
  done
  psql_test shop-bootstrap-observer \
    -c "select application_name || '|' || state || '|' || coalesce(wait_event_type, '') || '|' || left(query, 140) from pg_stat_activity where application_name in ($app_names)" \
    >&2 || true
  fail "expected $expected_count bootstrap lock waiter(s), found $waiting_count"
}

assert_same_imported_results() {
  python3 - "$1" "$2" <<'PY'
import json
import sys
from pathlib import Path

def last_json(path):
    return json.loads([line for line in Path(path).read_text().splitlines() if line.strip()][-1])

first, second = (last_json(path) for path in sys.argv[1:])
if first != second or first.get("status") != "imported":
    raise SystemExit("shop guest import v2 race: equal payloads did not receive one imported result")
print("same-ID equal race: both callers received the identical imported result")
PY
}

assert_result_statuses() {
  local expected="$1"
  shift
  python3 - "$expected" "$@" <<'PY'
import json
import sys
from pathlib import Path

expected = sorted(sys.argv[1].split(","))
statuses = []
for path in sys.argv[2:]:
    lines = [line for line in Path(path).read_text().splitlines() if line.strip()]
    statuses.append(json.loads(lines[-1]).get("status"))
if sorted(statuses) != expected:
    raise SystemExit(f"shop guest import v2 race: expected statuses {expected}, got {sorted(statuses)}")
print("race statuses: " + ", ".join(sorted(statuses)))
PY
}

assert_same_result_json() {
  python3 - "$1" "$2" <<'PY'
import json
import sys
from pathlib import Path

def last_json(path):
    return json.loads([line for line in Path(path).read_text().splitlines() if line.strip()][-1])

if last_json(sys.argv[1]) != last_json(sys.argv[2]):
    raise SystemExit("shop guest import v2 race: replay result differs from stored imported result")
PY
}

receipt_count() {
  psql_test shop-bootstrap-observer -v user_id="$1" -v import_id="$2" <<'SQL'
select count(*)::text from private.shop_guest_bootstrap_receipt
where user_id = :'user_id'::uuid and import_id = :'import_id'::uuid;
SQL
}

receipt_digest() {
  psql_test shop-bootstrap-observer -v user_id="$1" -v import_id="$2" <<'SQL'
select md5(payload::text || E'\n' || source_fingerprint || E'\n' || status || E'\n' || result::text || E'\n' || coalesce(normalized_proof::text, 'null'))
from private.shop_guest_bootstrap_receipt
where user_id = :'user_id'::uuid and import_id = :'import_id'::uuid;
SQL
}

assert_native_game_rows() {
  local user_id="$1"
  local expected="$2"
  local counts
  counts="$(psql_test shop-bootstrap-observer -v user_id="$user_id" <<'SQL'
select (select count(*) from public.planet_member_state where user_id = :'user_id'::uuid)::text || '|' ||
       (select count(*) from private.shop_account_state where user_id = :'user_id'::uuid)::text || '|' ||
       (select count(*) from private.growth_journal_state where user_id = :'user_id'::uuid)::text || '|' ||
       (select count(*) from private.growth_journal_cycles where user_id = :'user_id'::uuid)::text || '|' ||
       (select count(*) from private.growth_journal_days where user_id = :'user_id'::uuid)::text;
SQL
  )" || fail 'could not verify native bootstrap game-row counts'
  [[ "$counts" == "$expected" ]] || fail "expected native game-row counts $expected, got $counts"
  printf '%s\n' "$counts"
}

before_digest="$(database_digest "sgbr-before-$run_id")" \
  || fail 'could not compute the pre-race public/private/auth data digest'
[[ "$before_digest" =~ ^[0-9a-f]{32}$ ]] \
  || fail 'pre-race protected-table digest was malformed'
printf 'pre-race protected-table digest: %s\n' "$before_digest"
insert_test_users
same_request="$(bootstrap_request "$user_same" "$same_import_id" "$device_same" 'Same Native')"
seed_account_lock "$user_same"
start_lock_blocker "$user_same" same-equal
same_app_a="sgbr-same-a-$run_id"
same_app_b="sgbr-same-b-$run_id"
known_apps="$known_apps $same_app_a $same_app_b"
run_bootstrap "$same_app_a" "$user_same" "$same_import_id" "$same_request" \
  >"$run_dir/same-a.out" 2>&1 &
same_pid_a=$!
active_client_pids+=("$same_pid_a")
run_bootstrap "$same_app_b" "$user_same" "$same_import_id" "$same_request" \
  >"$run_dir/same-b.out" 2>&1 &
same_pid_b=$!
active_client_pids+=("$same_pid_b")
wait_for_lock_waiters 2 "$same_app_a" "$same_app_b"
release_lock_blocker
if ! wait "$same_pid_a"; then fail 'same-ID equal caller A failed'; fi
untrack_client_pid "$same_pid_a"
if ! wait "$same_pid_b"; then fail 'same-ID equal caller B failed'; fi
untrack_client_pid "$same_pid_b"
assert_same_imported_results "$run_dir/same-a.out" "$run_dir/same-b.out"
[[ "$(receipt_count "$user_same" "$same_import_id")" == '1' ]] \
  || fail 'same-ID equal race did not persist exactly one private receipt'
assert_native_game_rows "$user_same" '1|1|1|2|1'
same_digest_before="$(receipt_digest "$user_same" "$same_import_id")" \
  || fail 'could not read the immutable equal-race receipt digest'
run_bootstrap sgbr-same-replay "$user_same" "$same_import_id" "$same_request" \
  >"$run_dir/same-replay.out" 2>&1 || fail 'same-ID equal replay failed'
assert_same_result_json "$run_dir/same-a.out" "$run_dir/same-replay.out"
same_digest_after="$(receipt_digest "$user_same" "$same_import_id")" \
  || fail 'could not reread the equal-race receipt digest'
[[ "$same_digest_before" == "$same_digest_after" ]] \
  || fail 'same-ID equal replay changed its immutable private receipt'
printf 'same-ID equal evidence: receipts=1 immutable_digest=%s game_rows=1|1|1|2|1\n' "$same_digest_after"
printf 'shop guest import v2 race: same-ID equal race passed\n'

result_status() {
  python3 - "$1" <<'PY'
import json
import sys
from pathlib import Path

lines = [line for line in Path(sys.argv[1]).read_text().splitlines() if line.strip()]
print(json.loads(lines[-1]).get("status", ""))
PY
}

receipt_status() {
  psql_test shop-bootstrap-observer -v user_id="$1" -v import_id="$2" <<'SQL'
select status from private.shop_guest_bootstrap_receipt
where user_id = :'user_id'::uuid and import_id = :'import_id'::uuid;
SQL
}

assert_imported_active_results() {
  python3 - "$1" "$2" <<'PY'
import json
import sys
from pathlib import Path

def last_json(path):
    return json.loads([line for line in Path(path).read_text().splitlines() if line.strip()][-1])

first, second = (last_json(path) for path in sys.argv[1:])
if sorted((first.get("status"), second.get("status"))) != ["active_account", "imported"]:
    raise SystemExit("shop guest import v2 race: distinct IDs did not yield imported+active_account")
active = first if first.get("status") == "active_account" else second
imported = second if active is first else first
success_fields = ("shop_state", "planet_state", "effect_timeline", "canonical_contribution", "journal_confirmation", "ack")
if any(field in active for field in success_fields):
    raise SystemExit("shop guest import v2 race: active response exposed a success projection")
if imported.get("schema_version") != 2 or any(not isinstance(imported.get(field), dict) for field in success_fields):
    raise SystemExit("shop guest import v2 race: imported response omitted a typed success projection")
print("different-ID race: one full schema-2 import and one projection-free active_account response")
PY
}

conflict_request_a="$(bootstrap_request "$user_conflict" "$conflict_import_id" "$device_conflict" 'Conflict Native A')"
conflict_request_b="$(bootstrap_request "$user_conflict" "$conflict_import_id" "$device_conflict" 'Conflict Native B')"
seed_account_lock "$user_conflict"
start_lock_blocker "$user_conflict" same-different
conflict_app_a="sgbr-conflict-a-$run_id"
conflict_app_b="sgbr-conflict-b-$run_id"
known_apps="$known_apps $conflict_app_a $conflict_app_b"
run_bootstrap "$conflict_app_a" "$user_conflict" "$conflict_import_id" "$conflict_request_a" \
  >"$run_dir/conflict-a.out" 2>&1 &
conflict_pid_a=$!
[[ -n "$conflict_pid_a" ]] || fail 'same-ID conflict caller A PID is empty'
active_client_pids+=("$conflict_pid_a")
wait_for_lock_waiters 1 "$conflict_app_a"
run_bootstrap "$conflict_app_b" "$user_conflict" "$conflict_import_id" "$conflict_request_b" \
  >"$run_dir/conflict-b.out" 2>&1 &
conflict_pid_b=$!
[[ -n "$conflict_pid_b" ]] || fail 'same-ID conflict caller B PID is empty'
active_client_pids+=("$conflict_pid_b")
wait_for_lock_waiters 2 "$conflict_app_a" "$conflict_app_b"
release_lock_blocker
if ! wait "$conflict_pid_a"; then fail 'same-ID conflict caller A failed'; fi
untrack_client_pid "$conflict_pid_a"
if ! wait "$conflict_pid_b"; then fail 'same-ID conflict caller B failed'; fi
untrack_client_pid "$conflict_pid_b"
assert_result_statuses 'imported,request_conflict' \
  "$run_dir/conflict-a.out" "$run_dir/conflict-b.out"
[[ "$(receipt_count "$user_conflict" "$conflict_import_id")" == '1' ]] \
  || fail 'same-ID conflict race did not persist exactly one private receipt'
[[ "$(receipt_status "$user_conflict" "$conflict_import_id")" == 'imported' ]] \
  || fail 'same-ID conflict race did not preserve the winning imported receipt'
[[ "$(result_status "$run_dir/conflict-a.out")" == 'imported' ]] \
  || fail 'the first queued valid same-ID payload did not win the race'
[[ "$(result_status "$run_dir/conflict-b.out")" == 'request_conflict' ]] \
  || fail 'the changed same-ID payload did not conflict with the committed winner'
assert_native_game_rows "$user_conflict" '1|1|1|2|1'
stored_conflict_payload_matches="$(psql_test shop-bootstrap-observer \
  -v user_id="$user_conflict" -v import_id="$conflict_import_id" \
  -v winner_request="$conflict_request_a" -v loser_request="$conflict_request_b" <<'SQL'
select (payload = :'winner_request'::jsonb and payload <> :'loser_request'::jsonb)::text
from private.shop_guest_bootstrap_receipt
where user_id = :'user_id'::uuid and import_id = :'import_id'::uuid;
SQL
)" || fail 'could not verify the immutable conflicting receipt payload'
[[ "$stored_conflict_payload_matches" == 'true' ]] \
  || fail 'the losing valid payload replaced the first receipt'
conflict_digest_before="$(receipt_digest "$user_conflict" "$conflict_import_id")" \
  || fail 'could not read the conflicting receipt digest'
run_bootstrap sgbr-conflict-replay "$user_conflict" "$conflict_import_id" "$conflict_request_a" \
  >"$run_dir/conflict-replay.out" 2>&1 || fail 'same-ID conflict winner replay failed'
assert_same_result_json "$run_dir/conflict-a.out" "$run_dir/conflict-replay.out"
run_bootstrap sgbr-conflict-loser "$user_conflict" "$conflict_import_id" "$conflict_request_b" \
  >"$run_dir/conflict-loser-replay.out" 2>&1 || fail 'same-ID conflict loser replay failed'
[[ "$(result_status "$run_dir/conflict-loser-replay.out")" == 'request_conflict' ]] \
  || fail 'the losing payload no longer conflicts after the race'
conflict_digest_after="$(receipt_digest "$user_conflict" "$conflict_import_id")" \
  || fail 'could not reread the conflicting receipt digest'
[[ "$conflict_digest_before" == "$conflict_digest_after" ]] \
  || fail 'same-ID conflict replay changed the immutable private receipt'
printf 'same-ID conflict evidence: receipts=1 immutable_digest=%s game_rows=1|1|1|2|1\n' "$conflict_digest_after"
printf 'shop guest import v2 race: same-ID different-payload race passed\n'

different_request_a="$(bootstrap_request "$user_different" "$different_import_a" "$device_different" 'Different Native')"
different_request_b="$(bootstrap_request "$user_different" "$different_import_b" "$device_different" 'Different Native')"
seed_account_lock "$user_different"
start_lock_blocker "$user_different" different-id
different_app_a="sgbr-different-a-$run_id"
different_app_b="sgbr-different-b-$run_id"
known_apps="$known_apps $different_app_a $different_app_b"
run_bootstrap "$different_app_a" "$user_different" "$different_import_a" "$different_request_a" \
  >"$run_dir/different-a.out" 2>&1 &
different_pid_a=$!
[[ -n "$different_pid_a" ]] || fail 'different-ID caller A PID is empty'
active_client_pids+=("$different_pid_a")
run_bootstrap "$different_app_b" "$user_different" "$different_import_b" "$different_request_b" \
  >"$run_dir/different-b.out" 2>&1 &
different_pid_b=$!
[[ -n "$different_pid_b" ]] || fail 'different-ID caller B PID is empty'
active_client_pids+=("$different_pid_b")
wait_for_lock_waiters 2 "$different_app_a" "$different_app_b"
release_lock_blocker
if ! wait "$different_pid_a"; then fail 'different-ID caller A failed'; fi
untrack_client_pid "$different_pid_a"
if ! wait "$different_pid_b"; then fail 'different-ID caller B failed'; fi
untrack_client_pid "$different_pid_b"
assert_imported_active_results "$run_dir/different-a.out" "$run_dir/different-b.out"
[[ "$(receipt_count "$user_different" "$different_import_a")" == '1' ]] \
  || fail 'different-ID race did not persist its first receipt exactly once'
[[ "$(receipt_count "$user_different" "$different_import_b")" == '1' ]] \
  || fail 'different-ID race did not persist its second receipt exactly once'
different_receipts="$(psql_test shop-bootstrap-observer -v user_id="$user_different" <<'SQL'
select count(*)::text from private.shop_guest_bootstrap_receipt
where user_id = :'user_id'::uuid;
SQL
)" || fail 'could not verify different-ID receipt count'
[[ "$different_receipts" == '2' ]] || fail "expected two different-ID receipts, got $different_receipts"
assert_native_game_rows "$user_different" '1|1|1|2|1'
printf 'different-ID evidence: receipts=%s game_rows=1|1|1|2|1\n' "$different_receipts"
printf 'shop guest import v2 race: different-ID same-fresh-account race passed\n'

usage_payload() {
  python3 - "$1" "$2" <<'PY'
import json
import sys

cycle_id, device_id = sys.argv[1:]
state = {
    "version": 1,
    "profile": {"nickname": "Usage Writer", "avatar": "feminine"},
    "timezone": "Asia/Seoul",
    "current_cycle_id": cycle_id,
    "cycle_started_at_utc": "2026-09-30T00:00:00Z",
    "last_reset_at_utc": None,
    "wallet_balance": 0,
    "wallet_credits": [],
    "current_planet_tokens": 0,
    "lifetime_tokens": 0,
    "growth_credit": 0,
    "stage": 0,
    "progress_to_next": 0,
    "incomplete": False,
    "can_reset": True,
    "reset_available_at_utc": None,
    "objects": [],
}
device = {
    "device_id": device_id,
    "current_cycle_id": cycle_id,
    "lifetime_tokens": 1,
    "current_planet_tokens": 1,
    "daily_tokens": {"2026-09-30": 1},
    "incomplete": False,
    "canonical_version": 1,
    "daily_segments": [{
        "cycle_id": cycle_id, "date": "2026-09-30",
        "effect_revision": 0, "tokens": 1,
    }],
    "activity_days": [{
        "cycle_id": cycle_id, "reward_date": "2026-09-30",
        "first_occurred_at_utc": "2026-09-30T13:00:00Z", "tokens": 1,
    }],
}
print(json.dumps({"state": state, "device": device}, separators=(",", ":")))
PY
}

start_usage_writer_blocker() {
  local user_id="$1"
  local payload_json="$2"
  local fifo_path="$run_dir/usage-writer-blocker.in"
  blocker_app="sgbr-usage-writer-$run_id"
  known_apps="$known_apps $blocker_app"
  mkfifo "$fifo_path" || fail 'could not create the usage-writer input pipe'
  docker exec --env "PGAPPNAME=$blocker_app" -i "$TEST_DB_CONTAINER" \
    psql -X -q -A -t -v ON_ERROR_STOP=1 \
      -v user_id="$user_id" -v payload_json="$payload_json" \
      -U postgres -d postgres \
    <"$fifo_path" >"$run_dir/usage-blocker.out" 2>&1 &
  blocker_client_pid=$!
  [[ -n "$blocker_client_pid" ]] || fail 'usage-writer blocker PID is empty'
  active_client_pids+=("$blocker_client_pid")
  exec 3>"$fifo_path"
  cat <<'SQL' >&3
set statement_timeout = '45s';
set idle_in_transaction_session_timeout = '90s';
begin;
set role authenticated;
select set_config('request.jwt.claim.sub', :'user_id', true);
select public.upsert_my_planet_state(
  (:'payload_json'::jsonb->'state'), (:'payload_json'::jsonb->'device')
);
select 'USAGE_STATE|' || public.get_my_planet_state()::text;
reset role;
select 'USAGE_COUNTS|' ||
  (select count(*) from public.planet_member_state where user_id = :'user_id'::uuid)::text || '|' ||
  (select count(*) from private.shop_account_state where user_id = :'user_id'::uuid)::text || '|' ||
  (select count(*) from private.growth_journal_state where user_id = :'user_id'::uuid)::text || '|' ||
  (select count(*) from private.growth_journal_cycles where user_id = :'user_id'::uuid)::text;
select 'USAGE_WRITER_READY';
SQL
  for _ in {1..100}; do
    if rg -q 'USAGE_WRITER_READY' "$run_dir/usage-blocker.out"; then
      return
    fi
    sleep 0.1
  done
  fail 'the one-token usage writer did not become ready with its transaction open'
}

assert_usage_state_and_counts_preserved() {
  python3 - "$1" "$2" "$3" <<'PY'
import json
import sys
from pathlib import Path

before_state = None
before_counts = None
for line in Path(sys.argv[1]).read_text().splitlines():
    if line.startswith("USAGE_STATE|"):
        before_state = json.loads(line.split("|", 1)[1])
    if line.startswith("USAGE_COUNTS|"):
        before_counts = line.split("|", 1)[1]
if before_state is None or before_counts is None:
    raise SystemExit("shop guest import v2 race: usage writer did not emit state/count snapshots")
after_lines = [line for line in Path(sys.argv[2]).read_text().splitlines() if line.strip()]
after_state = json.loads(after_lines[-1])
after_counts = Path(sys.argv[3]).read_text().strip()
if before_state != after_state or before_counts != after_counts:
    raise SystemExit("shop guest import v2 race: bootstrap changed initial usage state or game-row counts")
if before_state.get("profile", {}).get("nickname") != "Usage Writer":
    raise SystemExit("shop guest import v2 race: usage writer profile is unexpected")
if before_state.get("current_planet_tokens") != 1 or before_state.get("lifetime_tokens") != 1:
    raise SystemExit("shop guest import v2 race: canonical one-token contribution was not retained")
print(f"usage-first evidence: profile=Usage Writer planet_tokens=1 lifetime_tokens=1 game_rows={before_counts}")
print("usage-first race: profile, one-token totals, and all four usage game-row counts stayed unchanged")
PY
}

usage_cycle="bootstrap-usage-$run_id"
usage_payload_json="$(usage_payload "$usage_cycle" "$device_usage")"
usage_request="$(bootstrap_request "$user_usage" "$usage_import_id" "$device_usage" 'Usage Bootstrap')"
start_usage_writer_blocker "$user_usage" "$usage_payload_json"
usage_bootstrap_app="sgbr-usage-bootstrap-$run_id"
known_apps="$known_apps $usage_bootstrap_app"
run_bootstrap "$usage_bootstrap_app" "$user_usage" "$usage_import_id" "$usage_request" \
  >"$run_dir/usage-bootstrap.out" 2>&1 &
usage_bootstrap_pid=$!
[[ -n "$usage_bootstrap_pid" ]] || fail 'usage-race bootstrap PID is empty'
active_client_pids+=("$usage_bootstrap_pid")
wait_for_lock_waiters 1 "$usage_bootstrap_app"
release_lock_blocker
if ! wait "$usage_bootstrap_pid"; then fail 'usage-first bootstrap caller failed'; fi
untrack_client_pid "$usage_bootstrap_pid"
[[ "$(result_status "$run_dir/usage-bootstrap.out")" == 'active_account' ]] \
  || fail 'usage-first bootstrap did not return active_account'
python3 - "$run_dir/usage-bootstrap.out" <<'PY'
import json
import sys
from pathlib import Path

result = json.loads([line for line in Path(sys.argv[1]).read_text().splitlines() if line.strip()][-1])
success_fields = ("shop_state", "planet_state", "effect_timeline", "canonical_contribution", "journal_confirmation", "ack")
if any(field in result for field in success_fields):
    raise SystemExit("shop guest import v2 race: active_account returned an imported success projection")
if result.get("schema_version") != 2 or result.get("status") != "active_account":
    raise SystemExit("shop guest import v2 race: active_account response lost its schema-2 correlation")
PY
[[ "$(receipt_count "$user_usage" "$usage_import_id")" == '1' ]] \
  || fail 'usage-first race did not persist exactly one private active receipt'
[[ "$(receipt_status "$user_usage" "$usage_import_id")" == 'active_account' ]] \
  || fail 'usage-first race receipt status is not active_account'
imported_receipts="$(psql_test shop-bootstrap-observer -v user_id="$user_usage" <<'SQL'
select count(*)::text from private.shop_guest_bootstrap_receipt
where user_id = :'user_id'::uuid and status = 'imported';
SQL
)" || fail 'could not verify imported receipt count after the usage race'
[[ "$imported_receipts" == '0' ]] || fail 'usage-first race unexpectedly wrote an imported receipt'
psql_test sgbr-usage-state-after -v user_id="$user_usage" \
  >"$run_dir/usage-state-after.out" <<'SQL'
set role authenticated;
select set_config('request.jwt.claim.sub', :'user_id', false);
select public.get_my_planet_state();
SQL
psql_test sgbr-usage-counts-after -v user_id="$user_usage" \
  >"$run_dir/usage-counts-after.out" <<'SQL'
select (select count(*) from public.planet_member_state where user_id = :'user_id'::uuid)::text || '|' ||
       (select count(*) from private.shop_account_state where user_id = :'user_id'::uuid)::text || '|' ||
       (select count(*) from private.growth_journal_state where user_id = :'user_id'::uuid)::text || '|' ||
       (select count(*) from private.growth_journal_cycles where user_id = :'user_id'::uuid)::text;
SQL
assert_usage_state_and_counts_preserved \
  "$run_dir/usage-blocker.out" "$run_dir/usage-state-after.out" "$run_dir/usage-counts-after.out"
printf 'shop guest import v2 race: usage-first bootstrap-lock race passed\n'
printf 'shop guest import v2 race: all four private bootstrap concurrency checks passed\n'
