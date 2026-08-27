#!/usr/bin/env python3
"""Tests for the record that seals the converged root disk.

The successor authority said in advance what a green run seals and what it does
not unlock, and it said it while the outcome was still unknown.  So these tests
read the green record against that authority rather than against itself: the
phrase it seals under, the two lists it may not widen, the closed timestamp set,
the checker argv, and the property the authority singled out -- that the digest
is the value both replicas independently reached and not one of them chosen.

The other half is what the record must not have done.  Four records were sealed
before this one, and the failure record in particular is the thing a green
result is most tempting to soften.  Every one of them is read off disk here and
required to hash to what it hashed to before.
"""

from __future__ import annotations

import hashlib
import json
import pathlib
import unittest

from scripts import native_shadow_boot_produce_phase_arm64_v1 as produce
from scripts import native_shadow_boot_root_disk_arm64_v1 as root_disk


REPO = pathlib.Path(__file__).resolve().parents[1]
CONTAINMENT = REPO / "native/containment"
GREEN_PATH = CONTAINMENT / "native-shadow-boot-root-disk-determinism-green-arm64-v1.json"
HARD_STOP_PATH = (
    CONTAINMENT / "native-shadow-boot-root-disk-determinism-hard-stop-arm64-v1.json"
)


def document() -> dict:
    return json.loads(GREEN_PATH.read_text(encoding="utf-8"))


def authority() -> dict:
    return json.loads(produce.SUCCESSOR_AUTHORITY_PATH.read_text(encoding="utf-8"))


def hard_stop() -> dict:
    return json.loads(HARD_STOP_PATH.read_text(encoding="utf-8"))


