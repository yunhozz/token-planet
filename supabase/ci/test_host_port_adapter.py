import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

if __package__:
    from .host_port_adapter import AdapterError, adapt_docker_argv
else:
    from host_port_adapter import AdapterError, adapt_docker_argv


ADAPTER = Path(__file__).with_name("host_port_adapter.py")
PROJECT_ID = "token-planet-ci-0123456789abcdef01234567"
WORKDIR = "/tmp/token-planet-ci.ABC12345"
NETWORK_ID = "network-id-123"
CONTAINER_NAME = f"supabase_db_{PROJECT_ID}"
IMAGE = "public.ecr.aws/supabase/postgres:17.6.1.171"
CONTRACT = {
    "project_id": PROJECT_ID,
    "workdir": WORKDIR,
    "network_id": NETWORK_ID,
    "container_name": CONTAINER_NAME,
    "image": IMAGE,
}


def valid_create_args():
    return [
        "create",
        "--name", CONTAINER_NAME,
        "-e", "POSTGRES_PASSWORD",
        "-v", f"{CONTAINER_NAME}:/var/lib/postgresql/data",
        "-p", "56432:5432",
        "--network", NETWORK_ID,
        "--label", f"com.supabase.cli.project={PROJECT_ID}",
        "--label", f"com.docker.compose.project={PROJECT_ID}",
        "--label", f"com.supabase.cli.workdir={WORKDIR}",
        IMAGE,
    ]


def valid_volume_create_args():
    return [
        "volume", "create",
        "--label", f"com.supabase.cli.project={PROJECT_ID}",
        "--label", f"com.docker.compose.project={PROJECT_ID}",
        CONTAINER_NAME,
    ]


