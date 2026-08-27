#!/usr/bin/env python3
"""Boot projection of the frozen Linux/arm64 native-shadow rootfs builder.

The boot source lock pins ``buildRecipe.builderSha256`` to the arm64 builder's
own bytes, so the builder that reads it has to be exactly that file.  This
module therefore projects the same bytes -- ``BUILDER_SHA256`` here equals the
pin -- and records its own digest separately as ``BOOT_PROJECTION_SHA256``,
because the widening below is not covered by the pin and should not pretend to
be.

Seven kinds of change, in decreasing order of ordinariness.

The first two are what a projection is for: the authority-file table grows from
six entries to ten (machine-id, the launcher unit, and the sysusers and tmpfiles
configuration the unit needs), and the provenance-closure table grows from three
to five (the guest init/launcher closure and the guest kernel/module closure).
Both are configuration for a new release, not a weakening of any check.

The third is an ordering normalisation.  One closure in the sealed lock lists
its logical roots in an order that is alphabetical but not byte-sorted, and the
frozen builder compares against ``sorted(set(roots))``.  The sealed bytes are
not touched: ``normalized_runtime_lock`` sorts a copy and records the declared
and normalised orders side by side, and refuses outright if sorting would also
drop a duplicate, because that half of the builder's check must stay real.

The fourth touches the frozen builder's dependency reading, and is the one worth
reading closely.

``_split_dependency_groups`` rejects architecture-qualified and build-profile
dependencies -- correctly -- but applies the pattern to the whole comma-joined
field.  ``python3 (<< 3.13), python3 (>= 3.12~)`` then matches, because the
``<`` of one constraint and the ``>`` of a later one look like a profile once
everything between them is ignored.  Across all 5816 dependency fields in the
frozen Packages index this fires 98 times and is wrong all 98 times; the same
scan finds no real qualifier anywhere in the index.  The fix applies the same
pattern to each alternative instead of the joined string.  A real ``[arm64]`` or
``<!nocheck>`` lives entirely inside one alternative, so nothing that the
original rejects survives the narrower scope.

``_dependency_matches`` refuses every Multi-Arch qualifier.  Five packages in
this closure depend on ``python3:any``, and all five resolve to the same
``python3``.  ``:any`` means "satisfied by that package from any architecture";
this closure holds exactly one concrete architecture, so ``:any`` and the bare
name select the same candidate.  That equivalence is a fact about this closure
rather than a general one, so it is guarded twice: ``assert_single_architecture``
refuses a second concrete architecture, and the projected matcher refuses
``:any`` against a provider that does not declare ``Multi-Arch: allowed``.
``:native`` stays unsupported.

The fifth changes what ends up inside an image rather than what the builder will
read, and it is enumerated rather than described.  Pointed at the sealed
191-package closure, the frozen builder refuses four times, and every refusal is
correct.  ``native-shadow-boot-rootfs-closure-exception-arm64-v1.json`` names the
four; this module reads that file rather than restating it, so the enumeration
cannot drift from the code that honours it.

The sealed image rule is ``ownership: root:root-only``.  The frozen builder
already writes every entry as uid 0 and gid 0 unconditionally; its two refusals
here exist so that a member arriving as something else stops the build instead
of being silently rewritten.  Eleven members do arrive as something else -- some
owned by group ``shadow``, all carrying a set-id bit, across
``libpam-modules-bin``, ``mount``, ``passwd`` and ``sudo``.  The record names
them with the exact ownership and mode each arrives with, and a member is
admitted only when all three match; a twelfth member, or one of the eleven
arriving differently, meets the original refusal with its original message.

Ubuntu 24.04 is merged-``/usr``, so ``bin``, ``lib`` and ``sbin`` arriving as
directories collide with the sealed lock's ``/lib -> usr/lib`` only because the
two halves are being read literally.  Relocation canonicalises the path and then
reuses the frozen collision rule verbatim, so it moves paths without deciding
which of two members wins.  ``base-files`` is not in this closure, so nothing
ships the merged-``/usr`` symlinks at all: ``/bin`` and ``/sbin`` are derived the
same way ``/lib`` already was, and ``/lib64`` is not, because arm64 does not use
it.  Nine symlinks point outside the image: six resolve into ``dev``, ``proc``,
``run``, ``sys`` or ``tmp`` and are allowed as a category, since a filesystem
mounted at boot is absent by construction; the other three are named one by one.

File contents are untouched throughout and still carry their sealed digests.

The sixth places the launcher.  ``/usr/libexec/boole`` is a declared closure root
that nothing fills, because the launcher is a build product rather than a deb or
a tracked repository file.  The frozen producer authority says how it may arrive
instead -- ``rebuild-and-match-seal`` -- and ``launcher_entry`` enforces exactly
that: a binary whose size or digest differs from the seal stops the build with
the authority's own ``launcher-digest-mismatch`` rather than being written into
the tree.  Supplying it is the caller's decision, so ``build_oci_layout`` takes
it by keyword and ``verify_oci_layout`` forwards it to its own rebuild.

The seventh is the one a boot depends on.  Directories reach the tree only when
some member needs them as a parent, and no member needs ``/proc``, so the image
had nowhere for the kernel filesystems to be mounted -- which is exactly where
the single MAC.3 boot stopped, with PID 1 freezing after three failed mounts.
The five directories are merged in from
``native-shadow-boot-rootfs-runtime-mount-points-arm64-v1.json``, whose list is
five rather than the three that transcript named because it was taken from the
guest's own systemd: the mount table decoded out of the ``libsystemd-shared``
the image ships, every ``.mount`` unit in it, and the absence of ``/etc/fstab``.
That record is read rather than restated here, and it is checked against the
sealed closure exception's ``runtimeFilesystemRoots`` so the two cannot drift.

Projecting a builder is not building an image.  Nothing here writes one.
"""

