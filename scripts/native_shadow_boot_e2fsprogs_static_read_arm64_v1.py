#!/usr/bin/env python3
"""Read an e2fsprogs build and say whether it still copies the staged st_ctime.

The frozen writer's defect was established by reading the binary, not by running
it, and a candidate that replaces it has to be held to the same standard.  So
this is a static read: `objdump -d` over the shipped `mke2fs` and its
`libext2fs.so.2`, no execution of either.

The rule was fixed before any candidate was read.  It is written down in
`native-shadow-boot-e2fsprogs-candidate-preregistration-arm64-v1.json` and it is
mechanical:

  `set_inode_extra` is the span between a call to `ext2fs_read_inode` and the
  next call to `ext2fs_write_inode` whose body loads the staged file's
  `st_ctim.tv_sec`.  On aarch64 LP64 that field sits at offset 0x68 of
  `struct stat`, so the span is found by a load at displacement 0x68.

  DEFECT  the span loads st_ctim (+0x68) and never reads fs->now (+0xb8): the
          image records whatever the staging filesystem happened to stamp, and
          no caller can set st_ctime, so the image cannot be made reproducible.
  FIXED   the span loads st_ctim, reads fs->now (+0xb8) and tests the
          fixed-time flag (+0xc8): i_ctime is clamped to the fixed time.

The library half is read separately and is decisive on its own.  The writer's
fixed branch is taken only when a flag inside `struct_ext2_filsys` is armed, and
only the library arms it -- reading `SOURCE_DATE_EPOCH`, storing it as fs->now
at +0xb8 and setting bit 0 at +0xc8.  A library that never arms that flag leaves
the writer's fixed branch unreachable no matter what the writer looks like, so a
missing gate decides the verdict before the writer is consulted at all.  That
ordering also keeps the read honest across versions: the span is found by a
heuristic that a compiler is free to inline away, while the gate is not.

Usage:

    native_shadow_boot_e2fsprogs_static_read_arm64_v1.py read --tree DIR
"""
from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re
import subprocess
import sys
from typing import Any, Iterator, Mapping, Optional

OBJDUMP = "/usr/bin/objdump"

ST_CTIME = 0x68  # struct stat (aarch64 LP64): st_ctim.tv_sec
FS_NOW = 0xB8  # struct struct_ext2_filsys: the fixed time
FS_FIXED_TIME_FLAG = 0xC8  # struct struct_ext2_filsys: bit 0 arms the fixed time

WRITER_MEMBER_PATH = "usr/sbin/mke2fs"
LIBRARY_DIRECTORY = "usr/lib/aarch64-linux-gnu"
LIBRARY_GLOB = "libext2fs.so.2.*"

VERDICT_FIXED = "FIXED"
VERDICT_DEFECT = "DEFECT"
VERDICT_INCONCLUSIVE = "INCONCLUSIVE"

_ADDRESS = re.compile(r"^\s+([0-9a-f]+):\s")
_CALL = re.compile(r"bl\s+0x[0-9a-f]+ <([A-Za-z_0-9]+)@plt>")

# How far the span between the two calls is allowed to run before it is taken to
# be two unrelated calls rather than one function body.
_MAX_SPAN_INSTRUCTIONS = 200


class StaticReadError(RuntimeError):
    """The binary could not be read at all -- which is not a verdict."""


