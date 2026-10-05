import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import textwrap
import time
import unittest


GUARD = Path(__file__).with_name("product_docker_guard.py")
DB_ID = "a" * 64
HELPER_ID = "b" * 64
PROJECT_ID = "token-planet-ci-0123456789abcdef01234567"
NETWORK_ID = "c" * 64
NETWORK_NAME = "token-planet-ci-net-0123456789abcdef01234567"
POSTGRES_IMAGE = "public.ecr.aws/supabase/postgres:17.6.1.171"

HELPERS = {
    "realtime": {
        "image": "public.ecr.aws/supabase/realtime:v2.140.3",
        "candidates": [
            "public.ecr.aws/supabase/realtime:v2.140.3",
            "ghcr.io/supabase/realtime:v2.140.3",
            "supabase/realtime:v2.140.3",
        ],
        "keys": "PORT DB_HOST DB_PORT DB_USER DB_PASSWORD DB_NAME DB_AFTER_CONNECT_QUERY DB_ENC_KEY API_JWT_SECRET API_JWT_JWKS METRICS_JWT_SECRET APP_NAME SECRET_KEY_BASE ERL_AFLAGS DNS_NODES RLIMIT_NOFILE SEED_SELF_HOST RUN_JANITOR MAX_HEADER_LENGTH".split(),
        "command": [
            "/app/bin/realtime", "eval",
            '{:ok, _} = Application.ensure_all_started(:realtime)\n'
            '{:ok, _} = Realtime.Tenants.health_check("realtime-dev")',
        ],
        "fixed": {
            "PORT": "4000", "DB_PORT": "5432", "DB_USER": "supabase_admin",
            "DB_NAME": "postgres", "DB_AFTER_CONNECT_QUERY": "SET search_path TO _realtime",
            "APP_NAME": "realtime", "DNS_NODES": "''", "RLIMIT_NOFILE": "",
            "SEED_SELF_HOST": "true", "RUN_JANITOR": "true",
            "ERL_AFLAGS": "-proto_dist inet_tcp",
        },
    },
    "storage": {
        "image": "public.ecr.aws/supabase/storage-api:v1.79.28",
        "candidates": [
            "public.ecr.aws/supabase/storage-api:v1.79.28",
            "ghcr.io/supabase/storage-api:v1.79.28",
            "supabase/storage-api:v1.79.28",
        ],
        "keys": "DB_INSTALL_ROLES DB_MIGRATIONS_FREEZE_AT ANON_KEY SERVICE_KEY PGRST_JWT_SECRET DATABASE_URL FILE_SIZE_LIMIT STORAGE_BACKEND STORAGE_FILE_BACKEND_PATH TENANT_ID REGION GLOBAL_S3_BUCKET".split(),
        "command": ["node", "dist/scripts/migrate-call.js"],
        "fixed": {
            "DB_INSTALL_ROLES": "false", "STORAGE_BACKEND": "file",
            "STORAGE_FILE_BACKEND_PATH": "/mnt", "TENANT_ID": "stub",
            "REGION": "stub", "GLOBAL_S3_BUCKET": "stub",
        },
    },
    "auth": {
        "image": "public.ecr.aws/supabase/gotrue:v2.197.0",
        "candidates": [
            "public.ecr.aws/supabase/gotrue:v2.197.0",
            "ghcr.io/supabase/gotrue:v2.197.0",
            "supabase/gotrue:v2.197.0",
        ],
        "keys": "API_EXTERNAL_URL GOTRUE_LOG_LEVEL GOTRUE_DB_DRIVER GOTRUE_DB_DATABASE_URL GOTRUE_SITE_URL GOTRUE_JWT_SECRET".split(),
        "command": ["gotrue", "migrate"],
        "fixed": {"GOTRUE_LOG_LEVEL": "error", "GOTRUE_DB_DRIVER": "postgres"},
    },
}

