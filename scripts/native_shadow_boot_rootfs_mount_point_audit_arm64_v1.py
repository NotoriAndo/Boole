#!/usr/bin/env python3
"""The directories the guest's own systemd mounts over, and a check for them.

PID 1 mounts `/proc`, `/sys`, `/dev` and `/run` before it runs a single unit,
and it stops if any of those directories is missing.  Nothing mounts over
`/tmp` in this image, so that one has to be a real directory instead.  Five
names, and the reason each is on the list is recorded next to it rather than
carried in someone's head.

The list is not restated here.  It is read from
`native-shadow-boot-rootfs-runtime-mount-points-arm64-v1.json`, which also holds
the audit it came from -- the mount table decoded out of the guest's own
`libsystemd-shared`, every `.mount` unit the image ships, and the absence of
`/etc/fstab`.  The sealed closure exception already names the same five for a
different purpose, and `required_root_directories` refuses if the two disagree,
so the new record cannot drift away from the sealed one without being caught.

Two callers.  The rootfs builder asks for `mount_point_entries` and merges them
into the tree it is about to write.  `main` takes a listing of a tree that
already exists and says what it is missing, which is how an image gets checked
without being booted.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import sys
from typing import Any, Iterable, Optional, Sequence


REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parents[1]
CONTAINMENT = REPOSITORY_ROOT / "native/containment"
RECORD_PATH = CONTAINMENT / "native-shadow-boot-rootfs-runtime-mount-points-arm64-v1.json"
CLOSURE_EXCEPTION_PATH = (
    CONTAINMENT / "native-shadow-boot-rootfs-closure-exception-arm64-v1.json"
)

BOOTABLE_CLAIM = False
ACTIVATION_ALLOWED = False

# A directory mode that is writable by anyone but not sticky is a mistake in a
# root filesystem, and a set-id bit on a mount point is never wanted.
FORBIDDEN_MODE_BITS = 0o6000
STICKY_BIT = 0o1000


class MountPointAuditError(RuntimeError):
    """Raised when the required directories cannot be read as a usable list."""


def _record() -> dict[str, Any]:
    try:
        return json.loads(RECORD_PATH.read_text(encoding="utf-8"))
    except OSError as exc:
        raise MountPointAuditError("the runtime mount point record is unreadable") from exc
    except json.JSONDecodeError as exc:
        raise MountPointAuditError("the runtime mount point record is not JSON") from exc


def _sealed_runtime_roots() -> frozenset[str]:
    """The same five names, from the record that was sealed before this one."""

    try:
        document = json.loads(CLOSURE_EXCEPTION_PATH.read_text(encoding="utf-8"))
    except OSError as exc:
        raise MountPointAuditError("the boot closure exception is unreadable") from exc
    roots = document.get("runtimeFilesystemRoots")
    if not isinstance(roots, list) or not roots:
        raise MountPointAuditError("the boot closure exception names no runtime filesystems")
    return frozenset(roots)


def required_root_directories() -> tuple[dict[str, Any], ...]:
    """The five, validated, in the order they will be written."""

    rows = _record().get("requiredRootDirectories")
    if not isinstance(rows, list) or not rows:
        raise MountPointAuditError("the record names no required root directories")
    seen: set[str] = set()
    for row in rows:
        path = row.get("path")
        if not isinstance(path, str) or not path or "/" in path or path.startswith("."):
            raise MountPointAuditError(f"required root directory is not a name: {path!r}")
        if path in seen:
            raise MountPointAuditError(f"required root directory is listed twice: {path}")
        seen.add(path)
        if row.get("kind") != "directory":
            raise MountPointAuditError(f"required root entry is not a directory: {path}")
        if row.get("uid") != 0 or row.get("gid") != 0:
            raise MountPointAuditError(f"required root directory is not root:root: {path}")
        if not row.get("reason"):
            raise MountPointAuditError(f"required root directory gives no reason: {path}")
        mode = _mode(path, row.get("mode"))
        if mode & FORBIDDEN_MODE_BITS:
            raise MountPointAuditError(f"required root directory carries a set-id bit: {path}")
        if mode & 0o022 and not mode & STICKY_BIT:
            raise MountPointAuditError(
                f"required root directory is group- or world-writable without the sticky bit: {path}"
            )
    if seen != set(_sealed_runtime_roots()):
        raise MountPointAuditError(
            "the required root directories differ from the sealed runtime filesystem roots"
        )
    return tuple(sorted(rows, key=lambda row: row["path"]))


def _mode(path: str, raw: Any) -> int:
    if not isinstance(raw, str) or not raw:
        raise MountPointAuditError(f"required root directory has no mode: {path}")
    try:
        return int(raw, 8)
    except ValueError as exc:
        raise MountPointAuditError(f"required root directory mode is not octal: {path}") from exc


def mount_point_entries() -> dict[str, dict[str, Any]]:
    """The five as rootfs builder entries, keyed by the path they are written to.

    The shape is the builder's own directory entry and nothing more: a leading
    slash would make them absolute and a mode carried as a string would be
    compared against an integer somewhere downstream, so both are settled here.
    """

    return {
        row["path"]: {
            "path": row["path"],
            "kind": "directory",
            "mode": _mode(row["path"], row["mode"]),
            "uid": 0,
            "gid": 0,
        }
        for row in required_root_directories()
    }


def top_level_of(where: str) -> str:
    """`/sys/fs/cgroup` and `sys/fs/cgroup` both live under `sys`."""

    return where.lstrip("/").split("/", 1)[0]


def missing_from(paths: Iterable[str]) -> tuple[str, ...]:
    """Which of the five a tree does not have, in a stable order."""

    present = {top_level_of(path) for path in paths}
    return tuple(row["path"] for row in required_root_directories() if row["path"] not in present)


def _paths_from_listing(text: str) -> set[str]:
    """Top-level names out of a `cpio -itv`-style or plain path listing.

    A listing line may carry metadata and a symlink arrow.  Only the path
    matters, and only its first segment, so everything after ` -> ` is dropped
    and the last field is taken.
    """

    paths: set[str] = set()
    for line in text.splitlines():
        line = line.split(" -> ", 1)[0].strip()
        if not line:
            continue
        candidate = line.split()[-1]
        if candidate in {".", "./"}:
            continue
        paths.add(top_level_of(candidate.removeprefix("./")))
    return paths


def audit_listing(text: str) -> dict[str, Any]:
    """What a listing of a produced tree is missing, as a result rather than a print."""

    present = _paths_from_listing(text)
    missing = missing_from(present)
    return {
        "schema": "boole.native-shadow.boot-rootfs-mount-point-audit.v1",
        "requiredRootDirectories": [row["path"] for row in required_root_directories()],
        "observedTopLevel": sorted(present),
        "missing": list(missing),
        "verdict": "PASS" if not missing else "FAIL",
        "bootableClaim": BOOTABLE_CLAIM,
        "activationAllowed": ACTIVATION_ALLOWED,
    }


def main(argv: Optional[Sequence[str]] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--listing",
        type=pathlib.Path,
        help="a file listing the tree's paths; omit to print the required directories",
    )
    args = parser.parse_args(argv)
    try:
        if args.listing is None:
            payload: dict[str, Any] = {
                "requiredRootDirectories": list(required_root_directories()),
                "bootableClaim": BOOTABLE_CLAIM,
                "activationAllowed": ACTIVATION_ALLOWED,
            }
            print(json.dumps(payload, indent=2, sort_keys=True))
            return 0
        result = audit_listing(args.listing.read_text(encoding="utf-8", errors="replace"))
    except (MountPointAuditError, OSError) as exc:
        print(f"mount-point-audit: {exc}", file=sys.stderr)
        return 2
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0 if result["verdict"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
