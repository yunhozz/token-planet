"""Local-only native Task 9 runner and its pure contract tests."""

import base64
import contextlib
import hashlib
import hmac
import io
import json
import os
import re
import shutil
import stat
import subprocess
import time
import unittest
import uuid
from pathlib import Path
from unittest import mock
import urllib.request
import urllib.error
from urllib.parse import urlsplit


BASELINE_DIGEST = "ce9ef4b37d29422d9d8cd8cb3c9d9bba"
EXPECTED_CARGO_ARGV = [
    "/Users/yunho/.cargo/bin/cargo",
    "test",
    "--offline",
    "--manifest-path",
    "/Users/yunho/Desktop/project/token-planet/apps/desktop/src-tauri/Cargo.toml",
    "--lib",
    "task9_local_api_import_worker_cache_and_explicit_world_visibility",
    "--",
    "--ignored",
    "--nocapture",
    "--test-threads=1",
]
RUN_PREFIX = "/private/tmp/shop-guest-import-v2-e2e."
REPO_ROOT = "/Users/yunho/Desktop/project/token-planet"
APPROVED_HOME = "/Users/yunho"
PROXY_VARIABLES = {
    "HTTP_PROXY",
    "http_proxy",
    "HTTPS_PROXY",
    "https_proxy",
    "ALL_PROXY",
    "all_proxy",
}
NATIVE_ENV_KEYS = (
    "TASK9_API_URL",
    "TASK9_ANON_KEY",
    "TASK9_USER_ID",
    "TASK9_USER_ACCESS_TOKEN",
    "TASK9_VIEWER_USER_ID",
    "TASK9_VIEWER_ACCESS_TOKEN",
    "TASK9_SOURCE_ROOT",
    "TASK9_WORLD_ID_FILE",
)
FORBIDDEN_PUBLIC_KEYS = frozenset(
    {
        "wallet_balance",
        "wallet_credits",
        "available_balance",
        "removal_debits",
        "removal_proofs",
        "purchase_proofs",
        "purchase_id",
        "purchases",
        "reward_timezone",
        "era_progress",
        "game_rewards",
        "last_reset_at_utc",
        "reset_available_at_utc",
        "reset_receipt",
        "reset_settlement_proofs",
        "import_id",
        "request_id",
        "expected_cycle_id",
        "source_fingerprint",
        "source_account_id",
        "source_path",
        "prefix_fingerprint",
        "lineage_id",
        "occurrence_id",
        "occurrences",
        "planet_device_id",
        "device_id",
        "canonical_version",
        "canonical_payload",
        "canonical_contribution",
        "ack",
        "journal_confirmation",
        "effect_revision",
        "cycle_id",
        "current_cycle_id",
        "old_cycle_id",
        "previous_cycle_id",
        "new_cycle_id",
        "next_cycle_id",
        "effect_history",
        "effect_timeline",
        "effect_contributions",
        "provenance",
        "agent",
        "reward_date",
        "raw_tokens",
        "prompt",
        "log",
    }
)
DIGEST_SQL = """create function pg_temp.shop_guest_v2_fixture_data_digest() returns text
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
select pg_temp.shop_guest_v2_fixture_data_digest();"""


class NativeE2EError(Exception):
    """A sanitized, user-safe local fixture error."""


def _reject_if(condition, message):
    if condition:
        raise NativeE2EError(message)


def _owned_info(path, uid, mode, description, *, directory=False):
    try:
        info = os.lstat(path)
    except OSError:
        raise NativeE2EError(f"owned {description} is missing") from None
    expected_type = stat.S_ISDIR(info.st_mode) if directory else stat.S_ISREG(info.st_mode)
    _reject_if(
        not expected_type
        or stat.S_ISLNK(info.st_mode)
        or info.st_uid != uid
        or stat.S_IMODE(info.st_mode) != mode,
        f"owned {description} failed type, UID, or mode validation",
    )
    return info


def _read_owned_json(path, uid, description):
    _owned_info(path, uid, 0o600, description)
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    try:
        fd = os.open(path, flags)
    except OSError:
        raise NativeE2EError(f"owned {description} could not be opened safely") from None
    with os.fdopen(fd, "r", encoding="utf-8") as stream:
        info = os.fstat(stream.fileno())
        _reject_if(
            not stat.S_ISREG(info.st_mode)
            or info.st_uid != uid
            or stat.S_IMODE(info.st_mode) != 0o600,
            f"owned {description} changed during validation",
        )
        try:
            value = json.load(stream)
        except (OSError, json.JSONDecodeError):
            raise NativeE2EError(f"owned {description} is invalid JSON") from None
    _reject_if(not isinstance(value, dict), f"owned {description} must be a JSON object")
    return value


def _canonical_loopback_url(value):
    if not isinstance(value, str):
        raise NativeE2EError("local API origin is missing")
    try:
        parsed = urlsplit(value)
        port = parsed.port
    except ValueError:
        raise NativeE2EError("local API origin is invalid") from None
    if (
        parsed.scheme != "http"
        or parsed.hostname != "127.0.0.1"
        or parsed.username
        or parsed.password
        or port is None
        or not 49152 <= port <= 65535
        or parsed.path not in ("", "/")
        or parsed.query
        or parsed.fragment
        or value != f"http://127.0.0.1:{port}"
    ):
        raise NativeE2EError("local API origin must be a canonical loopback HTTP origin")
    return port


def _read_owned_world_id(path, uid):
    _owned_info(path, uid, 0o600, "world-id file")
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    try:
        fd = os.open(path, flags)
    except OSError:
        raise NativeE2EError("owned world-id file could not be opened safely") from None
    with os.fdopen(fd, "rb") as stream:
        info = os.fstat(stream.fileno())
        _reject_if(
            not stat.S_ISREG(info.st_mode)
            or info.st_uid != uid
            or stat.S_IMODE(info.st_mode) != 0o600,
            "owned world-id file changed during validation",
        )
        contents = stream.read(37)
        _reject_if(stream.read(1) != b"", "owned world-id file is too large")
    try:
        value = contents.decode("ascii")
        parsed = uuid.UUID(value)
    except (UnicodeDecodeError, ValueError, AttributeError):
        raise NativeE2EError("owned world-id file does not contain a canonical UUID") from None
    _reject_if(str(parsed) != value, "owned world-id file does not contain a canonical UUID")
    return value


def _validate_source_roots(run_dir, uid, *, post_native=False):
    source_root = run_dir / "sources"
    codex_root = source_root / "emptycodex"
    claude_root = source_root / "emptyclaude"
    for path, description in (
        (source_root, "source root"),
        (codex_root, "Codex collector root"),
        (claude_root, "Claude collector root"),
    ):
        _owned_info(path, uid, 0o700, description, directory=True)
    _reject_if(
        sorted(path.name for path in source_root.iterdir()) != ["emptyclaude", "emptycodex"],
        "owned source root has unexpected entries",
    )
    _reject_if(any(codex_root.iterdir()) or any(claude_root.iterdir()), "collector roots must be empty")

    world_id_path = run_dir / "world-id.txt"
    if post_native:
        world_id = _read_owned_world_id(world_id_path, uid)
    else:
        info = _owned_info(world_id_path, uid, 0o600, "world-id file")
        _reject_if(info.st_size != 0, "world-id file must be empty before the native probe")
        world_id = None
    return source_root, world_id_path, world_id


