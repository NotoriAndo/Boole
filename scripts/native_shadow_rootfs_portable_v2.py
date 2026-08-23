#!/usr/bin/env python3
"""Create host-independent native-shadow rootfs authority documents.

Authority v1 captured the absolute paths and executable digests of the host's
``gpgv`` and ``zstd`` tools in otherwise portable source-lock bytes.  Authority
v2 keeps only logical tool roles in the source identity.  A concrete replay
records its chosen executable paths and digests in a separate run receipt.
"""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import os
import pathlib
import stat
import subprocess
import sys
from typing import Any, Optional

if __package__:
    from scripts import native_shadow_rootfs_acquire as acquire
    from scripts import native_shadow_rootfs_builder as rootfs
else:
    sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
    from scripts import native_shadow_rootfs_acquire as acquire
    from scripts import native_shadow_rootfs_builder as rootfs


SOURCE_LOCK_SCHEMA = "boole.native-shadow.runtime-rootfs-source-lock.v2"
SOURCE_LOCK_RELEASE = (
    "NATIVE-SHADOW-RUNTIME-ROOTFS-SOURCE-CLOSURE-V2-PORTABLE-NOT-ACTIVATABLE"
)
PORTABLE_V2_DERIVED_ENTRIES = (
    {
        "logicalPath": "/lib64",
        "kind": "symlink",
        "target": "usr/lib64",
        "mode": "0777",
        "uid": 0,
        "gid": 0,
    },
    {
        "logicalPath": "/usr/lib/x86_64-linux-gnu/libLLVM.so.22.1-rust-1.99.0-nightly",
        "kind": "symlink",
        "target": "../../../opt/boole/native-checker-toolchain/lib/libLLVM.so.22.1-rust-1.99.0-nightly",
        "mode": "0777",
        "uid": 0,
        "gid": 0,
    },
    {
        "logicalPath": "/usr/lib/x86_64-linux-gnu/librustc_driver-da0d54ffe246e605.so",
        "kind": "symlink",
        "target": "../../../opt/boole/native-checker-toolchain/lib/librustc_driver-da0d54ffe246e605.so",
        "mode": "0777",
        "uid": 0,
        "gid": 0,
    },
)
PLAN_SCHEMA = "boole.native-shadow.runtime-rootfs-portable-plan.v2"
RESOLUTION_SCHEMA = "boole.native-shadow.runtime-rootfs-resolution.v2"
EXPECTATION_SCHEMA = "boole.native-shadow.runtime-rootfs-replay-expectation.v2"
EXPECTED_TOOL_ROLES = ["gpgv", "zstd"]


class PortableAuthorityError(ValueError):
    """A portable authority document violates the v2 contract."""


