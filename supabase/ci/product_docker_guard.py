#!/usr/bin/env python3
"""Fail-closed Docker command boundary for the pinned product CI runner."""

import fcntl
import json
import os
from pathlib import Path
import re
import selectors
import signal
import stat
import subprocess
import sys
import time
from urllib.parse import urlsplit

from host_port_adapter import AdapterError, PINNED_IMAGE, adapt_docker_argv


POSTGRES_IMAGE = PINNED_IMAGE
PUBLISH = "56432:5432"
LOOPBACK_PUBLISH = "127.0.0.1:56432:5432"
DB_ENV_KEYS = ("POSTGRES_PASSWORD", "POSTGRES_HOST", "JWT_SECRET", "JWT_EXP")
REALTIME_ENV_KEYS = tuple(
    "PORT DB_HOST DB_PORT DB_USER DB_PASSWORD DB_NAME DB_AFTER_CONNECT_QUERY DB_ENC_KEY "
    "API_JWT_SECRET API_JWT_JWKS METRICS_JWT_SECRET APP_NAME SECRET_KEY_BASE ERL_AFLAGS "
    "DNS_NODES RLIMIT_NOFILE SEED_SELF_HOST RUN_JANITOR MAX_HEADER_LENGTH".split()
)
STORAGE_ENV_KEYS = tuple(
    "DB_INSTALL_ROLES DB_MIGRATIONS_FREEZE_AT ANON_KEY SERVICE_KEY PGRST_JWT_SECRET "
    "DATABASE_URL FILE_SIZE_LIMIT STORAGE_BACKEND STORAGE_FILE_BACKEND_PATH TENANT_ID REGION "
    "GLOBAL_S3_BUCKET".split()
)
AUTH_ENV_KEYS = tuple(
    "API_EXTERNAL_URL GOTRUE_LOG_LEVEL GOTRUE_DB_DRIVER GOTRUE_DB_DATABASE_URL GOTRUE_SITE_URL "
    "GOTRUE_JWT_SECRET".split()
)
HELPERS = (
    {
        "name": "realtime",
        "candidates": (
            "public.ecr.aws/supabase/realtime:v2.140.3",
            "ghcr.io/supabase/realtime:v2.140.3",
            "supabase/realtime:v2.140.3",
        ),
        "keys": REALTIME_ENV_KEYS,
        "fixed": {
            "PORT": "4000", "DB_PORT": "5432", "DB_USER": "supabase_admin",
            "DB_NAME": "postgres", "DB_AFTER_CONNECT_QUERY": "SET search_path TO _realtime",
            "APP_NAME": "realtime", "DNS_NODES": "''", "RLIMIT_NOFILE": "",
            "SEED_SELF_HOST": "true", "RUN_JANITOR": "true",
            "ERL_AFLAGS": "-proto_dist inet_tcp",
        },
        "command": (
            "/app/bin/realtime", "eval",
            '{:ok, _} = Application.ensure_all_started(:realtime)\n'
            '{:ok, _} = Realtime.Tenants.health_check("realtime-dev")',
        ),
        "database_url_key": None,
        "database_role": None,
    },
    {
        "name": "storage",
        "candidates": (
            "public.ecr.aws/supabase/storage-api:v1.79.28",
            "ghcr.io/supabase/storage-api:v1.79.28",
            "supabase/storage-api:v1.79.28",
        ),
        "keys": STORAGE_ENV_KEYS,
        "fixed": {
            "DB_INSTALL_ROLES": "false", "STORAGE_BACKEND": "file",
            "STORAGE_FILE_BACKEND_PATH": "/mnt", "TENANT_ID": "stub",
            "REGION": "stub", "GLOBAL_S3_BUCKET": "stub",
        },
        "command": ("node", "dist/scripts/migrate-call.js"),
        "database_url_key": "DATABASE_URL",
        "database_role": "supabase_storage_admin",
    },
    {
        "name": "auth",
        "candidates": (
            "public.ecr.aws/supabase/gotrue:v2.197.0",
            "ghcr.io/supabase/gotrue:v2.197.0",
            "supabase/gotrue:v2.197.0",
        ),
        "keys": AUTH_ENV_KEYS,
        "fixed": {"GOTRUE_LOG_LEVEL": "error", "GOTRUE_DB_DRIVER": "postgres"},
        "command": ("gotrue", "migrate"),
        "database_url_key": "GOTRUE_DB_DATABASE_URL",
        "database_role": "supabase_auth_admin",
    },
)
IMAGE_CANDIDATES = frozenset(
    [POSTGRES_IMAGE] + [candidate for helper in HELPERS for candidate in helper["candidates"]]
)
PROJECT_PATTERN = re.compile(r"token-planet-ci-[0-9a-f]{24}\Z")
DOCKER_ID_PATTERN = re.compile(r"[0-9a-f]{64}\Z")


