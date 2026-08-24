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
BASE_AUTHORITY_SHA256 = {
    "native/containment/native-shadow-closed-local-replay-registry-overlay-v1.json": "4c8673b18b0183cc4525ba73314b9860bb6a60792db9bef48c238b903a1486d5",
    "native/containment/native-shadow-closed-local-replay-grant-v1.json": "ebb7392ce8049ebd2beb8b9fde25023d53e96505c8d1fa55d1cbcc892104d291",
    "native/containment/native-shadow-local-execution-authority-v1.json": "e231a0558324fec409f955ec14924c286a5cb612db9893ce9f90ce0e865dcf7f",
    "native/containment/native-shadow-closed-local-replay-execution-authority-v1.json": "c4d25ccd3b4de15ebcb1bdda91b15f953ea054ad20f1a6713600618141b67ff0",
}

ARM64_POLICY = CONTAINMENT / "native-shadow-execution-policy-arm64-v1.json"
ARM64_TOOLCHAIN = CONTAINMENT / "native-shadow-toolchain-identity-arm64-v1.json"
ARM64_RELEASE = (
    ROOT
    / "native/checker/rust-tuple-struct-project-v1/RELEASE-MANIFEST-arm64-v1.json"
)
ARM64_REGISTRY = ROOT / "fixtures/native-shadow/registry-arm64-v1.json"


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


def _read_frozen_base(relative: str) -> dict[str, Any]:
    path = ROOT / relative
    raw = path.read_bytes()
    if _sha(raw) != BASE_AUTHORITY_SHA256[relative]:
        raise ValueError(f"frozen x86 authority differs: {relative}")
    value = json.loads(raw.decode("utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"frozen x86 authority is not an object: {relative}")
    return value


def dependent_authority(
    generated: dict[pathlib.Path, bytes],
) -> dict[pathlib.Path, bytes]:
    """Project the post-build authority chain after expectation is known."""

    expectation_path = (
        CONTAINMENT
        / "native-shadow-runtime-rootfs-replay-expectation-arm64-v1.json"
    )
    required = {
        CONTAINMENT / "native-shadow-runtime-rootfs-portable-plan-arm64-v1.json",
        CONTAINMENT / "native-shadow-runtime-rootfs-resolution-arm64-v1.json",
        CONTAINMENT / "native-shadow-runtime-rootfs-source-lock-arm64-v1.json",
        expectation_path,
    }
    if not required.issubset(generated):
        raise ValueError("arm64 post-build authority inputs are incomplete")

    arm_raw = {
        "policy": ARM64_POLICY.read_bytes(),
        "toolchain": ARM64_TOOLCHAIN.read_bytes(),
        "release": ARM64_RELEASE.read_bytes(),
        "registry": ARM64_REGISTRY.read_bytes(),
        "plan": generated[
            CONTAINMENT
            / "native-shadow-runtime-rootfs-portable-plan-arm64-v1.json"
        ],
        "resolution": generated[
            CONTAINMENT / "native-shadow-runtime-rootfs-resolution-arm64-v1.json"
        ],
        "lock": generated[
            CONTAINMENT / "native-shadow-runtime-rootfs-source-lock-arm64-v1.json"
        ],
        "expectation": generated[expectation_path],
    }
    for key, raw in arm_raw.items():
        rootfs.load_json_exact(raw, f"arm64 {key}", require_canonical=True)

    overlay = _read_frozen_base(
        "native/containment/native-shadow-closed-local-replay-registry-overlay-v1.json"
    )
    overlay["baseRegistrySha256"] = _sha(arm_raw["registry"])
    overlay["executionPolicySha256"] = _sha(arm_raw["policy"])
    overlay["toolchainIdentitySha256"] = _sha(arm_raw["toolchain"])
    for template in overlay["templates"]:
        template["checkerReleaseManifestSha256"] = _sha(arm_raw["release"])
    overlay_raw = _canonical(overlay)

    grant = _read_frozen_base(
        "native/containment/native-shadow-closed-local-replay-grant-v1.json"
    )
    grant["productionRegistry"]["sha256"] = _sha(arm_raw["registry"])
    grant["registry"]["sha256"] = _sha(overlay_raw)
    grant["checker"]["releaseManifestSha256"] = _sha(arm_raw["release"])
    grant["executionPolicy"]["sha256"] = _sha(arm_raw["policy"])
    grant["toolchainIdentity"]["sha256"] = _sha(arm_raw["toolchain"])
    grant_raw = _canonical(grant)

    local_execution = _read_frozen_base(
        "native/containment/native-shadow-local-execution-authority-v1.json"
    )
    local_execution["baseExecutionPolicySha256"] = _sha(arm_raw["policy"])
    local_execution["runtimeRootfsPortablePlanSha256"] = _sha(arm_raw["plan"])
    local_execution["runtimeRootfsSourceLockSha256"] = _sha(arm_raw["lock"])
    local_execution["runtimeRootfsResolutionSha256"] = _sha(
        arm_raw["resolution"]
    )
    local_execution["runtimeRootfsReplayExpectationSha256"] = _sha(
        arm_raw["expectation"]
    )
    local_execution_raw = _canonical(local_execution)

    closed_execution = _read_frozen_base(
        "native/containment/native-shadow-closed-local-replay-execution-authority-v1.json"
    )
    closed_execution["baseExecutionPolicySha256"] = _sha(arm_raw["policy"])
    closed_execution["closedLocalReplayGrantSha256"] = _sha(grant_raw)
    closed_execution["closedLocalReplayRegistryOverlaySha256"] = _sha(overlay_raw)
    closed_execution["checkerReleaseManifestSha256"] = _sha(arm_raw["release"])
    closed_execution["toolchainIdentitySha256"] = _sha(arm_raw["toolchain"])
    closed_execution["runtimeRootfsPortablePlanSha256"] = _sha(arm_raw["plan"])
    closed_execution["runtimeRootfsSourceLockSha256"] = _sha(arm_raw["lock"])
    closed_execution["runtimeRootfsResolutionSha256"] = _sha(
        arm_raw["resolution"]
    )
    closed_execution["runtimeRootfsReplayExpectationSha256"] = _sha(
        arm_raw["expectation"]
    )
    closed_execution_raw = _canonical(closed_execution)

    return {
        CONTAINMENT
        / "native-shadow-closed-local-replay-registry-overlay-arm64-v1.json": overlay_raw,
        CONTAINMENT
        / "native-shadow-closed-local-replay-grant-arm64-v1.json": grant_raw,
        CONTAINMENT
        / "native-shadow-local-execution-authority-arm64-v1.json": local_execution_raw,
        CONTAINMENT
        / "native-shadow-closed-local-replay-execution-authority-arm64-v1.json": closed_execution_raw,
    }


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
        outputs.update(dependent_authority(outputs))
    for path, raw in outputs.items():
        if args.write:
            path.write_bytes(raw)
        print(f"{_sha(raw)}  {path.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
