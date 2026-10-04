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
  printf 'shop reward lock race: %s\n' "$1" >&2
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
    raise SystemExit("shop reward lock race: isolated project ID does not match")
if str(config.get("db", {}).get("port")) != expected_port:
    raise SystemExit("shop reward lock race: isolated project database port does not match")
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
    raise SystemExit("shop reward lock race: pinned container data volume does not match")
bindings = ports.get("5432/tcp") or []
if not any(binding.get("HostPort") == expected_port for binding in bindings):
    raise SystemExit("shop reward lock race: pinned container is not bound to the expected database port")
PY

psql_test() {
  local application_name="$1"
  shift
  docker exec --env "PGAPPNAME=$application_name" -i "$TEST_DB_CONTAINER" \
    psql -X -q -A -t "$@" -U postgres -d postgres
}

server_identity="$(psql_test shop-reward-race-preflight \
  -c "select current_database() || '|' || current_user || '|' || current_setting('port')")" \
  || fail 'could not connect to the pinned isolated database'
[[ "$server_identity" == 'postgres|postgres|5432' ]] \
  || fail 'connected database identity does not match the expected local Supabase database'

safe_target='yes'
race_user_id="$(python3 -c 'import uuid; print(uuid.uuid4())')"
race_device_id="$(python3 -c 'import uuid; print(uuid.uuid4())')"
race_run_id="$(python3 -c 'import uuid; print(uuid.uuid4())')"
race_cycle_id="reward-lock-$race_run_id"
blocker_app="shop-reward-blocker-$race_run_id"
waiter_app="shop-reward-waiter-$race_run_id"
run_dir="$(mktemp -d "${TMPDIR:-/tmp}/shop-reward-race.XXXXXXXX")" \
  || fail 'could not create a private result directory'
blocker_client_pid=''
waiter_client_pid=''
blocker_backend_pid=''
waiter_backend_pid=''

