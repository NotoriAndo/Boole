"""The successor image production criteria, checked while they are still unrun.

This file exists to make one thing mechanical: that the conditions a produced
image would be judged against were written before any image existed. While the
record is unrun, that is checkable directly -- the result path is empty, the
count is zero, and the status says so. Once a run happens, this file does not
get edited to match; the outcome goes to the separate result record, and the
assertions here move to the form that survives the run, which is that the one
allowed attempt cannot be spent twice.

The other thing enforced here is that the record does not quietly grow into a
claim. Producing an image is not booting it, booting is not serving, and none of
the three is activation. The record has to keep saying that in its own fields.

Expected values are written out literally rather than read back from the record.
A record that agrees with itself proves nothing.
"""

import hashlib
import json
import pathlib
import unittest

REPO = pathlib.Path(__file__).resolve().parents[1]
CRITERIA = (
    REPO
    / "native/containment/native-shadow-mac3-successor-image-production-criteria-arm64-v1.json"
)
INPUTS = REPO / "native/containment/native-shadow-mac3-guest-runtime-inputs-arm64-v1.json"
CONTRACT = REPO / "native/containment/native-shadow-mac3-guest-runtime-contract-arm64-v1.json"

ATTEMPT_ID = "MAC3-SUCCESSOR-IMAGE-PRODUCTION-ARM64-ATTEMPT-1"
RESULT_PATH = "native/containment/native-shadow-mac3-successor-image-production-result-arm64-v1.json"

CONDITION_IDS = (
    "exactly-one-production-pair",
    "both-replicas-agree-byte-for-byte",
    "the-root-disk-passes-a-read-only-check",
    "the-seven-inputs-are-in-the-image-at-their-frozen-digests",
    "the-runtime-rootfs-and-its-manifest-are-in-the-image",
    "the-launcher-binary-is-unchanged",
    "nothing-secret-and-nothing-connected-is-in-the-image",
)

ABORT_IDS = (
    "result-file-already-exists",
    "replicas-disagree",
    "criteria-would-have-to-be-loosened",
    "input-drift",
    "isolation-would-have-to-be-bypassed",
)

STAGED_GUEST_PATHS = (
    "/etc/passwd",
    "/etc/group",
    "/etc/shadow",
    "/etc/gshadow",
    "/etc/nsswitch.conf",
    "/usr/lib/systemd/system/boole-native-shadow-launcher.service",
    "/usr/lib/tmpfiles.d/boole-native-shadow.conf",
)


def document():
    return json.loads(CRITERIA.read_text(encoding="utf-8"))


def digest(relative):
    return hashlib.sha256((REPO / relative).read_bytes()).hexdigest()


class FrozenBeforeAnythingRanTests(unittest.TestCase):
    """While it is unrun, unrun is a fact on the filesystem rather than a word."""

    def test_the_record_exists_and_parses(self) -> None:
        self.assertTrue(CRITERIA.exists(), CRITERIA)
        self.assertEqual(
            document()["schema"],
            "boole.native-shadow.mac3-successor-image-production-criteria.arm64.v1",
        )

    def test_it_says_it_was_frozen_before_a_production(self) -> None:
        self.assertEqual(document()["frozenBefore"], "any successor image production")
        self.assertEqual(
            document()["status"],
            "MAC3-SUCCESSOR-IMAGE-PRODUCTION-CRITERIA-PRE-FROZEN-NOT-RUN",
        )

    def test_exactly_one_attempt_is_allowed_and_none_is_spent(self) -> None:
        self.assertEqual(document()["runsAllowed"], 1)
        self.assertEqual(document()["runsPerformed"], 0)

    def test_the_attempt_has_an_identity_of_its_own(self) -> None:
        self.assertEqual(document()["attemptId"], ATTEMPT_ID)

    def test_the_result_path_is_named_and_still_empty(self) -> None:
        # The count is read off the filesystem rather than taken on trust from
        # this record: an occupied result path is what a driver refuses on.
        self.assertEqual(document()["resultPath"], RESULT_PATH)
        self.assertFalse((REPO / RESULT_PATH).exists(), RESULT_PATH)

    def test_it_carries_no_verdict(self) -> None:
        for absent in ("verdict", "passed", "result", "replicaDigests", "producedFiles"):
            with self.subTest(field=absent):
                self.assertNotIn(absent, document())


