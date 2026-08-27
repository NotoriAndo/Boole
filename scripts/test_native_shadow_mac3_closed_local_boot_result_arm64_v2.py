"""The successor boot ran once and passed. This holds the record of that.

The first attempt failed at `guest-systemd-is-pid-1`: PID 1 had no `/proc`,
`/sys` or `/dev` to mount and froze. A successor image was produced that adds
exactly those mount points, its qualification was frozen and merged before it
could be run, and the run took the single allowance it opened.

A pass is easier to overstate than a failure, so most of what follows is there
to hold the claim down to its size. The six conditions are the six the first
attempt was judged by, unchanged. The guest reached its default target -- and
the launcher unit, which systemd reports as Started, sent its own output to the
guest journal, which a closed machine has no channel to read. So where the
launcher refused is not something this run saw, and the record has to say so
rather than let Started be read as serving. Everything the pass does not unlock
-- CURL.3, BF.7, activation, clean-Mac evidence, a product claim -- stays where
it was.
"""

from __future__ import annotations

import hashlib
import json
import pathlib
import unittest

import scripts.native_shadow_mac3_closed_local_boot_arm64_v1 as driver

REPO = pathlib.Path(__file__).resolve().parents[1]
CONTAINMENT = REPO / "native/containment"
RESULT_PATH = CONTAINMENT / "native-shadow-mac3-closed-local-boot-result-arm64-v2.json"
QUALIFICATION_PATH = (
    CONTAINMENT / "native-shadow-mac3-closed-local-boot-qualification-arm64-v2.json"
)
FIRST_RESULT_PATH = (
    CONTAINMENT / "native-shadow-mac3-closed-local-boot-result-arm64-v1.json"
)

ATTEMPT = "MAC3-CLOSED-LOCAL-BOOT-ARM64-ATTEMPT-2"

# Written out rather than read from the record, so a record that agrees with
# itself about the wrong image still fails here.
KERNEL = "d29e317d66517190f6437b9b9bd2cedd26a424fe6da7b1a28451247a13fe1336"
INITRD = "3ae76ced73f180ccd9feb44260694871dde3e158b82bff18d2c23327989488ca"
ROOT_DISK = "566614b67ea749ee0061d73aad4e3320f92fe7d352df29d11e4494a8c063d41b"
FAILED_ROOT_DISK = "9834036f7738f3848fff23e5c3d1be85cd1f288f7ca43d2094b815eca2b378cc"


def document() -> dict:
    return json.loads(RESULT_PATH.read_text(encoding="utf-8"))


def qualification() -> dict:
    return json.loads(QUALIFICATION_PATH.read_text(encoding="utf-8"))


def canonical(value) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"))


