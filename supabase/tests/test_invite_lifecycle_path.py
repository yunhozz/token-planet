"""Exercise pure target helpers from the harness without Docker or SQL."""
import ast
from pathlib import Path
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("invite_lifecycle_concurrency.sh")
APPROVED = Path("/tmp/token-planet-multiplayer-qa-20261008")
PROJECT = "token-planet-multiplayer-qa-20261008"


def accepts_workdir(workdir, approved=APPROVED, container="supabase_db_" + PROJECT):
    if approved == APPROVED:
        try:
            helpers()['resolve_target']('qa', str(workdir), container)
            return True
        except RuntimeError:
            return False
    source = SCRIPT.read_text().split("<<'PY'\n", 1)[1].rsplit("\nPY", 1)[0]
    tree = ast.parse(source)
    helper = next(n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == 'is_approved_workdir')
    namespace = {'pathlib': __import__('pathlib')}
    exec(compile(ast.Module(body=[helper], type_ignores=[]), str(SCRIPT), 'exec'), namespace)
    return namespace['is_approved_workdir'](workdir, approved) and container == 'supabase_db_' + PROJECT



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

# Extract only pure helpers from the real heredoc; never execute SQL or Docker.
def helpers():
    source = SCRIPT.read_text().split("<<'PY'\n", 1)[1].rsplit("\nPY", 1)[0]
    tree = ast.parse(source)
    names = {'parse_arguments', 'resolve_target', 'validate_target_identity', 'fail', 'is_approved_workdir'}
    namespace = {'pathlib': __import__('pathlib'), 're': __import__('re')}
    exec(compile(ast.Module(body=[n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name in names], type_ignores=[]), str(SCRIPT), 'exec'), namespace)
    return namespace


