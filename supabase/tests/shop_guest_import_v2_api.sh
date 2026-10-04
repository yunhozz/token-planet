#!/usr/bin/env bash
set -euo pipefail
set +x

# Task 9 local PostgREST lifecycle. It uses one pinned disposable database and
# never changes Supabase services, pulls images, or contacts a hosted endpoint.
readonly TEST_PROJECT_ID='token-planet-shop-revamp-test'
readonly TEST_WORKDIR='/private/tmp/token-planet-shop-revamp-test'
readonly TEST_CONFIG="$TEST_WORKDIR/supabase/config.toml"
readonly TEST_CONTEXT='desktop-linux'
readonly EXPECTED_DOCKER_ENDPOINT='unix:///Users/yunho/.docker/run/docker.sock'
readonly TEST_DB_CONTAINER="supabase_db_$TEST_PROJECT_ID"
readonly TEST_DB_CONTAINER_ID='5c438ca666762433ccc04c237fd803f268953ac7d608b97394c790ad09651e33'
readonly TEST_DB_IMAGE_ID='sha256:658d1c9b09ae4f61b8e95087b6859181b4b7d6940d769cf7b605609c8aad43e9'
readonly TEST_DB_VOLUME="supabase_db_$TEST_PROJECT_ID"
readonly TEST_DB_PORT='55432'
readonly EXPECTED_DB_IDENTITY='postgres|postgres|5432'
readonly EXPECTED_CONFIG_SHA256='531e625eb1647a12a8faa886b72401260596b753b44b7e69e8528ac0f6a9e3a1'
readonly EXPECTED_CLI_VERSION='2.119.0'
readonly POSTGREST_IMAGE='ghcr.io/supabase/cli/postgrest:v16.4-r0@sha256:63a8d4acfdeb107b6568f4582759c78072100ef07951a7fbe58c9a51241138a7'
readonly API_HOST='127.0.0.1'
readonly API_RUN_PREFIX='/private/tmp/shop-guest-import-v2-e2e.'
readonly API_MANIFEST_NAME='runtime.env'
readonly POSTGREST_ENV_NAME='postgrest.env'
readonly AUTH_SHIM_MANIFEST_NAME='auth-shim.json'
readonly API_OWNER_LABEL='io.token-planet.shop-guest-import-v2.run'
readonly API_NETWORK="supabase_network_$TEST_PROJECT_ID"
READY_API_PORT=''

fail() {
  printf 'shop guest import v2 API readiness: %s\n' "$1" >&2
  exit 1
}

validate_run_dir_argument() {
  local run_dir="$1"
  local mode="$2"
  python3 - "$run_dir" "$API_RUN_PREFIX" "$mode" <<'PY'
import os
import re
import stat
import sys

run_dir, prefix, mode = sys.argv[1:]
def reject(message):
    raise SystemExit(f"shop guest import v2 API readiness: {message}")

run_id = run_dir[len(prefix):] if run_dir.startswith(prefix) else ""
if not re.fullmatch(r"[a-f0-9]{24}", run_id):
    reject("run directory must be a canonical direct child with a 24-character run ID")
if os.path.normpath(run_dir) != run_dir or os.path.realpath(prefix) != prefix:
    reject("run directory path is not canonical")
if os.path.lexists(run_dir):
    info = os.lstat(run_dir)
    if stat.S_ISLNK(info.st_mode) or not stat.S_ISDIR(info.st_mode):
        reject("run directory must not be a symlink and must be a directory")
    if info.st_uid != os.getuid():
        reject("run directory is not owned by the current user")
    if mode == "new":
        reject("run directory already exists; refusing to reuse it")
    if os.path.realpath(run_dir) != run_dir:
        reject("run directory path is not canonical")
elif mode == "existing":
    reject("the owned Task 9 run directory is missing")
PY
}

read_owned_manifest() {
  local run_dir="$1"
  local manifest="$run_dir/$API_MANIFEST_NAME"
  [[ -f "$manifest" && ! -L "$manifest" ]] \
    || fail 'the owned Task 9 API manifest is missing or is a symlink'
  python3 - "$manifest" "$run_dir" "$POSTGREST_IMAGE" <<'PY'
import hashlib
import json
import hashlib
import os
import re
import stat
import sys

manifest_path, run_dir, image = sys.argv[1:]
def reject(message):
    raise SystemExit(f"shop guest import v2 API readiness: {message}")

info = os.stat(manifest_path, follow_symlinks=False)
if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) != 0o600:
    reject("API manifest must be a current-user-owned mode-0600 regular file")
try:
    with open(manifest_path, encoding="utf-8") as stream:
        data = json.load(stream)
except (OSError, json.JSONDecodeError):
    reject("API manifest is not valid JSON")
run_id = data.get("run_id")
if not isinstance(run_id, str) or not re.fullmatch(r"[a-f0-9]{24}", run_id):
    reject("API manifest run ID is invalid")
if run_dir != "/private/tmp/shop-guest-import-v2-e2e." + run_id:
    reject("API manifest does not belong to this run directory")
name = "guest-import-v2-api-" + run_id
env_file = run_dir + "/postgrest.env"
if data.get("container_name") != name or data.get("env_file") != env_file:
    reject("API manifest resource names do not match the run ID")
if data.get("image") != image or data.get("owner_label") != run_id:
    reject("API manifest image or ownership label mismatch")
if data.get("container_name_absent_before_start") is not True:
    reject("API manifest lacks the required initial container-name absence proof")
port = data.get("api_port")
if not isinstance(port, int) or not 49152 <= port <= 65535:
    reject("API manifest port is invalid")
if data.get("api_url") != f"http://127.0.0.1:{port}":
    reject("API manifest URL does not match its loopback port")
container_id = data.get("container_id")
if not isinstance(container_id, str) or not re.fullmatch(r"[a-f0-9]{64}", container_id):
    reject("API manifest container ID is invalid")
print("\t".join((run_id, name, env_file, str(port), container_id)))
PY
}

verify_owned_container() {
  local container_name="$1"
  local run_id="$2"
  local expected_container_id="$3"
  local api_port="$4"
  local container_json
  container_json="$(docker --context "$TEST_CONTEXT" inspect --format '{{json .}}' "$container_name" 2>/dev/null)" \
    || fail 'the owned Task 9 API container is unavailable; refusing artifact cleanup'
  python3 - "$container_json" "$container_name" "$run_id" "$expected_container_id" \
    "$api_port" "$POSTGREST_IMAGE" "$API_OWNER_LABEL" "$API_NETWORK" <<'PY'
import json
import sys

raw, name, run_id, expected_id, port, image, owner_label, network = sys.argv[1:]
container = json.loads(raw)
config = container.get("Config") or {}
labels = config.get("Labels") or {}
if container.get("Id") != expected_id or container.get("Name") != "/" + name:
    raise SystemExit("shop guest import v2 API readiness: API container identity mismatch")
if config.get("Image") != image or labels.get(owner_label) != run_id:
    raise SystemExit("shop guest import v2 API readiness: API container ownership proof mismatch")
bindings = ((container.get("HostConfig") or {}).get("PortBindings") or {}).get("3000/tcp") or []
if bindings != [{"HostIp": "127.0.0.1", "HostPort": port}]:
    raise SystemExit("shop guest import v2 API readiness: API container loopback binding mismatch")
networks = (container.get("NetworkSettings") or {}).get("Networks") or {}
if network not in networks:
    raise SystemExit("shop guest import v2 API readiness: API container network mismatch")
PY
}

cleanup_owned_files() {
  local run_dir="$1"
  local env_file="$2"
  local manifest="$run_dir/$API_MANIFEST_NAME"
  local auth_shim_manifest="${3:-}"
  local -a owned_files=("$manifest" "$env_file")
  if [[ -n "$auth_shim_manifest" ]]; then
    owned_files+=("$auth_shim_manifest")
  fi
  for path in "${owned_files[@]}"; do
    [[ -e "$path" || -L "$path" ]] || continue
    [[ -f "$path" && ! -L "$path" ]] \
      || fail 'owned API cleanup encountered an unexpected artifact type'
    [[ "$(python3 -c 'import os,sys; print(os.stat(sys.argv[1], follow_symlinks=False).st_uid)' "$path")" == "$(id -u)" ]] \
      || fail 'owned API cleanup encountered an artifact owned by another user'
  done
  rm -f -- "${owned_files[@]}"
  if ! rmdir -- "$run_dir" 2>/dev/null; then
    printf 'shop guest import v2 API: left run directory in place because it contains other files: %s\n' "$run_dir" >&2
  fi
}

quiesce_owned_api() {
  local run_dir="$1"
  local fields run_id container_name env_file api_port container_id path container_ids named_container_ids remaining_ids
  validate_run_dir_argument "$run_dir" existing
  fields="$(read_owned_manifest "$run_dir")" || fail 'could not read owned API manifest'
  IFS=$'\t' read -r run_id container_name env_file api_port container_id <<<"$fields"
  for path in "$env_file" "$run_dir/$AUTH_SHIM_MANIFEST_NAME"; do
    [[ -f "$path" && ! -L "$path" ]] \
      || fail 'an owned API or auth-bridge proof file is missing or is a symlink; preserving run files'
    [[ "$(python3 -c 'import os,sys; print(os.stat(sys.argv[1], follow_symlinks=False).st_uid)' "$path")" == "$(id -u)" ]] \
      || fail 'an owned API or auth-bridge proof file belongs to another user; preserving run files'
    [[ "$(stat -f '%Lp' "$path" 2>/dev/null)" == '600' ]] \
      || fail 'an owned API or auth-bridge proof file is not mode 0600; preserving run files'
  done
  context="$(docker context show 2>/dev/null)" || fail 'could not identify Docker context for API quiesce'
  [[ "$context" == "$TEST_CONTEXT" ]] || fail 'refusing API quiesce in a Docker context other than desktop-linux'
  endpoint="$(docker --context "$TEST_CONTEXT" context inspect "$TEST_CONTEXT" --format '{{(index .Endpoints "docker").Host}}' 2>/dev/null)" \
    || fail 'could not inspect the Docker endpoint for API quiesce'
  [[ "$endpoint" == "$EXPECTED_DOCKER_ENDPOINT" ]] \
    || fail 'refusing API quiesce through a non-local or unexpected Docker endpoint'
  container_ids="$(docker --context "$TEST_CONTEXT" ps -aq --no-trunc \
    --filter "label=$API_OWNER_LABEL=$run_id" 2>/dev/null)" \
    || fail 'could not list this run’s API containers; preserving all run files'
  if [[ -z "$container_ids" ]]; then
    named_container_ids="$(docker --context "$TEST_CONTEXT" ps --all --quiet --no-trunc \
      --filter "name=^/${container_name}$" 2>/dev/null)" \
      || fail 'could not verify that the owned API container is absent; preserving all run files'
    [[ -z "$named_container_ids" ]] \
      || fail 'an API container with the owned name exists without this run’s ownership label; preserving all run files'
    printf 'shop guest import v2 API: owned container is already absent; preserved all run files\n'
    return 0
  fi
  [[ "$container_ids" == "$container_id" ]] \
    || fail 'run ownership label resolved to an unexpected container ID; preserving all run files'
  verify_owned_container "$container_name" "$run_id" "$container_id" "$api_port"
  docker --context "$TEST_CONTEXT" rm --force "$container_id" >/dev/null \
    || fail 'could not remove the verified owned API container; preserving all run files'
  remaining_ids="$(docker --context "$TEST_CONTEXT" ps -aq --no-trunc \
    --filter "label=$API_OWNER_LABEL=$run_id" 2>/dev/null)" \
    || fail 'could not verify API container removal; preserving all run files'
  [[ -z "$remaining_ids" ]] \
    || fail 'the owned API container remains after removal; preserving all run files'
  named_container_ids="$(docker --context "$TEST_CONTEXT" ps --all --quiet --no-trunc \
    --filter "name=^/${container_name}$" 2>/dev/null)" \
    || fail 'could not verify the owned API container name is absent; preserving all run files'
  [[ -z "$named_container_ids" ]] \
    || fail 'a container with the owned API name remains after quiesce; preserving all run files'
  printf 'shop guest import v2 API: quiesced verified owned container %s; preserved all run files\n' "$container_name"
}

