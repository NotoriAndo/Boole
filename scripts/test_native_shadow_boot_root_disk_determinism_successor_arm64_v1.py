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


class SealedFilePinTests(unittest.TestCase):
    """The promise is byte-unchanged; the check has to be a digest.

    The predecessor record is pinned by `predecessor.sha256` and so cannot rot
    unnoticed.  The other two files in the same sealed set were promised the
    same thing with nothing recomputing them, which makes the promise a
    convention rather than a check.  These pins close that: every file the
    record promises stays byte-unchanged carries a digest recorded from the
    commit that sealed it, and each is recomputed from disk here.
    """

    def pins(self) -> list[dict]:
        return document()["predecessor"]["sealedFileDigests"]

    def test_every_file_promised_byte_unchanged_is_pinned_by_a_digest(self) -> None:
        promised = set(document()["predecessor"]["sealedFilesThatStayByteUnchanged"])
        pinned = {pin["path"] for pin in self.pins()}
        self.assertEqual(pinned, promised, "a sealed file with no digest is unenforced")

    def test_every_pinned_sealed_file_still_matches_its_digest(self) -> None:
        for pin in self.pins():
            raw = (REPO / pin["path"]).read_bytes()
            self.assertEqual(hashlib.sha256(raw).hexdigest(), pin["sha256"], pin["path"])
            self.assertEqual(len(raw), pin["sizeBytes"], pin["path"])

    def test_the_pin_for_the_predecessor_agrees_with_the_binding_above_it(self) -> None:
        # Two places now record the same file's digest.  If an edit ever moves
        # one of them, they must not silently disagree.
        bound = document()["predecessor"]
        pin = next(p for p in self.pins() if p["path"] == bound["path"])
        self.assertEqual(pin["sha256"], bound["sha256"])
        self.assertEqual(pin["sizeBytes"], bound["sizeBytes"])

    def test_each_pin_names_the_commit_that_sealed_the_file(self) -> None:
        # The digest is only a faithful baseline if it came from the sealing
        # commit rather than from whatever happened to be on disk later.
        for pin in self.pins():
            self.assertRegex(pin["sealedBy"], r"^[0-9a-f]{7,40}$", pin["path"])
            self.assertTrue(pin["sealedBySubject"].strip(), pin["path"])
            self.assertTrue(pin["verifiedAgainstTheSealingCommit"], pin["path"])


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
        """The proof answers the failure, so its field list is the failure's field list.

        This checks the block as originally written, which is kept as written.  Its
        conclusion was later narrowed -- a second pass found that `mke2fs -d`
        overwrites three of these fields from the staging filesystem after the
        library has already stamped them -- so the block must carry the pointer to
        that correction.  Without the pointer, a reader arriving at this block alone
        would take a superseded claim for the current one.
        """

        coverage = document()["time"]["mechanicalPreVerification"]["coverage"]
        differed = set(predecessor()["investigation"]["byteDiff"]["byField"])
        self.assertEqual(set(coverage["fieldsThatDifferedBetweenReplicas"]), differed)
        self.assertEqual(
            set(coverage["fieldsTracedToFsNow"]) | set(coverage["fieldsDerivedFromThose"]),
            differed,
        )
        self.assertEqual(coverage["fieldsLeftUnexplained"], 0)
        self.assertIn("corrections[0]", coverage["supersededBy"])

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


