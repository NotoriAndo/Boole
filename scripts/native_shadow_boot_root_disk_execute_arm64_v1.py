#!/usr/bin/env python3
"""Execute the frozen ext4 root disk plan on the arm64 producer, deciding nothing.

The plan module works out what the image must be and deliberately runs nothing,
because the host that plans is not the host that builds.  This module is the
other half: it stages the tree the plan describes, checks the two tools against
the digests the plan froze, and runs the argv the plan wrote with the
environment the plan wrote.  Every value it uses comes from the plan or from the
layer; it contributes none of its own, so a difference between two independent
jobs cannot start here.

Two orderings matter and neither is cosmetic.  Entries are created in the plan's
order because `mke2fs -d` walks the staging tree with readdir and never sorts
it, so on a filesystem that returns creation order the tree it reads is the
order recorded here.  Modes and times are applied afterwards in reverse, deepest
first, because creating a child updates its parent's time and a directory whose
final mode forbids writing cannot be filled after the fact.

Ownership is not negotiable and not simulated.  `mke2fs -d` copies each staged
file's owner into the image, so the produce phase runs as root and the image is
root-owned throughout; a run that is not root stages fine and refuses to build,
rather than writing an image owned by whoever happened to invoke it.

This module reads the local filesystem and runs one pinned aarch64 tool.  It
fetches nothing.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import subprocess
import sys
from typing import Any, Mapping, Optional, Sequence

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

from scripts.native_shadow_boot_root_disk_arm64_v1 import (
    CANONICAL_MTIME,
    canonical_json,
    layer_entries,
    staging_entries,
)


BOOTABLE_CLAIM = False
ACTIVATION_ALLOWED = False
GUEST_IMAGE_BUILT = False

ABORT_TOOL_DIGEST_MISMATCH = "tool-binary-digest-mismatch"
ABORT_OUTPUT_MISSING = "output-missing-or-empty"

SCHEMA = "boole.native-shadow.boot-root-disk-execute-result.arm64.v1"
RELEASE = "NATIVE-SHADOW-BOOT-ROOT-DISK-EXECUTE-ARM64-V1"
STATUS = "ROOT-DISK-IMAGE-WRITTEN-FROM-THE-FROZEN-PLAN-NOT-BOOT-AUTHORITY"

# Permissive enough to fill, restrictive enough not to be the final answer: the
# real mode arrives in the second pass, once the children are in place.
BUILD_DIRECTORY_MODE = 0o755
BUILD_FILE_MODE = 0o600


class RootDiskExecuteError(RuntimeError):
    """The plan, the layer or the tools on disk do not agree."""


def _safe_relative(path: str) -> pathlib.PurePosixPath:
    """A staging path that names something inside the staging root, or nothing."""

    if not path or path.startswith("/"):
        raise RootDiskExecuteError(f"staging path is not relative: {path!r}")
    candidate = pathlib.PurePosixPath(path)
    if candidate.is_absolute() or any(part in ("", ".", "..") for part in candidate.parts):
        raise RootDiskExecuteError(f"staging path leaves the staging root: {path!r}")
    return candidate


def _rows(plan: Mapping[str, Any]) -> list[dict[str, Any]]:
    staging = plan.get("staging")
    if not isinstance(staging, Mapping):
        raise RootDiskExecuteError("plan describes no staging tree")
    rows = staging.get("entries")
    if not isinstance(rows, list) or not rows:
        raise RootDiskExecuteError("plan stages no entries")
    return [dict(row) for row in rows]


def assert_plan_matches_layer(plan: Mapping[str, Any], layer: bytes) -> list[dict[str, Any]]:
    """The plan's metadata and the layer's contents must be the same tree."""

    if plan.get("executed"):
        raise RootDiskExecuteError("plan is already marked executed; it is not a fresh plan")
    rows = _rows(plan)
    for row in rows:
        if row.get("uid") != 0 or row.get("gid") != 0:
            raise RootDiskExecuteError(
                f"staging entry is not root-owned: {row.get('path')!r}"
            )
        if row.get("mtime") != CANONICAL_MTIME:
            raise RootDiskExecuteError(
                f"staging entry does not carry the canonical time: {row.get('path')!r}"
            )
        _safe_relative(str(row.get("path")))
    derived = staging_entries(layer_entries(layer))
    if derived != rows:
        raise RootDiskExecuteError(
            "the layer does not produce the tree this plan stages; "
            f"plan={len(rows)} entries, layer={len(derived)} entries"
        )
    return rows


def stage_tree(plan: Mapping[str, Any], layer: bytes) -> list[str]:
    """Create the staging tree the plan describes, in the plan's order."""

    rows = assert_plan_matches_layer(plan, layer)
    root = pathlib.Path(str(plan["staging"]["path"]))
    if root.exists() and any(root.iterdir()):
        raise RootDiskExecuteError(f"staging root is not empty: {root}")
    root.mkdir(parents=True, exist_ok=True)

    contents = {entry["name"]: entry["data"] for entry in layer_entries(layer)}
    created: list[str] = []
    for row in rows:
        target = root / str(_safe_relative(str(row["path"])))
        kind = row["kind"]
        if kind == "directory":
            target.mkdir(mode=BUILD_DIRECTORY_MODE)
        elif kind == "symlink":
            os.symlink(row["target"], target)
        elif kind == "file":
            target.write_bytes(contents[row["path"]])
            os.chmod(target, BUILD_FILE_MODE)
        else:
            raise RootDiskExecuteError(f"staging entry has an unknown kind: {kind!r}")
        created.append(row["path"])

    staged_as_root = os.geteuid() == 0
    for row in reversed(rows):
        target = root / str(_safe_relative(str(row["path"])))
        if row["kind"] != "symlink":
            os.chmod(target, row["mode"])
        if staged_as_root:
            os.chown(target, 0, 0, follow_symlinks=False)
        os.utime(target, (CANONICAL_MTIME, CANONICAL_MTIME), follow_symlinks=False)
    if staged_as_root:
        os.chown(root, 0, 0)
    os.utime(root, (CANONICAL_MTIME, CANONICAL_MTIME))
    return created


