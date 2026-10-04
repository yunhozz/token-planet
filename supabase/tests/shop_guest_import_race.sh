#!/usr/bin/env bash
set -euo pipefail
set +x

# Concurrency coverage for the held guest-import boundary. This script never
# starts, resets, or cleans a project; it uses only random synthetic accounts
# in the pinned local Supabase test database.
readonly TEST_PROJECT_ID='token-planet-shop-revamp-test'
readonly TEST_WORKDIR='/private/tmp/token-planet-shop-revamp-test'
readonly TEST_CONFIG="$TEST_WORKDIR/supabase/config.toml"
readonly TEST_DB_PORT='55432'
readonly TEST_DB_CONTAINER="supabase_db_$TEST_PROJECT_ID"
readonly TEST_DB_VOLUME="supabase_db_$TEST_PROJECT_ID"

fail() {
  printf 'shop guest import race: %s\n' "$1" >&2
  exit 1
}

command -v docker >/dev/null 2>&1 || fail 'docker is unavailable'
command -v python3 >/dev/null 2>&1 || fail 'python3 is unavailable'
command -v rg >/dev/null 2>&1 || fail 'rg is unavailable'
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
    raise SystemExit("shop guest import race: disposable project ID does not match")
if str(config.get("db", {}).get("port")) != expected_port:
    raise SystemExit("shop guest import race: disposable database port does not match")
PY

container_id="$(docker inspect --format '{{.Id}}' "$TEST_DB_CONTAINER" 2>/dev/null)" \
  || fail 'the pinned disposable database container is unavailable'
[[ -n "$container_id" ]] || fail 'the pinned database container ID is empty'
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
    raise SystemExit("shop guest import race: pinned database volume does not match")
bindings = ports.get("5432/tcp") or []
if not any(binding.get("HostPort") == expected_port for binding in bindings):
    raise SystemExit("shop guest import race: pinned database port does not match")
PY

psql_test() {
  local application_name="$1"
  shift
  docker exec --env "PGAPPNAME=$application_name" -i "$TEST_DB_CONTAINER" \
    psql -X -q -A -t "$@" -U postgres -d postgres
}

observer_app="shop-guest-import-observer-$(python3 -c 'import uuid; print(uuid.uuid4())')"
server_identity="$(psql_test "$observer_app" \
  -c "select current_database() || '|' || current_user || '|' || current_setting('port')")" \
  || fail 'could not connect to the pinned local database'
[[ "$server_identity" == 'postgres|postgres|5432' ]] \
  || fail 'connected database identity does not match the pinned local Supabase database'

safe_target='no'
run_id="$(python3 -c 'import uuid; print(uuid.uuid4())')"
user_same="$(python3 -c 'import uuid; print(uuid.uuid4())')"
user_conflict="$(python3 -c 'import uuid; print(uuid.uuid4())')"
user_profile="$(python3 -c 'import uuid; print(uuid.uuid4())')"
same_import_id="$(python3 -c 'import uuid; print(uuid.uuid4())')"
conflict_import_id="$(python3 -c 'import uuid; print(uuid.uuid4())')"
profile_import_id="$(python3 -c 'import uuid; print(uuid.uuid4())')"
device_same="$(python3 -c 'import uuid; print(uuid.uuid4())')"
device_conflict="$(python3 -c 'import uuid; print(uuid.uuid4())')"
device_profile="$(python3 -c 'import uuid; print(uuid.uuid4())')"
test_users="$user_same $user_conflict $user_profile"
known_apps="$observer_app"
run_dir=''
blocker_app=''
blocker_client_pid=''
active_client_pids=()
cleanup_failed='no'

