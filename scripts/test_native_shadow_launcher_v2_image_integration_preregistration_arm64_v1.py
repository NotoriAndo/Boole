#!/usr/bin/env python3
"""Freeze the launcher-v2 image-integration boundary before wiring it.

This gate deliberately precedes a new image producer.  It binds the inputs and
the one expected staging delta, while keeping every production and boot budget
at zero.  A later implementation may consume this contract; this record cannot
grant that implementation permission to create an image.
"""
from __future__ import annotations

import hashlib
import json
import unittest
from pathlib import Path

from scripts import native_shadow_rootfs_builder_boot_arm64_v1 as builder_v1
from scripts import native_shadow_rootfs_builder_boot_arm64_v3 as builder_v3


REPO = Path(__file__).resolve().parents[1]
RECORD_PATH = (
    REPO
    / "native/containment/"
    "native-shadow-mac3-launcher-v2-image-integration-preregistration-arm64-v1.json"
)
MEASUREMENT_PATH = (
    REPO
    / "native/containment/native-shadow-boot-staging-tree-measurement-arm64-v1.json"
)
V1_RESULT_PATH = (
    REPO / "native/containment/native-shadow-launcher-build-result-arm64-v1.json"
)
V2_RESULT_PATH = (
    REPO / "native/containment/native-shadow-launcher-build-result-arm64-v2.json"
)
OLD_AUTHORITY_PATH = (
    REPO
    / "native/containment/native-shadow-mac3-successor-production-authority-arm64-v4.json"
)
OLD_FINGERPRINT_PATH = (
    REPO
    / "native/containment/native-shadow-mac3-successor-producer-fingerprint-arm64-v4.json"
)
OLD_PRODUCER_PATH = REPO / "scripts/native_shadow_successor_produce_phase_arm64_v2.py"
OLD_WORKFLOW_PATH = REPO / ".github/workflows/native-shadow-successor-produce-arm64.yml"
SELF_TEST_PATH = REPO / "scripts/self-test.sh"
DOCS_SMOKE_PATH = REPO / "scripts/docs-smoke.sh"
PLAN_PATH = REPO / "docs/mac-first-hidden-linux-execution-plan-v1.md"


