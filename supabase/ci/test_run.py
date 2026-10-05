import glob
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import tempfile
import textwrap
import unittest


RUNNER = Path(__file__).with_name("run.sh")


class WorkflowArtifactTests(unittest.TestCase):
    def test_upload_selects_exact_safe_diagnostic_artifacts(self):
        workflow = RUNNER.parents[2] / ".github/workflows/supabase-migrations.yml"
        text = workflow.read_text()
        upload = text.split("uses: actions/upload-artifact@v4", 1)[1]
        block = re.search(r"(?m)^          path: \|\n((?:            .+\n)+)", upload)
        self.assertIsNotNone(block, "upload artifact path block missing")
        patterns = [line.strip() for line in block.group(1).splitlines()]
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            results = root / "supabase-migration-results"
            results.mkdir()
            expected = {"manifest.json", "start.log", "diagnostics.jsonl", "reset-diagnostics.json"}
            for name in expected:
                (results / name).write_text("sanitized")
            (results / "raw-inspect.json").write_text("credential-sentinel")
            workdir = root / "token-planet-ci.fixture"
            workdir.mkdir()
            (workdir / "start.raw.log").write_text("credential-sentinel")
            selected = set()
            for pattern in patterns:
                expanded = pattern.replace("${{ runner.temp }}", str(root))
                selected.update(Path(path).relative_to(results).as_posix() for path in glob.glob(expanded, recursive=True))
            self.assertEqual(selected, expected)
            self.assertIn("${{ runner.temp }}/supabase-migration-results/diagnostics.jsonl", patterns)
            self.assertIn("${{ runner.temp }}/supabase-migration-results/reset-diagnostics.json", patterns)