class ConditionsTests(unittest.TestCase):
    """Seven conditions, each with the check that judges it."""

    def test_the_seven_ids_are_exactly_the_frozen_set(self) -> None:
        ids = [row["id"] for row in document()["productionConditions"]]
        self.assertEqual(sorted(ids), sorted(CONDITION_IDS))
        self.assertEqual(len(ids), len(set(ids)))

    def test_every_condition_names_what_judges_it(self) -> None:
        for row in document()["productionConditions"]:
            with self.subTest(id=row["id"]):
                self.assertTrue(row["condition"].strip())
                self.assertGreater(len(row["judgedBy"]), 60, row["id"])

    def test_determinism_is_judged_on_all_three_files(self) -> None:
        row = {r["id"]: r for r in document()["productionConditions"]}[
            "both-replicas-agree-byte-for-byte"
        ]
        for name in ("kernel", "initrd", "root disk"):
            with self.subTest(name=name):
                self.assertIn(name, row["condition"])

    def test_the_read_only_check_proves_it_wrote_nothing(self) -> None:
        row = {r["id"]: r for r in document()["productionConditions"]}[
            "the-root-disk-passes-a-read-only-check"
        ]
        self.assertIn("unchanged", row["judgedBy"])

    def test_the_manifest_condition_asks_for_equality_not_presence(self) -> None:
        row = {r["id"]: r for r in document()["productionConditions"]}[
            "the-runtime-rootfs-and-its-manifest-are-in-the-image"
        ]
        self.assertIn("equal", row["judgedBy"])
        self.assertIn("not enough", row["judgedBy"])


class AbortConditionsTests(unittest.TestCase):
    """Stopping is a listed outcome, and loosening the criteria is one of them."""

    def test_the_five_abort_ids_are_exactly_the_frozen_set(self) -> None:
        ids = [row["id"] for row in document()["abortConditions"]]
        self.assertEqual(sorted(ids), sorted(ABORT_IDS))

    def test_loosening_the_criteria_is_itself_an_abort(self) -> None:
        row = {r["id"]: r for r in document()["abortConditions"]}[
            "criteria-would-have-to-be-loosened"
        ]
        self.assertIn("reworded", row["abortIf"])
        self.assertIn("waived", row["abortIf"])

    def test_a_disagreement_between_replicas_stops_rather_than_retries(self) -> None:
        row = {r["id"]: r for r in document()["abortConditions"]}["replicas-disagree"]
        self.assertIn("Re-running", row["why"])

    def test_every_abort_says_why(self) -> None:
        for row in document()["abortConditions"]:
            with self.subTest(id=row["id"]):
                self.assertGreater(len(row["why"]), 30, row["id"])


class StagedInputsTests(unittest.TestCase):
    """The criteria are about a tree, and they are checked against that tree."""

    def test_the_seven_guest_paths_are_the_ones_the_input_set_named(self) -> None:
        rows = document()["stagedInputs"]
        self.assertEqual(sorted(row["guestPath"] for row in rows), sorted(STAGED_GUEST_PATHS))

    def test_every_digest_is_recomputed_from_the_file_it_names(self) -> None:
        for row in document()["stagedInputs"]:
            with self.subTest(path=row["path"]):
                self.assertEqual(row["sha256"], digest(row["path"]))
                self.assertEqual(row["sizeBytes"], (REPO / row["path"]).stat().st_size)

    def test_each_digest_matches_the_one_the_input_record_froze(self) -> None:
        sealed = {
            row["path"]: row["sha256"]
            for row in json.loads(INPUTS.read_text(encoding="utf-8"))["inputs"]
        }
        for row in document()["stagedInputs"]:
            with self.subTest(path=row["path"]):
                self.assertEqual(row["sha256"], sealed[row["path"]])

    def test_everything_is_staged_read_only_and_owned_by_root(self) -> None:
        for row in document()["stagedInputs"]:
            with self.subTest(path=row["path"]):
                self.assertEqual(row["mode"], "0444")
                self.assertEqual(row["uid"], 0)
                self.assertEqual(row["gid"], 0)

    def test_every_predecessor_record_is_still_at_its_stated_digest(self) -> None:
        rows = document()["predecessorRecords"]["leftByteUnchanged"]
        self.assertGreaterEqual(len(rows), 8)
        for row in rows:
            with self.subTest(path=row["path"]):
                self.assertEqual(row["sha256"], digest(row["path"]))