from __future__ import annotations

import copy
import hashlib
import json
import pathlib
import stat
import sys
from typing import Any, Optional


sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

from scripts import native_shadow_boot_rootfs_mount_point_audit_arm64_v1 as mount_points
from scripts import native_shadow_rootfs_builder_arm64_v1 as arm64


BOOTABLE_CLAIM = False
ACTIVATION_ALLOWED = False

ARM64_BUILDER = pathlib.Path(arm64.__file__).resolve()
ARM64_BUILDER_SHA256 = "180e893e9643c6fab110016119679b96a5ddf56785cd398b51c8cf8352615ef4"

BOOT_SOURCE_LOCK_RELEASE = (
    "NATIVE-SHADOW-BOOT-ROOTFS-SOURCE-LOCK-ARM64-V1-SOURCE-SHAPE-ONLY-NOT-BOOTABLE"
)

BOOT_AUTHORITY_FILES = {
    "guest-machine-id": (
        "native/etc/machine-id",
        "/etc/machine-id",
    ),
    "launcher-unit": (
        "native/systemd/boole-native-shadow-launcher.service",
        "/usr/lib/systemd/system/boole-native-shadow-launcher.service",
    ),
    "sysusers-config": (
        "native/sysusers.d/boole-native-shadow.conf",
        "/usr/lib/sysusers.d/boole-native-shadow.conf",
    ),
    "tmpfiles-config": (
        "native/tmpfiles.d/boole-native-shadow.conf",
        "/usr/lib/tmpfiles.d/boole-native-shadow.conf",
    ),
}
BOOT_PROVENANCE_CLOSURE_ROOTS = {
    "guest-init-and-launcher-file-manifest": (
        "/etc/systemd",
        "/usr/lib/systemd",
        "/usr/lib/sysusers.d",
        "/usr/lib/tmpfiles.d",
        "/usr/libexec/boole",
    ),
    "guest-kernel-and-module-file-manifest": (
        "/boot",
        "/usr/lib/modules",
    ),
}

ARCHITECTURE_INDEPENDENT = "all"
MULTI_ARCH_ANY_REQUIREMENT = "allowed"

REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parents[1]
CLOSURE_EXCEPTION_PATH = (
    REPOSITORY_ROOT
    / "native/containment/native-shadow-boot-rootfs-closure-exception-arm64-v1.json"
)
CLOSURE_EXCEPTION_RELEASE = "NATIVE-SHADOW-BOOT-ROOTFS-CLOSURE-EXCEPTION-ARM64-V1"
PRODUCER_AUTHORITY_PATH = (
    REPOSITORY_ROOT
    / "native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json"
)
ABORT_LAUNCHER_DIGEST_MISMATCH = "launcher-digest-mismatch"
LAUNCHER_ACQUISITION = "rebuild-and-match-seal"
LAUNCHER_MODE = 0o755
SET_ID_BITS = stat.S_ISUID | stat.S_ISGID
NORMALIZED_MODE_MASK = 0o1777

