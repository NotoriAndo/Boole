#!/usr/bin/env python3
"""Measure the assembled boot staging tree, without being able to produce one.

The sealed plan carried bounds where it needed measurements: an entry count and a
byte total that were declared from two separate builds rather than taken from one
assembled tree.  Adding the two numbers would not have answered the question,
because the merge derives parent directories, resolves a mount point and can
refuse a path that two halves both claim.  So this takes the measurement the only
way it can be taken -- assemble the tree through the merge production will use,
write it out, and walk it back.

Two numbers are produced for every quantity the plan names.  The first comes from
the assembled entry table, which the builder holds in memory.  The second comes
from a walk of the written tree that knows nothing about that table: it reads
directory entries, calls ``lstat``, and counts what it finds.  The two have to
agree exactly.  A filesystem that folds upper and lower case would silently merge
files whose names differ only in case -- this tree holds several such pairs -- so
the destination is probed before anything is written and refused if it folds.

What this module cannot do is make an image.  It writes a directory tree and a
JSON record; it names no filesystem-image tool, no boot-image assembler and no
workflow dispatch, it imports no production phase, and the only external programs
it will let a caller pass through are the two replay tools the release gate needs.
``FORBIDDEN_EXECUTABLES`` is the list its own gate greps for, so a tool that
appears anywhere else in this file fails that gate rather than running.

Passing the three limits means the preconditions for producing an image are met.
It is not a claim that an image was produced, that anything was served, or that
anything booted.
"""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
import pathlib
import shutil
import stat
import sys
import tarfile
from typing import Any, Optional


sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

from scripts import native_shadow_rootfs_builder_boot_arm64_v1 as boot_v1
from scripts import native_shadow_rootfs_builder_boot_arm64_v3 as builder
from scripts import native_shadow_rootfs_portable_boot_arm64_v2 as release_gate


IMAGE_PRODUCED_CLAIM = False
SERVING_CLAIM = False
BOOT_CLAIM = False

MEASUREMENT_SCHEMA = "boole.native-shadow.boot-staging-tree-measurement.arm64.v1"

REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parents[1]
BOOT_SOURCE_LOCK_PATH = (
    REPOSITORY_ROOT
    / "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v2.json"
)

# The release gate runs these two and nothing else, so nothing else is accepted.
ALLOWED_REPLAY_TOOLS = frozenset({"gpgv", "zstd"})

# Named once, here, so this module's gate can require that they appear nowhere
# else in this file.  Each is a step in making a disk image out of a tree; none of
# them is a step in measuring one.
FORBIDDEN_EXECUTABLES = (
    "mke2fs",
    "mkfs.ext4",
    "mkinitramfs",
    "dracut",
    "hdiutil",
    "qemu-img",
    "resize2fs",
    "tune2fs",
    "debugfs",
)


class StagingMeasurementError(RuntimeError):
    """The staging tree, or the place it would be written, is not measurable."""


def assert_replay_tool(path: pathlib.Path) -> pathlib.Path:
    """A program this measurement is allowed to hand to the release gate."""

    name = pathlib.Path(path).name
    if name in FORBIDDEN_EXECUTABLES:
        raise StagingMeasurementError(
            f"{name} is an image-production tool and is not runnable in measurement mode"
        )
    if name not in ALLOWED_REPLAY_TOOLS:
        raise StagingMeasurementError(
            "measurement mode runs only "
            f"{sorted(ALLOWED_REPLAY_TOOLS)}, and {name} is neither"
        )
    return pathlib.Path(path)


def filesystem_is_case_sensitive(path: pathlib.Path) -> bool:
    """Write one name and look for the other spelling of it."""

    probe = pathlib.Path(path) / "BooleCaseProbe"
    other = pathlib.Path(path) / "boolecaseprobe"
    try:
        probe.write_bytes(b"")
        return not other.exists()
    finally:
        if probe.exists():
            probe.unlink()


