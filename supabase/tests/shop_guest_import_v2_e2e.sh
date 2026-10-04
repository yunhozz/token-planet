#!/usr/bin/env bash
set -euo pipefail
set +x

# Default execution stays fail-closed. The lifecycle-only mode checks a single
# missing RPC; --run enables the approved local fixture and native E2E sequence.
readonly API_READINESS="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/shop_guest_import_v2_api.sh"
readonly NATIVE_E2E="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/shop_guest_import_v2_native.py"
readonly API_RUN_PREFIX='/private/tmp/shop-guest-import-v2-e2e.'
readonly FIXTURE_DOCKER_CONTEXT='desktop-linux'
readonly FIXTURE_DB_CONTAINER='supabase_db_token-planet-shop-revamp-test'
readonly EXPECTED_FIXTURE_DB_DIGEST='ce9ef4b37d29422d9d8cd8cb3c9d9bba'

fail() {
  printf 'shop guest import v2 E2E: %s\n' "$1" >&2
  exit 1
}

validate_loopback_api_url() {
  python3 - "$1" <<'PY'
import sys
from urllib.parse import urlsplit

value = sys.argv[1]
try:
    parsed = urlsplit(value)
    port = parsed.port
except ValueError as error:
    raise SystemExit(f"shop guest import v2 E2E: invalid local API URL: {error}")
if parsed.scheme != "http" or parsed.hostname != "127.0.0.1" or parsed.username or parsed.password:
    raise SystemExit("shop guest import v2 E2E: API URL must use loopback HTTP without credentials")
if port is None or not 49152 <= port <= 65535:
    raise SystemExit("shop guest import v2 E2E: API URL port is outside the owned test range")
if parsed.path not in ("", "/") or parsed.query or parsed.fragment:
    raise SystemExit("shop guest import v2 E2E: API URL must be an origin with no path or query")
print(port)
PY
}

write_owned_proxy_script() {
  local run_dir="$1"
  local proxy_script="$run_dir/proxy.py"
  python3 - "$run_dir" <<'PY'
import os
import re
import stat
import sys

run_dir = sys.argv[1]
prefix = "/private/tmp/shop-guest-import-v2-e2e."
if not re.fullmatch(re.escape(prefix) + r"[a-f0-9]{24}", run_dir):
    raise SystemExit("shop guest import v2 E2E: invalid proxy run directory")
info = os.lstat(run_dir)
if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) != 0o700:
    raise SystemExit("shop guest import v2 E2E: proxy run directory is not current-user-owned mode 0700")
if os.path.realpath(run_dir) != run_dir:
    raise SystemExit("shop guest import v2 E2E: proxy run directory is not canonical")
PY
  [[ ! -e "$proxy_script" && ! -L "$proxy_script" ]] \
    || fail 'owned proxy script path already exists; refusing to replace it'
  (umask 077; cat > "$proxy_script" <<'PROXY_PYTHON_EOF'
#!/usr/bin/env python3
import argparse
import http.server
import re
import sys
import urllib.error
import urllib.request
from urllib.parse import parse_qsl, urlsplit, urlunsplit

# BEGIN_TASK9_PROXY_HANDLER
def validate_loopback_url(value):
    try:
        parsed = urlsplit(value)
        port = parsed.port
    except ValueError as error:
        raise ValueError("invalid API URL") from error
    if parsed.scheme != "http" or parsed.hostname != "127.0.0.1" or parsed.username or parsed.password:
        raise ValueError("API URL must use loopback HTTP without credentials")
    if port is None or not 49152 <= port <= 65535:
        raise ValueError("API URL port is outside the owned test range")
    if parsed.path not in ("", "/") or parsed.query or parsed.fragment:
        raise ValueError("API URL must be an origin with no path or query")
    return port


def rewrite_rest_path(method, value):
    parsed = urlsplit(value)
    if parsed.scheme or parsed.netloc or parsed.fragment:
        raise ValueError("absolute-form request targets are not allowed")
    if method == "POST":
        match = re.fullmatch(r"/rest/v1/rpc/([A-Za-z_][A-Za-z0-9_]*)", parsed.path)
        if match:
            return urlunsplit(("", "", "/rpc/" + match.group(1), parsed.query, ""))
        if parsed.path == "/rest/v1/worlds" and not parsed.query:
            return "/worlds"
    if method == "GET" and parsed.path == "/rest/v1/worlds":
        try:
            query = parse_qsl(parsed.query, keep_blank_values=True, strict_parsing=True)
        except ValueError as error:
            raise ValueError("invalid world-read query") from error
        if query == [("select", "id,name,timezone,owner_id")]:
            return urlunsplit(("", "", "/worlds", parsed.query, ""))
    raise ValueError("request method or path is not allowed")
# END_TASK9_PROXY_HANDLER


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, new_url):
        return None


class ProxyHandler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    server_version = "GuestImportLoopbackProxy/1"

    def log_message(self, _format, *_args):
        return

    def _forward(self):
        try:
            rewritten = rewrite_rest_path(self.command, self.path)
        except ValueError:
            self.send_error(404)
            return
        if self.headers.get("Transfer-Encoding"):
            self.send_error(400)
            return
        try:
            content_length = int(self.headers.get("Content-Length", "0"))
        except ValueError:
            self.send_error(400)
            return
        if content_length < 0:
            self.send_error(400)
            return
        if self.command == "GET" and content_length != 0:
            self.send_error(400)
            return
        body = self.rfile.read(content_length) if self.command == "POST" else None
        excluded = {"connection", "content-length", "host", "keep-alive", "proxy-connection", "te", "trailer", "transfer-encoding", "upgrade"}
        headers = {name: value for name, value in self.headers.items() if name.lower() not in excluded}
        request = urllib.request.Request(self.server.upstream + rewritten, data=body, headers=headers, method=self.command)
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect)
        try:
            response = opener.open(request, timeout=30)
        except urllib.error.HTTPError as error:
            response = error
        except (urllib.error.URLError, TimeoutError, OSError):
            self.send_error(502, "local PostgREST unavailable")
            return
        with response:
            if 300 <= response.status < 400:
                self.send_error(502, "local PostgREST redirect refused")
                return
            response_body = response.read()
            self.send_response(response.status)
            excluded_response = excluded | {"content-encoding"}
            for name, value in response.headers.items():
                if name.lower() not in excluded_response:
                    self.send_header(name, value)
            self.send_header("Content-Length", str(len(response_body)))
            self.end_headers()
            if self.command != "HEAD":
                self.wfile.write(response_body)

    def do_GET(self):
        self._forward()

    def do_POST(self):
        self._forward()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--listen", required=True)
    parser.add_argument("--port", required=True, type=int)
    parser.add_argument("--upstream", required=True)
    parser.add_argument("--owner", required=True)
    args = parser.parse_args()
    if args.listen != "127.0.0.1":
        raise SystemExit("proxy listen address must be 127.0.0.1")
    if not re.fullmatch(r"[a-f0-9]{24}", args.owner):
        raise SystemExit("proxy owner token is invalid")
    validate_loopback_url(args.upstream)
    if args.port != 0 and not 49152 <= args.port <= 65535:
        raise SystemExit("proxy port is outside the owned test range")
    server = http.server.ThreadingHTTPServer((args.listen, args.port), ProxyHandler)
    server.daemon_threads = True
    server.upstream = args.upstream.rstrip("/")
    print(server.server_address[1], flush=True)
    try:
        server.serve_forever()
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
PROXY_PYTHON_EOF
  ) || fail 'could not write the owned loopback proxy script'
  chmod 600 "$proxy_script" || fail 'could not restrict proxy script permissions'
}

