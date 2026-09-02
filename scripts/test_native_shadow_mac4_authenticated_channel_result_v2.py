#!/usr/bin/env python3
"""Pins the second failed-closed MAC.4 observation and exact root cause."""

from __future__ import annotations

import hashlib
import json
import pathlib
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
RESULT = ROOT / "native/containment/native-shadow-mac4-authenticated-channel-result-arm64-v2.json"
PREDECESSOR = ROOT / "native/containment/native-shadow-mac4-authenticated-channel-result-arm64-v1.json"


def sha256(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


class Mac4AuthenticatedChannelResultV2Tests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.result = json.loads(RESULT.read_text(encoding="utf-8"))

    def test_append_only_status_is_failed_closed_not_mac4_complete(self):
        self.assertEqual(
            self.result["schema"],
            "boole.native-shadow.mac4-authenticated-channel-observation.arm64.v2",
        )
        self.assertEqual(
            self.result["status"],
            "MAC4-AUTHENTICATED-CHANNEL-OBSERVED-FAIL-CLOSED",
        )
        self.assertTrue(self.result["appendOnly"])
        self.assertFalse(self.result["boundaries"]["mac4Complete"])

    def test_predecessor_record_is_hash_bound_and_unchanged(self):
        predecessor = self.result["predecessor"]
        self.assertEqual(predecessor["path"], str(PREDECESSOR.relative_to(ROOT)))
        self.assertEqual(predecessor["sha256"], sha256(PREDECESSOR))

    def test_build_was_one_dispatch_with_two_successful_replicas(self):
        build = self.result["imageBuild"]
        self.assertEqual(build["runId"], 33_569_233_592)
        self.assertEqual(build["headSha"], "c5074b0935e84e466debe416adf37e06ee4081cb")
        self.assertEqual(build["runAttempt"], 1)
        self.assertEqual(build["workflowDispatches"], 1)
        self.assertEqual(build["workflowReruns"], 0)
        self.assertEqual(build["replicas"], 2)
        self.assertTrue(all(row["conclusion"] == "success" for row in build["jobs"]))

    def test_three_outputs_are_byte_identical_with_exact_identities(self):
        outputs = self.result["imageBuild"]["outputs"]
        self.assertEqual(
            [(row["name"], row["sizeBytes"], row["sha256"]) for row in outputs],
            [
                ("guest-kernel", 57_860_488, "d29e317d66517190f6437b9b9bd2cedd26a424fe6da7b1a28451247a13fe1336"),
                ("guest-initrd", 1_778_266_452, "cd4b70916f6bebc31dafa109cbc5ea1920b92116b81c77c54e55b6500c0172c3"),
                ("guest-root-disk", 2_037_739_520, "a4d7693f789e35df9e48d59f3c25f5675038da76ed490dcfb447ee523eaecd07"),
            ],
        )
        self.assertTrue(all(row["replicasByteIdentical"] for row in outputs))

    def test_ephemeral_artifacts_and_comparison_receipt_are_exact(self):
        build = self.result["imageBuild"]
        self.assertEqual(
            {row["artifactId"] for row in build["artifacts"]},
            {9_824_445_284, 9_824_579_098, 9_824_610_948},
        )
        self.assertEqual(build["comparisonStatus"], "TWO-REPLICAS-BYTE-IDENTICAL")
        self.assertEqual(
            build["comparisonReceipt"]["sha256"],
            "c0d2a49d0f9cddd1eda67cdaeee41fc505becf87124c6afb1cd61f21059ebc01",
        )

    def test_one_closed_mac_vm_started_and_authenticated_zero_round_trips(self):
        observation = self.result["macObservation"]
        self.assertEqual(observation["bootAttempts"], 1)
        self.assertEqual(observation["machinesStarted"], 1)
        self.assertTrue(observation["machineStopped"])
        self.assertTrue(observation["imagesUnchanged"])
        self.assertFalse(observation["channelAuthenticated"])
        self.assertEqual(observation["vsock"], {"port": 4050, "roundTrips": 0})
        self.assertEqual(observation["vmShape"]["networkDevices"], 0)
        self.assertEqual(observation["vmShape"]["sharedDirectories"], 0)
        self.assertTrue(observation["vmShape"]["rootDiskReadOnly"])

    def test_boot_evidence_is_hash_bound_and_launcher_readiness_survived(self):
        evidence = self.result["macObservation"]["evidence"]
        self.assertEqual(
            evidence["console"],
            {
                "lines": 230,
                "sha256": "fe2f05f330a447ec6348373862ce149efbef625a344ba91bd9d74cda8bc05f67",
                "sizeBytes": 21_355,
            },
        )
        self.assertEqual(
            evidence["receipt"]["sha256"],
            "3cb78810e1fc658ae1ccb3c6a6d3140a6f0c89248376fe8b009d6d600668f4c5",
        )
        self.assertTrue(evidence["launcherReadinessWasTrue"])
        self.assertTrue(evidence["vsockProtocolRegistered"])
        self.assertFalse(evidence["relayReadyMarkerObserved"])

    def test_read_only_differential_proves_var_tmp_is_the_exact_cause(self):
        cause = self.result["rootCause"]
        self.assertEqual(cause["status"], "EXACT-ROOT-CAUSE-REPRODUCED")
        self.assertEqual(cause["missingPath"], "/var/tmp")
        differential = cause["readOnlyRootDifferential"]
        self.assertEqual(differential["runId"], 33_572_058_564)
        self.assertEqual(differential["missingVarTmp"]["systemdStatus"], "226/NAMESPACE")
        self.assertFalse(differential["missingVarTmp"]["relayReady"])
        self.assertEqual(differential["presentVarTmp"]["systemdResult"], "success")
        self.assertTrue(differential["presentVarTmp"]["relayReady"])
        self.assertEqual(differential["effects"], {"imagesCreated": 0, "machinesStarted": 0})

    def test_successor_is_additive_readback_checked_and_not_booted(self):
        correction = self.result["correction"]
        self.assertEqual(correction["path"], "/var/tmp")
        self.assertEqual(correction["mode"], "1777")
        self.assertTrue(correction["mountedReadbackRequired"])
        self.assertTrue(correction["predecessorBytePreserved"])
        self.assertFalse(correction["imageBuilt"])
        self.assertFalse(correction["vmBooted"])
        for row in correction["implementation"]:
            path = ROOT / row["path"]
            self.assertEqual(path.stat().st_size, row["sizeBytes"])
            self.assertEqual(sha256(path), row["sha256"])

    def test_no_product_network_or_activation_boundary_opened(self):
        for key, value in self.result["boundaries"].items():
            self.assertFalse(value, key)


if __name__ == "__main__":
    unittest.main()
