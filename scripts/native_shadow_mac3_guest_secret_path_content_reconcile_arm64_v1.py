"""Reconciles the preserved ext4 image's paths with its historical raw hits.

This successor deliberately does not import or edit the historical raw scanner.
The old scan is retained as a raw-offset inventory.  This module adds the
missing logical view and refuses if the two views do not conserve every hit.

The filesystem reader is an exact-shape consumer for this sealed ext4 image,
not a general-purpose ext4 implementation.  A real run pins the checked-in
reader code, Python runtime and source-assembly tools, verifies the sealed image
before and after, and records only paths, digests, offsets and classifications
-- never the bytes surrounding a possible credential.
"""

from __future__ import annotations

import argparse
import collections
import dataclasses
import gc
import hashlib
import json
import os
import pathlib
import stat
import string
import sys
from typing import Callable, Iterable, List, Mapping, Sequence, Tuple


BLOCK_BYTES = 4096
JOURNAL_PATH = "[SYS]/Journal"
RUNTIME_PREFIX = "var/lib/boole/native-shadow/runtime-rootfs/"
GENERATED_PATHS = frozenset(
    {
        JOURNAL_PATH,
        "lost+found",
        "usr/libexec/boole",
        "usr/libexec/boole/boole-native-shadow-launcher",
    }
)

REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
CONTAINMENT = REPO_ROOT / "native/containment"
HISTORICAL_SCAN_PATH = (
    CONTAINMENT / "native-shadow-mac3-guest-secret-absence-scan-arm64-v1.json"
)
CORRECTION_PATH = (
    CONTAINMENT
    / "native-shadow-mac3-guest-secret-absence-raw-scan-correction-arm64-v1.json"
)
QUALIFICATION_PATH = (
    CONTAINMENT / "native-shadow-mac3-closed-local-boot-qualification-arm64-v3.json"
)
PRESERVATION_PATH = (
    CONTAINMENT / "native-shadow-mac3-successor-image-preservation-arm64-v4.json"
)
PRODUCTION_RESULT_PATH = (
    CONTAINMENT / "native-shadow-mac3-successor-image-production-result-arm64-v4.json"
)
PREFLIGHT_RESULT_PATH = (
    CONTAINMENT / "native-shadow-mac3-successor-preflight-result-arm64-v1.json"
)

AUTHORITY_SHA256 = {
    HISTORICAL_SCAN_PATH.relative_to(REPO_ROOT).as_posix(): (
        "feeb6264ff062af9813d6a05c44a2bca9ddc9d4f9ae96d33a51bd595c9fd8e2f"
    ),
    CORRECTION_PATH.relative_to(REPO_ROOT).as_posix(): (
        "10b6cf3d69607c226bf198fbadaed6c7c354aa36735a159ae5ecc8647a20dca7"
    ),
    QUALIFICATION_PATH.relative_to(REPO_ROOT).as_posix(): (
        "74b9507932b4eda97c89753f642bac579593b034b3e9eff24bb5b056c09079a6"
    ),
    PRESERVATION_PATH.relative_to(REPO_ROOT).as_posix(): (
        "2ff7a3a30513092495a2d8b67555b4e974ef75af47de08acfe8c049063549126"
    ),
    PRODUCTION_RESULT_PATH.relative_to(REPO_ROOT).as_posix(): (
        "0faddb098503bbf17bf94ec36148e6ccf1af8fa1335ba0e5e9c79cd9d573b7dd"
    ),
    PREFLIGHT_RESULT_PATH.relative_to(REPO_ROOT).as_posix(): (
        "be4a84e1c058fa25804cfade07727e35613369f58b0307182b93f24a4ecfb071"
    ),
    "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v2.json": (
        "1a1a1df9b61795a46e82f392bda82d29c0cbde0473a11efd1f1cbd7993a85a9f"
    ),
    "native/containment/native-shadow-runtime-rootfs-source-lock-arm64-v1.json": (
        "829ca81d321d412746cce7a62d59d7e538c394b92c1b6a9a966f3016b73cede0"
    ),
}

EXPECTED_TREE_MODULE_SHA256 = {
    "scripts.native_shadow_boot_staging_measure_arm64_v1": (
        "scripts/native_shadow_boot_staging_measure_arm64_v1.py",
        "d7deacc81e1262b8bd6c9b525a2784850db55c7d93425458243daf5d45fc75b1",
    ),
    "scripts.native_shadow_rootfs_builder_boot_arm64_v1": (
        "scripts/native_shadow_rootfs_builder_boot_arm64_v1.py",
        "a5dd54198878473c162ec306fbccd6edac8b22f036d9cf84d244b5f010f96d87",
    ),
    "scripts.native_shadow_rootfs_builder_boot_arm64_v3": (
        "scripts/native_shadow_rootfs_builder_boot_arm64_v3.py",
        "93bd05d06e43cc69f325036d204b7b57721e358dd5c5d5990227ef88c4de8c39",
    ),
    "scripts.native_shadow_rootfs_portable_boot_arm64_v2": (
        "scripts/native_shadow_rootfs_portable_boot_arm64_v2.py",
        "15f88cf286879ae30aae10bb7819aefea91095a819d96c2634ee9ecc4ea2f305",
    ),
}

SEALED_IMAGE_SHA256 = "51410d8113c28d6cd28c7b6c7578076226d5e19b6629649199af7b7f86540a1c"
SEALED_IMAGE_BYTES = 2_035_625_984
SEALED_STAGING_ENTRIES = 17_674
SEALED_STAGING_PAYLOAD_BYTES = 1_771_449_867
SEALED_STAGING_PATH_SHA256 = (
    "a342a1a59178af546c0c0d212aecd770d02333bf9c289a11b42627b271693736"
)
SEALED_COMPLETE_PATH_BYTES = 970_123
SEALED_COMPLETE_PATH_SHA256 = (
    "a6a7d0e858e62ca2f686b1d13ade4e10e9e97a49bbea582a2a76217236923fb6"
)
READ_CHUNK_BYTES = 4 * 1024 * 1024


class RefusedError(RuntimeError):
    """Raised when continuing would turn incomplete evidence into a verdict."""


@dataclasses.dataclass(frozen=True)
class Entry:
    path: str
    kind: str
    size: int
    mode: str
    uid: int
    gid: int
    symlink_target: str


@dataclasses.dataclass(frozen=True)
class PhysicalHit:
    marker: str
    raw_offset: int
    block_sha256: str
    block_position: int


@dataclasses.dataclass(frozen=True)
class LogicalOwner:
    inode: int
    paths: Tuple[str, ...]


@dataclasses.dataclass(frozen=True)
class ObservedContent:
    path: str
    kind: str
    sha256: str
    symlink_target: str


@dataclasses.dataclass(frozen=True)
class SealedContent:
    kind: str
    sha256: str
    symlink_target: str
    source: str


@dataclasses.dataclass(frozen=True)
class ExpectedEntry:
    path: str
    kind: str
    mode: int
    uid: int
    gid: int
    size: int
    sha256: str
    symlink_target: str


@dataclasses.dataclass(frozen=True)
class Marker:
    identifier: str
    tier: str
    needle: bytes


@dataclasses.dataclass(frozen=True)
class RawOccurrence:
    marker: str
    tier: str
    raw_offset: int


@dataclasses.dataclass(frozen=True)
class LogicalOccurrence:
    marker: str
    raw_offset: int
    inode: int
    file_offset: int
    paths: tuple[str, ...]


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def canonical_json(value: object) -> bytes:
    return (
        json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
        + "\n"
    ).encode("utf-8")