cleanup_auth_bridge() {
  local run_dir="$1"
  python3 - "$run_dir" "$API_RUN_PREFIX" "$AUTH_SHIM_MANIFEST_NAME" \
    "$TEST_CONTEXT" "$EXPECTED_DOCKER_ENDPOINT" "$EXPECTED_DB_IDENTITY" \
    "$TEST_DB_CONTAINER" "$TEST_DB_CONTAINER_ID" <<'PY'
import hashlib
import json
import os
import re
import selectors
import stat
import subprocess
import sys
import time

(run_dir, prefix, manifest_name, docker_context, docker_endpoint,
 database_identity, container_name, container_id) = sys.argv[1:]

def reject(message):
    raise SystemExit("shop guest import v2 API cleanup: " + message)

run_id = run_dir[len(prefix):] if run_dir.startswith(prefix) else ""
if os.getuid() != 501:
    reject("auth-bridge cleanup must run as the approved UID 501")
if not re.fullmatch(r"[a-f0-9]{24}", run_id) or run_dir != prefix + run_id:
    reject("auth-bridge cleanup run path is not the canonical owned path")
if os.path.normpath(run_dir) != run_dir or os.path.realpath(prefix) != prefix or os.path.realpath(run_dir) != run_dir:
    reject("auth-bridge cleanup run path is not canonical")
try:
    run_fd = os.open(run_dir, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    manifest_fd = os.open(manifest_name, os.O_RDONLY | os.O_NOFOLLOW, dir_fd=run_fd)
except OSError:
    reject("auth-bridge manifest is unavailable or unsafe; preserving all run files")
try:
    run_info = os.fstat(run_fd)
    manifest_info = os.fstat(manifest_fd)
    if not stat.S_ISDIR(run_info.st_mode) or run_info.st_uid != 501 or stat.S_IMODE(run_info.st_mode) != 0o700:
        reject("auth-bridge run directory ownership or mode changed; preserving all run files")
    if not stat.S_ISREG(manifest_info.st_mode) or manifest_info.st_uid != 501 or stat.S_IMODE(manifest_info.st_mode) != 0o600:
        reject("auth-bridge manifest ownership or mode changed; preserving all run files")
    with os.fdopen(manifest_fd, "r", encoding="utf-8") as stream:
        manifest = json.load(stream)
    if manifest.get("version") != 1 or manifest.get("run_id") != run_id or manifest.get("run_dir") != run_dir or manifest.get("owner_uid") != 501:
        reject("auth-bridge manifest identity changed; preserving all run files")
    pinned = manifest.get("pinned_target")
    if not isinstance(pinned, dict) or pinned.get("docker_context") != docker_context or pinned.get("docker_endpoint") != docker_endpoint or pinned.get("database_identity") != database_identity or pinned.get("container_name") != container_name or pinned.get("container_id") != container_id:
        reject("auth-bridge pinned database target changed; preserving all run files")
    function_record = manifest.get("function")
    expected_name = "task9_claim_bridge_" + run_id
    if not isinstance(function_record, dict) or function_record.get("schema") != "private" or function_record.get("name") != expected_name or function_record.get("signature") != f"private.{expected_name}()" or function_record.get("initial_absence") is not True:
        reject("auth-bridge manifest function identity is invalid; preserving all run files")
    status = manifest.get("status")
    ddl_started = manifest.get("ddl_started")
    created_oid = function_record.get("created_oid")
    if status == "pre_ddl_baseline_ready":
        if ddl_started is not False or created_oid is not None:
            reject("auth-bridge pre-DDL state is inconsistent; preserving all run files")
    elif status in {"auth_bridge_commit_pending", "auth_bridge_committed", "auth_bridge_commit_response_lost_recovered"}:
        if ddl_started is not True or not isinstance(created_oid, str) or not re.fullmatch(r"[1-9][0-9]*", created_oid):
            reject("auth-bridge saved OID checkpoint is missing; preserving all run files")
        required = ("created_owner", "created_signature", "created_definition_sha256", "created_acl",
                    "created_proconfig", "created_security_definer", "created_database_identity")
        if any(key not in function_record for key in required):
            reject("auth-bridge created-function metadata checkpoint is incomplete; preserving all run files")
    else:
        reject("auth-bridge manifest stage is unknown or incomplete; preserving all run files")

    docker_env = {key: value for key, value in os.environ.items() if not key.startswith("DOCKER_")}
    def docker_capture(args, input_text=None, max_bytes=65536):
        try:
            result = subprocess.run(
                ["docker", *args], input=input_text, text=True, capture_output=True,
                env=docker_env, timeout=10, check=True,
            )
        except (OSError, subprocess.CalledProcessError, subprocess.TimeoutExpired):
            reject("could not verify the pinned database target or auth-bridge metadata; preserving all run files")
        if len(result.stdout) > max_bytes:
            reject("auth-bridge cleanup query output exceeded its size limit; preserving all run files")
        return result.stdout.strip()

    def abort_cleanup_child(child):
        try:
            if child.stdin is not None and not child.stdin.closed:
                child.stdin.close()
        except OSError:
            pass
        try:
            child.wait(timeout=3)
        except (OSError, subprocess.TimeoutExpired):
            try:
                child.terminate()
                child.wait(timeout=3)
            except (OSError, subprocess.TimeoutExpired):
                try:
                    child.kill()
                    child.wait(timeout=3)
                except (OSError, subprocess.TimeoutExpired):
                    pass

    def start_cleanup_transaction(transaction_sql):
        command = [
            "docker", "--context", docker_context, "exec", "-i", container_id,
            "psql", "-X", "-q", "-A", "-t", "-v", "ON_ERROR_STOP=1",
            "-U", "postgres", "-d", "postgres",
        ]
        try:
            child = subprocess.Popen(
                command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL, bufsize=0, env=docker_env,
            )
        except OSError:
            reject("could not open the pinned auth-bridge cleanup transaction; preserving all run files")
        selector = selectors.DefaultSelector()
        selector.register(child.stdout, selectors.EVENT_READ)
        output_buffer = bytearray()
        child.stdin.write(transaction_sql.encode("utf-8"))
        child.stdin.flush()
        return child, selector, output_buffer

    def read_cleanup_marker(child, selector, output_buffer, marker, max_bytes, timeout):
        lines = []
        consumed = 0
        deadline = time.monotonic() + timeout
        marker_bytes = marker.encode("ascii")
        while True:
            while b"\n" in output_buffer:
                line, _, remainder = output_buffer.partition(b"\n")
                output_buffer[:] = remainder
                if line.strip() == marker_bytes:
                    return b"\n".join(lines).strip()
                lines.append(line)
                consumed += len(line) + 1
                if consumed > max_bytes:
                    reject("auth-bridge cleanup transaction output exceeded its size limit; preserving all run files")
            if consumed + len(output_buffer) > max_bytes:
                reject("auth-bridge cleanup transaction output exceeded its size limit; preserving all run files")
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not selector.select(remaining):
                reject("auth-bridge cleanup transaction marker timed out; preserving all run files")
            chunk = os.read(child.stdout.fileno(), 65536)
            if not chunk:
                reject("auth-bridge cleanup transaction ended before its proof marker; preserving all run files")
            output_buffer.extend(chunk)

    if docker_capture(["context", "show"]) != docker_context:
        reject("auth-bridge cleanup Docker context mismatch; preserving all run files")
    endpoint = docker_capture([
        "--context", docker_context, "context", "inspect", docker_context,
        "--format", '{{(index .Endpoints "docker").Host}}',
    ])
    if endpoint != docker_endpoint:
        reject("auth-bridge cleanup Docker endpoint mismatch; preserving all run files")
    container_text = docker_capture([
        "--context", docker_context, "inspect", "--format", "{{json .}}", container_name,
    ])
    try:
        container = json.loads(container_text)
    except json.JSONDecodeError:
        reject("auth-bridge cleanup database container identity is invalid; preserving all run files")
    if (
        not isinstance(container, dict)
        or container.get("Id") != container_id
        or container.get("Name") != "/" + container_name
        or (container.get("State") or {}).get("Status") != "running"
    ):
        reject("auth-bridge cleanup database container identity changed; preserving all run files")

    metadata_query = f"""-- TASK9_AUTH_BRIDGE_CLEANUP_METADATA
SELECT pg_catalog.jsonb_build_object(
  'database_identity', pg_catalog.current_database() || '|' || current_user || '|' || pg_catalog.current_setting('port'),
  'oid', p.oid::text,
  'owner', owner_role.rolname,
  'signature', n.nspname || '.' || p.proname || '(' || pg_catalog.pg_get_function_identity_arguments(p.oid) || ')',
  'definition', pg_catalog.pg_get_functiondef(p.oid),
  'acl', p.proacl::text,
  'proconfig', p.proconfig,
  'security_definer', p.prosecdef
)::text
FROM pg_catalog.pg_proc p
JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace
JOIN pg_catalog.pg_roles owner_role ON owner_role.oid = p.proowner
WHERE p.oid = pg_catalog.to_regprocedure('private.{expected_name}()');"""
    metadata_text = docker_capture([
        "--context", docker_context, "exec", "-i", container_id,
        "psql", "-X", "-q", "-A", "-t", "-v", "ON_ERROR_STOP=1",
        "-U", "postgres", "-d", "postgres",
    ], input_text=metadata_query)
    metadata_absent = not metadata_text
    if metadata_absent:
        if status not in {"pre_ddl_baseline_ready", "auth_bridge_commit_pending", "auth_bridge_committed", "auth_bridge_commit_response_lost_recovered"}:
            reject("auth-bridge function absence is not recoverable from this manifest stage; preserving all run files")
    else:
        try:
            metadata = json.loads(metadata_text)
        except json.JSONDecodeError:
            reject("auth-bridge cleanup metadata query returned invalid JSON; preserving all run files")
        expected_keys = {
            "database_identity", "oid", "owner", "signature", "definition", "acl",
            "proconfig", "security_definer",
        }
        if not isinstance(metadata, dict) or set(metadata) != expected_keys:
            reject("auth-bridge cleanup metadata fields are incomplete; preserving all run files")
        if metadata.get("database_identity") != database_identity or metadata.get("database_identity") != function_record.get("created_database_identity"):
            reject("auth-bridge cleanup database identity differs from the saved proof; preserving all run files")
        if metadata.get("oid") != created_oid:
            reject("live auth-bridge OID does not match the saved OID; preserving all run files")
        if metadata.get("owner") != function_record.get("created_owner") or metadata.get("owner") != "postgres":
            reject("live auth-bridge owner does not match the saved proof; preserving all run files")
        if metadata.get("signature") != function_record.get("created_signature") or metadata.get("signature") != function_record.get("signature"):
            reject("live auth-bridge signature does not match the saved proof; preserving all run files")
        definition = metadata.get("definition")
        if not isinstance(definition, str) or not definition:
            reject("live auth-bridge definition is missing; preserving all run files")
        definition_hash = hashlib.sha256(definition.encode("utf-8")).hexdigest()
        if definition_hash != function_record.get("created_definition_sha256"):
            reject("live auth-bridge definition hash does not match the saved proof; preserving all run files")
        if metadata.get("acl") != function_record.get("created_acl") or not isinstance(metadata.get("acl"), str):
            reject("live auth-bridge ACL does not match the saved proof; preserving all run files")
        if metadata.get("proconfig") != function_record.get("created_proconfig") or metadata.get("proconfig") != ['search_path=""']:
            reject("live auth-bridge search path does not match the saved proof; preserving all run files")
        if metadata.get("security_definer") is not function_record.get("created_security_definer") or metadata.get("security_definer") is not False:
            reject("live auth-bridge security mode does not match the saved proof; preserving all run files")

        acl_entries = metadata["acl"].strip("{}").split(",")
        authenticated_execute = any(
            entry.split("=", 1)[0] == "authenticated"
            and "X" in entry.split("=", 1)[1].split("/", 1)[0]
            for entry in acl_entries if "=" in entry
        )
        public_execute = any(
            entry.split("=", 1)[0] == ""
            and "X" in entry.split("=", 1)[1].split("/", 1)[0]
            for entry in acl_entries if "=" in entry
        )
        other_role_execute = any(
            entry.split("=", 1)[0] not in {"postgres", "authenticated", ""}
            and "X" in entry.split("=", 1)[1].split("/", 1)[0]
            for entry in acl_entries if "=" in entry
        )
        if not authenticated_execute or public_execute or other_role_execute:
            reject("live auth-bridge ACL is not authenticated-only; preserving all run files")

    baseline_query = f"""-- TASK9_AUTH_BRIDGE_CLEANUP_BASELINE
BEGIN TRANSACTION READ ONLY;
WITH role_defaults AS (
  SELECT COALESCE(pg_catalog.jsonb_agg(
    pg_catalog.jsonb_build_object(
      'database', COALESCE(d.datname, '*'),
      'role', COALESCE(r.rolname, '*'),
      'settings', s.setconfig
    ) ORDER BY COALESCE(d.datname, ''), COALESCE(r.rolname, '')
  ), '[]'::jsonb) AS value
  FROM pg_catalog.pg_db_role_setting s
  LEFT JOIN pg_catalog.pg_database d ON d.oid = s.setdatabase
  LEFT JOIN pg_catalog.pg_roles r ON r.oid = s.setrole
), functions AS (
  SELECT COALESCE(pg_catalog.jsonb_agg(
    pg_catalog.jsonb_build_object(
      'schema', n.nspname,
      'name', p.proname,
      'identity_arguments', pg_catalog.pg_get_function_identity_arguments(p.oid),
      'definition', pg_catalog.pg_get_functiondef(p.oid),
      'owner', owner_role.rolname,
      'acl', p.proacl::text,
      'proconfig', p.proconfig,
      'security_definer', p.prosecdef,
      'kind', p.prokind,
      'volatility', p.provolatile,
      'strict', p.proisstrict
    ) ORDER BY n.nspname, p.proname, pg_catalog.pg_get_function_identity_arguments(p.oid)
  ), '[]'::jsonb) AS value
  FROM pg_catalog.pg_proc p
  JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace
  JOIN pg_catalog.pg_roles owner_role ON owner_role.oid = p.proowner
  WHERE n.nspname IN ('public', 'private', 'auth')
    AND p.prokind IN ('f', 'p')
    AND p.oid <> pg_catalog.to_regprocedure('private.{expected_name}()')
), auth_uid AS (
  SELECT pg_catalog.jsonb_build_object(
    'definition', pg_catalog.pg_get_functiondef(p.oid),
    'owner', owner_role.rolname,
    'acl', p.proacl::text,
    'proconfig', p.proconfig,
    'identity_arguments', pg_catalog.pg_get_function_identity_arguments(p.oid)
  ) AS value
  FROM pg_catalog.pg_proc p
  JOIN pg_catalog.pg_roles owner_role ON owner_role.oid = p.proowner
  WHERE p.oid = pg_catalog.to_regprocedure('auth.uid()')
), migration_history AS (
  SELECT pg_catalog.jsonb_build_object(
    'rows', count(*)::integer,
    'max_version', max(m.version)::text,
    'versions', COALESCE(pg_catalog.jsonb_agg(m.version::text ORDER BY m.version), '[]'::jsonb),
    'version_hash', pg_catalog.md5(COALESCE(pg_catalog.string_agg(m.version::text, pg_catalog.chr(10) ORDER BY m.version), '')),
    'rows_snapshot', COALESCE(pg_catalog.jsonb_agg(pg_catalog.to_jsonb(m) ORDER BY m.version), '[]'::jsonb)
  ) AS value
  FROM supabase_migrations.schema_migrations m
), schemas AS (
  SELECT COALESCE(pg_catalog.jsonb_agg(
    pg_catalog.jsonb_build_object('name', n.nspname, 'owner', r.rolname, 'acl', n.nspacl::text)
    ORDER BY n.nspname
  ), '[]'::jsonb) AS value
  FROM pg_catalog.pg_namespace n
  JOIN pg_catalog.pg_roles r ON r.oid = n.nspowner
  WHERE n.nspname IN ('public', 'private', 'auth')
), relations AS (
  SELECT COALESCE(pg_catalog.jsonb_agg(
    pg_catalog.jsonb_build_object(
      'schema', n.nspname,
      'name', c.relname,
      'kind', c.relkind,
      'owner', r.rolname,
      'acl', c.relacl::text,
      'persistence', c.relpersistence,
      'row_security', c.relrowsecurity,
      'force_row_security', c.relforcerowsecurity,
      'options', c.reloptions,
      'view_definition', CASE WHEN c.relkind IN ('v', 'm') THEN pg_catalog.pg_get_viewdef(c.oid, true) ELSE NULL END,
      'index_definition', CASE WHEN c.relkind = 'i' THEN pg_catalog.pg_get_indexdef(c.oid) ELSE NULL END,
      'columns', (
        SELECT COALESCE(pg_catalog.jsonb_agg(pg_catalog.jsonb_build_object(
          'number', a.attnum,
          'name', a.attname,
          'type', pg_catalog.format_type(a.atttypid, a.atttypmod),
          'not_null', a.attnotnull,
          'default', pg_catalog.pg_get_expr(d.adbin, d.adrelid),
          'generated', a.attgenerated,
          'identity', a.attidentity
        ) ORDER BY a.attnum), '[]'::jsonb)
        FROM pg_catalog.pg_attribute a
        LEFT JOIN pg_catalog.pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
        WHERE a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped
      ),
      'constraints', (
        SELECT COALESCE(pg_catalog.jsonb_agg(pg_catalog.jsonb_build_object(
          'name', con.conname,
          'type', con.contype,
          'definition', pg_catalog.pg_get_constraintdef(con.oid, true),
          'validated', con.convalidated,
          'deferrable', con.condeferrable,
          'initially_deferred', con.condeferred
        ) ORDER BY con.conname), '[]'::jsonb)
        FROM pg_catalog.pg_constraint con WHERE con.conrelid = c.oid
      ),
      'policies', (
        SELECT COALESCE(pg_catalog.jsonb_agg(pg_catalog.jsonb_build_object(
          'name', pol.polname,
          'permissive', pol.polpermissive,
          'roles', pol.polroles::text,
          'command', pol.polcmd,
          'using', pg_catalog.pg_get_expr(pol.polqual, pol.polrelid),
          'check', pg_catalog.pg_get_expr(pol.polwithcheck, pol.polrelid)
        ) ORDER BY pol.polname), '[]'::jsonb)
        FROM pg_catalog.pg_policy pol WHERE pol.polrelid = c.oid
      ),
      'triggers', (
        SELECT COALESCE(pg_catalog.jsonb_agg(pg_catalog.jsonb_build_object(
          'name', tg.tgname,
          'definition', pg_catalog.pg_get_triggerdef(tg.oid, true),
          'enabled', tg.tgenabled
        ) ORDER BY tg.tgname), '[]'::jsonb)
        FROM pg_catalog.pg_trigger tg WHERE tg.tgrelid = c.oid AND NOT tg.tgisinternal
      )
    ) ORDER BY n.nspname, c.relname
  ), '[]'::jsonb) AS value
  FROM pg_catalog.pg_class c
  JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
  JOIN pg_catalog.pg_roles r ON r.oid = c.relowner
  WHERE n.nspname IN ('public', 'private', 'auth')
    AND c.relkind IN ('r', 'p', 'v', 'm', 'f', 'i', 'S')
), extensions AS (
  SELECT COALESCE(pg_catalog.jsonb_agg(pg_catalog.jsonb_build_object(
    'name', e.extname, 'version', e.extversion, 'schema', n.nspname, 'owner', r.rolname
  ) ORDER BY e.extname), '[]'::jsonb) AS value
  FROM pg_catalog.pg_extension e
  JOIN pg_catalog.pg_namespace n ON n.oid = e.extnamespace
  JOIN pg_catalog.pg_roles r ON r.oid = e.extowner
  WHERE n.nspname IN ('public', 'private', 'auth')
), schema_snapshot AS (
  SELECT pg_catalog.jsonb_build_object(
    'schemas', schemas.value,
    'relations', relations.value,
    'functions', functions.value,
    'role_defaults', role_defaults.value,
    'auth_uid', (SELECT value FROM auth_uid),
    'extensions', extensions.value,
    'authenticated_schema_usage', pg_catalog.has_schema_privilege('authenticated', 'private', 'USAGE')
  ) AS value
  FROM schemas, relations, functions, role_defaults, extensions
)
SELECT pg_catalog.jsonb_build_object(
  'database_identity', pg_catalog.current_database() || '|' || current_user || '|' || pg_catalog.current_setting('port'),
  'target_present', pg_catalog.to_regprocedure('private.{expected_name}()') IS NOT NULL,
  'private_schema_owner', (SELECT r.rolname FROM pg_catalog.pg_namespace n JOIN pg_catalog.pg_roles r ON r.oid = n.nspowner WHERE n.nspname = 'private'),
  'private_schema_acl', (SELECT n.nspacl::text FROM pg_catalog.pg_namespace n WHERE n.nspname = 'private'),
  'authenticated_schema_usage', pg_catalog.has_schema_privilege('authenticated', 'private', 'USAGE'),
  'role_defaults', role_defaults.value,
  'existing_function_definition_and_acl', functions.value,
  'auth_uid', (SELECT value FROM auth_uid),
  'migration_history', (SELECT value FROM migration_history),
  'schema_snapshot', schema_snapshot.value,
  'schema_fingerprint', pg_catalog.md5(schema_snapshot.value::text),
  'protected_table_count', (SELECT count(*)::integer FROM pg_catalog.pg_tables WHERE schemaname IN ('public', 'private', 'auth'))
)::text
FROM role_defaults, functions, schema_snapshot, extensions, migration_history;
SELECT pg_catalog.format(
  'SELECT pg_catalog.jsonb_build_object(''schema'', %L, ''table'', %L, ''row_count'', count(*), ''rows_hash'', pg_catalog.md5(COALESCE(pg_catalog.string_agg(pg_catalog.to_jsonb(t)::text, pg_catalog.chr(10) ORDER BY pg_catalog.to_jsonb(t)::text), %L)))::text FROM %I.%I AS t',
  schemaname, tablename, '', schemaname, tablename
)
FROM pg_catalog.pg_tables
WHERE schemaname IN ('public', 'private', 'auth')
ORDER BY schemaname, tablename
\\gexec
COMMIT;"""
    read_only_prefix = "-- TASK9_AUTH_BRIDGE_CLEANUP_BASELINE\nBEGIN TRANSACTION READ ONLY;\n"
    if not baseline_query.startswith(read_only_prefix) or not baseline_query.endswith("\nCOMMIT;"):
        reject("auth-bridge cleanup baseline query framing is invalid; preserving all run files")
    baseline_query_body = baseline_query[len(read_only_prefix):-len("\nCOMMIT;")]
    target_present_sql = f"'target_present', pg_catalog.to_regprocedure('private.{expected_name}()') IS NOT NULL,"
    target_absent_sql = f"'target_absent', pg_catalog.to_regprocedure('private.{expected_name}()') IS NULL,"
    if baseline_query_body.count(target_present_sql) != 1:
        reject("auth-bridge cleanup owned-function baseline query is invalid; preserving all run files")
    postdrop_baseline_body = baseline_query_body.replace(target_present_sql, target_absent_sql, 1)
    owned_function_filter = f"\n    AND p.oid <> pg_catalog.to_regprocedure('private.{expected_name}()')"
    if postdrop_baseline_body.count(owned_function_filter) != 1:
        reject("auth-bridge cleanup owned-function filter is invalid; preserving all run files")
    postdrop_baseline_body = postdrop_baseline_body.replace(owned_function_filter, "", 1)
    baseline_record = manifest.get("baseline")
    if not isinstance(baseline_record, dict) or baseline_record.get("status") != "pre_ddl_baseline_ready":
        reject("auth-bridge protected baseline checkpoint is missing; preserving all run files")
    cleanup_baseline_query = baseline_query
    if metadata_absent:
        cleanup_baseline_query = (
            "-- TASK9_AUTH_BRIDGE_CLEANUP_BASELINE\n"
            "BEGIN TRANSACTION READ ONLY;\n"
            + postdrop_baseline_body
            + "\nCOMMIT;"
        )
    baseline_text = docker_capture([
        "--context", docker_context, "exec", "-i", container_id,
        "psql", "-X", "-q", "-A", "-t", "-v", "ON_ERROR_STOP=1",
        "-U", "postgres", "-d", "postgres",
    ], input_text=cleanup_baseline_query, max_bytes=8 * 1024 * 1024)
    try:
        baseline_lines = baseline_text.splitlines()
        observed = json.loads(baseline_lines[0])
        table_digests = [json.loads(line) for line in baseline_lines[1:] if line]
    except (IndexError, json.JSONDecodeError):
        reject("auth-bridge cleanup read-only baseline query returned invalid JSON; preserving all run files")
    if not isinstance(observed, dict):
        reject("auth-bridge cleanup baseline record is invalid; preserving all run files")
    target_field = "target_absent" if metadata_absent else "target_present"
    if observed.get("database_identity") != database_identity or observed.get(target_field) is not True:
        reject("auth-bridge cleanup baseline database identity or owned function is invalid; preserving all run files")

    migration_history = observed.get("migration_history")
    if not isinstance(migration_history, dict):
        reject("auth-bridge cleanup migration history is missing; preserving all run files")
    rows_snapshot = migration_history.get("rows_snapshot")
    versions = migration_history.get("versions")
    if not isinstance(rows_snapshot, list) or not isinstance(versions, list) or len(rows_snapshot) != 28 or len(versions) != 28:
        reject("auth-bridge cleanup full migration history snapshot is incomplete; preserving all run files")
    migration_history["rows_snapshot_md5"] = hashlib.md5(
        json.dumps(rows_snapshot, sort_keys=True, separators=(",", ":")).encode("utf-8")
    ).hexdigest()

    protected_table_count = observed.get("protected_table_count")
    if type(protected_table_count) is not int or protected_table_count != 48 or len(table_digests) != protected_table_count:
        reject("auth-bridge cleanup protected table snapshot is incomplete; preserving all run files")
    previous_table_key = None
    digest_lines = []
    for table_digest in table_digests:
        if not isinstance(table_digest, dict) or set(table_digest) != {"schema", "table", "row_count", "rows_hash"}:
            reject("auth-bridge cleanup protected table digest is invalid; preserving all run files")
        schema_name = table_digest.get("schema")
        table_name = table_digest.get("table")
        row_count = table_digest.get("row_count")
        rows_hash = table_digest.get("rows_hash")
        if schema_name not in {"public", "private", "auth"}:
            reject("auth-bridge cleanup protected table schema is invalid; preserving all run files")
        if not isinstance(table_name, str) or not table_name.isascii() or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_$]{0,62}", table_name):
            reject("auth-bridge cleanup protected table name is invalid; preserving all run files")
        if type(row_count) is not int or row_count < 0 or not isinstance(rows_hash, str) or not re.fullmatch(r"[a-f0-9]{32}", rows_hash):
            reject("auth-bridge cleanup protected table digest value is invalid; preserving all run files")
        table_key = (schema_name, table_name)
        if previous_table_key is not None and table_key <= previous_table_key:
            reject("auth-bridge cleanup protected table digests are not uniquely ordered; preserving all run files")
        previous_table_key = table_key
        digest_lines.append(f"{schema_name}.{table_name}:{row_count}:{rows_hash}\n")
    public_row_digest = hashlib.md5("".join(digest_lines).encode("ascii")).hexdigest()

    observed_baseline = {
        "preflight_database_identity": observed.get("database_identity"),
        "private_schema_owner": observed.get("private_schema_owner"),
        "private_schema_acl": observed.get("private_schema_acl"),
        "authenticated_schema_usage": observed.get("authenticated_schema_usage"),
        "role_defaults": observed.get("role_defaults"),
        "existing_function_definition_and_acl": observed.get("existing_function_definition_and_acl"),
        "auth_uid": observed.get("auth_uid"),
        "schema_snapshot": observed.get("schema_snapshot"),
        "schema_fingerprint": observed.get("schema_fingerprint"),
        "migration_history": migration_history,
        "protected_table_digests": table_digests,
        "public_row_digest": public_row_digest,
    }
    required_baseline = set(observed_baseline) | {"status"}
    if not required_baseline.issubset(baseline_record):
        reject("auth-bridge saved protected baseline is incomplete; preserving all run files")
    if not isinstance(observed_baseline["schema_fingerprint"], str) or not re.fullmatch(r"[a-f0-9]{32}", observed_baseline["schema_fingerprint"]):
        reject("auth-bridge live schema fingerprint is invalid; preserving all run files")
    for field, live_value in observed_baseline.items():
        if baseline_record.get(field) != live_value:
            reject(f"auth-bridge protected baseline mismatch: {field}; preserving all run files")
    if metadata_absent:
        print("shop guest import v2 API cleanup: owned function already absent and original baseline verified")
        raise SystemExit(0)

    def verify_postdrop_snapshot(snapshot_text):
        snapshot_lines = snapshot_text.splitlines()
        try:
            restored = json.loads(snapshot_lines[0])
            restored_tables = [json.loads(line) for line in snapshot_lines[1:] if line]
        except (IndexError, json.JSONDecodeError):
            reject(f"auth-bridge post-DROP baseline returned invalid JSON ({len(snapshot_lines)} lines); preserving all run files")
        if not isinstance(restored, dict) or restored.get("database_identity") != database_identity or restored.get("target_absent") is not True:
            reject("auth-bridge post-DROP database identity or function absence is unproven; preserving all run files")
        history = restored.get("migration_history")
        if not isinstance(history, dict) or not isinstance(history.get("rows_snapshot"), list) or len(history["rows_snapshot"]) != 28:
            reject("auth-bridge post-DROP migration snapshot is incomplete; preserving all run files")
        history["rows_snapshot_md5"] = hashlib.md5(
            json.dumps(history["rows_snapshot"], sort_keys=True, separators=(",", ":")).encode("utf-8")
        ).hexdigest()
        table_count = restored.get("protected_table_count")
        if type(table_count) is not int or table_count != 48 or len(restored_tables) != table_count:
            reject("auth-bridge post-DROP protected table snapshot is incomplete; preserving all run files")
        previous_key = None
        digest_lines = []
        for row in restored_tables:
            if not isinstance(row, dict) or set(row) != {"schema", "table", "row_count", "rows_hash"}:
                reject("auth-bridge post-DROP protected table digest is invalid; preserving all run files")
            schema_name, table_name = row.get("schema"), row.get("table")
            row_count, rows_hash = row.get("row_count"), row.get("rows_hash")
            if schema_name not in {"public", "private", "auth"} or not isinstance(table_name, str) or not table_name.isascii() or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_$]{0,62}", table_name):
                reject("auth-bridge post-DROP protected table identity is invalid; preserving all run files")
            if type(row_count) is not int or row_count < 0 or not isinstance(rows_hash, str) or not re.fullmatch(r"[a-f0-9]{32}", rows_hash):
                reject("auth-bridge post-DROP protected table value is invalid; preserving all run files")
            key = (schema_name, table_name)
            if previous_key is not None and key <= previous_key:
                reject("auth-bridge post-DROP table digests are not uniquely ordered; preserving all run files")
            previous_key = key
            digest_lines.append(f"{schema_name}.{table_name}:{row_count}:{rows_hash}\n")
        restored_baseline = {
            "preflight_database_identity": restored.get("database_identity"),
            "private_schema_owner": restored.get("private_schema_owner"),
            "private_schema_acl": restored.get("private_schema_acl"),
            "authenticated_schema_usage": restored.get("authenticated_schema_usage"),
            "role_defaults": restored.get("role_defaults"),
            "existing_function_definition_and_acl": restored.get("existing_function_definition_and_acl"),
            "auth_uid": restored.get("auth_uid"),
            "schema_snapshot": restored.get("schema_snapshot"),
            "schema_fingerprint": restored.get("schema_fingerprint"),
            "migration_history": history,
            "protected_table_digests": restored_tables,
            "public_row_digest": hashlib.md5("".join(digest_lines).encode("ascii")).hexdigest(),
        }
        if (set(restored_baseline) | {"status"}) != required_baseline:
            reject("auth-bridge post-DROP baseline fields are incomplete; preserving all run files")
        for field, live_value in restored_baseline.items():
            if baseline_record.get(field) != live_value:
                reject(f"auth-bridge post-DROP baseline mismatch: {field}; preserving all run files")

    transaction_sql = f"""-- TASK9_AUTH_BRIDGE_CLEANUP_TRANSACTION
BEGIN;
{metadata_query}
\\echo TASK9_AUTH_BRIDGE_CLEANUP_TX_METADATA
{baseline_query_body}
\\echo TASK9_AUTH_BRIDGE_CLEANUP_TX_BASELINE
"""
    child = None
    selector = None
    try:
        child, selector, output_buffer = start_cleanup_transaction(transaction_sql)
        transaction_metadata = read_cleanup_marker(
            child, selector, output_buffer, "TASK9_AUTH_BRIDGE_CLEANUP_TX_METADATA", 65536, 15,
        )
        if transaction_metadata.decode("utf-8") != metadata_text:
            reject("auth-bridge cleanup transaction metadata changed before DROP; preserving all run files")
        transaction_baseline = read_cleanup_marker(
            child, selector, output_buffer, "TASK9_AUTH_BRIDGE_CLEANUP_TX_BASELINE", 8 * 1024 * 1024, 30,
        )
        if transaction_baseline.decode("utf-8") != baseline_text:
            reject(f"auth-bridge cleanup transaction baseline changed before DROP ({len(transaction_baseline)} vs {len(baseline_text)} bytes); preserving all run files")
        drop_and_verify = (
            f"DROP FUNCTION private.{expected_name}();\n"
            f"{postdrop_baseline_body}\n"
            "\\echo TASK9_AUTH_BRIDGE_CLEANUP_TX_POSTDROP\n"
        )
        child.stdin.write(drop_and_verify.encode("utf-8"))
        child.stdin.flush()
        postdrop_text = read_cleanup_marker(
            child, selector, output_buffer, "TASK9_AUTH_BRIDGE_CLEANUP_TX_POSTDROP", 8 * 1024 * 1024, 30,
        ).decode("utf-8")
        verify_postdrop_snapshot(postdrop_text)
        commit_attempted = False
        commit_confirmed = False
        try:
            commit_attempted = True
            child.stdin.write(b"COMMIT;\n\\echo TASK9_AUTH_BRIDGE_CLEANUP_TX_COMMITTED\n")
            child.stdin.flush()
            child.stdin.close()
            committed_output = read_cleanup_marker(
                child, selector, output_buffer, "TASK9_AUTH_BRIDGE_CLEANUP_TX_COMMITTED", 65536, 15,
            )
            child.wait(timeout=15)
            commit_confirmed = child.returncode == 0 and not committed_output
        except (SystemExit, OSError, subprocess.TimeoutExpired):
            commit_confirmed = False
        commit_recovered = False
        if not commit_confirmed:
            if not commit_attempted:
                reject("auth-bridge cleanup transaction did not reach COMMIT; preserving all run files")
            abort_cleanup_child(child)
            recovered_metadata = docker_capture([
                "--context", docker_context, "exec", "-i", container_id,
                "psql", "-X", "-q", "-A", "-t", "-v", "ON_ERROR_STOP=1",
                "-U", "postgres", "-d", "postgres",
            ], input_text="-- TASK9_AUTH_BRIDGE_CLEANUP_RECOVER_METADATA\n" + metadata_query)
            if recovered_metadata:
                if recovered_metadata != metadata_text:
                    reject("auth-bridge COMMIT outcome is ambiguous and the owned function proof changed; preserving all run files")
                reject("auth-bridge COMMIT response was lost but the exact owned function remains; preserving all run files")
            recovery_query = (
                "BEGIN TRANSACTION READ ONLY;\n"
                "-- TASK9_AUTH_BRIDGE_CLEANUP_BASELINE_RECOVERY\n"
                + postdrop_baseline_body
                + "\nCOMMIT;"
            )
            recovered_baseline = docker_capture([
                "--context", docker_context, "exec", "-i", container_id,
                "psql", "-X", "-q", "-A", "-t", "-v", "ON_ERROR_STOP=1",
                "-U", "postgres", "-d", "postgres",
            ], input_text=recovery_query, max_bytes=8 * 1024 * 1024)
            verify_postdrop_snapshot(recovered_baseline)
            commit_recovered = True
    except BaseException:
        if child is not None:
            abort_cleanup_child(child)
        raise
    finally:
        if selector is not None:
            selector.close()
        if child is not None and child.stdout is not None:
            child.stdout.close()
    if commit_recovered:
        print("shop guest import v2 API cleanup: COMMIT response loss recovered by owned-function absence and original baseline")
    else:
        print("shop guest import v2 API cleanup: dropped exact owned function and restored the original baseline")
