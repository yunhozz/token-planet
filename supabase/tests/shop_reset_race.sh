#!/usr/bin/env bash
set -euo pipefail
set +x

# Disposable local Supabase database race coverage. This script never starts or
# resets a project; it only uses the pinned test container and synthetic users.
readonly TEST_PROJECT_ID='token-planet-shop-revamp-test'
readonly TEST_WORKDIR='/private/tmp/token-planet-shop-revamp-test'
readonly TEST_CONFIG="$TEST_WORKDIR/supabase/config.toml"
readonly TEST_DB_PORT='55432'
readonly TEST_DB_CONTAINER="supabase_db_$TEST_PROJECT_ID"
readonly TEST_DB_VOLUME="supabase_db_$TEST_PROJECT_ID"

fail() {
  printf 'shop reset race: %s\n' "$1" >&2
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
    raise SystemExit("shop reset race: disposable project ID does not match")
if str(config.get("db", {}).get("port")) != expected_port:
    raise SystemExit("shop reset race: disposable database port does not match")
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
    raise SystemExit("shop reset race: pinned database volume does not match")
bindings = ports.get("5432/tcp") or []
if not any(binding.get("HostPort") == expected_port for binding in bindings):
    raise SystemExit("shop reset race: pinned database port does not match")
PY

psql_test() {
  local application_name="$1"
  shift
  docker exec --env "PGAPPNAME=$application_name" -i "$TEST_DB_CONTAINER" \
    psql -X -q -A -t "$@" -U postgres -d postgres
}

server_identity="$(psql_test shop-reset-race-preflight \
  -c "select current_database() || '|' || current_user || '|' || current_setting('port')")" \
  || fail 'could not connect to the pinned local database'
[[ "$server_identity" == 'postgres|postgres|5432' ]] \
  || fail 'connected database identity does not match the pinned local Supabase database'

safe_target='yes'
test_users=''
known_apps=''
run_dir=''
blocker_client_pid=''
racer_a_pid=''
racer_b_pid=''
blocker_backend_pid=''
race_reset_request=''

cleanup() {
  local exit_code=$?
  local cleanup_failed='no'
  local active_count=''
  trap - EXIT INT TERM
  exec 3>&- 2>/dev/null || true

  if [[ "$safe_target" == 'yes' ]]; then
    for app_name in $known_apps; do
      psql_test shop-reset-race-cleanup \
        -c "select pg_terminate_backend(pid) from pg_stat_activity where application_name = '$app_name' and pid <> pg_backend_pid()" \
        >/dev/null 2>&1 || cleanup_failed='yes'
    done

    for client_pid in "$blocker_client_pid" "$racer_a_pid" "$racer_b_pid"; do
      if [[ -n "$client_pid" ]]; then
        kill "$client_pid" >/dev/null 2>&1 || true
      fi
    done
    for client_pid in "$blocker_client_pid" "$racer_a_pid" "$racer_b_pid"; do
      if [[ -n "$client_pid" ]]; then
        wait "$client_pid" >/dev/null 2>&1 || true
      fi
    done

    for app_name in $known_apps; do
      active_count="$(psql_test shop-reset-race-cleanup \
        -c "select count(*)::text from pg_stat_activity where application_name = '$app_name'")" \
        || cleanup_failed='yes'
      [[ "$active_count" == '0' ]] || cleanup_failed='yes'
    done
    for user_id in $test_users; do
      psql_test shop-reset-race-cleanup \
        -c "delete from auth.users where id = '$user_id'::uuid" \
        >/dev/null 2>&1 || cleanup_failed='yes'
      active_count="$(psql_test shop-reset-race-cleanup \
        -c "select count(*)::text from auth.users where id = '$user_id'::uuid")" \
        || cleanup_failed='yes'
      [[ "$active_count" == '0' ]] || cleanup_failed='yes'
    done
  fi

  if [[ -n "$run_dir" && -d "$run_dir" ]]; then
    rm -rf -- "$run_dir"
  fi
  if [[ "$cleanup_failed" != 'no' ]]; then
    printf 'shop reset race: cleanup could not verify zero test sessions and users\n' >&2
    exit_code=1
  fi
  exit "$exit_code"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

new_race_context() {
  local race_label="$1"
  race_run_id="$(python3 -c 'import uuid; print(uuid.uuid4())')"
  race_user_id="$(python3 -c 'import uuid; print(uuid.uuid4())')"
  race_device_id="$(python3 -c 'import uuid; print(uuid.uuid4())')"
  race_cycle_id="shop-reset-$race_label-$race_run_id"
  race_funding_cycle_id="$race_cycle_id-funding"
  race_request_a="$(python3 -c 'import uuid; print(uuid.uuid4())')"
  race_request_b="$(python3 -c 'import uuid; print(uuid.uuid4())')"
  race_remove_request="$(python3 -c 'import uuid; print(uuid.uuid4())')"
  blocker_app="shop-reset-blocker-$race_run_id"
  racer_a_app="shop-reset-a-$race_run_id"
  racer_b_app="shop-reset-b-$race_run_id"
  test_users="$test_users $race_user_id"
  known_apps="$known_apps $blocker_app $racer_a_app $racer_b_app"
  run_dir="$(mktemp -d "/private/tmp/shop-reset-race.XXXXXXXX")" \
    || fail 'could not create a private result directory'
}

seed_race_account() {
  psql_test shop-reset-race-fixture \
    -v ON_ERROR_STOP=1 \
    -v user_id="$race_user_id" \
    -v device_id="$race_device_id" \
    -v cycle_id="$race_cycle_id" \
    -v funding_cycle_id="$race_funding_cycle_id" \
    >/dev/null <<'SQL' \
    || fail 'could not create the canonical funded race account'
insert into auth.users(id) values (:'user_id'::uuid);
-- Fund only this disposable account from the server-owned wallet ledger.
insert into private.planet_wallet_credits(user_id, previous_cycle_id, amount, created_at)
values (:'user_id'::uuid, :'funding_cycle_id', 1000000, '2026-09-30T00:00:00Z');
set role authenticated;
select set_config('request.jwt.claim.sub', :'user_id', false) \gset
select public.upsert_my_planet_state(
  jsonb_build_object(
    'version', 1,
    'profile', jsonb_build_object('nickname', 'shop-reset-race', 'avatar', 'feminine'),
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
  select statement_timestamp() as occurred_at,
    to_char(statement_timestamp() at time zone 'UTC', 'YYYY-MM-DD') as occurred_day
)
select public.upsert_my_planet_state(
  public.get_my_planet_state(),
  jsonb_build_object(
    'device_id', :'device_id'::uuid,
    'current_cycle_id', :'cycle_id',
    'current_planet_tokens', 100000,
    'lifetime_tokens', 100000,
    'daily_tokens', jsonb_build_object(tick.occurred_day, 100000),
    'incomplete', false,
    'canonical_version', 1,
    'daily_segments', jsonb_build_array(jsonb_build_object(
      'cycle_id', :'cycle_id', 'date', tick.occurred_day,
      'effect_revision', 0, 'tokens', 100000
    )),
    'activity_days', '[]'::jsonb
  )
)
from tick;
reset role;
SQL
}

capture_natural_quote() {
  local fixture
  fixture="$(psql_test shop-reset-natural-quote \
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
  ) as natural_key
  from planet
  cross join lateral jsonb_array_elements(planet.state->'objects') object(value)
  order by (object.value->>'stage')::integer, (object.value->>'ordinal')::integer
  limit 1
)
select jsonb_build_object(
  'key', first_object.natural_key,
  'quote', public.quote_shop_action(jsonb_build_object(
    'kind', 'remove_natural', 'key', first_object.natural_key
  ))
)
from first_object;
SQL
  )" || fail 'could not quote a current-cycle natural object'
  [[ -n "$fixture" ]] || fail 'the canonical current cycle has no natural object'
  race_natural_key="$(python3 - "$fixture" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
print(json.dumps(payload["key"], separators=(",", ":")))
PY
  )" || fail 'could not extract the natural-object key'
  race_quote="$(python3 - "$fixture" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
