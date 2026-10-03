#!/usr/bin/env bash
set -euo pipefail
set +x

# Runs only against the explicitly pinned disposable local Supabase database.
# It never starts, resets, or migrates a project.
readonly TEST_PROJECT_ID='token-planet-shop-revamp-test'
readonly TEST_WORKDIR='/private/tmp/token-planet-shop-revamp-test'
readonly TEST_CONFIG="$TEST_WORKDIR/supabase/config.toml"
readonly TEST_DB_PORT='55432'
readonly TEST_DB_CONTAINER="supabase_db_$TEST_PROJECT_ID"
readonly TEST_DB_VOLUME="supabase_db_$TEST_PROJECT_ID"
readonly TEST_CONTEXT='desktop-linux'
readonly TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly V2_JSON="$TEST_DIR/fixtures/shop_guest_import_v2_first_reset.json"
readonly V2_INC="$TEST_DIR/fixtures/shop_guest_import_v2_first_reset.inc"
readonly V1_JSON="$TEST_DIR/fixtures/shop_guest_import_native_first_reset.json"
readonly V1_INC="$TEST_DIR/fixtures/shop_guest_import_native_first_reset.inc"
readonly EXPECTED_V2_JSON='c5bbc955848e86d389c4d4071026b940f1f8837617cb9ed11f138a610d4e3a62'
readonly EXPECTED_V2_INC='c6280034c20667e5d9d06ca2746ed7c398bb5ba988aa10c4d69bf80fa00bac5f'
readonly EXPECTED_V1_JSON='8655804f5f7b4bb4761e3616362cbf519816cfcff3875b5bbe87dea60e42bf0c'
readonly EXPECTED_V1_INC='9261df576689250909dac48b4095f5cb2032ca1b00cc60110301fffe5c3077cb'

fail() {
  printf 'shop guest import v2 validation: %s\n' "$1" >&2
  exit 1
}

suite_name="${1:-shop_guest_import_v2_validation.sql}"
[[ "$suite_name" == "$(basename "$suite_name")" && "$suite_name" == *.sql ]] \
  || fail 'suite argument must be a SQL filename in supabase/tests'
readonly SUITE="$TEST_DIR/$suite_name"
[[ -f "$SUITE" ]] || fail 'requested SQL suite is missing'

for dependency in docker python3 rg shasum awk; do
  command -v "$dependency" >/dev/null 2>&1 || fail "$dependency is unavailable"
done
[[ -f "$TEST_CONFIG" ]] || fail 'the pinned disposable project config is missing'
for fixture in "$V2_JSON" "$V2_INC" "$V1_JSON" "$V1_INC"; do
  [[ -f "$fixture" ]] || fail "required native fixture is missing: $(basename "$fixture")"
done

fixture_hash() {
  shasum -a 256 "$1" | awk '{print $1}'
}
[[ "$(fixture_hash "$V2_JSON")" == "$EXPECTED_V2_JSON" ]] \
  || fail 'schema-2 native JSON fixture bytes changed'
[[ "$(fixture_hash "$V2_INC")" == "$EXPECTED_V2_INC" ]] \
  || fail 'schema-2 native SQL fixture bytes changed'
[[ "$(fixture_hash "$V1_JSON")" == "$EXPECTED_V1_JSON" ]] \
  || fail 'schema-1 held JSON fixture bytes changed'
[[ "$(fixture_hash "$V1_INC")" == "$EXPECTED_V1_INC" ]] \
  || fail 'schema-1 held SQL fixture bytes changed'
printf 'fixture hashes: schema2 JSON=%s INC=%s; schema1 JSON=%s INC=%s\n' \
  "$EXPECTED_V2_JSON" "$EXPECTED_V2_INC" "$EXPECTED_V1_JSON" "$EXPECTED_V1_INC"

context="$(docker context show 2>/dev/null)" || fail 'could not identify the Docker context'
[[ "$context" == "$TEST_CONTEXT" ]] || fail 'Docker context is not the pinned local desktop-linux context'
endpoint="$(docker context inspect "$context" --format '{{(index .Endpoints "docker").Host}}' 2>/dev/null)" \
  || fail 'could not verify the local Docker endpoint'
