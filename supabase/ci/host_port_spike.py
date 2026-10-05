"""Safe temporary-project preparation for the Supabase CLI host-port spike."""

import argparse
import json
import os
import re
import shutil
import shlex
import signal
import socket
import subprocess
import tempfile
from pathlib import Path
import sys
import uuid


POSTGRES_VERSION = "17.6.1.171"
POSTGRES_IMAGE = "public.ecr.aws/supabase/postgres:17.6.1.171"
DB_PORT = 56432
SHADOW_PORT = 56430
PROJECT_PREFIX = "token-planet-ci-"
WORKDIR_PREFIX = "token-planet-ci."
EXPECTED_CLI_VERSION = "2.119.0"
EXCLUDED_SERVICES = "gotrue,realtime,storage-api,imgproxy,kong,mailpit,postgrest,postgres-meta,studio,edge-runtime,logflare,vector,supavisor"
ADAPTER_PATH = Path(__file__).with_name("host_port_adapter.py").resolve()
_SAFE_IDENTITY_RE = re.compile(r"[A-Za-z0-9_.-]{1,128}\Z")
_CONTAINER_ID_RE = re.compile(r"[0-9a-f]{12,64}\Z")
_CONTAINER_STATES = {"created", "running", "paused", "restarting", "removing", "exited", "dead"}
_ADAPTER_REJECTION_CATEGORIES = {
    "unexpected publish mode": "publish_mode",
    "option is missing its value": "missing_option_value",
    "label is malformed": "malformed_label",
    "unexpected create option": "create_option",
    "unexpected create image": "create_image",
    "unexpected create name": "create_name",
    "unexpected create network": "create_network",
    "unexpected database publish": "database_publish",
    "unexpected database volume mount": "database_volume_mount",
    "unexpected database volume source": "database_volume_source",
    "duplicate create label": "duplicate_create_label",
    "unexpected create labels": "create_labels",
    "adapter is not configured": "adapter_configuration",
    "docker image pulls are disabled in this adapter": "image_pull_disabled",
    "docker network creation is disabled in this adapter": "network_creation_disabled",
    "docker container create is disabled in this adapter": "container_creation_disabled",
    "docker container run is disabled in this adapter": "container_run_disabled",
    "duplicate volume create in one cli phase": "duplicate_volume_create",
    "volume create guard could not be recorded": "volume_guard_unavailable",
    "volume create did not match this disposable database": "volume_create_mismatch",
    "duplicate create in one cli phase": "duplicate_container_create",
    "create guard could not be recorded": "container_guard_unavailable",
    "create did not match this disposable database": "container_create_mismatch",
}
_CLI_FAILURE_CATEGORIES = (
    (
        re.compile(r"address already in use|port(?:s)?\s+\d{1,5}\s+(?:is\s+)?already allocated", re.I),
        "port_conflict",
        "database host port is already in use",
    ),
    (
        re.compile(r"cannot connect to the docker daemon|docker daemon.{0,40}(?:unavailable|not running)", re.I),
        "docker_unavailable",
        "Docker could not start the database service",
    ),
    (
        re.compile(r"pull access denied|manifest unknown|image.{0,40}(?:not found|unavailable)", re.I),
        "image_unavailable",
        "the required database image is unavailable",
    ),
    (
        re.compile(r"health.?check|unhealthy", re.I),
        "service_unhealthy",
        "a database service did not become healthy",
    ),
    (
        re.compile(r"context deadline exceeded|timed? out|timeout", re.I),
        "start_timeout",
        "the database service did not start before the timeout",
    ),
)


class SpikeInterrupted(RuntimeError):
    """Raised once when this run receives SIGTERM."""

    def __init__(self, signum):
        self.signal_name = signal.Signals(signum).name
        super().__init__(f"interrupted by {self.signal_name}")


def _safe_identity(value):
    """Return an identity only when it cannot inject log text or secrets."""
    return value if isinstance(value, str) and _SAFE_IDENTITY_RE.fullmatch(value) else "unavailable"


