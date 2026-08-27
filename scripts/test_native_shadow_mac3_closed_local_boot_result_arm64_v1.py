"""The MAC.3 boot ran once and did not pass. This holds the record of that.

The frozen qualification named six conditions. Five were met -- including the
two that are hardest to fake, that the image the two replicas converged on is
the image that was read, and that it was byte-unchanged afterwards. The sixth
was not: the guest's PID 1 never reached the point of identifying itself,
because the root filesystem has no `/proc`, `/sys` or `/dev` for it to mount.

A failed run is only worth anything if it is written down as a failure. These
tests exist to make the record hard to soften later: the verdict is FAIL, the
attempt is spent, the frozen qualification is bound by digest and left as
written, and the cause the run found is recorded as a new finding rather than
back-dated into the list of things that were known in advance.
"""

from __future__ import annotations

import hashlib
import json
import pathlib
import unittest

import scripts.native_shadow_mac3_closed_local_boot_arm64_v1 as driver

REPO = pathlib.Path(__file__).resolve().parents[1]
CONTAINMENT = REPO / "native/containment"
RESULT_PATH = CONTAINMENT / "native-shadow-mac3-closed-local-boot-result-arm64-v1.json"
QUALIFICATION_PATH = (
    CONTAINMENT / "native-shadow-mac3-closed-local-boot-qualification-arm64-v1.json"
)


def document() -> dict:
    return json.loads(RESULT_PATH.read_text(encoding="utf-8"))


def qualification() -> dict:
    return json.loads(QUALIFICATION_PATH.read_text(encoding="utf-8"))


