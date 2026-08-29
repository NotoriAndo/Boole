#!/usr/bin/env python3
"""The current successor path must not be confused with its predecessor.

The historical gap record was true when the base builder did not stage the
nested runtime tree.  The preserved v4 image was produced later, through a
successor projection that does stage it.  This gate keeps those two statements
in their own generations so a literal search of the old builder cannot turn
back into a claim about the image that exists now.
"""

import hashlib
import inspect
import json
import pathlib
import unittest

from scripts import native_shadow_rootfs_builder_boot_arm64_v3 as builder
from scripts import native_shadow_successor_produce_phase_arm64_v2 as producer


REPO = pathlib.Path(__file__).resolve().parents[1]
RECORD = (
    REPO
    / "native/containment/native-shadow-mac3-runtime-path-generation-correction-arm64-v1.json"
)
AUTHORITY = (
    REPO
    / "native/containment/native-shadow-mac3-successor-production-authority-arm64-v4.json"
)
RESULT = (
    REPO
    / "native/containment/native-shadow-mac3-successor-image-production-result-arm64-v4.json"
)
QUALIFICATION = (
    REPO
    / "native/containment/native-shadow-mac3-closed-local-boot-qualification-arm64-v3.json"
)
HISTORICAL_MEASUREMENT = (
    REPO
    / "native/containment/native-shadow-mac3-runtime-serving-gap-measurement-arm64-v1.json"
)