class CorrectionTests(unittest.TestCase):
    """The second pass over the frozen writer, and what it costs the fix.

    The first pass read the library, where every time field is guarded by
    `fs->now`, and stopped once each differing field had a site.  The overwrite
    lives in `mke2fs` itself and runs after the library has already written a
    correct value, so reading the library alone could not show it.  These tests
    hold the correction to the same standard the original claim was held to, and
    hold the original claim in place rather than letting it be quietly rewritten.
    """

    def correction(self) -> dict:
        return document()["corrections"][0]

    def test_the_claim_it_corrects_is_named_and_still_says_what_it_said(self) -> None:
        """A correction that edits the claim away leaves nothing to check it against."""

        row = self.correction()
        self.assertEqual(row["correctsClaimAt"], "time.mechanicalPreVerification.coverage")
        self.assertTrue(row["originalClaimStaysAsWritten"])
        answer = document()["time"]["mechanicalPreVerification"]["answer"]
        self.assertIn("every field that differed", answer)

    def test_it_was_found_before_an_image_was_produced(self) -> None:
        """Reading the binary costs one attempt less than producing a pair and comparing."""

        row = self.correction()
        self.assertEqual(row["foundBefore"], "any image was produced by this record")
        self.assertIn("objdump", row["method"])
        self.assertIn("nothing was executed", row["method"])
        self.assertFalse(document()["boundaries"]["imageProduced"])

    def test_the_writer_time_is_called_necessary_and_not_sufficient(self) -> None:
        effect = self.correction()["effectOnTheSuccessorValue"]
        self.assertTrue(effect["necessary"])
        self.assertFalse(effect["sufficient"])

    def test_the_field_that_survives_the_fix_is_named(self) -> None:
        effect = self.correction()["effectOnTheSuccessorValue"]
        self.assertEqual(effect["fieldsStillNonDeterministic"], ["i_ctime"])

    def test_the_two_field_lists_partition_the_times_that_differed(self) -> None:
        """Removed plus surviving must be every differing time, with no overlap."""

        effect = self.correction()["effectOnTheSuccessorValue"]
        removed = set(effect["fieldsRemovedByTheSuccessorValue"])
        surviving = set(effect["fieldsStillNonDeterministic"])
        derived = set(
            document()["time"]["mechanicalPreVerification"]["coverage"]["fieldsDerivedFromThose"]
        )
        differed = set(predecessor()["investigation"]["byteDiff"]["byField"])
        self.assertEqual(removed & surviving, set())
        self.assertEqual(removed | surviving, differed - derived)

    def test_every_site_says_which_binary_and_which_address(self) -> None:
        for row in self.correction()["sites"]:
            self.assertEqual(row["binary"], "mke2fs")
            self.assertTrue(row["address"].startswith("0x"))
            self.assertTrue(row["instruction"].strip())

    def test_the_overwrite_site_names_the_offsets_the_predecessor_measured(self) -> None:
        offsets = {
            row.get("field"): row["inodeOffset"]
            for row in self.correction()["sites"]
            if "inodeOffset" in row
        }
        self.assertEqual(offsets["i_atime and i_ctime"], "0x8 and 0xc")
        self.assertEqual(offsets["i_mtime"], "0x10")

    def test_the_reconciliation_covers_the_inode_times_the_predecessor_counted(self) -> None:
        """The correction predicts a 5 / 5 / all / all split; the record measured one."""

        rows = self.correction()["reconciliationWithThePredecessorMeasurements"]
        counts = predecessor()["investigation"]["byteDiff"]["byField"]
        for field in ("i_atime", "i_ctime", "i_mtime", "i_crtime"):
            self.assertIn(field, rows)
            self.assertIn(str(counts[field]), rows[field].replace(",", ""))
        self.assertEqual(counts["i_atime"], counts["i_mtime"])
        self.assertGreater(counts["i_ctime"], counts["i_atime"])

    def test_the_reason_userspace_cannot_fix_it_is_stated(self) -> None:
        why = self.correction()["whyItCannotBeSetFromUserspace"]
        self.assertIn("utimensat", why)
        self.assertIn("ctime", why)

    def test_the_failure_mode_is_recorded_as_loud_rather_than_silent(self) -> None:
        """The audit added by this slice is what turns this into a detected fault."""

        self.assertIn(
            "wall-clock-survived-in-the-image",
            self.correction()["detectedRatherThanSilent"],
        )

    def test_the_correction_does_not_touch_the_bar(self) -> None:
        """A correction that moved the criterion would be a relaxation wearing a hat."""

        acceptance = document()["acceptance"]
        self.assertEqual(acceptance["criterion"], "byte identity, unchanged from the predecessor")
        self.assertTrue(acceptance["criterionRelaxationForbidden"])
        self.assertEqual(acceptance["timestampRule"]["allowedValues"], [0, 1])

    def test_the_remedy_is_left_to_the_operator_with_one_recommendation(self) -> None:
        row = self.correction()
        self.assertTrue(row["resolutionRequiresOperatorDecision"])
        recommended = [o for o in row["optionsPutToTheOperator"] if o["recommended"]]
        self.assertEqual(len(recommended), 1)
        self.assertIn("e2fsprogs", recommended[0]["option"])
        for option in row["optionsPutToTheOperator"]:
            self.assertTrue(option["why"].strip())

    def test_none_of_the_offered_options_is_taken_here(self) -> None:
        """Offering a remedy is not adopting one; the sealed files stay sealed."""

        self.assertFalse(document()["boundaries"]["sealedRecordsModified"])
        self.assertTrue(document()["time"]["debugfsMustNotBecomeAWriter"]["required"])

    def test_the_recommended_option_says_what_confirming_it_would_take(self) -> None:
        """Its requirement is that a version be confirmed to exist.

        Left at that, confirming drifts towards reading a changelog and believing
        it -- which is the same shape of mistake this correction exists to record.
        The contract names the read instead, and names the substitutes that do not
        count, including a matching pair of images: two runs that shared a clock
        look exactly like a fixed cause.
        """

        option = next(o for o in self.correction()["optionsPutToTheOperator"] if o["recommended"])
        contract = option["confirmationContract"]
        self.assertTrue(contract["nothingHereChoosesThisOption"])
        counts = " ".join(contract["whatWouldCount"])
        self.assertIn("lstat", counts)
        self.assertIn("i_links_count", counts)
        self.assertIn("libext2fs", counts)
        self.assertIn("digests", counts)
        rejected = " ".join(contract["whatWouldNotCount"])
        self.assertIn("changelog", rejected)
        self.assertIn("shared a clock", rejected)

    def test_confirming_the_recommended_option_keeps_the_download_order(self) -> None:
        """An external download is pre-registered before it happens, not after."""

        option = next(o for o in self.correction()["optionsPutToTheOperator"] if o["recommended"])
        order = option["confirmationContract"]["orderThatWouldApply"]
        self.assertIn("before the download", order)
        self.assertIn("191", order)
        self.assertIn("byte-unchanged", order)