cleanup() {
  local exit_code=$?
  local active_count=''
  trap - EXIT INT TERM
  exec 3>&- 2>/dev/null || true

  if [[ "$safe_target" == 'yes' ]]; then
    for app_name in $known_apps; do
      psql_test shop-guest-import-cleanup \
        -c "select pg_terminate_backend(pid) from pg_stat_activity where application_name = '$app_name' and pid <> pg_backend_pid()" \
        >/dev/null 2>&1 || cleanup_failed='yes'
    done
    if [[ -n "${active_client_pids[*]-}" ]]; then
      for client_pid in "${active_client_pids[@]}"; do
        kill "$client_pid" >/dev/null 2>&1 || true
      done
    fi
    if [[ -n "$blocker_client_pid" ]]; then
      kill "$blocker_client_pid" >/dev/null 2>&1 || true
    fi
    if [[ -n "${active_client_pids[*]-}" ]]; then
      for client_pid in "${active_client_pids[@]}"; do
        wait "$client_pid" >/dev/null 2>&1 || true
      done
    fi
    if [[ -n "$blocker_client_pid" ]]; then
      wait "$blocker_client_pid" >/dev/null 2>&1 || true
    fi
    for app_name in $known_apps; do
      active_count="$(psql_test shop-guest-import-cleanup \
        -c "select count(*)::text from pg_stat_activity where application_name = '$app_name'")" \
        || cleanup_failed='yes'
      [[ "$active_count" == '0' ]] || cleanup_failed='yes'
    done
    for user_id in $test_users; do
      psql_test shop-guest-import-cleanup \
        -v ON_ERROR_STOP=1 \
        -c "delete from auth.users where id = '$user_id'::uuid" \
        >/dev/null 2>&1 || cleanup_failed='yes'
      active_count="$(psql_test shop-guest-import-cleanup \
        -c "select count(*)::text from auth.users where id = '$user_id'::uuid")" \
        || cleanup_failed='yes'
      [[ "$active_count" == '0' ]] || cleanup_failed='yes'
    done
  fi

  if [[ -n "$run_dir" && -d "$run_dir" ]]; then
    rm -rf -- "$run_dir"
  fi
  if [[ "$cleanup_failed" != 'no' ]]; then
    printf 'shop guest import race: cleanup could not verify zero test sessions and users\n' >&2
    exit_code=1
  elif [[ "$safe_target" == 'yes' ]]; then
    printf 'shop guest import race: cleanup verified zero owned sessions and synthetic auth users\n'
  fi
  exit "$exit_code"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

run_dir="$(mktemp -d '/private/tmp/shop-guest-import-race.XXXXXXXX')" \
  || fail 'could not create a private result directory'

guest_request() {
  python3 - "$1" "$2" "$3" "$4" <<'PY'
import json
import sys

user_id, import_id, revision, device_id = sys.argv[1:]
data = {
    "world_timezone": "UTC",
    "planet_timezone": "UTC",
    "reward_timezone": "UTC",
    "planet_device_id": device_id,
    "shop_state_revision": int(revision),
    "profile": None,
    "activation_at_utc": "2026-10-01T00:00:00+00:00",
    "current_cycle": {
        "cycle_id": "guest-import-race-cycle",
        "started_at_utc": "2026-10-01T00:00:00+00:00",
        "ended_at_utc": None,
        "is_current": True,
        "settled_bonus_tokens": None,
    },
    "historical_cycles": [],
    "last_reset_at_utc": None,
    "reset_available_at_utc": None,
    "effect_timeline_state": None,
    "effect_cycle_bounds_authoritative": False,
    "contribution_canonical_version": None,
    "natural_objects": [],
    "landscape_instances": [],
    "placements": [],
    "landscape_edit_versions": [],
    "avatar_owned": [],
    "avatar_equipment": [],
    "cosmetic_equipment": [],
    "pending_purchases": [],
    "cosmetic_purchases": [],
    "purchases": [],
    "purchase_proofs": [],
    "natural_removals": [],
    "removal_debits": [],
    "removal_proofs": [],
    "effect_history": [],
    "effect_cycle_bounds": [],
    "effect_contributions": [],
    "activity_days": [],
    "game_rewards": [],
    "wallet_credits": [],
    "unverified_planet_wallet_claims": [],
    "cycle_settlements": [],
    "era_progress": [],
    "daily_agent_totals": [],
    "usage_aggregates": [],
    "lifetime_usage_tokens": None,
    "current_cycle_usage_tokens": None,
    "cycle_usage_totals": [],
    "growth_journal_state": None,
    "growth_journal_cycles": [],
    "growth_journal_entries": [],
    "reset_settlement_proofs": [],
    "integrity_issues": [],
    "reset_receipts_unverifiable": False,
    "legacy_partial_import_pending": False,
}
request = {
    "schema_version": 1,
    "snapshot": {
        "import_id": import_id,
        "target_account_id": f"account:{user_id}",
        "source_account_id": "local",
        "source_fingerprint": "a" * 64,
        "disposition": "local_integrity_validated",
        "data": data,
    },
}
print(json.dumps(request, separators=(",", ":")))
PY
}

insert_test_users() {
  local existing_users
  existing_users="$(psql_test shop-guest-import-fixture-check \
    -v user_same="$user_same" -v user_conflict="$user_conflict" -v user_profile="$user_profile" <<'SQL'
select count(*)::text from auth.users
where id in (:'user_same'::uuid, :'user_conflict'::uuid, :'user_profile'::uuid);
SQL
  )" \
    || fail 'could not verify the synthetic auth IDs are unused'
  [[ "$existing_users" == '0' ]] || fail 'a generated synthetic auth ID already exists'
  psql_test shop-guest-import-fixture -v ON_ERROR_STOP=1 \
    -v user_same="$user_same" -v user_conflict="$user_conflict" -v user_profile="$user_profile" \
    >/dev/null <<'SQL' || fail 'could not create synthetic auth accounts'
insert into auth.users(id) values
  (:'user_same'::uuid), (:'user_conflict'::uuid), (:'user_profile'::uuid);
SQL
  safe_target='yes'
}

