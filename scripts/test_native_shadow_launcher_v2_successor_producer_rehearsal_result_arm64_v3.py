#!/usr/bin/env python3
"""Seal the raw authority-zero v5 R3 payload without importing future authority."""

from __future__ import annotations

import hashlib
import json
import pathlib
import stat
import unittest


REPO = pathlib.Path(__file__).resolve().parents[1]
RESULT_PATH = REPO / (
    "native/containment/native-shadow-mac3-launcher-v2-successor-producer-"
    "rehearsal-result-arm64-v3.json"
)
P1_PATH = REPO / (
    "native/containment/native-shadow-mac3-launcher-v2-successor-producer-"
    "preregistration-arm64-v1.json"
)
P4_PATH = REPO / (
    "native/containment/native-shadow-mac3-launcher-v2-successor-main-branch-"
    "dispatch-fence-correction-arm64-v1.json"
)

RESULT_SHA256 = "44cd7d6feea2efc62d9ab6cb809e5d66c1452c9e4d2f034fd800e6573938fe87"
RESULT_SIZE_BYTES = 6_012
P1_SHA256 = "576bafd10600a05e9ab326e1e507c1a0351381d068f393ce402e295bf93afbec"
P1_SIZE_BYTES = 20_145
P4_SHA256 = "63f5bdf0ffaac00ac1af3972ed69051da9fcbe8a06b90ae3c9f70756bbfe144b"
P4_SIZE_BYTES = 13_335

EXPECTED_KEYS = {
    "activationAllowed",
    "authorisations",
    "bootableClaim",
    "boundInputs",
    "effects",
    "executionEnvelope",
    "generationFiles",
    "mainBranchDispatchFenceCorrection",
    "measurement",
    "predecessors",
    "repeatable",
    "reusedPinnedUpstream",
    "schema",
    "status",
}
ZERO_AUTHORISATIONS = {
    "bootAuthorised": False,
    "consensusActivated": False,
    "imageProductionAuthorised": False,
    "imageProductionRunsAllowed": 0,
    "mac4Started": False,
    "miningActivated": False,
    "p2pActivated": False,
    "rewardActivated": False,
    "testnetStarted": False,
}
ZERO_EFFECTS = {
    "attemptMarkersCreated": 0,
    "bootAttempts": 0,
    "imageOutputsCreated": 0,
    "productionOutputsCreated": 0,
}
EXPECTED_EXECUTION_ENVELOPE = {
    "cgroupV2": {
        "equalAtBeforeAndAfterObservations": True,
        "leafControlsKernelObserved": True,
        "limitEventsKernelObserved": True,
        "memoryHighEvents": 0,
        "memoryMaxBytes": 8_589_934_592,
        "memoryMaxEvents": 0,
        "memoryOomEvents": 0,
        "memoryOomKillEvents": 0,
        "memorySwapMaxBytes": 0,
        "pidsMax": 128,
        "pidsMaxEvents": 0,
        "requestedUnitMembershipMatched": True,
    },
    "systemdRuntimeMaxSec": {
        "evidence": "source-pinned-request-and-exact-unit-membership-at-exec",
        "execReachedRequestedUnit": True,
        "kernelObserved": False,
        "managerValueQueried": False,
        "requestedSeconds": 1_200,
        "sourcePinnedRequestPresent": True,
    },
}
GENERATION_PATHS = (
    "scripts/native_shadow_successor_produce_phase_arm64_v5.py",
    "scripts/test_native_shadow_successor_produce_phase_arm64_v5.py",
    "scripts/native-shadow-successor-produce-arm64-v5.sh",
    ".github/workflows/native-shadow-successor-produce-arm64-v5.yml",
    "scripts/test_native_shadow_successor_produce_workflow_arm64_v5.py",
)
REUSED_PATHS = (
    "scripts/native_shadow_successor_produce_phase_arm64_v3.py",
    "scripts/native_shadow_successor_root_disk_readback_arm64_v3.py",
    "scripts/test_native_shadow_successor_root_disk_readback_arm64_v3.py",
)


def canonical_json(value: object) -> bytes:
    return (json.dumps(value, sort_keys=True, indent=2) + "\n").encode("utf-8")