class NotAClaimTests(unittest.TestCase):
    """Producing is not booting, booting is not serving, none of it is activation."""

    def test_no_serving_claim(self) -> None:
        self.assertIs(document()["servingClaim"], False)

    def test_no_bootable_claim(self) -> None:
        self.assertIs(document()["bootableClaim"], False)

    def test_activation_stays_disallowed(self) -> None:
        self.assertIs(document()["activationAllowed"], False)

    def test_what_a_pass_would_not_establish_is_written_down(self) -> None:
        joined = " ".join(document()["notEstablishedByAPass"])
        for phrase in ("serves", "held", "MAC.4", "activation"):
            with self.subTest(phrase=phrase):
                self.assertIn(phrase, joined)

    def test_a_failure_is_recorded_rather_than_discarded(self) -> None:
        self.assertIn("whether it passes or fails", document()["recordRegardlessOfVerdict"])

    def test_the_boundaries_still_name_what_is_not_being_done(self) -> None:
        joined = " ".join(document()["boundaries"])
        for phrase in ("wallet", "API key", "network device", "public mining"):
            with self.subTest(phrase=phrase):
                self.assertIn(phrase, joined)


class WhatIsStillMissingTests(unittest.TestCase):
    """Freezing criteria does not make the dispatch possible, and says so."""

    def test_the_unmet_prerequisites_are_listed_rather_than_implied(self) -> None:
        rows = document()["stillRequiredBeforeADispatch"]
        self.assertGreaterEqual(len(rows), 2)
        states = {row["requirement"]: row["state"] for row in rows}
        self.assertTrue(any(state == "not done" for state in states.values()), states)

    def test_the_builder_is_named_as_not_yet_staging_the_inputs(self) -> None:
        joined = " ".join(
            row["requirement"] + " " + row["why"]
            for row in document()["stillRequiredBeforeADispatch"]
        )
        self.assertIn("builder", joined)
        self.assertIn("source lock", joined)

    def test_the_builder_really_does_not_stage_them_yet(self) -> None:
        # The record claims the staging table names four files and none of them
        # is the account database. That claim is checked against the builder.
        builder = (
            REPO / "scripts/native_shadow_rootfs_builder_boot_arm64_v1.py"
        ).read_text(encoding="utf-8")
        self.assertNotIn("native/etc/passwd", builder)
        self.assertNotIn("boole-native-shadow-launcher-v2.service", builder)


