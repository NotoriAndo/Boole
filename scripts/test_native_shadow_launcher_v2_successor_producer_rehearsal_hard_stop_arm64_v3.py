#!/usr/bin/env python3
"""Seal the third failed free v4 rehearsal without turning it into R2."""

from __future__ import annotations

import hashlib
import json
import pathlib
import stat
import unittest


REPO = pathlib.Path(__file__).resolve().parents[1]
RECORD_PATH = REPO / (
    "native/containment/native-shadow-mac3-launcher-v2-successor-producer-"
    "rehearsal-hard-stop-arm64-v3.json"
)
V2_PATH = REPO / (
    "native/containment/native-shadow-mac3-launcher-v2-successor-producer-"
    "rehearsal-hard-stop-arm64-v2.json"
)
R2_PATH = (
    "native/containment/native-shadow-mac3-launcher-v2-successor-producer-"
    "rehearsal-result-arm64-v2.json"
)
SELF_TEST_PATH = REPO / "scripts/self-test.sh"
DOC_PATHS = (
    REPO / "docs/mac-first-hidden-linux-execution-plan-v1.md",
    REPO / "docs/node-native-shadow-binding-containment-implementation-spec-v1.md",
    REPO / "docs/native-submission-shadow-verification-v1.md",
)
SECTION_BEGIN = (
    "<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-THIRD-FAILED-ATTEMPT-"
    "ARM64-V3-SEALED:BEGIN -->"
)
SECTION_END = (
    "<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-THIRD-FAILED-ATTEMPT-"
    "ARM64-V3-SEALED:END -->"
)

# Filled only from the canonical append-only record after its absence produces
# RED.  These constants must never be inferred from a future successful R2.
RECORD_SHA256 = "3cfe5cb9df41c15206e3ca56d5224c7b5e03ebb0a118d8a49fd9b4154bc86e07"
RECORD_SIZE_BYTES = 5_028
V2_SHA256 = "7a8cf17bfedfbe424978f41b38314d6ba5304a36df49a566a60ff4e5114328eb"
V2_SIZE_BYTES = 8_120

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
    "artifactsUploadedByWorkflow": 0,
    "attemptMarkersCreatedByRehearsal": 0,
    "bootAttemptsStartedByWorkflow": 0,
    "imageFilesCreatedByRehearsal": 0,
    "productionClaimTagsCreatedByWorkflow": 0,
    "productionJobsExecuted": 0,
    "rehearsalResultFilesCreated": 0,
}


def canonical_json(value: object) -> bytes:
    return (
        json.dumps(value, sort_keys=True, indent=2, ensure_ascii=False) + "\n"
    ).encode("utf-8")


def read_regular(path: pathlib.Path) -> bytes:
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or path.is_symlink():
        raise AssertionError(f"{path.relative_to(REPO)} is not regular")
    return path.read_bytes()


