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
  printf 'shop concurrency: %s\n' "$1" >&2
  exit 1
}

command -v docker >/dev/null 2>&1 || fail 'docker is unavailable'
command -v python3 >/dev/null 2>&1 || fail 'python3 is unavailable'
[[ -f "$TEST_CONFIG" ]] || fail 'isolated project config is missing'

docker_endpoint="$(docker context inspect "$(docker context show)" \
  --format '{{(index .Endpoints "docker").Host}}' 2>/dev/null)" \
  || fail 'could not verify the local Docker context'
[[ "$docker_endpoint" == unix://* ]] || fail 'refusing a non-local Docker endpoint'

python3 - "$TEST_CONFIG" "$TEST_PROJECT_ID" "$TEST_DB_PORT" <<'PY'
import sys
import tomllib
from pathlib import Path

config_path, expected_project, expected_port = sys.argv[1:]
config = tomllib.loads(Path(config_path).read_text())
if config.get("project_id") != expected_project:
    raise SystemExit("shop concurrency: isolated project ID does not match")
if str(config.get("db", {}).get("port")) != expected_port:
    raise SystemExit("shop concurrency: isolated project database port does not match")
PY

container_id="$(docker inspect --format '{{.Id}}' "$TEST_DB_CONTAINER" 2>/dev/null)" \
  || fail 'the pinned isolated database container is unavailable'
[[ -n "$container_id" ]] || fail 'the pinned isolated database container ID is empty'
container_running="$(docker inspect --format '{{.State.Running}}' "$TEST_DB_CONTAINER")"
[[ "$container_running" == 'true' ]] || fail 'the pinned isolated database container is not running'
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
    raise SystemExit("shop concurrency: pinned container data volume does not match")
bindings = ports.get("5432/tcp") or []
if not any(binding.get("HostPort") == expected_port for binding in bindings):
    raise SystemExit("shop concurrency: pinned container is not bound to the expected database port")
PY

psql_test() {
  local application_name="$1"
  shift
  docker exec --env "PGAPPNAME=$application_name" -i "$TEST_DB_CONTAINER" \
    psql -X -q -A -t "$@" -U postgres -d postgres
}

server_identity="$(psql_test shop-race-preflight \
  -c "select current_database() || '|' || current_user || '|' || current_setting('port')")" \
  || fail 'could not connect to the pinned isolated database'
[[ "$server_identity" == 'postgres|postgres|5432' ]] \
  || fail 'connected database identity does not match the expected local Supabase database'

safe_target='yes'
race_user_id="$(python3 -c 'import uuid; print(uuid.uuid4())')"
race_device_id="$(python3 -c 'import uuid; print(uuid.uuid4())')"
race_run_id="$(python3 -c 'import uuid; print(uuid.uuid4())')"
race_cycle_id="shop-race-$race_run_id"
race_credit_cycle_id="$race_cycle_id-credit"
blocker_app="shop-race-blocker-$race_run_id"
racer_a_app="shop-race-a-$race_run_id"
racer_b_app="shop-race-b-$race_run_id"
run_dir="$(mktemp -d "${TMPDIR:-/tmp}/shop-race.XXXXXXXX")" || fail 'could not create a private result directory'
blocker_backend_pid=''
blocker_client_pid=''

cleanup() {
  local exit_code=$?
  trap - EXIT
  if [[ "${safe_target:-no}" == 'yes' ]]; then
    if [[ -n "${blocker_app:-}" ]]; then
      psql_test shop-race-cleanup \
        -c "select pg_terminate_backend(pid) from pg_stat_activity where application_name = '$blocker_app' and pid <> pg_backend_pid()" \
        >/dev/null 2>&1 || true
    fi
    if [[ -n "${blocker_client_pid:-}" ]]; then
      wait "$blocker_client_pid" >/dev/null 2>&1 || true
    fi
    if [[ -n "${race_user_id:-}" ]]; then
      psql_test shop-race-cleanup -v user_id="$race_user_id" \
        >/dev/null 2>&1 <<'SQL' || true

delete from auth.users where id = :'user_id'::uuid;
SQL
    fi
  fi
  if [[ -n "${run_dir:-}" && -d "$run_dir" ]]; then
    rm -rf -- "$run_dir"
  fi
  exit "$exit_code"
}
trap cleanup EXIT

psql_test shop-race-fixture \
  -v ON_ERROR_STOP=1 \
  -v user_id="$race_user_id" \
  -v device_id="$race_device_id" \
  -v cycle_id="$race_cycle_id" \
  -v credit_cycle_id="$race_credit_cycle_id" \
  >/dev/null <<'SQL'
insert into auth.users(id) values (:'user_id'::uuid);
insert into private.planet_wallet_credits(user_id, previous_cycle_id, amount, created_at)
values (:'user_id'::uuid, :'credit_cycle_id', 100000000, '2026-09-30T00:00:00Z');
select set_config('request.jwt.claim.sub', :'user_id', false) as claim \gset
select public.upsert_my_planet_state(
  jsonb_build_object(
    'version', 1,
    'profile', jsonb_build_object('nickname', 'shop-race', 'avatar', 'feminine'),
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
    'device_id', :'device_id',
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
SQL

run_seed_purchase() {
  local request_id="$1"
  local status
  status="$(psql_test shop-race-seed \
    -v ON_ERROR_STOP=1 -v user_id="$race_user_id" -v request_id="$request_id" <<'SQL'
select set_config('request.jwt.claim.sub', :'user_id', false) as claim \gset
select public.apply_shop_action(jsonb_build_object(
  'kind', 'purchase',
  'request_id', :'request_id',
  'quote', public.quote_shop_action('{"kind":"purchase","sku":"land_market"}'::jsonb)
))->>'status';
SQL
  )" || fail 'a funded seed purchase failed'
  [[ "$status" == 'purchased' ]] || fail 'expected four successful seed purchases'
}

for slot in 1 2 3 4; do
  run_seed_purchase "race-seed-$slot-$race_run_id"
done

race_quote="$(psql_test shop-race-quote -v ON_ERROR_STOP=1 -v user_id="$race_user_id" <<'SQL'
select set_config('request.jwt.claim.sub', :'user_id', false) as claim \gset
select public.quote_shop_action('{"kind":"purchase","sku":"land_market"}'::jsonb);
SQL
)" || fail 'could not obtain the shared last-slot quote'
[[ -n "$race_quote" ]] || fail 'the shared last-slot quote is empty'

psql_test "$blocker_app" -v ON_ERROR_STOP=1 -v user_id="$race_user_id" \
  >"$run_dir/blocker.out" 2>&1 <<'SQL' &
begin;
select user_id from private.shop_account_lock where user_id = :'user_id'::uuid for update;
select pg_sleep(120);
commit;
SQL
blocker_client_pid=$!

blocker_backend_pid=''
for _ in {1..100}; do
  blocker_backend_pid="$(psql_test shop-race-observer \
    -c "select pid from pg_stat_activity where application_name = '$blocker_app' and state = 'active' and query like 'select pg_sleep(120)%' limit 1")"
  [[ -n "$blocker_backend_pid" ]] && break
  sleep 0.1
done
[[ -n "$blocker_backend_pid" ]] || fail 'blocker session did not acquire the account serialization row'

run_racer() {
  local application_name="$1"
  local request_id="$2"
  local output_path="$3"
  psql_test "$application_name" \
    -v ON_ERROR_STOP=1 \
    -v user_id="$race_user_id" \
    -v request_id="$request_id" \
    -v quote="$race_quote" \
    >"$output_path" 2>&1 <<'SQL'
select set_config('request.jwt.claim.sub', :'user_id', false) as claim \gset
select public.apply_shop_action(jsonb_build_object(
  'kind', 'purchase',
  'request_id', :'request_id',
  'quote', :'quote'::jsonb
))->>'status';
SQL
}

run_racer "$racer_a_app" "race-last-a-$race_run_id" "$run_dir/a.status" &
racer_a_pid=$!
run_racer "$racer_b_app" "race-last-b-$race_run_id" "$run_dir/b.status" &
racer_b_pid=$!

wait_count='0'
for _ in {1..100}; do
  wait_count="$(psql_test shop-race-observer \
    -c "with recursive racers as (
      select pid from pg_stat_activity
      where application_name in ('$racer_a_app', '$racer_b_app') and wait_event_type = 'Lock'
    ), wait_chain(root_pid, current_pid, path, depth) as (
      select pid, pid, array[pid], 0 from racers
      union all
      select chain.root_pid, blockers.pid, chain.path || blockers.pid, chain.depth + 1
      from wait_chain chain
      cross join lateral unnest(pg_blocking_pids(chain.current_pid)) as blockers(pid)
      where chain.depth < 16 and not blockers.pid = any(chain.path)
    )
    select count(distinct root_pid) from wait_chain where current_pid = $blocker_backend_pid")"
  [[ "$wait_count" == '2' ]] && break
  sleep 0.1
done
if [[ "$wait_count" != '2' ]]; then
  wait_graph="$(psql_test shop-race-observer \
    -c "select coalesce(jsonb_agg(jsonb_build_object(
      'application_name', application_name,
      'pid', pid,
      'wait_event_type', wait_event_type,
      'wait_event', wait_event,
      'blocking_pids', pg_blocking_pids(pid)
    )), '[]'::jsonb) from pg_stat_activity
    where application_name in ('$blocker_app', '$racer_a_app', '$racer_b_app')")"
  printf 'Race wait proof failed; safe wait graph: %s\n' "$wait_graph" >&2
  fail 'both independent racers were not observed with wait chains reaching the account lock blocker'
fi
printf 'Observed %s independent racer wait chains reaching the account-lock blocker.\n' "$wait_count"

blocker_release="$(psql_test shop-race-observer \
  -c "select pg_terminate_backend($blocker_backend_pid)")" \
  || fail 'could not release the test-only account lock blocker'
[[ "$blocker_release" == 't' ]] || fail 'the test-only account lock blocker was not terminated'
blocker_backend_pid=''
wait "$blocker_client_pid" >/dev/null 2>&1 || true
blocker_client_pid=''

wait "$racer_a_pid" || fail 'first independent purchase session failed'
wait "$racer_b_pid" || fail 'second independent purchase session failed'
status_a="$(awk 'NF {value = $0} END {print value}' "$run_dir/a.status")"
status_b="$(awk 'NF {value = $0} END {print value}' "$run_dir/b.status")"
if ! { [[ "$status_a" == 'purchased' && "$status_b" == 'limit_reached' ]] \
    || [[ "$status_b" == 'purchased' && "$status_a" == 'limit_reached' ]]; }; then
  fail 'last-slot race must return one purchased and one limit_reached result'
fi

summary="$(psql_test shop-race-summary -v ON_ERROR_STOP=1 -v user_id="$race_user_id" <<'SQL'
select jsonb_build_object(
  'owned_count', (select count(*) from private.shop_landscape_instance
    where user_id = :'user_id'::uuid and sku = 'land_market'),
  'purchase_count', (select count(*) from private.shop_purchase
    where user_id = :'user_id'::uuid and sku = 'land_market'),
  'available_balance', private.shop_available_balance(:'user_id'::uuid)
);
SQL
)" || fail 'could not verify the post-race account totals'
python3 - "$summary" <<'PY'
import json
import sys

result = json.loads(sys.argv[1])
expected = {"owned_count": 5, "purchase_count": 5, "available_balance": 75000000}
if result != expected:
    raise SystemExit("shop concurrency: final ownership, debit, or balance did not match five purchases")
PY

printf 'Observed both racer sessions waiting on one account-row blocker.\n'
printf 'Race statuses: %s, %s\n' "$status_a" "$status_b"
printf 'Verified: 5 owned, 5 purchases, 75,000,000 available from the 100,000,000 fixture\n'
