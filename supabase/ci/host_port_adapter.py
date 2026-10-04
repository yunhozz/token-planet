"""Fail-closed adapter for the Supabase CLI's pinned Docker create argv."""

import os
from pathlib import Path
import re
import sys

PINNED_IMAGE = "public.ecr.aws/supabase/postgres:17.6.1.171"
EXPECTED_PUBLISH = "56432:5432"
LOOPBACK_PUBLISH = "127.0.0.1:56432:5432"

_VALUE_OPTIONS = {
    "--name", "--hostname", "-e", "-v", "--volumes-from", "--tmpfs",
    "-p", "--publish", "--expose", "--health-cmd", "--health-interval",
    "--health-timeout", "--health-retries", "--health-start-period", "--restart",
    "--security-opt", "--add-host", "--network", "--network-alias", "--label",
    "--entrypoint",
}
_FLAG_OPTIONS = {"--rm"}
_EXPECTED_LABELS = (
    "com.supabase.cli.project",
    "com.docker.compose.project",
    "com.supabase.cli.workdir",
)


class AdapterError(ValueError):
    pass


def adapt_docker_argv(args, *, project_id, workdir, network_id, container_name, image=PINNED_IMAGE):
    """Rewrite exactly one pinned local DB publish and keep every other argv token unchanged."""
    args = list(args)
    if not args or args[0] != "create":
        return args

    values = {
        "--name": [],
        "--network": [],
        "--label": [],
        "publish": [],
        "-v": [],
        "--volumes-from": [],
        "--tmpfs": [],
    }
    publish_positions = []
    image_index = None
    index = 1
    while index < len(args):
        token = args[index]
        if token == "-P" or token == "--publish-all":
            raise AdapterError("unexpected publish mode")
        option, equals, inline_value = token.partition("=")
        if option in _VALUE_OPTIONS:
            if equals:
                value = inline_value
                value_index = index
            else:
                if index + 1 >= len(args):
                    raise AdapterError("option is missing its value")
                value = args[index + 1]
                value_index = index + 1
                index += 1
            key = "publish" if option in ("-p", "--publish") else option
            if key in values:
                values[key].append(value)
                if key == "publish":
                    publish_positions.append((value_index, option, equals))
                elif key == "--label" and "=" not in value:
                    raise AdapterError("label is malformed")
        elif option in _FLAG_OPTIONS and not equals:
            pass
        elif token.startswith("-"):
            raise AdapterError("unexpected create option")
        else:
            image_index = index
            break
        index += 1

    if image_index is None or args[image_index] != image:
        raise AdapterError("unexpected create image")
    if len(values["--name"]) != 1 or values["--name"][0] != container_name:
        raise AdapterError("unexpected create name")
    if len(values["--network"]) != 1 or values["--network"][0] != network_id:
        raise AdapterError("unexpected create network")
    if len(values["publish"]) != 1 or values["publish"][0] != EXPECTED_PUBLISH:
        raise AdapterError("unexpected database publish")
    if values["-v"] != [f"{container_name}:/var/lib/postgresql/data"]:
        raise AdapterError("unexpected database volume mount")
    if values["--volumes-from"] or values["--tmpfs"]:
        raise AdapterError("unexpected database volume source")

    labels = {}
    for value in values["--label"]:
        key, label_value = value.split("=", 1)
        if key in labels:
            raise AdapterError("duplicate create label")
        labels[key] = label_value
    expected_labels = {
        "com.supabase.cli.project": project_id,
        "com.docker.compose.project": project_id,
        "com.supabase.cli.workdir": workdir,
    }
    if labels != expected_labels:
        raise AdapterError("unexpected create labels")

    updated = args.copy()
    value_index, option, equals = publish_positions[0]
    replacement = LOOPBACK_PUBLISH
    updated[value_index] = f"{option}={replacement}" if equals else replacement
    updated.insert(1, "--pull=never")
    return updated


def _reject(message):
    print(f"host-port adapter rejected Docker operation: {message}", file=sys.stderr)
    raise SystemExit(125)


def _claim_marker(marker, operation):
    try:
        descriptor = os.open(marker, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    except FileExistsError:
        _reject(f"duplicate {operation} in one CLI phase")
    except OSError:
        _reject(f"{operation} guard could not be recorded")
    else:
        os.close(descriptor)


def main(args):
    real_docker = os.environ.get("TOKEN_PLANET_REAL_DOCKER", "")
    if not real_docker.startswith("/") or not os.access(real_docker, os.X_OK):
        _reject("adapter is not configured")
    real_docker = str(Path(real_docker).resolve())

    if args and (args[0] == "pull" or args[:2] == ["image", "pull"]):
        _reject("Docker image pulls are disabled in this adapter")
    if args[:2] == ["network", "create"]:
        _reject("Docker network creation is disabled in this adapter")
    if args[:2] == ["container", "create"]:
        _reject("Docker container create is disabled in this adapter")
    if args and (args[0] == "run" or args[:2] == ["container", "run"]):
        _reject("Docker container run is disabled in this adapter")

    if args[:2] == ["volume", "create"]:
        project_id = os.environ.get("TOKEN_PLANET_CI_PROJECT_ID", "")
        marker = os.environ.get("TOKEN_PLANET_CI_VOLUME_CREATE_MARKER", "")
        expected = [
            "volume", "create",
            "--label", f"com.supabase.cli.project={project_id}",
            "--label", f"com.docker.compose.project={project_id}",
            f"supabase_db_{project_id}",
        ]
        if (
            not re.fullmatch(r"token-planet-ci-[0-9a-f]{24}", project_id)
            or not Path(marker).is_absolute()
            or args != expected
        ):
            _reject("volume create did not match this disposable database")
        _claim_marker(marker, "volume create")
        os.execv(real_docker, [real_docker, *args])

    if not args or args[0] != "create":
        os.execv(real_docker, [real_docker, *args])

    project_id = os.environ.get("TOKEN_PLANET_CI_PROJECT_ID", "")
    workdir = os.environ.get("TOKEN_PLANET_CI_WORKDIR", "")
    network_id = os.environ.get("TOKEN_PLANET_CI_NETWORK_ID", "")
    container_name = os.environ.get("TOKEN_PLANET_CI_CONTAINER_NAME", "")
    marker = os.environ.get("TOKEN_PLANET_CI_CREATE_MARKER", "")
    if (
        not re.fullmatch(r"token-planet-ci-[0-9a-f]{24}", project_id)
        or container_name != f"supabase_db_{project_id}"
        or not Path(workdir).is_absolute()
        or not network_id
        or not Path(marker).is_absolute()
    ):
        _reject("adapter is not configured")

    try:
        rewritten = adapt_docker_argv(
            args,
            project_id=project_id,
            workdir=workdir,
            network_id=network_id,
            container_name=container_name,
        )
    except AdapterError:
        _reject("create did not match this disposable database")

    _claim_marker(marker, "create")

    os.execv(real_docker, [real_docker, *rewritten])


if __name__ == "__main__":
    main(sys.argv[1:])
