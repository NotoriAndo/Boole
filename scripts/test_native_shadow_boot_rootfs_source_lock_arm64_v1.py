#!/usr/bin/env python3
"""Acceptance tests for the ARM64 boot rootfs source lock successor.

The frozen plan states the grounds on which a successor source lock may be
accepted. These tests take the sealed lock, mutate exactly one ground at a
time, and require the verifier to refuse. A source lock that survives every
mutation still proves nothing about runtime behaviour: the ceiling verdict
stays source-shape only.
"""
from __future__ import annotations

import copy
import json
import pathlib
import sys
import unittest

REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from scripts import native_shadow_boot_rootfs_source_lock_arm64_v1 as lock_tool


def _load(path: pathlib.Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


class SourceLockAcceptanceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        # This is a historical plan.  Its schema and sealed source-lock
        # relationship remain testable, but TP7/TP8 no longer require today's
        # Cargo.lock to equal the old generation's workspace lockfile.
        cls.plan = _load(lock_tool.PLAN_PATH)
        cls.lock = _load(lock_tool.LOCK_PATH)

    def mutate(self) -> dict:
        return copy.deepcopy(self.lock)

    def assertRefused(self, lock: dict, needle: str) -> None:
        with self.assertRaises(lock_tool.SourceLockError) as caught:
            lock_tool.verify_source_lock(
                self.plan,
                lock,
                verify_current_source_bytes=False,
            )
        self.assertIn(needle, str(caught.exception))

    # -- the sealed lock itself -------------------------------------------
    def test_sealed_lock_verifies(self) -> None:
        audit = lock_tool.verify_source_lock(
            self.plan,
            self.lock,
            verify_current_source_bytes=False,
        )
        self.assertEqual(audit["status"], self.plan["expected"]["auditStatus"])
        self.assertEqual(audit["missingRoles"], self.plan["expected"]["auditMissingRoles"])
        self.assertFalse(audit["runtimeCompatibilityVerified"])
        self.assertFalse(audit["bootableClaim"])
        self.assertFalse(audit["activationAllowed"])

    def test_ceiling_verdict_is_not_inflated(self) -> None:
        audit = lock_tool.verify_source_lock(
            self.plan,
            self.lock,
            verify_current_source_bytes=False,
        )
        self.assertNotIn("BOOT", audit["status"])
        self.assertNotIn("COMPATIBLE", audit["status"])
        self.assertEqual(audit["artifactsWritten"], 0)

    # -- 1. a payload row goes missing ------------------------------------
    def test_missing_package_is_refused(self) -> None:
        lock = self.mutate()
        dropped = lock["ubuntu"]["packages"].pop()
        lock["artifacts"] = [a for a in lock["artifacts"] if a["id"] != dropped["artifactId"]]
        self.assertRefused(lock, "package closure")

    # -- 2. an extra payload row appears ----------------------------------
    def test_extra_package_is_refused(self) -> None:
        lock = self.mutate()
        extra = copy.deepcopy(lock["ubuntu"]["packages"][0])
        extra["packageId"] = "deb-" + "1" * 64
        extra["artifactId"] = extra["packageId"]
        extra["name"] = "zzz-not-in-closure"
        lock["ubuntu"]["packages"].append(extra)
        lock["ubuntu"]["packages"].sort(key=lambda row: row["packageId"])
        lock["artifacts"].append(
            {"id": extra["artifactId"], "kind": "deb", "sha256": "1" * 64, "sizeBytes": 1}
        )
        lock["artifacts"].sort(key=lambda row: row["id"])
        self.assertRefused(lock, "package closure")

    # -- 3. canonical ordering is broken ----------------------------------
    def test_reordered_packages_are_refused(self) -> None:
        lock = self.mutate()
        lock["ubuntu"]["packages"][0], lock["ubuntu"]["packages"][1] = (
            lock["ubuntu"]["packages"][1],
            lock["ubuntu"]["packages"][0],
        )
        self.assertRefused(lock, "sorted")

    def test_reordered_artifacts_are_refused(self) -> None:
        lock = self.mutate()
        lock["artifacts"][0], lock["artifacts"][1] = lock["artifacts"][1], lock["artifacts"][0]
        self.assertRefused(lock, "sorted")

    # -- 4. a size or digest is tampered with -----------------------------
    def test_tampered_artifact_size_is_refused(self) -> None:
        lock = self.mutate()
        for row in lock["artifacts"]:
            if row["kind"] == "deb":
                row["sizeBytes"] += 1
                break
        self.assertRefused(lock, "sizeBytes")

    def test_tampered_artifact_digest_is_refused(self) -> None:
        lock = self.mutate()
        for row in lock["artifacts"]:
            if row["kind"] == "deb":
                row["sha256"] = "0" * 63 + "1"
                break
        self.assertRefused(lock, "package closure")

    # -- 5. a foreign snapshot is mixed in --------------------------------
    def test_mixed_snapshot_is_refused(self) -> None:
        lock = self.mutate()
        lock["ubuntu"]["snapshot"] = "2024-05-01T00:00:00Z"
        self.assertRefused(lock, "snapshot")

    def test_mixed_snapshot_base_is_refused(self) -> None:
        lock = self.mutate()
        lock["ubuntu"]["repositories"][0]["snapshotBase"] = (
            "https://snapshot.ubuntu.com/ubuntu/20240501T000000Z"
        )
        self.assertRefused(lock, "repository")

    # -- 6. the systemd package seed disappears ---------------------------
    def test_missing_systemd_seed_is_refused(self) -> None:
        lock = self.mutate()
        lock["ubuntu"]["seeds"] = [s for s in lock["ubuntu"]["seeds"] if s != "systemd"]
        self.assertRefused(lock, "systemd")

    def test_missing_systemd_package_is_refused(self) -> None:
        lock = self.mutate()
        for row in lock["ubuntu"]["packages"]:
            if row["name"] == "systemd":
                row["name"] = "systemd-renamed"
                break
        self.assertRefused(lock, "package closure")

    # -- 7. the launcher unit disappears ----------------------------------
    def test_missing_launcher_unit_is_refused(self) -> None:
        self.assertRefused(
            self._without_tracked("/usr/lib/systemd/system/boole-native-shadow-launcher.service"),
            "launcher-unit",
        )

    # -- 8. sysusers / tmpfiles disappear ---------------------------------
    def test_missing_sysusers_config_is_refused(self) -> None:
        self.assertRefused(
            self._without_tracked("/usr/lib/sysusers.d/boole-native-shadow.conf"),
            "sysusers-config",
        )

    def test_missing_tmpfiles_config_is_refused(self) -> None:
        self.assertRefused(
            self._without_tracked("/usr/lib/tmpfiles.d/boole-native-shadow.conf"),
            "tmpfiles-config",
        )

    # -- 9. the empty machine-id rule disappears --------------------------
    def test_missing_machine_id_is_refused(self) -> None:
        self.assertRefused(self._without_tracked("/etc/machine-id"), "empty-machine-id")

    def test_non_empty_machine_id_is_refused(self) -> None:
        lock = self.mutate()
        for row in lock["trackedFiles"]:
            if row["logicalPath"] == "/etc/machine-id":
                row["sha256"] = "0" * 63 + "2"
                break
        self.assertRefused(lock, "empty-machine-id")

    # -- 10. unit enablement disappears -----------------------------------
    def test_missing_unit_enablement_is_refused(self) -> None:
        lock = self.mutate()
        wants = "/etc/systemd/system/multi-user.target.wants/boole-native-shadow-launcher.service"
        lock["derivedEntries"] = [r for r in lock["derivedEntries"] if r["logicalPath"] != wants]
        self.assertRefused(lock, "launcher-enablement")

    def test_enablement_pointing_elsewhere_is_refused(self) -> None:
        lock = self.mutate()
        wants = "/etc/systemd/system/multi-user.target.wants/boole-native-shadow-launcher.service"
        for row in lock["derivedEntries"]:
            if row["logicalPath"] == wants:
                row["target"] = "/usr/lib/systemd/system/multi-user.target"
                break
        self.assertRefused(lock, "launcher-enablement")

    # -- 11. the replay-node service is mixed in --------------------------
    def test_replay_node_tracked_file_is_refused(self) -> None:
        lock = self.mutate()
        lock["trackedFiles"].append(
            {
                "gid": 0,
                "logicalPath": (
                    "/usr/lib/systemd/system/boole-native-shadow-replay-node.service"
                ),
                "mode": "0444",
                "sha256": "0ae883d2" + "0" * 56,
                "sourcePath": "native/systemd/boole-native-shadow-replay-node.service",
                "uid": 0,
            }
        )
        lock["trackedFiles"].sort(key=lambda row: row["logicalPath"])
        self.assertRefused(lock, "replay-node")

    def test_replay_node_enablement_is_refused(self) -> None:
        lock = self.mutate()
        lock["derivedEntries"].append(
            {
                "gid": 0,
                "kind": "symlink",
                "logicalPath": (
                    "/etc/systemd/system/multi-user.target.wants/"
                    "boole-native-shadow-replay-node.service"
                ),
                "mode": "0777",
                "target": (
                    "/usr/lib/systemd/system/boole-native-shadow-replay-node.service"
                ),
                "uid": 0,
            }
        )
        lock["derivedEntries"].sort(key=lambda row: row["logicalPath"])
        self.assertRefused(lock, "replay-node")

    # -- 12. maintainer script execution is permitted ---------------------
    def test_maintainer_script_execution_is_refused(self) -> None:
        lock = self.mutate()
        lock["buildRecipe"]["maintainerScripts"] = "execute-on-install"
        self.assertRefused(lock, "maintainer script")

    def test_network_during_build_is_refused(self) -> None:
        lock = self.mutate()
        lock["buildRecipe"]["network"] = "allowed"
        self.assertRefused(lock, "network")

    # -- 13. activation is switched on ------------------------------------
    def test_activation_allowed_is_refused(self) -> None:
        lock = self.mutate()
        lock["activationAllowed"] = True
        self.assertRefused(lock, "activationAllowed")

    # -- 14. packages are reused from a different source lock -------------
    def test_foreign_source_lock_package_is_refused(self) -> None:
        # The predecessor lock's Rust distribution never went through the verified
        # deb closure, so borrowing its payload authority must not be accepted.
        baseline = _load(lock_tool.BASELINE_LOCK_PATH)
        borrowed = next(row for row in baseline["artifacts"] if row["kind"] == "rust-dist")
        lock = self.mutate()
        row = copy.deepcopy(lock["ubuntu"]["packages"][0])
        row["packageId"] = row["artifactId"] = "deb-" + borrowed["sha256"]
        row["name"] = "reused-from-the-predecessor-source-lock"
        lock["ubuntu"]["packages"][0] = row
        lock["ubuntu"]["packages"].sort(key=lambda item: item["packageId"])
        self.assertRefused(lock, "package closure")

    def test_baseline_lock_is_not_accepted_as_successor(self) -> None:
        self.assertRefused(_load(lock_tool.BASELINE_LOCK_PATH), "package closure")

    # -- 15. the deferred launcher binary digest is not invented ----------
    def test_invented_launcher_binary_digest_is_refused(self) -> None:
        lock = self.mutate()
        lock["trackedFiles"].append(
            {
                "gid": 0,
                "logicalPath": "/usr/libexec/boole/boole-native-shadow-launcher",
                "mode": "0755",
                "sha256": "3" * 64,
                "sourcePath": "crates/boole-native-shadow-launcher/Cargo.toml",
                "uid": 0,
            }
        )
        lock["trackedFiles"].sort(key=lambda row: row["logicalPath"])
        self.assertRefused(lock, "launcher binary")

    # -- bound source bytes must really exist -----------------------------
    def test_tracked_source_bytes_must_match_on_disk(self) -> None:
        lock = self.mutate()
        for row in lock["trackedFiles"]:
            if row["logicalPath"] == "/usr/lib/tmpfiles.d/boole-native-shadow.conf":
                row["sourcePath"] = "native/sysusers.d/boole-native-shadow.conf"
                break
        self.assertRefused(lock, "tmpfiles-config")

    def test_authority_binding_digest_must_match_on_disk(self) -> None:
        lock = self.mutate()
        lock["authorityBindings"][0]["sha256"] = "4" * 64
        self.assertRefused(lock, "authority binding")

    # -- helpers ----------------------------------------------------------
    def _without_tracked(self, logical_path: str) -> dict:
        lock = self.mutate()
        lock["trackedFiles"] = [
            row for row in lock["trackedFiles"] if row["logicalPath"] != logical_path
        ]
        return lock


class SealedDocumentTests(unittest.TestCase):
    def test_plan_pins_the_generator_normalized_digest(self) -> None:
        plan = _load(lock_tool.PLAN_PATH)
        generator = plan["authorityInputs"]["sourceLockGenerator"]
        self.assertEqual(
            generator["sourcePath"],
            "scripts/native_shadow_boot_rootfs_source_lock_arm64_v1.py",
        )
        self.assertEqual(len(generator["sha256"]), 64)
        self.assertGreater(generator["sizeBytes"], 0)

    def test_tool_pins_the_plan_digest(self) -> None:
        self.assertEqual(
            lock_tool.PLAN_SHA256,
            lock_tool.sha256_file(lock_tool.PLAN_PATH),
        )

    def test_documents_are_canonical_json(self) -> None:
        for path in (lock_tool.PLAN_PATH, lock_tool.LOCK_PATH, lock_tool.RESULT_PATH):
            with self.subTest(path=path.name):
                raw = path.read_bytes()
                self.assertEqual(raw, lock_tool.canonical_json(json.loads(raw)))

    def test_result_keeps_every_boundary_closed(self) -> None:
        result = _load(lock_tool.RESULT_PATH)
        self.assertFalse(result["activationAllowed"])
        self.assertFalse(result["bootableClaim"])
        self.assertEqual(result["bootArtifactsWritten"], 0)
        for name, value in result["boundaries"].items():
            with self.subTest(boundary=name):
                self.assertFalse(value)

    def test_sealed_plan_and_lock_agree_without_freezing_the_current_workspace(self) -> None:
        plan = _load(lock_tool.PLAN_PATH)
        lock = _load(lock_tool.LOCK_PATH)
        audit = lock_tool.verify_source_lock(
            plan,
            lock,
            verify_current_source_bytes=False,
        )
        self.assertEqual(audit["status"], plan["expected"]["auditStatus"])


if __name__ == "__main__":
    unittest.main()