def _classify_failure_text(value):
    if not isinstance(value, str) or not value:
        return None
    lowered = value.lower()
    for pattern, category, message in _CLI_FAILURE_CATEGORIES:
        if pattern.search(lowered):
            return category, message
    return None


def _append_start_diagnostics(summary, result):
    """Log only known static diagnostic categories, never CLI output itself."""
    adapter_prefix = "host-port adapter rejected Docker operation:"
    for source, output in (("stdout", result.stdout), ("stderr", result.stderr)):
        if not isinstance(output, str):
            output = ""
        output_bytes = len(output.encode("utf-8", errors="replace"))
        lines = output.splitlines()
        line_count = len(lines)
        if output_bytes == 0:
            summary.append(f"diagnostic=empty source={source} bytes=0 lines=0")
            continue

        diagnostics = []
        unclassified = False
        for line in lines:
            if not line.strip():
                unclassified = True
                continue
            if line.startswith(adapter_prefix):
                reason = line[len(adapter_prefix):].strip().lower()
                category = _ADAPTER_REJECTION_CATEGORIES.get(reason)
                if category:
                    diagnostic = f"diagnostic=adapter_reject category={category} source={source}"
                    if diagnostic not in diagnostics:
                        diagnostics.append(diagnostic)
                else:
                    unclassified = True
                continue
            classified = _classify_failure_text(line)
            if classified:
                category, message = classified
                diagnostic = f"diagnostic={category} source={source} message={message}"
                if diagnostic not in diagnostics:
                    diagnostics.append(diagnostic)
            else:
                unclassified = True

        if unclassified or not diagnostics:
            diagnostics.append(f"diagnostic=unclassified source={source}")
        for diagnostic in diagnostics:
            summary.append(f"{diagnostic} bytes={output_bytes} lines={line_count}")


def _run_process_group(args, *, cwd=None, env=None, capture_output=None, text=None, check=None):
    """Run a command in its own process group and reap it if interrupted."""
    process = None
    try:
        process = subprocess.Popen(
            args,
            cwd=cwd,
            env=env,
            stdout=subprocess.PIPE if capture_output else None,
            stderr=subprocess.PIPE if capture_output else None,
            text=text,
            start_new_session=True,
        )
        stdout, stderr = process.communicate()
    except BaseException:
        if process is None:
            raise
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            stdout, stderr = process.communicate(timeout=2)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            stdout, stderr = process.communicate()
        except BaseException:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            stdout, stderr = process.communicate()
        raise
    result = subprocess.CompletedProcess(args, process.returncode, stdout, stderr)
    if check and result.returncode:
        raise subprocess.CalledProcessError(result.returncode, args, stdout, stderr)
    return result


def _render_spike_config(project_id):
    """Render this spike profile, which has no product migrations or SQL tests."""
    return f'''project_id = "{project_id}"

[api]
enabled = false

[auth]
enabled = false

[db]
port = 56432
shadow_port = 56430
major_version = 17
health_timeout = "2m"

[db.migrations]
enabled = true
schema_paths = []

[db.seed]
enabled = false
sql_paths = []

[realtime]
enabled = false

[storage]
enabled = false
'''


