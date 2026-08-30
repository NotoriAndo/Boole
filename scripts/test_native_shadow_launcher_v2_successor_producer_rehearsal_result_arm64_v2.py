#!/usr/bin/env python3
"""Seal the raw authority-zero v4 R2 payload without binding future F6/A6 state."""

from __future__ import annotations

import hashlib
import json
import pathlib
import stat
import unittest


REPO = pathlib.Path(__file__).resolve().parents[1]
RESULT_PATH = REPO / (
    "native/containment/native-shadow-mac3-launcher-v2-successor-producer-"
    "rehearsal-result-arm64-v2.json"
)
HARD_STOP_PATH = REPO / (
    "native/containment/native-shadow-mac3-launcher-v2-successor-producer-"
    "rehearsal-hard-stop-arm64-v2.json"
)
P1_PATH = REPO / (
    "native/containment/native-shadow-mac3-launcher-v2-successor-producer-"
    "preregistration-arm64-v1.json"
)
P3_PATH = REPO / (
    "native/containment/native-shadow-mac3-launcher-v2-successor-production-"
    "dispatch-fence-correction-arm64-v1.json"
)

# These are accepted only if the downloaded arm64 artifact has exactly these
# bytes.  A mismatch is a hard stop, never a reason to synthesize replacement
# evidence locally.
RESULT_SHA256 = "7efe89c3bc558455313b76de2a625e708a580d0256760692914e9474eb0171f0"
RESULT_SIZE_BYTES = 6_928
HARD_STOP_SHA256 = (
    "7a8cf17bfedfbe424978f41b38314d6ba5304a36df49a566a60ff4e5114328eb"
)
HARD_STOP_SIZE_BYTES = 8_120
P1_SHA256 = "576bafd10600a05e9ab326e1e507c1a0351381d068f393ce402e295bf93afbec"
P1_SIZE_BYTES = 20_145

