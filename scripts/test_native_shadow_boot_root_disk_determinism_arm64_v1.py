#!/usr/bin/env python3
"""Tests for the root-disk determinism hard-stop record.

Two arm64 Linux CI jobs built the guest root disk from byte-identical inputs and
produced two different images.  The pre-registered rule for that outcome was to
stop rather than lower the bar, and the stop was taken.  This record is what the
stop is made of: which two images, which bytes differ, what each differing byte
is, and why they differ.

The tests here exist because a record whose numbers can drift is not a record.
Each count is checked against the others it has to agree with -- the attributed
bytes against the total, the per-field counts against the entry count, the
disassembly's binary digest against the digest the producer already pinned -- so
the document cannot be edited into something the evidence does not support.  The
boundary assertions are separate: byte identity FAILED, neither image is
adopted, and nothing here is a boot.
"""

from __future__ import annotations

import json
import pathlib
import unittest

from scripts import native_shadow_boot_root_disk_arm64_v1 as root_disk


REPO = pathlib.Path(__file__).resolve().parents[1]
RECORD_PATH = REPO / (
    "native/containment/native-shadow-boot-root-disk-determinism-hard-stop-arm64-v1.json"
)


def document() -> dict:
    return json.loads(RECORD_PATH.read_text(encoding="utf-8"))


def replicas() -> dict:
    return {row["replica"]: row for row in document()["hardStop"]["replicas"]}


class BoundaryTests(unittest.TestCase):
    """The stop is a stop.  None of it may read as progress."""

    def test_the_verdict_is_that_byte_identity_failed(self) -> None:
        doc = document()
        self.assertEqual(doc["hardStop"]["byteIdentity"], "FAILED")
        self.assertIs(doc["hardStop"]["held"], True)

    def test_no_image_is_adopted_and_no_boot_is_claimed(self) -> None:
        boundaries = document()["boundaries"]
        self.assertIs(boundaries["imageAdopted"], False)
        self.assertIs(boundaries["guestBootVerified"], False)
        self.assertIs(boundaries["runtimeCompatibilityVerified"], False)

    def test_the_criterion_was_not_relaxed_and_no_retry_was_run(self) -> None:
        """Equivalent-in-meaning is not the criterion; the criterion is bytes."""

        boundaries = document()["boundaries"]
        self.assertIs(boundaries["byteIdentityCriterionRelaxed"], False)
        self.assertIs(boundaries["thirdImageProduced"], False)
        self.assertIs(boundaries["productionCodeChanged"], False)

    def test_the_record_carries_the_standing_activation_boundary(self) -> None:
        doc = document()
        self.assertIs(doc["activationAllowed"], False)
        self.assertIs(doc["bootableClaim"], False)


class SubjectTests(unittest.TestCase):
    """Which two images this is about, and which of the three outputs split."""

    def test_the_two_replicas_are_the_two_jobs_of_one_run(self) -> None:
        doc = document()
        self.assertEqual(doc["hardStop"]["run"]["repository"], "NotoriAndo/Boole")
        self.assertEqual(doc["hardStop"]["run"]["runId"], "32966212739")
        self.assertEqual(len(doc["hardStop"]["replicas"]), 2)
        self.assertEqual(sorted(replicas()), [1, 2])

    def test_the_kernel_and_the_initrd_agreed(self) -> None:
        """Two of the three outputs are already reproducible; only one is not."""

        first, second = replicas()[1], replicas()[2]
        self.assertEqual(first["kernelSha256"], second["kernelSha256"])
        self.assertEqual(first["initrdSha256"], second["initrdSha256"])

    def test_the_root_disks_differ(self) -> None:
        first, second = replicas()[1], replicas()[2]
        self.assertNotEqual(first["rootDiskSha256"], second["rootDiskSha256"])

    def test_every_digest_is_a_sha256(self) -> None:
        for row in document()["hardStop"]["replicas"]:
            for key in ("kernelSha256", "initrdSha256", "rootDiskSha256"):
                self.assertRegex(row[key], r"\A[0-9a-f]{64}\Z", f"{row['replica']}:{key}")

    def test_the_downloaded_artifacts_were_checked_before_they_were_read(self) -> None:
        """Provenance first: an analysis of the wrong bytes proves nothing."""

        provenance = document()["investigation"]["provenance"]
        self.assertIs(provenance["digestsMatchedTheCiManifest"], True)
        self.assertEqual(provenance["artifactsChecked"], 6)


class DiffMapTests(unittest.TestCase):
    """Where the images differ, bounded before anything was interpreted."""

    def test_the_ranges_add_up_to_the_stated_block_count(self) -> None:
        diff = document()["investigation"]["blockDiff"]
        self.assertEqual(
            sum(row["count"] for row in diff["ranges"]), diff["differingBlocks"]
        )

    def test_each_range_count_matches_its_own_first_and_last(self) -> None:
        for row in document()["investigation"]["blockDiff"]["ranges"]:
            self.assertEqual(row["last"] - row["first"] + 1, row["count"], row)

    def test_the_differing_blocks_are_a_fraction_of_the_image(self) -> None:
        diff = document()["investigation"]["blockDiff"]
        self.assertEqual(diff["differingBlocks"], 847)
        self.assertEqual(diff["totalBlocks"], 285233)
        self.assertEqual(diff["blockSizeBytes"], root_disk.BLOCK_SIZE)

    def test_every_differing_range_is_named(self) -> None:
        """A range nobody can name is a range nobody has explained."""

        for row in document()["investigation"]["blockDiff"]["ranges"]:
            self.assertTrue(row["region"].strip(), row)