def _sha256(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def _sha(value: Any, context: str) -> str:
    if (
        not isinstance(value, str)
        or len(value) != 64
        or any(character not in "0123456789abcdef" for character in value)
    ):
        raise PortableAuthorityError(f"{context} is not lowercase SHA-256")
    return value


def _exact(value: Any, keys: set[str], context: str) -> dict[str, Any]:
    if not isinstance(value, dict) or set(value) != keys:
        raise PortableAuthorityError(f"{context} keys differ")
    return value


def _read_canonical(path: pathlib.Path, context: str) -> tuple[dict[str, Any], bytes]:
    try:
        raw = path.read_bytes()
        value = rootfs.load_json_exact(raw, context, require_canonical=True)
    except (OSError, rootfs.RootfsBuildError) as exc:
        raise PortableAuthorityError(f"cannot read canonical {context}") from exc
    if not isinstance(value, dict):
        raise PortableAuthorityError(f"{context} must be an object")
    return value, raw


def _runtime_executable(path: pathlib.Path, role: str) -> tuple[pathlib.Path, bytes]:
    """Read one concrete replay tool without following a symbolic link."""

    try:
        resolved = path.resolve(strict=True)
        metadata = path.lstat()
        if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISREG(metadata.st_mode):
            raise PortableAuthorityError(f"runtime {role} must be a regular non-symlink")
        if metadata.st_mode & 0o111 == 0:
            raise PortableAuthorityError(f"runtime {role} is not executable")
        flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
        descriptor = os.open(path, flags)
        try:
            raw = b""
            while True:
                chunk = os.read(descriptor, 1024 * 1024)
                if not chunk:
                    break
                raw += chunk
        finally:
            os.close(descriptor)
    except PortableAuthorityError:
        raise
    except OSError as exc:
        raise PortableAuthorityError(f"cannot read runtime {role}") from exc
    return resolved, raw


def _runtime_tool_version(path: pathlib.Path, role: str) -> str:
    try:
        completed = subprocess.run(
            [str(path), "--version"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            env={"LANG": "C", "LC_ALL": "C", "PATH": "/usr/bin:/bin"},
            timeout=5,
            check=False,
        )
    except (OSError, subprocess.SubprocessError) as exc:
        raise PortableAuthorityError(f"cannot identify runtime {role}") from exc
    if completed.returncode != 0 or len(completed.stdout) > 4096:
        raise PortableAuthorityError(f"runtime {role} version probe failed")
    try:
        lines = completed.stdout.decode("utf-8", errors="strict").splitlines()
    except UnicodeError as exc:
        raise PortableAuthorityError(f"runtime {role} version is not UTF-8") from exc
    if not lines or not lines[0] or len(lines[0].encode("utf-8")) > 512:
        raise PortableAuthorityError(f"runtime {role} version is invalid")
    return lines[0]


def portable_source_lock_from_v1(v1_lock: dict[str, Any]) -> dict[str, Any]:
    """Remove replay-host identity and close the v2 usrmerge loader alias."""

    if not isinstance(v1_lock, dict):
        raise PortableAuthorityError("v1 source lock must be an object")
    result = copy.deepcopy(v1_lock)
    if (
        result.get("schema")
        != "boole.native-shadow.runtime-rootfs-source-lock.v1"
        or result.get("activationAllowed") is not False
    ):
        raise PortableAuthorityError("v1 source lock identity differs")
    try:
        verification = result["ubuntu"]["verification"]
        recipe = result["buildRecipe"]
        for key in ("gpgvPath", "gpgvSha256"):
            verification.pop(key)
        for key in ("zstdPath", "zstdSha256"):
            recipe.pop(key)
    except (KeyError, TypeError) as exc:
        raise PortableAuthorityError("v1 runtime tool fields are missing") from exc
    verification["toolRole"] = "gpgv"
    recipe["zstdToolRole"] = "zstd"
    derived = result.setdefault("derivedEntries", [])
    if not isinstance(derived, list):
        raise PortableAuthorityError("v1 derived entries must be a list")
    successor_paths = {item["logicalPath"] for item in PORTABLE_V2_DERIVED_ENTRIES}
    if any(
        item.get("logicalPath") in successor_paths
        for item in derived
        if isinstance(item, dict)
    ):
        raise PortableAuthorityError("v1 source lock unexpectedly owns a v2 runtime alias")
    derived.extend(copy.deepcopy(PORTABLE_V2_DERIVED_ENTRIES))
    derived.sort(key=lambda item: item["logicalPath"])
    result["schema"] = SOURCE_LOCK_SCHEMA
    result["release"] = SOURCE_LOCK_RELEASE
    result["activationAllowed"] = False
    return result


def runtime_lock_v1_equivalent(runtime_lock: dict[str, Any]) -> dict[str, Any]:
    """Remove the one reviewed v2 filesystem fix before comparing to v1."""

    result = copy.deepcopy(runtime_lock)
    derived = result.get("derivedEntries")
    if not isinstance(derived, list):
        raise PortableAuthorityError("runtime derived entries must be a list")
    aliases = list(PORTABLE_V2_DERIVED_ENTRIES)
    successor_paths = {item["logicalPath"] for item in aliases}
    matches = [item for item in derived if item.get("logicalPath") in successor_paths]
    if matches != aliases:
        raise PortableAuthorityError("runtime successor aliases differ")
    result["derivedEntries"] = [
        item for item in derived if item.get("logicalPath") not in successor_paths
    ]
    return result


def portable_resolution_from_runtime(
    runtime_resolution: dict[str, Any],
    portable_plan: dict[str, Any],
    portable_plan_raw: bytes,
) -> dict[str, Any]:
    """Normalize a signed replay resolved with any concrete host tools."""

    if rootfs.canonical_json(portable_plan) != portable_plan_raw:
        raise PortableAuthorityError("portable plan is not canonical exact")
    _validate_plan(portable_plan)
    if (
        not isinstance(runtime_resolution, dict)
        or runtime_resolution.get("schema")
        != "boole.native-shadow.runtime-rootfs-resolution.v1"
        or runtime_resolution.get("snapshotId") != portable_plan["snapshotId"]
    ):
        raise PortableAuthorityError("runtime resolution identity differs")
    result = copy.deepcopy(runtime_resolution)
    result["schema"] = RESOLUTION_SCHEMA
    result["release"] = "NATIVE-SHADOW-RUNTIME-ROOTFS-RESOLUTION-V2-PORTABLE"
    result["activationAllowed"] = False
    result["bootstrapResolutionV1Sha256"] = portable_plan["bootstrapAuthority"][
        "signedResolutionV1Sha256"
    ]
    result["planSha256"] = _sha256(portable_plan_raw)
    return result


def materialize_runtime_lock(
    portable_lock: dict[str, Any],
    portable_raw: bytes,
    gpgv_path: pathlib.Path,
    zstd_path: pathlib.Path,
) -> tuple[dict[str, Any], dict[str, Any]]:
    """Inject replay-local tools into an ephemeral v1 builder input.

    The returned runtime lock is never source authority.  Its host-specific
    path and digest are recorded in the returned receipt and are absent from
    ``portable_raw``.
    """

    if rootfs.canonical_json(portable_lock) != portable_raw:
        raise PortableAuthorityError("portable source lock is not canonical exact")
    if (
        portable_lock.get("schema") != SOURCE_LOCK_SCHEMA
        or portable_lock.get("release") != SOURCE_LOCK_RELEASE
        or portable_lock.get("activationAllowed") is not False
    ):
        raise PortableAuthorityError("portable source lock identity differs")
    try:
        verification = portable_lock["ubuntu"]["verification"]
        recipe = portable_lock["buildRecipe"]
    except (KeyError, TypeError) as exc:
        raise PortableAuthorityError("portable runtime tool roles are missing") from exc
    if verification != {"toolRole": "gpgv"} or recipe.get("zstdToolRole") != "zstd":
        raise PortableAuthorityError("portable runtime tool roles differ")

    gpgv, gpgv_raw = _runtime_executable(gpgv_path, "gpgv")
    zstd, zstd_raw = _runtime_executable(zstd_path, "zstd")
    runtime = copy.deepcopy(portable_lock)
    runtime["schema"] = rootfs.LOCK_SCHEMA
    runtime["release"] = (
        "NATIVE-SHADOW-RUNTIME-ROOTFS-SOURCE-CLOSURE-COMPLETE-NOT-ACTIVATABLE"
    )
    runtime["ubuntu"]["verification"] = {
        "gpgvPath": str(gpgv),
        "gpgvSha256": _sha256(gpgv_raw),
    }
    runtime["buildRecipe"].pop("zstdToolRole")
    runtime["buildRecipe"]["zstdPath"] = str(zstd)
    runtime["buildRecipe"]["zstdSha256"] = _sha256(zstd_raw)
    runtime_raw = rootfs.canonical_json(runtime)
    receipt = {
        "schema": "boole.native-shadow.runtime-rootfs-replay-run-receipt.v2",
        "authorityStatus": "RUNTIME-TOOLS-BOUND-NOT-ACTIVATABLE",
        "activationAllowed": False,
        "productionByteProvenanceComplete": False,
        "portableSourceLockSha256": _sha256(portable_raw),
        "runtimeLockSha256": _sha256(runtime_raw),
        "tools": {
            "gpgv": {
                "path": str(gpgv),
                "sha256": _sha256(gpgv_raw),
                "version": _runtime_tool_version(gpgv, "gpgv"),
            },
            "zstd": {
                "path": str(zstd),
                "sha256": _sha256(zstd_raw),
                "version": _runtime_tool_version(zstd, "zstd"),
            },
        },
    }
    return runtime, receipt


def runtime_plan_from_v1(
    v1_plan: dict[str, Any],
    gpgv_path: pathlib.Path,
    zstd_path: pathlib.Path,
) -> tuple[dict[str, Any], dict[str, Any]]:
    """Bind concrete tools to an ephemeral copy of the exact v1 signed plan."""

    if v1_plan.get("schema") != acquire.PLAN_SCHEMA:
        raise PortableAuthorityError("v1 acquisition plan identity differs")
    gpgv, gpgv_raw = _runtime_executable(gpgv_path, "gpgv")
    zstd, zstd_raw = _runtime_executable(zstd_path, "zstd")
    runtime = copy.deepcopy(v1_plan)
    runtime["toolDigests"] = {
        "gpgvSha256": _sha256(gpgv_raw),
        "zstdSha256": _sha256(zstd_raw),
    }
    receipt = {
        "gpgv": {
            "path": str(gpgv),
            "sha256": _sha256(gpgv_raw),
            "version": _runtime_tool_version(gpgv, "gpgv"),
        },
        "zstd": {
            "path": str(zstd),
            "sha256": _sha256(zstd_raw),
            "version": _runtime_tool_version(zstd, "zstd"),
        },
    }
    return runtime, receipt


def _validate_plan(plan: dict[str, Any]) -> None:
    _exact(
        plan,
        {
            "schema",
            "release",
            "activationAllowed",
            "snapshotId",
            "builderSha256",
            "bootstrapAuthority",
            "runtimeToolRoles",
            "portabilityPolicy",
            "replayPolicy",
        },
        "portable plan",
    )
    bootstrap = _exact(
        plan["bootstrapAuthority"],
        {
            "acquisitionPlanV1Sha256",
            "completeSourceLockV1Sha256",
            "signedResolutionV1Sha256",
            "sourceClosureResultV1Sha256",
            "sourceLockScaffoldV1Sha256",
        },
        "portable plan bootstrapAuthority",
    )
    for key, digest in bootstrap.items():
        _sha(digest, f"bootstrapAuthority.{key}")
    _sha(plan["builderSha256"], "portable plan builderSha256")
    if (
        plan["schema"] != PLAN_SCHEMA
        or plan["release"]
        != "NATIVE-SHADOW-RUNTIME-ROOTFS-PORTABLE-SUCCESSOR-V2-QUALIFICATION"
        or plan["activationAllowed"] is not False
        or plan["snapshotId"] != "20240425T160000Z"
        or plan["runtimeToolRoles"] != EXPECTED_TOOL_ROLES
        or plan["portabilityPolicy"]
        != {
            "runtimeToolIdentity": "run-receipt-only",
            "sourceIdentityIncludesRuntimeToolPath": False,
            "sourceIdentityIncludesRuntimeToolDigest": False,
        }
        or plan["replayPolicy"]
        != {
            "automaticAdoptionOnMismatch": False,
            "exactExpectedOutputRequired": True,
            "successorNotNewSourceAuthority": True,
        }
    ):
        raise PortableAuthorityError("portable plan policy differs")


def _validate_source_lock(lock: dict[str, Any]) -> None:
    if (
        lock.get("schema") != SOURCE_LOCK_SCHEMA
        or lock.get("release") != SOURCE_LOCK_RELEASE
        or lock.get("activationAllowed") is not False
    ):
        raise PortableAuthorityError("portable source lock identity differs")
    try:
        verification = lock["ubuntu"]["verification"]
        recipe = lock["buildRecipe"]
    except (KeyError, TypeError) as exc:
        raise PortableAuthorityError("portable source lock tool roles are missing") from exc
    if verification != {"toolRole": "gpgv"}:
        raise PortableAuthorityError("portable gpgv role differs")
    if recipe.get("zstdToolRole") != "zstd":
        raise PortableAuthorityError("portable zstd role differs")
    for key in ("zstdPath", "zstdSha256"):
        if key in recipe:
            raise PortableAuthorityError(f"portable source lock contains {key}")


def _validate_resolution_source_binding(
    plan: dict[str, Any],
    resolution: dict[str, Any],
    source_lock: dict[str, Any],
) -> None:
    """Bind signed package decisions to every complete source-lock artifact."""

    try:
        artifacts = {item["id"]: item for item in source_lock["artifacts"]}
        source_packages = source_lock["ubuntu"]["packages"]
        resolution_packages = resolution["packages"]
        keyring = resolution["keyring"]
        recipe = source_lock["buildRecipe"]
    except (KeyError, TypeError) as exc:
        raise PortableAuthorityError("portable package closure is incomplete") from exc
    if len(artifacts) != len(source_lock["artifacts"]):
        raise PortableAuthorityError("portable package closure has duplicate artifacts")
    expected_packages: list[dict[str, Any]] = []
    for row in resolution_packages:
        try:
            artifact_id = row["artifactId"]
            artifact = artifacts[artifact_id]
            expected_artifact = {
                "id": artifact_id,
                "kind": "deb",
                "sha256": row["artifactSha256"],
                "sizeBytes": row["artifactSizeBytes"],
            }
        except (KeyError, TypeError) as exc:
            raise PortableAuthorityError("portable package closure is incomplete") from exc
        if artifact != expected_artifact:
            raise PortableAuthorityError("portable package closure artifact differs")
        expected_packages.append(
            {
                key: value
                for key, value in row.items()
                if key not in {"artifactSha256", "artifactSizeBytes"}
            }
        )
    if rootfs.canonical_json(source_packages) != rootfs.canonical_json(
        sorted(expected_packages, key=lambda item: item["packageId"])
    ):
        raise PortableAuthorityError("portable package closure decisions differ")
    try:
        keyring_artifact = artifacts[keyring["artifactId"]]
    except (KeyError, TypeError) as exc:
        raise PortableAuthorityError("portable package closure keyring is absent") from exc
    if keyring_artifact != {
        "id": keyring["artifactId"],
        "kind": "ubuntu-keyring",
        "sha256": keyring["sha256"],
        "sizeBytes": keyring["sizeBytes"],
    }:
        raise PortableAuthorityError("portable package closure keyring differs")
    if (
        source_lock["ubuntu"].get("snapshot") != resolution.get("snapshotTime")
        or source_lock["ubuntu"].get("seedPackageIds")
        != resolution.get("seedPackageIds")
        or recipe.get("builderSha256") != plan["builderSha256"]
    ):
        raise PortableAuthorityError("portable package closure authority differs")


def _validate_tracked_bootstrap(
    plan: dict[str, Any], source_lock: dict[str, Any], builder_path: pathlib.Path
) -> None:
    """Re-anchor portable bytes in the tracked v1 plan and repository authority."""

    repository_root = builder_path.resolve().parents[1]
    v1_plan_path = (
        repository_root
        / "native/containment/native-shadow-runtime-rootfs-acquisition-plan-v1.json"
    )
    v1_scaffold_path = (
        repository_root
        / "native/containment/native-shadow-runtime-rootfs-source-lock-v1.json"
    )
    acquirer_path = repository_root / "scripts/native_shadow_rootfs_acquire.py"
    try:
        v1_plan_raw = v1_plan_path.read_bytes()
        scaffold_raw = v1_scaffold_path.read_bytes()
        v1_plan = acquire.load_plan(v1_plan_raw, builder_path, acquirer_path)
    except (OSError, acquire.AcquisitionError) as exc:
        raise PortableAuthorityError("tracked v1 bootstrap authority differs") from exc
    bootstrap = plan["bootstrapAuthority"]
    if (
        _sha256(v1_plan_raw) != bootstrap["acquisitionPlanV1Sha256"]
        or _sha256(scaffold_raw) != bootstrap["sourceLockScaffoldV1Sha256"]
    ):
        raise PortableAuthorityError("tracked v1 bootstrap digest differs")

    artifacts = {item["id"]: item for item in source_lock["artifacts"]}
    for row in v1_plan["rustArtifacts"]:
        if artifacts.get(row["artifactId"]) != {
            "id": row["artifactId"],
            "kind": "rust-dist",
            "sha256": row["sha256"],
            "sizeBytes": row["sizeBytes"],
        }:
            raise PortableAuthorityError("portable bootstrap Rust artifact differs")
    for key, kind in (
        ("inRelease", "ubuntu-inrelease"),
        ("packagesIndex", "ubuntu-packages-index"),
    ):
        row = v1_plan["repository"][key]
        if artifacts.get(row["artifactId"]) != {
            "id": row["artifactId"],
            "kind": kind,
            "sha256": row["sha256"],
            "sizeBytes": row["sizeBytes"],
        }:
            raise PortableAuthorityError("portable bootstrap Ubuntu artifact differs")

    bindings: dict[str, str] = {}
    for row in source_lock.get("authorityBindings", []):
        try:
            source = row["sourcePath"]
            raw = (repository_root / source).read_bytes()
        except (KeyError, OSError, TypeError) as exc:
            raise PortableAuthorityError("portable repository binding is absent") from exc
        if _sha256(raw) != row.get("sha256"):
            raise PortableAuthorityError("portable repository binding bytes differ")
        bindings[source] = row["sha256"]
    for row in source_lock.get("trackedFiles", []):
        if bindings.get(row.get("sourcePath")) != row.get("sha256"):
            raise PortableAuthorityError("portable tracked authority binding differs")


def load_authority_set(
    plan_path: pathlib.Path,
    resolution_path: pathlib.Path,
    source_lock_path: pathlib.Path,
    expectation_path: pathlib.Path,
    builder_path: pathlib.Path,
) -> dict[str, dict[str, Any]]:
    """Load and cross-bind the four tracked portable successor documents."""

    plan, plan_raw = _read_canonical(plan_path, "portable plan")
    resolution, resolution_raw = _read_canonical(
        resolution_path, "portable resolution"
    )
    source_lock, source_lock_raw = _read_canonical(
        source_lock_path, "portable source lock"
    )
    expectation, expectation_raw = _read_canonical(
        expectation_path, "replay expectation"
    )
    _validate_plan(plan)
    _validate_source_lock(source_lock)
    if _sha256(builder_path.read_bytes()) != plan["builderSha256"]:
        raise PortableAuthorityError("portable plan builder bytes differ")

    if (
        resolution.get("schema") != RESOLUTION_SCHEMA
        or resolution.get("release")
        != "NATIVE-SHADOW-RUNTIME-ROOTFS-RESOLUTION-V2-PORTABLE"
        or resolution.get("activationAllowed") is not False
        or resolution.get("snapshotId") != plan["snapshotId"]
        or resolution.get("planSha256") != _sha256(plan_raw)
        or resolution.get("bootstrapResolutionV1Sha256")
        != plan["bootstrapAuthority"]["signedResolutionV1Sha256"]
    ):
        raise PortableAuthorityError("portable resolution binding differs")
    if not isinstance(resolution.get("packages"), list) or not isinstance(
        resolution.get("seedPackageIds"), list
    ):
        raise PortableAuthorityError("portable resolution package closure differs")
    _validate_tracked_bootstrap(plan, source_lock, builder_path)
    _validate_resolution_source_binding(plan, resolution, source_lock)

    _exact(
        expectation,
        {
            "schema",
            "release",
            "activationAllowed",
            "productionByteProvenanceComplete",
            "authority",
            "expectedOutput",
            "mismatchPolicy",
        },
        "replay expectation",
    )
    authority = _exact(
        expectation["authority"],
        {
            "bootstrapAuthority",
            "builderSha256",
            "portablePlanSha256",
            "portableResolutionSha256",
            "portableSourceLockSha256",
        },
        "replay expectation authority",
    )
    if (
        expectation["schema"] != EXPECTATION_SCHEMA
        or expectation["release"]
        != "NATIVE-SHADOW-RUNTIME-ROOTFS-REPLAY-EXPECTATION-V2-FROZEN"
        or expectation["activationAllowed"] is not False
        or expectation["productionByteProvenanceComplete"] is not False
        or authority["bootstrapAuthority"] != plan["bootstrapAuthority"]
        or authority["builderSha256"] != plan["builderSha256"]
        or authority["portablePlanSha256"] != _sha256(plan_raw)
        or authority["portableResolutionSha256"] != _sha256(resolution_raw)
        or authority["portableSourceLockSha256"] != _sha256(source_lock_raw)
        or expectation["mismatchPolicy"]
        != {"action": "reject-no-adoption", "automaticUpdateAllowed": False}
    ):
        raise PortableAuthorityError("replay expectation authority differs")
    expected_build_receipt(expectation)
    return {
        "plan": plan,
        "resolution": resolution,
        "sourceLock": source_lock,
        "expectation": expectation,
        "raw": {
            "plan": plan_raw,
            "resolution": resolution_raw,
            "sourceLock": source_lock_raw,
            "expectation": expectation_raw,
        },
    }


_OUTPUT_KEYS = {
    "rootfsContentManifestSha256",
    "rootfsContentManifestSizeBytes",
    "layerDigest",
    "layerSizeBytes",
    "configDigest",
    "configSizeBytes",
    "manifestDigest",
    "manifestSizeBytes",
    "indexSha256",
    "indexSizeBytes",
    "layerCount",
    "parentLayerCount",
}


def expected_build_receipt(expectation: dict[str, Any]) -> dict[str, Any]:
    """Return the exact frozen output tuple, validating its shape first."""

    output = _exact(expectation.get("expectedOutput"), _OUTPUT_KEYS, "expected output")
    for key in (
        "rootfsContentManifestSha256",
        "indexSha256",
    ):
        _sha(output[key], f"expectedOutput.{key}")
    for key in ("layerDigest", "configDigest", "manifestDigest"):
        value = output[key]
        if not isinstance(value, str) or not value.startswith("sha256:"):
            raise PortableAuthorityError(f"expectedOutput.{key} differs")
        _sha(value[7:], f"expectedOutput.{key}")
    for key in _OUTPUT_KEYS - {
        "rootfsContentManifestSha256",
        "indexSha256",
        "layerDigest",
        "configDigest",
        "manifestDigest",
    }:
        value = output[key]
        if isinstance(value, bool) or not isinstance(value, int) or value < 0:
            raise PortableAuthorityError(f"expectedOutput.{key} differs")
    if output["layerCount"] != 1 or output["parentLayerCount"] != 0:
        raise PortableAuthorityError("expected output layer cardinality differs")
    return copy.deepcopy(output)


def verify_replay_output(expectation: dict[str, Any], build_receipt: dict[str, Any]) -> None:
    """Reject any replay output drift; never adopt observed bytes automatically."""

    expected = expected_build_receipt(expectation)
    for key, value in expected.items():
        if build_receipt.get(key) != value:
            raise PortableAuthorityError(f"replay output {key} differs")


def verify_replay_receipts(
    expectation: dict[str, Any],
    build_receipt: dict[str, Any],
    run_receipt: dict[str, Any],
) -> None:
    """Bind exact OCI output to the frozen builder, source authority, and run."""

    verify_replay_output(expectation, build_receipt)
    authority = expectation["authority"]
    if build_receipt.get("builderSha256") != authority["builderSha256"]:
        raise PortableAuthorityError("replay builder authority differs")
    if (
        build_receipt.get("activationAllowed") is not False
        or build_receipt.get("productionByteProvenanceComplete") is not False
        or run_receipt.get("activationAllowed") is not False
        or run_receipt.get("productionByteProvenanceComplete") is not False
        or run_receipt.get("ephemeralRuntimeLock") is not True
    ):
        raise PortableAuthorityError("replay activation/provenance boundary differs")
    runtime_lock_sha256 = _sha(
        run_receipt.get("runtimeLockSha256"), "run receipt runtimeLockSha256"
    )
    if build_receipt.get("sourceLockSha256") != runtime_lock_sha256:
        raise PortableAuthorityError("replay ephemeral source lock differs")
    if (
        run_receipt.get("portableSourceLockSha256")
        != authority["portableSourceLockSha256"]
    ):
        raise PortableAuthorityError("replay portable source authority differs")
    expected_run_authority = {
        key: authority[key]
        for key in (
            "builderSha256",
            "portablePlanSha256",
            "portableResolutionSha256",
            "portableSourceLockSha256",
        )
    }
    if run_receipt.get("authority") != expected_run_authority:
        raise PortableAuthorityError("replay run authority differs")


def _write_canonical(path: pathlib.Path, value: dict[str, Any]) -> None:
    try:
        acquire._write_bytes_atomic(path, rootfs.canonical_json(value))
    except (OSError, acquire.AcquisitionError) as exc:
        raise PortableAuthorityError(f"cannot write canonical output: {path}") from exc


def _load_v1_runtime_plan(
    v1_plan_path: pathlib.Path,
    builder_path: pathlib.Path,
    acquirer_path: pathlib.Path,
    gpgv_path: pathlib.Path,
    zstd_path: pathlib.Path,
) -> tuple[dict[str, Any], dict[str, Any]]:
    try:
        v1_plan = acquire.load_plan(
            v1_plan_path.read_bytes(), builder_path, acquirer_path
        )
    except (OSError, acquire.AcquisitionError) as exc:
        raise PortableAuthorityError("exact v1 bootstrap acquisition plan differs") from exc
    return runtime_plan_from_v1(v1_plan, gpgv_path, zstd_path)


def _compare_portable_resolution(
    runtime_resolution: dict[str, Any], authority: dict[str, Any]
) -> None:
    normalized = portable_resolution_from_runtime(
        runtime_resolution,
        authority["plan"],
        authority["raw"]["plan"],
    )
    if rootfs.canonical_json(normalized) != authority["raw"]["resolution"]:
        raise PortableAuthorityError(
            "signed runtime resolution differs from tracked portable resolution"
        )


def _add_authority_arguments(parser: argparse.ArgumentParser) -> None:
    root = pathlib.Path(__file__).resolve().parents[1]
    containment = root / "native/containment"
    parser.add_argument(
        "--portable-plan",
        type=pathlib.Path,
        default=containment / "native-shadow-runtime-rootfs-portable-plan-v2.json",
    )
    parser.add_argument(
        "--portable-resolution",
        type=pathlib.Path,
        default=containment / "native-shadow-runtime-rootfs-resolution-v2.json",
    )
    parser.add_argument(
        "--portable-source-lock",
        type=pathlib.Path,
        default=containment / "native-shadow-runtime-rootfs-source-lock-v2.json",
    )
    parser.add_argument(
        "--replay-expectation",
        type=pathlib.Path,
        default=containment
        / "native-shadow-runtime-rootfs-replay-expectation-v2.json",
    )
    parser.add_argument(
        "--builder",
        type=pathlib.Path,
        default=root / "scripts/native_shadow_rootfs_builder.py",
    )


def _parser() -> argparse.ArgumentParser:
    root = pathlib.Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    validate = commands.add_parser("validate")
    _add_authority_arguments(validate)
    for name in ("resolve", "fetch-payloads", "seal"):
        command = commands.add_parser(name)
        _add_authority_arguments(command)
        command.add_argument(
            "--v1-plan",
            type=pathlib.Path,
            default=root
            / "native/containment/native-shadow-runtime-rootfs-acquisition-plan-v1.json",
        )
        command.add_argument(
            "--acquirer",
            type=pathlib.Path,
            default=root / "scripts/native_shadow_rootfs_acquire.py",
        )
        command.add_argument("--cas", type=pathlib.Path, required=True)
        command.add_argument("--gpgv", type=pathlib.Path, required=True)
        command.add_argument("--zstd", type=pathlib.Path, required=True)
    resolve = commands.choices["resolve"]
    resolve.add_argument("--runtime-resolution-output", type=pathlib.Path, required=True)
    fetch = commands.choices["fetch-payloads"]
    fetch.add_argument("--runtime-resolution", type=pathlib.Path, required=True)
    seal = commands.choices["seal"]
    seal.add_argument("--runtime-resolution", type=pathlib.Path, required=True)
    seal.add_argument(
        "--v1-scaffold",
        type=pathlib.Path,
        default=root
        / "native/containment/native-shadow-runtime-rootfs-source-lock-v1.json",
    )
    seal.add_argument("--repo-root", type=pathlib.Path, default=root)
    seal.add_argument("--runtime-lock-output", type=pathlib.Path, required=True)
    seal.add_argument("--run-receipt-output", type=pathlib.Path, required=True)
    verify = commands.add_parser("verify-output")
    _add_authority_arguments(verify)
    verify.add_argument("--build-receipt", type=pathlib.Path, required=True)
    verify.add_argument("--run-receipt", type=pathlib.Path, required=True)
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    args = _parser().parse_args(argv)
    authority = load_authority_set(
        args.portable_plan,
        args.portable_resolution,
        args.portable_source_lock,
        args.replay_expectation,
        args.builder,
    )
    if args.command == "validate":
        print(
            json.dumps(
                {
                    "activationAllowed": False,
                    "authorityStatus": "PORTABLE-SUCCESSOR-V2-VALID-NOT-ACTIVATABLE",
                    "portablePlanSha256": _sha256(authority["raw"]["plan"]),
                    "portableResolutionSha256": _sha256(
                        authority["raw"]["resolution"]
                    ),
                    "portableSourceLockSha256": _sha256(
                        authority["raw"]["sourceLock"]
                    ),
                },
                sort_keys=True,
                separators=(",", ":"),
            )
        )
        return 0
    if args.command == "verify-output":
        build_receipt, _ = _read_canonical(args.build_receipt, "build receipt")
        run_receipt, _ = _read_canonical(args.run_receipt, "run receipt")
        verify_replay_receipts(
            authority["expectation"], build_receipt, run_receipt
        )
        print(
            '{"activationAllowed":false,"replayOutput":"EXACT-MATCH"}'
        )
        return 0

    runtime_plan, _ = _load_v1_runtime_plan(
        args.v1_plan, args.builder, args.acquirer, args.gpgv, args.zstd
    )
    if args.command == "resolve":
        try:
            runtime_resolution = acquire.resolve_from_cas(
                runtime_plan,
                args.cas,
                args.gpgv.resolve(),
                args.zstd.resolve(),
            )
        except acquire.AcquisitionError as exc:
            raise PortableAuthorityError(str(exc)) from exc
        _compare_portable_resolution(runtime_resolution, authority)
        _write_canonical(args.runtime_resolution_output, runtime_resolution)
        return 0

    try:
        runtime_resolution = acquire.replay_resolution_from_cas(
            runtime_plan,
            args.runtime_resolution,
            args.cas,
            args.gpgv.resolve(),
            args.zstd.resolve(),
        )
    except acquire.AcquisitionError as exc:
        raise PortableAuthorityError(str(exc)) from exc
    _compare_portable_resolution(runtime_resolution, authority)
    if args.command == "fetch-payloads":
        base_specs = acquire._specs(runtime_plan)
        package_specs = [
            acquire._package_spec(package, runtime_plan)
            for package in runtime_resolution["packages"]
        ]
        base_actual = sum(
            len(acquire._verified_cas_artifact(args.cas, spec))
            for spec in base_specs
        )
        budget = acquire.ArtifactBudget(
            runtime_plan["networkPolicy"],
            package_specs,
            initial_declared_bytes=sum(spec["sizeBytes"] for spec in base_specs),
            initial_actual_bytes=base_actual,
        )
        for spec in package_specs:
            acquire.fetch_artifact(
                args.cas, spec, runtime_plan["networkPolicy"]["allowedHosts"]
            )
            budget.account(
                len(acquire._verified_cas_artifact(args.cas, spec)),
                spec["artifactId"],
            )
        return 0

    try:
        sealed_runtime, sealed_raw = acquire.seal_candidate(
            runtime_plan,
            args.v1_scaffold,
            runtime_resolution,
            args.cas,
            args.repo_root,
            args.gpgv.resolve(),
            args.zstd.resolve(),
        )
    except acquire.AcquisitionError as exc:
        raise PortableAuthorityError(str(exc)) from exc
    if rootfs.canonical_json(portable_source_lock_from_v1(sealed_runtime)) != authority[
        "raw"
    ]["sourceLock"]:
        raise PortableAuthorityError(
            "sealed runtime source lock differs from portable successor authority"
        )
    runtime_lock, receipt = materialize_runtime_lock(
        authority["sourceLock"],
        authority["raw"]["sourceLock"],
        args.gpgv,
        args.zstd,
    )
    runtime_raw = rootfs.canonical_json(runtime_lock)
    if rootfs.canonical_json(runtime_lock_v1_equivalent(runtime_lock)) != sealed_raw:
        raise PortableAuthorityError("ephemeral runtime lock differs from signed seal")
    receipt["authority"] = {
        "builderSha256": authority["plan"]["builderSha256"],
        "portablePlanSha256": _sha256(authority["raw"]["plan"]),
        "portableResolutionSha256": _sha256(authority["raw"]["resolution"]),
        "portableSourceLockSha256": _sha256(authority["raw"]["sourceLock"]),
    }
    receipt["ephemeralRuntimeLock"] = True
    _write_canonical(args.runtime_lock_output, runtime_lock)
    _write_canonical(args.run_receipt_output, receipt)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except PortableAuthorityError as exc:
        print(f"native-shadow-rootfs-portable-v2: FAIL: {exc}", file=sys.stderr)
        raise SystemExit(2)