quote = payload.get("quote") or {}
if quote.get("price") != 100000 or quote.get("target", {}).get("key") != payload.get("key"):
    raise SystemExit("shop reset race: expected a matching stage-zero 100,000-token quote")
print(json.dumps(quote, separators=(",", ":")))
PY
  )" || fail 'the natural-object quote did not match the current fixture'
}

start_blocker() {
  mkfifo "$run_dir/blocker.in" || fail 'could not create the account-lock input pipe'
  docker exec --env "PGAPPNAME=$blocker_app" -i "$TEST_DB_CONTAINER" \
    psql -X -q -A -t -v ON_ERROR_STOP=1 -v user_id="$race_user_id" \
      -U postgres -d postgres \
    <"$run_dir/blocker.in" >"$run_dir/blocker.out" 2>&1 &
  blocker_client_pid=$!
  exec 3>"$run_dir/blocker.in"
  cat <<'SQL' >&3
set statement_timeout = '45s';
set idle_in_transaction_session_timeout = '90s';
begin;
select user_id from private.shop_account_lock where user_id = :'user_id'::uuid for update;
select 'SHOP_RESET_RACE_BLOCKER_READY';
SQL
  blocker_ready='no'
  for _ in {1..100}; do
    if rg -q 'SHOP_RESET_RACE_BLOCKER_READY' "$run_dir/blocker.out"; then
      blocker_ready='yes'
      break
    fi
    sleep 0.1
  done
  [[ "$blocker_ready" == 'yes' ]] || fail 'the blocker did not acquire the account row lock'
  blocker_backend_pid="$(psql_test shop-reset-race-observer \
    -c "select pid::text from pg_stat_activity where application_name = '$blocker_app' and state = 'idle in transaction' limit 1")" \
    || fail 'could not identify the account-lock blocker backend'
  [[ -n "$blocker_backend_pid" ]] || fail 'the blocker backend PID is empty'
}