def prepare_spike(artifacts_dir):
    """Prepare an empty external artifacts directory and isolated CLI project."""
    repo_root = Path(__file__).resolve().parents[2]
    requested_artifacts = Path(artifacts_dir).expanduser()
    if not requested_artifacts.is_absolute():
        raise ValueError("artifacts directory must be absolute")
    if requested_artifacts.is_symlink():
        raise ValueError("artifacts directory must not be a symlink")

    canonical_artifacts = requested_artifacts.resolve(strict=False)
    if canonical_artifacts == repo_root or repo_root in canonical_artifacts.parents:
        raise ValueError("artifacts directory must be outside the repository")
    if canonical_artifacts.exists() and not canonical_artifacts.is_dir():
        raise ValueError("artifacts path must be a directory")
    if canonical_artifacts.exists() and any(canonical_artifacts.iterdir()):
        raise ValueError("artifacts directory must be empty")

    canonical_artifacts.mkdir(parents=True, exist_ok=True, mode=0o700)
    if any(canonical_artifacts.iterdir()):
        raise ValueError("artifacts directory must be empty")

    temporary_root = Path(tempfile.gettempdir()).resolve()
    if temporary_root == repo_root or repo_root in temporary_root.parents:
        raise ValueError("temporary directory must be outside the repository")

    project_id = PROJECT_PREFIX + uuid.uuid4().hex[:24]
    workdir = Path(tempfile.mkdtemp(prefix=WORKDIR_PREFIX, dir=temporary_root)).resolve()
    try:
        supabase_dir = workdir / "supabase"
        temp_dir = supabase_dir / ".temp"
        temp_dir.mkdir(parents=True, mode=0o700)
        config_path = supabase_dir / "config.toml"
        config_path.write_text(_render_spike_config(project_id))
        (temp_dir / "postgres-version").write_text(f"{POSTGRES_VERSION}\n")
        (supabase_dir / "migrations").mkdir(mode=0o700)
        return workdir, project_id, config_path
    except BaseException:
        shutil.rmtree(workdir, ignore_errors=True)
        raise


