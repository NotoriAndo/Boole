#!/usr/bin/env python3
"""Tests for the root-disk determinism successor authority.

The predecessor record says the two arm64 replicas produced different root disks
and says why: `E2FSPROGS_FAKE_TIME=0` is the frozen library's unset sentinel, so
the pin never took effect and every stamping site read the clock instead.  This
record is the bar the fix has to clear, written down before the fix exists.

That ordering is the whole point, so most of what is tested here is that the bar
cannot move.  The acceptance criterion stays byte identity.  The allowed
timestamps stay a closed set, because a closed set cannot be widened afterwards
to admit whatever came out.  `e2fsck` stays read-only with a forced check and a
single accepted exit code.  A mismatch stays a stop rather than a retry.

The rest is anti-drift.  The predecessor is bound by its own digest, so this
record cannot outlive an edit to it.  The disassembly's coverage is checked
against the predecessor's list of fields that actually differed, so the proof
cannot claim more or less than the failure it answers.  And the record is
checked for copies of digests that are already pinned elsewhere: a second copy
of a sealed fact is a fact that can drift silently.
"""

from __future__ import annotations

import hashlib
import json
import pathlib
import re
import unittest

from scripts import native_shadow_boot_initrd_arm64_v1 as initrd
from scripts import native_shadow_boot_root_disk_arm64_v1 as root_disk


REPO = pathlib.Path(__file__).resolve().parents[1]
RECORD_PATH = REPO / (
    "native/containment/"
    "native-shadow-boot-root-disk-determinism-successor-authority-arm64-v1.json"
)

SHA256_LITERAL = re.compile(r"\b[0-9a-f]{64}\b")


def document() -> dict:
    return json.loads(RECORD_PATH.read_text(encoding="utf-8"))


def predecessor() -> dict:
    path = REPO / document()["predecessor"]["path"]
    return json.loads(path.read_text(encoding="utf-8"))


def live_writer_time() -> str:
    return root_disk.mke2fs_env(config="/x")[root_disk.FAKE_TIME_ENV]


class PreRegistrationTests(unittest.TestCase):
    def test_the_record_changed_no_production_code_and_produced_nothing(self) -> None:
        pre = document()["preRegistration"]
        self.assertFalse(pre["productionCodeChangedByThisRecord"])
        self.assertEqual(pre["producedArtifactsByThisRecord"], 0)

    def test_the_pre_registration_refuses_to_be_amended(self) -> None:
        pre = document()["preRegistration"]
        self.assertTrue(pre["amendForbidden"])
        self.assertTrue(pre["followUpCommitsAllowedOnSameBranch"])

    def test_nothing_here_claims_a_boot_or_an_activation(self) -> None:
        record = document()
        self.assertFalse(record["activationAllowed"])
        self.assertFalse(record["bootableClaim"])
        for value in record["boundaries"].values():
            self.assertFalse(value)


class PredecessorTests(unittest.TestCase):
    def test_the_predecessor_is_bound_by_the_digest_of_the_file_on_disk(self) -> None:
        bound = document()["predecessor"]
        path = REPO / bound["path"]
        raw = path.read_bytes()
        self.assertEqual(hashlib.sha256(raw).hexdigest(), bound["sha256"])
        self.assertEqual(len(raw), bound["sizeBytes"])

    def test_the_bound_status_is_the_status_the_predecessor_actually_carries(self) -> None:
        self.assertEqual(document()["predecessor"]["status"], predecessor()["status"])

    def test_the_predecessor_may_not_be_edited_or_reread_as_a_success(self) -> None:
        bound = document()["predecessor"]
        self.assertTrue(bound["mustNotBeModified"])
        self.assertTrue(bound["mustNotBeReinterpretedAsSuccess"])

    def test_every_file_promised_byte_unchanged_exists_to_be_unchanged(self) -> None:
        for relative in document()["predecessor"]["sealedFilesThatStayByteUnchanged"]:
            self.assertTrue((REPO / relative).is_file(), relative)

    def test_the_predecessor_record_is_among_the_files_that_stay_sealed(self) -> None:
        bound = document()["predecessor"]
        self.assertIn(bound["path"], bound["sealedFilesThatStayByteUnchanged"])