def _read_proxy_port(run_dir, uid):
    path = run_dir / "proxy.port"
    _owned_info(path, uid, 0o600, "proxy port file")
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    try:
        fd = os.open(path, flags)
    except OSError:
        raise NativeE2EError("owned proxy port file could not be opened safely") from None
    with os.fdopen(fd, "rb") as stream:
        info = os.fstat(stream.fileno())
        _reject_if(
            not stat.S_ISREG(info.st_mode)
            or info.st_uid != uid
            or stat.S_IMODE(info.st_mode) != 0o600,
            "owned proxy port file changed during validation",
        )
        contents = stream.read(7)
        _reject_if(stream.read(1) != b"", "owned proxy port file is too large")
    digits = contents[:-1] if contents.endswith(b"\n") else contents
    _reject_if(
        len(digits) != 5 or not digits.isdigit(),
        "owned proxy port file must contain one canonical decimal port",
    )
    try:
        port = int(digits.decode("ascii"))
    except (UnicodeDecodeError, ValueError):
        raise NativeE2EError("owned proxy port file is invalid") from None
    _reject_if(
        not 49152 <= port <= 65535 or str(port).encode("ascii") != digits,
        "owned proxy port is outside the approved range",
    )
    return port


def _decode_jwt(token):
    if not isinstance(token, str):
        raise NativeE2EError("anonymous API key is missing")
    parts = token.split(".")
    if len(parts) != 3:
        raise NativeE2EError("anonymous API key is malformed")
    try:
        header_padding = "=" * (-len(parts[0]) % 4)
        payload_padding = "=" * (-len(parts[1]) % 4)
        header = json.loads(base64.urlsafe_b64decode(parts[0] + header_padding))
        payload = json.loads(base64.urlsafe_b64decode(parts[1] + payload_padding))
    except (ValueError, json.JSONDecodeError):
        raise NativeE2EError("anonymous API key is malformed") from None
    _reject_if(not isinstance(header, dict) or not isinstance(payload, dict), "anonymous API key payload is invalid")
    return header, payload, parts[2], f"{parts[0]}.{parts[1]}"


def load_run_context(run_path, *, post_native=False):
    raw_run_path = os.fspath(run_path)
    run_dir = Path(raw_run_path)
    run_id = raw_run_path[len(RUN_PREFIX) :] if raw_run_path.startswith(RUN_PREFIX) else ""
    _reject_if(
        len(run_id) != 24 or any(ch not in "0123456789abcdef" for ch in run_id),
        "run directory must use the owned Task 9 path",
    )
    _reject_if(not run_dir.is_absolute() or str(run_dir) != raw_run_path, "run path is not canonical")
    for path in (Path("/private"), Path("/private/tmp"), run_dir):
        try:
            info = os.lstat(path)
        except OSError:
            raise NativeE2EError("owned run directory path is unavailable") from None
        _reject_if(stat.S_ISLNK(info.st_mode), "owned run directory path contains a symlink")
    _reject_if(os.path.realpath(run_dir) != raw_run_path, "run path does not resolve canonically")
    uid = os.getuid()
    _owned_info(run_dir, uid, 0o700, "run directory", directory=True)

    fixture_path = run_dir / "fixture-manifest.json"
    fixture = _read_owned_json(fixture_path, uid, "fixture manifest")
    _reject_if(fixture.get("version") != 1, "fixture manifest version is unsupported")
    _reject_if(
        fixture.get("run_id") != run_id or fixture.get("run_dir") != raw_run_path,
        "fixture manifest does not belong to this run",
    )
    _reject_if(
        fixture.get("initial_auth_user_absence_proven") is not True
        or fixture.get("cleanup_eligible") is not True
        or fixture.get("initial_database_digest") != BASELINE_DIGEST,
        "fixture manifest lacks the approved initial cleanup proofs",
    )
    fixture_ids = fixture.get("fixture_user_ids")
    _reject_if(
        not isinstance(fixture_ids, dict) or set(fixture_ids) != {"owner", "viewer", "outsider"},
        "fixture manifest must identify owner, viewer, and outsider",
    )
    normalized_ids = {}
    for name in ("owner", "viewer", "outsider"):
        value = fixture_ids[name]
        try:
            parsed = uuid.UUID(value)
        except (TypeError, ValueError, AttributeError):
            raise NativeE2EError("fixture manifest contains an invalid user ID") from None
        _reject_if(str(parsed) != value or parsed.version != 4, "fixture user IDs must be canonical UUIDv4")
        normalized_ids[name] = value
    _reject_if(len(set(normalized_ids.values())) != 3, "fixture user IDs must be distinct")

    proxy_port = _read_proxy_port(run_dir, uid)
    source_root, world_id_path, primary_world_id = _validate_source_roots(
        run_dir, uid, post_native=post_native
    )

    runtime_path = run_dir / "runtime.env"
    runtime = _read_owned_json(runtime_path, uid, "API runtime manifest")
    _reject_if(
        runtime.get("run_id") != run_id or runtime.get("run_dir") != raw_run_path,
        "API runtime manifest does not belong to this run",
    )
    api_port = _canonical_loopback_url(runtime.get("api_url"))
    if "api_port" in runtime:
        _reject_if(runtime.get("api_port") != api_port, "API runtime port does not match its URL")

    # Credential fields are read only after the run, fixture, source, world-id,
    # proxy port, and direct API origin have all passed their ownership checks.
    jwt_secret = runtime.get("jwt_secret")
    _reject_if(not isinstance(jwt_secret, str) or len(jwt_secret) < 32, "runtime JWT secret is invalid")
    anon_jwt = runtime.get("anon_jwt")
    _reject_if(not isinstance(anon_jwt, str) or not anon_jwt, "anonymous API key is missing")
    header, claims, signature, unsigned = _decode_jwt(anon_jwt)
    _reject_if(header.get("alg") != "HS256", "anonymous API key must use HS256")
    expected_signature = base64.urlsafe_b64encode(
        hmac.new(jwt_secret.encode("utf-8"), unsigned.encode("ascii"), hashlib.sha256).digest()
    ).rstrip(b"=").decode("ascii")
    _reject_if(
        not hmac.compare_digest(signature, expected_signature),
        "anonymous API key signature does not match the local runtime secret",
    )
    _reject_if(
        claims.get("role") != "anon" or claims.get("iss") != "supabase",
        "runtime key is not a Supabase anonymous JWT",
    )
    _reject_if(
        not isinstance(claims.get("exp"), int) or claims["exp"] <= int(time.time()),
        "anonymous API key is expired or has no numeric expiry",
    )

    return {
        "run_dir": run_dir,
        "run_id": run_id,
        "uid": uid,
        "fixture_ids": normalized_ids,
        "initial_database_digest": fixture["initial_database_digest"],
        "api_url": runtime["api_url"],
        "proxy_url": f"http://127.0.0.1:{proxy_port}",
        "anon_jwt": anon_jwt,
        "jwt_secret": jwt_secret,
        "source_root": source_root,
        "world_id_path": world_id_path,
        "primary_world_id": primary_world_id,
    }