finally:
    os.close(run_fd)
PY
}

stop_owned_api() {
  local run_dir="$1"
  local fields run_id container_name env_file api_port container_id
  validate_run_dir_argument "$run_dir" existing
  fields="$(read_owned_manifest "$run_dir")" || fail 'could not read owned API manifest'
  IFS=$'\t' read -r run_id container_name env_file api_port container_id <<<"$fields"
  [[ -f "$env_file" && ! -L "$env_file" ]] \
    || fail 'the owned PostgREST environment file is missing or is a symlink'
  [[ "$(python3 -c 'import os,sys; print(os.stat(sys.argv[1], follow_symlinks=False).st_uid)' "$env_file")" == "$(id -u)" ]] \
    || fail 'the owned PostgREST environment file belongs to another user'
  [[ "$(stat -f '%Lp' "$env_file" 2>/dev/null)" == '600' ]] \
    || fail 'the owned PostgREST environment file is not mode 0600'
  context="$(docker context show 2>/dev/null)" || fail 'could not identify Docker context for cleanup'
  [[ "$context" == "$TEST_CONTEXT" ]] || fail 'refusing cleanup in a Docker context other than desktop-linux'
  endpoint="$(docker --context "$TEST_CONTEXT" context inspect "$TEST_CONTEXT" --format '{{(index .Endpoints "docker").Host}}' 2>/dev/null)" \
    || fail 'could not inspect the Docker endpoint for cleanup'
  [[ "$endpoint" == "$EXPECTED_DOCKER_ENDPOINT" ]] \
    || fail 'refusing cleanup through a non-local or unexpected Docker endpoint'
  local container_ids
  container_ids="$(docker --context "$TEST_CONTEXT" ps -aq --no-trunc \
    --filter "label=$API_OWNER_LABEL=$run_id" 2>/dev/null)" \
    || fail 'could not list this run’s API containers; preserving owned API run files'
  if [[ -z "$container_ids" ]]; then
    cleanup_auth_bridge "$run_dir" \
      || fail 'could not prove safe auth-bridge cleanup; preserving owned API run files'
    cleanup_owned_files "$run_dir" "$env_file" "$run_dir/$AUTH_SHIM_MANIFEST_NAME"
    printf 'shop guest import v2 API: owned container is already absent; removed its run files\n'
    return 0
  fi
  [[ "$container_ids" == "$container_id" ]] \
    || fail 'run ownership label resolved to an unexpected container ID; preserving owned API run files'
  verify_owned_container "$container_name" "$run_id" "$container_id" "$api_port"
  docker --context "$TEST_CONTEXT" rm --force "$container_id" >/dev/null \
    || fail 'could not remove the verified owned API container'
  cleanup_auth_bridge "$run_dir" \
    || fail 'could not prove safe auth-bridge cleanup; preserving owned API run files'
  cleanup_owned_files "$run_dir" "$env_file" "$run_dir/$AUTH_SHIM_MANIFEST_NAME"
  printf 'shop guest import v2 API: stopped owned local container %s and removed its run files\n' "$container_name"
}