def digest(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


class SealTests(unittest.TestCase):
    """It seals under the phrase the authority pre-registered, and no other."""

    def test_the_record_is_on_disk_and_parses(self) -> None:
        self.assertTrue(GREEN_PATH.is_file())
        self.assertIsInstance(document(), dict)

    def test_it_seals_under_the_phrase_the_authority_fixed(self) -> None:
        self.assertEqual(
            document()["sealedAs"], authority()["acceptance"]["onGreen"]["sealAppendOnly"]
        )

    def test_it_names_the_one_run_that_produced_it(self) -> None:
        production = document()["production"]
        self.assertEqual(production["runId"], "33045285925")
        self.assertEqual(production["dispatchCount"], 1)
        self.assertEqual(production["replicas"], 2)
        self.assertEqual(production["producedPerReplica"], 1)

    def test_it_records_the_refusal_that_produced_nothing_as_not_a_production(
        self,
    ) -> None:
        """A run that refused before writing did not spend the one pair.

        The distinction is the whole of why a second dispatch was allowed, so it
        is written down rather than left to be reconstructed from logs that
        expire.
        """

        refused = document()["production"]["refusedBeforeProducing"]
        self.assertEqual(refused["runId"], "33042531739")
        self.assertEqual(refused["artifactsCreated"], 0)
        self.assertEqual(refused["imagesWritten"], 0)
        self.assertTrue(refused["countsAgainstTheOnePair"] is False)


class ConvergenceTests(unittest.TestCase):
    """The property the authority asked to be recorded, checked as arithmetic."""

    def replicas(self) -> list[dict]:
        return document()["perReplicaReport"]

    def test_both_replicas_are_reported(self) -> None:
        self.assertEqual([row["replica"] for row in self.replicas()], [1, 2])

    def test_the_three_digests_agree_across_the_two_replicas(self) -> None:
        first, second = self.replicas()
        for role in ("kernel", "initrd", "rootDisk"):
            self.assertEqual(first[role]["sha256"], second[role]["sha256"], role)

    def test_the_sealed_digest_is_what_both_replicas_reached(self) -> None:
        """Not one replica chosen over the other -- the authority asked for this."""

        converged = document()["converged"]
        for row in self.replicas():
            for role in ("kernel", "initrd", "rootDisk"):
                self.assertEqual(converged[role]["sha256"], row[role]["sha256"], role)
        self.assertIn("both", document()["convergenceNote"].lower())

    def test_the_root_disk_is_neither_of_the_two_that_disagreed(self) -> None:
        """A green that reproduced one of the sealed pair would be a coincidence."""

        failed = {
            row["rootDiskSha256"] for row in hard_stop()["hardStop"]["replicas"]
        }
        self.assertEqual(len(failed), 2)
        self.assertNotIn(document()["converged"]["rootDisk"]["sha256"], failed)

    def test_the_kernel_and_initrd_are_the_ones_that_already_agreed(self) -> None:
        """They were identical in the failed pair too, and are not new evidence."""

        previous = hard_stop()["hardStop"]["replicas"][0]
        converged = document()["converged"]
        self.assertEqual(converged["kernel"]["sha256"], previous["kernelSha256"])
        self.assertEqual(converged["initrd"]["sha256"], previous["initrdSha256"])


class GreenConditionTests(unittest.TestCase):
    """Every condition the authority listed is answered, by its own words."""

    def test_each_pre_registered_condition_has_a_verdict(self) -> None:
        answered = {row["condition"]: row for row in document()["greenConditions"]}
        for condition in authority()["acceptance"]["greenConditions"]:
            self.assertIn(condition, answered)
            self.assertEqual(answered[condition]["verdict"], "MET", condition)

    def test_no_condition_was_invented_here(self) -> None:
        """Adding an easier condition beside the fixed ones is widening them."""

        pre_registered = set(authority()["acceptance"]["greenConditions"])
        answered = {row["condition"] for row in document()["greenConditions"]}
        self.assertEqual(answered, pre_registered)

    def test_the_checker_ran_with_the_argv_the_contract_fixed(self) -> None:
        contract = authority()["e2fsckContract"]
        for row in document()["perReplicaReport"]:
            fsck = row["fsck"]
            self.assertEqual(fsck["argvOptions"], contract["argv"][1:3])
            self.assertIn(
                fsck["exitCode"], contract["normalExit"]["acceptedExitCodes"]
            )
            self.assertTrue(fsck["ran"])
            self.assertFalse(fsck["repairOptionsUsed"])

    def test_the_checker_is_the_frozen_build_that_did_not_write_the_image(
        self,
    ) -> None:
        contract = authority()["e2fsckContract"]
        for row in document()["perReplicaReport"]:
            self.assertEqual(
                row["toolDigests"]["e2fsck"], contract["binary"]["sha256"]
            )
            self.assertNotEqual(
                row["toolDigests"]["e2fsck"], row["toolDigests"]["mke2fs"]
            )

    def test_the_writer_is_the_binary_the_static_read_passed(self) -> None:
        selection = produce.selection_record()
        expected = selection["controls"]["positive"]["writer"]["sha256"]
        for row in document()["perReplicaReport"]:
            self.assertEqual(row["toolDigests"]["mke2fs"], expected)
            self.assertEqual(row["toolDigests"]["mke2fs"], root_disk.MKE2FS_SHA256)

    def test_every_timestamp_is_inside_the_closed_set_the_authority_fixed(
        self,
    ) -> None:
        allowed = authority()["acceptance"]["timestampRule"]["allowedValues"]
        floor = authority()["acceptance"]["timestampRule"]["wallClockLowerBound"]
        for row in document()["perReplicaReport"]:
            audit = row["timeAudit"]
            self.assertEqual(audit["allowedTimestamps"], allowed)
            self.assertEqual(audit["violationCount"], 0)
            self.assertTrue(audit["passed"])
            self.assertGreater(row["writerTime"], 0)
            self.assertLess(row["writerTime"], floor)

    def test_the_writer_time_is_the_successor_value_and_not_the_no_op(self) -> None:
        successor_value = int(authority()["time"]["ext4WriterTime"]["successorValue"])
        for row in document()["perReplicaReport"]:
            self.assertEqual(row["writerTime"], successor_value)
            self.assertEqual(row["writerTime"], int(root_disk.EXT4_WRITER_TIME))

    def test_the_variable_the_writer_was_handed_is_the_one_it_reads(self) -> None:
        """The superseded name is recorded as superseded, not dropped."""

        time_block = document()["writerTime"]
        self.assertEqual(time_block["variable"], root_disk.WRITER_TIME_ENV)
        self.assertEqual(
            time_block["supersededVariable"], root_disk.SUPERSEDED_WRITER_TIME_ENV
        )
        self.assertNotEqual(time_block["variable"], time_block["supersededVariable"])


class LoaderEvidenceTests(unittest.TestCase):
    """Which bytes wrote the image, recorded rather than recomputed later."""

    def test_both_replicas_recorded_the_same_library_digests(self) -> None:
        first, second = document()["perReplicaReport"]
        self.assertEqual(first["loaderLibraries"], second["loaderLibraries"])

    def test_the_writer_and_the_checker_did_not_share_the_sealed_library(self) -> None:
        """The tool that judged the image is not built from the writer's library."""

        for row in document()["perReplicaReport"]:
            writer = row["loaderLibraries"]["writer"]
            checker = row["loaderLibraries"]["checker"]
            self.assertNotEqual(writer["libext2fs.so.2"], checker["libext2fs.so.2"])

    def test_the_writers_exact_version_library_came_from_the_writer_set(self) -> None:
        selection = produce.selection_record()
        expected = selection["controls"]["positive"]["library"]["sha256"]
        for row in document()["perReplicaReport"]:
            self.assertEqual(row["loaderLibraries"]["writer"]["libext2fs.so.2"], expected)

    def test_the_only_difference_between_the_two_results_is_named(self) -> None:
        differences = document()["unexplainedDifferences"]
        self.assertEqual(differences["count"], 0)
        self.assertTrue(differences["explained"])

    def test_the_guest_lock_this_run_read_is_the_one_still_on_disk(self) -> None:
        selection = produce.selection_record()
        for row in document()["perReplicaReport"]:
            authorities = row["inputAuthorities"]
            path = REPO / authorities["bootSourceLockPath"]
            self.assertEqual(
                authorities["bootSourceLockSha256"],
                hashlib.sha256(path.read_bytes()).hexdigest(),
            )
            self.assertEqual(
                authorities["bootSourceLockSha256"],
                selection["guestPackages"]["sourceLockSha256"],
            )

    def test_the_older_receipt_field_is_not_passed_off_as_the_guest_lock(self) -> None:
        # The guest layer's build receipt carries a sourceLockSha256 of its own,
        # for a different and older document. A reader who checked it against the
        # boot lock would find a mismatch and have no way to tell whether that
        # meant a moved lock or two unrelated fields sharing a name.
        for row in document()["perReplicaReport"]:
            authorities = row["inputAuthorities"]
            self.assertNotEqual(
                authorities["guestBuildReceipt"]["sourceLockSha256"],
                authorities["bootSourceLockSha256"],
            )
            self.assertIn("older", authorities["guestBuildReceiptNote"])


class AppendOnlyTests(unittest.TestCase):
    """Nothing before this record moved, and this record says so by digest."""

    def test_every_predecessor_still_hashes_to_what_it_bound(self) -> None:
        for row in document()["appendOnly"]["recordsLeftByteUnchanged"]:
            path = REPO / row["path"]
            self.assertTrue(path.is_file(), row["path"])
            self.assertEqual(digest(path), row["sha256"], row["path"])

    def test_the_failure_record_is_among_them(self) -> None:
        """The one a green result is most tempting to soften."""

        paths = {
            row["path"] for row in document()["appendOnly"]["recordsLeftByteUnchanged"]
        }
        self.assertIn(
            HARD_STOP_PATH.relative_to(REPO).as_posix(),
            paths,
        )
        self.assertIn(
            produce.SUCCESSOR_AUTHORITY_PATH.relative_to(REPO).as_posix(),
            paths,
        )

    def test_the_predecessor_is_linked_as_the_authority_asked(self) -> None:
        self.assertEqual(
            document()["appendOnly"]["predecessorLinkedAs"],
            authority()["acceptance"]["onGreen"]["predecessorLinkedAs"],
        )
        self.assertTrue(document()["appendOnly"]["predecessorStaysAsWritten"])


class BoundaryTests(unittest.TestCase):
    """A green root disk is a converged file, and nothing else."""

    def test_it_unlocks_exactly_what_the_authority_said_it_unlocks(self) -> None:
        on_green = authority()["acceptance"]["onGreen"]
        self.assertEqual(document()["unlocks"], on_green["unlocks"])

    def test_it_does_not_unlock_what_the_authority_listed(self) -> None:
        on_green = authority()["acceptance"]["onGreen"]
        self.assertEqual(document()["doesNotUnlock"], on_green["doesNotUnlock"])

    def test_producing_the_files_is_not_booting_them(self) -> None:
        self.assertFalse(document()["bootableClaim"])
        self.assertFalse(document()["boundaries"]["guestBootVerified"])

    def test_it_claims_no_activation_and_no_public_run(self) -> None:
        self.assertFalse(document()["activationAllowed"])
        self.assertFalse(document()["boundaries"]["publicMiningOrBenchmark"])

    def test_the_invariants_carry_across_unchanged(self) -> None:
        self.assertEqual(document()["invariants"], authority()["invariants"])


class GateTests(unittest.TestCase):
    """The record and its tests are pinned where the gates can see them."""

    def test_the_record_is_pinned_by_the_docs_gate(self) -> None:
        smoke = (REPO / "scripts" / "docs-smoke.sh").read_text(encoding="utf-8")
        self.assertIn(GREEN_PATH.relative_to(REPO).as_posix(), smoke)

    def test_this_module_stays_registered_in_the_self_test(self) -> None:
        self_test = (REPO / "scripts" / "self-test.sh").read_text(encoding="utf-8")
        self.assertIn(pathlib.Path(__file__).name, self_test)


if __name__ == "__main__":
    unittest.main()