def _file_binding(path: pathlib.Path, *, name: str | None = None) -> dict:
    resolved = pathlib.Path(path).resolve(strict=True)
    flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)
    descriptor = os.open(resolved, flags)
    try:
        info = os.fstat(descriptor)
        if not stat.S_ISREG(info.st_mode) or info.st_size <= 0:
            raise RefusedError("implementation binding is not a regular file")
        digest = _hash_descriptor(descriptor, info.st_size)
    finally:
        os.close(descriptor)
    row = {"sha256": digest, "sizeBytes": info.st_size}
    if name is not None:
        row["name"] = name
    return row


def implementation_bindings(
    repository_root: pathlib.Path,
    *,
    gpgv: pathlib.Path,
    zstd: pathlib.Path,
) -> dict:
    scripts = []
    for relative in (
        "scripts/native_shadow_ext4_readonly_owner_map_arm64_v1.py",
        "scripts/native_shadow_mac3_guest_secret_path_content_reconcile_arm64_v1.py",
    ):
        row = _file_binding(repository_root / relative)
        row["path"] = relative
        scripts.append(row)
    python = _file_binding(pathlib.Path(sys.executable))
    python.update(
        {
            "implementation": sys.implementation.name.upper()
            if sys.implementation.name != "cpython"
            else "CPython",
            "version": "%d.%d.%d"
            % (
                sys.version_info.major,
                sys.version_info.minor,
                sys.version_info.micro,
            ),
            "executableBytes": python.pop("sizeBytes"),
            "executableSha256": python.pop("sha256"),
        }
    )
    return {
        "scripts": scripts,
        "pythonRuntime": python,
        "sourceAssemblyTools": [
            _file_binding(gpgv, name="gpgv"),
            _file_binding(zstd, name="zstd"),
        ],
    }


def _expected_entry(path: str, row: Mapping[str, object]) -> ExpectedEntry:
    validate_logical_paths([path])
    if row.get("path") != path:
        raise RefusedError("builder entry key and path differ")
    kind = row.get("kind")
    mode = row.get("mode")
    uid = row.get("uid")
    gid = row.get("gid")
    if (
        kind not in ("directory", "file", "symlink")
        or not isinstance(mode, int)
        or isinstance(mode, bool)
        or not 0 <= mode <= 0o7777
        or not isinstance(uid, int)
        or isinstance(uid, bool)
        or uid < 0
        or not isinstance(gid, int)
        or isinstance(gid, bool)
        or gid < 0
    ):
        raise RefusedError("builder entry metadata differs from the sealed shape")
    if kind == "file":
        raw = row.get("raw")
        if not isinstance(raw, bytes):
            raise RefusedError("builder file has no exact bytes")
        size = len(raw)
        digest = sha256_bytes(raw)
        target = ""
    elif kind == "symlink":
        target = row.get("target")
        if not isinstance(target, str) or not target:
            raise RefusedError("builder symlink has no exact target")
        encoded = target.encode("utf-8")
        size = len(encoded)
        digest = ""
    else:
        if "raw" in row or "target" in row:
            raise RefusedError("builder directory carries content")
        size = 0
        digest = ""
        target = ""
    return ExpectedEntry(path, kind, mode, uid, gid, size, digest, target)


def complete_expected_table(
    assembled_entries: Mapping[str, Mapping[str, object]],
    *,
    launcher_path: str,
    launcher_size: int,
    launcher_sha256: str,
    enforce_sealed_counts: bool = True,
) -> dict[str, ExpectedEntry]:
    """Finish the production staging table with the two external generated rows.

    The arm64 launcher bytes exist only on the Linux producer, and ``lost+found``
    is created by the filesystem writer.  Their identities are sealed outside
    the in-memory staging table, so they are added from those seals rather than
    read back from the image under test.
    """

    if not isinstance(assembled_entries, Mapping) or not assembled_entries:
        raise RefusedError("assembled expected tree is empty")
    table = {
        path: _expected_entry(path, row)
        for path, row in assembled_entries.items()
    }
    validate_logical_paths(list(table))
    validate_logical_paths([launcher_path])
    launcher_parent = str(pathlib.PurePosixPath(launcher_path).parent)
    if (
        launcher_path in table
        or launcher_parent in table
        or "lost+found" in table
        or not isinstance(launcher_size, int)
        or isinstance(launcher_size, bool)
        or launcher_size <= 0
    ):
        raise RefusedError("generated expected rows overlap the staging tree")
    _validate_content_digest(launcher_sha256)
    table[launcher_parent] = ExpectedEntry(
        launcher_parent, "directory", 0o755, 0, 0, 0, "", ""
    )
    table[launcher_path] = ExpectedEntry(
        launcher_path,
        "file",
        0o755,
        0,
        0,
        launcher_size,
        launcher_sha256.lower(),
        "",
    )
    table["lost+found"] = ExpectedEntry(
        "lost+found", "directory", 0o700, 0, 0, 0, "", ""
    )
    table = dict(sorted(table.items(), key=lambda item: item[0].encode("utf-8")))
    if enforce_sealed_counts:
        counts = collections.Counter(row.kind for row in table.values())
        if len(table) != 17_677 or counts != {
            "directory": 1_738,
            "file": 15_102,
            "symlink": 837,
        }:
            raise RefusedError("expected tree counts differ from the sealed assembly")
    return table


def expected_table_manifest(table: Mapping[str, ExpectedEntry]) -> dict:
    if not table or list(table) != sorted(table, key=lambda value: value.encode("utf-8")):
        raise RefusedError("expected table is not in canonical path order")
    rows = []
    counts = collections.Counter()
    path_payload = bytearray()
    for path, entry in table.items():
        if not isinstance(entry, ExpectedEntry) or entry.path != path:
            raise RefusedError("expected table entry identity differs")
        counts[entry.kind] += 1
        path_payload.extend(path.encode("utf-8") + b"\n")
        rows.append(
            {
                "gid": entry.gid,
                "kind": entry.kind,
                "mode": "%04o" % entry.mode,
                "path": path,
                "sha256": entry.sha256,
                "sizeBytes": entry.size,
                "symlinkTarget": entry.symlink_target,
                "uid": entry.uid,
            }
        )
    raw = canonical_json(rows)
    return {
        "entries": len(rows),
        "byKind": dict(sorted(counts.items())),
        "rowsBytes": len(raw),
        "rowsSha256": sha256_bytes(raw),
        "pathManifestBytes": len(path_payload),
        "pathManifestSha256": sha256_bytes(bytes(path_payload)),
    }


def resolve_preserved_root_disk(
    qualification: Mapping[str, object],
    preservation: Mapping[str, object],
) -> dict:
    """Resolve the historical target from replica-1 records, never a host path."""

    subject = qualification.get("subject")
    images = subject.get("images") if isinstance(subject, Mapping) else None
    if not isinstance(images, list):
        raise RefusedError("qualification image table is absent")
    selected = [
        row
        for row in images
        if isinstance(row, Mapping)
        and row.get("name") == "guest-root-disk"
        and row.get("replica") == 1
        and row.get("used") is True
    ]
    if len(selected) != 1:
        raise RefusedError("qualification does not name one replica-1 root disk")
    image = selected[0]
    relative = image.get("archivePath")
    parts = pathlib.PurePosixPath(relative).parts if isinstance(relative, str) else ()
    if (
        not relative
        or relative.startswith("/")
        or any(part in ("", ".", "..") for part in parts)
    ):
        raise RefusedError("qualification archive path is unsafe")
    rows = preservation.get("preservedFiles")
    matches = [
        row
        for row in rows or []
        if isinstance(row, Mapping) and row.get("path") == relative
    ]
    if len(matches) != 1:
        raise RefusedError("preservation has no unique root-disk row")
    preserved = matches[0]
    if (
        image.get("bytes") != preserved.get("bytes")
        or image.get("sha256") != preserved.get("sha256")
        or not isinstance(image.get("bytes"), int)
        or isinstance(image.get("bytes"), bool)
        or image.get("bytes") <= 0
    ):
        raise RefusedError("qualification and preservation root-disk rows differ")
    _validate_content_digest(image.get("sha256"))
    return {
        "relativePath": relative,
        "sizeBytes": image["bytes"],
        "sha256": image["sha256"].lower(),
        "replica": 1,
    }


