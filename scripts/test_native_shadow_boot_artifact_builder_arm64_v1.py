#!/usr/bin/env python3
"""Contract tests for the ARM64 boot-artifact builder preflight."""

from __future__ import annotations

import hashlib
import json
import os
import pathlib
import socket
import stat
import tempfile
import unittest
from unittest import mock

from scripts import native_shadow_boot_artifact_builder_arm64_v1 as boot


ROOT = pathlib.Path(__file__).resolve().parents[1]
TRACKED_PLAN = (
    ROOT
    / "native/containment/native-shadow-boot-artifact-build-plan-arm64-v1-scaffold.json"
)
TRACKED_LOCK = (
    ROOT / "native/containment/native-shadow-runtime-rootfs-source-lock-arm64-v1.json"
)
TRACKED_POLICY = (
    ROOT / "native/containment/native-shadow-execution-policy-arm64-v1.json"
)


def _canonical(value: object) -> bytes:
    return json.dumps(
        value,
        ensure_ascii=False,
        sort_keys=True,
        indent=2,
        allow_nan=False,
    ).encode("utf-8") + b"\n"


def _sha(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


class NativeShadowBootArtifactBuilderPreflightTests(unittest.TestCase):
    def test_tracked_scaffold_binds_systemd_policy_and_never_static_pid1(self) -> None:
        plan = json.loads(TRACKED_PLAN.read_bytes())
        policy_raw = TRACKED_POLICY.read_bytes()
        self.assertNotIn("pid1", plan["inputs"])
        self.assertEqual(
            set(plan["inputs"]),
            {"imageBuilderToolchain", "kernel", "systemdGuestClosure"},
        )
        self.assertEqual(
            plan["guestExecutionPolicy"],
            {
                "cgroupParent": "/sys/fs/cgroup/system.slice/boole-native-shadow-launcher.service",
                "sha256": _sha(policy_raw),
                "systemdRequired": True,
                "unitName": "boole-native-shadow-launcher.service",
            },
        )

    def _write_regular(
        self, path: pathlib.Path, raw: bytes, *, executable: bool = False
    ) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(raw)
        path.chmod(0o755 if executable else 0o644)

    def _fixture(self, root: pathlib.Path) -> dict[str, pathlib.Path | bytes | dict[str, object]]:
        artifact_a = b"rootfs-a"
        artifact_b = b"rootfs-b-longer"
        artifacts = [
            {
                "id": "artifact-a",
                "kind": "deb",
                "sha256": _sha(artifact_a),
                "sizeBytes": len(artifact_a),
            },
            {
                "id": "artifact-b",
                "kind": "rust-dist",
                "sha256": _sha(artifact_b),
                "sizeBytes": len(artifact_b),
            },
        ]
        lock = {
            "activationAllowed": False,
            "artifacts": artifacts,
            "platform": {
                "debArchitecture": "arm64",
                "ociArchitecture": "arm64",
                "os": "linux",
                "rustTarget": "aarch64-unknown-linux-gnu",
            },
            "schema": "boole.native-shadow.runtime-rootfs-source-lock.arm64.v1",
        }
        lock_raw = _canonical(lock)
        lock_path = root / "source-lock.json"
        lock_path.write_bytes(lock_raw)

        cgroup_parent = (
            "/sys/fs/cgroup/system.slice/boole-native-shadow-launcher.service"
        )
        policy = {
            "activationAllowed": False,
            "crashRecovery": {"cgroupParent": cgroup_parent},
            "platform": {
                "architecture": "aarch64",
                "operatingSystem": "linux",
                "systemdRequired": True,
            },
            "privilege": {
                "systemdUnit": {"UnitName": "boole-native-shadow-launcher.service"}
            },
            "schema": "boole.native-shadow.execution-policy.arm64.v1",
        }
        policy_raw = _canonical(policy)
        policy_path = root / "execution-policy.json"
        policy_path.write_bytes(policy_raw)

        plan = {
            "activationAllowed": False,
            "bootFormatVersion": 1,
            "guestExecutionPolicy": {
                "cgroupParent": cgroup_parent,
                "sha256": _sha(policy_raw),
                "systemdRequired": True,
                "unitName": "boole-native-shadow-launcher.service",
            },
            "guestDownloadMaxBytes": 2_147_483_648,
            "inputs": {
                "imageBuilderToolchain": {
                    "format": "initrd-ext4-builder-authority-v1",
                    "sha256": None,
                    "sizeBytes": None,
                },
                "kernel": {
                    "format": "linux-arm64-image",
                    "sha256": None,
                    "sizeBytes": None,
                },
                "systemdGuestClosure": {
                    "format": "systemd-rootfs-closure-authority-v1",
                    "sha256": None,
                    "sizeBytes": None,
                },
            },
            "release": "SYNTHETIC-BOOT-ARTIFACT-PREFLIGHT-NOT-ACTIVATABLE",
            "rootfsSourceLock": {
                "artifactCount": len(artifacts),
                "sha256": _sha(lock_raw),
                "totalBytes": sum(int(row["sizeBytes"]) for row in artifacts),
            },
            "schema": "boole.native-shadow.boot-artifact-build-plan.arm64.v1",
        }
        plan_path = root / "plan.json"
        plan_path.write_bytes(_canonical(plan))
        cas = root / "cas"
        (cas / "sha256").mkdir(parents=True)
        return {
            "artifact_a": artifact_a,
            "artifact_b": artifact_b,
            "artifacts": artifacts,
            "cas": cas,
            "lock": lock_path,
            "plan": plan_path,
            "policy": policy_path,
        }

    def _audit(self, fixture: dict[str, object]) -> dict[str, object]:
        return boot.audit_inputs(
            pathlib.Path(fixture["plan"]),
            pathlib.Path(fixture["lock"]),
            pathlib.Path(fixture["policy"]),
            [pathlib.Path(fixture["cas"])],
        )

    def test_tracked_scaffold_pins_the_exact_incomplete_arm64_closure(self) -> None:
        plan = boot.load_plan(TRACKED_PLAN)
        lock_raw = TRACKED_LOCK.read_bytes()
        lock = json.loads(lock_raw)
        self.assertEqual(plan["rootfsSourceLock"]["sha256"], _sha(lock_raw))
        self.assertEqual(plan["rootfsSourceLock"]["artifactCount"], 62)
        self.assertEqual(plan["rootfsSourceLock"]["totalBytes"], 181_623_999)
        self.assertEqual(len(lock["artifacts"]), 62)
        self.assertEqual(sum(row["sizeBytes"] for row in lock["artifacts"]), 181_623_999)
        self.assertEqual(
            [
                plan["inputs"][name]["sha256"]
                for name in ("imageBuilderToolchain", "kernel", "systemdGuestClosure")
            ],
            [None, None, None],
        )

    def test_missing_rootfs_inputs_are_sorted_and_never_claim_bootability(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            fixture = self._fixture(pathlib.Path(raw_root))
            artifact = fixture["artifacts"][1]
            self._write_regular(
                pathlib.Path(fixture["cas"]) / "sha256" / artifact["sha256"],
                fixture["artifact_b"],
            )
            result = self._audit(fixture)
        self.assertEqual(result["status"], "BLOCKED_MISSING_INPUTS")
        self.assertEqual(result["missingArtifactIds"], ["artifact-a"])
        self.assertEqual(
            result["inputSummary"],
            {
                "expectedArtifacts": 2,
                "expectedBytes": len(fixture["artifact_a"]) + len(fixture["artifact_b"]),
                "invalidArtifacts": 0,
                "missingArtifacts": 1,
                "missingBytes": len(fixture["artifact_a"]),
                "presentArtifacts": 1,
                "presentBytes": len(fixture["artifact_b"]),
            },
        )
        self.assertEqual(result["artifactsWritten"], 0)
        self.assertIs(result["bootableClaim"], False)

    def test_complete_source_closure_stays_blocked_until_authorities_exist(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            fixture = self._fixture(pathlib.Path(raw_root))
            for artifact, key in zip(
                fixture["artifacts"], ("artifact_a", "artifact_b")
            ):
                self._write_regular(
                    pathlib.Path(fixture["cas"]) / "sha256" / artifact["sha256"],
                    fixture[key],
                )
            result = self._audit(fixture)
        self.assertEqual(result["status"], "BLOCKED_MISSING_INPUTS")
        self.assertEqual(result["missingArtifactIds"], [])
        self.assertEqual(
            result["missingInputAuthorities"],
            ["imageBuilderToolchain", "kernel", "systemdGuestClosure"],
        )
        self.assertEqual(result["artifactsWritten"], 0)
        self.assertIs(result["bootableClaim"], False)

    def test_digest_size_and_symlink_artifacts_are_hard_errors(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            fixture = self._fixture(pathlib.Path(raw_root))
            artifact = fixture["artifacts"][0]
            destination = pathlib.Path(fixture["cas"]) / "sha256" / artifact["sha256"]
            self._write_regular(destination, b"wrong")
            with self.assertRaisesRegex(boot.BootArtifactPreflightError, "digest/size"):
                self._audit(fixture)
            destination.unlink()
            target = pathlib.Path(raw_root) / "target"
            self._write_regular(target, fixture["artifact_a"])
            destination.symlink_to(target)
            with self.assertRaisesRegex(boot.BootArtifactPreflightError, "symlink|unsafe"):
                self._audit(fixture)

    def test_audit_has_no_network_path(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            fixture = self._fixture(pathlib.Path(raw_root))
            with mock.patch.object(
                socket,
                "socket",
                side_effect=AssertionError("network access attempted"),
            ):
                result = self._audit(fixture)
        self.assertEqual(result["status"], "BLOCKED_MISSING_INPUTS")

    def test_cas_parent_symlink_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            root = pathlib.Path(raw_root)
            fixture = self._fixture(root)
            real_parent = root / "real-parent"
            real_parent.mkdir()
            pathlib.Path(fixture["cas"]).rename(real_parent / "cas")
            alias = root / "alias-parent"
            alias.symlink_to(real_parent, target_is_directory=True)
            fixture["cas"] = alias / "cas"
            with self.assertRaisesRegex(
                boot.BootArtifactPreflightError, "symlink|unsafe"
            ):
                self._audit(fixture)

    def test_opened_cas_descriptors_close_if_a_later_root_is_invalid(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            fixture = self._fixture(pathlib.Path(raw_root))
            with mock.patch.object(
                boot,
                "_cas_sha_directory",
                side_effect=[101, boot.BootArtifactPreflightError("later root invalid")],
            ), mock.patch.object(boot.os, "close") as close:
                with self.assertRaisesRegex(
                    boot.BootArtifactPreflightError, "later root invalid"
                ):
                    boot.audit_inputs(
                        pathlib.Path(fixture["plan"]),
                        pathlib.Path(fixture["lock"]),
                        pathlib.Path(fixture["policy"]),
                        [pathlib.Path("first"), pathlib.Path("second")],
                    )
        close.assert_any_call(101)

    def test_systemd_policy_mismatch_is_a_hard_error(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            fixture = self._fixture(pathlib.Path(raw_root))
            policy_path = pathlib.Path(fixture["policy"])
            policy = json.loads(policy_path.read_bytes())
            policy["platform"]["systemdRequired"] = False
            policy_raw = _canonical(policy)
            policy_path.write_bytes(policy_raw)
            plan_path = pathlib.Path(fixture["plan"])
            plan = json.loads(plan_path.read_bytes())
            plan["guestExecutionPolicy"]["sha256"] = _sha(policy_raw)
            plan_path.write_bytes(_canonical(plan))
            with self.assertRaisesRegex(
                boot.BootArtifactPreflightError, "require systemd"
            ):
                self._audit(fixture)

    def test_undefined_authority_cannot_be_populated_in_the_scaffold(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            fixture = self._fixture(pathlib.Path(raw_root))
            plan_path = pathlib.Path(fixture["plan"])
            plan = json.loads(plan_path.read_bytes())
            plan["inputs"]["kernel"].update(sha256="0" * 64, sizeBytes=1)
            plan_path.write_bytes(_canonical(plan))
            with self.assertRaisesRegex(
                boot.BootArtifactPreflightError, "authority contract exists"
            ):
                self._audit(fixture)


if __name__ == "__main__":
    unittest.main()