OWNED_PROXY_PID=''
OWNED_PROXY_URL=''
OWNED_PROXY_RUN_DIR=''
OWNED_PROXY_RUN_ID=''
OWNED_PROXY_SCRIPT=''
OWNED_PROXY_UPSTREAM_URL=''
OWNED_PROXY_SCRIPT_CREATED=0
OWNED_PROXY_PORT_CREATED=0
OWNED_PROXY_LOG_CREATED=0
OWNED_API_STARTED=0
OWNED_API_RUN_DIR=''

start_owned_proxy() {
  [[ "$#" -eq 2 ]] || fail 'start_owned_proxy requires a run directory and upstream URL'
  local run_dir="$1"
  local upstream_url="$2"
  local run_id="${run_dir##*.}"
  local proxy_script="$run_dir/proxy.py"
  local port_file="$run_dir/proxy.port"
  local log_file="$run_dir/proxy.log"
  local attempt=0
  local reported_port=''

  [[ "$run_dir" =~ ^/private/tmp/shop-guest-import-v2-e2e\.[a-f0-9]{24}$ ]] \
    || fail 'invalid owned proxy run directory'
  [[ "$run_id" =~ ^[a-f0-9]{24}$ ]] || fail 'invalid owned proxy run id'
  [[ -z "$OWNED_PROXY_PID" && -z "$OWNED_PROXY_RUN_DIR" ]] \
    || fail 'an owned proxy is already tracked by this E2E run'
  validate_loopback_api_url "$upstream_url" >/dev/null \
    || fail 'invalid local API URL for owned proxy'
  write_owned_proxy_script "$run_dir"
  OWNED_PROXY_RUN_DIR="$run_dir"
  OWNED_PROXY_RUN_ID="$run_id"
  OWNED_PROXY_SCRIPT="$proxy_script"
  OWNED_PROXY_UPSTREAM_URL="$upstream_url"
  OWNED_PROXY_SCRIPT_CREATED=1

  [[ ! -e "$port_file" && ! -L "$port_file" ]] \
    || fail 'owned proxy port path already exists; refusing to replace it'
  [[ ! -e "$log_file" && ! -L "$log_file" ]] \
    || fail 'owned proxy log path already exists; refusing to replace it'
  (umask 077; set -o noclobber; : > "$port_file") \
    || fail 'could not create the owned proxy port file exclusively'
  OWNED_PROXY_PORT_CREATED=1
  (umask 077; set -o noclobber; : > "$log_file") \
    || fail 'could not create the owned proxy log file exclusively'
  OWNED_PROXY_LOG_CREATED=1
  chmod 600 "$port_file" "$log_file" \
    || fail 'could not restrict owned proxy file permissions'

  python3 "$proxy_script" \
    --listen 127.0.0.1 \
    --port 0 \
    --upstream "$upstream_url" \
    --owner "$run_id" \
    > "$log_file" 2>&1 &
  OWNED_PROXY_PID=$!

  while (( attempt < 50 )); do
    if ! kill -0 "$OWNED_PROXY_PID" 2>/dev/null; then
      wait "$OWNED_PROXY_PID" 2>/dev/null || true
      OWNED_PROXY_PID=''
      fail 'owned proxy exited before reporting its loopback port'
    fi

    if [[ -s "$log_file" ]]; then
      IFS= read -r reported_port < "$log_file" || true
      if [[ "$reported_port" =~ ^[0-9]{1,5}$ ]]; then
        local port_number=$((10#$reported_port))
        if (( port_number >= 49152 && port_number <= 65535 )); then
          printf '%s\n' "$port_number" > "$port_file" \
            || fail 'could not record the owned proxy port'
          OWNED_PROXY_URL="http://127.0.0.1:$port_number"
          return 0
        fi
      fi
      kill "$OWNED_PROXY_PID" 2>/dev/null || true
      wait "$OWNED_PROXY_PID" 2>/dev/null || true
      OWNED_PROXY_PID=''
      fail 'owned proxy reported a port outside the owned test range'
    fi

    sleep 0.1
    attempt=$((attempt + 1))
  done

  kill "$OWNED_PROXY_PID" 2>/dev/null || true
  wait "$OWNED_PROXY_PID" 2>/dev/null || true
  OWNED_PROXY_PID=''
  fail 'timed out waiting for the owned proxy to report its loopback port'
}

prepare_native_run() {
  [[ "$#" -eq 2 ]] || fail 'prepare_native_run requires a run directory and proxy URL'
  python3 - "$1" "$2" <<'PY'
import os
import re
import stat
import sys
from pathlib import Path
from urllib.parse import urlsplit

run_dir, proxy_url = sys.argv[1:]
prefix = "/private/tmp/shop-guest-import-v2-e2e."
run_id = run_dir[len(prefix):] if run_dir.startswith(prefix) else ""
if not re.fullmatch(r"[a-f0-9]{24}", run_id):
    raise SystemExit("shop guest import v2 E2E: native run directory is invalid")
if os.path.normpath(run_dir) != run_dir or os.path.realpath(run_dir) != run_dir:
    raise SystemExit("shop guest import v2 E2E: native run directory is not canonical")

def owned_mode(path, expected_mode, description, directory=False):
    try:
        info = os.lstat(path)
    except OSError:
        raise SystemExit(f"shop guest import v2 E2E: {description} is unavailable")
    expected_type = stat.S_ISDIR(info.st_mode) if directory else stat.S_ISREG(info.st_mode)
    if (not expected_type or stat.S_ISLNK(info.st_mode) or info.st_uid != os.getuid()
            or stat.S_IMODE(info.st_mode) != expected_mode):
        raise SystemExit(f"shop guest import v2 E2E: {description} ownership or mode check failed")
    return info

owned_mode(run_dir, 0o700, "native run directory", directory=True)
port_path = os.path.join(run_dir, "proxy.port")
port_info = owned_mode(port_path, 0o600, "owned proxy port file")
try:
    with open(port_path, "rb") as stream:
        port_contents = stream.read()
except OSError:
    raise SystemExit("shop guest import v2 E2E: owned proxy port file is unavailable")
digits = port_contents[:-1] if port_contents.endswith(b"\n") else port_contents
if (len(digits) != 5 or not digits.isdigit() or b"\n" in digits
        or b"\r" in digits):
    raise SystemExit("shop guest import v2 E2E: owned proxy port file is not canonical")
try:
    port = int(digits.decode("ascii"))
    parsed = urlsplit(proxy_url)
    url_port = parsed.port
except (UnicodeDecodeError, ValueError):
    raise SystemExit("shop guest import v2 E2E: owned proxy URL is invalid")
if (not 49152 <= port <= 65535 or str(port).encode("ascii") != digits
        or parsed.scheme != "http" or parsed.hostname != "127.0.0.1"
        or parsed.username or parsed.password or url_port != port
        or parsed.path or parsed.query or parsed.fragment
        or proxy_url != f"http://127.0.0.1:{port}"):
    raise SystemExit("shop guest import v2 E2E: proxy URL does not match the owned port file")

source_root = Path(run_dir) / "sources"
world_id_path = Path(run_dir) / "world-id.txt"
codex_root = source_root / "emptycodex"
claude_root = source_root / "emptyclaude"
for path, description in (
    (source_root, "native source root"),
    (codex_root, "empty Codex source root"),
    (claude_root, "empty Claude source root"),
    (world_id_path, "world ID file"),
):
    if os.path.lexists(path):
        raise SystemExit(f"shop guest import v2 E2E: refusing pre-existing {description}")

old_umask = os.umask(0o077)
try:
    os.mkdir(source_root, 0o700)
    os.mkdir(codex_root, 0o700)
    os.mkdir(claude_root, 0o700)
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    flags |= getattr(os, "O_NOFOLLOW", 0)
    world_fd = os.open(world_id_path, flags, 0o600)
    os.close(world_fd)
finally:
    os.umask(old_umask)

for path, description in (
    (source_root, "native source root"),
    (codex_root, "empty Codex source root"),
    (claude_root, "empty Claude source root"),
):
    owned_mode(path, 0o700, description, directory=True)
if sorted(path.name for path in source_root.iterdir()) != ["emptyclaude", "emptycodex"]:
    raise SystemExit("shop guest import v2 E2E: native source root has unexpected children")
for path, description in (
    (codex_root, "empty Codex source root"),
    (claude_root, "empty Claude source root"),
):
    if any(path.iterdir()):
        raise SystemExit(f"shop guest import v2 E2E: {description} is not empty")
world_info = owned_mode(world_id_path, 0o600, "world ID file")
if world_info.st_size != 0:
    raise SystemExit("shop guest import v2 E2E: world ID file must start empty")
if os.stat(port_path, follow_symlinks=False).st_ino != port_info.st_ino:
    raise SystemExit("shop guest import v2 E2E: proxy port file changed during native preflight")
PY
}

fixture_psql() {
  [[ "$#" -eq 1 ]] || fail 'fixture_psql requires one SQL statement'
  docker --context "$FIXTURE_DOCKER_CONTEXT" exec "$FIXTURE_DB_CONTAINER" \
    psql -X -q -A -t -v ON_ERROR_STOP=1 -U postgres -d postgres -c "$1"
}

fixture_database_digest() {
  local sql
  sql="$(cat <<'SQL'
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
)" || fail 'could not prepare the protected database digest query'
  fixture_psql "$sql"
}

prepare_fixture_manifest() {
  [[ "$#" -eq 1 ]] || fail 'prepare_fixture_manifest requires a run directory'
  local run_dir="$1"
  local actual_digest run_id user_ids owner_id viewer_id outsider_id absence_sql existing_ids
  run_id="${run_dir##*.}"
  [[ "$run_dir" == "$API_RUN_PREFIX$run_id" && "$run_id" =~ ^[a-f0-9]{24}$ ]] \
    || fail 'fixture manifest run directory is invalid'

  actual_digest="$(fixture_database_digest)" \
    || fail 'could not read the initial protected database digest'
  [[ "$actual_digest" =~ ^[a-f0-9]{32}$ ]] \
    || fail 'initial protected database digest is invalid'
  [[ "$actual_digest" == "$EXPECTED_FIXTURE_DB_DIGEST" ]] \
    || fail 'initial protected database digest does not match the approved baseline'

  user_ids="$(python3 -c 'import uuid; print("\t".join(str(uuid.uuid4()) for _ in range(3)))')" \
    || fail 'could not generate synthetic fixture user IDs'
  IFS=$'\t' read -r owner_id viewer_id outsider_id <<<"$user_ids"
  [[ "$owner_id" =~ ^[0-9a-f-]{36}$ && "$viewer_id" =~ ^[0-9a-f-]{36}$ \
      && "$outsider_id" =~ ^[0-9a-f-]{36}$ ]] \
    || fail 'generated synthetic fixture user IDs are invalid'

  absence_sql="SELECT id::text FROM auth.users WHERE id IN ('$owner_id'::uuid, '$viewer_id'::uuid, '$outsider_id'::uuid) ORDER BY id;"
  existing_ids="$(fixture_psql "$absence_sql")" \
    || fail 'could not prove synthetic fixture IDs absent from auth.users'
  [[ -z "$existing_ids" ]] \
    || fail 'a synthetic fixture ID already exists in auth.users; refusing fixture setup'

  python3 - "$run_dir" "$run_id" "$actual_digest" "$owner_id" "$viewer_id" "$outsider_id" \
    "$EXPECTED_FIXTURE_DB_DIGEST" <<'PY'
import json
import os
import re
import stat
import sys
import uuid

run_dir, run_id, digest, owner_id, viewer_id, outsider_id, expected_digest = sys.argv[1:]
def reject(message):
    raise SystemExit(f"shop guest import v2 E2E: {message}")

prefix = "/private/tmp/shop-guest-import-v2-e2e."
if run_dir != prefix + run_id or not re.fullmatch(r"[a-f0-9]{24}", run_id):
    reject("fixture manifest run directory is invalid")
if os.path.realpath(run_dir) != run_dir:
    reject("fixture manifest run directory is not canonical")
run_info = os.lstat(run_dir)
if not stat.S_ISDIR(run_info.st_mode) or run_info.st_uid != os.getuid() or stat.S_IMODE(run_info.st_mode) != 0o700:
    reject("fixture manifest run directory ownership or mode check failed")
runtime_path = os.path.join(run_dir, "runtime.env")
runtime_info = os.stat(runtime_path, follow_symlinks=False)
if (not stat.S_ISREG(runtime_info.st_mode) or runtime_info.st_uid != os.getuid()
        or stat.S_IMODE(runtime_info.st_mode) != 0o600):
    reject("API runtime manifest ownership or mode check failed")
try:
    with open(runtime_path, encoding="utf-8") as stream:
        runtime = json.load(stream)
except (OSError, json.JSONDecodeError):
    reject("API runtime manifest is invalid")
if runtime.get("run_id") != run_id or runtime.get("run_dir") != run_dir:
    reject("API runtime manifest does not match this fixture run")
if digest != expected_digest or not re.fullmatch(r"[0-9a-f]{32}", digest):
    reject("initial protected database digest does not match the approved baseline")
ids = {"owner": owner_id, "viewer": viewer_id, "outsider": outsider_id}
try:
    parsed = {name: uuid.UUID(value) for name, value in ids.items()}
except ValueError:
    reject("synthetic fixture user IDs are invalid")
if len(set(ids.values())) != 3 or any(str(parsed[name]) != value or parsed[name].version != 4 for name, value in ids.items()):
    reject("synthetic fixture user IDs are not unique canonical UUIDv4 values")
manifest_path = os.path.join(run_dir, "fixture-manifest.json")
if os.path.lexists(manifest_path):
    reject("fixture manifest already exists; refusing to replace it")
manifest = {
    "version": 1,
    "run_id": run_id,
    "run_dir": run_dir,
    "fixture_user_ids": ids,
    "initial_auth_user_absence_proven": True,
    "initial_database_digest": digest,
    "cleanup_eligible": True,
    "fixture_rows_created": False,
}
fd = os.open(manifest_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0), 0o600)
with os.fdopen(fd, "w", encoding="utf-8") as stream:
    json.dump(manifest, stream, separators=(",", ":"))
    stream.write("\n")
    stream.flush()
    os.fsync(stream.fileno())
os.chmod(manifest_path, 0o600)
PY
}