def digest(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


class VerdictTests(unittest.TestCase):
    """It says PASS, and says how much of one, in the fields read first."""

    def test_the_record_is_on_disk_and_parses(self) -> None:
        self.assertTrue(RESULT_PATH.is_file())
        self.assertIsInstance(document(), dict)

    def test_the_verdict_is_pass(self) -> None:
        self.assertEqual(document()["verdict"], "PASS")

    def test_the_status_line_reads_as_a_pass_and_names_the_attempt(self) -> None:
        status = document()["status"]
        self.assertIn("PASS", status)
        self.assertNotIn("FAIL", status)
        self.assertIn("SUCCESSOR", status)

    def test_the_one_attempt_is_recorded_as_spent(self) -> None:
        record = document()
        self.assertEqual(record["runsAllowed"], 1)
        self.assertEqual(record["runsPerformed"], 1)
        self.assertFalse(record["rerunPermitted"])

    def test_it_is_the_successor_attempt_and_says_so(self) -> None:
        self.assertEqual(document()["attemptId"], ATTEMPT)

    def test_it_says_in_words_that_it_was_not_retried(self) -> None:
        self.assertIn("one run", document()["notRetried"].lower())


class ConditionTests(unittest.TestCase):
    """Six conditions, all met, and the same six the first attempt failed."""

    def test_there_are_exactly_six_and_all_are_met(self) -> None:
        rows = document()["passConditions"]
        self.assertEqual(len(rows), 6)
        self.assertEqual({row["verdict"] for row in rows}, {"MET"})

    def test_the_condition_that_failed_the_first_time_is_the_one_that_passed(self) -> None:
        rows = {row["id"]: row for row in document()["passConditions"]}
        self.assertEqual(rows["guest-systemd-is-pid-1"]["verdict"], "MET")
        first = json.loads(FIRST_RESULT_PATH.read_text(encoding="utf-8"))
        self.assertEqual(first["whatFailed"]["condition"], "guest-systemd-is-pid-1")

    def test_the_conditions_are_the_frozen_ones_and_were_not_reworded(self) -> None:
        # id and wording both, so a condition cannot be kept by name and
        # softened in its text.
        judged = [
            {"id": row["id"], "condition": row["condition"]}
            for row in document()["passConditions"]
        ]
        frozen = [
            {"id": row["id"], "condition": row["condition"]}
            for row in qualification()["passConditions"]
        ]
        self.assertEqual(canonical(judged), canonical(frozen))

    def test_no_condition_was_dropped(self) -> None:
        self.assertEqual(
            [row["id"] for row in document()["passConditions"]],
            [row["id"] for row in qualification()["passConditions"]],
        )


class BoundToTheFrozenRecordTests(unittest.TestCase):
    """The record it was judged by is named and pinned, not just referenced."""

    def test_it_names_the_successor_qualification(self) -> None:
        self.assertEqual(
            document()["judgedAgainst"]["path"],
            str(QUALIFICATION_PATH.relative_to(REPO)),
        )

    def test_the_digest_it_claims_is_the_digest_on_disk(self) -> None:
        claim = document()["judgedAgainst"]
        self.assertEqual(claim["sha256"], digest(QUALIFICATION_PATH))
        self.assertEqual(claim["sizeBytes"], QUALIFICATION_PATH.stat().st_size)

    def test_the_run_happened_on_the_tree_it_records(self) -> None:
        preflight = document()["preflight"]
        self.assertEqual(preflight["headSha"], preflight["originMain"])
        self.assertTrue(preflight["allPassed"])
        self.assertGreaterEqual(preflight["checksRun"], 15)


class SubjectTests(unittest.TestCase):
    """The images read were the converged ones, not the failed ones."""

    def test_the_three_digests_are_the_sealed_ones(self) -> None:
        subject = document()["subject"]
        self.assertEqual(subject["kernel"]["sha256"], KERNEL)
        self.assertEqual(subject["initrd"]["sha256"], INITRD)
        self.assertEqual(subject["rootDisk"]["sha256"], ROOT_DISK)

    def test_the_failed_images_root_disk_is_not_what_ran(self) -> None:
        self.assertNotEqual(document()["subject"]["rootDisk"]["sha256"], FAILED_ROOT_DISK)

    def test_the_initrd_was_hashed_and_not_used(self) -> None:
        initrd = document()["subject"]["initrd"]
        self.assertIs(initrd["used"], False)
        self.assertTrue(initrd["whyUnused"].strip())

    def test_the_images_came_from_two_converged_replicas(self) -> None:
        provenance = document()["subject"]["provenance"]
        self.assertEqual(provenance["convergedReplicas"], 2)
        self.assertEqual(provenance["producedPerReplica"], 1)


class ClosedMachineTests(unittest.TestCase):
    """Nothing was reachable from the guest, and the record shows the counts."""

    def test_no_network_device_no_shared_directory_no_socket(self) -> None:
        machine = document()["host"]["machine"]
        self.assertEqual(machine["networkDevices"], 0)
        self.assertEqual(machine["sharedDirectories"], 0)
        self.assertEqual(machine["socketDevices"], 0)

    def test_one_storage_device_and_it_was_read_only(self) -> None:
        self.assertEqual(document()["host"]["machine"]["storageDevices"], 1)
        self.assertIs(document()["subject"]["rootDisk"]["attachedReadOnly"], True)

    def test_the_signing_carries_no_team_identity(self) -> None:
        signing = document()["host"]["signing"]
        self.assertIn("ad-hoc", signing)
        self.assertNotIn("Developer ID", signing)


class SealedImageUnchangedTests(unittest.TestCase):
    """The one condition the guest could not have faked."""

    def test_the_root_disk_hashed_the_same_before_and_after(self) -> None:
        root = document()["rootDisk"]
        self.assertEqual(root["sha256Before"], root["sha256After"])
        self.assertEqual(root["sha256Before"], ROOT_DISK)


class ConsoleTests(unittest.TestCase):
    """Both digests of the transcript, and why there are two."""

    def test_the_raw_transcript_is_recorded_by_size_and_digest(self) -> None:
        console = document()["console"]
        self.assertEqual(console["sizeBytes"], 20409)
        self.assertEqual(len(console["sha256"]), 64)

    def test_the_judged_transcript_is_recorded_separately(self) -> None:
        console = document()["console"]
        judged = console["judgedTranscript"]
        self.assertNotEqual(judged["sha256"], console["sha256"])
        self.assertNotEqual(judged["sizeBytes"], console["sizeBytes"])

    def test_the_difference_between_them_is_explained_and_accounted_for(self) -> None:
        console = document()["console"]
        judged = console["judgedTranscript"]
        # The gap is line endings, and the record has to show the arithmetic
        # rather than assert it: 230 CRLF pairs, 230 bytes.
        self.assertEqual(console["sizeBytes"] - judged["sizeBytes"], 230)
        self.assertIn("CRLF", judged["whyItDiffers"])

    def test_the_excerpt_shows_the_two_stages_the_first_attempt_never_reached(self) -> None:
        excerpt = "\n".join(document()["console"]["decisiveExcerpt"])
        self.assertIn("EXT4-fs (vda): mounted filesystem", excerpt)
        self.assertIn("systemd 255.4", excerpt)
        self.assertIn("graphical.target", excerpt)


class RecordedRegardlessOfVerdictTests(unittest.TestCase):
    """The five things the frozen record required, pass or fail."""

    def test_all_five_are_present(self) -> None:
        recorded = document()["recordedRegardlessOfVerdict"]
        self.assertEqual(
            sorted(recorded),
            [
                "consoleTranscript",
                "furthestBootStage",
                "launcherUnit",
                "stopReasonAndExitStatus",
                "unitsThatEnteredAFailedState",
            ],
        )

    def test_the_furthest_stage_is_named_with_its_console_line(self) -> None:
        stage = document()["recordedRegardlessOfVerdict"]["furthestBootStage"]
        self.assertEqual(stage["target"], "graphical.target")
        self.assertIn("graphical.target", stage["evidence"])

    def test_every_failed_unit_is_named(self) -> None:
        failed = document()["recordedRegardlessOfVerdict"]["unitsThatEnteredAFailedState"]
        self.assertEqual([row["unit"] for row in failed], ["ldconfig.service"])
        for row in failed:
            self.assertTrue(row["evidence"].strip())
            self.assertTrue(row["why"].strip())

    def test_the_stop_reason_and_exit_status_are_recorded(self) -> None:
        stop = document()["recordedRegardlessOfVerdict"]["stopReasonAndExitStatus"]
        self.assertEqual(stop["stopReason"], "stopped-at-timeout")
        self.assertEqual(stop["hostExitStatus"], 0)
        self.assertGreater(stop["ranForSeconds"], 180)


class NotOverclaimedTests(unittest.TestCase):
    """A pass this size, and no larger."""

    def test_started_is_not_recorded_as_serving(self) -> None:
        launcher = document()["recordedRegardlessOfVerdict"]["launcherUnit"]
        self.assertIs(launcher["wasStarted"], True)
        self.assertIs(document()["boundaries"]["launcherServing"], False)
        self.assertIn("not a serving state", launcher["whatStartedMeansHere"])

    def test_where_the_launcher_refused_is_recorded_as_unobserved(self) -> None:
        # The unit logs to the guest journal and the machine has no channel
        # out. Not knowing is the honest answer and has to be the written one.
        launcher = document()["recordedRegardlessOfVerdict"]["launcherUnit"]
        self.assertEqual(launcher["whereItRefused"], "not observable from this run")
        self.assertIn("journal", launcher["whyNotObservable"])

    def test_the_boundaries_a_boot_does_not_move_stay_false(self) -> None:
        boundaries = document()["boundaries"]
        self.assertIs(boundaries["cleanMacEvidence"], False)
        self.assertIs(boundaries["launcherServing"], False)
        self.assertIs(boundaries["productRelease"], False)
        self.assertIs(boundaries["publicMiningOrBenchmark"], False)
        self.assertIs(boundaries["runtimeCompatibilityVerified"], False)

    def test_the_bootable_claim_is_scoped_to_one_development_mac(self) -> None:
        self.assertIs(document()["bootableClaim"], True)
        scope = document()["bootableClaimScope"]
        self.assertIn("one development Mac", scope)
        self.assertIn("not a claim about a clean Mac", scope)

    def test_activation_stays_disallowed(self) -> None:
        self.assertIs(document()["activationAllowed"], False)

    def test_the_invariants_are_the_frozen_ones_and_did_not_move(self) -> None:
        self.assertEqual(canonical(document()["invariants"]), canonical(qualification()["invariants"]))
        invariants = document()["invariants"]
        self.assertIn("NOT PASSED", invariants["CURL.3"])
        self.assertEqual(invariants["BF.7"], "HOLD")
        self.assertEqual(invariants["RP0-MD"], "HOLD")
        self.assertEqual(invariants["mineable_now"], 0)
        self.assertEqual(invariants["REWARD_READY"], 0)
        self.assertIs(invariants["baseActivation"], False)

    def test_it_lists_what_the_pass_does_not_unlock(self) -> None:
        unlocked = "\n".join(document()["doesNotUnlock"])
        self.assertIn("CURL.3", unlocked)
        self.assertIn("BF.7", unlocked)
        self.assertIn("activation", unlocked)
        self.assertIn("public mining", unlocked)

    def test_what_a_pass_does_not_establish_is_copied_from_the_frozen_record(self) -> None:
        self.assertEqual(
            canonical(document()["notEstablishedByThisPass"]),
            canonical(qualification()["notEstablishedByAPass"]),
        )

    def test_the_two_things_known_absent_are_still_recorded_as_absent(self) -> None:
        absent = {row["path"] for row in document()["knownAbsentAndStillAbsent"]}
        self.assertIn("/etc/passwd", absent)
        self.assertIn("/var/lib/boole/native-shadow/runtime-rootfs", absent)


class PredecessorUntouchedTests(unittest.TestCase):
    """The failure record is still a failure record."""

    def test_the_first_attempts_result_still_says_fail(self) -> None:
        first = json.loads(FIRST_RESULT_PATH.read_text(encoding="utf-8"))
        self.assertEqual(first["verdict"], "FAIL")
        self.assertEqual(first["runsPerformed"], 1)
        self.assertFalse(first["rerunPermitted"])

    def test_every_record_claimed_unchanged_is_unchanged(self) -> None:
        for row in document()["appendOnly"]["recordsLeftByteUnchanged"]:
            target = REPO / row["path"]
            self.assertEqual(digest(target), row["sha256"], row["path"])
            self.assertEqual(target.stat().st_size, row["sizeBytes"], row["path"])

    def test_the_predecessor_is_referred_to_and_not_rewritten(self) -> None:
        compared = document()["whatThisFixedComparedWithTheFirstAttempt"]
        self.assertEqual(
            compared["predecessorResult"], str(FIRST_RESULT_PATH.relative_to(REPO))
        )
        self.assertIs(compared["predecessorLeftUnchanged"], True)
        self.assertEqual(compared["firstAttemptFailedCondition"], "guest-systemd-is-pid-1")

    def test_the_fix_is_the_five_mount_points_and_nothing_wider(self) -> None:
        self.assertEqual(
            document()["whatThisFixedComparedWithTheFirstAttempt"]["addedPaths"],
            ["/dev", "/proc", "/run", "/sys", "/tmp"],
        )


class SpentTests(unittest.TestCase):
    """The successor cannot be run again, by the same mechanism as the first."""

    def test_this_file_sits_where_the_frozen_record_said_it_would(self) -> None:
        self.assertEqual(RESULT_PATH, REPO / qualification()["resultPath"])
        self.assertEqual(driver.sealed_result_path(ATTEMPT), RESULT_PATH)

    def test_the_driver_now_refuses_a_second_successor_run(self) -> None:
        with self.assertRaises(driver.RefusedError):
            driver.assert_no_run_has_been_spent(driver.sealed_result_path(ATTEMPT))

    def test_the_cleanup_left_nothing_of_the_machine_running(self) -> None:
        self.assertEqual(document()["cleanup"]["hostProcessesLeftRunning"], 0)


if __name__ == "__main__":
    unittest.main()
