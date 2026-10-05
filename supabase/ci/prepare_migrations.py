#!/usr/bin/env python3
"""Stage migrations in whole filename UTF-8 byte order with a CI-only synthetic history."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import sys


FORMAT_VERSION = 1
HISTORY_MODE = "synthetic_ci_only"
MANIFEST_NAME = "manifest.json"
SYNTHETIC_BASE = 20000101000000
SYNTHETIC_NAMESPACE_PREFIX = "20000101"
MIGRATION_NAME = re.compile(r"^([0-9]+)_(.+)\.sql$")


class MigrationError(Exception):
    """An invalid source, staging directory, or manifest was supplied."""


def _absolute_path(path, label):
    path = Path(path)
    if not path.is_absolute():
        raise MigrationError(f"{label} path must be absolute: {path}")
    return path


def _validate_directories(source, output):
    source = _absolute_path(source, "source")
    output = _absolute_path(output, "output")

    if source.is_symlink():
        raise MigrationError(f"source directory must not be a symlink: {source}")
    if output.is_symlink():
        raise MigrationError(f"output directory must not be a symlink: {output}")
    if not source.exists() or not source.is_dir():
        raise MigrationError(f"source must be an existing directory: {source}")

    source_real = source.resolve()
    output_real = output.resolve()
    try:
        shared = os.path.commonpath((source_real, output_real))
    except ValueError:
        shared = None
    if shared in (str(source_real), str(output_real)):
        raise MigrationError("source and output directories must not overlap")

    if output.exists() and not output.is_dir():
        raise MigrationError(f"output must be a directory: {output}")
    if not output.exists() and not output.parent.is_dir():
        raise MigrationError(
            f"output parent directory must exist: {output.parent}"
        )
    return source, output


def _parse_filename(path):
    match = MIGRATION_NAME.fullmatch(path.name)
    if match is None:
        raise MigrationError(f"invalid migration filename: {path.name}")
    version, suffix = match.groups()
    if version.startswith(SYNTHETIC_NAMESPACE_PREFIX):
        raise MigrationError(
            f"source migration uses the reserved synthetic namespace: {path.name}"
        )
    return version, suffix


def _read_regular_file(path, label):
    if path.is_symlink():
        raise MigrationError(f"{label} file must not be a symlink: {path}")
    try:
        flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
        descriptor = os.open(path, flags)
    except OSError as error:
        raise MigrationError(f"could not read {label} file {path}: {error}") from error

    try:
        if not stat.S_ISREG(os.fstat(descriptor).st_mode):
            raise MigrationError(f"{label} entry must be a regular file: {path}")
        with os.fdopen(descriptor, "rb") as stream:
            descriptor = -1
            return stream.read()
    finally:
        if descriptor >= 0:
            os.close(descriptor)


def _source_snapshot(source):
    files = []
    versions = set()
    try:
        children = sorted(source.iterdir(), key=lambda child: child.name)
    except OSError as error:
        raise MigrationError(f"could not list source directory {source}: {error}") from error

    if not children:
        raise MigrationError(f"source migration directory is empty: {source}")

    for child in children:
        if child.is_symlink():
            raise MigrationError(f"source migration entry must not be a symlink: {child}")
        if not child.is_file():
            raise MigrationError(f"source migration entry is not a regular file or is a directory: {child}")
        version, suffix = _parse_filename(child)
        if version in versions:
            raise MigrationError(f"duplicate migration version: {version}")
        versions.add(version)
        contents = _read_regular_file(child, "source migration")
        files.append(
            {
                "source_filename": child.name,
                "source_version": version,
                "suffix": suffix,
                "contents": contents,
            }
        )

    files.sort(key=lambda item: item["source_filename"].encode("utf-8"))
    return files


def _manifest_for(files):
    entries = []
    for ordinal, source_file in enumerate(files, start=1):
        synthetic_number = SYNTHETIC_BASE + ordinal
        synthetic_version = f"{synthetic_number:014d}"
        if len(synthetic_version) != 14:
            raise MigrationError("migration count exceeds the synthetic version range")
        entries.append(
            {
                "ordinal": ordinal,
                "source_filename": source_file["source_filename"],
                "source_version": source_file["source_version"],
                "synthetic_version": synthetic_version,
                "staged_filename": (
                    f"{synthetic_version}_{source_file['source_filename']}"
                ),
                "sha256": hashlib.sha256(source_file["contents"]).hexdigest(),
            }
        )
    return {
        "format_version": FORMAT_VERSION,
        "history_mode": HISTORY_MODE,
        "entries": entries,
    }


def _output_inventory(output, expected_names):
    if output.is_symlink() or not output.is_dir():
        raise MigrationError(f"staged output directory is missing or invalid: {output}")
    try:
        children = list(output.iterdir())
    except OSError as error:
        raise MigrationError(f"could not list staged output directory {output}: {error}") from error

    for child in children:
        if child.is_symlink():
            raise MigrationError(f"staged output entry must not be a symlink: {child}")
    actual_names = {child.name for child in children}
    if actual_names != expected_names:
        raise MigrationError(
            "staged output files do not match the manifest "
            f"(missing={sorted(expected_names - actual_names)}, "
            f"extra={sorted(actual_names - expected_names)})"
        )
    return {child.name: child for child in children}


def _verify_snapshot_still_current(source, initial):
    current = _source_snapshot(source)
    if current != initial:
        raise MigrationError("source migrations changed while staging or verifying")


def prepare(source: Path, output: Path) -> dict:
    """Copy source SQL bytes in whole filename UTF-8 byte order and write a manifest."""
    source, output = _validate_directories(source, output)
    source_files = _source_snapshot(source)

    if output.exists() and any(output.iterdir()):
        raise MigrationError(f"output directory must be empty: {output}")

    manifest = _manifest_for(source_files)
    expected_names = {entry["staged_filename"] for entry in manifest["entries"]}
    expected_names.add(MANIFEST_NAME)
    output_was_created = not output.exists()
    if output_was_created:
        try:
            output.mkdir()
        except OSError as error:
            raise MigrationError(f"could not create output directory {output}: {error}") from error

    created = []
    try:
        for entry, source_file in zip(manifest["entries"], source_files):
            staged_path = output / entry["staged_filename"]
            with staged_path.open("xb") as stream:
                created.append(staged_path)
                stream.write(source_file["contents"])
        manifest_path = output / MANIFEST_NAME
        with manifest_path.open("x", encoding="utf-8", newline="\n") as stream:
            created.append(manifest_path)
            json.dump(manifest, stream, indent=2, ensure_ascii=False)
            stream.write("\n")

        _verify_snapshot_still_current(source, source_files)
        _verify_staged_contents(output, source_files, manifest, expected_names)
    except Exception:
        for path in reversed(created):
            try:
                path.unlink(missing_ok=True)
            except OSError:
                pass
        if output_was_created:
            try:
                output.rmdir()
            except OSError:
                pass
        raise

    return manifest


def _verify_staged_contents(output, source_files, manifest, expected_names):
    staged_paths = _output_inventory(output, expected_names)
    manifest_path = staged_paths[MANIFEST_NAME]
    if not stat.S_ISREG(manifest_path.stat().st_mode):
        raise MigrationError(f"staged manifest is not a regular file: {manifest_path}")
    try:
        actual_manifest = json.loads(_read_regular_file(manifest_path, "staged manifest"))
    except (json.JSONDecodeError, UnicodeDecodeError) as error:
        raise MigrationError(f"staged manifest is invalid JSON: {error}") from error
    if actual_manifest != manifest:
        raise MigrationError("staged manifest does not match current source migrations")

    for entry, source_file in zip(manifest["entries"], source_files):
        staged_path = staged_paths[entry["staged_filename"]]
        staged_bytes = _read_regular_file(staged_path, "staged migration")
        if staged_bytes != source_file["contents"]:
            raise MigrationError(f"staged migration bytes differ from source: {staged_path.name}")
        if hashlib.sha256(staged_bytes).hexdigest() != entry["sha256"]:
            raise MigrationError(f"staged migration hash differs from manifest: {staged_path.name}")


def verify(source: Path, output: Path) -> None:
    """Read-only check that source, staged SQL bytes, and manifest still match."""
    source, output = _validate_directories(source, output)
    source_files = _source_snapshot(source)
    manifest = _manifest_for(source_files)
    expected_names = {entry["staged_filename"] for entry in manifest["entries"]}
    expected_names.add(MANIFEST_NAME)
    _verify_staged_contents(output, source_files, manifest, expected_names)
    _verify_snapshot_still_current(source, source_files)


def _parse_args(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    for command in ("prepare", "verify"):
        command_parser = subparsers.add_parser(command)
        command_parser.add_argument("--source", required=True, type=Path)
        command_parser.add_argument("--output", required=True, type=Path)
    return parser.parse_args(argv)


def main(argv=None):
    args = _parse_args(argv)
    try:
        if args.command == "prepare":
            manifest = prepare(args.source, args.output)
            print(f"Prepared {len(manifest['entries'])} migrations in {args.output}")
        else:
            verify(args.source, args.output)
            print(f"Verified migration staging in {args.output}")
    except MigrationError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
