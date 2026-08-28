"""The entry-count half of the nesting budget, bounded rather than measured.

The closure plan answered the byte half and said the entry half could not be
answered because no entry count was pinned anywhere. That was too strong: one is
pinned, in the record that walked both root disk replicas entry by entry. Adding
the already-sealed fact that the runtime closure is contained in the boot closure
turns it into an upper bound on the nested total.

A bound is only worth recording if it cannot quietly become a measurement, so
every number here is re-derived from the file that holds it, the subset relation
is recomputed from the two locks rather than asserted, and the record is required
to keep saying which side of the build it counted.
"""

import hashlib
import json
import pathlib
import unittest

REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parent.parent
RECORD_PATH = (
    REPOSITORY_ROOT
    / "native/containment/native-shadow-mac3-nested-runtime-entry-budget-arm64-v1.json"
)
COUNTED_PATH = (
    REPOSITORY_ROOT
    / "native/containment/native-shadow-boot-root-disk-determinism-hard-stop-arm64-v1.json"
)
BOOT_LOCK_PATH = (
    REPOSITORY_ROOT
    / "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v1.json"
)
RUNTIME_LOCK_PATH = (
    REPOSITORY_ROOT
    / "native/containment/native-shadow-runtime-rootfs-source-lock-arm64-v1.json"
)
PLAN_PATH = (
    REPOSITORY_ROOT
    / "native/containment/native-shadow-mac3-serving-gap-closure-plan-arm64-v1.json"
)
BUILDER_PATH = REPOSITORY_ROOT / "scripts/native_shadow_rootfs_builder.py"


def digest_of(path):
    payload = path.read_bytes()
    return hashlib.sha256(payload).hexdigest(), len(payload)


def load(path):
    return json.loads(path.read_text(encoding="utf-8"))


def artifact_digests(lock):
    return {
        row.get("sha256") or row.get("digest") for row in lock.get("artifacts") or []
    }


def closure_root_names(lock):
    return {row["name"] for row in lock["closureRoots"]}


class RecordShapeTests(unittest.TestCase):
    def setUp(self):
        self.record = load(RECORD_PATH)

    def test_the_record_is_the_arm64_entry_budget_schema(self):
        self.assertEqual(
            self.record["schema"],
            "boole.native-shadow.mac3-nested-runtime-entry-budget.arm64.v1",
        )
        self.assertEqual(
            self.record["status"],
            "MAC3-NESTED-RUNTIME-ENTRY-BUDGET-BOUNDED-NOT-MEASURED",
        )

    def test_the_record_carries_no_outcome_key(self):
        forbidden = {"verdict", "passed", "servingReached"}
        seen = set()

        def walk(node):
            if isinstance(node, dict):
                for key, value in node.items():
                    seen.add(key)
                    walk(value)
            elif isinstance(node, list):
                for item in node:
                    walk(item)

        walk(self.record)
        self.assertEqual(forbidden & seen, set())

    def test_nothing_was_built_produced_or_booted(self):
        nothing = self.record["nothingWasBuilt"]
        for field in (
            "imageProduced",
            "productionDispatched",
            "bootPerformed",
            "builderChanged",
            "treeAssembled",
        ):
            self.assertIs(nothing[field], False, field)
        self.assertIs(self.record["activationAllowed"], False)


