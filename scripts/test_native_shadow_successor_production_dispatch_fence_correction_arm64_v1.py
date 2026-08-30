#!/usr/bin/env python3
"""Pin the authority-zero repository dispatch-fence correction."""

from __future__ import annotations

import hashlib
import json
import pathlib
import stat
import unittest


REPO = pathlib.Path(__file__).resolve().parents[1]
P2 = REPO / (
    "native/containment/native-shadow-mac3-launcher-v2-successor-"
    "production-generation-preregistration-arm64-v1.json"
)
CORRECTION = REPO / (
    "native/containment/native-shadow-mac3-launcher-v2-successor-"
    "production-dispatch-fence-correction-arm64-v1.json"
)

P2_IDENTITY = {
    "documentedSizeBytesBeforeCorrection": 8096,
    "path": P2.relative_to(REPO).as_posix(),
    "preservedByteUnchanged": True,
    "sha256": "4c801a52d4c6d47dbbc1c9a7657eb8bce215f9f258586b97064359caefd28a95",
    "sizeBytes": 8156,
    "sizeDocumentationCorrected": True,
}
CORRECTION_FIELD = "productionDispatchFenceCorrection"
MARKER = (
    "LAUNCHER-V2-SUCCESSOR-PRODUCTION-DISPATCH-FENCE-"
    "CORRECTION-ARM64-V1-FROZEN"
)
GATE = (
    "scripts/test_native_shadow_successor_production_dispatch_"
    "fence_correction_arm64_v1.py"
)
CORRECTION_SHA256 = "16f15bd7b9fcddeb02e104a3628d218817b047a3927fdfd77983ffaf0760910b"
CORRECTION_SIZE_BYTES = 7295