read_fixture_manifest_ids() {
  [[ "$#" -eq 1 ]] || fail 'read_fixture_manifest_ids requires a run directory'
  python3 - "$1" "$EXPECTED_FIXTURE_DB_DIGEST" <<'PY'
import json
import os
import re
import stat
import sys
import uuid

run_dir, expected_digest = sys.argv[1:]
def reject(message):
    raise SystemExit(f"shop guest import v2 E2E: {message}")

prefix = "/private/tmp/shop-guest-import-v2-e2e."
run_id = run_dir[len(prefix):] if run_dir.startswith(prefix) else ""
if run_dir != prefix + run_id or not re.fullmatch(r"[a-f0-9]{24}", run_id):
    reject("fixture insertion run directory is invalid")
try:
    run_info = os.lstat(run_dir)
except OSError:
    reject("fixture run directory is unavailable")
if (not stat.S_ISDIR(run_info.st_mode) or stat.S_ISLNK(run_info.st_mode)
        or run_info.st_uid != os.getuid() or stat.S_IMODE(run_info.st_mode) != 0o700
        or os.path.realpath(run_dir) != run_dir):
    reject("fixture run directory ownership, mode, or canonical path check failed")
manifest_path = os.path.join(run_dir, "fixture-manifest.json")
try:
    info = os.stat(manifest_path, follow_symlinks=False)
except OSError:
    reject("protected fixture manifest is unavailable before auth DML")
if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) != 0o600:
    reject("protected fixture manifest ownership or mode check failed")