def frozen_markers(
    rows: Sequence[Mapping[str, object]],
    *,
    private_host_home: bytes | None,
) -> tuple[Marker, ...]:
    """Materialise the historical marker table without consulting HOME."""

    if not isinstance(rows, Sequence) or not rows:
        raise RefusedError("historical marker table is absent")
    markers = []
    seen = set()
    private_rows = 0
    for row in rows:
        identifier = row.get("id") if isinstance(row, Mapping) else None
        tier = row.get("tier") if isinstance(row, Mapping) else None
        disclosed = row.get("needle") if isinstance(row, Mapping) else None
        if (
            not isinstance(identifier, str)
            or not identifier
            or identifier in seen
            or tier not in ("host-identity", "secret-shape")
        ):
            raise RefusedError("historical marker identity differs")
        seen.add(identifier)
        if disclosed is None:
            private_rows += 1
            if identifier != "host-home-directory" or not isinstance(
                private_host_home, bytes
            ):
                raise RefusedError("private historical marker was not supplied")
            needle = private_host_home
        elif isinstance(disclosed, str):
            needle = disclosed.encode("utf-8")
        else:
            raise RefusedError("historical marker bytes are not reconstructable")
        if (
            not needle
            or row.get("needleBytes") != len(needle)
            or row.get("needleSha256") != sha256_bytes(needle)
        ):
            raise RefusedError("historical marker digest or length differs")
        markers.append(Marker(identifier, tier, needle))
    if private_rows != 1:
        raise RefusedError("historical marker table has the wrong private-input count")
    return tuple(markers)


def producer_build_home_marker(preflight: Mapping[str, object]) -> Marker:
    provenance = preflight.get("provenance")
    repository = (
        provenance.get("repositoryRoot") if isinstance(provenance, Mapping) else None
    )
    artifact_store = (
        provenance.get("artifactStore") if isinstance(provenance, Mapping) else None
    )
    if not isinstance(repository, str) or not isinstance(artifact_store, str):
        raise RefusedError("sealed preflight has no producer path evidence")
    parts = pathlib.PurePosixPath(repository).parts
    if len(parts) < 4 or parts[:2] != ("/", "home"):
        raise RefusedError("producer repository is not under one sealed Linux home")
    home = pathlib.PurePosixPath(*parts[:3]).as_posix()
    if (
        repository == home
        or not repository.startswith(home + "/")
        or not artifact_store.startswith(repository + "/")
    ):
        raise RefusedError("producer path evidence does not share one sealed root")
    return Marker(
        "producer-build-home-directory",
        "producer-build-provenance",
        home.encode("utf-8"),
    )