def assert_case_sensitive(path: pathlib.Path) -> None:
    """Refuse before writing, because folding loses entries silently.

    The assembled tree holds names that differ only in case.  On a folding
    filesystem the second one overwrites the first, the walk finds fewer entries
    than the builder assembled, and the two measurements disagree for a reason
    that has nothing to do with the tree.  Refusing here keeps that from being
    mistaken for a defect in the tree, and keeps it from being papered over.
    """

    if not filesystem_is_case_sensitive(path):
        raise StagingMeasurementError(
            f"{path} is not on a case-sensitive filesystem; the staging tree holds "
            "names that differ only in case and they would be lost"
        )


def _resolved_link(path: str, target: str) -> str:
    """Where a symlink points, read as if the tree were the root."""

    if target.startswith("/"):
        return os.path.normpath(target.lstrip("/"))
    return os.path.normpath(os.path.join(os.path.dirname(path), target))


def _path_manifest_sha256(paths: list) -> str:
    """One newline-terminated path per line, ordered by the paths' own bytes."""

    ordered = sorted(paths, key=lambda value: value.encode("utf-8"))
    return hashlib.sha256(
        b"".join(path.encode("utf-8") + b"\n" for path in ordered)
    ).hexdigest()


def largest_regular_file(sized_paths) -> tuple:
    """The greatest size, and the byte-smallest path among the files that reach it.

    Two regular files in the staging tree carry exactly the same largest size --
    the checker toolchain's ``libLLVM`` in the guest root, and the copy of it
    inside the nested runtime rootfs carried for replay -- so "the largest file"
    needs a second question answered before it names one path.  Answering it by
    whichever file was met first makes the answer a property of the filesystem
    the tree was written to rather than of the tree, and the two modes do not
    write to the same kind of filesystem.

    The rule is the one that produced the sealed value.  ``builder_totals``
    iterates its paths in the ordering the path manifest is built from -- the
    paths' own UTF-8 bytes -- and keeps the first file at the maximum, which
    together is exactly this: among the regular files of greatest size, the path
    whose canonical bytes sort first.  Sorting by anything else -- a locale, a
    case fold, a Unicode normalisation, the order a directory was read in --
    would answer a different question, so none of them appear here.

    An empty size wins nothing: a tree of only zero-byte files reports no path,
    which is what the sealed side reports for the same tree.
    """

    largest_bytes = 0
    largest_path = ""
    for path, size in sized_paths:
        if size > largest_bytes:
            largest_bytes = size
            largest_path = path
        elif size == largest_bytes and largest_path:
            if path.encode("utf-8") < largest_path.encode("utf-8"):
                largest_path = path
    return largest_bytes, largest_path


def _case_folded_siblings(paths: list) -> int:
    """How many entries share a case-folded name with another entry.

    This is the count a folding filesystem would lose, so both measurements
    compute it and the comparison catches a folding destination even if the probe
    somehow did not.
    """

    groups: dict = {}
    for path in paths:
        groups.setdefault(path.casefold(), 0)
        groups[path.casefold()] += 1
    return sum(count - 1 for count in groups.values() if count > 1)


def builder_totals(entries: dict) -> dict:
    """What the assembled entry table says about itself."""

    kinds: dict = {}
    payload = 0
    largest_bytes = 0
    largest_path = ""
    escapes = 0
    paths = list(entries)
    for path in sorted(paths, key=lambda value: value.encode("utf-8")):
        entry = entries[path]
        kind = entry["kind"]
        kinds[kind] = kinds.get(kind, 0) + 1
        if kind == "file":
            size = len(entry["raw"])
            payload += size
            if size > largest_bytes:
                largest_bytes = size
                largest_path = path
        elif kind == "symlink":
            if _resolved_link(path, entry["target"]).startswith(".."):
                escapes += 1
    return {
        "entries": len(paths),
        "byKind": kinds,
        "payloadBytes": payload,
        "largestFileBytes": largest_bytes,
        "largestFilePath": largest_path,
        "pathManifestSha256": _path_manifest_sha256(paths),
        "caseFoldedSiblings": _case_folded_siblings(paths),
        "duplicatePaths": len(paths) - len(set(paths)),
        "pathCollisions": 0,
        "symlinkEscapes": escapes,
    }