try:
    with open(manifest_path, encoding="utf-8") as stream:
        manifest = json.load(stream)
except (OSError, json.JSONDecodeError):
    reject("protected fixture manifest is invalid")
if (manifest.get("run_id") != run_id or manifest.get("run_dir") != run_dir
        or manifest.get("version") != 1
        or manifest.get("cleanup_eligible") is not True
        or manifest.get("initial_auth_user_absence_proven") is not True
        or manifest.get("initial_database_digest") != expected_digest):
    reject("fixture manifest lacks required pre-DML cleanup proofs")
ids = manifest.get("fixture_user_ids")
if not isinstance(ids, dict) or set(ids) != {"owner", "viewer", "outsider"}:
    reject("fixture manifest user IDs are incomplete")
values = [ids[name] for name in ("owner", "viewer", "outsider")]
try:
    parsed = [uuid.UUID(value) for value in values]
except (TypeError, ValueError):
    reject("fixture manifest user IDs are invalid")
if len(set(values)) != 3 or any(str(parsed[index]) != values[index] or parsed[index].version != 4
                                for index in range(3)):
    reject("fixture manifest user IDs are not unique canonical UUIDv4 values")
print("\t".join((*values, manifest["initial_database_digest"])))
PY
}

insert_fixture_users() {
  [[ "$#" -eq 1 ]] || fail 'insert_fixture_users requires a run directory'
  local run_dir="$1"
  local fixture_ids owner_id viewer_id outsider_id digest sql
  fixture_ids="$(read_fixture_manifest_ids "$run_dir")" \
    || fail 'could not validate the protected fixture manifest before auth DML'
  IFS=$'\t' read -r owner_id viewer_id outsider_id digest <<<"$fixture_ids"
  [[ "$digest" == "$EXPECTED_FIXTURE_DB_DIGEST" ]] \
    || fail 'fixture manifest database baseline does not match the approved baseline'
  sql="INSERT INTO auth.users(id) VALUES ('$owner_id'::uuid), ('$viewer_id'::uuid), ('$outsider_id'::uuid);"
  fixture_psql "$sql" || fail 'synthetic auth fixture insert failed; protected cleanup manifest retained'
}