def _sign_jwt(payload, secret):
    def encode(value):
        return base64.urlsafe_b64encode(value).rstrip(b"=").decode("ascii")

    header = encode(b'{"alg":"HS256","typ":"JWT"}')
    body = encode(json.dumps(payload, separators=(",", ":")).encode("utf-8"))
    unsigned = f"{header}.{body}"
    signature = encode(hmac.new(secret.encode("utf-8"), unsigned.encode("ascii"), hashlib.sha256).digest())
    return f"{unsigned}.{signature}"


def build_native_environment(context, now):
    common_claims = {"role": "authenticated", "iss": "supabase", "iat": int(now), "exp": int(now) + 3600}
    owner_token = _sign_jwt({**common_claims, "sub": context["fixture_ids"]["owner"]}, context["jwt_secret"])
    viewer_token = _sign_jwt({**common_claims, "sub": context["fixture_ids"]["viewer"]}, context["jwt_secret"])
    return {
        "TASK9_API_URL": context["proxy_url"],
        "TASK9_ANON_KEY": context["anon_jwt"],
        "TASK9_USER_ID": context["fixture_ids"]["owner"],
        "TASK9_USER_ACCESS_TOKEN": owner_token,
        "TASK9_VIEWER_USER_ID": context["fixture_ids"]["viewer"],
        "TASK9_VIEWER_ACCESS_TOKEN": viewer_token,
        "TASK9_SOURCE_ROOT": str(context["source_root"]),
        "TASK9_WORLD_ID_FILE": str(context["world_id_path"]),
    }


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, new_url):
        return None


def build_no_proxy_opener():
    return urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())


def _request_json(context, method, path, *, access_token=None, payload=None, prefer=None):
    allowed_paths = {
        ("POST", "/rest/v1/worlds"),
        ("GET", "/rest/v1/worlds?select=id,name,timezone,owner_id"),
        ("POST", "/rest/v1/rpc/get_world_planets"),
    }
    _reject_if((method, path) not in allowed_paths, "public probe request is outside the approved API surface")
    proxy_port = _canonical_loopback_url(context.get("proxy_url"))
    _reject_if(
        context.get("proxy_url") != f"http://127.0.0.1:{proxy_port}",
        "owned API proxy URL is not canonical",
    )
    headers = {
        "Accept": "application/json",
        "apikey": context["anon_jwt"],
        "Authorization": f"Bearer {access_token or context['anon_jwt']}",
    }
    data = None
    if payload is not None:
        headers["Content-Type"] = "application/json"
        data = json.dumps(payload, separators=(",", ":")).encode("utf-8")
    if prefer is not None:
        headers["Prefer"] = prefer
    request = urllib.request.Request(
        context["proxy_url"] + path,
        data=data,
        headers=headers,
        method=method,
    )
    response = None
    try:
        response = build_no_proxy_opener().open(request, timeout=20)
    except urllib.error.HTTPError as error:
        response = error
    except (urllib.error.URLError, TimeoutError, OSError):
        raise NativeE2EError("owned local API proxy request failed") from None
    try:
        status = response.code if isinstance(response, urllib.error.HTTPError) else response.status
        _reject_if(300 <= status < 400, "owned local API proxy returned a redirect")
        raw_body = response.read(1_048_577)
        _reject_if(len(raw_body) > 1_048_576, "owned local API response exceeds the size limit")
    finally:
        response.close()
    if not raw_body:
        return status, None
    try:
        decoded = json.loads(raw_body.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError):
        raise NativeE2EError("owned local API response is not valid JSON") from None
    return status, decoded


def assert_scene_has_no_private_keys(value):
    if isinstance(value, dict):
        for key, child in value.items():
            if key in FORBIDDEN_PUBLIC_KEYS:
                raise NativeE2EError("public scene contains a forbidden private field")
            assert_scene_has_no_private_keys(child)
    elif isinstance(value, list):
        for child in value:
            assert_scene_has_no_private_keys(child)


def database_digest():
    argv = [
        "docker",
        "--context",
        "desktop-linux",
        "exec",
        "supabase_db_token-planet-shop-revamp-test",
        "psql",
        "-X",
        "-q",
        "-A",
        "-t",
        "-v",
        "ON_ERROR_STOP=1",
        "-U",
        "postgres",
        "-d",
        "postgres",
        "-c",
        DIGEST_SQL,
    ]
    docker_env = {"HOME": APPROVED_HOME}
    docker_env.update(
        {
            key: os.environ[key]
            for key in ("PATH", "TMPDIR", "LANG", "LC_ALL", "LC_CTYPE")
            if key in os.environ
        }
    )
    docker_env["NO_PROXY"] = "*"
    docker_env["no_proxy"] = "*"
    try:
        completed = subprocess.run(
            argv,
            env=docker_env,
            capture_output=True,
            text=True,
            check=False,
            timeout=30,
        )
    except (OSError, subprocess.TimeoutExpired):
        raise NativeE2EError("local fixture database fingerprint could not be read") from None
    _reject_if(completed.returncode != 0, "local fixture database fingerprint failed")
    digest = completed.stdout.strip()
    _reject_if(not re.fullmatch(r"[0-9a-f]{32}", digest), "local fixture database fingerprint is invalid")
    return digest


