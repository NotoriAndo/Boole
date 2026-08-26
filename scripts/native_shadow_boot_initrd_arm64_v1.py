#!/usr/bin/env python3
"""Turn the frozen OCI rootfs layer into a deterministic guest initrd.

The 2026-08-26j image builder authority froze the initrd's shape: `cpio` in the
`newc` format, `initrdCompression: "none"`, `fileOrder:
"sorted-by-logical-path-bytes"`, `canonicalMtime: 0`, `ownership:
"root:root-only"`, and two independent builds whose bytes must match.  This
module is the writer for that format and nothing else -- it does not fetch, it
does not resolve, and it does not boot.

Two decisions are worth stating.

First, the layer's ordering and ownership are re-checked here rather than
assumed.  The frozen OCI builder already sorts by path bytes and already forces
uid/gid to 0, so these checks should never fire.  That is exactly why they are
cheap to keep: an invariant that is only ever asserted somewhere else is an
invariant that quietly stops holding the day someone feeds this writer a
different tar.

Second, inode numbers are positions, not host facts.  `newc` has an inode field
and the obvious way to fill it -- ask the filesystem -- would differ between the
two independent CI jobs and fail the byte comparison for a reason that has
nothing to do with the image.  Numbering from 1 in archive order makes the field
a function of the layer alone.

Compression stays off.  A gzip member carries its own mtime, so compressing here
would reintroduce the timestamp the canonical mtime exists to remove, and
`forbidTimestampSuppression` rules out papering over it after the fact.
"""

from __future__ import annotations

import argparse
import hashlib
import io
import pathlib
import stat
import sys
import tarfile
from typing import Any, Optional


MAGIC = b"070701"
HEADER_FIELDS = 13
HEADER_BYTES = len(MAGIC) + HEADER_FIELDS * 8
TRAILER_NAME = "TRAILER!!!"
COMPRESSION = "none"
CANONICAL_MTIME = 0
BOOTABLE_CLAIM = False
ACTIVATION_ALLOWED = False

# `newc` pads both the name and the payload up to a four byte boundary.  A
# writer that forgets either one produces an archive the kernel misparses
# rather than rejects, so the padding is part of the format, not a nicety.
ALIGNMENT = 4

DIRECTORY_NLINK = 2
DEFAULT_NLINK = 1


class InitrdBuildError(RuntimeError):
    """The layer cannot be turned into a deterministic initrd."""


def _field(value: int, name: str) -> bytes:
    if value < 0 or value > 0xFFFFFFFF:
        raise InitrdBuildError(f"{name} does not fit a newc field: {value}")
    return b"%08x" % value


def _pad(raw: bytes) -> bytes:
    remainder = len(raw) % ALIGNMENT
    return raw if remainder == 0 else raw + b"\0" * (ALIGNMENT - remainder)


def _record(
    *,
    ino: int,
    mode: int,
    nlink: int,
    name: str,
    data: bytes,
) -> bytes:
    encoded = name.encode("utf-8") + b"\0"
    header = MAGIC + b"".join(
        (
            _field(ino, "ino"),
            _field(mode, "mode"),
            _field(0, "uid"),
            _field(0, "gid"),
            _field(nlink, "nlink"),
            _field(CANONICAL_MTIME, "mtime"),
            _field(len(data), "filesize"),
            _field(0, "devmajor"),
            _field(0, "devminor"),
            _field(0, "rdevmajor"),
            _field(0, "rdevminor"),
            _field(len(encoded), "namesize"),
            _field(0, "check"),
        )
    )
    return _pad(header + encoded) + _pad(data)


def _check_path(name: str) -> str:
    if not name:
        raise InitrdBuildError("layer entry has an empty path")
    if name.startswith("/"):
        raise InitrdBuildError(f"layer entry path is absolute: {name}")
    parts = name.split("/")
    if "" in parts or "." in parts or ".." in parts:
        raise InitrdBuildError(f"layer entry path is not normalized: {name}")
    return name


def _kind_and_mode(member: tarfile.TarInfo) -> tuple[str, int]:
    if member.isdir():
        return "directory", stat.S_IFDIR
    if member.issym():
        return "symlink", stat.S_IFLNK
    if member.isreg():
        return "file", stat.S_IFREG
    if member.islnk():
        # Assembly materializes hardlinks into regular files before the layer is
        # written, so one surviving to here means the input is not that layer.
        raise InitrdBuildError(f"layer entry is a hardlink: {member.name}")
    raise InitrdBuildError(f"layer entry is not a file, directory or symlink: {member.name}")