def sha256_bytes(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def read_regular(path: pathlib.Path) -> bytes:
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or path.is_symlink():
        raise AssertionError(f"{path.relative_to(REPO)} is not regular")
    return path.read_bytes()


def live_identity(relative: str) -> dict[str, object]:
    raw = read_regular(REPO / relative)
    return {"path": relative, "sha256": sha256_bytes(raw), "sizeBytes": len(raw)}


def assert_strict_equal(actual: object, expected: object, path: str = "$") -> None:
    if type(actual) is not type(expected):
        raise AssertionError(f"{path} type differs")
    if isinstance(expected, dict):
        if set(actual) != set(expected):
            raise AssertionError(f"{path} keys differ")
        for key in expected:
            assert_strict_equal(actual[key], expected[key], f"{path}.{key}")
        return
    if isinstance(expected, list):
        if len(actual) != len(expected):
            raise AssertionError(f"{path} length differs")
        for index, (actual_item, expected_item) in enumerate(zip(actual, expected)):
            assert_strict_equal(actual_item, expected_item, f"{path}[{index}]")
        return
    if actual != expected:
        raise AssertionError(f"{path} value differs: {actual!r} != {expected!r}")


class LauncherV2SuccessorProducerRehearsalResultV3Tests(unittest.TestCase):
    def setUp(self) -> None:
        self.raw = read_regular(RESULT_PATH)
        self.result = json.loads(self.raw.decode("utf-8"))

    def test_result_is_the_exact_canonical_arm64_artifact_payload(self) -> None:
        self.assertEqual(len(self.raw), RESULT_SIZE_BYTES)
        self.assertEqual(sha256_bytes(self.raw), RESULT_SHA256)
        self.assertEqual(self.raw, canonical_json(self.result))
        self.assertEqual(set(self.result), EXPECTED_KEYS)
        self.assertEqual(
            self.result["schema"],
            "boole.native-shadow.mac3.launcher-v2-successor-producer-"
            "rehearsal.arm64.v3",
        )
        self.assertEqual(self.result["status"], "PASS-NO-IMAGE-PRODUCED")

    def test_result_preserves_strict_zero_authority_and_effects(self) -> None:
        assert_strict_equal(self.result["authorisations"], ZERO_AUTHORISATIONS)
        assert_strict_equal(self.result["effects"], ZERO_EFFECTS)
        self.assertIs(self.result["activationAllowed"], False)
        self.assertIs(self.result["bootableClaim"], False)
        self.assertIs(self.result["repeatable"], True)

    def test_lineage_directly_binds_p4_and_exact_live_v5_bytes(self) -> None:
        p4 = live_identity(P4_PATH.relative_to(REPO).as_posix())
        self.assertEqual(p4["sha256"], P4_SHA256)
        self.assertEqual(p4["sizeBytes"], P4_SIZE_BYTES)
        generation = [live_identity(path) for path in GENERATION_PATHS]
        reused = [live_identity(path) for path in REUSED_PATHS]
        self.assertEqual(self.result["predecessors"], [p4])
        self.assertEqual(self.result["generationFiles"], generation)
        self.assertEqual(self.result["reusedPinnedUpstream"], reused)
        self.assertEqual(self.result["boundInputs"], [p4, *generation, *reused])
        paths = [row["path"] for row in self.result["boundInputs"]]
        self.assertEqual(len(paths), 9)
        self.assertEqual(len(set(paths)), 9)
        self.assertEqual(self.result["mainBranchDispatchFenceCorrection"], p4)

    def test_measurement_matches_the_preregistered_staging_contract(self) -> None:
        p1_raw = read_regular(P1_PATH)
        self.assertEqual(len(p1_raw), P1_SIZE_BYTES)
        self.assertEqual(sha256_bytes(p1_raw), P1_SHA256)
        p1 = json.loads(p1_raw.decode("utf-8"))
        assert_strict_equal(
            self.result["measurement"], p1["expectedPreflight"]["measurement"]
        )

    def test_execution_envelope_states_only_observed_kernel_facts(self) -> None:
        assert_strict_equal(
            self.result["executionEnvelope"], EXPECTED_EXECUTION_ENVELOPE
        )
        runtime = self.result["executionEnvelope"]["systemdRuntimeMaxSec"]
        self.assertIs(runtime["kernelObserved"], False)
        self.assertIs(runtime["managerValueQueried"], False)

    def test_future_authority_transport_and_production_do_not_flow_into_r3(self) -> None:
        encoded = self.raw.decode("utf-8")
        for forbidden in (
            "artifactId",
            "archiveDigest",
            "headSha",
            "jobId",
            "producer-fingerprint-arm64-v7",
            "production-authority-arm64-v7",
            "image-production-result-arm64-v7",
            "boole-native-shadow-mac3-successor-production-a7-",
        ):
            self.assertNotIn(forbidden, encoded)


if __name__ == "__main__":
    unittest.main()