def assert_tools(plan: Mapping[str, Any]) -> dict[str, str]:
    """Both pinned tools must be on disk with the bytes the plan froze."""

    tools = plan.get("tools")
    if not isinstance(tools, Mapping) or not tools:
        raise RootDiskExecuteError("plan pins no tools")
    resolved: dict[str, str] = {}
    for name in sorted(tools):
        row = tools[name]
        path = pathlib.Path(str(row["path"]))
        try:
            raw = path.read_bytes()
        except OSError as exc:
            raise RootDiskExecuteError(
                f"{ABORT_TOOL_DIGEST_MISMATCH}: cannot read {name} at {path}"
            ) from exc
        if len(raw) != row["sizeBytes"]:
            raise RootDiskExecuteError(
                f"{ABORT_TOOL_DIGEST_MISMATCH}: {name} is {len(raw)} bytes, "
                f"the plan says {row['sizeBytes']}"
            )
        digest = hashlib.sha256(raw).hexdigest()
        if digest != row["sha256"]:
            raise RootDiskExecuteError(
                f"{ABORT_TOOL_DIGEST_MISMATCH}: {name} hashes to {digest}, "
                f"the plan says {row['sha256']}"
            )
        resolved[name] = digest
    return resolved


def mke2fs_command(plan: Mapping[str, Any]) -> list[str]:
    argv = plan.get("mke2fs", {}).get("argv")
    if not isinstance(argv, list) or not argv:
        raise RootDiskExecuteError("plan carries no mke2fs argv")
    return [str(item) for item in argv]


def mke2fs_environment(plan: Mapping[str, Any]) -> dict[str, str]:
    """Exactly the plan's environment: nothing from the caller leaks in."""

    env = plan.get("mke2fs", {}).get("env")
    if not isinstance(env, Mapping) or not env:
        raise RootDiskExecuteError("plan carries no mke2fs environment")
    return {str(key): str(value) for key, value in env.items()}


def execute(plan: Mapping[str, Any], layer: bytes) -> dict[str, Any]:
    """Stage, check the tools, write the image, and report what was written."""

    if os.geteuid() != 0:
        raise RootDiskExecuteError(
            "the produce phase must run as root: mke2fs -d copies the staged owner "
            "into the image, and the frozen plan says root:root throughout"
        )
    digests = assert_tools(plan)
    created = stage_tree(plan, layer)
    argv = mke2fs_command(plan)
    finished = subprocess.run(  # noqa: S603 - pinned argv from the frozen plan
        argv,
        env=mke2fs_environment(plan),
        stdin=subprocess.DEVNULL,
        capture_output=True,
        check=False,
    )
    if finished.returncode != 0:
        raise RootDiskExecuteError(
            f"mke2fs exited {finished.returncode}: "
            f"{finished.stderr.decode('utf-8', 'replace').strip()}"
        )
    image = pathlib.Path(str(plan["mke2fs"]["argv"][-2]))
    if not image.is_file():
        raise RootDiskExecuteError(f"{ABORT_OUTPUT_MISSING}: {image} was not written")
    raw = image.read_bytes()
    if not raw:
        raise RootDiskExecuteError(f"{ABORT_OUTPUT_MISSING}: {image} is zero bytes")
    return {
        "activationAllowed": ACTIVATION_ALLOWED,
        "bootableClaim": BOOTABLE_CLAIM,
        "boundaries": {
            "guestBootVerified": False,
            "guestImageBuilt": GUEST_IMAGE_BUILT,
            "rootDiskBuilt": True,
            "runtimeCompatibilityVerified": False,
        },
        "executed": True,
        "image": {
            "name": image.name,
            "sha256": hashlib.sha256(raw).hexdigest(),
            "sizeBytes": len(raw),
        },
        "release": RELEASE,
        "schema": SCHEMA,
        "stagedEntryCount": len(created),
        "status": STATUS,
        "toolDigests": digests,
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    run = sub.add_parser("run", help="execute a frozen root disk plan")
    run.add_argument("--plan", type=pathlib.Path, required=True)
    run.add_argument("--layer", type=pathlib.Path, required=True)
    run.add_argument("--output", type=pathlib.Path, required=True)
    return parser


def main(argv: Optional[Sequence[str]] = None) -> int:
    args = _parser().parse_args(argv)
    try:
        plan = json.loads(args.plan.read_text(encoding="utf-8"))
        result = execute(plan, args.layer.read_bytes())
    except (RootDiskExecuteError, OSError, ValueError) as exc:
        print(f"root-disk-execute: {exc}", file=sys.stderr)
        return 1
    args.output.write_bytes(canonical_json(result))
    print(
        f"root-disk {result['image']['name']} bytes={result['image']['sizeBytes']} "
        f"sha256={result['image']['sha256']}"
    )
    print(f"bootableClaim: {str(BOOTABLE_CLAIM).lower()}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
