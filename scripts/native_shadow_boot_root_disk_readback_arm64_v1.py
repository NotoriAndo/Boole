#!/usr/bin/env python3
"""Open the produced ext4 root disk and check what is actually inside it.

Everything between the frozen layer and the staging tree is pinned step by step,
and the image is hashed the moment it is written.  What is not pinned is the
writer: `mke2fs` was handed a tree and a plan, and nothing so far has opened the
file it produced to see whether the tree arrived.  A digest proves two runs
wrote the same bytes; it does not prove those bytes hold the right filesystem.

So this reads the image back through the same ext4 driver a boot would use, and
hands what it finds to the verification stage that already exists.  Reusing that
stage is the whole point: the initrd and the root disk are two containers for
one tree, so they answer to one set of checks, and a second set written for the
second container would be a second thing that can drift.

This runs outside the transient unit the producer authority seals, and it has
to.  That unit is sealed with private devices, which is exactly the thing a loop
mount needs, so the reading is a separate stage rather than one more step of the
producing one.  It is also the honest arrangement: the stage that checks the
work is not the stage that did it.

Reading an image back is not booting it.  Nothing here starts a virtual machine,
and `bootableClaim` stays false in everything it writes.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import platform
import stat
import subprocess
import sys
from typing import Any, Optional

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

from scripts import native_shadow_boot_image_verify_arm64_v1 as image_verify
from scripts import native_shadow_boot_produce_phase_arm64_v1 as phase


BOOTABLE_CLAIM = False
ACTIVATION_ALLOWED = False
GUEST_BOOT_VERIFIED = False

REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parents[1]
LAUNCHER_BUILD_RESULT_PATH = (
    REPOSITORY_ROOT
    / "native/containment/native-shadow-launcher-build-result-arm64-v1.json"
)

SCHEMA = "boole.native-shadow.boot-root-disk-readback.arm64.v1"
RELEASE = "NATIVE-SHADOW-BOOT-ROOT-DISK-READBACK-ARM64-V1"
STATUS = "ROOT-DISK-CONTENTS-READ-BACK-NOT-BOOT-AUTHORITY"

# The checks are the verification stage's, named once, over there.
REQUIRED_CHECKS = tuple(image_verify.REQUIRED_CHECKS)

# The image is evidence being read, not a filesystem being used: read-only, and
# with no way for anything it contains to become a device or a program on the
# host that is reading it.
MOUNT_OPTIONS = ("ro", "nodev", "noexec", "nosuid", "loop")
FILESYSTEM = "ext4"


class RootDiskReadbackError(RuntimeError):
    """The produced root disk cannot be read, or is not what the seals require."""


def mount_argv(*, image: pathlib.Path, mountpoint: pathlib.Path) -> list[str]:
    """No `-w` and no defaults: every option the mount gets is spelled here."""

    return [
        "mount",
        "-t",
        FILESYSTEM,
        "-o",
        ",".join(MOUNT_OPTIONS),
        str(image),
        str(mountpoint),
    ]


def umount_argv(mountpoint: pathlib.Path) -> list[str]:
    return ["umount", str(mountpoint)]


def output_paths(outputs: pathlib.Path) -> dict[str, pathlib.Path]:
    """The produced files, named by the producer authority rather than here."""

    return phase.output_paths(outputs)


def sealed_launcher_sha256(
    path: pathlib.Path = LAUNCHER_BUILD_RESULT_PATH,
) -> str:
    """The digest the launcher build sealed, read rather than restated."""

    try:
        document = json.loads(path.read_text(encoding="utf-8"))
    except OSError as exc:
        raise RootDiskReadbackError(f"the sealed launcher result is unreadable: {path}") from exc
    digest = document.get("launcher", {}).get("sha256")
    if not isinstance(digest, str) or len(digest) != 64:
        raise RootDiskReadbackError(f"the sealed launcher result names no digest: {path}")
    return digest


def _digest(path: str) -> str:
    found = hashlib.sha256()
    with open(path, "rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            found.update(block)
    return found.hexdigest()


def tree_from_directory(root: pathlib.Path) -> dict[str, dict[str, Any]]:
    """Read a mounted tree into the shape the verification stage compares.

    Symlinks are recorded and never walked through.  One inside the image that
    pointed back at a parent, or out at the reading host, would otherwise be
    followed by the reader rather than described by it.
    """

    tree: dict[str, dict[str, Any]] = {}
    pending: list[tuple[str, str]] = [(str(root), "")]
    while pending:
        directory, prefix = pending.pop()
        with os.scandir(directory) as scan:
            rows = sorted(scan, key=lambda row: row.name)
        for row in rows:
            logical = prefix + "/" + row.name
            info = row.stat(follow_symlinks=False)
            bits = stat.S_IFMT(info.st_mode)
            entry: dict[str, Any] = {
                "gid": info.st_gid,
                "kind": "",
                "mode": stat.S_IMODE(info.st_mode),
                "uid": info.st_uid,
            }
            if bits == stat.S_IFDIR:
                entry["kind"] = "directory"
                pending.append((row.path, logical))
            elif bits == stat.S_IFLNK:
                entry["kind"] = "symlink"
                entry["target"] = os.readlink(row.path)
            elif bits == stat.S_IFREG:
                entry["kind"] = "file"
                entry["sha256"] = _digest(row.path)
            else:
                # The layer this image was built from holds only these three
                # kinds -- the initrd writer refuses anything else -- so a fourth
                # one here came from the writing, and is reported rather than
                # described in a shape that cannot hold it.
                raise RootDiskReadbackError(
                    f"{logical}: the image holds an entry that is not a file, "
                    "directory or symlink"
                )
            tree[logical] = entry
    return tree


def _unmet_requirements() -> list[str]:
    unmet = []
    if platform.system() != "Linux":
        unmet.append("Linux, whose driver reads this filesystem")
    if os.geteuid() != 0:
        unmet.append("root, without which the image cannot be mounted")
    return unmet


def _run(argv: list[str]) -> None:
    completed = subprocess.run(
        argv,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if completed.returncode != 0:
        raise RootDiskReadbackError(
            f"{argv[0]} failed: {completed.stderr.decode(errors='replace').strip()}"
        )


def verify(
    *,
    outputs: pathlib.Path,
    mountpoint: Optional[pathlib.Path] = None,
    result: Optional[pathlib.Path] = None,
) -> dict[str, Any]:
    """Mount the produced image, read it, and run the checks that already exist."""

    unmet = _unmet_requirements()
    if unmet:
        raise RootDiskReadbackError("reading the image back requires " + "; ".join(unmet))

    produced = output_paths(outputs)
    image = produced["root-disk"]
    if not image.is_file() or image.is_symlink():
        raise RootDiskReadbackError(f"the produced root disk is absent: {image}")

    where = outputs.parent / "root-disk-readback" if mountpoint is None else mountpoint
    where.mkdir(parents=True, exist_ok=True)

    _run(mount_argv(image=image, mountpoint=where))
    try:
        tree = tree_from_directory(where)
    finally:
        _run(umount_argv(where))

    lock = json.loads(phase.BOOT_SOURCE_LOCK_PATH.read_text(encoding="utf-8"))
    report = image_verify.verify_tree(
        tree=tree,
        expectations=image_verify.expectations_from_lock(lock),
        launcherSha256=sealed_launcher_sha256(),
        kernel=produced["kernel"].read_bytes(),
    )

    document = {
        "activationAllowed": ACTIVATION_ALLOWED,
        "bootableClaim": BOOTABLE_CLAIM,
        "boundaries": [
            "Reading the image back is not booting it; no virtual machine was started.",
            "This stage runs outside the producer authority's transient unit, which "
            "seals private devices and so cannot hold a loop mount.",
        ],
        "entryCount": len(tree),
        "guestBootVerified": GUEST_BOOT_VERIFIED,
        "image": {"name": image.name, "sha256": _digest(str(image))},
        "release": RELEASE,
        "schema": SCHEMA,
        "status": STATUS,
        "verification": report,
    }
    if result is not None:
        result.parent.mkdir(parents=True, exist_ok=True)
        result.write_bytes(image_verify.canonical_json(document))
    image_verify.assert_passed(report)
    return document


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    run = sub.add_parser("verify", help="mount the produced root disk and check it")
    run.add_argument("--outputs", type=pathlib.Path, required=True)
    run.add_argument("--mountpoint", type=pathlib.Path)
    run.add_argument("--result", type=pathlib.Path)
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    options = _parser().parse_args(argv)
    try:
        document = verify(
            outputs=options.outputs,
            mountpoint=options.mountpoint,
            result=options.result,
        )
    except (RootDiskReadbackError, image_verify.ImageVerifyError) as exc:
        print(f"native-shadow root disk read-back: FAIL: {exc}", file=sys.stderr)
        return 1
    print(f"native-shadow root disk read-back: PASS ({document['entryCount']} entries)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