def run_public_boundary_checks(context, native_environment, *, request_fn=None, digest_fn=None, now=None):
    if request_fn is None:
        request_fn = _request_json
    if digest_fn is None:
        digest_fn = database_digest
    if now is None:
        now = int(time.time())
    primary_world_id = _read_owned_world_id(context["world_id_path"], context["uid"])
    _reject_if(
        native_environment.get("TASK9_USER_ID") != context["fixture_ids"]["owner"]
        or native_environment.get("TASK9_VIEWER_USER_ID") != context["fixture_ids"]["viewer"],
        "native fixture identity does not match its owned manifest",
    )

    outsider_id = context["fixture_ids"]["outsider"]
    outsider_token = _sign_jwt(
        {
            "role": "authenticated",
            "iss": "supabase",
            "iat": int(now),
            "exp": int(now) + 3600,
            "sub": outsider_id,
        },
        context["jwt_secret"],
    )
    status, create_response = request_fn(
        context,
        "POST",
        "/rest/v1/worlds",
        access_token=outsider_token,
        payload={"owner_id": outsider_id, "name": "Task 9 outsider visibility probe", "timezone": "UTC"},
        prefer="return=minimal",
    )
    _reject_if(
        status != 201 or create_response not in (None, []),
        "outsider-owned visibility world could not be created",
    )

    worlds_path = "/rest/v1/worlds?select=id,name,timezone,owner_id"
    status, visible_worlds = request_fn(
        context, "GET", worlds_path, access_token=outsider_token
    )
    _reject_if(status != 200 or not isinstance(visible_worlds, list), "outsider world visibility read failed")
    matching_worlds = [
        row
        for row in visible_worlds
        if isinstance(row, dict)
        and row.get("owner_id") == outsider_id
        and row.get("name") == "Task 9 outsider visibility probe"
        and row.get("timezone") == "UTC"
    ]
    _reject_if(
        len(matching_worlds) != 1 or set(matching_worlds[0]) != {"id", "name", "timezone", "owner_id"},
        "outsider world visibility response is invalid",
    )
    outsider_world_id = matching_worlds[0].get("id")
    try:
        canonical_outsider_world_id = str(uuid.UUID(outsider_world_id))
    except (TypeError, ValueError, AttributeError):
        raise NativeE2EError("outsider world response has an invalid ID") from None
    _reject_if(
        canonical_outsider_world_id != outsider_world_id or outsider_world_id == primary_world_id,
        "outsider world response does not match the owned fixture identity",
    )

    rpc_path = "/rest/v1/rpc/get_world_planets"
    denial_requests = (
        (None, primary_world_id),
        (outsider_token, primary_world_id),
        (native_environment["TASK9_VIEWER_ACCESS_TOKEN"], outsider_world_id),
    )
    before_digest = digest_fn()
    _reject_if(not re.fullmatch(r"[0-9a-f]{32}", before_digest), "database fingerprint before reads is invalid")
    for access_token, world_id in denial_requests:
        status, _response = request_fn(
            context,
            "POST",
            rpc_path,
            access_token=access_token,
            payload={"p_world_id": world_id},
        )
        _reject_if(not 400 <= status < 500, "an unauthorized public-scene read was not denied")

    for _ in range(3):
        status, scene = request_fn(
            context,
            "POST",
            rpc_path,
            access_token=native_environment["TASK9_VIEWER_ACCESS_TOKEN"],
            payload={"p_world_id": primary_world_id},
        )
        _reject_if(status != 200 or not isinstance(scene, list) or not scene, "viewer public-scene read failed")
        assert_scene_has_no_private_keys(scene)
    after_digest = digest_fn()
    _reject_if(not re.fullmatch(r"[0-9a-f]{32}", after_digest), "database fingerprint after reads is invalid")
    digest_unchanged = hmac.compare_digest(before_digest, after_digest)
    _reject_if(not digest_unchanged, "public-scene reads changed the fixture database fingerprint")
    return {"denials": len(denial_requests), "viewer_reads": 3, "digest_unchanged": digest_unchanged}


def build_child_environment(base_environment, native_environment, uid):
    _reject_if(uid != 501, "native Task 9 runner must execute as UID 501")
    _reject_if(base_environment.get("HOME") != APPROVED_HOME, "native Task 9 runner requires the approved HOME")
    path_value = base_environment.get("PATH", "")
    _reject_if(not path_value or "/usr/sbin" not in path_value.split(":"), "native Task 9 PATH must include /usr/sbin")
    child = {
        name: base_environment[name]
        for name in (
            "HOME",
            "PATH",
            "TMPDIR",
            "LANG",
            "LC_ALL",
            "LC_CTYPE",
        )
        if name in base_environment
    }
    child["CARGO_INCREMENTAL"] = "0"
    child["NO_PROXY"] = "*"
    child["no_proxy"] = "*"
    _reject_if(set(native_environment) != set(NATIVE_ENV_KEYS), "native Task 9 environment keys are incomplete")
    child.update(native_environment)
    _reject_if(any(name in child for name in PROXY_VARIABLES), "proxy variables must not reach native Cargo")
    return child


def _create_owned_output(path, contents=None):
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0)
    try:
        fd = os.open(path, flags, 0o600)
    except OSError:
        raise NativeE2EError(f"refusing to replace an existing owned output: {path.name}") from None
    os.fchmod(fd, 0o600)
    if contents is None:
        return os.fdopen(fd, "wb")
    with os.fdopen(fd, "wb") as stream:
        stream.write(contents)
        stream.flush()
        os.fsync(stream.fileno())
    return None


def native_cargo_argv():
    return list(EXPECTED_CARGO_ARGV)


def summarize_native_failure_log(log_path, secret_values=()):
    uid = os.getuid()
    _owned_info(log_path, uid, 0o600, "native log")
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    try:
        fd = os.open(log_path, flags)
    except OSError:
        raise NativeE2EError("owned native log could not be opened safely") from None
    with os.fdopen(fd, "rb") as stream:
        info = os.fstat(stream.fileno())
        _reject_if(
            not stat.S_ISREG(info.st_mode)
            or info.st_uid != uid
            or stat.S_IMODE(info.st_mode) != 0o600,
            "owned native log changed during validation",
        )
        stream.seek(max(0, info.st_size - 16_384))
        raw_tail = stream.read(16_384)
    text = raw_tail.decode("utf-8", errors="replace")
    text = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", text)
    jwt_pattern = re.compile(
        r"(?<![A-Za-z0-9_-])eyJ[A-Za-z0-9_-]*\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+(?![A-Za-z0-9_-])"
    )
    environment_assignment = re.compile(
        r"(?:TASK9_[A-Z0-9_]+|JWT_SECRET|ANON_JWT|SUPABASE_(?:ANON|SERVICE_ROLE)_KEY|[A-Za-z_]*ACCESS_TOKEN)\s*(?:=|:)",
        re.IGNORECASE,
    )
    panic_location = re.compile(r"panicked at .+:\d+:\d+:", re.IGNORECASE)
    safe_diagnostic = re.compile(
        r"(?i)(?:panicked at .+:\d+:\d+:|^test result:|^failures:|^---- .+ ----$|"
        r"^error(?::|\[)|^running \d+ tests?$|^test .+ \.\.\. FAILED$|^note: run with)"
    )
    secrets = tuple(sorted({value for value in secret_values if isinstance(value, str) and value}, key=len, reverse=True))
    raw_lines = text.splitlines()
    selected_indexes = {
        index for index, line in enumerate(raw_lines) if safe_diagnostic.search(line.strip())
    }
    for index, line in enumerate(raw_lines):
        if panic_location.search(line):
            selected_indexes.update(range(index, min(len(raw_lines), index + 13)))
    lines = []
    for index in sorted(selected_indexes):
        line = raw_lines[index]
        if environment_assignment.search(line):
            continue
        line = jwt_pattern.sub("[REDACTED_JWT]", line)
        for secret in secrets:
            line = line.replace(secret, "[REDACTED]")
        line = "".join(character for character in line if character == "\t" or ord(character) >= 32)
        line = line.strip()
        if line:
            lines.append(line[:400])
    summary = "\n".join(lines[-12:])
    return summary[:2400] if summary else "no safe native test diagnostics were available"