def write_staging_tree(entries: dict, destination: pathlib.Path, mtime: int) -> None:
    """Write the assembled table out, through the builder's own layer writer.

    The bytes that land on disk are the bytes production would put in its layer,
    because they are produced by the same function; extracting them is how the
    tree is obtained on the other side there too.  Ownership is not reproduced --
    a non-root writer cannot -- and no measurement here depends on it.
    """

    destination = pathlib.Path(destination)
    if destination.exists() or destination.is_symlink():
        raise StagingMeasurementError(f"staging destination already exists: {destination}")
    for path, entry in sorted(entries.items()):
        if entry["kind"] != "symlink":
            continue
        if _resolved_link(path, entry["target"]).startswith(".."):
            raise StagingMeasurementError(
                f"symlink {path} leaves the staging tree: {entry['target']}"
            )
    layer = builder.__getattr__("_layer_bytes")(entries, mtime)
    destination.mkdir(parents=True)
    with tarfile.open(fileobj=io.BytesIO(layer), mode="r:") as archive:
        archive.extractall(path=str(destination), numeric_owner=True)


def traverse_staging_tree(destination: pathlib.Path) -> dict:
    """Walk what is on disk, knowing nothing about what put it there."""

    destination = pathlib.Path(destination)
    kinds: dict = {}
    payload = 0
    sized_files: list = []
    escapes = 0
    paths: list = []
    pending = [destination]
    while pending:
        current = pending.pop()
        with os.scandir(current) as scan:
            for item in scan:
                relative = os.path.relpath(item.path, destination)
                paths.append(relative)
                info = item.stat(follow_symlinks=False)
                if stat.S_ISLNK(info.st_mode):
                    kind = "symlink"
                    if _resolved_link(relative, os.readlink(item.path)).startswith(".."):
                        escapes += 1
                elif stat.S_ISDIR(info.st_mode):
                    kind = "directory"
                    pending.append(pathlib.Path(item.path))
                elif stat.S_ISREG(info.st_mode):
                    kind = "file"
                    payload += info.st_size
                    sized_files.append((relative, info.st_size))
                else:
                    raise StagingMeasurementError(
                        f"staging tree holds something that is not a file, directory "
                        f"or symlink: {relative}"
                    )
                kinds[kind] = kinds.get(kind, 0) + 1
    largest_bytes, largest_path = largest_regular_file(sized_files)
    return {
        "entries": len(paths),
        "byKind": kinds,
        "payloadBytes": payload,
        "largestFileBytes": largest_bytes,
        "largestFilePath": largest_path,
        "pathManifestSha256": _path_manifest_sha256(paths),
        "caseFoldedSiblings": _case_folded_siblings(paths),
        "duplicatePaths": len(paths) - len(set(paths)),
        "pathCollisions": 0,
        "symlinkEscapes": escapes,
    }


AGREEMENT_KEYS = (
    "entries",
    "byKind",
    "payloadBytes",
    "largestFileBytes",
    "largestFilePath",
    "pathManifestSha256",
    "caseFoldedSiblings",
    "duplicatePaths",
    "symlinkEscapes",
)


def assert_measurements_agree(computed: dict, walked: dict) -> None:
    """Every quantity the plan names, from both sides, or the run fails."""

    for key in AGREEMENT_KEYS:
        if computed[key] != walked[key]:
            raise StagingMeasurementError(
                f"the assembled table and the staging tree disagree on {key}: "
                f"{computed[key]!r} against {walked[key]!r}"
            )