REJECTION_CODES = {'Docker command could not be invoked': 'DOCKER_COMMAND_COULD_NOT_BE_INVOKED',
 'Docker command is not approved': 'DOCKER_COMMAND_IS_NOT_APPROVED',
 'Docker connection override is not approved': 'DOCKER_CONNECTION_OVERRIDE_IS_NOT_APPROVED',
 'Postgres create adapter rejected the pinned contract': 'POSTGRES_CREATE_ADAPTER_REJECTED_THE_PINNED_CONTRACT',
 'Postgres create does not match the pinned contract': 'POSTGRES_CREATE_DOES_NOT_MATCH_THE_PINNED_CONTRACT',
 'Postgres create environment is incomplete': 'POSTGRES_CREATE_ENVIRONMENT_IS_INCOMPLETE',
 'Postgres create environment is invalid': 'POSTGRES_CREATE_ENVIRONMENT_IS_INVALID',
 'Postgres create order is invalid': 'POSTGRES_CREATE_ORDER_IS_INVALID',
 'Postgres create returned an invalid container ID': 'POSTGRES_CREATE_RETURNED_AN_INVALID_CONTAINER_ID',
 'Postgres reset create order is invalid': 'POSTGRES_RESET_CREATE_ORDER_IS_INVALID',
 'Realtime database target is invalid': 'REALTIME_DATABASE_TARGET_IS_INVALID',
 'container inspection target is not owned': 'CONTAINER_INSPECTION_TARGET_IS_NOT_OWNED',
 'database container ownership could not be verified': 'DATABASE_CONTAINER_OWNERSHIP_COULD_NOT_BE_VERIFIED',
 'database container removal is out of order': 'DATABASE_CONTAINER_REMOVAL_IS_OUT_OF_ORDER',
 'database secret transfer is out of order': 'DATABASE_SECRET_TRANSFER_IS_OUT_OF_ORDER',
 'database start is out of order': 'DATABASE_START_IS_OUT_OF_ORDER',
 'database volume create is out of order': 'DATABASE_VOLUME_CREATE_IS_OUT_OF_ORDER',
 'database volume ownership could not be verified': 'DATABASE_VOLUME_OWNERSHIP_COULD_NOT_BE_VERIFIED',
 'database volume removal is out of order': 'DATABASE_VOLUME_REMOVAL_IS_OUT_OF_ORDER',
 'excluded service absence could not be verified': 'EXCLUDED_SERVICE_ABSENCE_COULD_NOT_BE_VERIFIED',
 'excluded service restart is not approved': 'EXCLUDED_SERVICE_RESTART_IS_NOT_APPROVED',
 'guard context is invalid': 'GUARD_CONTEXT_IS_INVALID',
 'guard state could not be recorded': 'GUARD_STATE_COULD_NOT_BE_RECORDED',
 'guard state is invalid': 'GUARD_STATE_IS_INVALID',
 'helper cidfile is invalid': 'HELPER_CIDFILE_IS_INVALID',
 'helper cidfile location is invalid': 'HELPER_CIDFILE_LOCATION_IS_INVALID',
 'helper cidfile location is not unique': 'HELPER_CIDFILE_LOCATION_IS_NOT_UNIQUE',
 'helper cleanup could not be verified': 'HELPER_CLEANUP_COULD_NOT_BE_VERIFIED',
 'helper cleanup failed': 'HELPER_CLEANUP_FAILED',
 'helper container is not owned by this run': 'HELPER_CONTAINER_IS_NOT_OWNED_BY_THIS_RUN',
 'helper database target is invalid': 'HELPER_DATABASE_TARGET_IS_INVALID',
 'helper environment does not match the pinned contract': 'HELPER_ENVIRONMENT_DOES_NOT_MATCH_THE_PINNED_CONTRACT',
 'helper environment is incomplete': 'HELPER_ENVIRONMENT_IS_INCOMPLETE',
 'helper environment is malformed': 'HELPER_ENVIRONMENT_IS_MALFORMED',
 'helper environment value is invalid': 'HELPER_ENVIRONMENT_VALUE_IS_INVALID',
 'helper image or command is not approved': 'HELPER_IMAGE_OR_COMMAND_IS_NOT_APPROVED',
 'helper options do not match the pinned contract': 'HELPER_OPTIONS_DO_NOT_MATCH_THE_PINNED_CONTRACT',
 'helper ownership inspection failed': 'HELPER_OWNERSHIP_INSPECTION_FAILED',
 'helper ownership inspection was invalid': 'HELPER_OWNERSHIP_INSPECTION_WAS_INVALID',
 'helper recovery command is invalid': 'HELPER_RECOVERY_COMMAND_IS_INVALID',
 'helper recovery ledger is invalid': 'HELPER_RECOVERY_LEDGER_IS_INVALID',
 'helper recovery ledger is missing': 'HELPER_RECOVERY_LEDGER_IS_MISSING',
 'helper recovery removal failed': 'HELPER_RECOVERY_REMOVAL_FAILED',
 'helper run does not match the pinned contract': 'HELPER_RUN_DOES_NOT_MATCH_THE_PINNED_CONTRACT',
 'helper run is out of order': 'HELPER_RUN_IS_OUT_OF_ORDER',
 'image inspection is not approved': 'IMAGE_INSPECTION_IS_NOT_APPROVED'}
REJECTION_CODES.update({
    "database inspect identity rejected": "DB_INSPECT_IDENTITY_REJECTED",
    "database inspect image rejected": "DB_INSPECT_IMAGE_REJECTED",
    "database inspect labels rejected": "DB_INSPECT_LABELS_REJECTED",
    "database inspect network rejected": "DB_INSPECT_NETWORK_REJECTED",
    "database inspect network mode rejected": "DB_INSPECT_NETWORK_MODE_REJECTED",
    "database inspect network attachment rejected": "DB_INSPECT_NETWORK_ATTACHMENT_REJECTED",
    "database inspect network attachment count rejected": "DB_INSPECT_NETWORK_ATTACHMENT_COUNT_REJECTED",
    "database inspect pre-start state rejected": "DB_INSPECT_PRE_START_STATE_REJECTED",
    "database inspect network attachment name rejected": "DB_INSPECT_NETWORK_ATTACHMENT_NAME_REJECTED",
    "database inspect network attachment id rejected": "DB_INSPECT_NETWORK_ATTACHMENT_ID_REJECTED",
    "database inspect network attachment malformed rejected": "DB_INSPECT_NETWORK_ATTACHMENT_INVALID",
    "database inspect publish rejected": "DB_INSPECT_PUBLISH_REJECTED",
    "database inspect volume mount rejected": "DB_INSPECT_VOLUME_MOUNT_REJECTED",
    "database inspect invalid": "DB_INSPECT_INVALID",
    "database inspect command failed": "DB_INSPECT_COMMAND_FAILED",
})
DIAGNOSTIC_CODES = frozenset(REJECTION_CODES.values()) | frozenset({
    "GUARD_REJECTION_UNKNOWN", "GUARD_IO_FAILED", "EVIDENCE_INVALID", "EVIDENCE_UNAVAILABLE",
})
EVIDENCE_PHASES = frozenset({"start", "reset", "cleanup"})
EVIDENCE_LIMIT = 8192
DIAGNOSTIC_INSPECT_TIMEOUT_SECONDS = 2
DIAGNOSTIC_INSPECT_OUTPUT_LIMIT = 65536


