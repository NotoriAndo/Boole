#!/usr/bin/env python3
"""Rebuild and publish the sealed arm64 launcher-v2 ELF exactly once.

The tracked launcher-v2 result proves what bytes two independent arm64 builds
produced.  This module is a narrower consumer: it performs one fresh build and
publishes a file only when the bytes reproduce that exact result and the
producer-path scan is complete and zero.  It neither places the file in a guest
image nor grants image production, boot, activation, or serving authority.
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import stat
import sys
import tempfile
from typing import Any, Mapping, Optional

REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parents[1]
if str(REPOSITORY_ROOT) not in sys.path:
    sys.path.insert(0, str(REPOSITORY_ROOT))

from scripts import native_shadow_launcher_build_arm64_v2 as build


BOOTABLE_CLAIM = False
ACTIVATION_ALLOWED = False
LAUNCHER_DEPLOYED_INTO_GUEST = False

SEALED_RESULT_SHA256 = (
    "0ffa4035b8f7f3e698c2ac57eead4b8122cb0c462ab2cb170a87c1973bb01b08"
)
RESULT_PATH = build.RESULT_PATH
PUBLISH_MODE = 0o755
TEMPORARY_SUFFIX = ".boole-v2-new"


class LauncherV2EmitError(RuntimeError):
    """The rebuild or publication differs from the sealed launcher-v2 result."""


def _canonical(raw: bytes) -> dict[str, Any]:
    try:
        record = json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise LauncherV2EmitError("launcher-v2 result is not UTF-8 JSON") from exc
    if build.v1.canonical_json(record) != raw:
        raise LauncherV2EmitError("launcher-v2 result is not canonical JSON")
    if not isinstance(record, dict):
        raise LauncherV2EmitError("launcher-v2 result is not an object")
    return record


def validate_path_scan(scan: Mapping[str, Any]) -> dict[str, int]:
    """Require every declared producer-path class, and no invented class."""

    authority = build.load_authority()
    expected = set(authority["determinism"]["producerPathClasses"])
    if not isinstance(scan, Mapping) or set(scan) != expected:
        raise LauncherV2EmitError("launcher-v2 producer-path scan classes differ")
    if any(type(value) is not int or value != 0 for value in scan.values()):
        raise LauncherV2EmitError("launcher-v2 producer-path scan is not all zero")
    return {label: int(scan[label]) for label in sorted(expected)}


def validate_seal(record: Any) -> dict[str, Any]:
    """Reconstruct the complete result and reject partial or aliased records."""

    if not isinstance(record, dict):
        raise LauncherV2EmitError("launcher-v2 result is not an object")
    try:
        launcher = record["launcher"]
        if not isinstance(launcher, dict):
            raise LauncherV2EmitError("launcher-v2 result seals no launcher")
        sha256 = launcher["sha256"]
        size = launcher["sizeBytes"]
        if (
            not isinstance(sha256, str)
            or len(sha256) != 64
            or any(character not in "0123456789abcdef" for character in sha256)
        ):
            raise LauncherV2EmitError("launcher-v2 digest is malformed")
        if type(size) is not int or size <= 0:
            raise LauncherV2EmitError("launcher-v2 size is not positive")
        scan = validate_path_scan(record["producerPathHits"])
        authority = build.load_authority()
        expected = build.build_result(
            authority,
            built={
                "buildCount": record["independentBuildCount"],
                "producerPathHits": scan,
                "sha256": sha256,
                "sizeBytes": size,
            },
            identity=record["observedToolchain"],
        )
    except (KeyError, TypeError) as exc:
        raise LauncherV2EmitError("launcher-v2 result fields differ") from exc
    if record != expected:
        raise LauncherV2EmitError("launcher-v2 result differs from its reconstruction")
    if launcher["guestLogicalPath"] != authority["build"]["guestLogicalPath"]:
        raise LauncherV2EmitError("launcher-v2 guest path differs from its authority")
    if any(value is not False for value in record["boundaries"].values()):
        raise LauncherV2EmitError("launcher-v2 result opens a later boundary")
    return record


def load_seal(path: pathlib.Path = RESULT_PATH) -> dict[str, Any]:
    """Load the exact tracked result, not merely a similar JSON object."""

    try:
        raw = path.read_bytes()
    except OSError as exc:
        raise LauncherV2EmitError("sealed launcher-v2 result is unreadable") from exc
    if build.sha256_bytes(raw) != SEALED_RESULT_SHA256:
        raise LauncherV2EmitError("sealed launcher-v2 result differs from its pin")
    return validate_seal(_canonical(raw))


def rebuild(repo_root: pathlib.Path = REPOSITORY_ROOT) -> bytes:
    """Run one authorised arm64 build and require a complete zero path scan."""

    build._require_arm64_linux()
    authority = build.load_authority()
    with tempfile.TemporaryDirectory(prefix="boole-launcher-v2-emit-") as scratch:
        raw, scan = build.build_once(
            authority,
            build_root=pathlib.Path(scratch),
            repo_root=repo_root,
        )
    validate_path_scan(scan)
    return raw


def _temporary_path(path: pathlib.Path) -> pathlib.Path:
    if not path.name or path.name in {".", ".."}:
        raise LauncherV2EmitError("launcher-v2 output path has no filename")
    return path.with_name(f".{path.name}{TEMPORARY_SUFFIX}")


def _open_output_directory(path: pathlib.Path) -> int:
    if not path.is_absolute():
        raise LauncherV2EmitError("launcher-v2 output path must be absolute")
    _temporary_path(path)
    flags = os.O_RDONLY
    flags |= getattr(os, "O_CLOEXEC", 0)
    flags |= getattr(os, "O_DIRECTORY", 0)
    flags |= getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(os.fspath(path.parent), flags)
    except OSError as exc:
        raise LauncherV2EmitError(
            "launcher-v2 output parent is not a real directory"
        ) from exc
    if not stat.S_ISDIR(os.fstat(descriptor).st_mode):
        os.close(descriptor)
        raise LauncherV2EmitError(
            "launcher-v2 output parent is not a real directory"
        )
    return descriptor


def _name_exists(directory: int, name: str) -> bool:
    try:
        os.stat(name, dir_fd=directory, follow_symlinks=False)
    except FileNotFoundError:
        return False
    return True


def _preflight_destination(path: pathlib.Path) -> None:
    """Reject known destination state before spending a build."""

    temporary_name = _temporary_path(path).name
    directory = _open_output_directory(path)
    try:
        if _name_exists(directory, path.name):
            raise LauncherV2EmitError(
                f"refusing to overwrite launcher-v2 output {path}"
            )
        if _name_exists(directory, temporary_name):
            raise LauncherV2EmitError(
                f"refusing to reuse launcher-v2 temporary output "
                f"{_temporary_path(path)}"
            )
    finally:
        os.close(directory)


def _publish_once(path: pathlib.Path, raw: bytes) -> None:
    """Expose only complete bytes and never overwrite a concurrent writer."""

    temporary = _temporary_path(path)
    temporary_name = temporary.name
    directory = _open_output_directory(path)
    if _name_exists(directory, path.name):
        os.close(directory)
        raise LauncherV2EmitError(f"refusing to overwrite launcher-v2 output {path}")
    if _name_exists(directory, temporary_name):
        os.close(directory)
        raise LauncherV2EmitError(
            f"refusing to reuse launcher-v2 temporary output {temporary}"
        )

    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    flags |= getattr(os, "O_CLOEXEC", 0)
    flags |= getattr(os, "O_NOFOLLOW", 0)
    descriptor: Optional[int] = None
    try:
        descriptor = os.open(temporary_name, flags, 0o600, dir_fd=directory)
        view = memoryview(raw)
        while view:
            written = os.write(descriptor, view)
            if written <= 0:
                raise OSError("short write while publishing launcher-v2")
            view = view[written:]
        os.fchmod(descriptor, PUBLISH_MODE)
        os.fsync(descriptor)
        os.close(descriptor)
        descriptor = None
        # A hard link fails with EEXIST rather than replacing a path that raced
        # into existence after the checks above.  The final name therefore
        # never exposes partially written bytes.
        os.link(
            temporary_name,
            path.name,
            src_dir_fd=directory,
            dst_dir_fd=directory,
            follow_symlinks=False,
        )
        os.fsync(directory)
        os.unlink(temporary_name, dir_fd=directory)
        os.fsync(directory)
    finally:
        if descriptor is not None:
            os.close(descriptor)
        try:
            os.unlink(temporary_name, dir_fd=directory)
            os.fsync(directory)
        except FileNotFoundError:
            pass
        finally:
            os.close(directory)


def emit(path: pathlib.Path) -> dict[str, Any]:
    """Rebuild, match the exact seal, and atomically publish once."""

    path = pathlib.Path(path)
    _preflight_destination(path)
    # Keep both checks on the production path. load_seal binds the tracked raw
    # bytes; validate_seal reconstructs the complete semantic record. Neither
    # dependency is injectable through this public API.
    accepted = validate_seal(load_seal())
    raw = rebuild()
    if not isinstance(raw, bytes) or not raw:
        raise LauncherV2EmitError("launcher-v2 rebuild produced no bytes")
    launcher = accepted["launcher"]
    if len(raw) != launcher["sizeBytes"]:
        raise LauncherV2EmitError("launcher-v2 rebuild size differs from its seal")
    digest = build.sha256_bytes(raw)
    if digest != launcher["sha256"]:
        raise LauncherV2EmitError("launcher-v2 rebuild digest differs from its seal")
    _publish_once(path, raw)
    return {
        "activationAllowed": ACTIVATION_ALLOWED,
        "bootableClaim": BOOTABLE_CLAIM,
        "guestLogicalPath": launcher["guestLogicalPath"],
        "launcherDeployedIntoGuest": LAUNCHER_DEPLOYED_INTO_GUEST,
        "path": str(path),
        "sha256": digest,
        "sizeBytes": len(raw),
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subcommands = parser.add_subparsers(dest="command", required=True)
    run = subcommands.add_parser("emit", help="rebuild and publish launcher-v2")
    run.add_argument("--out", type=pathlib.Path, required=True)
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    args = _parser().parse_args(argv)
    try:
        result = emit(args.out)
    except (LauncherV2EmitError, build.LauncherV2BuildError, OSError) as exc:
        print(f"launcher-v2 emit refused: {exc}", file=sys.stderr)
        return 1
    print(
        f"launcher-v2 emitted bytes={result['sizeBytes']} "
        f"sha256={result['sha256']} path={result['path']}"
    )
    print("bootableClaim: false")
    print("activationAllowed: false")
    return 0


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
