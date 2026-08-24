#!/usr/bin/env python3
"""MAC.2 contracts for the append-only Linux/arm64 checker authority."""

from __future__ import annotations

import hashlib
import json
import pathlib
import subprocess
import sys
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]

FROZEN_X86_SHA256 = {
    "scripts/native_shadow_rootfs_acquire.py": "31348981687939ff7cf63b5584947b3e09a92bb35f9f4e76f78a657ae139d49b",
    "scripts/native_shadow_rootfs_builder.py": "aa25701a8a29cfb0059c911a5df8dcc2f09c8b4c61b4ff46adfc0ef446cdf689",
    "scripts/native_shadow_rootfs_portable_v2.py": "11fe7f5672655cbfcf88e830d34ccc5b35274857df06cc123ed05e775bcd4fc3",
    "scripts/native_shadow_rootfs_oci_verify.py": "150b9cf5a37d2841c1c8132a08ac072256fbce39387a689f3601bbaacfd0ed93",
    "scripts/install-native-checker-toolchain.sh": "b56b1aaa05767da9c6bc3c26500236f0700a1c4088d1e7e7f01c9a9371a958ec",
    "native/checker/rust-tuple-struct-project-v1/checker.py": "d17dca244628bb55f6fbbf799c71adcae3d548169ef0655ca27c8eb1f7ba95d7",
    "native/checker/rust-tuple-struct-project-v1/RELEASE-MANIFEST.json": "9e3e6bd9d0ea716988829f0251cc9a5e9bc1b7c63b90c289f9dd4ae1f5345fd7",
    "native/containment/native-shadow-execution-policy-v1.json": "8806708be7c624b202e7ef8a88bfbd1d99fbb78064442d1c0f1baad6252e90c2",
    "native/containment/native-shadow-toolchain-identity-v1.json": "29799eff53df764024de8f1020eac177b2ee936243d2ac52d192ba2423f0fa84",
    "native/containment/native-shadow-runtime-rootfs-acquisition-plan-v1.json": "8d8ac1a4fd82370c1f0c12a270bd38b9b2b78f0c1a155432298b4d654a0fb06e",
    "native/containment/native-shadow-runtime-rootfs-portable-plan-v2.json": "fa4119964d87f30ad9fde496f509f0dbcc641f33ea52a345b19c1d2296cabb42",
    "native/containment/native-shadow-runtime-rootfs-source-lock-v2.json": "01b2180a5d9a2274076630775729904448a0894b05cfaaccec142d0d476e12e1",
    "native/containment/native-shadow-runtime-rootfs-resolution-v2.json": "5ff55eb8193ef8e5236b7401264bac08144b3431fd1cf0d378c8130d0d602af5",
    "native/containment/native-shadow-runtime-rootfs-replay-expectation-v2.json": "ce1597ce06ed7a89d3293e69997c3c129085e326ee90e8fb1d17cb6e92d2518b",
}