seed_account_lock() {
  psql_test shop-guest-import-lock-seed -v ON_ERROR_STOP=1 -v user_id="$1" \
    >/dev/null <<'SQL' || fail 'could not prepare a synthetic account lock row'
insert into private.shop_account_lock(user_id) values (:'user_id'::uuid) on conflict do nothing;
SQL
}

start_lock_blocker() {
  local user_id="$1"
  local label="$2"
  local fifo_path="$run_dir/$label-blocker.in"
  blocker_app="gimp-$label-$run_id"
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
select 'GUEST_IMPORT_LOCK_READY';
SQL
  for _ in {1..100}; do
    if rg -q 'GUEST_IMPORT_LOCK_READY' "$run_dir/$label-blocker.out"; then
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
  for client_pid in "${active_client_pids[@]}"; do
    [[ "$client_pid" == "$target_pid" ]] || remaining_pids+=("$client_pid")
  done
  active_client_pids=()
  if ((${#remaining_pids[@]})); then
    active_client_pids=("${remaining_pids[@]}")
  fi
}

release_lock_blocker() {
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

write_profile_and_release_blocker() {
  cat <<'SQL' >&3
insert into public.planet_member_state(
  user_id, nickname, avatar, timezone, current_cycle_id, cycle_started_at,
  current_planet_tokens, lifetime_tokens, growth_credit, stage, progress_to_next,
  incomplete, objects
) values (
  :'user_id'::uuid, 'race-profile-writer', 'feminine', 'UTC', 'writer-cycle',
  '2026-10-01T00:00:00Z', 0, 0, 0, 0, 0, false, '[]'::jsonb
);
commit;
\q
SQL
  exec 3>&-
  if ! wait "$blocker_client_pid"; then
    fail 'the synthetic profile writer failed'
  fi
  untrack_client_pid "$blocker_client_pid"
  blocker_client_pid=''
}

run_import() {
  local application_name="$1"
  local user_id="$2"
  local import_id="$3"
  local request_json="$4"
  psql_test "$application_name" \
    -v ON_ERROR_STOP=1 \
    -v user_id="$user_id" \
    -v import_id="$import_id" \
    -v request_json="$request_json" <<'SQL'
set statement_timeout = '45s';
set role authenticated;
select set_config('request.jwt.claim.sub', :'user_id', false) \gset
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
    waiting_count="$(psql_test shop-guest-import-observer \
      -c "select count(*)::text from pg_stat_activity where application_name in ($app_names) and wait_event_type = 'Lock'")" \
      || fail 'could not observe guest import lock waiters'
    if [[ "$waiting_count" == "$expected_count" ]]; then
      return
    fi
    sleep 0.1
  done
  printf 'shop guest import race: lock waiter diagnostics for %s\n' "$app_names" >&2
  psql_test shop-guest-import-observer \
    -c "select application_name || '|' || state || '|' || coalesce(wait_event_type, '') || '|' || left(query, 160) from pg_stat_activity where application_name in ($app_names)" \
    >&2 || true
  for app_name in "$@"; do
    case "$app_name" in
      gims-a-*) cat "$run_dir/same-a.out" >&2 2>/dev/null || true ;;
      gims-b-*) cat "$run_dir/same-b.out" >&2 2>/dev/null || true ;;
      gimd-a-*) cat "$run_dir/different-a.out" >&2 2>/dev/null || true ;;
      gimd-b-*) cat "$run_dir/different-b.out" >&2 2>/dev/null || true ;;
      gim-profile-*) cat "$run_dir/profile-import.out" >&2 2>/dev/null || true ;;
    esac
  done
  fail "expected $expected_count import lock waiter(s), found $waiting_count"
}