def read_json(path: pathlib.Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def sha256(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


class CorrectionRecordTests(unittest.TestCase):
    def test_the_current_image_is_not_described_by_the_historical_zero(self) -> None:
        record = read_json(RECORD)
        self.assertEqual(
            record["schema"],
            "boole.native-shadow.mac3.runtime-path-generation-correction.arm64.v1",
        )
        self.assertEqual(record["status"], "CORRECTED-CURRENT-PATHS-PRESENT-RUNTIME-UNMEASURED")
        self.assertTrue(record["historicalStatement"]["wasTrueForItsGeneration"])
        self.assertFalse(record["historicalStatement"]["describesThePreservedV4Image"])
        self.assertTrue(record["currentStatement"]["runtimeTreeAndManifestWereAssembled"])
        self.assertFalse(record["currentStatement"]["launcherRuntimeVerificationMeasured"])

    def test_current_generation_is_bound_to_the_live_production_chain(self) -> None:
        record = read_json(RECORD)
        generation = record["currentGeneration"]

        live_builder = producer.builder()
        self.assertIs(live_builder, builder)
        self.assertEqual(
            generation["producer"],
            {
                "path": "scripts/native_shadow_successor_produce_phase_arm64_v2.py",
                "sha256": sha256(pathlib.Path(producer.__file__)),
            },
        )
        self.assertEqual(
            generation["builder"],
            {
                "path": "scripts/native_shadow_rootfs_builder_boot_arm64_v3.py",
                "sha256": sha256(pathlib.Path(builder.__file__)),
            },
        )
        self.assertEqual(producer.BUILDER_SHA256, generation["builder"]["sha256"])

        authority = read_json(AUTHORITY)
        authority_builder = authority["successorInputs"]["builder"]
        nested = authority["successorInputs"]["nestedRuntime"]
        self.assertEqual(authority_builder["module"], generation["builder"]["path"])
        self.assertEqual(authority_builder["sha256"], generation["builder"]["sha256"])
        self.assertEqual(
            generation["authority"],
            {"path": str(AUTHORITY.relative_to(REPO)), "sha256": sha256(AUTHORITY)},
        )
        self.assertEqual(
            generation["runtimeRootfsGuestPath"],
            nested["guestPrefix"],
        )
        self.assertEqual(
            generation["contentManifestGuestPath"],
            nested["contentManifestGuestPath"],
        )
        self.assertEqual(builder.NESTED_RUNTIME_TREE["guestPrefix"], nested["guestPrefix"])
        self.assertEqual(
            builder.NESTED_RUNTIME_TREE["contentManifestGuestPath"],
            nested["contentManifestGuestPath"],
        )

        result = read_json(RESULT)
        self.assertEqual(
            generation["productionResult"],
            {"path": str(RESULT.relative_to(REPO)), "sha256": sha256(RESULT)},
        )
        self.assertEqual(result["status"], "SUCCESSOR-PRODUCTION-PASSED")
        self.assertTrue(result["readBack"]["passed"])
        self.assertEqual(result["readBack"]["entryCount"], 17_677)
        self.assertEqual(result["readBack"]["checksThatFailed"], [])

        qualification = read_json(QUALIFICATION)
        self.assertEqual(
            generation["qualification"],
            {"path": str(QUALIFICATION.relative_to(REPO)), "sha256": sha256(QUALIFICATION)},
        )
        gap = next(
            item
            for item in qualification["gapsOpenAtTheSecondAttempt"]
            if item["path"] == generation["runtimeRootfsGuestPath"]
        )
        self.assertFalse(gap["stillAbsent"])

    def test_the_zero_occurrence_measurement_stays_scoped_to_builder_v1(self) -> None:
        record = read_json(RECORD)
        history = record["historicalGeneration"]
        measurement = read_json(HISTORICAL_MEASUREMENT)

        self.assertEqual(
            history["measurement"],
            {
                "path": str(HISTORICAL_MEASUREMENT.relative_to(REPO)),
                "sha256": sha256(HISTORICAL_MEASUREMENT),
            },
        )
        measured_builder = measurement["whatTheImageBuilderKnows"]
        self.assertEqual(
            history["builder"],
            {"path": measured_builder["path"], "sha256": measured_builder["sha256"]},
        )
        self.assertEqual(measured_builder["mentionsOfRequiredPaths"], 0)
        self.assertEqual(
            [item["presentInCurrentImage"] for item in measurement["fixedGuestPathsRequired"]],
            [False, False],
        )
        self.assertNotEqual(history["builder"]["path"], record["currentGeneration"]["builder"]["path"])
        self.assertNotEqual(
            history["builder"]["sha256"], record["currentGeneration"]["builder"]["sha256"]
        )

    def test_nested_tree_is_mandatory_in_the_producer_and_merged_by_builder_v3(self) -> None:
        record = read_json(RECORD)
        self.assertEqual(
            record["mechanicalBindings"],
            [
                "main-builds-the-sealed-nested-tree",
                "main-forwards-the-tree-to-both-modes",
                "preflight-and-produce-require-the-tree",
                "assemble-forwards-the-tree-to-builder-v3",
                "builder-v3-merges-before-deriving-parent-paths",
            ],
        )

        for function in (producer.preflight, producer.produce, producer._assemble):
            nested = inspect.signature(function).parameters["nested_tree"]
            self.assertIs(nested.default, inspect.Parameter.empty)

        main_source = inspect.getsource(producer.main)
        self.assertIn("nested_tree = builder().nested_runtime_tree(", main_source)
        self.assertIn('"nested_tree": nested_tree,', main_source)
        assemble_source = inspect.getsource(producer._assemble)
        self.assertIn("nested_tree=nested_tree,", assemble_source)

        derived = builder._derived_source()
        merge = '_merge(entries, nested_tree, "nested runtime tree")'
        parents = "_ensure_parents(entries)"
        self.assertEqual(derived.count(merge), 1)
        self.assertLess(derived.index(merge), derived.index(parents, derived.index(merge)))

    def test_assembly_evidence_does_not_turn_into_a_runtime_or_boot_claim(self) -> None:
        record = read_json(RECORD)
        generation = record["currentGeneration"]
        boundary = record["claimBoundary"]
        authority = read_json(AUTHORITY)
        result = read_json(RESULT)
        qualification = read_json(QUALIFICATION)

        nested = authority["successorInputs"]["nestedRuntime"]
        self.assertEqual(
            generation["contentManifest"],
            {
                "sha256": nested["contentManifestSha256"],
                "sizeBytes": nested["contentManifestSizeBytes"],
                "replayExpectationPath": nested["replayExpectationPath"],
                "replayExpectationSha256": nested["replayExpectationSha256"],
            },
        )
        self.assertEqual(result["authorityPath"], generation["authority"]["path"])
        self.assertEqual(result["authoritySha256"], generation["authority"]["sha256"])
        self.assertEqual(result["runId"], generation["productionRunId"])
        self.assertEqual(qualification["subject"]["productionRunId"], result["runId"])

        runtime_gap = next(
            item
            for item in qualification["gapsOpenAtTheSecondAttempt"]
            if item["path"] == generation["runtimeRootfsGuestPath"]
        )
        self.assertEqual(runtime_gap["evidence"]["path"], generation["authority"]["path"])
        self.assertEqual(runtime_gap["evidence"]["sha256"], generation["authority"]["sha256"])

        self.assertEqual(
            boundary,
            {
                "imageAssemblyEstablished": True,
                "preservedRootDiskPathsDirectlyReadByThisCorrection": False,
                "launcherRuntimeVerificationMeasured": False,
                "guestBootVerified": False,
                "servingClaim": False,
                "activationAllowed": False,
            },
        )
        self.assertFalse(result["boundaries"]["runtimeCompatibilityVerified"])
        self.assertFalse(result["boundaries"]["guestBootVerified"])
        self.assertFalse(result["boundaries"]["servingClaim"])
        self.assertFalse(result["boundaries"]["activationAllowed"])
        self.assertFalse(qualification["boundaries"]["runtimeCompatibilityVerified"])
        self.assertFalse(qualification["boundaries"]["guestBootVerified"])
        self.assertFalse(qualification["boundaries"]["launcherServing"])


if __name__ == "__main__":
    unittest.main()