[[ "$endpoint" == unix://* ]] || fail 'refusing a non-local Docker endpoint'

python3 - "$TEST_CONFIG" "$TEST_PROJECT_ID" "$TEST_DB_PORT" <<'PY'
import sys
import tomllib
from pathlib import Path

config_path, expected_project, expected_port = sys.argv[1:]
config = tomllib.loads(Path(config_path).read_text())
if config.get("project_id") != expected_project:
    raise SystemExit("shop guest import v2 validation: disposable project ID mismatch")
if str(config.get("db", {}).get("port")) != expected_port:
    raise SystemExit("shop guest import v2 validation: disposable database port mismatch")
PY

container_id="$(docker inspect --format '{{.Id}}' "$TEST_DB_CONTAINER" 2>/dev/null)" \
  || fail 'the pinned disposable database container is unavailable'
[[ -n "$container_id" ]] || fail 'the pinned database container ID is empty'
[[ "$(docker inspect --format '{{.State.Running}}' "$TEST_DB_CONTAINER")" == true ]] \
  || fail 'the pinned database container is not running'
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
data_mounts = [m for m in mounts if m.get("Destination") == "/var/lib/postgresql/data"]
if len(data_mounts) != 1 or data_mounts[0].get("Type") != "volume" or data_mounts[0].get("Name") != expected_volume:
    raise SystemExit("shop guest import v2 validation: pinned database volume mismatch")
bindings = ports.get("5432/tcp") or []
if not any(binding.get("HostPort") == expected_port for binding in bindings):
    raise SystemExit("shop guest import v2 validation: pinned database port mismatch")
PY

run_id="$(python3 -c 'import uuid; print(uuid.uuid4().hex)')"
readonly RUN_TAG="shop_guest_v2_$run_id"
readonly REMOTE_DIR="/tmp/$RUN_TAG"
readonly REMOTE_APP="shop-guest-v2-$run_id"
readonly ROLLBACK_PROBE="shop_guest_v2_probe_$run_id"
readonly RUN_DIR="$(mktemp -d '/private/tmp/shop-guest-import-v2.XXXXXXXX')" \
  || fail 'could not create a private result directory'
readonly OUTPUT="$RUN_DIR/test.out"
safe_target='no'
cleanup_failed='no'

psql_test() {
  local application_name="$1"
  shift
  docker exec --env "PGAPPNAME=$application_name" -i "$TEST_DB_CONTAINER" \
    psql -X -q -A -t "$@" -U postgres -d postgres
}

database_digest() {
  local application_name="$1"
  psql_test "$application_name" -v ON_ERROR_STOP=1 <<'SQL'
create function pg_temp.shop_guest_v2_fixture_data_digest() returns text
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
select pg_temp.shop_guest_v2_fixture_data_digest();
SQL
}

probe_exists() {
  psql_test shop-guest-v2-probe -v ON_ERROR_STOP=1 \
    -c "select (to_regclass('public.\"$ROLLBACK_PROBE\"') is not null)::text"
}

cleanup() {
  local exit_code=$?
  trap - EXIT INT TERM
  if [[ "$safe_target" == yes ]]; then
    local marker
    marker="$(probe_exists 2>/dev/null | tail -n 1)" || cleanup_failed='yes'
    if [[ "$marker" == true ]]; then
      psql_test shop-guest-v2-cleanup -v ON_ERROR_STOP=1 \
        -c "drop table if exists public.\"$ROLLBACK_PROBE\"" >/dev/null 2>&1 \
        || cleanup_failed='yes'
      cleanup_failed='yes'
    elif [[ "$marker" != false ]]; then
      cleanup_failed='yes'
    fi
    docker exec "$TEST_DB_CONTAINER" rm -rf -- "$REMOTE_DIR" >/dev/null 2>&1 \
      || cleanup_failed='yes'
  fi
  rm -rf -- "$RUN_DIR"
  if [[ "$cleanup_failed" != no ]]; then
    printf 'shop guest import v2 validation: cleanup or rollback verification failed\n' >&2
    exit_code=1
  fi
  exit "$exit_code"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

observer_app="shop-guest-v2-observer-$run_id"
server_identity="$(psql_test "$observer_app" \
  -c "select current_database() || '|' || current_user || '|' || current_setting('port')")" \
  || fail 'could not connect to the pinned local database'
[[ "$server_identity" == 'postgres|postgres|5432' ]] \
  || fail 'connected database identity does not match the pinned local Supabase database'
[[ "$(probe_exists)" == false ]] || fail 'generated rollback probe name already exists'
safe_target='yes'
printf 'verified pinned local target: project=%s context=%s volume=%s host_port=%s db=%s\n' \
  "$TEST_PROJECT_ID" "$context" "$TEST_DB_VOLUME" "$TEST_DB_PORT" "$server_identity"

before_digest="$(database_digest "shop-guest-v2-before-$run_id")" \
  || fail 'could not compute the pre-test public/private/auth data digest'
[[ "$before_digest" =~ ^[0-9a-f]{32}$ ]] || fail 'pre-test data digest was malformed'
printf 'pre-test protected-table digest: %s\n' "$before_digest"

docker exec "$TEST_DB_CONTAINER" mkdir -p "$REMOTE_DIR/fixtures" \
  || fail 'could not create the isolated fixture copy path'
docker cp "$SUITE" "$TEST_DB_CONTAINER:$REMOTE_DIR/$suite_name" >/dev/null \
  || fail 'could not copy the SQL suite into the pinned container'
docker cp "$V2_JSON" "$TEST_DB_CONTAINER:$REMOTE_DIR/fixtures/$(basename "$V2_JSON")" >/dev/null \
  || fail 'could not copy the schema-2 JSON fixture into the pinned container'
docker cp "$V2_INC" "$TEST_DB_CONTAINER:$REMOTE_DIR/fixtures/$(basename "$V2_INC")" >/dev/null \
  || fail 'could not copy the schema-2 SQL fixture into the pinned container'
docker cp "$V1_JSON" "$TEST_DB_CONTAINER:$REMOTE_DIR/fixtures/$(basename "$V1_JSON")" >/dev/null \
  || fail 'could not copy the schema-1 JSON fixture into the pinned container'
docker cp "$V1_INC" "$TEST_DB_CONTAINER:$REMOTE_DIR/fixtures/$(basename "$V1_INC")" >/dev/null \
  || fail 'could not copy the schema-1 SQL fixture into the pinned container'
for fixture in \
  "shop_guest_import_v2_first_reset.json:$EXPECTED_V2_JSON" \
  "shop_guest_import_v2_first_reset.inc:$EXPECTED_V2_INC" \
  "shop_guest_import_native_first_reset.json:$EXPECTED_V1_JSON" \
  "shop_guest_import_native_first_reset.inc:$EXPECTED_V1_INC"; do
  fixture_name="${fixture%%:*}"
  expected_hash="${fixture#*:}"
  copied_hash="$(docker exec "$TEST_DB_CONTAINER" sha256sum "$REMOTE_DIR/fixtures/$fixture_name" | awk '{print $1}')" \
    || fail "could not verify copied fixture: $fixture_name"
  [[ "$copied_hash" == "$expected_hash" ]] \
    || fail "fixture copy hash mismatch: $fixture_name"
done
printf 'container fixture copies: all four frozen SHA-256 values match\n'

psql_status=0
docker exec --env "PGAPPNAME=$REMOTE_APP" "$TEST_DB_CONTAINER" \
  psql -X -A -t -v ON_ERROR_STOP=1 -v rollback_probe="$ROLLBACK_PROBE" \
    -U postgres -d postgres -f "$REMOTE_DIR/$suite_name" >"$OUTPUT" 2>&1 \
  || psql_status=$?
cat "$OUTPUT"
[[ "$psql_status" == 0 ]] || fail "psql exited with status $psql_status"
rg -q '^SHOP_GUEST_IMPORT_V2_ROLLBACK_COMPLETED$' "$OUTPUT" \
  || fail 'suite did not complete its explicit transaction rollback'

tap_stats="$(python3 - "$OUTPUT" <<'PY'
import re
import sys
from pathlib import Path

lines = Path(sys.argv[1]).read_text().splitlines()
plan = [int(m.group(1)) for line in lines if (m := re.match(r"^1\.\.(\d+)$", line))]
ok = [line for line in lines if re.match(r"^ok(?:\s|$)", line)]
not_ok = [line for line in lines if re.match(r"^not ok(?:\s|$)", line)]
if len(plan) != 1:
    raise SystemExit(f"shop guest import v2 validation: expected one TAP plan, found {len(plan)}")
if len(ok) + len(not_ok) != plan[0]:
    raise SystemExit(f"shop guest import v2 validation: TAP plan {plan[0]} has {len(ok) + len(not_ok)} results")
print(plan[0], len(ok), len(not_ok))
PY
)" || fail 'TAP output did not contain one complete plan'
read -r tap_plan tap_ok tap_not_ok <<<"$tap_stats"
printf 'TAP summary: plan=%s ok=%s not_ok=%s\n' "$tap_plan" "$tap_ok" "$tap_not_ok"

[[ "$(probe_exists)" == false ]] || fail 'suite left the rollback probe table committed'
after_digest="$(database_digest "shop-guest-v2-after-$run_id")" \
  || fail 'could not compute the post-test public/private/auth data digest'
[[ "$after_digest" == "$before_digest" ]] \
  || fail "suite changed protected table data (before=$before_digest after=$after_digest)"
printf 'post-test protected-table digest: %s\n' "$after_digest"
if [[ "$tap_not_ok" != 0 ]]; then
  fail "TAP has $tap_not_ok failing assertion(s)"
fi
printf 'shop guest import v2 validation: pinned SQL suite passed and rolled back\n'