def _read_json_exact(path: pathlib.Path) -> tuple[dict, bytes]:
    try:
        raw = pathlib.Path(path).read_bytes()
        value = json.loads(raw.decode("utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise RefusedError("authority record is absent or malformed: %s" % path) from error
    if not isinstance(value, dict):
        raise RefusedError("authority record root is not an object: %s" % path)
    return value, raw


def verify_authority_files(repository_root: pathlib.Path) -> list[dict]:
    repository_root = pathlib.Path(repository_root)
    rows = []
    for relative, expected in sorted(AUTHORITY_SHA256.items()):
        path = repository_root / relative
        try:
            raw = path.read_bytes()
        except OSError as error:
            raise RefusedError("sealed authority file is absent: %s" % relative) from error
        observed = sha256_bytes(raw)
        if observed != expected:
            raise RefusedError("sealed authority file drifted: %s" % relative)
        rows.append({"path": relative, "sha256": observed, "sizeBytes": len(raw)})
    return rows


def verify_expected_tree_builder_chain(
    repository_root: pathlib.Path,
    preflight: Mapping[str, object],
) -> list[dict]:
    provenance = preflight.get("provenance")
    recorded = provenance.get("modules") if isinstance(provenance, Mapping) else None
    if not isinstance(recorded, Mapping):
        raise RefusedError("sealed preflight has no expected-tree module table")
    rows = []
    for module_name, (relative, expected) in sorted(
        EXPECTED_TREE_MODULE_SHA256.items()
    ):
        if recorded.get(module_name) != expected:
            raise RefusedError("sealed preflight module digest differs: %s" % module_name)
        row = _file_binding(repository_root / relative)
        if row["sha256"] != expected:
            raise RefusedError("expected-tree builder drifted: %s" % relative)
        row["path"] = relative
        rows.append(row)
    return rows


def assemble_expected_tree(
    repository_root: pathlib.Path,
    artifact_store: pathlib.Path,
    gpgv: pathlib.Path,
    zstd: pathlib.Path,
) -> tuple[dict[str, ExpectedEntry], dict, list[dict]]:
    """Rebuild the expected table from sealed locks, never from the image."""

    repository_root = pathlib.Path(repository_root).resolve()
    artifact_store = pathlib.Path(artifact_store).resolve()
    verify_authority_files(repository_root)
    preflight, _preflight_raw = _read_json_exact(
        repository_root / PREFLIGHT_RESULT_PATH.relative_to(REPO_ROOT)
    )
    builder_bindings = verify_expected_tree_builder_chain(
        repository_root, preflight
    )
    if str(repository_root) not in sys.path:
        sys.path.insert(0, str(repository_root))
    try:
        from scripts import native_shadow_boot_staging_measure_arm64_v1 as measurement
        from scripts import native_shadow_rootfs_builder_boot_arm64_v1 as boot_v1
        from scripts import native_shadow_rootfs_builder_boot_arm64_v3 as builder
        from scripts import native_shadow_rootfs_portable_boot_arm64_v2 as release_gate
    except ImportError as error:
        raise RefusedError("sealed expected-tree builders cannot be imported") from error

    sealed_raw = measurement.BOOT_SOURCE_LOCK_PATH.read_bytes()
    if sha256_bytes(sealed_raw) != AUTHORITY_SHA256[
        "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v2.json"
    ]:
        raise RefusedError("boot source lock differs before expected-tree assembly")
    sealed = json.loads(sealed_raw.decode("utf-8"))
    runtime, _receipt = release_gate.materialize_runtime_lock(
        sealed, sealed_raw, pathlib.Path(gpgv), pathlib.Path(zstd)
    )
    lock, lock_raw, _normalization = boot_v1.normalized_runtime_lock(runtime)
    validated = builder.validate_source_lock(
        lock,
        lock_raw,
        repository_root,
        artifact_store,
        require_complete=True,
    )
    nested = builder.nested_runtime_tree(
        repository_root,
        artifact_store,
        pathlib.Path(gpgv),
        pathlib.Path(zstd),
    )
    assembled = builder.materialize_staging_tree(
        validated,
        repository_root,
        artifact_store,
        launcher_binary=None,
        nested_tree=nested,
    )
    measured = measurement.builder_totals(assembled)
    if (
        len(assembled) != SEALED_STAGING_ENTRIES
        or measured["payloadBytes"] != SEALED_STAGING_PAYLOAD_BYTES
        or measured["pathManifestSha256"] != SEALED_STAGING_PATH_SHA256
        or measured["byKind"]
        != {"directory": 1_736, "file": 15_101, "symlink": 837}
    ):
        raise RefusedError("reassembled staging tree differs from its sealed measurement")
    table = complete_expected_table(
        assembled,
        launcher_path=boot_v1.LAUNCHER_GUEST_PATH.lstrip("/"),
        launcher_size=boot_v1.LAUNCHER_SIZE_BYTES,
        launcher_sha256=boot_v1.LAUNCHER_SHA256,
    )
    manifest = expected_table_manifest(table)
    if (
        manifest["pathManifestBytes"] != SEALED_COMPLETE_PATH_BYTES
        or manifest["pathManifestSha256"] != SEALED_COMPLETE_PATH_SHA256
    ):
        raise RefusedError("complete expected path manifest differs")
    # The assembled table owns roughly 1.8 GiB of immutable byte strings.  Only
    # their exact digests survive in ``table``; release them before the image is
    # traversed so evidence collection does not require both copies in memory.
    del assembled, nested, validated, lock, runtime
    gc.collect()
    return table, manifest, builder_bindings


def _marker_lookup(markers: Sequence[Marker]) -> dict[str, Marker]:
    lookup = {row.identifier: row for row in markers}
    if len(lookup) != len(markers):
        raise RefusedError("marker identifiers are duplicated")
    return lookup


def historical_occurrences(
    record: Mapping[str, object], markers: Sequence[Marker]
) -> tuple[RawOccurrence, ...]:
    lookup = _marker_lookup(markers)
    rows = record.get("hits")
    if not isinstance(rows, list) or record.get("hitCount") != len(rows):
        raise RefusedError("historical raw hit count differs")
    occurrences = []
    seen = set()
    for row in rows:
        marker = row.get("marker") if isinstance(row, Mapping) else None
        offset = row.get("offset") if isinstance(row, Mapping) else None
        tier = row.get("tier") if isinstance(row, Mapping) else None
        known = lookup.get(marker)
        identity = (marker, offset)
        if (
            known is None
            or tier != known.tier
            or not isinstance(offset, int)
            or isinstance(offset, bool)
            or offset < 0
            or identity in seen
        ):
            raise RefusedError("historical raw hit identity differs")
        seen.add(identity)
        occurrences.append(RawOccurrence(marker, tier, offset))
    expected_order = sorted(occurrences, key=lambda row: (row.raw_offset, row.marker))
    if occurrences != expected_order or len(occurrences) != 135:
        raise RefusedError("historical raw hit sequence differs")
    return tuple(occurrences)


def require_no_host_identity_hits(rows: Sequence[RawOccurrence]) -> None:
    if any(
        not isinstance(row, RawOccurrence) or row.tier == "host-identity"
        for row in rows
    ):
        raise RefusedError("a private host-identity marker occurs in the guest image")


def _pread_exact(descriptor: int, size: int, offset: int) -> bytes:
    try:
        value = os.pread(descriptor, size, offset)
    except OSError as error:
        raise RefusedError("read-only image pread failed") from error
    if len(value) != size:
        raise RefusedError("read-only image ended before the sealed boundary")
    return value


def _hash_descriptor(descriptor: int, size: int) -> str:
    digest = hashlib.sha256()
    offset = 0
    while offset < size:
        take = min(READ_CHUNK_BYTES, size - offset)
        digest.update(_pread_exact(descriptor, take, offset))
        offset += take
    return digest.hexdigest()


def scan_descriptor(
    descriptor: int,
    size: int,
    markers: Sequence[Marker],
) -> tuple[tuple[RawOccurrence, ...], str]:
    """Hash and rescan every image byte through one read-only descriptor."""

    if size != SEALED_IMAGE_BYTES or not markers:
        raise RefusedError("raw rescan target or marker table differs")
    overlap = max(len(row.needle) for row in markers) - 1
    carry = b""
    digest = hashlib.sha256()
    occurrences = []
    seen = set()
    offset = 0
    while offset < size:
        take = min(READ_CHUNK_BYTES, size - offset)
        chunk = _pread_exact(descriptor, take, offset)
        digest.update(chunk)
        window = carry + chunk
        window_start = offset - len(carry)
        for marker in markers:
            cursor = 0
            while True:
                found = window.find(marker.needle, cursor)
                if found < 0:
                    break
                raw_offset = window_start + found
                identity = (marker.identifier, raw_offset)
                if identity not in seen:
                    seen.add(identity)
                    occurrences.append(
                        RawOccurrence(marker.identifier, marker.tier, raw_offset)
                    )
                cursor = found + 1
        carry = window[-overlap:] if overlap and len(window) > overlap else window
        offset += take
    occurrences.sort(key=lambda row: (row.raw_offset, row.marker))
    return tuple(occurrences), digest.hexdigest()


def _stable_stat_identity(value: os.stat_result) -> tuple:
    return (
        value.st_dev,
        value.st_ino,
        value.st_mode,
        value.st_nlink,
        value.st_uid,
        value.st_gid,
        value.st_size,
        value.st_mtime_ns,
        value.st_ctime_ns,
    )


def open_sealed_image(path: pathlib.Path) -> int:
    flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags)
    except OSError as error:
        raise RefusedError("preserved root disk cannot be opened read-only") from error
    try:
        info = os.fstat(descriptor)
        if not stat.S_ISREG(info.st_mode) or info.st_size != SEALED_IMAGE_BYTES:
            raise RefusedError("preserved root disk file identity differs")
        return descriptor
    except BaseException:
        os.close(descriptor)
        raise


def load_ext4_state(descriptor: int):
    """Parse every allocated inode/block and the complete directory graph."""

    try:
        from scripts import native_shadow_ext4_readonly_owner_map_arm64_v1 as ext4
    except ImportError:
        import native_shadow_ext4_readonly_owner_map_arm64_v1 as ext4

    def read_block(block: int) -> bytes:
        if not isinstance(block, int) or isinstance(block, bool) or not 0 <= block < 496_979:
            raise RefusedError("ext4 block request is outside the sealed image")
        return _pread_exact(descriptor, BLOCK_BYTES, block * BLOCK_BYTES)

    geometry = ext4.parse_superblock(
        _pread_exact(descriptor, 1_024, 1_024),
        image_size=SEALED_IMAGE_BYTES,
    )
    descriptor_table = _pread_exact(
        descriptor,
        geometry.group_count * geometry.descriptor_bytes,
        geometry.block_bytes,
    )
    groups = ext4.parse_group_descriptors(descriptor_table, geometry=geometry)
    allocation = ext4.verify_allocation_bitmaps(
        groups, geometry=geometry, read_block=read_block
    )
    inodes = ext4.read_allocated_inodes(
        groups,
        allocation=allocation,
        geometry=geometry,
        read_block=read_block,
    )
    owners = ext4.build_block_ownership(
        geometry=geometry,
        descriptors=groups,
        allocation=allocation,
        inodes=inodes,
        read_block=read_block,
    )
    graph = ext4.walk_directory_tree(
        inodes=inodes, owners=owners, read_block=read_block
    )
    return ext4, geometry, groups, allocation, inodes, owners, graph, read_block


def _payload_occurrences(payload: bytes, markers: Sequence[Marker]) -> list[tuple[str, int]]:
    rows = []
    for marker in markers:
        cursor = 0
        while True:
            found = payload.find(marker.needle, cursor)
            if found < 0:
                break
            rows.append((marker.identifier, found))
            cursor = found + 1
    rows.sort(key=lambda row: (row[1], row[0]))
    return rows


def _raw_offset_for_file_offset(inode, file_offset: int) -> int:
    if (
        not isinstance(file_offset, int)
        or isinstance(file_offset, bool)
        or not 0 <= file_offset < inode.size
    ):
        raise RefusedError("logical occurrence is outside its inode")
    logical_block = file_offset // BLOCK_BYTES
    within = file_offset % BLOCK_BYTES
    matches = []
    for extent in inode.extents:
        if extent.logical <= logical_block < extent.logical + extent.length:
            matches.append(extent.physical + logical_block - extent.logical)
    if len(matches) != 1:
        raise RefusedError("logical occurrence has no unique physical block")
    return matches[0] * BLOCK_BYTES + within


def verify_expected_tree_and_scan(
    *,
    expected: Mapping[str, ExpectedEntry],
    markers: Sequence[Marker],
    ext4,
    inodes: Mapping[int, object],
    owners: Mapping[int, object],
    graph,
    read_block: Callable[[int], bytes],
) -> tuple[dict, tuple[LogicalOccurrence, ...], dict[int, str]]:
    """Exact-match all visible metadata/content and scan each logical payload."""

    actual_paths = set(graph.path_to_inode)
    if actual_paths != set(expected):
        raise RefusedError(
            "logical path set differs: missing=%d extra=%d"
            % (len(set(expected) - actual_paths), len(actual_paths - set(expected)))
        )
    if forbidden_logical_path_hits(actual_paths):
        raise RefusedError("guest carries a forbidden host-secret logical path")

    root = inodes.get(2)
    if (
        root is None
        or root.kind != "directory"
        or root.mode & 0o7777 != 0o755
        or root.uid != 0
        or root.gid != 0
        or root.size != 4_096
        or root.links != 13
    ):
        raise RefusedError("root inode differs from the sealed filesystem contract")
    lost_number = graph.path_to_inode.get("lost+found")
    lost = inodes.get(lost_number)
    if (
        lost_number != 11
        or lost is None
        or lost.kind != "directory"
        or lost.mode & 0o7777 != 0o700
        or lost.uid != 0
        or lost.gid != 0
        or lost.size != 16_384
        or lost.links != 2
    ):
        raise RefusedError("lost+found inode differs from the filesystem writer output")

    by_kind = collections.Counter()
    content_digests: dict[int, str] = {}
    logical_occurrences = []
    symlink_targets = {}
    scanned_inodes = set()
    for path, expected_row in expected.items():
        inode_number = graph.path_to_inode[path]
        inode = inodes[inode_number]
        by_kind[inode.kind] += 1
        if (
            inode.kind != expected_row.kind.replace("file", "regular")
            or inode.mode & 0o7777 != expected_row.mode
            or inode.uid != expected_row.uid
            or inode.gid != expected_row.gid
        ):
            raise RefusedError("logical entry metadata differs: %s" % path)
        aliases = graph.paths_by_inode[inode_number]
        if inode.kind == "regular":
            if inode_number not in scanned_inodes:
                try:
                    payload = ext4.read_inode_payload(inode, read_block)
                except Exception as error:
                    raise RefusedError(
                        "regular file payload cannot be read exactly: %s inode=%d size=%d extents=%r"
                        % (path, inode_number, inode.size, inode.extents)
                    ) from error
                digest = sha256_bytes(payload)
                content_digests[inode_number] = digest
                expected_aliases = [expected[alias] for alias in aliases]
                if any(
                    row.kind != "file"
                    or row.size != inode.size
                    or row.sha256 != digest
                    for row in expected_aliases
                ):
                    raise RefusedError("regular file content differs: %s" % path)
                for marker, file_offset in _payload_occurrences(payload, markers):
                    raw_offset = _raw_offset_for_file_offset(inode, file_offset)
                    logical_occurrences.append(
                        LogicalOccurrence(
                            marker,
                            raw_offset,
                            inode_number,
                            file_offset,
                            aliases,
                        )
                    )
                scanned_inodes.add(inode_number)
        elif inode.kind == "symlink":
            payload = ext4.read_inode_payload(inode, read_block)
            try:
                target = payload.decode("utf-8")
            except UnicodeDecodeError as error:
                raise RefusedError("symlink target is not UTF-8: %s" % path) from error
            if inode.size != expected_row.size or target != expected_row.symlink_target:
                raise RefusedError("symlink target differs: %s" % path)
            symlink_targets[path] = target
            if _payload_occurrences(payload, markers):
                raise RefusedError("a secret marker occurs in a symlink target")
        elif inode.kind != "directory":
            raise RefusedError("visible inode has an unsupported kind")

    if forbidden_symlink_target_hits(symlink_targets):
        raise RefusedError("a symlink resolves to a forbidden host-secret path")
    journal = inodes.get(8)
    if journal is None or journal.kind != "journal":
        raise RefusedError("ext4 journal inode is absent")
    journal_payload = ext4.read_inode_payload(journal, read_block)
    if _payload_occurrences(journal_payload, markers):
        raise RefusedError("a secret marker occurs in the ext4 journal")
    del journal_payload
    logical_occurrences.sort(key=lambda row: (row.raw_offset, row.marker))
    counts = collections.Counter(row.kind for row in expected.values())
    if by_kind != {"directory": 1_738, "regular": 15_102, "symlink": 837}:
        raise RefusedError("observed visible inode kinds differ")
    return (
        {
            "logicalPaths": len(actual_paths),
            "byKind": dict(sorted(counts.items())),
            "regularFilesHashed": len(content_digests),
            "regularFileBytesHashed": sum(
                inodes[number].size for number in content_digests
            ),
            "symlinkTargetsChecked": len(symlink_targets),
            "forbiddenLogicalPaths": 0,
            "forbiddenSymlinkTargets": 0,
            "journalBytesScanned": journal.size,
            "journalHits": 0,
        },
        tuple(logical_occurrences),
        content_digests,
    )


def reconcile_preserved_image(
    *,
    repository_root: pathlib.Path,
    archive_root: pathlib.Path,
    artifact_store: pathlib.Path,
    gpgv: pathlib.Path,
    zstd: pathlib.Path,
    private_host_home: bytes,
) -> dict:
    """Run the complete read-only raw→owner→path→sealed-content proof."""

    repository_root = pathlib.Path(repository_root).resolve()
    if repository_root != REPO_ROOT.resolve():
        raise RefusedError("repository root differs from this checked-in consumer")
    authority_rows = verify_authority_files(repository_root)
    historical, historical_raw = _read_json_exact(
        repository_root / HISTORICAL_SCAN_PATH.relative_to(REPO_ROOT)
    )
    correction, _correction_raw = _read_json_exact(
        repository_root / CORRECTION_PATH.relative_to(REPO_ROOT)
    )
    qualification, _qualification_raw = _read_json_exact(
        repository_root / QUALIFICATION_PATH.relative_to(REPO_ROOT)
    )
    preservation, _preservation_raw = _read_json_exact(
        repository_root / PRESERVATION_PATH.relative_to(REPO_ROOT)
    )
    production, _production_raw = _read_json_exact(
        repository_root / PRODUCTION_RESULT_PATH.relative_to(REPO_ROOT)
    )
    preflight, _preflight_raw = _read_json_exact(
        repository_root / PREFLIGHT_RESULT_PATH.relative_to(REPO_ROOT)
    )
    if (
        correction.get("status")
        != "RAW-SCAN-PATH-AND-ORIGIN-INFERENCES-FALSIFIED-CONDITION-NOT-SETTLED"
        or correction.get("historicalFactsRetained", {}).get(
            "genericSecretShapeRawHits"
        )
        != 135
        or correction.get("claimBoundary", {}).get("conditionSettled") is not False
        or historical.get("sha256Before") != SEALED_IMAGE_SHA256
        or historical.get("sha256After") != SEALED_IMAGE_SHA256
        or historical.get("bytesRead") != SEALED_IMAGE_BYTES
        or production.get("passed") is not True
        or production.get("imageProducedClaim") is not True
    ):
        raise RefusedError("historical correction or production lineage differs")

    target = resolve_preserved_root_disk(qualification, preservation)
    if target != {
        "relativePath": "successor-outputs-1/guest-root-disk",
        "sizeBytes": SEALED_IMAGE_BYTES,
        "sha256": SEALED_IMAGE_SHA256,
        "replica": 1,
    }:
        raise RefusedError("resolved historical target differs from replica-1")
    relative_parts = pathlib.PurePosixPath(target["relativePath"]).parts
    image_path = pathlib.Path(archive_root).joinpath(*relative_parts)

    markers = frozen_markers(
        historical.get("markersSearched"), private_host_home=private_host_home
    )
    if len(markers) != 25:
        raise RefusedError("historical marker inventory differs")
    historical_hits = historical_occurrences(historical, markers)
    require_no_host_identity_hits(historical_hits)
    producer_marker = producer_build_home_marker(preflight)
    all_markers = markers + (producer_marker,)
    expected, expected_manifest, expected_tree_builders = assemble_expected_tree(
        repository_root,
        artifact_store,
        pathlib.Path(gpgv).resolve(),
        pathlib.Path(zstd).resolve(),
    )

    descriptor = open_sealed_image(image_path)
    try:
        initial_stat = os.fstat(descriptor)
        all_rescanned_hits, before_sha = scan_descriptor(
            descriptor, initial_stat.st_size, all_markers
        )
        rescanned_hits = tuple(
            row for row in all_rescanned_hits if row.marker != producer_marker.identifier
        )
        producer_hits = tuple(
            row for row in all_rescanned_hits if row.marker == producer_marker.identifier
        )
        if before_sha != SEALED_IMAGE_SHA256 or rescanned_hits != historical_hits:
            raise RefusedError("independent raw rescan differs from the historical inventory")
        require_no_host_identity_hits(rescanned_hits)
        if not producer_hits:
            raise RefusedError("sealed producer build path was not observed")
        (
            ext4,
            geometry,
            groups,
            allocation,
            inodes,
            owners,
            graph,
            read_block,
        ) = load_ext4_state(descriptor)
        tree_report, all_logical_hits, content_digests = verify_expected_tree_and_scan(
            expected=expected,
            markers=all_markers,
            ext4=ext4,
            inodes=inodes,
            owners=owners,
            graph=graph,
            read_block=read_block,
        )
        logical_hits = tuple(
            row for row in all_logical_hits if row.marker != producer_marker.identifier
        )
        producer_logical_hits = tuple(
            row for row in all_logical_hits if row.marker == producer_marker.identifier
        )
        raw_counter = collections.Counter(
            (row.marker, row.raw_offset) for row in rescanned_hits
        )
        logical_counter = collections.Counter(
            (row.marker, row.raw_offset) for row in logical_hits
        )
        if raw_counter != logical_counter:
            raise RefusedError("logical file scan does not conserve all raw occurrences")
        if collections.Counter(
            (row.marker, row.raw_offset) for row in producer_hits
        ) != collections.Counter(
            (row.marker, row.raw_offset) for row in producer_logical_hits
        ):
            raise RefusedError("producer build-path occurrences are not conserved")

        marker_lookup = _marker_lookup(markers)
        mappings = []
        distinct_inodes = set()
        distinct_blocks = set()
        for occurrence in rescanned_hits:
            marker = marker_lookup[occurrence.marker]
            mapped = ext4.map_raw_hit(
                marker=occurrence.marker,
                raw_offset=occurrence.raw_offset,
                needle=marker.needle,
                image_size=initial_stat.st_size,
                owners=owners,
                inodes=inodes,
                paths_by_inode=graph.paths_by_inode,
                read_block=read_block,
            )
            if (
                not mapped.paths
                or content_digests.get(mapped.inode)
                != expected[mapped.paths[0]].sha256
                or any(
                    expected[path].sha256 != content_digests[mapped.inode]
                    for path in mapped.paths
                )
            ):
                raise RefusedError("raw owner is not exact sealed expected content")
            distinct_inodes.add(mapped.inode)
            distinct_blocks.update(mapped.physical_blocks)
            mappings.append(
                {
                    "blockSha256": list(mapped.block_sha256s),
                    "fileOffset": mapped.file_offset,
                    "fileSha256": content_digests[mapped.inode],
                    "inode": mapped.inode,
                    "marker": mapped.marker,
                    "needleBytes": mapped.needle_bytes,
                    "paths": list(mapped.paths),
                    "physicalBlocks": list(mapped.physical_blocks),
                    "rawEnd": mapped.raw_offset + mapped.needle_bytes,
                    "rawOffset": mapped.raw_offset,
                }
            )
        launcher_path = "usr/libexec/boole/boole-native-shadow-launcher"
        producer_owner_inodes = set()
        producer_owner_paths = set()
        for occurrence in producer_hits:
            mapped = ext4.map_raw_hit(
                marker=occurrence.marker,
                raw_offset=occurrence.raw_offset,
                needle=producer_marker.needle,
                image_size=initial_stat.st_size,
                owners=owners,
                inodes=inodes,
                paths_by_inode=graph.paths_by_inode,
                read_block=read_block,
            )
            if (
                mapped.paths != (launcher_path,)
                or content_digests.get(mapped.inode) != expected[launcher_path].sha256
                or expected[launcher_path].sha256
                != "11b5d1cf1728aff271c589129292bcd8ad07a1d928652d2435b1c9010f73c434"
            ):
                raise RefusedError(
                    "producer build path is not exact sealed launcher provenance"
                )
            producer_owner_inodes.add(mapped.inode)
            producer_owner_paths.update(mapped.paths)
        after_sha = _hash_descriptor(descriptor, initial_stat.st_size)
        final_stat = os.fstat(descriptor)
        if (
            after_sha != before_sha
            or _stable_stat_identity(final_stat) != _stable_stat_identity(initial_stat)
        ):
            raise RefusedError("preserved image changed while it was reconciled")
    finally:
        os.close(descriptor)

    marker_counts = collections.Counter(row.marker for row in rescanned_hits)
    owner_counts = collections.Counter(row.classification for row in owners.values())
    inode_counts = collections.Counter(row.kind for row in inodes.values())
    bound_implementations = implementation_bindings(
        repository_root,
        gpgv=pathlib.Path(gpgv),
        zstd=pathlib.Path(zstd),
    )
    bound_implementations["expectedTreeBuilders"] = expected_tree_builders
    return {
        "schema": "boole.native-shadow.mac3.guest-secret-path-content-reconciliation.arm64.v1",
        "status": (
            "LOGICAL-PATH-CONTENT-AND-PHYSICAL-OWNER-RECONCILIATION-PASS-"
            "HOST-PATH-CONDITION-NOT-SETTLED"
        ),
        "appendOnly": True,
        "condition": "no-host-wallet-model-key-or-node-secret-in-the-guest",
        "reconciliationPassed": True,
        "conditionSettled": False,
        "verdict": "NOT-SETTLED",
        "method": {
            "imageAccess": "O_RDONLY|O_CLOEXEC|O_NOFOLLOW-and-pread-only",
            "filesystemMounted": False,
            "filesystemRepaired": False,
            "expectedTreeSource": "sealed-source-locks-and-generation-seals-not-the-image",
            "rawScanReperformed": True,
            "surroundingBytesRecorded": False,
            "privateMarkerRecorded": False,
        },
        "authorityBindings": authority_rows,
        "implementationBindings": bound_implementations,
        "targetImage": {
            "archiveRelativePath": target["relativePath"],
            "replica": target["replica"],
            "sizeBytes": target["sizeBytes"],
            "sha256Before": before_sha,
            "sha256After": after_sha,
        },
        "expectedTree": expected_manifest,
        "ext4": {
            "blockBytes": geometry.block_bytes,
            "blocks": geometry.blocks_count,
            "groups": geometry.group_count,
            "allocatedBlocks": len(allocation.blocks),
            "allocatedInodes": len(inodes),
            "externalXattrBlocks": 0,
            "inlineXattrInodes": 0,
            "inodeKinds": dict(sorted(inode_counts.items())),
            "physicalOwnerCounts": dict(sorted(owner_counts.items())),
            "groupDescriptorsVerified": len(groups),
        },
        "logicalTree": tree_report,
        "rawReconciliation": {
            "markersSearched": len(markers),
            "rawHits": len(rescanned_hits),
            "logicalFileHits": len(logical_hits),
            "attributedRawHits": len(mappings),
            "distinctOwnerInodes": len(distinct_inodes),
            "distinctPhysicalBlocks": len(distinct_blocks),
            "hostIdentityHits": sum(
                1 for row in rescanned_hits if row.tier == "host-identity"
            ),
            "journalHits": 0,
            "directoryHits": 0,
            "symlinkHits": 0,
            "metadataHits": 0,
            "slackHits": 0,
            "freeBlockHits": 0,
            "unmappedHits": 0,
            "ambiguousHits": 0,
            "markerCounts": dict(sorted(marker_counts.items())),
            "mappings": mappings,
        },
        "producerBuildPathObservation": {
            "marker": producer_marker.identifier,
            "needleBytes": len(producer_marker.needle),
            "needleSha256": sha256_bytes(producer_marker.needle),
            "rawHits": len(producer_hits),
            "attributedRawHits": len(producer_hits),
            "distinctOwnerInodes": len(producer_owner_inodes),
            "paths": sorted(producer_owner_paths),
            "fileSha256": expected[launcher_path].sha256,
            "classification": (
                "SEALED-LAUNCHER-BUILD-PROVENANCE-PATH-NOT-SECRET-MATERIAL-"
                "BUT-HOST-PATH-CONDITION-BLOCKER"
            ),
            "zeroHostDerivedBuildPathStringsClaim": False,
            "hostPathCriterionMet": False,
            "noHostWalletModelOrNodeSecretMaterialObserved": True,
        },
        "claimBoundary": {
            "bootAttempted": False,
            "imageProducedOrModified": False,
            "servingClaim": False,
            "publicMiningClaim": False,
            "paidApiBenchmarkClaim": False,
            "activationAllowed": False,
            "mineableNow": 0,
            "rewardReady": 0,
        },
    }


def _read_private_marker_file(path: pathlib.Path) -> bytes:
    flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags)
    except OSError as error:
        raise RefusedError("private host-home marker file cannot be opened") from error
    try:
        info = os.fstat(descriptor)
        if not stat.S_ISREG(info.st_mode) or info.st_size <= 0 or info.st_size > 4_096:
            raise RefusedError("private host-home marker file shape differs")
        raw = _pread_exact(descriptor, info.st_size, 0)
        if raw.endswith(b"\n") and raw.count(b"\n") == 1:
            raw = raw[:-1]
        if not raw or b"\n" in raw or b"\x00" in raw:
            raise RefusedError("private host-home marker file is not one byte string")
        return raw
    finally:
        os.close(descriptor)