EXPECTED_REJECTION_CODES = {'Docker command could not be invoked': 'DOCKER_COMMAND_COULD_NOT_BE_INVOKED',
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

FAKE_DOCKER = r'''#!/usr/bin/env python3
import json, os, pathlib, sys
args = sys.argv[1:]
events = pathlib.Path(os.environ["FAKE_DOCKER_EVENTS"])
state_path = pathlib.Path(os.environ["FAKE_DOCKER_STATE"])
def record():
    with events.open("a") as stream:
        stream.write(json.dumps({"args": args}) + "\n")
def read_state():
    return json.loads(state_path.read_text()) if state_path.exists() else {}
def write_state(state):
    state_path.write_text(json.dumps(state))
record()
state = read_state()
if args[:2] == ["image", "inspect"]:
    print(json.dumps({"Id": "cached-image"}))
elif args[:2] == ["network", "inspect"]:
    print(json.dumps({"Id": os.environ["TOKEN_PLANET_GUARD_NETWORK_ID"]}))
elif args[:2] in (["volume", "create"], ["volume", "rm"]):
    pass
elif args[:2] == ["volume", "inspect"]:
    project = os.environ["TOKEN_PLANET_GUARD_PROJECT_ID"]
    print(json.dumps({"Name": "supabase_db_" + project, "Labels": {
        "com.supabase.cli.project": "foreign" if os.environ.get("FAKE_VOLUME_FOREIGN") == "1" else project,
        "com.docker.compose.project": project,
    }}))
elif args and args[0] == "create":
    state["db_started"] = False
    state["db_id"] = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    write_state(state)
    print(state["db_id"])
elif args[:1] == ["cp"]:
    pass
elif args[:1] == ["start"]:
    if os.environ.get("FAKE_START_EXIT"):
        sys.exit(int(os.environ["FAKE_START_EXIT"]))
    state["db_started"] = True
    state["post_start_bad"] = os.environ.get("FAKE_POST_START_BAD", "")
    write_state(state)
elif args[:2] == ["container", "inspect"]:
    target = args[2] if len(args) > 2 else ""
    if os.environ.get("TOKEN_PLANET_GUARD_PHASE") == "diagnostic":
        mode = os.environ.get("FAKE_DIAGNOSTIC_LOOKUP", "owned")
        if mode == "timeout":
            import time
            time.sleep(30)
        if mode == "oversize":
            sys.stdout.write("password-JWT-log-sentinel" * 100000)
            sys.stdout.flush()
            sys.exit(0)
        if mode == "stderr_oversize":
            sys.stderr.write("password-JWT-log-sentinel" * 100000)
            sys.stderr.flush()
            sys.exit(1)
        if mode in ("absent", "inspect_failed"):
            print(f"Error: No such object: {target}" if mode == "absent" else "daemon password-JWT-log-sentinel", file=sys.stderr)
            sys.exit(1)
        if mode == "inspect_invalid":
            print("password-JWT-log-sentinel")
            sys.exit(0)
    if os.environ.get("FAKE_DB_INSPECT_JSON"):
        print(os.environ["FAKE_DB_INSPECT_JSON"])
        sys.exit(int(os.environ.get("FAKE_DB_INSPECT_EXIT", "0")))
    if target.startswith(("supabase_storage_", "supabase_auth_", "supabase_realtime_", "supabase_pooler_", "supabase_kong_")):
        if os.environ.get("FAKE_EXCLUDED_ABSENCE"):
            code, stdout, stderr = json.loads(os.environ["FAKE_EXCLUDED_ABSENCE"])
            sys.stdout.write(stdout)
            sys.stderr.write(stderr)
            sys.exit(code)
        mode = os.environ.get("FAKE_EXCLUDED_SERVICE", "absent")
        if mode == "absent":
            print(f"Error: No such object: {target}", file=sys.stderr)
            sys.exit(1)
        if mode == "inspect-error":
            print("Cannot connect to Docker daemon", file=sys.stderr)
            sys.exit(1)
        print(json.dumps({"Running": mode == "running"}))
        sys.exit(0)
    if target == "b" * 64 and os.environ.get("FAKE_HELPER_ABSENCE"):
        code, stdout, stderr = json.loads(os.environ["FAKE_HELPER_ABSENCE"])
        sys.stdout.write(stdout)
        sys.stderr.write(stderr)
        sys.exit(code)
    if target in state.get("helpers", {}):
        print(json.dumps(state["helpers"][target]))
        sys.exit(0)
    if target == state.get("helper_id") and state.get("helper"):
        if os.environ.get("FAKE_HELPER_NOT_FOUND_WITH_OUTPUT") == "1":
            print("unexpected inspect output")
            print(f"Error: No such object: {target}", file=sys.stderr)
            sys.exit(1)
        print(os.environ.get("FAKE_HELPER_INSPECT_JSON", json.dumps(state["helper"])))
    elif target == state.get("db_id") or target == "supabase_db_" + os.environ["TOKEN_PLANET_GUARD_PROJECT_ID"]:
        project = os.environ["TOKEN_PLANET_GUARD_PROJECT_ID"]
        network = os.environ["TOKEN_PLANET_GUARD_NETWORK_ID"]
        print(json.dumps({"Id": state["db_id"], "Name": "/supabase_db_" + project,
            "State": {"Status": "created", "Running": False} if not state.get("db_started") else json.loads(os.environ.get("FAKE_DIAGNOSTIC_STATE", '{"Status":"exited","ExitCode":17,"Error":"password-JWT-log-sentinel","Health":{"Status":"unhealthy","Log":[{"Output":"password-JWT-log-sentinel"}]}}')),
            "Config": {"Image": "foreign" if os.environ.get("FAKE_DB_MUTATION") == "image" else "public.ecr.aws/supabase/postgres:17.6.1.171", "Labels": {
                "com.supabase.cli.project": "foreign" if os.environ.get("FAKE_DB_FOREIGN") == "1" else project,
                "com.docker.compose.project": project,
                "com.supabase.cli.workdir": os.environ["TOKEN_PLANET_GUARD_WORKDIR"],
            }}, "HostConfig": {"NetworkMode": network, "PortBindings": {"5432/tcp": [{"HostIp": "127.0.0.1", "HostPort": "56432"}]}},
            "NetworkSettings": {"Networks": {os.environ["TOKEN_PLANET_GUARD_NETWORK_NAME"]: {"NetworkID": "f" * 64 if os.environ.get("FAKE_DB_MUTATION") == "network" else (state.get("post_start_bad") or network) if state.get("db_started") else ""}},
                                "Ports": {"5432/tcp": [{"HostIp": "0.0.0.0" if os.environ.get("FAKE_DB_MUTATION") == "publish" else "127.0.0.1", "HostPort": "56432"}]}},
            "Mounts": [{"Type": "volume", "Name": "foreign" if os.environ.get("FAKE_DB_MUTATION") == "mount" else "supabase_db_" + project,
                        "Destination": "/var/lib/postgresql/data", "RW": True}],
        }))
    else:
        print(f"Error: No such object: {target}", file=sys.stderr)
        sys.exit(1)
elif args[:2] == ["container", "rm"]:
    target = args[-1]
    if target == state.get("helper_id"):
        state["helper"] = None
        write_state(state)
elif args[:1] == ["run"]:
    cidfile = args[args.index("--cidfile") + 1]
    helper_id = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    cidfile_path = pathlib.Path(cidfile)
    cidfile_path.parent.mkdir(parents=True, exist_ok=True)
    cidfile_path.write_text("not-a-container-id\n" if os.environ.get("FAKE_BAD_CIDFILE") == "1" else helper_id + "\n")
    if os.environ.get("FAKE_HELPER_RETAIN") == "1":
        image = args[args.index("--label", args.index("-e")) + 4]
        state["helper_id"] = helper_id
        labels = {
            "com.supabase.cli.project": os.environ["TOKEN_PLANET_GUARD_PROJECT_ID"],
            "com.docker.compose.project": os.environ["TOKEN_PLANET_GUARD_PROJECT_ID"],
        }
        if os.environ.get("FAKE_HELPER_FOREIGN") == "1":
            labels["com.supabase.cli.project"] = "foreign-project"
        network = os.environ["TOKEN_PLANET_GUARD_NETWORK_ID"]
        state["helper"] = {
            "Id": helper_id,
            "Name": "/helper-container",
            "Config": {"Image": image, "Labels": labels},
            "HostConfig": {"NetworkMode": network, "PortBindings": {}},
            "NetworkSettings": {"Networks": {network: {"NetworkID": network}}, "Ports": {}},
            "Mounts": [],
        }
        if os.environ.get("FAKE_HELPER_MUTATION"):
            mutation = os.environ["FAKE_HELPER_MUTATION"]
            if mutation == "network":
                state["helper"]["NetworkSettings"]["Networks"][network]["NetworkID"] = "f" * 64
            elif mutation == "network_malformed_extra":
                state["helper"]["NetworkSettings"]["Networks"]["unexpected"] = None
            elif mutation == "image":
                state["helper"]["Config"]["Image"] = "foreign/image"
            elif mutation == "mount":
                state["helper"]["Mounts"] = [{"Source": "/foreign"}]
            elif mutation == "ports":
                state["helper"]["HostConfig"]["PortBindings"] = {"80/tcp": [{"HostPort": "80"}]}
        write_state(state)
    if os.environ.get("FAKE_HELPER_EXIT") == "1":
        print("fake helper failure", file=sys.stderr)
        sys.exit(44)
else:
    print("unexpected fake Docker command", file=sys.stderr)
    sys.exit(99)
'''


class ProductDockerGuardTests(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory()
        self.root = Path(self.temp_dir.name).resolve()
        self.workdir = self.root / "token-planet-ci.ABC12345"
        self.workdir.mkdir()
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.fake_docker = self.bin / "docker"
        self.fake_docker.write_text(textwrap.dedent(FAKE_DOCKER))
        self.fake_docker.chmod(0o755)
        self.events = self.root / "docker-events.jsonl"
        self.state = self.root / "docker-state.json"
        self.env = os.environ.copy()
        self.env.update({
            "TOKEN_PLANET_REAL_DOCKER": str(self.fake_docker),
            "TOKEN_PLANET_GUARD_PROJECT_ID": PROJECT_ID,
            "TOKEN_PLANET_GUARD_WORKDIR": str(self.workdir),
            "TOKEN_PLANET_GUARD_NETWORK_ID": NETWORK_ID,
            "TOKEN_PLANET_GUARD_NETWORK_NAME": NETWORK_NAME,
            "TOKEN_PLANET_GUARD_PHASE": "start",
            "FAKE_DOCKER_EVENTS": str(self.events),
            "FAKE_DOCKER_STATE": str(self.state),
            "POSTGRES_PASSWORD": "db-secret-fixture",
            "POSTGRES_HOST": "/var/run/postgresql",
            "JWT_SECRET": "jwt-secret-fixture",
            "JWT_EXP": "3600",
        })
        self.set_helper_env()

    def tearDown(self):
        self.temp_dir.cleanup()

    @property
    def db_name(self):
        return "supabase_db_" + PROJECT_ID

    def set_helper_env(self):
        values = {
            "DB_HOST": self.db_name,
            "DB_PORT": "5432", "DB_USER": "supabase_admin", "DB_PASSWORD": "db-secret-fixture",
            "DB_NAME": "postgres", "DB_AFTER_CONNECT_QUERY": "SET search_path TO _realtime",
            "DB_ENC_KEY": "encryption-key-fixture", "API_JWT_SECRET": "api-secret-fixture",
            "API_JWT_JWKS": "jwks-fixture", "METRICS_JWT_SECRET": "metrics-secret-fixture",
            "SECRET_KEY_BASE": "secret-key-base-fixture", "MAX_HEADER_LENGTH": "8192",
            "DB_INSTALL_ROLES": "false", "DB_MIGRATIONS_FREEZE_AT": "",
            "ANON_KEY": "anon-fixture", "SERVICE_KEY": "service-fixture",
            "PGRST_JWT_SECRET": "pgrst-secret-fixture",
            "DATABASE_URL": "postgresql://supabase_storage_admin:db-secret-fixture@" + self.db_name + ":5432/postgres",
            "FILE_SIZE_LIMIT": "52428800", "STORAGE_BACKEND": "file",
            "STORAGE_FILE_BACKEND_PATH": "/mnt", "TENANT_ID": "stub", "REGION": "stub",
            "GLOBAL_S3_BUCKET": "stub", "API_EXTERNAL_URL": "http://127.0.0.1:54321",
            "GOTRUE_LOG_LEVEL": "error", "GOTRUE_DB_DRIVER": "postgres",
            "GOTRUE_DB_DATABASE_URL": "postgresql://supabase_auth_admin:db-secret-fixture@" + self.db_name + ":5432/postgres",
            "GOTRUE_SITE_URL": "http://127.0.0.1:3000", "GOTRUE_JWT_SECRET": "jwt-secret-fixture",
            "PORT": "4000", "APP_NAME": "realtime", "DNS_NODES": "''",
            "RLIMIT_NOFILE": "", "SEED_SELF_HOST": "true", "RUN_JANITOR": "true",
            "ERL_AFLAGS": "-proto_dist inet_tcp",
        }
        self.env.update(values)

    def db_create_args(self):
        name = self.db_name
        args = [
            "create", "--name", name,
            "-e", "POSTGRES_PASSWORD", "-e", "POSTGRES_HOST", "-e", "JWT_SECRET", "-e", "JWT_EXP",
            "-v", f"{name}:/var/lib/postgresql/data", "-p", "56432:5432",
            "--health-cmd", "pg_isready -U postgres -h 127.0.0.1 -p 5432",
            "--health-interval", "10s", "--health-timeout", "2s", "--health-retries", "3",
            "--restart", "unless-stopped", "--network", NETWORK_ID,
            "--network-alias", "db", "--network-alias", "db.supabase.internal",
            "--label", "com.supabase.cli.project=" + PROJECT_ID,
            "--label", "com.docker.compose.project=" + PROJECT_ID,
            "--label", "com.supabase.cli.workdir=" + str(self.workdir),
            "--entrypoint", "sh", POSTGRES_IMAGE, "-c", "opaque pinned init script",
        ]

        if sys.platform.startswith("linux"):
            index = args.index("--network")
            args[index:index] = ["--add-host", "host.docker.internal:host-gateway"]
        return args

    def helper_args(self, helper):
        contract = HELPERS[helper]
        args = ["run", "--rm", "--network", NETWORK_ID]
        if sys.platform.startswith("linux"):
            args.extend(["--add-host", "host.docker.internal:host-gateway"])
        for key in contract["keys"]:
            args.extend(["-e", key])
        args.extend([
            "--label", "com.supabase.cli.project=" + PROJECT_ID,
            "--label", "com.docker.compose.project=" + PROJECT_ID,
            contract["image"], *contract["command"],
        ])
        return args

    def invoke(self, args, extra_env=None):
        env = self.env.copy()
        if extra_env:
            env.update(extra_env)
        return subprocess.run(
            [sys.executable, str(GUARD), *args], env=env,
            check=False, capture_output=True, text=True,
        )

    def docker_events(self):
        if not self.events.exists():
            return []
        return [json.loads(line)["args"] for line in self.events.read_text().splitlines()]

    def create_volume(self):
        result = self.invoke([
            "volume", "create", "--label", "com.supabase.cli.project=" + PROJECT_ID,
            "--label", "com.docker.compose.project=" + PROJECT_ID, self.db_name,
        ])
        self.assertEqual(result.returncode, 0, result.stderr)

    def prime_db(self):
        self.create_volume()
        result = self.invoke(self.db_create_args())
        self.assertEqual(result.returncode, 0, result.stderr)
        for args in (["cp", "-", DB_ID + ":/"], ["start", DB_ID]):
            result = self.invoke(args)
            self.assertEqual(result.returncode, 0, result.stderr)
        self.events.write_text("")

    def db_fixture(self):
        return {"Id": DB_ID, "Name": "/" + self.db_name,
                "State": {"Status": "created", "Running": False},
                "Config": {"Image": POSTGRES_IMAGE, "Labels": {
                    "com.supabase.cli.project": PROJECT_ID, "com.docker.compose.project": PROJECT_ID,
                    "com.supabase.cli.workdir": str(self.workdir)}},
                "HostConfig": {"NetworkMode": NETWORK_ID, "PortBindings": {"5432/tcp": [{"HostIp": "127.0.0.1", "HostPort": "56432"}]}},
                "NetworkSettings": {"Networks": {NETWORK_NAME: {"NetworkID": ""}}, "Ports": {}},
                "Mounts": [{"Type": "volume", "Name": self.db_name, "Destination": "/var/lib/postgresql/data", "RW": True}]}

    def prepare_created_db(self):
        self.create_volume()
        result = self.invoke(self.db_create_args())
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_pre_start_accepts_empty_and_exact_attachment_id(self):
        self.prepare_created_db()
        ledger = self.workdir / ".product-docker-guard" / "state.json"
        saved = ledger.read_text()
        for network_id in ("", NETWORK_ID):
            with self.subTest(network_id=network_id):
                ledger.write_text(saved)
                value = self.db_fixture()
                value["NetworkSettings"]["Networks"][NETWORK_NAME]["NetworkID"] = network_id
                result = self.invoke(["cp", "-", DB_ID + ":/"], {"FAKE_DB_INSPECT_JSON": json.dumps(value)})
                self.assertEqual(result.returncode, 0, result.stderr)

    def test_pre_start_rejects_invalid_lifecycle_network_and_common_ownership(self):
        self.prepare_created_db()
        ledger = self.workdir / ".product-docker-guard" / "state.json"
        saved = ledger.read_text()
        for mutation in ("mode", "name", "id", "missing", "null", "type", "extra", "malformed", "status", "running", "no_state", "image", "labels", "mount", "publish", "identity"):
            with self.subTest(mutation=mutation):
                ledger.write_text(saved)
                value = self.db_fixture()
                networks = value["NetworkSettings"]["Networks"]
                networks[NETWORK_NAME]["NetworkID"] = NETWORK_ID
                if mutation == "mode": value["HostConfig"]["NetworkMode"] = "foreign"
                elif mutation == "name": value["NetworkSettings"]["Networks"] = {NETWORK_ID: {"NetworkID": NETWORK_ID}}
                elif mutation == "id": networks[NETWORK_NAME]["NetworkID"] = "f" * 64
                elif mutation == "missing": networks[NETWORK_NAME] = {}
                elif mutation == "null": networks[NETWORK_NAME]["NetworkID"] = None
                elif mutation == "type": networks[NETWORK_NAME]["NetworkID"] = 1
                elif mutation == "extra": networks["foreign"] = {"NetworkID": NETWORK_ID}
                elif mutation == "malformed": networks[NETWORK_NAME] = []
                elif mutation == "status": value["State"]["Status"] = "exited"
                elif mutation == "running": value["State"]["Running"] = True
                elif mutation == "no_state": del value["State"]
                elif mutation == "image": value["Config"]["Image"] = "foreign"
                elif mutation == "labels": value["Config"]["Labels"] = {}
                elif mutation == "mount": value["Mounts"] = []
                elif mutation == "publish": value["HostConfig"]["PortBindings"] = {}
                elif mutation == "identity": value["Id"] = "f" * 64
                self.events.write_text("")
                sidecar = ledger.parent / "rejections.jsonl"
                sidecar.unlink(missing_ok=True)
                result = self.invoke(["cp", "-", DB_ID + ":/"], {"FAKE_DB_INSPECT_JSON": json.dumps(value)})
                self.assertEqual(result.returncode, 125, result.stderr)
                self.assertFalse(any(args[0] == "cp" for args in self.docker_events()))
                if mutation in ("name", "status", "running", "no_state"):
                    code = "DB_INSPECT_NETWORK_ATTACHMENT_NAME_REJECTED" if mutation == "name" else "DB_INSPECT_PRE_START_STATE_REJECTED"
                    self.assertEqual(json.loads(sidecar.read_text()), {"phase": "start", "code": code})

    def test_post_start_attachment_is_verified_before_ledger_success(self):
        self.prepare_created_db()
        self.assertEqual(self.invoke(["cp", "-", DB_ID + ":/"]).returncode, 0)
        result = self.invoke(["start", DB_ID], {"FAKE_POST_START_BAD": "f" * 64})
        self.assertEqual(result.returncode, 125, result.stderr)
        self.assertIn(["start", DB_ID], self.docker_events())
        ledger = json.loads((self.workdir / ".product-docker-guard" / "state.json").read_text())
        self.assertFalse(ledger["started"]["start"])

    def test_failed_start_preserves_exit_without_marking_started(self):
        self.prepare_created_db()
        self.assertEqual(self.invoke(["cp", "-", DB_ID + ":/"]).returncode, 0)
        result = self.invoke(["start", DB_ID], {"FAKE_START_EXIT": "47"})
        self.assertEqual(result.returncode, 47, result.stderr)
        ledger = json.loads((self.workdir / ".product-docker-guard" / "state.json").read_text())
        self.assertFalse(ledger["started"]["start"])

    def test_context_requires_matching_generated_network_name(self):
        for name in ("", "token-planet-ci-net-" + "f" * 24, NETWORK_ID):
            with self.subTest(name=name):
                result = self.invoke(["image", "inspect", POSTGRES_IMAGE], {"TOKEN_PLANET_GUARD_NETWORK_NAME": name})
                self.assertEqual(result.returncode, 125, result.stderr)

    def test_reset_and_diagnostics_require_strict_named_attachment(self):
        self.prime_db()
        for networks in ({NETWORK_NAME: {"NetworkID": ""}}, {NETWORK_ID: {"NetworkID": NETWORK_ID}}):
            with self.subTest(networks=networks):
                value = self.db_fixture()
                value["NetworkSettings"]["Networks"] = networks
                extra = {"FAKE_DB_INSPECT_JSON": json.dumps(value)}
                result = self.invoke(["container", "rm", "-f", DB_ID], {**extra, "TOKEN_PLANET_GUARD_PHASE": "reset"})
                self.assertEqual(result.returncode, 125, result.stderr)
                db = next(row for row in self.diagnostic_rows(extra) if "lookup" in row)
                self.assertEqual(db["lookup"], "ownership_unverified")

    def diagnostic_rows(self, extra=None):
        env = {"TOKEN_PLANET_GUARD_PHASE": "diagnostic"}
        if extra:
            env.update(extra)
        result = self.invoke(["--diagnose-start"], env)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("password-JWT-log-sentinel", result.stdout + result.stderr)
        return [json.loads(line) for line in result.stdout.splitlines()]

    def test_start_diagnostic_requires_recorded_id_and_exposes_only_owned_safe_state(self):
        self.prime_db()
        rows = self.diagnostic_rows({"FAKE_DB_MUTATION": "publish"})
        db = next(row for row in rows if "lookup" in row)
        self.assertEqual(db, {"lookup": "owned", "state": "exited", "health": "unhealthy", "exit_code": 17})
        self.assertEqual(self.docker_events(), [["container", "inspect", DB_ID, "--format", "{{json .}}"]])
        self.assertNotIn(DB_ID, json.dumps(rows))
        self.assertNotIn(str(self.workdir), json.dumps(rows))

    def test_start_diagnostic_fixed_lookup_outcomes_hide_unverified_metadata(self):
        self.prime_db()
        cases = (({"FAKE_DIAGNOSTIC_LOOKUP": "absent"}, "absent"),
                 ({"FAKE_DIAGNOSTIC_LOOKUP": "inspect_failed"}, "inspect_failed"),
                 ({"FAKE_DIAGNOSTIC_LOOKUP": "inspect_invalid"}, "inspect_invalid"),
                 ({"FAKE_DB_FOREIGN": "1"}, "ownership_unverified"))
        for extra, lookup in cases:
            with self.subTest(lookup=lookup):
                rows = self.diagnostic_rows(extra)
                self.assertEqual(next(row for row in rows if "lookup" in row), {
                    "lookup": lookup, "state": "unknown", "health": "unknown", "exit_code": None,
                })

    def test_start_diagnostic_bounds_inspect_time_and_both_output_streams(self):
        self.prime_db()
        for mode in ("timeout", "oversize", "stderr_oversize"):
            with self.subTest(mode=mode):
                started = time.monotonic()
                rows = self.diagnostic_rows({"FAKE_DIAGNOSTIC_LOOKUP": mode})
                self.assertLess(time.monotonic() - started, 5)
                self.assertEqual(next(row for row in rows if "lookup" in row), {
                    "lookup": "inspect_failed", "state": "unknown", "health": "unknown", "exit_code": None,
                })

    def test_start_diagnostic_unrecorded_container_does_not_inspect(self):
        rows = self.diagnostic_rows()
        self.assertEqual(next(row for row in rows if "lookup" in row)["lookup"], "container_unrecorded")
        self.assertEqual(self.docker_events(), [])

    def test_start_diagnostic_normalizes_state_health_and_exit_type_boundaries(self):
        self.prime_db()
        cases = [({"Status": "password-JWT-log-sentinel", "ExitCode": True, "Health": {"Status": ["healthy"]}}, "unknown", "unknown", None),
                 ({"Status": "running", "ExitCode": "password-JWT-log-sentinel"}, "running", "none", None),
                 ({"Status": "paused", "ExitCode": -2, "Health": {"Status": "starting"}}, "paused", "starting", -2)]
        for state, status, health, exit_code in cases:
            with self.subTest(state=state):
                rows = self.diagnostic_rows({"FAKE_DIAGNOSTIC_STATE": json.dumps(state)})
                self.assertEqual(next(row for row in rows if "lookup" in row), {
                    "lookup": "owned", "state": status, "health": health, "exit_code": exit_code,
                })

    def test_unapproved_operation_family_preserves_only_closed_enum(self):
        cases = [(["logs", "password-JWT-log-sentinel"], "LOGS"),
                 (["container", "logs", "password-JWT-log-sentinel"], "CONTAINER_LOGS"),
                 (["container", "wait", "password-JWT-log-sentinel"], "CONTAINER_WAIT"),
                 (["network", "disconnect", "password-JWT-log-sentinel"], "NETWORK_DISCONNECT"),
                 (["container", "password-JWT-log-sentinel"], "CONTAINER_OTHER"),
                 (["password-JWT-log-sentinel", "resource-sentinel"], "UNKNOWN")]
        for args, operation in cases:
            with self.subTest(operation=operation):
                sidecar = self.workdir / ".product-docker-guard" / "rejections.jsonl"
                sidecar.unlink(missing_ok=True)
                self.events.write_text("")
                result = self.invoke(args)
                self.assertEqual(result.returncode, 125)
                expected = {"phase": "start", "code": "DOCKER_COMMAND_IS_NOT_APPROVED", "operation": operation}
                self.assertEqual(json.loads(sidecar.read_text()), expected)
                self.assertIn(expected, self.diagnostic_rows())
                self.assertEqual(self.docker_events(), [])
                for sentinel in ("password-JWT-log-sentinel", "resource-sentinel"):
                    self.assertNotIn(sentinel, sidecar.read_text() + result.stdout + result.stderr)

    def test_operation_schema_accepts_only_generic_rejection_fixed_enum(self):
        script = "import json, sys; from product_docker_guard import normalize_diagnostics; print(json.dumps(normalize_diagnostics(sys.argv[1].encode())))"
        good = {"phase": "start", "code": "DOCKER_COMMAND_IS_NOT_APPROVED", "operation": "CONTAINER_LOGS"}
        invalid = [dict(good, operation="password-JWT-log-sentinel"), dict(good, operation=True),
                   dict(good, code="GUARD_IO_FAILED"), dict(good, argv="password-JWT-log-sentinel")]
        for row, expected in [(good, [good])] + [(row, [{"phase": "start", "code": "EVIDENCE_INVALID"}]) for row in invalid]:
            with self.subTest(row=row):
                result = subprocess.run([sys.executable, "-c", script, json.dumps(row) + "\n"], cwd=GUARD.parent, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0)
                self.assertEqual(json.loads(result.stdout), expected)
                self.assertNotIn("password-JWT-log-sentinel", result.stdout + result.stderr)

    def test_start_diagnostic_exports_only_normalized_rejection_codes(self):
        rejected = self.invoke(["pull", "password-JWT-log-sentinel"])
        self.assertEqual(rejected.returncode, 125)
        rows = self.diagnostic_rows()
        self.assertIn({"phase": "start", "code": "DOCKER_COMMAND_IS_NOT_APPROVED", "operation": "PULL"}, rows)
        self.assertNotIn("password-JWT-log-sentinel", json.dumps(rows))
        sidecar = self.workdir / ".product-docker-guard" / "rejections.jsonl"
        sidecar.write_text('{"phase":"start","code":"password-JWT-log-sentinel"}\n')
        rows = self.diagnostic_rows()
        self.assertIn({"phase": "start", "code": "EVIDENCE_INVALID"}, rows)

    def test_diagnostic_evidence_corruption_permissions_and_symlink_are_fixed_only(self):
        self.invoke(["pull", "password-JWT-log-sentinel"])
        sidecar = self.workdir / ".product-docker-guard" / "rejections.jsonl"
        outside = self.root / "outside-sidecar"
        outside.write_text("password-JWT-log-sentinel")
        for case in ("symlink", "hardlink", "oversize", "corrupt", "permissions"):
            with self.subTest(case=case):
                sidecar.unlink(missing_ok=True)
                if case == "symlink":
                    sidecar.symlink_to(outside)
                elif case == "hardlink":
                    os.link(outside, sidecar)
                else:
                    sidecar.write_text("x" * 8193 if case == "oversize" else "password-JWT-log-sentinel")
                    sidecar.chmod(0o644 if case == "permissions" else 0o600)
                self.assertIn({"phase": "start", "code": "EVIDENCE_INVALID"}, self.diagnostic_rows())
        self.assertEqual(outside.read_text(), "password-JWT-log-sentinel")

    def test_unknown_rejection_and_io_failure_never_export_exception_text(self):
        for failure, code in (("guard.reject('password-JWT-log-sentinel')", "GUARD_REJECTION_UNKNOWN"),
                              ("(_ for _ in ()).throw(OSError('password-JWT-log-sentinel'))", "GUARD_IO_FAILED")):
            with self.subTest(code=code):
                sidecar = self.workdir / ".product-docker-guard" / "rejections.jsonl"
                sidecar.unlink(missing_ok=True)
                script = "import sys; import product_docker_guard as guard; guard.handle_command=lambda *args: " + failure + "; sys.exit(guard.main(['invalid']))"
                result = subprocess.run([sys.executable, "-c", script], env=self.env, cwd=GUARD.parent, capture_output=True, text=True)
                self.assertEqual(result.returncode, 125)
                self.assertNotIn("password-JWT-log-sentinel", result.stdout + result.stderr + sidecar.read_text())
                self.assertEqual(json.loads(sidecar.read_text()), {"phase": "start", "code": code})

    def test_unsafe_context_and_sidecar_directory_omit_evidence_without_outside_write(self):
        outside = self.root / "outside-directory"
        outside.mkdir()
        sentinel = outside / "rejections.jsonl"
        sentinel.write_text("password-JWT-log-sentinel")
        (self.workdir / ".product-docker-guard").symlink_to(outside)
        result = self.invoke(["pull", "password-JWT-log-sentinel"])
        self.assertEqual(result.returncode, 125)
        self.assertEqual(sentinel.read_text(), "password-JWT-log-sentinel")
        rows = self.diagnostic_rows({"TOKEN_PLANET_GUARD_PROJECT_ID": "invalid"})
        self.assertIn({"phase": "start", "code": "EVIDENCE_UNAVAILABLE"}, rows)
        self.assertEqual(sentinel.read_text(), "password-JWT-log-sentinel")

    def test_runner_normalizer_rejects_extra_fields_unknown_enum_and_boolean_exit(self):
        records = [
            {"phase": "start", "code": "password-JWT-log-sentinel"},
            {"phase": "foreign", "code": "EVIDENCE_INVALID"},
            {"phase": "start", "code": "EVIDENCE_INVALID", "error": "password-JWT-log-sentinel"},
            {"lookup": "owned", "state": "running", "health": "healthy", "exit_code": True},
            {"lookup": "owned", "state": "password-JWT-log-sentinel", "health": "healthy", "exit_code": 0},
            {"lookup": "ownership_unverified", "state": "running", "health": "healthy", "exit_code": 0},
        ]
        script = "import json, sys; from product_docker_guard import normalize_diagnostics; print(json.dumps(normalize_diagnostics(sys.argv[1].encode())))"
        for record in records:
            with self.subTest(record=record):
                result = subprocess.run([sys.executable, "-c", script, json.dumps(record) + "\n"], cwd=GUARD.parent, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(json.loads(result.stdout), [{"phase": "start", "code": "EVIDENCE_INVALID"}])
                self.assertNotIn("password-JWT-log-sentinel", result.stdout + result.stderr)

    def test_diagnostic_ownership_dimensions_and_malformed_state_hide_metadata(self):
        self.prime_db()
        for mutation in ("image", "network", "mount"):
            with self.subTest(mutation=mutation):
                rows = self.diagnostic_rows({"FAKE_DB_MUTATION": mutation})
                self.assertEqual(next(row for row in rows if "lookup" in row), {
                    "lookup": "ownership_unverified", "state": "unknown", "health": "unknown", "exit_code": None,
                })
        rows = self.diagnostic_rows({"FAKE_DIAGNOSTIC_STATE": json.dumps("password-JWT-log-sentinel")})
        self.assertEqual(next(row for row in rows if "lookup" in row)["lookup"], "inspect_invalid")

    def test_evidence_parent_symlink_swap_cannot_write_outside_validated_directory(self):
        outside = self.root / "outside-race"
        outside.mkdir()
        sink = outside / "rejections.jsonl"
        sink.write_text("")
        sink.chmod(0o600)
        script = """
import os, pathlib, sys
import product_docker_guard as guard
real_open = os.open
swapped = False
def race_open(path, *args, **kwargs):
    global swapped
    if pathlib.Path(path).name == "rejections.jsonl" and not swapped:
        swapped = True
        directory = pathlib.Path(os.environ["TOKEN_PLANET_GUARD_WORKDIR"]) / ".product-docker-guard"
        directory.rename(directory.with_name("saved-guard"))
        directory.symlink_to(sys.argv[1])
    return real_open(path, *args, **kwargs)
guard.os.open = race_open
sys.exit(guard.main(["invalid"]))
"""
        result = subprocess.run([sys.executable, "-c", script, str(outside)], env=self.env,
                                cwd=GUARD.parent, capture_output=True, text=True)
        self.assertEqual(result.returncode, 125, result.stderr)
        self.assertEqual(sink.read_text(), "")

    def test_every_rejection_maps_to_private_fixed_evidence(self):
        sidecar = self.workdir / ".product-docker-guard" / "rejections.jsonl"
        for message, code in EXPECTED_REJECTION_CODES.items():
            with self.subTest(message=message):
                sidecar.unlink(missing_ok=True)
                script = ("import sys; import product_docker_guard as guard; "
                          "guard.handle_command=lambda *args: guard.reject(sys.argv[1]); "
                          "sys.exit(guard.main(['invalid']))")
                result = subprocess.run([sys.executable, "-c", script, message], env=self.env,
                                        cwd=GUARD.parent, capture_output=True, text=True)
                self.assertEqual(result.returncode, 125, result.stderr)
                self.assertTrue(sidecar.is_file(), "guard rejection evidence was not written")
                expected = {"phase": "start", "code": code}
                if code == "DOCKER_COMMAND_IS_NOT_APPROVED":
                    expected["operation"] = "UNKNOWN"
                self.assertEqual(json.loads(sidecar.read_text()), expected)
                self.assertEqual(sidecar.stat().st_mode & 0o777, 0o600)
                self.assertNotIn(message, sidecar.read_text())

    def test_rejection_evidence_sink_errors_never_change_rejection_exit(self):
        directory = self.workdir / ".product-docker-guard"
        directory.mkdir(mode=0o700)
        sidecar = directory / "rejections.jsonl"
        outside = self.root / "outside-evidence"
        outside.write_text("password-JWT-log-sentinel")
        for case in ("symlink", "hardlink", "oversize", "corrupt", "permissions"):
            with self.subTest(case=case):
                sidecar.unlink(missing_ok=True)
                if case == "symlink":
                    sidecar.symlink_to(outside)
                elif case == "hardlink":
                    os.link(outside, sidecar)
                else:
                    sidecar.write_text("x" * 8193 if case == "oversize" else "password-JWT-log-sentinel")
                    sidecar.chmod(0o644 if case == "permissions" else 0o600)
                before = sidecar.read_bytes()
                result = self.invoke(["pull", "password-JWT-log-sentinel"])
                self.assertEqual(result.returncode, 125, result.stderr)
                self.assertEqual(sidecar.read_bytes(), before)
                self.assertEqual(outside.read_text(), "password-JWT-log-sentinel")

    def test_unapproved_mutations_and_global_options_never_reach_docker(self):
        bad_commands = [
            ["pull", "unapproved/image:latest"],
            ["image", "pull", POSTGRES_IMAGE],
            ["container", "prune", "--force"],
            ["volume", "prune", "--force"],
            ["network", "create", "foreign-network"],
            ["--host", "tcp://foreign.example", "ps"],
            ["run", "--rm", "foreign/image", "sh"],
            ["arbitrary", "command"],
        ]
        for args in bad_commands:
            with self.subTest(args=args):
                if self.events.exists():
                    self.events.unlink()
                result = self.invoke(args)
                self.assertEqual(result.returncode, 125, result.stderr)
                self.assertEqual(self.docker_events(), [])

    def test_only_exact_cached_image_candidates_are_inspected(self):
        allowed = [candidate for helper in HELPERS.values() for candidate in helper["candidates"]] + [POSTGRES_IMAGE]
        for image in allowed:
            with self.subTest(image=image):
                result = self.invoke(["image", "inspect", image])
                self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(self.docker_events()), len(allowed))
        self.events.write_text("")

        result = self.invoke(["image", "inspect", "unapproved/image:latest"])

        self.assertEqual(result.returncode, 125, result.stderr)
        self.assertEqual(self.docker_events(), [])

    def test_db_create_is_pinned_cached_and_loopback_only(self):
        self.create_volume()
        self.events.write_text("")
        result = self.invoke(self.db_create_args())

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, DB_ID + "\n")
        forwarded = next(args for args in self.docker_events() if args[:1] == ["create"])
        self.assertEqual(forwarded[0], "create")
        self.assertEqual(forwarded.count("--pull=never"), 1)
        self.assertIn("127.0.0.1:56432:5432", forwarded)
        self.assertNotIn("56432:5432", [value for value in forwarded if value != "127.0.0.1:56432:5432"])

    def test_db_create_accepts_only_pinned_postgres_socket_host(self):
        self.create_volume()
        state_file = self.workdir / ".product-docker-guard" / "state.json"
        saved = state_file.read_text()
        self.events.write_text("")
        for host in (self.db_name, "localhost", "/foreign/socket"):
            with self.subTest(host=host):
                state_file.write_text(saved)
                self.events.write_text("")
                rejected = self.invoke(self.db_create_args(), {"POSTGRES_HOST": host})
                self.assertEqual(rejected.returncode, 125, rejected.stderr)
                self.assertEqual(self.docker_events(), [])
        state_file.write_text(saved)
        accepted = self.invoke(self.db_create_args(), {"POSTGRES_HOST": "/var/run/postgresql"})
        self.assertEqual(accepted.returncode, 0, accepted.stderr)
        self.assertEqual(accepted.stdout.strip(), DB_ID)

    def test_db_create_with_unapproved_publish_or_security_option_is_rejected(self):
        for mutation in ("0.0.0.0:56432:5432", "--security-opt"):
            args = self.db_create_args()
            if mutation.startswith("0."):
                args[args.index("56432:5432")] = mutation
            else:
                args.insert(-3, mutation)
                args.insert(-3, "no-new-privileges")
            with self.subTest(mutation=mutation):
                result = self.invoke(args)
                self.assertEqual(result.returncode, 125, result.stderr)
        self.assertEqual(self.docker_events(), [])

    def test_container_inspect_requires_exact_generated_target_and_state_format(self):
        self.prime_db()
        allowed = self.invoke([
            "container", "inspect", self.db_name, "--format", "{{json .State}}",
        ])
        self.assertEqual(allowed.returncode, 0, allowed.stderr)

        self.events.write_text("")
        bad_commands = [
            ["container", "inspect", "foreign-container", "--format", "{{json .State}}"],
            ["container", "inspect", self.db_name, "--format", "{{json .}}"],
            ["inspect", self.db_name],
        ]
        for args in bad_commands:
            with self.subTest(args=args):
                result = self.invoke(args)
                self.assertEqual(result.returncode, 125, result.stderr)
        self.assertEqual(self.docker_events(), [])

    def test_reset_absent_excluded_service_restarts_return_tolerated_not_found_without_mutation(self):
        for suffix in ("storage", "auth", "realtime", "pooler"):
            with self.subTest(suffix=suffix):
                self.events.write_text("")
                target = "supabase_" + suffix + "_" + PROJECT_ID
                result = self.invoke(["restart", target], {"TOKEN_PLANET_GUARD_PHASE": "reset"})
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertEqual(result.stdout, "")
                self.assertEqual(result.stderr, "Error response from daemon: No such container: " + target + "\n")
                self.assertEqual(self.docker_events(), [["container", "inspect", target, "--format", "{{json .State}}"]])

    def test_reset_excluded_restart_accepts_exact_daemon_absence_without_mutation(self):
        for suffix in ("storage", "auth", "realtime", "pooler"):
            with self.subTest(suffix=suffix):
                self.events.write_text("")
                target = "supabase_" + suffix + "_" + PROJECT_ID
                stderr = "Error response from daemon: No such container: " + target + "\n"
                result = self.invoke(["restart", target], {"TOKEN_PLANET_GUARD_PHASE": "reset",
                    "FAKE_EXCLUDED_ABSENCE": json.dumps([1, "\n", stderr])})
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertEqual(result.stdout, "")
                self.assertEqual(result.stderr, stderr)
                self.assertEqual(self.docker_events(), [["container", "inspect", target, "--format", "{{json .State}}"]])

    def test_reset_excluded_restart_absence_near_misses_fail_closed(self):
        target = "supabase_storage_" + PROJECT_ID
        daemon = "Error response from daemon: No such container: " + target + "\n"
        old = "Error: No such object: " + target + "\n"
        cases = ((1, "", daemon), (1, "\n\n", daemon), (1, " \n", daemon),
                 (1, "\nextra", daemon), (1, "\n", daemon.replace(target, "foreign")),
                 (1, "\n", daemon + "diagnostic\n"), (1, "\n", daemon + "x"),
                 (1, "\n", daemon.rstrip("\n")), (1, "\n", "Cannot connect to Docker daemon\n"),
                 (0, "\n", daemon), (2, "\n", daemon), (1, "\n", old), (1, "", old + "x"))
        for response in cases:
            with self.subTest(response=response):
                self.events.write_text("")
                result = self.invoke(["restart", target], {"TOKEN_PLANET_GUARD_PHASE": "reset",
                    "FAKE_EXCLUDED_ABSENCE": json.dumps(response)})
                self.assertEqual(result.returncode, 125, result.stderr)
                self.assertEqual(self.docker_events(), [["container", "inspect", target, "--format", "{{json .State}}"]])

    def test_reset_existing_excluded_service_or_inspection_failure_is_fail_closed(self):
        for mode in ("stopped", "running", "inspect-error"):
            with self.subTest(mode=mode):
                self.events.write_text("")
                result = self.invoke(["restart", "supabase_auth_" + PROJECT_ID], {
                    "TOKEN_PLANET_GUARD_PHASE": "reset", "FAKE_EXCLUDED_SERVICE": mode,
                })
                self.assertEqual(result.returncode, 125, result.stderr)
                self.assertNotRegex(result.stderr.lower(), r"no such container|no such object|no container with name or id")
                self.assertFalse(any(args[:1] == ["restart"] for args in self.docker_events()))

    def test_reset_kong_inspection_is_read_only_and_exec_is_denied(self):
        target = "supabase_kong_" + PROJECT_ID
        for mode, status in (("absent", 1), ("stopped", 0), ("running", 0)):
            with self.subTest(mode=mode):
                self.events.write_text("")
                args = ["container", "inspect", target, "--format", "{{json .State}}"]
                inspected = self.invoke(args, {"TOKEN_PLANET_GUARD_PHASE": "reset", "FAKE_EXCLUDED_SERVICE": mode})
                self.assertEqual(inspected.returncode, status, inspected.stderr)
                self.assertEqual(self.docker_events(), [args])
                denied = self.invoke(["exec", target, "kong", "reload", "--nginx-conf", "/home/kong/custom_nginx.template"],
                                     {"TOKEN_PLANET_GUARD_PHASE": "reset"})
                self.assertEqual(denied.returncode, 125, denied.stderr)
                self.assertEqual(self.docker_events(), [args])

    def test_excluded_service_contract_rejects_foreign_names_aliases_options_and_start_phase(self):
        target = "supabase_auth_" + PROJECT_ID
        for args in (["restart", "foreign"], ["restart", target, "extra"], ["container", "restart", target],
                     ["restart", "supabase_db_" + PROJECT_ID], ["container", "inspect", "supabase_kong_" + PROJECT_ID, "--format", "{{json .}}"]):
            with self.subTest(args=args):
                denied = self.invoke(args, {"TOKEN_PLANET_GUARD_PHASE": "reset"})
                self.assertEqual(denied.returncode, 125, denied.stderr)
        denied = self.invoke(["restart", target])
        self.assertEqual(denied.returncode, 125, denied.stderr)
        self.assertEqual(self.docker_events(), [])

    def test_reset_accepts_only_exact_generated_db_name_after_ownership_verification(self):
        self.prime_db()
        self.env["TOKEN_PLANET_GUARD_PHASE"] = "reset"
        for target in ("foreign-container", self.db_name + "-foreign"):
            with self.subTest(target=target):
                denied = self.invoke(["container", "rm", "-f", target])
                self.assertEqual(denied.returncode, 125, denied.stderr)
                self.assertEqual(self.docker_events(), [])
        foreign = self.invoke(["container", "rm", "-f", self.db_name], {"FAKE_DB_FOREIGN": "1"})
        self.assertEqual(foreign.returncode, 125, foreign.stderr)
        self.assertFalse(any(args[:2] == ["container", "rm"] for args in self.docker_events()))
        self.events.write_text("")
        removed = self.invoke(["container", "rm", "-f", self.db_name])
        self.assertEqual(removed.returncode, 0, removed.stderr)
        inspections = [args for args in self.docker_events() if args[:2] == ["container", "inspect"]]
        self.assertEqual(inspections, [["container", "inspect", DB_ID, "--format", "{{json .}}"]])
        self.assertIn(["container", "rm", "-f", DB_ID], self.docker_events())
        self.assertNotIn(["container", "rm", "-f", self.db_name], self.docker_events())

    def test_reset_removes_only_recorded_container_and_exact_volume(self):
        self.prime_db()
        env = self.env.copy()
        env["TOKEN_PLANET_GUARD_PHASE"] = "reset"
        self.env = env

        result = self.invoke(["container", "rm", "-f", DB_ID])
        self.assertEqual(result.returncode, 0, result.stderr)
        volume = self.invoke(["volume", "rm", "-f", self.db_name])
        self.assertEqual(volume.returncode, 0, volume.stderr)

        self.events.write_text("")
        for args in (["container", "rm", "-f", "foreign"], ["volume", "rm", "-f", "foreign-volume"]):
            with self.subTest(args=args):
                denied = self.invoke(args)
                self.assertEqual(denied.returncode, 125, denied.stderr)
        self.assertEqual(self.docker_events(), [])

    def test_exact_helpers_run_in_realtime_storage_auth_order_cache_only(self):
        self.prime_db()

        for helper in ("realtime", "storage", "auth"):
            with self.subTest(helper=helper):
                result = self.invoke(self.helper_args(helper))
                self.assertEqual(result.returncode, 0, result.stderr)

        runs = [args for args in self.docker_events() if args[:1] == ["run"]]
        self.assertEqual(len(runs), 3)
        self.assertEqual([args[-len(HELPERS[name]["command"]) - 1] for args, name in zip(runs, ("realtime", "storage", "auth"))], [HELPERS[name]["image"] for name in ("realtime", "storage", "auth")])
        for args in runs:
            self.assertEqual(args.count("--pull=never"), 1)
            self.assertEqual(args.count("--cidfile"), 1)
            self.assertNotIn("--mount", args)
            self.assertNotIn("-p", args)
        secrets = ("db-secret-fixture", "api-secret-fixture", "jwt-secret-fixture", "anon-fixture")
        event_text = self.events.read_text()
        for secret in secrets:
            self.assertNotIn(secret, event_text)

    def test_helper_run_rejects_wrong_environment_image_command_and_run_alias(self):
        self.prime_db()
        valid = self.helper_args("realtime")
        cases = []
        missing_key = valid.copy()
        missing_key.remove("MAX_HEADER_LENGTH")
        cases.append(missing_key)
        wrong_image = valid.copy()
        wrong_image[wrong_image.index(HELPERS["realtime"]["image"])] = "unapproved/image:latest"
        cases.append(wrong_image)
        wrong_command = valid.copy()
        wrong_command[-1] = "unexpected-command"
        cases.append(wrong_command)
        cases.append(["container", *valid])
        for args in cases:
            with self.subTest(args=args):
                self.events.write_text("")
                result = self.invoke(args)
                self.assertEqual(result.returncode, 125, result.stderr)
                self.assertEqual(self.docker_events(), [])

    def test_helper_cidfile_requires_valid_owned_container_before_exact_remove(self):
        self.prime_db()
        result = self.invoke(self.helper_args("realtime"), {"FAKE_HELPER_RETAIN": "1"})

        self.assertEqual(result.returncode, 0, result.stderr)
        events = self.docker_events()
        removals = [args for args in events if args[:3] == ["container", "rm", "-f"]]
        self.assertEqual(removals, [["container", "rm", "-f", HELPER_ID]])

    def test_helper_cleanup_rejects_foreign_network_image_mount_and_ports(self):
        self.prime_db()
        state_file = self.workdir / ".product-docker-guard" / "state.json"
        saved = state_file.read_text()
        for mutation in ("network", "image", "mount", "ports"):
            with self.subTest(mutation=mutation):
                self.events.write_text("")
                result = self.invoke(self.helper_args("realtime"), {
                    "FAKE_HELPER_RETAIN": "1", "FAKE_HELPER_MUTATION": mutation,
                })
                self.assertEqual(result.returncode, 125, result.stderr)
                self.assertFalse(any(args[:2] == ["container", "rm"] for args in self.docker_events()))
                state_file.write_text(saved)
                for cidfile in (state_file.parent / "cidfiles").glob("*.cid"):
                    cidfile.unlink()

    def test_helper_failure_preserves_original_status_and_stderr(self):
        self.prime_db()
        result = self.invoke(self.helper_args("realtime"), {"FAKE_HELPER_EXIT": "1"})
        self.assertEqual(result.returncode, 44, result.stderr)
        self.assertIn("fake helper failure", result.stderr)

    def test_db_inspect_contract_categories_deny_without_secret_export(self):
        self.prime_db()
        fixture = {"Id": DB_ID, "Name": "/" + self.db_name,
                   "Config": {"Image": POSTGRES_IMAGE, "Labels": {
                       "com.supabase.cli.project": PROJECT_ID, "com.docker.compose.project": PROJECT_ID,
                       "com.supabase.cli.workdir": str(self.workdir)}},
                   "HostConfig": {"NetworkMode": NETWORK_ID, "PortBindings": {"5432/tcp": [{"HostIp": "127.0.0.1", "HostPort": "56432"}]}},
                   "NetworkSettings": {"Networks": {NETWORK_NAME: {"NetworkID": NETWORK_ID}}, "Ports": {}},
                   "Mounts": [{"Type": "volume", "Name": self.db_name, "Destination": "/var/lib/postgresql/data", "RW": True}]}
        cases = (("identity", "DB_INSPECT_IDENTITY_REJECTED"), ("image", "DB_INSPECT_IMAGE_REJECTED"),
                 ("labels", "DB_INSPECT_LABELS_REJECTED"), ("network", "DB_INSPECT_NETWORK_ATTACHMENT_ID_REJECTED"),
                 ("network_mode", "DB_INSPECT_NETWORK_MODE_REJECTED"),
                 ("network_absent", "DB_INSPECT_NETWORK_ATTACHMENT_COUNT_REJECTED"),
                 ("network_extra", "DB_INSPECT_NETWORK_ATTACHMENT_COUNT_REJECTED"),
                 ("network_malformed", "DB_INSPECT_NETWORK_ATTACHMENT_INVALID"),
                 ("network_missing_id", "DB_INSPECT_NETWORK_ATTACHMENT_INVALID"),
                 ("network_empty_list", "DB_INSPECT_NETWORK_ATTACHMENT_INVALID"),
                 ("network_nonmapping", "DB_INSPECT_NETWORK_ATTACHMENT_INVALID"),
                 ("network_multi_malformed", "DB_INSPECT_NETWORK_ATTACHMENT_INVALID"),
                 ("network_id_type", "DB_INSPECT_NETWORK_ATTACHMENT_INVALID"),
                 ("publish", "DB_INSPECT_PUBLISH_REJECTED"), ("mount", "DB_INSPECT_VOLUME_MOUNT_REJECTED"),
                 ("invalid", "DB_INSPECT_INVALID"), ("command", "DB_INSPECT_COMMAND_FAILED"))
        for category, code in cases:
            with self.subTest(category=category):
                value = json.loads(json.dumps(fixture))
                sentinel = "password-JWT-log-sentinel"
                if category == "identity": value["Id"] = sentinel
                elif category == "image": value["Config"]["Image"] = sentinel
                elif category == "labels": value["Config"]["Labels"]["com.docker.compose.project"] = sentinel
                elif category == "network": value["NetworkSettings"]["Networks"][NETWORK_NAME]["NetworkID"] = sentinel
                elif category == "network_mode": value["HostConfig"]["NetworkMode"] = sentinel
                elif category == "network_absent": value["NetworkSettings"]["Networks"] = {}
                elif category == "network_extra": value["NetworkSettings"]["Networks"][sentinel] = {"NetworkID": sentinel}
                elif category == "network_malformed": value["NetworkSettings"]["Networks"] = [sentinel]
                elif category == "network_missing_id": value["NetworkSettings"]["Networks"][NETWORK_NAME] = {}
                elif category == "network_empty_list": value["NetworkSettings"]["Networks"] = []
                elif category == "network_nonmapping": value["NetworkSettings"]["Networks"] = ""
                elif category == "network_multi_malformed": value["NetworkSettings"]["Networks"][sentinel] = {"unexpected": sentinel}
                elif category == "network_id_type": value["NetworkSettings"]["Networks"][NETWORK_NAME]["NetworkID"] = [sentinel]
                elif category == "publish": value["HostConfig"]["PortBindings"]["5432/tcp"][0]["HostIp"] = sentinel
                elif category == "mount": value["Mounts"][0]["Name"] = sentinel
                elif category == "invalid": value = sentinel
                sidecar = self.workdir / ".product-docker-guard" / "rejections.jsonl"
                sidecar.unlink(missing_ok=True)
                self.events.write_text("")
                result = self.invoke(["container", "rm", "-f", DB_ID], {
                    "TOKEN_PLANET_GUARD_PHASE": "reset", "FAKE_DB_INSPECT_JSON": json.dumps(value),
                    "FAKE_DB_INSPECT_EXIT": "7" if category == "command" else "0"})
                self.assertEqual(result.returncode, 125)
                self.assertEqual(json.loads(sidecar.read_text()), {"phase": "reset", "code": code})
                self.assertNotIn(sentinel, result.stdout + result.stderr + sidecar.read_text())
                self.assertFalse(any(args[:2] == ["container", "rm"] for args in self.docker_events()))
        result = self.invoke(["container", "rm", "-f", DB_ID], {
            "TOKEN_PLANET_GUARD_PHASE": "reset", "FAKE_DB_INSPECT_JSON": json.dumps(fixture)})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(["container", "rm", "-f", DB_ID], self.docker_events())

    def test_db_reset_rejects_wrong_image_network_mount_and_publish(self):
        self.prime_db()
        state_file = self.workdir / ".product-docker-guard" / "state.json"
        saved = state_file.read_text()
        for mutation in ("image", "network", "mount", "publish"):
            state_file.write_text(saved)
            with self.subTest(mutation=mutation):
                self.events.write_text("")
                result = self.invoke(["container", "rm", "-f", DB_ID], {
                    "TOKEN_PLANET_GUARD_PHASE": "reset", "FAKE_DB_MUTATION": mutation,
                })
                self.assertEqual(result.returncode, 125, result.stderr)
                self.assertFalse(any(args[:2] == ["container", "rm"] for args in self.docker_events()))

    def test_db_cp_requires_owned_container_before_secret_transfer(self):
        self.create_volume()
        created = self.invoke(self.db_create_args())
        self.assertEqual(created.returncode, 0, created.stderr)
        self.events.write_text("")
        result = self.invoke(["cp", "-", DB_ID + ":/"], {"FAKE_DB_FOREIGN": "1"})
        self.assertEqual(result.returncode, 125, result.stderr)
        self.assertFalse(any(args[:1] == ["cp"] for args in self.docker_events()))

    def test_cli_child_connection_override_never_reaches_real_docker(self):
        for key in ("DOCKER_HOST", "DOCKER_CONTEXT", "DOCKER_TLS_VERIFY", "DOCKER_CERT_PATH"):
            with self.subTest(key=key):
                self.events.write_text("")
                result = self.invoke(["image", "inspect", POSTGRES_IMAGE], {key: "foreign"})
                self.assertEqual(result.returncode, 125, result.stderr)
                self.assertEqual(self.docker_events(), [])

    def test_db_create_rejects_foreign_volume_before_container_creation(self):
        self.create_volume()
        self.events.write_text("")
        result = self.invoke(self.db_create_args(), {"FAKE_VOLUME_FOREIGN": "1"})
        self.assertEqual(result.returncode, 125, result.stderr)
        self.assertFalse(any(args[:1] == ["create"] for args in self.docker_events()))

    def test_db_start_requires_owned_container_before_start(self):
        self.create_volume()
        self.assertEqual(self.invoke(self.db_create_args()).returncode, 0)
        self.assertEqual(self.invoke(["cp", "-", DB_ID + ":/"]).returncode, 0)
        self.events.write_text("")
        result = self.invoke(["start", DB_ID], {"FAKE_DB_FOREIGN": "1"})
        self.assertEqual(result.returncode, 125, result.stderr)
        self.assertFalse(any(args[:1] == ["start"] for args in self.docker_events()))

    def pending_helper(self):
        self.prime_db()
        result = self.invoke(self.helper_args("realtime"), {
            "FAKE_HELPER_RETAIN": "1", "FAKE_HELPER_INSPECT_JSON": "[]",
        })
        self.assertEqual(result.returncode, 125, result.stderr)
        self.events.write_text("")

    def test_cleanup_mode_retries_exact_pending_helper(self):
        self.pending_helper()
        result = self.invoke(["--cleanup-helpers"], {"TOKEN_PLANET_GUARD_PHASE": "cleanup"})
        self.assertEqual(result.returncode, 0, result.stderr)
        removals = [args for args in self.docker_events() if args[:2] == ["container", "rm"]]
        self.assertEqual(removals, [["container", "rm", "-f", HELPER_ID]])
        self.events.write_text("")
        repeated = self.invoke(["--cleanup-helpers"], {"TOKEN_PLANET_GUARD_PHASE": "cleanup"})
        self.assertEqual(repeated.returncode, 0, repeated.stderr)
        self.assertEqual(self.docker_events(), [])

    def test_cleanup_preflights_all_pending_helpers_before_any_removal(self):
        self.pending_helper()
        second_id = "d" * 64
        docker_state = json.loads(self.state.read_text())
        first = docker_state["helper"]
        second = json.loads(json.dumps(first))
        second["Id"] = second_id
        second["Config"]["Image"] = HELPERS["storage"]["image"]
        second["Config"]["Labels"]["com.supabase.cli.project"] = "foreign-project"
        docker_state["helpers"] = {HELPER_ID: first, second_id: second}
        self.state.write_text(json.dumps(docker_state))

        state_file = self.workdir / ".product-docker-guard" / "state.json"
        ledger = json.loads(state_file.read_text())
        ledger["helpers_done"]["start"].append("storage")
        ledger["helper_records"].append({
            "phase": "start", "helper": "storage", "image": HELPERS["storage"]["image"], "cleaned": False,
        })
        state_file.write_text(json.dumps(ledger))
        (state_file.parent / "cidfiles" / "start-storage.cid").write_text(second_id + "\n")
        before = state_file.read_bytes()
        self.events.write_text("")

        result = self.invoke(["--cleanup-helpers"], {"TOKEN_PLANET_GUARD_PHASE": "cleanup"})

        self.assertEqual(result.returncode, 125, result.stderr)
        self.assertEqual(self.docker_events(), [
            ["container", "inspect", HELPER_ID, "--format", "{{json .}}"],
            ["container", "inspect", second_id, "--format", "{{json .}}"],
        ])
        self.assertFalse(any(args[:2] == ["container", "rm"] for args in self.docker_events()))
        self.assertEqual(state_file.read_bytes(), before)

    def test_inline_helper_accepts_exact_daemon_absence_tuple(self):
        self.prime_db()
        result = self.invoke(self.helper_args("realtime"), {"FAKE_HELPER_ABSENCE": json.dumps([
            1, "\n", "Error response from daemon: No such container: " + HELPER_ID + "\n"])})
        self.assertEqual(result.returncode, 0, result.stderr)
        ledger = json.loads((self.workdir / ".product-docker-guard" / "state.json").read_text())
        self.assertTrue(ledger["helper_records"][0]["cleaned"])
        self.assertFalse(any(args[:2] == ["container", "rm"] for args in self.docker_events()))

    def test_recovery_accepts_exact_daemon_absence_tuple(self):
        self.pending_helper()
        result = self.invoke(["--cleanup-helpers"], {"TOKEN_PLANET_GUARD_PHASE": "cleanup",
            "FAKE_HELPER_ABSENCE": json.dumps([1, "\n", "Error response from daemon: No such container: " + HELPER_ID + "\n"])})
        self.assertEqual(result.returncode, 0, result.stderr)
        ledger = json.loads((self.workdir / ".product-docker-guard" / "state.json").read_text())
        self.assertTrue(ledger["helper_records"][0]["cleaned"])
        self.assertFalse(any(args[:2] == ["container", "rm"] for args in self.docker_events()))

    def test_daemon_absence_near_misses_fail_inline_and_recovery(self):
        self.prime_db()
        ledger = self.workdir / ".product-docker-guard" / "state.json"
        saved = ledger.read_text()
        stderr = "Error response from daemon: No such container: " + HELPER_ID + "\n"
        cases = ((1, "\nextra", stderr), (1, "\n\n", stderr), (1, "", stderr),
                 (1, " \n", stderr), (1, "\n", stderr.replace(HELPER_ID, "f" * 64)),
                 (1, "\n", stderr + "diagnostic\n"), (1, "\n", stderr + "x"),
                 (1, "\n", "Cannot connect to Docker daemon\n"), (2, "\n", stderr), (0, "\n", stderr))
        for response in cases:
            with self.subTest(response=response):
                ledger.write_text(saved)
                for cidfile in (ledger.parent / "cidfiles").glob("*.cid"):
                    cidfile.unlink()
                self.events.write_text("")
                extra = {"FAKE_HELPER_ABSENCE": json.dumps(response)}
                result = self.invoke(self.helper_args("realtime"), extra)
                self.assertEqual(result.returncode, 125, result.stderr)
                pending = ledger.read_bytes()
                result = self.invoke(["--cleanup-helpers"], {**extra, "TOKEN_PLANET_GUARD_PHASE": "cleanup"})
                self.assertEqual(result.returncode, 125, result.stderr)
                self.assertEqual(ledger.read_bytes(), pending)
                self.assertFalse(any(args[:2] == ["container", "rm"] for args in self.docker_events()))

    def test_cleanup_mode_accepts_only_exact_not_found(self):
        self.pending_helper()
        state = json.loads(self.state.read_text())
        state["helper"] = None
        self.state.write_text(json.dumps(state))
        result = self.invoke(["--cleanup-helpers"], {"TOKEN_PLANET_GUARD_PHASE": "cleanup"})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(any(args[:2] == ["container", "rm"] for args in self.docker_events()))

    def test_cleanup_mode_rejects_not_found_with_unexpected_stdout(self):
        self.pending_helper()
        result = self.invoke(["--cleanup-helpers"], {
            "TOKEN_PLANET_GUARD_PHASE": "cleanup", "FAKE_HELPER_NOT_FOUND_WITH_OUTPUT": "1",
        })
        self.assertEqual(result.returncode, 125, result.stderr)
        self.assertFalse(any(args[:2] == ["container", "rm"] for args in self.docker_events()))

    def test_cleanup_mode_rejects_malformed_ledger_without_docker_calls(self):
        self.pending_helper()
        state_file = self.workdir / ".product-docker-guard" / "state.json"
        saved = state_file.read_text()
        for records in ("malformed", [{"phase": [], "helper": "realtime", "image": HELPERS["realtime"]["image"], "cleaned": False}], [{"phase": "foreign", "helper": "realtime", "image": HELPERS["realtime"]["image"], "cleaned": False}],
                        [{"phase": "start", "helper": "auth", "image": HELPERS["realtime"]["image"], "cleaned": False}]):
            with self.subTest(records=records):
                value = json.loads(saved)
                value["helper_records"] = records
                state_file.write_text(json.dumps(value))
                self.events.write_text("")
                result = self.invoke(["--cleanup-helpers"], {"TOKEN_PLANET_GUARD_PHASE": "cleanup"})
                self.assertEqual(result.returncode, 125, result.stderr)
                self.assertEqual(self.docker_events(), [])

    def test_inline_helper_rejects_owned_attachment_with_malformed_extra(self):
        self.prime_db()
        result = self.invoke(self.helper_args("realtime"), {
            "FAKE_HELPER_RETAIN": "1", "FAKE_HELPER_MUTATION": "network_malformed_extra"})
        self.assertEqual(result.returncode, 125, result.stderr)
        self.assertFalse(any(args[:2] == ["container", "rm"] for args in self.docker_events()))
        ledger = json.loads((self.workdir / ".product-docker-guard" / "state.json").read_text())
        self.assertFalse(ledger["helper_records"][0]["cleaned"])

    def test_recovery_rejects_owned_attachment_with_malformed_extra(self):
        self.pending_helper()
        state = json.loads(self.state.read_text())
        state["helper"]["NetworkSettings"]["Networks"]["unexpected"] = None
        self.state.write_text(json.dumps(state))
        ledger = self.workdir / ".product-docker-guard" / "state.json"
        before = ledger.read_bytes()
        result = self.invoke(["--cleanup-helpers"], {"TOKEN_PLANET_GUARD_PHASE": "cleanup"})
        self.assertEqual(result.returncode, 125, result.stderr)
        self.assertFalse(any(args[:2] == ["container", "rm"] for args in self.docker_events()))
        self.assertEqual(ledger.read_bytes(), before)
        self.assertFalse(json.loads(before)["helper_records"][0]["cleaned"])

    def test_cleanup_mode_rejects_foreign_helper_without_removal(self):
        self.pending_helper()
        saved = self.state.read_text()
        for mutation in ("image", "labels", "network", "ports", "mounts"):
            with self.subTest(mutation=mutation):
                state = json.loads(saved)
                helper = state["helper"]
                if mutation == "image":
                    helper["Config"]["Image"] = HELPERS["auth"]["image"]
                elif mutation == "labels":
                    helper["Config"]["Labels"]["com.docker.compose.project"] = "foreign"
                elif mutation == "network":
                    helper["HostConfig"]["NetworkMode"] = "foreign"
                elif mutation == "ports":
                    helper["HostConfig"]["PortBindings"] = {"80/tcp": [{"HostPort": "80"}]}
                else:
                    helper["Mounts"] = [{"Source": "/foreign"}]
                self.state.write_text(json.dumps(state))
                self.events.write_text("")
                result = self.invoke(["--cleanup-helpers"], {"TOKEN_PLANET_GUARD_PHASE": "cleanup"})
                self.assertEqual(result.returncode, 125, result.stderr)
                self.assertFalse(any(args[:2] == ["container", "rm"] for args in self.docker_events()))

    def test_reset_checks_volume_ownership_before_removing_database(self):
        self.prime_db()
        result = self.invoke(["container", "rm", "-f", DB_ID], {
            "TOKEN_PLANET_GUARD_PHASE": "reset", "FAKE_VOLUME_FOREIGN": "1",
        })
        self.assertEqual(result.returncode, 125, result.stderr)
        self.assertFalse(any(args[:2] == ["container", "rm"] for args in self.docker_events()))

    def test_reset_foreign_db_ownership_never_reaches_remove(self):
        self.prime_db()
        result = self.invoke(["container", "rm", "-f", DB_ID], {
            "TOKEN_PLANET_GUARD_PHASE": "reset", "FAKE_DB_FOREIGN": "1",
        })
        self.assertEqual(result.returncode, 125, result.stderr)
        self.assertFalse(any(args[:2] == ["container", "rm"] for args in self.docker_events()))

    def test_reset_foreign_volume_ownership_never_reaches_remove(self):
        self.prime_db()
        self.env["TOKEN_PLANET_GUARD_PHASE"] = "reset"
        removed = self.invoke(["container", "rm", "-f", DB_ID])
        self.assertEqual(removed.returncode, 0, removed.stderr)
        self.events.write_text("")
        result = self.invoke(["volume", "rm", "-f", self.db_name], {"FAKE_VOLUME_FOREIGN": "1"})
        self.assertEqual(result.returncode, 125, result.stderr)
        self.assertFalse(any(args[:2] == ["volume", "rm"] for args in self.docker_events()))

    def test_malformed_helper_inspect_fails_closed_without_traceback_or_remove(self):
        self.prime_db()
        result = self.invoke(self.helper_args("realtime"), {
            "FAKE_HELPER_RETAIN": "1", "FAKE_HELPER_INSPECT_JSON": "[]",
        })
        self.assertEqual(result.returncode, 125, result.stderr)
        self.assertNotIn("Traceback", result.stderr)
        self.assertFalse(any(args[:2] == ["container", "rm"] for args in self.docker_events()))

    def test_helper_cidfile_and_ownership_fail_closed(self):
        self.prime_db()
        for extra in (
            {"FAKE_BAD_CIDFILE": "1"},
            {"FAKE_HELPER_RETAIN": "1", "FAKE_HELPER_FOREIGN": "1"},
        ):
            with self.subTest(extra=extra):
                state_file = self.workdir / ".product-docker-guard" / "state.json"
                saved = state_file.read_text()
                self.events.write_text("")
                result = self.invoke(self.helper_args("realtime"), extra)
                state_file.write_text(saved)
                for cidfile in (state_file.parent / "cidfiles").glob("*.cid"):
                    cidfile.unlink()
                self.assertEqual(result.returncode, 125, result.stderr)
                self.assertFalse(any(args[:2] == ["container", "rm"] for args in self.docker_events()))


if __name__ == "__main__":
    unittest.main()
