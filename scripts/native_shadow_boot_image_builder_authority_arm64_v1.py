#!/usr/bin/env python3
"""Frozen input authority for the Linux/arm64 guest image builder.

The rootfs stage already has an authority: the source lock's `buildRecipe` fixes
the builder digest, a canonical mtime of 0, root-only ownership, "never execute or
copy maintainer scripts", and a forbidden network, and its output is an OCI image
layout.  An OCI layout is not something a machine boots, so one stage remains
unowned -- turning that layout plus the pinned kernel package into a kernel image,
an initrd and an ext4 root disk.  This module fixes the inputs of that stage.

Two properties are worth stating plainly, because they are the reason the document
exists rather than decoration on top of it.

First, no tool is taken from `PATH`.  Every executable the stage consumes is named
by the Ubuntu package it comes from, and that package is already pinned by digest
in the committed source lock -- so "which mke2fs" has one answer that a different
build machine cannot change.  `mkfs.ext4` is a symlink to `mke2fs` in that package,
so the role pins `mke2fs` itself; pinning the symlink would let an upstream rename
repoint the tool without touching the digest.

Second, this module is an authority, not a builder.  It produces no kernel, no
initrd and no root disk, and every boundary it declares stays false.  Freezing the
inputs of a build is not the build, and it is emphatically not a boot.
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import pathlib
import subprocess
import sys
import tarfile
import tempfile
from typing import Any, Optional


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from scripts import native_shadow_boot_rootfs_payload_acquire_arm64_v1 as payload

canonical_json = payload.canonical_json

CONTAINMENT = REPO_ROOT / "native" / "containment"
TOOL_PATH = pathlib.Path(__file__).resolve()
AUTHORITY_PATH = CONTAINMENT / "native-shadow-boot-image-builder-authority-arm64-v1.json"
SOURCE_LOCK_PATH = CONTAINMENT / "native-shadow-boot-rootfs-source-lock-arm64-v1.json"

AUTHORITY_SCHEMA = "boole.native-shadow.boot-image-builder-authority.arm64.v1"
RELEASE = "NATIVE-SHADOW-BOOT-IMAGE-BUILDER-AUTHORITY-ARM64-V1"

# The scaffold plan declares this exact format for its still-null
# `inputs.imageBuilderToolchain`, so a successor plan can slot this document in
# without inventing a second vocabulary for the same role.
AUTHORITY_FORMAT = "initrd-ext4-builder-authority-v1"

GENERATOR_PATH = "scripts/native_shadow_boot_image_builder_authority_arm64_v1.py"

AUTHORITY_KEYS = {
    "activationAllowed",
    "boundaries",
    "bootableClaim",
    "builderScripts",
    "determinism",
    "format",
    "generator",
    "hostTools",
    "kernelSource",
    "platform",
    "prohibitions",
    "release",
    "roles",
    "schema",
    "sourceLock",
    "toolBinaries",
}
GENERATOR_KEYS = {"path", "sha256"}
SOURCE_LOCK_KEYS = {"artifactCount", "path", "sha256", "sizeBytes"}
BUILDER_SCRIPT_KEYS = {"path", "role", "sha256", "sizeBytes"}
TOOL_BINARY_KEYS = {
    "memberMode",
    "memberPath",
    "package",
    "packageArtifactId",
    "packageSha256",
    "role",
    "sha256",
    "sizeBytes",
    "version",
}
HOST_TOOL_KEYS = {"path", "pinnedAtBuildTime", "role", "sha256"}
KERNEL_SOURCE_KEYS = {
    "compressedSha256",
    "compressedSizeBytes",
    "compression",
    "imageSha256",
    "imageSizeBytes",
    "memberPath",
    "package",
    "packageArtifactId",
    "packageSha256",
    "version",
}

BOUNDARY_KEYS = {
    "bootAuthority",
    "guestImageBuilt",
    "initrdBuilt",
    "kernelImageExtracted",
    "rootDiskBuilt",
    "runtimeCompatibilityVerified",
    "toolByteProvenanceVerifiedInCi",
}

# Every prohibition is a claim this tool has to be able to check, not a slogan.
# The checks live in `_validate_tool_binaries` and `_validate_determinism`.
PROHIBITIONS = {
    "forbidHostPathLookup": True,
    "forbidLatestVersionSelection": True,
    "forbidMaintainerScripts": True,
    "forbidNetworkDuringBuild": True,
    "forbidProductionSigningMaterial": True,
    "forbidSymlinkToolPins": True,
}

PLATFORM = {
    "architecture": "aarch64",
    "os": "linux",
    "rustTarget": "aarch64-unknown-linux-gnu",
}

ROLES = {
    "input": "oci-image-layout-single-uncompressed-layer-v1",
    "outputs": [
        {"format": "linux-arm64-image-uncompressed", "name": "guest-kernel"},
        {"format": "cpio-newc-uncompressed", "name": "guest-initrd"},
        {"format": "ext4-filesystem-image", "name": "guest-root-disk"},
    ],
}

DETERMINISM = {
    "canonicalMtime": 0,
    # An empty machine-id is systemd's documented first-boot state.  Writing a
    # generated one would put host entropy in the image and make two builds of the
    # same inputs differ; leaving the file absent would change systemd's behaviour.
    "machineId": "empty-file-first-boot",
    # `mke2fs` would otherwise draw a filesystem UUID and a directory hash seed
    # from host entropy, which is enough on its own to make two builds of the
    # same inputs differ.  Both are fixed here to stated synthetic constants; they
    # carry no meaning beyond being constant, and they differ from each other so a
    # copy-paste of one into the other is visible.
    "ext4": {
        "hashSeed": "00000000-0000-4000-8000-000000000002",
        "uuid": "00000000-0000-4000-8000-000000000001",
    },
    "fileOrder": "sorted-by-logical-path-bytes",
    "forbidTimestampSuppression": True,
    "independentBuildCount": 2,
    # The kernel ships gzip-compressed inside the deb and Apple's VZLinuxBootLoader
    # wants a raw arm64 Image, so the decompression step is named here rather than
    # discovered later.  gzip decompression is fully determined by its input.
    "initrdCompression": "none",
    "kernelDecompression": "gzip",
    "mismatchAction": "report-the-difference-never-force-a-match",
    "ownership": "root:root-only",
    "sourceDateEpoch": None,
}

AUTHORITY_SHA256 = "59a14469bbb9710a1f6c79202d3e804b2f79268966c12d4259cd99e59e8d6e1e"


class ImageBuilderAuthorityError(RuntimeError):
    """The image builder authority or one of its pinned inputs is invalid."""


def sha256_bytes(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def image_builder_authority_sha256(raw: bytes) -> str:
    """Digest this tool with its own authority pin blanked out.

    The pin names the document that names this tool, so a plain file digest can
    never equal it.  Blanking the literal breaks the cycle in a way both sides can
    reproduce.
    """

    marker = b'AUTHORITY_SHA256 = "'
    start = raw.index(marker) + len(marker)
    return sha256_bytes(raw[:start] + b"0" * 64 + raw[start + 64 :])


def _exact(value: Any, keys: set[str], context: str) -> dict[str, Any]:
    if not isinstance(value, dict) or set(value) != keys:
        raise ImageBuilderAuthorityError(f"{context} keys differ from the frozen contract")
    return value


def _digest(value: Any, context: str) -> str:
    if not isinstance(value, str) or len(value) != 64:
        raise ImageBuilderAuthorityError(f"{context} is not a sha256 digest")
    if value.lower() != value or any(c not in "0123456789abcdef" for c in value):
        raise ImageBuilderAuthorityError(f"{context} is not a sha256 digest")
    return value


def _count(value: Any, context: str) -> int:
    if not isinstance(value, int) or isinstance(value, bool) or value <= 0:
        raise ImageBuilderAuthorityError(f"{context} is not a positive integer")
    return value


def _relative_path(value: Any, context: str) -> str:
    if not isinstance(value, str) or not value or value.startswith("/") or ".." in value:
        raise ImageBuilderAuthorityError(f"{context} is not repository-relative")
    return value


def _member_path(value: Any, context: str) -> str:
    if (
        not isinstance(value, str)
        or not value.startswith("./")
        or ".." in value
        or "//" in value
    ):
        raise ImageBuilderAuthorityError(f"{context} member path is not deb-relative")
    return value


def load_authority(path: pathlib.Path = AUTHORITY_PATH) -> dict[str, Any]:
    raw = path.read_bytes()
    document = json.loads(raw.decode("utf-8"))
    if canonical_json(document) != raw:
        raise ImageBuilderAuthorityError("image builder authority is not canonical JSON")
    if sha256_bytes(raw) != AUTHORITY_SHA256:
        raise ImageBuilderAuthorityError("image builder authority differs from the pin")
    return validate_authority(document)


def validate_authority(authority: Any) -> dict[str, Any]:
    document = _exact(authority, AUTHORITY_KEYS, "authority")
    if document["schema"] != AUTHORITY_SCHEMA or document["release"] != RELEASE:
        raise ImageBuilderAuthorityError("authority identity differs from the frozen contract")
    if document["format"] != AUTHORITY_FORMAT:
        raise ImageBuilderAuthorityError(
            "authority format differs from the role the boot plan declares"
        )
    if document["activationAllowed"] is not False or document["bootableClaim"] is not False:
        raise ImageBuilderAuthorityError("authority must not claim activation or boot")
    _validate_boundaries(document["boundaries"])
    if document["platform"] != PLATFORM:
        raise ImageBuilderAuthorityError("platform differs from the frozen contract")
    if document["prohibitions"] != PROHIBITIONS:
        raise ImageBuilderAuthorityError("prohibitions differ from the frozen contract")
    if document["roles"] != ROLES:
        raise ImageBuilderAuthorityError("input/output roles differ from the frozen contract")
    _validate_determinism(document["determinism"])
    _validate_generator(document["generator"])
    _validate_source_lock(document["sourceLock"])
    _validate_builder_scripts(document["builderScripts"])
    _validate_tool_binaries(document["toolBinaries"])
    _validate_host_tools(document["hostTools"])
    _validate_kernel_source(document["kernelSource"])
    return document


def _validate_boundaries(value: Any) -> None:
    boundaries = _exact(value, BOUNDARY_KEYS, "boundaries")
    for name, flag in boundaries.items():
        if flag is not False:
            raise ImageBuilderAuthorityError(f"boundary {name} must stay false")


def _validate_determinism(value: Any) -> None:
    if value != DETERMINISM:
        raise ImageBuilderAuthorityError("determinism contract differs from the frozen contract")
    if value["sourceDateEpoch"] is not None:
        raise ImageBuilderAuthorityError("SOURCE_DATE_EPOCH must stay unset")
    if value["forbidTimestampSuppression"] is not True:
        raise ImageBuilderAuthorityError("timestamp suppression must stay forbidden")


def _validate_generator(value: Any) -> dict[str, Any]:
    generator = _exact(value, GENERATOR_KEYS, "generator")
    _digest(generator["sha256"], "generator digest")
    if generator["path"] != GENERATOR_PATH:
        raise ImageBuilderAuthorityError("generator path is not the frozen authority tool")
    return generator


def _validate_source_lock(value: Any) -> dict[str, Any]:
    lock = _exact(value, SOURCE_LOCK_KEYS, "sourceLock")
    _digest(lock["sha256"], "sourceLock digest")
    _count(lock["artifactCount"], "sourceLock artifactCount")
    _count(lock["sizeBytes"], "sourceLock sizeBytes")
    _relative_path(lock["path"], "sourceLock path")
    return lock


def _validate_builder_scripts(value: Any) -> list[dict[str, Any]]:
    if not isinstance(value, list) or not value:
        raise ImageBuilderAuthorityError("builderScripts is empty")
    roles: list[str] = []
    for row in value:
        entry = _exact(row, BUILDER_SCRIPT_KEYS, "builder script")
        _digest(entry["sha256"], "builder script digest")
        _count(entry["sizeBytes"], "builder script sizeBytes")
        _relative_path(entry["path"], "builder script path")
        if not isinstance(entry["role"], str) or not entry["role"]:
            raise ImageBuilderAuthorityError("builder script role is empty")
        roles.append(entry["role"])
    if roles != sorted(roles):
        raise ImageBuilderAuthorityError("builderScripts is not sorted by role")
    if len(set(roles)) != len(roles):
        raise ImageBuilderAuthorityError("a builder script role is duplicated")
    return value


def _validate_tool_binaries(value: Any) -> list[dict[str, Any]]:
    if not isinstance(value, list) or not value:
        raise ImageBuilderAuthorityError("toolBinaries is empty")
    roles: list[str] = []
    for row in value:
        entry = _exact(row, TOOL_BINARY_KEYS, "tool binary")
        _digest(entry["sha256"], "tool binary digest")
        _digest(entry["packageSha256"], "tool binary package digest")
        _count(entry["sizeBytes"], "tool binary sizeBytes")
        if entry["packageArtifactId"] != f"deb-{entry['packageSha256']}":
            raise ImageBuilderAuthorityError(
                "tool binary artifact ID does not derive from its package digest"
            )
        # A path that is not deb-relative is the `PATH` accident the prohibition
        # forbids, arriving one indirection later.
        _member_path(entry["memberPath"], "tool binary")
        if not isinstance(entry["memberMode"], str) or not entry["memberMode"].startswith("0"):
            raise ImageBuilderAuthorityError("tool binary mode is not octal text")
        for field in ("package", "role", "version"):
            if not isinstance(entry[field], str) or not entry[field]:
                raise ImageBuilderAuthorityError(f"tool binary {field} is empty")
        roles.append(entry["role"])
    if roles != sorted(roles):
        raise ImageBuilderAuthorityError("toolBinaries is not sorted by role")
    if len(set(roles)) != len(roles):
        raise ImageBuilderAuthorityError("a tool binary role is duplicated")
    return value


def _validate_host_tools(value: Any) -> list[dict[str, Any]]:
    """Host tools are named and left unfrozen, and the document has to say so.

    The rootfs builder already takes `zstd` as a (path, digest) pair supplied at
    build time, and both committed locks carry them null.  Recording a null pair
    here keeps that shape visible instead of quietly implying the tool is frozen.
    """

    if not isinstance(value, list):
        raise ImageBuilderAuthorityError("hostTools is not a list")
    roles: list[str] = []
    for row in value:
        entry = _exact(row, HOST_TOOL_KEYS, "host tool")
        if entry["pinnedAtBuildTime"] is not True:
            raise ImageBuilderAuthorityError("a host tool must be pinned at build time")
        if (entry["path"] is None) != (entry["sha256"] is None):
            raise ImageBuilderAuthorityError("host tool path and digest must both be null")
        if entry["path"] is not None:
            raise ImageBuilderAuthorityError(
                "a host tool path belongs to the build invocation, not the committed document"
            )
        if not isinstance(entry["role"], str) or not entry["role"]:
            raise ImageBuilderAuthorityError("host tool role is empty")
        roles.append(entry["role"])
    if roles != sorted(roles) or len(set(roles)) != len(roles):
        raise ImageBuilderAuthorityError("hostTools is not sorted by unique role")
    return value


def _validate_kernel_source(value: Any) -> dict[str, Any]:
    kernel = _exact(value, KERNEL_SOURCE_KEYS, "kernelSource")
    _digest(kernel["compressedSha256"], "kernel compressed digest")
    _digest(kernel["imageSha256"], "kernel image digest")
    _digest(kernel["packageSha256"], "kernel package digest")
    _count(kernel["compressedSizeBytes"], "kernel compressed sizeBytes")
    _count(kernel["imageSizeBytes"], "kernel image sizeBytes")
    if kernel["packageArtifactId"] != f"deb-{kernel['packageSha256']}":
        raise ImageBuilderAuthorityError(
            "kernel artifact ID does not derive from its package digest"
        )
    if kernel["compression"] != DETERMINISM["kernelDecompression"]:
        raise ImageBuilderAuthorityError("kernel compression differs from the declared step")
    if kernel["imageSizeBytes"] <= kernel["compressedSizeBytes"]:
        raise ImageBuilderAuthorityError(
            "the decompressed kernel is not larger than the compressed one"
        )
    member = kernel["memberPath"]
    if not isinstance(member, str) or not member.startswith("./boot/"):
        raise ImageBuilderAuthorityError("kernel member path is not under ./boot/")
    return kernel


def verify_builder_scripts(
    authority: dict[str, Any], *, repo_root: pathlib.Path = REPO_ROOT
) -> list[str]:
    """Re-hash every pinned builder script and report the ones that drifted."""

    drifted: list[str] = []
    for row in authority["builderScripts"]:
        candidate = repo_root / row["path"]
        if not candidate.is_file():
            drifted.append(row["path"])
            continue
        raw = candidate.read_bytes()
        if sha256_bytes(raw) != row["sha256"] or len(raw) != row["sizeBytes"]:
            drifted.append(row["path"])
    return drifted


def verify_source_lock_binding(
    authority: dict[str, Any], *, repo_root: pathlib.Path = REPO_ROOT
) -> None:
    """Bind every pinned package to the committed source lock.

    This is the check CI can actually run.  The CAS holding the package bytes is
    gitignored, so a clean runner cannot extract anything -- but it can prove that
    the packages named here are the packages the lock already froze, which is what
    stops a tool being pinned to a deb nobody reviewed.
    """

    declared = authority["sourceLock"]
    lock_path = repo_root / declared["path"]
    raw = lock_path.read_bytes()
    if sha256_bytes(raw) != declared["sha256"] or len(raw) != declared["sizeBytes"]:
        raise ImageBuilderAuthorityError("the committed source lock differs from its pin")
    lock = json.loads(raw.decode("utf-8"))
    artifacts = {row["id"]: row for row in lock["artifacts"]}
    if len(artifacts) != declared["artifactCount"]:
        raise ImageBuilderAuthorityError("source lock artifact count differs from its pin")
    packages = {row["name"]: row for row in lock["ubuntu"]["packages"]}

    pinned = list(authority["toolBinaries"]) + [authority["kernelSource"]]
    for row in pinned:
        artifact = artifacts.get(row["packageArtifactId"])
        if artifact is None:
            raise ImageBuilderAuthorityError(
                f"pinned package is absent from the source lock: {row['package']}"
            )
        if artifact["sha256"] != row["packageSha256"]:
            raise ImageBuilderAuthorityError(
                f"pinned package digest differs from the source lock: {row['package']}"
            )
        package = packages.get(row["package"])
        if package is None or package["artifactId"] != row["packageArtifactId"]:
            raise ImageBuilderAuthorityError(
                f"pinned package name does not resolve in the source lock: {row['package']}"
            )
        # A version recorded here that disagrees with the lock would let the
        # document read as one release while the bytes are another.
        if package["version"] != row["version"]:
            raise ImageBuilderAuthorityError(
                f"pinned package version differs from the source lock: {row['package']}"
            )


def _deb_members(raw: bytes) -> dict[str, bytes]:
    if raw[:8] != b"!<arch>\n":
        raise ImageBuilderAuthorityError("package is not an ar archive")
    offset, members = 8, {}
    while offset < len(raw):
        header = raw[offset : offset + 60]
        if len(header) < 60:
            raise ImageBuilderAuthorityError("package ar header is truncated")
        name = header[0:16].decode("ascii", errors="replace").strip()
        size = int(header[48:58].decode("ascii", errors="replace").strip())
        members[name] = raw[offset + 60 : offset + 60 + size]
        offset += 60 + size + (size & 1)
    return members


def _pinned_members(authority: dict[str, Any]) -> list[dict[str, Any]]:
    """Flatten the two differently-shaped pin families into one checkable list.

    Tool binaries are consumed as-is; the kernel is consumed after a gzip step, so
    it carries a second expected digest.  Normalizing here keeps the verifier from
    guessing which shape it holds.
    """

    rows = [
        {
            "expectedSha256": row["sha256"],
            "expectedSizeBytes": row["sizeBytes"],
            "gunzipSha256": None,
            "gunzipSizeBytes": None,
            "label": f"{row['package']}:{row['role']}",
            "memberPath": row["memberPath"],
            "packageSha256": row["packageSha256"],
        }
        for row in authority["toolBinaries"]
    ]
    kernel = authority["kernelSource"]
    rows.append(
        {
            "expectedSha256": kernel["compressedSha256"],
            "expectedSizeBytes": kernel["compressedSizeBytes"],
            "gunzipSha256": kernel["imageSha256"],
            "gunzipSizeBytes": kernel["imageSizeBytes"],
            "label": f"{kernel['package']}:kernel",
            "memberPath": kernel["memberPath"],
            "packageSha256": kernel["packageSha256"],
        }
    )
    return rows


def _extract_member(
    raw: bytes, member_path: str, *, zstd: pathlib.Path
) -> tuple[Optional[bytes], Optional[str]]:
    members = _deb_members(raw)
    data_name = next((name for name in members if name.startswith("data.tar")), None)
    if data_name is None or not data_name.endswith(".zst"):
        return None, "no zstd data member"
    with tempfile.TemporaryDirectory(prefix="boole-image-builder-verify-") as scratch:
        directory = pathlib.Path(scratch)
        compressed = directory / data_name
        expanded = directory / "data.tar"
        compressed.write_bytes(members[data_name])
        completed = subprocess.run(
            [str(zstd), "-d", "-q", "-f", str(compressed), "-o", str(expanded)],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
            timeout=600,
        )
        if completed.returncode != 0:
            return None, "zstd refused the data member"
        with tarfile.open(expanded) as archive:
            try:
                member = archive.getmember(member_path)
            except KeyError:
                return None, f"{member_path} is absent"
            if not member.isfile():
                # A symlink here is the `mkfs.ext4` failure the module docstring warns about.
                return None, f"{member_path} is not a regular file"
            return archive.extractfile(member).read(), None


def verify_tool_bytes(
    authority: dict[str, Any], *, cas_root: pathlib.Path, zstd: pathlib.Path
) -> list[str]:
    """Extract each pinned member from its pinned package and confirm the digest.

    This needs the content-addressed store, which is gitignored, so it is a local
    verification rather than a CI gate -- which is why `toolByteProvenanceVerifiedInCi`
    is one of the boundaries that stays false.
    """

    failures: list[str] = []
    for row in _pinned_members(authority):
        label = row["label"]
        blob = cas_root / "sha256" / row["packageSha256"]
        if not blob.is_file():
            failures.append(f"{label}: package absent from the CAS")
            continue
        raw = blob.read_bytes()
        if sha256_bytes(raw) != row["packageSha256"]:
            failures.append(f"{label}: CAS bytes differ from the pinned digest")
            continue
        extracted, reason = _extract_member(raw, row["memberPath"], zstd=zstd)
        if extracted is None:
            failures.append(f"{label}: {reason}")
            continue
        if (
            sha256_bytes(extracted) != row["expectedSha256"]
            or len(extracted) != row["expectedSizeBytes"]
        ):
            failures.append(f"{label}: {row['memberPath']} digest differs")
            continue
        if row["gunzipSha256"] is None:
            continue
        try:
            expanded = gzip.decompress(extracted)
        except (OSError, EOFError) as error:
            failures.append(f"{label}: gzip refused the pinned member ({error})")
            continue
        if (
            sha256_bytes(expanded) != row["gunzipSha256"]
            or len(expanded) != row["gunzipSizeBytes"]
        ):
            failures.append(f"{label}: decompressed image digest differs")
            continue
        # The whole point of carrying the kernel here is that something later boots
        # it, so check it is the architecture claimed rather than trusting the name.
        if expanded[0x38:0x3C] != b"ARM\x64":
            failures.append(f"{label}: decompressed image is not an arm64 Linux Image")
    return failures


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="validate the authority")
    parser.add_argument(
        "--verify-tool-bytes",
        action="store_true",
        help="extract each pinned member from the CAS and confirm its digest",
    )
    parser.add_argument("--cas", type=pathlib.Path, help="content-addressed store root")
    parser.add_argument("--zstd", type=pathlib.Path, help="zstd executable used for extraction")
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    args = _parser().parse_args(argv)
    authority = load_authority()
    drifted = verify_builder_scripts(authority)
    if drifted:
        raise ImageBuilderAuthorityError(
            "pinned builder script drifted: " + ", ".join(sorted(drifted))
        )
    verify_source_lock_binding(authority)
    if args.verify_tool_bytes:
        if args.cas is None or args.zstd is None:
            raise ImageBuilderAuthorityError("--verify-tool-bytes needs --cas and --zstd")
        failures = verify_tool_bytes(authority, cas_root=args.cas, zstd=args.zstd)
        if failures:
            raise ImageBuilderAuthorityError(
                "pinned tool bytes differ: " + "; ".join(failures)
            )
        print(
            f"image builder authority: {AUTHORITY_SHA256} "
            f"tools={len(authority['toolBinaries'])} toolBytes=verified-locally built=no"
        )
        return 0
    print(
        f"image builder authority: {AUTHORITY_SHA256} "
        f"tools={len(authority['toolBinaries'])} "
        f"scripts={len(authority['builderScripts'])} built=no"
    )
    return 0


if __name__ == "__main__":  # pragma: no cover
    try:
        raise SystemExit(main())
    except ImageBuilderAuthorityError as error:
        print(f"image builder authority refused: {error}", file=sys.stderr)
        raise SystemExit(1) from error