run_reset() {
  local application_name="$1"
  local request_id="$2"
  local output_path="$3"
  psql_test "$application_name" \
    -v ON_ERROR_STOP=1 \
    -v user_id="$race_user_id" \
    -v request_id="$request_id" \
    -v cycle_id="$race_cycle_id" \
    >"$output_path" 2>&1 <<'SQL'
set role authenticated;
select set_config('request.jwt.claim.sub', :'user_id', false) \gset
set statement_timeout = '45s';
select public.reset_my_planet(:'request_id'::uuid, :'cycle_id');
SQL
}

run_remove() {
  local application_name="$1"
  local request_id="$2"
  local output_path="$3"
  psql_test "$application_name" \
    -v ON_ERROR_STOP=1 \
    -v user_id="$race_user_id" \
    -v request_id="$request_id" \
    -v cycle_id="$race_cycle_id" \
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
));
SQL
}

launch_racer() {
  local slot="$1"
  local operation="$2"
  local request_id="$3"
  local output_path="$4"
  local application_name="$5"
  if [[ "$operation" == 'reset' ]]; then
    run_reset "$application_name" "$request_id" "$output_path" &
  else
    run_remove "$application_name" "$request_id" "$output_path" &
  fi
  if [[ "$slot" == 'a' ]]; then
    racer_a_pid=$!
  else
    racer_b_pid=$!
  fi
}

wait_for_lock_chains() {
  local expected_count="$1"
  local wait_count
  wait_count="$(psql_test shop-reset-race-observer \
    -v ON_ERROR_STOP=1 \
    -v racer_a_app="$racer_a_app" \
    -v racer_b_app="$racer_b_app" \
    -v blocker_backend_pid="$blocker_backend_pid" \
    -v expected_count="$expected_count" <<'SQL'
set statement_timeout = '45s';
create function pg_temp.wait_for_racer_lock_chains(
  p_racer_apps text[], p_blocker_pid integer, p_expected_count integer
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

    if v_waiting_roots >= p_expected_count then
      return v_waiting_roots;
    end if;
    perform pg_sleep(0.1);
  end loop;
  return v_waiting_roots;
end;
$$;
select pg_temp.wait_for_racer_lock_chains(
  array[:'racer_a_app', :'racer_b_app']::text[],
  :'blocker_backend_pid'::integer,
  :'expected_count'::integer
);
SQL
  )" || fail 'could not inspect the account-lock wait chains'
  [[ "$wait_count" == "$expected_count" ]] || {
    printf 'Expected %s racer wait chains; observed %s\n' "$expected_count" "$wait_count" >&2
    fail 'independent requests were not both queued on the account-lock holder'
  }
  printf 'Observed %s independent request wait chain(s) reaching the account lock.\n' "$wait_count"
}

release_blocker() {
  cat <<'SQL' >&3
commit;
select 'SHOP_RESET_RACE_BLOCKER_RELEASED';
SQL
  exec 3>&-
  if ! wait "$blocker_client_pid"; then
    fail 'the account-lock blocker did not exit after release'
  fi
  blocker_client_pid=''
  blocker_backend_pid=''
}

