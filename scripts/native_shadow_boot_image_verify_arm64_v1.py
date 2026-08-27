#!/usr/bin/env python3
"""Read the produced boot images back and check what the operator asked for.

This is the separate verification stage, deliberately not part of the producer.
A producer that verifies its own output can only confirm that it did what it
did; the checks here are written against the sealed lock and the sealed launcher
result, so they can disagree with the thing that built the image.

Six checks came from the operator's list: the kernel is arm64, PID 1 is real
systemd, the launcher's digest equals the sealed build result, the launcher unit
is enabled through the `multi-user.target.wants` symlink, no replay node is
anywhere in the tree, and every tracked path's mode, ownership and content match
the lock.

A seventh came from a boot.  The single MAC.3 attempt got as far as PID 1 and
froze, because the image carries no directory for the kernel filesystems to be
mounted on -- and every check above passed on that image.  `pid1-is-systemd`
asks whether systemd is present, not whether it can get past its first act, so
the five directories are now checked for by name, mode and ownership.  The list
is not written here: it is read from the audit record that took it from the
guest's own systemd, so this stage and the builder cannot end up disagreeing.

The initrd side runs here, because a `newc` archive is readable without root and
without a Linux host.  The root disk side does not: it is an ext4 image, and the
tool that reads it is `debugfs`, which v1 sealed as `ext4-image-inspector`.  This
module emits read-only `debugfs` commands for that stage and never a write.

Verifying an image is not booting one.  Nothing here starts a guest.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import sys
from typing import Any, Optional

from scripts import native_shadow_boot_rootfs_mount_point_audit_arm64_v1 as mount_points
from scripts.native_shadow_boot_initrd_arm64_v1 import InitrdBuildError, parse_newc


BOOTABLE_CLAIM = False
ACTIVATION_ALLOWED = False

# arm64 Linux `Image` header, Documentation/arm64/booting.rst: 64 bytes, with
# the magic at offset 0x38.
KERNEL_MAGIC = b"ARM\x64"
KERNEL_MAGIC_OFFSET = 0x38
KERNEL_HEADER_BYTES = 64
GZIP_MAGIC = b"\x1f\x8b"

SYSTEMD_PATH = "/usr/lib/systemd/systemd"
LAUNCHER_PATH = "/usr/libexec/boole/boole-native-shadow-launcher"
LAUNCHER_UNIT_PATH = "/usr/lib/systemd/system/boole-native-shadow-launcher.service"
LAUNCHER_ENABLEMENT_PATH = (
    "/etc/systemd/system/multi-user.target.wants/boole-native-shadow-launcher.service"
)
REPLAY_NODE_MARKER = "replay-node"

REQUIRED_CHECKS = [
    "kernel-is-arm64",
    "launcher-digest-matches-seal",
    "launcher-service-is-enabled",
    "modes-owners-and-paths-match-the-lock",
    "pid1-is-systemd",
    "replay-node-absent",
    "runtime-mount-points-present",
]

DEBUGFS_READ_COMMANDS = ("stat", "dump", "ls", "features", "show_super_stats")


class ImageVerifyError(RuntimeError):
    """The produced images do not match what the sealed records require."""


def canonical_json(value: Any) -> bytes:
    return (
        json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2, allow_nan=False).encode(
            "utf-8"
        )
        + b"\n"
    )


def kernel_report(raw: bytes) -> dict[str, Any]:
    """An arm64 `Image` announces itself; anything else is not one."""

    if raw[: len(GZIP_MAGIC)] == GZIP_MAGIC:
        return {"architecture": None, "detail": "kernel is still gzip compressed", "ok": False}
    if len(raw) < KERNEL_HEADER_BYTES:
        return {"architecture": None, "detail": f"kernel is {len(raw)} bytes", "ok": False}
    found = raw[KERNEL_MAGIC_OFFSET : KERNEL_MAGIC_OFFSET + len(KERNEL_MAGIC)]
    if found != KERNEL_MAGIC:
        return {
            "architecture": None,
            "detail": f"header magic is {found!r}, not {KERNEL_MAGIC!r}",
            "ok": False,
        }
    return {"architecture": "aarch64", "detail": "arm64 Image header magic present", "ok": True}


def tree_from_initrd(raw: bytes) -> dict[str, dict[str, Any]]:
    """Read the initrd back into logical paths, digests and link targets."""

    tree: dict[str, dict[str, Any]] = {}
    for row in parse_newc(raw):
        if row["name"] == "TRAILER!!!":
            continue
        kind_bits = row["mode"] & 0o170000
        if kind_bits == 0o040000:
            kind = "directory"
        elif kind_bits == 0o120000:
            kind = "symlink"
        else:
            kind = "file"
        entry: dict[str, Any] = {
            "gid": row["gid"],
            "kind": kind,
            "mode": row["mode"] & 0o7777,
            "uid": row["uid"],
        }
        if kind == "symlink":
            entry["target"] = row["data"].decode("utf-8")
        elif kind == "file":
            entry["sha256"] = hashlib.sha256(row["data"]).hexdigest()
        tree["/" + row["name"]] = entry
    return tree


def expectations_from_lock(lock: dict[str, Any]) -> dict[str, Any]:
    """The mode, ownership and content the sealed lock requires of each path."""

    files = {
        row["logicalPath"]: {
            "gid": row["gid"],
            "mode": int(row["mode"], 8),
            "sha256": row["sha256"],
            "uid": row["uid"],
        }
        for row in lock["trackedFiles"]
    }
    symlinks = {
        row["logicalPath"]: {
            "gid": row["gid"],
            "mode": int(row["mode"], 8),
            "target": row["target"],
            "uid": row["uid"],
        }
        for row in lock["derivedEntries"]
        if row["kind"] == "symlink"
    }
    return {"files": files, "symlinks": symlinks}


def _check(identifier: str, ok: bool, detail: str) -> dict[str, Any]:
    return {"detail": detail, "id": identifier, "ok": ok}


def _lock_differences(tree: dict[str, dict[str, Any]], expectations: dict[str, Any]) -> list[str]:
    differences: list[str] = []
    for path, want in sorted(expectations["files"].items()):
        found = tree.get(path)
        if found is None:
            differences.append(f"{path}: missing")
            continue
        for field in ("gid", "mode", "sha256", "uid"):
            if found.get(field) != want[field]:
                differences.append(f"{path}: {field} {found.get(field)!r} != {want[field]!r}")
    for path, want in sorted(expectations["symlinks"].items()):
        found = tree.get(path)
        if found is None:
            differences.append(f"{path}: missing")
            continue
        for field in ("gid", "mode", "target", "uid"):
            if found.get(field) != want[field]:
                differences.append(f"{path}: {field} {found.get(field)!r} != {want[field]!r}")
    return differences


def verify_tree(
    *,
    tree: dict[str, dict[str, Any]],
    expectations: dict[str, Any],
    launcherSha256: str,
    kernel: Optional[bytes] = None,
) -> dict[str, Any]:
    """Run every check the operator named against one assembled tree."""

    checks = []

    if kernel is None:
        checks.append(
            _check("kernel-is-arm64", False, "no kernel was given to this verification run")
        )
    else:
        found = kernel_report(kernel)
        checks.append(_check("kernel-is-arm64", found["ok"], found["detail"]))

    systemd = tree.get(SYSTEMD_PATH)
    checks.append(
        _check(
            "pid1-is-systemd",
            systemd is not None and systemd["kind"] == "file",
            f"{SYSTEMD_PATH} " + ("present" if systemd else "missing"),
        )
    )

    launcher = tree.get(LAUNCHER_PATH)
    checks.append(
        _check(
            "launcher-digest-matches-seal",
            launcher is not None and launcher.get("sha256") == launcherSha256,
            f"{LAUNCHER_PATH}: "
            + ("missing" if launcher is None else f"sha256 {launcher.get('sha256')}"),
        )
    )

    enablement = tree.get(LAUNCHER_ENABLEMENT_PATH)
    checks.append(
        _check(
            "launcher-service-is-enabled",
            enablement is not None
            and enablement["kind"] == "symlink"
            and enablement.get("target") == LAUNCHER_UNIT_PATH,
            f"{LAUNCHER_ENABLEMENT_PATH}: "
            + ("missing" if enablement is None else f"target {enablement.get('target')}"),
        )
    )

    # PID 1 mounts four of these before it runs a unit and stops if one is
    # missing; nothing mounts over the fifth, so it has to be a real directory.
    # An image that reaches this check without them is the image the single
    # MAC.3 boot froze on, and it is cheaper to say so here than to spend a boot
    # finding out.
    mount_point_differences: list[str] = []
    for row in mount_points.required_root_directories():
        path = "/" + row["path"]
        found = tree.get(path)
        if found is None:
            mount_point_differences.append(f"{path}: missing")
        elif found["kind"] != "directory":
            mount_point_differences.append(f"{path}: {found['kind']} rather than a directory")
        elif (found["mode"], found["uid"], found["gid"]) != (int(row["mode"], 8), 0, 0):
            mount_point_differences.append(
                f"{path}: mode {found['mode']:04o} uid {found['uid']} gid {found['gid']}"
            )
    checks.append(
        _check(
            "runtime-mount-points-present",
            not mount_point_differences,
            "all present"
            if not mount_point_differences
            else "; ".join(mount_point_differences),
        )
    )

    replay = sorted(path for path in tree if REPLAY_NODE_MARKER in path)
    checks.append(
        _check(
            "replay-node-absent",
            not replay,
            "none present" if not replay else f"{len(replay)} path(s): {replay[:3]}",
        )
    )

    differences = _lock_differences(tree, expectations)
    checks.append(
        _check(
            "modes-owners-and-paths-match-the-lock",
            not differences,
            "all tracked paths match" if not differences else "; ".join(differences[:5]),
        )
    )

    checks.sort(key=lambda row: row["id"])
    return {
        "activationAllowed": ACTIVATION_ALLOWED,
        "bootableClaim": BOOTABLE_CLAIM,
        "checks": checks,
        "guestBootVerified": False,
        "passed": all(row["ok"] for row in checks),
    }


def assert_passed(report: dict[str, Any]) -> None:
    failed = [row["id"] for row in report["checks"] if not row["ok"]]
    if failed:
        raise ImageVerifyError("verification failed: " + ", ".join(failed))


def debugfs_argv(*, debugfs: str, image: str, command_file: str) -> list[str]:
    """Read-only: no `-w`, so the inspector cannot become a writer by accident."""

    return [debugfs, "-f", command_file, image]


def debugfs_commands(paths: list[str]) -> str:
    lines = ["features", "show_super_stats -h"]
    for path in sorted(paths):
        lines.append(f"stat {path}")
    return "\n".join(lines) + "\n"


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    run = sub.add_parser("verify", help="verify a produced initrd against the sealed records")
    run.add_argument("--initrd", type=pathlib.Path, required=True)
    run.add_argument("--kernel", type=pathlib.Path, required=True)
    run.add_argument("--lock", type=pathlib.Path, required=True)
    run.add_argument("--launcher-sha256", required=True)
    run.add_argument("--output", type=pathlib.Path)
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    args = _parser().parse_args(argv)
    try:
        lock = json.loads(args.lock.read_text(encoding="utf-8"))
        report = verify_tree(
            tree=tree_from_initrd(args.initrd.read_bytes()),
            expectations=expectations_from_lock(lock),
            launcherSha256=args.launcher_sha256,
            kernel=args.kernel.read_bytes(),
        )
    except (InitrdBuildError, OSError, ValueError) as exc:
        print(f"verify: {exc}", file=sys.stderr)
        return 1
    if args.output:
        args.output.write_bytes(canonical_json(report))
    for row in report["checks"]:
        print(f"{'PASS' if row['ok'] else 'FAIL'}  {row['id']}  {row['detail']}")
    print(f"guestBootVerified: {str(report['guestBootVerified']).lower()}")
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