REPLACEMENTS = (
    (
        '    if re.search(r"\\[[^\\]]*\\]|<[^<>=]+>", value):\n'
        '        raise RootfsBuildError("architecture/profile-qualified runtime dependency is unsupported")\n'
        "    groups: list[list[str]] = []\n"
        '    for raw_group in value.split(","):\n'
        '        alternatives = [item.strip() for item in raw_group.split("|")]\n'
        "        if not alternatives or any(not item for item in alternatives):\n"
        '            raise RootfsBuildError("Ubuntu dependency group syntax differs")\n'
        "        groups.append(alternatives)\n",
        "    groups: list[list[str]] = []\n"
        '    for raw_group in value.split(","):\n'
        '        alternatives = [item.strip() for item in raw_group.split("|")]\n'
        "        if not alternatives or any(not item for item in alternatives):\n"
        '            raise RootfsBuildError("Ubuntu dependency group syntax differs")\n'
        "        for alternative in alternatives:\n"
        '            if re.search(r"\\[[^\\]]*\\]|<[^<>=]+>", alternative):\n'
        '                raise RootfsBuildError("architecture/profile-qualified runtime dependency is unsupported")\n'
        "        groups.append(alternatives)\n",
        1,
    ),
    (
        '    if match.group("qualifier") is not None:\n'
        '        raise RootfsBuildError("Ubuntu Multi-Arch dependency qualifier is not supported in v1")\n',
        '    if match.group("qualifier") not in (None, ":any"):\n'
        '        raise RootfsBuildError("Ubuntu Multi-Arch dependency qualifier is not supported in v1")\n'
        '    if match.group("qualifier") == ":any" and candidate.get("multiArch") != "allowed":\n'
        '        raise RootfsBuildError("Ubuntu :any dependency needs a Multi-Arch: allowed provider")\n',
        1,
    ),
    (
        "                if member.uid != 0 or member.gid != 0:\n"
        '                    raise RootfsBuildError(f"{context} member is not root:root: {path}")\n',
        "                if member.uid != 0 or member.gid != 0:\n"
        "                    if not _metadata_exception(path, member):\n"
        '                        raise RootfsBuildError(f"{context} member is not root:root: {path}")\n',
        1,
    ),
    (
        "                if mode & (stat.S_ISUID | stat.S_ISGID):\n"
        '                    raise RootfsBuildError(f"{context} member has set-id mode: {path}")\n',
        "                if mode & (stat.S_ISUID | stat.S_ISGID):\n"
        "                    if not _metadata_exception(path, member):\n"
        '                        raise RootfsBuildError(f"{context} member has set-id mode: {path}")\n'
        "                    mode &= 0o1777\n",
        1,
    ),
    (
        '    return _tar_entries(payload, f"{context} data", recipe)\n',
        '    return _usrmerge(_tar_entries(payload, f"{context} data", recipe), f"{context} data")\n',
        1,
    ),
    (
        '        if entry["kind"] == "symlink" and entry["resolvedTarget"] not in entries:\n'
        '            raise RootfsBuildError(f"rootfs symlink target is absent: {path}")\n',
        '        if entry["kind"] == "symlink" and entry["resolvedTarget"] not in entries:\n'
        '            if not _dangling_allowed(path, entry["resolvedTarget"]):\n'
        '                raise RootfsBuildError(f"rootfs symlink target is absent: {path}")\n',
        1,
    ),
    (
        '            target = entries.get(entries[current]["resolvedTarget"])\n'
        "            if target is None:\n"
        '                raise RootfsBuildError(f"rootfs symlink target is absent: {current}")\n',
        '            target = entries.get(entries[current]["resolvedTarget"])\n'
        "            if target is None:\n"
        '                if _dangling_allowed(current, entries[current]["resolvedTarget"]):\n'
        "                    break\n"
        '                raise RootfsBuildError(f"rootfs symlink target is absent: {current}")\n',
        1,
    ),
    (
        "def _assemble_entries(\n"
        "    validated: dict[str, Any], repository_root: pathlib.Path, artifact_store: pathlib.Path\n"
        ") -> dict[str, dict[str, Any]]:\n",
        "def _assemble_entries(\n"
        "    validated: dict[str, Any],\n"
        "    repository_root: pathlib.Path,\n"
        "    artifact_store: pathlib.Path,\n"
        "    launcher_binary: Any = None,\n"
        ") -> dict[str, dict[str, Any]]:\n",
        1,
    ),
    (
        '    for derived in lock["derivedEntries"]:\n',
        "    if launcher_binary is not None:\n"
        "        entry = launcher_entry(launcher_binary)\n"
        '        _merge(entries, {entry["path"]: entry}, "rebuilt launcher")\n'
        '    for derived in lock["derivedEntries"]:\n',
        1,
    ),
    (
        "def build_oci_layout(\n"
        "    lock: Any,\n"
        "    lock_raw: bytes,\n"
        "    repository_root: pathlib.Path,\n"
        "    artifact_store: pathlib.Path,\n"
        "    output_dir: pathlib.Path,\n"
        "    *,\n"
        "    trusted_ubuntu_fingerprints: frozenset[str] = UBUNTU_ARCHIVE_SIGNING_FINGERPRINTS,\n"
        ") -> dict[str, Any]:\n",
        "def build_oci_layout(\n"
        "    lock: Any,\n"
        "    lock_raw: bytes,\n"
        "    repository_root: pathlib.Path,\n"
        "    artifact_store: pathlib.Path,\n"
        "    output_dir: pathlib.Path,\n"
        "    *,\n"
        "    trusted_ubuntu_fingerprints: frozenset[str] = UBUNTU_ARCHIVE_SIGNING_FINGERPRINTS,\n"
        "    launcher_binary: Any = None,\n"
        ") -> dict[str, Any]:\n",
        1,
    ),
    (
        "    entries = _assemble_entries(validated, repository_root, artifact_store)\n",
        "    entries = _assemble_entries(\n"
        "        validated, repository_root, artifact_store, launcher_binary\n"
        "    )\n",
        1,
    ),
    (
        "def verify_oci_layout(\n"
        "    lock: Any,\n"
        "    lock_raw: bytes,\n"
        "    repository_root: pathlib.Path,\n"
        "    artifact_store: pathlib.Path,\n"
        "    layout: pathlib.Path,\n"
        "    *,\n"
        "    trusted_ubuntu_fingerprints: frozenset[str] = UBUNTU_ARCHIVE_SIGNING_FINGERPRINTS,\n"
        ") -> dict[str, Any]:\n",
        "def verify_oci_layout(\n"
        "    lock: Any,\n"
        "    lock_raw: bytes,\n"
        "    repository_root: pathlib.Path,\n"
        "    artifact_store: pathlib.Path,\n"
        "    layout: pathlib.Path,\n"
        "    *,\n"
        "    trusted_ubuntu_fingerprints: frozenset[str] = UBUNTU_ARCHIVE_SIGNING_FINGERPRINTS,\n"
        "    launcher_binary: Any = None,\n"
        ") -> dict[str, Any]:\n",
        1,
    ),
    (
        "            rebuilt,\n"
        "            trusted_ubuntu_fingerprints=trusted_ubuntu_fingerprints,\n"
        "        )\n",
        "            rebuilt,\n"
        "            trusted_ubuntu_fingerprints=trusted_ubuntu_fingerprints,\n"
        "            launcher_binary=launcher_binary,\n"
        "        )\n",
        1,
    ),
    # A CI job runs the builder as a process, so the keyword parameter needs a
    # flag. It stays optional: a build that supplies no launcher is still the
    # build the two local runs already agreed on, byte for byte.
    (
        '        if command == "build":\n'
        '            sub.add_argument("--output", required=True, type=pathlib.Path)\n'
        '        elif command == "verify":\n'
        '            sub.add_argument("--layout", required=True, type=pathlib.Path)\n',
        '        if command == "build":\n'
        '            sub.add_argument("--output", required=True, type=pathlib.Path)\n'
        '            sub.add_argument("--launcher", type=pathlib.Path)\n'
        '        elif command == "verify":\n'
        '            sub.add_argument("--layout", required=True, type=pathlib.Path)\n'
        '            sub.add_argument("--launcher", type=pathlib.Path)\n',
        1,
    ),
    # Seventh, and the one a boot depends on. `_ensure_parents` fills in every
    # directory some member needs, and no member needs `/proc`, so none of the
    # five the kernel filesystems are mounted on ends up in the tree. PID 1
    # mounts four of them before it runs a unit and stops if one is missing --
    # which is what the single MAC.3 boot showed. They are merged before
    # `_ensure_parents` so an entry that already exists still collides loudly
    # rather than being overwritten from here.
    (
        "    _ensure_parents(entries)\n",
        '    _merge(entries, runtime_mount_point_entries(), "runtime mount point")\n'
        "    _ensure_parents(entries)\n",
        1,
    ),
    (
        '        elif args.command == "build":\n'
        "            receipt = build_oci_layout(\n"
        "                lock, raw, args.repo_root, args.artifact_store, args.output\n"
        "            )\n",
        '        elif args.command == "build":\n'
        "            receipt = build_oci_layout(\n"
        "                lock,\n"
        "                raw,\n"
        "                args.repo_root,\n"
        "                args.artifact_store,\n"
        "                args.output,\n"
        "                launcher_binary=read_launcher(args.launcher),\n"
        "            )\n",
        1,
    ),
    (
        "        else:\n"
        "            receipt = verify_oci_layout(\n"
        "                lock, raw, args.repo_root, args.artifact_store, args.layout\n"
        "            )\n",
        "        else:\n"
        "            receipt = verify_oci_layout(\n"
        "                lock,\n"
        "                raw,\n"
        "                args.repo_root,\n"
        "                args.artifact_store,\n"
        "                args.layout,\n"
        "                launcher_binary=read_launcher(args.launcher),\n"
        "            )\n",
        1,
    ),
)