class CIIdentityTests(unittest.TestCase):
    project = 'token-planet-ci-' + 'a' * 24
    container = 'supabase_db_' + project
    root = '/tmp/token-planet-ci.abcdefgh'
    cid = 'c' * 64
    nid = 'd' * 64

    def fixture(self):
        root = Path(self.root).resolve()
        labels = {'com.supabase.cli.project': self.project, 'com.supabase.cli.workdir': str(root)}
        net = 'token-planet-ci-net-' + 'a' * 24
        return dict(target=(root, self.project, 56432), container=self.container,
                    container_id=self.cid, network_id=self.nid, config=f'project_id = "{self.project}"\n',
                    context={'Endpoints': {'docker': {'Host': 'unix:///var/run/docker.sock'}}},
                    container_info={'Id': self.cid, 'Name': '/' + self.container, 'State': {'Running': True},
                        'Config': {'Labels': labels}, 'NetworkSettings': {'Ports': {'5432/tcp': [{'HostIp': '127.0.0.1', 'HostPort': '56432'}]}, 'Networks': {net: {'NetworkID': self.nid}}},
                        'Mounts': [{'Type': 'volume', 'Name': self.container, 'Destination': '/var/lib/postgresql/data', 'RW': True}]},
                    volume_info={'Name': self.container, 'Labels': {'com.supabase.cli.project': self.project}},
                    network_info={'Id': self.nid, 'Name': net, 'Driver': 'bridge', 'Labels': {'com.tokenplanet.ci.project': self.project, 'com.tokenplanet.ci.workdir': str(root)}, 'Options': {'com.docker.network.bridge.host_binding_ipv4': '127.0.0.1'}})

    def test_ci_arguments_require_exact_complete_contract(self):
        args = ['--ci', '--workdir', self.root, '--container', self.container, '--container-id', self.cid, '--network-id', self.nid]
        parse = helpers()['parse_arguments']
        self.assertEqual(parse(args), ('ci', self.root, self.container, self.cid, self.nid))
        for bad in ([], args[1:], args[:-2], args + ['--extra', 'x'], args + ['--workdir', self.root], args + ['--ci']):
            with self.subTest(bad=bad), self.assertRaises(ValueError): parse(bad)

    def test_qa_arguments_preserve_existing_contract(self):
        self.assertEqual(helpers()['parse_arguments'](['--workdir', str(APPROVED), '--container', 'supabase_db_' + PROJECT]), ('qa', str(APPROVED), 'supabase_db_' + PROJECT, None, None))

    def test_qa_target_preserves_literal_approved_workdir_label(self):
        target = helpers()['resolve_target']('qa', str(APPROVED), 'supabase_db_' + PROJECT)
        self.assertEqual(target, (APPROVED, PROJECT, 56322))
        fixture = self.fixture()
        fixture.update(target=target, container='supabase_db_' + PROJECT, container_id=None,
                       network_id=None, network_info=None, config=f'project_id = "{PROJECT}"\n')
        info = fixture['container_info']
        info['Name'] = '/supabase_db_' + PROJECT
        info['Config']['Labels'] = {'com.supabase.cli.project': PROJECT, 'com.supabase.cli.workdir': str(APPROVED)}
        info['NetworkSettings']['Ports']['5432/tcp'][0]['HostPort'] = '56322'
        info['Mounts'][0]['Name'] = 'supabase_db_' + PROJECT
        fixture['volume_info'] = {'Name': 'supabase_db_' + PROJECT, 'Labels': {'com.supabase.cli.project': PROJECT}}
        self.assertEqual(helpers()['validate_target_identity'](**fixture), self.cid)

    def test_ci_target_accepts_generated_identity_on_56432(self):
        self.assertEqual(helpers()['resolve_target']('ci', self.root, self.container, self.cid, self.nid), (Path(self.root).resolve(), self.project, 56432))

    def test_ci_target_rejects_partial_or_malformed_identity(self):
        resolve = helpers()['resolve_target']
        for args in [('ci', self.root, self.container, None, self.nid), ('ci', self.root, self.container, self.cid, None), ('ci', self.root, 'supabase_db_token-planet-ci-bad', self.cid, self.nid), ('ci', 'relative/token-planet-ci.abcdefgh', self.container, self.cid, self.nid), ('ci', '/tmp/foreign', self.container, self.cid, self.nid), ('qa', self.root, self.container, self.cid, self.nid)]:
            with self.subTest(args=args), self.assertRaises(RuntimeError): resolve(*args)

    def test_ci_target_accepts_canonical_tmp_alias(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); (root / 'real').mkdir(); (root / 'alias').symlink_to(root / 'real', target_is_directory=True)
            resolve = helpers()['resolve_target']
            args = (self.container, self.cid, self.nid)
            self.assertEqual(resolve('ci', str(root / 'alias' / 'token-planet-ci.abcdefgh'), *args), resolve('ci', str(root / 'real' / 'token-planet-ci.abcdefgh'), *args))

    def test_ci_identity_accepts_exact_owned_resources(self):
        self.assertEqual(helpers()['validate_target_identity'](**self.fixture()), self.cid)

    def test_ci_identity_rejects_each_ownership_or_connection_mismatch(self):
        mutations = [(['config'], 'project_id = "foreign"'), (['container_info', 'Id'], 'e'*64), (['container_info', 'Name'], '/foreign'), (['container_info', 'State', 'Running'], False), (['container_info', 'Config', 'Labels', 'com.supabase.cli.project'], 'foreign'), (['container_info', 'Config', 'Labels', 'com.supabase.cli.workdir'], '/foreign'), (['container_info', 'Mounts'], []), (['volume_info', 'Name'], 'foreign'), (['volume_info', 'Labels'], {}), (['network_info', 'Id'], 'e'*64), (['network_info', 'Name'], 'foreign'), (['network_info', 'Labels'], {}), (['network_info', 'Options'], {}), (['network_info', 'Driver'], 'overlay'), (['network_info', 'Labels', 'com.tokenplanet.ci.workdir'], '/foreign'), (['network_info', 'Labels', 'com.tokenplanet.ci.project'], 'foreign'), (['container_info', 'Mounts', 0, 'Type'], 'bind'), (['container_info', 'Mounts', 0, 'Name'], 'foreign'), (['container_info', 'Mounts', 0, 'Destination'], '/foreign'), (['container_info', 'Mounts', 0, 'RW'], False), (['container_info', 'NetworkSettings', 'Networks'], {}), (['context', 'Endpoints', 'docker', 'Host'], 'ssh://remote')]
        for path, value in mutations:
            fixture = self.fixture(); node = fixture
            for key in path[:-1]: node = node[key]
            node[path[-1]] = value
            with self.subTest(path=path), self.assertRaises(RuntimeError): helpers()['validate_target_identity'](**fixture)

    def test_ci_identity_rejects_wrong_missing_or_extra_wildcard_binding(self):
        for entries in ([], [{'HostIp': '127.0.0.1', 'HostPort': '56322'}], [{'HostIp': '0.0.0.0', 'HostPort': '56432'}], [{'HostIp': '::', 'HostPort': '56432'}]):
            fixture = self.fixture(); fixture['container_info']['NetworkSettings']['Ports']['5432/tcp'] = entries
            with self.subTest(entries=entries), self.assertRaises(RuntimeError): helpers()['validate_target_identity'](**fixture)
        fixture = self.fixture(); fixture['container_info']['NetworkSettings']['Ports']['other/tcp'] = [{'HostIp': '0.0.0.0', 'HostPort': '9999'}]
        with self.assertRaises(RuntimeError): helpers()['validate_target_identity'](**fixture)


if __name__ == "__main__":
    unittest.main()