class TheSuccessorChainTests(unittest.TestCase):
    """The cost of staging the inputs is surveyed against the tree, not asserted."""

    def test_the_four_steps_are_ordered_without_ties(self) -> None:
        steps = document()["successorChainForStaging"]["steps"]
        self.assertEqual([row["order"] for row in steps], [1, 2, 3, 4])

    def test_every_historical_step_keeps_the_identity_recorded_before_the_run(self) -> None:
        observed = [
            (row["order"], row["path"], row["sha256"], row["sizeBytes"])
            for row in document()["successorChainForStaging"]["steps"]
        ]
        self.assertEqual(
            observed,
            [
                (
                    1,
                    "native/containment/native-shadow-boot-rootfs-source-lock-plan-arm64-v1.json",
                    "c047c20144167a4f28f222c4026a33e2d70b89340ee13cba79c207b7c92dc583",
                    14_099,
                ),
                (
                    2,
                    "scripts/native_shadow_boot_rootfs_source_lock_arm64_v1.py",
                    "02cc8917c19a7f07810645cde70cf388e7a9ed7dd1b0814028fbcf9ae407577a",
                    25_470,
                ),
                (
                    3,
                    "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v1.json",
                    "9eb70e05e0daf8cc56c0741c5c8ca266cad819d059ca28bcadeaecf84c0531cf",
                    357_104,
                ),
                (
                    4,
                    "scripts/native_shadow_rootfs_builder_boot_arm64_v1.py",
                    "a5dd54198878473c162ec306fbccd6edac8b22f036d9cf84d244b5f010f96d87",
                    37_435,
                ),
            ],
        )

    def test_the_builder_is_changed_last(self) -> None:
        steps = {row["order"]: row["path"] for row in document()["successorChainForStaging"]["steps"]}
        self.assertEqual(steps[4], "scripts/native_shadow_rootfs_builder_boot_arm64_v1.py")

    def test_the_plan_really_lists_ten_tracked_files_today(self) -> None:
        # The chain says ten rows grow to seventeen. Ten is checked rather than
        # trusted, because a claim about a file is only worth as much as the
        # file agrees with it.
        plan = json.loads(
            (
                REPO
                / "native/containment/native-shadow-boot-rootfs-source-lock-plan-arm64-v1.json"
            ).read_text(encoding="utf-8")
        )
        self.assertEqual(len(plan["trackedFiles"]), 10)
        self.assertEqual(len(plan["authorityBindings"]), 10)
        self.assertEqual(plan["expected"]["trackedFileCount"], 10)

    def test_the_builders_own_digest_really_is_computed_rather_than_pinned(self) -> None:
        # Step 4 claims extending the staging table invalidates no pin because
        # the module hashes itself at import. If that ever became a literal, the
        # claim would be false and this fails.
        builder = (
            REPO / "scripts/native_shadow_rootfs_builder_boot_arm64_v1.py"
        ).read_text(encoding="utf-8")
        self.assertIn("BOOT_PROJECTION_SHA256 = hashlib.sha256(", builder)

    def test_surveying_the_chain_is_not_walking_it(self) -> None:
        self.assertIn("not walking it", document()["successorChainForStaging"]["whatThisIsNot"])

    def test_the_no_redownload_finding_was_demonstrated_not_inferred(self) -> None:
        shown = document()["successorChainForStaging"]["noRedownloadDemonstrated"]
        for field in ("claim", "howItWasChecked", "whyItMatters", "whatItDoesNotShow"):
            with self.subTest(field=field):
                self.assertGreater(len(shown[field]), 40, field)

    def test_the_generator_really_has_no_network_code(self) -> None:
        # The demonstration rests on the generator being unable to fetch, so
        # that is checked against the generator rather than taken from the
        # record. A future import of any of these makes the claim false.
        source = (
            REPO / "scripts/native_shadow_boot_rootfs_source_lock_arm64_v1.py"
        ).read_text(encoding="utf-8")
        for module in ("urllib", "requests", "socket", "subprocess"):
            with self.subTest(module=module):
                self.assertNotIn(module, source)

    def test_the_lock_digest_it_reproduced_is_the_one_in_the_tree(self) -> None:
        shown = document()["successorChainForStaging"]["noRedownloadDemonstrated"]
        self.assertIn(
            digest("native/containment/native-shadow-boot-rootfs-source-lock-arm64-v1.json"),
            shown["howItWasChecked"],
        )

    def test_the_demonstration_states_what_it_does_not_show(self) -> None:
        shown = document()["successorChainForStaging"]["noRedownloadDemonstrated"]
        self.assertIn("not that a different pair would pass", shown["whatItDoesNotShow"])

    def test_the_pin_finding_was_demonstrated_not_inferred(self) -> None:
        shown = document()["successorChainForStaging"]["pinsSurviveStagingDemonstrated"]
        for field in ("claim", "howItWasChecked", "whyItMatters", "whatItDoesNotShow"):
            with self.subTest(field=field):
                self.assertGreater(len(shown[field]), 40, field)

    def test_the_builder_it_restored_is_the_builder_in_the_tree(self) -> None:
        # The probe is only honest if the file came back. The digest the record
        # says it restored to is compared against the builder as it stands.
        shown = document()["successorChainForStaging"]["pinsSurviveStagingDemonstrated"]
        self.assertIn(
            digest("scripts/native_shadow_rootfs_builder_boot_arm64_v1.py"),
            shown["howItWasChecked"],
        )

    def test_the_check_that_refused_the_probe_still_exists(self) -> None:
        # The finding names one check as the thing that refuses. If that check
        # were renamed or deleted, the record would describe a tree that no
        # longer exists.
        tests = (
            REPO / "scripts/test_native_shadow_rootfs_builder_boot_arm64_v1.py"
        ).read_text(encoding="utf-8")
        self.assertIn("def test_the_authority_table_covers_every_tracked_file_in_the_boot_lock", tests)

    def test_the_probe_states_that_one_entry_is_not_seven(self) -> None:
        shown = document()["successorChainForStaging"]["pinsSurviveStagingDemonstrated"]
        self.assertIn("not that nothing else refuses later", shown["whatItDoesNotShow"])

    def test_deferring_the_chain_is_a_recorded_decision_not_a_silence(self) -> None:
        # Not walking the chain could mean "decided against it" or "ran out of
        # time and said nothing". Those read identically in a tree unless the
        # reason is written down, so the reason is a required field.
        why = document()["successorChainForStaging"]["whyItIsNotWalkedYet"]
        for field in ("decision", "reasoning", "consequence", "whatWouldChangeIt"):
            with self.subTest(field=field):
                self.assertGreater(len(why[field]), 40, field)

    def test_the_deferral_names_the_condition_a_partial_walk_would_fail(self) -> None:
        why = document()["successorChainForStaging"]["whyItIsNotWalkedYet"]
        self.assertIn("runtime rootfs", why["reasoning"])
        self.assertIn("one allowed run", why["reasoning"])

    def test_the_deferral_says_what_would_reverse_it(self) -> None:
        # A deferral with no stated reversal condition is indistinguishable from
        # an excuse, so the record has to name what would make it wrong.
        why = document()["successorChainForStaging"]["whyItIsNotWalkedYet"]
        self.assertIn("plan", why["whatWouldChangeIt"])
        self.assertIn("lock", why["whatWouldChangeIt"])

    def test_the_deferral_claims_neither_thing_is_done(self) -> None:
        why = document()["successorChainForStaging"]["whyItIsNotWalkedYet"]
        self.assertIn("neither is started here", why["consequence"])


class TheGapAndTheHeldConditionSurviveTests(unittest.TestCase):
    """Neither of the two open questions is closed by freezing these criteria."""

    def test_the_runtime_rootfs_gap_is_still_open_in_the_contract(self) -> None:
        contract = json.loads(CONTRACT.read_text(encoding="utf-8"))
        gaps = [row["path"] for row in contract["gaps"]]
        self.assertIn("/var/lib/boole/native-shadow/runtime-rootfs", gaps)

    def test_the_held_condition_is_still_held(self) -> None:
        contract = json.loads(CONTRACT.read_text(encoding="utf-8"))
        held = contract["heldCondition"]
        self.assertIs(held["relaxed"], False)
        self.assertIs(held["waived"], False)
        self.assertIs(held["satisfied"], False)

    def test_the_input_record_still_reads_as_not_built(self) -> None:
        inputs = json.loads(INPUTS.read_text(encoding="utf-8"))
        self.assertIs(inputs["imageProduced"], False)
        self.assertIs(inputs["servingClaim"], False)


if __name__ == "__main__":
    unittest.main()
