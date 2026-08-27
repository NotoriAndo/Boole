"""The MAC.3 serving-gap closure plan is re-derived from the tree, not trusted.

Every number the plan states about a tracked file is recomputed here from that
file, so a plan that drifts away from what it describes fails rather than
quietly ages. The plan makes no claim about the developer machine, so unlike the
gap measurement it needs no local-observation tests -- everything below runs the
same on a clean runner.
"""

from __future__ import annotations

import hashlib
import json
import pathlib
import re
import unittest

REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parents[1]
RECORD_PATH = (
    REPOSITORY_ROOT
    / "native/containment/native-shadow-mac3-serving-gap-closure-plan-arm64-v1.json"
)
CONTRACT_PATH = (
    REPOSITORY_ROOT
    / "native/containment/native-shadow-mac3-guest-runtime-contract-arm64-v1.json"
)
BOOT_LOCK_PATH = (
    REPOSITORY_ROOT
    / "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v1.json"
)
RUNTIME_LOCK_PATH = (
    REPOSITORY_ROOT
    / "native/containment/native-shadow-runtime-rootfs-source-lock-arm64-v1.json"
)
LAUNCHER_BINARY_PATH = (
    REPOSITORY_ROOT
    / "crates/boole-native-shadow-launcher/src/bin/boole-native-shadow-launcher.rs"
)
VERIFIER_PATH = (
    REPOSITORY_ROOT / "crates/boole-native-shadow-launcher/src/runtime_rootfs_replay.rs"
)
SELF_TEST_PATH = REPOSITORY_ROOT / "scripts/self-test.sh"
DOCS_SMOKE_PATH = REPOSITORY_ROOT / "scripts/docs-smoke.sh"


def digest_of(path: pathlib.Path) -> tuple[str, int]:
    raw = path.read_bytes()
    return hashlib.sha256(raw).hexdigest(), len(raw)