def assert_within_limits(recipe: dict, totals: dict) -> None:
    """The sealed recipe's own three numbers, applied without adjustment."""

    if totals["entries"] > recipe["maxEntries"]:
        raise StagingMeasurementError(
            f"staging tree exceeds the entry limit: {totals['entries']} "
            f"against {recipe['maxEntries']}"
        )
    if totals["payloadBytes"] > recipe["maxTotalBytes"]:
        raise StagingMeasurementError(
            f"staging tree exceeds the total byte limit: {totals['payloadBytes']} "
            f"against {recipe['maxTotalBytes']}"
        )
    if totals["largestFileBytes"] > recipe["maxFileBytes"]:
        raise StagingMeasurementError(
            f"staging tree exceeds the single file limit: "
            f"{totals['largestFileBytes']} against {recipe['maxFileBytes']}"
        )


def nested_manifest_on_disk(destination: pathlib.Path) -> dict:
    """Read back the manifest the launcher verifies, from the written tree."""

    declaration = builder.NESTED_RUNTIME_TREE
    relative = declaration["contentManifestGuestPath"].lstrip("/")
    path = pathlib.Path(destination) / relative
    try:
        raw = path.read_bytes()
    except OSError as exc:
        raise StagingMeasurementError(
            f"the nested content manifest is not in the staging tree: {relative}"
        ) from exc
    digest = hashlib.sha256(raw).hexdigest()
    if digest != declaration["contentManifestSha256"]:
        raise StagingMeasurementError(
            f"the nested content manifest in the tree hashes to {digest}, the seal "
            f"says {declaration['contentManifestSha256']}"
        )
    if len(raw) != declaration["contentManifestSizeBytes"]:
        raise StagingMeasurementError(
            f"the nested content manifest in the tree is {len(raw)} bytes, the seal "
            f"says {declaration['contentManifestSizeBytes']}"
        )
    return {
        "guestPath": declaration["contentManifestGuestPath"],
        "sha256": digest,
        "sizeBytes": len(raw),
    }


def launcher_accounting(entries: dict) -> dict:
    """The one sealed file this host cannot hold, accounted for exactly.

    The guest launcher is an aarch64 Linux ELF that is rebuilt and digest-matched
    at build time; a machine that cannot produce those bytes cannot put them in
    the tree, and substituting different bytes is refused by the builder rather
    than allowed.  So the tree is measured without it and the sealed size is added
    back for the limit decision, which is therefore taken on the larger number.
    """

    guest_path = boot_v1.LAUNCHER_GUEST_PATH.lstrip("/")
    parent = os.path.dirname(guest_path)
    added_directories = 0
    while parent and parent not in entries:
        added_directories += 1
        parent = os.path.dirname(parent)
    return {
        "includedInTheMeasuredTree": guest_path in entries,
        "guestPath": boot_v1.LAUNCHER_GUEST_PATH,
        "sealedSha256": boot_v1.LAUNCHER_SHA256,
        "sealedSizeBytes": boot_v1.LAUNCHER_SIZE_BYTES,
        "entriesItWouldAdd": 0 if guest_path in entries else 1 + added_directories,
    }


