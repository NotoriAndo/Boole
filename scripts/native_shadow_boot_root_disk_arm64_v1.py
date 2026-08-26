#!/usr/bin/env python3
"""Plan the ext4 root disk the arm64 CI producer will write, without writing it.

The 2026-08-26j builder authority froze the root disk's shape -- one ext4 image,
a fixed UUID, a fixed directory hash seed, `canonicalMtime: 0`, root-owned
throughout, and two independent builds whose bytes must match.  This module
turns a frozen rootfs layer into the exact argv, environment and staging order
that produce that image.  It runs nothing: `mke2fs` is an aarch64 ELF and the
host that plans is not the host that builds, so planning and executing are kept
apart on purpose.

Three findings from reading the frozen tools shaped this, and each is recorded
in the plan rather than assumed away.

`SOURCE_DATE_EPOCH` does nothing here.  The string is absent from this build of
`mke2fs`; what the shipped `libext2fs.so.2.4` actually reads is
`E2FSPROGS_FAKE_TIME`.  Setting the wrong variable would have left the
superblock's mkfs time at wall clock and split the two jobs on a field neither
of them chose.

`mke2fs -d` walks the staging tree with `opendir`/`readdir` and never sorts it:
`scandir`, `alphasort` and `versionsort` are all absent from the binary.  On
ext4 a directory large enough to become an htree is returned in filename-hash
order, and that hash is seeded per filesystem, so the two jobs would disagree.
Staging on tmpfs -- whose readdir order is creation order -- and creating
entries in logical path byte order removes the dependency.  That is an
assumption about the runner's kernel, so it is listed as one, and the byte
comparison is what settles it.

Every shared library the two tools need is already inside the frozen package
set.  `mke2fs` and `debugfs` name eight `DT_NEEDED` sonames between them, and
each one is shipped by one of the 191 packages -- `libe2p.so.2` among them,
which comes from `libext2fs2t64` rather than from a package of its own.  What is
*not* settled is which copy the loader picks at run time, so the plan lists the
providers and leaves the resolution to be recorded at build time.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import sys
from typing import Any, Optional

from scripts.native_shadow_boot_initrd_arm64_v1 import (
    CANONICAL_MTIME,
    InitrdBuildError,
    layer_entries,
)


BLOCK_SIZE = 4096
INODE_SIZE = 256
EXT4_UUID = "00000000-0000-4000-8000-000000000001"
EXT4_HASH_SEED = "00000000-0000-4000-8000-000000000002"
FAKE_TIME_ENV = "E2FSPROGS_FAKE_TIME"
STAGING_FILESYSTEM = "tmpfs"
RESERVED_BLOCK_PERCENT = 0
VOLUME_LABEL = ""
BOOTABLE_CLAIM = False
ACTIVATION_ALLOWED = False

# The time handed to the ext4 writer, which is not the time the staged inputs
# carry.  `CANONICAL_MTIME` is zero and stays zero: it is what every staged file
# is stamped with, and the image reproduces it faithfully.  But zero is also the
# frozen library's "no fixed time was given" sentinel -- `ext2fs_initialize`
# stores the parsed variable at `fs->now` and every writer of a time field tests
# `cbz` against it before falling back to `time()`.  Handing it zero therefore
# asked for the wall clock, which is how two builds of identical inputs came out
# different.  One is the smallest value the sentinel does not swallow.
EXT4_WRITER_TIME = "1"
ALLOWED_TIMESTAMPS = (CANONICAL_MTIME, int(EXT4_WRITER_TIME))

# Any surviving wall clock is far above both allowed values; this only exists so
# a violation report can say which kind of wrong value it found.
WALL_CLOCK_LOWER_BOUND = 1000000

MKE2FS_SHA256 = "763be3ec03774647799b1186d30b4b524e6e73dd27be01cbe0be4b6043f62cb1"
MKE2FS_SIZE_BYTES = 133512
DEBUGFS_SHA256 = "2c0bf348d91f9b3bd6eec6666b9897b9f733c430e6baa8066bd70b645b2ca023"
DEBUGFS_SIZE_BYTES = 271944
E2FSCK_SHA256 = "05b3292174fdaadf96324ad349c006b1881b20647826bd869162e5ad8d34723b"
E2FSCK_SIZE_BYTES = 413720
# The checker ships in the same e2fsprogs package as the writer and the
# inspector, so it is bound to the frozen closure by a path inside it rather
# than by a new acquisition.
E2FSCK_MEMBER_PATH = "./usr/sbin/e2fsck"
E2FSPROGS_PACKAGE_SHA256 = (
    "6e1cdd65bf58fe77968f8ac45f1802586baf18bfb8541f4a88fe843ab85bef8b"
)

# `-f` forces the check.  Without it e2fsck reads a superblock that mke2fs just
# marked clean and exits zero without looking at the filesystem, which is close
# enough to not running that recording it as a pass would be dishonest.  It is a
# force flag, not a repair flag; the repair flags are `-p`, `-y` and `-a`, and
# `-n` answers no to every question the checker could ask.
E2FSCK_ARGV_OPTIONS = ("-f", "-n")
E2FSCK_FORBIDDEN_OPTIONS = ("-a", "-p", "-w", "-y")
E2FSCK_ACCEPTED_EXIT_CODES = (0,)

# Spare inodes above the entry count.  Pinning `-N` keeps the inode table a
# function of the layer instead of a function of the config's `inode_ratio`.
INODE_MARGIN = 4096
INODE_ALIGNMENT = 1024

# The size check is a floor, not a model of ext4.  It counts the blocks the
# content needs, the inode table, a journal, and a flat allowance for group
# metadata -- enough to reject a size that cannot possibly hold the tree, and
# never enough to be mistaken for a size recommendation.
METADATA_MARGIN_RATIO = 0.10
JOURNAL_MIN_BLOCKS = 1024
JOURNAL_MAX_BLOCKS = 32768
JOURNAL_BLOCKS_PER_FS_BLOCK = 64
FAST_SYMLINK_MAX_BYTES = 60


class RootDiskPlanError(RuntimeError):
    """The layer and the pinned size cannot produce the frozen root disk."""


def canonical_json(value: Any) -> bytes:
    return (
        json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2, allow_nan=False).encode(
            "utf-8"
        )
        + b"\n"
    )


def _align_up(value: int, alignment: int) -> int:
    remainder = value % alignment
    return value if remainder == 0 else value + alignment - remainder


def inode_count(entries: list[dict[str, Any]]) -> int:
    return _align_up(len(entries) + INODE_MARGIN, INODE_ALIGNMENT)


def _journal_blocks(total_blocks: int) -> int:
    return min(max(total_blocks // JOURNAL_BLOCKS_PER_FS_BLOCK, JOURNAL_MIN_BLOCKS), JOURNAL_MAX_BLOCKS)


def required_bytes(entries: list[dict[str, Any]]) -> int:
    """A floor on the image size: below this the tree provably does not fit."""

    content = 0
    for entry in entries:
        if entry["kind"] == "file":
            content += _align_up(len(entry["data"]), BLOCK_SIZE)
        elif entry["kind"] == "directory":
            content += BLOCK_SIZE
        elif len(entry["data"]) > FAST_SYMLINK_MAX_BYTES:
            content += BLOCK_SIZE
    inodes = inode_count(entries) * INODE_SIZE
    journal = _journal_blocks(content // BLOCK_SIZE + 1) * BLOCK_SIZE
    total = content + inodes + journal
    return _align_up(int(total * (1.0 + METADATA_MARGIN_RATIO)), BLOCK_SIZE)


def staging_entries(entries: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """The order and metadata the staging tree must be created with."""

    rows: list[dict[str, Any]] = []
    for entry in entries:
        row: dict[str, Any] = {
            "gid": 0,
            "kind": entry["kind"],
            "mode": entry["mode"] & 0o7777,
            "mtime": CANONICAL_MTIME,
            "path": entry["name"],
            "uid": 0,
        }
        if entry["kind"] == "symlink":
            row["target"] = entry["data"].decode("utf-8")
        else:
            row["sizeBytes"] = len(entry["data"])
        rows.append(row)
    return rows


def mke2fs_argv(*, mke2fs: str, image: str, staging: str, blocks: int, inodes: int) -> list[str]:
    return [
        mke2fs,
        "-t",
        "ext4",
        "-b",
        str(BLOCK_SIZE),
        "-I",
        str(INODE_SIZE),
        "-N",
        str(inodes),
        "-m",
        str(RESERVED_BLOCK_PERCENT),
        "-U",
        EXT4_UUID,
        "-E",
        f"hash_seed={EXT4_HASH_SEED},root_owner=0:0",
        "-d",
        staging,
        "-F",
        "-q",
        image,
        str(blocks),
    ]


def mke2fs_env(*, config: str) -> dict[str, str]:
    return {
        FAKE_TIME_ENV: EXT4_WRITER_TIME,
        "LC_ALL": "C",
        "MKE2FS_CONFIG": config,
        "TZ": "UTC",
    }


def e2fsck_argv(*, e2fsck: str, image: str) -> list[str]:
    """Read the produced filesystem back and answer no to every question."""

    return [e2fsck, *E2FSCK_ARGV_OPTIONS, image]


def e2fsck_env() -> dict[str, str]:
    """No fake time here: the checker is a reader and writes no time field."""

    return {"LC_ALL": "C", "TZ": "UTC"}


UNVERIFIED_ASSUMPTIONS = [
    {
        "detail": (
            "this build of mke2fs has no SOURCE_DATE_EPOCH string; the shipped "
            "libext2fs.so.2.4 has E2FSPROGS_FAKE_TIME. Read from the binaries, not run. "
            "The first pair of builds falsified the value this was set to rather than "
            "the variable: zero is the library's unset sentinel, so it was honoured by "
            "being ignored. A non-zero fixed time replaces it."
        ),
        "id": "fake-time-honoured-by-this-build",
        "onMismatch": "abort-never-relax",
        "settledBy": "the superblock times in the two independent builds",
    },
    {
        "detail": (
            "mke2fs walks the staging tree with readdir and never sorts it, so the "
            "image depends on the staging filesystem returning creation order."
        ),
        "id": "staging-readdir-order-is-creation-order",
        "onMismatch": "abort-never-relax",
        "settledBy": "the byte comparison between the two independent builds",
    },
    {
        "detail": (
            "every DT_NEEDED soname is shipped by a frozen package, but which copy the "
            "loader picks is a run-time fact this plan cannot settle."
        ),
        "id": "loader-resolves-only-frozen-libraries",
        "onMismatch": "abort-never-relax",
        "settledBy": "the resolved library paths recorded at build time",
    },
]

# Read from the two ELF headers, then matched against the frozen package set.
# `libe2p.so.2` has no package of its own in this release; `libext2fs2t64`
# carries it, which is why looking for a `libe2p2t64` finds nothing.
SHARED_LIBRARIES = [
    {"logicalPath": "/usr/lib/aarch64-linux-gnu/ld-linux-aarch64.so.1", "package": "libc6", "soname": "ld-linux-aarch64.so.1"},
    {"logicalPath": "/usr/lib/aarch64-linux-gnu/libblkid.so.1", "package": "libblkid1", "soname": "libblkid.so.1"},
    {"logicalPath": "/usr/lib/aarch64-linux-gnu/libc.so.6", "package": "libc6", "soname": "libc.so.6"},
    {"logicalPath": "/usr/lib/aarch64-linux-gnu/libcom_err.so.2", "package": "libcom-err2", "soname": "libcom_err.so.2"},
    {"logicalPath": "/usr/lib/aarch64-linux-gnu/libe2p.so.2", "package": "libext2fs2t64", "soname": "libe2p.so.2"},
    {"logicalPath": "/usr/lib/aarch64-linux-gnu/libext2fs.so.2", "package": "libext2fs2t64", "soname": "libext2fs.so.2"},
    {"logicalPath": "/usr/lib/aarch64-linux-gnu/libss.so.2", "package": "libss2", "soname": "libss.so.2"},
    {"logicalPath": "/usr/lib/aarch64-linux-gnu/libuuid.so.1", "package": "libuuid1", "soname": "libuuid.so.1"},
]


def root_disk_plan(
    *,
    layer: bytes,
    mke2fs: str,
    debugfs: str,
    e2fsck: str,
    config: str,
    image: str,
    staging: str,
    sizeBytes: int,
) -> dict[str, Any]:
    """Everything the producer needs to write the image, and nothing it runs."""

    if sizeBytes % BLOCK_SIZE != 0:
        raise RootDiskPlanError(
            f"pinned size {sizeBytes} is not a whole number of {BLOCK_SIZE} byte blocks"
        )
    entries = layer_entries(layer)
    floor = required_bytes(entries)
    if sizeBytes < floor:
        raise RootDiskPlanError(
            f"pinned size {sizeBytes} is below the {floor} byte floor for this layer"
        )
    inodes = inode_count(entries)
    return {
        "activationAllowed": ACTIVATION_ALLOWED,
        "bootableClaim": BOOTABLE_CLAIM,
        "executed": False,
        "mke2fs": {
            "argv": mke2fs_argv(
                mke2fs=mke2fs,
                image=image,
                staging=staging,
                blocks=sizeBytes // BLOCK_SIZE,
                inodes=inodes,
            ),
            "env": mke2fs_env(config=config),
        },
        "e2fsck": {
            "acceptedExitCodes": list(E2FSCK_ACCEPTED_EXIT_CODES),
            "argv": e2fsck_argv(e2fsck=e2fsck, image=image),
            "env": e2fsck_env(),
            "forbiddenOptions": list(E2FSCK_FORBIDDEN_OPTIONS),
            "notRunIsNotAPass": True,
            "runs": 1,
        },
        "sharedLibraries": SHARED_LIBRARIES,
        "sizeBytes": {"pinned": sizeBytes, "required": floor},
        "staging": {
            "entries": staging_entries(entries),
            "filesystem": STAGING_FILESYSTEM,
            "path": staging,
        },
        "tools": {
            "debugfs": {
                "path": debugfs,
                "role": "ext4-image-inspector",
                "sha256": DEBUGFS_SHA256,
                "sizeBytes": DEBUGFS_SIZE_BYTES,
            },
            "e2fsck": {
                "packageSha256": E2FSPROGS_PACKAGE_SHA256,
                "path": e2fsck,
                "role": "ext4-image-read-only-checker",
                "sha256": E2FSCK_SHA256,
                "sizeBytes": E2FSCK_SIZE_BYTES,
            },
            "mke2fs": {
                "packageSha256": E2FSPROGS_PACKAGE_SHA256,
                "path": mke2fs,
                "role": "ext4-image-writer",
                "sha256": MKE2FS_SHA256,
                "sizeBytes": MKE2FS_SIZE_BYTES,
            },
        },
        "unverifiedAssumptions": UNVERIFIED_ASSUMPTIONS,
        "volumeLabel": VOLUME_LABEL,
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    make = sub.add_parser("plan", help="write the root disk plan for a layer")
    make.add_argument("--layer", type=pathlib.Path, required=True)
    make.add_argument("--mke2fs", required=True)
    make.add_argument("--debugfs", required=True)
    make.add_argument("--e2fsck", required=True)
    make.add_argument("--config", required=True)
    make.add_argument("--image", required=True)
    make.add_argument("--staging", required=True)
    make.add_argument("--size-bytes", type=int, required=True)
    make.add_argument("--output", type=pathlib.Path, required=True)
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    args = _parser().parse_args(argv)
    try:
        plan = root_disk_plan(
            layer=args.layer.read_bytes(),
            mke2fs=args.mke2fs,
            debugfs=args.debugfs,
            e2fsck=args.e2fsck,
            config=args.config,
            image=args.image,
            staging=args.staging,
            sizeBytes=args.size_bytes,
        )
    except (RootDiskPlanError, InitrdBuildError, OSError) as exc:
        print(f"root-disk: {exc}", file=sys.stderr)
        return 1
    args.output.write_bytes(canonical_json(plan))
    print(f"root-disk plan {args.output} entries={len(plan['staging']['entries'])}")
    print(f"nothing was executed  bootableClaim: {str(BOOTABLE_CLAIM).lower()}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