fixture_cleanup_files() {
  if [[ "$#" -ne 6 ]]; then
    printf 'shop guest import v2 E2E cleanup: fixture_cleanup_files received invalid arguments\n' >&2
    return 1
  fi
  python3 - "$@" <<'PY'
import json
import os
import re
import stat
import sys
import uuid

run_dir, owner_id, viewer_id, outsider_id, expected_digest, action = sys.argv[1:]
def reject(message):
    raise SystemExit(f"shop guest import v2 E2E cleanup: {message}; preserving protected files")

prefix = "/private/tmp/shop-guest-import-v2-e2e."
run_id = run_dir[len(prefix):] if run_dir.startswith(prefix) else ""
if run_dir != prefix + run_id or not re.fullmatch(r"[a-f0-9]{24}", run_id):
    reject("fixture run directory is invalid")
try:
    root = os.lstat(run_dir)
except OSError:
    reject("fixture run directory is unavailable")
if (not stat.S_ISDIR(root.st_mode) or stat.S_ISLNK(root.st_mode)
        or root.st_uid != os.getuid() or stat.S_IMODE(root.st_mode) != 0o700
        or os.path.realpath(run_dir) != run_dir):
    reject("fixture run directory ownership, mode, or canonical path check failed")
if action not in {"check", "remove"}:
    reject("fixture cleanup action is invalid")
if not re.fullmatch(r"[0-9a-f]{32}", expected_digest):
    reject("fixture baseline digest is invalid")
expected_ids = {"owner": owner_id, "viewer": viewer_id, "outsider": outsider_id}
try:
    parsed_ids = {name: uuid.UUID(value) for name, value in expected_ids.items()}
except (TypeError, ValueError):
    reject("fixture user IDs are invalid")
if (len(set(expected_ids.values())) != 3
        or any(str(parsed_ids[name]) != value or parsed_ids[name].version != 4
               for name, value in expected_ids.items())):
    reject("fixture user IDs are not unique canonical UUIDv4 values")

def owned_path(name, expected_mode=0o600, required=False, directory=False):
    path = os.path.join(run_dir, name)
    if not os.path.lexists(path):
        if required:
            reject(f"required fixture cleanup path {name} is unavailable")
        return None
    try:
        info = os.lstat(path)
    except OSError:
        reject(f"fixture cleanup path {name} is unavailable")
    correct_type = stat.S_ISDIR(info.st_mode) if directory else stat.S_ISREG(info.st_mode)
    if (not correct_type or stat.S_ISLNK(info.st_mode) or info.st_uid != os.getuid()
            or stat.S_IMODE(info.st_mode) != expected_mode or os.path.realpath(path) != path):
        reject(f"fixture cleanup path {name} ownership, type, or mode check failed")
    return path

manifest_path = owned_path("fixture-manifest.json", required=True)
try:
    with open(manifest_path, encoding="utf-8") as stream:
        manifest = json.load(stream)
except (OSError, json.JSONDecodeError):
    reject("protected fixture manifest is invalid")
if (manifest.get("version") != 1 or manifest.get("run_id") != run_id
        or manifest.get("run_dir") != run_dir or manifest.get("cleanup_eligible") is not True
        or manifest.get("initial_auth_user_absence_proven") is not True
        or manifest.get("initial_database_digest") != expected_digest
        or manifest.get("fixture_user_ids") != expected_ids):
    reject("protected fixture manifest no longer matches the approved cleanup proof")

# These are the only expected run artifacts. In particular, unknown entries
# are never removed or silently ignored by fixture cleanup.
known_files = {
    "runtime.env", "postgrest.env", "auth-shim.json", "proxy.py", "proxy.port", "proxy.log",
    "native-env.json", "native.log",
    "world-id.txt", "fixture-manifest.json",
}
known_directories = {"sources"}
try:
    entries = set(os.listdir(run_dir))
except OSError:
    reject("fixture run directory cannot be listed")
unknown = entries - known_files - known_directories
if unknown:
    reject("unknown run-directory entries are present: " + ", ".join(sorted(unknown)))

for name in ("runtime.env", "postgrest.env", "auth-shim.json", "proxy.py", "proxy.port", "proxy.log",
             "native-env.json", "native.log"):
    owned_path(name)
world_path = owned_path("world-id.txt", required=True)
try:
    world_contents = open(world_path, "rb").read()
except OSError:
    reject("world ID file cannot be read")
if world_contents not in (b"",) and not re.fullmatch(rb"[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}\n?", world_contents):
    reject("world ID file contains an invalid value")

source_path = owned_path("sources", 0o700, required=True, directory=True)
try:
    source_names = sorted(os.listdir(source_path))
except OSError:
    reject("native source root cannot be listed")
if source_names != ["emptyclaude", "emptycodex"]:
    reject("native source root contains unknown entries")
for name in source_names:
    path = os.path.join(source_path, name)
    try:
        info = os.lstat(path)
    except OSError:
        reject(f"native source path {name} is unavailable")
    if (not stat.S_ISDIR(info.st_mode) or stat.S_ISLNK(info.st_mode)
            or info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) != 0o700
            or os.path.realpath(path) != path):
        reject(f"native source path {name} ownership, type, or mode check failed")
    try:
        if os.listdir(path):
            reject(f"native source path {name} is not empty")
    except OSError:
        reject(f"native source path {name} cannot be listed")

if action == "remove":
    for name in ("native-env.json", "native.log"):
        path = os.path.join(run_dir, name)
        if os.path.lexists(path):
            os.unlink(path)
    os.unlink(world_path)
    for name in source_names:
        os.rmdir(os.path.join(source_path, name))
    os.rmdir(source_path)
    os.unlink(manifest_path)
PY
}

cleanup_unmanifested_native_run() {
  if [[ "$#" -ne 1 || -z "$OWNED_API_RUN_DIR" || "$1" != "$OWNED_API_RUN_DIR" ]]; then
    printf 'shop guest import v2 E2E cleanup: unmanifested native cleanup lacks the exact owned run directory\n' >&2
    return 1
  fi
  python3 - "$1" <<'PY'
import os
import re
import stat
import sys

run_dir = sys.argv[1]
def reject(message):
    raise SystemExit(f"shop guest import v2 E2E cleanup: {message}; preserving unmanifested artifacts")

prefix = "/private/tmp/shop-guest-import-v2-e2e."
run_id = run_dir[len(prefix):] if run_dir.startswith(prefix) else ""
if run_dir != prefix + run_id or not re.fullmatch(r"[a-f0-9]{24}", run_id):
    reject("unmanifested run directory is invalid")
try:
    root = os.lstat(run_dir)
except OSError:
    reject("unmanifested run directory is unavailable")
if (not stat.S_ISDIR(root.st_mode) or stat.S_ISLNK(root.st_mode)
        or root.st_uid != os.getuid() or stat.S_IMODE(root.st_mode) != 0o700
        or os.path.realpath(run_dir) != run_dir):
    reject("unmanifested run directory ownership, mode, or canonical path check failed")

known_files = {"runtime.env", "postgrest.env", "auth-shim.json", "proxy.py", "proxy.port", "proxy.log"}
known_paths = known_files | {"world-id.txt", "sources"}
try:
    entries = set(os.listdir(run_dir))
except OSError:
    reject("unmanifested run directory cannot be listed")
unknown = entries - known_paths
if unknown:
    reject("unknown unmanifested run entries are present: " + ", ".join(sorted(unknown)))

for name in known_files:
    path = os.path.join(run_dir, name)
    if not os.path.lexists(path):
        continue
    try:
        info = os.lstat(path)
    except OSError:
        reject(f"unmanifested API path {name} is unavailable")
    if (not stat.S_ISREG(info.st_mode) or stat.S_ISLNK(info.st_mode)
            or info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) != 0o600
            or os.path.realpath(path) != path):
        reject(f"unmanifested API path {name} ownership or mode check failed")

world_path = os.path.join(run_dir, "world-id.txt")
if os.path.lexists(world_path):
    try:
        world_info = os.lstat(world_path)
        with open(world_path, "rb") as stream:
            world_contents = stream.read()
    except OSError:
        reject("unmanifested world ID file is unavailable")
    if (not stat.S_ISREG(world_info.st_mode) or stat.S_ISLNK(world_info.st_mode)
            or world_info.st_uid != os.getuid() or stat.S_IMODE(world_info.st_mode) != 0o600
            or os.path.realpath(world_path) != world_path or world_contents):
        reject("unmanifested world ID file is not an owned empty preflight artifact")