# Ubuntu 24.04 is merged-/usr: ``/lib`` is a symlink to ``usr/lib``, so a member
# arriving as ``lib/modules/x`` and one arriving as ``usr/lib/modules/x`` name the
# same file. The sealed lock already says so -- it derives ``/lib -> usr/lib`` --
# and these are the roots that statement implies.
USRMERGE_ROOTS = {
    "bin": "usr/bin",
    "lib": "usr/lib",
    "lib64": "usr/lib64",
    "sbin": "usr/sbin",
}


class BootProjectionError(RuntimeError):
    """The boot source lock does not satisfy what this projection guards."""


def _closure_exception() -> dict[str, Any]:
    """The sealed record of where this closure departs from the frozen builder."""

    try:
        document = json.loads(CLOSURE_EXCEPTION_PATH.read_text(encoding="utf-8"))
    except OSError as exc:
        raise BootProjectionError("boot closure exception file is unreadable") from exc
    if document.get("release") != CLOSURE_EXCEPTION_RELEASE:
        raise BootProjectionError("boot closure exception release differs")
    return document


def _launcher_seal() -> dict[str, Any]:
    """The sealed launcher block, read from the frozen producer authority.

    ``/usr/libexec/boole`` is a declared closure root that nothing fills: the
    launcher is a build product, so it arrives neither as a deb nor as a tracked
    repository file.  The authority is what says how it may arrive instead, and
    reading it here rather than restating the digest keeps one copy of the fact.
    """

    try:
        document = json.loads(PRODUCER_AUTHORITY_PATH.read_text(encoding="utf-8"))
    except OSError as exc:
        raise BootProjectionError("boot producer authority is unreadable") from exc
    launcher = document.get("launcher")
    if not isinstance(launcher, dict):
        raise BootProjectionError("boot producer authority seals no launcher")
    if launcher.get("acquisition") != LAUNCHER_ACQUISITION:
        raise BootProjectionError("boot producer authority changed how the launcher arrives")
    aborts = {
        row.get("id")
        for row in document.get("abortConditions", [])
        if isinstance(row, dict)
    }
    if ABORT_LAUNCHER_DIGEST_MISMATCH not in aborts:
        raise BootProjectionError("boot producer authority dropped the launcher abort condition")
    digest = launcher.get("sha256")
    size = launcher.get("sizeBytes")
    path = launcher.get("guestLogicalPath")
    if not isinstance(digest, str) or len(digest) != 64:
        raise BootProjectionError("sealed launcher digest is unusable")
    if not isinstance(size, int) or size <= 0:
        raise BootProjectionError("sealed launcher size is unusable")
    if not isinstance(path, str) or not path.startswith("/"):
        raise BootProjectionError("sealed launcher guest path is unusable")
    return launcher