def _write_new_record(path: pathlib.Path, value: Mapping[str, object]) -> None:
    raw = canonical_json(value)
    path = pathlib.Path(path)
    temporary = path.with_name(path.name + ".tmp")
    if path.exists() or path.is_symlink() or temporary.exists() or temporary.is_symlink():
        raise RefusedError("output record path already exists")
    descriptor = os.open(
        temporary,
        os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_CLOEXEC", 0),
        0o600,
    )
    try:
        written = os.write(descriptor, raw)
        if written != len(raw):
            raise RefusedError("output record write was incomplete")
        os.fsync(descriptor)
    finally:
        os.close(descriptor)
    os.replace(temporary, path)


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Reconcile every historical raw hit to sealed ext4 file content."
    )
    parser.add_argument("--repo-root", required=True, type=pathlib.Path)
    parser.add_argument("--archive-root", required=True, type=pathlib.Path)
    parser.add_argument("--cas", required=True, type=pathlib.Path)
    parser.add_argument("--gpgv", required=True, type=pathlib.Path)
    parser.add_argument("--zstd", required=True, type=pathlib.Path)
    parser.add_argument("--private-host-home-marker-file", required=True, type=pathlib.Path)
    parser.add_argument("--output", required=True, type=pathlib.Path)
    args = parser.parse_args(argv)
    record = reconcile_preserved_image(
        repository_root=args.repo_root,
        archive_root=args.archive_root,
        artifact_store=args.cas,
        gpgv=args.gpgv,
        zstd=args.zstd,
        private_host_home=_read_private_marker_file(args.private_host_home_marker_file),
    )
    _write_new_record(args.output, record)
    print("native-shadow-mac3-guest-secret-reconcile: PASS: %s" % args.output)
    return 0