source_path = os.path.join(run_dir, "sources")
source_names = []
if os.path.lexists(source_path):
    try:
        source_info = os.lstat(source_path)
        source_names = sorted(os.listdir(source_path))
    except OSError:
        reject("unmanifested source root is unavailable")
    if (not stat.S_ISDIR(source_info.st_mode) or stat.S_ISLNK(source_info.st_mode)
            or source_info.st_uid != os.getuid() or stat.S_IMODE(source_info.st_mode) != 0o700
            or os.path.realpath(source_path) != source_path
            or not set(source_names) <= {"emptycodex", "emptyclaude"}):
        reject("unmanifested source root ownership, mode, or layout check failed")
    for name in source_names:
        path = os.path.join(source_path, name)
        try:
            info = os.lstat(path)
            children = os.listdir(path)
        except OSError:
            reject(f"unmanifested source path {name} is unavailable")
        if (not stat.S_ISDIR(info.st_mode) or stat.S_ISLNK(info.st_mode)
                or info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) != 0o700
                or os.path.realpath(path) != path or children):
            reject(f"unmanifested source path {name} is not an owned empty directory")

# This branch only removes empty, preflight-created source roots and an empty
# world-id file. API/proxy files and all unknown paths stay in place.
if os.path.lexists(world_path):
    os.unlink(world_path)
for name in source_names:
    os.rmdir(os.path.join(source_path, name))
if os.path.lexists(source_path):
    os.rmdir(source_path)
PY
}

cleanup_fixture_users() {
  if [[ "$#" -ne 1 ]]; then
    printf 'shop guest import v2 E2E cleanup: cleanup_fixture_users requires a run directory\n' >&2
    return 1
  fi
  local run_dir="$1"
  local manifest_path="$run_dir/fixture-manifest.json"
  if [[ ! -e "$manifest_path" && ! -L "$manifest_path" ]]; then
    cleanup_unmanifested_native_run "$run_dir" || return 1
    return 0
  fi

  local fixture_ids owner_id viewer_id outsider_id digest sql after_digest after_ids
  fixture_ids="$(read_fixture_manifest_ids "$run_dir")" \
    || { printf 'shop guest import v2 E2E cleanup: could not validate the protected fixture manifest; preserving files\n' >&2; return 1; }
  IFS=$'\t' read -r owner_id viewer_id outsider_id digest <<<"$fixture_ids"
  if [[ "$digest" != "$EXPECTED_FIXTURE_DB_DIGEST" ]]; then
    printf 'shop guest import v2 E2E cleanup: fixture cleanup baseline does not match the approved database state; preserving files\n' >&2
    return 1
  fi
  fixture_cleanup_files "$run_dir" "$owner_id" "$viewer_id" "$outsider_id" "$digest" check \
    || { printf 'shop guest import v2 E2E cleanup: fixture cleanup file ownership or layout proof failed; preserving protected files\n' >&2; return 1; }

  sql="BEGIN;
DELETE FROM public.worlds WHERE owner_id IN ('$owner_id'::uuid, '$viewer_id'::uuid, '$outsider_id'::uuid);
DELETE FROM auth.users WHERE id IN ('$owner_id'::uuid, '$viewer_id'::uuid, '$outsider_id'::uuid);
COMMIT;"
  fixture_psql "$sql" \
    || { printf 'shop guest import v2 E2E cleanup: scoped fixture database cleanup failed; protected files retained for retry\n' >&2; return 1; }
  after_digest="$(fixture_database_digest)" \
    || { printf 'shop guest import v2 E2E cleanup: could not verify database digest after cleanup; protected files retained\n' >&2; return 1; }
  if [[ "$after_digest" != "$digest" || "$after_digest" != "$EXPECTED_FIXTURE_DB_DIGEST" ]]; then
    printf 'shop guest import v2 E2E cleanup: post-cleanup database digest differs from the approved baseline; protected files retained\n' >&2
    return 1
  fi

  after_ids="$(read_fixture_manifest_ids "$run_dir")" \
    || { printf 'shop guest import v2 E2E cleanup: protected fixture manifest changed during cleanup; preserving files\n' >&2; return 1; }
  if [[ "$after_ids" != "$fixture_ids" ]]; then
    printf 'shop guest import v2 E2E cleanup: protected fixture manifest changed during cleanup; preserving files\n' >&2
    return 1
  fi
  fixture_cleanup_files "$run_dir" "$owner_id" "$viewer_id" "$outsider_id" "$digest" remove \
    || { printf 'shop guest import v2 E2E cleanup: fixture cleanup file proof failed; preserving the protected manifest\n' >&2; return 1; }
}

cleanup_owned_proxy() {
  local run_dir="$OWNED_PROXY_RUN_DIR"
  local run_id="$OWNED_PROXY_RUN_ID"
  local proxy_script="$OWNED_PROXY_SCRIPT"
  local pid="$OWNED_PROXY_PID"
  local uid command_line wait_status=0
  local job_pid found_job=0

  if [[ -z "$run_dir" && -z "$pid" ]]; then
    return 0
  fi
  if [[ ! "$run_dir" =~ ^/private/tmp/shop-guest-import-v2-e2e\.[a-f0-9]{24}$ \
      || ! "$run_id" =~ ^[a-f0-9]{24}$ \
      || "$run_dir" != "/private/tmp/shop-guest-import-v2-e2e.$run_id" \
      || "$proxy_script" != "$run_dir/proxy.py" ]]; then
    printf 'shop guest import v2 E2E cleanup: invalid tracked proxy ownership state; preserving files\n' >&2
    return 1
  fi

  if [[ -n "$pid" ]]; then
    if [[ ! "$pid" =~ ^[0-9]+$ ]]; then
      printf 'shop guest import v2 E2E cleanup: invalid captured proxy PID; preserving files\n' >&2
      return 1
    fi
    if kill -0 "$pid" 2>/dev/null; then
      while IFS= read -r job_pid; do
        [[ "$job_pid" == "$pid" ]] && found_job=1
      done < <(jobs -pr)
      if (( found_job == 0 )); then
        printf 'shop guest import v2 E2E cleanup: proxy PID is not a running child job; preserving files\n' >&2
        return 1
      fi
      uid="$(ps -p "$pid" -o uid= 2>/dev/null)" || {
        printf 'shop guest import v2 E2E cleanup: could not inspect proxy PID owner; preserving files\n' >&2
        return 1
      }
      uid="${uid//[[:space:]]/}"
      if [[ "$uid" != "$(id -u)" ]]; then
        printf 'shop guest import v2 E2E cleanup: proxy PID owner mismatch; preserving files\n' >&2
        return 1
      fi
      command_line="$(ps -ww -p "$pid" -o command= 2>/dev/null)" || {
        printf 'shop guest import v2 E2E cleanup: could not inspect proxy command; preserving files\n' >&2
        return 1
      }
      if ! python3 - "$command_line" "$proxy_script" "$OWNED_PROXY_UPSTREAM_URL" "$run_id" <<'PY'
import shlex
import sys

command, script, upstream, run_id = sys.argv[1:]
try:
    args = shlex.split(command)
except ValueError:
    raise SystemExit(1)
expected = ["--listen", "127.0.0.1", "--port", "0", "--upstream", upstream, "--owner", run_id]
if not any(args[index] == script and args[index + 1:] == expected for index in range(len(args))):
    raise SystemExit(1)
PY
      then
        printf 'shop guest import v2 E2E cleanup: proxy PID command does not match the owned script and run ID; preserving files\n' >&2
        return 1
      fi
      if ! kill "$pid" 2>/dev/null; then
        printf 'shop guest import v2 E2E cleanup: could not stop verified proxy child; preserving files\n' >&2
        return 1
      fi
    fi

    wait "$pid" || wait_status=$?
    if (( wait_status == 127 )); then
      printf 'shop guest import v2 E2E cleanup: captured proxy PID is not a waitable child; preserving files\n' >&2
      return 1
    fi
    OWNED_PROXY_PID=''
  fi

  if ! python3 - "$run_dir" "$OWNED_PROXY_SCRIPT_CREATED" \
      "$OWNED_PROXY_PORT_CREATED" "$OWNED_PROXY_LOG_CREATED" <<'PY'
import os
import stat
import sys

run_dir, *created = sys.argv[1:]
try:
    directory = os.lstat(run_dir)
except OSError:
    raise SystemExit(1)
if (not stat.S_ISDIR(directory.st_mode) or directory.st_uid != os.getuid()
        or stat.S_IMODE(directory.st_mode) != 0o700 or os.path.realpath(run_dir) != run_dir):
    raise SystemExit(1)
for name, was_created in zip(("proxy.py", "proxy.port", "proxy.log"), created):
    if was_created != "1":
        continue
    path = os.path.join(run_dir, name)
    if not os.path.lexists(path):
        continue
    info = os.lstat(path)
    if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid()
            or stat.S_IMODE(info.st_mode) != 0o600 or os.path.realpath(path) != path):
        raise SystemExit(1)