STARTUP_ACTIVE=0
STARTUP_DIR_CREATED=0
STARTUP_CONTAINER_NAME_ABSENT=0
STARTUP_CONTAINER_CREATE_ATTEMPTED=0
STARTUP_RUN_DIR=''
STARTUP_RUN_ID=''
STARTUP_CONTAINER_NAME=''
STARTUP_API_PORT=''
STARTUP_CONTAINER_ID=''

cleanup_failed_start() {
  local exit_status=$?
  local container_json verified_id container_ids preserve_artifacts=0
  trap - EXIT
  if [[ "$STARTUP_ACTIVE" != 1 ]]; then
    return "$exit_status"
  fi
  if [[ "$STARTUP_CONTAINER_NAME_ABSENT" == 1 && "$STARTUP_CONTAINER_CREATE_ATTEMPTED" == 1 ]]; then
    if container_json="$(docker --context "$TEST_CONTEXT" inspect --format '{{json .}}' "$STARTUP_CONTAINER_NAME" 2>/dev/null)"; then
      if verified_id="$(python3 - "$container_json" "$STARTUP_CONTAINER_NAME" "$STARTUP_RUN_ID" \
        "$STARTUP_CONTAINER_ID" "$STARTUP_API_PORT" "$POSTGREST_IMAGE" "$API_OWNER_LABEL" <<'PY'
import json
import sys

raw, name, run_id, expected_id, port, image, owner_label = sys.argv[1:]
container = json.loads(raw)
config = container.get("Config") or {}
labels = config.get("Labels") or {}
if container.get("Name") != "/" + name or config.get("Image") != image or labels.get(owner_label) != run_id:
    raise SystemExit("not this run's container")
if expected_id and container.get("Id") != expected_id:
    raise SystemExit("container ID changed")
bindings = ((container.get("HostConfig") or {}).get("PortBindings") or {}).get("3000/tcp") or []
if bindings != [{"HostIp": "127.0.0.1", "HostPort": port}]:
    raise SystemExit("container binding changed")
print(container["Id"])
PY
      )"; then
        if ! docker --context "$TEST_CONTEXT" rm --force "$verified_id" >/dev/null 2>&1; then
          preserve_artifacts=1
          printf 'shop guest import v2 API cleanup: could not remove verified failed-start container %s\n' "$verified_id" >&2
        fi
      else
        preserve_artifacts=1
        printf 'shop guest import v2 API cleanup: preserved container whose ownership could not be proven: %s\n' "$STARTUP_CONTAINER_NAME" >&2
      fi
    else
      if container_ids="$(docker --context "$TEST_CONTEXT" ps --all --quiet --filter "name=^/${STARTUP_CONTAINER_NAME}$" 2>/dev/null)"; then
        if [[ -n "$container_ids" ]]; then
          preserve_artifacts=1
          printf 'shop guest import v2 API cleanup: container exists but inspection failed; retained run files at %s\n' "$STARTUP_RUN_DIR" >&2
        fi
      else
        preserve_artifacts=1
        printf 'shop guest import v2 API cleanup: Docker is unavailable; retained run files at %s\n' "$STARTUP_RUN_DIR" >&2
      fi
    fi
  fi
  if [[ "$STARTUP_DIR_CREATED" == 1 && "$preserve_artifacts" == 0 \
        && ( -e "$STARTUP_RUN_DIR/$AUTH_SHIM_MANIFEST_NAME" || -L "$STARTUP_RUN_DIR/$AUTH_SHIM_MANIFEST_NAME" ) ]]; then
    if ! cleanup_auth_bridge "$STARTUP_RUN_DIR"; then
      preserve_artifacts=1
      printf 'shop guest import v2 API cleanup: retained auth-bridge proof because safe database cleanup could not be verified at %s\n' "$STARTUP_RUN_DIR" >&2
    fi
  fi
  if [[ "$STARTUP_DIR_CREATED" == 1 && "$preserve_artifacts" == 0 ]]; then
    cleanup_owned_files "$STARTUP_RUN_DIR" "$STARTUP_RUN_DIR/$POSTGREST_ENV_NAME" \
      "$STARTUP_RUN_DIR/$AUTH_SHIM_MANIFEST_NAME" \
      || printf 'shop guest import v2 API cleanup: left incomplete run artifacts in %s\n' "$STARTUP_RUN_DIR" >&2
  elif [[ "$STARTUP_DIR_CREATED" == 1 ]]; then
    printf 'shop guest import v2 API cleanup: retained run files to support safe recovery at %s\n' "$STARTUP_RUN_DIR" >&2
  fi
  exit "$exit_status"
}

write_runtime_files() {
  local run_dir="$1"
  local run_id="$2"
  local container_name="$3"
  local api_port="$4"
  DB_URI_PASSWORD_ENCODED="$DB_URI_PASSWORD_ENCODED" python3 - "$run_dir" "$run_id" \
    "$container_name" "$api_port" "$POSTGREST_IMAGE" "$API_OWNER_LABEL" <<'PY'
import base64
import hashlib
import hmac
import json
import os
import secrets
import sys
import time
import uuid

run_dir, run_id, container_name, port_text, image, owner_label = sys.argv[1:]
port = int(port_text)
env_path = os.path.join(run_dir, "postgrest.env")
manifest_path = os.path.join(run_dir, "runtime.env")
secret = secrets.token_urlsafe(48)
health_user_id = str(uuid.uuid4())

def b64url(value):
    return base64.urlsafe_b64encode(value).rstrip(b"=").decode("ascii")

now = int(time.time())
header = b64url(json.dumps({"alg": "HS256", "typ": "JWT"}, separators=(",", ":")).encode())
payload = b64url(json.dumps({"role": "anon", "iss": "supabase", "iat": now, "exp": now + 3600}, separators=(",", ":")).encode())
unsigned = f"{header}.{payload}"
signature = b64url(hmac.new(secret.encode(), unsigned.encode(), hashlib.sha256).digest())
anon_jwt = f"{unsigned}.{signature}"
health_payload = b64url(json.dumps({
    "role": "authenticated",
    "iss": "supabase",
    "iat": now,
    "exp": now + 300,
    "sub": health_user_id,
}, separators=(",", ":")).encode())
health_unsigned = f"{header}.{health_payload}"
health_signature = b64url(hmac.new(secret.encode(), health_unsigned.encode(), hashlib.sha256).digest())
health_jwt = f"{health_unsigned}.{health_signature}"
db_uri = "postgres://postgres:" + os.environ["DB_URI_PASSWORD_ENCODED"] + "@db:5432/postgres?application_name=guest_import_v2_" + run_id
claim_bridge_function = "private.task9_claim_bridge_" + run_id
env_content = "\n".join((
    "PGRST_DB_URI=" + db_uri,
    "PGRST_DB_SCHEMAS=public",
    "PGRST_DB_ANON_ROLE=anon",
    "PGRST_JWT_SECRET=" + secret,
    "PGRST_DB_PRE_REQUEST=" + claim_bridge_function,
    "PGRST_SERVER_PORT=3000",
    "",
))
manifest = {
    "version": 1,
    "run_id": run_id,
    "run_dir": run_dir,
    "container_name": container_name,
    "container_id": "",
    "container_name_absent_before_start": True,
    "image": image,
    "owner_label": run_id,
    "api_port": port,
    "api_url": f"http://127.0.0.1:{port}",
    "env_file": env_path,
    "anon_jwt": anon_jwt,
    "health_jwt": health_jwt,
    "health_user_id": health_user_id,
    "claim_bridge_function": claim_bridge_function,
    "jwt_secret": secret,
}
for path, content in ((env_path, env_content), (manifest_path, json.dumps(manifest, separators=(",", ":")) + "\n")):
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8") as stream:
        stream.write(content)
        stream.flush()
        os.fsync(stream.fileno())
    os.chmod(path, 0o600)
PY
}

