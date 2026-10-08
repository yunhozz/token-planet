#!/usr/bin/env bash
set -Eeuo pipefail
umask 077

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../.." && pwd -P)"
SOURCE_MIGRATIONS="$REPO_ROOT/supabase/migrations"
SUPABASE_BIN="${SUPABASE_BIN:-$(command -v supabase || true)}"
DOCKER_BIN="$(command -v docker || true)"
PYTHON_BIN="$(command -v python3 || true)"
POSTGRES_IMAGE="public.ecr.aws/supabase/postgres:17.6.1.171"
DB_PORT=56432
SHADOW_PORT=56430

usage() {
  printf 'Usage: %s --artifacts-dir <absolute-path>\n' "$0" >&2
}

if [[ $# -ne 2 || "$1" != "--artifacts-dir" ]]; then
  usage
  exit 2
fi
ARTIFACTS_DIR="$2"
if [[ "$ARTIFACTS_DIR" != /* ]]; then
  printf 'error: artifacts directory must be absolute\n' >&2
  exit 2
fi
if [[ -L "$ARTIFACTS_DIR" ]]; then
  printf 'error: artifacts directory must not be a symlink\n' >&2
  exit 2
fi
if ! ARTIFACTS_DIR="$(python3 - "$REPO_ROOT" "$ARTIFACTS_DIR" <<'PY'
from pathlib import Path
import sys

repo_root = Path(sys.argv[1]).resolve()
artifact_dir = Path(sys.argv[2]).resolve(strict=False)
if artifact_dir == repo_root or repo_root in artifact_dir.parents:
    print("error: artifacts directory must be outside the repository", file=sys.stderr)
    raise SystemExit(2)
print(artifact_dir)
PY
)"; then
  exit 2
fi
for remote_variable in ${!SUPABASE_@}; do
  case "$remote_variable" in
    SUPABASE_BIN|SUPABASE_TELEMETRY_DISABLED) ;;
    *)
      if [[ -n "${!remote_variable:-}" ]]; then
        printf 'error: refusing Supabase environment override %s\n' "$remote_variable" >&2
        exit 2
      fi
      ;;
  esac
done
for database_override in DATABASE_URL PGHOST PGHOSTADDR PGPORT PGDATABASE PGUSER PGPASSWORD PGSERVICE PGSERVICEFILE; do
  if [[ -n "${!database_override:-}" ]]; then
    printf 'error: refusing database connection environment variable %s\n' "$database_override" >&2
    exit 2
  fi
done
if [[ -z "$SUPABASE_BIN" || "$SUPABASE_BIN" != /* || ! -x "$SUPABASE_BIN" ]]; then
  printf 'error: SUPABASE_BIN must be an absolute executable path\n' >&2
  exit 2
fi
if [[ -z "$DOCKER_BIN" || "$DOCKER_BIN" != /* || ! -x "$DOCKER_BIN" ]]; then
  printf 'error: Docker executable path could not be resolved\n' >&2
  exit 2
fi
if [[ -z "$PYTHON_BIN" || "$PYTHON_BIN" != /* || ! -x "$PYTHON_BIN" ]]; then
  printf 'error: Python executable path could not be resolved\n' >&2
  exit 2
fi

mkdir -p -- "$ARTIFACTS_DIR"
if [[ -n "$(find "$ARTIFACTS_DIR" -mindepth 1 -maxdepth 1 -print -quit)" ]]; then
  printf 'error: artifacts directory must be empty\n' >&2
  exit 2
fi
RUN_LOG="$ARTIFACTS_DIR/run.log"
CLI_VERSION_LOG="$ARTIFACTS_DIR/cli-version.log"
NETWORK_LOG="$ARTIFACTS_DIR/network.log"
CLEANUP_LOG="$ARTIFACTS_DIR/cleanup.log"

WORKDIR=""
PROJECT_ID=""
PROJECT_SUFFIX=""
NETWORK_NAME=""
NETWORK_ID=""
NETWORK_CREATE_ATTEMPTED=0
NETWORK_OWNED=0
CONTAINER_NAME=""
CONTAINER_ID=""
CONTAINER_OWNED=0
VOLUME_NAME=""
VOLUME_OWNED=0
CLI_START_ATTEMPTED=0

log() {
  printf '%s\n' "$*" | tee -a "$RUN_LOG"
}

fail() {
  local status="$1"
  shift
  log "ERROR: $*"
  exit "$status"
}

owned_identity() {
  local kind="$1"
  local json_file="$2"
  local expected_id="${3:-}"
  python3 - "$kind" "$json_file" "$PROJECT_ID" "$WORKDIR" "$NETWORK_NAME" "$NETWORK_ID" "$CONTAINER_NAME" "$CONTAINER_ID" "$VOLUME_NAME" "$expected_id" <<'PY'
import json
import pathlib
import sys

kind, path, project, workdir, network_name, network_id, container_name, container_id, volume_name, expected_id = sys.argv[1:]
try:
    value = json.loads(pathlib.Path(path).read_text())
except (OSError, json.JSONDecodeError) as error:
    raise SystemExit(f"invalid Docker inspect response: {error}")

if kind == "network":
    labels = value.get("Labels") or {}
    identity = value.get("Id", "")
    valid = (
        identity
        and value.get("Name") == network_name
        and labels.get("com.tokenplanet.ci.project") == project
        and labels.get("com.tokenplanet.ci.workdir") == workdir
        and (not (expected_id or network_id) or identity == (expected_id or network_id))
    )
elif kind == "container":
    labels = (value.get("Config") or {}).get("Labels") or {}
    identity = value.get("Id", "")
    valid = (
        identity
        and value.get("Name", "").lstrip("/") == container_name
        and labels.get("com.supabase.cli.project") == project
        and labels.get("com.supabase.cli.workdir") == workdir
        and (not (expected_id or container_id) or identity == (expected_id or container_id))
    )
elif kind == "volume":
    labels = value.get("Labels") or {}
    identity = value.get("Name", "")
    valid = (
        identity == volume_name
        and labels.get("com.supabase.cli.project") == project
        and (not expected_id or identity == expected_id)
    )
else:
    raise SystemExit(f"unknown Docker identity kind: {kind}")

if not valid:
    raise SystemExit(f"Docker {kind} identity does not match this run")
print(identity)
PY
}

check_network_options() {
  local json_file="$1"
  python3 - "$json_file" <<'PY'
import json
import pathlib
import sys
try:
    value = json.loads(pathlib.Path(sys.argv[1]).read_text())
except (OSError, json.JSONDecodeError) as error:
    raise SystemExit(f"invalid network inspect response: {error}")
options = value.get("Options") or {}
if options.get("com.docker.network.bridge.host_binding_ipv4") != "127.0.0.1":
    raise SystemExit("CI bridge network is not restricted to loopback host bindings")
PY
}

check_container_binding() {
  local json_file="$1"
  python3 - "$json_file" "$DB_PORT" <<'PY'
import json
import pathlib
import re
import sys

def fail(code, fields=""):
    suffix = f" {fields}" if fields else ""
    print(f"binding_status=FAIL code={code}{suffix}")
    raise SystemExit(1)

try:
    value = json.loads(pathlib.Path(sys.argv[1]).read_text())
except (OSError, json.JSONDecodeError):
    fail("invalid_inspect")

# NetworkSettings.Ports contains Docker's resolved host bindings. HostConfig
# records the request and may legitimately retain an empty HostIp.
ports = ((value.get("NetworkSettings") or {}).get("Ports") or {})
if not isinstance(ports, dict):
    fail("actual_ports_missing")
for port_name, entries in ports.items():
    if not re.fullmatch(r"[0-9]+/(?:tcp|udp)", port_name):
        port_name = "unknown"
    for entry in entries or []:
        host_ip = entry.get("HostIp")
        host_port = entry.get("HostPort")
        if host_ip != "127.0.0.1":
            safe_ip = host_ip if isinstance(host_ip, str) and re.fullmatch(r"[A-Fa-f0-9:.]+", host_ip) else "invalid"
            safe_port = host_port if isinstance(host_port, str) and host_port.isdigit() else "unknown"
            fail("published_host_ip", f"port={port_name} actual_ip={safe_ip} host_port={safe_port} expected_ip=127.0.0.1")

postgres = ports.get("5432/tcp") or []
if not any(entry.get("HostIp") == "127.0.0.1" and entry.get("HostPort") == sys.argv[2] for entry in postgres):
    actual = ",".join(
        f"127.0.0.1:{entry.get('HostPort')}"
        for entry in postgres
        if isinstance(entry.get("HostPort"), str) and entry.get("HostPort").isdigit()
    ) or "missing"
    fail("postgres_binding_missing", f"expected=127.0.0.1:{sys.argv[2]} actual={actual}")
print(f"binding_status=PASS check=published_ports postgres=127.0.0.1:{sys.argv[2]}")
PY
}

check_container_network() {
  local json_file="$1"
  local expected_network_id="$2"
  python3 - "$json_file" "$expected_network_id" <<'PY'
import json
import pathlib
import sys
try:
    value = json.loads(pathlib.Path(sys.argv[1]).read_text())
except (OSError, json.JSONDecodeError):
    print("network_status=FAIL code=invalid_inspect")
    raise SystemExit(1)
networks = ((value.get("NetworkSettings") or {}).get("Networks") or {}).values()
if not any(network.get("NetworkID") == sys.argv[2] for network in networks):
    print("network_status=FAIL code=bridge_attachment expected=owned actual=unmatched")
    raise SystemExit(1)
print("network_status=PASS code=bridge_attachment")
PY
}

check_history() {
  local expected_manifest="$1"
  local actual_versions="$2"
  python3 - "$expected_manifest" "$actual_versions" <<'PY'
import json
import pathlib
import sys
manifest = json.loads(pathlib.Path(sys.argv[1]).read_text())
expected = [entry["synthetic_version"] for entry in manifest["entries"]]
actual = [line.strip() for line in pathlib.Path(sys.argv[2]).read_text().splitlines() if line.strip()]
if actual != expected:
    raise SystemExit(f"database migration history differs from manifest: expected {len(expected)} versions, found {len(actual)}")
PY
}

check_tap() {
  local tap_file="$1"
  python3 - "$tap_file" <<'PY'
import pathlib
import re
import sys

def fail(summary):
    print(summary)
    raise SystemExit(1)

lines = pathlib.Path(sys.argv[1]).read_text(errors="replace").splitlines()
plans = []
results = []
failed = []
for raw in lines:
    line = raw.strip()
    if not line or line.startswith("#"):
        continue
    if line.startswith("Bail out!"):
        fail("TAP status=FAIL bailout details=redacted")
    directive = re.search(r"\s+#\s*(SKIP|TODO)\b", line, re.I)
    if directive:
        fail(f"TAP status=INVALID directive={directive.group(1).upper()} details=redacted")
    plan = re.fullmatch(r"1\.\.(\d+)", line)
    if plan:
        plans.append(int(plan.group(1)))
        continue
    result = re.fullmatch(r"(not )?ok (\d+)(?:\s+-\s+.*)?", line)
    if result:
        number = int(result.group(2))
        results.append(number)
        if result.group(1):
            failed.append(str(number))
        continue
    if line.startswith(("ok", "not ok", "1..")):
        fail("TAP status=INVALID malformed_record details=redacted")
    # psql tuple output can include values from set_config() in the test SQL.
    # Only TAP records affect the plan and assertion result below.
    continue
if len(plans) != 1:
    fail("TAP status=INVALID plan_count details=redacted")
if plans[0] <= 0:
    fail("TAP status=INVALID empty_plan details=redacted")
if plans[0] != len(results) or results != list(range(1, plans[0] + 1)):
    fail("TAP status=INVALID incomplete_plan details=redacted")
if failed:
    fail(f"TAP status=FAIL failing_numbers={','.join(failed)} details=redacted")
print(f"TAP status=PASS plan={plans[0]} assertions={len(results)}")
PY
}

sanitize_cli_log() {
  local source_file="$1"
  local destination_file="$2"
  python3 - "$source_file" "$destination_file" <<'PY'
import pathlib
import re
import sys
source = pathlib.Path(sys.argv[1]).read_text(errors="replace").splitlines()
safe = []
credential_line = re.compile(r"(?:\b(?:url|key|secret|password|token)\s*[:=]|service[_ -]?role|anon\s+key)", re.I)
jwt = re.compile(r"\beyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\b")
allowlisted = re.compile(
    r"^(?:Applying migration \d{14}_[A-Za-z0-9_.-]+\.sql|"
    r"(?:Creating|Starting|Resetting|Finished|Stopped|Stopping)(?: [A-Za-z0-9_.:/ -]+)?)$"
)
for line in source:
    if credential_line.search(line):
        safe.append("[credential-bearing CLI output redacted]")
    elif jwt.search(line):
        safe.append("[credential-bearing CLI output redacted]")
    elif allowlisted.fullmatch(line.strip()):
        safe.append(line)
    else:
        safe.append("[unclassified CLI output redacted]")
pathlib.Path(sys.argv[2]).write_text("\n".join(safe) + ("\n" if safe else ""))
PY
}

collect_start_diagnostics() {
  local diagnostic_dir
  diagnostic_dir="$(mktemp -d "$WORKDIR/start-diagnostics.XXXXXXXX")" || return 0
  TOKEN_PLANET_REAL_DOCKER="$DOCKER_BIN" \
    TOKEN_PLANET_GUARD_PROJECT_ID="$PROJECT_ID" \
    TOKEN_PLANET_GUARD_WORKDIR="$WORKDIR" \
    TOKEN_PLANET_GUARD_NETWORK_ID="$NETWORK_ID" \
    TOKEN_PLANET_GUARD_NETWORK_NAME="$NETWORK_NAME" \
    TOKEN_PLANET_GUARD_PHASE=diagnostic \
    "$PYTHON_BIN" "$SCRIPT_DIR/product_docker_guard.py" --diagnose-start \
    >"$diagnostic_dir/records.jsonl" 2>"$diagnostic_dir/diagnostic.stderr" || :
  "$PYTHON_BIN" - "$SCRIPT_DIR" "$diagnostic_dir/records.jsonl" "$ARTIFACTS_DIR/diagnostics.jsonl" \
    >"$diagnostic_dir/export.stdout" 2>"$diagnostic_dir/export.stderr" <<'PY' || :
import json
import os
import sys
sys.path.insert(0, sys.argv[1])
from product_docker_guard import normalize_diagnostics, read_private_file
try:
    raw = read_private_file(sys.argv[2], 8192)
except (OSError, ValueError):
    raw = b""
rows = normalize_diagnostics(raw)
try:
    descriptor = os.open(sys.argv[3], os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
        for row in rows:
            stream.write(json.dumps(row, sort_keys=True) + "\n")
except OSError:
    pass
PY
  return 0
}

collect_reset_diagnostics() {
  "$PYTHON_BIN" - --reset-diagnostics "$SCRIPT_DIR" "$WORKDIR" "$ARTIFACTS_DIR/reset-diagnostics.json" <<'PY'
import json
import os
import re
import stat
import sys
sys.path.insert(0, sys.argv[2])
from product_docker_guard import evidence_records, read_evidence_file, read_private_file
workdir, destination = sys.argv[3:]
row = {"guard_codes": [], "last_announced_migration": None, "sqlstate": None,
       "diagnostic_status": "ok", "error_class": "unknown"}
try:
    records = evidence_records(read_evidence_file({"workdir": workdir}, "rejections.jsonl", 8192))
    row["guard_codes"] = sorted({record["code"] for record in records if record["phase"] == "reset"})
except FileNotFoundError:
    pass
except (OSError, ValueError, TypeError, UnicodeError):
    row["diagnostic_status"] = "invalid"
raw = None
descriptor = None
try:
    descriptor = os.open(os.path.join(workdir, "reset.raw.log"), os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    info = os.fstat(descriptor)
    if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid()
            or stat.S_IMODE(info.st_mode) != 0o600 or info.st_nlink != 1):
        row["diagnostic_status"] = "invalid"
    elif info.st_size > 65536:
        row["diagnostic_status"] = "truncated"
    else:
        candidate = os.read(descriptor, 65537)
        if len(candidate) > 65536 or any(len(line) > 4096 for line in candidate.splitlines()):
            row["diagnostic_status"] = "truncated"
        else:
            raw = candidate.decode("utf-8")
except FileNotFoundError:
    row["diagnostic_status"] = "unavailable"
except (OSError, ValueError, UnicodeError):
    row["diagnostic_status"] = "invalid"
finally:
    if descriptor is not None:
        os.close(descriptor)
if raw is not None:
    try:
        manifest_limit = 1048576
        manifest = json.loads(read_private_file(os.path.join(workdir, "supabase/migrations/manifest.json"), manifest_limit))
        filenames = {entry["staged_filename"] for entry in manifest["entries"]}
        if any(not isinstance(name, str) or not re.fullmatch(r"\d{14}_[A-Za-z0-9_.-]+\.sql", name) for name in filenames):
            raise ValueError
    except FileNotFoundError:
        filenames = set()
        row["diagnostic_status"] = "unavailable"
    except (OSError, ValueError, KeyError, TypeError, UnicodeError):
        filenames = set()
        row["diagnostic_status"] = "invalid"
    credential = re.compile(r"(?:\b(?:url|key|secret|password|token)\s*[:=]|service[_ -]?role|anon\s+key|postgres(?:ql)?://|\beyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\b)", re.I)
    sqlstates = set()
    for line in raw.splitlines():
        if credential.search(line):
            continue
        announcement = re.fullmatch(r"Applying migration ([A-Za-z0-9_.-]+\.sql)\.\.\.", line)
        if announcement and announcement.group(1) in filenames:
            row["last_announced_migration"] = announcement.group(1)
        error = re.fullmatch(r"ERROR: .+ \(SQLSTATE ([A-Z0-9]{5})\)", line)
        if error:
            sqlstates.add(error.group(1))
    if len(sqlstates) == 1:
        row["sqlstate"] = next(iter(sqlstates))
    elif len(sqlstates) > 1 and row["diagnostic_status"] == "ok":
        row["diagnostic_status"] = "ambiguous"
if row["guard_codes"]:
    row["error_class"] = "guard_rejection_observed"
if row["sqlstate"]:
    row["error_class"] = "guard_and_sql_error_observed" if row["guard_codes"] else "sql_error_observed"
try:
    descriptor = os.open(destination, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
        stream.write(json.dumps(row, sort_keys=True) + "\n")
except OSError:
    pass
PY
  return 0
}

record_cleanup_inspect_failure() {
  local kind="$1"
  local stderr_file="$2"
  local inspect_status="$3"
  local not_found_pattern
  case "$kind" in
    container) not_found_pattern='no such (object|container)(:|[[:space:]])|container .* (not found|does not exist)' ;;
    volume) not_found_pattern='no such volume(:|[[:space:]])|volume .* (not found|does not exist)' ;;
    network) not_found_pattern='no such network(:|[[:space:]])|network .* (not found|does not exist)' ;;
    *) not_found_pattern='a^' ;;
  esac

  if grep -Eiq "$not_found_pattern" "$stderr_file"; then
    printf 'cleanup_status=SKIP kind=%s_inspect code=confirmed_not_found\n' "$kind" >>"$CLEANUP_LOG"
  else
    printf 'cleanup_status=FAIL kind=%s_inspect code=inspect_error exit=%s\n' "$kind" "$inspect_status" >>"$CLEANUP_LOG"
    cleanup_status=1
  fi
}

cleanup_on_exit() {
  local original_status="$?"
  local cleanup_status=0
  local preserve_helper_evidence=0
  trap - EXIT INT TERM
  set +e

  if [[ -n "$WORKDIR" && "$CLI_START_ATTEMPTED" == 1 ]]; then
    if TOKEN_PLANET_REAL_DOCKER="$DOCKER_BIN" \
      TOKEN_PLANET_GUARD_PROJECT_ID="$PROJECT_ID" \
      TOKEN_PLANET_GUARD_WORKDIR="$WORKDIR" \
      TOKEN_PLANET_GUARD_NETWORK_ID="$NETWORK_ID" \
      TOKEN_PLANET_GUARD_NETWORK_NAME="$NETWORK_NAME" \
      TOKEN_PLANET_GUARD_PHASE=cleanup \
      "$PYTHON_BIN" "$SCRIPT_DIR/product_docker_guard.py" --cleanup-helpers >>"$CLEANUP_LOG" 2>&1; then
      printf 'cleanup_status=PASS kind=helper code=recorded_helpers_recovered\n' >>"$CLEANUP_LOG"
    else
      printf 'cleanup_status=FAIL kind=helper code=recovery_failed\n' >>"$CLEANUP_LOG"
      cleanup_status=1
      preserve_helper_evidence=1
    fi

    local inspect_file="$WORKDIR/container-cleanup.json"
    local target="$CONTAINER_NAME"
    if docker container inspect --format '{{json .}}' "$target" >"$inspect_file" 2>"$WORKDIR/container-cleanup.stderr"; then
      local current_id
      current_id="$(owned_identity container "$inspect_file" 2>>"$WORKDIR/container-cleanup.stderr")"
      if [[ -n "$current_id" ]]; then
        CONTAINER_ID="$current_id"
        CONTAINER_OWNED=1
        if docker container rm --force "$CONTAINER_ID" >"$WORKDIR/container-remove.stdout" 2>"$WORKDIR/container-remove.stderr"; then
          printf 'cleanup_status=PASS kind=container_remove code=owned_resource_removed\n' >>"$CLEANUP_LOG"
        else
          printf 'cleanup_status=FAIL kind=container_remove code=remove_failed exit=%s diagnostics=redacted\n' "$?" >>"$CLEANUP_LOG"
          cleanup_status=1
        fi
      else
        printf 'Skipped container with mismatched identity\n' >>"$CLEANUP_LOG"
        printf 'cleanup_status=FAIL kind=container_identity code=identity_mismatch\n' >>"$CLEANUP_LOG"
        cleanup_status=1
      fi
    else
      local inspect_status="$?"
      record_cleanup_inspect_failure container "$WORKDIR/container-cleanup.stderr" "$inspect_status"
    fi

    inspect_file="$WORKDIR/volume-cleanup.json"
    if docker volume inspect --format '{{json .}}' "$VOLUME_NAME" >"$inspect_file" 2>"$WORKDIR/volume-cleanup.stderr"; then
      local current_volume
      current_volume="$(owned_identity volume "$inspect_file" 2>>"$WORKDIR/volume-cleanup.stderr")"
      if [[ -n "$current_volume" ]]; then
        VOLUME_OWNED=1
        if docker volume rm "$current_volume" >"$WORKDIR/volume-remove.stdout" 2>"$WORKDIR/volume-remove.stderr"; then
          printf 'cleanup_status=PASS kind=volume_remove code=owned_resource_removed\n' >>"$CLEANUP_LOG"
        else
          printf 'cleanup_status=FAIL kind=volume_remove code=remove_failed exit=%s diagnostics=redacted\n' "$?" >>"$CLEANUP_LOG"
          cleanup_status=1
        fi
      else
        printf 'Skipped volume with mismatched identity\n' >>"$CLEANUP_LOG"
        printf 'cleanup_status=FAIL kind=volume_identity code=identity_mismatch\n' >>"$CLEANUP_LOG"
        cleanup_status=1
      fi
    else
      local inspect_status="$?"
      record_cleanup_inspect_failure volume "$WORKDIR/volume-cleanup.stderr" "$inspect_status"
    fi
  fi

  if [[ "$NETWORK_CREATE_ATTEMPTED" == 1 ]]; then
    local inspect_file="$WORKDIR/network-cleanup.json"
    local target="$NETWORK_ID"
    [[ -n "$target" ]] || target="$NETWORK_NAME"
    if docker network inspect --format '{{json .}}' "$target" >"$inspect_file" 2>"$WORKDIR/network-cleanup.stderr"; then
      local current_id
      current_id="$(owned_identity network "$inspect_file" 2>>"$WORKDIR/network-cleanup.stderr")"
      if [[ -n "$current_id" ]]; then
        NETWORK_ID="$current_id"
        NETWORK_OWNED=1
        if docker network rm "$NETWORK_ID" >"$WORKDIR/network-remove.stdout" 2>"$WORKDIR/network-remove.stderr"; then
          printf 'cleanup_status=PASS kind=network_remove code=owned_resource_removed\n' >>"$CLEANUP_LOG"
        else
          printf 'cleanup_status=FAIL kind=network_remove code=remove_failed exit=%s diagnostics=redacted\n' "$?" >>"$CLEANUP_LOG"
          cleanup_status=1
        fi
      else
        printf 'Skipped network with mismatched identity\n' >>"$CLEANUP_LOG"
        printf 'cleanup_status=FAIL kind=network_identity code=identity_mismatch\n' >>"$CLEANUP_LOG"
        cleanup_status=1
      fi
    else
      local inspect_status="$?"
      record_cleanup_inspect_failure network "$WORKDIR/network-cleanup.stderr" "$inspect_status"
    fi
  fi

  if [[ "$preserve_helper_evidence" == 1 ]]; then
    printf 'Preserved helper recovery evidence in %s\n' "$WORKDIR" >>"$CLEANUP_LOG"
  elif [[ -n "$WORKDIR" && -d "$WORKDIR" && "$(basename -- "$WORKDIR")" == token-planet-ci.* ]]; then
    if ! rm -rf -- "$WORKDIR"; then
      printf 'Failed to remove generated work directory\n' >>"$CLEANUP_LOG"
      cleanup_status=1
    fi
  fi
  if [[ "$cleanup_status" -ne 0 ]]; then
    printf 'Cleanup reported one or more errors\n' >>"$CLEANUP_LOG"
  fi
  if [[ "$original_status" -ne 0 ]]; then
    exit "$original_status"
  fi
  exit "$cleanup_status"
}

trap cleanup_on_exit EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

for docker_override in DOCKER_HOST DOCKER_CONTEXT DOCKER_TLS_VERIFY DOCKER_CERT_PATH; do
  if [[ -n "${!docker_override:-}" ]]; then
    fail 2 "refusing Docker connection override: $docker_override"
  fi
done

if [[ -e "$ARTIFACTS_DIR" && -n "$(find "$ARTIFACTS_DIR" -mindepth 1 -maxdepth 1 -print -quit)" ]]; then
  fail 2 "artifacts directory must be empty"
fi

export SUPABASE_TELEMETRY_DISABLED=1
PROJECT_SUFFIX="$(python3 -c 'import uuid; print(uuid.uuid4().hex[:24])')"
PROJECT_ID="token-planet-ci-$PROJECT_SUFFIX"
NETWORK_NAME="token-planet-ci-net-$PROJECT_SUFFIX"
CONTAINER_NAME="supabase_db_$PROJECT_ID"
VOLUME_NAME="supabase_db_$PROJECT_ID"
TMP_BASE="${TMPDIR:-/tmp}"
if ! WORKDIR="$(mktemp -d "$TMP_BASE/token-planet-ci.XXXXXXXX")"; then
  fail 1 "could not create the generated temporary project directory"
fi
WORKDIR="$(cd -- "$WORKDIR" && pwd -P)"
mkdir -p "$WORKDIR/supabase/.temp"
printf '17.6.1.171\n' >"$WORKDIR/supabase/.temp/postgres-version"
cat >"$WORKDIR/supabase/config.toml" <<EOF
project_id = "$PROJECT_ID"

[api]
enabled = false

[db]
port = $DB_PORT
shadow_port = $SHADOW_PORT
major_version = 17
health_timeout = "2m"

[db.migrations]
enabled = true
schema_paths = []

[db.seed]
enabled = false
sql_paths = []
EOF

if ! (cd "$WORKDIR" && "$SUPABASE_BIN" --version) >"$CLI_VERSION_LOG" 2>"$WORKDIR/cli-version.stderr"; then
  fail 1 "Supabase CLI version check failed"
fi
CLI_VERSION="$(tr -d '\r\n' <"$CLI_VERSION_LOG")"
if [[ "$CLI_VERSION" != "2.119.0" ]]; then
  fail 1 "expected Supabase CLI 2.119.0, found $CLI_VERSION"
fi

if ! python3 "$SCRIPT_DIR/prepare_migrations.py" prepare --source "$SOURCE_MIGRATIONS" --output "$WORKDIR/supabase/migrations" >"$ARTIFACTS_DIR/prepare.log" 2>&1; then
  cat "$ARTIFACTS_DIR/prepare.log" >>"$RUN_LOG"
  fail 1 "migration staging failed"
fi
cp "$WORKDIR/supabase/migrations/manifest.json" "$ARTIFACTS_DIR/manifest.json"
cp -R "$REPO_ROOT/supabase/tests" "$WORKDIR/supabase/tests"
python3 "$SCRIPT_DIR/prepare_migrations.py" verify --source "$SOURCE_MIGRATIONS" --output "$WORKDIR/supabase/migrations" >"$ARTIFACTS_DIR/verify-before.log" 2>&1 || fail 1 "migration staging verification failed before database start"

if ! docker context show >"$WORKDIR/docker-context-name" 2>"$WORKDIR/docker-context.stderr"; then
  fail 1 "could not inspect the active Docker context"
fi
DOCKER_CONTEXT_NAME="$(tr -d '\r\n' <"$WORKDIR/docker-context-name")"
if [[ -z "$DOCKER_CONTEXT_NAME" ]]; then
  fail 1 "active Docker context has no name"
fi
if ! docker context inspect --format '{{json .Endpoints.docker.Host}}' "$DOCKER_CONTEXT_NAME" >"$WORKDIR/docker-context-endpoint.json" 2>"$WORKDIR/docker-context-inspect.stderr"; then
  fail 1 "could not inspect the active Docker endpoint"
fi
if ! python3 - "$WORKDIR/docker-context-endpoint.json" >"$WORKDIR/docker-context-check.log" 2>&1 <<'PY'
import json
import pathlib
import sys
try:
    endpoint = json.loads(pathlib.Path(sys.argv[1]).read_text())
except (OSError, json.JSONDecodeError) as error:
    raise SystemExit(f"invalid Docker endpoint response: {error}")
if not isinstance(endpoint, str) or not endpoint.startswith("unix:///"):
    raise SystemExit("Docker endpoint is not a local Unix socket")
PY
then
  fail 2 "Docker daemon must use a local Unix socket endpoint"
fi
printf 'Verified active Docker context uses a local Unix socket.\n' >"$ARTIFACTS_DIR/docker-context-check.log"

if ! docker info >"$WORKDIR/docker-info.log" 2>&1; then
  fail 1 "Docker engine is unavailable"
fi
printf 'Docker engine is available.\n' >"$ARTIFACTS_DIR/docker-info.log"
if ! docker image inspect "$POSTGRES_IMAGE" >"$WORKDIR/image-inspect.json" 2>"$WORKDIR/image-inspect.stderr"; then
  fail 1 "required cached Postgres image is unavailable: $POSTGRES_IMAGE"
fi
printf 'Required cached Postgres image is available: %s\n' "$POSTGRES_IMAGE" >"$ARTIFACTS_DIR/image-check.log"
if docker network inspect --format '{{json .}}' "$NETWORK_NAME" >"$WORKDIR/network-collision.json" 2>"$WORKDIR/network-collision.stderr"; then
  fail 1 "generated Docker network name already exists: $NETWORK_NAME"
fi
if docker container inspect --format '{{json .}}' "$CONTAINER_NAME" >"$WORKDIR/container-collision.json" 2>"$WORKDIR/container-collision.stderr"; then
  fail 1 "generated Supabase container name already exists: $CONTAINER_NAME"
fi
if docker volume inspect --format '{{json .}}' "$VOLUME_NAME" >"$WORKDIR/volume-collision.json" 2>"$WORKDIR/volume-collision.stderr"; then
  fail 1 "generated Supabase volume name already exists: $VOLUME_NAME"
fi
if python3 - "$DB_PORT" "$SHADOW_PORT" >"$WORKDIR/port-preflight.log" 2>&1 <<'PY'
import socket
import sys
for port_text in sys.argv[1:]:
    port = int(port_text)
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        try:
            sock.bind(("127.0.0.1", port))
        except OSError as error:
            raise SystemExit(f"port {port} is unavailable: {error}")
PY
then
  :
else
  port_status="$?"
  fail "$port_status" "one or more required local ports are unavailable"
fi
printf 'Required local database ports are available.\n' >"$ARTIFACTS_DIR/port-preflight.log"

NETWORK_CREATE_ATTEMPTED=1
if docker network create --driver bridge \
  --opt com.docker.network.bridge.host_binding_ipv4=127.0.0.1 \
  --label "com.tokenplanet.ci.project=$PROJECT_ID" \
  --label "com.tokenplanet.ci.workdir=$WORKDIR" \
  "$NETWORK_NAME" >"$WORKDIR/network-create.log" 2>&1; then
  :
else
  network_create_status="$?"
  fail "$network_create_status" "could not create the private loopback CI bridge"
fi
NETWORK_ID="$(tail -n 1 "$WORKDIR/network-create.log" | tr -d '\r')"
if [[ -z "$NETWORK_ID" ]]; then
  fail 1 "Docker did not return the created network ID"
fi
printf 'Created private loopback CI bridge.\n' >"$NETWORK_LOG"
if ! docker network inspect --format '{{json .}}' "$NETWORK_ID" >"$WORKDIR/network-inspect.json" 2>"$WORKDIR/network-inspect.stderr"; then
  fail 1 "could not inspect the generated CI bridge"
fi
if ! NETWORK_ID="$(owned_identity network "$WORKDIR/network-inspect.json" 2>"$WORKDIR/network-inspect.stderr")"; then
  fail 1 "generated CI bridge ownership could not be verified"
fi
NETWORK_OWNED=1
if ! check_network_options "$WORKDIR/network-inspect.json" >"$WORKDIR/network-options-check.log" 2>&1; then
  fail 1 "generated CI bridge does not enforce loopback host bindings"
fi
printf 'Verified bridge host binding is 127.0.0.1.\n' >>"$NETWORK_LOG"

GUARD_BIN="$WORKDIR/.product-docker-guard/bin"
if ! mkdir -p -- "$GUARD_BIN"; then
  fail 1 "could not prepare the product Docker guard"
fi
cat >"$GUARD_BIN/docker" <<EOF
#!/bin/sh
exec "$PYTHON_BIN" "$SCRIPT_DIR/product_docker_guard.py" "\$@"
EOF
if ! chmod 700 "$GUARD_BIN/docker"; then
  fail 1 "could not prepare the product Docker guard"
fi

CLI_START_ATTEMPTED=1
if (
  cd "$WORKDIR" &&
  PATH="$GUARD_BIN:$PATH" \
  TOKEN_PLANET_REAL_DOCKER="$DOCKER_BIN" \
  TOKEN_PLANET_GUARD_PROJECT_ID="$PROJECT_ID" \
  TOKEN_PLANET_GUARD_WORKDIR="$WORKDIR" \
  TOKEN_PLANET_GUARD_NETWORK_ID="$NETWORK_ID" \
  TOKEN_PLANET_GUARD_NETWORK_NAME="$NETWORK_NAME" \
  TOKEN_PLANET_GUARD_PHASE=start \
  "$SUPABASE_BIN" db start --network-id "$NETWORK_ID"
) >"$WORKDIR/start.raw.log" 2>&1; then
  :
else
  start_status="$?"
  collect_start_diagnostics >/dev/null 2>&1 || :
  sanitize_cli_log "$WORKDIR/start.raw.log" "$ARTIFACTS_DIR/start.log"
  fail "$start_status" "Supabase local database start failed"
fi
sanitize_cli_log "$WORKDIR/start.raw.log" "$ARTIFACTS_DIR/start.log"

if ! docker container inspect --format '{{json .}}' "$CONTAINER_NAME" >"$WORKDIR/container-inspect.json" 2>"$WORKDIR/container-inspect.stderr"; then
  fail 1 "generated Supabase database container is missing"
fi
if ! CONTAINER_ID="$(owned_identity container "$WORKDIR/container-inspect.json" 2>"$WORKDIR/container-inspect.stderr")"; then
  fail 1 "generated Supabase database container ownership could not be verified"
fi
CONTAINER_OWNED=1
if ! check_container_binding "$WORKDIR/container-inspect.json" >"$ARTIFACTS_DIR/container-binding-start.log" 2>"$WORKDIR/container-binding-check.stderr"; then
  fail 1 "generated Supabase database container is not bound to loopback"
fi
if ! check_container_network "$WORKDIR/container-inspect.json" "$NETWORK_ID" >"$ARTIFACTS_DIR/container-network-start.log" 2>"$WORKDIR/container-network-check.stderr"; then
  fail 1 "generated Supabase database container is not attached to the owned CI bridge"
fi
printf 'Verified database container ownership and loopback binding.\n' >"$ARTIFACTS_DIR/container-check.log"
if ! docker volume inspect --format '{{json .}}' "$VOLUME_NAME" >"$WORKDIR/volume-inspect.json" 2>"$WORKDIR/volume-inspect.stderr"; then
  fail 1 "generated Supabase database volume is missing"
fi
if ! VOLUME_NAME="$(owned_identity volume "$WORKDIR/volume-inspect.json" 2>"$WORKDIR/volume-inspect.stderr")"; then
  fail 1 "generated Supabase database volume ownership could not be verified"
fi
VOLUME_OWNED=1

# Reset may replace the container or volume. Cleanup must re-verify by the generated
# project name and immutable project/workdir labels if reset exits partway through.
CONTAINER_ID=""
if (
  cd "$WORKDIR" &&
  PATH="$GUARD_BIN:$PATH" \
  TOKEN_PLANET_REAL_DOCKER="$DOCKER_BIN" \
  TOKEN_PLANET_GUARD_PROJECT_ID="$PROJECT_ID" \
  TOKEN_PLANET_GUARD_WORKDIR="$WORKDIR" \
  TOKEN_PLANET_GUARD_NETWORK_ID="$NETWORK_ID" \
  TOKEN_PLANET_GUARD_NETWORK_NAME="$NETWORK_NAME" \
  TOKEN_PLANET_GUARD_PHASE=reset \
  "$SUPABASE_BIN" db reset --local --no-seed --network-id "$NETWORK_ID"
) >"$WORKDIR/reset.raw.log" 2>&1; then
  :
else
  reset_status="$?"
  collect_reset_diagnostics >/dev/null 2>&1 || :
  { printf '[reset CLI output redacted; see reset-diagnostics.json]\n' >"$ARTIFACTS_DIR/reset.log"; } 2>/dev/null || :
  fail "$reset_status" "local migration reset failed"
fi
sanitize_cli_log "$WORKDIR/reset.raw.log" "$ARTIFACTS_DIR/reset.log"
python3 "$SCRIPT_DIR/prepare_migrations.py" verify --source "$SOURCE_MIGRATIONS" --output "$WORKDIR/supabase/migrations" >"$ARTIFACTS_DIR/verify-after-reset.log" 2>&1 || fail 1 "migration staging changed during reset"

# A local reset may restart the database container, so refresh its verified ID.
if ! docker container inspect --format '{{json .}}' "$CONTAINER_NAME" >"$WORKDIR/container-after-reset.json" 2>"$WORKDIR/container-after-reset.stderr"; then
  fail 1 "generated Supabase database container is missing after reset"
fi
if ! CONTAINER_ID="$(owned_identity container "$WORKDIR/container-after-reset.json" 2>"$WORKDIR/container-after-reset.stderr")"; then
  fail 1 "generated Supabase database container ownership changed after reset"
fi
if ! check_container_binding "$WORKDIR/container-after-reset.json" >"$ARTIFACTS_DIR/container-binding-reset.log" 2>"$WORKDIR/container-after-reset-binding.stderr"; then
  fail 1 "generated Supabase database container is not bound to loopback after reset"
fi
if ! check_container_network "$WORKDIR/container-after-reset.json" "$NETWORK_ID" >"$ARTIFACTS_DIR/container-network-reset.log" 2>"$WORKDIR/container-after-reset-network.stderr"; then
  fail 1 "generated Supabase database container is not attached to the owned CI bridge after reset"
fi
printf 'Verified database container identity and loopback binding after reset.\n' >>"$ARTIFACTS_DIR/container-check.log"
if ! docker network inspect --format '{{json .}}' "$NETWORK_ID" >"$WORKDIR/network-after-reset.json" 2>"$WORKDIR/network-after-reset.stderr"; then
  fail 1 "generated CI bridge is missing after reset"
fi
if ! NETWORK_ID="$(owned_identity network "$WORKDIR/network-after-reset.json" 2>"$WORKDIR/network-after-reset.stderr")"; then
  fail 1 "generated CI bridge ownership changed after reset"
fi
if ! check_network_options "$WORKDIR/network-after-reset.json" >"$WORKDIR/network-after-reset-options.log" 2>&1; then
  fail 1 "generated CI bridge lost loopback host binding after reset"
fi
printf 'Verified reset kept the database on the owned loopback CI bridge.\n' >>"$ARTIFACTS_DIR/network.log"

TEST_DEST="/tmp/token-planet-ci-tests"
docker exec "$CONTAINER_ID" mkdir -p "$TEST_DEST" >"$ARTIFACTS_DIR/test-copy.log" 2>&1 || fail 1 "could not create the in-container SQL test directory"
docker cp "$WORKDIR/supabase/tests/." "$CONTAINER_ID:$TEST_DEST" >>"$ARTIFACTS_DIR/test-copy.log" 2>&1 || fail 1 "could not copy SQL tests into the generated database container"
if ! docker exec "$CONTAINER_ID" psql -X -qAt -v ON_ERROR_STOP=1 -U postgres -d postgres \
  -c 'select version from supabase_migrations.schema_migrations order by version' \
  >"$ARTIFACTS_DIR/database-history.log" 2>"$WORKDIR/database-history-error.log"; then
  fail 1 "could not read synthetic migration history from the disposable database"
fi
if ! check_history "$ARTIFACTS_DIR/manifest.json" "$ARTIFACTS_DIR/database-history.log" >"$ARTIFACTS_DIR/history-check.log" 2>&1; then
  fail 1 "disposable database migration history differs from the manifest"
fi

test_count=0
shopt -s nullglob
for test_file in "$WORKDIR"/supabase/tests/*.sql; do
  test_count=$((test_count + 1))
  test_name="$(basename -- "$test_file" .sql)"
  tap_file="$WORKDIR/test-$test_name.tap"
  test_log="$WORKDIR/test-$test_name.log"
  psql_args=(-X -qAt -v ON_ERROR_STOP=1 -U postgres -d postgres)
  case "$test_name" in
    shop_guest_import_v2_public|shop_guest_import_v2_public_scene|shop_guest_import_v2_validation|shop_guest_import_v2_writer)
      psql_args+=(-v "rollback_probe=ci_$PROJECT_SUFFIX")
      ;;
  esac
  if docker exec "$CONTAINER_ID" psql "${psql_args[@]}" -f "$TEST_DEST/$test_name.sql" >"$tap_file" 2>"$test_log"; then
    :
  else
    psql_status="$?"
    printf 'TAP status=ERROR psql_exit=%s diagnostics=redacted\n' "$psql_status" >"$ARTIFACTS_DIR/test-$test_name.tap-summary.log"
    fail "$psql_status" "pgTAP suite failed while executing $test_name.sql"
  fi
  if check_tap "$tap_file" >"$ARTIFACTS_DIR/test-$test_name.tap-summary.log" 2>"$WORKDIR/test-$test_name-check.log"; then
    :
  else
    fail 1 "pgTAP output was incomplete or contained a failure in $test_name.sql"
  fi
done
shopt -u nullglob
if [[ "$test_count" -eq 0 ]]; then
  fail 1 "no SQL pgTAP suites were found"
fi
python3 "$SCRIPT_DIR/prepare_migrations.py" verify --source "$SOURCE_MIGRATIONS" --output "$WORKDIR/supabase/migrations" >"$ARTIFACTS_DIR/verify-after-tests.log" 2>&1 || fail 1 "migration staging changed during SQL tests"
# Keep raw harness diagnostics private. Export only the seven fixed results.
concurrency_status=0
if bash "$REPO_ROOT/supabase/tests/invite_lifecycle_concurrency.sh" \
  --ci --workdir "$WORKDIR" --container "$CONTAINER_NAME" \
  --container-id "$CONTAINER_ID" --network-id "$NETWORK_ID" \
  >"$WORKDIR/invite-concurrency.stdout" 2>"$WORKDIR/invite-concurrency.stderr"; then
  if python3 - "$WORKDIR/invite-concurrency.stdout" "$ARTIFACTS_DIR/invite-lifecycle-concurrency-summary.log" <<'PY'
import pathlib
import sys
scenarios = (
    "same-code competition and retry", "last-slot competition", "accept versus revoke",
    "accept versus transfer", "expiry during world-lock wait",
    "same-user different-world competition", "independently committed rate-limit counter",
)
lines = pathlib.Path(sys.argv[1]).read_text(errors="replace").splitlines()
expected = ["PASS " + name for name in scenarios]
if any(lines.count(line) != 1 for line in expected):
    raise SystemExit(1)
pathlib.Path(sys.argv[2]).write_text("\n".join(expected) + "\n")
PY
  then
    :
  else
    concurrency_status=1
  fi
else
  concurrency_status="$?"
fi
if [[ "$concurrency_status" != 0 ]]; then
  printf 'concurrency_status=FAIL exit=%s diagnostics=redacted\n' "$concurrency_status" >"$ARTIFACTS_DIR/invite-lifecycle-concurrency-summary.log"
  fail "$concurrency_status" "invite lifecycle concurrency failed; diagnostics redacted"
fi
log "Completed local replay and $test_count SQL pgTAP suites and seven invite concurrency scenarios for $PROJECT_ID"
exit 0