def evidence_records(raw):
    if len(raw) > EVIDENCE_LIMIT:
        raise ValueError
    records = []
    for line in raw.decode("utf-8").splitlines():
        value = json.loads(line)
        if (not isinstance(value, dict) or set(value) != {"phase", "code"}
                or not isinstance(value["phase"], str) or value["phase"] not in EVIDENCE_PHASES
                or not isinstance(value["code"], str) or value["code"] not in DIAGNOSTIC_CODES):
            raise ValueError
        records.append(value)
    if len(records) > 64:
        raise ValueError
    return records


def private_regular(descriptor, limit):
    value = os.fstat(descriptor)
    return (stat.S_ISREG(value.st_mode) and stat.S_IMODE(value.st_mode) == 0o600
            and value.st_uid == os.getuid() and value.st_nlink == 1 and value.st_size <= limit)


def evidence_open(context, name, flags):
    root = os.open(context["workdir"], os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    directory = None
    try:
        if os.fstat(root).st_uid != os.getuid():
            raise ValueError
        directory = os.open(".product-docker-guard", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=root)
        value = os.fstat(directory)
        if stat.S_IMODE(value.st_mode) != 0o700 or value.st_uid != os.getuid():
            raise ValueError
        return os.open(name, flags | os.O_NOFOLLOW | os.O_NONBLOCK, 0o600, dir_fd=directory)
    finally:
        if directory is not None:
            os.close(directory)
        os.close(root)


def record_rejection(context, code):
    if context is None or context.get("phase") not in EVIDENCE_PHASES or code not in DIAGNOSTIC_CODES:
        return
    descriptor = None
    try:
        descriptor = evidence_open(context, "rejections.jsonl", os.O_RDWR | os.O_CREAT | os.O_APPEND)
        fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        if not private_regular(descriptor, EVIDENCE_LIMIT):
            return
        raw = os.read(descriptor, EVIDENCE_LIMIT + 1)
        if len(evidence_records(raw)) >= 64:
            return
        record = (json.dumps({"phase": context["phase"], "code": code}, sort_keys=True) + "\n").encode("ascii")
        if len(raw) + len(record) <= EVIDENCE_LIMIT:
            os.write(descriptor, record)
    except (OSError, ValueError, TypeError, UnicodeError):
        pass
    finally:
        if descriptor is not None:
            try:
                os.close(descriptor)
            except OSError:
                pass


LOOKUPS = frozenset({"owned", "absent", "inspect_failed", "inspect_invalid", "ownership_unverified", "container_unrecorded"})
STATES = frozenset({"created", "running", "paused", "restarting", "removing", "exited", "dead", "unknown"})
HEALTHS = frozenset({"none", "starting", "healthy", "unhealthy", "unknown"})


def read_private_file(path, limit):
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    try:
        if not private_regular(descriptor, limit):
            raise ValueError
        raw = os.read(descriptor, limit + 1)
        if len(raw) > limit:
            raise ValueError
        return raw
    finally:
        os.close(descriptor)


def read_evidence_file(context, name, limit):
    descriptor = evidence_open(context, name, os.O_RDONLY)
    try:
        if not private_regular(descriptor, limit):
            raise ValueError
        raw = os.read(descriptor, limit + 1)
        if len(raw) > limit:
            raise ValueError
        return raw
    finally:
        os.close(descriptor)


def exported_evidence(context):
    try:
        return evidence_records(read_evidence_file(context, "rejections.jsonl", EVIDENCE_LIMIT))
    except FileNotFoundError:
        return [{"phase": "start", "code": "EVIDENCE_UNAVAILABLE"}]
    except (OSError, ValueError, TypeError, UnicodeError):
        return [{"phase": "start", "code": "EVIDENCE_INVALID"}]


def diagnostic_inspect(context, container_id):
    """Capture inspect only in bounded memory; never persist Docker metadata."""
    child = None
    try:
        child = subprocess.Popen(
            [context["docker"], "container", "inspect", container_id, "--format", "{{json .}}"],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True,
        )
        deadline = time.monotonic() + DIAGNOSTIC_INSPECT_TIMEOUT_SECONDS
        output = {"stdout": bytearray(), "stderr": bytearray()}
        total = 0
        with selectors.DefaultSelector() as selector:
            selector.register(child.stdout, selectors.EVENT_READ, "stdout")
            selector.register(child.stderr, selectors.EVENT_READ, "stderr")
            while selector.get_map():
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    reject("Docker command could not be invoked")
                for key, _ in selector.select(remaining):
                    chunk = os.read(key.fileobj.fileno(), min(4096, DIAGNOSTIC_INSPECT_OUTPUT_LIMIT - total + 1))
                    if not chunk:
                        selector.unregister(key.fileobj)
                        continue
                    total += len(chunk)
                    if total > DIAGNOSTIC_INSPECT_OUTPUT_LIMIT:
                        reject("Docker command could not be invoked")
                    output[key.data].extend(chunk)
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                reject("Docker command could not be invoked")
            status = child.wait(timeout=remaining)
        return subprocess.CompletedProcess([], status, bytes(output["stdout"]), bytes(output["stderr"]))
    except (OSError, subprocess.TimeoutExpired):
        reject("Docker command could not be invoked")
    finally:
        if child is not None:
            # Kill the isolated group even if descendants still hold pipe descriptors.
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            child.wait()
            child.stdout.close()
            child.stderr.close()


def db_diagnostic(context):
    summary = {"lookup": "container_unrecorded", "state": "unknown", "health": "unknown", "exit_code": None}
    try:
        ledger = json.loads(read_evidence_file(context, "state.json", 65536))
        if (not isinstance(ledger, dict) or ledger.get("version") != 1
                or (ledger.get("project"), ledger.get("workdir"), ledger.get("network"))
                != (context["project"], context["workdir"], context["network"])):
            return summary
        container_id = ledger.get("db_id")
        if not isinstance(container_id, str) or not DOCKER_ID_PATTERN.fullmatch(container_id):
            return summary
    except (OSError, ValueError, TypeError, UnicodeError):
        return summary
    summary["lookup"] = "inspect_failed"
    try:
        result = diagnostic_inspect(context, container_id)
    except GuardError:
        return summary
    if result.returncode:
        if (result.returncode == 1 and not result.stdout
                and result.stderr == ("Error: No such object: " + container_id + "\n").encode("ascii")):
            summary["lookup"] = "absent"
        return summary
    summary["lookup"] = "inspect_invalid"
    try:
        if len(result.stdout) > 65536:
            return summary
        value = json.loads(result.stdout)
        if not isinstance(value, dict):
            return summary
        if any(not isinstance(value.get(key), dict) for key in ("Config", "HostConfig", "NetworkSettings", "State")):
            return summary
        config, network_settings = value["Config"], value["NetworkSettings"]
        networks, mounts = network_settings.get("Networks"), value.get("Mounts")
        if not isinstance(networks, dict) or not isinstance(mounts, list):
            return summary
        summary["lookup"] = "ownership_unverified"
        expected_labels = {"com.supabase.cli.project": context["project"],
                           "com.docker.compose.project": context["project"],
                           "com.supabase.cli.workdir": context["workdir"]}
        owned = (value.get("Id") == container_id and value.get("Name") == "/" + context["db_name"]
                 and config.get("Image") == POSTGRES_IMAGE and config.get("Labels") == expected_labels
                 and value["HostConfig"].get("NetworkMode") == context["network"]
                 and set(networks) == {context["network_name"]} and all(isinstance(network, dict) and network.get("NetworkID") == context["network"] for network in networks.values())
                 and len(mounts) == 1 and isinstance(mounts[0], dict)
                 and mounts[0].get("Type") == "volume" and mounts[0].get("Name") == context["volume_name"]
                 and mounts[0].get("Destination") == "/var/lib/postgresql/data" and mounts[0].get("RW") is True)
        if not owned:
            return summary
        state = value["State"]
        status = state.get("Status")
        health_value = state.get("Health")
        health = "none" if health_value is None else (health_value.get("Status") if isinstance(health_value, dict) else None)
        exit_code = state.get("ExitCode")
        return {"lookup": "owned", "state": status if isinstance(status, str) and status in STATES else "unknown",
                "health": health if isinstance(health, str) and health in HEALTHS else "unknown",
                "exit_code": exit_code if type(exit_code) is int else None}
    except (ValueError, TypeError, UnicodeError):
        return summary


def normalize_diagnostics(raw):
    try:
        if not raw or len(raw) > EVIDENCE_LIMIT:
            raise ValueError
        rows = [json.loads(line) for line in raw.decode("utf-8").splitlines()]
        if len(rows) > 65:
            raise ValueError
        for row in rows:
            if not isinstance(row, dict):
                raise ValueError
            if set(row) == {"phase", "code"}:
                if (not isinstance(row["phase"], str) or row["phase"] not in EVIDENCE_PHASES
                        or not isinstance(row["code"], str) or row["code"] not in DIAGNOSTIC_CODES):
                    raise ValueError
            elif set(row) == {"lookup", "state", "health", "exit_code"}:
                if (not isinstance(row["lookup"], str) or row["lookup"] not in LOOKUPS
                        or not isinstance(row["state"], str) or row["state"] not in STATES
                        or not isinstance(row["health"], str) or row["health"] not in HEALTHS
                        or (row["exit_code"] is not None and type(row["exit_code"]) is not int)):
                    raise ValueError
                if row["lookup"] != "owned" and (row["state"] != "unknown" or row["health"] != "unknown" or row["exit_code"] is not None):
                    raise ValueError
            else:
                raise ValueError
        return rows
    except (ValueError, TypeError, UnicodeError):
        return [{"phase": "start", "code": "EVIDENCE_INVALID"}]


def diagnose_start():
    try:
        context = context_from_env()
        rows = exported_evidence(context)
        rows.append(db_diagnostic(context))
    except (GuardError, OSError, ValueError, TypeError):
        rows = [{"phase": "start", "code": "EVIDENCE_UNAVAILABLE"},
                {"lookup": "container_unrecorded", "state": "unknown", "health": "unknown", "exit_code": None}]
    for row in rows:
        print(json.dumps(row, sort_keys=True))
    return 0


class GuardError(Exception):
    def __init__(self, message):
        self.code = REJECTION_CODES.get(message, "GUARD_REJECTION_UNKNOWN")
        super().__init__(self.code)


def reject(message):
    raise GuardError(message)


def context_from_env():
    if any(os.environ.get(key) for key in ("DOCKER_HOST", "DOCKER_CONTEXT", "DOCKER_TLS_VERIFY", "DOCKER_CERT_PATH")):
        reject("Docker connection override is not approved")
    docker = os.environ.get("TOKEN_PLANET_REAL_DOCKER", "")
    project = os.environ.get("TOKEN_PLANET_GUARD_PROJECT_ID", "")
    workdir_text = os.environ.get("TOKEN_PLANET_GUARD_WORKDIR", "")
    network = os.environ.get("TOKEN_PLANET_GUARD_NETWORK_ID", "")
    network_name = os.environ.get("TOKEN_PLANET_GUARD_NETWORK_NAME", "")
    phase = os.environ.get("TOKEN_PLANET_GUARD_PHASE", "")
    if not docker.startswith("/") or not os.path.isfile(docker) or not os.access(docker, os.X_OK):
        reject("guard context is invalid")
    if not PROJECT_PATTERN.fullmatch(project) or not DOCKER_ID_PATTERN.fullmatch(network):
        reject("guard context is invalid")
    if network_name != "token-planet-ci-net-" + project.removeprefix("token-planet-ci-"):
        reject("guard context is invalid")
    if phase not in ("start", "reset", "cleanup", "diagnostic") or not workdir_text.startswith("/"):
        reject("guard context is invalid")
    workdir_path = Path(workdir_text)
    try:
        if workdir_path.is_symlink() or not workdir_path.is_dir():
            reject("guard context is invalid")
        workdir = str(workdir_path.resolve(strict=True))
    except OSError:
        reject("guard context is invalid")
    if workdir != workdir_text or not workdir_path.name.startswith("token-planet-ci."):
        reject("guard context is invalid")
    return {
        "docker": str(Path(docker).resolve()),
        "project": project,
        "workdir": workdir,
        "network": network,
        "network_name": network_name,
        "phase": phase,
        "db_name": "supabase_db_" + project,
        "volume_name": "supabase_db_" + project,
    }


def state_path(context):
    directory = Path(context["workdir"]) / ".product-docker-guard"
    try:
        if directory.is_symlink():
            reject("guard state is invalid")
        directory.mkdir(mode=0o700, exist_ok=True)
        if not directory.is_dir():
            reject("guard state is invalid")
    except OSError:
        reject("guard state is invalid")
    return directory / "state.json"


def initial_state(context):
    return {
        "version": 1,
        "project": context["project"],
        "workdir": context["workdir"],
        "network": context["network"],
        "db_id": None,
        "db_phase": None,
        "db_removed": False,
        "volume_present": False,
        "cp_done": {"start": False, "reset": False},
        "started": {"start": False, "reset": False},
        "helpers_done": {"start": [], "reset": []},
        "helper_records": [],
    }


def load_state(context):
    path = state_path(context)
    if path.is_symlink():
        reject("guard state is invalid")
    if not path.exists():
        state = initial_state(context)
        save_state(path, state)
        return path, state
    try:
        if not path.is_file() or path.stat().st_size > 65536:
            reject("guard state is invalid")
        state = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, ValueError):
        reject("guard state is invalid")
    expected_context = (context["project"], context["workdir"], context["network"])
    if not isinstance(state, dict) or state.get("version") != 1:
        reject("guard state is invalid")
    if (state.get("project"), state.get("workdir"), state.get("network")) != expected_context:
        reject("guard state is invalid")
    for key in ("cp_done", "started", "helpers_done"):
        if not isinstance(state.get(key), dict):
            reject("guard state is invalid")
    if state["cp_done"].get("start") not in (True, False) or state["cp_done"].get("reset") not in (True, False):
        reject("guard state is invalid")
    if state["started"].get("start") not in (True, False) or state["started"].get("reset") not in (True, False):
        reject("guard state is invalid")
    for phase in ("start", "reset"):
        if not isinstance(state["helpers_done"].get(phase), list):
            reject("guard state is invalid")
    db_id = state.get("db_id")
    if db_id is not None and (not isinstance(db_id, str) or not DOCKER_ID_PATTERN.fullmatch(db_id)):
        reject("guard state is invalid")
    return path, state