prepare_auth_bridge() {
  local run_dir="$1"
  local run_id="$2"
  validate_run_dir_argument "$run_dir" existing
  python3 - "$run_dir" "$run_id" "$API_RUN_PREFIX" "$AUTH_SHIM_MANIFEST_NAME" \
    "$TEST_CONTEXT" "$EXPECTED_DOCKER_ENDPOINT" "$EXPECTED_DB_IDENTITY" \
    "$TEST_DB_CONTAINER" "$TEST_DB_CONTAINER_ID" "$TEST_DB_IMAGE_ID" \
    "$TEST_DB_VOLUME" "$TEST_DB_PORT" <<'PY'
import hashlib
import json
import os
import re
import stat
import sys

(run_dir, run_id, prefix, manifest_name, docker_context, docker_endpoint,
 database_identity, container_name, container_id, image_id, volume_name,
 database_host_port) = sys.argv[1:]

def reject(message):
    raise SystemExit("shop guest import v2 API readiness: " + message)

if os.getuid() != 501:
    reject("auth-bridge preparation must run as the approved UID 501")
if not re.fullmatch(r"[a-f0-9]{24}", run_id):
    reject("auth-bridge run ID is not canonical")
if run_dir != prefix + run_id or os.path.normpath(run_dir) != run_dir:
    reject("auth-bridge run path is not the exact canonical owned path")
if os.path.realpath(prefix) != prefix or os.path.realpath(run_dir) != run_dir:
    reject("auth-bridge run path must not contain symlinks")

try:
    run_fd = os.open(run_dir, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
except OSError:
    reject("auth-bridge run path is not an owned real directory")
try:
    run_info = os.fstat(run_fd)
    if not stat.S_ISDIR(run_info.st_mode) or run_info.st_uid != 501 or stat.S_IMODE(run_info.st_mode) != 0o700:
        reject("auth-bridge run directory must be UID-501-owned mode 0700")

    function_name = "task9_claim_bridge_" + run_id
    planned_definition_sql = f"""CREATE FUNCTION private.{function_name}() RETURNS void
LANGUAGE plpgsql
SECURITY INVOKER
SET search_path = ''
AS $function$
DECLARE
  claims json;
  subject_text text;
BEGIN
  IF current_user <> 'authenticated' THEN
    RAISE EXCEPTION USING ERRCODE = '42501', MESSAGE = 'authenticated role required';
  END IF;
  BEGIN
    claims := pg_catalog.current_setting('request.jwt.claims', true)::json;
  EXCEPTION WHEN OTHERS THEN
    RAISE EXCEPTION USING ERRCODE = '42501', MESSAGE = 'invalid request claims';
  END;
  IF pg_catalog.json_typeof(claims) IS DISTINCT FROM 'object'
     OR pg_catalog.json_typeof(claims->'role') IS DISTINCT FROM 'string'
     OR claims->>'role' IS DISTINCT FROM 'authenticated'
     OR pg_catalog.json_typeof(claims->'sub') IS DISTINCT FROM 'string'
     OR claims->>'sub' !~ '^[0-9a-f]{{8}}-[0-9a-f]{{4}}-[0-9a-f]{{4}}-[0-9a-f]{{4}}-[0-9a-f]{{12}}$' THEN
    RAISE EXCEPTION USING ERRCODE = '42501', MESSAGE = 'invalid authenticated claims';
  END IF;
  subject_text := claims->>'sub';
  BEGIN
    PERFORM subject_text::pg_catalog.uuid;
  EXCEPTION WHEN OTHERS THEN
    RAISE EXCEPTION USING ERRCODE = '42501', MESSAGE = 'invalid authenticated subject';
  END;
  PERFORM pg_catalog.set_config('request.jwt.claim.sub', subject_text, true);
END;
$function$;"""
    planned_definition_sha256 = hashlib.sha256(planned_definition_sql.encode("utf-8")).hexdigest()
    manifest = {
        "version": 1,
        "status": "pre_ddl_baseline_pending",
        "run_id": run_id,
        "run_dir": run_dir,
        "owner_uid": 501,
        "pinned_target": {
            "docker_context": docker_context,
            "docker_endpoint": docker_endpoint,
            "container_name": container_name,
            "container_id": container_id,
            "image_id": image_id,
            "volume_name": volume_name,
            "database_host_port": database_host_port,
            "database_identity": database_identity,
        },
        "function": {
            "schema": "private",
            "name": function_name,
            "signature": f"private.{function_name}()",
            "initial_absence": None,
            "planned_definition_sql": planned_definition_sql,
            "planned_definition_sha256": planned_definition_sha256,
            "planned_acl": {"PUBLIC_EXECUTE": False, "authenticated_EXECUTE": True},
            "created_oid": None,
            "created_owner": None,
            "created_acl": None,
        },
        "baseline": {
            "status": "pending_read_only_capture",
            "private_schema_owner": None,
            "private_schema_acl": None,
            "authenticated_schema_usage": None,
            "role_defaults": None,
            "existing_function_definition_and_acl": None,
            "schema_fingerprint": None,
            "migration_history": None,
            "protected_table_digests": None,
            "public_row_digest": None,
        },
        "expected_baseline": {
            "migration_history_rows": 28,
            "migration_history_max_version": "20261003061830",
            "public_row_digest": "ce9ef4b37d29422d9d8cd8cb3c9d9bba",
        },
        "ddl_started": False,
    }
    payload = (json.dumps(manifest, sort_keys=True, separators=(",", ":")) + "\n").encode("utf-8")
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW
    try:
        manifest_fd = os.open(manifest_name, flags, 0o600, dir_fd=run_fd)
    except FileExistsError:
        reject("auth-bridge manifest already exists; refusing to overwrite it")
    try:
        os.fchmod(manifest_fd, 0o600)
        with os.fdopen(manifest_fd, "wb") as stream:
            stream.write(payload)
            stream.flush()
            os.fsync(stream.fileno())
        os.fsync(run_fd)
    except BaseException:
        try:
            os.unlink(manifest_name, dir_fd=run_fd)
            os.fsync(run_fd)
        except OSError:
            pass
        raise
finally:
    os.close(run_fd)
PY
  python3 - "$run_dir" "$run_id" "$API_RUN_PREFIX" "$AUTH_SHIM_MANIFEST_NAME" \
    "$TEST_CONTEXT" "$EXPECTED_DOCKER_ENDPOINT" "$EXPECTED_DB_IDENTITY" "$TEST_DB_CONTAINER" <<'PY'
import hashlib
import json
import os
import re
import stat
import subprocess
import sys

(run_dir, run_id, prefix, manifest_name, docker_context, docker_endpoint,
 database_identity, container_name) = sys.argv[1:]

def reject(message):
    raise SystemExit("shop guest import v2 API readiness: " + message)

if run_dir != prefix + run_id or not re.fullmatch(r"[a-f0-9]{24}", run_id):
    reject("auth-bridge baseline run identity is invalid")
if os.path.normpath(run_dir) != run_dir or os.path.realpath(prefix) != prefix or os.path.realpath(run_dir) != run_dir:
    reject("auth-bridge baseline run path is not canonical")
try:
    run_fd = os.open(run_dir, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    manifest_fd = os.open(manifest_name, os.O_RDONLY | os.O_NOFOLLOW, dir_fd=run_fd)
except OSError:
    reject("auth-bridge pre-DDL manifest is unavailable or unsafe")
try:
    run_info = os.fstat(run_fd)
    if not stat.S_ISDIR(run_info.st_mode) or run_info.st_uid != 501 or stat.S_IMODE(run_info.st_mode) != 0o700:
        reject("auth-bridge run directory ownership or mode changed")
    info = os.fstat(manifest_fd)
    if not stat.S_ISREG(info.st_mode) or info.st_uid != 501 or stat.S_IMODE(info.st_mode) != 0o600:
        reject("auth-bridge pre-DDL manifest ownership or mode changed")
    with os.fdopen(manifest_fd, "r", encoding="utf-8") as stream:
        manifest = json.load(stream)
    if manifest.get("run_id") != run_id or manifest.get("run_dir") != run_dir or manifest.get("status") != "pre_ddl_baseline_pending":
        reject("auth-bridge pre-DDL manifest identity or stage changed")
    expected_function_name = "task9_claim_bridge_" + run_id
    function_record = manifest.get("function")
    if not isinstance(function_record, dict) or function_record.get("name") != expected_function_name or function_record.get("signature") != f"private.{expected_function_name}()":
        reject("auth-bridge manifest function name does not match this run")

    docker_env = {key: value for key, value in os.environ.items() if not key.startswith("DOCKER_")}
    def docker(args, input_text=None):
        try:
            return subprocess.run(
                ["docker", *args], input=input_text, text=True, capture_output=True,
                env=docker_env, check=True,
            ).stdout.strip()
        except (OSError, subprocess.CalledProcessError):
            reject("could not verify the fixed local Docker context or read-only database query")

    if docker(["context", "show"]) != docker_context:
        reject("auth-bridge baseline Docker context mismatch")
    endpoint = docker(["--context", docker_context, "context", "inspect", docker_context,
                       "--format", '{{(index .Endpoints "docker").Host}}'])
    if endpoint != docker_endpoint:
        reject("auth-bridge baseline Docker endpoint mismatch")

    query = f"""BEGIN TRANSACTION READ ONLY;
WITH role_defaults AS (
  SELECT COALESCE(pg_catalog.jsonb_agg(
    pg_catalog.jsonb_build_object(
      'database', COALESCE(d.datname, '*'),
      'role', COALESCE(r.rolname, '*'),
      'settings', s.setconfig
    ) ORDER BY COALESCE(d.datname, ''), COALESCE(r.rolname, '')
  ), '[]'::jsonb) AS value
  FROM pg_catalog.pg_db_role_setting s
  LEFT JOIN pg_catalog.pg_database d ON d.oid = s.setdatabase
  LEFT JOIN pg_catalog.pg_roles r ON r.oid = s.setrole
), functions AS (
  SELECT COALESCE(pg_catalog.jsonb_agg(
    pg_catalog.jsonb_build_object(
      'schema', n.nspname,
      'name', p.proname,
      'identity_arguments', pg_catalog.pg_get_function_identity_arguments(p.oid),
      'definition', pg_catalog.pg_get_functiondef(p.oid),
      'owner', owner_role.rolname,
      'acl', p.proacl::text,
      'proconfig', p.proconfig,
      'security_definer', p.prosecdef,
      'kind', p.prokind,
      'volatility', p.provolatile,
      'strict', p.proisstrict
    ) ORDER BY n.nspname, p.proname, pg_catalog.pg_get_function_identity_arguments(p.oid)
  ), '[]'::jsonb) AS value
  FROM pg_catalog.pg_proc p
  JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace
  JOIN pg_catalog.pg_roles owner_role ON owner_role.oid = p.proowner
  WHERE n.nspname IN ('public', 'private', 'auth')
    AND p.prokind IN ('f', 'p')
), auth_uid AS (
  SELECT pg_catalog.jsonb_build_object(
    'definition', pg_catalog.pg_get_functiondef(p.oid),
    'owner', owner_role.rolname,
    'acl', p.proacl::text,
    'proconfig', p.proconfig,
    'identity_arguments', pg_catalog.pg_get_function_identity_arguments(p.oid)
  ) AS value
  FROM pg_catalog.pg_proc p
  JOIN pg_catalog.pg_roles owner_role ON owner_role.oid = p.proowner
  WHERE p.oid = pg_catalog.to_regprocedure('auth.uid()')
), migration_history AS (
  SELECT pg_catalog.jsonb_build_object(
    'rows', count(*)::integer,
    'max_version', max(m.version)::text,
    'versions', COALESCE(pg_catalog.jsonb_agg(m.version::text ORDER BY m.version), '[]'::jsonb),
    'version_hash', pg_catalog.md5(COALESCE(pg_catalog.string_agg(m.version::text, pg_catalog.chr(10) ORDER BY m.version), '')),
    'rows_snapshot', COALESCE(pg_catalog.jsonb_agg(pg_catalog.to_jsonb(m) ORDER BY m.version), '[]'::jsonb)
  ) AS value
  FROM supabase_migrations.schema_migrations m
), schemas AS (
  SELECT COALESCE(pg_catalog.jsonb_agg(
    pg_catalog.jsonb_build_object('name', n.nspname, 'owner', r.rolname, 'acl', n.nspacl::text)
    ORDER BY n.nspname
  ), '[]'::jsonb) AS value
  FROM pg_catalog.pg_namespace n
  JOIN pg_catalog.pg_roles r ON r.oid = n.nspowner
  WHERE n.nspname IN ('public', 'private', 'auth')
), relations AS (
  SELECT COALESCE(pg_catalog.jsonb_agg(
    pg_catalog.jsonb_build_object(
      'schema', n.nspname,
      'name', c.relname,
      'kind', c.relkind,
      'owner', r.rolname,
      'acl', c.relacl::text,
      'persistence', c.relpersistence,
      'row_security', c.relrowsecurity,
      'force_row_security', c.relforcerowsecurity,
      'options', c.reloptions,
      'view_definition', CASE WHEN c.relkind IN ('v', 'm') THEN pg_catalog.pg_get_viewdef(c.oid, true) ELSE NULL END,
      'index_definition', CASE WHEN c.relkind = 'i' THEN pg_catalog.pg_get_indexdef(c.oid) ELSE NULL END,
      'columns', (
        SELECT COALESCE(pg_catalog.jsonb_agg(pg_catalog.jsonb_build_object(
          'number', a.attnum,
          'name', a.attname,
          'type', pg_catalog.format_type(a.atttypid, a.atttypmod),
          'not_null', a.attnotnull,
          'default', pg_catalog.pg_get_expr(d.adbin, d.adrelid),
          'generated', a.attgenerated,
          'identity', a.attidentity
        ) ORDER BY a.attnum), '[]'::jsonb)
        FROM pg_catalog.pg_attribute a
        LEFT JOIN pg_catalog.pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
        WHERE a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped
      ),
      'constraints', (
        SELECT COALESCE(pg_catalog.jsonb_agg(pg_catalog.jsonb_build_object(
          'name', con.conname,
          'type', con.contype,
          'definition', pg_catalog.pg_get_constraintdef(con.oid, true),
          'validated', con.convalidated,
          'deferrable', con.condeferrable,
          'initially_deferred', con.condeferred
        ) ORDER BY con.conname), '[]'::jsonb)
        FROM pg_catalog.pg_constraint con WHERE con.conrelid = c.oid
      ),
      'policies', (
        SELECT COALESCE(pg_catalog.jsonb_agg(pg_catalog.jsonb_build_object(
          'name', pol.polname,
          'permissive', pol.polpermissive,
          'roles', pol.polroles::text,
          'command', pol.polcmd,
          'using', pg_catalog.pg_get_expr(pol.polqual, pol.polrelid),
          'check', pg_catalog.pg_get_expr(pol.polwithcheck, pol.polrelid)
        ) ORDER BY pol.polname), '[]'::jsonb)
        FROM pg_catalog.pg_policy pol WHERE pol.polrelid = c.oid
      ),
      'triggers', (
        SELECT COALESCE(pg_catalog.jsonb_agg(pg_catalog.jsonb_build_object(
          'name', tg.tgname,
          'definition', pg_catalog.pg_get_triggerdef(tg.oid, true),
          'enabled', tg.tgenabled
        ) ORDER BY tg.tgname), '[]'::jsonb)
        FROM pg_catalog.pg_trigger tg WHERE tg.tgrelid = c.oid AND NOT tg.tgisinternal
      )
    ) ORDER BY n.nspname, c.relname
  ), '[]'::jsonb) AS value
  FROM pg_catalog.pg_class c
  JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
  JOIN pg_catalog.pg_roles r ON r.oid = c.relowner
  WHERE n.nspname IN ('public', 'private', 'auth')
    AND c.relkind IN ('r', 'p', 'v', 'm', 'f', 'i', 'S')
), extensions AS (
  SELECT COALESCE(pg_catalog.jsonb_agg(pg_catalog.jsonb_build_object(
    'name', e.extname, 'version', e.extversion, 'schema', n.nspname, 'owner', r.rolname
  ) ORDER BY e.extname), '[]'::jsonb) AS value
  FROM pg_catalog.pg_extension e
  JOIN pg_catalog.pg_namespace n ON n.oid = e.extnamespace
  JOIN pg_catalog.pg_roles r ON r.oid = e.extowner
  WHERE n.nspname IN ('public', 'private', 'auth')
), schema_snapshot AS (
  SELECT pg_catalog.jsonb_build_object(
    'schemas', schemas.value,
    'relations', relations.value,
    'functions', functions.value,
    'role_defaults', role_defaults.value,
    'auth_uid', (SELECT value FROM auth_uid),
    'extensions', extensions.value,
    'authenticated_schema_usage', pg_catalog.has_schema_privilege('authenticated', 'private', 'USAGE')
  ) AS value
  FROM schemas, relations, functions, role_defaults, extensions
)
SELECT pg_catalog.jsonb_build_object(
  'database_identity', pg_catalog.current_database() || '|' || current_user || '|' || pg_catalog.current_setting('port'),
  'target_absent', pg_catalog.to_regprocedure('private.{expected_function_name}()') IS NULL,
  'private_schema_owner', (SELECT r.rolname FROM pg_catalog.pg_namespace n JOIN pg_catalog.pg_roles r ON r.oid = n.nspowner WHERE n.nspname = 'private'),
  'private_schema_acl', (SELECT n.nspacl::text FROM pg_catalog.pg_namespace n WHERE n.nspname = 'private'),
  'authenticated_schema_usage', pg_catalog.has_schema_privilege('authenticated', 'private', 'USAGE'),
  'role_defaults', role_defaults.value,
  'existing_function_definition_and_acl', functions.value,
  'auth_uid', (SELECT value FROM auth_uid),
  'migration_history', (SELECT value FROM migration_history),
  'schema_snapshot', schema_snapshot.value,
  'schema_fingerprint', pg_catalog.md5(schema_snapshot.value::text),
  'protected_table_count', (SELECT count(*)::integer FROM pg_catalog.pg_tables WHERE schemaname IN ('public', 'private', 'auth'))
)::text
FROM role_defaults, functions, schema_snapshot, extensions, migration_history;
SELECT pg_catalog.format(
  'SELECT pg_catalog.jsonb_build_object(''schema'', %L, ''table'', %L, ''row_count'', count(*), ''rows_hash'', pg_catalog.md5(COALESCE(pg_catalog.string_agg(pg_catalog.to_jsonb(t)::text, pg_catalog.chr(10) ORDER BY pg_catalog.to_jsonb(t)::text), %L)))::text FROM %I.%I AS t',
  schemaname, tablename, '', schemaname, tablename
)
FROM pg_catalog.pg_tables
WHERE schemaname IN ('public', 'private', 'auth')
ORDER BY schemaname, tablename
\\gexec
COMMIT;"""
    raw = docker([
        "--context", docker_context, "exec", "-i", container_name, "psql", "-X", "-q", "-A", "-t",
        "-v", "ON_ERROR_STOP=1", "-U", "postgres", "-d", "postgres",
    ], input_text=query)
    try:
        output_lines = raw.splitlines()
        observed = json.loads(output_lines[0])
        table_digests = [json.loads(line) for line in output_lines[1:] if line]
    except (IndexError, json.JSONDecodeError):
        reject("auth-bridge read-only baseline query returned invalid JSON")
    if observed.get("database_identity") != database_identity:
        reject("auth-bridge baseline database identity mismatch")
    if observed.get("target_absent") is not True:
        reject("auth-bridge target function already exists or absence is unproven")
    if not isinstance(observed.get("private_schema_owner"), str) or not observed["private_schema_owner"]:
        reject("auth-bridge private schema owner is missing")
    if not isinstance(observed.get("private_schema_acl"), str):
        reject("auth-bridge private schema ACL is missing")
    if observed.get("authenticated_schema_usage") is not True:
        reject("authenticated role lacks private schema USAGE")
    role_defaults = observed.get("role_defaults")
    function_snapshot = observed.get("existing_function_definition_and_acl")
    auth_uid = observed.get("auth_uid")
    schema_snapshot = observed.get("schema_snapshot")
    schema_fingerprint = observed.get("schema_fingerprint")
    migration_history = observed.get("migration_history")
    if not isinstance(role_defaults, list):
        reject("auth-bridge role defaults baseline is missing")
    if not isinstance(function_snapshot, list):
        reject("auth-bridge protected function definitions and ACLs are missing")
    if not isinstance(auth_uid, dict) or not isinstance(auth_uid.get("definition"), str) or not isinstance(auth_uid.get("owner"), str):
        reject("auth.uid definition or owner baseline is missing")
    if "acl" not in auth_uid or "proconfig" not in auth_uid:
        reject("auth.uid ACL or settings baseline is missing")
    if not isinstance(schema_snapshot, dict) or not all(key in schema_snapshot for key in ("schemas", "relations", "functions", "role_defaults", "auth_uid", "extensions", "authenticated_schema_usage")):
        reject("auth-bridge full schema fingerprint inputs are incomplete")
    if not isinstance(schema_fingerprint, str) or not re.fullmatch(r"[a-f0-9]{32}", schema_fingerprint):
        reject("auth-bridge schema fingerprint is invalid")
    expected_history_rows = 28
    expected_history_max = "20261003061830"
    expected_baseline = manifest.get("expected_baseline")
    expected_row_digest = "ce9ef4b37d29422d9d8cd8cb3c9d9bba"
    if not isinstance(expected_baseline, dict) or expected_baseline.get("migration_history_rows") != expected_history_rows or expected_baseline.get("migration_history_max_version") != expected_history_max or expected_baseline.get("public_row_digest") != expected_row_digest:
        reject("auth-bridge expected migration history was modified")
    if not isinstance(migration_history, dict):
        reject("auth-bridge migration history baseline is missing")
    versions = migration_history.get("versions")
    version_hash = migration_history.get("version_hash")
    rows_snapshot = migration_history.get("rows_snapshot")
    if migration_history.get("rows") != expected_history_rows or migration_history.get("max_version") != expected_history_max:
        reject("auth-bridge migration history does not match the approved baseline")
    if not isinstance(versions, list) or len(versions) != expected_history_rows or any(not isinstance(version, str) or not re.fullmatch(r"(?:[0-9]{12}|[0-9]{14})", version) for version in versions):
        reject("auth-bridge migration version list is incomplete or invalid")
    if versions != sorted(set(versions)) or versions[-1] != expected_history_max:
        reject("auth-bridge migration version list is not canonical")
    if not isinstance(version_hash, str) or not re.fullmatch(r"[a-f0-9]{32}", version_hash):
        reject("auth-bridge migration history hash is invalid")
    calculated_history_hash = hashlib.md5("\n".join(versions).encode("ascii")).hexdigest()
    if version_hash != calculated_history_hash:
        reject("auth-bridge migration history hash does not match its version list")
    if not isinstance(rows_snapshot, list) or len(rows_snapshot) != expected_history_rows:
        reject("auth-bridge full migration row snapshot is incomplete")
    for version, row in zip(versions, rows_snapshot):
        if not isinstance(row, dict) or row.get("version") != version or not isinstance(row.get("name"), str) or not isinstance(row.get("statements"), list):
            reject("auth-bridge full migration row snapshot is invalid")
    rows_snapshot_md5 = hashlib.md5(
        json.dumps(rows_snapshot, sort_keys=True, separators=(",", ":")).encode("utf-8")
    ).hexdigest()
    migration_history["rows_snapshot_md5"] = rows_snapshot_md5

    protected_table_count = observed.get("protected_table_count")
    if type(protected_table_count) is not int or protected_table_count <= 0 or len(table_digests) != protected_table_count:
        reject("auth-bridge protected table digest list is incomplete")
    previous_table_key = None
    digest_lines = []
    for table_digest in table_digests:
        if not isinstance(table_digest, dict) or set(table_digest) != {"schema", "table", "row_count", "rows_hash"}:
            reject("auth-bridge protected table digest record is invalid")
        schema_name = table_digest.get("schema")
        table_name = table_digest.get("table")
        row_count = table_digest.get("row_count")
        rows_hash = table_digest.get("rows_hash")
        if schema_name not in {"public", "private", "auth"}:
            reject("auth-bridge protected table digest schema is invalid")
        if not isinstance(table_name, str) or not table_name.isascii() or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_$]{0,62}", table_name):
            reject("auth-bridge protected table digest identifier is invalid")
        if type(row_count) is not int or row_count < 0 or not isinstance(rows_hash, str) or not re.fullmatch(r"[a-f0-9]{32}", rows_hash):
            reject("auth-bridge protected table digest value is invalid")
        table_key = (schema_name, table_name)
        if previous_table_key is not None and table_key <= previous_table_key:
            reject("auth-bridge protected table digests are not uniquely ordered")
        previous_table_key = table_key
        digest_lines.append(f"{schema_name}.{table_name}:{row_count}:{rows_hash}\n")
    calculated_row_digest = hashlib.md5("".join(digest_lines).encode("ascii")).hexdigest()
    if calculated_row_digest != expected_row_digest:
        reject("auth-bridge protected row digest does not match the approved baseline")

    manifest["function"]["initial_absence"] = True
    manifest["baseline"]["private_schema_owner"] = observed["private_schema_owner"]
    manifest["baseline"]["private_schema_acl"] = observed["private_schema_acl"]
    manifest["baseline"]["authenticated_schema_usage"] = True
    manifest["baseline"]["preflight_database_identity"] = observed["database_identity"]
    manifest["baseline"]["role_defaults"] = role_defaults
    manifest["baseline"]["existing_function_definition_and_acl"] = function_snapshot
    manifest["baseline"]["auth_uid"] = auth_uid
    manifest["baseline"]["schema_snapshot"] = schema_snapshot
    manifest["baseline"]["schema_fingerprint"] = schema_fingerprint
    manifest["baseline"]["migration_history"] = migration_history
    manifest["baseline"]["protected_table_digests"] = table_digests
    manifest["baseline"]["public_row_digest"] = calculated_row_digest
    manifest["baseline"]["status"] = "pre_ddl_baseline_ready"
    manifest["status"] = "pre_ddl_baseline_ready"
    temp_name = "." + manifest_name + ".tmp"
    payload = (json.dumps(manifest, sort_keys=True, separators=(",", ":")) + "\n").encode("utf-8")
    temp_fd = os.open(temp_name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=run_fd)
    try:
        os.fchmod(temp_fd, 0o600)
        with os.fdopen(temp_fd, "wb") as stream:
            stream.write(payload)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temp_name, manifest_name, src_dir_fd=run_fd, dst_dir_fd=run_fd)
        os.fsync(run_fd)
    except BaseException:
        try:
            os.unlink(temp_name, dir_fd=run_fd)
        except OSError:
            pass
        raise
finally:
    os.close(run_fd)
PY
}

