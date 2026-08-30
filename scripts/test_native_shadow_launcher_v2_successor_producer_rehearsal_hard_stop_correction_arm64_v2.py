#!/usr/bin/env python3
"""Pin the append-only wording correction for the fourth failed rehearsal."""

from __future__ import annotations

import hashlib
import json
import pathlib
import stat
import unittest


REPO = pathlib.Path(__file__).resolve().parents[1]
CORRECTION_PATH = REPO / (
    "native/containment/native-shadow-mac3-launcher-v2-successor-producer-"
    "rehearsal-hard-stop-correction-arm64-v2.json"
)
V4_PATH = REPO / (
    "native/containment/native-shadow-mac3-launcher-v2-successor-producer-"
    "rehearsal-hard-stop-arm64-v4.json"
)
SELF_TEST_PATH = REPO / "scripts/self-test.sh"
DOC_PATHS = (
    REPO / "docs/mac-first-hidden-linux-execution-plan-v1.md",
    REPO / "docs/node-native-shadow-binding-containment-implementation-spec-v1.md",
    REPO / "docs/native-submission-shadow-verification-v1.md",
)
SECTION_BEGIN = (
    "<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-FOURTH-FAILED-ATTEMPT-"
    "WORDING-CORRECTION-ARM64-V2-SEALED:BEGIN -->"
)
SECTION_END = (
    "<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-FOURTH-FAILED-ATTEMPT-"
    "WORDING-CORRECTION-ARM64-V2-SEALED:END -->"
)

CORRECTION_SHA256 = "b0f140161df0029eec5359a25d2ec6a207511d6787fa7a9000de997a95b90177"
CORRECTION_SIZE_BYTES = 3_043
V4_SHA256 = "96721d93d6016a6ee9c8714672ee9e49c0672336181bc1ef8082ab5445081eae"
V4_SIZE_BYTES = 6_147

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


class LauncherV2SuccessorProducerHardStopCorrectionV2Tests(unittest.TestCase):
    def setUp(self) -> None:
        self.raw = read_regular(CORRECTION_PATH)
        self.record = json.loads(self.raw.decode("utf-8"))

    def test_correction_is_exact_canonical_json(self) -> None:
        self.assertEqual(self.raw, canonical_json(self.record))
        self.assertEqual(len(self.raw), CORRECTION_SIZE_BYTES)
        self.assertEqual(sha256(self.raw), CORRECTION_SHA256)

    def test_original_v4_record_remains_byte_unchanged(self) -> None:
        raw = read_regular(V4_PATH)
        self.assertEqual(len(raw), V4_SIZE_BYTES)
        self.assertEqual(sha256(raw), V4_SHA256)
        self.assertEqual(
            self.record["correctedRecord"],
            {
                "path": V4_PATH.relative_to(REPO).as_posix(),
                "preservedByteUnchanged": True,
                "sha256": V4_SHA256,
                "sizeBytes": V4_SIZE_BYTES,
            },
        )

    def test_correction_replaces_execution_wording_with_static_wiring_fact(self) -> None:
        correction = self.record["corrections"]["productionWriterAcquisition"]
        self.assertEqual(
            correction["historicalField"],
            "productionPathAlreadyInvokedTheWriterSetAcquirer",
        )
        self.assertTrue(correction["historicalValue"])
        self.assertTrue(correction["historicalFieldWasOverbroad"])
        self.assertEqual(
            correction["correctedField"],
            "productionPathAlreadyWiredToInvokeTheWriterSetAcquirer",
        )
        self.assertTrue(correction["correctedValue"])
        self.assertEqual(correction["productionRunsObserved"], 0)
        self.assertEqual(correction["productionClaimTagsObserved"], 0)

    def test_run_identity_and_failure_are_unchanged(self) -> None:
        self.assertEqual(
            self.record["sourceAttempt"],
            {
                "exactFailure": (
                    "native-shadow successor producer v4: FAIL: low-level image "
                    "preparation failed: the content-addressed store holds no object "
                    "89c94171d47851896b9c0bf600dd753b5b8770a4550b38304cd873fa7c8aabea"
                ),
                "freeRehearsalJobId": 99_278_062_868,
                "headSha": "0029b3df45b87a2f2643abfff0f30f57f0c46d48",
                "runId": 33_319_199_252,
            },
        )

    def test_job_timestamps_and_writer_evidence_are_scoped_precisely(self) -> None:
        timestamp = self.record["corrections"]["timestampScope"]
        self.assertEqual(timestamp["historicalStartedAt"], "2026-08-30T15:17:05Z")
        self.assertEqual(timestamp["historicalCompletedAt"], "2026-08-30T15:20:55Z")
        self.assertEqual(timestamp["correctScope"], "free-rehearsal job")
        self.assertEqual(timestamp["workflowRunStartedAt"], "2026-08-30T15:17:00Z")
        self.assertEqual(timestamp["workflowRunUpdatedAt"], "2026-08-30T15:20:56Z")

        evidence = self.record["corrections"]["writerOmissionEvidence"]
        self.assertEqual(
            evidence["directlyObservedMissingObject"],
            {
                "name": "e2fsprogs",
                "sha256": "89c94171d47851896b9c0bf600dd753b5b8770a4550b38304cd873fa7c8aabea",
            },
        )
        self.assertEqual(
            {row["name"] for row in evidence["staticallyDerivedOmittedWriterSet"]},
            {"e2fsprogs", "libext2fs2t64"},
        )

    def test_correction_grants_no_authority_or_success_claim(self) -> None:
        self.assertEqual(self.record["authorisations"], ZERO_AUTHORISATIONS)
        self.assertFalse(self.record["activationAllowed"])
        self.assertFalse(self.record["bootableClaim"])
        self.assertFalse(self.record["cleanupCompleteClaim"])
        self.assertFalse(self.record["offlineClaim"])
        self.assertFalse(self.record["r2SealedByThisCorrection"])
        self.assertEqual(self.record["successfulR2ResultsCreatedByThisCorrection"], 0)

    def test_record_is_exactly_three_append_only_scope_corrections(self) -> None:
        self.assertEqual(
            self.record["schema"],
            "boole.native-shadow.mac3.launcher-v2-successor-producer-rehearsal-"
            "hard-stop-correction.arm64.v2",
        )
        self.assertEqual(self.record["status"], "CORRECTED-SCOPE-NO-R2-NO-AUTHORITY")
        self.assertTrue(self.record["appendOnly"])
        self.assertFalse(self.record["editsAnyEarlierRecord"])
        self.assertEqual(
            set(self.record["corrections"]),
            {"productionWriterAcquisition", "timestampScope", "writerOmissionEvidence"},
        )

    def test_gate_and_three_docs_pin_the_correction(self) -> None:
        gate = pathlib.Path(__file__).name
        self.assertEqual(SELF_TEST_PATH.read_text().count(gate), 1)
        for path in DOC_PATHS:
            text = path.read_text(encoding="utf-8")
            self.assertEqual(text.count(SECTION_BEGIN), 1, path.name)
            self.assertEqual(text.count(SECTION_END), 1, path.name)
            section = text.split(SECTION_BEGIN, 1)[1].split(SECTION_END, 1)[0]
            words = " ".join(section.split())
            self.assertIn(CORRECTION_PATH.relative_to(REPO).as_posix(), section)
            self.assertIn(CORRECTION_SHA256, section)
            self.assertIn("wired to invoke", words)
            self.assertIn("production execution is not claimed", words)


if __name__ == "__main__":
    unittest.main()
