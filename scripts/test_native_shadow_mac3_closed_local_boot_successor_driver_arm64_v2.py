"""The successor attempt is a second record, not a second chance at the first.

The first MAC.3 boot was attempted once and failed, and the record that says so
is sealed. A successor image exists, built to close the cause, and it has its
own frozen qualification. What must not happen is that the successor arrives by
loosening the first attempt's machinery: reusing its spent allowance, pointing
at its images, quietly relaxing a condition, or writing over the receipt that
records the failure.

So the driver is asked to select an attempt rather than assume one, and every
refusal below is a way the wrong attempt could otherwise be run. None of it
needs a Mac: these are the decisions taken *before* a machine is built, which is
exactly where the second attempt could be spent on the wrong thing.
"""

from __future__ import annotations

import hashlib
import json
import pathlib
import tempfile
import unittest
from unittest import mock

import scripts.native_shadow_mac3_closed_local_boot_arm64_v1 as driver

REPO = pathlib.Path(__file__).resolve().parents[1]
CONTAINMENT = REPO / "native/containment"

FIRST = "MAC3-CLOSED-LOCAL-BOOT-ARM64-ATTEMPT-1"
SECOND = "MAC3-CLOSED-LOCAL-BOOT-ARM64-ATTEMPT-2"

FIRST_QUALIFICATION = (
    CONTAINMENT / "native-shadow-mac3-closed-local-boot-qualification-arm64-v1.json"
)
SECOND_QUALIFICATION = (
    CONTAINMENT / "native-shadow-mac3-closed-local-boot-qualification-arm64-v2.json"
)
FIRST_RESULT = (
    CONTAINMENT / "native-shadow-mac3-closed-local-boot-result-arm64-v1.json"
)
SECOND_RESULT = (
    CONTAINMENT / "native-shadow-mac3-closed-local-boot-result-arm64-v2.json"
)

# Written out rather than read from the records, so a record that agrees with
# itself about the wrong image still fails here.
FAILED_ROOT_DISK = "9834036f7738f3848fff23e5c3d1be85cd1f288f7ca43d2094b815eca2b378cc"
FAILED_INITRD = "4674128144befeea20b1cbeb5af340b981b7b125d32d43630c721bb4b0aecab2"
SUCCESSOR_ROOT_DISK = (
    "566614b67ea749ee0061d73aad4e3320f92fe7d352df29d11e4494a8c063d41b"
)
SUCCESSOR_INITRD = "3ae76ced73f180ccd9feb44260694871dde3e158b82bff18d2c23327989488ca"
KERNEL = "d29e317d66517190f6437b9b9bd2cedd26a424fe6da7b1a28451247a13fe1336"