create_auth_bridge_transaction() {
  local run_dir="$1"
  local run_id="$2"
  python3 - "$run_dir" "$run_id" "$API_RUN_PREFIX" "$AUTH_SHIM_MANIFEST_NAME" \
    "$TEST_CONTEXT" "$EXPECTED_DOCKER_ENDPOINT" "$EXPECTED_DB_IDENTITY" \
    "$TEST_DB_CONTAINER" "$TEST_DB_CONTAINER_ID" <<'PY'
import hashlib
import json
import os
import re
import selectors
import stat
import subprocess
import sys
import time

(run_dir, run_id, prefix, manifest_name, docker_context, docker_endpoint,
 database_identity, container_name, container_id) = sys.argv[1:]

def reject(message):
    raise SystemExit("shop guest import v2 API readiness: " + message)

if os.getuid() != 501 or not re.fullmatch(r"[a-f0-9]{24}", run_id):
    reject("auth-bridge transaction run identity is invalid")
if run_dir != prefix + run_id or os.path.normpath(run_dir) != run_dir or os.path.realpath(run_dir) != run_dir:
    reject("auth-bridge transaction run path is not canonical")
try:
    run_fd = os.open(run_dir, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    manifest_fd = os.open(manifest_name, os.O_RDONLY | os.O_NOFOLLOW, dir_fd=run_fd)
except OSError:
    reject("auth-bridge ready manifest is unavailable or unsafe")

def abort_child(child):
    try:
        if child.stdin is not None and not child.stdin.closed:
            child.stdin.close()
    except OSError:
        pass
    try:
        child.wait(timeout=3)
    except (OSError, subprocess.TimeoutExpired):
        try:
            child.terminate()
            child.wait(timeout=3)
        except (OSError, subprocess.TimeoutExpired):
            pass
        try:
            child.kill()
            child.wait(timeout=3)
        except (OSError, subprocess.TimeoutExpired):
            pass

try:
    run_info = os.fstat(run_fd)
    manifest_info = os.fstat(manifest_fd)
    if not stat.S_ISDIR(run_info.st_mode) or run_info.st_uid != 501 or stat.S_IMODE(run_info.st_mode) != 0o700:
        reject("auth-bridge transaction run directory ownership or mode changed")
    if not stat.S_ISREG(manifest_info.st_mode) or manifest_info.st_uid != 501 or stat.S_IMODE(manifest_info.st_mode) != 0o600:
        reject("auth-bridge transaction manifest ownership or mode changed")
    with os.fdopen(manifest_fd, "r", encoding="utf-8") as stream:
        manifest = json.load(stream)
    if manifest.get("run_id") != run_id or manifest.get("run_dir") != run_dir or manifest.get("status") != "pre_ddl_baseline_ready":
        reject("auth-bridge transaction baseline is not ready")
    pinned = manifest.get("pinned_target")
    if not isinstance(pinned, dict) or pinned.get("docker_context") != docker_context or pinned.get("docker_endpoint") != docker_endpoint or pinned.get("container_name") != container_name or pinned.get("container_id") != container_id or pinned.get("database_identity") != database_identity:
        reject("auth-bridge transaction pinned target changed")
    function_record = manifest.get("function")
    expected_name = "task9_claim_bridge_" + run_id
    if not isinstance(function_record, dict) or function_record.get("name") != expected_name or function_record.get("signature") != f"private.{expected_name}()" or function_record.get("initial_absence") is not True or function_record.get("created_oid") is not None:
        reject("auth-bridge transaction function record is invalid")
    definition_sql = function_record.get("planned_definition_sql")
    planned_hash = function_record.get("planned_definition_sha256")
    expected_definition_sql = """CREATE FUNCTION private.__TASK9_FUNCTION_NAME__() RETURNS void
LANGUAGE plpgsql
SECURITY INVOKER
SET search_path = ''
AS $function$
DECLARE
  claims json;
  subject_text text;
BEGIN
  IF current_user <> 'authenticated' THEN
    RAISE EXCEPTION USING ERRCODE = '42501', MESSAGE = 'authenticated role required';
  END IF;
  BEGIN
    claims := pg_catalog.current_setting('request.jwt.claims', true)::json;
  EXCEPTION WHEN OTHERS THEN
    RAISE EXCEPTION USING ERRCODE = '42501', MESSAGE = 'invalid request claims';
  END;
  IF pg_catalog.json_typeof(claims) IS DISTINCT FROM 'object'
     OR pg_catalog.json_typeof(claims->'role') IS DISTINCT FROM 'string'
     OR claims->>'role' IS DISTINCT FROM 'authenticated'
     OR pg_catalog.json_typeof(claims->'sub') IS DISTINCT FROM 'string'
     OR claims->>'sub' !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$' THEN
    RAISE EXCEPTION USING ERRCODE = '42501', MESSAGE = 'invalid authenticated claims';
  END IF;
  subject_text := claims->>'sub';
  BEGIN
    PERFORM subject_text::pg_catalog.uuid;
  EXCEPTION WHEN OTHERS THEN
    RAISE EXCEPTION USING ERRCODE = '42501', MESSAGE = 'invalid authenticated subject';
  END;
  PERFORM pg_catalog.set_config('request.jwt.claim.sub', subject_text, true);
END;
$function$;""".replace("__TASK9_FUNCTION_NAME__", expected_name)
    if not isinstance(definition_sql, str) or definition_sql != expected_definition_sql:
        reject("auth-bridge planned function definition differs from the trusted template")
    if not isinstance(planned_hash, str) or hashlib.sha256(definition_sql.encode("utf-8")).hexdigest() != planned_hash:
        reject("auth-bridge planned function definition hash changed")

    docker_env = {key: value for key, value in os.environ.items() if not key.startswith("DOCKER_")}
    def docker_capture(args, input_text=None, timeout=10):
        try:
            return subprocess.run(["docker", *args], input=input_text, text=True, capture_output=True,
                                  env=docker_env, timeout=timeout, check=True).stdout.strip()
        except (OSError, subprocess.CalledProcessError, subprocess.TimeoutExpired):
            reject("could not verify the pinned local database target")

    if docker_capture(["context", "show"]) != docker_context:
        reject("auth-bridge transaction Docker context mismatch")
    endpoint = docker_capture(["--context", docker_context, "context", "inspect", docker_context,
                               "--format", '{{(index .Endpoints "docker").Host}}'])
    if endpoint != docker_endpoint:
        reject("auth-bridge transaction Docker endpoint mismatch")

    metadata_query = f"""SELECT pg_catalog.jsonb_build_object(
  'database_identity', pg_catalog.current_database() || '|' || current_user || '|' || pg_catalog.current_setting('port'),
  'oid', p.oid::text,
  'owner', owner_role.rolname,
  'signature', n.nspname || '.' || p.proname || '(' || pg_catalog.pg_get_function_identity_arguments(p.oid) || ')',
  'definition', pg_catalog.pg_get_functiondef(p.oid),
  'acl', p.proacl::text,
  'proconfig', p.proconfig,
  'security_definer', p.prosecdef
)::text
FROM pg_catalog.pg_proc p
JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace
JOIN pg_catalog.pg_roles owner_role ON owner_role.oid = p.proowner
WHERE p.oid = pg_catalog.to_regprocedure('private.{expected_name}()');"""
    transaction_sql = f"""BEGIN;
{definition_sql}
REVOKE ALL ON FUNCTION private.{expected_name}() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION private.{expected_name}() TO authenticated;
{metadata_query}
\\echo TASK9_AUTH_BRIDGE_PRECOMMIT
"""
    command = ["docker", "--context", docker_context, "exec", "-i", container_name,
               "psql", "-X", "-q", "-A", "-t", "-v", "ON_ERROR_STOP=1",
               "-U", "postgres", "-d", "postgres"]
    try:
        child = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                 stderr=subprocess.DEVNULL, bufsize=0, env=docker_env)
    except OSError:
        reject("could not open the pinned auth-bridge transaction")
    try:
        child.stdin.write(transaction_sql.encode("utf-8"))
        child.stdin.flush()
        selector = selectors.DefaultSelector()
        selector.register(child.stdout, selectors.EVENT_READ)
        deadline = time.monotonic() + 15
        metadata_lines = []
        output_buffer = bytearray()
        precommit_marker = b"TASK9_AUTH_BRIDGE_PRECOMMIT"
        while True:
            while b"\n" in output_buffer:
                line, _, remainder = output_buffer.partition(b"\n")
                output_buffer = bytearray(remainder)
                if line.strip() == precommit_marker:
                    break
                metadata_lines.append(line)
            else:
                remaining = deadline - time.monotonic()
                if remaining <= 0 or not selector.select(remaining):
                    reject("auth-bridge transaction metadata timed out")
                chunk = os.read(child.stdout.fileno(), 65537)
                if not chunk:
                    reject("auth-bridge transaction ended before its OID checkpoint")
                if len(chunk) > 65536 or len(output_buffer) + sum(map(len, metadata_lines)) + len(chunk) > 65536:
                    reject("auth-bridge transaction metadata exceeded its size limit")
                output_buffer.extend(chunk)
                continue
            break
        selector.close()
        metadata_text = b"\n".join(metadata_lines).strip()
        if not metadata_text or len(metadata_text) > 65536:
            reject("auth-bridge transaction metadata is missing or too large")
        metadata = json.loads(metadata_text)

        expected_keys = {"database_identity", "oid", "owner", "signature", "definition", "acl", "proconfig", "security_definer"}
        if not isinstance(metadata, dict) or set(metadata) != expected_keys:
            reject("auth-bridge transaction metadata fields are incomplete")
        if metadata.get("database_identity") != database_identity:
            reject("auth-bridge transaction database identity changed")
        if not isinstance(metadata.get("oid"), str) or not re.fullmatch(r"[1-9][0-9]*", metadata["oid"]):
            reject("auth-bridge transaction OID is invalid")
        if metadata.get("owner") != "postgres" or metadata.get("signature") != function_record["signature"]:
            reject("auth-bridge created function owner or signature does not match the plan")
        definition = metadata.get("definition")
        acl = metadata.get("acl")
        proconfig = metadata.get("proconfig")
        if not isinstance(definition, str) or not definition or not isinstance(acl, str) or not isinstance(proconfig, list):
            reject("auth-bridge created function definition or ACL metadata is invalid")
        if metadata.get("security_definer") is not False or 'search_path=""' not in proconfig:
            reject("auth-bridge created function security settings do not match the plan")
        acl_entries = acl.strip("{}").split(",")
        authenticated_execute = any(entry.split("=", 1)[0] == "authenticated" and "X" in entry.split("=", 1)[1].split("/", 1)[0] for entry in acl_entries if "=" in entry)
        public_execute = any(entry.split("=", 1)[0] == "" and "X" in entry.split("=", 1)[1].split("/", 1)[0] for entry in acl_entries if "=" in entry)
        other_role_execute = any(entry.split("=", 1)[0] not in {"postgres", "authenticated", ""} and "X" in entry.split("=", 1)[1].split("/", 1)[0] for entry in acl_entries if "=" in entry)
        if not authenticated_execute or public_execute or other_role_execute:
            reject("auth-bridge created function ACL is not authenticated-only")

        function_record["created_oid"] = metadata["oid"]
        function_record["created_owner"] = metadata["owner"]
        function_record["created_signature"] = metadata["signature"]
        function_record["created_definition_sha256"] = hashlib.sha256(definition.encode("utf-8")).hexdigest()
        function_record["created_acl"] = acl
        function_record["created_proconfig"] = proconfig
        function_record["created_security_definer"] = False
        function_record["created_database_identity"] = database_identity
        manifest["status"] = "auth_bridge_commit_pending"
        manifest["transaction_state"] = "oid_and_definition_persisted_before_commit"
        manifest["ddl_started"] = True

        def persist_manifest():
            temp_name = "." + manifest_name + ".tmp"
            payload = (json.dumps(manifest, sort_keys=True, separators=(",", ":")) + "\n").encode("utf-8")
            temp_fd = os.open(temp_name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=run_fd)
            try:
                os.fchmod(temp_fd, 0o600)
                with os.fdopen(temp_fd, "wb") as stream:
                    stream.write(payload)
                    stream.flush()
                    os.fsync(stream.fileno())
                os.replace(temp_name, manifest_name, src_dir_fd=run_fd, dst_dir_fd=run_fd)
                os.fsync(run_fd)
            except BaseException:
                try:
                    os.unlink(temp_name, dir_fd=run_fd)
                except OSError:
                    pass
                raise

        try:
            persist_manifest()
        except BaseException:
            abort_child(child)
            reject("could not durably record the auth-bridge OID; transaction was closed before COMMIT")

        try:
            child.stdin.write(b"COMMIT;\n\\echo TASK9_AUTH_BRIDGE_COMMITTED\n")
            child.stdin.flush()
            child.stdin.close()
            child.wait(timeout=20)
            commit_output = os.read(child.stdout.fileno(), 65537)
            if len(commit_output) > 65536:
                commit_output = b""
            committed = child.returncode == 0 and b"TASK9_AUTH_BRIDGE_COMMITTED" in commit_output
        except (BrokenPipeError, OSError, subprocess.TimeoutExpired):
            committed = False
            abort_child(child)

        if not committed:
            verification_query = "-- TASK9_AUTH_BRIDGE_VERIFY\n" + metadata_query
            try:
                verified_text = docker_capture([
                    "--context", docker_context, "exec", "-i", container_name, "psql", "-X", "-q", "-A", "-t",
                    "-v", "ON_ERROR_STOP=1", "-U", "postgres", "-d", "postgres",
                ], input_text=verification_query, timeout=10)
                if len(verified_text) > 65536:
                    reject("auth-bridge commit outcome verification exceeded its size limit")
                verified = json.loads(verified_text)
                verified_definition_hash = hashlib.sha256(verified["definition"].encode("utf-8")).hexdigest()
                if verified.get("oid") != function_record["created_oid"] or verified.get("owner") != function_record["created_owner"] or verified.get("signature") != function_record["created_signature"] or verified_definition_hash != function_record["created_definition_sha256"] or verified.get("acl") != function_record["created_acl"] or verified.get("proconfig") != function_record["created_proconfig"] or verified.get("security_definer") is not False or verified.get("database_identity") != database_identity:
                    reject("auth-bridge COMMIT response was lost and the saved OID did not verify")
            except (KeyError, TypeError, json.JSONDecodeError):
                reject("auth-bridge COMMIT response was lost and the saved OID could not be verified")
            manifest["status"] = "auth_bridge_commit_response_lost_recovered"
            manifest["transaction_state"] = "commit_verified_by_saved_oid"
            try:
                persist_manifest()
            except BaseException:
                reject("auth-bridge COMMIT was verified but recovery status could not be persisted")
            print("shop guest import v2 API: auth-bridge commit response loss recovered by saved OID")
        else:
            manifest["status"] = "auth_bridge_committed"
            manifest["transaction_state"] = "commit_acknowledged"
            try:
                persist_manifest()
            except BaseException:
                reject("auth-bridge COMMIT succeeded but committed status could not be persisted; saved OID retained")
        if child.stdout is not None:
            child.stdout.close()
    except BaseException:
        abort_child(child)
        raise
finally:
    os.close(run_fd)
PY
}