def preflight_spike(workdir, project_id, *, environ=None, run=None, port_probe=None):
    """Check local CLI/Docker prerequisites without mutating Docker resources."""
    environment = dict(os.environ if environ is None else environ)
    runner = subprocess.run if run is None else run
    probe = _probe_loopback_ports if port_probe is None else port_probe
    workdir = Path(workdir).resolve()
    repo_root = Path(__file__).resolve().parents[2]
    temporary_root = Path(tempfile.gettempdir()).resolve()

    if not re.fullmatch(r"token-planet-ci-[0-9a-f]{24}", project_id):
        raise RuntimeError("generated project identity is invalid")
    if (
        not workdir.is_dir()
        or workdir.parent != temporary_root
        or not workdir.name.startswith(WORKDIR_PREFIX)
        or workdir == repo_root
        or repo_root in workdir.parents
    ):
        raise RuntimeError("generated project directory is invalid")
    config_path = workdir / "supabase" / "config.toml"
    cache_pin = workdir / "supabase" / ".temp" / "postgres-version"
    migrations_dir = workdir / "supabase" / "migrations"
    try:
        config_matches = (
            config_path.is_file()
            and config_path.read_text() == _render_spike_config(project_id)
        )
    except OSError:
        config_matches = False
    if (
        not config_matches
        or not cache_pin.is_file()
        or cache_pin.read_text() != f"{POSTGRES_VERSION}\n"
        or not migrations_dir.is_dir()
        or any(migrations_dir.iterdir())
    ):
        raise RuntimeError("generated empty CLI project does not match this run")

    for variable, value in environment.items():
        if variable.startswith("SUPABASE_") and variable not in {
            "SUPABASE_BIN", "SUPABASE_TELEMETRY_DISABLED"
        } and value:
            raise RuntimeError(f"refusing Supabase environment override: {variable}")
    for variable in (
        "DATABASE_URL", "PGHOST", "PGHOSTADDR", "PGPORT", "PGDATABASE",
        "PGUSER", "PGPASSWORD", "PGSERVICE", "PGSERVICEFILE",
        "DOCKER_HOST", "DOCKER_CONTEXT", "DOCKER_TLS_VERIFY", "DOCKER_CERT_PATH",
    ):
        if environment.get(variable):
            raise RuntimeError(f"refusing connection environment override: {variable}")

    supabase_bin = environment.get("SUPABASE_BIN", "")
    if not Path(supabase_bin).is_absolute() or not os.access(supabase_bin, os.X_OK):
        raise RuntimeError("SUPABASE_BIN must be an absolute executable path")
    supabase_bin = str(Path(supabase_bin).resolve())
    docker_bin = shutil.which("docker", path=environment.get("PATH"))
    if not docker_bin or not Path(docker_bin).is_absolute() or not os.access(docker_bin, os.X_OK):
        raise RuntimeError("Docker executable is unavailable")
    docker_bin = str(Path(docker_bin).resolve())

    def invoke(command, *, cwd=None):
        try:
            return runner(
                command,
                cwd=cwd,
                env=environment,
                capture_output=True,
                text=True,
                check=False,
            )
        except OSError as error:
            raise RuntimeError("preflight command could not be executed") from error

    version = invoke([supabase_bin, "--version"], cwd=workdir)
    if version.returncode != 0 or version.stdout.strip() != EXPECTED_CLI_VERSION:
        raise RuntimeError(f"expected Supabase CLI {EXPECTED_CLI_VERSION}")

    context_result = invoke([docker_bin, "context", "show"])
    context = context_result.stdout.strip()
    if context_result.returncode != 0 or not context:
        raise RuntimeError("could not inspect the active Docker context")
    endpoint_result = invoke([
        docker_bin,
        "context", "inspect", "--format", "{{json .Endpoints.docker.Host}}", context,
    ])
    if endpoint_result.returncode != 0:
        raise RuntimeError("could not inspect the active Docker endpoint")
    try:
        endpoint = json.loads(endpoint_result.stdout)
    except (TypeError, json.JSONDecodeError) as error:
        raise RuntimeError("Docker endpoint response is invalid") from error
    if not isinstance(endpoint, str) or not endpoint.startswith("unix:///"):
        raise RuntimeError("Docker daemon must use a local Unix socket endpoint")

    info = invoke([docker_bin, "info"])
    if info.returncode != 0:
        raise RuntimeError("Docker engine is unavailable")

    image = invoke([docker_bin, "image", "inspect", POSTGRES_IMAGE])
    if image.returncode != 0:
        raise RuntimeError("required cached Postgres image is unavailable")
    try:
        inspected_images = json.loads(image.stdout)
    except (TypeError, json.JSONDecodeError) as error:
        raise RuntimeError("cached Postgres image inspect response is invalid") from error
    if not isinstance(inspected_images, list) or not any(
        isinstance(entry, dict) and POSTGRES_IMAGE in (entry.get("RepoTags") or [])
        for entry in inspected_images
    ):
        raise RuntimeError("required cached Postgres image tag is unavailable")

    suffix = project_id[len(PROJECT_PREFIX):]
    generated = (
        ("network", f"token-planet-ci-net-{suffix}"),
        ("container", f"supabase_db_{project_id}"),
        ("volume", f"supabase_db_{project_id}"),
    )
    not_found_patterns = {
        "network": re.compile(r"no such network(?::|\s)|network .* (?:not found|does not exist)", re.I),
        "container": re.compile(r"no such (?:object|container)(?::|\s)|container .* (?:not found|does not exist)", re.I),
        "volume": re.compile(r"no such volume(?::|\s)|volume .* (?:not found|does not exist)", re.I),
    }
    for kind, name in generated:
        result = invoke([
            docker_bin, kind, "inspect", "--format", "{{json .}}", name,
        ])
        if result.returncode == 0:
            raise RuntimeError(f"generated Docker {kind} name already exists")
        if not not_found_patterns[kind].search(result.stderr):
            raise RuntimeError(f"could not confirm generated Docker {kind} name is absent")

    try:
        probe((DB_PORT, SHADOW_PORT))
    except OSError as error:
        raise RuntimeError("one or more required local ports are unavailable") from error

    return {
        "docker_bin": docker_bin,
        "supabase_bin": supabase_bin,
        "context": context,
        "endpoint": endpoint,
        "postgres_image": POSTGRES_IMAGE,
        "project_id": project_id,
        "workdir": workdir,
        "network_name": generated[0][1],
        "container_name": generated[1][1],
        "volume_name": generated[2][1],
        "ports": (DB_PORT, SHADOW_PORT),
    }


def _probe_loopback_ports(ports):
    for port in ports:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
            sock.bind(("127.0.0.1", port))