def save_state(path, state):
    temporary = path.with_name("state.json.tmp")
    if temporary.exists() or temporary.is_symlink():
        reject("guard state is invalid")
    try:
        descriptor = os.open(str(temporary), os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
            json.dump(state, stream, sort_keys=True)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(str(temporary), str(path))
    except OSError:
        try:
            temporary.unlink()
        except OSError:
            pass
        reject("guard state could not be recorded")


def run_inherited(context, args):
    try:
        return subprocess.run([context["docker"], *args], check=False).returncode
    except OSError:
        reject("Docker command could not be invoked")


def run_captured(context, args):
    try:
        return subprocess.run(
            [context["docker"], *args], check=False, stdout=subprocess.PIPE, stderr=subprocess.PIPE
        )
    except OSError:
        reject("Docker command could not be invoked")


def write_child_output(result):
    if result.stdout:
        sys.stdout.buffer.write(result.stdout)
        sys.stdout.buffer.flush()
    if result.stderr:
        sys.stderr.buffer.write(result.stderr)
        sys.stderr.buffer.flush()


def db_create_args(args, context):
    prefix = [
        "create", "--name", context["db_name"],
        "-e", "POSTGRES_PASSWORD", "-e", "POSTGRES_HOST", "-e", "JWT_SECRET", "-e", "JWT_EXP",
        "-v", context["db_name"] + ":/var/lib/postgresql/data", "-p", PUBLISH,
        "--health-cmd", "pg_isready -U postgres -h 127.0.0.1 -p 5432",
        "--health-interval", "10s", "--health-timeout", "2s", "--health-retries", "3",
        "--restart", "unless-stopped",
    ]
    if sys.platform.startswith("linux"):
        prefix.extend(["--add-host", "host.docker.internal:host-gateway"])
    prefix.extend([
        "--network", context["network"], "--network-alias", "db",
        "--network-alias", "db.supabase.internal",
        "--label", "com.supabase.cli.project=" + context["project"],
        "--label", "com.docker.compose.project=" + context["project"],
        "--label", "com.supabase.cli.workdir=" + context["workdir"],
        "--entrypoint", "sh", POSTGRES_IMAGE, "-c",
    ])
    if len(args) != len(prefix) + 1 or args[:-1] != prefix or not isinstance(args[-1], str) or not args[-1]:
        reject("Postgres create does not match the pinned contract")
    for key in DB_ENV_KEYS:
        if key not in os.environ:
            reject("Postgres create environment is incomplete")
    if os.environ.get("POSTGRES_HOST") != "/var/run/postgresql":
        reject("Postgres create environment is invalid")


def handle_create(args, context, path, state):
    db_create_args(args, context)
    if context["phase"] == "start":
        if state.get("db_id") is not None or state.get("volume_present") is not True:
            reject("Postgres create order is invalid")
    else:
        if state.get("db_removed") is not True or state.get("volume_present") is not True:
            reject("Postgres reset create order is invalid")
        if state.get("db_phase") != "start":
            reject("Postgres reset create order is invalid")
    verify_volume_ownership(context)
    try:
        adapted = adapt_docker_argv(
            args, project_id=context["project"], workdir=context["workdir"],
            network_id=context["network"], container_name=context["db_name"],
        )
    except AdapterError:
        reject("Postgres create adapter rejected the pinned contract")
    result = run_captured(context, adapted)
    if result.returncode == 0:
        try:
            output = result.stdout.decode("ascii")
        except UnicodeError:
            reject("Postgres create returned an invalid container ID")
        container_id = output.rstrip("\r\n")
        if not DOCKER_ID_PATTERN.fullmatch(container_id) or output not in (container_id, container_id + "\n", container_id + "\r\n"):
            reject("Postgres create returned an invalid container ID")
        state["db_id"] = container_id
        state["db_phase"] = context["phase"]
        state["db_removed"] = False
        state["cp_done"][context["phase"]] = False
        state["started"][context["phase"]] = False
        save_state(path, state)
    write_child_output(result)
    return result.returncode


def validate_database_url(value, role, db_name):
    try:
        parsed = urlsplit(value)
        valid = (
            parsed.scheme == "postgresql"
            and parsed.username == role
            and parsed.password not in (None, "")
            and parsed.hostname == db_name
            and parsed.port == 5432
            and parsed.path == "/postgres"
            and not parsed.query
            and not parsed.fragment
        )
    except (TypeError, ValueError):
        valid = False
    return valid


def match_helper(args, context):
    if len(args) < 4 or args[:2] != ["run", "--rm"]:
        reject("helper run does not match the pinned contract")
    for helper in HELPERS:
        suffix_length = len(helper["command"]) + 1
        if len(args) < suffix_length:
            continue
        image_index = len(args) - suffix_length
        image = args[image_index]
        if image not in helper["candidates"] or tuple(args[image_index + 1:]) != helper["command"]:
            continue
        expected_prefix = ["run", "--rm", "--network", context["network"]]
        if sys.platform.startswith("linux"):
            expected_prefix.extend(["--add-host", "host.docker.internal:host-gateway"])
        env_pairs = []
        index = len(expected_prefix)
        while index < image_index and args[index] == "-e":
            if index + 1 >= image_index:
                reject("helper environment is malformed")
            env_pairs.append(args[index + 1])
            index += 2
        if tuple(env_pairs) != helper["keys"]:
            reject("helper environment does not match the pinned contract")
        expected_labels = [
            "--label", "com.supabase.cli.project=" + context["project"],
            "--label", "com.docker.compose.project=" + context["project"],
        ]
        actual_prefix = args[:index]
        actual_labels = args[index:image_index]
        if actual_prefix != [
            "run", "--rm", "--network", context["network"],
            *(["--add-host", "host.docker.internal:host-gateway"] if sys.platform.startswith("linux") else []),
            *[token for key in helper["keys"] for token in ("-e", key)],
        ] or actual_labels != expected_labels:
            reject("helper options do not match the pinned contract")
        for key in helper["keys"]:
            if key not in os.environ:
                reject("helper environment is incomplete")
        for key, expected in helper["fixed"].items():
            if os.environ.get(key) != expected:
                reject("helper environment value is invalid")
        if helper["name"] == "realtime":
            if os.environ.get("DB_HOST") != context["db_name"]:
                reject("Realtime database target is invalid")
        elif not validate_database_url(
            os.environ.get(helper["database_url_key"], ""),
            helper["database_role"], context["db_name"],
        ):
            reject("helper database target is invalid")
        return helper, image
    reject("helper image or command is not approved")


def inspect_helper(context, helper, image, container_id):
    result = run_captured(context, [
        "container", "inspect", container_id, "--format", "{{json .}}",
    ])
    if result.returncode != 0:
        expected_missing = ("Error: No such object: " + container_id + "\n").encode("ascii")
        expected_daemon_missing = ("Error response from daemon: No such container: " + container_id + "\n").encode("ascii")
        if result.returncode == 1 and (result.stdout, result.stderr) in (
                (b"", expected_missing), (b"\n", expected_daemon_missing)):
            return False
        reject("helper ownership inspection failed")
    try:
        value = json.loads(result.stdout.decode("utf-8"))
    except (UnicodeError, ValueError):
        reject("helper ownership inspection was invalid")
    if not isinstance(value, dict):
        reject("helper ownership inspection was invalid")
    for key in ("Config", "HostConfig", "NetworkSettings"):
        if not isinstance(value.get(key), dict):
            reject("helper ownership inspection was invalid")
    for parent, key in (("Config", "Labels"), ("HostConfig", "PortBindings"),
                        ("NetworkSettings", "Networks"), ("NetworkSettings", "Ports")):
        if not isinstance(value[parent].get(key, {}), dict):
            reject("helper ownership inspection was invalid")
    labels = ((value.get("Config") or {}).get("Labels") or {})
    expected_labels = {
        "com.supabase.cli.project": context["project"],
        "com.docker.compose.project": context["project"],
    }
    host_config = value.get("HostConfig") or {}
    network_settings = value.get("NetworkSettings") or {}
    networks = network_settings.get("Networks") or {}
    if any(not isinstance(network, dict) or not isinstance(network.get("NetworkID"), str)
           for network in networks.values()):
        reject("helper ownership inspection was invalid")
    attached_ids = [network["NetworkID"] for network in networks.values()]
    ports = network_settings.get("Ports") or {}
    host_ports = host_config.get("PortBindings") or {}
    no_ports = not any(entries for entries in ports.values()) and not any(entries for entries in host_ports.values())
    no_mounts = not value.get("Mounts") and not host_config.get("Binds") and not host_config.get("Mounts")
    no_security_changes = (
        not host_config.get("Privileged", False)
        and not host_config.get("SecurityOpt")
        and not host_config.get("CapAdd")
        and not host_config.get("Devices")
    )
    if (
        value.get("Id") != container_id
        or ((value.get("Config") or {}).get("Image")) != image
        or labels != expected_labels
        or len(attached_ids) != 1
        or attached_ids[0] != context["network"]
        or host_config.get("NetworkMode") != context["network"]
        or not no_ports
        or not no_mounts
        or not no_security_changes
    ):
        reject("helper container is not owned by this run")
    return True


def read_cidfile(path):
    try:
        if path.is_symlink() or not path.is_file() or path.stat().st_size > 128:
            reject("helper cidfile is invalid")
        raw = path.read_bytes()
    except OSError:
        reject("helper cidfile is invalid")
    try:
        text = raw.decode("ascii")
    except UnicodeError:
        reject("helper cidfile is invalid")
    if text.endswith("\r\n"):
        text = text[:-2]
    elif text.endswith("\n"):
        text = text[:-1]
    if not DOCKER_ID_PATTERN.fullmatch(text):
        reject("helper cidfile is invalid")
    return text


def clean_helper(context, helper, image, cidfile):
    container_id = read_cidfile(cidfile)
    exists = inspect_helper(context, helper, image, container_id)
    if not exists:
        return True
    result = run_captured(context, ["container", "rm", "-f", container_id])
    if result.returncode != 0:
        write_child_output(result)
        return False
    return True


def handle_helper(args, context, path, state):
    helper, image = match_helper(args, context)
    phase = context["phase"]
    if state.get("db_phase") != phase or state.get("db_id") is None or not state["started"].get(phase):
        reject("helper run is out of order")
    done = state["helpers_done"][phase]
    expected_index = len(done)
    if expected_index >= len(HELPERS) or helper["name"] != HELPERS[expected_index]["name"]:
        reject("helper run is out of order")
    cid_dir = Path(context["workdir"]) / ".product-docker-guard" / "cidfiles"
    try:
        cid_dir.mkdir(mode=0o700, exist_ok=True)
        if cid_dir.is_symlink() or not cid_dir.is_dir():
            reject("helper cidfile location is invalid")
    except OSError:
        reject("helper cidfile location is invalid")
    cidfile = cid_dir / (phase + "-" + helper["name"] + ".cid")
    if cidfile.exists() or cidfile.is_symlink():
        reject("helper cidfile location is not unique")
    guarded_args = ["run", "--rm", "--pull=never", "--cidfile", str(cidfile), *args[2:]]
    done.append(helper["name"])
    record = {"phase": phase, "helper": helper["name"], "image": image, "cleaned": False}
    state["helper_records"].append(record)
    save_state(path, state)
    result = run_captured(context, guarded_args)
    cleaned = False
    try:
        cleaned = clean_helper(context, helper, image, cidfile)
    except GuardError as error:
        record_rejection(context, error.code)
        write_child_output(result)
        if result.returncode:
            return result.returncode
        reject("helper cleanup could not be verified")
    if cleaned:
        record["cleaned"] = True
        save_state(path, state)
    write_child_output(result)
    if not cleaned and result.returncode == 0:
        reject("helper cleanup failed")
    return result.returncode


def cleanup_helpers(context, path, state):
    records = state.get("helper_records")
    if not isinstance(records, list) or len(records) > 6:
        reject("helper recovery ledger is invalid")
    names = {"start": [], "reset": []}
    pending = []
    seen_ids = set()
    cid_dir = path.parent / "cidfiles"
    if cid_dir.is_symlink():
        reject("helper recovery ledger is invalid")
    for record in records:
        if not isinstance(record, dict) or set(record) != {"phase", "helper", "image", "cleaned"}:
            reject("helper recovery ledger is invalid")
        phase = record["phase"]
        if not isinstance(phase, str) or phase not in names or type(record["cleaned"]) is not bool:
            reject("helper recovery ledger is invalid")
        index = len(names[phase])
        if index >= len(HELPERS):
            reject("helper recovery ledger is invalid")
        helper = HELPERS[index]
        if record["helper"] != helper["name"] or record["image"] not in helper["candidates"]:
            reject("helper recovery ledger is invalid")
        names[phase].append(helper["name"])
        if not record["cleaned"]:
            cidfile = cid_dir / (phase + "-" + helper["name"] + ".cid")
            container_id = read_cidfile(cidfile)
            if container_id in seen_ids:
                reject("helper recovery ledger is invalid")
            seen_ids.add(container_id)
            pending.append((record, helper, container_id))
    if names != state["helpers_done"]:
        reject("helper recovery ledger is invalid")
    # Verify every recorded target before removing any container.
    verified = [(record, container_id, inspect_helper(context, helper, record["image"], container_id))
                for record, helper, container_id in pending]
    for record, container_id, exists in verified:
        if exists:
            result = run_captured(context, ["container", "rm", "-f", container_id])
            if result.returncode:
                reject("helper recovery removal failed")
        record["cleaned"] = True
        save_state(path, state)
    return 0


def verify_db_ownership(context, state, *, pre_start=False):
    try:
        result = run_captured(context, ["container", "inspect", state["db_id"], "--format", "{{json .}}"])
    except GuardError:
        reject("database inspect command failed")
    if result.returncode != 0:
        reject("database inspect command failed")
    try:
        value = json.loads(result.stdout.decode("utf-8"))
        config = value["Config"]
        attachment_count_valid = False
        attachment_id_valid = False
        attachment_name_valid = False
        try:
            networks = value["NetworkSettings"]["Networks"]
            if (not isinstance(networks, dict)
                    or any(not isinstance(network, dict) or not isinstance(network.get("NetworkID"), str)
                           for network in networks.values())):
                attachment_valid = None
            else:
                attachment_valid = True
                attachment_count_valid = len(networks) == 1
                attachment_name_valid = set(networks) == {context["network_name"]}
                allowed_ids = ("", context["network"]) if pre_start else (context["network"],)
                attachment_id_valid = (attachment_count_valid
                                       and all(network["NetworkID"] in allowed_ids for network in networks.values()))
        except (KeyError, TypeError, AttributeError):
            attachment_valid = None
        expected_labels = {
            "com.supabase.cli.project": context["project"],
            "com.docker.compose.project": context["project"],
            "com.supabase.cli.workdir": context["workdir"],
        }
        mounts = value["Mounts"]
        binding = [{"HostIp": "127.0.0.1", "HostPort": "56432"}]
        actual_ports = value["NetworkSettings"]["Ports"]
        lifecycle = value.get("State")
        checks = (
            (not pre_start or (isinstance(lifecycle, dict) and lifecycle.get("Status") == "created"
                               and lifecycle.get("Running") is False), "pre-start state"),
            (value["Id"] == state["db_id"] and value["Name"] == "/" + context["db_name"], "identity"),
            (config["Image"] == POSTGRES_IMAGE, "image"),
            (config["Labels"] == expected_labels, "labels"),
            (value["HostConfig"]["NetworkMode"] == context["network"], "network mode"),
            (attachment_valid is not None, "network attachment malformed"),
            (attachment_count_valid, "network attachment count"),
            (attachment_name_valid, "network attachment name"),
            (attachment_id_valid, "network attachment id"),
            (value["HostConfig"]["PortBindings"] == {"5432/tcp": binding}
             and (not actual_ports or actual_ports == {"5432/tcp": binding}), "publish"),
            (isinstance(mounts, list) and len(mounts) == 1
             and mounts[0]["Type"] == "volume" and mounts[0]["Name"] == context["volume_name"]
             and mounts[0]["Destination"] == "/var/lib/postgresql/data" and mounts[0]["RW"] is True, "volume mount"),
        )
    except (UnicodeError, ValueError, KeyError, TypeError, AttributeError):
        reject("database inspect invalid")
    for valid, category in checks:
        if not valid:
            reject("database inspect " + category + " rejected")


def verify_volume_ownership(context):
    result = run_captured(context, ["volume", "inspect", context["volume_name"], "--format", "{{json .}}"])
    try:
        value = json.loads(result.stdout.decode("utf-8"))
        valid = (
            result.returncode == 0 and value["Name"] == context["volume_name"]
            and value["Labels"] == {
                "com.supabase.cli.project": context["project"],
                "com.docker.compose.project": context["project"],
            }
        )
    except (UnicodeError, ValueError, KeyError, TypeError):
        valid = False
    if not valid:
        reject("database volume ownership could not be verified")


def handle_excluded_restart(args, context):
    allowed = ["supabase_" + suffix + "_" + context["project"]
               for suffix in ("storage", "auth", "realtime", "pooler")]
    if context["phase"] != "reset" or len(args) != 2 or args[1] not in allowed:
        reject("excluded service restart is not approved")
    target = args[1]
    result = run_captured(context, ["container", "inspect", target, "--format", "{{json .State}}"])
    expected_missing = ("Error: No such object: " + target + "\n").encode("ascii")
    expected_daemon_missing = ("Error response from daemon: No such container: " + target + "\n").encode("ascii")
    if result.returncode != 1 or (result.stdout, result.stderr) not in (
            (b"", expected_missing), (b"\n", expected_daemon_missing)):
        reject("excluded service absence could not be verified")
    # Pinned reset tolerates this exact target's absence; never forward a restart mutation.
    print("Error response from daemon: No such container: " + target, file=sys.stderr)
    return 1


def handle_command(args, context, path, state):
    if args and args[0] == "restart":
        return handle_excluded_restart(args, context)
    if len(args) == 3 and args[:2] == ["image", "inspect"]:
        if args[2] not in IMAGE_CANDIDATES:
            reject("image inspection is not approved")
        return run_inherited(context, args)
    if args == ["network", "inspect", context["network"]]:
        return run_inherited(context, args)
    if args == ["volume", "inspect", context["volume_name"]]:
        return run_inherited(context, args)
    if args == [
        "volume", "create", "--label", "com.supabase.cli.project=" + context["project"],
        "--label", "com.docker.compose.project=" + context["project"], context["volume_name"],
    ]:
        if state.get("volume_present"):
            reject("database volume create is out of order")
        if context["phase"] == "reset" and state.get("db_removed") is not True:
            reject("database volume create is out of order")
        result = run_inherited(context, args)
        if result == 0:
            state["volume_present"] = True
            save_state(path, state)
        return result
    if args and args[0] == "create":
        return handle_create(args, context, path, state)
    if args and args[0] == "run":
        return handle_helper(args, context, path, state)
    if len(args) == 5 and args[:2] == ["container", "inspect"] and args[3:] == ["--format", "{{json .State}}"]:
        if context["phase"] == "reset" and args[2] == "supabase_kong_" + context["project"]:
            return run_inherited(context, args)
        if args[2] not in (context["db_name"], state.get("db_id")):
            reject("container inspection target is not owned")
        return run_inherited(context, args)
    if args == ["cp", "-", (state.get("db_id") or "") + ":/"]:
        phase = context["phase"]
        if state.get("db_phase") != phase or state.get("db_id") is None or state["cp_done"].get(phase):
            reject("database secret transfer is out of order")
        verify_db_ownership(context, state, pre_start=True)
        result = run_inherited(context, args)
        if result == 0:
            state["cp_done"][phase] = True
            save_state(path, state)
        return result
    if args == ["start", state.get("db_id") or ""]:
        phase = context["phase"]
        if state.get("db_phase") != phase or not state["cp_done"].get(phase) or state["started"].get(phase):
            reject("database start is out of order")
        verify_db_ownership(context, state, pre_start=True)
        result = run_inherited(context, args)
        if result == 0:
            verify_db_ownership(context, state)
            state["started"][phase] = True
            save_state(path, state)
        return result
    if len(args) == 4 and args[:3] == ["container", "rm", "-f"] and args[3] in (state.get("db_id"), context["db_name"]):
        if context["phase"] != "reset" or state.get("db_phase") != "start" or state.get("db_removed"):
            reject("database container removal is out of order")
        verify_volume_ownership(context)
        verify_db_ownership(context, state)
        result = run_inherited(context, ["container", "rm", "-f", state["db_id"]])
        if result == 0:
            state["db_removed"] = True
            save_state(path, state)
        return result
    if args == ["volume", "rm", "-f", context["volume_name"]]:
        if context["phase"] != "reset" or state.get("db_removed") is not True or not state.get("volume_present"):
            reject("database volume removal is out of order")
        verify_volume_ownership(context)
        result = run_inherited(context, args)
        if result == 0:
            state["volume_present"] = False
            save_state(path, state)
        return result
    reject("Docker command is not approved")


def main(args):
    if os.environ.get("TOKEN_PLANET_GUARD_PHASE") == "diagnostic" and args == ["--diagnose-start"]:
        return diagnose_start()
    context = None
    try:
        context = context_from_env()
        if context["phase"] == "diagnostic":
            reject("Docker command is not approved")
        if context["phase"] == "cleanup":
            if args != ["--cleanup-helpers"]:
                reject("helper recovery command is invalid")
            path = state_path(context)
            if not path.exists() and not path.is_symlink():
                if (path.parent / "cidfiles").exists() or (path.parent / "cidfiles").is_symlink():
                    reject("helper recovery ledger is missing")
                return 0
            path, state = load_state(context)
            return cleanup_helpers(context, path, state)
        path, state = load_state(context)
        return handle_command(args, context, path, state)
    except GuardError as error:
        record_rejection(context, error.code)
        print("product Docker guard rejected operation code=" + error.code, file=sys.stderr)
        return 125
    except OSError:
        record_rejection(context, "GUARD_IO_FAILED")
        print("product Docker guard rejected operation code=GUARD_IO_FAILED", file=sys.stderr)
        return 125


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