assert_receipt_count() {
  local user_id="$1"
  local import_id="$2"
  local expected="$3"
  local count
  count="$(psql_test shop-guest-import-observer \
    -v user_id="$user_id" -v import_id="$import_id" <<'SQL'
select count(*)::text from private.shop_guest_import_request
where user_id = :'user_id'::uuid and import_id = :'import_id'::uuid;
SQL
  )" \
    || fail 'could not read the private synthetic receipt count'
  [[ "$count" == "$expected" ]] || fail "expected $expected receipt row(s), found $count"
}

assert_same_held_results() {
  python3 - "$1" "$2" <<'PY'
import json
import sys
from pathlib import Path

results = [json.loads(Path(path).read_text().strip().splitlines()[-1]) for path in sys.argv[1:]]
if results[0] != results[1] or results[0].get("status") != "source_unverifiable":
    raise SystemExit("shop guest import race: same-payload callers did not receive one held result")
print("same-ID same-payload race: both callers received the identical source_unverifiable result")
PY
}

assert_conflict_results() {
  python3 - "$1" "$2" <<'PY'
import json
import sys
from pathlib import Path

statuses = sorted(json.loads(Path(path).read_text().strip().splitlines()[-1]).get("status") for path in sys.argv[1:])
if statuses != ["request_conflict", "source_unverifiable"]:
    raise SystemExit("shop guest import race: different payloads did not yield held+conflict")
print("same-ID different-payload race: one source_unverifiable result and one request_conflict")
PY
}

insert_test_users

same_request="$(guest_request "$user_same" "$same_import_id" 0 "$device_same")"
seed_account_lock "$user_same"
start_lock_blocker "$user_same" same-payload
same_app_a="gims-a-$run_id"
same_app_b="gims-b-$run_id"
known_apps="$known_apps $same_app_a $same_app_b"
run_import "$same_app_a" "$user_same" "$same_import_id" "$same_request" \
  >"$run_dir/same-a.out" 2>&1 &
same_pid_a=$!
active_client_pids+=("$same_pid_a")
run_import "$same_app_b" "$user_same" "$same_import_id" "$same_request" \
  >"$run_dir/same-b.out" 2>&1 &
same_pid_b=$!
active_client_pids+=("$same_pid_b")
wait_for_lock_waiters 2 "$same_app_a" "$same_app_b"
release_lock_blocker
if ! wait "$same_pid_a"; then fail 'same-payload import caller A failed'; fi
untrack_client_pid "$same_pid_a"
if ! wait "$same_pid_b"; then fail 'same-payload import caller B failed'; fi
untrack_client_pid "$same_pid_b"
assert_same_held_results "$run_dir/same-a.out" "$run_dir/same-b.out"
assert_receipt_count "$user_same" "$same_import_id" 1

