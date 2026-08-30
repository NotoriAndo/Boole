#!/usr/bin/env python3
"""Seal the two failed free v4 rehearsals without turning either into R2."""

from __future__ import annotations

import hashlib
import json
import pathlib
import stat
import unittest


REPO = pathlib.Path(__file__).resolve().parents[1]
RECORD_PATH = REPO / (
    "native/containment/native-shadow-mac3-launcher-v2-successor-producer-"
    "rehearsal-hard-stop-arm64-v2.json"
)
SELF_TEST_PATH = REPO / "scripts/self-test.sh"
DOCS_SMOKE_PATH = REPO / "scripts/docs-smoke.sh"
PLAN_PATH = REPO / "docs/mac-first-hidden-linux-execution-plan-v1.md"
SPEC_PATH = (
    REPO
    / "docs/node-native-shadow-binding-containment-implementation-spec-v1.md"
)
SHADOW_PATH = REPO / "docs/native-submission-shadow-verification-v1.md"
SECTION_BEGIN = (
    "<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-FAILED-ATTEMPTS-"
    "ARM64-V2-SEALED:BEGIN -->"
)
SECTION_END = (
    "<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-FAILED-ATTEMPTS-"
    "ARM64-V2-SEALED:END -->"
)

# Filled with the canonical record's identity after the RED cycle proves the
# record is absent.  Keeping this as an exact byte pin makes the failure
# history append-only rather than an editable summary.
RECORD_SHA256 = "7a8cf17bfedfbe424978f41b38314d6ba5304a36df49a566a60ff4e5114328eb"
RECORD_SIZE_BYTES = 8_120
SUCCESSFUL_R2_PATH = (
    "native/containment/native-shadow-mac3-launcher-v2-successor-producer-"
    "rehearsal-result-arm64-v2.json"
)

