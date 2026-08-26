#!/usr/bin/env python3
"""Produce-phase isolation, manifest and byte comparison for the arm64 boot images.

The frozen producer authority splits the work into two phases: ``acquire`` may
reach the network to fetch pinned URLs at pinned digests, and ``produce`` may
not reach it at all.  This module builds the wrapper that makes the second half
true rather than merely stated -- a transient ``systemd-run`` unit carrying the
properties the authority sealed -- and the two pieces that decide whether two
independent runs agreed: a ``sha256sum``-format manifest and a comparison that
reports a difference instead of resolving one.

Everything is derived from the sealed document rather than restated here,
because a second copy of a sealed fact can drift from it and the drift between
two copies of the same fact is invisible.  Deriving is not the same as trusting,
though: a document that dropped ``PrivateNetwork=yes``, or set it to ``no``, or
allowed the network in the produce phase, or named a runner other than the arm64
Linux one, is refused rather than faithfully turned into a weaker command line.

This module builds argv.  It does not execute anything, and it produces no
image.
"""

from __future__ import annotations

import argparse
import hashlib
import pathlib
import sys
from typing import Any, Iterable, Mapping, Optional, Sequence


sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

from scripts import native_shadow_boot_image_producer_authority_arm64_v2 as authority


BOOTABLE_CLAIM = False
ACTIVATION_ALLOWED = False
GUEST_IMAGE_BUILT = False

AUTHORITY_PATH = authority.AUTHORITY_PATH

SYSTEMD_RUN = "systemd-run"
REQUIRED_RUNNER = "ubuntu-24.04-arm"
PRODUCE_PHASE = "produce"
NETWORK_PROPERTY = "PrivateNetwork"
NETWORK_PROPERTY_REQUIRED_VALUE = "yes"
READ_WRITE_PROPERTY = "ReadWritePaths"

MANIFEST_FORMAT = "sha256sum-text"
MANIFEST_SEPARATOR = "  "
MISMATCH_ACTION = "report-the-difference-never-force-a-match"

ABORT_BUILDS_DIFFER = "independent-builds-differ"
ABORT_OUTPUT_MISSING = "output-missing-or-empty"
ABORT_NETWORK_ATTEMPTED = "network-attempted-during-produce"

# `ProtectSystem=strict` makes the whole hierarchy read-only, so the produce
# phase needs an explicit hole for its outputs. These roots are never that hole,
# and neither is anything inside them: punching one would undo the property
# rather than make room beside it.
FORBIDDEN_READ_WRITE_ROOTS = frozenset({"/usr", "/etc", "/boot", "/bin", "/lib", "/sbin"})

HEX = frozenset("0123456789abcdef")


class ProduceError(RuntimeError):
    """The sealed producer contract does not hold for this input."""


def load_authority(root: pathlib.Path) -> dict[str, Any]:
    """Read the sealed producer authority through its own frozen loader."""

    return authority.load_authority(root / AUTHORITY_PATH)


def output_names(document: Mapping[str, Any]) -> tuple[str, ...]:
    outputs = document.get("outputs")
    if not isinstance(outputs, list) or not outputs:
        raise ProduceError("producer authority declares no outputs")
    names: list[str] = []
    for row in outputs:
        name = row.get("name") if isinstance(row, Mapping) else None
        if not isinstance(name, str) or not name:
            raise ProduceError("producer authority output lacks a name")
        if name in names:
            raise ProduceError(f"producer authority repeats output {name!r}")
        names.append(name)
    return tuple(names)


def _isolation(document: Mapping[str, Any]) -> Mapping[str, Any]:
    isolation = document.get("buildIsolation")
    if not isinstance(isolation, Mapping):
        raise ProduceError("producer authority declares no buildIsolation")
    if isolation.get("runner") != REQUIRED_RUNNER:
        raise ProduceError(
            f"produce phase must run on {REQUIRED_RUNNER!r}, not {isolation.get('runner')!r}"
        )
    if isolation.get("separateJobs") is not True:
        raise ProduceError("the two independent produce jobs must stay separate")
    phases = isolation.get("phases")
    if not isinstance(phases, list):
        raise ProduceError("producer authority declares no build phases")
    produce = [row for row in phases if isinstance(row, Mapping) and row.get("name") == PRODUCE_PHASE]
    if len(produce) != 1:
        raise ProduceError(f"producer authority must declare exactly one {PRODUCE_PHASE!r} phase")
    if produce[0].get("networkAllowed") is not False:
        raise ProduceError(f"the {PRODUCE_PHASE!r} phase must not be allowed to reach the network")
    return isolation


def isolation_properties(document: Mapping[str, Any]) -> tuple[str, ...]:
    """The sealed property list, checked for the one property that must not weaken."""

    isolation = _isolation(document)
    properties = isolation.get("systemdRunProperties")
    if not isinstance(properties, list) or not properties:
        raise ProduceError("producer authority declares no systemd-run properties")
    seen: list[str] = []
    network: Optional[str] = None
    for entry in properties:
        if not isinstance(entry, str) or "=" not in entry:
            raise ProduceError(f"systemd-run property is not a NAME=VALUE pair: {entry!r}")
        name, _, value = entry.partition("=")
        if entry in seen:
            raise ProduceError(f"systemd-run property is repeated: {entry!r}")
        if name == READ_WRITE_PROPERTY:
            raise ProduceError("read-write holes belong to the caller, not to the sealed list")
        if name == NETWORK_PROPERTY:
            network = value
        seen.append(entry)
    if network is None:
        raise ProduceError(f"sealed properties do not carry {NETWORK_PROPERTY}=")
    if network != NETWORK_PROPERTY_REQUIRED_VALUE:
        raise ProduceError(
            f"{NETWORK_PROPERTY} must be {NETWORK_PROPERTY_REQUIRED_VALUE!r}, not {network!r}"
        )
    return tuple(seen)


