#!/usr/bin/env python3
"""Generate architecture-only MAC.2 authority projections.

The Linux/x86_64 authority remains frozen.  This generator accepts only those
exact bytes and changes the three fields that are necessarily architecture
specific: platform identity, Rust distribution artifact names, and artifact
digests.  Everything else stays semantically identical.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
from typing import Any


ROOT = pathlib.Path(__file__).resolve().parents[1]
BASE_SHA256 = {
    "fixtures/native-shadow/registry-v1.json": (
        "53a8271fdf92ae5a4204a9779c0badfec86c7fd3399b8b12700199c6de9dc61a"
    ),
    "native/containment/native-shadow-execution-policy-v1.json": (
        "8806708be7c624b202e7ef8a88bfbd1d99fbb78064442d1c0f1baad6252e90c2"
    ),
    "native/containment/native-shadow-toolchain-identity-v1.json": (
        "29799eff53df764024de8f1020eac177b2ee936243d2ac52d192ba2423f0fa84"
    ),
    "native/checker/rust-tuple-struct-project-v1/RELEASE-MANIFEST.json": (
        "9e3e6bd9d0ea716988829f0251cc9a5e9bc1b7c63b90c289f9dd4ae1f5345fd7"
    ),
}
ARM64_RUST_ARTIFACTS = {
    "cargo-nightly-aarch64-unknown-linux-gnu.tar.xz": (
        "dd30cce8fc7c5029287dc75b24bb549e6ed957f6ddeac205fd1e6360993d4903"
    ),
    "rust-std-nightly-aarch64-unknown-linux-gnu.tar.xz": (
        "2c8a2d0f1f2c51caf5805f4a0ab510298dfebe4db2551a22b35ccc10d9bc3edc"
    ),
    "rustc-nightly-aarch64-unknown-linux-gnu.tar.xz": (
        "055bf89237ff4e3dfa4259ae5e63a8cc78fb2f1919421ac9e01edccd20ee6a5f"
    ),
}


def _canonical(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, indent=2) + "\n").encode()


def _load(relative: str) -> dict[str, Any]:
    raw = (ROOT / relative).read_bytes()
    if hashlib.sha256(raw).hexdigest() != BASE_SHA256[relative]:
        raise ValueError(f"frozen x86 authority differs: {relative}")
    value = json.loads(raw.decode("utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"authority is not an object: {relative}")
    return value


def generate() -> dict[pathlib.Path, bytes]:
    policy = _load("native/containment/native-shadow-execution-policy-v1.json")
    policy["schema"] = "boole.native-shadow.execution-policy.arm64.v1"
    policy["release"] = "NATIVE-SHADOW-EXECUTION-POLICY-ARM64-V1-QUALIFICATION"
    policy["purpose"] = (
        "non-activatable Linux/arm64 projection of the frozen native-shadow "
        "node/launcher containment policy"
    )
    policy["platform"]["architecture"] = "aarch64"
    # The arm64 rootfs has no /lib64 compatibility tree.  Keeping that path in
    # the Landlock authority would make the launcher require a path that the
    # frozen arm64 filesystem deliberately does not contain.
    policy["landlock"]["executeAllow"].remove("/lib64")

    toolchain = _load(
        "native/containment/native-shadow-toolchain-identity-v1.json"
    )
    toolchain["schema"] = "boole.native-shadow.toolchain-identity.arm64.v1"
    toolchain["release"] = (
        "NATIVE-SHADOW-TOOLCHAIN-IDENTITY-ARM64-V1-QUALIFICATION"
    )
    toolchain["purpose"] = (
        "root-owned Linux/arm64 runtime identity authority for native checker "
        "qualification"
    )
    toolchain["platform"] = {"architecture": "aarch64", "os": "linux"}
    toolchain["rust"].pop("linuxX8664ArtifactSha256")
    toolchain["rust"]["linuxArm64ArtifactSha256"] = ARM64_RUST_ARTIFACTS

    release = _load(
        "native/checker/rust-tuple-struct-project-v1/RELEASE-MANIFEST.json"
    )
    release["toolchain"].pop("linuxX8664ArtifactSha256")
    release["toolchain"]["linuxArm64ArtifactSha256"] = ARM64_RUST_ARTIFACTS

    registry = _load("fixtures/native-shadow/registry-v1.json")
    registry["executionPolicySha256"] = hashlib.sha256(
        _canonical(policy)
    ).hexdigest()
    registry["toolchainIdentitySha256"] = hashlib.sha256(
        _canonical(toolchain)
    ).hexdigest()
    for template in registry["templates"]:
        template["checkerReleaseManifestSha256"] = hashlib.sha256(
            _canonical(release)
        ).hexdigest()

    return {
        ROOT / "native/containment/native-shadow-execution-policy-arm64-v1.json": _canonical(policy),
        ROOT / "native/containment/native-shadow-toolchain-identity-arm64-v1.json": _canonical(toolchain),
        ROOT
        / "native/checker/rust-tuple-struct-project-v1/RELEASE-MANIFEST-arm64-v1.json": _canonical(release),
        ROOT / "fixtures/native-shadow/registry-arm64-v1.json": _canonical(registry),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write", action="store_true")
    args = parser.parse_args()
    outputs = generate()
    for path, raw in outputs.items():
        if args.write:
            path.write_bytes(raw)
        print(f"{hashlib.sha256(raw).hexdigest()}  {path.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
