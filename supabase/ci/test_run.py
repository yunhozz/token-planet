import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import textwrap
import unittest


RUNNER = Path(__file__).with_name("run.sh")


class RunnerTests(unittest.TestCase):
    FAKE_SUPABASE = r'''#!/usr/bin/env python3
import json, os, pathlib, re, sys
args = sys.argv[1:]
events = pathlib.Path(os.environ["FAKE_EVENTS"])
state_path = pathlib.Path(os.environ["FAKE_STATE"])
def record(tool, args, **extra):
    with events.open("a") as stream:
        stream.write(json.dumps({"tool": tool, "args": args, **extra}) + "\n")
state = json.loads(state_path.read_text()) if state_path.exists() else {}
record("supabase", args, cwd=os.getcwd())
if args == ["--version"]:
    cache = pathlib.Path("supabase/.temp/cli-latest")
    cache.parent.mkdir(parents=True, exist_ok=True)
    cache.write_text("fake-cli-latest")
    print(os.environ.get("FAKE_CLI_VERSION", "2.119.0"))
elif args and args[0] == "start":
    config = pathlib.Path("supabase/config.toml").read_text()
    state["project_id"] = re.search(r'^project_id\s*=\s*"([^"]+)"', config, re.M).group(1)
    state["workdir"] = os.getcwd()
    state["started"] = os.environ.get("FAKE_FAIL_START_BEFORE_CONTAINER") != "1"
    state["network_id"] = args[args.index("--network-id") + 1]
    manifest = json.loads(pathlib.Path("supabase/migrations/manifest.json").read_text())
    state["versions"] = [entry["synthetic_version"] for entry in manifest["entries"]]
    state_path.write_text(json.dumps(state))
    print("anon key: eyJfixture.header.signature")
    if os.environ.get("FAKE_FAIL_START") == "1" or os.environ.get("FAKE_FAIL_START_BEFORE_CONTAINER") == "1":
        sys.exit(23)
elif args[:2] == ["db", "reset"]:
    print("Database URL: postgres://postgres:fixture-password@localhost:56432/postgres")
    state["reset_complete"] = True
    if os.environ.get("FAKE_RESET_RECREATE") == "1" or os.environ.get("FAKE_FAIL_RESET_AFTER_RECREATE") == "1":
        state["container_id"] = "recreated-container-id"
    state_path.write_text(json.dumps(state))
    if os.environ.get("FAKE_FAIL_RESET") == "1" or os.environ.get("FAKE_FAIL_RESET_AFTER_RECREATE") == "1":
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
    state.update(network_created=True, network_id="owned-network-id",
                 network_name=args[-1], network_labels=labels)
    write_state(state)
    print(state["network_id"])
elif args[:2] == ["container", "inspect"]:
    target = args[-1]
    inspect_attempt("container", 4)
    if not state.get("started") or state.get("container_removed"):
        print(f"Error: No such object: {target}", file=sys.stderr)
        sys.exit(1)
    project_id = state["project_id"]
    if os.environ.get("FAKE_MISMATCH_CONTAINER") == "1":
        project_id += "-foreign"
    attached_network_id = state.get("network_id")
    if os.environ.get("FAKE_MISMATCH_NETWORK") == "1" and state.get("reset_complete"):
        attached_network_id = "foreign-network-id"
    actual_host_ip = os.environ.get("FAKE_ACTUAL_HOSTIP", "127.0.0.1")
    actual_ports = [] if os.environ.get("FAKE_EMPTY_ACTUAL_BINDING") == "1" else [
        {"HostIp": actual_host_ip, "HostPort": "56432"}
    ]
    print(json.dumps({"Id": state.get("container_id", "owned-container-id"), "Name": "/supabase_db_" + project_id,
                      "Config": {"Labels": {"com.supabase.cli.project": project_id,
                                               "com.supabase.cli.workdir": state["workdir"]}},
                      "NetworkSettings": {"Networks": {state.get("network_name", "owned-network"): {"NetworkID": attached_network_id}},
                                          "Ports": {"5432/tcp": actual_ports}},
                      "HostConfig": {"PortBindings": {"5432/tcp": [{"HostIp": "", "HostPort": "56432"}]}}}))
elif args[:2] == ["volume", "inspect"]:
    target = args[-1]
    inspect_attempt("volume", 3)
    if not state.get("started") or state.get("volume_removed"):
        print(f"Error: No such volume: {target}", file=sys.stderr)
        sys.exit(1)
    print(json.dumps({"Name": "supabase_db_" + state["project_id"],
                      "Labels": {"com.supabase.cli.project": state["project_id"]}}))
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
        self.assertTrue(any(args[:2] == ["network", "rm"] and args[-1] == "owned-network-id" for args in docker))
        self.assertFalse(any(args[:2] == ["db", "reset"] for args in [event["args"] for event in events if event["tool"] == "supabase"]))
        self.assertTrue(any(args[:2] == ["container", "rm"] for args in docker))
        self.assertTrue(any(args[:2] == ["volume", "rm"] for args in docker))

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