def _fields(block: str) -> dict:
    result = {}
    for line in block.splitlines():
        if " =" in line:
            key, value = line.split(" =", 1)
            if value.startswith(" "):
                value = value[1:]
            if key in result:
                raise RefusedError("duplicate field in ext listing: %s" % key)
            result[key] = value
    return result


def parse_slt(text: str) -> List[Entry]:
    """Parses p7zip's technical listing and discards the archive header."""

    entries: List[Entry] = []
    header_seen = False
    for block in text.split("\n\n"):
        row = _fields(block)
        if not row:
            continue
        if "Path" not in row:
            continue
        if "Mode" not in row:
            if "Type" in row and not header_seen:
                header_seen = True
                continue
            raise RefusedError("incomplete ext entry metadata for %s" % row["Path"])
        mode = row["Mode"]
        path = row["Path"]
        if path == JOURNAL_PATH:
            kind = "journal"
        elif mode.startswith("d"):
            kind = "directory"
        elif mode.startswith("l"):
            kind = "symlink"
        elif mode.startswith("-"):
            kind = "file"
        else:
            raise RefusedError("unsupported ext entry mode for %s: %s" % (path, mode))
        try:
            if "Size" not in row:
                raise KeyError("Size")
            size = int(row["Size"] or 0)
            uid = int(row["User"])
            gid = int(row["Group"])
        except (KeyError, ValueError) as error:
            raise RefusedError("incomplete ext entry metadata for %s" % path) from error
        if size < 0 or uid < 0 or gid < 0:
            raise RefusedError("negative ext entry metadata for %s" % path)
        entries.append(
            Entry(
                path=path,
                kind=kind,
                size=size,
                mode=mode,
                uid=uid,
                gid=gid,
                symlink_target=row.get("Symbolic Link", ""),
            )
        )
    if not entries:
        raise RefusedError("ext listing contains no logical entries")
    validate_logical_paths([row.path for row in entries])
    return entries