def layer_entries(layer: bytes) -> list[dict[str, Any]]:
    """Read the frozen layer tar, checking the invariants it claims to hold."""

    entries: list[dict[str, Any]] = []
    with tarfile.open(fileobj=io.BytesIO(layer), mode="r:") as archive:
        for member in archive:
            name = _check_path(member.name)
            if member.mtime != CANONICAL_MTIME:
                raise InitrdBuildError(f"layer entry mtime is not canonical: {name}")
            if member.uid != 0 or member.gid != 0:
                raise InitrdBuildError(f"layer entry is not owned by root: {name}")
            kind, type_bits = _kind_and_mode(member)
            if kind == "symlink":
                data = member.linkname.encode("utf-8")
            elif kind == "file":
                handle = archive.extractfile(member)
                data = b"" if handle is None else handle.read()
            else:
                data = b""
            entries.append(
                {
                    "data": data,
                    "kind": kind,
                    "mode": type_bits | (member.mode & 0o7777),
                    "name": name,
                }
            )
    if not entries:
        raise InitrdBuildError("layer contains no entries")
    order = [entry["name"].encode("utf-8") for entry in entries]
    if order != sorted(order):
        raise InitrdBuildError("layer is not sorted by logical path bytes")
    if len(set(order)) != len(order):
        raise InitrdBuildError("layer repeats a logical path")
    return entries


def initrd_bytes(layer: bytes) -> bytes:
    """Write the uncompressed `newc` archive for a frozen rootfs layer."""

    out = io.BytesIO()
    for index, entry in enumerate(layer_entries(layer), start=1):
        out.write(
            _record(
                ino=index,
                mode=entry["mode"],
                nlink=DIRECTORY_NLINK if entry["kind"] == "directory" else DEFAULT_NLINK,
                name=entry["name"],
                data=entry["data"],
            )
        )
    out.write(_record(ino=0, mode=0, nlink=DEFAULT_NLINK, name=TRAILER_NAME, data=b""))
    return out.getvalue()


def parse_newc(raw: bytes) -> list[dict[str, Any]]:
    """Read back a `newc` archive.  Used by the tests and by the verify stage."""

    rows: list[dict[str, Any]] = []
    offset = 0
    while offset < len(raw):
        header = raw[offset : offset + HEADER_BYTES]
        if len(header) < HEADER_BYTES or header[:6] != MAGIC:
            raise InitrdBuildError(f"newc header is malformed at offset {offset}")
        values = [
            int(header[6 + i * 8 : 14 + i * 8], 16) for i in range(HEADER_FIELDS)
        ]
        (
            ino,
            mode,
            uid,
            gid,
            nlink,
            mtime,
            filesize,
            _devmajor,
            _devminor,
            _rdevmajor,
            _rdevminor,
            namesize,
            _check,
        ) = values
        name_start = offset + HEADER_BYTES
        name = raw[name_start : name_start + namesize - 1].decode("utf-8")
        header_end = name_start + namesize
        header_end += (-header_end) % ALIGNMENT
        data = raw[header_end : header_end + filesize]
        data_end = header_end + filesize
        data_end += (-data_end) % ALIGNMENT
        rows.append(
            {
                "data": data,
                "dataEnd": data_end,
                "filesize": filesize,
                "gid": gid,
                "headerEnd": header_end,
                "ino": ino,
                "magic": header[:6],
                "mode": mode,
                "mtime": mtime,
                "name": name,
                "nlink": nlink,
                "uid": uid,
            }
        )
        offset = data_end
        if name == TRAILER_NAME:
            break
    if not rows or rows[-1]["name"] != TRAILER_NAME:
        raise InitrdBuildError("newc archive has no trailer")
    return rows


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    write = sub.add_parser("write", help="write an initrd from a layer tar")
    write.add_argument("--layer", type=pathlib.Path, required=True)
    write.add_argument("--output", type=pathlib.Path, required=True)
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    args = _parser().parse_args(argv)
    try:
        raw = initrd_bytes(args.layer.read_bytes())
    except (InitrdBuildError, OSError, tarfile.TarError) as exc:
        print(f"initrd: {exc}", file=sys.stderr)
        return 1
    args.output.write_bytes(raw)
    print(f"initrd {args.output} bytes={len(raw)} sha256={hashlib.sha256(raw).hexdigest()}")
    print(f"compression: {COMPRESSION}  bootableClaim: {str(BOOTABLE_CLAIM).lower()}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
