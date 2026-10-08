"""Evaluate only the concurrency harness path predicate; never execute its body."""
import ast
from pathlib import Path
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("invite_lifecycle_concurrency.sh")
APPROVED = Path("/tmp/token-planet-multiplayer-qa-20261008")
PROJECT = "token-planet-multiplayer-qa-20261008"


def accepts_workdir(workdir, approved=APPROVED, container="supabase_db_" + PROJECT):
    source = SCRIPT.read_text().split("<<'PY'\n", 1)[1].rsplit("\nPY", 1)[0]
    tree = ast.parse(source)
    # Compile only the actual guard predicate and its pure path helper. No top-level
    # imports, Docker calls, fixture setup, SQL or subprocess code can run.
    guard = next(node for node in tree.body if isinstance(node, ast.If)
                 and {"workdir", "container"}.issubset({name.id for name in ast.walk(node.test) if isinstance(name, ast.Name)}))
    namespace = {"pathlib": __import__("pathlib"), "workdir": str(workdir),
                 "root": approved, "expected": PROJECT, "container": container}
    helper = [node for node in tree.body if isinstance(node, ast.FunctionDef)
              and node.name == "is_approved_workdir"]
    if helper:
        exec(compile(ast.Module(body=helper, type_ignores=[]), str(SCRIPT), "exec"), namespace)
    return not eval(compile(ast.Expression(guard.test), str(SCRIPT), "eval"), namespace)


class InviteLifecyclePathTests(unittest.TestCase):
    def test_approved_tmp_path_is_accepted(self):
        self.assertTrue(accepts_workdir(APPROVED))

    def test_mac_style_tmp_symlink_is_accepted_by_canonical_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            canonical_tmp = root / "private" / "tmp"
            canonical_tmp.mkdir(parents=True)
            (root / "tmp").symlink_to(canonical_tmp, target_is_directory=True)
            approved = root / "tmp" / PROJECT
            (canonical_tmp / PROJECT).mkdir()
            self.assertTrue(accepts_workdir(approved, approved))
            self.assertTrue(accepts_workdir(canonical_tmp / PROJECT, approved))

    def test_different_path_and_foreign_container_are_rejected(self):
        self.assertFalse(accepts_workdir(APPROVED.with_name("another-project")))
        self.assertFalse(accepts_workdir(APPROVED, container="supabase_db_foreign"))


if __name__ == "__main__":
    unittest.main()