TOP_KEYS = {
    "authorisations",
    "claimFence",
    "corrects",
    "futureBindingRequirement",
    "hardStopConditions",
    "invariants",
    "runs",
    "schema",
    "status",
    "subject",
    "whatThisRecordDoesNotEstablish",
}

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
RUNS = {
    "bootsAllowed": 0,
    "bootsPerformed": 0,
    "freeRehearsalsAllowed": 0,
    "freeRehearsalsPerformed": 0,
    "imageProductionsAllowed": 0,
    "imageProductionsPerformed": 0,
    "productionDispatchClaimsAllowed": 0,
    "productionDispatchClaimsCreated": 0,
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

NON_SUBSTITUTES = [
    "runner-local attempt marker",
    "workflow artifact presence or absence",
    "runner cache presence or absence",
    "result-v6 path presence or absence",
    "workflow concurrency",
    "a human promise not to dispatch twice",
]

MESSAGE_FIELDS = [
    "a6Sha256",
    "attemptId",
    "githubRunId",
    "headSha",
    "schema",
    "workflowPath",
]

FUTURE_RECORDS = [
    {
        "label": "R2",
        "path": "native/containment/native-shadow-mac3-launcher-v2-successor-producer-rehearsal-result-arm64-v2.json",
        "schema": "boole.native-shadow.mac3.launcher-v2-successor-producer-rehearsal.arm64.v2",
    },
    {
        "label": "F6",
        "path": "native/containment/native-shadow-mac3-successor-producer-fingerprint-arm64-v6.json",
        "schema": "boole.native-shadow.mac3.successor-producer-fingerprint.arm64.v6",
    },
    {
        "label": "A6",
        "path": "native/containment/native-shadow-mac3-successor-production-authority-arm64-v6.json",
        "schema": "boole.native-shadow.mac3.successor-production-authority.arm64.v6",
    },
]


def digest(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


class ProductionDispatchFenceCorrectionTests(unittest.TestCase):
    def setUp(self) -> None:
        self.raw = CORRECTION.read_bytes()
        self.record = json.loads(self.raw)

    def test_record_is_canonical_and_self_contained(self) -> None:
        info = CORRECTION.lstat()
        self.assertTrue(stat.S_ISREG(info.st_mode))
        self.assertFalse(CORRECTION.is_symlink())
        self.assertEqual(info.st_size, CORRECTION_SIZE_BYTES)
        self.assertEqual(digest(CORRECTION), CORRECTION_SHA256)
        self.assertEqual(set(self.record), TOP_KEYS)
        self.assertEqual(
            self.record["schema"],
            "boole.native-shadow.mac3.launcher-v2-successor-production-dispatch-fence-correction.arm64.v1",
        )
        self.assertEqual(
            self.record["status"],
            "CORRECTED-BEFORE-R2-NO-PRODUCTION-DISPATCH-AUTHORITY",
        )
        canonical = (
            json.dumps(self.record, ensure_ascii=False, indent=2, sort_keys=True)
            + "\n"
        ).encode()
        self.assertEqual(self.raw, canonical)

    def test_p2_is_preserved_and_its_documented_size_is_corrected(self) -> None:
        self.assertEqual(self.record["corrects"]["p2Identity"], P2_IDENTITY)
        info = P2.lstat()
        self.assertTrue(stat.S_ISREG(info.st_mode))
        self.assertFalse(P2.is_symlink())
        self.assertEqual(info.st_size, 8156)
        self.assertEqual(digest(P2), P2_IDENTITY["sha256"])
        self.assertNotEqual(8096, info.st_size)
        self.assertEqual(
            self.record["corrects"]["scope"],
            "append-only correction; P2 remains byte-for-byte unchanged",
        )

    def test_one_run_declaration_is_not_the_dispatch_fence(self) -> None:
        defect = self.record["corrects"]["dispatchDefect"]
        self.assertEqual(
            defect["declaration"], "grant.workflowDispatchesAllowed=1"
        )
        self.assertIs(defect["declarationIsDurableGlobalClaim"], False)
        self.assertIs(defect["correctionRequiredBeforeR2F6A6"], True)
        self.assertEqual(defect["nonSubstitutes"], NON_SUBSTITUTES)

    def test_only_the_guard_may_write_repository_contents(self) -> None:
        permissions = self.record["claimFence"]["jobPermissions"]
        self.assertEqual(
            permissions,
            {
                "allOtherJobs": {"contents": "read"},
                "productionAuthorityGuard": {"contents": "write"},
                "soleWriteJob": "production-authority-guard",
                "workflowDefault": {"contents": "read"},
            },
        )

    def test_claim_is_an_attempt_specific_annotated_git_tag(self) -> None:
        claim = self.record["claimFence"]["repositoryClaim"]
        self.assertEqual(
            claim["refTemplate"],
            "refs/tags/boole-native-shadow-mac3-successor-production-a6-{attemptId}",
        )
        self.assertEqual(claim["refObject"], "annotated Git tag")
        self.assertEqual(
            claim["attemptIdPattern"], "^[a-z0-9][a-z0-9._-]{0,127}$"
        )
        self.assertEqual(claim["requiredGitHubRunAttempt"], 1)
        self.assertEqual(
            claim["messageCanonicalisation"],
            "UTF-8 canonical JSON with sorted keys, compact separators and no trailing newline",
        )
        self.assertEqual(claim["messageFields"], MESSAGE_FIELDS)
        self.assertEqual(
            claim["messageSchema"],
            "boole.native-shadow.mac3.successor-production-dispatch-claim.arm64.v1",
        )

    def test_ref_create_is_atomic_and_is_the_consumption_point(self) -> None:
        creation = self.record["claimFence"]["atomicCreation"]
        self.assertEqual(
            creation["orderedSteps"],
            [
                "require github.run_attempt == 1",
                "verify live A6 digest, attemptId, workflow path and head SHA",
                "create annotated tag object targeting the exact head SHA",
                "atomically create the fixed refs/tags claim without force",
                "re-read the ref and annotated tag message",
            ],
        )
        self.assertEqual(
            creation["claimCreationMoment"],
            "successful atomic creation of the fixed refs/tags claim",
        )
        self.assertEqual(
            creation["runConsumptionMoment"], creation["claimCreationMoment"]
        )
        self.assertTrue(creation["existingRefIsHardStop"])
        self.assertTrue(creation["deleteForbidden"])
        self.assertTrue(creation["forceUpdateForbidden"])
        self.assertTrue(creation["reuseForbidden"])

    def test_each_replica_rechecks_the_same_claim_before_all_effects(self) -> None:
        check = self.record["claimFence"]["replicaReverification"]
        self.assertTrue(check["requiredForEveryReplica"])
        self.assertEqual(check["mustPrecede"], ["dependency acquisition", "scratch creation", "attempt marker", "assembly", "image effects"])
        self.assertEqual(
            check["orderedChecks"],
            [
                "resolve the exact claim ref",
                "require an annotated tag object and exact target head SHA",
                "parse canonical tag-message JSON and reject extra or missing keys",
                "match A6 digest, attemptId, github run ID, workflow path and head SHA",
                "recompute live A6 digest from the checked-out head",
            ],
        )

    def test_r2_f6_and_a6_must_directly_bind_this_correction(self) -> None:
        binding = self.record["futureBindingRequirement"]
        self.assertEqual(binding["fieldName"], CORRECTION_FIELD)
        self.assertEqual(binding["fieldKeys"], ["path", "sha256", "sizeBytes"])
        self.assertTrue(binding["exactKeysOnly"])
        self.assertTrue(binding["directBindingRequired"])
        self.assertFalse(binding["transitiveBindingAccepted"])
        self.assertEqual(binding["correctionPath"], CORRECTION.relative_to(REPO).as_posix())
        self.assertEqual(binding["requiredRecords"], FUTURE_RECORDS)
        identity = {
            "path": CORRECTION.relative_to(REPO).as_posix(),
            "sha256": CORRECTION_SHA256,
            "sizeBytes": CORRECTION_SIZE_BYTES,
        }
        for row in FUTURE_RECORDS:
            path = REPO / row["path"]
            if not path.exists():
                continue
            future = json.loads(path.read_text(encoding="utf-8"))
            self.assertEqual(future["schema"], row["schema"], row["label"])
            self.assertEqual(future[CORRECTION_FIELD], identity, row["label"])

    def test_record_grants_nothing_and_reports_no_run(self) -> None:
        self.assertEqual(self.record["authorisations"], AUTHORISATIONS)
        self.assertEqual(self.record["runs"], RUNS)
        self.assertEqual(self.record["invariants"], INVARIANTS)
        self.assertEqual(
            self.record["claimFence"]["effectsByThisRecord"],
            {
                "annotatedTagsCreated": 0,
                "attemptMarkersCreated": 0,
                "dependenciesAcquired": 0,
                "imagesCreated": 0,
                "productionOutputsCreated": 0,
            },
        )
        for value in self.record["authorisations"].values():
            if type(value) is bool:
                self.assertFalse(value)
            else:
                self.assertEqual(value, 0)
        for value in self.record["runs"].values():
            self.assertEqual(value, 0)

    def test_record_itself_does_not_establish_future_records_or_effects(self) -> None:
        claims = self.record["whatThisRecordDoesNotEstablish"]
        for phrase in (
            "producer generation v4",
            "R2",
            "F6",
            "A6",
            "result-v6",
            "image production",
            "guest boot",
        ):
            self.assertTrue(any(phrase in row for row in claims), phrase)

    def test_hard_stops_keep_the_fence_machine_enforced(self) -> None:
        hard_stops = self.record["hardStopConditions"]
        required = (
            "github.run_attempt",
            "existing claim ref",
            "contents:write",
            "annotated tag message",
            "replica",
            "delete, update or reuse",
            CORRECTION_FIELD,
        )
        for phrase in required:
            self.assertTrue(any(phrase in row for row in hard_stops), phrase)

    def test_permanent_gates_and_three_append_only_docs_are_active(self) -> None:
        self_test = (REPO / "scripts/self-test.sh").read_text(encoding="utf-8")
        docs_smoke = (REPO / "scripts/docs-smoke.sh").read_text(encoding="utf-8")
        self.assertEqual(self_test.count(GATE), 1)
        self.assertIn(f'require_file "{GATE}"', docs_smoke)
        self.assertIn(MARKER, docs_smoke)
        self.assertIn(P2_IDENTITY["sha256"], docs_smoke)
        self.assertIn('"sizeBytes": 8156', docs_smoke)
        for relative in (
            "docs/mac-first-hidden-linux-execution-plan-v1.md",
            "docs/node-native-shadow-binding-containment-implementation-spec-v1.md",
            "docs/native-submission-shadow-verification-v1.md",
        ):
            text = (REPO / relative).read_text(encoding="utf-8")
            self.assertEqual(text.count(MARKER), 1, relative)
            self.assertIn("8,156", text)
            self.assertIn("8,096", text)


if __name__ == "__main__":
    unittest.main()