class TimeDesignTests(unittest.TestCase):
    def test_the_source_epoch_is_the_epoch_the_staged_entries_actually_carry(self) -> None:
        source = document()["time"]["canonicalSourceEpoch"]
        self.assertEqual(source["value"], initrd.CANONICAL_MTIME)
        self.assertFalse(source["changedBySuccessor"])

    def test_the_writer_time_names_the_variable_the_producer_sets(self) -> None:
        self.assertEqual(
            document()["time"]["ext4WriterTime"]["variable"], root_disk.FAKE_TIME_ENV
        )

    def test_the_producer_holds_one_of_the_two_values_the_record_names(self) -> None:
        """Before the fix it is the sentinel, after it the pin.  Never a third thing."""

        writer = document()["time"]["ext4WriterTime"]
        self.assertIn(
            live_writer_time(), {writer["currentValue"], writer["successorValue"]}
        )

    def test_the_successor_value_is_fixed_and_is_not_the_sentinel(self) -> None:
        writer = document()["time"]["ext4WriterTime"]
        self.assertNotEqual(writer["successorValue"], writer["currentValue"])
        self.assertNotEqual(writer["successorValue"], "0")
        self.assertGreater(int(writer["successorValue"]), 0)
        self.assertTrue(writer["isNotAWallClock"])

    def test_the_two_times_are_separate_fields_and_separate_numbers(self) -> None:
        time = document()["time"]
        self.assertNotEqual(
            str(time["canonicalSourceEpoch"]["value"]),
            time["ext4WriterTime"]["successorValue"],
        )

    def test_the_substitutions_that_would_hide_the_bug_are_refused(self) -> None:
        refused = {row["value"] for row in document()["time"]["forbiddenSubstitutions"]}
        self.assertIn("SOURCE_DATE_EPOCH", refused)
        self.assertIn("the current time", refused)
        self.assertIn("0", refused)
        for row in document()["time"]["forbiddenSubstitutions"]:
            self.assertTrue(row["why"].strip())

    def test_the_inspector_is_not_promoted_to_a_writer(self) -> None:
        self.assertTrue(document()["time"]["debugfsMustNotBecomeAWriter"]["required"])


class MechanicalPreVerificationTests(unittest.TestCase):
    def test_the_proof_was_static_and_ran_before_any_result(self) -> None:
        proof = document()["time"]["mechanicalPreVerification"]
        self.assertIn("objdump", proof["method"])
        self.assertIn("nothing was executed", proof["method"])
        self.assertEqual(proof["performedBefore"], "any production change, any build, any result")

    def test_every_field_that_differed_is_accounted_for(self) -> None:
        """The proof answers the failure, so its field list is the failure's field list."""

        coverage = document()["time"]["mechanicalPreVerification"]["coverage"]
        differed = set(predecessor()["investigation"]["byteDiff"]["byField"])
        self.assertEqual(set(coverage["fieldsThatDifferedBetweenReplicas"]), differed)
        self.assertEqual(
            set(coverage["fieldsTracedToFsNow"]) | set(coverage["fieldsDerivedFromThose"]),
            differed,
        )
        self.assertEqual(coverage["fieldsLeftUnexplained"], 0)

    def test_the_traced_and_derived_fields_do_not_overlap(self) -> None:
        coverage = document()["time"]["mechanicalPreVerification"]["coverage"]
        self.assertEqual(
            set(coverage["fieldsTracedToFsNow"]) & set(coverage["fieldsDerivedFromThose"]),
            set(),
        )

    def test_every_field_traced_to_the_clock_has_a_disassembly_site(self) -> None:
        proof = document()["time"]["mechanicalPreVerification"]
        sites = {row["field"] for row in proof["sites"]}
        for field in proof["coverage"]["fieldsTracedToFsNow"]:
            self.assertIn(field, sites, field)

    def test_every_site_says_which_binary_and_which_address(self) -> None:
        for row in document()["time"]["mechanicalPreVerification"]["sites"]:
            self.assertTrue(row["field"].strip())
            self.assertIn(row["binary"], {"mke2fs", "libext2fs.so.2.4"})
            self.assertTrue(row["address"].startswith("0x"))
            self.assertTrue(row["instruction"].strip())

    def test_the_inode_sites_name_the_offsets_the_predecessor_measured(self) -> None:
        offsets = {
            row["field"]: row["inodeOffset"]
            for row in document()["time"]["mechanicalPreVerification"]["sites"]
            if "inodeOffset" in row
        }
        self.assertEqual(offsets["i_atime"], "0x8")
        self.assertEqual(offsets["i_ctime"], "0xc")
        self.assertEqual(offsets["i_mtime"], "0x10")
        self.assertEqual(offsets["i_crtime"], "0x90")

    def test_the_proof_does_not_claim_the_images_will_match(self) -> None:
        limit = document()["time"]["mechanicalPreVerification"]["coverage"]["claimLimit"]
        self.assertIn("does not say", limit)