def load(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def record() -> dict:
    return load(RECORD_PATH)


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


class RecordShapeTests(unittest.TestCase):
    def test_record_is_a_preregistration_not_a_production_authority(self) -> None:
        value = record()
        self.assertEqual(
            value["schema"],
            "boole.native-shadow.mac3.launcher-v2-image-integration-preregistration.arm64.v1",
        )
        self.assertEqual(
            value["status"], "PRE-REGISTERED-NO-IMAGE-PRODUCTION-AUTHORITY"
        )
        self.assertEqual(value["runsPerformed"], 0)

    def test_every_later_authority_stays_false(self) -> None:
        authorisations = record()["authorisations"]
        self.assertEqual(
            set(authorisations),
            {
                "imageProductionRunsAllowed",
                "imageProductionAuthorised",
                "bootAuthorised",
                "mac4Started",
                "testnetStarted",
                "miningActivated",
                "rewardActivated",
                "consensusActivated",
                "p2pActivated",
            },
        )
        self.assertEqual(authorisations["imageProductionRunsAllowed"], 0)
        for key in (
            "imageProductionAuthorised",
            "bootAuthorised",
            "mac4Started",
            "testnetStarted",
            "miningActivated",
            "rewardActivated",
            "consensusActivated",
            "p2pActivated",
        ):
            self.assertFalse(authorisations[key], key)

    def test_the_old_generation_is_history_not_a_template_to_edit(self) -> None:
        generation = record()["generation"]
        self.assertTrue(generation["newGenerationFilesOnly"])
        self.assertTrue(generation["historicalProducerAndWorkflowStayByteUnchanged"])
        self.assertFalse(generation["editsHistoricalWorkflow"])
        self.assertFalse(generation["editsHistoricalProducer"])

    def test_the_known_historical_gate_drift_is_preserved_not_denied(self) -> None:
        historical = load(
            REPO
            / "native/containment/native-shadow-mac3-successor-image-production-result-arm64-v4.json"
        )["producerFingerprintAfterTheAttempt"]
        self.assertFalse(historical["stillPinsLiveBytes"])
        self.assertEqual(
            record()["generation"]["knownHistoricalFingerprintDrift"], historical
        )

    def test_the_record_is_canonical_json(self) -> None:
        value = record()
        expected = (
            json.dumps(value, sort_keys=True, indent=2, ensure_ascii=False) + "\n"
        ).encode("utf-8")
        self.assertEqual(RECORD_PATH.read_bytes(), expected)


class BindingTests(unittest.TestCase):
    def pins(self) -> list[dict]:
        return record()["bindings"]

    def test_every_bound_file_matches_its_recorded_bytes(self) -> None:
        for pin in self.pins():
            path = REPO / pin["path"]
            raw = path.read_bytes()
            with self.subTest(path=pin["path"]):
                self.assertEqual(hashlib.sha256(raw).hexdigest(), pin["sha256"])
                self.assertEqual(len(raw), pin["sizeBytes"])

    def test_binding_paths_are_unique_safe_relative_and_exact(self) -> None:
        paths = [pin["path"] for pin in self.pins()]
        self.assertEqual(len(paths), len(set(paths)))
        for raw in paths:
            path = Path(raw)
            self.assertFalse(path.is_absolute(), raw)
            self.assertNotIn("..", path.parts, raw)
        self.assertEqual(
            set(paths),
            {
                "native/containment/native-shadow-launcher-source-overlay-arm64-v2.json",
                "native/containment/native-shadow-launcher-build-authority-arm64-v2.json",
                "native/containment/native-shadow-launcher-build-result-arm64-v2.json",
                "native/containment/native-shadow-launcher-v2-console-evidence-protocol-arm64-v1.json",
                "scripts/native_shadow_launcher_emit_arm64_v2.py",
                "native/containment/native-shadow-launcher-build-result-arm64-v1.json",
                "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v2.json",
                "native/containment/native-shadow-boot-staging-tree-measurement-arm64-v1.json",
                "native/containment/native-shadow-runtime-rootfs-source-lock-arm64-v1.json",
                "scripts/native_shadow_rootfs_builder_boot_arm64_v3.py",
                "scripts/native_shadow_rootfs_portable_boot_arm64_v2.py",
                "native/systemd/boole-native-shadow-launcher-v2.service",
                "native/etc/passwd",
                "native/etc/shadow",
                "native/etc/group",
                "native/etc/gshadow",
                "native/etc/nsswitch.conf",
                "native/containment/native-shadow-mac3-successor-production-authority-arm64-v4.json",
                "native/containment/native-shadow-mac3-successor-producer-fingerprint-arm64-v4.json",
                "native/containment/native-shadow-mac3-successor-image-production-result-arm64-v4.json",
                "native/containment/native-shadow-mac3-successor-image-preservation-arm64-v4.json",
                "native/containment/native-shadow-mac3-closed-local-boot-qualification-arm64-v3.json",
            },
        )

    def test_required_launcher_v2_inputs_are_all_bound(self) -> None:
        paths = {pin["path"] for pin in self.pins()}
        required = {
            "native/containment/native-shadow-launcher-source-overlay-arm64-v2.json",
            "native/containment/native-shadow-launcher-build-authority-arm64-v2.json",
            "native/containment/native-shadow-launcher-build-result-arm64-v2.json",
            "native/containment/native-shadow-launcher-v2-console-evidence-protocol-arm64-v1.json",
            "scripts/native_shadow_launcher_emit_arm64_v2.py",
        }
        self.assertTrue(required <= paths, sorted(required - paths))

    def test_required_staging_inputs_are_all_bound(self) -> None:
        paths = {pin["path"] for pin in self.pins()}
        required = {
            "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v2.json",
            "native/containment/native-shadow-boot-staging-tree-measurement-arm64-v1.json",
            "native/containment/native-shadow-runtime-rootfs-source-lock-arm64-v1.json",
            "scripts/native_shadow_rootfs_builder_boot_arm64_v3.py",
            "scripts/native_shadow_rootfs_portable_boot_arm64_v2.py",
            "native/systemd/boole-native-shadow-launcher-v2.service",
            "native/etc/passwd",
            "native/etc/shadow",
            "native/etc/group",
            "native/etc/gshadow",
            "native/etc/nsswitch.conf",
        }
        self.assertTrue(required <= paths, sorted(required - paths))

    def test_historical_result_preservation_and_boot_criteria_are_bound(self) -> None:
        paths = {pin["path"] for pin in self.pins()}
        required = {
            "native/containment/native-shadow-mac3-successor-production-authority-arm64-v4.json",
            "native/containment/native-shadow-mac3-successor-producer-fingerprint-arm64-v4.json",
            "native/containment/native-shadow-mac3-successor-image-production-result-arm64-v4.json",
            "native/containment/native-shadow-mac3-successor-image-preservation-arm64-v4.json",
            "native/containment/native-shadow-mac3-closed-local-boot-qualification-arm64-v3.json",
        }
        self.assertTrue(required <= paths, sorted(required - paths))


class HistoricalGenerationTests(unittest.TestCase):
    def test_old_workflow_still_matches_the_old_authority(self) -> None:
        authority = load(OLD_AUTHORITY_PATH)
        pin = next(
            row
            for row in authority["boundInputDigests"]["files"]
            if row["path"] == ".github/workflows/native-shadow-successor-produce-arm64.yml"
        )
        self.assertEqual(digest(OLD_WORKFLOW_PATH), pin["sha256"])

    def test_old_producer_still_matches_the_old_fingerprint(self) -> None:
        fingerprint = load(OLD_FINGERPRINT_PATH)
        pin = next(
            row
            for row in fingerprint["files"]
            if row["path"] == "scripts/native_shadow_successor_produce_phase_arm64_v2.py"
        )
        self.assertEqual(digest(OLD_PRODUCER_PATH), pin["sha256"])

    def test_old_producer_and_workflow_do_not_import_the_v2_emitter(self) -> None:
        needle = "native_shadow_launcher_emit_arm64_v2"
        self.assertNotIn(needle, OLD_PRODUCER_PATH.read_text(encoding="utf-8"))
        self.assertNotIn(needle, OLD_WORKFLOW_PATH.read_text(encoding="utf-8"))


class Arm64EmitterProofTests(unittest.TestCase):
    def test_the_emitter_was_proved_on_the_exact_pr_head_before_preregistration(self) -> None:
        proof = record()["launcherV2Arm64Proof"]
        self.assertEqual(proof["pullRequest"], 301)
        self.assertEqual(
            proof["headSha"], "d1278ff28f6594e6fbeab76ee784a016f7bcf988"
        )
        self.assertTrue(proof["pullRequestMerged"])
        self.assertEqual(
            proof["mainMergeSha"],
            "d0fa6bed4996688b0d8121f3a1a9f912e1b3dfb3",
        )
        self.assertTrue(proof["allRequiredChecksPassed"])
        self.assertTrue(proof["twoBuildReproofPassed"])
        self.assertTrue(proof["thirdEmitterBuildPassed"])

    def test_the_independent_shell_observation_matches_the_sealed_result(self) -> None:
        proof = record()["launcherV2Arm64Proof"]
        launcher = load(V2_RESULT_PATH)["launcher"]
        self.assertEqual(proof["emittedSha256"], launcher["sha256"])
        self.assertEqual(proof["emittedSizeBytes"], launcher["sizeBytes"])
        self.assertEqual(
            proof["ciRun"],
            "https://github.com/NotoriAndo/Boole/actions/runs/33269194319",
        )


class ProjectionTests(unittest.TestCase):
    def setUp(self) -> None:
        self.value = record()["expectedProjection"]
        self.measurement = load(MEASUREMENT_PATH)
        self.v1 = load(V1_RESULT_PATH)["launcher"]
        self.v2 = load(V2_RESULT_PATH)["launcher"]

    def test_the_launcher_keeps_the_same_guest_path(self) -> None:
        self.assertEqual(self.v1["guestLogicalPath"], self.v2["guestLogicalPath"])
        self.assertEqual(self.value["launcherGuestPath"], self.v2["guestLogicalPath"])

    def test_the_launcher_delta_is_derived_from_both_sealed_results(self) -> None:
        delta = self.v2["sizeBytes"] - self.v1["sizeBytes"]
        self.assertEqual(delta, 18560)
        self.assertEqual(self.value["launcherSizeDeltaBytes"], delta)
        self.assertEqual(self.value["historicalLauncher"], self.v1)
        self.assertEqual(self.value["successorLauncher"], self.v2)

    def test_the_launcher_excluded_measurement_is_not_rewritten(self) -> None:
        self.assertEqual(
            self.value["withoutLauncher"], self.measurement["builderInternal"]
        )

    def test_the_successor_total_changes_by_exactly_the_launcher_delta(self) -> None:
        base = self.measurement["builderInternal"]
        projected = self.value["withLauncherV2"]
        self.assertEqual(projected["entries"], base["entries"] + 2)
        self.assertEqual(projected["entries"], 17676)
        self.assertEqual(projected["payloadBytes"], base["payloadBytes"] + self.v2["sizeBytes"])
        self.assertEqual(projected["payloadBytes"], 1773475059)
        self.assertEqual(projected["largestFileBytes"], base["largestFileBytes"])

    def test_the_historical_total_is_rederived_not_hand_copied(self) -> None:
        base = self.measurement["builderInternal"]
        historical = self.measurement["withSealedLauncher"]
        self.assertEqual(historical["entries"], base["entries"] + 2)
        self.assertEqual(
            historical["payloadBytes"], base["payloadBytes"] + self.v1["sizeBytes"]
        )

    def test_only_launcher_content_may_change(self) -> None:
        change = self.value["onlyExpectedChange"]
        self.assertEqual(change["paths"], [self.v2["guestLogicalPath"]])
        for key in ("path", "kind", "mode", "uid", "gid"):
            self.assertTrue(change[f"{key}Unchanged"], key)
        self.assertEqual(change["contentChanges"], 1)

    def test_launcher_metadata_is_derived_from_the_pinned_builder(self) -> None:
        raw = b"launcher-metadata-probe"
        entry = builder_v3.launcher_entry(
            raw,
            sha256=hashlib.sha256(raw).hexdigest(),
            size=len(raw),
        )
        entry.pop("raw")
        self.assertEqual(self.value["launcherMetadata"], entry)

    def test_current_builder_refuses_launcher_v2_and_a_new_projection_is_required(self) -> None:
        with self.assertRaisesRegex(builder_v1.RootfsBuildError, "launcher-digest-mismatch"):
            builder_v3.launcher_entry(bytes(self.v2["sizeBytes"]))
        self.assertTrue(record()["plannedSuccessor"]["newBuilderProjectionRequired"])

    def test_successor_projection_stays_within_all_frozen_limits(self) -> None:
        lock = load(
            REPO
            / "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v2.json"
        )
        limits = {
            key: lock["buildRecipe"][key]
            for key in ("maxEntries", "maxFileBytes", "maxTotalBytes")
        }
        self.assertEqual(self.value["limits"], limits)
        projected = self.value["withLauncherV2"]
        self.assertLessEqual(projected["entries"], limits["maxEntries"])
        self.assertLessEqual(projected["payloadBytes"], limits["maxTotalBytes"])
        self.assertLessEqual(
            projected["largestFileBytes"], limits["maxFileBytes"]
        )


class FreePreflightBoundaryTests(unittest.TestCase):
    def test_preflight_is_repeatable_but_cannot_produce(self) -> None:
        preflight = record()["preflight"]
        self.assertTrue(preflight["repeatable"])
        self.assertEqual(preflight["runsPerformedByThisRecord"], 0)
        self.assertFalse(preflight["createsOutputDirectory"])
        self.assertFalse(preflight["writesAttemptConsumedMarker"])
        self.assertFalse(preflight["emitsImageArtifacts"])

    def test_image_tools_and_one_use_names_are_forbidden(self) -> None:
        preflight = record()["preflight"]
        self.assertEqual(preflight["allowedImageTools"], [])
        self.assertIn("ATTEMPT-CONSUMED.json", preflight["forbiddenNames"])
        for name in ("guest-kernel", "guest-initrd", "guest-root-disk"):
            self.assertIn(name, preflight["forbiddenNames"])

    def test_the_next_implementation_must_be_a_new_generation(self) -> None:
        planned = record()["plannedSuccessor"]
        self.assertTrue(planned["newProducerModuleRequired"])
        self.assertTrue(planned["newWorkflowRequired"])
        self.assertTrue(planned["newBuilderProjectionRequired"])
        self.assertTrue(planned["builderProjectionPinsPredecessorByDigest"])
        self.assertTrue(planned["v2AcceptedAndV1Rejected"])
        self.assertTrue(planned["globalMonkeypatchForbidden"])
        self.assertTrue(planned["sameAssemblerForPreflightAndFutureProduction"])
        self.assertFalse(planned["implementedByThisRecord"])


class HardStopTests(unittest.TestCase):
    def test_every_stop_is_fail_closed(self) -> None:
        stops = record()["hardStopConditions"]
        self.assertGreaterEqual(len(stops), 6)
        ids = [stop["id"] for stop in stops]
        self.assertEqual(len(ids), len(set(ids)))
        for stop in stops:
            self.assertTrue(stop["stop"], stop["id"])
            self.assertFalse(stop["proceedAnyway"], stop["id"])

    def test_the_critical_stop_classes_are_named(self) -> None:
        ids = {stop["id"] for stop in record()["hardStopConditions"]}
        self.assertTrue(
            {
                "bound-input-drift",
                "v1-launcher-consumer-reintroduced",
                "projection-mismatch",
                "image-tool-or-output-observed",
                "historical-generation-modified",
                "emitter-not-exactly-reproved-on-arm64",
            }
            <= ids
        )


class GateWiringTests(unittest.TestCase):
    def test_self_test_runs_this_preregistration_gate(self) -> None:
        self.assertIn(
            "scripts/test_native_shadow_launcher_v2_image_integration_preregistration_arm64_v1.py",
            SELF_TEST_PATH.read_text(encoding="utf-8"),
        )

    def test_docs_smoke_pins_the_zero_authority_and_builder_boundary(self) -> None:
        smoke = DOCS_SMOKE_PATH.read_text(encoding="utf-8")
        for needle in (
            RECORD_PATH.name,
            "PRE-REGISTERED-NO-IMAGE-PRODUCTION-AUTHORITY",
            '"imageProductionRunsAllowed": 0',
            '"newBuilderProjectionRequired": true',
            '"payloadBytes": 1773475059',
        ):
            self.assertIn(needle, smoke)

    def test_plan_moves_to_the_new_builder_without_opening_production(self) -> None:
        plan = PLAN_PATH.read_text(encoding="utf-8")
        self.assertIn("LAUNCHER V2 IMAGE INTEGRATION  PRE-REGISTERED / AUTHORITY 0", plan)
        self.assertIn("SUCCESSOR BUILDER PROJECTION  NEXT", plan)
        self.assertIn("IMAGE PRODUCTION  NOT AUTHORISED", plan)


class InvariantTests(unittest.TestCase):
    def test_activation_and_mining_values_do_not_move(self) -> None:
        invariants = record()["invariants"]
        self.assertEqual(invariants["LLM-MINEABLE-ELIGIBLE-V5"], 14160)
        self.assertEqual(invariants["mineable_now"], 0)
        self.assertEqual(invariants["REWARD_READY"], 0)
        self.assertEqual(invariants["RP0-MD"], "HOLD")
        self.assertEqual(invariants["BF.7"], "HOLD")
        self.assertFalse(invariants["baseActivation"])
        self.assertFalse(invariants["activationAllowed"])


if __name__ == "__main__":
    unittest.main()
