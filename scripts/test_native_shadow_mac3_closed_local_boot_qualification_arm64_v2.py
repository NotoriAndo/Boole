"""The successor boot qualification is frozen before the attempt it opens.

The first attempt was made once and failed, and that record is sealed. This is
not a second run of it: it is a separate attempt against a separate image,
opened because the cause of the failure was found and removed, and it carries
its own single allowance rather than reusing one already spent.

What this module holds is the shape the record has to have *before* that
attempt. The six conditions are the first attempt's conditions byte for byte --
a successor that quietly reworded them would be scoring a new run on an easier
exam. The image is named by the digest two replicas independently converged on.
The difference from the failed image is named, and bound to the read-only
comparison that established it rather than to the arithmetic that only agreed
with it. And the first attempt's records are bound by digest here, so this one
cannot outlive an edit to them.
"""

from __future__ import annotations

import hashlib
import json
import pathlib
import unittest

REPO = pathlib.Path(__file__).resolve().parents[1]
CONTAINMENT = REPO / "native/containment"
QUALIFICATION_PATH = (
    CONTAINMENT / "native-shadow-mac3-closed-local-boot-qualification-arm64-v2.json"
)
FIRST_QUALIFICATION_PATH = (
    CONTAINMENT / "native-shadow-mac3-closed-local-boot-qualification-arm64-v1.json"
)
FIRST_RESULT_PATH = (
    CONTAINMENT / "native-shadow-mac3-closed-local-boot-result-arm64-v1.json"
)
GREEN_PATH = (
    CONTAINMENT / "native-shadow-boot-root-disk-determinism-green-arm64-v1.json"
)

# The image the failed attempt froze on, and the image this one names. Written
# out rather than read from the records so that a record editing itself into
# agreement still fails here.
FAILED_ROOT_DISK = "9834036f7738f3848fff23e5c3d1be85cd1f288f7ca43d2094b815eca2b378cc"
FAILED_INITRD = "4674128144befeea20b1cbeb5af340b981b7b125d32d43630c721bb4b0aecab2"
SUCCESSOR_ROOT_DISK = (
    "566614b67ea749ee0061d73aad4e3320f92fe7d352df29d11e4494a8c063d41b"
)
SUCCESSOR_INITRD = "3ae76ced73f180ccd9feb44260694871dde3e158b82bff18d2c23327989488ca"
KERNEL = "d29e317d66517190f6437b9b9bd2cedd26a424fe6da7b1a28451247a13fe1336"
ADDED_DIRECTORIES = ("/dev", "/proc", "/run", "/sys", "/tmp")


def document() -> dict:
    return json.loads(QUALIFICATION_PATH.read_text(encoding="utf-8"))


def first_qualification() -> dict:
    return json.loads(FIRST_QUALIFICATION_PATH.read_text(encoding="utf-8"))


def first_result() -> dict:
    return json.loads(FIRST_RESULT_PATH.read_text(encoding="utf-8"))


def green() -> dict:
    return json.loads(GREEN_PATH.read_text(encoding="utf-8"))