def read(path: pathlib.Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def canonical(value) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"))


class SelectableAttemptTests(unittest.TestCase):
    """The driver knows both attempts and can be told which one it is running."""

    def test_both_attempts_are_registered(self) -> None:
        self.assertEqual(set(driver.ATTEMPTS), {FIRST, SECOND})

    def test_each_attempt_names_its_own_frozen_record(self) -> None:
        self.assertEqual(driver.qualification_path(FIRST), FIRST_QUALIFICATION)
        self.assertEqual(driver.qualification_path(SECOND), SECOND_QUALIFICATION)

    def test_the_successor_record_loads(self) -> None:
        record = driver.qualification(SECOND)
        self.assertEqual(record["attemptId"], SECOND)
        self.assertEqual(record["runsAllowed"], 1)
        self.assertEqual(record["runsPerformed"], 0)

    def test_the_default_attempt_is_still_the_first_one(self) -> None:
        # Everything written before the successor existed keeps meaning what it
        # meant: the module-level names are the first attempt's.
        self.assertEqual(driver.qualification(), read(FIRST_QUALIFICATION))
        self.assertEqual(driver.QUALIFICATION_PATH, FIRST_QUALIFICATION)

    def test_an_unknown_attempt_is_refused_rather_than_invented(self) -> None:
        with self.assertRaises(driver.RefusedError):
            driver.qualification("MAC3-CLOSED-LOCAL-BOOT-ARM64-ATTEMPT-3")


class AttemptIdentityTests(unittest.TestCase):
    """A record may only be run as the attempt it says it is."""

    def test_the_successor_record_cannot_be_run_as_the_first_attempt(self) -> None:
        with self.assertRaises(driver.RefusedError):
            driver.assert_attempt_identity(read(SECOND_QUALIFICATION), FIRST)

    def test_the_first_record_cannot_be_run_as_the_successor(self) -> None:
        with self.assertRaises(driver.RefusedError):
            driver.assert_attempt_identity(read(FIRST_QUALIFICATION), SECOND)

    def test_each_record_is_accepted_as_itself(self) -> None:
        driver.assert_attempt_identity(read(SECOND_QUALIFICATION), SECOND)
        # The first record predates the naming scheme and carries no attemptId;
        # absence means the first attempt rather than "any attempt".
        driver.assert_attempt_identity(read(FIRST_QUALIFICATION), FIRST)

    def test_a_record_that_claims_nothing_is_not_read_as_the_successor(self) -> None:
        with self.assertRaises(driver.RefusedError):
            driver.assert_attempt_identity({"runsPerformed": 0}, SECOND)


class SuccessorImageTests(unittest.TestCase):
    """The successor runs the successor images and refuses the failed ones."""

    def test_the_successor_expects_the_new_root_disk(self) -> None:
        digests = driver.expected_digests(SECOND)
        self.assertEqual(digests["rootDisk"], SUCCESSOR_ROOT_DISK)
        self.assertEqual(digests["kernel"], KERNEL)

    def test_the_first_attempts_root_disk_is_not_what_the_successor_wants(
        self,
    ) -> None:
        self.assertNotEqual(driver.expected_digests(SECOND)["rootDisk"], FAILED_ROOT_DISK)
        self.assertEqual(driver.expected_digests(FIRST)["rootDisk"], FAILED_ROOT_DISK)

    def test_all_three_sealed_images_are_declared_for_the_successor(self) -> None:
        declared = driver.declared_images(read(SECOND_QUALIFICATION))
        self.assertEqual(set(declared), {"kernel", "initrd", "rootDisk"})
        self.assertEqual(declared["initrd"]["sha256"], SUCCESSOR_INITRD)
        self.assertNotEqual(declared["initrd"]["sha256"], FAILED_INITRD)

    def test_a_mismatched_file_refuses_before_anything_boots(self) -> None:
        with tempfile.TemporaryDirectory() as work:
            path = pathlib.Path(work) / "guest-root-disk"
            path.write_bytes(b"the first attempt's image, or any other")
            with self.assertRaises(driver.RefusedError):
                driver.assert_file_matches(path, SUCCESSOR_ROOT_DISK, "rootDisk")


class PreflightTests(unittest.TestCase):
    """Every refusal that has to happen before a machine is built."""

    def images(self, work: pathlib.Path) -> dict:
        paths = {}
        for role, name in (
            ("kernel", "guest-kernel"),
            ("initrd", "guest-initrd"),
            ("rootDisk", "guest-root-disk"),
        ):
            path = work / name
            path.write_bytes(b"stand-in for " + name.encode("ascii"))
            paths[role] = path
        return paths

    def test_a_missing_image_path_is_refused_not_skipped(self) -> None:
        # The record declares three files. Handing over two and booting anyway
        # would mean the unchecked one was never a requirement.
        with tempfile.TemporaryDirectory() as work:
            work = pathlib.Path(work)
            images = self.images(work)
            del images["initrd"]
            with self.assertRaises(driver.RefusedError):
                driver.preflight(SECOND, images, work)

    def test_any_one_wrong_digest_refuses(self) -> None:
        with tempfile.TemporaryDirectory() as work:
            work = pathlib.Path(work)
            with self.assertRaises(driver.RefusedError):
                driver.preflight(SECOND, self.images(work), work)

    def test_a_spent_record_refuses(self) -> None:
        spent = dict(read(SECOND_QUALIFICATION), runsPerformed=1)
        with self.assertRaises(driver.RefusedError):
            driver.assert_record_has_an_attempt_left(spent)
        driver.assert_record_has_an_attempt_left(read(SECOND_QUALIFICATION))

    def test_an_existing_successor_receipt_refuses_a_rerun(self) -> None:
        with tempfile.TemporaryDirectory() as work:
            work = pathlib.Path(work)
            already = work / "ALREADY-SEALED.json"
            already.write_text("{}", encoding="utf-8")
            with mock.patch.object(driver, "sealed_result_path", return_value=already):
                with self.assertRaises(driver.RefusedError):
                    driver.preflight(SECOND, self.images(work), work)

    def test_a_receipt_left_in_the_working_directory_refuses(self) -> None:
        with tempfile.TemporaryDirectory() as work:
            work = pathlib.Path(work)
            images = self.images(work)
            (work / "RUN-RECEIPT.json").write_text("{}", encoding="utf-8")
            with self.assertRaises(driver.RefusedError):
                driver.preflight(SECOND, images, work)


class SeparateResultPathTests(unittest.TestCase):
    """Two attempts, two receipts. Neither can overwrite or unlock the other."""

    def test_the_attempts_seal_to_different_paths(self) -> None:
        self.assertNotEqual(
            driver.sealed_result_path(FIRST), driver.sealed_result_path(SECOND)
        )

    def test_the_successor_seals_where_its_own_record_says(self) -> None:
        self.assertEqual(
            driver.sealed_result_path(SECOND),
            REPO / read(SECOND_QUALIFICATION)["resultPath"],
        )

    def test_the_first_attempt_stays_spent(self) -> None:
        # The receipt that records the failure is on disk, so the first attempt
        # is refused by the code and not by anyone remembering.
        self.assertTrue(FIRST_RESULT.exists())
        with self.assertRaises(driver.RefusedError):
            driver.assert_no_run_has_been_spent(driver.sealed_result_path(FIRST))

    def test_each_attempt_is_refused_by_its_own_receipt(self) -> None:
        # The successor has since been run, so both attempts are spent. What
        # this was written to catch still holds and is what is checked: each
        # refusal names that attempt's own receipt. If the first attempt's file
        # were standing in for the successor's, the message would say so.
        with self.assertRaises(driver.RefusedError) as refusal:
            driver.assert_no_run_has_been_spent(driver.sealed_result_path(SECOND))
        self.assertIn(SECOND_RESULT.name, str(refusal.exception))
        self.assertNotIn(FIRST_RESULT.name, str(refusal.exception))

    def test_running_the_successor_does_not_reset_the_first_allowance(self) -> None:
        record = read(SECOND_QUALIFICATION)
        self.assertIs(record["predecessorAttempt"]["resetsTheSpentAttempt"], False)
        self.assertIs(record["predecessorAttempt"]["reusesTheSpentAttempt"], False)
        self.assertEqual(record["predecessorAttempt"]["runsPerformed"], 1)


class ConditionsNotRelaxedTests(unittest.TestCase):
    """The bar the successor is judged by is the bar the first attempt failed."""

    def test_the_six_conditions_are_byte_identical_to_the_first_attempts(self) -> None:
        self.assertEqual(
            canonical(read(SECOND_QUALIFICATION)["passConditions"]),
            canonical(read(FIRST_QUALIFICATION)["passConditions"]),
        )

    def test_the_sealed_record_passes_the_check(self) -> None:
        driver.assert_conditions_are_not_relaxed(read(SECOND_QUALIFICATION))

    def test_a_dropped_condition_refuses(self) -> None:
        record = read(SECOND_QUALIFICATION)
        record["passConditions"] = record["passConditions"][:-1]
        with self.assertRaises(driver.RefusedError):
            driver.assert_conditions_are_not_relaxed(record)

    def test_a_reworded_condition_refuses(self) -> None:
        record = read(SECOND_QUALIFICATION)
        record["passConditions"][0]["condition"] = "the machine started"
        with self.assertRaises(driver.RefusedError):
            driver.assert_conditions_are_not_relaxed(record)

    def test_a_moved_baseline_refuses(self) -> None:
        # The comparison is against the first attempt's file at the digest it
        # actually has, so pointing the claim somewhere friendlier fails.
        record = read(SECOND_QUALIFICATION)
        record["passConditionsUnchanged"]["sha256"] = "0" * 64
        with self.assertRaises(driver.RefusedError):
            driver.assert_conditions_are_not_relaxed(record)

    def test_every_condition_still_has_a_rule_to_judge_it(self) -> None:
        ids = {row["id"] for row in read(SECOND_QUALIFICATION)["passConditions"]}
        self.assertEqual(ids, set(driver.RULES))


class ClosedMachineTests(unittest.TestCase):
    """A machine that could reach anything is refused before it is built."""

    def test_the_sealed_record_describes_a_closed_machine(self) -> None:
        driver.assert_isolation_is_closed(read(SECOND_QUALIFICATION))

    def test_a_network_device_refuses(self) -> None:
        record = read(SECOND_QUALIFICATION)
        record["isolation"]["networkDevices"] = 1
        with self.assertRaises(driver.RefusedError):
            driver.assert_isolation_is_closed(record)

    def test_a_shared_directory_refuses(self) -> None:
        record = read(SECOND_QUALIFICATION)
        record["isolation"]["sharedDirectories"] = 1
        with self.assertRaises(driver.RefusedError):
            driver.assert_isolation_is_closed(record)

    def test_a_writable_disk_refuses(self) -> None:
        record = read(SECOND_QUALIFICATION)
        record["isolation"]["writableDisksAttached"] = 1
        with self.assertRaises(driver.RefusedError):
            driver.assert_isolation_is_closed(record)

    def test_a_read_write_attachment_refuses(self) -> None:
        record = read(SECOND_QUALIFICATION)
        record["isolation"]["rootDiskAttachedReadOnly"] = False
        with self.assertRaises(driver.RefusedError):
            driver.assert_isolation_is_closed(record)

    def test_exposing_the_host_filesystem_refuses(self) -> None:
        record = read(SECOND_QUALIFICATION)
        record["isolation"]["hostFilesystemExposedToGuest"] = True
        with self.assertRaises(driver.RefusedError):
            driver.assert_isolation_is_closed(record)


class SuccessorHostInvocationTests(unittest.TestCase):
    """What the Swift host is told when it is the successor being run."""

    def argv(self) -> list:
        return driver.host_argv(
            host=pathlib.Path("/tmp/host"),
            kernel=pathlib.Path("/tmp/kernel"),
            root_disk=pathlib.Path("/tmp/root.img"),
            console=pathlib.Path("/tmp/console.log"),
            receipt=pathlib.Path("/tmp/receipt.json"),
            attempt=SECOND,
        )

    def test_it_hands_over_the_successor_digests(self) -> None:
        argv = self.argv()
        self.assertEqual(argv[argv.index("--root-disk-sha256") + 1], SUCCESSOR_ROOT_DISK)
        self.assertEqual(argv[argv.index("--kernel-sha256") + 1], KERNEL)

    def test_it_never_hands_over_the_failed_image(self) -> None:
        self.assertNotIn(FAILED_ROOT_DISK, " ".join(self.argv()))

    def test_the_command_line_comes_from_the_successor_record(self) -> None:
        argv = self.argv()
        self.assertEqual(
            argv[argv.index("--cmdline") + 1],
            read(SECOND_QUALIFICATION)["boot"]["kernelCommandLine"],
        )

    def test_the_unused_initrd_is_still_not_offered_to_the_host(self) -> None:
        # It is hashed because it is part of the sealed set, and withheld
        # because the record says this image does not boot through one.
        self.assertNotIn("--initrd", self.argv())
        self.assertIs(read(SECOND_QUALIFICATION)["subject"]["initrd"]["used"], False)


class SuccessorJudgingTests(unittest.TestCase):
    """The verdict is read against the successor's digests, not the first's."""

    GOOD_TRANSCRIPT = (
        "[    0.000000] Linux version 6.8.0-31-generic\n"
        "[    1.204000] EXT4-fs (vda): mounted filesystem ro with ordered data mode\n"
        "[    1.900000] systemd[1]: systemd 255 running in system mode.\n"
    )

    def receipt(self, **overrides) -> dict:
        base = {
            "outcome": "guest-stopped",
            "dryRun": False,
            "kernel": {"sha256": KERNEL},
            "rootDisk": {"sha256": SUCCESSOR_ROOT_DISK, "attachedReadOnly": True},
            "machine": {
                "networkDevices": 0,
                "sharedDirectories": 0,
                "socketDevices": 0,
                "storageDevices": 1,
                "serialPorts": 1,
            },
        }
        base.update(overrides)
        return base

    def judge(self, receipt: dict, transcript: str = None) -> dict:
        rows = driver.judge_pass_conditions(
            transcript=self.GOOD_TRANSCRIPT if transcript is None else transcript,
            root_disk_digest_before=SUCCESSOR_ROOT_DISK,
            root_disk_digest_after=SUCCESSOR_ROOT_DISK,
            receipt=receipt,
            attempt=SECOND,
        )
        return {row["id"]: row["verdict"] for row in rows}

    def test_a_clean_successor_boot_meets_every_condition(self) -> None:
        self.assertEqual(set(self.judge(self.receipt()).values()), {"MET"})

    def test_a_run_of_the_failed_image_is_not_read_as_the_successor(self) -> None:
        receipt = self.receipt(
            rootDisk={"sha256": FAILED_ROOT_DISK, "attachedReadOnly": True}
        )
        self.assertEqual(self.judge(receipt)["loads-the-converged-image"], "NOT MET")

    def test_the_same_transcript_fails_under_the_first_attempts_digests(self) -> None:
        # The two attempts do not share a subject; judging one against the
        # other's expectations is the mistake this argument exists to prevent.
        rows = driver.judge_pass_conditions(
            transcript=self.GOOD_TRANSCRIPT,
            root_disk_digest_before=SUCCESSOR_ROOT_DISK,
            root_disk_digest_after=SUCCESSOR_ROOT_DISK,
            receipt=self.receipt(),
            attempt=FIRST,
        )
        verdicts = {row["id"]: row["verdict"] for row in rows}
        self.assertEqual(verdicts["loads-the-converged-image"], "NOT MET")

    def test_a_changed_image_still_fails_however_well_it_booted(self) -> None:
        rows = driver.judge_pass_conditions(
            transcript=self.GOOD_TRANSCRIPT,
            root_disk_digest_before=SUCCESSOR_ROOT_DISK,
            root_disk_digest_after="b" * 64,
            receipt=self.receipt(),
            attempt=SECOND,
        )
        verdicts = {row["id"]: row["verdict"] for row in rows}
        self.assertEqual(verdicts["sealed-image-unchanged-after-the-run"], "NOT MET")

    def test_a_panic_is_still_not_userspace(self) -> None:
        transcript = (
            "[    1.204000] EXT4-fs (vda): mounted filesystem ro\n"
            "[    1.500000] Kernel panic - not syncing: No working init found.\n"
        )
        verdicts = self.judge(self.receipt(), transcript)
        self.assertEqual(verdicts["guest-systemd-is-pid-1"], "NOT MET")

    def test_a_dry_run_cannot_stand_in_for_the_attempt(self) -> None:
        receipt = self.receipt(dryRun=True, outcome="dry-run-configuration-valid")
        self.assertEqual(self.judge(receipt)["loads-the-converged-image"], "NOT MET")


class PredecessorUntouchedTests(unittest.TestCase):
    """Opening a second attempt leaves the first attempt's record alone."""

    def test_the_sealed_records_still_hash_to_what_the_successor_cites(self) -> None:
        for row in read(SECOND_QUALIFICATION)["appendOnly"][
            "recordsLeftByteUnchanged"
        ]:
            path = REPO / row["path"]
            self.assertEqual(driver.sha256_file(path), row["sha256"], row["path"])
            self.assertEqual(path.stat().st_size, row["sizeBytes"], row["path"])

    def test_the_first_record_still_counts_no_run_of_its_own(self) -> None:
        # The spend is recorded in the result file, not by editing the
        # qualification, which is why the qualification still reads zero.
        self.assertEqual(read(FIRST_QUALIFICATION)["runsPerformed"], 0)

    def test_the_first_result_still_records_the_failure(self) -> None:
        result = read(FIRST_RESULT)
        self.assertEqual(result["verdict"], "FAIL")
        self.assertEqual(result["runsPerformed"], 1)


class GateTests(unittest.TestCase):
    """Held by the gates that run on every push."""

    def test_this_module_stays_registered_in_the_self_test(self) -> None:
        self_test = (REPO / "scripts" / "self-test.sh").read_text(encoding="utf-8")
        self.assertIn(pathlib.Path(__file__).name, self_test)

    def test_the_successor_record_is_pinned_by_the_docs_gate(self) -> None:
        smoke = (REPO / "scripts" / "docs-smoke.sh").read_text(encoding="utf-8")
        self.assertIn(
            "native-shadow-mac3-closed-local-boot-qualification-arm64-v2.json", smoke
        )
        self.assertIn(pathlib.Path(__file__).name, smoke)

    def test_the_driver_carries_no_second_copy_of_the_command_line(self) -> None:
        source = pathlib.Path(driver.__file__).read_text(encoding="utf-8")
        self.assertNotIn("root=/dev/vda", source)
        self.assertNotIn(SUCCESSOR_ROOT_DISK, source)
        self.assertNotIn(FAILED_ROOT_DISK, source)

    def test_the_successor_record_is_bound_by_digest_here(self) -> None:
        # If the frozen record is edited, this module stops agreeing with it
        # rather than silently judging against something new.
        self.assertEqual(
            hashlib.sha256(SECOND_QUALIFICATION.read_bytes()).hexdigest(),
            "bf703945ec02f1f66b492f7bb0c6e4080190caea17dc063e2868ef688669abb7",
        )


if __name__ == "__main__":
    unittest.main()