class LoaderEvidenceTests(unittest.TestCase):
    def test_the_evidence_is_the_value_the_executor_already_computes(self) -> None:
        emit = document()["loaderEvidenceContract"]["emitDoNotRecompute"]
        self.assertTrue(emit["required"])
        self.assertIn("resolved_libraries()", emit["existingComputation"])

    def test_the_function_it_names_is_the_function_that_exists(self) -> None:
        module = REPO / "scripts/native_shadow_boot_root_disk_execute_arm64_v1.py"
        self.assertIn("def resolved_libraries(", module.read_text(encoding="utf-8"))

    def test_paths_and_digests_are_both_required(self) -> None:
        required = " ".join(document()["loaderEvidenceContract"]["requiredFields"])
        self.assertIn("absolute path", required)
        self.assertIn("sha256", required)

    def test_a_library_from_outside_the_frozen_closure_is_a_stop(self) -> None:
        stops = " ".join(document()["loaderEvidenceContract"]["hardStopConditions"])
        self.assertIn("outside the frozen extraction tree", stops)
        self.assertIn("runner default library", stops)

    def test_the_two_replicas_must_agree_on_their_loader_evidence(self) -> None:
        stops = " ".join(document()["loaderEvidenceContract"]["hardStopConditions"])
        self.assertIn("two replicas", stops)

    def test_missing_evidence_is_not_a_pass(self) -> None:
        self.assertTrue(document()["loaderEvidenceContract"]["notRunIsNotAPass"])


class FilesystemCheckTests(unittest.TestCase):
    def test_the_check_is_forced_and_read_only(self) -> None:
        argv = document()["e2fsckContract"]["argv"]
        self.assertIn("-f", argv)
        self.assertIn("-n", argv)

    def test_a_check_that_checks_nothing_is_refused_in_writing(self) -> None:
        note = document()["e2fsckContract"]["argvNotes"]["-f"]
        self.assertIn("without it", note.lower())

    def test_no_repair_option_is_permitted(self) -> None:
        contract = document()["e2fsckContract"]
        for option in ("-p", "-y", "-a"):
            self.assertIn(option, contract["forbiddenOptions"])
            self.assertNotIn(option, contract["argv"])

    def test_only_a_clean_exit_counts_as_clean(self) -> None:
        normal = document()["e2fsckContract"]["normalExit"]
        self.assertEqual(normal["acceptedExitCodes"], [0])
        self.assertEqual(normal["preRegisteredBefore"], "any run")
        self.assertIn("4", normal["rejected"])

    def test_the_check_runs_once_per_replica_and_absence_is_a_failure(self) -> None:
        contract = document()["e2fsckContract"]
        self.assertEqual(contract["runsPerReplica"], 1)
        self.assertTrue(contract["notRunIsNotAPass"])
        self.assertTrue(contract["absentResultIsAFailure"])

    def test_the_binary_is_pinned_by_size_and_digest(self) -> None:
        binary = document()["e2fsckContract"]["binary"]
        self.assertTrue(SHA256_LITERAL.fullmatch(binary["sha256"]))
        self.assertGreater(binary["sizeBytes"], 0)
        self.assertTrue(binary["firstPinnedHere"])

    def test_the_checker_needs_no_library_the_plan_does_not_already_carry(self) -> None:
        planned = {row["soname"] for row in root_disk.SHARED_LIBRARIES}
        binary = document()["e2fsckContract"]["binary"]
        needed = set(binary["neededLibraries"]) | {binary["loaderSoname"]}
        self.assertTrue(needed <= planned, needed - planned)
        self.assertTrue(binary["neededSetIsSubsetOfThePlanClosure"])


class ProductionTests(unittest.TestCase):
    def test_two_separate_jobs_each_produce_exactly_once(self) -> None:
        production = document()["production"]
        self.assertEqual(production["replicas"], 2)
        self.assertEqual(production["producesPerJob"], 1)
        self.assertTrue(production["jobsAreSeparate"])
        self.assertTrue(production["sameCommit"])
        self.assertTrue(production["sameFrozenInputs"])

    def test_a_mismatch_is_a_stop_and_never_another_roll(self) -> None:
        retry = document()["production"]["retryPolicy"]
        self.assertEqual(retry["rerunAllowedCount"], 1)
        self.assertIn("before any artifact was created", retry["rerunAllowedOnlyWhen"])
        self.assertIn("did not match", retry["rerunForbiddenWhen"])
        self.assertIn("HARD STOP", retry["onMismatch"])

    def test_discovering_anything_new_over_the_network_is_refused(self) -> None:
        forbidden = " ".join(document()["production"]["networkForbidden"])
        self.assertIn("new packages", forbidden)
        self.assertIn("latest version", forbidden)

    def test_each_replica_reports_the_evidence_the_verdict_needs(self) -> None:
        report = " ".join(document()["production"]["perReplicaReport"])
        for needle in ("kernel", "initrd", "root disk", "loader", "e2fsck"):
            self.assertIn(needle, report)