def digest(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def disassemble(binary: pathlib.Path) -> list:
    """(address, instruction text) for every instruction objdump prints."""

    try:
        finished = subprocess.run(
            [OBJDUMP, "-d", "--no-show-raw-insn", str(binary)],
            capture_output=True,
            text=True,
            check=True,
        )
    except (OSError, subprocess.CalledProcessError) as exc:
        raise StaticReadError(f"could not disassemble {binary}: {exc}") from exc

    out = []
    for line in finished.stdout.splitlines():
        matched = _ADDRESS.match(line)
        if matched:
            out.append((int(matched.group(1), 16), line.split("\t", 1)[-1].strip()))
    if not out:
        raise StaticReadError(f"{binary} disassembled to nothing")
    return out


def _spans(code: list) -> Iterator:
    """Every ext2fs_read_inode -> ext2fs_write_inode body, as instruction text."""

    for index, (address, text) in enumerate(code):
        call = _CALL.search(text)
        if not (call and call.group(1) == "ext2fs_read_inode"):
            continue
        limit = min(index + _MAX_SPAN_INSTRUCTIONS, len(code))
        for forward in range(index + 1, limit):
            inner = _CALL.search(code[forward][1])
            if not inner:
                continue
            if inner.group(1) == "ext2fs_write_inode":
                yield address, code[forward][0], [text for _, text in code[index : forward + 1]]
                break
            if inner.group(1) == "ext2fs_read_inode":
                break


def _loads_at(body: list, offset: int) -> int:
    pattern = re.compile(rf"ldr[bh]?\s+[wxd][0-9]+, \[x[0-9]+, #{hex(offset)}\]")
    return sum(1 for line in body if pattern.search(line))


def read_writer(binary: pathlib.Path) -> dict:
    """Measure mke2fs.  Absence of a span is reported, never guessed at."""

    for start, end, body in _spans(disassemble(binary)):
        if _loads_at(body, ST_CTIME) == 0:
            continue  # not set_inode_extra: it never touches the staged ctime
        return {
            "window": f"0x{start:x}..0x{end:x}",
            "instructions": len(body),
            "stagedCtimeLoads": _loads_at(body, ST_CTIME),
            "fsNowLoads": _loads_at(body, FS_NOW),
            "fixedTimeFlagLoads": _loads_at(body, FS_FIXED_TIME_FLAG),
        }
    return {"window": None}


def read_library(binary: pathlib.Path) -> dict:
    """Measure libext2fs: does anything arm the fixed-time flag, and where."""

    code = disassemble(binary)
    text = binary.read_bytes()

    armed = []
    for index, (address, instruction) in enumerate(code):
        if not re.search(rf"str\s+w[0-9]+, \[x[0-9]+, #{hex(FS_FIXED_TIME_FLAG)}\]", instruction):
            continue
        before = [line for _, line in code[max(0, index - 6) : index]]
        if not any("orr" in line and "#0x1" in line for line in before):
            continue
        if not any(
            re.search(rf"str\s+x[0-9]+, \[x[0-9]+, #{hex(FS_NOW)}\]", line) for line in before
        ):
            continue
        armed.append(f"0x{address:x}")

    unguarded = []
    for index, (address, instruction) in enumerate(code):
        call = _CALL.search(instruction)
        if not (call and call.group(1) == "time"):
            continue
        before = [line for _, line in code[max(0, index - 8) : index]]
        if any(f"#{hex(FS_FIXED_TIME_FLAG)}]" in line for line in before):
            continue
        unguarded.append(f"0x{address:x}")

    return {
        "hasSourceDateEpochString": b"SOURCE_DATE_EPOCH" in text,
        "hasFakeTimeString": b"E2FSPROGS_FAKE_TIME" in text,
        "flagArmedAt": armed,
        "timeCallsNotBehindTheFlag": unguarded,
    }


def verdict(writer: Mapping, library: Mapping) -> dict:
    """Apply the pre-registered rule.  The library gate is consulted first."""

    if not library.get("hasSourceDateEpochString") or not library.get("flagArmedAt"):
        return {
            "verdict": VERDICT_DEFECT,
            "why": (
                "the library never arms the fixed-time flag, so the writer's "
                "clamped branch is unreachable and i_ctime comes from the staging "
                "filesystem"
            ),
        }
    if writer.get("window") is None:
        return {
            "verdict": VERDICT_INCONCLUSIVE,
            "why": "no set_inode_extra span was found in this build",
        }
    if writer.get("stagedCtimeLoads") and not writer.get("fsNowLoads"):
        return {
            "verdict": VERDICT_DEFECT,
            "why": "the staged st_ctime is copied with no reference to the fixed time",
        }
    if not (writer.get("fsNowLoads") and writer.get("fixedTimeFlagLoads")):
        return {
            "verdict": VERDICT_INCONCLUSIVE,
            "why": "the fixed time is referenced but the branch is not flag-gated",
        }
    return {
        "verdict": VERDICT_FIXED,
        "why": "i_ctime is clamped to the fixed time and the library arms the gate",
    }


def _library_of(tree: pathlib.Path) -> pathlib.Path:
    candidates = sorted((tree / LIBRARY_DIRECTORY).glob(LIBRARY_GLOB))
    real = [path for path in candidates if not path.is_symlink()]
    if len(real) != 1:
        raise StaticReadError(
            f"expected exactly one {LIBRARY_GLOB} under {tree / LIBRARY_DIRECTORY}, "
            f"found {len(real)}"
        )
    return real[0]


def read_tree(tree: pathlib.Path) -> dict:
    """Read one extracted e2fsprogs tree and return its measurement and verdict."""

    writer_path = tree / WRITER_MEMBER_PATH
    library_path = _library_of(tree)
    if not writer_path.is_file():
        raise StaticReadError(f"no writer at {writer_path}")

    writer = read_writer(writer_path)
    library = read_library(library_path)
    outcome = verdict(writer, library)
    return {
        "library": {
            "memberPath": f"/{library_path.relative_to(tree)}",
            "sha256": digest(library_path),
            "sizeBytes": library_path.stat().st_size,
        },
        "libraryMeasurement": library,
        "verdict": outcome["verdict"],
        "why": outcome["why"],
        "writer": {
            "memberPath": f"/{WRITER_MEMBER_PATH}",
            "sha256": digest(writer_path),
            "sizeBytes": writer_path.stat().st_size,
        },
        "writerMeasurement": writer,
    }


def canonical_json(document: Any) -> bytes:
    return (
        json.dumps(document, indent=2, sort_keys=True, ensure_ascii=False) + "\n"
    ).encode("utf-8")


def main(argv: Optional[list] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    read = sub.add_parser("read", help="read one extracted e2fsprogs tree")
    read.add_argument("--tree", type=pathlib.Path, required=True)
    read.add_argument("--out", type=pathlib.Path)

    args = parser.parse_args(argv)
    try:
        measured = read_tree(args.tree)
    except StaticReadError as exc:
        print(f"native-shadow e2fsprogs static read: FAIL: {exc}", file=sys.stderr)
        return 2

    body = canonical_json(measured)
    if args.out is not None:
        args.out.write_bytes(body)
    sys.stdout.write(body.decode("utf-8"))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