class TheLimitTests(unittest.TestCase):
    """The limit is only a limit if both locks agree on it."""

    def setUp(self):
        self.section = load(RECORD_PATH)["theLimit"]

    def test_the_limit_is_what_both_locks_actually_seal(self):
        boot = load(BOOT_LOCK_PATH)["buildRecipe"]["maxEntries"]
        runtime = load(RUNTIME_LOCK_PATH)["buildRecipe"]["maxEntries"]
        self.assertEqual(boot, runtime)
        self.assertEqual(self.section["maxEntries"], boot)

    def test_both_locks_are_stamped_as_the_tree_holds_them(self):
        for stamp in self.section["sealedIn"]:
            path = REPOSITORY_ROOT / stamp["path"]
            sha256, size = digest_of(path)
            self.assertEqual(stamp["sha256"], sha256, stamp["path"])
            self.assertEqual(stamp["sizeBytes"], size, stamp["path"])
        self.assertEqual(len(self.section["sealedIn"]), 2)

    def test_the_builder_refuses_rather_than_truncating(self):
        """A limit that trimmed the tree would make the bound meaningless."""
        self.assertIs(self.section["builderRefusesRatherThanTruncates"], True)
        enforced = self.section["enforcedIn"]
        path = REPOSITORY_ROOT / enforced["path"]
        sha256, size = digest_of(path)
        self.assertEqual(enforced["sha256"], sha256)
        self.assertEqual(enforced["sizeBytes"], size)
        source = path.read_text(encoding="utf-8")
        self.assertIn(enforced["refusal"], source)
        index = source.find(enforced["refusal"])
        line_start = source.rfind("\n", 0, index) + 1
        self.assertIn("raise", source[line_start:index])

    def test_the_limit_is_compared_against_the_assembled_tree(self):
        """The builder counts what it is about to write, not what it wrote."""
        source = BUILDER_PATH.read_text(encoding="utf-8")
        self.assertIn('recipe["maxEntries"]', source)
        self.assertEqual(self.section["comparedAgainst"], "assembled-entry-set")


class TheCountedTreeTests(unittest.TestCase):
    """One entry count is pinned, and this is where it comes from."""

    def setUp(self):
        self.section = load(RECORD_PATH)["theCountedTree"]
        self.counted = load(COUNTED_PATH)["investigation"]["inventory"]

    def test_the_source_record_is_stamped_as_the_tree_holds_it(self):
        stamp = self.section["source"]
        sha256, size = digest_of(COUNTED_PATH)
        self.assertEqual(stamp["sha256"], sha256)
        self.assertEqual(stamp["sizeBytes"], size)

    def test_every_number_is_quoted_from_that_record(self):
        for field in ("entries", "directories", "files", "symlinks"):
            self.assertEqual(self.section[field], self.counted[field], field)

    def test_the_parts_add_up_to_the_whole(self):
        """A count whose components do not sum is a transcription error."""
        self.assertEqual(
            self.section["entries"],
            self.section["directories"] + self.section["files"] + self.section["symlinks"],
        )

    def test_the_record_says_which_side_of_the_build_was_counted(self):
        self.assertEqual(self.section["whichSide"], "produced-output")
        self.assertTrue(self.section["method"].strip())
        self.assertEqual(self.section["method"], self.counted["method"])


class SubsetProofTests(unittest.TestCase):
    """The bound rests on containment, so containment is recomputed here."""

    def setUp(self):
        self.section = load(RECORD_PATH)["subsetProof"]
        self.boot = load(BOOT_LOCK_PATH)
        self.runtime = load(RUNTIME_LOCK_PATH)

    def test_the_runtime_closure_roots_are_contained_in_the_boot_ones(self):
        boot_names = closure_root_names(self.boot)
        runtime_names = closure_root_names(self.runtime)
        self.assertTrue(runtime_names <= boot_names)
        self.assertEqual(self.section["bootClosureRootCount"], len(boot_names))
        self.assertEqual(self.section["runtimeClosureRootCount"], len(runtime_names))
        self.assertEqual(
            sorted(self.section["closureRootsOnlyInBoot"]),
            sorted(boot_names - runtime_names),
        )
        self.assertEqual(self.section["closureRootsOnlyInRuntime"], [])

    def test_every_runtime_artifact_is_a_boot_artifact(self):
        boot_digests = artifact_digests(self.boot)
        runtime_digests = artifact_digests(self.runtime)
        self.assertTrue(runtime_digests <= boot_digests)
        self.assertEqual(self.section["bootArtifactCount"], len(boot_digests))
        self.assertEqual(self.section["runtimeArtifactCount"], len(runtime_digests))
        self.assertEqual(self.section["runtimeArtifactsAbsentFromBoot"], 0)
        self.assertIs(self.section["runtimeIsContainedInBoot"], True)

    def test_the_two_locks_are_stamped(self):
        for key in ("bootLock", "runtimeLock"):
            stamp = self.section[key]
            path = REPOSITORY_ROOT / stamp["path"]
            sha256, size = digest_of(path)
            self.assertEqual(stamp["sha256"], sha256, key)
            self.assertEqual(stamp["sizeBytes"], size, key)