wait_racers() {
  if ! wait "$racer_a_pid"; then
    printf 'Racer A output:\n' >&2
    tail -n 40 "$run_dir/a.result" >&2 || true
    fail 'racer A failed'
  fi
  racer_a_pid=''
  if ! wait "$racer_b_pid"; then
    printf 'Racer B output:\n' >&2
    tail -n 40 "$run_dir/b.result" >&2 || true
    fail 'racer B failed'
  fi
  racer_b_pid=''
}

read_reset_summary() {
  psql_test shop-reset-race-summary \
    -v ON_ERROR_STOP=1 \
    -v user_id="$race_user_id" \
    -v cycle_id="$race_cycle_id" \
    -v request_a="$race_request_a" \
    -v request_b="$race_request_b" <<'SQL'
select jsonb_build_object(
  'current_cycle_id', p.current_cycle_id,
  'current_planet_tokens', p.current_planet_tokens,
  'current_objects', p.objects,
  'cooldown_active', p.reset_available_at > now(),
  'reset_receipt_count', (select count(*) from private.shop_reset_request r
    where r.user_id = p.user_id
      and r.request_id = any(array[:'request_a', :'request_b']::uuid[])),
  'reset_count', (select count(*) from private.shop_reset_request r
    where r.user_id = p.user_id
      and r.request_id = any(array[:'request_a', :'request_b']::uuid[])
      and r.status = 'reset'),
  'cycle_mismatch_count', (select count(*) from private.shop_reset_request r
    where r.user_id = p.user_id
      and r.request_id = any(array[:'request_a', :'request_b']::uuid[])
      and r.status = 'cycle_mismatch'),
  'old_cycle_settlement_count', (select count(*) from private.shop_cycle_token_settlement s
    where s.user_id = p.user_id and s.cycle_id = :'cycle_id'),
  'old_cycle_raw_tokens', (select coalesce(sum(s.raw_tokens), 0)::bigint
    from private.shop_cycle_token_settlement s
    where s.user_id = p.user_id and s.cycle_id = :'cycle_id'),
  'old_cycle_bonus_tokens', (select coalesce(sum(s.bonus_tokens), 0)::bigint
    from private.shop_cycle_token_settlement s
    where s.user_id = p.user_id and s.cycle_id = :'cycle_id'),
  'old_cycle_wallet_rows', (select count(*) from private.planet_wallet_credits w
    where w.user_id = p.user_id and w.previous_cycle_id = :'cycle_id'),
  'old_cycle_wallet_amount', (select coalesce(sum(w.amount), 0)::bigint
    from private.planet_wallet_credits w
    where w.user_id = p.user_id and w.previous_cycle_id = :'cycle_id'),
  'wallet_credit_total', (select coalesce(sum(w.amount), 0)::bigint
    from private.planet_wallet_credits w where w.user_id = p.user_id),
  'available_balance', private.shop_available_balance(p.user_id)
)
from public.planet_member_state p where p.user_id = :'user_id'::uuid;
SQL
}

validate_reset_pair() {
  local summary
  summary="$(read_reset_summary)" || fail 'could not inspect the reset-race state'
  python3 - "$run_dir/a.result" "$run_dir/b.result" "$summary" \
    "$race_cycle_id" "$race_request_a" "$race_request_b" <<'PY'
import json
import sys

def load(path):
    with open(path, encoding="utf-8") as source:
        return json.loads(source.read().strip().splitlines()[-1])

def require(condition, message):
    if not condition:
        raise SystemExit("shop reset race: " + message)

first, second = load(sys.argv[1]), load(sys.argv[2])
summary = json.loads(sys.argv[3])
old_cycle, request_a, request_b = sys.argv[4:]
require(first["action"]["request_id"] == request_a, "racer A receipt ID changed")
require(second["action"]["request_id"] == request_b, "racer B receipt ID changed")
statuses = [first["action"]["status"], second["action"]["status"]]
require(sorted(statuses) == ["cycle_mismatch", "reset"],
        f"expected one reset and one cycle_mismatch, got {statuses}")
new_cycle = summary["current_cycle_id"]
require(new_cycle and new_cycle != old_cycle, "server did not select one new cycle")
for result in (first, second):
    require(result["planet_state"]["current_cycle_id"] == new_cycle,
            "a reset response did not return the committed new cycle")
    require(result["action"]["state"]["current_cycle_id"] == new_cycle,
            "a reset action receipt did not return the committed new cycle")
require(summary == {
    "current_cycle_id": new_cycle,
    "current_planet_tokens": 0,
    "current_objects": [],
    "cooldown_active": True,
    "reset_receipt_count": 2,
    "reset_count": 1,
    "cycle_mismatch_count": 1,
    "old_cycle_settlement_count": 1,
    "old_cycle_raw_tokens": 100000,
    "old_cycle_bonus_tokens": 0,
    "old_cycle_wallet_rows": 1,
    "old_cycle_wallet_amount": 100000,
    "wallet_credit_total": 1100000,
    "available_balance": 1100000,
}, "the reset race changed settlement, wallet, receipt, or new-cycle state: " + repr(summary))
print(f"Reset statuses: {statuses[0]}, {statuses[1]}")
print("Verified one reset, one stale-cycle receipt, one 100,000 settlement/credit, and one 1,100,000 balance.")
PY
}

