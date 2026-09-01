#!/usr/bin/env python3
"""Pins the single failed-closed MAC.4 authenticated-channel observation."""

from __future__ import annotations

import hashlib
import json
import pathlib
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
RESULT_PATH = (
    ROOT
    / "native/containment/native-shadow-mac4-authenticated-channel-result-arm64-v1.json"
)
CONTRACT_PATH = (
    ROOT
    / "native/containment/native-shadow-mac4-authenticated-channel-contract-v1.json"
)
EXPECTED_HEAD = "957319e0a2aa780febd25e97ea27ad8243e287d0"
EXPECTED_RUN = 33510635018
MODULE_PREFLIGHT_PATH = (
    ROOT
    / "native/containment/native-shadow-mac4-vsock-module-preflight-result-arm64-v1.json"
)
MODULE_PREFLIGHT_RUN = 33519178333
MODULE_PREFLIGHT_HEAD = "b08828200cb7848959baad9fabf9f05f4c5e6db7"


def sha256(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


class Mac4AuthenticatedChannelResultTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.result = json.loads(RESULT_PATH.read_text(encoding="utf-8"))

    def test_schema_and_status_preserve_the_failed_closed_observation(self):
        self.assertEqual(
            self.result["schema"],
            "boole.native-shadow.mac4-authenticated-channel-observation.arm64.v1",
        )
        self.assertEqual(
            self.result["status"],
            "MAC4-AUTHENTICATED-CHANNEL-OBSERVED-FAIL-CLOSED",
        )
        self.assertTrue(self.result["appendOnly"])
        self.assertEqual(self.result["artifactClass"], "DISPOSABLE-DEVELOPMENT")

    def test_build_is_the_single_authorized_dispatch_at_the_exact_head(self):
        build = self.result["imageBuild"]
        self.assertEqual(build["runId"], EXPECTED_RUN)
        self.assertEqual(build["headSha"], EXPECTED_HEAD)
        self.assertEqual(build["runAttempt"], 1)
        self.assertEqual(build["workflowDispatches"], 1)
        self.assertEqual(build["workflowReruns"], 0)
        self.assertEqual(build["manualReplicas"], 0)
        self.assertEqual(
            [job["jobId"] for job in build["jobs"]],
            [99865163228, 99865163758, 99868389658],
        )
        self.assertTrue(all(job["conclusion"] == "success" for job in build["jobs"]))

    def test_two_replicas_and_all_three_outputs_are_byte_identical(self):
        build = self.result["imageBuild"]
        self.assertEqual(build["replicas"], 2)
        self.assertEqual(build["comparisonStatus"], "TWO-REPLICAS-BYTE-IDENTICAL")
        outputs = build["outputs"]
        self.assertEqual(
            [row["name"] for row in outputs],
            ["guest-kernel", "guest-initrd", "guest-root-disk"],
        )
        self.assertEqual(
            [(row["sizeBytes"], row["sha256"]) for row in outputs],
            [
                (
                    57_860_488,
                    "d29e317d66517190f6437b9b9bd2cedd26a424fe6da7b1a28451247a13fe1336",
                ),
                (
                    1_776_942_700,
                    "104e8e170abaaaf03f0e9a0f9c9eecc39a5fa0b96658bb0a5128f7d198ed784b",
                ),
                (
                    2_036_236_288,
                    "f1a18c2c1c77ee8e27657c3d0f5a2d2fb82061468a6e82ffa1583d5c0b0b088a",
                ),
            ],
        )
        self.assertTrue(all(row["replicasByteIdentical"] for row in outputs))

    def test_artifact_provenance_is_complete_and_ephemeral(self):
        build = self.result["imageBuild"]
        self.assertEqual(len(build["artifacts"]), 3)
        self.assertTrue(all(row["expiresAt"] == "2026-09-08" for row in build["artifacts"]))
        self.assertEqual(build["rawOutputBytesAcrossTwoReplicas"], 7_742_078_952)
        self.assertEqual(
            {row["artifactId"] for row in build["artifacts"]},
            {9801647057, 9801843388, 9801971881},
        )

    def test_observation_used_exactly_one_vm_and_did_not_authenticate(self):
        observation = self.result["macObservation"]
        self.assertEqual(observation["bootAttempts"], 1)
        self.assertEqual(observation["machinesStarted"], 1)
        self.assertTrue(observation["machineStopped"])
        self.assertFalse(observation["channelAuthenticated"])
        self.assertTrue(observation["imagesUnchanged"])
        self.assertEqual(observation["vsock"], {"port": 4050, "roundTrips": 0})
        self.assertEqual(observation["host"]["chip"], "Apple M4 Max")
        self.assertEqual(observation["host"]["macOS"], "26.5.2")

    def test_vm_remained_closed_except_for_one_vsock_device(self):
        shape = self.result["macObservation"]["vmShape"]
        self.assertEqual(shape["networkDevices"], 0)
        self.assertEqual(shape["sharedDirectories"], 0)
        self.assertEqual(shape["socketDevices"], 1)
        self.assertEqual(shape["storageDevices"], 1)
        self.assertTrue(shape["rootDiskReadOnly"])

    def test_fresh_nonce_boot_tuple_and_contract_were_bound_before_failure(self):
        observation = self.result["macObservation"]
        self.assertRegex(observation["nonceHex"], r"^[0-9a-f]{64}$")
        self.assertNotEqual(observation["nonceHex"], "0" * 64)
        self.assertRegex(observation["bootTupleBindingHex"], r"^[0-9a-f]{64}$")
        contract = self.result["contract"]
        self.assertEqual(contract["sha256"], sha256(CONTRACT_PATH))
        self.assertEqual(observation["contractSha256"], contract["sha256"])

    def test_console_and_receipt_evidence_are_hash_bound(self):
        evidence = self.result["macObservation"]["evidence"]
        self.assertEqual(
            evidence["console"],
            {
                "lines": 226,
                "sha256": "7ad66ad5f7fceae52efb35de9eb8f8816ad0a81b0a51e71703eb1b59ede1d739",
                "sizeBytes": 21182,
            },
        )
        self.assertEqual(
            evidence["receipt"]["sha256"],
            "f704201fba1faa148861fe83654ca7152133b46ffdf6f5d67389bc81f41cde0d",
        )
        self.assertTrue(evidence["systemdReachedMultiUser"])
        self.assertTrue(evidence["launcherReadinessWasTrue"])
        self.assertTrue(evidence["relayServiceFailed"])
        self.assertFalse(evidence["relayReadyMarkerObserved"])

    def test_sufficient_root_cause_is_exact_and_does_not_claim_an_errno(self):
        cause = self.result["rootCause"]
        self.assertEqual(cause["status"], "SUFFICIENT-ROOT-CAUSE-IDENTIFIED")
        self.assertEqual(
            cause["kernelConfig"],
            {
                "CONFIG_VIRTIO_VSOCKETS": "m",
                "CONFIG_VIRTIO_VSOCKETS_COMMON": "m",
                "CONFIG_VSOCKETS": "m",
            },
        )
        self.assertTrue(cause["moduleObjectsPresent"])
        self.assertEqual(
            cause["presentModuleIndexes"],
            ["modules.builtin", "modules.builtin.modinfo", "modules.order"],
        )
        self.assertEqual(
            cause["missingModuleIndexes"],
            ["modules.alias", "modules.alias.bin", "modules.dep", "modules.dep.bin"],
        )
        self.assertEqual(cause["modulesLoadConfiguration"]["kind"], "dangling-symlink")
        self.assertEqual(cause["modulesLoadConfiguration"]["target"], "../modules")
        self.assertFalse(cause["modulesLoadConfiguration"]["targetPresent"])
        self.assertFalse(cause["exactRelayErrnoPreserved"])
        self.assertEqual(cause["fixStatus"], "NOT-STARTED")

    def test_new_image_and_boot_authority_are_required_before_retry(self):
        next_step = self.result["nextStep"]
        self.assertEqual(next_step["recommendation"], "DETERMINISTIC-DEPMOD-INDEXES")
        self.assertTrue(next_step["newImageAuthorityRequired"])
        self.assertTrue(next_step["newBootAuthorityRequired"])
        self.assertFalse(next_step["retryAuthorizedByThisRecord"])

    def test_host_remains_the_authority_and_no_product_boundary_opened(self):
        self.assertEqual(self.result["authorityOwner"], "MAC-HOST-NODE")
        self.assertEqual(self.result["guestRole"], "BOUNDED-RELAY")
        for key in (
            "activationAllowed",
            "consensusTouched",
            "mac4Complete",
            "miningActivated",
            "nodeExecutionConnected",
            "p2pTouched",
            "productionRelease",
            "publicMining",
            "rewardReady",
            "testnetClaim",
        ):
            self.assertFalse(self.result["boundaries"][key], key)


class Mac4VsockModulePreflightResultTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.result = json.loads(MODULE_PREFLIGHT_PATH.read_text(encoding="utf-8"))

    def test_green_preflight_is_bound_to_the_exact_feature_head_and_run(self):
        self.assertEqual(
            self.result["schema"],
            "boole.native-shadow.mac4-vsock-module-preflight-result.arm64.v1",
        )
        self.assertEqual(self.result["status"], "GREEN-NO-IMAGE-NO-VM")
        self.assertEqual(self.result["headSha"], MODULE_PREFLIGHT_HEAD)
        self.assertEqual(self.result["runId"], MODULE_PREFLIGHT_RUN)
        self.assertEqual(self.result["jobId"], 99893784061)

    def test_preflight_created_no_image_vm_or_activation_authority(self):
        self.assertEqual(
            self.result["effects"], {"imagesCreated": 0, "machinesStarted": 0}
        )
        self.assertEqual(self.result["authorisations"]["imageProductionRunsAllowed"], 0)
        self.assertFalse(self.result["authorisations"]["bootAuthorised"])
        self.assertFalse(self.result["authorisations"]["activationAllowed"])

    def test_all_required_depmod_indexes_are_present_and_hash_bound(self):
        rows = self.result["moduleMetadata"]
        self.assertEqual(
            [row["name"] for row in rows],
            [
                "modules.alias",
                "modules.alias.bin",
                "modules.builtin.alias.bin",
                "modules.builtin.bin",
                "modules.dep",
                "modules.dep.bin",
                "modules.devname",
                "modules.softdep",
                "modules.symbols",
                "modules.symbols.bin",
            ],
        )
        self.assertTrue(all(row["sizeBytes"] > 0 for row in rows))
        self.assertTrue(all(len(row["sha256"]) == 64 for row in rows))

    def test_load_contract_and_measurement_are_exact(self):
        self.assertEqual(
            self.result["moduleLoadContract"],
            {
                "path": "etc/modules",
                "sha256": "3c02b7df355c02e3c1bd223628af759447f969d78a73bec3db42fe62ef67ed87",
                "modules": [
                    "vsock",
                    "vmw_vsock_virtio_transport_common",
                    "vmw_vsock_virtio_transport",
                ],
            },
        )
        measurement = self.result["measurement"]
        self.assertEqual(measurement["entries"], 17_702)
        self.assertEqual(measurement["payloadBytes"], 1_775_284_622)
        self.assertEqual(measurement["pathCollisions"], 0)
        self.assertEqual(measurement["duplicatePaths"], 0)
        self.assertEqual(measurement["symlinkEscapes"], 0)

    def test_failed_free_probe_is_retained_without_consuming_an_execution_budget(self):
        first = self.result["predecessorProbe"]
        self.assertEqual(first["runId"], 33518658937)
        self.assertEqual(first["headSha"], "c866ed0de411f8f2c625d336447c1437f603db70")
        self.assertEqual(first["failure"], "DEPMOD-MULTICALL-NAME-LOST")
        self.assertEqual(first["imagesCreated"], 0)
        self.assertEqual(first["machinesStarted"], 0)

    def test_implementation_bindings_match_the_live_repository_bytes(self):
        for row in self.result["implementation"].values():
            path = ROOT / row["path"]
            self.assertEqual(path.stat().st_size, row["sizeBytes"])
            self.assertEqual(sha256(path), row["sha256"])


if __name__ == "__main__":
    unittest.main()