def sha256_hex(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def launcher_entry(
    binary: bytes,
    *,
    sha256: Optional[str] = None,
    size: Optional[int] = None,
) -> dict[str, Any]:
    """The rebuilt launcher, checked against the seal, placed on its guest path.

    ``rebuild-and-match-seal`` is the whole rule: a launcher that does not match
    stops the build with the authority's own ``launcher-digest-mismatch`` rather
    than being written into the tree and noticed later, or not at all.
    """

    expected_digest = LAUNCHER_SHA256 if sha256 is None else sha256
    expected_size = LAUNCHER_SIZE_BYTES if size is None else size
    if not binary:
        raise RootfsBuildError(
            f"{ABORT_LAUNCHER_DIGEST_MISMATCH}: the rebuilt launcher is empty"
        )
    if len(binary) != expected_size:
        raise RootfsBuildError(
            f"{ABORT_LAUNCHER_DIGEST_MISMATCH}: the rebuilt launcher is "
            f"{len(binary)} bytes, the seal says {expected_size}"
        )
    actual = sha256_hex(binary)
    if actual != expected_digest:
        raise RootfsBuildError(
            f"{ABORT_LAUNCHER_DIGEST_MISMATCH}: the rebuilt launcher hashes to "
            f"{actual}, the seal says {expected_digest}"
        )
    return {
        "path": LAUNCHER_GUEST_PATH.lstrip("/"),
        "kind": "file",
        "mode": LAUNCHER_MODE,
        "uid": 0,
        "gid": 0,
        "raw": binary,
    }


def read_launcher(path: Optional[pathlib.Path]) -> Optional[bytes]:
    """The rebuilt launcher named on the command line, or nothing.

    Nothing is a real answer: builds that place no launcher are the ones whose
    bytes are already known to agree, and the difference between "no launcher"
    and "an empty launcher" is the difference between that build and a refusal.
    A launcher that was named but cannot be read is neither, so it stops here.
    """

    if path is None:
        return None
    try:
        return path.read_bytes()
    except OSError as exc:
        raise RootfsBuildError(
            f"{ABORT_LAUNCHER_DIGEST_MISMATCH}: cannot read the rebuilt launcher: {path}"
        ) from exc


def _load_metadata_exceptions(document: dict[str, Any]) -> dict[str, tuple[int, int, int]]:
    """The eleven members the sealed exception names, keyed by logical path."""

    rows = document.get("metadataMembers")
    if not isinstance(rows, list) or not rows:
        raise BootProjectionError("boot closure exception names no metadata members")
    if document.get("memberCount") != len(rows):
        raise BootProjectionError("boot closure exception member count differs")
    table: dict[str, tuple[int, int, int]] = {}
    for row in rows:
        path = row.get("path")
        if not isinstance(path, str) or not path or path in table:
            raise BootProjectionError(f"boot closure exception path is unusable: {path!r}")
        mode = int(row["originalMode"], 8)
        if row["originalUid"] == 0 and row["originalGid"] == 0 and not mode & SET_ID_BITS:
            raise BootProjectionError(f"boot closure exception row changes nothing: {path}")
        if int(row["normalizedMode"], 8) != mode & NORMALIZED_MODE_MASK:
            raise BootProjectionError(
                f"boot closure exception row does not drop the set-id bits: {path}"
            )
        if row["normalizedUid"] != 0 or row["normalizedGid"] != 0:
            raise BootProjectionError(
                f"boot closure exception row does not normalise to root:root: {path}"
            )
        table[path] = (row["originalUid"], row["originalGid"], mode)
    return table


def _load_dangling_symlinks(document: dict[str, Any]) -> dict[str, str]:
    """The named symlinks whose target no package in this closure provides."""

    rows = document.get("danglingSymlinks")
    if not isinstance(rows, list):
        raise BootProjectionError("boot closure exception names no dangling symlinks")
    if document.get("danglingSymlinkCount") != len(rows):
        raise BootProjectionError("boot closure exception dangling count differs")
    table: dict[str, str] = {}
    for row in rows:
        path, resolved = row.get("path"), row.get("resolvedTarget")
        if not isinstance(path, str) or not path or path in table:
            raise BootProjectionError(f"boot closure exception path is unusable: {path!r}")
        if not isinstance(resolved, str) or not resolved:
            raise BootProjectionError(f"boot closure exception target is unusable: {path}")
        if not row.get("reason"):
            raise BootProjectionError(f"boot closure exception row gives no reason: {path}")
        table[path] = resolved
    return table


def _load_runtime_roots(document: dict[str, Any]) -> frozenset[str]:
    roots = document.get("runtimeFilesystemRoots")
    if not isinstance(roots, list) or not roots:
        raise BootProjectionError("boot closure exception names no runtime filesystems")
    return frozenset(roots)


def _load_derived_usrmerge_symlinks(document: dict[str, Any]) -> tuple[dict[str, Any], ...]:
    """The merged-/usr symlinks no package in this closure ships."""

    rows = document.get("derivedUsrmergeSymlinks")
    if not isinstance(rows, list) or not rows:
        raise BootProjectionError("boot closure exception names no derived symlinks")
    for row in rows:
        path = row.get("logicalPath", "")
        if row.get("kind") != "symlink" or not path.startswith("/"):
            raise BootProjectionError(f"derived usrmerge entry is not a symlink: {path!r}")
        if row.get("target") != f"usr{path}":
            raise BootProjectionError(f"derived usrmerge entry does not point into /usr: {path}")
        if path.lstrip("/") not in USRMERGE_ROOTS:
            raise BootProjectionError(f"derived usrmerge entry is not a merged root: {path}")
    return tuple(rows)


_LAUNCHER_SEAL = _launcher_seal()
LAUNCHER_SHA256 = _LAUNCHER_SEAL["sha256"]
LAUNCHER_SIZE_BYTES = _LAUNCHER_SEAL["sizeBytes"]
LAUNCHER_GUEST_PATH = _LAUNCHER_SEAL["guestLogicalPath"]

_CLOSURE_EXCEPTION = _closure_exception()
METADATA_EXCEPTIONS = _load_metadata_exceptions(_CLOSURE_EXCEPTION)
DANGLING_SYMLINKS = _load_dangling_symlinks(_CLOSURE_EXCEPTION)
RUNTIME_FILESYSTEM_ROOTS = _load_runtime_roots(_CLOSURE_EXCEPTION)
DERIVED_USRMERGE_SYMLINKS = _load_derived_usrmerge_symlinks(_CLOSURE_EXCEPTION)


def _metadata_exception(path: str, member: Any) -> bool:
    """True only for a named member arriving exactly as the exception recorded it."""

    expected = METADATA_EXCEPTIONS.get(path)
    if expected is None:
        return False
    return (member.uid, member.gid, member.mode & 0o7777) == expected


def _dangling_allowed(path: str, resolved: str) -> bool:
    """A symlink target the image is not expected to carry.

    Two cases, and only two.  ``/dev``, ``/proc``, ``/run``, ``/sys`` and
    ``/tmp`` are filled in at boot by the kernel and by PID 1, so nothing under
    them can exist in an image and a mask pointing at ``/dev/null`` is doing its
    job.  Everything else has to be named, because a symlink that quietly points
    at nothing is exactly what this check is for.
    """

    if resolved.split("/", 1)[0] in RUNTIME_FILESYSTEM_ROOTS:
        return True
    return DANGLING_SYMLINKS.get(path) == resolved


def runtime_mount_point_entries() -> dict[str, dict[str, Any]]:
    """The five directories the kernel filesystems are mounted on.

    Read from the audit record rather than listed here, so the builder and the
    check that reads a produced tree cannot end up with different lists.
    """

    return mount_points.mount_point_entries()


def usrmerge_path(path: str) -> str:
    """``lib/modules/x`` and ``usr/lib/modules/x`` are one path on merged-/usr."""

    head, _, rest = path.partition("/")
    root = USRMERGE_ROOTS.get(head)
    if root is None:
        return path
    return f"{root}/{rest}" if rest else root


def _usrmerge(entries: dict[str, Any], context: str) -> dict[str, Any]:
    """Relocate a package's members onto the merged-/usr layout the lock declares.

    Two members that land on one path are still a collision; this moves paths,
    it does not decide who wins.  The frozen builder's rule is reused verbatim --
    identical entries collapse, directories collapse when their modes agree, and
    anything else stops the build.
    """

    merged: dict[str, Any] = {}
    for path, entry in entries.items():
        moved = usrmerge_path(path)
        entry = dict(entry, path=moved)
        if entry["kind"] == "symlink":
            entry["resolvedTarget"] = usrmerge_path(entry["resolvedTarget"])
        elif entry["kind"] == "hardlink":
            entry["target"] = usrmerge_path(entry["target"])
        existing = merged.get(moved)
        if existing is None:
            merged[moved] = entry
        elif existing != entry:
            if existing["kind"] == entry["kind"] == "directory" and existing["mode"] == entry["mode"]:
                continue
            raise RootfsBuildError(f"usrmerge path collision differs ({context}): {moved}")
    return merged


def _derived_source() -> str:
    raw = ARM64_BUILDER.read_bytes()
    if hashlib.sha256(raw).hexdigest() != ARM64_BUILDER_SHA256:
        raise BootProjectionError("frozen arm64 rootfs builder bytes differ")
    source = arm64._derived_source()
    for old, new, expected_count in REPLACEMENTS:
        actual_count = source.count(old)
        if actual_count != expected_count:
            raise BootProjectionError(
                f"boot builder projection count differs: {actual_count} != {expected_count}"
            )
        source = source.replace(old, new)
    return source


# ``BUILDER_SOURCE_BYTES`` reads ``__file__``, and the sealed lock pins the arm64
# builder's digest, so the projected namespace has to name that file.  This
# module's own contribution is recorded below as ``BOOT_PROJECTION_SHA256``.
_IMPL: dict[str, Any] = {
    "__name__": "scripts.native_shadow_rootfs_builder_boot_arm64_v1_impl",
    "__file__": str(ARM64_BUILDER),
    "__package__": "scripts",
}
exec(compile(_derived_source(), __file__ + "<derived>", "exec"), _IMPL)

_IMPL["EXPECTED_AUTHORITY_FILES"] = dict(
    _IMPL["EXPECTED_AUTHORITY_FILES"], **BOOT_AUTHORITY_FILES
)
_IMPL["EXPECTED_PROVENANCE_CLOSURE_ROOTS"] = dict(
    _IMPL["EXPECTED_PROVENANCE_CLOSURE_ROOTS"], **BOOT_PROVENANCE_CLOSURE_ROOTS
)
_IMPL["REQUIRED_PROVENANCE_CLOSURES"] = tuple(
    sorted(_IMPL["EXPECTED_PROVENANCE_CLOSURE_ROOTS"])
)
_IMPL["_metadata_exception"] = _metadata_exception
_IMPL["METADATA_EXCEPTIONS"] = METADATA_EXCEPTIONS
_IMPL["_usrmerge"] = _usrmerge
_IMPL["USRMERGE_ROOTS"] = USRMERGE_ROOTS
_IMPL["_dangling_allowed"] = _dangling_allowed
_IMPL["runtime_mount_point_entries"] = runtime_mount_point_entries
_IMPL["DANGLING_SYMLINKS"] = DANGLING_SYMLINKS
_IMPL["launcher_entry"] = launcher_entry
_IMPL["read_launcher"] = read_launcher

RootfsBuildError = _IMPL["RootfsBuildError"]
BUILDER_SHA256 = _IMPL["BUILDER_SHA256"]
LOCK_SCHEMA = _IMPL["LOCK_SCHEMA"]
canonical_json = _IMPL["canonical_json"]
load_json_exact = _IMPL["load_json_exact"]
build_oci_layout = _IMPL["build_oci_layout"]
verify_oci_layout = _IMPL["verify_oci_layout"]
directory_digest = _IMPL["directory_digest"]

BOOT_PROJECTION_SHA256 = hashlib.sha256(
    pathlib.Path(__file__).resolve().read_bytes()
).hexdigest()


def concrete_architectures(lock: dict[str, Any]) -> tuple[str, ...]:
    """Every architecture in the closure that is not ``all``."""

    try:
        packages = lock["ubuntu"]["packages"]
    except (KeyError, TypeError) as exc:
        raise BootProjectionError("boot source lock has no Ubuntu package closure") from exc
    found = {
        package.get("architecture")
        for package in packages
        if package.get("architecture") != ARCHITECTURE_INDEPENDENT
    }
    if None in found:
        raise BootProjectionError("an Ubuntu package in the closure declares no architecture")
    return tuple(sorted(found))


def assert_single_architecture(lock: dict[str, Any]) -> str:
    """``:any`` equals the bare name only while one architecture is present."""

    found = concrete_architectures(lock)
    if len(found) != 1:
        raise BootProjectionError(
            "boot closure must hold exactly one concrete architecture, found: "
            + (", ".join(found) or "none")
        )
    declared = lock.get("platform", {}).get("debArchitecture")
    if declared != found[0]:
        raise BootProjectionError(
            f"closure architecture {found[0]!r} differs from platform {declared!r}"
        )
    return found[0]


def normalized_runtime_lock(
    lock: dict[str, Any],
) -> tuple[dict[str, Any], bytes, dict[str, Any]]:
    """Sort each closure's logical roots and keep both orders on the record."""

    result = copy.deepcopy(lock)
    closures = result.get("closureRoots")
    if not isinstance(closures, list) or not closures:
        raise BootProjectionError("boot source lock has no provenance closures")
    rows: list[dict[str, Any]] = []
    for closure in closures:
        declared = closure.get("logicalRoots")
        if not isinstance(declared, list) or not declared:
            raise BootProjectionError("a provenance closure has no logical roots")
        if len(set(declared)) != len(declared):
            raise BootProjectionError(
                f"duplicate logical root in closure: {closure.get('name')!r}"
            )
        normalized = sorted(declared)
        closure["logicalRoots"] = normalized
        rows.append(
            {
                "declaredOrder": list(declared),
                "name": closure.get("name"),
                "normalizedOrder": list(normalized),
                "reordered": list(declared) != normalized,
            }
        )
    derived = result.get("derivedEntries")
    if not isinstance(derived, list) or not derived:
        raise BootProjectionError("boot source lock has no derived entries")
    existing = {row.get("logicalPath"): row for row in derived}
    added: list[str] = []
    for row in DERIVED_USRMERGE_SYMLINKS:
        path = row["logicalPath"]
        present = existing.get(path)
        if present is None:
            derived.append(dict(row))
            added.append(path)
        elif present != row:
            raise BootProjectionError(
                f"boot source lock already derives {path} differently: {present.get('target')!r}"
            )
    derived.sort(key=lambda row: row["logicalPath"])
    record = {
        "addedDerivedEntries": added,
        "closureRoots": rows,
        "reorderedClosures": [row["name"] for row in rows if row["reordered"]],
    }
    return result, canonical_json(result), record


def validate_source_lock(
    lock: dict[str, Any],
    lock_raw: bytes,
    repository_root: pathlib.Path,
    artifact_store: Optional[pathlib.Path],
    *,
    require_complete: bool,
) -> dict[str, Any]:
    """Guard what the widening depends on, then hand the frozen builder the lock."""

    for closure in lock.get("closureRoots") or []:
        roots = closure.get("logicalRoots") or []
        if list(roots) != sorted(set(roots)):
            raise BootProjectionError(
                f"closure {closure.get('name')!r} is not sorted and unique; "
                "run normalized_runtime_lock first so both orders stay on the record"
            )
    assert_single_architecture(lock)
    return _IMPL["validate_source_lock"](
        lock,
        lock_raw,
        repository_root,
        artifact_store,
        require_complete=require_complete,
    )


def __getattr__(name: str) -> Any:
    try:
        return _IMPL[name]
    except KeyError as exc:
        raise AttributeError(name) from exc


def main(argv: Optional[list[str]] = None) -> int:
    return _IMPL["main"](argv)


if __name__ == "__main__":
    raise SystemExit(main())