PY
  then
    printf 'shop guest import v2 E2E cleanup: proxy artifacts failed ownership checks; preserving files\n' >&2
    return 1
  fi

  local path flag
  for path in "$proxy_script" "$run_dir/proxy.port" "$run_dir/proxy.log"; do
    case "$path" in
      "$proxy_script") flag="$OWNED_PROXY_SCRIPT_CREATED" ;;
      "$run_dir/proxy.port") flag="$OWNED_PROXY_PORT_CREATED" ;;
      *) flag="$OWNED_PROXY_LOG_CREATED" ;;
    esac
    [[ "$flag" == 1 && ( -e "$path" || -L "$path" ) ]] || continue
    if ! rm -f -- "$path"; then
      printf 'shop guest import v2 E2E cleanup: could not remove owned proxy artifact %s\n' "$path" >&2
      return 1
    fi
  done

  OWNED_PROXY_URL=''
  OWNED_PROXY_RUN_DIR=''
  OWNED_PROXY_RUN_ID=''
  OWNED_PROXY_SCRIPT=''
  OWNED_PROXY_UPSTREAM_URL=''
  OWNED_PROXY_SCRIPT_CREATED=0
  OWNED_PROXY_PORT_CREATED=0
  OWNED_PROXY_LOG_CREATED=0
  return 0
}

read_owned_api_url() {
  local run_dir="$1"
  python3 - "$run_dir/runtime.env" "$run_dir" <<'PY'
import json
import os
import re
import stat
import sys
from urllib.parse import urlsplit

manifest_path, run_dir = sys.argv[1:]
try:
    info = os.stat(manifest_path, follow_symlinks=False)
except OSError:
    raise SystemExit("shop guest import v2 E2E: API runtime manifest is unavailable")
if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid()
        or stat.S_IMODE(info.st_mode) != 0o600 or os.path.realpath(manifest_path) != manifest_path):
    raise SystemExit("shop guest import v2 E2E: API runtime manifest ownership check failed")
try:
    with open(manifest_path, encoding="utf-8") as stream:
        manifest = json.load(stream)
except (OSError, json.JSONDecodeError):
    raise SystemExit("shop guest import v2 E2E: API runtime manifest is invalid")
run_id = run_dir.rsplit(".", 1)[-1]
if (not re.fullmatch(r"[a-f0-9]{24}", run_id)
        or manifest.get("run_id") != run_id or manifest.get("run_dir") != run_dir):
    raise SystemExit("shop guest import v2 E2E: API runtime manifest does not match this run")
api_url = manifest.get("api_url")
try:
    parsed = urlsplit(api_url)
    port = parsed.port
except (TypeError, ValueError):
    raise SystemExit("shop guest import v2 E2E: API manifest loopback URL is invalid")
if (parsed.scheme != "http" or parsed.hostname != "127.0.0.1"
        or parsed.username or parsed.password or port is None
        or not 49152 <= port <= 65535 or parsed.path not in ("", "/")
        or parsed.query or parsed.fragment):
    raise SystemExit("shop guest import v2 E2E: API manifest loopback URL is invalid")
print(api_url)
PY
}

probe_missing_function() {
  local proxy_url="$1"
  local manifest_path="$2"
  python3 - "$proxy_url" "$manifest_path" <<'PY'
import json
import os
import stat
import sys
import urllib.error
import urllib.request
from urllib.parse import urlsplit

proxy_url, manifest_path = sys.argv[1:]
try:
    info = os.stat(manifest_path, follow_symlinks=False)
except OSError:
    raise SystemExit("shop guest import v2 E2E: API runtime manifest is unavailable")
if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid()
        or stat.S_IMODE(info.st_mode) != 0o600):
    raise SystemExit("shop guest import v2 E2E: API runtime manifest ownership check failed")
try:
    with open(manifest_path, encoding="utf-8") as stream:
        manifest = json.load(stream)
except (OSError, json.JSONDecodeError):
    raise SystemExit("shop guest import v2 E2E: API runtime manifest is invalid")
anon_jwt = manifest.get("anon_jwt")
if not isinstance(anon_jwt, str) or not anon_jwt:
    raise SystemExit("shop guest import v2 E2E: local anonymous API key is missing")
try:
    parsed = urlsplit(proxy_url)
    port = parsed.port
except (TypeError, ValueError):
    raise SystemExit("shop guest import v2 E2E: owned proxy URL is invalid")
if (parsed.scheme != "http" or parsed.hostname != "127.0.0.1"
        or parsed.username or parsed.password or port is None
        or not 49152 <= port <= 65535 or parsed.path not in ("", "/")
        or parsed.query or parsed.fragment):
    raise SystemExit("shop guest import v2 E2E: owned proxy URL is invalid")

class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, new_url):
        return None