class TheBoundTests(unittest.TestCase):
    """The headroom is the number a reader would act on, so it is recomputed."""

    def setUp(self):
        self.record = load(RECORD_PATH)
        self.section = self.record["theBound"]

    def test_the_bound_is_the_counted_tree_taken_twice(self):
        counted = self.record["theCountedTree"]["entries"]
        self.assertEqual(self.section["boundedTotalEntries"], counted * 2)
        self.assertEqual(self.section["multiplier"], 2)
        self.assertTrue(self.section["whyTwice"].strip())

    def test_the_headroom_is_the_arithmetic(self):
        limit = self.record["theLimit"]["maxEntries"]
        self.assertEqual(
            self.section["headroomEntries"], limit - self.section["boundedTotalEntries"]
        )
        self.assertIs(
            self.section["fitsUnderTheSealedLimit"],
            self.section["headroomEntries"] > 0,
        )

    def test_the_margin_factor_matches_the_numbers(self):
        limit = self.record["theLimit"]["maxEntries"]
        expected = round(limit / self.section["boundedTotalEntries"], 2)
        self.assertEqual(self.section["marginFactor"], expected)


class BoundNotMeasurementTests(unittest.TestCase):
    """A bound that forgets it is a bound becomes a false claim."""

    def setUp(self):
        self.section = load(RECORD_PATH)["whyThisIsABoundAndNotACount"]

    def test_the_record_keeps_the_two_sides_of_the_build_apart(self):
        self.assertEqual(self.section["limitAppliesTo"], "assembly-input")
        self.assertEqual(self.section["countedNumberDescribes"], "produced-output")
        self.assertIs(self.section["sameNumber"], False)

    def test_the_pre_assembly_check_is_still_required(self):
        """A wide margin is a reason to expect a pass, not to skip the check."""
        self.assertIs(self.section["preAssemblyCheckStillRequired"], True)
        self.assertTrue(self.section["whatWouldMakeThisWrong"].strip())


class CorrectionTests(unittest.TestCase):
    """The earlier over-statement is corrected in place of being quietly dropped."""

    def setUp(self):
        self.section = load(RECORD_PATH)["correctionOfAnEarlierStatement"]

    def test_the_earlier_sentence_is_quoted_from_the_record_that_holds_it(self):
        plan = load(PLAN_PATH)
        self.assertIn(
            self.section["earlierStatement"],
            plan["theByteBudget"]["whatThisDoesNotEstablish"],
        )
        stamp = self.section["earlierRecord"]
        sha256, size = digest_of(PLAN_PATH)
        self.assertEqual(stamp["sha256"], sha256)
        self.assertEqual(stamp["sizeBytes"], size)

    def test_the_correction_says_exactly_which_half_was_wrong(self):
        self.assertTrue(self.section["whatWasOverstated"].strip())
        self.assertTrue(self.section["whatStaysTrue"].strip())
        self.assertIs(self.section["wasAHardStop"], False)
        self.assertIs(self.section["earlierRecordEdited"], False)

    def test_the_earlier_record_still_carries_its_original_sentence(self):
        """Append-only: the superseded claim stays readable where it was made."""
        plan = load(PLAN_PATH)
        self.assertIn(
            "neither tree's entry count is pinned anywhere in the repository",
            " ".join(plan["theByteBudget"]["whatThisDoesNotEstablish"]),
        )


class RegistrationTests(unittest.TestCase):
    def test_the_suite_is_registered_in_self_test(self):
        text = (REPOSITORY_ROOT / "scripts/self-test.sh").read_text(encoding="utf-8")
        self.assertIn(pathlib.Path(__file__).name, text)

    def test_the_record_is_pinned_in_docs_smoke(self):
        text = (REPOSITORY_ROOT / "scripts/docs-smoke.sh").read_text(encoding="utf-8")
        self.assertIn(
            "native-shadow-mac3-nested-runtime-entry-budget-arm64-v1.json", text
        )


if __name__ == "__main__":
    unittest.main()