def measure(
    repository_root: pathlib.Path,
    artifact_store: pathlib.Path,
    gpgv: pathlib.Path,
    zstd: pathlib.Path,
    staging: pathlib.Path,
    *,
    launcher_binary: Optional[bytes] = None,
) -> dict:
    """Assemble, write, walk, compare, and apply the sealed limits."""

    gpgv = assert_replay_tool(gpgv)
    zstd = assert_replay_tool(zstd)
    staging = pathlib.Path(staging)
    staging.parent.mkdir(parents=True, exist_ok=True)
    assert_case_sensitive(staging.parent)

    sealed_raw = BOOT_SOURCE_LOCK_PATH.read_bytes()
    sealed = json.loads(sealed_raw.decode("utf-8"))
    runtime, _receipt = release_gate.materialize_runtime_lock(
        sealed, sealed_raw, gpgv, zstd
    )
    lock, lock_raw, _record = boot_v1.normalized_runtime_lock(runtime)
    validated = builder.validate_source_lock(
        lock, lock_raw, repository_root, artifact_store, require_complete=True
    )
    nested = builder.nested_runtime_tree(repository_root, artifact_store, gpgv, zstd)
    entries = builder.materialize_staging_tree(
        validated,
        repository_root,
        artifact_store,
        launcher_binary=launcher_binary,
        nested_tree=nested,
    )

    recipe = lock["buildRecipe"]
    computed = builder_totals(entries)
    write_staging_tree(entries, staging, recipe["canonicalMtime"])
    walked = traverse_staging_tree(staging)
    assert_measurements_agree(computed, walked)
    assert_within_limits(recipe, walked)

    launcher = launcher_accounting(entries)
    complete = {
        "entries": walked["entries"] + launcher["entriesItWouldAdd"],
        "payloadBytes": walked["payloadBytes"]
        + (0 if launcher["includedInTheMeasuredTree"] else launcher["sealedSizeBytes"]),
        "largestFileBytes": max(
            walked["largestFileBytes"],
            0 if launcher["includedInTheMeasuredTree"] else launcher["sealedSizeBytes"],
        ),
    }
    assert_within_limits(recipe, complete)

    return {
        "schema": MEASUREMENT_SCHEMA,
        "authorityStatus": "MEASURED-NOT-PRODUCED",
        "imageProduced": IMAGE_PRODUCED_CLAIM,
        "servingClaim": SERVING_CLAIM,
        "bootClaim": BOOT_CLAIM,
        "activationAllowed": False,
        "imageProductionPreconditionsMet": True,
        "payloadBytesIsAMeasuredTotal": True,
        "bootSourceLockSha256": hashlib.sha256(sealed_raw).hexdigest(),
        "bootSourceLockRelease": sealed["release"],
        "nestedSourceLockSha256": builder.NESTED_RUNTIME_TREE["drivingSourceLockSha256"],
        "builderProjectionSha256": builder.SUCCESSOR_PROJECTION_SHA256,
        "frozenBuilderSha256": builder.BUILDER_SHA256,
        "limits": {
            "maxEntries": recipe["maxEntries"],
            "maxFileBytes": recipe["maxFileBytes"],
            "maxTotalBytes": recipe["maxTotalBytes"],
        },
        "builderInternal": computed,
        "independentTraversal": walked,
        "withSealedLauncher": complete,
        "launcher": launcher,
        "nestedContentManifest": nested_manifest_on_disk(staging),
    }


def main(argv: Optional[list] = None) -> int:
    parser = argparse.ArgumentParser(
        description="Measure the assembled boot staging tree. Produces no image."
    )
    parser.add_argument("--repo-root", required=True, type=pathlib.Path)
    parser.add_argument("--cas", required=True, type=pathlib.Path)
    parser.add_argument("--gpgv", required=True, type=pathlib.Path)
    parser.add_argument("--zstd", required=True, type=pathlib.Path)
    parser.add_argument("--staging", required=True, type=pathlib.Path)
    parser.add_argument("--output", required=True, type=pathlib.Path)
    parser.add_argument(
        "--keep-staging",
        action="store_true",
        help="leave the written tree in place instead of removing it",
    )
    args = parser.parse_args(argv)
    record = measure(
        args.repo_root.resolve(),
        args.cas.resolve(),
        args.gpgv.resolve(),
        args.zstd.resolve(),
        args.staging,
    )
    args.output.write_bytes(builder.canonical_json(record))
    if not args.keep_staging:
        shutil.rmtree(args.staging, ignore_errors=True)
    print(f"native-shadow-boot-staging-measure: PASS: {args.output}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except StagingMeasurementError as exc:
        print(f"native-shadow-boot-staging-measure: FAIL: {exc}", file=sys.stderr)
        raise SystemExit(2)
