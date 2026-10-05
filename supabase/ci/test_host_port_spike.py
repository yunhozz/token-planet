from contextlib import redirect_stderr, redirect_stdout
import io
import json
import os
import re
from pathlib import Path
import signal
import subprocess
import sys
from unittest.mock import patch
import tempfile
import unittest

if __package__:
    from . import host_port_spike
else:
    import host_port_spike


@unittest.skipUnless(os.name == "posix", "process-group cleanup requires POSIX")
class ProcessGroupRunnerTests(unittest.TestCase):
    def test_sigterm_kills_and_reaps_harmless_python_child(self):
        with tempfile.TemporaryDirectory() as temp:
            pid_file = Path(temp) / "child.pid"
            child_code = (
                "import os, signal, time; "
                f"open({str(pid_file)!r}, 'w').write(str(os.getpid())); "
                "time.sleep(0.1); os.kill(os.getppid(), signal.SIGTERM); time.sleep(60)"
            )
            previous_handler = signal.getsignal(signal.SIGTERM)

            def interrupt(signum, _frame):
                raise host_port_spike.SpikeInterrupted(signum)

            signal.signal(signal.SIGTERM, interrupt)
            try:
                with self.assertRaises(host_port_spike.SpikeInterrupted):
                    host_port_spike._run_process_group(
                        [sys.executable, "-c", child_code], capture_output=True, text=True,
                    )
            finally:
                signal.signal(signal.SIGTERM, previous_handler)

            child_pid = int(pid_file.read_text())
            with self.assertRaises(ChildProcessError):
                os.waitpid(child_pid, os.WNOHANG)


class SpikePreparationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.artifacts = self.root / "artifacts"
        self.artifacts.mkdir()

    def tearDown(self):
        self.temp.cleanup()

    def test_rejects_artifacts_directory_inside_repository(self):
        repo_root = Path(host_port_spike.__file__).resolve().parents[2]
        artifacts = repo_root / ".host-port-spike-test-artifacts"

        with self.assertRaises(ValueError):
            host_port_spike.prepare_spike(artifacts)

        self.assertFalse(artifacts.exists())

    def test_rejects_nonempty_artifacts_directory(self):
        (self.artifacts / "existing.txt").write_text("keep")

        with self.assertRaises(ValueError):
            host_port_spike.prepare_spike(self.artifacts)

        self.assertEqual((self.artifacts / "existing.txt").read_text(), "keep")

    def test_rejects_symlink_artifacts_directory(self):
        linked = self.root / "linked-artifacts"
        linked.symlink_to(self.artifacts, target_is_directory=True)

        with self.assertRaises(ValueError):
            host_port_spike.prepare_spike(linked)

    def test_main_accepts_only_the_required_artifacts_dir_argument(self):
        output = io.StringIO()
        with patch.object(host_port_spike, "run_spike", return_value={"status": "PASS"}) as run_spike:
            with redirect_stdout(output):
                result = host_port_spike.main(["--artifacts-dir", str(self.artifacts)])

        self.assertEqual(result, 0)
        self.assertEqual(output.getvalue(), "spike_status=PASS\n")
        run_spike.assert_called_once_with(str(self.artifacts))

    def test_main_forwards_start_only_flag(self):
        output = io.StringIO()
        with patch.object(host_port_spike, "run_spike", return_value={"status": "PASS"}) as run_spike:
            with redirect_stderr(io.StringIO()), redirect_stdout(output):
                try:
                    result = host_port_spike.main([
                        "--artifacts-dir", str(self.artifacts), "--start-only",
                    ])
                except SystemExit as error:
                    self.fail(f"main rejected --start-only: {error}")

        self.assertEqual(result, 0)
        self.assertEqual(output.getvalue(), "spike_status=PASS\n")
        run_spike.assert_called_once_with(str(self.artifacts), start_only=True)

    def test_main_rejects_unknown_arguments(self):
        with patch.object(host_port_spike, "run_spike") as run_spike:
            with redirect_stderr(io.StringIO()):
                with self.assertRaises(SystemExit) as exit_info:
                    host_port_spike.main(["--artifacts-dir", str(self.artifacts), "--run-real-cli"])

        self.assertEqual(exit_info.exception.code, 2)
        run_spike.assert_not_called()

    def test_prepares_only_generated_project_config_and_cache_pin(self):
        prepared = host_port_spike.prepare_spike(self.artifacts)
        self.assertIsNotNone(prepared, "preparation should return the isolated project paths")
        workdir, project_id, config_path = prepared
        workdir = Path(workdir)
        config_path = Path(config_path)

        self.assertTrue(workdir.is_dir())
        self.assertTrue(workdir.is_absolute())
        self.assertRegex(project_id, re.compile(r"^token-planet-ci-[0-9a-f]{24}$"))
        self.assertEqual(len(project_id), 40)
        self.assertEqual(workdir.parent, Path(tempfile.gettempdir()).resolve())
        self.assertEqual(config_path, workdir / "supabase" / "config.toml")
        config = config_path.read_text()
        self.assertIn(f'project_id = "{project_id}"', config)
        self.assertIn("[db.migrations]\nenabled = true", config)
        cache_pin = workdir / "supabase" / ".temp" / "postgres-version"
        self.assertEqual(cache_pin.read_text(), "17.6.1.171\n")
        migrations = workdir / "supabase" / "migrations"
        self.assertTrue(migrations.is_dir())
        self.assertEqual(list(migrations.iterdir()), [])
        self.assertFalse((workdir / "supabase" / "tests").exists())
        project_files = {
            path.relative_to(workdir / "supabase").as_posix()
            for path in (workdir / "supabase").rglob("*")
            if path.is_file()
        }
        self.assertEqual(project_files, {"config.toml", ".temp/postgres-version"})
        self.assertEqual(list(self.artifacts.iterdir()), [])

        host_port_spike.cleanup_spike(workdir)
        self.assertFalse(workdir.exists())

    def test_generated_config_disables_auth_storage_and_realtime(self):
        workdir, _, config_path = host_port_spike.prepare_spike(self.artifacts)
        config = Path(config_path).read_text()

        for section in ("auth", "storage", "realtime"):
            with self.subTest(section=section):
                self.assertIn(f"[{section}]\nenabled = false\n", config)

        host_port_spike.cleanup_spike(workdir)


class SpikePreflightTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.artifacts = self.root / "artifacts"
        self.artifacts.mkdir()
        self.workdir, self.project_id, _ = host_port_spike.prepare_spike(self.artifacts)
        self.bin_dir = self.root / "bin"
        self.bin_dir.mkdir()
        self.docker = self.bin_dir / "docker"
        self.supabase = self.bin_dir / "supabase"
        for binary in (self.docker, self.supabase):
            binary.write_text("#!/bin/sh\nexit 0\n")
            binary.chmod(0o755)
        self.environ = {
            "PATH": str(self.bin_dir),
            "SUPABASE_BIN": str(self.supabase),
            "SUPABASE_TELEMETRY_DISABLED": "1",
        }

    def tearDown(self):
        host_port_spike.cleanup_spike(self.workdir)
        self.temp.cleanup()

    def fake_runner(
        self,
        *,
        version="2.119.0",
        endpoint="unix:///var/run/docker.sock",
        image_tags=None,
        collisions=(),
        inspect_error=None,
    ):
        calls = []
        image_tags = image_tags if image_tags is not None else [host_port_spike.POSTGRES_IMAGE]

        def run(args, *, cwd=None, env=None, capture_output=None, text=None, check=None):
            args = [str(part) for part in args]
            calls.append({"args": args, "cwd": cwd, "env": env})
            if args[0] == str(self.supabase.resolve()):
                return subprocess.CompletedProcess(args, 0, stdout=f"{version}\n", stderr="")
            docker_args = args[1:]
            if docker_args[:2] == ["context", "show"]:
                return subprocess.CompletedProcess(args, 0, stdout="default\n", stderr="")
            if docker_args[:2] == ["context", "inspect"]:
                return subprocess.CompletedProcess(args, 0, stdout=json.dumps(endpoint) + "\n", stderr="")
            if docker_args[:1] == ["info"]:
                return subprocess.CompletedProcess(args, 0, stdout="engine ready\n", stderr="")
            if docker_args[:2] == ["image", "inspect"]:
                output = json.dumps([{"RepoTags": image_tags}])
                return subprocess.CompletedProcess(args, 0, stdout=output, stderr="")
            if docker_args[:2] in (["network", "inspect"], ["container", "inspect"], ["volume", "inspect"]):
                kind = docker_args[0]
                target = docker_args[-1]
                if kind in collisions:
                    return subprocess.CompletedProcess(args, 0, stdout="{}\n", stderr="")
                stderr = inspect_error or f"Error: No such {('object' if kind == 'container' else kind)}: {target}\n"
                return subprocess.CompletedProcess(args, 1, stdout="", stderr=stderr)
            return subprocess.CompletedProcess(args, 99, stdout="", stderr="unexpected fake command")

        return run, calls

    def test_readonly_preflight_checks_version_context_image_collisions_and_ports(self):
        run, calls = self.fake_runner()
        probed = []

        def port_probe(ports):
            probed.append(tuple(ports))

        result = host_port_spike.preflight_spike(
            self.workdir,
            self.project_id,
            environ=self.environ,
            run=run,
            port_probe=port_probe,
        )

        self.assertIsNotNone(result, "preflight should return verified local runtime details")
        self.assertEqual(result["docker_bin"], str(self.docker.resolve()))
        self.assertEqual(result["supabase_bin"], str(self.supabase.resolve()))
        self.assertEqual(result["context"], "default")
        self.assertEqual(result["postgres_image"], host_port_spike.POSTGRES_IMAGE)
        self.assertEqual(probed, [(56432, 56430)])
        cli_version_call = next(call for call in calls if call["args"][0] == str(self.supabase.resolve()))
        self.assertEqual(cli_version_call["args"][1:], ["--version"])
        self.assertEqual(Path(cli_version_call["cwd"]), self.workdir)
        image_call = next(call for call in calls if call["args"][1:3] == ["image", "inspect"])
        self.assertEqual(image_call["args"][3], host_port_spike.POSTGRES_IMAGE)
        self.assertFalse(any(call["args"][1:2] in (["start"], ["reset"], ["create"]) for call in calls))

    def test_rejects_remote_context_before_cached_image_or_collision_checks(self):
        run, calls = self.fake_runner(endpoint="ssh://docker.example")

        with self.assertRaises(RuntimeError):
            host_port_spike.preflight_spike(self.workdir, self.project_id, environ=self.environ, run=run)

        self.assertFalse(any(call["args"][1:3] == ["image", "inspect"] for call in calls))

    def test_rejects_wrong_cli_version_from_the_prepared_workdir(self):
        run, calls = self.fake_runner(version="2.118.0")

        with self.assertRaises(RuntimeError):
            host_port_spike.preflight_spike(self.workdir, self.project_id, environ=self.environ, run=run)

        self.assertEqual(len(calls), 1)
        self.assertEqual(Path(calls[0]["cwd"]), self.workdir)

    def test_rejects_unapproved_environment_before_running_commands(self):
        for variable in ("SUPABASE_ACCESS_TOKEN", "DATABASE_URL", "DOCKER_HOST"):
            with self.subTest(variable=variable):
                run, calls = self.fake_runner()
                environ = {**self.environ, variable: "unexpected"}

                with self.assertRaises(RuntimeError):
                    host_port_spike.preflight_spike(self.workdir, self.project_id, environ=environ, run=run)

                self.assertEqual(calls, [])

    def test_rejects_unavailable_pinned_image_instead_of_trying_to_pull(self):
        run, calls = self.fake_runner(image_tags=["other/image:tag"])
        probed = []

        with self.assertRaises(RuntimeError):
            host_port_spike.preflight_spike(
                self.workdir,
                self.project_id,
                environ=self.environ,
                run=run,
                port_probe=lambda ports: probed.append(tuple(ports)),
            )

        self.assertTrue(any(call["args"][1:3] == ["image", "inspect"] for call in calls))
        self.assertFalse(probed)
        self.assertFalse(any(call["args"][1:2] == ["pull"] for call in calls))

    def test_rejects_existing_generated_resource_collision(self):
        run, calls = self.fake_runner(collisions=("volume",))

        with self.assertRaises(RuntimeError):
            host_port_spike.preflight_spike(self.workdir, self.project_id, environ=self.environ, run=run)

        collision_calls = [call for call in calls if call["args"][1:2] == ["volume"]]
        self.assertEqual(len(collision_calls), 1)

    def test_rejects_inspect_error_that_is_not_confirmed_absence(self):
        run, calls = self.fake_runner(inspect_error="permission denied\n")

        with self.assertRaises(RuntimeError):
            host_port_spike.preflight_spike(self.workdir, self.project_id, environ=self.environ, run=run)

        self.assertTrue(any(call["args"][1:2] == ["network"] for call in calls))

    def test_rejects_invalid_service_enabled_flags_before_external_commands(self):
        config_path = Path(self.workdir) / "supabase" / "config.toml"
        original = config_path.read_text()
        invalid_configs = []
        for section in ("auth", "storage", "realtime"):
            expected = f"[{section}]\nenabled = false\n"
            invalid_configs.extend((
                (section, "missing", original.replace(expected, f"[{section}]\n")),
                (section, "true", original.replace(expected, f"[{section}]\nenabled = true\n")),
                (section, "wrong type", original.replace(expected, f'[{section}]\nenabled = "false"\n')),
            ))

        for section, value, invalid_config in invalid_configs:
            with self.subTest(section=section, value=value):
                config_path.write_text(invalid_config)
                run, calls = self.fake_runner()
                probed = []

                with self.assertRaises(RuntimeError):
                    host_port_spike.preflight_spike(
                        self.workdir,
                        self.project_id,
                        environ=self.environ,
                        run=run,
                        port_probe=lambda ports: probed.append(tuple(ports)),
                    )

                self.assertEqual(calls, [])
                self.assertEqual(probed, [])

    def test_rejects_busy_loopback_port_before_mutation(self):
        run, calls = self.fake_runner()

        def busy_port_probe(ports):
            raise OSError("port busy")

        with self.assertRaises(RuntimeError):
            host_port_spike.preflight_spike(
                self.workdir,
                self.project_id,
                environ=self.environ,
                run=run,
                port_probe=busy_port_probe,
            )

        self.assertFalse(any(call["args"][1:2] in (["create"], ["start"], ["reset"]) for call in calls))


class SpikeLifecycleTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.artifacts = self.root / "artifacts"
        self.artifacts.mkdir()
        self.bin_dir = self.root / "bin"
        self.bin_dir.mkdir()
        self.docker = self.bin_dir / "docker"
        self.supabase = self.bin_dir / "supabase"
        for binary in (self.docker, self.supabase):
            binary.write_text("#!/bin/sh\nexit 0\n")
            binary.chmod(0o755)
        self.environ = {
            "PATH": str(self.bin_dir),
            "SUPABASE_BIN": str(self.supabase),
            "SUPABASE_TELEMETRY_DISABLED": "1",
        }
        self.binding_after_start = "127.0.0.1"
        self.binding_after_reset = "127.0.0.1"
        self.fail_start = False
        self.fail_reset = False
        self.start_stdout = ""
        self.start_stderr = ""
        self.start_exit_code = 1
        self.failed_container_id = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        self.failed_container_error = "bind: address already in use password=must-not-appear"
        self.fail_container_inspect = False
        self.fail_container_remove = False
        self.container_absent_after_start_failure = False
        self.start_markers = (True, True)
        self.cli_calls = []
        self.docker_calls = []
        self.network = None
        self.container = None
        self.volume = None
        self.signal_at = None
        self.original_sigterm_handler = signal.getsignal(signal.SIGTERM)
        self.sigterm_handler_during_cli = None
        self.generated_workdir = None

    def tearDown(self):
        self.temp.cleanup()

    def _container_json(self, project_id, workdir, network_name, volume_name, binding, container_id):
        return {
            "Id": container_id,
            "Name": f"/supabase_db_{project_id}",
            "Config": {
                "Image": host_port_spike.POSTGRES_IMAGE,
                "Labels": {
                    "com.supabase.cli.project": project_id,
                    "com.docker.compose.project": project_id,
                    "com.supabase.cli.workdir": str(workdir),
                },
            },
            "NetworkSettings": {
                "Ports": {"5432/tcp": [{"HostIp": binding, "HostPort": str(host_port_spike.DB_PORT)}]},
                "Networks": {network_name: {"NetworkID": self.network["Id"]}},
            },
            "Mounts": [{"Name": volume_name, "Destination": "/var/lib/postgresql/data"}],
        }

    def fake_runner(self, args, *, cwd=None, env=None, capture_output=None, text=None, check=None):
        args = [str(part) for part in args]
        command = args[0]
        if command == str(self.supabase.resolve()):
            self.cli_calls.append({"args": args, "cwd": cwd, "env": env})
            if args[1:] == ["--version"]:
                return subprocess.CompletedProcess(args, 0, stdout="2.119.0\n", stderr="")
            if args[1:2] == ["start"]:
                if self.fail_start:
                    self._simulate_cli_creation(env, "start", markers=self.start_markers)
                    if not self.container_absent_after_start_failure:
                        self.container = self._container_json(
                            env["TOKEN_PLANET_CI_PROJECT_ID"], cwd, self.network["Name"],
                            f"supabase_db_{env['TOKEN_PLANET_CI_PROJECT_ID']}", self.binding_after_start,
                            self.failed_container_id,
                        )
                        self.container["State"] = {
                            "Status": "exited", "ExitCode": 17, "Error": self.failed_container_error,
                        }
                    return subprocess.CompletedProcess(
                        args, self.start_exit_code, stdout=self.start_stdout, stderr=self.start_stderr,
                    )
                self._simulate_cli_creation(env, "start")
                self.container = self._container_json(
                    env["TOKEN_PLANET_CI_PROJECT_ID"], cwd, self.network["Name"],
                    f"supabase_db_{env['TOKEN_PLANET_CI_PROJECT_ID']}", self.binding_after_start,
                    "container-start-id",
                )
                if self.signal_at == "start":
                    handler = signal.getsignal(signal.SIGTERM)
                    self.sigterm_handler_during_cli = handler
                    if handler is not self.original_sigterm_handler and callable(handler):
                        handler(signal.SIGTERM, None)
                return subprocess.CompletedProcess(args, 0, stdout="started", stderr="")
            if args[1:3] == ["db", "reset"]:
                if self.fail_reset:
                    return subprocess.CompletedProcess(args, 1, stdout="", stderr="reset failed")
                self._simulate_cli_creation(env, "reset")
                self.container = self._container_json(
                    env["TOKEN_PLANET_CI_PROJECT_ID"], cwd, self.network["Name"],
                    f"supabase_db_{env['TOKEN_PLANET_CI_PROJECT_ID']}", self.binding_after_reset,
                    "container-reset-id",
                )
                return subprocess.CompletedProcess(args, 0, stdout="reset", stderr="")
            return subprocess.CompletedProcess(args, 99, stdout="", stderr="unexpected CLI command")

        if command != str(self.docker.resolve()):
            return subprocess.CompletedProcess(args, 99, stdout="", stderr="unexpected executable")
        docker_args = args[1:]
        self.docker_calls.append(docker_args)
        project_id = next((value for value in (env or {}).values() if value.startswith("token-planet-ci-") and len(value) == 40), None)
        # Direct Docker calls use the original environment, so infer generated names from network args/state.
        if docker_args[:2] == ["context", "show"]:
            return subprocess.CompletedProcess(args, 0, stdout="default\n", stderr="")
        if docker_args[:2] == ["context", "inspect"]:
            return subprocess.CompletedProcess(args, 0, stdout='"unix:///var/run/docker.sock"\n', stderr="")
        if docker_args[:1] == ["info"]:
            return subprocess.CompletedProcess(args, 0, stdout="engine ready\n", stderr="")
        if docker_args[:2] == ["image", "inspect"]:
            return subprocess.CompletedProcess(args, 0, stdout=json.dumps([{"RepoTags": [host_port_spike.POSTGRES_IMAGE]}]), stderr="")
        if docker_args[:2] == ["network", "create"]:
            name = docker_args[-1]
            labels = {docker_args[i + 1].split("=", 1)[0]: docker_args[i + 1].split("=", 1)[1] for i, item in enumerate(docker_args[:-1]) if item == "--label"}
            self.network = {
                "Id": "network-id-spike",
                "Name": name,
                "Labels": labels,
                "Options": {"com.docker.network.bridge.host_binding_ipv4": "127.0.0.1"},
            }
            self.generated_workdir = Path(labels["com.tokenplanet.ci.workdir"])
            if self.signal_at == "network":
                signal.raise_signal(signal.SIGTERM)
            return subprocess.CompletedProcess(args, 0, stdout=f"{self.network['Id']}\n", stderr="")
        if docker_args[:2] == ["network", "inspect"]:
            if self.network:
                return subprocess.CompletedProcess(args, 0, stdout=json.dumps(self.network), stderr="")
            return subprocess.CompletedProcess(args, 1, stdout="", stderr=f"Error: No such network: {docker_args[-1]}")
        if docker_args[:2] == ["container", "inspect"]:
            if self.fail_container_inspect and any(
                call["args"][1:2] == ["start"] for call in self.cli_calls
            ):
                return subprocess.CompletedProcess(args, 1, stdout="", stderr="permission denied")
            if self.container:
                return subprocess.CompletedProcess(args, 0, stdout=json.dumps(self.container), stderr="")
            return subprocess.CompletedProcess(args, 1, stdout="", stderr=f"Error: No such object: {docker_args[-1]}")
        if docker_args[:2] == ["volume", "inspect"]:
            if self.volume:
                return subprocess.CompletedProcess(args, 0, stdout=json.dumps(self.volume), stderr="")
            return subprocess.CompletedProcess(args, 1, stdout="", stderr=f"Error: No such volume: {docker_args[-1]}")
        if docker_args[:3] == ["container", "rm", "-f"]:
            if self.fail_container_remove:
                return subprocess.CompletedProcess(args, 1, stdout="", stderr="remove denied")
            self.container = None
            return subprocess.CompletedProcess(args, 0, stdout="", stderr="")
        if docker_args[:2] == ["volume", "rm"]:
            self.volume = None
            return subprocess.CompletedProcess(args, 0, stdout="", stderr="")
        if docker_args[:2] == ["network", "rm"]:
            self.network = None
            return subprocess.CompletedProcess(args, 0, stdout="", stderr="")
        return subprocess.CompletedProcess(args, 99, stdout="", stderr=f"unexpected Docker command {docker_args[:2]}")

    def _simulate_cli_creation(self, env, phase, *, markers=(True, True)):
        self.generated_workdir = Path(env["TOKEN_PLANET_CI_WORKDIR"])
        cli_path = env["PATH"].split(os.pathsep)[0]
        self.assertTrue((Path(cli_path) / "docker").is_file())
        self.assertEqual(env["TOKEN_PLANET_REAL_DOCKER"], str(self.docker.resolve()))
        self.assertTrue(env["TOKEN_PLANET_CI_CREATE_MARKER"].endswith(f"{phase}-container-create-seen"))
        self.assertTrue(env["TOKEN_PLANET_CI_VOLUME_CREATE_MARKER"].endswith(f"{phase}-volume-create-seen"))
        if markers[0]:
            Path(env["TOKEN_PLANET_CI_CREATE_MARKER"]).touch()
        if markers[1]:
            Path(env["TOKEN_PLANET_CI_VOLUME_CREATE_MARKER"]).touch()
        project_id = env["TOKEN_PLANET_CI_PROJECT_ID"]
        self.volume = {
            "Name": f"supabase_db_{project_id}",
            "Labels": {
                "com.supabase.cli.project": project_id,
                "com.docker.compose.project": project_id,
            },
        }

    def _run(self, artifacts_dir=None, *, start_only=False):
        arguments = {
            "environ": self.environ,
            "run": self.fake_runner,
            "port_probe": lambda ports: None,
        }
        if start_only:
            try:
                return host_port_spike.run_spike(
                    self.artifacts if artifacts_dir is None else artifacts_dir,
                    start_only=True,
                    **arguments,
                )
            except TypeError as error:
                self.fail(f"run_spike does not support start_only: {error}")
        return host_port_spike.run_spike(
            self.artifacts if artifacts_dir is None else artifacts_dir,
            **arguments,
        )

    def test_start_wildcard_binding_fails_before_reset_and_cleans_only_owned_resources(self):
        self.binding_after_start = "0.0.0.0"

        with self.assertRaises(RuntimeError):
            self._run()

        self.assertEqual([call["args"][1:2] for call in self.cli_calls if call["args"][1:2] in (["start"], ["db"])], [["start"]])
        self.assertIsNone(self.container)
        self.assertIsNone(self.volume)
        self.assertIsNone(self.network)
        self.assertIn(
            "phase=start status=FAIL code=postgres_binding",
            (self.artifacts / "spike.log").read_text(),
        )

    def test_failed_start_reports_redacted_diagnostics_and_exact_resource_state_before_cleanup(self):
        self.fail_start = True
        self.start_stdout = "host-port adapter rejected Docker operation: unexpected create option\n"
        self.start_stderr = (
            "bind: address already in use password=do-not-log "
            "postgresql://user:secret@example.invalid/db?token=also-secret\n"
        )

        with self.assertRaises(RuntimeError):
            self._run()

        log = (self.artifacts / "spike.log").read_text()
        project_id = self.cli_calls[1]["env"]["TOKEN_PLANET_CI_PROJECT_ID"]
        expected_name = f"supabase_db_{project_id}"
        self.assertIn(f"run_identity project_id={project_id} network_id=network-id-spike", log)
        self.assertIn(f"container_name={expected_name}", log)
        self.assertIn(f"container_id={self.failed_container_id}", log)
        self.assertIn("container_state status=exited exit_code=17 error=port_conflict", log)
        stdout_bytes = len(self.start_stdout.encode("utf-8"))
        stderr_bytes = len(self.start_stderr.encode("utf-8"))
        self.assertIn(
            f"diagnostic=port_conflict source=stderr message=database host port is already in use bytes={stderr_bytes} lines=1",
            log,
        )
        self.assertIn(f"diagnostic=adapter_reject category=create_option source=stdout bytes={stdout_bytes} lines=1", log)
        self.assertIn("phase=start adapter_markers container_create=present volume_create=present", log)
        for secret in ("do-not-log", "secret", "also-secret", "must-not-appear", "permission denied"):
            self.assertNotIn(secret, log)
        self.assertLess(log.index("container_state status=exited"), log.index("cleanup_status=PASS kind=container"))
        docker_commands = [call[:3] for call in self.docker_calls]
        self.assertLess(docker_commands.index(["container", "inspect", "--format"]), docker_commands.index(["container", "rm", "-f"]))
        self.assertFalse(any(call[1:3] == ["db", "reset"] for call in (entry["args"] for entry in self.cli_calls)))

    def test_failed_start_records_container_absence_without_raw_cli_output(self):
        self.fail_start = True
        self.container_absent_after_start_failure = True
        self.start_stderr = "private token=must-not-appear"

        with self.assertRaises(RuntimeError):
            self._run()

        log = (self.artifacts / "spike.log").read_text()
        self.assertIn("start_failure container=absent", log)
        self.assertNotIn("must-not-appear", log)
        self.assertNotIn("container_state", log)

    def test_failed_start_records_empty_and_unclassified_output_counts_without_raw_text(self):
        self.fail_start = True
        self.start_stdout = "private-token=never-record-this\nsecond private line\n"
        self.start_stderr = ""

        with self.assertRaises(RuntimeError):
            self._run()

        log = (self.artifacts / "spike.log").read_text()
        self.assertIn(
            f"diagnostic=unclassified source=stdout bytes={len(self.start_stdout.encode('utf-8'))} lines=2",
            log,
        )
        self.assertIn("diagnostic=empty source=stderr bytes=0 lines=0", log)
        self.assertNotIn("private-token", log)
        self.assertNotIn("never-record-this", log)

    def test_failed_start_records_each_adapter_marker_combination_before_cleanup(self):
        self.fail_start = True
        for container_seen, volume_seen in ((False, False), (False, True), (True, False), (True, True)):
            with self.subTest(container_seen=container_seen, volume_seen=volume_seen):
                self.start_markers = (container_seen, volume_seen)
                artifacts = self.root / f"artifacts-{int(container_seen)}-{int(volume_seen)}"
                artifacts.mkdir()
                with self.assertRaises(RuntimeError):
                    self._run(artifacts)

                log = (artifacts / "spike.log").read_text()
                container_state = "present" if container_seen else "absent"
                volume_state = "present" if volume_seen else "absent"
                marker_line = (
                    "phase=start adapter_markers "
                    f"container_create={container_state} volume_create={volume_state}"
                )
                self.assertIn(marker_line, log)
                self.assertLess(log.index(marker_line), log.index("cleanup_status=PASS kind=container"))

    def test_failed_start_inspect_error_is_redacted_and_cleanup_keeps_first_error(self):
        self.fail_start = True
        self.start_stderr = "database container exited"
        self.fail_container_inspect = True
        self.fail_container_remove = True

        with self.assertRaisesRegex(RuntimeError, "Supabase start failed; cleanup also reported 1 error"):
            self._run()

        log = (self.artifacts / "spike.log").read_text()
        self.assertIn("start_failure container=unavailable reason=inspect_failed", log)
        self.assertIn("cleanup_status=FAIL kind=container", log)
        self.assertNotIn("permission denied", log)
        self.assertNotIn("remove denied", log)

    def test_success_checks_start_and_reset_then_cleans_owned_resources(self):
        result = self._run()

        self.assertEqual(result["status"], "PASS")
        self.assertEqual([call["args"][1] for call in self.cli_calls if call["args"][1] in ("start", "db")], ["start", "db"])
        reset_call = next(call for call in self.cli_calls if call["args"][1:3] == ["db", "reset"])
        self.assertIn("--local", reset_call["args"])
        self.assertIn("--no-seed", reset_call["args"])
        self.assertIsNone(self.container)
        self.assertIsNone(self.volume)
        self.assertIsNone(self.network)

    def test_default_mode_records_start_reset_artifact_mode(self):
        self._run()

        self.assertIn("spike_mode=start_reset", (self.artifacts / "spike.log").read_text())

    def test_start_only_runs_start_once_checks_actual_resources_and_cleans_exact_owned_resources(self):
        result = self._run(start_only=True)

        self.assertEqual(result["status"], "PASS")
        cli_phases = [
            call["args"][1]
            for call in self.cli_calls
            if call["args"][1] in ("start", "db")
        ]
        self.assertEqual(cli_phases, ["start"])
        self.assertTrue(any(
            call[:2] == ["container", "inspect"]
            for call in self.docker_calls
        ), "start-only must inspect the actual database binding and ownership")
        self.assertIn(["container", "rm", "-f", "container-start-id"], self.docker_calls)
        project_id = next(call["env"]["TOKEN_PLANET_CI_PROJECT_ID"] for call in self.cli_calls if call["args"][1] == "start")
        self.assertIn(["volume", "rm", f"supabase_db_{project_id}"], self.docker_calls)
        self.assertIn(["network", "rm", "network-id-spike"], self.docker_calls)
        self.assertIsNone(self.container)
        self.assertIsNone(self.volume)
        self.assertIsNone(self.network)
        self.assertIsNotNone(self.generated_workdir)
        self.assertFalse(self.generated_workdir.exists())
        summary = (self.artifacts / "spike.log").read_text()
        self.assertIn("spike_mode=start_only", summary)
        self.assertIn("phase=start status=PASS", summary)

    def test_start_only_rejects_non_loopback_actual_binding_and_cleans_resources(self):
        self.binding_after_start = "0.0.0.0"

        with self.assertRaisesRegex(RuntimeError, "database published binding is not 127.0.0.1"):
            self._run(start_only=True)

        cli_phases = [
            call["args"][1]
            for call in self.cli_calls
            if call["args"][1] in ("start", "db")
        ]
        self.assertEqual(cli_phases, ["start"])
        self.assertIsNone(self.container)
        self.assertIsNone(self.volume)
        self.assertIsNone(self.network)
        summary = (self.artifacts / "spike.log").read_text()
        self.assertIn("spike_mode=start_only", summary)
        self.assertIn("phase=start status=FAIL code=postgres_binding", summary)

    def test_start_only_start_failure_skips_later_cli_calls_and_cleans_owned_resources(self):
        self.fail_start = True

        with self.assertRaisesRegex(RuntimeError, "Supabase start failed"):
            self._run(start_only=True)

        cli_phases = [
            call["args"][1]
            for call in self.cli_calls
            if call["args"][1] in ("start", "db")
        ]
        self.assertEqual(cli_phases, ["start"])
        self.assertIn(["container", "rm", "-f", self.failed_container_id], self.docker_calls)
        project_id = next(call["env"]["TOKEN_PLANET_CI_PROJECT_ID"] for call in self.cli_calls if call["args"][1] == "start")
        self.assertIn(["volume", "rm", f"supabase_db_{project_id}"], self.docker_calls)
        self.assertIn(["network", "rm", "network-id-spike"], self.docker_calls)
        self.assertIsNone(self.container)
        self.assertIsNone(self.volume)
        self.assertIsNone(self.network)
        self.assertIsNotNone(self.generated_workdir)
        self.assertFalse(self.generated_workdir.exists())
        summary = (self.artifacts / "spike.log").read_text()
        self.assertIn("spike_mode=start_only", summary)
        self.assertIn("spike_status=FAIL", summary)

    def test_start_only_cleanup_failure_keeps_overall_run_failed(self):
        self.fail_container_remove = True

        with self.assertRaisesRegex(RuntimeError, "spike cleanup reported 1 error"):
            self._run(start_only=True)

        cli_phases = [
            call["args"][1]
            for call in self.cli_calls
            if call["args"][1] in ("start", "db")
        ]
        self.assertEqual(cli_phases, ["start"])
        self.assertIsNotNone(self.container)
        self.assertIsNone(self.volume)
        self.assertIsNone(self.network)
        self.assertIsNotNone(self.generated_workdir)
        self.assertFalse(self.generated_workdir.exists())
        summary = (self.artifacts / "spike.log").read_text()
        self.assertIn("spike_mode=start_only", summary)
        self.assertIn("cleanup_status=FAIL kind=container", summary)
        self.assertIn("spike_status=FAIL code=cleanup", summary)

    def test_reset_wildcard_binding_fails_and_cleans_owned_resources(self):
        self.binding_after_reset = "0.0.0.0"

        with self.assertRaises(RuntimeError):
            self._run()

        self.assertTrue(any(call["args"][1:3] == ["db", "reset"] for call in self.cli_calls))
        self.assertIsNone(self.container)
        self.assertIsNone(self.volume)
        self.assertIsNone(self.network)
        self.assertIn(
            "phase=reset status=FAIL code=postgres_binding",
            (self.artifacts / "spike.log").read_text(),
        )

    def test_refused_nonempty_artifacts_preserves_existing_spike_log(self):
        existing_log = self.artifacts / "spike.log"
        existing_log.write_text("preserve this report\n")

        with self.assertRaises(RuntimeError):
            self._run()

        self.assertEqual(existing_log.read_text(), "preserve this report\n")

    def test_run_spike_rejects_symlink_artifacts_before_commands(self):
        linked_artifacts = self.root / "linked-artifacts"
        linked_artifacts.symlink_to(self.artifacts, target_is_directory=True)

        with self.assertRaises(ValueError):
            host_port_spike.run_spike(
                linked_artifacts,
                environ=self.environ,
                run=self.fake_runner,
                port_probe=lambda ports: None,
            )

        self.assertEqual(self.docker_calls, [])
        self.assertEqual(list(self.artifacts.iterdir()), [])

    def test_sigterm_during_start_stops_reset_cleans_resources_and_restores_handler(self):
        self.signal_at = "start"
        try:
            with self.assertRaises(RuntimeError):
                self._run()
        finally:
            signal.signal(signal.SIGTERM, self.original_sigterm_handler)

        self.assertTrue(callable(self.sigterm_handler_during_cli))
        self.assertIsNot(self.sigterm_handler_during_cli, self.original_sigterm_handler)
        self.assertIs(signal.getsignal(signal.SIGTERM), self.original_sigterm_handler)
        self.assertFalse(any(call["args"][1:3] == ["db", "reset"] for call in self.cli_calls))
        self.assertIsNone(self.container)
        self.assertIsNone(self.volume)
        self.assertIsNone(self.network)
        self.assertIsNotNone(self.generated_workdir)
        self.assertFalse(self.generated_workdir.exists())
        summary = (self.artifacts / "spike.log").read_text()
        self.assertIn("spike_status=FAIL code=interrupted signal=SIGTERM", summary)
        self.assertNotIn("Traceback", summary)


if __name__ == "__main__":
    unittest.main()