ARM64_PLAN = ROOT / "native/containment/native-shadow-runtime-rootfs-acquisition-plan-arm64-v1.json"
ARM64_LOCK = ROOT / "native/containment/native-shadow-runtime-rootfs-source-lock-arm64-v1.json"
ARM64_RESOLUTION = ROOT / "native/containment/native-shadow-runtime-rootfs-resolution-arm64-v1.json"
ARM64_EXPECTATION = ROOT / "native/containment/native-shadow-runtime-rootfs-replay-expectation-arm64-v1.json"
ARM64_TOOLCHAIN = ROOT / "native/containment/native-shadow-toolchain-identity-arm64-v1.json"
ARM64_POLICY = ROOT / "native/containment/native-shadow-execution-policy-arm64-v1.json"
ARM64_RELEASE = (
    ROOT
    / "native/checker/rust-tuple-struct-project-v1/RELEASE-MANIFEST-arm64-v1.json"
)
ARM64_REGISTRY = ROOT / "fixtures/native-shadow/registry-arm64-v1.json"
ARM64_OVERLAY = (
    ROOT
    / "native/containment/native-shadow-closed-local-replay-registry-overlay-arm64-v1.json"
)
ARM64_GRANT = (
    ROOT
    / "native/containment/native-shadow-closed-local-replay-grant-arm64-v1.json"
)
ARM64_LOCAL_EXECUTION = (
    ROOT
    / "native/containment/native-shadow-local-execution-authority-arm64-v1.json"
)
ARM64_CLOSED_EXECUTION = (
    ROOT
    / "native/containment/native-shadow-closed-local-replay-execution-authority-arm64-v1.json"
)
ARM64_SOURCE_RESULT = (
    ROOT
    / "native/containment/native-shadow-runtime-rootfs-source-closure-result-arm64-v1.json"
)
ARM64_PORTABLE_PLAN = (
    ROOT
    / "native/containment/native-shadow-runtime-rootfs-portable-plan-arm64-v1.json"
)
ARM64_BUILDER = ROOT / "scripts/native_shadow_rootfs_builder_arm64_v1.py"
ARM64_ACQUIRER = ROOT / "scripts/native_shadow_rootfs_acquire_arm64_v1.py"
ARM64_PORTABLE = ROOT / "scripts/native_shadow_rootfs_portable_arm64_v1.py"
ARM64_OCI_VERIFIER = (
    ROOT / "scripts/native_shadow_rootfs_oci_verify_arm64_v1.py"
)
ARM64_SCAFFOLD = (
    ROOT
    / "native/containment/native-shadow-runtime-rootfs-source-lock-arm64-v1-scaffold.json"
)


