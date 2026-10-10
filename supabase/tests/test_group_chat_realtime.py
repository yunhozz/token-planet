"""Pure local-harness safety tests; no database or network calls."""
import importlib.util
import json
from pathlib import Path
import unittest
from unittest.mock import patch

PATH = Path(__file__).with_name("group_chat_realtime.py")
spec = importlib.util.spec_from_file_location("group_chat_realtime", PATH)
harness = importlib.util.module_from_spec(spec)
spec.loader.exec_module(harness)

class HarnessSafetyTests(unittest.TestCase):
    def test_exact_local_endpoint_only(self):
        self.assertEqual(harness.validate_api("http://127.0.0.1:54321"), harness.API)
        for value in ["https://example.supabase.co", "http://localhost:54321", "http://127.0.0.1:54322", "http://127.0.0.1:54321/", "http://user:secret@127.0.0.1:54321", "http://127.0.0.1:54321?target=remote"]:
            with self.subTest(value=value), self.assertRaises(harness.HarnessFailure): harness.validate_api(value)
    def test_target_overrides_and_proxies_rejected(self):
        for key in ["DATABASE_URL", "SUPABASE_ACCESS_TOKEN", "DOCKER_HOST", "DOCKER_CONTEXT", "HTTP_PROXY", "http_proxy", "ALL_PROXY", "PGHOST"]:
            with self.subTest(key=key), self.assertRaises(harness.HarnessFailure): harness.validate_environment({key: "secret"})
        harness.validate_environment({})
    def test_container_project_path_port_and_unix_socket_guard(self):
        fixture = {"Id": "a" * 64, "Name": "/supabase_db_token-planet-group-chat-qa", "State": {"Running": True}, "Config": {"Labels": {"com.supabase.cli.project": "token-planet-group-chat-qa", "com.supabase.cli.workdir": "/tmp/token-planet-group-chat-supabase"}}, "NetworkSettings": {"Ports": {"5432/tcp": [{"HostIp": "127.0.0.1", "HostPort": "54322"}]}}}
        context = {"Endpoints": {"docker": {"Host": "unix:///var/run/docker.sock"}}}
        self.assertEqual(harness.validate_container(fixture, context), "a" * 64)
        for path, value in [( ["Name"], "/production"), (["Config", "Labels", "com.supabase.cli.project"], "production"), (["Config", "Labels", "com.supabase.cli.workdir"], "/tmp/foreign"), (["NetworkSettings", "Ports", "5432/tcp"], [{"HostIp": "127.0.0.1", "HostPort": "55432"}])]:
            changed = json.loads(json.dumps(fixture)); node = changed
            for key in path[:-1]: node = node[key]
            node[path[-1]] = value
            with self.subTest(path=path), self.assertRaises(harness.HarnessFailure): harness.validate_container(changed, context)
        with self.assertRaises(harness.HarnessFailure): harness.validate_container(fixture, {"Endpoints": {"docker": {"Host": "ssh://remote"}}})
    def test_aggregated_report_cannot_include_exception_body_or_token(self):
        results = harness.Results()
        results.record("R01", True)
        results.record("R02", False)
        encoded = json.dumps(results.report())
        self.assertIn('"proven"', encoded); self.assertIn('"unmet"', encoded)
        with self.assertRaises(harness.HarnessFailure): results.record("secret body token", False)
        error = harness.safe_error(RuntimeError("Bearer secret-token user-private-body"))
        self.assertEqual(error, "operation_failed")
        self.assertEqual(harness.safe_error(harness.HarnessFailure("auth_fixture_failed")), "auth_fixture_failed")
        self.assertEqual(harness.safe_error(harness.HarnessFailure("Bearer secret-token")), "operation_failed")
        self.assertNotIn("secret", json.dumps({"result": results.report(), "error": error}))
    def test_command_timeout_is_bounded_and_diagnostics_withheld(self):
        import subprocess
        with patch.object(harness.subprocess, "run", side_effect=subprocess.TimeoutExpired(["secret"], 20, output="private body")) as run:
            with self.assertRaises(harness.HarnessFailure) as error: harness.run_command(["docker", "inspect", "local"])
            self.assertEqual(str(error.exception), "command_timeout")
            self.assertEqual(run.call_args.kwargs["timeout"], 20)
    def test_http_redirect_is_rejected_without_following(self):
        with self.assertRaises(harness.HarnessFailure): harness.NoRedirect().redirect_request(None, None, 302, "secret", {}, "https://remote")
    def test_world_creation_uses_minimal_before_membership_select(self):
        from unittest.mock import Mock
        target = Mock()
        target.http.side_effect = [(201, None), (200, [{"id": "22222222-2222-4222-8222-222222222222"}])]
        verification = harness.Verification(target, harness.Results())
        value = verification.world({"access_token": "in-memory-test-token", "user": {"id": "11111111-1111-4111-8111-111111111111"}})
        self.assertEqual(value, "22222222-2222-4222-8222-222222222222")
        self.assertEqual(target.http.call_args_list[0].args[-1], {"Prefer": "return=minimal"})
    def launcher(self, workdir):
        import subprocess
        script = PATH.parents[2] / "apps/desktop/scripts/dev-local.mjs"
        code = r"""
const fs = require('fs'), vm = require('vm'), path = require('path'), url = require('url');
const calls = [], fakeProcess = { env: { npm_execpath: 'fixture-npm', ...(process.argv[2] ? {TOKEN_PLANET_LOCAL_SUPABASE_WORKDIR: process.argv[2]} : {}) }, execPath: 'fixture-node', exit: () => {throw Error('safe-exit')}, exitCode: 0 };
const context = { dirname:path.dirname, resolve:path.resolve, fileURLToPath:url.fileURLToPath, URL, console:{error:()=>{}}, process:fakeProcess,
  spawnSync: (...args) => { calls.push(args); return {status:0, stdout:JSON.stringify({API_URL:'http://127.0.0.1:54321', PUBLISHABLE_KEY:'fixture-public-key'})}; },
  spawn: () => ({on:()=>{}}) };
let source = fs.readFileSync(process.argv[1],'utf8').replace(/^import .*;$/gm,'').replace(/import\.meta\.url/g,JSON.stringify(url.pathToFileURL(process.argv[1]).href));
let failed = false; try {vm.runInNewContext(source,context)} catch {failed = true}
console.log(JSON.stringify({calls,failed}));
"""
        run = subprocess.run(["node", "-e", code, str(script), workdir], capture_output=True, text=True, timeout=5, check=True)
        return json.loads(run.stdout)
    def test_local_launcher_safe_default_explicit_scratch_and_timeout(self):
        default = self.launcher("")
        self.assertFalse(default["failed"])
        self.assertEqual(default["calls"][0][2]["cwd"], str(PATH.parents[2]))
        scratch = self.launcher("/tmp/token-planet-group-chat-supabase")
        self.assertFalse(scratch["failed"])
        self.assertIn("--workdir", scratch["calls"][0][1])
        self.assertIn("/tmp/token-planet-group-chat-supabase", scratch["calls"][0][1])
        self.assertEqual(scratch["calls"][0][2]["timeout"], 20000)
        foreign = self.launcher("/tmp/foreign")
        self.assertTrue(foreign["failed"])
        self.assertEqual(foreign["calls"], [])
    def test_realtime_heartbeat_is_phoenix_scoped_and_bounded(self):
        from unittest.mock import Mock
        connection = harness.Realtime.__new__(harness.Realtime)
        connection.closed = False; connection.last_heartbeat = 0; connection.send = Mock()
        connection.tick(14)
        connection.send.assert_not_called()
        connection.tick(15)
        connection.send.assert_called_once_with("heartbeat", {}, topic="phoenix")
        connection.closed = True; connection.tick(40)
        connection.send.assert_called_once()
    def test_subscription_readiness_requires_postgres_changes_success(self):
        self.assertFalse(harness.subscription_ready({"event": "system", "payload": {"extension": "broadcast", "status": "ok"}}))
        self.assertFalse(harness.subscription_ready({"event": "system", "payload": {"extension": "postgres_changes", "status": "error"}}))
        self.assertTrue(harness.subscription_ready({"event": "system", "payload": {"extension": "postgres_changes", "status": "ok"}}))
    def test_failed_check_preserves_only_safe_failure_code(self):
        results = harness.Results(); verification = harness.Verification(None, results)
        with self.assertRaises(harness.HarnessFailure) as failure:
            verification.check("R15", lambda: harness.require(False, "ws_message_timeout"))
        self.assertEqual(str(failure.exception), "ws_message_timeout")
        with self.assertRaises(harness.HarnessFailure) as failure:
            verification.check("R15", lambda: harness.require(False, "Bearer secret-token"))
        self.assertEqual(str(failure.exception), "operation_failed")
    def test_receive_polls_heartbeat(self):
        from unittest.mock import Mock
        connection = harness.Realtime.__new__(harness.Realtime)
        connection.tick = Mock(); connection.exact = Mock(side_effect=[bytes([0x81, 2]), b"{}"])
        self.assertEqual(connection.receive(999), {})
        connection.tick.assert_called_once()
    def test_transport_latency_threshold_and_sample_floor(self):
        self.assertTrue(harness.latency_goal_met([1000] * 100))
        self.assertFalse(harness.latency_goal_met([1001] * 100))
        results = harness.Results()
        verification = harness.Verification(None, results)
        with self.assertRaises(harness.HarnessFailure): verification.check("R15", lambda: harness.require(harness.latency_goal_met([1001] * 100)))
        self.assertEqual(results.report()["checks"]["R15"]["status"], "unmet")
        with self.assertRaises(harness.HarnessFailure): harness.latency_goal_met([1] * 99)
    def test_percentile_and_report_metrics_are_numeric_only(self):
        self.assertEqual(harness.percentile95(list(range(1, 101))), 95)
        with self.assertRaises(harness.HarnessFailure): harness.percentile95([])
        results = harness.Results()
        results.metric("transport_samples", 100)
        self.assertEqual(results.report()["metrics"]["transport_samples"], 100)
        with self.assertRaises(harness.HarnessFailure): results.metric("access_token", "secret")

if __name__ == "__main__": unittest.main()
