#!/usr/bin/env python3
"""Read every timestamp out of a produced root disk and refuse the wrong ones.

Two independently built copies of this image differed, and every field that
differed was a time.  The cause was that the ext4 writer had been handed zero,
which its library reads as "no fixed time was given" rather than as a time, so
each build silently fell back to the wall clock.

Byte identity between two replicas remains the acceptance criterion; this is the
guard that says *why* an image is identical.  Two builds a second apart could
agree on a wall clock and look deterministic while still carrying one.  So the
rule here is a closed set: the source epoch the staged inputs already carry, and
the one fixed value the writer is handed.  A closed set cannot quietly widen
later to admit whatever came out.

Reading, not repairing: this opens the image read-only and parses the superblock,
the group descriptors and the inode table.  It is not a filesystem checker -- the
frozen ``e2fsck -n`` does that job -- and it deliberately understands nothing
about directories, extents or extended attributes.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import struct
import sys
from typing import Any, BinaryIO, Optional, Sequence

from scripts.native_shadow_boot_root_disk_arm64_v1 import (
    ALLOWED_TIMESTAMPS,
    WALL_CLOCK_LOWER_BOUND,
)

SCHEMA = "boole.native-shadow.boot-root-disk-time-audit.arm64.v1"

SUPERBLOCK_OFFSET = 1024
SUPERBLOCK_MAGIC = 0xEF53
INCOMPAT_64BIT = 0x80
MINIMUM_DESCRIPTOR_SIZE = 32
BASE_INODE_SIZE = 128

SUPERBLOCK_TIME_FIELDS = (
    ("s_mtime", 0x2C),
    ("s_wtime", 0x30),
    ("s_lastcheck", 0x40),
    ("s_mkfs_time", 0x108),
)

INODE_TIME_FIELDS = (
    ("i_atime", 0x08),
    ("i_ctime", 0x0C),
    ("i_mtime", 0x10),
    ("i_dtime", 0x14),
)

CRTIME_OFFSET = 0x90
CRTIME_END = CRTIME_OFFSET + 4
EXTRA_ISIZE_OFFSET = 0x80


class TimeAuditError(RuntimeError):
    """The image is not shaped like the ext4 filesystem this was built to read."""


class _Bytes:
    """A reader over an image already held in memory."""

    def __init__(self, raw: bytes) -> None:
        self._raw = raw

    def read(self, offset: int, length: int) -> bytes:
        chunk = self._raw[offset : offset + length]
        if len(chunk) != length:
            raise TimeAuditError(f"the image ends before offset {offset + length}")
        return chunk


class _File:
    """A reader over an image on disk, so a gigabyte need not be resident."""

    def __init__(self, handle: BinaryIO) -> None:
        self._handle = handle

    def read(self, offset: int, length: int) -> bytes:
        self._handle.seek(offset)
        chunk = self._handle.read(length)
        if len(chunk) != length:
            raise TimeAuditError(f"the image ends before offset {offset + length}")
        return chunk


def _u16(raw: bytes, offset: int) -> int:
    return struct.unpack_from("<H", raw, offset)[0]


def _u32(raw: bytes, offset: int) -> int:
    return struct.unpack_from("<I", raw, offset)[0]


def geometry(reader: Any) -> dict[str, Any]:
    """The handful of superblock fields needed to find the inode tables."""

    raw = reader.read(SUPERBLOCK_OFFSET, 1024)
    magic = _u16(raw, 0x38)
    if magic != SUPERBLOCK_MAGIC:
        raise TimeAuditError(f"superblock magic is {magic:#x}, not {SUPERBLOCK_MAGIC:#x}")
    inode_size = _u16(raw, 0x58)
    if inode_size < BASE_INODE_SIZE:
        raise TimeAuditError(f"inode size {inode_size} is below the base inode")
    inodes_per_group = _u32(raw, 0x28)
    if inodes_per_group == 0:
        raise TimeAuditError("the superblock says zero inodes per group")
    incompat = _u32(raw, 0x60)
    descriptor_size = _u16(raw, 0xFE)
    if not incompat & INCOMPAT_64BIT or descriptor_size < MINIMUM_DESCRIPTOR_SIZE:
        descriptor_size = MINIMUM_DESCRIPTOR_SIZE
    inodes_count = _u32(raw, 0x00)
    return {
        "blockSize": 1024 << _u32(raw, 0x18),
        "descriptorSize": descriptor_size,
        "firstDataBlock": _u32(raw, 0x14),
        "groupCount": -(-inodes_count // inodes_per_group),
        "inodeSize": inode_size,
        "inodesCount": inodes_count,
        "inodesPerGroup": inodes_per_group,
        "sixtyFourBit": bool(incompat & INCOMPAT_64BIT),
        "superblockTimes": {name: _u32(raw, offset) for name, offset in SUPERBLOCK_TIME_FIELDS},
    }


def _inode_table_offset(reader: Any, shape: dict[str, Any], group: int) -> int:
    table = (shape["firstDataBlock"] + 1) * shape["blockSize"]
    raw = reader.read(table + group * shape["descriptorSize"], shape["descriptorSize"])
    block = _u32(raw, 0x08)
    if shape["sixtyFourBit"] and shape["descriptorSize"] >= 0x2C:
        block |= _u32(raw, 0x28) << 32
    return block * shape["blockSize"]


def _violation(field: str, value: int, where: str) -> dict[str, Any]:
    return {
        "field": field,
        "looksLikeAWallClock": value >= WALL_CLOCK_LOWER_BOUND,
        "value": value,
        "where": where,
    }


def audit(image: Any) -> dict[str, Any]:
    """Every timestamp in the image, checked against the closed set.

    ``image`` is either the bytes or a path.  A path is read in pieces, which is
    what the produce phase wants for a gigabyte-sized disk.
    """

    if isinstance(image, (bytes, bytearray)):
        return _audit(_Bytes(bytes(image)))
    with open(str(image), "rb") as handle:
        return _audit(_File(handle))


def _audit(reader: Any) -> dict[str, Any]:
    shape = geometry(reader)
    allowed = set(ALLOWED_TIMESTAMPS)
    violations: list[dict[str, Any]] = []

    for name, value in sorted(shape["superblockTimes"].items()):
        if value not in allowed:
            violations.append(_violation(name, value, "superblock"))

    inode_size = shape["inodeSize"]
    read_count = 0
    remaining = shape["inodesCount"]
    for group in range(shape["groupCount"]):
        count = min(shape["inodesPerGroup"], remaining)
        remaining -= count
        offset = _inode_table_offset(reader, shape, group)
        table = reader.read(offset, inode_size * count)
        for index in range(count):
            base = index * inode_size
            number = group * shape["inodesPerGroup"] + index + 1
            read_count += 1
            for name, field in INODE_TIME_FIELDS:
                value = _u32(table, base + field)
                if value not in allowed:
                    violations.append(_violation(name, value, f"inode {number}"))
            if inode_size < CRTIME_END:
                continue
            extra = _u16(table, base + EXTRA_ISIZE_OFFSET)
            if BASE_INODE_SIZE + extra < CRTIME_END:
                continue
            value = _u32(table, base + CRTIME_OFFSET)
            if value not in allowed:
                violations.append(_violation("i_crtime", value, f"inode {number}"))

    return {
        "allowedTimestamps": sorted(allowed),
        "geometry": {key: value for key, value in shape.items() if key != "superblockTimes"},
        "inodesRead": read_count,
        "passed": not violations,
        "schema": SCHEMA,
        "superblockTimes": shape["superblockTimes"],
        "violationCount": len(violations),
        "violations": violations[:64],
        "wallClockLowerBound": WALL_CLOCK_LOWER_BOUND,
    }


def assert_no_wall_clock(image: Any) -> dict[str, Any]:
    """Audit and raise, for callers that must not continue on a violation."""

    report = audit(image)
    if not report["passed"]:
        first = report["violations"][0]
        raise TimeAuditError(
            f"{report['violationCount']} timestamps are outside "
            f"{report['allowedTimestamps']}: {first['field']} in {first['where']} "
            f"is {first['value']}"
        )
    return report


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("image", help="the produced root disk to read")
    return parser


def main(argv: Optional[Sequence[str]] = None) -> int:
    arguments = _parser().parse_args(argv)
    report = audit(pathlib.Path(arguments.image))
    json.dump(report, sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write("\n")
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