def run_native_probe(run_path, *, base_environment=None, now=None):
    uid = os.getuid()
    _reject_if(uid != 501, "native Task 9 runner must execute as UID 501")
    context = load_run_context(run_path)
    if now is None:
        import time

        now = int(time.time())
    native_environment = build_native_environment(context, now)
    child_environment = build_child_environment(
        dict(os.environ) if base_environment is None else dict(base_environment),
        native_environment,
        uid,
    )
    native_env_path = context["run_dir"] / "native-env.json"
    native_log_path = context["run_dir"] / "native.log"
    encoded_environment = (json.dumps(native_environment, separators=(",", ":")) + "\n").encode("utf-8")
    _create_owned_output(native_env_path, encoded_environment)
    log_stream = _create_owned_output(native_log_path)
    try:
        completed = subprocess.run(
            native_cargo_argv(),
            cwd=REPO_ROOT,
            env=child_environment,
            stdout=log_stream,
            stderr=subprocess.STDOUT,
            check=False,
        )
    except OSError as error:
        log_stream.flush()
        os.fsync(log_stream.fileno())
        log_stream.close()
        safe_tail = summarize_native_failure_log(
            native_log_path,
            secret_values=(context["jwt_secret"], *native_environment.values()),
        )
        print(f"native Cargo probe could not start ({type(error).__name__}); sanitized diagnostics follow")
        print(safe_tail)
        return 1
    else:
        log_stream.flush()
        os.fsync(log_stream.fileno())
        log_stream.close()
    if completed.returncode == 0:
        print(
            "native Cargo test passed: "
            "task9_local_api_import_worker_cache_and_explicit_world_visibility (1 passed)"
        )
        return 0
    safe_tail = summarize_native_failure_log(
        native_log_path,
        secret_values=(context["jwt_secret"], *native_environment.values()),
    )
    print(f"native Cargo probe failed (exit {completed.returncode}); sanitized diagnostics follow")
    print(safe_tail)
    return completed.returncode if completed.returncode > 0 else 1


def run_full_probe(run_path, *, base_environment=None, now=None):
    if now is None:
        now = int(time.time())
    native_status = run_native_probe(
        run_path,
        base_environment=base_environment,
        now=now,
    )
    if native_status != 0:
        return native_status
    context = load_run_context(run_path, post_native=True)
    native_environment = build_native_environment(context, now)
    result = run_public_boundary_checks(context, native_environment, now=now)
    print(
        "Task 9 public boundary passed: "
        f"denials={result['denials']}; viewer_reads={result['viewer_reads']}; "
        f"database_digest_unchanged={str(result['digest_unchanged']).lower()}"
    )
    return 0


def main(argv=None):
    import argparse

    parser = argparse.ArgumentParser(description="Run the owned local Task 9 native probe")
    parser.add_argument("--run", required=True, metavar="RUN_DIR")
    args = parser.parse_args(argv)
    try:
        return run_full_probe(args.run)
    except NativeE2EError as error:
        print(f"native Task 9 run failed: {error}", file=__import__("sys").stderr)
        return 1