url = proxy_url.rstrip("/") + "/rest/v1/rpc/task9_missing_probe_function"
request = urllib.request.Request(
    url,
    data=b"{}",
    headers={
        "apikey": anon_jwt,
        "Authorization": "Bearer " + anon_jwt,
        "Content-Type": "application/json",
    },
    method="POST",
)
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect)
status = 0
body = b""
try:
    with opener.open(request, timeout=10) as response:
        status = response.status
        body = response.read()
except urllib.error.HTTPError as response:
    status = response.code
    body = response.read()
except (urllib.error.URLError, TimeoutError, OSError):
    raise SystemExit("shop guest import v2 E2E: missing-function request did not reach the owned proxy")
try:
    payload = json.loads(body)
except (UnicodeDecodeError, json.JSONDecodeError):
    payload = {}
if status != 404 or payload.get("code") != "PGRST202":
    raise SystemExit(
        "shop guest import v2 E2E: local missing-function probe returned an unexpected status/code"
    )
print("shop guest import v2 E2E: local PostgREST missing-function probe passed")
PY
}

require_native_helper() {
  [[ -f "$NATIVE_E2E" && ! -L "$NATIVE_E2E" ]]
}

run_lifecycle_probe() {
  local run_id run_dir api_url
  run_id="$(python3 -c 'import secrets; print(secrets.token_hex(12))')" \
    || fail 'could not create a unique local API run ID'
  [[ "$run_id" =~ ^[a-f0-9]{24}$ ]] || fail 'generated local API run ID is invalid'
  run_dir="$API_RUN_PREFIX$run_id"
  [[ ! -e "$run_dir" && ! -L "$run_dir" ]] \
    || fail 'generated local API run directory already exists'

  OWNED_API_RUN_DIR="$run_dir"
  if ! bash "$API_READINESS" --start "$run_dir"; then
    fail 'reviewed local API startup failed'
  fi
  OWNED_API_STARTED=1

  if ! api_url="$(read_owned_api_url "$run_dir")"; then
    fail 'could not read the owned local API URL'
  fi
  validate_loopback_api_url "$api_url" >/dev/null \
    || fail 'owned local API URL failed loopback validation'

  start_owned_proxy "$run_dir" "$api_url"
  probe_missing_function "$OWNED_PROXY_URL" "$run_dir/runtime.env"
}

run_full_e2e() {
  [[ "$#" -eq 0 ]] || fail 'run_full_e2e does not accept arguments'
  require_native_helper \
    || fail 'the repository native E2E helper is missing or is a symlink'

  local run_id run_dir api_url
  run_id="$(python3 -c 'import secrets; print(secrets.token_hex(12))')" \
    || fail 'could not create a unique local API run ID'
  [[ "$run_id" =~ ^[a-f0-9]{24}$ ]] || fail 'generated local API run ID is invalid'
  run_dir="$API_RUN_PREFIX$run_id"
  [[ ! -e "$run_dir" && ! -L "$run_dir" ]] \
    || fail 'generated local API run directory already exists'

  OWNED_API_RUN_DIR="$run_dir"
  if ! bash "$API_READINESS" --start "$run_dir"; then
    fail 'reviewed local API startup failed'
  fi
  OWNED_API_STARTED=1

  if ! api_url="$(read_owned_api_url "$run_dir")"; then
    fail 'could not read the owned local API URL'
  fi
  validate_loopback_api_url "$api_url" >/dev/null \
    || fail 'owned local API URL failed loopback validation'

  start_owned_proxy "$run_dir" "$api_url" \
    || fail 'could not start the owned local PostgREST proxy'
  prepare_native_run "$run_dir" "$OWNED_PROXY_URL" \
    || fail 'native source and world-ID preflight failed'
  prepare_fixture_manifest "$run_dir" \
    || fail 'could not verify the database baseline and prepare fixture cleanup proof'
  insert_fixture_users "$run_dir" \
    || fail 'synthetic auth fixture insert failed; protected cleanup manifest retained'
  python3 "$NATIVE_E2E" --run "$run_dir" \
    || fail 'repository native E2E helper failed'

  printf 'shop guest import v2 E2E: local API, native import, and explicit world visibility passed\n'
}

cleanup_task9_e2e() {
  local original_status=$?
  local cleanup_failed=0
  local api_quiesced=0
  trap - EXIT

  cleanup_owned_proxy || cleanup_failed=1
  if [[ "$OWNED_API_STARTED" == 1 ]]; then
    if [[ -z "$OWNED_API_RUN_DIR" ]] \
        || ! bash "$API_READINESS" --quiesce "$OWNED_API_RUN_DIR"; then
      printf 'shop guest import v2 E2E cleanup: owned API quiesce failed; preserving fixture proof\n' >&2
      cleanup_failed=1
    else
      api_quiesced=1
    fi
  else
    api_quiesced=1
  fi

  if [[ -n "$OWNED_API_RUN_DIR" \
      && ( -e "$OWNED_API_RUN_DIR" || -L "$OWNED_API_RUN_DIR" ) ]]; then
    if (( api_quiesced == 0 )); then
      printf 'shop guest import v2 E2E cleanup: fixture cleanup skipped because API quiesce was not verified; protected files retained\n' >&2
    elif ! cleanup_fixture_users "$OWNED_API_RUN_DIR"; then
      printf 'shop guest import v2 E2E cleanup: fixture cleanup failed; protected files retained\n' >&2
      cleanup_failed=1
    fi
  fi
  if [[ "$OWNED_API_STARTED" == 1 ]]; then
    if [[ -z "$OWNED_API_RUN_DIR" ]] \
        || ! bash "$API_READINESS" --stop "$OWNED_API_RUN_DIR"; then
      printf 'shop guest import v2 E2E cleanup: owned API stop failed\n' >&2
      cleanup_failed=1
    fi
  fi

  if (( cleanup_failed != 0 )); then
    exit 1
  fi
  exit "$original_status"
}

trap cleanup_task9_e2e EXIT

main() {
  if [[ "$#" -gt 1 ]]; then
    fail 'accepts exactly one supported mode'
  fi
  [[ -f "$API_READINESS" ]] || fail 'the API readiness script is missing'

  case "${1:-}" in
    --check-only)
      bash "$API_READINESS" --check-only
      ;;
    --lifecycle-only)
      run_lifecycle_probe
      printf 'shop guest import v2 E2E: API lifecycle and missing-function probe passed\n'
      ;;
    --run)
      run_full_e2e
      ;;
    --help)
      printf 'Usage: bash %s [--check-only | --lifecycle-only | --run]\n' "${BASH_SOURCE[0]}"
      printf 'Default execution remains disabled; --run enables owned local fixture setup, native E2E, and cleanup.\n'
      ;;
    '')
      fail 'default execution is disabled; choose --check-only, --lifecycle-only, or --run'
      ;;
    *)
      fail 'supported modes are --check-only, --lifecycle-only, and --run'
      ;;
  esac
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  main "$@"
fi