def _read_write_property(path: pathlib.Path) -> str:
    text = str(path)
    if not path.is_absolute():
        raise ProduceError(f"read-write path must be absolute: {text!r}")
    candidate = pathlib.PurePosixPath(text)
    normalized = str(candidate)
    if normalized == "/":
        raise ProduceError(f"read-write path would undo ProtectSystem=strict: {text!r}")
    for root in FORBIDDEN_READ_WRITE_ROOTS:
        if candidate == pathlib.PurePosixPath(root) or candidate.is_relative_to(root):
            raise ProduceError(f"read-write path would undo ProtectSystem=strict: {text!r}")
    return f"{READ_WRITE_PROPERTY}={normalized}"


def isolation_argv(
    document: Mapping[str, Any],
    *,
    command: Sequence[str],
    read_write_paths: Iterable[pathlib.Path] = (),
) -> list[str]:
    """Build the transient-unit argv that runs ``command`` with no network."""

    properties = isolation_properties(document)
    if not command:
        raise ProduceError("produce phase was given no command to run")
    argv = [SYSTEMD_RUN, "--pipe", "--wait", "--collect", "--service-type=exec"]
    for entry in properties:
        argv.append(f"--property={entry}")
    for path in read_write_paths:
        argv.append(f"--property={_read_write_property(path)}")
    argv.append("--")
    argv.extend(command)
    return argv


def manifest_text(entries: Mapping[str, str]) -> str:
    lines = []
    for name in sorted(entries, key=lambda value: value.encode("utf-8")):
        lines.append(f"{entries[name]}{MANIFEST_SEPARATOR}{name}\n")
    return "".join(lines)


def parse_manifest(text: str) -> dict[str, str]:
    entries: dict[str, str] = {}
    for line in text.splitlines():
        if not line:
            continue
        digest, separator, name = line.partition(MANIFEST_SEPARATOR)
        if not separator or not name:
            raise ProduceError(f"manifest line is not sha256sum text: {line!r}")
        if len(digest) != 64 or not set(digest) <= HEX:
            raise ProduceError(f"manifest digest is not lowercase sha-256 hex: {digest!r}")
        if name in entries:
            raise ProduceError(f"manifest repeats output {name!r}")
        entries[name] = digest
    return entries


def manifest_from_directory(
    directory: pathlib.Path, names: Sequence[str]
) -> dict[str, str]:
    entries: dict[str, str] = {}
    for name in names:
        path = directory / name
        if not path.is_file():
            raise ProduceError(f"{ABORT_OUTPUT_MISSING}: {name} is not a regular file")
        raw = path.read_bytes()
        if not raw:
            raise ProduceError(f"{ABORT_OUTPUT_MISSING}: {name} is zero bytes")
        entries[name] = hashlib.sha256(raw).hexdigest()
    return entries


def compare_manifests(left: Mapping[str, str], right: Mapping[str, str]) -> None:
    """Report a difference between two independent jobs; never resolve one."""

    for name in sorted(set(left) | set(right)):
        if name not in left or name not in right:
            present = "first" if name in left else "second"
            raise ProduceError(
                f"{ABORT_BUILDS_DIFFER}: {name} is present only in the {present} job"
            )
        if left[name] != right[name]:
            raise ProduceError(
                f"{ABORT_BUILDS_DIFFER}: {name} first={left[name]} second={right[name]}"
            )


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)

    isolate = sub.add_parser("isolation-argv", help="print the produce-phase argv")
    isolate.add_argument("--repository-root", type=pathlib.Path, default=pathlib.Path("."))
    isolate.add_argument("--read-write-path", type=pathlib.Path, action="append", default=[])
    isolate.add_argument("rest", nargs=argparse.REMAINDER)

    manifest = sub.add_parser("manifest", help="print the sha256sum manifest of the outputs")
    manifest.add_argument("--repository-root", type=pathlib.Path, default=pathlib.Path("."))
    manifest.add_argument("--outputs", type=pathlib.Path, required=True)

    compare = sub.add_parser("compare", help="compare two manifests byte for byte")
    compare.add_argument("--first", type=pathlib.Path, required=True)
    compare.add_argument("--second", type=pathlib.Path, required=True)
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    args = _parser().parse_args(argv)
    if args.command == "isolation-argv":
        document = load_authority(args.repository_root)
        rest = [item for item in args.rest if item != "--"]
        line = isolation_argv(
            document, command=rest, read_write_paths=args.read_write_path
        )
        print("\n".join(line))
        return 0
    if args.command == "manifest":
        document = load_authority(args.repository_root)
        entries = manifest_from_directory(args.outputs, output_names(document))
        sys.stdout.write(manifest_text(entries))
        return 0
    first = parse_manifest(args.first.read_text(encoding="utf-8"))
    second = parse_manifest(args.second.read_text(encoding="utf-8"))
    compare_manifests(first, second)
    print(f"identical: {len(first)} outputs")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