class NativeRunnerContractTests(unittest.TestCase):
    def setUp(self):
        run_id = uuid.uuid4().hex[:24]
        self.run_dir = Path("/private/tmp") / f"shop-guest-import-v2-e2e.{run_id}"
        self.run_dir.mkdir(mode=0o700)
        os.chmod(self.run_dir, 0o700)
        self.owner_id = str(uuid.uuid4())
        self.viewer_id = str(uuid.uuid4())
        self.outsider_id = str(uuid.uuid4())

        self._write_private(
            self.run_dir / "proxy.port",
            b"54480\n",
        )
        source_root = self.run_dir / "sources"
        source_root.mkdir(mode=0o700)
        for name in ("emptycodex", "emptyclaude"):
            (source_root / name).mkdir(mode=0o700)
        self._write_private(self.run_dir / "world-id.txt", b"")

        manifest = {
            "version": 1,
            "run_id": run_id,
            "run_dir": str(self.run_dir),
            "fixture_user_ids": {
                "owner": self.owner_id,
                "viewer": self.viewer_id,
                "outsider": self.outsider_id,
            },
            "initial_auth_user_absence_proven": True,
            "initial_database_digest": BASELINE_DIGEST,
            "cleanup_eligible": True,
            "fixture_rows_created": False,
        }
        self._write_private(
            self.run_dir / "fixture-manifest.json",
            (json.dumps(manifest, separators=(",", ":")) + "\n").encode(),
        )

        self.jwt_secret = "unit-test-only-secret-0123456789abcdef"
        self.anon_jwt = self._signed_test_jwt(
            {"role": "anon", "iss": "supabase", "iat": 1_700_000_000, "exp": 1_800_000_000},
            self.jwt_secret,
        )
        runtime = {
            "version": 1,
            "run_id": run_id,
            "run_dir": str(self.run_dir),
            "api_url": "http://127.0.0.1:50259",
            "anon_jwt": self.anon_jwt,
            "jwt_secret": self.jwt_secret,
        }
        self._write_private(
            self.run_dir / "runtime.env",
            (json.dumps(runtime, separators=(",", ":")) + "\n").encode(),
        )

    def tearDown(self):
        shutil.rmtree(self.run_dir)

    @staticmethod
    def _write_private(path, contents):
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, "wb") as stream:
            stream.write(contents)
        os.chmod(path, 0o600)

    @staticmethod
    def _signed_test_jwt(payload, secret):
        encode = lambda value: base64.urlsafe_b64encode(value).rstrip(b"=").decode()
        header = encode(b'{"alg":"HS256","typ":"JWT"}')
        body = encode(json.dumps(payload, separators=(",", ":")).encode())
        unsigned = f"{header}.{body}"
        signature = encode(hmac.new(secret.encode(), unsigned.encode(), hashlib.sha256).digest())
        return f"{unsigned}.{signature}"

    @staticmethod
    def _jwt_payload(token):
        segment = token.split(".")[1]
        padding = "=" * (-len(segment) % 4)
        return json.loads(base64.urlsafe_b64decode(segment + padding))

    @staticmethod
    def _jwt_signature_valid(token, secret):
        header, payload, signature = token.split(".")
        unsigned = f"{header}.{payload}"
        expected = base64.urlsafe_b64encode(
            hmac.new(secret.encode(), unsigned.encode(), hashlib.sha256).digest()
        ).rstrip(b"=").decode()
        return hmac.compare_digest(signature, expected)

    def test_run_uses_owned_manifest_env_and_exact_offline_cargo_argv(self):
        base_environment = {
            "HOME": "/Users/yunho",
            "PATH": "/Users/yunho/.cargo/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin",
            "TASK9_USER_ACCESS_TOKEN": "inherited-secret-must-be-replaced",
            "HTTP_PROXY": "http://untrusted-proxy.invalid:8080",
            "https_proxy": "http://untrusted-proxy.invalid:8080",
            "NO_PROXY": "localhost",
            "no_proxy": "localhost",
        }
        completed = type("Completed", (), {"returncode": 0})()
        captured_stdout = io.StringIO()
        captured_stderr = io.StringIO()

        def mock_cargo(argv, **kwargs):
            self.assertEqual(argv, EXPECTED_CARGO_ARGV)
            output_info = os.fstat(kwargs["stdout"].fileno())
            log_info = os.stat(self.run_dir / "native.log", follow_symlinks=False)
            self.assertEqual(output_info.st_ino, log_info.st_ino)
            return completed

        with mock.patch("subprocess.run", side_effect=mock_cargo) as cargo_run:
            with contextlib.redirect_stdout(captured_stdout), contextlib.redirect_stderr(
                captured_stderr
            ):
                status = run_native_probe(
                    str(self.run_dir),
                    base_environment=base_environment,
                    now=1_800_000_000,
                )

        self.assertEqual(status, 0)
        self.assertEqual(cargo_run.call_args.args[0], EXPECTED_CARGO_ARGV)
        kwargs = cargo_run.call_args.kwargs
        child_environment = kwargs["env"]
        self.assertEqual(child_environment["HOME"], "/Users/yunho")
        self.assertEqual(child_environment["PATH"], base_environment["PATH"])
        self.assertEqual(child_environment["CARGO_INCREMENTAL"], "0")
        self.assertEqual(child_environment["NO_PROXY"], "*")
        self.assertEqual(child_environment["no_proxy"], "*")
        for name in (
            "HTTP_PROXY",
            "http_proxy",
            "HTTPS_PROXY",
            "https_proxy",
            "ALL_PROXY",
            "all_proxy",
        ):
            self.assertTrue(
                name not in child_environment,
                f"{name} leaked into the native child environment",
            )
        self.assertNotEqual(
            child_environment["TASK9_USER_ACCESS_TOKEN"],
            "inherited-secret-must-be-replaced",
        )
        self.assertEqual(child_environment["TASK9_API_URL"], "http://127.0.0.1:54480")
        self.assertEqual(child_environment["TASK9_ANON_KEY"], self.anon_jwt)
        self.assertEqual(child_environment["TASK9_USER_ID"], self.owner_id)
        self.assertEqual(child_environment["TASK9_VIEWER_USER_ID"], self.viewer_id)
        self.assertEqual(
            child_environment["TASK9_SOURCE_ROOT"], str(self.run_dir / "sources")
        )
        self.assertEqual(
            child_environment["TASK9_WORLD_ID_FILE"], str(self.run_dir / "world-id.txt")
        )
        self.assertEqual(kwargs["cwd"], "/Users/yunho/Desktop/project/token-planet")
        self.assertEqual(kwargs["stderr"], subprocess.STDOUT)

        native_env_path = self.run_dir / "native-env.json"
        self.assertEqual(stat.S_IMODE(native_env_path.stat().st_mode), 0o600)
        native_log_info = os.stat(self.run_dir / "native.log", follow_symlinks=False)
        self.assertTrue(stat.S_ISREG(native_log_info.st_mode))
        self.assertEqual(stat.S_IMODE(native_log_info.st_mode), 0o600)
        native_env = json.loads(native_env_path.read_text(encoding="utf-8"))
        self.assertEqual(native_env, {key: child_environment[key] for key in native_env})
        self.assertNotIn("jwt_secret", native_env)
        self.assertNotIn("TASK9_OUTSIDER_ACCESS_TOKEN", native_env)
        self.assertNotIn("unit-test-only-secret", captured_stdout.getvalue())
        self.assertNotIn("unit-test-only-secret", captured_stderr.getvalue())

    def test_runtime_key_role_signature_and_generated_user_subjects_are_bound(self):
        runtime_path = self.run_dir / "runtime.env"

        def update_anon_token(token):
            runtime = json.loads(runtime_path.read_text(encoding="utf-8"))
            runtime["anon_jwt"] = token
            runtime_path.write_text(json.dumps(runtime, separators=(",", ":")) + "\n")
            os.chmod(runtime_path, 0o600)

        good_claims = {
            "role": "anon",
            "iss": "supabase",
            "iat": 1_700_000_000,
            "exp": 1_800_000_000,
        }
        update_anon_token(self._signed_test_jwt(good_claims, "wrong-test-secret-not-the-runtime-secret"))
        with self.assertRaises(NativeE2EError):
            load_run_context(str(self.run_dir))

        for role in ("service_role", "authenticated"):
            update_anon_token(self._signed_test_jwt({**good_claims, "role": role}, self.jwt_secret))
            with self.assertRaises(NativeE2EError):
                load_run_context(str(self.run_dir))

        update_anon_token(self.anon_jwt)
        context = load_run_context(str(self.run_dir))
        native_environment = build_native_environment(context, 1_700_000_000)
        self.assertEqual(self._jwt_payload(native_environment["TASK9_ANON_KEY"])["role"], "anon")
        for env_key, user_id in (
            ("TASK9_USER_ACCESS_TOKEN", self.owner_id),
            ("TASK9_VIEWER_ACCESS_TOKEN", self.viewer_id),
        ):
            token = native_environment[env_key]
            claims = self._jwt_payload(token)
            self.assertTrue(self._jwt_signature_valid(token, self.jwt_secret))
            self.assertEqual(claims["role"], "authenticated")
            self.assertEqual(claims["sub"], user_id)
            self.assertEqual(claims["iss"], "supabase")
            self.assertEqual(claims["iat"], 1_700_000_000)
            self.assertEqual(claims["exp"], 1_700_003_600)
        self.assertNotIn("TASK9_OUTSIDER_ACCESS_TOKEN", native_environment)

    def test_child_environment_drops_inherited_cargo_and_rustc_redirectors(self):
        base_environment = {
            "HOME": "/Users/yunho",
            "PATH": "/Users/yunho/.cargo/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin",
            "CARGO_TARGET_DIR": "/tmp/untrusted-target",
            "CARGO_HOME": "/tmp/untrusted-cargo-home",
            "RUSTUP_HOME": "/tmp/untrusted-rustup-home",
            "RUSTUP_TOOLCHAIN": "untrusted-toolchain",
            "RUSTC": "/tmp/untrusted-rustc",
            "RUSTDOC": "/tmp/untrusted-rustdoc",
            "RUSTC_WRAPPER": "/tmp/untrusted-rustc-wrapper",
            "RUSTC_WORKSPACE_WRAPPER": "/tmp/untrusted-workspace-wrapper",
            "CARGO_BUILD_RUSTC_WRAPPER": "/tmp/untrusted-build-wrapper",
            "RUSTFLAGS": "--out-dir /tmp/untrusted-out",
            "CARGO_ENCODED_RUSTFLAGS": "--out-dir/tmp/untrusted-encoded-out",
            "SUPABASE_SERVICE_ROLE_KEY": "must-not-reach-native-cargo",
        }
        context = load_run_context(str(self.run_dir))
        native_environment = build_native_environment(context, 1_700_000_000)
        child_environment = build_child_environment(base_environment, native_environment, uid=501)

        for name in (
            "CARGO_TARGET_DIR",
            "CARGO_HOME",
            "RUSTUP_HOME",
            "RUSTUP_TOOLCHAIN",
            "RUSTC",
            "RUSTDOC",
            "RUSTC_WRAPPER",
            "RUSTC_WORKSPACE_WRAPPER",
            "CARGO_BUILD_RUSTC_WRAPPER",
            "RUSTFLAGS",
            "CARGO_ENCODED_RUSTFLAGS",
            "SUPABASE_SERVICE_ROLE_KEY",
        ):
            self.assertTrue(
                name not in child_environment,
                f"{name} leaked into the native child environment",
            )

    def test_public_boundary_denials_and_three_viewer_reads_preserve_database_digest(self):
        # The CLI validates this run context while world-id.txt is still empty;
        # the native subprocess writes the UUID before public checks begin.
        context = load_run_context(str(self.run_dir))
        native_environment = build_native_environment(context, 1_700_000_000)
        primary_world_id = str(uuid.uuid4())
        outsider_world_id = str(uuid.uuid4())
        world_id_path = self.run_dir / "world-id.txt"
        world_id_path.write_text(primary_world_id, encoding="utf-8")
        os.chmod(world_id_path, 0o600)

        expected_scene = [
            {
                "nickname": "Imported owner",
                "current_planet_tokens": 17,
                "lifetime_tokens": 517,
                "growth_credit": 0.25,
                "objects": [],
                "private": {"safe_display_value": True},
            }
        ]
        request_calls = []

        def mocked_request(_context, method, path, *, access_token=None, payload=None, prefer=None):
            request_calls.append((method, path, access_token, payload, prefer))
            if method == "POST" and path == "/rest/v1/worlds":
                self.assertEqual(payload["owner_id"], self.outsider_id)
                self.assertEqual(prefer, "return=minimal")
                return 201, None
            if method == "GET" and path == "/rest/v1/worlds?select=id,name,timezone,owner_id":
                return 200, [
                    {
                        "id": outsider_world_id,
                        "owner_id": self.outsider_id,
                        "name": "Task 9 outsider visibility probe",
                        "timezone": "UTC",
                    }
                ]
            if method == "POST" and path == "/rest/v1/rpc/get_world_planets":
                world_id = payload["p_world_id"]
                if access_token != native_environment["TASK9_VIEWER_ACCESS_TOKEN"]:
                    return 403, {"message": "world access denied"}
                if world_id != primary_world_id:
                    return 403, {"message": "world access denied"}
                return 200, expected_scene
            self.fail("public boundary helper issued an unexpected method or path")

        digest = mock.Mock(side_effect=[BASELINE_DIGEST, BASELINE_DIGEST])
        result = run_public_boundary_checks(
            context,
            native_environment,
            request_fn=mocked_request,
            digest_fn=digest,
            now=1_700_000_000,
        )

        self.assertEqual(result["denials"], 3)
        self.assertEqual(result["viewer_reads"], 3)
        self.assertTrue(result["digest_unchanged"])
        self.assertEqual(digest.call_count, 2)
        self.assertEqual(
            sum(call[1] == "/rest/v1/rpc/get_world_planets" for call in request_calls),
            6,
        )
        self.assertEqual(
            sum(call[0] == "GET" and call[1] == "/rest/v1/worlds?select=id,name,timezone,owner_id" for call in request_calls),
            1,
        )

    def test_digest_brackets_all_denials_and_viewer_reads(self):
        context = load_run_context(str(self.run_dir))
        native_environment = build_native_environment(context, 1_700_000_000)
        primary_world_id = str(uuid.uuid4())
        outsider_world_id = str(uuid.uuid4())
        world_id_path = self.run_dir / "world-id.txt"
        world_id_path.write_text(primary_world_id, encoding="utf-8")
        os.chmod(world_id_path, 0o600)
        state = {"mutated_during_denial": False}
        events = []
        safe_scene = [{"nickname": "Imported owner", "current_planet_tokens": 17}]

        def mocked_request(_context, method, path, *, access_token=None, payload=None, prefer=None):
            if method == "POST" and path == "/rest/v1/worlds":
                events.append("world-post")
                return 201, None
            if method == "GET" and path == "/rest/v1/worlds?select=id,name,timezone,owner_id":
                events.append("world-read")
                return 200, [
                    {
                        "id": outsider_world_id,
                        "owner_id": self.outsider_id,
                        "name": "Task 9 outsider visibility probe",
                        "timezone": "UTC",
                    }
                ]
            if method == "POST" and path == "/rest/v1/rpc/get_world_planets":
                world_id = payload["p_world_id"]
                if access_token == native_environment["TASK9_VIEWER_ACCESS_TOKEN"] and world_id == primary_world_id:
                    events.append("viewer-read")
                    return 200, safe_scene
                if access_token is None:
                    events.append("denial-anon")
                    state["mutated_during_denial"] = True
                elif access_token == native_environment["TASK9_VIEWER_ACCESS_TOKEN"]:
                    events.append("denial-viewer-otherworld")
                else:
                    events.append("denial-outsider")
                return 403, {"message": "world access denied"}
            self.fail("public boundary helper issued an unexpected method or path")

        digest_calls = 0

        def changing_digest():
            nonlocal digest_calls
            digest_calls += 1
            events.append(f"digest-{digest_calls}")
            return "0" * 32 if state["mutated_during_denial"] else BASELINE_DIGEST

        with self.assertRaises(NativeE2EError):
            run_public_boundary_checks(
                context,
                native_environment,
                request_fn=mocked_request,
                digest_fn=changing_digest,
                now=1_700_000_000,
            )

        self.assertEqual(
            events,
            [
                "world-post",
                "world-read",
                "digest-1",
                "denial-anon",
                "denial-outsider",
                "denial-viewer-otherworld",
                "viewer-read",
                "viewer-read",
                "viewer-read",
                "digest-2",
            ],
        )

    def test_recursive_public_scene_deny_includes_private_cycle_identifiers(self):
        for private_key in ("cycle_id", "current_cycle_id"):
            with self.assertRaises(NativeE2EError):
                assert_scene_has_no_private_keys(
                    [{"member": {"details": [{private_key: "private-cycle-value"}]}}]
                )

    def test_http_opener_disables_proxies_and_redirects(self):
        with mock.patch("urllib.request.build_opener", return_value=object()) as builder:
            opener = build_no_proxy_opener()

        self.assertIsNotNone(opener)
        handlers = builder.call_args.args
        self.assertEqual(handlers[0].proxies, {})
        self.assertIsInstance(handlers[1], NoRedirect)
        self.assertIsNone(
            handlers[1].redirect_request(
                urllib.request.Request("http://127.0.0.1:54480/"),
                object(),
                302,
                "Found",
                {},
                "http://127.0.0.1:54481/",
            )
        )

    def test_native_failure_summary_redacts_secrets_and_keeps_panic_location(self):
        token = self._signed_test_jwt(
            {"role": "authenticated", "sub": self.owner_id}, self.jwt_secret
        )
        native_log = self.run_dir / "native.log"
        native_log.write_text(
            "running 1 test\n"
            "thread 'sync::task9_local_api_import_worker_cache_and_explicit_world_visibility' "
            "panicked at apps/desktop/src-tauri/src/sync/guest_shop_import.rs:1472:9:\n"
            f"TASK9_USER_ACCESS_TOKEN={token} JWT_SECRET={self.jwt_secret}\n"
            "test result: FAILED. 0 passed; 1 failed; 0 ignored\n",
            encoding="utf-8",
        )
        os.chmod(native_log, 0o600)

        summary = summarize_native_failure_log(
            native_log, secret_values=(token, self.jwt_secret)
        )

        self.assertTrue("guest_shop_import.rs:1472:9" in summary)
        self.assertTrue("test result: FAILED" in summary)
        self.assertTrue(token not in summary, "generated JWT was retained")
        self.assertTrue(self.jwt_secret not in summary, "runtime secret was retained")
        self.assertLessEqual(len(summary), 2400)

    def test_native_failure_summary_keeps_error_cause_and_redacts_environment(self):
        token = self._signed_test_jwt(
            {"role": "authenticated", "sub": self.owner_id}, self.jwt_secret
        )
        native_log = self.run_dir / "native.log"
        native_log.write_text(
            "running 1 test\n"
            "thread 'sync::task9_local_api_import_worker_cache_and_explicit_world_visibility' "
            "panicked at apps/desktop/src-tauri/src/sync/guest_shop_import.rs:1097:9:\n"
            "the current append must produce a fresh canonical revision: Error {\n"
            "    kind: Database,\n"
            "    cause: SQLSTATE 40001: serialization failure while appending the captured prefix,\n"
            "}\n"
            f"TASK9_USER_ACCESS_TOKEN={token} JWT_SECRET={self.jwt_secret}\n"
            "test result: FAILED. 0 passed; 1 failed; 0 ignored\n",
            encoding="utf-8",
        )
        os.chmod(native_log, 0o600)

        summary = summarize_native_failure_log(
            native_log, secret_values=(token, self.jwt_secret)
        )

        self.assertTrue("guest_shop_import.rs:1097:9" in summary)
        self.assertTrue("fresh canonical revision: Error" in summary)
        self.assertTrue("SQLSTATE 40001: serialization failure" in summary)
        self.assertTrue("TASK9_USER_ACCESS_TOKEN" not in summary)
        self.assertTrue("JWT_SECRET" not in summary)
        self.assertTrue(token not in summary, "generated JWT was retained")
        self.assertTrue(self.jwt_secret not in summary, "runtime secret was retained")
        self.assertLessEqual(len(summary), 2400)

    def test_native_failure_output_describes_cleanup_and_emits_summary(self):
        context = load_run_context(str(self.run_dir))
        native_log = (
            b"thread 'task9' panicked at apps/desktop/src-tauri/src/sync/guest_shop_import.rs:1097:9:\n"
            b"the current append must produce a fresh canonical revision: Error {\n"
            b"    cause: SQLSTATE 40001: serialization failure,\n"
            b"}\n"
        )
        completed = type("Completed", (), {"returncode": 101})()

        def mock_cargo(_argv, **kwargs):
            kwargs["stdout"].write(native_log)
            return completed

        base_environment = {
            "HOME": "/Users/yunho",
            "PATH": "/Users/yunho/.cargo/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin",
        }
        captured_stdout = io.StringIO()
        with mock.patch("subprocess.run", side_effect=mock_cargo):
            with contextlib.redirect_stdout(captured_stdout):
                status = run_native_probe(
                    str(self.run_dir),
                    base_environment=base_environment,
                    now=1_700_000_000,
                )

        output = captured_stdout.getvalue()
        self.assertEqual(status, 101)
        self.assertTrue("private log retained" not in output.lower())
        self.assertTrue("sanitized diagnostics follow" in output.lower())
        self.assertTrue("SQLSTATE 40001: serialization failure" in output)

    def test_database_digest_uses_fixed_desktop_context_and_embedded_sql(self):
        completed = subprocess.CompletedProcess(
            args=[], returncode=0, stdout=BASELINE_DIGEST + "\n", stderr=""
        )
        with mock.patch("subprocess.run", return_value=completed) as docker_run:
            digest = database_digest()

        self.assertEqual(digest, BASELINE_DIGEST)
        argv = docker_run.call_args.args[0]
        self.assertEqual(argv[:4], ["docker", "--context", "desktop-linux", "exec"])
        self.assertEqual(argv[4], "supabase_db_token-planet-shop-revamp-test")
        self.assertEqual(argv[5], "psql")
        self.assertEqual(argv[6:8], ["-X", "-q"])
        self.assertTrue("-c" in argv)
        self.assertTrue(any("pg_catalog.pg_tables" in item for item in argv))
        self.assertFalse(docker_run.call_args.kwargs.get("shell", False))

    def test_database_digest_drops_inherited_docker_endpoint_overrides(self):
        completed = subprocess.CompletedProcess(
            args=[], returncode=0, stdout=BASELINE_DIGEST + "\n", stderr=""
        )
        inherited = {
            "HOME": "/Users/yunho",
            "PATH": "/usr/bin:/bin:/usr/sbin",
            "DOCKER_CONFIG": "/tmp/untrusted-docker-config",
            "DOCKER_HOST": "tcp://untrusted.example:2375",
            "DOCKER_CONTEXT": "untrusted-context",
            "HTTP_PROXY": "http://untrusted-proxy.invalid:8080",
        }
        with mock.patch.dict(os.environ, inherited, clear=True):
            with mock.patch("subprocess.run", return_value=completed) as docker_run:
                database_digest()

        child_environment = docker_run.call_args.kwargs["env"]
        self.assertEqual(child_environment["HOME"], "/Users/yunho")
        self.assertTrue("DOCKER_CONFIG" not in child_environment)
        self.assertTrue("DOCKER_HOST" not in child_environment)
        self.assertTrue("DOCKER_CONTEXT" not in child_environment)
        self.assertTrue("HTTP_PROXY" not in child_environment)
        self.assertEqual(child_environment["NO_PROXY"], "*")

    def test_cli_runs_post_native_world_and_public_checks_after_native_success(self):
        context = {
            "fixture_ids": {
                "owner": self.owner_id,
                "viewer": self.viewer_id,
                "outsider": self.outsider_id,
            }
        }
        native_environment = {"TASK9_USER_ID": self.owner_id}
        result = {"denials": 3, "viewer_reads": 3, "digest_unchanged": True}
        with mock.patch(__name__ + ".run_native_probe", return_value=0) as native_run:
            with mock.patch(__name__ + ".load_run_context", return_value=context) as load_context:
                with mock.patch(
                    __name__ + ".build_native_environment", return_value=native_environment
                ) as build_environment:
                    with mock.patch(
                        __name__ + ".run_public_boundary_checks", return_value=result
                    ) as public_checks:
                        with contextlib.redirect_stdout(io.StringIO()):
                            status = main(["--run", str(self.run_dir)])

        self.assertEqual(status, 0)
        native_run.assert_called_once()
        load_context.assert_called_once_with(str(self.run_dir), post_native=True)
        build_environment.assert_called_once()
        public_checks.assert_called_once_with(context, native_environment, now=mock.ANY)

    def test_cli_stops_before_public_checks_when_native_test_fails(self):
        with mock.patch(__name__ + ".run_native_probe", return_value=7) as native_run:
            with mock.patch(__name__ + ".load_run_context") as load_context:
                with mock.patch(__name__ + ".run_public_boundary_checks") as public_checks:
                    status = run_full_probe(str(self.run_dir), now=1_700_000_000)

        self.assertEqual(status, 7)
        native_run.assert_called_once()
        load_context.assert_not_called()
        public_checks.assert_not_called()


if __name__ == "__main__":
    raise SystemExit(main())