class HostPortAdapterTests(unittest.TestCase):
    def test_rewrites_only_the_exact_expected_database_publish(self):
        args = valid_create_args()

        actual = adapt_docker_argv(args, **CONTRACT)

        expected = args.copy()
        expected[expected.index("56432:5432")] = "127.0.0.1:56432:5432"
        expected.insert(1, "--pull=never")
        self.assertEqual(actual, expected)

    def test_database_create_output_has_exactly_one_never_pull_policy(self):
        actual = adapt_docker_argv(valid_create_args(), **CONTRACT)

        self.assertEqual(actual.count("--pull=never"), 1)
        self.assertEqual(actual[1], "--pull=never")

    def test_rejects_input_database_create_pull_policy(self):
        valid = valid_create_args()
        image_index = valid.index(IMAGE)
        cases = (
            ["--pull=never"],
            ["--pull=always"],
            ["--pull=missing"],
            ["--pull", "never"],
        )
        for policy in cases:
            with self.subTest(policy=policy):
                args = valid.copy()
                args[image_index:image_index] = policy
                with self.assertRaises(AdapterError):
                    adapt_docker_argv(args, **CONTRACT)

    def test_leaves_non_create_docker_commands_unchanged(self):
        args = ["container", "inspect", "--format", "{{json .}}", CONTAINER_NAME]

        self.assertEqual(adapt_docker_argv(args, **CONTRACT), args)

    def test_rejects_create_with_wrong_generated_identity_or_image(self):
        mutations = (
            ("container name", lambda args: args.index("--name") + 1, "foreign-container"),
            ("network", lambda args: args.index("--network") + 1, "foreign-network"),
            (
                "project label",
                lambda args: next(i for i, value in enumerate(args) if value.startswith("com.supabase.cli.project=")),
                f"com.supabase.cli.project={PROJECT_ID}-foreign",
            ),
            (
                "compose project label",
                lambda args: next(i for i, value in enumerate(args) if value.startswith("com.docker.compose.project=")),
                f"com.docker.compose.project={PROJECT_ID}-foreign",
            ),
            (
                "workdir label",
                lambda args: next(i for i, value in enumerate(args) if value.startswith("com.supabase.cli.workdir=")),
                f"com.supabase.cli.workdir={WORKDIR}/foreign",
            ),
            ("image", lambda args: args.index(IMAGE), "public.ecr.aws/supabase/postgres:other"),
        )
        for label, locate, new in mutations:
            with self.subTest(label=label):
                args = valid_create_args()
                args[locate(args)] = new
                with self.assertRaises(AdapterError):
                    adapt_docker_argv(args, **CONTRACT)

    def test_rejects_missing_duplicate_or_malformed_database_publish(self):
        valid = valid_create_args()
        image_index = valid.index(IMAGE)
        duplicate_publish = valid.copy()
        duplicate_publish[image_index:image_index] = ["-p", "56432:5432"]
        publish_all = valid.copy()
        publish_all[image_index:image_index] = ["-P"]
        cases = {
            "missing": [item for item in valid if item not in ("-p", "56432:5432")],
            "duplicate": duplicate_publish,
            "wrong-port": ["127.0.0.1:56432:5432" if item == "56432:5432" else item for item in valid],
            "malformed-protocol": ["56432:5432/udp" if item == "56432:5432" else item for item in valid],
            "publish-all": publish_all,
        }
        for label, args in cases.items():
            with self.subTest(label=label):
                with self.assertRaises(AdapterError):
                    adapt_docker_argv(args, **CONTRACT)

    def test_rejects_duplicate_identity_flags(self):
        args = valid_create_args()
        image_index = args.index(IMAGE)
        args[image_index:image_index] = ["--network", NETWORK_ID]

        with self.assertRaises(AdapterError):
            adapt_docker_argv(args, **CONTRACT)

    def test_rejects_missing_foreign_or_duplicate_database_volume_mount(self):
        valid = valid_create_args()
        mount_index = valid.index("-v")
        image_index = valid.index(IMAGE)
        missing = valid.copy()
        del missing[mount_index:mount_index + 2]
        foreign = valid.copy()
        foreign[mount_index + 1] = "foreign-volume:/var/lib/postgresql/data"
        duplicate = valid.copy()
        duplicate[image_index:image_index] = ["-v", f"{CONTAINER_NAME}:/var/lib/postgresql/data"]

        for label, args in (("missing", missing), ("foreign", foreign), ("duplicate", duplicate)):
            with self.subTest(label=label):
                with self.assertRaises(AdapterError):
                    adapt_docker_argv(args, **CONTRACT)

    def run_adapter(self, args, real_docker, marker, volume_marker=None):
        env = os.environ.copy()
        env.update({
            "TOKEN_PLANET_REAL_DOCKER": str(real_docker),
            "TOKEN_PLANET_CI_PROJECT_ID": PROJECT_ID,
            "TOKEN_PLANET_CI_WORKDIR": WORKDIR,
            "TOKEN_PLANET_CI_NETWORK_ID": NETWORK_ID,
            "TOKEN_PLANET_CI_CONTAINER_NAME": CONTAINER_NAME,
            "TOKEN_PLANET_CI_CREATE_MARKER": str(marker),
            "TOKEN_PLANET_CI_VOLUME_CREATE_MARKER": str(
                volume_marker if volume_marker is not None else marker.with_name(f"volume-{marker.name}")
            ),
        })
        return subprocess.run(
            [sys.executable, str(ADAPTER), *args],
            check=False,
            capture_output=True,
            text=True,
            env=env,
        )

    def test_non_create_exec_preserves_argv_stdio_and_exit_status(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            received = root / "received.json"
            docker = root / "docker"
            docker.write_text(
                "#!/usr/bin/env python3\n"
                "import json, os, sys\n"
                f"open({str(received)!r}, 'w').write(json.dumps(sys.argv[1:]))\n"
                "print('forwarded stdout')\n"
                "print('forwarded stderr', file=sys.stderr)\n"
                "raise SystemExit(41)\n"
            )
            docker.chmod(0o755)
            args = ["container", "inspect", "--format", "{{json .}}", CONTAINER_NAME]

            result = self.run_adapter(args, docker, root / "create-seen")

            self.assertEqual(result.returncode, 41)
            self.assertEqual(result.stdout, "forwarded stdout\n")
            self.assertEqual(result.stderr, "forwarded stderr\n")
            self.assertEqual(json.loads(received.read_text()), args)

    def test_duplicate_create_is_rejected_before_second_docker_exec(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            docker = root / "docker"
            received = root / "received.jsonl"
            docker.write_text(
                "#!/usr/bin/env python3\n"
                "import json, sys\n"
                f"open({str(received)!r}, 'a').write(json.dumps(sys.argv[1:]) + '\\n')\n"
            )
            docker.chmod(0o755)
            marker = root / "create-seen"
            args = valid_create_args()

            first = self.run_adapter(args, docker, marker)
            second = self.run_adapter(args, docker, marker)

            self.assertEqual(first.returncode, 0, first.stderr)
            self.assertNotEqual(second.returncode, 0)
            self.assertEqual(len(received.read_text().splitlines()), 1)
            self.assertNotIn(PROJECT_ID, second.stderr)
            self.assertNotIn(WORKDIR, second.stderr)

    def test_separate_start_and_reset_markers_allow_one_create_each(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            docker = root / "docker"
            received = root / "received.jsonl"
            docker.write_text(
                "#!/usr/bin/env python3\n"
                "import json, sys\n"
                f"open({str(received)!r}, 'a').write(json.dumps(sys.argv[1:]) + '\\n')\n"
            )
            docker.chmod(0o755)
            args = valid_create_args()

            start = self.run_adapter(args, docker, root / "create-start-seen")
            reset = self.run_adapter(args, docker, root / "create-reset-seen")

            self.assertEqual(start.returncode, 0, start.stderr)
            self.assertEqual(reset.returncode, 0, reset.stderr)
            self.assertEqual(len(received.read_text().splitlines()), 2)

    def test_exact_volume_create_is_forwarded_and_recorded_with_its_own_marker(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            received = root / "received.json"
            docker = root / "docker"
            docker.write_text(
                "#!/usr/bin/env python3\n"
                "import json, sys\n"
                f"open({str(received)!r}, 'w').write(json.dumps(sys.argv[1:]))\n"
                "print('volume stdout')\n"
                "print('volume stderr', file=sys.stderr)\n"
                "raise SystemExit(29)\n"
            )
            docker.chmod(0o755)
            create_marker = root / "container-create-seen"
            volume_marker = root / "volume-create-seen"
            args = valid_volume_create_args()

            result = self.run_adapter(args, docker, create_marker, volume_marker)

            self.assertEqual(result.returncode, 29)
            self.assertEqual(result.stdout, "volume stdout\n")
            self.assertEqual(result.stderr, "volume stderr\n")
            self.assertEqual(json.loads(received.read_text()), args)
            self.assertTrue(volume_marker.exists())
            self.assertFalse(create_marker.exists())

    def test_rejects_pull_network_create_and_run_before_real_docker_exec(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            docker = root / "docker"
            received = root / "received.jsonl"
            docker.write_text(
                "#!/usr/bin/env python3\n"
                "import json, sys\n"
                f"open({str(received)!r}, 'a').write(json.dumps(sys.argv[1:]) + '\\n')\n"
            )
            docker.chmod(0o755)

            forbidden = (
                ["pull", IMAGE],
                ["image", "pull", IMAGE],
                ["network", "create", "foreign-network"],
                ["run", IMAGE],
                ["container", "run", IMAGE],
                ["container", "create", *valid_create_args()[1:]],
            )
            for args in forbidden:
                with self.subTest(args=args):
                    result = self.run_adapter(args, docker, root / "create-seen")
                    self.assertNotEqual(result.returncode, 0)

            self.assertFalse(received.exists())

    def test_rejects_non_exact_volume_create_before_real_docker_exec(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            docker = root / "docker"
            received = root / "received.jsonl"
            docker.write_text(
                "#!/usr/bin/env python3\n"
                "import json, sys\n"
                f"open({str(received)!r}, 'a').write(json.dumps(sys.argv[1:]) + '\\n')\n"
            )
            docker.chmod(0o755)
            valid = valid_volume_create_args()
            wrong_project = valid.copy()
            wrong_project[wrong_project.index(f"com.supabase.cli.project={PROJECT_ID}")] = (
                f"com.supabase.cli.project={PROJECT_ID}-foreign"
            )
            duplicate_label = valid.copy()
            duplicate_label[-1:-1] = ["--label", f"com.docker.compose.project={PROJECT_ID}"]
            extra_option = valid.copy()
            extra_option[-1:-1] = ["--driver", "local"]
            volume_source = valid.copy()
            volume_source[-1:-1] = ["--opt", "device=/dev/sda"]
            foreign_volume = valid.copy()
            foreign_volume[-1] = "supabase_db_foreign-project"

            for args in (wrong_project, duplicate_label, extra_option, volume_source, foreign_volume):
                with self.subTest(args=args):
                    result = self.run_adapter(args, docker, root / "create-seen")
                    self.assertNotEqual(result.returncode, 0)

            self.assertFalse(received.exists())

    def test_duplicate_volume_create_is_rejected_before_second_docker_exec(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            docker = root / "docker"
            received = root / "received.jsonl"
            docker.write_text(
                "#!/usr/bin/env python3\n"
                "import json, sys\n"
                f"open({str(received)!r}, 'a').write(json.dumps(sys.argv[1:]) + '\\n')\n"
            )
            docker.chmod(0o755)
            create_marker = root / "container-create-seen"
            volume_marker = root / "volume-create-seen"
            args = valid_volume_create_args()

            first = self.run_adapter(args, docker, create_marker, volume_marker)
            second = self.run_adapter(args, docker, create_marker, volume_marker)

            self.assertEqual(first.returncode, 0, first.stderr)
            self.assertNotEqual(second.returncode, 0)
            self.assertEqual(len(received.read_text().splitlines()), 1)

    def test_separate_start_and_reset_markers_allow_one_volume_create_each(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            docker = root / "docker"
            received = root / "received.jsonl"
            docker.write_text(
                "#!/usr/bin/env python3\n"
                "import json, sys\n"
                f"open({str(received)!r}, 'a').write(json.dumps(sys.argv[1:]) + '\\n')\n"
            )
            docker.chmod(0o755)
            create_marker = root / "container-create-seen"

            start = self.run_adapter(valid_volume_create_args(), docker, create_marker, root / "volume-start-seen")
            reset = self.run_adapter(valid_volume_create_args(), docker, create_marker, root / "volume-reset-seen")

            self.assertEqual(start.returncode, 0, start.stderr)
            self.assertEqual(reset.returncode, 0, reset.stderr)
            self.assertEqual(len(received.read_text().splitlines()), 2)
            self.assertTrue((root / "volume-start-seen").exists())
            self.assertTrue((root / "volume-reset-seen").exists())

    def test_container_and_volume_create_use_independent_phase_markers(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            docker = root / "docker"
            received = root / "received.jsonl"
            docker.write_text(
                "#!/usr/bin/env python3\n"
                "import json, sys\n"
                f"open({str(received)!r}, 'a').write(json.dumps(sys.argv[1:]) + '\\n')\n"
            )
            docker.chmod(0o755)
            container_marker = root / "container-create-seen"
            volume_marker = root / "volume-create-seen"

            container = self.run_adapter(valid_create_args(), docker, container_marker, volume_marker)
            volume = self.run_adapter(valid_volume_create_args(), docker, container_marker, volume_marker)

            self.assertEqual(container.returncode, 0, container.stderr)
            self.assertEqual(volume.returncode, 0, volume.stderr)
            self.assertEqual(len(received.read_text().splitlines()), 2)


if __name__ == "__main__":
    unittest.main()