def validate_logical_paths(paths: Sequence[str]) -> None:
    seen = set()
    for value in paths:
        if not isinstance(value, str) or not value:
            raise RefusedError("empty or non-POSIX logical path")
        if value.startswith("/") or value.endswith("/"):
            raise RefusedError("unsafe logical path: %s" % value)
        parts = value.split("/")
        if any(part in ("", ".", "..") for part in parts):
            raise RefusedError("unsafe logical path: %s" % value)
        if any(ord(character) < 32 or ord(character) == 127 for character in value):
            raise RefusedError("control character in logical path")
        if value in seen:
            raise RefusedError("duplicate logical path: %s" % value)
        seen.add(value)


def staging_path_manifest(paths: Iterable[str]) -> Tuple[List[str], str]:
    """Reconstructs the pre-image path manifest from the read-back inventory."""

    materialized = list(paths)
    validate_logical_paths(materialized)
    base = sorted(path for path in materialized if path not in GENERATED_PATHS)
    payload = ("\n".join(base) + "\n").encode("utf-8")
    return base, sha256_bytes(payload)


def reconcile_signature_counters(
    raw: Mapping[tuple, int],
    logical_files: Mapping[tuple, int],
    journal: Mapping[tuple, int],
) -> dict:
    """Requires exact marker/block/position conservation and an empty journal."""

    raw_counter = collections.Counter(raw)
    logical_counter = collections.Counter(logical_files)
    journal_counter = collections.Counter(journal)
    for label, counter in (
        ("raw", raw_counter),
        ("logical", logical_counter),
        ("journal", journal_counter),
    ):
        for signature, count in counter.items():
            if (
                not isinstance(signature, tuple)
                or len(signature) != 3
                or not isinstance(signature[0], str)
                or not signature[0]
                or not isinstance(signature[1], str)
                or len(signature[1]) != 64
                or any(character not in string.hexdigits for character in signature[1])
                or not isinstance(signature[2], int)
                or isinstance(signature[2], bool)
                or not 0 <= signature[2] < BLOCK_BYTES
                or not isinstance(count, int)
                or isinstance(count, bool)
                or count <= 0
            ):
                raise RefusedError("invalid %s signature counter" % label)
    if journal_counter:
        raise RefusedError("a historical secret-shape hit is present in the ext4 journal")
    if raw_counter != logical_counter:
        unmapped = raw_counter - logical_counter
        extra = logical_counter - raw_counter
        raise RefusedError(
            "raw/logical hit conservation failed: unmapped=%d extra=%d"
            % (sum(unmapped.values()), sum(extra.values()))
        )
    return {
        "rawHits": sum(raw_counter.values()),
        "logicalFileHits": sum(logical_counter.values()),
        "journalHits": 0,
        "unmappedRawHits": 0,
        "extraLogicalHits": 0,
    }