def _sha256(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _json(path: pathlib.Path) -> dict:
    raw = path.read_bytes()
    value = json.loads(raw.decode("utf-8"))
    canonical = (json.dumps(value, sort_keys=True, indent=2) + "\n").encode()
    if raw != canonical:
        raise AssertionError(f"authority JSON is not canonical: {path}")
    return value


def _source_json(path: pathlib.Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


class NativeShadowArm64AuthorityTest(unittest.TestCase):
    def test_x86_authority_bytes_remain_frozen(self) -> None:
        self.assertEqual(
            {relative: _sha256(ROOT / relative) for relative in FROZEN_X86_SHA256},
            FROZEN_X86_SHA256,
        )

    def test_arm64_authority_is_exactly_bound_to_linux_aarch64(self) -> None:
        plan = _json(ARM64_PLAN)
        lock = _json(ARM64_LOCK)
        resolution = _json(ARM64_RESOLUTION)
        expectation = _json(ARM64_EXPECTATION)
        toolchain = _json(ARM64_TOOLCHAIN)

        self.assertFalse(plan["activationAllowed"])
        self.assertEqual(plan["snapshotId"], "20240425T160000Z")
        self.assertEqual(plan["repository"]["architecture"], "arm64")
        self.assertEqual(
            plan["repository"]["packagesIndex"]["releasePath"],
            "main/binary-arm64/Packages.xz",
        )
        self.assertEqual(
            plan["seeds"],
            [
                "binutils-aarch64-linux-gnu",
                "gcc-13-aarch64-linux-gnu",
                "libc6-dev",
                "python3.12",
            ],
        )

        self.assertEqual(
            lock["platform"],
            {
                "debArchitecture": "arm64",
                "ociArchitecture": "arm64",
                "os": "linux",
                "rustTarget": "aarch64-unknown-linux-gnu",
            },
        )
        package_architectures = {
            row["architecture"] for row in resolution["packages"]
        }
        self.assertIn("arm64", package_architectures)
        self.assertLessEqual(package_architectures, {"arm64", "all"})
        self.assertEqual(expectation["platform"], {"architecture": "arm64", "os": "linux"})
        self.assertEqual(toolchain["platform"], {"architecture": "aarch64", "os": "linux"})
        self.assertEqual(
            toolchain["rust"]["rustcCommitHash"],
            "e7795af6d2449fb05a6393c3320ced873a999eb3",
        )

    def test_arm64_inputs_are_pinned_and_bounded(self) -> None:
        plan = _json(ARM64_PLAN)
        lock = _json(ARM64_LOCK)
        artifacts = lock["artifacts"]
        self.assertTrue(artifacts)
        self.assertLessEqual(sum(row["sizeBytes"] for row in artifacts), 2 * 1024**3)
        self.assertTrue(all(row["sizeBytes"] <= 512 * 1024**2 for row in artifacts))
        self.assertTrue(all(len(row["sha256"]) == 64 for row in artifacts))
        self.assertEqual(
            {row["url"].split("/", 3)[2] for row in plan["rustArtifacts"]},
            {"ci-artifacts.rust-lang.org"},
        )
        self.assertEqual(
            plan["repository"]["snapshotBase"],
            "https://snapshot.ubuntu.com/ubuntu/20240425T160000Z",
        )
        self.assertEqual(lock["buildRecipe"]["network"], "forbidden")
        self.assertLessEqual(lock["buildRecipe"]["maxTotalBytes"], 2 * 1024**3)

    def test_arm64_plan_validates_against_successor_code(self) -> None:
        from scripts import native_shadow_rootfs_acquire_arm64_v1 as acquire

        builder = ROOT / "scripts/native_shadow_rootfs_builder_arm64_v1.py"
        acquirer = ROOT / "scripts/native_shadow_rootfs_acquire_arm64_v1.py"
        value = acquire.load_plan(
            ARM64_PLAN.read_bytes(), builder, acquirer
        )
        self.assertEqual(value["repository"]["architecture"], "arm64")
        self.assertEqual(
            value["acquirerAuthoritySha256"],
            acquire.acquirer_authority_sha256(ARM64_ACQUIRER.read_bytes()),
        )

    def test_arm64_architecture_projection_reproduces_tracked_bytes(self) -> None:
        from scripts import native_shadow_arm64_authority_generate as generator

        generated = generator.generate()
        self.assertEqual(
            set(generated),
            {ARM64_POLICY, ARM64_TOOLCHAIN, ARM64_RELEASE, ARM64_REGISTRY},
        )
        self.assertEqual(
            {path: path.read_bytes() for path in generated},
            generated,
        )

    def test_arm64_portable_authority_is_fully_cross_bound(self) -> None:
        from scripts import native_shadow_rootfs_portable_arm64_v1 as portable

        authority = portable.load_authority_set(
            ARM64_PORTABLE_PLAN,
            ARM64_RESOLUTION,
            ARM64_LOCK,
            ARM64_EXPECTATION,
            ARM64_BUILDER,
        )
        self.assertEqual(
            authority["expectation"]["platform"],
            {"architecture": "arm64", "os": "linux"},
        )

    def test_arm64_source_closure_binds_every_successor_authority(self) -> None:
        source_result = _json(ARM64_SOURCE_RESULT)
        portable_plan = _json(ARM64_PORTABLE_PLAN)
        authority = source_result["authority"]
        self.assertEqual(
            authority,
            {
                "acquirerAuthoritySha256": _json(ARM64_PLAN)[
                    "acquirerAuthoritySha256"
                ],
                "acquisitionPlanSha256": _sha256(ARM64_PLAN),
                "builderSha256": _sha256(ARM64_BUILDER),
                "ociVerifierSha256": _sha256(ARM64_OCI_VERIFIER),
                "portableProjectionSha256": _sha256(ARM64_PORTABLE),
                "runtimeResolutionSha256": portable_plan[
                    "bootstrapAuthority"
                ]["signedResolutionV1Sha256"],
                "runtimeSourceLockSha256": portable_plan[
                    "bootstrapAuthority"
                ]["completeSourceLockV1Sha256"],
                "sourceLockScaffoldSha256": _sha256(ARM64_SCAFFOLD),
            },
        )
        self.assertEqual(
            portable_plan["bootstrapAuthority"]["sourceClosureResultV1Sha256"],
            _sha256(ARM64_SOURCE_RESULT),
        )

    def test_arm64_successor_clis_are_directly_invocable(self) -> None:
        for relative in (
            "scripts/native_shadow_rootfs_builder_arm64_v1.py",
            "scripts/native_shadow_rootfs_acquire_arm64_v1.py",
            "scripts/native_shadow_rootfs_portable_arm64_v1.py",
            "scripts/native_shadow_rootfs_oci_verify_arm64_v1.py",
        ):
            completed = subprocess.run(
                [sys.executable, str(ROOT / relative), "--help"],
                cwd=ROOT,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )
            self.assertEqual(
                completed.returncode,
                0,
                f"{relative}: {completed.stderr.decode(errors='replace')}",
            )

    def test_arm64_successor_does_not_relabel_x86_payloads(self) -> None:
        for path in (ARM64_PLAN, ARM64_LOCK, ARM64_RESOLUTION, ARM64_TOOLCHAIN):
            text = path.read_text(encoding="utf-8")
            self.assertNotIn("x86_64-unknown-linux-gnu", text)
            self.assertNotIn("binary-amd64", text)
            self.assertNotIn('"architecture": "amd64"', text)

    def test_arm64_portable_aliases_match_the_arm64_filesystem(self) -> None:
        lock = _json(ARM64_LOCK)
        derived = {row["logicalPath"]: row for row in lock["derivedEntries"]}
        self.assertNotIn("/lib64", derived)
        self.assertEqual(derived["/lib"]["target"], "usr/lib")
        self.assertEqual(derived["/usr/bin/as"]["target"], "aarch64-linux-gnu-as")
        self.assertEqual(derived["/usr/bin/ld"]["target"], "aarch64-linux-gnu-ld")

    def test_arm64_execution_policy_changes_only_architecture_identity(self) -> None:
        base = _source_json(
            ROOT / "native/containment/native-shadow-execution-policy-v1.json"
        )
        arm64 = _json(ARM64_POLICY)
        for document in (base, arm64):
            document.pop("schema")
            document.pop("release")
            document.pop("purpose")
            document["platform"].pop("architecture")
        arm64["landlock"]["executeAllow"].append("/lib64")
        arm64["landlock"]["executeAllow"].sort()
        base["landlock"]["executeAllow"].sort()
        self.assertEqual(arm64, base)

    def test_arm64_registry_binds_the_arm64_authority_chain(self) -> None:
        registry = _json(ARM64_REGISTRY)
        self.assertEqual(
            registry["executionPolicySha256"], _sha256(ARM64_POLICY)
        )
        self.assertEqual(
            registry["toolchainIdentitySha256"], _sha256(ARM64_TOOLCHAIN)
        )
        self.assertEqual(
            {row["checkerReleaseManifestSha256"] for row in registry["templates"]},
            {_sha256(ARM64_RELEASE)},
        )

    def test_arm64_dependent_authority_chain_is_exactly_cross_bound(self) -> None:
        overlay = _json(ARM64_OVERLAY)
        grant = _json(ARM64_GRANT)
        local_execution = _json(ARM64_LOCAL_EXECUTION)
        closed_execution = _json(ARM64_CLOSED_EXECUTION)

        self.assertEqual(overlay["baseRegistrySha256"], _sha256(ARM64_REGISTRY))
        self.assertEqual(
            overlay["executionPolicySha256"], _sha256(ARM64_POLICY)
        )
        self.assertEqual(
            overlay["toolchainIdentitySha256"], _sha256(ARM64_TOOLCHAIN)
        )
        self.assertEqual(
            {row["checkerReleaseManifestSha256"] for row in overlay["templates"]},
            {_sha256(ARM64_RELEASE)},
        )

        self.assertEqual(
            grant["productionRegistry"]["sha256"], _sha256(ARM64_REGISTRY)
        )
        self.assertEqual(grant["registry"]["sha256"], _sha256(ARM64_OVERLAY))
        self.assertEqual(
            grant["checker"]["releaseManifestSha256"], _sha256(ARM64_RELEASE)
        )
        self.assertEqual(
            grant["executionPolicy"]["sha256"], _sha256(ARM64_POLICY)
        )
        self.assertEqual(
            grant["toolchainIdentity"]["sha256"], _sha256(ARM64_TOOLCHAIN)
        )

        rootfs_digests = {
            "runtimeRootfsPortablePlanSha256": _sha256(ARM64_PORTABLE_PLAN),
            "runtimeRootfsSourceLockSha256": _sha256(ARM64_LOCK),
            "runtimeRootfsResolutionSha256": _sha256(ARM64_RESOLUTION),
            "runtimeRootfsReplayExpectationSha256": _sha256(ARM64_EXPECTATION),
        }
        self.assertEqual(local_execution["baseExecutionPolicySha256"], _sha256(ARM64_POLICY))
        self.assertEqual(
            {key: local_execution[key] for key in rootfs_digests}, rootfs_digests
        )
        self.assertEqual(
            closed_execution["baseExecutionPolicySha256"], _sha256(ARM64_POLICY)
        )
        self.assertEqual(
            closed_execution["closedLocalReplayGrantSha256"], _sha256(ARM64_GRANT)
        )
        self.assertEqual(
            closed_execution["closedLocalReplayRegistryOverlaySha256"],
            _sha256(ARM64_OVERLAY),
        )
        self.assertEqual(
            closed_execution["checkerReleaseManifestSha256"],
            _sha256(ARM64_RELEASE),
        )
        self.assertEqual(
            closed_execution["toolchainIdentitySha256"],
            _sha256(ARM64_TOOLCHAIN),
        )
        self.assertEqual(
            {key: closed_execution[key] for key in rootfs_digests},
            rootfs_digests,
        )

    def test_arm64_dependent_projection_reproduces_tracked_bytes(self) -> None:
        from scripts import native_shadow_arm64_portable_authority_generate as generator

        inputs = {
            ARM64_PORTABLE_PLAN: ARM64_PORTABLE_PLAN.read_bytes(),
            ARM64_RESOLUTION: ARM64_RESOLUTION.read_bytes(),
            ARM64_LOCK: ARM64_LOCK.read_bytes(),
            ARM64_EXPECTATION: ARM64_EXPECTATION.read_bytes(),
        }
        generated = generator.dependent_authority(inputs)
        self.assertEqual(
            set(generated),
            {ARM64_OVERLAY, ARM64_GRANT, ARM64_LOCAL_EXECUTION, ARM64_CLOSED_EXECUTION},
        )
        self.assertEqual({path: path.read_bytes() for path in generated}, generated)

    def test_arm64_checker_release_changes_only_toolchain_artifacts(self) -> None:
        base = _source_json(
            ROOT / "native/checker/rust-tuple-struct-project-v1/RELEASE-MANIFEST.json"
        )
        arm64 = _json(ARM64_RELEASE)
        for document in (base, arm64):
            document["toolchain"].pop(
                "linuxX8664ArtifactSha256", None
            )
            document["toolchain"].pop(
                "linuxArm64ArtifactSha256", None
            )
        self.assertEqual(arm64, base)

    def test_arm64_toolchain_identity_changes_only_platform_artifacts(self) -> None:
        base = _source_json(
            ROOT / "native/containment/native-shadow-toolchain-identity-v1.json"
        )
        arm64 = _json(ARM64_TOOLCHAIN)
        arm64.pop("platform")
        for document in (base, arm64):
            document.pop("schema")
            document.pop("release")
            document.pop("purpose")
            document["rust"].pop("linuxX8664ArtifactSha256", None)
            document["rust"].pop("linuxArm64ArtifactSha256", None)
        self.assertEqual(arm64, base)


if __name__ == "__main__":
    unittest.main()