save_container_id() {
  local manifest="$1"
  local expected_run_id="$2"
  local container_id="$3"
  python3 - "$manifest" "$expected_run_id" "$container_id" <<'PY'
import json
import os
import re
import sys

path, run_id, container_id = sys.argv[1:]
if not re.fullmatch(r"[a-f0-9]{64}", container_id):
    raise SystemExit("shop guest import v2 API readiness: Docker returned an invalid API container ID")
with open(path, encoding="utf-8") as stream:
    data = json.load(stream)
if data.get("run_id") != run_id:
    raise SystemExit("shop guest import v2 API readiness: runtime manifest run ID changed")
data["container_id"] = container_id
with open(path, "w", encoding="utf-8") as stream:
    json.dump(data, stream, separators=(",", ":"))
    stream.write("\n")
    stream.flush()
    os.fsync(stream.fileno())
os.chmod(path, 0o600)
PY
}

wait_for_loopback_api() {
  local api_port="$1"
  local manifest="$2"
  python3 - "$API_HOST" "$api_port" "$manifest" <<'PY'
import sys
import time
import json
import os
import stat
import urllib.error
import urllib.request

manifest_path = sys.argv[3]
manifest_stat = os.stat(manifest_path, follow_symlinks=False)
if not stat.S_ISREG(manifest_stat.st_mode) or manifest_stat.st_uid != os.getuid() or stat.S_IMODE(manifest_stat.st_mode) != 0o600:
    raise SystemExit("shop guest import v2 API readiness: health manifest must be current-user-owned mode-0600 regular file")
with open(manifest_path, encoding="utf-8") as stream:
    manifest = json.load(stream)
health_jwt = manifest.get("health_jwt")
if not isinstance(health_jwt, str) or health_jwt.count(".") != 2:
    raise SystemExit("shop guest import v2 API readiness: authenticated health JWT is missing")

class NoRedirectHandler(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None

url = f"http://{sys.argv[1]}:{sys.argv[2]}/"
opener = urllib.request.build_opener(
    urllib.request.ProxyHandler({}),
    NoRedirectHandler(),
)
deadline = time.monotonic() + 30
last_error = "no response"
while time.monotonic() < deadline:
    try:
        request = urllib.request.Request(url, headers={"Authorization": "Bearer " + health_jwt})
        with opener.open(request, timeout=1) as response:
            if response.status == 200:
                print("shop guest import v2 API: loopback PostgREST health check passed")
                raise SystemExit(0)
            last_error = f"HTTP {response.status}"
    except (urllib.error.URLError, TimeoutError, OSError) as error:
        last_error = str(error)
    time.sleep(0.5)
raise SystemExit(f"shop guest import v2 API readiness: loopback PostgREST health check timed out: {last_error}")
PY
}

start_owned_api() {
  local run_dir="$1"
  local run_id container_name env_file container_id existing_container_ids
  validate_run_dir_argument "$run_dir" new
  run_id="${run_dir#"$API_RUN_PREFIX"}"
  container_name="guest-import-v2-api-$run_id"
  env_file="$run_dir/$POSTGREST_ENV_NAME"
  mkdir -m 700 -- "$run_dir" || fail 'could not create the unique owned Task 9 run directory'
  STARTUP_ACTIVE=1
  STARTUP_DIR_CREATED=1
  STARTUP_RUN_DIR="$run_dir"
  STARTUP_RUN_ID="$run_id"
  STARTUP_CONTAINER_NAME="$container_name"
  STARTUP_API_PORT="$READY_API_PORT"
  trap cleanup_failed_start EXIT
  existing_container_ids="$(docker --context "$TEST_CONTEXT" ps --all --quiet \
    --filter "name=^/${container_name}$")" \
    || fail 'could not prove the unique API container name is unused'
  [[ -z "$existing_container_ids" ]] \
    || fail 'the unique API container name already exists; refusing to reuse it'
  STARTUP_CONTAINER_NAME_ABSENT=1
  prepare_auth_bridge "$run_dir" "$run_id"
  create_auth_bridge_transaction "$run_dir" "$run_id" \
    || fail 'could not create and durably record the owned auth-bridge function'
  write_runtime_files "$run_dir" "$run_id" "$container_name" "$READY_API_PORT" \
    || fail 'could not write mode-0600 owned PostgREST runtime files'
  STARTUP_CONTAINER_CREATE_ATTEMPTED=1
  container_id="$(docker --context "$TEST_CONTEXT" run --pull=never --detach \
    --name "$container_name" \
    --label "$API_OWNER_LABEL=$run_id" \
    --network "$API_NETWORK" \
    --publish "$API_HOST:$READY_API_PORT:3000" \
    --env-file "$env_file" \
    "$POSTGREST_IMAGE")" || fail 'could not start the pinned local PostgREST container'
  STARTUP_CONTAINER_ID="$container_id"
  save_container_id "$run_dir/$API_MANIFEST_NAME" "$run_id" "$container_id" \
    || fail 'could not record the owned PostgREST container ID'
  verify_owned_container "$container_name" "$run_id" "$container_id" "$READY_API_PORT"
  wait_for_loopback_api "$READY_API_PORT" "$run_dir/$API_MANIFEST_NAME"
  STARTUP_ACTIVE=0
  trap - EXIT
  printf 'shop guest import v2 API: started container=%s url=http://%s:%s manifest=%s\n' \
    "$container_name" "$API_HOST" "$READY_API_PORT" "$run_dir/$API_MANIFEST_NAME"
}

MODE='check'
RUN_DIR=''
case "${1:-}" in
  '')
    [[ "$#" == 0 ]] || fail 'accepts no arguments or one supported mode'
    ;;
  --check-only)
    [[ "$#" == 1 ]] || fail 'usage: --check-only'
    ;;
  --help)
    [[ "$#" == 1 ]] || fail 'usage: --help'
    printf 'Usage: bash %s [--check-only | --start RUN_DIR | --quiesce RUN_DIR | --stop RUN_DIR]\n' "${BASH_SOURCE[0]}"
    printf 'Starts or quiesces only an owned loopback PostgREST container; --stop performs final local cleanup.\n'
    exit 0
    ;;
  --start)
    [[ "$#" == 2 ]] || fail 'usage: --start RUN_DIR'
    MODE='start'
    RUN_DIR="$2"
    validate_run_dir_argument "$RUN_DIR" new
    ;;
  --quiesce)
    [[ "$#" == 2 ]] || fail 'usage: --quiesce RUN_DIR'
    MODE='quiesce'
    RUN_DIR="$2"
    validate_run_dir_argument "$RUN_DIR" existing
    ;;
  --stop)
    [[ "$#" == 2 ]] || fail 'usage: --stop RUN_DIR'
    MODE='stop'
    RUN_DIR="$2"
    validate_run_dir_argument "$RUN_DIR" existing
    ;;
  *) fail 'supported modes are --check-only, --start RUN_DIR, and --stop RUN_DIR' ;;