class WalkArithmeticTests(unittest.TestCase):
    """The walk was read off a disassembly by hand, and hand-read displacements slip.

    A mistyped one moves a field without making the record read any less
    plausible: `sp+0xd8` and `sp+0xe8` are one character apart and name
    different times, and swapping them would leave the correction pointing at
    the wrong cause while every sentence around it still made sense.  The record
    therefore carries the operands the walk used -- the two buffer bases and each
    field's offset inside its struct -- and these tests recompute every address
    from them rather than trusting the address as written.

    They also refuse to let a base rest on the field it was derived from.  Six
    fields are recomputed against each base, so a base off by a byte fails six
    times instead of passing as an assumption.
    """

    def arithmetic(self) -> dict:
        return document()["corrections"][0]["offsetArithmetic"]

    @staticmethod
    def displacement(address: str) -> int:
        prefix, _, offset = address.partition("+")
        assert prefix == "sp", address
        return int(offset, 16)

    def test_every_stat_field_address_follows_from_its_offset_in_the_struct(self) -> None:
        buffer = self.arithmetic()["statBuffer"]
        base = self.displacement(buffer["base"])
        for row in buffer["fields"]:
            self.assertEqual(
                base + row["structOffset"],
                self.displacement(row["address"]),
                f"{row['name']} does not sit where {buffer['base']} puts it",
            )

    def test_every_inode_field_address_follows_from_its_offset_in_the_struct(self) -> None:
        buffer = self.arithmetic()["inodeBuffer"]
        base = self.displacement(buffer["base"])
        for row in buffer["fields"]:
            self.assertEqual(
                base + row["structOffset"],
                self.displacement(row["address"]),
                f"{row['name']} does not sit where {buffer['base']} puts it",
            )

    def test_each_base_is_corroborated_by_fields_the_finding_does_not_rest_on(self) -> None:
        """`st_mode`, `st_uid` and `st_gid` are copied in the same straight line.

        They are not part of the claim, which makes them the useful witnesses: if
        the base were wrong the timestamps could still be explained away, but the
        mode and the ownership landing correctly could not.
        """

        arithmetic = self.arithmetic()
        for buffer in (arithmetic["statBuffer"], arithmetic["inodeBuffer"]):
            named = {row["name"] for row in buffer["fields"]}
            times = {n for n in named if "time" in n or "tim." in n}
            self.assertGreaterEqual(len(named - times), 3, buffer["base"])

    def test_the_stat_buffer_base_is_computed_rather_than_inferred(self) -> None:
        """One instruction takes the address and hands it to `lstat64`.

        Deriving the base from `st_mode` alone would be an inference about a
        struct layout.  Reading the instruction that forms the pointer is not.
        """

        buffer = self.arithmetic()["statBuffer"]
        self.assertEqual(buffer["baseEstablishedAt"], "0x13970")
        established = buffer["baseEstablishedBy"]
        self.assertIn("add x0, sp, #0x80", established)
        self.assertIn("lstat64", established)

    def test_the_three_copies_run_from_the_stat_times_to_the_inode_times(self) -> None:
        copies = {row["to"]: row["from"] for row in self.arithmetic()["copies"]}
        self.assertEqual(
            copies,
            {
                "i_atime": "st_atim.tv_sec",
                "i_ctime": "st_ctim.tv_sec",
                "i_mtime": "st_mtim.tv_sec",
            },
        )
        addressed = {row["name"] for row in self.arithmetic()["statBuffer"]["fields"]}
        self.assertTrue(set(copies.values()) <= addressed)

    def test_the_surviving_field_is_one_of_the_copies(self) -> None:
        """The arithmetic has to land on the field the correction says survives."""

        surviving = self.correction_fields()
        copied = {row["to"] for row in self.arithmetic()["copies"]}
        self.assertTrue(set(surviving) <= copied)

    @staticmethod
    def correction_fields() -> list:
        return document()["corrections"][0]["effectOnTheSuccessorValue"][
            "fieldsStillNonDeterministic"
        ]