def sha256(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


class LauncherV2SuccessorProducerRehearsalHardStopV3Tests(unittest.TestCase):
    def setUp(self) -> None:
        self.raw = read_regular(RECORD_PATH)
        self.record = json.loads(self.raw.decode("utf-8"))

    def test_record_is_exact_canonical_json(self) -> None:
        self.assertEqual(self.raw, canonical_json(self.record))
        self.assertEqual(len(self.raw), RECORD_SIZE_BYTES)
        self.assertEqual(sha256(self.raw), RECORD_SHA256)

    def test_record_is_only_the_third_failed_free_rehearsal(self) -> None:
        self.assertEqual(
            self.record["schema"],
            "boole.native-shadow.mac3.launcher-v2-successor-producer-rehearsal-"
            "hard-stop.arm64.v3",
        )
        self.assertEqual(self.record["status"], "HARD-STOP")
        self.assertEqual(self.record["outcome"], "THIRD-FAILED-FREE-REHEARSAL")
        self.assertEqual(self.record["successfulR2ResultsCreatedByThisAttempt"], 0)
        self.assertEqual(self.record["r2PayloadsCreatedByThisAttempt"], 0)
        self.assertNotIn("PASS-NO-IMAGE-PRODUCED", json.dumps(self.record))

    def test_predecessor_is_the_unchanged_two_attempt_record(self) -> None:
        raw = read_regular(V2_PATH)
        self.assertEqual(len(raw), V2_SIZE_BYTES)
        self.assertEqual(sha256(raw), V2_SHA256)
        self.assertEqual(
            self.record["predecessor"],
            {
                "path": V2_PATH.relative_to(REPO).as_posix(),
                "sha256": V2_SHA256,
                "sizeBytes": V2_SIZE_BYTES,
            },
        )

    def test_attempt_pins_the_official_run_job_and_exact_failure(self) -> None:
        attempt = self.record["attempt"]
        self.assertEqual(
            attempt,
            {
                "artifactApiTotalCount": 0,
                "completedAt": "2026-08-30T14:14:46Z",
                "event": "workflow_dispatch",
                "failure": {
                    "conclusion": "failure",
                    "exactMessage": (
                        "native-shadow successor producer v4: FAIL: verified "
                        "layer 'etc/rmt' link escapes"
                    ),
                    "stepName": (
                        "Run the v4 rehearsal from a verified root-owned HEAD anchor"
                    ),
                    "stepNumber": 9,
                    "systemdTransientServiceStarted": True,
                },
                "freeRehearsalJobId": 99_269_811_610,
                "freeRehearsalJobUrl": (
                    "https://github.com/NotoriAndo/Boole/actions/runs/"
                    "33316130780/job/99269811610"
                ),
                "headSha": "8dc57c531b01e4b2b72864969eddfdeaeb6cda5a",
                "jobs": {
                    "compare": {"conclusion": "skipped", "jobId": 99_269_812_413},
                    "produce": {"conclusion": "skipped", "jobId": 99_269_812_495},
                    "productionAuthorityGuard": {
                        "conclusion": "skipped",
                        "jobId": 99_269_812_285,
                    },
                },
                "mode": "rehearsal",
                "runAttempt": 1,
                "runConclusion": "failure",
                "runId": 33_316_130_780,
                "runUrl": (
                    "https://github.com/NotoriAndo/Boole/actions/runs/33316130780"
                ),
                "scopedObservedEffects": ZERO_EFFECTS,
                "startedAt": "2026-08-30T14:10:17Z",
                "workflow": "native-shadow-successor-produce-arm64-v4",
            },
        )

    def test_failure_happened_after_prerequisites_and_before_payload_steps(self) -> None:
        self.assertEqual(
            self.record["preparationSteps"],
            [
                {"number": 2, "name": "Checkout", "conclusion": "success"},
                {
                    "number": 3,
                    "name": "Verify the complete preregistered v4 bindings first",
                    "conclusion": "success",
                },
                {
                    "number": 4,
                    "name": "Complete exact HEAD history without importing tags",
                    "conclusion": "success",
                },
                {
                    "number": 5,
                    "name": "Install Rust toolchain",
                    "conclusion": "success",
                },
                {
                    "number": 6,
                    "name": "Acquire the frozen Rust distribution and re-prove its record",
                    "conclusion": "success",
                },
                {
                    "number": 7,
                    "name": "Acquire the frozen package payloads",
                    "conclusion": "success",
                },
                {
                    "number": 8,
                    "name": "Emit and independently match the sealed launcher v2",
                    "conclusion": "success",
                },
            ],
        )
        self.assertEqual(
            self.record["postFailureSteps"],
            [
                {
                    "number": 10,
                    "name": "Require exactly one canonical R2 JSON member",
                    "conclusion": "skipped",
                },
                {
                    "number": 11,
                    "name": "Keep the sole authority-zero rehearsal result for seven days",
                    "conclusion": "skipped",
                },
            ],
        )

    def test_authority_effects_and_claims_remain_narrow_and_zero(self) -> None:
        self.assertEqual(self.record["authorisations"], ZERO_AUTHORISATIONS)
        self.assertEqual(self.record["attempt"]["scopedObservedEffects"], ZERO_EFFECTS)
        self.assertFalse(self.record["activationAllowed"])
        self.assertFalse(self.record["bootableClaim"])
        self.assertFalse(self.record["offlineClaim"])
        self.assertFalse(self.record["runnerGlobalTransientAbsenceClaim"])
        self.assertFalse(self.record["cleanupCompleteClaim"])
        self.assertEqual(
            set(self.record["claimsNotMade"]),
            {
                "complete cleanup of runner-global transient state",
                "global absence of all filesystem or process side effects",
                "offline execution of dependency acquisition",
                "successful R2 or any rehearsal PASS",
                "production, image, boot, MAC.4, testnet, mining, reward, consensus or P2P authority",
            },
        )

    def test_failure_and_future_success_use_distinct_append_only_paths(self) -> None:
        binding = self.record["appendOnlyTargetBinding"]
        self.assertEqual(
            binding,
            {
                "pathsAreDistinct": True,
                "successfulR2PathNotOccupiedByThisRecord": True,
                "successfulR2ResultPath": R2_PATH,
                "thisHardStopRecordPath": RECORD_PATH.relative_to(REPO).as_posix(),
            },
        )

    def test_self_test_and_three_docs_pin_this_separate_failure_record(self) -> None:
        gate = pathlib.Path(__file__).name
        self.assertEqual(SELF_TEST_PATH.read_text().count(gate), 1)
        for path in DOC_PATHS:
            text = path.read_text(encoding="utf-8")
            self.assertEqual(text.count(SECTION_BEGIN), 1, path.name)
            self.assertEqual(text.count(SECTION_END), 1, path.name)
            section = text.split(SECTION_BEGIN, 1)[1].split(SECTION_END, 1)[0]
            self.assertIn(RECORD_PATH.relative_to(REPO).as_posix(), section)
            self.assertIn(RECORD_SHA256, section)
            self.assertIn("33316130780", section)
            self.assertIn("99269811610", section)


if __name__ == "__main__":
    unittest.main()