EXPECTED_KEYS = {
    "activationAllowed",
    "authorisations",
    "bootableClaim",
    "boundInputs",
    "effects",
    "executionEnvelope",
    "generationFiles",
    "measurement",
    "predecessors",
    "productionDispatchFenceCorrection",
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
EXPECTED_MEASUREMENT = {
    "byKind": {"directory": 1_737, "file": 15_102, "symlink": 837},
    "caseFoldedSiblings": 20,
    "duplicatePaths": 0,
    "entries": 17_676,
    "largestFileBytes": 160_096_808,
    "largestFilePath": (
        "opt/boole/native-checker-toolchain/lib/"
        "libLLVM.so.22.1-rust-1.99.0-nightly"
    ),
    "pathCollisions": 0,
    "pathManifestSha256": (
        "0dbc17aeaaa8ef63ddeb53ac8b7615f361c21bda95f0ba3d9677bdbdb76dcb9a"
    ),
    "payloadBytes": 1_773_475_059,
    "symlinkEscapes": 0,
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

PREDECESSOR_PATHS = (
    "native/containment/native-shadow-mac3-launcher-v2-successor-production-"
    "generation-preregistration-arm64-v1.json",
    "native/containment/native-shadow-mac3-launcher-v2-successor-producer-"
    "rehearsal-result-arm64-v1.json",
    "native/containment/native-shadow-mac3-successor-producer-fingerprint-"
    "arm64-v5.json",
)
GENERATION_PATHS = (
    "scripts/native_shadow_successor_produce_phase_arm64_v4.py",
    "scripts/test_native_shadow_successor_produce_phase_arm64_v4.py",
    "scripts/native-shadow-successor-produce-arm64-v4.sh",
    ".github/workflows/native-shadow-successor-produce-arm64-v4.yml",
    "scripts/test_native_shadow_successor_produce_workflow_arm64_v4.py",
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


def assert_strict_equal(actual: object, expected: object, path: str = "$.") -> None:
    if type(actual) is not type(expected):
        raise AssertionError(
            f"{path} type differs: {type(actual).__name__} != "
            f"{type(expected).__name__}"
        )
    if isinstance(expected, dict):
        if set(actual) != set(expected):
            raise AssertionError(f"{path} keys differ")
        for key in expected:
            assert_strict_equal(actual[key], expected[key], f"{path}{key}.")
        return
    if isinstance(expected, list):
        if len(actual) != len(expected):
            raise AssertionError(f"{path} length differs")
        for index, (actual_item, expected_item) in enumerate(zip(actual, expected)):
            assert_strict_equal(actual_item, expected_item, f"{path}{index}.")
        return
    if actual != expected:
        raise AssertionError(f"{path} value differs: {actual!r} != {expected!r}")


class LauncherV2SuccessorProducerRehearsalResultV2Tests(unittest.TestCase):
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
            "rehearsal.arm64.v2",
        )
        self.assertEqual(self.result["status"], "PASS-NO-IMAGE-PRODUCED")

    def test_result_preserves_strict_zero_authority_and_effects(self) -> None:
        assert_strict_equal(self.result["authorisations"], ZERO_AUTHORISATIONS)
        assert_strict_equal(self.result["effects"], ZERO_EFFECTS)
        self.assertIs(self.result["activationAllowed"], False)
        self.assertIs(self.result["bootableClaim"], False)
        self.assertIs(self.result["repeatable"], True)

    def test_lineage_arrays_rederive_exact_live_bytes_in_order(self) -> None:
        predecessors = [live_identity(path) for path in PREDECESSOR_PATHS]
        generation = [live_identity(path) for path in GENERATION_PATHS]
        reused = [live_identity(path) for path in REUSED_PATHS]
        self.assertEqual(self.result["predecessors"], predecessors)
        self.assertEqual(self.result["generationFiles"], generation)
        self.assertEqual(self.result["reusedPinnedUpstream"], reused)
        self.assertEqual(
            self.result["boundInputs"], [*predecessors, *generation, *reused]
        )
        paths = [row["path"] for row in self.result["boundInputs"]]
        self.assertEqual(len(paths), 11)
        self.assertEqual(len(set(paths)), 11)
        for row in self.result["boundInputs"]:
            self.assertEqual(set(row), {"path", "sha256", "sizeBytes"})

    def test_dispatch_correction_is_direct_and_not_duplicated_in_union(self) -> None:
        correction = live_identity(P3_PATH.relative_to(REPO).as_posix())
        self.assertEqual(
            self.result["productionDispatchFenceCorrection"], correction
        )
        self.assertNotIn(
            correction["path"], [row["path"] for row in self.result["boundInputs"]]
        )

    def test_measurement_matches_both_p1_and_the_frozen_literal(self) -> None:
        p1_raw = read_regular(P1_PATH)
        self.assertEqual(len(p1_raw), P1_SIZE_BYTES)
        self.assertEqual(sha256_bytes(p1_raw), P1_SHA256)
        p1 = json.loads(p1_raw.decode("utf-8"))
        assert_strict_equal(self.result["measurement"], EXPECTED_MEASUREMENT)
        assert_strict_equal(
            self.result["measurement"], p1["expectedPreflight"]["measurement"]
        )

    def test_execution_envelope_states_only_what_the_kernel_observed(self) -> None:
        assert_strict_equal(
            self.result["executionEnvelope"], EXPECTED_EXECUTION_ENVELOPE
        )
        runtime = self.result["executionEnvelope"]["systemdRuntimeMaxSec"]
        self.assertIs(runtime["kernelObserved"], False)
        self.assertIs(runtime["managerValueQueried"], False)

    def test_first_two_failure_record_remains_separate_from_success_payload(self) -> None:
        raw = read_regular(HARD_STOP_PATH)
        self.assertEqual(len(raw), HARD_STOP_SIZE_BYTES)
        self.assertEqual(sha256_bytes(raw), HARD_STOP_SHA256)
        self.assertNotIn(HARD_STOP_PATH.relative_to(REPO).as_posix(), self.raw.decode())
        forbidden_top_level_keys = {
            "artifactId",
            "archiveDigest",
            "headSha",
            "jobId",
            "runId",
            "F6",
            "A6",
        }
        self.assertTrue(forbidden_top_level_keys.isdisjoint(self.result))


if __name__ == "__main__":
    unittest.main()
