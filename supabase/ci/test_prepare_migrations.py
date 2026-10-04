import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("prepare_migrations.py")
REPO_ROOT = Path(__file__).resolve().parents[2]


class PrepareMigrationsTests(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory()
        self.root = Path(self.temp_dir.name)
        self.source = self.root / "source"
        self.output = self.root / "staged"
        self.source.mkdir()

    def tearDown(self):
        self.temp_dir.cleanup()

    def write_source(self, filename, contents=b"select 1;\n"):
        (self.source / filename).write_bytes(contents)

    def run_cli(self, command, source=None, output=None):
        return subprocess.run(
            [
                sys.executable,
                str(SCRIPT),
                command,
                "--source",
                str(source or self.source),
                "--output",
                str(output or self.output),
            ],
            check=False,
            capture_output=True,
            text=True,
        )

    def assert_cli_succeeds(self, command="prepare", source=None, output=None):
        result = self.run_cli(command, source, output)
        self.assertEqual(result.returncode, 0, result.stderr)
        return result

    def assert_cli_rejects(self, command="prepare", source=None, output=None):
        result = self.run_cli(command, source, output)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        return result

    def prepared_manifest(self):
        self.assert_cli_succeeds()
        return json.loads((self.output / "manifest.json").read_text())

    def test_prepare_orders_version_strings_and_preserves_sql_bytes(self):
        fixtures = {
            "202610010001_parent.sql": b"create table parent(id int);\n",
            "20261001000101_child.sql": b"alter table parent add column child int;\n",
            "202610010002_parent.sql": b"create table parent_two(id int);\n",
            "20261001000200_child.sql": b"alter table parent_two add column child int;\n",
        }
        for filename, contents in reversed(list(fixtures.items())):
            self.write_source(filename, contents)

        manifest = self.prepared_manifest()

        self.assertEqual(manifest["format_version"], 1)
        self.assertEqual(manifest["history_mode"], "synthetic_ci_only")
        self.assertEqual(
            [entry["source_filename"] for entry in manifest["entries"]],
            [
                "202610010001_parent.sql",
                "20261001000101_child.sql",
                "202610010002_parent.sql",
                "20261001000200_child.sql",
            ],
        )
        self.assertEqual(
            [entry["synthetic_version"] for entry in manifest["entries"]],
            [
                "20000101000001",
                "20000101000002",
                "20000101000003",
                "20000101000004",
            ],
        )
        self.assertEqual(
            [entry["ordinal"] for entry in manifest["entries"]], [1, 2, 3, 4]
        )
        self.assertEqual(
            [path.name for path in sorted(self.output.glob("*.sql"))],
            [
                "20000101000001_202610010001_parent.sql",
                "20000101000002_20261001000101_child.sql",
                "20000101000003_202610010002_parent.sql",
                "20000101000004_20261001000200_child.sql",
            ],
        )
        for entry in manifest["entries"]:
            source_bytes = fixtures[entry["source_filename"]]
            staged_path = self.output / entry["staged_filename"]
            self.assertEqual(staged_path.read_bytes(), source_bytes)
            self.assertEqual(
                entry["sha256"], hashlib.sha256(source_bytes).hexdigest()
            )

    def test_prepare_rejects_empty_source(self):
        result = self.assert_cli_rejects()
        self.assertIn("empty", result.stderr.lower())
        self.assertFalse(self.output.exists())

    def test_prepare_rejects_invalid_migration_filename(self):
        self.write_source("not_a_migration.sql")

        result = self.assert_cli_rejects()

        self.assertIn("filename", result.stderr.lower())
        self.assertFalse(self.output.exists())

    def test_prepare_rejects_duplicate_versions(self):
        self.write_source("202610010001_first.sql")
        self.write_source("202610010001_second.sql")

        result = self.assert_cli_rejects()

        self.assertIn("duplicate", result.stderr.lower())
        self.assertFalse(self.output.exists())

    def test_prepare_rejects_symlinked_migration(self):
        outside = self.root / "outside.sql"
        outside.write_bytes(b"select 1;\n")
        try:
            (self.source / "202610010001_link.sql").symlink_to(outside)
        except OSError as error:
            self.skipTest(f"symlink creation is unavailable: {error}")

        result = self.assert_cli_rejects()

        self.assertIn("symlink", result.stderr.lower())
        self.assertFalse(self.output.exists())

    def test_prepare_rejects_directory_in_source(self):
        (self.source / "nested").mkdir()

        result = self.assert_cli_rejects()

        self.assertIn("directory", result.stderr.lower())
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.output.exists())

    def test_prepare_rejects_symlinked_source_directory(self):
        source_link = self.root / "source-link"
        try:
            source_link.symlink_to(self.source, target_is_directory=True)
        except OSError as error:
            self.skipTest(f"symlink creation is unavailable: {error}")

        result = self.assert_cli_rejects(source=source_link)

        self.assertIn("symlink", result.stderr.lower())

    def test_prepare_preserves_nonempty_output(self):
        self.write_source("202610010001_first.sql")
        self.output.mkdir()
        sentinel = self.output / "keep.txt"
        sentinel.write_text("existing data")

        result = self.assert_cli_rejects()

        self.assertIn("empty", result.stderr.lower())
        self.assertEqual(sentinel.read_text(), "existing data")

    def test_prepare_rejects_overlapping_source_and_output(self):
        self.write_source("202610010001_first.sql")

        same_path = self.assert_cli_rejects(output=self.source)
        self.assertIn("overlap", same_path.stderr.lower())

        child_output = self.source / "staged"
        child = self.assert_cli_rejects(output=child_output)
        self.assertIn("overlap", child.stderr.lower())
        self.assertFalse(child_output.exists())

    def test_prepare_rejects_source_nested_in_output(self):
        output = self.root / "parent"
        nested_source = output / "migrations"
        nested_source.mkdir(parents=True)
        (nested_source / "202610010001_first.sql").write_bytes(b"select 1;\n")

        result = self.assert_cli_rejects(source=nested_source, output=output)

        self.assertIn("overlap", result.stderr.lower())

    def test_prepare_rejects_reserved_synthetic_namespace(self):
        self.write_source("20000101000001_original.sql")

        result = self.assert_cli_rejects()

        self.assertIn("synthetic", result.stderr.lower())
        self.assertFalse(self.output.exists())

    def test_verify_rejects_source_changed_after_staging(self):
        self.write_source("202610010001_first.sql", b"select 1;\n")
        self.prepared_manifest()
        self.write_source("202610010001_first.sql", b"select 2;\n")

        result = self.assert_cli_rejects("verify")

        self.assertIn("source", result.stderr.lower())

    def test_verify_rejects_staged_bytes_changed(self):
        self.write_source("202610010001_first.sql", b"select 1;\n")
        manifest = self.prepared_manifest()
        (self.output / manifest["entries"][0]["staged_filename"]).write_bytes(
            b"select 2;\n"
        )

        result = self.assert_cli_rejects("verify")

        self.assertIn("staged", result.stderr.lower())

    def test_verify_rejects_manifest_changed(self):
        self.write_source("202610010001_first.sql", b"select 1;\n")
        self.prepared_manifest()
        (self.output / "manifest.json").write_text("{}\n")

        result = self.assert_cli_rejects("verify")

        self.assertIn("manifest", result.stderr.lower())

    def test_verify_rejects_extra_staged_file(self):
        self.write_source("202610010001_first.sql", b"select 1;\n")
        self.prepared_manifest()
        (self.output / "extra.sql").write_bytes(b"select 2;\n")

        result = self.assert_cli_rejects("verify")

        self.assertIn("staged", result.stderr.lower())

    def test_verify_rejects_missing_staged_file(self):
        self.write_source("202610010001_first.sql", b"select 1;\n")
        manifest = self.prepared_manifest()
        (self.output / manifest["entries"][0]["staged_filename"]).unlink()

        result = self.assert_cli_rejects("verify")

        self.assertIn("staged", result.stderr.lower())

    def test_cli_rejects_relative_paths(self):
        self.write_source("202610010001_first.sql")

        result = subprocess.run(
            [
                sys.executable,
                str(SCRIPT),
                "prepare",
                "--source",
                os.path.relpath(self.source),
                "--output",
                os.path.relpath(self.output),
            ],
            check=False,
            capture_output=True,
            text=True,
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("absolute", result.stderr.lower())


class WorkflowAndReadmeTests(unittest.TestCase):
    def test_workflow_has_bounded_read_only_ci_contract(self):
        workflow_path = REPO_ROOT / ".github/workflows/supabase-migrations.yml"
        self.assertTrue(workflow_path.is_file(), "Supabase migration workflow is missing")
        workflow = workflow_path.read_text()

        self.assertIn("pull_request:", workflow)
        self.assertRegex(workflow, r"(?ms)^  push:\n    branches:\n      - master\b")
        self.assertIn("workflow_dispatch:", workflow)
        self.assertRegex(workflow, r"(?ms)^permissions:\n  contents: read\s*$")
        self.assertIn("cancel-in-progress: false", workflow)
        self.assertRegex(workflow, r"timeout-minutes:\s*[1-9][0-9]*")
        self.assertIn("supabase/setup-cli@1dedf2c611547ede7232d26866dd3c56ab903bbb", workflow)
        self.assertRegex(workflow, r"(?m)^\s+version:\s*['\"]?2\.119\.0['\"]?\s*$")
        self.assertIn("python3 -m unittest discover -s supabase/ci -p 'test_*.py' -v", workflow)
        self.assertIn('bash supabase/ci/run.sh --artifacts-dir "$RUNNER_TEMP/supabase-migration-results"', workflow)
        self.assertIn("if: always()", workflow)
        self.assertIn("actions/upload-artifact@", workflow)
        self.assertIn("${{ runner.temp }}/supabase-migration-results/manifest.json", workflow)
        self.assertIn("${{ runner.temp }}/supabase-migration-results/*.log", workflow)
        self.assertNotIn("${{ runner.temp }}/supabase-migration-results/\n", workflow)
        guard = workflow.index("name: Verify local Docker endpoint")
        image_pull = workflow.index("docker pull public.ecr.aws/supabase/postgres:17.6.1.171")
        replay = workflow.index("bash supabase/ci/run.sh")
        self.assertLess(guard, image_pull)
        self.assertLess(image_pull, replay)
        for docker_override in ("DOCKER_HOST", "DOCKER_CONTEXT", "DOCKER_TLS_VERIFY", "DOCKER_CERT_PATH"):
            self.assertIn(docker_override, workflow[guard:image_pull])
        self.assertIn("docker context show", workflow[guard:image_pull])
        self.assertIn("unix:///", workflow[guard:image_pull])
        self.assertEqual(workflow.count("docker pull "), 1)

        lowered = workflow.lower()
        for forbidden in (
            "supabase_access_token",
            "supabase_db_url",
            "supabase link",
            "supabase db push",
            "--db-url",
            "--project-ref",
            "secrets.",
        ):
            self.assertNotIn(forbidden, lowered)

    def test_readme_explains_ci_only_synthetic_replay_and_cache_prerequisites(self):
        readme = (REPO_ROOT / "supabase/README.md").read_text()
        self.assertIn("## CI migration replay", readme)
        section = readme.split("## CI migration replay", 1)[1]
        for required in (
            "synthetic_ci_only",
            "2.119.0",
            "SUPABASE_BIN",
            "17.6.1.171",
            "56432",
            "56430",
            "cached",
            "absolute",
            "deployments",
            "existing databases",
        ):
            self.assertIn(required, section)


if __name__ == "__main__":
    unittest.main()