class RunnerTests(unittest.TestCase):
    FAKE_SUPABASE = r'''#!/usr/bin/env python3
import concurrent.futures, json, os, pathlib, re, subprocess, sys
args = sys.argv[1:]
events = pathlib.Path(os.environ["FAKE_EVENTS"])
state_path = pathlib.Path(os.environ["FAKE_STATE"])
def record(tool, args, **extra):
    with events.open("a") as stream:
        stream.write(json.dumps({"tool": tool, "args": args, **extra}) + "\n")
state = json.loads(state_path.read_text()) if state_path.exists() else {}
record("supabase", args, cwd=os.getcwd())
def bootstrap(reset=False):
    project = re.search(r'^project_id\s*=\s*"([^"]+)"', pathlib.Path("supabase/config.toml").read_text(), re.M).group(1)
    workdir = os.getcwd()
    network = args[args.index("--network-id") + 1]
    db_name = "supabase_db_" + project
    os.environ.update({
        "POSTGRES_PASSWORD": "fixture-db-password",
        "POSTGRES_HOST": "/var/run/postgresql",
        "JWT_SECRET": "fixture-jwt-secret",
        "JWT_EXP": "3600",
    })
    create = [
        "create", "--name", db_name,
        "-e", "POSTGRES_PASSWORD", "-e", "POSTGRES_HOST", "-e", "JWT_SECRET", "-e", "JWT_EXP",
        "-v", db_name + ":/var/lib/postgresql/data", "-p", "56432:5432",
        "--health-cmd", "pg_isready -U postgres -h 127.0.0.1 -p 5432",
        "--health-interval", "10s", "--health-timeout", "2s", "--health-retries", "3",
        "--restart", "unless-stopped", "--network", network,
        "--network-alias", "db", "--network-alias", "db.supabase.internal",
        "--label", "com.supabase.cli.project=" + project,
        "--label", "com.docker.compose.project=" + project,
        "--label", "com.supabase.cli.workdir=" + workdir,
        "--entrypoint", "sh", "public.ecr.aws/supabase/postgres:17.6.1.171",
        "-c", "opaque pinned init script",
    ]
    if sys.platform.startswith("linux"):
        index = create.index("--network")
        create[index:index] = ["--add-host", "host.docker.internal:host-gateway"]
    def docker_child(child_args):
        child = subprocess.run(["docker", *child_args], check=False, capture_output=True, text=True)
        record("supabase_docker", child_args, status=child.returncode)
        sys.stdout.write(child.stdout)
        sys.stderr.write(child.stderr)
        if child.returncode:
            sys.exit(child.returncode)
        return child.stdout.strip()
    if reset:
        docker_child(["container", "rm", "-f", db_name])
        docker_child(["volume", "rm", "-f", db_name])
    docker_child(["volume", "create", "--label", "com.supabase.cli.project=" + project,
                  "--label", "com.docker.compose.project=" + project, db_name])
    state.update(json.loads(state_path.read_text()))
    state.update(project_id=project, workdir=workdir, network_id=network)
    state_path.write_text(json.dumps(state))
    db_id = docker_child(create)
    docker_child(["cp", "-", db_id + ":/"])
    docker_child(["start", db_id])
    if os.environ.get("FAKE_PRODUCT_GUARD_SCENARIO") in ("helper", "full-bootstrap"):
        realtime_keys = "PORT DB_HOST DB_PORT DB_USER DB_PASSWORD DB_NAME DB_AFTER_CONNECT_QUERY DB_ENC_KEY API_JWT_SECRET API_JWT_JWKS METRICS_JWT_SECRET APP_NAME SECRET_KEY_BASE ERL_AFLAGS DNS_NODES RLIMIT_NOFILE SEED_SELF_HOST RUN_JANITOR MAX_HEADER_LENGTH".split()
        os.environ.update({
            "PORT": "4000", "DB_HOST": db_name, "DB_PORT": "5432", "DB_USER": "supabase_admin",
            "DB_PASSWORD": "fixture-db-password", "DB_NAME": "postgres",
            "DB_AFTER_CONNECT_QUERY": "SET search_path TO _realtime", "DB_ENC_KEY": "fixture-encryption-key",
            "API_JWT_SECRET": "fixture-api-secret", "API_JWT_JWKS": "fixture-jwks",
            "METRICS_JWT_SECRET": "fixture-metrics-secret", "APP_NAME": "realtime",
            "SECRET_KEY_BASE": "fixture-secret-key-base", "ERL_AFLAGS": "-proto_dist inet_tcp",
            "DNS_NODES": "''", "RLIMIT_NOFILE": "", "SEED_SELF_HOST": "true",
            "RUN_JANITOR": "true", "MAX_HEADER_LENGTH": "8192",
        })
        helper = ["run", "--rm", "--network", network]
        if sys.platform.startswith("linux"):
            helper.extend(["--add-host", "host.docker.internal:host-gateway"])
        for key in realtime_keys:
            helper.extend(["-e", key])
        helper.extend([
            "--label", "com.supabase.cli.project=" + project,
            "--label", "com.docker.compose.project=" + project,
            "public.ecr.aws/supabase/realtime:v2.140.3", "/app/bin/realtime", "eval",
            '{:ok, _} = Application.ensure_all_started(:realtime)\n'
            '{:ok, _} = Realtime.Tenants.health_check("realtime-dev")',
        ])
        docker_child(helper)
        if os.environ.get("FAKE_PRODUCT_GUARD_SCENARIO") == "full-bootstrap":
            os.environ.update({
                "DB_INSTALL_ROLES": "false", "DB_MIGRATIONS_FREEZE_AT": "", "ANON_KEY": "anon-fixture",
                "SERVICE_KEY": "service-fixture", "PGRST_JWT_SECRET": "jwt-fixture",
                "DATABASE_URL": "postgresql://supabase_storage_admin:password@" + db_name + ":5432/postgres",
                "FILE_SIZE_LIMIT": "52428800", "STORAGE_BACKEND": "file", "STORAGE_FILE_BACKEND_PATH": "/mnt",
                "TENANT_ID": "stub", "REGION": "stub", "GLOBAL_S3_BUCKET": "stub",
                "API_EXTERNAL_URL": "http://127.0.0.1:54321", "GOTRUE_LOG_LEVEL": "error",
                "GOTRUE_DB_DRIVER": "postgres", "GOTRUE_DB_DATABASE_URL": "postgresql://supabase_auth_admin:password@" + db_name + ":5432/postgres",
                "GOTRUE_SITE_URL": "http://127.0.0.1:3000", "GOTRUE_JWT_SECRET": "jwt-fixture",
            })
            contracts = [
                ("storage-api:v1.79.28", "DB_INSTALL_ROLES DB_MIGRATIONS_FREEZE_AT ANON_KEY SERVICE_KEY PGRST_JWT_SECRET DATABASE_URL FILE_SIZE_LIMIT STORAGE_BACKEND STORAGE_FILE_BACKEND_PATH TENANT_ID REGION GLOBAL_S3_BUCKET", ["node", "dist/scripts/migrate-call.js"]),
                ("gotrue:v2.197.0", "API_EXTERNAL_URL GOTRUE_LOG_LEVEL GOTRUE_DB_DRIVER GOTRUE_DB_DATABASE_URL GOTRUE_SITE_URL GOTRUE_JWT_SECRET", ["gotrue", "migrate"]),
            ]
            for image, keys, command in contracts:
                helper = ["run", "--rm", "--network", network]
                if sys.platform.startswith("linux"):
                    helper.extend(["--add-host", "host.docker.internal:host-gateway"])
                for key in keys.split():
                    helper.extend(["-e", key])
                helper.extend(["--label", "com.supabase.cli.project=" + project,
                               "--label", "com.docker.compose.project=" + project,
                               "public.ecr.aws/supabase/" + image, *command])
                docker_child(helper)
    state.update(json.loads(state_path.read_text()))

if args == ["--version"]:
    cache = pathlib.Path("supabase/.temp/cli-latest")
    cache.parent.mkdir(parents=True, exist_ok=True)
    cache.write_text("fake-cli-latest")
    print(os.environ.get("FAKE_CLI_VERSION", "2.119.0"))
elif args and args[0] == "start":
    product_docker_args = None
    if os.environ.get("FAKE_PRODUCT_GUARD_PULL") == "1":
        product_docker_args = ["pull", "unapproved/image:latest"]
    elif os.environ.get("FAKE_PRODUCT_GUARD_ARGS"):
        product_docker_args = json.loads(os.environ["FAKE_PRODUCT_GUARD_ARGS"])
    if product_docker_args is not None:
        child = subprocess.run(["docker", *product_docker_args], check=False)
        record("supabase_docker", product_docker_args, status=child.returncode)
        sys.exit(child.returncode)
    if os.environ.get("FAKE_PRODUCT_GUARD_SCENARIO") in ("db-create", "helper", "full-bootstrap"):
        bootstrap()
    config = pathlib.Path("supabase/config.toml").read_text()
    state["project_id"] = re.search(r'^project_id\s*=\s*"([^"]+)"', config, re.M).group(1)
    state["workdir"] = os.getcwd()
    state["started"] = os.environ.get("FAKE_FAIL_START_BEFORE_CONTAINER") != "1"
    state["network_id"] = args[args.index("--network-id") + 1]
    manifest = json.loads(pathlib.Path("supabase/migrations/manifest.json").read_text())
    state["versions"] = [entry["synthetic_version"] for entry in manifest["entries"]]
    state_path.write_text(json.dumps(state))
    print("anon key: eyJfixture.header.signature")
    if os.environ.get("FAKE_DIAGNOSTIC_ARTIFACT_BLOCK"):
        pathlib.Path(os.environ["FAKE_DIAGNOSTIC_ARTIFACT_BLOCK"]).mkdir()
    if os.environ.get("FAKE_FAIL_START") == "1":
        print("diagnostic-password-sentinel eyJdiagnostic.jwt.signature diagnostic-health-log-sentinel")
    if os.environ.get("FAKE_FAIL_START") == "1" or os.environ.get("FAKE_FAIL_START_BEFORE_CONTAINER") == "1":
        sys.exit(23)
elif args[:2] == ["db", "reset"]:
    manifest = json.loads(pathlib.Path("supabase/migrations/manifest.json").read_text())
    if os.environ.get("FAKE_PRODUCT_GUARD_SCENARIO") == "full-bootstrap":
        bootstrap(reset=True)
        project = state["project_id"]
        def restart_excluded(suffix):
            target = "supabase_" + suffix + "_" + project
            child = subprocess.run(["docker", "restart", target], capture_output=True, text=True)
            record("supabase_docker", ["restart", target], status=child.returncode)
            if child.returncode and not re.search(r"no such container|no such object|no container with name or id", child.stderr.strip(), re.I):
                return child.returncode
            return 0
        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as executor:
            statuses = list(executor.map(restart_excluded, ("storage", "auth", "realtime", "pooler")))
        if any(statuses):
            sys.exit(next(status for status in statuses if status))
        kong = "supabase_kong_" + project
        args_kong = ["container", "inspect", kong, "--format", "{{json .State}}"]
        inspected = subprocess.run(["docker", *args_kong], capture_output=True, text=True)
        record("supabase_docker", args_kong, status=inspected.returncode)
        if inspected.returncode:
            if not re.search(r"no such container|no such object|no container with name or id", inspected.stderr.strip(), re.I):
                sys.exit(inspected.returncode)
        elif json.loads(inspected.stdout).get("Running") is True:
            child = subprocess.run(["docker", "exec", kong, "kong", "reload", "--nginx-conf", "/home/kong/custom_nginx.template"])
            sys.exit(child.returncode)
    if os.environ.get("FAKE_RESET_OUTPUT"):
        print(os.environ["FAKE_RESET_OUTPUT"].replace("{migration}", manifest["entries"][0]["staged_filename"]))
    if os.environ.get("FAKE_RESET_EVIDENCE"):
        directory = pathlib.Path(".product-docker-guard")
        directory.mkdir(mode=0o700, exist_ok=True)
        sidecar = directory / "rejections.jsonl"
        sidecar.write_text(os.environ["FAKE_RESET_EVIDENCE"])
        sidecar.chmod(0o600)
    if os.environ.get("FAKE_RESET_RAW_MODE"):
        sys.stdout.flush()
        raw = pathlib.Path("reset.raw.log")
        mode = os.environ["FAKE_RESET_RAW_MODE"]
        if mode == "symlink":
            raw.unlink()
            raw.symlink_to("missing-raw")
        elif mode == "permissions": raw.chmod(0o644)
    if os.environ.get("FAKE_RESET_SINK_BLOCK"):
        pathlib.Path(os.environ["FAKE_RESET_SINK_BLOCK"]).mkdir()
    print("Database URL: postgres://postgres:fixture-password@localhost:56432/postgres")
    state["reset_complete"] = True
    if os.environ.get("FAKE_RESET_RECREATE") == "1" or os.environ.get("FAKE_FAIL_RESET_AFTER_RECREATE") == "1":
        state["container_id"] = "recreated-container-id"
    state_path.write_text(json.dumps(state))
    if os.environ.get("FAKE_FAIL_RESET") == "1" or os.environ.get("FAKE_FAIL_RESET_AFTER_RECREATE") == "1":
        if os.environ.get("FAKE_RESET_RAW_MODE") == "corrupt":
            sys.stdout.flush()
            pathlib.Path("reset.raw.log").write_bytes(b"\xff")
        if os.environ.get("FAKE_RESET_MANIFEST_MODE"):
            target = pathlib.Path("supabase/migrations/manifest.json")
            mode = os.environ["FAKE_RESET_MANIFEST_MODE"]
            if mode == "fifo":
                target.unlink()
                os.mkfifo(target, 0o600)
            elif mode == "symlink":
                target.rename(target.with_name("saved-manifest.json"))
                target.symlink_to("saved-manifest.json")
            elif mode == "oversize": target.write_text(" " * 1048577 + target.read_text())
            elif mode == "missing": target.unlink()
        sys.exit(37)
sys.exit(0)
'''

    FAKE_DOCKER = r'''#!/usr/bin/env python3
import json, os, pathlib, sys
args = sys.argv[1:]
events = pathlib.Path(os.environ["FAKE_EVENTS"])
state_path = pathlib.Path(os.environ["FAKE_STATE"])
def record():
    with events.open("a") as stream:
        stream.write(json.dumps({"tool": "docker", "args": args}) + "\n")
def read_state():
    return json.loads(state_path.read_text()) if state_path.exists() else {}
def write_state(state):
    state_path.write_text(json.dumps(state))
def inspect_attempt(kind, threshold):
    key = kind + "_inspect_count"
    count = state.get(key, 0) + 1
    state[key] = count
    write_state(state)
    if count >= threshold and os.environ.get("FAKE_CLEANUP_INSPECT_ERROR") in (kind, "all"):
        print("Cannot connect to the Docker daemon at unix:///var/run/docker.sock", file=sys.stderr)
        sys.exit(1)
    return count
record()
state = read_state()
if args[:2] == ["context", "show"]:
    print(os.environ.get("FAKE_DOCKER_CONTEXT", "default"))
elif args[:2] == ["context", "inspect"]:
    print(json.dumps(os.environ.get("FAKE_DOCKER_ENDPOINT", "unix:///var/run/docker.sock")))
elif args[0] == "info":
    pass
elif args[:2] == ["image", "inspect"]:
    print(json.dumps({"Config": {"Env": ["POSTGRES_PASSWORD=fixture-image-secret"]}}))
elif args[:2] == ["network", "inspect"]:
    target = args[-1]
    inspect_attempt("network", 4)
    if not state.get("network_created"):
        print(f"Error: No such network: {target}", file=sys.stderr)
        sys.exit(1)
    print(json.dumps({"Id": state["network_id"], "Name": state["network_name"],
                      "Options": {"com.docker.network.bridge.host_binding_ipv4": "127.0.0.1"},
                      "Labels": state["network_labels"]}))
elif args[:2] == ["network", "create"]:
    if os.environ.get("FAKE_FAIL_NETWORK_CREATE") == "1":
        sys.exit(32)
    labels = {}
    for index, arg in enumerate(args[:-1]):
        if arg == "--label":
            key, value = args[index + 1].split("=", 1)
            labels[key] = value
    state.update(network_created=True, network_id="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                 network_name=args[-1], network_labels=labels)
    write_state(state)
    print(state["network_id"])
elif args[:2] == ["container", "inspect"]:
    if args[-1:] == ["{{json .State}}"]:
        target = args[2]
        excluded = ("storage", "auth", "realtime", "pooler", "kong")
        project = state.get("project_id", "")
        if target in ["supabase_" + suffix + "_" + project for suffix in excluded]:
            if target == "supabase_" + os.environ.get("FAKE_SATELLITE_EXISTING", "") + "_" + project:
                print(json.dumps({"Running": True}))
                sys.exit(0)
            if target == "supabase_kong_" + project and os.environ.get("FAKE_KONG_STATE", "absent") != "absent":
                print(json.dumps({"Running": os.environ["FAKE_KONG_STATE"] == "running"}))
                sys.exit(0)
            print(f"Error: No such object: {target}", file=sys.stderr)
            sys.exit(1)
        if target not in (state.get("container_id", "owned-container-id"), "supabase_db_" + state.get("project_id", "")):
            print(f"Error: No such object: {target}", file=sys.stderr)
            sys.exit(1)
        print(json.dumps({"Running": bool(state.get("started"))}))
        sys.exit(0)
    inspect_target = args[2] if len(args) >= 3 else ""
    if os.environ.get("TOKEN_PLANET_GUARD_PHASE") == "diagnostic":
        mode = os.environ.get("FAKE_DIAGNOSTIC_LOOKUP", "owned")
        if mode in ("absent", "inspect_failed"):
            print(f"Error: No such object: {inspect_target}" if mode == "absent" else "daemon diagnostic-password-sentinel", file=sys.stderr)
            sys.exit(1)
        if mode == "inspect_invalid":
            print("diagnostic-health-log-sentinel")
            sys.exit(0)
    if inspect_target == state.get("helper_id"):
        if not state.get("helper"):
            print(f"Error: No such object: {inspect_target}", file=sys.stderr)
            sys.exit(1)
        state["helper_inspect_count"] = state.get("helper_inspect_count", 0) + 1
        write_state(state)
        if os.environ.get("FAKE_HELPER_INSPECT_TRANSIENT") == "1" and state["helper_inspect_count"] == 1:
            print("temporary inspection failure", file=sys.stderr)
            sys.exit(1)
        if os.environ.get("FAKE_HELPER_FOREIGN") == "1":
            state["helper"]["Config"]["Labels"]["com.supabase.cli.project"] = "foreign"
        print(json.dumps(state["helper"]))
        sys.exit(0)
    if args[-1:] == ["{{json .}}"] and inspect_target != state.get("container_id"):
        print(f"Error: No such object: {inspect_target}", file=sys.stderr)
        sys.exit(1)
    target = args[-1]
    inspect_attempt("container", 4)
    if not (state.get("started") or state.get("container_created")) or state.get("container_removed"):
        print(f"Error: No such object: {target}", file=sys.stderr)
        sys.exit(1)
    project_id = state["project_id"]
    if os.environ.get("FAKE_MISMATCH_CONTAINER") == "1":
        project_id += "-foreign"
    attached_network_id = state.get("network_id") if state.get("started") else ""
    if os.environ.get("FAKE_MISMATCH_NETWORK") == "1" and state.get("reset_complete"):
        attached_network_id = "foreign-network-id"
    actual_host_ip = os.environ.get("FAKE_ACTUAL_HOSTIP", "127.0.0.1")
    actual_ports = [] if os.environ.get("FAKE_EMPTY_ACTUAL_BINDING") == "1" else [
        {"HostIp": actual_host_ip, "HostPort": "56432"}
    ]
    print(json.dumps({"Id": state.get("container_id", "owned-container-id"), "Name": "/supabase_db_" + project_id,
                      "State": {"Status": "created" if state.get("container_created") and not state.get("started") else "exited", "Running": False, "ExitCode": 17, "Error": "diagnostic-password-sentinel eyJdiagnostic.jwt.signature",
                                "Health": {"Status": "unhealthy", "Log": [{"Output": "diagnostic-health-log-sentinel"}]}},
                      "Config": {"Image": "public.ecr.aws/supabase/postgres:17.6.1.171", "Labels": {"com.supabase.cli.project": project_id,
                                               "com.docker.compose.project": project_id,
                                               "com.supabase.cli.workdir": state["workdir"]}},
                      "NetworkSettings": {"Networks": {state.get("network_name", "owned-network"): {"NetworkID": attached_network_id}},
                                          "Ports": {"5432/tcp": actual_ports}},
                      "HostConfig": {"NetworkMode": state["network_id"], "PortBindings": {"5432/tcp": [{"HostIp": "127.0.0.1", "HostPort": "56432"}]}},
                      "Mounts": [{"Type": "volume", "Name": "supabase_db_" + project_id, "Destination": "/var/lib/postgresql/data", "RW": True}]}))
elif args[:2] == ["volume", "create"]:
    state["volume_removed"] = False
    state["volume_created"] = True
    write_state(state)
elif args[:2] == ["volume", "inspect"]:
    target = args[-1]
    inspect_attempt("volume", 3)
    if not (state.get("started") or state.get("volume_created")) or state.get("volume_removed"):
        print(f"Error: No such volume: {target}", file=sys.stderr)
        sys.exit(1)
    print(json.dumps({"Name": "supabase_db_" + state["project_id"],
                      "Labels": {"com.supabase.cli.project": state["project_id"], "com.docker.compose.project": state["project_id"]}}))
elif args[0] == "exec":
    command = args[2:]
    if command[:1] == ["psql"] and "-c" in command:
        print("\n".join(state.get("versions", [])))
    elif command[:1] == ["psql"] and "-f" in command:
        if os.environ.get("FAKE_FAIL_PSQL") == "1":
            print("fixture-password in SQL diagnostics", file=sys.stderr)
            sys.exit(45)
        print(os.environ.get("FAKE_TAP", "1..1\nok 1 - fake test"))
elif args[:1] == ["cp"]:
    pass
elif args[:1] == ["create"]:
    state.update(container_id=("e" if state.get("container_removed") else "c") * 64, started=False, container_created=True, container_removed=False)
    write_state(state)
    print(state["container_id"])
elif args[:1] == ["start"]:
    state["started"] = True
    write_state(state)
elif args[:1] == ["run"]:
    if "--pull=never" not in args or "--cidfile" not in args:
        print("fake Docker requires guarded helper arguments", file=sys.stderr)
        sys.exit(89)
    cidfile = pathlib.Path(args[args.index("--cidfile") + 1])
    cidfile.parent.mkdir(parents=True, exist_ok=True)
    helper_id = "d" * 64
    cidfile.write_text(helper_id + "\n")
    if os.environ.get("FAKE_HELPER_RETAIN") == "1":
        label_indexes = [index for index, arg in enumerate(args) if arg == "--label"]
        image = args[label_indexes[-1] + 2]
        network = args[args.index("--network") + 1]
        state["helper_id"] = helper_id
        state["helper"] = {
            "Id": helper_id, "Name": "/helper-container",
            "Config": {"Image": image, "Labels": {
                "com.supabase.cli.project": state["project_id"],
                "com.docker.compose.project": state["project_id"],
            }},
            "HostConfig": {"NetworkMode": network, "PortBindings": {}},
            "NetworkSettings": {"Networks": {network: {"NetworkID": network}}, "Ports": {}},
            "Mounts": [],
        }
        write_state(state)
    if os.environ.get("FAKE_HELPER_EXIT") == "1":
        print("original helper failure", file=sys.stderr)
        sys.exit(44)
elif args[:3] == ["container", "rm", "-f"]:
    if args[-1] == state.get("helper_id"):
        state["helper_remove_count"] = state.get("helper_remove_count", 0) + 1
        write_state(state)
        if os.environ.get("FAKE_HELPER_REMOVE_TRANSIENT") == "1" and state["helper_remove_count"] == 1:
            print("temporary removal failure", file=sys.stderr)
            sys.exit(1)
        state["helper"] = None
    else:
        state["container_removed"] = True
    write_state(state)
elif args[:2] == ["container", "rm"]:
    state["container_removed"] = True
    write_state(state)
elif args[:2] == ["volume", "rm"]:
    state["volume_removed"] = True
    write_state(state)
elif args[:2] == ["network", "rm"]:
    if args[-1] != state.get("network_id"):
        sys.exit(54)
    state["network_created"] = False
    write_state(state)
else:
    print("unexpected docker arguments: " + " ".join(args), file=sys.stderr)
    sys.exit(99)
'''

    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory()
        self.root = Path(self.temp_dir.name)
        self.repo = self.root / "repo"
        self.supabase = self.repo / "supabase"
        self.bin = self.root / "bin"
        self.ci = self.supabase / "ci"
        self.migrations = self.supabase / "migrations"
        self.tests = self.supabase / "tests"
        self.ci.mkdir(parents=True)
        self.migrations.mkdir()
        self.tests.mkdir()
        self.bin.mkdir()
        self.source_cli_latest = self.supabase / ".temp" / "cli-latest"
        self.source_cli_latest.parent.mkdir(parents=True)
        self.source_cli_latest.write_text("source-cli-cache-baseline")
        self.runner = self.ci / "run.sh"
        shutil.copy2(RUNNER, self.runner)
        for filename in ("product_docker_guard.py", "host_port_adapter.py"):
            shutil.copy2(RUNNER.with_name(filename), self.ci / filename)
        shutil.copy2(RUNNER.with_name("prepare_migrations.py"), self.ci / "prepare_migrations.py")
        (self.migrations / "202610010001_parent.sql").write_text("select 1;\n")
        (self.tests / "sample.sql").write_text(
            "begin;\nselect plan(1);\nselect ok(true);\nselect * from finish();\nrollback;\n"
        )
        self.fake_supabase = self.bin / "supabase"
        self.fake_docker = self.bin / "docker"
        self.fake_supabase.write_text(textwrap.dedent(self.FAKE_SUPABASE))
        self.fake_docker.write_text(textwrap.dedent(self.FAKE_DOCKER))
        self.fake_supabase.chmod(0o755)
        self.fake_docker.chmod(0o755)
        self.real_python = shutil.which("python3")
        self.fake_python = self.bin / "python3"
        self.fake_python.write_text(
            textwrap.dedent(
                f'''\
                #!/bin/sh
                if [ "$1" = "-" ] && [ "$2" = "56432" ] && [ "$3" = "56430" ]; then
                    cat >/dev/null
                    if [ "$FAKE_PORT_BUSY" = "1" ]; then exit 41; fi
                    exit 0
                fi
                if [ "$FAKE_DIAGNOSTIC_EXPORT_FAIL" = "1" ] && [ "$2" = "--diagnose-start" ]; then
                    printf 'diagnostic-password-sentinel' >&2
                    exit 42
                fi
                case "$1" in
                    */product_docker_guard.py)
                        "{self.real_python}" -c 'import json, os; f=open(os.environ["FAKE_EVENTS"], "a"); f.write(json.dumps(dict(tool="guard", guard_phase=os.environ.get("TOKEN_PLANET_GUARD_PHASE"), guard_network_name=os.environ.get("TOKEN_PLANET_GUARD_NETWORK_NAME"))) + "\\n"); f.close()'
                        ;;
                esac
                if [ "$FAKE_RESET_DIAGNOSTIC_FAIL" = "1" ] && [ "$2" = "--reset-diagnostics" ]; then
                    printf 'credential-sentinel' >&2
                    exit 43
                fi
                exec "{self.real_python}" "$@"
                '''
            )
        )
        self.fake_python.chmod(0o755)
        self.events = self.root / "events.jsonl"
        self.state = self.root / "state.json"
        self.artifacts = self.root / "artifacts"
        self.env = os.environ.copy()
        self.env.update(
            {
                "SUPABASE_BIN": str(self.fake_supabase),
                "PATH": f"{self.bin}:{self.env['PATH']}",
                "FAKE_EVENTS": str(self.events),
                "FAKE_STATE": str(self.state),
                "SUPABASE_TELEMETRY_DISABLED": "1",
            }
        )

    def tearDown(self):
        self.temp_dir.cleanup()

    def run_runner(self, extra_env=None, artifacts_path=None):
        env = self.env.copy()
        if extra_env:
            env.update(extra_env)
        return subprocess.run(
            ["bash", str(self.runner), "--artifacts-dir", str(artifacts_path or self.artifacts)],
            cwd=self.repo,
            env=env,
            check=False,
            capture_output=True,
            text=True,
        )

    def event_rows(self):
        if not self.events.exists():
            return []
        return [json.loads(line) for line in self.events.read_text().splitlines()]

    def test_success_starts_resets_tests_and_cleans_resources_in_order(self):
        result = self.run_runner()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.source_cli_latest.read_text(), "source-cli-cache-baseline")
        events = self.event_rows()
        cli = [event for event in events if event["tool"] == "supabase"]
        docker = [event for event in events if event["tool"] == "docker"]
        version = next(event for event in cli if event["args"] == ["--version"])
        self.assertTrue(Path(version["cwd"]).name.startswith("token-planet-ci."))
        start = next(i for i, event in enumerate(cli) if event["args"][0] == "start")
        reset = next(i for i, event in enumerate(cli) if event["args"][:2] == ["db", "reset"])
        self.assertLess(start, reset)
        self.assertIn("--network-id", cli[start]["args"])
        self.assertIn("--local", cli[reset]["args"])
        self.assertIn("--no-seed", cli[reset]["args"])
        self.assertIn("--network-id", cli[reset]["args"])
        self.assertEqual(
            cli[start]["args"][cli[start]["args"].index("--network-id") + 1],
            cli[reset]["args"][cli[reset]["args"].index("--network-id") + 1],
        )
        self.assertTrue(any(event["args"][:2] == ["network", "create"] for event in docker))
        self.assertTrue(any(event["args"][:2] == ["container", "rm"] for event in docker))
        self.assertTrue(any(event["args"][:2] == ["volume", "rm"] for event in docker))
        self.assertTrue(any(event["args"][:2] == ["network", "rm"] for event in docker))
        self.assertTrue((self.artifacts / "manifest.json").is_file())
        self.assertIn("binding_status=PASS", (self.artifacts / "container-binding-start.log").read_text())
        self.assertIn("network_status=PASS", (self.artifacts / "container-network-start.log").read_text())
        self.assertIn("status=PASS", (self.artifacts / "test-sample.tap-summary.log").read_text())
        self.assertFalse((self.artifacts / "image-inspect.log").exists())
        artifacts_text = "\n".join(
            path.read_text(errors="replace") for path in self.artifacts.iterdir() if path.is_file()
        )
        self.assertNotIn("fixture-image-secret", artifacts_text)
        self.assertNotIn("fixture-password", artifacts_text)
        self.assertNotIn("eyJfixture.header.signature", artifacts_text)
        self.assertIn("credential-bearing CLI output redacted", (self.artifacts / "start.log").read_text())
        self.assertIn("credential-bearing CLI output redacted", (self.artifacts / "reset.log").read_text())

    def test_start_failure_preserves_exit_and_cleans_only_created_network(self):
        result = self.run_runner({"FAKE_FAIL_START": "1"})

        self.assertEqual(result.returncode, 23, result.stderr)
        events = self.event_rows()
        docker = [event["args"] for event in events if event["tool"] == "docker"]
        self.assertTrue(any(args[:2] == ["network", "create"] for args in docker))
        self.assertTrue(any(args[:2] == ["network", "rm"] and args[-1] == "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" for args in docker))
        self.assertFalse(any(args[:2] == ["db", "reset"] for args in [event["args"] for event in events if event["tool"] == "supabase"]))
        self.assertTrue(any(args[:2] == ["container", "rm"] for args in docker))
        self.assertTrue(any(args[:2] == ["volume", "rm"] for args in docker))

    def test_cli_child_pull_is_rejected_before_the_real_docker_command(self):
        result = self.run_runner({"FAKE_PRODUCT_GUARD_PULL": "1"})

        self.assertEqual(result.returncode, 125, result.stderr)
        docker = [event["args"] for event in self.event_rows() if event["tool"] == "docker"]
        self.assertFalse(any(args[:1] == ["pull"] for args in docker))

    def test_cli_child_prune_is_rejected_before_the_real_docker_command(self):
        result = self.run_runner(
            {"FAKE_PRODUCT_GUARD_ARGS": json.dumps(["container", "prune", "--force"])},
            artifacts_path=self.root / "artifacts-prune",
        )

        self.assertEqual(result.returncode, 125, result.stderr)
        docker = [event["args"] for event in self.event_rows() if event["tool"] == "docker"]
        self.assertFalse(any(args[:2] == ["container", "prune"] for args in docker))

    def test_cli_child_global_docker_override_is_rejected_before_the_real_command(self):
        result = self.run_runner(
            {"FAKE_PRODUCT_GUARD_ARGS": json.dumps(["--host", "tcp://untrusted.example", "ps"])},
            artifacts_path=self.root / "artifacts-global-option",
        )

        self.assertEqual(result.returncode, 125, result.stderr)
        docker = [event["args"] for event in self.event_rows() if event["tool"] == "docker"]
        self.assertFalse(any(args and args[0] == "--host" for args in docker))

    def test_cli_db_create_is_rewritten_to_cached_loopback_publish(self):
        result = self.run_runner({"FAKE_PRODUCT_GUARD_SCENARIO": "db-create"})

        self.assertEqual(result.returncode, 0, result.stderr)
        docker = [event["args"] for event in self.event_rows() if event["tool"] == "docker"]
        creates = [args for args in docker if args[:1] == ["create"]]
        self.assertEqual(len(creates), 1)
        self.assertEqual(creates[0].count("--pull=never"), 1)
        self.assertIn("127.0.0.1:56432:5432", creates[0])
        self.assertNotIn("56432:5432", creates[0])

    def test_cli_helper_run_is_cache_only_and_removes_only_verified_cidfile_owner(self):
        result = self.run_runner({
            "FAKE_PRODUCT_GUARD_SCENARIO": "helper",
            "FAKE_HELPER_RETAIN": "1",
        })

        self.assertEqual(result.returncode, 0, result.stderr)
        docker = [event["args"] for event in self.event_rows() if event["tool"] == "docker"]
        runs = [args for args in docker if args[:1] == ["run"]]
        self.assertEqual(len(runs), 1)
        self.assertEqual(runs[0].count("--pull=never"), 1)
        self.assertEqual(runs[0].count("--cidfile"), 1)
        self.assertTrue(any(
            args[:3] == ["container", "rm", "-f"] and args[-1] == "d" * 64
            for args in docker
        ))

    def test_start_failure_exports_safe_owned_diagnostic_and_preserves_exit_cleanup(self):
        result = self.run_runner({"FAKE_PRODUCT_GUARD_SCENARIO": "db-create", "FAKE_FAIL_START": "1"})
        self.assertEqual(result.returncode, 23, result.stderr)
        diagnostic = self.artifacts / "diagnostics.jsonl"
        self.assertTrue(diagnostic.is_file(), "structured start diagnostic artifact is missing")
        rows = [json.loads(line) for line in diagnostic.read_text().splitlines()]
        self.assertIn({"lookup": "owned", "state": "exited", "health": "unhealthy", "exit_code": 17}, rows)
        cli = [e["args"] for e in self.event_rows() if e["tool"] == "supabase"]
        self.assertFalse(any(args[:2] == ["db", "reset"] for args in cli))
        docker = [e["args"] for e in self.event_rows() if e["tool"] == "docker"]
        self.assertIn(["container", "rm", "--force", "c" * 64], docker)
        for path in self.artifacts.rglob("*"):
            if path.is_file():
                contents = path.read_text()
                for sentinel in ("diagnostic-password-sentinel", "eyJdiagnostic.jwt.signature", "diagnostic-health-log-sentinel"):
                    self.assertNotIn(sentinel, contents, str(path))

    def test_guard_rejection_code_survives_only_structured_export(self):
        result = self.run_runner({"FAKE_PRODUCT_GUARD_PULL": "1"})
        self.assertEqual(result.returncode, 125)
        diagnostic = self.artifacts / "diagnostics.jsonl"
        self.assertTrue(diagnostic.is_file(), "guard code artifact is missing")
        rows = [json.loads(line) for line in diagnostic.read_text().splitlines()]
        self.assertIn({"phase": "start", "code": "DOCKER_COMMAND_IS_NOT_APPROVED"}, rows)
        self.assertNotIn("DOCKER_COMMAND_IS_NOT_APPROVED", (self.artifacts / "start.log").read_text())
        self.assertIn("[unclassified CLI output redacted]", (self.artifacts / "start.log").read_text())

    def test_diagnostic_export_failure_never_masks_original_start_exit(self):
        result = self.run_runner({"FAKE_PRODUCT_GUARD_SCENARIO": "db-create", "FAKE_FAIL_START": "1", "FAKE_DIAGNOSTIC_EXPORT_FAIL": "1"})
        self.assertEqual(result.returncode, 23, result.stderr)
        self.assertNotIn("diagnostic-password-sentinel", result.stderr)
        state = json.loads(self.state.read_text())
        self.assertTrue(state["container_removed"])
        self.assertTrue(state["volume_removed"])
        self.assertFalse(state["network_created"])

    def test_diagnostic_artifact_sink_failure_preserves_exit_and_cleanup_order(self):
        result = self.run_runner({"FAKE_PRODUCT_GUARD_SCENARIO": "db-create", "FAKE_FAIL_START": "1",
                                  "FAKE_DIAGNOSTIC_ARTIFACT_BLOCK": str(self.artifacts / "diagnostics.jsonl")})
        self.assertEqual(result.returncode, 23, result.stderr)
        self.assertNotIn("diagnostic-password-sentinel", result.stdout + result.stderr)
        self.assertTrue((self.artifacts / "diagnostics.jsonl").is_dir())
        events = self.event_rows()
        self.assertFalse(any(e["tool"] == "supabase" and e["args"][:2] == ["db", "reset"] for e in events))
        docker = [e["args"] for e in events if e["tool"] == "docker"]
        container = docker.index(["container", "rm", "--force", "c" * 64])
        volume = next(i for i, args in enumerate(docker) if args[:2] == ["volume", "rm"])
        network = next(i for i, args in enumerate(docker) if args[:2] == ["network", "rm"])
        self.assertLess(container, volume)
        self.assertLess(volume, network)

    def test_diagnostic_temp_sink_failure_is_silent_and_preserves_start_exit(self):
        real_mktemp = shutil.which("mktemp")
        fake_mktemp = self.bin / "mktemp"
        fake_mktemp.write_text("#!/bin/sh\ncase \"$*\" in\n *start-diagnostics*) printf 'diagnostic-password-sentinel' >&2; exit 41;;\nesac\nexec \"" + real_mktemp + "\" \"$@\"\n")
        fake_mktemp.chmod(0o755)
        result = self.run_runner({"FAKE_PRODUCT_GUARD_SCENARIO": "db-create", "FAKE_FAIL_START": "1"})
        self.assertEqual(result.returncode, 23, result.stderr)
        self.assertNotIn("diagnostic-password-sentinel", result.stderr)
        self.assertTrue(json.loads(self.state.read_text())["container_removed"])

    def test_exit_retries_helpers_before_db_cleanup_and_preserves_cli_failure(self):
        for failure in ("FAKE_HELPER_INSPECT_TRANSIENT", "FAKE_HELPER_REMOVE_TRANSIENT"):
            with self.subTest(failure=failure):
                if self.artifacts.exists():
                    shutil.rmtree(self.artifacts)
                self.events.unlink(missing_ok=True)
                self.state.unlink(missing_ok=True)
                result = self.run_runner({"FAKE_PRODUCT_GUARD_SCENARIO": "helper",
                    "FAKE_HELPER_RETAIN": "1", "FAKE_HELPER_EXIT": "1", failure: "1"})
                self.assertEqual(result.returncode, 44, result.stderr)
                state = json.loads(self.state.read_text())
                self.assertIsNone(state.get("helper"))
                docker = [e["args"] for e in self.event_rows() if e["tool"] == "docker"]
                helper_removal = max(i for i, args in enumerate(docker) if args == ["container", "rm", "-f", "d" * 64])
                db_removal = next(i for i, args in enumerate(docker) if args[:3] == ["container", "rm", "--force"])
                self.assertLess(helper_removal, db_removal)
                self.assertTrue(state["volume_removed"])
                self.assertFalse(state["network_created"])

    def test_exit_foreign_helper_is_preserved_with_recovery_evidence(self):
        result = self.run_runner({"FAKE_PRODUCT_GUARD_SCENARIO": "helper", "FAKE_HELPER_RETAIN": "1",
                                  "FAKE_HELPER_EXIT": "1", "FAKE_HELPER_FOREIGN": "1"})
        self.assertEqual(result.returncode, 44, result.stderr)
        state = json.loads(self.state.read_text())
        workdir = Path(state["workdir"])
        try:
            self.assertIsNotNone(state["helper"])
            self.assertTrue((workdir / ".product-docker-guard" / "state.json").is_file())
            self.assertIn("kind=helper", (self.artifacts / "cleanup.log").read_text())
        finally:
            if workdir.is_dir():
                shutil.rmtree(workdir)

    def test_guard_network_name_propagates_to_all_runner_phases(self):
        scenarios = (({"FAKE_FAIL_RESET": "1"}, 37, {"start", "reset", "cleanup"}),
                     ({"FAKE_FAIL_START": "1"}, 23, {"start", "diagnostic", "cleanup"}))
        for extra, status, phases in scenarios:
            with self.subTest(extra=extra):
                self.events.unlink(missing_ok=True)
                self.state.unlink(missing_ok=True)
                result = self.run_runner({"FAKE_PRODUCT_GUARD_SCENARIO": "full-bootstrap", **extra}, self.root / ("artifacts-" + str(status)))
                self.assertEqual(result.returncode, status, result.stderr)
                events = [row for row in self.event_rows() if row["tool"] == "guard"]
                self.assertEqual({row["guard_phase"] for row in events}, phases)
                names = {row.get("guard_network_name") for row in events}
                self.assertEqual(len(names), 1)
                self.assertRegex(next(iter(names)) or "", r"^token-planet-ci-net-[0-9a-f]{24}$")

    def test_guarded_reset_recreates_database_and_runs_all_helpers_in_both_phases(self):
        result = self.run_runner({"FAKE_PRODUCT_GUARD_SCENARIO": "full-bootstrap"})
        self.assertEqual(result.returncode, 0, result.stderr)
        docker = [event["args"] for event in self.event_rows() if event["tool"] == "docker"]
        creates = [args for args in docker if args[:1] == ["create"]]
        self.assertEqual(len(creates), 2)
        runs = [args for args in docker if args[:1] == ["run"]]
        self.assertEqual(len(runs), 6)
        images = [next(arg for arg in args if arg.startswith("public.ecr.aws/supabase/")) for args in runs]
        self.assertEqual(images, ["public.ecr.aws/supabase/realtime:v2.140.3",
                                  "public.ecr.aws/supabase/storage-api:v1.79.28",
                                  "public.ecr.aws/supabase/gotrue:v2.197.0"] * 2)
        self.assertEqual(len({args[args.index("--cidfile") + 1] for args in runs}), 6)
        restarts = [event for event in self.event_rows() if event["tool"] == "supabase_docker" and event["args"][:1] == ["restart"]]
        self.assertEqual(len(restarts), 4)
        self.assertEqual({event["args"][1].split("_")[1] for event in restarts}, {"storage", "auth", "realtime", "pooler"})
        self.assertTrue(all(event["status"] == 1 for event in restarts))
        self.assertFalse(any(args[:1] == ["restart"] for args in docker))
        self.assertTrue(any(args[:2] == ["container", "inspect"] and args[2].startswith("supabase_kong_") for args in docker))

    def test_full_reset_existing_satellite_is_denied_without_restart(self):
        result = self.run_runner({"FAKE_PRODUCT_GUARD_SCENARIO": "full-bootstrap", "FAKE_SATELLITE_EXISTING": "auth"})
        self.assertEqual(result.returncode, 125, result.stderr)
        docker = [event["args"] for event in self.event_rows() if event["tool"] == "docker"]
        self.assertFalse(any(args[:1] == ["restart"] for args in docker))

    def test_full_reset_stopped_kong_is_noop_and_running_kong_exec_is_denied(self):
        for mode, status in (("stopped", 0), ("running", 125)):
            with self.subTest(mode=mode):
                if self.artifacts.exists():
                    shutil.rmtree(self.artifacts)
                self.events.unlink(missing_ok=True)
                self.state.unlink(missing_ok=True)
                result = self.run_runner({"FAKE_PRODUCT_GUARD_SCENARIO": "full-bootstrap", "FAKE_KONG_STATE": mode})
                self.assertEqual(result.returncode, status, result.stderr)
                docker = [event["args"] for event in self.event_rows() if event["tool"] == "docker"]
                self.assertFalse(any(args[:1] == ["restart"] or (args[:1] == ["exec"] and args[1].startswith("supabase_kong_")) for args in docker))

    def test_malformed_tap_record_fails_instead_of_being_ignored(self):
        result = self.run_runner({"FAKE_TAP": "1..1\nok 1 - fake test\nok banana"})

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("pgTAP output was incomplete or contained a failure", (self.artifacts / "run.log").read_text())

    def test_remote_docker_context_is_refused_before_creating_resources(self):
        result = self.run_runner({"FAKE_DOCKER_ENDPOINT": "ssh://docker.example"})

        self.assertEqual(result.returncode, 2, result.stdout)
        self.assertIn("Docker daemon must use a local Unix socket endpoint", (self.artifacts / "run.log").read_text())
        docker = [event["args"] for event in self.event_rows() if event["tool"] == "docker"]
        self.assertFalse(any(args[:2] == ["network", "create"] for args in docker))

    def test_wrong_cli_version_is_refused_before_docker_calls(self):
        result = self.run_runner({"FAKE_CLI_VERSION": "2.118.0"})

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("expected Supabase CLI 2.119.0", (self.artifacts / "run.log").read_text())
        self.assertFalse(any(event["tool"] == "docker" for event in self.event_rows()))
        self.assertEqual(self.source_cli_latest.read_text(), "source-cli-cache-baseline")

    def test_artifact_path_with_parent_hop_resolving_inside_repo_is_refused(self):
        detour = self.root / "detour"
        detour.mkdir()
        artifacts_path = detour / ".." / "repo" / "parent-hop-artifacts"

        result = self.run_runner(artifacts_path=artifacts_path)

        self.assertEqual(result.returncode, 2)
        self.assertIn("artifacts directory must be outside the repository", result.stderr)
        self.assertFalse((self.repo / "parent-hop-artifacts").exists())
        self.assertEqual(self.event_rows(), [])

    def test_artifact_path_through_symlinked_ancestor_into_repo_is_refused(self):
        repo_alias = self.root / "repo-alias"
        repo_alias.symlink_to(self.repo, target_is_directory=True)
        artifacts_path = repo_alias / "symlinked-artifacts"

        result = self.run_runner(artifacts_path=artifacts_path)

        self.assertEqual(result.returncode, 2)
        self.assertIn("artifacts directory must be outside the repository", result.stderr)
        self.assertFalse((self.repo / "symlinked-artifacts").exists())
        self.assertEqual(self.event_rows(), [])

    def test_remote_supabase_credentials_are_refused_before_cli_or_docker_calls(self):
        result = self.run_runner({"SUPABASE_ACCESS_TOKEN": "fixture-token"})

        self.assertEqual(result.returncode, 2)
        self.assertIn("refusing Supabase environment override SUPABASE_ACCESS_TOKEN", result.stderr)
        self.assertEqual(self.event_rows(), [])

    def test_database_connection_overrides_are_refused_before_cli_or_docker_calls(self):
        for name in ("DATABASE_URL", "PGHOST", "PGHOSTADDR", "PGPORT", "PGDATABASE", "PGUSER", "PGPASSWORD", "PGSERVICE", "PGSERVICEFILE"):
            with self.subTest(name=name):
                result = self.run_runner({name: "fixture-remote-value"})
                self.assertEqual(result.returncode, 2)
                self.assertIn(name, result.stderr)
                self.assertEqual(self.event_rows(), [])

    def test_project_id_override_is_refused_before_cli_or_docker_calls(self):
        result = self.run_runner({"SUPABASE_PROJECT_ID": "token-planet"})

        self.assertEqual(result.returncode, 2)
        self.assertIn("refusing Supabase environment override SUPABASE_PROJECT_ID", result.stderr)
        self.assertEqual(self.event_rows(), [])

    def test_docker_host_override_is_refused_before_docker_calls(self):
        result = self.run_runner({"DOCKER_HOST": "tcp://docker.example:2376"})

        self.assertEqual(result.returncode, 2)
        self.assertIn("refusing Docker connection override: DOCKER_HOST", (self.artifacts / "run.log").read_text())
        self.assertFalse(any(event["tool"] == "docker" for event in self.event_rows()))

    def test_network_create_failure_preserves_status_without_cli_start(self):
        result = self.run_runner({"FAKE_FAIL_NETWORK_CREATE": "1"})

        self.assertEqual(result.returncode, 32)
        self.assertFalse(any(event["tool"] == "supabase" and event["args"][0] == "start" for event in self.event_rows()))
        docker = [event["args"] for event in self.event_rows() if event["tool"] == "docker"]
        self.assertFalse(any(args[:2] == ["network", "rm"] for args in docker))
        self.assertIn("cleanup_status=SKIP kind=network_inspect code=confirmed_not_found", (self.artifacts / "cleanup.log").read_text())

    def test_start_failure_with_missing_container_and_volume_is_confirmed_absence(self):
        result = self.run_runner({"FAKE_FAIL_START_BEFORE_CONTAINER": "1"})

        self.assertEqual(result.returncode, 23)
        cleanup = (self.artifacts / "cleanup.log").read_text()
        self.assertIn("cleanup_status=SKIP kind=container_inspect code=confirmed_not_found", cleanup)
        self.assertIn("cleanup_status=SKIP kind=volume_inspect code=confirmed_not_found", cleanup)
        self.assertNotIn("cleanup_status=FAIL", cleanup)

    def test_cleanup_daemon_inspect_errors_fail_success_without_removing_unknown_resources(self):
        result = self.run_runner({"FAKE_CLEANUP_INSPECT_ERROR": "all"})

        self.assertNotEqual(result.returncode, 0)
        cleanup = (self.artifacts / "cleanup.log").read_text()
        for kind in ("container", "volume", "network"):
            self.assertIn(f"cleanup_status=FAIL kind={kind}_inspect code=inspect_error exit=1", cleanup)
        self.assertNotIn("Cannot connect to the Docker daemon", cleanup)
        docker = [event["args"] for event in self.event_rows() if event["tool"] == "docker"]
        self.assertFalse(any(args[:2] in (["container", "rm"], ["volume", "rm"], ["network", "rm"]) for args in docker))

    def test_port_collision_stops_before_network_or_database_start(self):
        result = self.run_runner({"FAKE_PORT_BUSY": "1"})

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("one or more required local ports are unavailable", (self.artifacts / "run.log").read_text())
        events = self.event_rows()
        self.assertFalse(any(event["tool"] == "supabase" and event["args"][0] == "start" for event in events))
        self.assertFalse(any(event["tool"] == "docker" and event["args"][:2] == ["network", "create"] for event in events))

    def test_reset_failure_exports_only_validated_evidence(self):
        evidence = '\n'.join(json.dumps(row) for row in [
            {"phase": "start", "code": "DOCKER_COMMAND_IS_NOT_APPROVED"},
            {"phase": "reset", "code": "DB_INSPECT_NETWORK_ATTACHMENT_ID_REJECTED"},
            {"phase": "cleanup", "code": "DOCKER_COMMAND_IS_NOT_APPROVED"}])
        result = self.run_runner({"FAKE_FAIL_RESET": "1", "FAKE_RESET_EVIDENCE": evidence,
            "FAKE_RESET_OUTPUT": "Starting ID-sentinel\nApplying migration {migration}...\nApplying migration outside.sql...\nERROR: discarded SQL-sentinel (SQLSTATE 23505)\nERROR: password=credential-sentinel (SQLSTATE 22000)"})
        self.assertEqual(result.returncode, 37)
        path = self.artifacts / "reset-diagnostics.json"
        self.assertTrue(path.is_file(), "reset diagnostic artifact missing")
        row = json.loads(path.read_text())
        manifest = json.loads((self.artifacts / "manifest.json").read_text())
        self.assertEqual(row, {"guard_codes": ["DB_INSPECT_NETWORK_ATTACHMENT_ID_REJECTED"],
            "last_announced_migration": manifest["entries"][0]["staged_filename"], "sqlstate": "23505",
            "diagnostic_status": "ok", "error_class": "guard_and_sql_error_observed"})
        self.assertFalse(any(e["tool"] == "docker" and e["args"][:1] == ["exec"] for e in self.event_rows()))
        self.assertTrue(json.loads(self.state.read_text())["container_removed"])
        for artifact in self.artifacts.rglob("*"):
            if artifact.is_file():
                for sentinel in ("SQL-sentinel", "credential-sentinel", "ID-sentinel"):
                    self.assertNotIn(sentinel, artifact.read_text())

    def test_reset_diagnostics_reject_invalid_and_ambiguous_inputs(self):
        cases = [("ERROR: msg (SQLSTATE 23505)\nERROR: msg (SQLSTATE 22000)", "", "ambiguous"),
                 ("ERROR: msg (SQLSTATE abcde)\nprefix ERROR: msg (SQLSTATE 23505)\nERROR: msg (SQLSTATE 23505) suffix", "", "ok"),
                 ("unclassified SQL-sentinel", '{"phase":"reset","code":"credential-sentinel"}', "invalid"),
                 ("x" * 65537, "", "truncated"), ("x" * 4097, "", "truncated")]
        for output, evidence, status in cases:
            with self.subTest(status=status, length=len(output)):
                shutil.rmtree(self.artifacts, ignore_errors=True)
                self.events.unlink(missing_ok=True)
                self.state.unlink(missing_ok=True)
                result = self.run_runner({"FAKE_FAIL_RESET": "1", "FAKE_RESET_OUTPUT": output, "FAKE_RESET_EVIDENCE": evidence})
                self.assertEqual(result.returncode, 37)
                path = self.artifacts / "reset-diagnostics.json"
                self.assertTrue(path.is_file(), "reset diagnostic artifact missing")
                row = json.loads(path.read_text())
                self.assertEqual(row["diagnostic_status"], status)
                self.assertEqual(row["sqlstate"], None)
                self.assertEqual(row["error_class"], "unknown")
                self.assertTrue(json.loads(self.state.read_text())["volume_removed"])

    def test_reset_diagnostic_input_and_sink_failures_preserve_exit_cleanup(self):
        for mode in ("symlink", "permissions", "corrupt", "sink", "diagnostic"):
            with self.subTest(mode=mode):
                shutil.rmtree(self.artifacts, ignore_errors=True)
                self.events.unlink(missing_ok=True)
                self.state.unlink(missing_ok=True)
                extra = {"FAKE_FAIL_RESET": "1"}
                if mode == "sink": extra["FAKE_RESET_SINK_BLOCK"] = str(self.artifacts / "reset-diagnostics.json")
                elif mode == "diagnostic": extra["FAKE_RESET_DIAGNOSTIC_FAIL"] = "1"
                else: extra["FAKE_RESET_RAW_MODE"] = mode
                result = self.run_runner(extra)
                self.assertEqual(result.returncode, 37, result.stderr)
                self.assertNotIn("credential-sentinel", result.stdout + result.stderr)
                state = json.loads(self.state.read_text())
                self.assertTrue(state["container_removed"])
                self.assertTrue(state["volume_removed"])
                self.assertFalse(state["network_created"])
                if mode not in ("sink", "diagnostic"):
                    row = json.loads((self.artifacts / "reset-diagnostics.json").read_text())
                    self.assertEqual(row["diagnostic_status"], "invalid")

    def test_reset_manifest_unsafe_inputs_cannot_delay_exit_cleanup(self):
        for mode in ("fifo", "symlink", "oversize", "missing"):
            with self.subTest(mode=mode):
                shutil.rmtree(self.artifacts, ignore_errors=True)
                self.events.unlink(missing_ok=True)
                self.state.unlink(missing_ok=True)
                env = self.env.copy()
                env.update({"FAKE_FAIL_RESET": "1", "FAKE_RESET_MANIFEST_MODE": mode})
                child = subprocess.Popen(["bash", str(self.runner), "--artifacts-dir", str(self.artifacts)],
                    cwd=self.repo, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                    text=True, start_new_session=True)
                try:
                    stdout, stderr = child.communicate(timeout=10)
                except subprocess.TimeoutExpired:
                    os.killpg(child.pid, signal.SIGKILL)
                    child.communicate()
                    self.fail("unsafe manifest delayed reset exit and cleanup")
                self.assertEqual(child.returncode, 37, stderr)
                row = json.loads((self.artifacts / "reset-diagnostics.json").read_text())
                self.assertEqual(row["diagnostic_status"], "unavailable" if mode == "missing" else "invalid")
                self.assertEqual(row["last_announced_migration"], None)
                state = json.loads(self.state.read_text())
                self.assertTrue(state["container_removed"])
                self.assertTrue(state["volume_removed"])
                self.assertFalse(state["network_created"])
                self.assertFalse(any(e["tool"] == "docker" and e["args"][:1] == ["exec"] for e in self.event_rows()))

    def test_reset_failure_preserves_status_skips_tap_and_cleans_owned_resources(self):
        result = self.run_runner({"FAKE_FAIL_RESET": "1"})

        self.assertEqual(result.returncode, 37)
        events = self.event_rows()
        docker = [event["args"] for event in events if event["tool"] == "docker"]
        cli = [event["args"] for event in events if event["tool"] == "supabase"]
        self.assertFalse(any(args[0] == "exec" and "-f" in args for args in docker))
        self.assertTrue(any(args[:2] == ["container", "rm"] for args in docker))
        self.assertTrue(any(args[:2] == ["volume", "rm"] for args in docker))
        self.assertTrue(any(args[:2] == ["network", "rm"] for args in docker))
        self.assertNotIn("fixture-password", "\n".join(p.read_text(errors="replace") for p in self.artifacts.iterdir() if p.is_file()))

    def test_reset_recreated_container_is_reverified_and_cleaned(self):
        result = self.run_runner({"FAKE_RESET_RECREATE": "1"})

        self.assertEqual(result.returncode, 0, result.stderr)
        docker = [event["args"] for event in self.event_rows() if event["tool"] == "docker"]
        container_inspects = [args for args in docker if args[:2] == ["container", "inspect"]]
        expected_name = "supabase_db_" + json.loads(self.state.read_text())["project_id"]
        self.assertEqual(container_inspects[-1][-1], expected_name)
        self.assertTrue(any(args[:2] == ["container", "rm"] and args[-1] == "recreated-container-id" for args in docker))

    def test_reset_failure_after_recreation_cleans_replacement_by_verified_labels(self):
        result = self.run_runner({"FAKE_FAIL_RESET_AFTER_RECREATE": "1"})

        self.assertEqual(result.returncode, 37)
        docker = [event["args"] for event in self.event_rows() if event["tool"] == "docker"]
        self.assertTrue(any(args[:2] == ["container", "rm"] and args[-1] == "recreated-container-id" for args in docker))
        self.assertTrue(any(args[:2] == ["volume", "rm"] for args in docker))
        self.assertTrue(any(args[:2] == ["network", "rm"] for args in docker))

    def test_reset_cannot_move_database_off_owned_loopback_bridge(self):
        result = self.run_runner({"FAKE_MISMATCH_NETWORK": "1"})

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("not attached to the owned CI bridge after reset", (self.artifacts / "run.log").read_text())
        docker = [event["args"] for event in self.event_rows() if event["tool"] == "docker"]
        self.assertFalse(any(args[0] == "exec" and "-f" in args for args in docker))
        self.assertTrue(any(args[:2] == ["container", "rm"] for args in docker))
        self.assertTrue(any(args[:2] == ["network", "rm"] for args in docker))
        self.assertIn("network_status=FAIL code=bridge_attachment", (self.artifacts / "container-network-reset.log").read_text())

    def test_wildcard_actual_postgres_binding_is_rejected_with_safe_details(self):
        result = self.run_runner({"FAKE_ACTUAL_HOSTIP": "0.0.0.0"})

        self.assertNotEqual(result.returncode, 0)
        summary = (self.artifacts / "container-binding-start.log").read_text()
        self.assertIn("binding_status=FAIL code=published_host_ip", summary)
        self.assertIn("actual_ip=0.0.0.0", summary)
        self.assertNotIn("fixture-password", summary)

    def test_missing_actual_postgres_binding_is_rejected_with_safe_details(self):
        result = self.run_runner({"FAKE_EMPTY_ACTUAL_BINDING": "1"})

        self.assertNotEqual(result.returncode, 0)
        summary = (self.artifacts / "container-binding-start.log").read_text()
        self.assertIn("binding_status=FAIL code=postgres_binding_missing", summary)
        self.assertNotIn("fixture-password", summary)

    def test_pg_tap_assertion_failure_cleans_owned_resources(self):
        result = self.run_runner({"FAKE_TAP": "1..1\nnot ok 1 - fake test"})

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("pgTAP output was incomplete or contained a failure", (self.artifacts / "run.log").read_text())
        summary = (self.artifacts / "test-sample.tap-summary.log").read_text()
        self.assertIn("TAP status=FAIL", summary)
        self.assertIn("failing_numbers=1", summary)
        docker = [event["args"] for event in self.event_rows() if event["tool"] == "docker"]
        self.assertTrue(any(args[:2] == ["container", "rm"] for args in docker))
        self.assertTrue(any(args[:2] == ["volume", "rm"] for args in docker))
        self.assertTrue(any(args[:2] == ["network", "rm"] for args in docker))

    def test_psql_failure_keeps_safe_exit_summary_and_cleans_resources(self):
        result = self.run_runner({"FAKE_FAIL_PSQL": "1"})

        self.assertEqual(result.returncode, 45)
        summary = (self.artifacts / "test-sample.tap-summary.log").read_text()
        self.assertIn("TAP status=ERROR psql_exit=45", summary)
        self.assertNotIn("fixture-password", "\n".join(p.read_text(errors="replace") for p in self.artifacts.iterdir() if p.is_file()))
        docker = [event["args"] for event in self.event_rows() if event["tool"] == "docker"]
        self.assertTrue(any(args[:2] == ["container", "rm"] for args in docker))
        self.assertTrue(any(args[:2] == ["volume", "rm"] for args in docker))
        self.assertTrue(any(args[:2] == ["network", "rm"] for args in docker))

    def test_empty_tap_plan_is_rejected_with_safe_summary(self):
        result = self.run_runner({"FAKE_TAP": "1..0"})

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("TAP status=INVALID empty_plan", (self.artifacts / "test-sample.tap-summary.log").read_text())

    def test_tap_skip_directive_is_rejected(self):
        result = self.run_runner({"FAKE_TAP": "1..1\nok 1 - fake test # SKIP not supported"})

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("TAP status=INVALID directive=SKIP", (self.artifacts / "test-sample.tap-summary.log").read_text())

    def test_tap_todo_directive_is_rejected(self):
        result = self.run_runner({"FAKE_TAP": "1..1\nok 1 - fake test # TODO not supported"})

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("TAP status=INVALID directive=TODO", (self.artifacts / "test-sample.tap-summary.log").read_text())

    def test_cleanup_skips_container_with_mismatched_identity(self):
        result = self.run_runner({"FAKE_MISMATCH_CONTAINER": "1"})

        self.assertNotEqual(result.returncode, 0)
        docker = [event["args"] for event in self.event_rows() if event["tool"] == "docker"]
        self.assertFalse(any(args[:2] == ["container", "rm"] for args in docker))
        self.assertTrue(any(args[:2] == ["volume", "rm"] for args in docker))
        self.assertTrue(any(args[:2] == ["network", "rm"] for args in docker))
        self.assertIn("Skipped container with mismatched identity", (self.artifacts / "cleanup.log").read_text())


if __name__ == "__main__":
    unittest.main()
