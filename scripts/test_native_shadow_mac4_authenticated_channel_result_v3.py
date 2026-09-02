#!/usr/bin/env python3
"""Pins the first successful closed-local MAC.4 transport observation."""

from __future__ import annotations

import hashlib
import json
import pathlib
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
RESULT = ROOT / "native/containment/native-shadow-mac4-authenticated-channel-result-arm64-v3.json"
PREDECESSOR = ROOT / "native/containment/native-shadow-mac4-authenticated-channel-result-arm64-v2.json"


def sha256(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


class Mac4AuthenticatedChannelResultV3Tests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.result = json.loads(RESULT.read_text(encoding="utf-8"))

    def test_status_records_transport_and_readiness_without_claiming_mac4_complete(self):
        self.assertEqual(
            self.result["schema"],
            "boole.native-shadow.mac4-authenticated-channel-observation.arm64.v3",
        )
        self.assertEqual(
            self.result["status"],
            "MAC4-AUTHENTICATED-TRANSPORT-AND-READINESS-PASS",
        )
        self.assertTrue(self.result["appendOnly"])
        boundaries = self.result["boundaries"]
        self.assertTrue(boundaries["authenticatedTransportObserved"])
        self.assertTrue(boundaries["guestReadinessObserved"])
        self.assertFalse(boundaries["mac4Complete"])
        self.assertFalse(boundaries["nodeExecutionConnected"])

    def test_predecessor_and_contract_are_exactly_bound(self):
        predecessor = self.result["predecessor"]
        self.assertEqual(predecessor["path"], str(PREDECESSOR.relative_to(ROOT)))
        self.assertEqual(predecessor["sha256"], sha256(PREDECESSOR))
        self.assertEqual(
            self.result["contract"]["sha256"],
            "4f2ec110d72f628207ac383668daff7bda6b568449fd315d8376aeb20ae08bbd",
        )

    def test_free_preflight_created_no_image_or_machine(self):
        preflight = self.result["preflight"]
        self.assertEqual(preflight["runId"], 33_583_707_702)
        self.assertEqual(preflight["status"], "READY-NO-IMAGE-CREATED")
        self.assertEqual(preflight["effects"], {"imagesCreated": 0, "machinesStarted": 0})
        self.assertEqual(preflight["privateTmp"], {"path": "/var/tmp", "mode": "1777"})

    def test_build_was_one_dispatch_with_two_replicas_and_one_comparison(self):
        build = self.result["imageBuild"]
        self.assertEqual(build["runId"], 33_584_005_767)
        self.assertEqual(build["headSha"], "9c42ac4a7ebf5b0bc29ba715bdc16d106b8084d0")
        self.assertEqual(build["workflowDispatches"], 1)
        self.assertEqual(build["workflowReruns"], 0)
        self.assertEqual(build["replicas"], 2)
        self.assertEqual(
            [(row["jobId"], row["conclusion"]) for row in build["jobs"]],
            [
                (100_104_193_663, "success"),
                (100_104_193_891, "success"),
                (100_106_205_617, "success"),
            ],
        )

    def test_three_outputs_are_byte_identical_with_exact_identities(self):
        outputs = self.result["imageBuild"]["outputs"]
        self.assertEqual(
            [(row["name"], row["sizeBytes"], row["sha256"]) for row in outputs],
            [
                ("guest-kernel", 57_860_488, "d29e317d66517190f6437b9b9bd2cedd26a424fe6da7b1a28451247a13fe1336"),
                ("guest-initrd", 1_778_266_572, "e9e6f7a1ac668ab9272ab955efee777ce1ab56431b3e58bf1f69b460cca28a71"),
                ("guest-root-disk", 2_037_743_616, "bc63b07a1a2dd17c49a6be0befa54b6aa48e8a2a635bffa434cb38c8f189d4f6"),
            ],
        )
        self.assertTrue(all(row["replicasByteIdentical"] for row in outputs))

    def test_artifacts_and_downloaded_receipts_are_hash_bound(self):
        build = self.result["imageBuild"]
        self.assertEqual(
            {row["artifactId"] for row in build["artifacts"]},
            {9_829_667_894, 9_829_630_356, 9_829_516_074},
        )
        self.assertEqual(build["comparisonStatus"], "TWO-REPLICAS-BYTE-IDENTICAL")
        self.assertEqual(
            build["comparisonReceipt"],
            {
                "sha256": "57d1df41ec94baf92ebfef860bdc5c693dbc35fbf3963c36c9291d96b828886a",
                "sizeBytes": 749,
            },
        )

    def test_exactly_one_closed_vm_completed_authenticated_round_trip(self):
        observation = self.result["macObservation"]
        self.assertEqual(observation["bootAttempts"], 1)
        self.assertEqual(observation["machinesStarted"], 1)
        self.assertTrue(observation["machineStopped"])
        self.assertTrue(observation["imagesUnchanged"])
        self.assertTrue(observation["channelAuthenticated"])
        self.assertEqual(observation["vsock"], {"port": 4050, "roundTrips": 1})
        self.assertEqual(observation["vmShape"]["networkDevices"], 0)
        self.assertEqual(observation["vmShape"]["sharedDirectories"], 0)
        self.assertTrue(observation["vmShape"]["rootDiskReadOnly"])

    def test_same_boot_observed_all_readiness_records(self):
        evidence = self.result["macObservation"]["readinessEvidence"]
        self.assertEqual(
            set(evidence["checks"]),
            {"launcher-executable", "launcher-prerequisites", "supervisor-privilege", "readiness"},
        )
        self.assertTrue(all(row["met"] for row in evidence["checks"].values()))
        self.assertEqual(evidence["malformed"], [])
        self.assertEqual(evidence["conflicting"], [])
        self.assertEqual(evidence["missing"], [])
        self.assertEqual(evidence["unknownRecordIds"], [])

    def test_boot_evidence_and_post_boot_cleanup_are_exact(self):
        observation = self.result["macObservation"]
        self.assertEqual(observation["ranForSeconds"], 11.123239994049072)
        self.assertEqual(
            observation["console"],
            {
                "lines": 233,
                "sha256": "d71f48105dab5c9a06a5f5505cd18c8d62eb81d4d7fb370e222e7d696c500b02",
                "sizeBytes": 21_654,
            },
        )
        self.assertEqual(observation["remainingHostProcesses"], 0)
        self.assertEqual(
            observation["aggregateResult"],
            {
                "sha256": "762a65aabdd797bc860060b4927780ac5a8a442e9cbcc5f32dfc49b9beda069c",
                "sizeBytes": 3_607,
            },
        )

    def test_next_step_is_node_route_binding_not_activation(self):
        self.assertEqual(self.result["nextStep"]["recommendation"], "MAC4-NODE-ROUTE-BINDING")
        self.assertFalse(self.result["nextStep"]["testnetAuthorized"])
        self.assertFalse(self.result["nextStep"]["activationAuthorized"])
        for key in (
            "consensusTouched",
            "miningActivated",
            "p2pTouched",
            "productionRelease",
            "publicMining",
            "rewardReady",
            "testnetClaim",
            "activationAllowed",
        ):
            self.assertFalse(self.result["boundaries"][key], key)


if __name__ == "__main__":
    unittest.main()
