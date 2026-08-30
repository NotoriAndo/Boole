#!/usr/bin/env python3
"""Seal the fourth failed free v4 rehearsal without turning it into R2."""

from __future__ import annotations

import hashlib
import json
import pathlib
import stat
import unittest


REPO = pathlib.Path(__file__).resolve().parents[1]
RECORD_PATH = REPO / (
    "native/containment/native-shadow-mac3-launcher-v2-successor-producer-"
    "rehearsal-hard-stop-arm64-v4.json"
)
PREDECESSOR_PATH = REPO / (
    "native/containment/native-shadow-mac3-launcher-v2-successor-producer-"
    "rehearsal-hard-stop-correction-arm64-v1.json"
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
    "<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-FOURTH-FAILED-ATTEMPT-"
    "ARM64-V4-SEALED:BEGIN -->"
)
SECTION_END = (
    "<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-FOURTH-FAILED-ATTEMPT-"
    "ARM64-V4-SEALED:END -->"
)

RECORD_SHA256 = "96721d93d6016a6ee9c8714672ee9e49c0672336181bc1ef8082ab5445081eae"
RECORD_SIZE_BYTES = 6_147
PREDECESSOR_SHA256 = (
    "88a7fc38963f48fa42018ba7e29ab5648f6767f7cecaac66d1aa4e7047c292c8"
)
PREDECESSOR_SIZE_BYTES = 2_837

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


