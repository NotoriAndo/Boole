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

The writer is not the guest's own `mke2fs`.  The guest's copy overwrites each
staged file's `i_ctime` from the staging file's `st_ctime`, which no caller can
set, so the two jobs split on a field neither of them chose -- the failure
sealed in the root-disk determinism hard-stop record.  The writer here is a
selected official build that clamps that field to a fixed time, added as a
production tool only: the 191 packages the guest is built from do not move, and
the inspector and the read-only checker stay on the frozen build so the image is
judged by tools that did not write it.

Which variable carries that fixed time is not interchangeable.  `SOURCE_DATE_EPOCH`
sets it *and* arms the flag the writer branches on; `E2FSPROGS_FAKE_TIME` sets it
and leaves the flag clear, which puts the writer back on the branch that copies
the staged time.  Setting the wrong one of the two would look correct and
reproduce the sealed failure with a newer binary.

`mke2fs -d` walks the staging tree with `opendir`/`readdir` and never sorts it:
`scandir`, `alphasort` and `versionsort` are all absent from the binary.  On
ext4 a directory large enough to become an htree is returned in filename-hash
order, and that hash is seeded per filesystem, so the two jobs would disagree.
Staging on tmpfs -- whose readdir order is creation order -- and creating
entries in logical path byte order removes the dependency.  That is an
assumption about the runner's kernel, so it is listed as one, and the byte
comparison is what settles it.

There are two library closures rather than one, and keeping them apart is the
cost of changing the writer.  `debugfs` and `e2fsck` need eight `DT_NEEDED`
sonames between them, all shipped by the 191 frozen packages -- `libe2p.so.2`
among them, which comes from `libext2fs2t64` rather than from a package of its
own.  The writer needs seven, and two of those must be the build sealed with it,
because a new `mke2fs` resolved against the old `libext2fs` would find the flag
unarmed and write staged times again with nothing in the output saying so.  What
is *not* settled here is which copy the loader picks at run time, so the plan
lists the providers for each closure and leaves the resolution to be recorded,
and compared, at build time.
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
# The variable the selected writer honours.  Its predecessor,
# `E2FSPROGS_FAKE_TIME`, is still read by this family of tools and is still not
# enough: it sets the fixed time without arming the flag the writer branches on,
# so a plan that set it would look correct and produce the sealed failure.
WRITER_TIME_ENV = "SOURCE_DATE_EPOCH"
SUPERSEDED_WRITER_TIME_ENV = "E2FSPROGS_FAKE_TIME"
STAGING_FILESYSTEM = "tmpfs"
RESERVED_BLOCK_PERCENT = 0
VOLUME_LABEL = ""
BOOTABLE_CLAIM = False
ACTIVATION_ALLOWED = False

# The time handed to the ext4 writer, which is not the time the staged inputs
# carry.  `CANONICAL_MTIME` is zero and stays zero: it is what every staged file
# is stamped with, and the image reproduces it faithfully.  But zero is also the
# library's "no fixed time was given" sentinel -- `ext2fs_initialize` stores the
# parsed variable at `fs->now` and every writer of a time field tests `cbz`
# against it before falling back to `time()`.  Handing it zero therefore asked
# for the wall clock, which is how two builds of identical inputs came out
# different.  One is the smallest value the sentinel does not swallow.
EXT4_WRITER_TIME = "1"
ALLOWED_TIMESTAMPS = (CANONICAL_MTIME, int(EXT4_WRITER_TIME))

# Any surviving wall clock is far above both allowed values; this only exists so
# a violation report can say which kind of wrong value it found.
WALL_CLOCK_LOWER_BOUND = 1000000