read_mixed_summary() {
  psql_test shop-reset-race-summary \
    -v ON_ERROR_STOP=1 \
    -v user_id="$race_user_id" \
    -v cycle_id="$race_cycle_id" \
    -v reset_request="$race_reset_request" \
    -v remove_request="$race_remove_request" <<'SQL'
select jsonb_build_object(
  'current_cycle_id', p.current_cycle_id,
  'current_planet_tokens', p.current_planet_tokens,
  'current_objects', p.objects,
  'current_removed_keys', private.shop_current_removed_natural_keys(p.user_id),
  'cooldown_active', p.reset_available_at > now(),
  'reset_receipt_count', (select count(*) from private.shop_reset_request r
    where r.user_id = p.user_id and r.request_id = :'reset_request'::uuid),
  'reset_receipt_status', (select r.status from private.shop_reset_request r
    where r.user_id = p.user_id and r.request_id = :'reset_request'::uuid),
  'action_receipt_count', (select count(*) from private.shop_action_request r
    where r.user_id = p.user_id and r.request_id = :'remove_request'),
  'action_receipt_status', (select r.status from private.shop_action_request r
    where r.user_id = p.user_id and r.request_id = :'remove_request'),
  'old_cycle_tombstones', (select count(*) from private.shop_natural_removal r
    where r.user_id = p.user_id and r.cycle_id = :'cycle_id'),
  'old_cycle_removal_price', (select coalesce(sum(r.price), 0)::bigint
    from private.shop_natural_removal r
    where r.user_id = p.user_id and r.cycle_id = :'cycle_id'),
  'new_cycle_tombstones', (select count(*) from private.shop_natural_removal r
    where r.user_id = p.user_id and r.cycle_id = p.current_cycle_id),
  'old_cycle_settlement_count', (select count(*) from private.shop_cycle_token_settlement s
    where s.user_id = p.user_id and s.cycle_id = :'cycle_id'),
  'old_cycle_raw_tokens', (select coalesce(sum(s.raw_tokens), 0)::bigint
    from private.shop_cycle_token_settlement s
    where s.user_id = p.user_id and s.cycle_id = :'cycle_id'),
  'old_cycle_wallet_rows', (select count(*) from private.planet_wallet_credits w
    where w.user_id = p.user_id and w.previous_cycle_id = :'cycle_id'),
  'old_cycle_wallet_amount', (select coalesce(sum(w.amount), 0)::bigint
    from private.planet_wallet_credits w
    where w.user_id = p.user_id and w.previous_cycle_id = :'cycle_id'),
  'wallet_credit_total', (select coalesce(sum(w.amount), 0)::bigint
    from private.planet_wallet_credits w where w.user_id = p.user_id),
  'available_balance', private.shop_available_balance(p.user_id)
)
from public.planet_member_state p where p.user_id = :'user_id'::uuid;
SQL
}

