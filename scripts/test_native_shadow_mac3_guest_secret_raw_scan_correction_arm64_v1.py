"""Pins the append-only correction to the historical guest-secret raw scan.

The historical scanner and its result remain evidence of a read-only whole-image
scan.  They are not a logical-path parser: ext4 stores each directory component
as a separate entry, so a logical path need not occur as one contiguous byte
string in the image.  This gate preserves the historical NOT SETTLED result and
requires the successor to keep the condition closed until paths and contents are
reconciled independently.
"""

from __future__ import annotations

import hashlib
import json
import pathlib
import unittest


REPO = pathlib.Path(__file__).resolve().parents[1]
CONTAINMENT = REPO / "native/containment"
CORRECTION = (
    CONTAINMENT
    / "native-shadow-mac3-guest-secret-absence-raw-scan-correction-arm64-v1.json"
)


def sha256(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


class GuestSecretRawScanCorrectionArm64V1Tests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.record = json.loads(CORRECTION.read_text(encoding="utf-8"))

    def test_record_identity_and_scope_are_fail_closed(self) -> None:
        self.assertEqual(
            self.record["schema"],
            "boole.native-shadow.mac3.guest-secret-absence-raw-scan-correction.arm64.v1",
        )
        self.assertEqual(
            self.record["status"],
            "RAW-SCAN-PATH-AND-ORIGIN-INFERENCES-FALSIFIED-CONDITION-NOT-SETTLED",
        )
        self.assertTrue(self.record["appendOnly"])
        self.assertFalse(self.record["editsAnyEarlierRecord"])
        self.assertEqual(
            self.record["condition"],
            "no-host-wallet-model-key-or-node-secret-in-the-guest",
        )
        self.assertEqual(
            self.record["historicalFactsRetained"],
            {
                "imageSha256BeforeAndAfter": "51410d8113c28d6cd28c7b6c7578076226d5e19b6629649199af7b7f86540a1c",
                "imageSizeBytes": 2_035_625_984,
                "wholeImageWasRead": True,
                "openedReadOnly": True,
                "hostIdentityRawHits": 0,
                "genericSecretShapeRawHits": 135,
                "historicalVerdictNoEntryFound": False,
                "historicalVerdict": "NOT-SETTLED",
            },
        )
        self.assertEqual(
            self.record["claimBoundary"],
            {
                "bootAttemptId": "MAC3-CLOSED-LOCAL-BOOT-ARM64-ATTEMPT-3",
                "conditionSettled": False,
                "bootAuthorisationGrantedByThisRecord": False,
                "bootAttemptsUsedByThisRecord": 0,
                "targetImageProducedOrModifiedByThisRecord": False,
                "servingClaim": False,
                "publicMiningClaim": False,
                "activationAllowed": False,
            },
        )

    def test_historical_inputs_are_byte_preserved(self) -> None:
        expected = {
            "scripts/native_shadow_mac3_guest_secret_absence_scan_arm64_v1.py": (
                "c9dcd6e934231d641673b4a8a7deeb681135847f9c0ed500b221168873132279"
            ),
            "scripts/test_native_shadow_mac3_guest_secret_absence_scan_arm64_v1.py": (
                "d02da2d4839e84f896491309520285748072694148bbe5b48c30db644fb6a6bc"
            ),
            "native/containment/native-shadow-mac3-guest-secret-absence-scan-arm64-v1.json": (
                "feeb6264ff062af9813d6a05c44a2bca9ddc9d4f9ae96d33a51bd595c9fd8e2f"
            ),
            "native/containment/native-shadow-mac3-guest-evidence-channel-design-arm64-v1.json": (
                "70c13a8102104b3744006acdd4fac15539d81eecfcec2bd9eea2f0f93b14daae"
            ),
        }
        cited = {
            row["path"]: row["sha256"]
            for row in self.record["historicalRecordsPreserved"]
        }
        self.assertEqual(cited, expected)
        for relative, digest in expected.items():
            self.assertEqual(sha256(REPO / relative), digest, relative)

    def test_the_raw_superset_inference_is_falsified(self) -> None:
        correction = self.record["correction"]
        self.assertEqual(correction["status"], "FALSIFIED-AS-LOGICAL-PATH-PROOF")
        self.assertFalse(
            correction[
                "joinedMultiComponentRawNeedleSearchIsASupersetOfLogicalPathEnumeration"
            ]
        )
        self.assertEqual(
            correction["forbiddenInference"],
            "raw-zero-for-a-joined-multi-component-path-needle-implies-logical-path-absence",
        )
        self.assertFalse(
            correction["rawHostIdentityMarkerHitProvesHostOriginOrLeak"]
        )
        self.assertEqual(
            correction["secondForbiddenInference"],
            "raw-hit-on-a-host-identity-tier-needle-proves-the-bytes-came-from-the-host-or-are-a-secret-leak",
        )

        # A directory graph, not just two unrelated byte strings: traversing the
        # root entry reaches inode 11, whose child entry reaches inode 12.
        # Neither ext4 directory entry has to store the joined path spelling.
        counterexample = self.record["counterexample"]
        self.assertEqual(counterexample["kind"], "SCHEMATIC-EXT4-DIRECTORY-GRAPH")
        graph = {
            (row["parentInode"], row["entryName"]): row["targetInode"]
            for row in counterexample["directoryGraph"]
        }
        current = counterexample["rootInode"]
        for component in (".boole", "keys"):
            current = graph[(current, component)]
        self.assertEqual(current, 12)
        directory_entry_payloads = b"\x06.boole\x00" + b"\x04keys\x00"
        self.assertNotIn(b".boole/keys", directory_entry_payloads)
        self.assertEqual(counterexample["logicalPath"], "/.boole/keys")
        self.assertFalse(counterexample["joinedPathNeedleMustOccurContiguously"])

    def test_historical_result_stays_not_settled(self) -> None:
        historical = json.loads(
            (
                CONTAINMENT
                / "native-shadow-mac3-guest-secret-absence-scan-arm64-v1.json"
            ).read_text(encoding="utf-8")
        )
        self.assertFalse(historical["verdict"]["noEntryFound"])
        self.assertEqual(historical["bytesRead"], 2_035_625_984)
        self.assertTrue(historical["wholeFileRead"])
        self.assertTrue(historical["openedReadOnly"])
        self.assertEqual(
            historical["sha256Before"],
            self.record["historicalFactsRetained"]["imageSha256BeforeAndAfter"],
        )
        self.assertEqual(historical["sha256After"], historical["sha256Before"])
        self.assertEqual(historical["hitCount"], 135)
        self.assertEqual(len(historical["hits"]), 135)
        self.assertEqual(
            {row["tier"] for row in historical["hits"]},
            {"secret-shape"},
        )
        self.assertEqual(
            historical["verdict"]["hitsByTier"],
            {"host-identity": 0, "secret-shape": 135},
        )
        boundary = self.record["claimBoundary"]
        self.assertFalse(boundary["conditionSettled"])
        self.assertFalse(boundary["bootAuthorisationGrantedByThisRecord"])
        self.assertEqual(boundary["bootAttemptsUsedByThisRecord"], 0)
        self.assertFalse(boundary["targetImageProducedOrModifiedByThisRecord"])
        self.assertFalse(boundary["activationAllowed"])

    def test_authority_bindings_are_byte_preserved(self) -> None:
        expected = {
            "native/containment/native-shadow-mac3-guest-runtime-contract-arm64-v1.json": (
                "27a6c16eabd0162331e635656695b2f4adf3027d39f888bcddd3268e1d78cd9f"
            ),
            "native/containment/native-shadow-mac3-closed-local-boot-qualification-arm64-v3.json": (
                "74b9507932b4eda97c89753f642bac579593b034b3e9eff24bb5b056c09079a6"
            ),
            "native/containment/native-shadow-mac3-closed-local-boot-execution-contract-arm64-v3.json": (
                "86683f56f78549152b9e2c061403d34649951f675f976a2f9cf8c8ffac331c75"
            ),
            "scripts/native_shadow_mac3_closed_local_boot_arm64_v3.py": (
                "bc3f5fbb36b44f99a4527a0796020d30f31188ccc28fceb10d7155fcd393b4a5"
            ),
        }
        cited = {row["path"]: row["sha256"] for row in self.record["authorityBindings"]}
        self.assertEqual(cited, expected)
        for relative, digest in expected.items():
            self.assertEqual(sha256(REPO / relative), digest, relative)

        attempt_ids = []
        for relative in expected:
            if relative.endswith(".json"):
                record = json.loads((REPO / relative).read_text(encoding="utf-8"))
                if "attemptId" in record:
                    attempt_ids.append(record["attemptId"])
        design = json.loads(
            (
                CONTAINMENT
                / "native-shadow-mac3-guest-evidence-channel-design-arm64-v1.json"
            ).read_text(encoding="utf-8")
        )
        attempt_ids.append(design["attemptId"])
        runner_source = (
            REPO / "scripts/native_shadow_mac3_closed_local_boot_arm64_v3.py"
        ).read_text(encoding="utf-8")
        self.assertEqual(
            attempt_ids,
            ["MAC3-CLOSED-LOCAL-BOOT-ARM64-ATTEMPT-3"] * 3,
        )
        self.assertIn("MAC3-CLOSED-LOCAL-BOOT-ARM64-ATTEMPT-3", runner_source)

    def test_the_target_image_and_its_preservation_record_are_bound(self) -> None:
        target = self.record["targetImage"]
        self.assertEqual(target["role"], "guest-root-disk")
        self.assertEqual(target["sizeBytes"], 2_035_625_984)
        self.assertEqual(
            target["sha256"],
            "51410d8113c28d6cd28c7b6c7578076226d5e19b6629649199af7b7f86540a1c",
        )
        expected = {
            "native/containment/native-shadow-mac3-successor-image-production-result-arm64-v4.json": (
                "0faddb098503bbf17bf94ec36148e6ccf1af8fa1335ba0e5e9c79cd9d573b7dd"
            ),
            "native/containment/native-shadow-mac3-successor-image-preservation-arm64-v4.json": (
                "2ff7a3a30513092495a2d8b67555b4e974ef75af47de08acfe8c049063549126"
            ),
        }
        cited = {row["path"]: row["sha256"] for row in target["provenance"]}
        self.assertEqual(cited, expected)
        for relative, digest in expected.items():
            self.assertEqual(sha256(REPO / relative), digest, relative)

        production = json.loads(
            (CONTAINMENT / "native-shadow-mac3-successor-image-production-result-arm64-v4.json").read_text(
                encoding="utf-8"
            )
        )
        preservation = json.loads(
            (CONTAINMENT / "native-shadow-mac3-successor-image-preservation-arm64-v4.json").read_text(
                encoding="utf-8"
            )
        )
        self.assertEqual(production["outputs"]["guest-root-disk"], target["sha256"])
        self.assertEqual(
            production["attemptId"],
            preservation["attemptId"],
        )
        self.assertEqual(production["runId"], preservation["source"]["runId"])
        self.assertEqual(
            preservation["resultDocument"]["path"],
            "native/containment/native-shadow-mac3-successor-image-production-result-arm64-v4.json",
        )
        self.assertEqual(
            preservation["resultDocument"]["sha256"],
            expected[
                "native/containment/native-shadow-mac3-successor-image-production-result-arm64-v4.json"
            ],
        )
        preserved_image = next(
            row for row in preservation["images"] if row["name"] == "guest-root-disk"
        )
        self.assertEqual(preserved_image["sha256"], target["sha256"])
        self.assertEqual(preserved_image["bytes"], target["sizeBytes"])
        preserved_replicas = [
            row
            for row in preservation["preservedFiles"]
            if row["path"].endswith("/guest-root-disk")
        ]
        self.assertEqual(len(preserved_replicas), 2)
        self.assertTrue(
            all(
                row["sha256"] == target["sha256"]
                and row["bytes"] == target["sizeBytes"]
                for row in preserved_replicas
            )
        )

        qualification = json.loads(
            (
                CONTAINMENT
                / "native-shadow-mac3-closed-local-boot-qualification-arm64-v3.json"
            ).read_text(encoding="utf-8")
        )
        qualified_root = next(
            row
            for row in qualification["subject"]["images"]
            if row["name"] == "guest-root-disk"
        )
        self.assertEqual(qualification["subject"]["productionRunId"], production["runId"])
        self.assertEqual(qualified_root["sha256"], target["sha256"])
        self.assertEqual(qualified_root["bytes"], target["sizeBytes"])
        self.assertEqual(
            qualification["subject"]["preservationRecord"]["sha256"],
            expected[
                "native/containment/native-shadow-mac3-successor-image-preservation-arm64-v4.json"
            ],
        )

    def test_superseded_assertions_are_named_not_erased(self) -> None:
        rows = self.record["supersededAssertions"]
        self.assertEqual(
            [row["path"] for row in rows],
            [
                "scripts/native_shadow_mac3_guest_secret_absence_scan_arm64_v1.py",
                "scripts/test_native_shadow_mac3_guest_secret_absence_scan_arm64_v1.py",
                "native/containment/native-shadow-mac3-guest-secret-absence-scan-arm64-v1.json",
                "native/containment/native-shadow-mac3-guest-evidence-channel-design-arm64-v1.json",
                "docs/mac-first-hidden-linux-execution-plan-v1.md",
            ],
        )
        self.assertEqual(
            rows[0]["symbols"],
            [
                "module-docstring-byte-search-superset-claim",
                "WHY_A_BYTE_SEARCH_SETTLES_IT",
                "host-identity-marker-can-only-appear-from-host-comment",
                "markers()[host-identity].anyHitIsAFailure",
            ],
        )
        self.assertIn(
            "test_no_hits_answers_the_condition",
            rows[1]["symbols"],
        )
        self.assertIn(
            "test_a_hit_on_a_host_identity_marker_can_only_come_from_this_host",
            rows[1]["symbols"],
        )
        self.assertIn(
            "test_a_host_identity_hit_is_a_failure_outright",
            rows[1]["symbols"],
        )
        self.assertIn("whyAByteSearchSettlesIt", rows[2]["symbols"])
        self.assertIn(
            "markersSearched[tier=host-identity].anyHitIsAFailure",
            rows[2]["symbols"],
        )
        self.assertIn(
            "evidenceChannels[id=host-side-read-only-image-scan].carriesConditions",
            rows[3]["symbols"],
        )
        self.assertEqual(
            rows[4]["symbols"],
            ["section-53-empty-superset-logical-path-inference"],
        )
        self.assertTrue(all(row["replacement"].strip() for row in rows))

    def test_successor_requires_independent_path_and_content_reconciliation(self) -> None:
        requirements = self.record["successorRequirements"]
        self.assertEqual(
            requirements,
            [
                "freeze-the-parser-binary-and-version",
                "revalidate-the-fixed-input-list-no-host-path-and-no-environment-passthrough",
                "enumerate-logical-paths-independently-of-raw-marker-search",
                "bind-every-candidate-path-type-and-content-digest-to-a-sealed-source-or-an-approved-local-generation-recipe-and-digest",
                "reject-every-unsupported-ext4-feature-inode-layout-or-data-mapping",
                "scan-file-content-symlink-targets-journal-and-unmapped-bytes-separately",
                "freeze-operator-markers-instead-of-reading-the-current-home",
                "reconcile-all-135-historical-raw-hits-without-recording-secret-bytes",
                "fail-closed-on-any-journal-slack-unmapped-or-unreconciled-hit",
                "prove-the-image-sha256-is-unchanged-before-and-after",
            ],
        )
        self.assertEqual(
            self.record["reconciliationPolicy"],
            {
                "duplicateContentCopies": "allowed-only-after-each-raw-offset-is-attributed-to-one-physical-owner-and-the-logical-signature-multiset-is-exactly-conserved",
                "unresolvedAmbiguity": "FAIL-CLOSED",
                "journalSlackOrUnmappedBytes": "FAIL-CLOSED",
            },
        )
        self.assertEqual(
            self.record["historicalRawScanAllowedUse"],
            {
                "singleNeedleOccurrencesAndNonOccurrences": "PRESERVED-AS-RAW-BYTE-FACTS",
                "joinedMultiComponentLogicalPathAbsence": "NOT-PROVEN",
                "hostOriginOrSecretLeakFromAnyRawHit": "NOT-PROVEN",
                "conditionVerdict": "NOT-SETTLED",
            },
        )

    def test_docs_name_both_corrected_inferences_without_erasing_raw_facts(self) -> None:
        plan = (
            REPO / "docs/mac-first-hidden-linux-execution-plan-v1.md"
        ).read_text(encoding="utf-8")
        spec = (
            REPO
            / "docs/node-native-shadow-binding-containment-implementation-spec-v1.md"
        ).read_text(encoding="utf-8")
        for document in (plan, spec):
            self.assertIn("host origin", document.lower())
            self.assertIn("single-needle", document.lower())
            self.assertIn("NOT-SETTLED", document)

    def test_plan_appends_the_correction_after_the_generation_correction(self) -> None:
        plan = (
            REPO / "docs/mac-first-hidden-linux-execution-plan-v1.md"
        ).read_text(encoding="utf-8")
        section_56 = "## 56. Keeping the old zero in the generation where it was measured"
        section_57 = "## 57. Keeping the raw scan as an inventory, not a joined-path proof"
        self.assertEqual(plan.count(section_56), 1)
        self.assertEqual(plan.count(section_57), 1)
        self.assertGreater(plan.index(section_57), plan.index(section_56))


if __name__ == "__main__":
    unittest.main()