class AcceptanceTests(unittest.TestCase):
    def test_the_criterion_is_byte_identity_and_cannot_be_relaxed(self) -> None:
        acceptance = document()["acceptance"]
        self.assertEqual(acceptance["criterion"], "byte identity, unchanged from the predecessor")
        self.assertTrue(acceptance["criterionRelaxationForbidden"])
        self.assertTrue(acceptance["semanticEquivalenceIsNotIdentity"])

    def test_all_three_artifacts_must_be_identical_not_merely_the_disk(self) -> None:
        green = " ".join(document()["acceptance"]["greenConditions"])
        for artifact in ("kernel", "initrd", "root disk"):
            self.assertIn(f"{artifact} files are byte-identical", green)

    def test_the_allowed_timestamps_are_the_two_values_the_design_names(self) -> None:
        record = document()
        rule = record["acceptance"]["timestampRule"]
        time = record["time"]
        self.assertEqual(
            sorted(rule["allowedValues"]),
            sorted(
                {
                    time["canonicalSourceEpoch"]["value"],
                    int(time["ext4WriterTime"]["successorValue"]),
                }
            ),
        )

    def test_a_plausible_clock_reading_is_rejected_by_construction(self) -> None:
        rule = document()["acceptance"]["timestampRule"]
        self.assertGreater(rule["wallClockLowerBound"], max(rule["allowedValues"]))
        self.assertIn("closed set", rule["why"])

    def test_a_green_result_records_convergence_not_a_choice(self) -> None:
        on_green = document()["acceptance"]["onGreen"]
        self.assertIn("not one replica chosen over the other", on_green["mustRecord"])
        self.assertTrue(on_green["predecessorStaysAsWritten"])

    def test_green_unlocks_the_closed_local_boot_and_nothing_further(self) -> None:
        on_green = document()["acceptance"]["onGreen"]
        self.assertIn("MAC.3 closed-local development-Mac boot work", on_green["unlocks"])
        for withheld in ("CURL.3", "public mining", "activationAllowed", "product release"):
            self.assertIn(withheld, on_green["doesNotUnlock"])

    def test_a_red_result_produces_no_third_image_and_no_new_criterion(self) -> None:
        on_red = document()["acceptance"]["onRed"]
        self.assertEqual(on_red["action"], "HARD STOP")
        self.assertTrue(on_red["thirdImageForbidden"])
        self.assertTrue(on_red["criterionEditForbidden"])


class NoSecondCopyTests(unittest.TestCase):
    """A digest restated here is a digest that can drift from its pin in silence."""

    def test_the_record_holds_no_digest_that_is_already_pinned_elsewhere(self) -> None:
        raw = RECORD_PATH.read_text(encoding="utf-8")
        for pinned in (
            root_disk.MKE2FS_SHA256,
            root_disk.DEBUGFS_SHA256,
            root_disk.E2FSPROGS_PACKAGE_SHA256,
        ):
            self.assertNotIn(pinned, raw)

    def test_the_only_digests_present_are_the_binding_and_the_first_pin(self) -> None:
        record = document()
        found = set(SHA256_LITERAL.findall(RECORD_PATH.read_text(encoding="utf-8")))
        self.assertEqual(
            found,
            {record["predecessor"]["sha256"], record["e2fsckContract"]["binary"]["sha256"]},
        )

    def test_the_binaries_point_at_their_pins_rather_than_repeating_them(self) -> None:
        for row in document()["time"]["mechanicalPreVerification"]["binaries"]:
            self.assertTrue(row["digestPinnedBy"].strip())
            self.assertFalse(SHA256_LITERAL.search(row["digestPinnedBy"]))


class InvariantTests(unittest.TestCase):
    def test_the_numbers_this_slice_must_not_move_are_written_down_unmoved(self) -> None:
        invariants = document()["invariants"]
        self.assertEqual(invariants["LLM-MINEABLE-ELIGIBLE-V5"], 14160)
        self.assertEqual(invariants["mineable_now"], 0)
        self.assertEqual(invariants["REWARD_READY"], 0)
        self.assertEqual(invariants["RP0-MD"], "HOLD")
        self.assertEqual(invariants["BF.7"], "HOLD")
        self.assertFalse(invariants["activationAllowed"])
        self.assertFalse(invariants["baseActivation"])
        self.assertIn("NOT PASSED", invariants["CURL.3"])

    def test_the_status_says_pre_registered_and_not_produced(self) -> None:
        status = document()["status"]
        self.assertIn("PRE-REGISTERED", status)
        self.assertIn("NOT-IMPLEMENTED", status)
        self.assertIn("NOT-PRODUCED", status)
        self.assertIn("NOT-BOOT-AUTHORITY", status)


if __name__ == "__main__":
    unittest.main()