cleanup() {
  local exit_code=$?
  trap - EXIT
  if [[ "${safe_target:-no}" == 'yes' ]]; then
    for application_name in "${blocker_app:-}" "${waiter_app:-}"; do
      if [[ -n "$application_name" ]]; then
        psql_test shop-reward-race-cleanup \
          -c "select pg_terminate_backend(pid) from pg_stat_activity where application_name = '$application_name' and pid <> pg_backend_pid()" \
          >/dev/null 2>&1 || true
      fi
    done
    if [[ -n "${blocker_client_pid:-}" ]]; then
      kill "$blocker_client_pid" >/dev/null 2>&1 || true
    fi
    if [[ -n "${waiter_client_pid:-}" ]]; then
      kill "$waiter_client_pid" >/dev/null 2>&1 || true
    fi
    exec 3>&- 2>/dev/null || true
    if [[ -n "${blocker_client_pid:-}" ]]; then
      wait "$blocker_client_pid" >/dev/null 2>&1 || true
    fi
    if [[ -n "${waiter_client_pid:-}" ]]; then
      wait "$waiter_client_pid" >/dev/null 2>&1 || true
    fi
    if [[ -n "${race_user_id:-}" ]]; then
      psql_test shop-reward-race-cleanup -v user_id="$race_user_id" \
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

psql_test shop-reward-race-fixture \
  -v ON_ERROR_STOP=1 \
  -v user_id="$race_user_id" \
  -v device_id="$race_device_id" \
  -v cycle_id="$race_cycle_id" \
  >/dev/null <<'SQL'
insert into auth.users(id) values (:'user_id'::uuid);
select set_config('request.jwt.claim.sub', :'user_id', false) as claim \gset
select public.upsert_my_planet_state(
  jsonb_build_object(
    'version', 1,
    'profile', jsonb_build_object('nickname', 'reward-lock-race', 'avatar', 'feminine'),
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
    'canonical_version', 1,
    'daily_segments', '[]'::jsonb,
    'activity_days', '[]'::jsonb
  )
);
update public.planet_member_state set stage = 1 where user_id = :'user_id'::uuid;
delete from private.shop_effect_history where user_id = :'user_id'::uuid;
insert into private.shop_effect_history(
  user_id, cycle_id, revision, started_at, ended_at, active_instance_ids, effects
) values (
  :'user_id'::uuid, :'cycle_id', 1, clock_timestamp() - interval '1 hour', null, '[]'::jsonb,
  '{"token_earning_bps":0,"civilization_growth_bps":0,"shop_discount_bps":0,"reset_cooldown_bps":0,"natural_removal_discount_bps":0,"era_reward_tokens":10000000,"streak_reward_tokens":0}'::jsonb
);
delete from private.shop_game_reward where user_id = :'user_id'::uuid;
SQL

# Hold the exact account serialization row and keep this backend open for coordinated updates.
mkfifo "$run_dir/blocker.in" || fail 'could not create the blocker input pipe'
docker exec --env "PGAPPNAME=$blocker_app" -i "$TEST_DB_CONTAINER" \
  psql -X -q -A -t -v ON_ERROR_STOP=1 \
  -v user_id="$race_user_id" -v cycle_id="$race_cycle_id" \
  -U postgres -d postgres <"$run_dir/blocker.in" >"$run_dir/blocker.out" 2>&1 &
blocker_client_pid=$!
exec 3>"$run_dir/blocker.in"
cat <<'SQL' >&3
begin;
select user_id from private.shop_account_lock where user_id = :'user_id'::uuid for update;
select 'SHOP_REWARD_BLOCKER_LOCKED';
SQL

blocker_ready='no'
for _ in {1..100}; do
  if rg -q 'SHOP_REWARD_BLOCKER_LOCKED' "$run_dir/blocker.out"; then
    blocker_ready='yes'
    break
  fi
  sleep 0.1
done
[[ "$blocker_ready" == 'yes' ]] || fail 'blocker session did not acquire the account row lock'
blocker_backend_pid="$(psql_test shop-reward-race-observer \
  -c "select pid::text from pg_stat_activity where application_name = '$blocker_app' and state = 'idle in transaction' limit 1")" \
  || fail 'could not find the blocker backend'
[[ -n "$blocker_backend_pid" ]] || fail 'blocker backend PID is empty'

run_waiter() {
  psql_test "$waiter_app" -v ON_ERROR_STOP=1 \
    -v user_id="$race_user_id" -v cycle_id="$race_cycle_id" \
    >"$run_dir/waiter.out" 2>&1 <<'SQL'
select set_config('request.jwt.claim.sub', :'user_id', false) as claim \gset
select private.settle_shop_rewards(:'user_id'::uuid, :'cycle_id');
select 'settled';
SQL
}
run_waiter &
waiter_client_pid=$!

wait_proven='no'
for _ in {1..100}; do
  waiter_backend_pid="$(psql_test shop-reward-race-observer \
    -c "select pid::text from pg_stat_activity
      where application_name = '$waiter_app' and wait_event_type = 'Lock'
        and $blocker_backend_pid = any(pg_blocking_pids(pid)) limit 1")"
  [[ -n "$waiter_backend_pid" ]] && { wait_proven='yes'; break; }
  sleep 0.1
done
if [[ "$wait_proven" != 'yes' ]]; then
  wait_graph="$(psql_test shop-reward-race-observer \
    -c "select coalesce(jsonb_agg(jsonb_build_object(
      'application_name', application_name, 'pid', pid,
      'wait_event_type', wait_event_type, 'wait_event', wait_event,
      'blocking_pids', pg_blocking_pids(pid)
    )), '[]'::jsonb) from pg_stat_activity
    where application_name in ('$blocker_app', '$waiter_app')")"
  printf 'Lock wait proof failed; safe session data: %s\n' "$wait_graph" >&2
  fail 'the settlement waiter was not observed blocked by the account lock holder'
fi
printf 'Observed settlement waiter PID %s blocked by holder PID %s.\n' \
  "$waiter_backend_pid" "$blocker_backend_pid"

# Publish a newer zero-effect interval while the settlement call is still blocked.
cat <<'SQL' >&3
update private.shop_effect_history
set ended_at = clock_timestamp()
where user_id = :'user_id'::uuid and cycle_id = :'cycle_id' and ended_at is null;
with latest as (
  select max(revision)::bigint + 1 as revision, max(ended_at) as ended_at
  from private.shop_effect_history
  where user_id = :'user_id'::uuid and cycle_id = :'cycle_id'
)
insert into private.shop_effect_history(
  user_id, cycle_id, revision, started_at, ended_at, active_instance_ids, effects
)
select :'user_id'::uuid, :'cycle_id', latest.revision,
  latest.ended_at, null, '[]'::jsonb,
  '{"token_earning_bps":0,"civilization_growth_bps":0,"shop_discount_bps":0,"reset_cooldown_bps":0,"natural_removal_discount_bps":0,"era_reward_tokens":0,"streak_reward_tokens":0}'::jsonb
from latest;
select 'SHOP_REWARD_ZERO_INTERVAL_READY';
SQL

interval_ready='no'
for _ in {1..100}; do
  if rg -q 'SHOP_REWARD_ZERO_INTERVAL_READY' "$run_dir/blocker.out"; then
    interval_ready='yes'
    break
  fi
  sleep 0.1
done
[[ "$interval_ready" == 'yes' ]] || fail 'holder did not install the newer zero-effect interval'

cat <<'SQL' >&3
commit;
select 'SHOP_REWARD_BLOCKER_RELEASED';
SQL
exec 3>&-
wait "$blocker_client_pid" || fail 'holder session failed before releasing its transaction'
blocker_client_pid=''
wait "$waiter_client_pid" || fail 'settlement waiter failed after the account lock was released'
waiter_client_pid=''

summary="$(psql_test shop-reward-race-summary -v ON_ERROR_STOP=1 \
  -v user_id="$race_user_id" -v cycle_id="$race_cycle_id" <<'SQL'
select jsonb_build_object(
  'rows', count(*),
  'amount', coalesce(sum(amount::numeric), 0)::bigint,
  'snapshot_era_reward_tokens', coalesce(max((effect_snapshot->>'era_reward_tokens')::bigint), 0)
)
from private.shop_game_reward
where user_id = :'user_id'::uuid and trigger_key = 'era:' || :'cycle_id' || ':1';
SQL
)" || fail 'could not inspect the settlement result'
python3 - "$summary" <<'PY'
import json
import sys

result = json.loads(sys.argv[1])
expected = {"rows": 1, "amount": 0, "snapshot_era_reward_tokens": 0}
if result != expected:
    print(f"shop reward lock race: expected post-lock zero snapshot, observed {result}", file=sys.stderr)
    raise SystemExit(1)
PY

printf 'Reward settlement used the zero-effect interval visible after the account lock.\n'