class StaticNegativeTests(unittest.TestCase):
    """The load-bearing half of the correction is an absence.

    The claim is not that the staging path reads the wrong time; it is that it
    reads no time from the filesystem struct at all, which is why no value of
    the successor variable can reach these fields.  An absence found by looking
    is worth what the looking was worth, so the record has to say where it
    looked, how wide, and what would have shown up.  The positive control is the
    part that makes the zero mean something: the same register in the same binary
    does read `fs->now` a few instructions earlier.
    """

    def negative(self) -> dict:
        return document()["corrections"][0]["staticNegative"]

    def test_the_window_is_bounded_and_ends_at_the_write_back(self) -> None:
        window = self.negative()["window"]
        self.assertLess(int(window["from"], 16), int(window["to"], 16))
        self.assertIn("ext2fs_write_inode", window["endsAt"])

    def test_nothing_in_the_window_reads_the_filesystem_struct(self) -> None:
        row = self.negative()
        self.assertEqual(row["loadsFromTheFsRegisterInTheWindow"], 0)
        self.assertEqual(row["loadsAtTheFsNowDisplacementInTheWindow"], 0)
        self.assertEqual(row["fsNowDisplacement"], "0xb8")

    def test_the_register_it_counted_is_shown_to_be_the_filesystem_pointer(self) -> None:
        """A count over the wrong register would be zero for an uninteresting reason."""

        row = self.negative()
        self.assertEqual(row["fsRegisterInThatWindow"], "x21")
        shown = row["fsRegisterEstablishedBy"]
        for callee in ("ext2fs_write_new_inode", "ext2fs_read_inode", "ext2fs_write_inode"):
            self.assertIn(callee, shown)

    def test_the_zero_has_a_positive_control_in_the_same_binary(self) -> None:
        control = self.negative()["positiveControl"]
        self.assertEqual(control["address"], "0x13ca4")
        self.assertIn("x21", control["instruction"])
        self.assertIn("0xb8", control["instruction"])
        self.assertOutsideWindow = int(control["address"], 16)
        window = self.negative()["window"]
        self.assertLess(self.assertOutsideWindow, int(window["from"], 16))

    def test_the_absence_is_what_makes_the_successor_value_insufficient(self) -> None:
        consequence = self.negative()["consequence"]
        self.assertIn(root_disk.FAKE_TIME_ENV, consequence)
        self.assertIn("not sufficient", consequence)


class ProductionReadinessTests(unittest.TestCase):
    def test_production_is_blocked_by_the_correction(self) -> None:
        readiness = document()["productionReadiness"]
        self.assertTrue(readiness["blocked"])
        self.assertTrue(readiness["dispatchForbiddenWhileBlocked"])
        self.assertIn("staged-inode-ctime-is-not-fs-now", readiness["blockedBy"])

    def test_every_named_blocker_is_a_correction_that_says_it_blocks(self) -> None:
        rows = {row["id"]: row for row in document()["corrections"]}
        for blocker in document()["productionReadiness"]["blockedBy"]:
            self.assertIn(blocker, rows)
            self.assertTrue(rows[blocker]["blocksProduction"])

    def test_the_reason_is_the_one_pair_rule_rather_than_a_doubt_about_the_result(self) -> None:
        why = document()["productionReadiness"]["why"]
        self.assertIn("one production pair", why)
        self.assertEqual(document()["production"]["replicas"], 2)


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

    def test_every_digest_present_is_one_this_record_recomputes(self) -> None:
        # The set grew when the sealed files were pinned, and the test stays an
        # equality rather than a subset: a digest that appears here and is not
        # recomputed from a file on disk is exactly what this guard is for.
        # Each member of the allowed set is checked against reality elsewhere —
        # the pins by SealedFilePinTests, the e2fsck binary by its own contract.
        record = document()
        allowed = {pin["sha256"] for pin in record["predecessor"]["sealedFileDigests"]}
        allowed.add(record["predecessor"]["sha256"])
        allowed.add(record["e2fsckContract"]["binary"]["sha256"])
        found = set(SHA256_LITERAL.findall(RECORD_PATH.read_text(encoding="utf-8")))
        self.assertEqual(found, allowed)

    def test_the_predecessor_digest_is_the_only_one_written_twice(self) -> None:
        # It is restated by the pin block, which is the drift risk this class
        # names.  SealedFilePinTests ties the two copies together, so the
        # restatement cannot drift in silence; nothing else may be restated.
        record = document()
        pinned = [pin["sha256"] for pin in record["predecessor"]["sealedFileDigests"]]
        self.assertEqual(len(pinned), len(set(pinned)))
        self.assertIn(record["predecessor"]["sha256"], pinned)

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