def _run_spike(artifacts_dir, *, environ=None, run=None, port_probe=None, start_only=False):
    """Run one isolated host-port probe and remove owned Docker resources."""
    environment = dict(os.environ if environ is None else environ)
    runner = _run_process_group if run is None else run
    requested_artifacts = Path(artifacts_dir).expanduser()
    if requested_artifacts.is_symlink():
        raise ValueError("artifacts directory must not be a symlink")
    artifacts = requested_artifacts.resolve(strict=False)
    workdir = None
    project_id = None
    preflight = None
    network_attempted = False
    failure = None
    cleanup_errors = []
    summary = [f"spike_mode={'start_only' if start_only else 'start_reset'}"]
    artifacts_claimed = False

    def write_summary():
        if artifacts_claimed and artifacts.exists() and artifacts.is_dir():
            (artifacts / "spike.log").write_text("\n".join(summary) + ("\n" if summary else ""))

    def invoke(command, *, cwd=None, env=None):
        try:
            return runner(
                command,
                cwd=cwd,
                env=environment if env is None else env,
                capture_output=True,
                text=True,
                check=False,
            )
        except OSError as error:
            raise RuntimeError("spike command could not be executed") from error

    def read_json(kind, target, *, allow_absent=False):
        result = invoke([
            preflight["docker_bin"], kind, "inspect", "--format", "{{json .}}", target,
        ])
        if result.returncode != 0:
            pattern = {
                "network": r"no such network(?::|\s)|network .* (?:not found|does not exist)",
                "container": r"no such (?:object|container)(?::|\s)|container .* (?:not found|does not exist)",
                "volume": r"no such volume(?::|\s)|volume .* (?:not found|does not exist)",
            }[kind]
            if allow_absent and re.search(pattern, result.stderr, re.I):
                return None
            raise RuntimeError(f"could not inspect generated Docker {kind}")
        try:
            value = json.loads(result.stdout)
        except (TypeError, json.JSONDecodeError) as error:
            raise RuntimeError(f"generated Docker {kind} inspect response is invalid") from error
        if not isinstance(value, dict):
            raise RuntimeError(f"generated Docker {kind} inspect response is invalid")
        return value

    def assert_owned(kind, value):
        if value is None:
            return False
        labels = value.get("Labels") or {}
        if kind == "network":
            expected = (
                value.get("Name") == preflight["network_name"]
                and labels.get("com.tokenplanet.ci.project") == project_id
                and labels.get("com.tokenplanet.ci.workdir") == str(workdir)
            )
            options = value.get("Options") or {}
            expected = expected and options.get("com.docker.network.bridge.host_binding_ipv4") == "127.0.0.1"
        elif kind == "container":
            config = value.get("Config") or {}
            labels = config.get("Labels") or {}
            expected = (
                value.get("Name", "").lstrip("/") == preflight["container_name"]
                and labels.get("com.supabase.cli.project") == project_id
                and labels.get("com.docker.compose.project") == project_id
                and labels.get("com.supabase.cli.workdir") == str(workdir)
                and config.get("Image") == POSTGRES_IMAGE
            )
        elif kind == "volume":
            expected = (
                value.get("Name") == preflight["volume_name"]
                and labels.get("com.supabase.cli.project") == project_id
                and labels.get("com.docker.compose.project") == project_id
            )
        else:
            raise RuntimeError("unknown generated Docker resource type")
        if not expected:
            raise RuntimeError(f"generated Docker {kind} ownership could not be verified")
        return True

    def check_binding_and_ownership():
        container = read_json("container", preflight["container_name"])
        assert_owned("container", container)
        volume = read_json("volume", preflight["volume_name"])
        assert_owned("volume", volume)
        network = read_json("network", preflight["network_id"])
        assert_owned("network", network)

        ports = ((container.get("NetworkSettings") or {}).get("Ports") or {})
        postgres_bindings = ports.get("5432/tcp") or []
        if not isinstance(postgres_bindings, list) or not any(
            isinstance(entry, dict)
            and entry.get("HostIp") == "127.0.0.1"
            and entry.get("HostPort") == str(DB_PORT)
            for entry in postgres_bindings
        ):
            raise RuntimeError("database published binding is not 127.0.0.1")
        for entries in ports.values():
            for entry in entries or []:
                if not isinstance(entry, dict) or entry.get("HostIp") != "127.0.0.1":
                    raise RuntimeError("a published database port is not restricted to loopback")

        attached = ((container.get("NetworkSettings") or {}).get("Networks") or {}).values()
        if not any(item.get("NetworkID") == preflight["network_id"] for item in attached):
            raise RuntimeError("database container is not attached to the generated network")

        mounts = container.get("Mounts") or []
        data_mounts = [mount for mount in mounts if mount.get("Destination") == "/var/lib/postgresql/data"]
        if len(data_mounts) != 1 or data_mounts[0].get("Name") != preflight["volume_name"]:
            raise RuntimeError("database data volume does not match this run")
        named_volumes = [mount for mount in mounts if mount.get("Type") == "volume"]
        if any(mount.get("Name") != preflight["volume_name"] for mount in named_volumes):
            raise RuntimeError("database container has an unexpected named volume")
        return container.get("Id")

    def run_phase(phase, command):
        container_marker = workdir / f"{phase}-container-create-seen"
        volume_marker = workdir / f"{phase}-volume-create-seen"
        if container_marker.exists() or volume_marker.exists():
            raise RuntimeError(f"{phase} adapter marker already exists")
        cli_bin = workdir / "adapter-bin"
        cli_bin.mkdir(mode=0o700, exist_ok=True)
        wrapper = cli_bin / "docker"
        wrapper.write_text(
            "#!/bin/sh\nexec "
            + shlex.quote(sys.executable)
            + " "
            + shlex.quote(str(ADAPTER_PATH))
            + ' "$@"\n'
        )
        wrapper.chmod(0o700)
        child_env = dict(environment)
        child_env["PATH"] = str(cli_bin) + os.pathsep + environment.get("PATH", "")
        child_env["TOKEN_PLANET_REAL_DOCKER"] = preflight["docker_bin"]
        child_env["TOKEN_PLANET_CI_PROJECT_ID"] = project_id
        child_env["TOKEN_PLANET_CI_WORKDIR"] = str(workdir)
        child_env["TOKEN_PLANET_CI_NETWORK_ID"] = preflight["network_id"]
        child_env["TOKEN_PLANET_CI_CONTAINER_NAME"] = preflight["container_name"]
        child_env["TOKEN_PLANET_CI_CREATE_MARKER"] = str(container_marker)
        child_env["TOKEN_PLANET_CI_VOLUME_CREATE_MARKER"] = str(volume_marker)
        result = invoke([preflight["supabase_bin"], *command], cwd=workdir, env=child_env)
        if result.returncode != 0:
            summary.append(f"phase={phase} status=FAIL exit={result.returncode}")
            if phase == "start":
                container_marker_state = "present" if container_marker.is_file() else "absent"
                volume_marker_state = "present" if volume_marker.is_file() else "absent"
                summary.append(
                    "phase=start adapter_markers "
                    f"container_create={container_marker_state} volume_create={volume_marker_state}"
                )
                _append_start_diagnostics(summary, result)
                try:
                    failed_container = read_json("container", preflight["container_name"], allow_absent=True)
                except RuntimeError:
                    summary.append("start_failure container=unavailable reason=inspect_failed")
                else:
                    if failed_container is None:
                        summary.append(
                            f"start_failure container=absent expected_name={_safe_identity(preflight['container_name'])}"
                        )
                    else:
                        try:
                            assert_owned("container", failed_container)
                        except RuntimeError:
                            summary.append("start_failure container=unavailable reason=ownership_unverified")
                        else:
                            container_id = failed_container.get("Id")
                            if not isinstance(container_id, str) or not _CONTAINER_ID_RE.fullmatch(container_id):
                                container_id = "unavailable"
                            state = failed_container.get("State") or {}
                            status = state.get("Status")
                            if status not in _CONTAINER_STATES:
                                status = "unknown"
                            exit_code = state.get("ExitCode")
                            if type(exit_code) is not int:
                                exit_code = "unknown"
                            state_error = state.get("Error")
                            if not state_error:
                                error_category = "none"
                            else:
                                classified_error = _classify_failure_text(state_error)
                                error_category = classified_error[0] if classified_error else "other"
                            summary.append(
                                f"start_failure container_id={container_id} "
                                f"container_name={_safe_identity(preflight['container_name'])}"
                            )
                            summary.append(
                                f"container_state status={status} exit_code={exit_code} error={error_category}"
                            )
            write_summary()
            raise RuntimeError(f"Supabase {phase} failed")
        if not container_marker.is_file() or not volume_marker.is_file():
            summary.append(f"phase={phase} status=FAIL code=adapter_marker_missing")
            write_summary()
            raise RuntimeError(f"Supabase {phase} did not create its expected database resources")
        try:
            container_id = check_binding_and_ownership()
        except RuntimeError as error:
            diagnostic_code = {
                "database published binding is not 127.0.0.1": "postgres_binding",
                "a published database port is not restricted to loopback": "published_port_binding",
            }.get(str(error), "resource_ownership")
            summary.append(f"phase={phase} status=FAIL code={diagnostic_code}")
            write_summary()
            raise
        summary.append(f"phase={phase} status=PASS container_id={container_id}")
        write_summary()

    try:
        workdir, project_id, _config = prepare_spike(artifacts)
        artifacts_claimed = True
        preflight = preflight_spike(
            workdir,
            project_id,
            environ=environment,
            run=runner,
            port_probe=port_probe,
        )
        suffix = project_id[len(PROJECT_PREFIX):]
        network_name = f"token-planet-ci-net-{suffix}"
        network_attempted = True
        create_network = invoke([
            preflight["docker_bin"], "network", "create", "--driver", "bridge",
            "--opt", "com.docker.network.bridge.host_binding_ipv4=127.0.0.1",
            "--label", f"com.tokenplanet.ci.project={project_id}",
            "--label", f"com.tokenplanet.ci.workdir={workdir}",
            network_name,
        ])
        if create_network.returncode != 0:
            raise RuntimeError("could not create the generated loopback network")
        network_id = create_network.stdout.strip()
        if not network_id:
            raise RuntimeError("Docker did not return the generated network id")
        network = read_json("network", network_id)
        assert_owned("network", network)
        preflight["network_id"] = network_id
        summary.append("network_status=PASS binding=127.0.0.1")
        summary.append(
            "run_identity "
            f"project_id={_safe_identity(project_id)} "
            f"network_id={_safe_identity(network_id)} "
            f"container_name={_safe_identity(preflight['container_name'])} "
            f"volume_name={_safe_identity(preflight['volume_name'])}"
        )
        write_summary()

        run_phase("start", [
            "start", "--network-id", network_id, "--exclude", EXCLUDED_SERVICES,
        ])
        if not start_only:
            run_phase("reset", [
                "db", "reset", "--local", "--no-seed", "--network-id", network_id,
            ])
    except BaseException as error:
        failure = error

    if preflight is not None:
        docker_bin = preflight["docker_bin"]
        for kind, target, remove_command in (
            ("container", preflight["container_name"], lambda value: [docker_bin, "container", "rm", "-f", value]),
            ("volume", preflight["volume_name"], lambda value: [docker_bin, "volume", "rm", value]),
        ):
            try:
                resource = read_json(kind, target, allow_absent=True)
                if resource is None:
                    summary.append(f"cleanup_status=PASS kind={kind} state=absent")
                    continue
                assert_owned(kind, resource)
                identity = resource.get("Id") if kind == "container" else resource.get("Name")
                if not identity:
                    raise RuntimeError(f"generated Docker {kind} identity is empty")
                result = invoke(remove_command(identity))
                if result.returncode != 0:
                    raise RuntimeError(f"could not remove generated Docker {kind}")
                summary.append(f"cleanup_status=PASS kind={kind} state=removed")
            except BaseException as error:
                cleanup_errors.append(str(error))
                summary.append(f"cleanup_status=FAIL kind={kind}")

        if network_attempted:
            try:
                network = read_json("network", preflight.get("network_id") or preflight["network_name"], allow_absent=True)
                if network is None:
                    summary.append("cleanup_status=PASS kind=network state=absent")
                else:
                    assert_owned("network", network)
                    identity = network.get("Id")
                    if not identity:
                        raise RuntimeError("generated Docker network identity is empty")
                    result = invoke([docker_bin, "network", "rm", identity])
                    if result.returncode != 0:
                        raise RuntimeError("could not remove generated Docker network")
                    summary.append("cleanup_status=PASS kind=network state=removed")
            except BaseException as error:
                cleanup_errors.append(str(error))
                summary.append("cleanup_status=FAIL kind=network")

    if workdir is not None:
        try:
            cleanup_spike(workdir)
            summary.append("cleanup_status=PASS kind=workdir state=removed")
        except BaseException as error:
            cleanup_errors.append(str(error))
            summary.append("cleanup_status=FAIL kind=workdir")
    if isinstance(failure, SpikeInterrupted):
        summary.append(f"spike_status=FAIL code=interrupted signal={failure.signal_name}")
    elif failure is not None:
        summary.append("spike_status=FAIL")
    elif cleanup_errors:
        summary.append("spike_status=FAIL code=cleanup")
    else:
        summary.append("spike_status=PASS")
    write_summary()

    if failure is not None:
        if cleanup_errors:
            raise RuntimeError(f"{failure}; cleanup also reported {len(cleanup_errors)} error(s)") from failure
        raise RuntimeError(str(failure)) from failure
    if cleanup_errors:
        raise RuntimeError(f"spike cleanup reported {len(cleanup_errors)} error(s)")
    return {"status": "PASS", "project_id": project_id, "artifacts_dir": artifacts}