class ByteAttributionTests(unittest.TestCase):
    """What each differing byte is.  This is the part that classifies."""

    def test_the_attributed_bytes_are_all_the_differing_bytes(self) -> None:
        byte = document()["investigation"]["byteDiff"]
        self.assertEqual(
            sum(byte["byField"].values()), byte["differingBytes"]
        )

    def test_nothing_is_left_unexplained(self) -> None:
        byte = document()["investigation"]["byteDiff"]
        self.assertEqual(byte["unexplainedBytes"], 0)

    def test_every_named_field_is_a_time_or_a_checksum_over_one(self) -> None:
        for field in document()["investigation"]["byteDiff"]["byField"]:
            self.assertTrue(
                field.endswith(("time", "check", "lastcheck")) or "checksum" in field,
                field,
            )

    def test_the_two_populations_reconcile_exactly(self) -> None:
        """Byte counts are over inode records; per-file counts are over walked
        entries.  Left unreconciled, the two totals read as a contradiction."""

        doc = document()
        byte = doc["investigation"]["byteDiff"]
        reconciliation = byte["populationReconciliation"]
        self.assertEqual(
            reconciliation["walkedEntries"] + len(reconciliation["reservedInodesNotWalked"]),
            reconciliation["inodeRecordsDiffering"],
        )
        self.assertEqual(
            reconciliation["inodeRecordsDiffering"], byte["differingInodeRecords"]
        )
        self.assertEqual(
            reconciliation["walkedEntries"], doc["investigation"]["inventory"]["entries"]
        )

    def test_the_atime_bytes_and_the_atime_entries_agree(self) -> None:
        """Five records moved: the three reserved ones and the two mke2fs stamps."""

        doc = document()
        byte = doc["investigation"]["byteDiff"]
        self.assertEqual(len(byte["atimeDifferingInodes"]), byte["byField"]["i_atime"])
        reserved = set(byte["populationReconciliation"]["reservedInodesNotWalked"])
        walked = set(byte["atimeDifferingInodes"]) - reserved
        self.assertEqual(
            len(walked), doc["investigation"]["inventory"]["differingFields"]["atime"]
        )

    def test_the_classification_is_metadata_with_no_content_difference(self) -> None:
        classification = document()["investigation"]["classification"]
        self.assertEqual(classification["verdict"], "METADATA-TIMESTAMPS-ONLY")
        self.assertEqual(classification["fileContentDifferences"], 0)


class InventoryTests(unittest.TestCase):
    """Per-file comparison: what stayed identical is the load-bearing half."""

    def test_the_two_walks_produced_the_same_number_of_entries(self) -> None:
        inventory = document()["investigation"]["inventory"]
        self.assertEqual(inventory["entries"], 13448)
        self.assertEqual(
            inventory["directories"] + inventory["symlinks"] + inventory["files"],
            inventory["entries"],
        )

    def test_only_timestamps_differ_across_the_entries(self) -> None:
        differing = document()["investigation"]["inventory"]["differingFields"]
        self.assertEqual(
            sorted(differing), ["atime", "crtime", "ctime", "mtime"]
        )

    def test_the_two_entries_whose_atime_differs_are_the_ones_mke2fs_makes(self) -> None:
        """Everything else carried the staged time; these two mke2fs stamped."""

        inventory = document()["investigation"]["inventory"]
        self.assertEqual(inventory["differingFields"]["atime"], 2)
        self.assertEqual(inventory["differingFields"]["mtime"], 2)
        self.assertEqual(sorted(inventory["atimeDifferingPaths"]), ["/", "/lost+found"])

    def test_the_fields_that_would_have_mattered_are_recorded_identical(self) -> None:
        identical = document()["investigation"]["inventory"]["identicalFields"]
        for field in (
            "contentSha256",
            "path",
            "inode",
            "mode",
            "uid",
            "gid",
            "size",
            "entryOrder",
            "layout",
            "target",
        ):
            self.assertIn(field, identical)

    def test_the_content_that_was_hashed_is_stated_in_bytes(self) -> None:
        inventory = document()["investigation"]["inventory"]
        self.assertEqual(inventory["fileContentBytesHashed"], 1008783262)


