#!/usr/bin/env python3
"""Pin the append-only scope correction for the third failed v4 rehearsal."""

from __future__ import annotations

import hashlib
import json
import pathlib
import stat
import unittest


REPO = pathlib.Path(__file__).resolve().parents[1]
CORRECTION_PATH = REPO / (
    "native/containment/native-shadow-mac3-launcher-v2-successor-producer-"
    "rehearsal-hard-stop-correction-arm64-v1.json"
)
V3_PATH = REPO / (
    "native/containment/native-shadow-mac3-launcher-v2-successor-producer-"
    "rehearsal-hard-stop-arm64-v3.json"
)
SELF_TEST_PATH = REPO / "scripts/self-test.sh"
DOC_PATHS = (
    REPO / "docs/mac-first-hidden-linux-execution-plan-v1.md",
    REPO / "docs/node-native-shadow-binding-containment-implementation-spec-v1.md",
    REPO / "docs/native-submission-shadow-verification-v1.md",
)
SECTION_BEGIN = (
    "<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-THIRD-FAILED-ATTEMPT-"
    "SCOPE-CORRECTION-ARM64-V1-SEALED:BEGIN -->"
)
SECTION_END = (
    "<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-THIRD-FAILED-ATTEMPT-"
    "SCOPE-CORRECTION-ARM64-V1-SEALED:END -->"
)

CORRECTION_SHA256 = "88a7fc38963f48fa42018ba7e29ab5648f6767f7cecaac66d1aa4e7047c292c8"
CORRECTION_SIZE_BYTES = 2_837
V3_SHA256 = "3cfe5cb9df41c15206e3ca56d5224c7b5e03ebb0a118d8a49fd9b4154bc86e07"
V3_SIZE_BYTES = 5_028

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


class LauncherV2SuccessorProducerHardStopCorrectionV1Tests(unittest.TestCase):
    def setUp(self) -> None:
        self.raw = read_regular(CORRECTION_PATH)
        self.record = json.loads(self.raw.decode("utf-8"))

    def test_correction_is_exact_canonical_json(self) -> None:
        self.assertEqual(self.raw, canonical_json(self.record))
        self.assertEqual(len(self.raw), CORRECTION_SIZE_BYTES)
        self.assertEqual(sha256(self.raw), CORRECTION_SHA256)

    def test_original_v3_record_remains_byte_unchanged(self) -> None:
        raw = read_regular(V3_PATH)
        self.assertEqual(len(raw), V3_SIZE_BYTES)
        self.assertEqual(sha256(raw), V3_SHA256)
        self.assertEqual(
            self.record["correctedRecord"],
            {
                "path": V3_PATH.relative_to(REPO).as_posix(),
                "preservedByteUnchanged": True,
                "sha256": V3_SHA256,
                "sizeBytes": V3_SIZE_BYTES,
            },
        )

    def test_correction_narrows_only_the_service_subject(self) -> None:
        correction = self.record["corrections"]["serviceSubject"]
        self.assertEqual(correction["historicalPhrase"], "claim-bound systemd service")
        self.assertEqual(
            correction["correctedPhrase"],
            "root-owned HEAD-bound rehearsal systemd service",
        )
        self.assertEqual(correction["productionClaimTagsObserved"], 0)
        self.assertTrue(correction["historicalPhraseWasOverbroad"])

    def test_correction_narrows_only_the_image_effect_scope(self) -> None:
        correction = self.record["corrections"]["imageEffectScope"]
        self.assertEqual(correction["historicalField"], "imageFilesCreatedByRehearsal")
        self.assertEqual(correction["historicalValue"], 0)
        self.assertTrue(correction["historicalFieldWasOverbroad"])
        self.assertEqual(correction["finalGuestImageOutputsCreatedByRehearsal"], 0)
        self.assertTrue(correction["transientOciScratchLayoutCreatedByRehearsal"])
        self.assertFalse(correction["runnerGlobalCleanupClaimed"])

    def test_run_identity_and_exact_failure_are_unchanged(self) -> None:
        self.assertEqual(
            self.record["sourceAttempt"],
            {
                "exactFailure": (
                    "native-shadow successor producer v4: FAIL: verified layer "
                    "'etc/rmt' link escapes"
                ),
                "freeRehearsalJobId": 99_269_811_610,
                "headSha": "8dc57c531b01e4b2b72864969eddfdeaeb6cda5a",
                "runId": 33_316_130_780,
            },
        )

    def test_correction_grants_no_authority_or_success_claim(self) -> None:
        self.assertEqual(self.record["authorisations"], ZERO_AUTHORISATIONS)
        self.assertFalse(self.record["activationAllowed"])
        self.assertFalse(self.record["bootableClaim"])
        self.assertFalse(self.record["cleanupCompleteClaim"])
        self.assertFalse(self.record["offlineClaim"])
        self.assertFalse(self.record["r2SealedByThisCorrection"])
        self.assertEqual(self.record["successfulR2ResultsCreatedByThisCorrection"], 0)

    def test_record_is_exactly_a_two_claim_append_only_correction(self) -> None:
        self.assertEqual(
            self.record["schema"],
            "boole.native-shadow.mac3.launcher-v2-successor-producer-rehearsal-"
            "hard-stop-correction.arm64.v1",
        )
        self.assertEqual(self.record["status"], "CORRECTED-SCOPE-NO-R2-NO-AUTHORITY")
        self.assertTrue(self.record["appendOnly"])
        self.assertFalse(self.record["editsAnyEarlierRecord"])
        self.assertEqual(
            set(self.record["corrections"]), {"imageEffectScope", "serviceSubject"}
        )

    def test_gate_and_three_docs_pin_the_correction(self) -> None:
        gate = pathlib.Path(__file__).name
        self.assertEqual(SELF_TEST_PATH.read_text().count(gate), 1)
        for path in DOC_PATHS:
            text = path.read_text(encoding="utf-8")
            self.assertEqual(text.count(SECTION_BEGIN), 1, path.name)
            self.assertEqual(text.count(SECTION_END), 1, path.name)
            section = text.split(SECTION_BEGIN, 1)[1].split(SECTION_END, 1)[0]
            self.assertIn(CORRECTION_PATH.relative_to(REPO).as_posix(), section)
            self.assertIn(CORRECTION_SHA256, section)
            self.assertIn("root-owned HEAD-bound rehearsal systemd service", section)
            self.assertIn("transient OCI scratch layout", section)
            self.assertIn("final guest image outputs remained at zero", section)


if __name__ == "__main__":
    unittest.main()
