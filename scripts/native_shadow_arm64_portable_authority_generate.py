#!/usr/bin/env python3
"""Generate the host-independent MAC.2 arm64 rootfs authority documents."""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import sys
from typing import Any

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

from scripts import native_shadow_rootfs_acquire_arm64_v1 as acquire
from scripts import native_shadow_rootfs_builder_arm64_v1 as rootfs
from scripts import native_shadow_rootfs_portable_arm64_v1 as portable


ROOT = pathlib.Path(__file__).resolve().parents[1]
CONTAINMENT = ROOT / "native/containment"
ACQUISITION_PLAN = (
    CONTAINMENT / "native-shadow-runtime-rootfs-acquisition-plan-arm64-v1.json"
)
SCAFFOLD = (
    CONTAINMENT
    / "native-shadow-runtime-rootfs-source-lock-arm64-v1-scaffold.json"
)
BUILDER = ROOT / "scripts/native_shadow_rootfs_builder_arm64_v1.py"
ACQUIRER = ROOT / "scripts/native_shadow_rootfs_acquire_arm64_v1.py"


def _sha(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def _canonical(value: Any) -> bytes:
    return rootfs.canonical_json(value)


def _read_canonical(path: pathlib.Path, context: str) -> tuple[dict[str, Any], bytes]:
    raw = path.read_bytes()
    value = rootfs.load_json_exact(raw, context, require_canonical=True)
    if not isinstance(value, dict):
        raise ValueError(f"{context} must be an object")
    return value, raw


def generate(
    runtime_resolution_path: pathlib.Path,
    runtime_source_lock_path: pathlib.Path,
) -> dict[pathlib.Path, bytes]:
    acquisition_plan, acquisition_raw = _read_canonical(
        ACQUISITION_PLAN, "arm64 acquisition plan"
    )
    acquire.load_plan(acquisition_raw, BUILDER, ACQUIRER)
    runtime_resolution, runtime_resolution_raw = _read_canonical(
        runtime_resolution_path, "arm64 runtime resolution"
    )
    runtime_lock, runtime_lock_raw = _read_canonical(
        runtime_source_lock_path, "arm64 runtime source lock"
    )
    if (
        runtime_resolution.get("schema")
        != "boole.native-shadow.runtime-rootfs-resolution.arm64.runtime.v1"
        or runtime_resolution.get("planSha256") != _sha(acquisition_raw)
        or runtime_lock.get("schema")
        != "boole.native-shadow.runtime-rootfs-source-lock.arm64.runtime.v1"
        or runtime_lock.get("activationAllowed") is not False
        or runtime_lock.get("platform")
        != {
            "debArchitecture": "arm64",
            "ociArchitecture": "arm64",
            "os": "linux",
            "rustTarget": "aarch64-unknown-linux-gnu",
        }
    ):
        raise ValueError("arm64 runtime bootstrap identity differs")

    source_result = {
        "activationAllowed": False,
        "authority": {
            "acquirerAuthoritySha256": acquisition_plan[
                "acquirerAuthoritySha256"
            ],
            "acquisitionPlanSha256": _sha(acquisition_raw),
            "builderSha256": _sha(BUILDER.read_bytes()),
            "ociVerifierSha256": _sha(
                (ROOT / "scripts/native_shadow_rootfs_oci_verify_arm64_v1.py").read_bytes()
            ),
            "portableProjectionSha256": _sha(
                (ROOT / "scripts/native_shadow_rootfs_portable_arm64_v1.py").read_bytes()
            ),
            "runtimeResolutionSha256": _sha(runtime_resolution_raw),
            "runtimeSourceLockSha256": _sha(runtime_lock_raw),
            "sourceLockScaffoldSha256": _sha(SCAFFOLD.read_bytes()),
        },
        "closure": {
            "artifactCount": len(runtime_lock["artifacts"]),
            "packageCount": len(runtime_resolution["packages"]),
            "totalArtifactBytes": sum(
                row["sizeBytes"] for row in runtime_lock["artifacts"]
            ),
        },
        "platform": {"architecture": "arm64", "os": "linux"},
        "productionByteProvenanceComplete": False,
        "release": "NATIVE-SHADOW-RUNTIME-ROOTFS-SOURCE-CLOSURE-ARM64-V1-SEALED",
        "schema": "boole.native-shadow.runtime-rootfs-source-closure-result.arm64.v1",
        "verification": {
            "rustArtifactsMatchedPinnedSha256": True,
            "ubuntuMetadataVerifiedByFrozenArchiveKey": True,
            "ubuntuPackageClosureResolvedFromSignedIndex": True,
        },
    }
    source_result_raw = _canonical(source_result)

    portable_plan = {
        "activationAllowed": False,
        "bootstrapAuthority": {
            "acquisitionPlanV1Sha256": _sha(acquisition_raw),
            "completeSourceLockV1Sha256": _sha(runtime_lock_raw),
            "signedResolutionV1Sha256": _sha(runtime_resolution_raw),
            "sourceClosureResultV1Sha256": _sha(source_result_raw),
            "sourceLockScaffoldV1Sha256": _sha(SCAFFOLD.read_bytes()),
        },
        "builderSha256": _sha(BUILDER.read_bytes()),
        "portabilityPolicy": {
            "runtimeToolIdentity": "run-receipt-only",
            "sourceIdentityIncludesRuntimeToolDigest": False,
            "sourceIdentityIncludesRuntimeToolPath": False,
        },
        "release": "NATIVE-SHADOW-RUNTIME-ROOTFS-PORTABLE-SUCCESSOR-ARM64-V1-QUALIFICATION",
        "replayPolicy": {
            "automaticAdoptionOnMismatch": False,
            "exactExpectedOutputRequired": True,
            "successorNotNewSourceAuthority": True,
        },
        "runtimeToolRoles": ["gpgv", "zstd"],
        "schema": "boole.native-shadow.runtime-rootfs-portable-plan.arm64.v1",
        "snapshotId": "20240425T160000Z",
    }
    portable_plan_raw = _canonical(portable_plan)
    portable._validate_plan(portable_plan)
    portable_resolution = portable.portable_resolution_from_runtime(
        runtime_resolution, portable_plan, portable_plan_raw
    )
    portable_source_lock = portable.portable_source_lock_from_v1(runtime_lock)

    return {
        CONTAINMENT
        / "native-shadow-runtime-rootfs-source-closure-result-arm64-v1.json": source_result_raw,
        CONTAINMENT
        / "native-shadow-runtime-rootfs-portable-plan-arm64-v1.json": portable_plan_raw,
        CONTAINMENT
        / "native-shadow-runtime-rootfs-resolution-arm64-v1.json": _canonical(
            portable_resolution
        ),
        CONTAINMENT
        / "native-shadow-runtime-rootfs-source-lock-arm64-v1.json": _canonical(
            portable_source_lock
        ),
    }


def replay_expectation(
    build_receipt_path: pathlib.Path,
    generated: dict[pathlib.Path, bytes] | None = None,
) -> bytes:
    plan_path = (
        CONTAINMENT / "native-shadow-runtime-rootfs-portable-plan-arm64-v1.json"
    )
    resolution_path = (
        CONTAINMENT / "native-shadow-runtime-rootfs-resolution-arm64-v1.json"
    )
    source_lock_path = (
        CONTAINMENT / "native-shadow-runtime-rootfs-source-lock-arm64-v1.json"
    )
    if generated is None:
        plan, plan_raw = _read_canonical(plan_path, "arm64 portable plan")
        _, resolution_raw = _read_canonical(
            resolution_path, "arm64 portable resolution"
        )
        _, source_lock_raw = _read_canonical(
            source_lock_path, "arm64 portable source lock"
        )
    else:
        plan_raw = generated[plan_path]
        resolution_raw = generated[resolution_path]
        source_lock_raw = generated[source_lock_path]
        plan = rootfs.load_json_exact(
            plan_raw, "generated arm64 portable plan", require_canonical=True
        )
    receipt, _ = _read_canonical(build_receipt_path, "arm64 build receipt")
    if (
        receipt.get("schema")
        != "boole.native-shadow.runtime-rootfs-build-receipt.arm64.v1"
        or receipt.get("builderSha256") != plan["builderSha256"]
        or receipt.get("activationAllowed") is not False
        or receipt.get("productionByteProvenanceComplete") is not False
        or receipt.get("layerCount") != 1
        or receipt.get("parentLayerCount") != 0
        or receipt.get("layerSizeBytes", 2 * 1024**3 + 1) > 2 * 1024**3
    ):
        raise ValueError("arm64 build receipt identity or bounds differ")
    expected_output = {
        key: receipt[key]
        for key in portable._OUTPUT_KEYS
    }
    expectation = {
        "activationAllowed": False,
        "authority": {
            "bootstrapAuthority": plan["bootstrapAuthority"],
            "builderSha256": plan["builderSha256"],
            "portablePlanSha256": _sha(plan_raw),
            "portableResolutionSha256": _sha(resolution_raw),
            "portableSourceLockSha256": _sha(source_lock_raw),
        },
        "expectedOutput": expected_output,
        "mismatchPolicy": {
            "action": "reject-no-adoption",
            "automaticUpdateAllowed": False,
        },
        "platform": {"architecture": "arm64", "os": "linux"},
        "productionByteProvenanceComplete": False,
        "release": "NATIVE-SHADOW-RUNTIME-ROOTFS-REPLAY-EXPECTATION-ARM64-V1-FROZEN",
        "schema": "boole.native-shadow.runtime-rootfs-replay-expectation.arm64.v1",
    }
    portable.expected_build_receipt(expectation)
    return _canonical(expectation)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime-resolution", type=pathlib.Path, required=True)
    parser.add_argument("--runtime-source-lock", type=pathlib.Path, required=True)
    parser.add_argument("--build-receipt", type=pathlib.Path)
    parser.add_argument("--write", action="store_true")
    args = parser.parse_args()
    outputs = generate(
        args.runtime_resolution, args.runtime_source_lock
    )
    if args.build_receipt is not None:
        outputs[
            CONTAINMENT
            / "native-shadow-runtime-rootfs-replay-expectation-arm64-v1.json"
        ] = replay_expectation(args.build_receipt, outputs)
    for path, raw in outputs.items():
        if args.write:
            path.write_bytes(raw)
        print(f"{_sha(raw)}  {path.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