validate_reset_remove_pair() {
  local first_operation="$1"
  local summary
  summary="$(read_mixed_summary)" || fail 'could not inspect the reset/remove race state'
  python3 - "$run_dir/a.result" "$run_dir/b.result" "$summary" \
    "$first_operation" "$race_cycle_id" "$race_reset_request" "$race_remove_request" "$race_natural_key" <<'PY'
import json
import sys

def load(path):
    with open(path, encoding="utf-8") as source:
        return json.loads(source.read().strip().splitlines()[-1])

def require(condition, message):
    if not condition:
        raise SystemExit("shop reset race: " + message)

result_a, result_b = load(sys.argv[1]), load(sys.argv[2])
summary = json.loads(sys.argv[3])
first_operation, old_cycle, reset_request, remove_request = sys.argv[4:8]
natural_key = json.loads(sys.argv[8])
reset_result = result_a if result_a.get("action") else result_b
remove_result = result_a if result_a.get("status") else result_b
require(reset_result["action"]["request_id"] == reset_request,
        "reset response request ID does not match")
require(reset_result["action"]["status"] == "reset", "reset did not commit")
new_cycle = summary["current_cycle_id"]
require(new_cycle and new_cycle != old_cycle, "reset did not advance the server cycle")
require(reset_result["planet_state"]["current_cycle_id"] == new_cycle,
        "reset response did not return the new cycle")
require(reset_result["planet_state"]["objects"] == [],
        "reset did not clear the previous cycle natural-object scene")
require(reset_result["planet_state"]["removed_natural_keys"] == [],
        "reset returned a previous-cycle natural-removal tombstone")
require(remove_result["request_id"] == remove_request, "removal receipt request ID does not match")
if first_operation == "reset":
    expected_remove_status = "cycle_mismatch"
    expected_balance = 1100000
    expected_tombstones = 0
    require(remove_result["state"]["current_cycle_id"] == new_cycle,
            "stale removal receipt did not return the new cycle")
    require(remove_result["state"]["removed_natural_keys"] == [],
            "stale removal created or exposed a new-cycle tombstone")
else:
    expected_remove_status = "removed"
    expected_balance = 1000000
    expected_tombstones = 1
    require(remove_result["state"]["current_cycle_id"] == old_cycle,
            "successful removal receipt did not retain its old-cycle state")
    require(natural_key in remove_result["state"]["removed_natural_keys"],
            "successful removal receipt did not include its exact old-cycle tombstone")
require(remove_result["status"] == expected_remove_status,
        f"expected {expected_remove_status}, got {remove_result['status']}")
require(summary == {
    "current_cycle_id": new_cycle,
    "current_planet_tokens": 0,
    "current_objects": [],
    "current_removed_keys": [],
    "cooldown_active": True,
    "reset_receipt_count": 1,
    "reset_receipt_status": "reset",
    "action_receipt_count": 1,
    "action_receipt_status": expected_remove_status,
    "old_cycle_tombstones": expected_tombstones,
    "old_cycle_removal_price": expected_tombstones * 100000,
    "new_cycle_tombstones": 0,
    "old_cycle_settlement_count": 1,
    "old_cycle_raw_tokens": 100000,
    "old_cycle_wallet_rows": 1,
    "old_cycle_wallet_amount": 100000,
    "wallet_credit_total": 1100000,
    "available_balance": expected_balance,
}, "reset/remove race changed a receipt, tombstone, settlement, wallet, or scene: " + repr(summary))
print(f"Serialized order {first_operation} first: reset=reset, remove={expected_remove_status}.")
print(f"Verified old-cycle tombstones={expected_tombstones}, current-cycle tombstones=0, balance={expected_balance}.")
PY
}

run_race() {
  local race_label="$1"
  local operation_a="$2"
  local operation_b="$3"
  local request_a="$race_request_a"
  local request_b="$race_request_b"
  if [[ "$operation_a" == 'remove' ]]; then request_a="$race_remove_request"; fi
  if [[ "$operation_b" == 'remove' ]]; then request_b="$race_remove_request"; fi
  if [[ "$operation_a" == 'reset' ]]; then
    race_reset_request="$request_a"
  else
    race_reset_request="$request_b"
  fi

  printf '\nStarting %s: %s queued before %s.\n' "$race_label" "$operation_a" "$operation_b"
  start_blocker
  launch_racer a "$operation_a" "$request_a" "$run_dir/a.result" "$racer_a_app"
  wait_for_lock_chains 1
  launch_racer b "$operation_b" "$request_b" "$run_dir/b.result" "$racer_b_app"
  wait_for_lock_chains 2
  release_blocker
  wait_racers

  if [[ "$operation_a" == 'reset' && "$operation_b" == 'reset' ]]; then
    validate_reset_pair
  else
    validate_reset_remove_pair "$operation_a"
  fi
  rm -rf -- "$run_dir"
  run_dir=''
}

new_race_context 'reset-reset'
seed_race_account
run_race 'reset x reset' 'reset' 'reset'

new_race_context 'reset-remove'
seed_race_account
capture_natural_quote
run_race 'reset x remove' 'reset' 'remove'

new_race_context 'remove-reset'
seed_race_account
capture_natural_quote
run_race 'remove x reset' 'remove' 'reset'

printf '\nAll reset race families passed on the pinned disposable local database.\n'