def digest(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


class VerdictTests(unittest.TestCase):
    """It says FAIL, in the field a reader checks first."""

    def test_the_record_is_on_disk_and_parses(self) -> None:
        self.assertTrue(RESULT_PATH.is_file())
        self.assertIsInstance(document(), dict)

    def test_the_verdict_is_fail(self) -> None:
        self.assertEqual(document()["verdict"], "FAIL")

    def test_the_status_line_does_not_read_as_a_pass(self) -> None:
        status = document()["status"]
        self.assertIn("FAIL", status)
        self.assertNotIn("GREEN", status)

    def test_the_one_attempt_is_recorded_as_spent(self) -> None:
        record = document()
        self.assertEqual(record["runsAllowed"], 1)
        self.assertEqual(record["runsPerformed"], 1)
        self.assertFalse(record["rerunPermitted"])

    def test_it_says_in_words_that_it_was_not_retried(self) -> None:
        self.assertIn("not", document()["notRetried"].lower())


class ConditionTests(unittest.TestCase):
    """Every frozen condition is answered, and the one that failed is named."""

    def rows(self) -> list:
        return document()["passConditions"]

    def test_each_frozen_condition_has_exactly_one_verdict(self) -> None:
        self.assertEqual(
            [row["id"] for row in self.rows()],
            [row["id"] for row in qualification()["passConditions"]],
        )

    def test_each_verdict_carries_the_evidence_it_was_read_from(self) -> None:
        for row in self.rows():
            self.assertIn(row["verdict"], {"MET", "NOT MET"})
            self.assertTrue(row["evidence"].strip())

    def test_the_conditions_are_quoted_as_frozen_not_reworded(self) -> None:
        frozen = {row["id"]: row["condition"] for row in qualification()["passConditions"]}
        for row in self.rows():
            self.assertEqual(row["condition"], frozen[row["id"]])

    def test_exactly_one_condition_was_not_met_and_it_is_pid_one(self) -> None:
        failed = [row["id"] for row in self.rows() if row["verdict"] == "NOT MET"]
        self.assertEqual(failed, ["guest-systemd-is-pid-1"])

    def test_the_two_conditions_a_bad_run_would_fake_were_met(self) -> None:
        # Loading the converged image and leaving it byte-unchanged are the two
        # a run that went wrong has the most reason to be vague about.
        met = {row["id"] for row in self.rows() if row["verdict"] == "MET"}
        self.assertIn("loads-the-converged-image", met)
        self.assertIn("sealed-image-unchanged-after-the-run", met)

    def test_the_overall_verdict_follows_from_the_rows(self) -> None:
        self.assertFalse(driver.overall_verdict(self.rows()))


class EvidenceTests(unittest.TestCase):
    """What was read, kept in a form a later reader can check."""

    def test_the_image_hashed_the_same_before_and_after(self) -> None:
        root_disk = document()["rootDisk"]
        self.assertEqual(root_disk["sha256Before"], root_disk["sha256After"])
        self.assertEqual(
            root_disk["sha256Before"],
            qualification()["subject"]["rootDisk"]["sha256"],
        )

    def test_the_console_transcript_is_kept_by_size_and_digest(self) -> None:
        console = document()["console"]
        self.assertGreater(console["sizeBytes"], 0)
        self.assertEqual(len(console["sha256"]), 64)

    def test_the_decisive_lines_are_quoted_verbatim(self) -> None:
        excerpt = "\n".join(document()["console"]["decisiveExcerpt"])
        self.assertIn("Run /usr/lib/systemd/systemd as init process", excerpt)
        self.assertIn("Freezing execution.", excerpt)

    def test_the_machine_that_ran_is_recorded_as_closed_local(self) -> None:
        machine = document()["host"]["machine"]
        self.assertEqual(machine["networkDevices"], 0)
        self.assertEqual(machine["sharedDirectories"], 0)
        self.assertEqual(machine["socketDevices"], 0)
        self.assertEqual(machine["storageDevices"], 1)

    def test_the_run_is_not_a_dry_run(self) -> None:
        self.assertFalse(document()["host"]["dryRun"])


class CauseTests(unittest.TestCase):
    """The cause is stated, and stated as newly found rather than foreseen."""

    def test_the_three_missing_mount_points_are_named(self) -> None:
        paths = {row["path"] for row in document()["foundByThisRun"]}
        for missing in ("/proc", "/sys", "/dev"):
            self.assertIn(missing, paths)

    def test_each_finding_says_how_the_run_showed_it(self) -> None:
        for row in document()["foundByThisRun"]:
            self.assertTrue(row["what"].strip())
            self.assertTrue(row["shownBy"].strip())

    def test_the_new_findings_are_not_back_dated_into_the_frozen_gaps(self) -> None:
        # Writing them into the pre-run list would turn a discovery into a
        # thing that was known all along, which is exactly what a record of a
        # failed run must not be allowed to do.
        foreseen = {row["path"] for row in qualification()["knownAbsentBeforeTheRun"]}
        for row in document()["foundByThisRun"]:
            self.assertNotIn(row["path"], foreseen)

    def test_the_kernel_side_is_credited_as_working(self) -> None:
        # The failure is in the image's contents, not in the boot path, and
        # saying so is part of reporting the run accurately.
        self.assertTrue(document()["whatWorked"])
        joined = " ".join(document()["whatWorked"]).lower()
        self.assertIn("ext4", joined)
        self.assertIn("read-only", joined)


class AppendOnlyTests(unittest.TestCase):
    """The frozen record it was judged against is bound, not edited."""

    def test_the_qualification_is_bound_by_its_digest(self) -> None:
        row = document()["judgedAgainst"]
        self.assertEqual(row["path"], QUALIFICATION_PATH.relative_to(REPO).as_posix())
        self.assertEqual(row["sha256"], digest(QUALIFICATION_PATH))

    def test_the_frozen_record_still_says_it_had_not_run(self) -> None:
        # It is a pre-freeze document. Editing it to agree with the outcome
        # would destroy the only proof the conditions predate the run.
        self.assertEqual(qualification()["runsPerformed"], 0)

    def test_every_record_it_leaves_alone_is_pinned_by_digest(self) -> None:
        for row in document()["appendOnly"]["recordsLeftByteUnchanged"]:
            path = REPO / row["path"]
            self.assertTrue(path.is_file(), row["path"])
            self.assertEqual(digest(path), row["sha256"], row["path"])
            self.assertEqual(path.stat().st_size, row["sizeBytes"], row["path"])

    def test_the_predecessor_green_seal_is_among_them(self) -> None:
        paths = {row["path"] for row in document()["appendOnly"]["recordsLeftByteUnchanged"]}
        self.assertIn(
            "native/containment/native-shadow-boot-root-disk-determinism-green-arm64-v1.json",
            paths,
        )


class BoundaryTests(unittest.TestCase):
    """A failed boot moves nothing, and the record says which nothing."""

    def test_it_claims_no_boot_and_no_activation(self) -> None:
        record = document()
        self.assertFalse(record["bootableClaim"])
        self.assertFalse(record["activationAllowed"])
        self.assertFalse(record["boundaries"]["guestBootVerified"])

    def test_the_invariants_are_unchanged(self) -> None:
        green = json.loads(
            (
                CONTAINMENT
                / "native-shadow-boot-root-disk-determinism-green-arm64-v1.json"
            ).read_text(encoding="utf-8")
        )
        self.assertEqual(document()["invariants"], green["invariants"])

    def test_the_determinism_result_is_not_withdrawn_by_this_failure(self) -> None:
        # Two identical images stay two identical images; what this run shows
        # is that they are not yet a bootable userspace.
        self.assertTrue(document()["determinismResultUnaffected"])

    def test_the_next_step_needs_a_new_image_and_says_so(self) -> None:
        next_step = document()["whatWouldBeNeeded"].lower()
        self.assertIn("image", next_step)
        self.assertIn("approval", next_step)


class OneShotTests(unittest.TestCase):
    """The sealed result is itself what stops a second attempt."""

    def test_the_driver_refuses_once_this_record_exists(self) -> None:
        with self.assertRaises(driver.RefusedError):
            driver.assert_no_run_has_been_spent(RESULT_PATH)

    def test_the_driver_looks_at_the_sealed_result_not_only_a_work_directory(
        self,
    ) -> None:
        # A wiped scratch directory must not buy a second run.
        self.assertEqual(driver.SEALED_RESULT_PATH, RESULT_PATH)


class GateTests(unittest.TestCase):
    """Held by the gates that run on every push."""

    def test_the_record_is_pinned_by_the_docs_gate(self) -> None:
        smoke = (REPO / "scripts" / "docs-smoke.sh").read_text(encoding="utf-8")
        self.assertIn(RESULT_PATH.relative_to(REPO).as_posix(), smoke)
        self.assertIn('"verdict": "FAIL"', smoke)

    def test_this_module_stays_registered_in_the_self_test(self) -> None:
        self_test = (REPO / "scripts" / "self-test.sh").read_text(encoding="utf-8")
        self.assertIn(pathlib.Path(__file__).name, self_test)


if __name__ == "__main__":
    unittest.main()