esac

if [[ "$MODE" != 'stop' ]]; then
for dependency in docker supabase python3 shasum awk lsof; do
  command -v "$dependency" >/dev/null 2>&1 || fail "$dependency is unavailable"
done
cli_version="$(SUPABASE_TELEMETRY_DISABLED=1 supabase --version 2>&1)" \
  || fail 'could not read the installed Supabase CLI version'
[[ "$cli_version" == "$EXPECTED_CLI_VERSION" ]] \
  || fail "expected Supabase CLI $EXPECTED_CLI_VERSION; found $cli_version"
[[ -f "$TEST_CONFIG" ]] || fail 'the pinned disposable project config is missing'
config_sha256="$(shasum -a 256 "$TEST_CONFIG" | awk '{print $1}')" \
  || fail 'could not hash the pinned disposable project config'
[[ "$config_sha256" == "$EXPECTED_CONFIG_SHA256" ]] \
  || fail 'the pinned disposable project config hash changed'
python3 - "$TEST_CONFIG" "$TEST_PROJECT_ID" "$TEST_DB_PORT" <<'PY'
import sys
import tomllib
from pathlib import Path

config_path, expected_project, expected_port = sys.argv[1:]
config = tomllib.loads(Path(config_path).read_text())
if config.get("project_id") != expected_project:
    raise SystemExit("shop guest import v2 API readiness: disposable project ID mismatch")
if str(config.get("db", {}).get("port")) != expected_port:
    raise SystemExit("shop guest import v2 API readiness: disposable database port mismatch")
if config.get("api", {}).get("enabled") is not False:
    raise SystemExit("shop guest import v2 API readiness: pinned config must keep its API disabled")
if config.get("db", {}).get("migrations", {}).get("enabled") is not False:
    raise SystemExit("shop guest import v2 API readiness: pinned config must keep migrations disabled")
if config.get("db", {}).get("seed", {}).get("enabled") is not False:
    raise SystemExit("shop guest import v2 API readiness: pinned config must keep seeding disabled")
PY

context="$(docker context show 2>/dev/null)" || fail 'could not identify Docker context'
[[ "$context" == "$TEST_CONTEXT" ]] || fail 'refusing a Docker context other than desktop-linux'
endpoint="$(docker --context "$TEST_CONTEXT" context inspect "$context" --format '{{(index .Endpoints "docker").Host}}' 2>/dev/null)" \
  || fail 'could not inspect the Docker endpoint'
[[ "$endpoint" == "$EXPECTED_DOCKER_ENDPOINT" ]] \
  || fail 'refusing a non-local or unexpected Docker endpoint'

container_json="$(docker --context "$TEST_CONTEXT" inspect --format '{{json .}}' "$TEST_DB_CONTAINER" 2>/dev/null)" \
  || fail 'the pinned disposable database container is unavailable'
python3 - "$container_json" "$TEST_PROJECT_ID" "$TEST_WORKDIR" "$TEST_DB_CONTAINER_ID" \
  "$TEST_DB_IMAGE_ID" "$TEST_DB_VOLUME" "$TEST_DB_PORT" "$API_NETWORK" <<'PY'
import json
import sys

raw, project, workdir, expected_id, expected_image, expected_volume, expected_port, expected_network = sys.argv[1:]
container = json.loads(raw)
if container.get("Id") != expected_id:
    raise SystemExit("shop guest import v2 API readiness: pinned database container ID mismatch")
if container.get("Image") != expected_image:
    raise SystemExit("shop guest import v2 API readiness: pinned database image identity mismatch")
if container.get("State", {}).get("Status") != "running":
    raise SystemExit("shop guest import v2 API readiness: pinned database container is not running")
if container.get("State", {}).get("Health", {}).get("Status") != "healthy":
    raise SystemExit("shop guest import v2 API readiness: pinned database container is not healthy")
config = container.get("Config", {})
labels = config.get("Labels") or {}
if config.get("WorkingDir") != "/":
    raise SystemExit("shop guest import v2 API readiness: pinned database container workdir mismatch")
if labels.get("com.docker.compose.project") != project:
    raise SystemExit("shop guest import v2 API readiness: Compose project label mismatch")
if labels.get("com.supabase.cli.project") != project:
    raise SystemExit("shop guest import v2 API readiness: Supabase project label mismatch")
if labels.get("com.supabase.cli.workdir") != workdir:
    raise SystemExit("shop guest import v2 API readiness: Supabase workdir label mismatch")
mounts = [m for m in container.get("Mounts", []) if m.get("Destination") == "/var/lib/postgresql/data"]
if len(mounts) != 1 or mounts[0].get("Type") != "volume" or mounts[0].get("Name") != expected_volume:
    raise SystemExit("shop guest import v2 API readiness: pinned database volume mismatch")
bindings = container.get("NetworkSettings", {}).get("Ports", {}).get("5432/tcp") or []
actual = {(item.get("HostIp"), item.get("HostPort")) for item in bindings}
expected = {("0.0.0.0", expected_port), ("::", expected_port)}
if actual != expected:
    raise SystemExit("shop guest import v2 API readiness: existing database host-port bindings changed")
network = (container.get("NetworkSettings", {}).get("Networks") or {}).get(expected_network)
if not network or "db" not in (network.get("Aliases") or []):
    raise SystemExit("shop guest import v2 API readiness: pinned database network or db alias mismatch")
PY

DB_URI_PASSWORD_ENCODED="$(python3 - "$container_json" <<'PY'
import json
import sys
from urllib.parse import quote

container = json.loads(sys.argv[1])
passwords = [entry.partition("=")[2] for entry in (container.get("Config", {}).get("Env") or []) if entry.startswith("POSTGRES_PASSWORD=")]
if len(passwords) != 1 or not passwords[0]:
    raise SystemExit("shop guest import v2 API readiness: could not identify the pinned local database password")
print(quote(passwords[0], safe=""))
PY
)" || fail 'could not prepare the pinned local PostgREST database URI'

db_network_id="$(python3 - "$container_json" "$API_NETWORK" <<'PY'
import json
import sys

container = json.loads(sys.argv[1])
network = (container.get("NetworkSettings", {}).get("Networks") or {}).get(sys.argv[2])
if not network or not network.get("NetworkID"):
    raise SystemExit("shop guest import v2 API readiness: pinned database network ID is missing")
print(network["NetworkID"])
PY
)" || fail 'could not identify the pinned database network'
network_json="$(docker --context "$TEST_CONTEXT" network inspect --format '{{json .}}' "$API_NETWORK" 2>/dev/null)" \
  || fail 'the existing pinned disposable database network is unavailable'
python3 - "$network_json" "$db_network_id" "$API_NETWORK" "$TEST_PROJECT_ID" "$TEST_DB_CONTAINER_ID" <<'PY'
import json
import sys

raw, expected_id, expected_name, project, db_container_id = sys.argv[1:]
network = json.loads(raw)
if network.get("Id") != expected_id or network.get("Name") != expected_name:
    raise SystemExit("shop guest import v2 API readiness: existing database network identity mismatch")
if network.get("Driver") != "bridge" or network.get("Scope") != "local":
    raise SystemExit("shop guest import v2 API readiness: database network is not the expected local bridge")
labels = network.get("Labels") or {}
if labels.get("com.docker.compose.project") != project:
    raise SystemExit("shop guest import v2 API readiness: database network Compose project mismatch")
attached = (network.get("Containers") or {}).get(db_container_id)
if not attached or attached.get("Name") != "supabase_db_" + project:
    raise SystemExit("shop guest import v2 API readiness: pinned database is not attached to its expected network")
PY

database_identity="$(docker --context "$TEST_CONTEXT" exec "$TEST_DB_CONTAINER" psql -X -q -A -t -U postgres -d postgres \
  -c "select current_database() || '|' || current_user || '|' || current_setting('port')" 2>/dev/null)" \
  || fail 'could not read the pinned local database identity'
[[ "$database_identity" == "$EXPECTED_DB_IDENTITY" ]] \
  || fail "pinned local database identity mismatch: $database_identity"

api_port="$(python3 - "$API_HOST" <<'PY'
import secrets
import subprocess
import sys

host = sys.argv[1]
for _ in range(64):
    port = 49152 + secrets.randbelow(16384)
    result = subprocess.run(
        ["lsof", "-nP", f"-iTCP:{host}:{port}", "-sTCP:LISTEN", "-t"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    if result.returncode == 1:
        print(port)
        break
    if result.returncode != 0:
        raise SystemExit("shop guest import v2 API readiness: could not inspect local API port")
else:
    raise SystemExit("shop guest import v2 API readiness: no unoccupied run-specific API port found")
PY
)" || fail 'could not select a run-specific local API port'
READY_API_PORT="$api_port"

printf 'shop guest import v2 API readiness: pinned CLI=%s config_sha256=%s context=%s endpoint=%s db_container=%s db_image=%s volume=%s db_host_port=%s db_identity=%s candidate_api=%s:%s (not reserved)\n' \
  "$cli_version" "$config_sha256" "$context" "$endpoint" "$TEST_DB_CONTAINER_ID" \
  "$TEST_DB_IMAGE_ID" "$TEST_DB_VOLUME" "$TEST_DB_PORT" "$database_identity" "$API_HOST" "$api_port"
if docker --context "$TEST_CONTEXT" image inspect "$POSTGREST_IMAGE" >/dev/null 2>&1; then
  printf 'shop guest import v2 API readiness: pinned PostgREST image is cached: %s\n' "$POSTGREST_IMAGE"
else
  fail "pinned PostgREST image is absent from the local cache: $POSTGREST_IMAGE; refusing pull or service start"
fi
fi

case "$MODE" in
  check) ;;
  start) start_owned_api "$RUN_DIR" ;;
  quiesce) quiesce_owned_api "$RUN_DIR" ;;
  stop) stop_owned_api "$RUN_DIR" ;;
esac