conflict_request_a="$(guest_request "$user_conflict" "$conflict_import_id" 0 "$device_conflict")"
conflict_request_b="$(guest_request "$user_conflict" "$conflict_import_id" 1 "$device_conflict")"
seed_account_lock "$user_conflict"
start_lock_blocker "$user_conflict" different-payload
conflict_app_a="gimd-a-$run_id"
conflict_app_b="gimd-b-$run_id"
known_apps="$known_apps $conflict_app_a $conflict_app_b"
run_import "$conflict_app_a" "$user_conflict" "$conflict_import_id" "$conflict_request_a" \
  >"$run_dir/different-a.out" 2>&1 &
conflict_pid_a=$!
active_client_pids+=("$conflict_pid_a")
run_import "$conflict_app_b" "$user_conflict" "$conflict_import_id" "$conflict_request_b" \
  >"$run_dir/different-b.out" 2>&1 &
conflict_pid_b=$!
active_client_pids+=("$conflict_pid_b")
wait_for_lock_waiters 2 "$conflict_app_a" "$conflict_app_b"
release_lock_blocker
if ! wait "$conflict_pid_a"; then fail 'different-payload import caller A failed'; fi
untrack_client_pid "$conflict_pid_a"
if ! wait "$conflict_pid_b"; then fail 'different-payload import caller B failed'; fi
untrack_client_pid "$conflict_pid_b"
assert_conflict_results "$run_dir/different-a.out" "$run_dir/different-b.out"
assert_receipt_count "$user_conflict" "$conflict_import_id" 1

profile_request="$(guest_request "$user_profile" "$profile_import_id" 0 "$device_profile")"
seed_account_lock "$user_profile"
start_lock_blocker "$user_profile" profile-writer
profile_app="gim-profile-$run_id"
known_apps="$known_apps $profile_app"
run_import "$profile_app" "$user_profile" "$profile_import_id" "$profile_request" \
  >"$run_dir/profile-import.out" 2>&1 &
profile_pid=$!
active_client_pids+=("$profile_pid")
wait_for_lock_waiters 1 "$profile_app"
write_profile_and_release_blocker
if ! wait "$profile_pid"; then fail 'profile-race import caller failed'; fi
untrack_client_pid "$profile_pid"
python3 - "$run_dir/profile-import.out" <<'PY'
import json
import sys
from pathlib import Path

result = json.loads(Path(sys.argv[1]).read_text().strip().splitlines()[-1])
if result.get("status") != "active_account":
    raise SystemExit("shop guest import race: waiting import did not observe the committed profile")
print("profile-writer race: waiting import returned active_account after the shared lock released")
PY
assert_receipt_count "$user_profile" "$profile_import_id" 1
profile_name="$(psql_test shop-guest-import-observer -v user_id="$user_profile" <<'SQL'
select nickname from public.planet_member_state where user_id = :'user_id'::uuid;
SQL
)" \
  || fail 'could not verify the synthetic profile after the race'
[[ "$profile_name" == 'race-profile-writer' ]] || fail 'the import changed the synthetic profile'

fresh_game_rows="$(psql_test shop-guest-import-observer -v user_id="$user_same" <<'SQL'
select (select count(*) from public.planet_member_state where user_id = :'user_id'::uuid)
  + (select count(*) from private.shop_account_state where user_id = :'user_id'::uuid)
  + (select count(*) from private.planet_wallet_credits where user_id = :'user_id'::uuid)
  + (select count(*) from private.shop_purchase where user_id = :'user_id'::uuid);
SQL
)" \
  || fail 'could not verify no game-state writes for the fresh race account'
[[ "$fresh_game_rows" == '0' ]] || fail 'a fresh-account import race wrote game state'
printf 'shop guest import race: all held-import concurrency checks passed\n'