def _validate_physical_hit(hit: PhysicalHit) -> None:
    if (
        not isinstance(hit.marker, str)
        or not hit.marker
        or not isinstance(hit.raw_offset, int)
        or isinstance(hit.raw_offset, bool)
        or hit.raw_offset < 0
        or not isinstance(hit.block_sha256, str)
        or len(hit.block_sha256) != 64
        or any(character not in string.hexdigits for character in hit.block_sha256)
        or not isinstance(hit.block_position, int)
        or isinstance(hit.block_position, bool)
        or not 0 <= hit.block_position < BLOCK_BYTES
        or hit.raw_offset % BLOCK_BYTES != hit.block_position
    ):
        raise RefusedError("invalid physical hit")


def reconcile_physical_owners(
    raw_hits: Sequence[PhysicalHit],
    logical_owners: Mapping[PhysicalHit, LogicalOwner],
    journal_hits: Sequence[PhysicalHit],
) -> dict:
    """Every raw offset must resolve to one inode and one or more aliases."""

    if journal_hits:
        raise RefusedError("a candidate hit is owned by the ext4 journal")
    if len(set(raw_hits)) != len(raw_hits):
        raise RefusedError("duplicate raw physical hit identity")
    if set(logical_owners) != set(raw_hits):
        raise RefusedError("physical-owner map does not cover exactly the raw hits")
    for hit in raw_hits:
        _validate_physical_hit(hit)
        owner = logical_owners[hit]
        if (
            not isinstance(owner, LogicalOwner)
            or not isinstance(owner.inode, int)
            or isinstance(owner.inode, bool)
            or owner.inode <= 0
            or not owner.paths
            or len(set(owner.paths)) != len(owner.paths)
        ):
            raise RefusedError("raw hit does not have exactly one inode owner")
        validate_logical_paths(owner.paths)
    return {
        "rawHits": len(raw_hits),
        "attributedRawHits": len(raw_hits),
        "journalHits": 0,
        "unmappedRawHits": 0,
        "ambiguousRawHits": 0,
    }


def classify_candidate_path(path: str) -> dict:
    """Returns a namespace candidate, never sealed-source proof."""

    validate_logical_paths([path])
    if path in GENERATED_PATHS:
        raise RefusedError("generated or journal content needs its own classification: %s" % path)
    canonical = path[len(RUNTIME_PREFIX) :] if path.startswith(RUNTIME_PREFIX) else path
    if canonical.startswith("usr/libexec/boole/"):
        raise RefusedError("locally generated boole files are not upstream payloads")
    allowed = (
        canonical.startswith("usr/"),
        canonical.startswith("boot/"),
        canonical.startswith("opt/boole/native-checker-toolchain/"),
    )
    if not any(allowed):
        raise RefusedError("candidate is not in a sealed upstream namespace: %s" % path)
    return {
        "classification": "UPSTREAM-NAMESPACE-CANDIDATE",
        "canonicalPath": canonical,
        "nestedRuntimeCopy": path.startswith(RUNTIME_PREFIX),
    }


def _validate_content_digest(value: str) -> None:
    if (
        not isinstance(value, str)
        or len(value) != 64
        or any(character not in string.hexdigits for character in value)
    ):
        raise RefusedError("invalid content digest")


def bind_candidate_to_sealed_source(
    observed: ObservedContent,
    sealed_entries: Mapping[str, SealedContent],
) -> dict:
    """Binds a candidate to an exact sealed path, kind, digest and target."""

    validate_logical_paths([observed.path])
    if observed.kind not in ("file", "symlink"):
        raise RefusedError("candidate kind is not content-bearing")
    expected = sealed_entries.get(observed.path)
    if expected is None or not expected.source:
        raise RefusedError("candidate has no sealed source membership")
    if observed.kind != expected.kind:
        raise RefusedError("candidate differs from its sealed source entry")
    if observed.kind == "file":
        _validate_content_digest(observed.sha256)
        _validate_content_digest(expected.sha256)
        if (
            observed.sha256 != expected.sha256
            or observed.symlink_target
            or expected.symlink_target
        ):
            raise RefusedError("candidate differs from its sealed source entry")
        classification = "SEALED-EXPECTED-FILE-CONTENT"
    else:
        if (
            observed.sha256
            or expected.sha256
            or not observed.symlink_target
            or observed.symlink_target != expected.symlink_target
        ):
            raise RefusedError("candidate differs from its sealed source entry")
        classification = "SEALED-EXPECTED-SYMLINK"
    return {
        "classification": classification,
        "path": observed.path,
        "source": expected.source,
    }


def forbidden_logical_path_hits(paths: Iterable[str]) -> List[str]:
    """Finds actual guest paths; it never treats a documentation substring as one."""

    materialized = list(paths)
    validate_logical_paths(materialized)
    hits = []
    secret_basenames = {".netrc", "id_rsa", "id_dsa", "id_ecdsa", "id_ed25519"}
    for value in materialized:
        parts = pathlib.PurePosixPath(value).parts
        if (
            ".boole" in parts
            or "boole-artifacts" in parts
            or (parts and parts[-1] in secret_basenames)
        ):
            hits.append(value)
    return sorted(hits)


def validate_symlink_target(link_path: str, target: str) -> str:
    """Resolve a target inside the guest root or the nested runtime pseudo-root."""

    validate_logical_paths([link_path])
    if (
        not isinstance(target, str)
        or not target
        or any(ord(character) < 32 or ord(character) == 127 for character in target)
    ):
        raise RefusedError("unsafe symlink target")
    if target.startswith("/"):
        stack = (
            list(pathlib.PurePosixPath(RUNTIME_PREFIX.rstrip("/")).parts)
            if link_path.startswith(RUNTIME_PREFIX)
            else []
        )
    else:
        stack = list(pathlib.PurePosixPath(link_path).parent.parts)
    for part in target.split("/"):
        if part in ("", "."):
            continue
        if part == "..":
            if not stack:
                raise RefusedError("symlink target escapes the guest root")
            stack.pop()
        else:
            stack.append(part)
    resolved = "/".join(stack)
    validate_logical_paths([resolved])
    return resolved


def forbidden_symlink_target_hits(targets: Mapping[str, str]) -> List[str]:
    hits = []
    for link_path, target in targets.items():
        resolved = validate_symlink_target(link_path, target)
        if forbidden_logical_path_hits([resolved]):
            hits.append("%s -> %s" % (link_path, resolved))
    return sorted(hits)


if __name__ == "__main__":
    raise SystemExit(main())