# The writer is no longer the guest's own mke2fs.  The guest's copy overwrites
# each staged file's `i_ctime` from the staging file's `st_ctime`, which no
# caller can set, so it cannot produce the same image twice.  This one clamps
# that field to the fixed time.  It is an official build, selected by a static
# read of the shipped binaries recorded in
# `native-shadow-boot-e2fsprogs-selection-plucky-arm64-v1.json`, and it is added
# as a production tool: the 191 packages the guest is built from do not move,
# and the inspector and the read-only checker below stay on the frozen build so
# that the image is judged by tools that did not write it.
MKE2FS_SHA256 = "d20ac1862b68136bd1ffc03b19d3be24733d714345c2e1531178d13d3ddb5945"
MKE2FS_SIZE_BYTES = 199208
WRITER_PACKAGE_SHA256 = (
    "89c94171d47851896b9c0bf600dd753b5b8770a4550b38304cd873fa7c8aabea"
)
# Sealed with the writer because the package itself says so: its `Pre-Depends`
# names one exact-version dependency, `libext2fs2t64 (= 1.47.2-1ubuntu1)`, and
# five floors that the frozen guest already clears.  Which auxiliary library has
# to match the writer exactly is therefore not this plan's judgement.
WRITER_LIBRARY_PACKAGE_SHA256 = (
    "da4d465823f2653b35bd316f9c479e4a531165e01840151184f015f6e0d391a5"
)
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
        "LC_ALL": "C",
        "MKE2FS_CONFIG": config,
        "TZ": "UTC",
        WRITER_TIME_ENV: EXT4_WRITER_TIME,
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
            "this build's libext2fs reads SOURCE_DATE_EPOCH, stores it as the fixed "
            "time and arms the flag mke2fs branches on to clamp i_ctime; "
            "E2FSPROGS_FAKE_TIME is read too but sets the time without arming the "
            "flag, which leaves the writer on the branch that copies the staged "
            "st_ctime. Read from the shipped binaries, not run. This supersedes the "
            "assumption made about the previous writer, whose falsification is sealed "
            "in the root-disk determinism hard-stop record."
        ),
        "id": "writer-honours-source-date-epoch",
        "onMismatch": "abort-never-relax",
        "settledBy": "the inode and superblock times in the two independent builds",
        "supersedes": "fake-time-honoured-by-this-build",
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
            "every DT_NEEDED soname is shipped by a pinned package, but which copy the "
            "loader picks is a run-time fact this plan cannot settle. There are now two "
            "closures rather than one -- the writer's and the checkers' -- and they "
            "hold different builds of libext2fs, so a loader handed the wrong one would "
            "run the new writer against the old library and reinstate the defect "
            "silently."
        ),
        "id": "loader-resolves-only-frozen-libraries",
        "onMismatch": "abort-never-relax",
        "settledBy": (
            "the resolved library paths and digests recorded at build time for each "
            "closure separately"
        ),
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

# Where each library in a closure came from.  The distinction is the whole point
# of describing two closures: a writer-set library is pinned here by digest
# because it is the reason the writer was changed, while a frozen-guest library
# is whatever the frozen tree holds and must be the same bytes in both closures.
ORIGIN_WRITER_SET = "writer-set"
ORIGIN_FROZEN_GUEST = "frozen-guest"

# Read from the writer's ELF header.  `libss.so.2` is absent on purpose: it is
# debugfs's line editor and the writer does not link it, which is why the writer
# closure is seven libraries where the checkers' is eight.
WRITER_NEEDED = (
    "ld-linux-aarch64.so.1",
    "libblkid.so.1",
    "libc.so.6",
    "libcom_err.so.2",
    "libe2p.so.2",
    "libext2fs.so.2",
    "libuuid.so.1",
)

WRITER_LIBRARIES = [
    {"logicalPath": "/usr/lib/aarch64-linux-gnu/ld-linux-aarch64.so.1", "origin": ORIGIN_FROZEN_GUEST, "package": "libc6", "soname": "ld-linux-aarch64.so.1"},
    {"logicalPath": "/usr/lib/aarch64-linux-gnu/libblkid.so.1", "origin": ORIGIN_FROZEN_GUEST, "package": "libblkid1", "soname": "libblkid.so.1"},
    {"logicalPath": "/usr/lib/aarch64-linux-gnu/libc.so.6", "origin": ORIGIN_FROZEN_GUEST, "package": "libc6", "soname": "libc.so.6"},
    {"logicalPath": "/usr/lib/aarch64-linux-gnu/libcom_err.so.2", "origin": ORIGIN_FROZEN_GUEST, "package": "libcom-err2", "soname": "libcom_err.so.2"},
    {
        "logicalPath": "/usr/lib/aarch64-linux-gnu/libe2p.so.2",
        "origin": ORIGIN_WRITER_SET,
        "package": "libext2fs2t64",
        "sha256": "85ea4f7b70d499b204c030e45e91d95b814ab8dd26db6cd788a62831d92222c6",
        "sizeBytes": 69584,
        "soname": "libe2p.so.2",
    },
    {
        "logicalPath": "/usr/lib/aarch64-linux-gnu/libext2fs.so.2",
        "origin": ORIGIN_WRITER_SET,
        "package": "libext2fs2t64",
        "sha256": "6f0fe581693a63d0dcf075874f85448670bda656a7f79dcf040ed3bb88ea6fa0",
        "sizeBytes": 463016,
        "soname": "libext2fs.so.2",
    },
    {"logicalPath": "/usr/lib/aarch64-linux-gnu/libuuid.so.1", "origin": ORIGIN_FROZEN_GUEST, "package": "libuuid1", "soname": "libuuid.so.1"},
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
                "packageSha256": WRITER_PACKAGE_SHA256,
                "path": mke2fs,
                "role": "ext4-image-writer",
                "sha256": MKE2FS_SHA256,
                "sizeBytes": MKE2FS_SIZE_BYTES,
            },
        },
        "unverifiedAssumptions": UNVERIFIED_ASSUMPTIONS,
        "volumeLabel": VOLUME_LABEL,
        "writerLibraries": WRITER_LIBRARIES,
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
