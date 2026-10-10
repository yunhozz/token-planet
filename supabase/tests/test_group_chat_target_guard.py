"""Target guard rejects foreign projects before any SQL mutation."""
import ast
from pathlib import Path
import unittest

SCRIPT = Path(__file__).with_name("group_chat_concurrency.sh")

def helpers():
    source = SCRIPT.read_text().split("<<'PY'\n", 1)[1].rsplit("\nPY", 1)[0]
    nodes = [n for n in ast.parse(source).body if isinstance(n, ast.FunctionDef) and n.name in {"validate_target", "fail"}]
    namespace = {"pathlib": __import__("pathlib"), "re": __import__("re")}
    exec(compile(ast.Module(body=nodes, type_ignores=[]), str(SCRIPT), "exec"), namespace)
    return namespace

class TargetGuardTests(unittest.TestCase):
    def fixture(self):
        return dict(workdir="/tmp/token-planet-group-chat-supabase", config='project_id = "token-planet-group-chat-qa"',
            context={"Endpoints":{"docker":{"Host":"unix:///var/run/docker.sock"}}},
            info={"Id":"a"*64,"Name":"/supabase_db_token-planet-group-chat-qa","State":{"Running":True},
                "Config":{"Labels":{"com.supabase.cli.project":"token-planet-group-chat-qa","com.supabase.cli.workdir":"/tmp/token-planet-group-chat-supabase"}},
                "NetworkSettings":{"Ports":{"5432/tcp":[{"HostIp":"0.0.0.0","HostPort":"54322"},{"HostIp":"::","HostPort":"54322"}]}}})
    def test_exact_local_target(self):
        self.assertEqual(helpers()["validate_target"](**self.fixture()), "a"*64)
    def test_wrong_path_project_port_remote_docker_rejected(self):
        mutations=[(["workdir"],"/tmp/foreign"),(["config"],'project_id = "production"'),
            (["context","Endpoints","docker","Host"],"ssh://remote"),(["info","Name"],"/foreign"),
            (["info","Config","Labels","com.supabase.cli.project"],"foreign"),
            (["info","NetworkSettings","Ports","5432/tcp"],[{"HostIp":"127.0.0.1","HostPort":"55432"}])]
        for path,value in mutations:
            fixture=self.fixture(); node=fixture
            for key in path[:-1]: node=node[key]
            node[path[-1]]=value
            with self.subTest(path=path), self.assertRaises(RuntimeError): helpers()["validate_target"](**fixture)

if __name__ == "__main__": unittest.main()