class LauncherV2SuccessorProducerRehearsalHardStopV4Tests(unittest.TestCase):
    def setUp(self) -> None:
        self.raw = read_regular(RECORD_PATH)
        self.record = json.loads(self.raw.decode("utf-8"))

    def test_record_is_exact_canonical_json(self) -> None:
        self.assertEqual(self.raw, canonical_json(self.record))
        self.assertEqual(len(self.raw), RECORD_SIZE_BYTES)
        self.assertEqual(sha256(self.raw), RECORD_SHA256)

    def test_record_is_only_the_fourth_failed_free_rehearsal(self) -> None:
        self.assertEqual(
            self.record["schema"],
            "boole.native-shadow.mac3.launcher-v2-successor-producer-rehearsal-"
            "hard-stop.arm64.v4",
        )
        self.assertEqual(self.record["status"], "HARD-STOP")
        self.assertEqual(self.record["outcome"], "FOURTH-FAILED-FREE-REHEARSAL")
        self.assertEqual(self.record["successfulR2ResultsCreatedByThisAttempt"], 0)
        self.assertEqual(self.record["r2PayloadsCreatedByThisAttempt"], 0)
        self.assertNotIn("PASS-NO-IMAGE-PRODUCED", json.dumps(self.record))

    def test_predecessor_is_the_unchanged_scope_correction(self) -> None:
        raw = read_regular(PREDECESSOR_PATH)
        self.assertEqual(len(raw), PREDECESSOR_SIZE_BYTES)
        self.assertEqual(sha256(raw), PREDECESSOR_SHA256)
        self.assertEqual(
            self.record["predecessor"],
            {
                "path": PREDECESSOR_PATH.relative_to(REPO).as_posix(),
                "sha256": PREDECESSOR_SHA256,
                "sizeBytes": PREDECESSOR_SIZE_BYTES,
            },
        )

    def test_attempt_pins_the_official_run_jobs_and_exact_failure(self) -> None:
        attempt = self.record["attempt"]
        self.assertEqual(attempt["artifactApiTotalCount"], 0)
        self.assertEqual(attempt["runId"], 33_319_199_252)
        self.assertEqual(attempt["freeRehearsalJobId"], 99_278_062_868)
        self.assertEqual(attempt["headSha"], "0029b3df45b87a2f2643abfff0f30f57f0c46d48")
        self.assertEqual(attempt["runConclusion"], "failure")
        self.assertEqual(
            attempt["failure"],
            {
                "conclusion": "failure",
                "exactMessage": (
                    "native-shadow successor producer v4: FAIL: low-level image "
                    "preparation failed: the content-addressed store holds no object "
                    "89c94171d47851896b9c0bf600dd753b5b8770a4550b38304cd873fa7c8aabea"
                ),
                "stepName": (
                    "Run the v4 rehearsal from a verified root-owned HEAD anchor"
                ),
                "stepNumber": 9,
                "systemdTransientServiceStarted": True,
            },
        )
        self.assertEqual(
            attempt["jobs"],
            {
                "compare": {"conclusion": "skipped", "jobId": 99_278_063_467},
                "produce": {"conclusion": "skipped", "jobId": 99_278_063_403},
                "productionAuthorityGuard": {
                    "conclusion": "skipped",
                    "jobId": 99_278_063_331,
                },
            },
        )

    def test_diagnosis_names_both_missing_writer_packages_and_the_narrow_fix(self) -> None:
        diagnosis = self.record["diagnosis"]
        self.assertEqual(diagnosis["classification"], "REHEARSAL-WIRING-OMISSION")
        self.assertEqual(
            {row["name"] for row in diagnosis["writerPackagesAbsentFromRehearsalCas"]},
            {"e2fsprogs", "libext2fs2t64"},
        )
        self.assertEqual(
            {row["sha256"] for row in diagnosis["writerPackagesAbsentFromRehearsalCas"]},
            {
                "89c94171d47851896b9c0bf600dd753b5b8770a4550b38304cd873fa7c8aabea",
                "da4d465823f2653b35bd316f9c479e4a531165e01840151184f015f6e0d391a5",
            },
        )
        self.assertEqual(
            diagnosis["minimalSuccessor"],
            "invoke the already-sealed writer-set acquirer in the free rehearsal before root isolation",
        )
        self.assertFalse(diagnosis["fixIncludedInThisRecord"])

    def test_authority_and_effect_claims_stay_narrow_and_zero(self) -> None:
        self.assertEqual(self.record["authorisations"], ZERO_AUTHORISATIONS)
        self.assertFalse(self.record["activationAllowed"])
        self.assertFalse(self.record["bootableClaim"])
        self.assertFalse(self.record["cleanupCompleteClaim"])
        self.assertFalse(self.record["offlineClaim"])
        effects = self.record["scopedObservedEffects"]
        self.assertEqual(effects["artifactsUploadedByWorkflow"], 0)
        self.assertEqual(effects["attemptMarkersCreatedByRehearsal"], 0)
        self.assertEqual(effects["bootAttemptsStartedByWorkflow"], 0)
        self.assertEqual(effects["finalGuestImageOutputsCreatedByRehearsal"], 0)
        self.assertEqual(effects["productionClaimTagsCreatedByWorkflow"], 0)
        self.assertEqual(effects["productionJobsExecuted"], 0)
        self.assertEqual(effects["rehearsalResultFilesCreated"], 0)
        self.assertTrue(effects["transientOciScratchLayoutCreatedByRehearsal"])
        self.assertFalse(effects["runnerGlobalCleanupClaimed"])

    def test_failure_and_future_success_use_distinct_append_only_paths(self) -> None:
        self.assertEqual(
            self.record["appendOnlyTargetBinding"],
            {
                "pathsAreDistinct": True,
                "successfulR2PathNotOccupiedByThisRecord": True,
                "successfulR2ResultPath": R2_PATH,
                "thisHardStopRecordPath": RECORD_PATH.relative_to(REPO).as_posix(),
            },
        )

    def test_gate_and_three_docs_pin_this_separate_failure_record(self) -> None:
        gate = pathlib.Path(__file__).name
        self.assertEqual(SELF_TEST_PATH.read_text().count(gate), 1)
        for path in DOC_PATHS:
            text = path.read_text(encoding="utf-8")
            self.assertEqual(text.count(SECTION_BEGIN), 1, path.name)
            self.assertEqual(text.count(SECTION_END), 1, path.name)
            section = text.split(SECTION_BEGIN, 1)[1].split(SECTION_END, 1)[0]
            self.assertIn(RECORD_PATH.relative_to(REPO).as_posix(), section)
            self.assertIn(RECORD_SHA256, section)
            self.assertIn("33319199252", section)
            self.assertIn("99278062868", section)


if __name__ == "__main__":
    unittest.main()