def load(path: pathlib.Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


class PlanTestCase(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.record = load(RECORD_PATH)

    def assert_stamp(self, stamp: dict) -> pathlib.Path:
        """A recorded path/digest/size triple must match the file it names."""
        path = REPOSITORY_ROOT / stamp["path"]
        self.assertTrue(path.is_file(), f"{stamp['path']} is not a file")
        sha256, size = digest_of(path)
        self.assertEqual(stamp["sha256"], sha256, stamp["path"])
        self.assertEqual(stamp["sizeBytes"], size, stamp["path"])
        return path


class RecordShapeTests(PlanTestCase):
    def test_the_record_is_a_plan_and_says_so(self) -> None:
        self.assertEqual(
            self.record["schema"],
            "boole.native-shadow.mac3-serving-gap-closure-plan.arm64.v1",
        )
        self.assertEqual(
            self.record["status"], "MAC3-SERVING-GAP-CLOSURE-PLANNED-NOT-IMPLEMENTED"
        )
        self.assertIn("PLANNED-NOT-IMPLEMENTED", self.record["release"])

    def test_nothing_was_run_built_or_claimed(self) -> None:
        for key in (
            "productionDispatched",
            "bootPerformed",
            "builderChanged",
            "lockSuccessorProduced",
            "servingClaim",
            "activationAllowed",
        ):
            self.assertIs(self.record[key], False, key)

    def test_the_plan_carries_no_verdict(self) -> None:
        """A plan that starts reporting outcomes has stopped being a plan."""
        text = json.dumps(self.record)
        for forbidden in ('"verdict"', '"passed"', '"result"', '"servingReached"'):
            self.assertNotIn(forbidden, text)

    def test_boundaries_are_stated(self) -> None:
        boundaries = self.record["boundaries"]
        self.assertGreaterEqual(len(boundaries), 7)
        joined = " ".join(boundaries)
        for phrase in ("no boot was performed", "no pass condition"):
            self.assertIn(phrase, joined)


class GapsMatchTheSealedContractTests(PlanTestCase):
    @classmethod
    def setUpClass(cls) -> None:
        super().setUpClass()
        cls.section = cls.record["gapsToClose"]
        cls.contract = load(CONTRACT_PATH)

    def test_the_contract_it_reads_from_is_pinned_and_unmoved(self) -> None:
        self.assert_stamp(self.section["source"])
        self.assertEqual(
            self.section["source"]["path"],
            "native/containment/native-shadow-mac3-guest-runtime-contract-arm64-v1.json",
        )

    def test_every_sealed_gap_is_carried_forward_unaltered(self) -> None:
        """Re-wording a sealed gap while planning to close it fails here."""
        sealed = {row["path"]: row for row in self.contract["gaps"]}
        planned = {row["path"]: row for row in self.section["gaps"]}
        self.assertEqual(set(sealed), set(planned))
        for path, row in sealed.items():
            for field in ("what", "consequence", "closedBy", "requiresNewImage"):
                self.assertEqual(planned[path][field], row[field], f"{path}.{field}")

    def test_the_count_is_three_and_matches_the_rows(self) -> None:
        self.assertEqual(self.section["count"], 3)
        self.assertEqual(len(self.section["gaps"]), 3)

    def test_exactly_one_gap_is_an_architectural_decision(self) -> None:
        architectural = [
            row for row in self.section["gaps"] if row["architecturalDecision"]
        ]
        self.assertEqual(len(architectural), 1)
        self.assertEqual(architectural[0]["id"], "runtime-rootfs-and-its-content-manifest")

    def test_the_earliest_refusal_is_not_the_measured_gap(self) -> None:
        """The whole reason the three are one unit: something refuses sooner."""
        by_id = {row["id"]: row for row in self.section["gaps"]}
        self.assertEqual(by_id["account-database"]["stageOrder"], 1)
        self.assertEqual(
            by_id["runtime-rootfs-and-its-content-manifest"]["stageOrder"], 7
        )
        self.assertLess(
            by_id["account-database"]["stageOrder"],
            by_id["runtime-rootfs-and-its-content-manifest"]["stageOrder"],
        )

    def test_every_gap_still_requires_a_new_image(self) -> None:
        for row in self.section["gaps"]:
            self.assertIs(row["requiresNewImage"], True, row["id"])


class OneUnitTests(PlanTestCase):
    @classmethod
    def setUpClass(cls) -> None:
        super().setUpClass()
        cls.section = cls.record["whyTheyAreOneUnit"]

    def test_the_production_budget_is_one_and_unspent(self) -> None:
        self.assertEqual(self.section["productionRunsAllowed"], 1)
        self.assertEqual(self.section["productionRunsSpent"], 0)

    def test_the_two_records_it_builds_on_are_pinned(self) -> None:
        self.assert_stamp(self.section["criteriaSource"])
        self.assert_stamp(self.section["measurementSource"])


class BuilderCommonGroundTests(PlanTestCase):
    @classmethod
    def setUpClass(cls) -> None:
        super().setUpClass()
        cls.section = cls.record["builderCommonGround"]
        cls.boot_lock = load(BOOT_LOCK_PATH)
        cls.runtime_lock = load(RUNTIME_LOCK_PATH)

    def test_the_five_named_files_are_pinned_and_unmoved(self) -> None:
        for key in (
            "sharedBuilder",
            "legacyBuilder",
            "bootBuilder",
            "bootLock",
            "runtimeLock",
        ):
            self.assert_stamp(self.section[key])

    def test_one_builder_digest_is_what_both_locks_actually_name(self) -> None:
        """The claim that one module builds both is re-read from both locks."""
        recorded = self.section["sharedBuilderSha256"]
        self.assertEqual(self.boot_lock["buildRecipe"]["builderSha256"], recorded)
        self.assertEqual(self.runtime_lock["buildRecipe"]["builderSha256"], recorded)
        self.assertEqual(self.section["sharedBuilder"]["sha256"], recorded)

    def test_the_pinned_builder_is_the_projection_not_the_assembler(self) -> None:
        """These are two different files, and conflating them hides a check."""
        projection = REPOSITORY_ROOT / self.section["sharedBuilder"]["path"]
        legacy = REPOSITORY_ROOT / self.section["legacyBuilder"]["path"]
        self.assertNotEqual(projection, legacy)
        source = projection.read_text(encoding="utf-8")
        pinned = re.search(r'LEGACY_SHA256 = "([0-9a-f]{64})"', source)
        self.assertIsNotNone(pinned, "the projection no longer pins the legacy digest")
        self.assertEqual(self.section["legacyDigestPinnedByTheProjection"], pinned.group(1))
        self.assertEqual(digest_of(legacy)[0], pinned.group(1))
        self.assertEqual(self.section["legacyBuilder"]["sha256"], pinned.group(1))

    def test_the_two_locks_share_one_schema(self) -> None:
        self.assertIs(self.section["schemasEqual"], True)
        self.assertEqual(self.boot_lock["schema"], self.section["schema"])
        self.assertEqual(self.runtime_lock["schema"], self.section["schema"])

    def test_the_artifact_counts_are_recounted(self) -> None:
        self.assertEqual(
            self.section["bootLock"]["artifactCount"], len(self.boot_lock["artifacts"])
        )
        self.assertEqual(
            self.section["runtimeLock"]["artifactCount"],
            len(self.runtime_lock["artifacts"]),
        )

    def test_the_boot_closure_roots_are_still_a_superset(self) -> None:
        """If this stops holding, nesting stops being duplication of what is there."""
        boot = {row["name"] for row in self.boot_lock["closureRoots"]}
        runtime = {row["name"] for row in self.runtime_lock["closureRoots"]}
        self.assertEqual(set(self.section["bootClosureRoots"]), boot)
        self.assertEqual(set(self.section["runtimeClosureRoots"]), runtime)
        self.assertTrue(runtime <= boot)
        self.assertIs(self.section["runtimeRootsAreASubsetOfBootRoots"], True)
        self.assertEqual(set(self.section["rootsOnlyInBoot"]), boot - runtime)

    def test_the_two_recipes_are_still_the_same_recipe(self) -> None:
        self.assertIs(self.section["recipesEqual"], True)
        self.assertEqual(
            self.boot_lock["buildRecipe"], self.runtime_lock["buildRecipe"]
        )


class ByteBudgetTests(PlanTestCase):
    @classmethod
    def setUpClass(cls) -> None:
        super().setUpClass()
        cls.section = cls.record["theByteBudget"]
        cls.recipe = load(BOOT_LOCK_PATH)["buildRecipe"]

    def test_the_limits_are_the_sealed_limits(self) -> None:
        for key in ("maxTotalBytes", "maxFileBytes", "maxEntries"):
            self.assertEqual(self.section["limits"][key], self.recipe[key], key)

    def test_the_module_that_enforces_them_is_pinned(self) -> None:
        """The limits are only real if the checks are in the file named here."""
        path = self.assert_stamp(self.section["limits"]["enforcedBy"])
        source = path.read_text(encoding="utf-8")
        self.assertIn('recipe["maxTotalBytes"]', source)
        self.assertIn('recipe["maxEntries"]', source)
        self.assertIn("exceeds total byte limit", source)
        self.assertIn("exceeds entry limit", source)
        self.assert_stamp(self.section["limits"]["reachedThrough"])

    def test_the_two_sizes_are_read_from_their_sealed_records(self) -> None:
        boot = load(REPOSITORY_ROOT / self.section["currentBoot"]["source"]["path"])
        self.assert_stamp(self.section["currentBoot"]["source"])
        self.assertEqual(
            self.section["currentBoot"]["initrdSizeBytes"],
            boot["subject"]["initrd"]["sizeBytes"],
        )
        self.assertEqual(
            self.section["currentBoot"]["rootDiskSizeBytes"],
            boot["subject"]["rootDisk"]["sizeBytes"],
        )
        runtime = load(REPOSITORY_ROOT / self.section["runtimeToNest"]["source"]["path"])
        self.assert_stamp(self.section["runtimeToNest"]["source"])
        self.assertEqual(
            self.section["runtimeToNest"]["layerSizeBytes"],
            runtime["expectedOutput"]["layerSizeBytes"],
        )
        self.assertEqual(
            self.section["runtimeToNest"]["contentManifestSha256"],
            runtime["expectedOutput"]["rootfsContentManifestSha256"],
        )

    def test_the_arithmetic_is_the_arithmetic(self) -> None:
        """The headroom is the one number a reader would act on, so it is recomputed."""
        expected_sum = (
            self.section["currentBoot"]["initrdSizeBytes"]
            + self.section["runtimeToNest"]["layerSizeBytes"]
        )
        self.assertEqual(self.section["upperBoundSumBytes"], expected_sum)
        self.assertEqual(
            self.section["headroomBytes"],
            self.recipe["maxTotalBytes"] - expected_sum,
        )
        self.assertIs(
            self.section["fitsUnderTheSealedLimit"], self.section["headroomBytes"] > 0
        )

    def test_the_bound_is_declared_as_a_bound(self) -> None:
        missing = self.section["whatThisDoesNotEstablish"]
        self.assertGreaterEqual(len(missing), 3)
        joined = " ".join(missing)
        self.assertIn("entry count", joined)
        self.assertIn("only that the pinned sizes leave room", joined)


class VerifierSemanticsTests(PlanTestCase):
    @classmethod
    def setUpClass(cls) -> None:
        super().setUpClass()
        cls.section = cls.record["whyDuplicationAndNotSymlinks"]

    def test_the_verifier_it_reads_is_pinned_and_unmoved(self) -> None:
        self.assert_stamp(self.section["source"])

    def test_every_recorded_check_is_present_in_the_verifier(self) -> None:
        """The bytes cost turns on these four checks, so they are re-read."""
        source = VERIFIER_PATH.read_text(encoding="utf-8")
        self.assertIn("require_read_only_mount", source)
        self.assertIn("metadata.nlink() != 1", source)
        self.assertIn("metadata.mode() & 0o7777 != 0o444", source)
        self.assertIn("rootfs path set mismatch", source)
        self.assertEqual(len(self.section["checksThatDecideThis"]), 4)

    def test_the_manifest_metadata_the_plan_states_is_what_the_code_demands(
        self,
    ) -> None:
        source = VERIFIER_PATH.read_text(encoding="utf-8")
        self.assertEqual(self.section["manifestRequiredMode"], "0444")
        self.assertEqual(self.section["manifestRequiredLinkCount"], 1)
        self.assertIn("!= RUNTIME_ROOTFS_CONTENT_MANIFEST_SIZE", source)
        runtime = load(
            REPOSITORY_ROOT
            / "native/containment/native-shadow-runtime-rootfs-replay-expectation-arm64-v1.json"
        )
        self.assertEqual(
            self.section["manifestRequiredSizeBytes"],
            runtime["expectedOutput"]["rootfsContentManifestSizeBytes"],
        )


class StagingPathTests(PlanTestCase):
    @classmethod
    def setUpClass(cls) -> None:
        super().setUpClass()
        cls.section = cls.record["stagingPathsAreNotMasked"]

    def test_the_two_fixed_paths_are_the_binary_s_own_constants(self) -> None:
        source = LAUNCHER_BINARY_PATH.read_text(encoding="utf-8")
        rootfs = re.search(r'FIXED_RUNTIME_ROOTFS: &str = "([^"]+)"', source)
        manifest = re.search(
            r'FIXED_RUNTIME_ROOTFS_MANIFEST: &str =\s*"([^"]+)"', source
        )
        self.assertIsNotNone(rootfs)
        self.assertIsNotNone(manifest)
        self.assertEqual(self.section["fixedRootfsPath"], rootfs.group(1))
        self.assertEqual(self.section["fixedManifestPath"], manifest.group(1))

    def test_no_writable_path_covers_either_fixed_path(self) -> None:
        """A tmpfs over the staging path would hide it, so this is checked."""
        contract = load(CONTRACT_PATH)
        writable = [row["path"] for row in contract["writablePaths"]]
        self.assertEqual(self.section["writablePaths"], writable)
        for path in writable:
            self.assertFalse(self.section["fixedRootfsPath"].startswith(path))
            self.assertFalse(self.section["fixedManifestPath"].startswith(path))
        self.assertIs(self.section["anyWritablePathCoversAFixedPath"], False)

    def test_the_tmpfiles_configuration_only_creates_directories(self) -> None:
        path = self.assert_stamp(self.section["tmpfilesSource"])
        lines = [
            line
            for line in path.read_text(encoding="utf-8").splitlines()
            if line.strip() and not line.startswith("#")
        ]
        self.assertTrue(lines)
        self.assertTrue(all(line.startswith("d ") for line in lines))
        self.assertIs(self.section["tmpfilesDeclaresOnlyDirectories"], True)


class PinsThatMoveTests(PlanTestCase):
    @classmethod
    def setUpClass(cls) -> None:
        super().setUpClass()
        cls.rows = cls.record["pinsThatMove"]

    def test_there_is_one_row_per_gap(self) -> None:
        gaps = {row["id"] for row in self.record["gapsToClose"]["gaps"]}
        self.assertEqual({row["gap"] for row in self.rows}, gaps)

    def test_only_the_unit_edit_moves_an_existing_file_s_digest(self) -> None:
        moving = [row for row in self.rows if row["movesADigestOfAnExistingFile"]]
        self.assertEqual(len(moving), 1)
        self.assertEqual(moving[0]["gap"], "refusal-is-not-readable")
        self.assert_stamp(moving[0]["fileEdited"])

    def test_the_unit_being_edited_is_the_one_the_lock_names(self) -> None:
        """The edit is only cheap if the lock pins the same file it would move."""
        row = next(row for row in self.rows if row["movesADigestOfAnExistingFile"])
        lock = load(BOOT_LOCK_PATH)
        tracked = {entry["sourcePath"]: entry for entry in lock["trackedFiles"]}
        self.assertIn(row["fileEdited"]["path"], tracked)
        self.assertEqual(
            tracked[row["fileEdited"]["path"]]["sha256"], row["fileEdited"]["sha256"]
        )


class SuccessorChainTests(PlanTestCase):
    def test_the_chain_is_four_steps_in_a_fixed_order_and_unwalked(self) -> None:
        section = self.record["successorChainRequired"]
        self.assertIs(section["walkedInThisSession"], False)
        orders = [step["order"] for step in section["steps"]]
        self.assertEqual(orders, [1, 2, 3, 4])


class HeldConditionTests(PlanTestCase):
    def test_the_held_condition_is_carried_over_unchanged(self) -> None:
        """A plan is not a place to quietly resolve an open operator question."""
        held = load(CONTRACT_PATH)["heldCondition"]
        section = self.record["heldConditionUnchanged"]
        for field in ("id", "state", "relaxed", "waived", "satisfied"):
            self.assertEqual(section[field], held[field], field)
        self.assertIs(section["relaxed"], False)
        self.assertIs(section["waived"], False)
        self.assertIs(section["satisfied"], False)


class RegistrationTests(PlanTestCase):
    def test_the_record_is_pinned_in_docs_smoke(self) -> None:
        smoke = DOCS_SMOKE_PATH.read_text(encoding="utf-8")
        self.assertIn(
            "native-shadow-mac3-serving-gap-closure-plan-arm64-v1.json", smoke
        )
        self.assertIn("MAC3-SERVING-GAP-CLOSURE-PLANNED-NOT-IMPLEMENTED", smoke)

    def test_this_file_runs_in_self_test(self) -> None:
        self.assertIn(
            pathlib.Path(__file__).name,
            SELF_TEST_PATH.read_text(encoding="utf-8"),
        )


if __name__ == "__main__":
    unittest.main()