class CauseTests(unittest.TestCase):
    """Why.  Read out of the frozen binaries, not inferred from the outcome."""

    def test_the_cause_names_the_variable_the_producer_sets(self) -> None:
        cause = document()["cause"]
        self.assertEqual(cause["variable"], root_disk.FAKE_TIME_ENV)
        self.assertEqual(cause["valueSet"], root_disk.mke2fs_env(config="/x")[root_disk.FAKE_TIME_ENV])

    def test_the_value_that_was_set_is_the_sentinel_that_disables_it(self) -> None:
        cause = document()["cause"]
        self.assertEqual(cause["sentinel"], "0")
        self.assertIs(cause["pinTookEffect"], False)

    def test_the_disassembly_evidence_is_from_the_binary_the_plan_pinned(self) -> None:
        """A cause read out of some other mke2fs would not be this cause."""

        binaries = {row["name"]: row for row in document()["cause"]["binaries"]}
        self.assertEqual(binaries["mke2fs"]["sha256"], root_disk.MKE2FS_SHA256)
        self.assertRegex(binaries["libext2fs.so.2.4"]["sha256"], r"\A[0-9a-f]{64}\Z")

    def test_the_evidence_shows_the_branch_and_the_two_stores_it_reaches(self) -> None:
        sites = {row["site"]: row for row in document()["cause"]["disassembly"]}
        self.assertIn("ext2fs_initialize:read-fs-now", sites)
        self.assertIn("ext2fs_initialize:call-time", sites)
        self.assertIn("ext2fs_initialize:store-s_lastcheck", sites)
        self.assertIn("ext2fs_initialize:store-s_mkfs_time", sites)
        for row in sites.values():
            self.assertRegex(row["address"], r"\A0x[0-9a-f]+\Z", row)

    def test_the_observed_times_are_the_ones_the_cause_predicts(self) -> None:
        """Wall clock, seconds apart -- not a pinned constant in either job."""

        cause = document()["cause"]
        first, second = replicas()[1], replicas()[2]
        self.assertNotEqual(first["mkfsTime"], second["mkfsTime"])
        self.assertEqual(
            cause["observedSkewSeconds"], abs(first["mkfsTime"] - second["mkfsTime"])
        )


class AssumptionTests(unittest.TestCase):
    """The producer wrote down what it could not settle.  This settles them."""

    def test_every_assumption_the_producer_listed_is_dispositioned(self) -> None:
        recorded = {row["id"] for row in document()["assumptions"]}
        declared = {row["id"] for row in root_disk.UNVERIFIED_ASSUMPTIONS}
        self.assertEqual(recorded, declared)

    def test_the_time_assumption_is_recorded_falsified(self) -> None:
        rows = {row["id"]: row for row in document()["assumptions"]}
        self.assertEqual(rows["fake-time-honoured-by-this-build"]["disposition"], "FALSIFIED")

    def test_the_readdir_order_assumption_is_recorded_held(self) -> None:
        """The identical layout is real evidence and should not be lost in the noise."""

        rows = {row["id"]: row for row in document()["assumptions"]}
        self.assertEqual(
            rows["staging-readdir-order-is-creation-order"]["disposition"], "HELD"
        )

    def test_the_loader_assumption_is_recorded_unsettled(self) -> None:
        """Its stated evidence was never emitted, so it cannot be called settled."""

        rows = {row["id"]: row for row in document()["assumptions"]}
        self.assertEqual(
            rows["loader-resolves-only-frozen-libraries"]["disposition"], "UNSETTLED"
        )


class HonestyTests(unittest.TestCase):
    """What was not done has to be as easy to find as what was."""

    def test_the_check_that_could_not_be_run_is_named(self) -> None:
        ids = {row["id"] for row in document()["notPerformed"]}
        self.assertIn("e2fsck-n", ids)

    def test_the_vacuous_check_is_marked_vacuous(self) -> None:
        """No image has xattrs, so "xattrs agree" is not evidence of anything."""

        rows = {row["id"]: row for row in document()["notPerformed"]}
        self.assertIn("xattr-comparison-vacuous", rows)
        self.assertEqual(document()["investigation"]["inventory"]["entriesWithXattrs"], 0)

    def test_the_host_difference_is_recorded_even_though_it_is_not_the_cause(self) -> None:
        rows = {row["id"]: row for row in document()["notPerformed"]}
        self.assertIn("replica-hosts-were-not-identical", rows)
        first, second = replicas()[1], replicas()[2]
        self.assertNotEqual(first["runnerImage"], second["runnerImage"])

    def test_the_inputs_are_recorded_as_the_thing_that_did_not_vary(self) -> None:
        inputs = document()["investigation"]["inputs"]
        self.assertIs(inputs["identical"], True)
        self.assertEqual(inputs["commit"], "b5557470c0db3d2a783792cf0d70c308dedf27d4")


class SuccessorTests(unittest.TestCase):
    def test_the_fix_is_left_to_a_successor_and_not_taken_here(self) -> None:
        successor = document()["successor"]
        self.assertIs(successor["fixAttemptedHere"], False)
        self.assertEqual(successor["decision"], "DEFERRED-TO-SUCCESSOR")

    def test_the_successor_is_told_what_would_have_to_be_true(self) -> None:
        self.assertTrue(document()["successor"]["requirement"].strip())


if __name__ == "__main__":
    unittest.main()