AUTHORISATIONS = {
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
INVARIANTS = {
    "BF.7": "HOLD",
    "LLM-MINEABLE-ELIGIBLE-V5": 14160,
    "REWARD_READY": 0,
    "RP0-MD": "HOLD",
    "activationAllowed": False,
    "baseActivation": False,
    "mineable_now": 0,
}
EXPECTED_ATTEMPTS = [
    {
        "attempt": 1,
        "completedAt": "2026-08-30T12:27:59Z",
        "failure": {
            "conclusion": "failure",
            "exactMessage": (
                "Failed to start transient service unit: Failed to set unit "
                "properties: Invalid argument"
            ),
            "stepName": "Run the v4 rehearsal from a verified root-owned HEAD anchor",
            "stepNumber": 9,
            "systemdTransientServiceStarted": False,
        },
        "freeRehearsalJobId": 99257028065,
        "headSha": "586a8dc16eb8d448badbea04bc04e797ac03901a",
        "jobs": {
            "compare": {"conclusion": "skipped", "jobId": 99257028788},
            "produce": {"conclusion": "skipped", "jobId": 99257028770},
            "productionAuthorityGuard": {
                "conclusion": "skipped",
                "jobId": 99257028725,
            },
        },
        "runId": 33311411461,
        "startedAt": "2026-08-30T12:24:51Z",
    },
    {
        "attempt": 2,
        "completedAt": "2026-08-30T13:24:25Z",
        "failure": {
            "conclusion": "failure",
            "exactMessage": (
                "native-shadow successor producer v4: FAIL: preloaded repository "
                "module is not backend-owned: __main__"
            ),
            "stepName": "Run the v4 rehearsal from a verified root-owned HEAD anchor",
            "stepNumber": 9,
            "systemdTransientServiceStarted": True,
        },
        "freeRehearsalJobId": 99263676278,
        "headSha": "97b82383bde815f3de723c920243ddeddaa0b072",
        "jobs": {
            "compare": {"conclusion": "skipped", "jobId": 99263677021},
            "produce": {"conclusion": "skipped", "jobId": 99263676856},
            "productionAuthorityGuard": {
                "conclusion": "skipped",
                "jobId": 99263676906,
            },
        },
        "runId": 33313895353,
        "startedAt": "2026-08-30T13:20:11Z",
    },
]
SCOPED_ZERO_EFFECTS = {
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


def read_exact_regular_file(path: pathlib.Path) -> bytes:
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or path.is_symlink():
        raise AssertionError(f"{path.relative_to(REPO)} is not a regular non-symlink")
    return path.read_bytes()


def sealed_section(path: pathlib.Path) -> str:
    text = path.read_text(encoding="utf-8")
    if text.count(SECTION_BEGIN) != 1 or text.count(SECTION_END) != 1:
        raise AssertionError(f"{path.name} does not contain one sealed section")
    before, remainder = text.split(SECTION_BEGIN, 1)
    section, after = remainder.split(SECTION_END, 1)
    if SECTION_END in before or SECTION_BEGIN in after:
        raise AssertionError(f"{path.name} has crossed sealed-section markers")
    return section


class LauncherV2SuccessorProducerRehearsalHardStopV2Tests(unittest.TestCase):
    def setUp(self) -> None:
        self.raw = read_exact_regular_file(RECORD_PATH)
        self.record = json.loads(self.raw.decode("utf-8"))

    def test_record_is_exact_canonical_json(self) -> None:
        self.assertEqual(self.raw, canonical_json(self.record))
        self.assertEqual(len(self.raw), RECORD_SIZE_BYTES)
        self.assertEqual(hashlib.sha256(self.raw).hexdigest(), RECORD_SHA256)

    def test_record_is_a_hard_stop_and_never_a_successful_r2(self) -> None:
        self.assertEqual(
            self.record["schema"],
            "boole.native-shadow.mac3.launcher-v2-successor-producer-rehearsal-"
            "hard-stop.arm64.v2",
        )
        self.assertEqual(self.record["status"], "HARD-STOP")
        self.assertEqual(self.record["outcome"], "TWO-FAILED-FREE-REHEARSALS")
        flattened = json.dumps(self.record, sort_keys=True)
        self.assertNotIn("PASS-NO-IMAGE-PRODUCED", flattened)
        self.assertNotIn("successfulR2Exists", self.record)
        self.assertNotIn("r2PayloadExists", self.record)
        self.assertEqual(self.record["successfulR2ResultsCreatedByTheseAttempts"], 0)
        self.assertEqual(self.record["r2PayloadsCreatedByTheseAttempts"], 0)

    def test_append_only_target_is_distinct_from_the_successful_r2_path(self) -> None:
        binding = self.record["appendOnlyTargetBinding"]
        self.assertEqual(
            binding["thisHardStopRecordPath"],
            RECORD_PATH.relative_to(REPO).as_posix(),
        )
        self.assertEqual(binding["successfulR2ResultPath"], SUCCESSFUL_R2_PATH)
        self.assertNotEqual(
            binding["thisHardStopRecordPath"], binding["successfulR2ResultPath"]
        )
        self.assertTrue(binding["pathsAreDistinct"])
        self.assertTrue(binding["successfulR2PathNotOccupiedByThisRecord"])

    def test_attempts_pin_the_two_official_run_and_job_histories(self) -> None:
        self.assertEqual(len(self.record["attempts"]), 2)
        for actual, expected in zip(self.record["attempts"], EXPECTED_ATTEMPTS):
            for key, value in expected.items():
                self.assertEqual(actual[key], value, (actual["attempt"], key))
            self.assertEqual(actual["event"], "workflow_dispatch")
            self.assertEqual(actual["mode"], "rehearsal")
            self.assertEqual(actual["runAttempt"], 1)
            self.assertEqual(actual["runConclusion"], "failure")
            self.assertEqual(actual["workflow"], "native-shadow-successor-produce-arm64-v4")
            self.assertEqual(
                actual["runUrl"],
                f"https://github.com/NotoriAndo/Boole/actions/runs/{actual['runId']}",
            )
            self.assertEqual(
                actual["freeRehearsalJobUrl"],
                f"{actual['runUrl']}/job/{actual['freeRehearsalJobId']}",
            )

    def test_only_steps_proved_by_the_run_metadata_are_called_successful(self) -> None:
        expected = [
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
            {"number": 5, "name": "Install Rust toolchain", "conclusion": "success"},
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
        ]
        for attempt in self.record["attempts"]:
            self.assertEqual(attempt["preparationSteps"], expected)
            self.assertEqual(attempt["failure"]["stepNumber"], 9)
            self.assertEqual(attempt["failure"]["conclusion"], "failure")
            self.assertEqual(
                attempt["postFailureSteps"],
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

    def test_zero_effects_are_narrowly_scoped_to_these_workflow_attempts(self) -> None:
        for attempt in self.record["attempts"]:
            self.assertEqual(attempt["scopedObservedEffects"], SCOPED_ZERO_EFFECTS)
            self.assertEqual(attempt["artifactApiTotalCount"], 0)
        self.assertEqual(
            self.record["effectScope"],
            "these two v4 workflow attempts and their rehearsal path only",
        )

    def test_record_explicitly_refuses_the_unsupported_broad_claims(self) -> None:
        claims = set(self.record["claimsNotMade"])
        self.assertEqual(
            claims,
            {
                "complete cleanup of runner-global transient state",
                "global absence of all filesystem or process side effects",
                "offline execution of dependency acquisition",
                "successful R2 or any rehearsal PASS",
                "production, image, boot, MAC.4, testnet, mining, reward, consensus or P2P authority",
            },
        )
        self.assertFalse(self.record["offlineClaim"])
        self.assertFalse(self.record["runnerGlobalTransientAbsenceClaim"])
        self.assertFalse(self.record["cleanupCompleteClaim"])

    def test_authority_and_project_invariants_remain_zero_or_held(self) -> None:
        self.assertEqual(self.record["authorisations"], AUTHORISATIONS)
        self.assertEqual(self.record["invariants"], INVARIANTS)
        self.assertFalse(self.record["activationAllowed"])
        self.assertFalse(self.record["bootableClaim"])

    def test_self_test_runs_this_gate_in_the_named_launcher_v2_lane(self) -> None:
        text = SELF_TEST_PATH.read_text(encoding="utf-8")
        gate = pathlib.Path(__file__).name
        self.assertEqual(text.count(gate), 1)
        lane = text.split("run_logged native-shadow-launcher-v2-contract", 1)[1]
        lane = lane.split("\nrun_logged ", 1)[0]
        self.assertIn(gate, lane)

    def test_three_authority_docs_seal_the_same_narrow_failure_history(self) -> None:
        required = (
            "sourceRunId=33311411461",
            "sourceRunId=33313895353",
            "artifactsUploadedByTheseAttempts=0",
            "successfulR2ResultsCreatedByTheseAttempts=0",
            "productionGuardJobs=skipped",
            "imageProductionClaim=false",
            "bootClaim=false",
            "R2 remains unsealed by these two attempts",
        )
        for path in (PLAN_PATH, SPEC_PATH, SHADOW_PATH):
            section = sealed_section(path)
            with self.subTest(path=path.name):
                for token in required:
                    self.assertEqual(section.count(token), 1, token)
                self.assertNotIn("PASS-NO-IMAGE-PRODUCED", section)
                self.assertNotIn("OFFLINE", section)
                self.assertNotIn("PRODUCTION-READY", section)

    def test_docs_smoke_permanently_pins_the_record_gate_and_section(self) -> None:
        smoke = DOCS_SMOKE_PATH.read_text(encoding="utf-8")
        section_token = SECTION_BEGIN.removeprefix("<!-- ").removesuffix(" -->")
        for token in (
            RECORD_PATH.relative_to(REPO).as_posix(),
            pathlib.Path(__file__).relative_to(REPO).as_posix(),
            RECORD_SHA256,
            "33311411461",
            "33313895353",
            "successfulR2ResultsCreatedByTheseAttempts",
            section_token,
        ):
            self.assertIn(token, smoke, token)


if __name__ == "__main__":
    unittest.main()