def run_spike(artifacts_dir, *, environ=None, run=None, port_probe=None, start_only=False):
    """Run a spike while converting the first SIGTERM into orderly cleanup."""
    previous_handler = signal.getsignal(signal.SIGTERM)
    interrupted = False

    def handle_sigterm(signum, _frame):
        nonlocal interrupted
        if interrupted:
            return
        interrupted = True
        raise SpikeInterrupted(signum)

    signal.signal(signal.SIGTERM, handle_sigterm)
    try:
        return _run_spike(
            artifacts_dir,
            environ=environ,
            run=run,
            port_probe=port_probe,
            start_only=start_only,
        )
    finally:
        signal.signal(signal.SIGTERM, previous_handler)


def cleanup_spike(workdir):
    """Remove only a generated spike workdir directly under the system temp root."""
    path = Path(workdir).resolve(strict=False)
    temporary_root = Path(tempfile.gettempdir()).resolve()
    if (
        not path.is_absolute()
        or path.parent != temporary_root
        or not path.name.startswith(WORKDIR_PREFIX)
        or path == temporary_root
    ):
        raise ValueError("refusing to remove a non-generated spike workdir")
    shutil.rmtree(path, ignore_errors=False)


def main(argv=None):
    parser = argparse.ArgumentParser(description="Run an isolated Supabase host-port spike")
    parser.add_argument("--artifacts-dir", required=True, help="empty absolute path outside the repository")
    parser.add_argument("--start-only", action="store_true", help="start and verify without database reset")
    arguments = parser.parse_args(argv)
    try:
        if arguments.start_only:
            result = run_spike(arguments.artifacts_dir, start_only=True)
        else:
            result = run_spike(arguments.artifacts_dir)
    except (RuntimeError, ValueError):
        print("spike_status=FAIL", file=sys.stderr)
        return 1
    print(f"spike_status={result['status']}")
    return 0 if result.get("status") == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