def digest(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def canonical(value) -> str:
    return json.dumps(value, sort_keys=True, ensure_ascii=False)


class PreFreezeTests(unittest.TestCase):
    """The record exists, and says of itself that nothing has been run yet."""

    def test_the_record_is_on_disk_and_parses(self) -> None:
        self.assertTrue(QUALIFICATION_PATH.is_file())
        self.assertIsInstance(document(), dict)

    def test_exactly_one_attempt_is_allowed_and_none_has_been_spent(self) -> None:
        record = document()
        self.assertEqual(record["runsAllowed"], 1)
        self.assertEqual(record["runsPerformed"], 0)

    def test_the_status_does_not_read_as_a_result(self) -> None:
        self.assertIn("NOT-RUN", document()["status"])

    def test_the_conditions_are_frozen_before_the_attempt(self) -> None:
        self.assertEqual(document()["frozenBefore"], "any qualification run")

    def test_no_receipt_for_this_attempt_exists_yet(self) -> None:
        # The driver refuses to start when the receipt path is occupied, so its
        # absence is what makes "not yet run" mechanical rather than asserted.
        receipt = REPO / document()["resultPath"]
        self.assertFalse(receipt.exists(), receipt)

    def test_the_record_carries_no_verdict_of_its_own(self) -> None:
        for absent in ("verdict", "console", "whatFailed", "whatWorked"):
            self.assertNotIn(absent, document())


class SeparateAttemptTests(unittest.TestCase):
    """A successor attempt, not a reopening of the one already spent."""

    def predecessor(self) -> dict:
        return document()["predecessorAttempt"]

    def test_the_first_attempt_is_bound_by_digest_and_left_unchanged(self) -> None:
        for row in document()["appendOnly"]["recordsLeftByteUnchanged"]:
            path = REPO / row["path"]
            self.assertTrue(path.is_file(), row["path"])
            self.assertEqual(digest(path), row["sha256"], row["path"])
            self.assertEqual(path.stat().st_size, row["sizeBytes"], row["path"])

    def test_both_first_attempt_records_are_among_those_bound(self) -> None:
        bound = {row["path"] for row in document()["appendOnly"]["recordsLeftByteUnchanged"]}
        for path in (FIRST_QUALIFICATION_PATH, FIRST_RESULT_PATH):
            self.assertIn(path.relative_to(REPO).as_posix(), bound)

    def test_it_restates_the_first_attempt_as_spent_and_failed(self) -> None:
        predecessor = self.predecessor()
        self.assertEqual(predecessor["runsAllowed"], 1)
        self.assertEqual(predecessor["runsPerformed"], 1)
        self.assertEqual(predecessor["verdict"], "FAIL")
        self.assertFalse(predecessor["rerunPermitted"])

    def test_what_it_says_of_the_first_attempt_matches_that_record(self) -> None:
        # Restating a sealed record is only safe if the restatement is checked.
        sealed = first_result()
        predecessor = self.predecessor()
        self.assertEqual(predecessor["verdict"], sealed["verdict"])
        self.assertEqual(predecessor["runsPerformed"], sealed["runsPerformed"])
        self.assertEqual(predecessor["rerunPermitted"], sealed["rerunPermitted"])

    def test_the_spent_allowance_is_not_reset_or_borrowed(self) -> None:
        predecessor = self.predecessor()
        self.assertFalse(predecessor["resetsTheSpentAttempt"])
        self.assertFalse(predecessor["reusesTheSpentAttempt"])
        self.assertTrue(predecessor["why"].strip())

    def test_this_attempt_has_an_identifier_of_its_own(self) -> None:
        record = document()
        self.assertTrue(record["attemptId"].strip())
        self.assertNotEqual(record["attemptId"], self.predecessor()["attemptId"])

    def test_the_two_attempts_write_to_different_receipts(self) -> None:
        # Sharing a receipt path would make one attempt overwrite the other's
        # evidence, and would make the driver's spent-check answer for the
        # wrong attempt.
        self.assertNotEqual(
            document()["resultPath"],
            FIRST_RESULT_PATH.relative_to(REPO).as_posix(),
        )

    def test_the_first_attempts_receipt_is_still_on_disk(self) -> None:
        # It is what stops the first attempt from being run a second time.
        self.assertTrue(FIRST_RESULT_PATH.is_file())

    def test_the_record_is_not_the_first_records_name_reused(self) -> None:
        record = document()
        first = first_qualification()
        for field in ("record", "release", "schema"):
            self.assertNotEqual(record[field], first[field], field)


class SubjectTests(unittest.TestCase):
    """A different image, named by digest, with the difference accounted for."""

    def subject(self) -> dict:
        return document()["subject"]

    def test_the_root_disk_is_the_successor_image_not_the_failed_one(self) -> None:
        root_disk = self.subject()["rootDisk"]
        self.assertEqual(root_disk["sha256"], SUCCESSOR_ROOT_DISK)
        self.assertNotEqual(root_disk["sha256"], FAILED_ROOT_DISK)

    def test_the_kernel_is_the_same_file_as_before_and_says_so(self) -> None:
        kernel = self.subject()["kernel"]
        self.assertEqual(kernel["sha256"], KERNEL)
        self.assertEqual(kernel["sha256"], first_qualification()["subject"]["kernel"]["sha256"])
        self.assertTrue(kernel["unchangedFromTheFirstAttempt"])

    def test_the_initrd_is_named_moved_and_still_unused_with_a_reason(self) -> None:
        initrd = self.subject()["initrd"]
        self.assertEqual(initrd["sha256"], SUCCESSOR_INITRD)
        self.assertNotEqual(initrd["sha256"], FAILED_INITRD)
        self.assertFalse(initrd["used"])
        self.assertTrue(initrd["whyUnused"].strip())

    def test_the_moved_initrd_digest_is_explained_rather_than_left_open(self) -> None:
        # The fix targeted the root disk. An initrd digest that moved anyway is
        # either explained or it is an unexplained difference.
        self.assertTrue(self.subject()["initrd"]["whyItsDigestMoved"].strip())

    def test_the_digests_are_re_checked_against_the_files_at_boot_time(self) -> None:
        self.assertTrue(self.subject()["verifiedImmediatelyBeforeBoot"])

    def test_the_before_digest_for_the_unchanged_check_is_this_record(self) -> None:
        # One condition compares the image against a frozen digest. Which
        # document holds that digest has to be unambiguous before the run.
        source = self.subject()["digestSourceForTheUnchangedCheck"]
        self.assertEqual(source["path"], QUALIFICATION_PATH.relative_to(REPO).as_posix())
        self.assertEqual(source["field"], "subject.rootDisk.sha256")


class ConvergenceTests(unittest.TestCase):
    """The image is what two independent replicas both arrived at."""

    def convergence(self) -> dict:
        return document()["convergence"]

    def test_two_replicas_produced_it_and_the_phase_ran_once(self) -> None:
        convergence = self.convergence()
        self.assertEqual(convergence["replicas"], 2)
        self.assertEqual(convergence["dispatchCount"], 1)
        self.assertEqual(convergence["producedPerReplica"], 1)
        self.assertTrue(convergence["runId"].strip())

    def test_every_replica_reports_the_subject_digests(self) -> None:
        subject = document()["subject"]
        reports = self.convergence()["perReplicaReport"]
        self.assertEqual(len(reports), 2)
        for report in reports:
            for role in ("initrd", "kernel", "rootDisk"):
                self.assertEqual(report[role]["sha256"], subject[role]["sha256"], role)

    def test_the_filesystem_check_passed_on_both_without_repairing(self) -> None:
        for report in self.convergence()["perReplicaReport"]:
            fsck = report["fsck"]
            self.assertTrue(fsck["ran"])
            self.assertTrue(fsck["passed"])
            self.assertEqual(fsck["exitCode"], 0)
            self.assertFalse(fsck["repairOptionsUsed"])

    def test_no_wall_clock_stamp_survived_in_either_replica(self) -> None:
        for report in self.convergence()["perReplicaReport"]:
            audit = report["timeAudit"]
            self.assertTrue(audit["passed"])
            self.assertEqual(audit["violationCount"], 0)
            self.assertGreater(audit["inodesRead"], 0)

    def test_the_only_difference_between_the_replicas_is_named(self) -> None:
        differences = self.convergence()["unexplainedDifferences"]
        self.assertEqual(differences["count"], 0)
        self.assertTrue(differences["explained"])
        self.assertTrue(differences["theOnlyDifference"].strip())

    def test_the_image_was_checked_by_the_producers_own_verifier(self) -> None:
        for report in self.convergence()["perReplicaReport"]:
            verification = report["verification"]
            self.assertTrue(verification["passed"])
            self.assertFalse(verification["guestBootVerified"])
            self.assertIn("runtime-mount-points-present", verification["checkIds"])


class CauseFixTests(unittest.TestCase):
    """Why this attempt exists, and what evidence closes the difference."""

    def fix(self) -> dict:
        return document()["causeFix"]

    def test_it_names_the_condition_the_first_attempt_failed(self) -> None:
        self.assertEqual(self.fix()["failedCondition"], first_result()["whatFailed"]["condition"])

    def test_it_names_the_five_directories_that_were_added(self) -> None:
        self.assertEqual(tuple(self.fix()["addedPaths"]), ADDED_DIRECTORIES)

    def test_the_missing_paths_the_first_run_found_are_all_covered(self) -> None:
        added = set(self.fix()["addedPaths"])
        for found in first_result()["foundByThisRun"]:
            self.assertIn(found["path"], added, found["path"])

    def test_it_names_the_commit_that_made_the_change(self) -> None:
        self.assertTrue(self.fix()["commit"].strip())

    def test_the_difference_rests_on_a_direct_comparison_not_on_counts(self) -> None:
        # Five more inodes and five more blocks agree with the claim and do not
        # establish it: they survive a changed file body or a moved mode.
        comparison = self.fix()["comparison"]
        self.assertTrue(comparison["runId"].strip())
        self.assertTrue(comparison["readOnly"])
        for side in ("rootDisks", "initrds"):
            verdict = comparison[side]
            self.assertTrue(verdict["ok"], side)
            self.assertEqual(verdict["addedPaths"], list(ADDED_DIRECTORIES), side)
            self.assertEqual(verdict["changedPaths"], [], side)
            self.assertEqual(verdict["removedPaths"], [], side)

    def test_the_comparison_covered_the_fields_a_count_cannot_see(self) -> None:
        compared = self.fix()["comparison"]["fieldsCompared"]
        for field in (
            "contentDigest",
            "extendedAttributes",
            "group",
            "hardLinkGrouping",
            "mode",
            "owner",
            "path",
            "symlinkTarget",
        ):
            self.assertIn(field, compared, field)

    def test_the_initrds_growth_is_accounted_for_byte_by_byte(self) -> None:
        accounting = self.fix()["comparison"]["initrds"]["byteAccounting"]
        self.assertTrue(accounting["balanced"])
        self.assertEqual(
            accounting["afterBytes"] - accounting["beforeBytes"],
            accounting["addedRecordBytes"],
        )
        self.assertTrue(accounting["renumberingConsistent"])

    def test_the_two_images_are_not_claimed_to_be_byte_identical(self) -> None:
        # They cannot be: five directories were added. What is established is
        # that nothing else in either tree differs.
        self.assertFalse(self.fix()["comparison"]["imagesAreByteIdentical"])

    def test_a_comparison_is_not_recorded_as_a_boot(self) -> None:
        comparison = self.fix()["comparison"]
        self.assertFalse(comparison["guestBootVerified"])
        self.assertFalse(comparison["bootableClaim"])


class PassConditionTests(unittest.TestCase):
    """The exam is the first attempt's exam, word for word."""

    def conditions(self) -> list:
        return document()["passConditions"]

    def test_the_six_conditions_are_byte_identical_to_the_first_attempts(self) -> None:
        self.assertEqual(
            canonical(self.conditions()),
            canonical(first_qualification()["passConditions"]),
        )

    def test_the_record_says_where_that_equality_can_be_checked(self) -> None:
        unchanged = document()["passConditionsUnchanged"]
        self.assertEqual(
            unchanged["path"], FIRST_QUALIFICATION_PATH.relative_to(REPO).as_posix()
        )
        self.assertEqual(unchanged["sha256"], digest(FIRST_QUALIFICATION_PATH))
        self.assertTrue(unchanged["identical"])

    def test_every_condition_carries_an_id_and_a_way_to_judge_it(self) -> None:
        for condition in self.conditions():
            self.assertTrue(condition["id"].strip())
            self.assertTrue(condition["condition"].strip())
            self.assertTrue(condition["judgedBy"].strip())

    def test_the_ids_are_distinct(self) -> None:
        ids = [condition["id"] for condition in self.conditions()]
        self.assertEqual(len(ids), len(set(ids)))

    def test_no_condition_carries_a_verdict_yet(self) -> None:
        for condition in self.conditions():
            self.assertNotIn("verdict", condition)

    def test_the_condition_that_failed_before_is_still_required(self) -> None:
        # Dropping it would turn the successor into an easier exam that the
        # first image would also have passed.
        ids = {condition["id"] for condition in self.conditions()}
        self.assertIn(first_result()["whatFailed"]["condition"], ids)

    def test_the_image_must_still_be_byte_unchanged_afterwards(self) -> None:
        ids = {condition["id"] for condition in self.conditions()}
        self.assertIn("sealed-image-unchanged-after-the-run", ids)

    def test_the_transcript_is_still_kept_whatever_the_verdict(self) -> None:
        ids = {condition["id"] for condition in self.conditions()}
        self.assertIn("console-transcript-captured-and-hashed", ids)


class CarriedOverTests(unittest.TestCase):
    """Everything not about the new image is the first attempt's, unchanged."""

    def test_the_kernel_command_line_is_the_same_frozen_string(self) -> None:
        self.assertEqual(
            document()["boot"]["kernelCommandLine"],
            first_qualification()["boot"]["kernelCommandLine"],
        )

    def test_the_isolation_is_the_same_and_still_closed(self) -> None:
        isolation = document()["isolation"]
        self.assertEqual(isolation["networkDevices"], 0)
        self.assertEqual(isolation["sharedDirectories"], 0)
        self.assertEqual(isolation["writableDisksAttached"], 0)
        self.assertFalse(isolation["hostFilesystemExposedToGuest"])
        self.assertTrue(isolation["rootDiskAttachedReadOnly"])

    def test_no_release_identity_is_used(self) -> None:
        signing = document()["signing"]
        self.assertTrue(signing["adHocOnly"])
        self.assertEqual(signing["entitlement"], "com.apple.security.virtualization")
        for forbidden in (
            "teamId",
            "developerIdCertificate",
            "provisioningProfile",
            "notarization",
        ):
            self.assertFalse(signing[forbidden], forbidden)

    def test_the_invariants_carry_across_unchanged(self) -> None:
        self.assertEqual(document()["invariants"], green()["invariants"])

    def test_the_first_attempts_abort_conditions_all_survive(self) -> None:
        carried = set(document()["abortConditions"])
        for condition in first_qualification()["abortConditions"]:
            self.assertIn(condition, carried, condition)

    def test_it_aborts_rather_than_reaching_for_the_spent_attempt(self) -> None:
        aborts = " ".join(document()["abortConditions"]).lower()
        self.assertIn("spent", aborts)

    def test_what_gets_recorded_regardless_of_verdict_is_unchanged(self) -> None:
        self.assertEqual(
            canonical(document()["recordRegardlessOfVerdict"]),
            canonical(first_qualification()["recordRegardlessOfVerdict"]),
        )


class KnownGapTests(unittest.TestCase):
    """What is still missing is written down before, not explained after."""

    def gaps(self) -> list:
        return document()["knownAbsentBeforeTheRun"]

    def test_each_gap_names_what_is_missing_and_what_follows_from_it(self) -> None:
        self.assertTrue(self.gaps())
        for gap in self.gaps():
            self.assertTrue(gap["what"].strip())
            self.assertTrue(gap["path"].strip())
            self.assertTrue(gap["consequence"].strip())

    def test_the_gaps_from_the_first_attempt_are_all_still_listed(self) -> None:
        # The fix added five directories and nothing else, so nothing that was
        # missing before has been filled in.
        carried = {gap["path"] for gap in self.gaps()}
        for gap in first_qualification()["knownAbsentBeforeTheRun"]:
            self.assertIn(gap["path"], carried, gap["path"])

    def test_each_gap_says_how_it_was_re_checked_against_the_new_image(self) -> None:
        for gap in self.gaps():
            self.assertTrue(gap["stillAbsentInTheSuccessorImage"])
            self.assertTrue(gap["how"].strip())

    def test_no_gap_is_listed_as_a_pass_condition(self) -> None:
        conditions = " ".join(
            condition["condition"] for condition in document()["passConditions"]
        )
        for gap in self.gaps():
            self.assertNotIn(gap["path"], conditions)


class BoundaryTests(unittest.TestCase):
    """A boot is not a product, and this record refuses to be read as one."""

    def test_the_record_claims_no_boot_and_no_activation_before_running(self) -> None:
        record = document()
        self.assertFalse(record["bootableClaim"])
        self.assertFalse(record["activationAllowed"])
        self.assertFalse(record["boundaries"]["guestBootVerified"])

    def test_a_pass_would_not_reopen_the_release_gates(self) -> None:
        not_established = " ".join(document()["notEstablishedByAPass"]).lower()
        for gate in ("launcher", "curl.3", "clean-mac", "release", "public mining", "activation"):
            self.assertIn(gate, not_established)

    def test_the_first_attempts_boundaries_are_not_narrowed(self) -> None:
        self.assertEqual(
            canonical(document()["notEstablishedByAPass"]),
            canonical(first_qualification()["notEstablishedByAPass"]),
        )


class PredecessorRecordTests(unittest.TestCase):
    """Every authority this record leans on is bound by digest."""

    def test_each_predecessor_is_on_disk_at_the_digest_named(self) -> None:
        predecessors = document()["predecessorRecords"]
        self.assertTrue(predecessors)
        for row in predecessors:
            path = REPO / row["path"]
            self.assertTrue(path.is_file(), row["path"])
            self.assertEqual(digest(path), row["sha256"], row["path"])
            self.assertTrue(row["role"].strip(), row["path"])

    def test_the_sealed_failure_and_the_production_authority_are_among_them(self) -> None:
        paths = {row["path"] for row in document()["predecessorRecords"]}
        for name in (
            "native-shadow-boot-root-disk-determinism-hard-stop-arm64-v1.json",
            "native-shadow-boot-root-disk-determinism-successor-authority-arm64-v1.json",
            "native-shadow-boot-rootfs-runtime-mount-points-arm64-v1.json",
        ):
            self.assertIn("native/containment/" + name, paths, name)


class GateTests(unittest.TestCase):
    """The record and this module are held by the gates that run on every push."""

    def test_the_record_is_pinned_by_the_docs_gate(self) -> None:
        smoke = (REPO / "scripts" / "docs-smoke.sh").read_text(encoding="utf-8")
        self.assertIn(QUALIFICATION_PATH.relative_to(REPO).as_posix(), smoke)

    def test_this_module_stays_registered_in_the_self_test(self) -> None:
        self_test = (REPO / "scripts" / "self-test.sh").read_text(encoding="utf-8")
        self.assertIn(pathlib.Path(__file__).name, self_test)


if __name__ == "__main__":
    unittest.main()
