#!/usr/bin/env python3
"""Gate the authority-zero successor to the v4 branch-ref dispatch defect."""

import hashlib
import json
import os
import pathlib
import stat
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
CORRECTION = ROOT / (
    "native/containment/native-shadow-mac3-launcher-v2-successor-main-branch-"
    "dispatch-fence-correction-arm64-v1.json"
)
CORRECTION_SHA256 = "63f5bdf0ffaac00ac1af3972ed69051da9fcbe8a06b90ae3c9f70756bbfe144b"
CORRECTION_SIZE_BYTES = 13335
V4_WRAPPER = ROOT / "scripts/native-shadow-successor-produce-arm64-v4.sh"
V4_WORKFLOW = ROOT / ".github/workflows/native-shadow-successor-produce-arm64-v4.yml"


def load_record():
    return json.loads(CORRECTION.read_text(encoding="utf-8"))


def live_identity(relative):
    path = ROOT / relative
    info = path.lstat()
    if path.is_symlink() or not stat.S_ISREG(info.st_mode):
        raise AssertionError(f"binding is not a regular file: {relative}")
    raw = path.read_bytes()
    return {
        "path": relative,
        "sha256": hashlib.sha256(raw).hexdigest(),
        "sizeBytes": len(raw),
    }


class SuccessorMainBranchDispatchFenceCorrectionTests(unittest.TestCase):
    def test_correction_record_exists_before_any_successor_authority(self):
        self.assertTrue(CORRECTION.is_file())

    def test_record_is_canonical_authority_zero_correction(self):
        raw = CORRECTION.read_bytes()
        record = json.loads(raw)
        self.assertEqual(len(raw), CORRECTION_SIZE_BYTES)
        self.assertEqual(hashlib.sha256(raw).hexdigest(), CORRECTION_SHA256)
        self.assertEqual(
            raw,
            (json.dumps(record, indent=2, sort_keys=True) + "\n").encode(),
        )
        self.assertEqual(
            record["schema"],
            "boole.native-shadow.mac3.launcher-v2-successor-main-branch-"
            "dispatch-fence-correction.arm64.v1",
        )
        self.assertEqual(
            record["status"],
            "A6-WITHHELD-PENDING-MAIN-ONLY-SUCCESSOR-GENERATION",
        )
        self.assertEqual(
            set(record),
            {
                "authorisations",
                "boundaries",
                "correction",
                "dag",
                "externalTagRetentionBoundary",
                "futureBindingRequirement",
                "generationLabel",
                "hardStopConditions",
                "historicalGeneration",
                "invariants",
                "predecessors",
                "runs",
                "schema",
                "status",
                "subject",
                "successorClaimNamespace",
                "successorGeneration",
                "unusedReservations",
                "whatThisRecordDoesNotEstablish",
            },
        )
        self.assertEqual(
            record["authorisations"],
            {
                "bootAuthorised": False,
                "consensusActivated": False,
                "imageProductionAuthorised": False,
                "imageProductionRunsAllowed": 0,
                "mac4Started": False,
                "miningActivated": False,
                "p2pActivated": False,
                "rewardActivated": False,
                "testnetStarted": False,
            },
        )
        self.assertIs(
            type(record["authorisations"]["imageProductionRunsAllowed"]), int
        )
        for key, value in record["authorisations"].items():
            if key != "imageProductionRunsAllowed":
                self.assertIs(type(value), bool, key)
                self.assertIs(value, False, key)
        self.assertEqual(
            record["runs"],
            {
                "bootsAllowed": 0,
                "bootsPerformed": 0,
                "freeRehearsalsAllowed": 0,
                "freeRehearsalsPerformed": 0,
                "imageProductionsAllowed": 0,
                "imageProductionsPerformed": 0,
                "productionDispatchClaimsAllowed": 0,
                "productionDispatchClaimsCreated": 0,
            },
        )
        self.assertTrue(
            all(type(value) is int and value == 0 for value in record["runs"].values())
        )
        self.assertEqual(
            record["boundaries"],
            {
                "activationAllowed": False,
                "bootableClaim": False,
                "servingClaim": False,
            },
        )
        self.assertTrue(
            all(
                type(value) is bool and value is False
                for value in record["boundaries"].values()
            )
        )

    def test_record_binds_the_live_historical_generation_and_predecessors(self):
        record = load_record()
        self.assertEqual(
            record["historicalGeneration"]["files"],
            [
                live_identity(row["path"])
                for row in record["historicalGeneration"]["files"]
            ],
        )
        self.assertEqual(
            record["predecessors"],
            [live_identity(row["path"]) for row in record["predecessors"]],
        )

    def test_record_names_the_observed_prefix_only_defect_without_rewriting_v4(self):
        record = load_record()
        wrapper = V4_WRAPPER.read_text(encoding="utf-8")
        workflow = V4_WORKFLOW.read_text(encoding="utf-8")
        self.assertIn('[[ $github_workflow_ref == "$workflow_prefix"* ]]', wrapper)
        self.assertNotIn(
            '[[ $github_workflow_ref == "NotoriAndo/Boole/.github/workflows/'
            'native-shadow-successor-produce-arm64-v4.yml@refs/heads/main" ]]',
            wrapper,
        )
        self.assertNotIn("GITHUB_REF_VALUE: ${{ github.ref }}", workflow)
        self.assertEqual(
            record["historicalGeneration"]["observedDefect"],
            {
                "acceptedWorkflowRefRule": "prefix-only plus non-empty suffix",
                "eventNameRecheckedByGuard": False,
                "exactMainDispatchRefCheckPresent": False,
                "exactMainWorkflowRefCheckPresent": False,
                "featureBranchCanReachClaimCreation": True,
                "pullRequestRefCanReachCurrentWorkflowDispatch": False,
                "tagRefCanReachClaimCreation": True,
                "workflowTriggerSetIsWorkflowDispatchOnly": True,
                "location": (
                    "scripts/native-shadow-successor-produce-arm64-v4.sh:"
                    "prepare_dispatch_context"
                ),
            },
        )

    def test_successor_contract_requires_exact_main_and_rejects_lookalikes(self):
        correction = load_record()["correction"]
        exact = (
            "NotoriAndo/Boole/.github/workflows/"
            "native-shadow-successor-produce-arm64-v5.yml@refs/heads/main"
        )
        self.assertEqual(correction["exactDispatchRef"], "refs/heads/main")
        self.assertEqual(correction["exactEventName"], "workflow_dispatch")
        self.assertEqual(correction["exactWorkflowRef"], exact)
        self.assertEqual(
            correction["mustRejectEventNames"],
            [
                "pull_request",
                "pull_request_target",
                "push",
                "schedule",
                "workflow_call",
            ],
        )
        self.assertEqual(len(correction["mustRejectWorkflowRefs"]), 5)
        self.assertNotIn(exact, correction["mustRejectWorkflowRefs"])
        self.assertEqual(
            {value.rsplit("@", 1)[-1] for value in correction["mustRejectWorkflowRefs"]},
            {
                "refs/heads/feature-x",
                "refs/pull/1/merge",
                "refs/tags/release",
                "refs/heads/main-old",
                "",
            },
        )
        self.assertIn(
            "the canonical claim message records the exact event name, dispatch ref, "
            "workflow ref and A7 digest under authoritySha256",
            correction["requiredChecks"],
        )
        self.assertIn(
            "every claim consumer rechecks the same live event name, ref, workflow ref, "
            "head and A7 digest before effects",
            correction["requiredChecks"],
        )

    def test_successor_claim_uses_only_the_fresh_a7_namespace(self):
        namespace = load_record()["successorClaimNamespace"]
        self.assertEqual(
            namespace,
            {
                "authority": {
                    "digestField": "authoritySha256",
                    "label": "A7",
                    "path": (
                        "native/containment/native-shadow-mac3-successor-production-"
                        "authority-arm64-v7.json"
                    ),
                    "schema": (
                        "boole.native-shadow.mac3.successor-production-authority."
                        "arm64.v7"
                    ),
                    "status": "ONE-NAMED-PRODUCTION-RUN-AUTHORISED-NOT-RUN",
                },
                "authorityContextFields": [
                    "dispatchRef",
                    "eventName",
                    "workflowPath",
                    "workflowRef",
                ],
                "claim": {
                    "messageCanonicalisation": (
                        "UTF-8 canonical JSON with sorted keys, compact separators and "
                        "no trailing newline"
                    ),
                    "messageFields": [
                        "authoritySha256",
                        "attemptId",
                        "dispatchRef",
                        "eventName",
                        "githubRunAttempt",
                        "githubRunId",
                        "headSha",
                        "schema",
                        "workflowPath",
                        "workflowRef",
                    ],
                    "refTemplate": (
                        "refs/tags/boole-native-shadow-mac3-successor-production-a7-"
                        "{attemptId}"
                    ),
                    "schema": (
                        "boole.native-shadow.mac3.successor-production-dispatch-claim."
                        "arm64.v2"
                    ),
                },
                "forbiddenLegacyTokens": [
                    "native/containment/native-shadow-mac3-successor-production-authority-arm64-v6.json",
                    "native/containment/native-shadow-mac3-successor-image-production-result-arm64-v6.json",
                    "boole.native-shadow.mac3.successor-production-authority.arm64.v6",
                    "refs/tags/boole-native-shadow-mac3-successor-production-a6-",
                    "boole.native-shadow.mac3.successor-production-dispatch-claim.arm64.v1",
                    "a6Sha256",
                    "--head-a6-sha256",
                    "A6_PATH",
                    "A6_SCHEMA",
                    "head_a6_sha256",
                    'identities["A6"]',
                ],
                "generation": {
                    "forbiddenLegacyNamespaceTokens": [
                        "native-shadow-successor-v4-replica-",
                        "/var/lib/boole/native-shadow-successor-v4",
                        "boole-nsv4-",
                        ".arm64.v4",
                    ],
                    "internalSchemaSuffix": ".arm64.v5",
                    "recoveryStemPrefix": "boole-nsv5-",
                    "replicaArtifactPrefix": "native-shadow-successor-v5-replica-",
                    "rootPrefix": "/var/lib/boole/native-shadow-successor-v5",
                },
                "result": {
                    "label": "RESULT-V7",
                    "path": (
                        "native/containment/native-shadow-mac3-successor-image-"
                        "production-result-arm64-v7.json"
                    ),
                    "schema": (
                        "boole.native-shadow.mac3.successor-image-production-result."
                        "arm64.v7"
                    ),
                },
            },
        )
        hard_stops = load_record()["hardStopConditions"]
        self.assertIn(
            "the successor reuses an A6 authority path, schema, tag prefix, claim "
            "schema, digest field, CLI or identity symbol",
            hard_stops,
        )
        self.assertIn(
            "a v5 canonical claim omits eventName, dispatchRef, workflowRef, "
            "githubRunAttempt or authoritySha256",
            hard_stops,
        )

    def test_unused_v6_authority_and_result_reservations_are_withdrawn(self):
        record = load_record()
        expected = {
            "native/containment/native-shadow-mac3-successor-production-authority-arm64-v6.json",
            "native/containment/native-shadow-mac3-successor-image-production-result-arm64-v6.json",
        }
        self.assertEqual({row["path"] for row in record["unusedReservations"]}, expected)
        for row in record["unusedReservations"]:
            self.assertIs(row["authorityEverGranted"], False)
            self.assertIs(row["requiredAbsent"], True)
            self.assertIs(row["reuseForbidden"], True)
            self.assertFalse(os.path.lexists(ROOT / row["path"]))

    def test_external_tag_deletion_is_not_overclaimed(self):
        boundary = load_record()["externalTagRetentionBoundary"]
        self.assertIs(
            boundary["administratorDeletionIsPreventedByObservedServerRuleset"],
            False,
        )
        self.assertIs(boundary["serverRulesetEvidenceRecorded"], False)
        self.assertIs(boundary["irrevocablyUndeletableClaim"], False)
        self.assertIs(boundary["hardStopOnMissingPreviouslyObservedClaim"], True)

    def test_successor_namespace_is_append_only_and_keeps_all_authority_zero(self):
        record = load_record()
        self.assertEqual(record["generationLabel"], "P4")
        self.assertEqual(
            record["successorGeneration"]["futureFiles"],
            [
                "scripts/native_shadow_successor_produce_phase_arm64_v5.py",
                "scripts/test_native_shadow_successor_produce_phase_arm64_v5.py",
                "scripts/native-shadow-successor-produce-arm64-v5.sh",
                ".github/workflows/native-shadow-successor-produce-arm64-v5.yml",
                "scripts/test_native_shadow_successor_produce_workflow_arm64_v5.py",
            ],
        )
        self.assertIs(record["successorGeneration"]["implementedByThisRecord"], False)
        self.assertIs(
            record["successorGeneration"]["requiresFreshAuthorityZeroRehearsal"],
            True,
        )
        self.assertEqual(
            record["dag"]["futureOrder"],
            ["P4", "producer-generation-v5", "R3", "F7", "A7", "RESULT-V7"],
        )
        self.assertEqual(
            record["dag"]["edges"],
            [
                {
                    "binder": "P4",
                    "binds": [
                        "P2",
                        "P3",
                        "R1",
                        "F5",
                        "R2",
                        "F6",
                        "producer-generation-v4",
                    ],
                },
                {
                    "binder": "R3",
                    "binds": ["P4", "producer-generation-v5"],
                },
                {
                    "binder": "F7",
                    "binds": ["P4", "R3", "producer-generation-v5"],
                },
                {"binder": "A7", "binds": ["P4", "R3", "F7"]},
                {
                    "binder": "RESULT-V7",
                    "binds": ["P4", "R3", "F7", "A7"],
                },
            ],
        )
        self.assertEqual(
            record["futureBindingRequirement"],
            {
                "correctionPath": CORRECTION.relative_to(ROOT).as_posix(),
                "directBindingRequired": True,
                "exactKeysOnly": True,
                "fieldKeys": ["path", "sha256", "sizeBytes"],
                "fieldName": "mainBranchDispatchFenceCorrection",
                "requiredRecords": ["R3", "F7", "A7", "RESULT-V7"],
                "transitiveBindingAccepted": False,
            },
        )
        self.assertIs(record["dag"]["cycleAllowed"], False)
        self.assertIs(record["dag"]["reverseDigestEdgesAllowed"], False)

    def test_gate_is_registered_in_the_full_self_test(self):
        self.assertIn(
            "scripts/test_native_shadow_successor_main_branch_dispatch_fence_"
            "correction_arm64_v1.py",
            (ROOT / "scripts/self-test.sh").read_text(encoding="utf-8"),
        )

    def test_all_three_authority_documents_record_the_corrected_cursor(self):
        marker = (
            "LAUNCHER-V2-SUCCESSOR-MAIN-BRANCH-DISPATCH-FENCE-CORRECTION-"
            "ARM64-V1-SEALED:BEGIN"
        )
        cursor = "A6-V6 AND RESULT-V6 WITHDRAWN UNUSED"
        for relative in (
            "docs/mac-first-hidden-linux-execution-plan-v1.md",
            "docs/node-native-shadow-binding-containment-implementation-spec-v1.md",
            "docs/native-submission-shadow-verification-v1.md",
        ):
            text = (ROOT / relative).read_text(encoding="utf-8")
            self.assertEqual(text.count(marker), 1, relative)
            self.assertIn("A6-V6", text, relative)
        self.assertIn(
            cursor,
            (ROOT / "docs/mac-first-hidden-linux-execution-plan-v1.md").read_text(
                encoding="utf-8"
            ),
        )

    def test_docs_smoke_pins_the_correction_and_gate(self):
        smoke = (ROOT / "scripts/docs-smoke.sh").read_text(encoding="utf-8")
        self.assertIn(CORRECTION.relative_to(ROOT).as_posix(), smoke)
        self.assertIn(pathlib.Path(__file__).relative_to(ROOT).as_posix(), smoke)
        self.assertIn(
            CORRECTION_SHA256,
            smoke,
        )


if __name__ == "__main__":
    unittest.main()
