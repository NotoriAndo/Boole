"""The host driver for the one MAC.3 boot, tested where a Mac is not required.

The boot itself needs Virtualization.framework and cannot run on the Linux
runner that gates this repository. What can run anywhere is everything the
driver decides *around* the boot: which digests it insists on, how it refuses a
second attempt, what it asks `codesign` for, and how it reads a console
transcript into a verdict. Those are the parts that would let a bad run be
recorded as a good one, so those are the parts held here.
"""

from __future__ import annotations

import hashlib
import json
import pathlib
import tempfile
import unittest

import scripts.native_shadow_mac3_closed_local_boot_arm64_v1 as driver

REPO = pathlib.Path(__file__).resolve().parents[1]


def qualification() -> dict:
    return json.loads(driver.QUALIFICATION_PATH.read_text(encoding="utf-8"))


class OneShotTests(unittest.TestCase):
    """One attempt means the second one is refused by the code, not by care."""

    def test_the_allowance_matches_the_frozen_record(self) -> None:
        self.assertEqual(driver.RUNS_ALLOWED, qualification()["runsAllowed"])

    def test_a_run_is_refused_once_a_receipt_for_one_exists(self) -> None:
        with tempfile.TemporaryDirectory() as work:
            receipt = pathlib.Path(work) / "RUN-RECEIPT.json"
            driver.assert_no_run_has_been_spent(receipt)  # nothing there yet
            receipt.write_text("{}", encoding="utf-8")
            with self.assertRaises(driver.RefusedError):
                driver.assert_no_run_has_been_spent(receipt)

    def test_a_run_is_refused_if_the_record_already_counts_one(self) -> None:
        spent = dict(qualification(), runsPerformed=1)
        with self.assertRaises(driver.RefusedError):
            driver.assert_record_has_an_attempt_left(spent)
        driver.assert_record_has_an_attempt_left(qualification())


class DigestTests(unittest.TestCase):
    """The files on disk are hashed, not the paths trusted."""

    def test_it_hashes_a_file_the_same_way_the_seal_did(self) -> None:
        with tempfile.TemporaryDirectory() as work:
            path = pathlib.Path(work) / "payload"
            path.write_bytes(b"boole" * 1000)
            self.assertEqual(
                driver.sha256_file(path),
                hashlib.sha256(path.read_bytes()).hexdigest(),
            )

    def test_a_wrong_digest_refuses_before_anything_boots(self) -> None:
        with tempfile.TemporaryDirectory() as work:
            path = pathlib.Path(work) / "kernel"
            path.write_bytes(b"not the sealed kernel")
            with self.assertRaises(driver.RefusedError):
                driver.assert_file_matches(path, "0" * 64, "guest-kernel")

    def test_the_expected_digests_come_from_the_frozen_record(self) -> None:
        subject = qualification()["subject"]
        expected = driver.expected_digests()
        self.assertEqual(expected["kernel"], subject["kernel"]["sha256"])
        self.assertEqual(expected["rootDisk"], subject["rootDisk"]["sha256"])


class SigningTests(unittest.TestCase):
    """Ad-hoc, and carrying exactly the entitlement virtualization needs."""

    def test_the_signing_identity_is_ad_hoc(self) -> None:
        self.assertEqual(driver.CODESIGN_IDENTITY, "-")

    def test_the_codesign_call_is_ad_hoc_and_names_the_entitlement_file(
        self,
    ) -> None:
        argv = driver.codesign_argv(pathlib.Path("/tmp/host"))
        self.assertEqual(argv[0], "codesign")
        self.assertIn("-s", argv)
        self.assertEqual(argv[argv.index("-s") + 1], "-")
        self.assertIn("--entitlements", argv)
        self.assertEqual(
            argv[argv.index("--entitlements") + 1],
            str(driver.ENTITLEMENTS_PATH),
        )

    def test_no_release_identity_can_be_named(self) -> None:
        joined = " ".join(driver.codesign_argv(pathlib.Path("/tmp/host")))
        for forbidden in ("Developer ID", "--timestamp", "Team"):
            self.assertNotIn(forbidden, joined)


class HostInvocationTests(unittest.TestCase):
    """What the Swift host is told, spelled out here rather than inline."""

    def argv(self, dry_run: bool = False) -> list:
        return driver.host_argv(
            host=pathlib.Path("/tmp/host"),
            kernel=pathlib.Path("/tmp/kernel"),
            root_disk=pathlib.Path("/tmp/root.img"),
            console=pathlib.Path("/tmp/console.log"),
            receipt=pathlib.Path("/tmp/receipt.json"),
            dry_run=dry_run,
        )

    def test_it_passes_the_frozen_command_line_verbatim(self) -> None:
        argv = self.argv()
        self.assertEqual(
            argv[argv.index("--cmdline") + 1],
            qualification()["boot"]["kernelCommandLine"],
        )

    def test_it_states_the_digests_so_the_host_can_refuse_too(self) -> None:
        # The host re-hashes what it opens. Handing it the expected values means
        # a swapped file is caught by the program that reads it, not only by the
        # program that chose the path.
        argv = self.argv()
        expected = driver.expected_digests()
        self.assertEqual(argv[argv.index("--kernel-sha256") + 1], expected["kernel"])
        self.assertEqual(
            argv[argv.index("--root-disk-sha256") + 1], expected["rootDisk"]
        )

    def test_a_dry_run_is_marked_as_one(self) -> None:
        self.assertIn("--dry-run", self.argv(dry_run=True))
        self.assertNotIn("--dry-run", self.argv(dry_run=False))

    def test_no_initrd_is_offered_to_the_host(self) -> None:
        # The record says the initrd is unused; an option to pass one would make
        # that a preference rather than a property.
        self.assertNotIn("--initrd", self.argv())


class JudgingTests(unittest.TestCase):
    """A transcript is read into verdicts by fixed rules, written in advance."""

    def receipt(self, **overrides) -> dict:
        expected = driver.expected_digests()
        base = {
            "outcome": "stopped-at-timeout",
            "dryRun": False,
            "kernel": {"sha256": expected["kernel"]},
            "rootDisk": {"sha256": expected["rootDisk"], "attachedReadOnly": True},
            "kernelCommandLine": qualification()["boot"]["kernelCommandLine"],
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

    GOOD_TRANSCRIPT = (
        "[    0.000000] Linux version 6.8.0-31-generic\n"
        "[    1.204000] EXT4-fs (vda): mounted filesystem ro with ordered data mode\n"
        "[    1.310000] VFS: Mounted root (ext4 filesystem) readonly on device 254:0.\n"
        "[    1.900000] systemd[1]: systemd 255 running in system mode.\n"
    )

    def judge(self, transcript: str, before: str, after: str, receipt: dict) -> dict:
        return {
            row["id"]: row["verdict"]
            for row in driver.judge_pass_conditions(
                transcript=transcript,
                root_disk_digest_before=before,
                root_disk_digest_after=after,
                receipt=receipt,
            )
        }

    def test_every_frozen_condition_gets_exactly_one_verdict(self) -> None:
        verdicts = self.judge(
            self.GOOD_TRANSCRIPT, "a" * 64, "a" * 64, self.receipt()
        )
        self.assertEqual(
            set(verdicts),
            {row["id"] for row in qualification()["passConditions"]},
        )

    def test_a_clean_boot_meets_every_condition(self) -> None:
        verdicts = self.judge(
            self.GOOD_TRANSCRIPT, "a" * 64, "a" * 64, self.receipt()
        )
        self.assertEqual(set(verdicts.values()), {"MET"})

    def test_a_changed_image_fails_no_matter_how_well_it_booted(self) -> None:
        verdicts = self.judge(
            self.GOOD_TRANSCRIPT, "a" * 64, "b" * 64, self.receipt()
        )
        self.assertEqual(verdicts["sealed-image-unchanged-after-the-run"], "NOT MET")
        self.assertEqual(verdicts["guest-systemd-is-pid-1"], "MET")

    def test_a_kernel_that_never_mounted_its_root_fails(self) -> None:
        transcript = "[    0.000000] Linux version 6.8.0-31-generic\n"
        verdicts = self.judge(transcript, "a" * 64, "a" * 64, self.receipt())
        self.assertEqual(verdicts["kernel-reaches-its-root-filesystem"], "NOT MET")
        self.assertEqual(verdicts["guest-systemd-is-pid-1"], "NOT MET")

    def test_a_panic_is_not_read_as_reaching_userspace(self) -> None:
        transcript = (
            "[    1.204000] EXT4-fs (vda): mounted filesystem ro\n"
            "[    1.500000] Kernel panic - not syncing: No working init found.\n"
        )
        verdicts = self.judge(transcript, "a" * 64, "a" * 64, self.receipt())
        self.assertEqual(verdicts["kernel-reaches-its-root-filesystem"], "MET")
        self.assertEqual(verdicts["guest-systemd-is-pid-1"], "NOT MET")

    def test_an_empty_transcript_fails_the_evidence_condition(self) -> None:
        verdicts = self.judge("", "a" * 64, "a" * 64, self.receipt())
        self.assertEqual(verdicts["console-transcript-captured-and-hashed"], "NOT MET")

    def test_a_device_that_should_not_exist_fails_the_isolation_condition(
        self,
    ) -> None:
        receipt = self.receipt(
            machine={
                "networkDevices": 1,
                "sharedDirectories": 0,
                "socketDevices": 0,
                "storageDevices": 1,
                "serialPorts": 1,
            }
        )
        verdicts = self.judge(self.GOOD_TRANSCRIPT, "a" * 64, "a" * 64, receipt)
        self.assertEqual(verdicts["closed-local-configuration"], "NOT MET")

    def test_a_dry_run_receipt_cannot_be_judged_as_the_attempt(self) -> None:
        # Otherwise the cheapest way to a green record is to never boot at all.
        receipt = self.receipt(dryRun=True, outcome="dry-run-configuration-valid")
        verdicts = self.judge(self.GOOD_TRANSCRIPT, "a" * 64, "a" * 64, receipt)
        self.assertEqual(verdicts["loads-the-converged-image"], "NOT MET")

    def test_the_overall_verdict_needs_every_condition(self) -> None:
        rows = driver.judge_pass_conditions(
            transcript=self.GOOD_TRANSCRIPT,
            root_disk_digest_before="a" * 64,
            root_disk_digest_after="a" * 64,
            receipt=self.receipt(),
        )
        self.assertTrue(driver.overall_verdict(rows))
        rows[0]["verdict"] = "NOT MET"
        self.assertFalse(driver.overall_verdict(rows))

    def test_every_verdict_carries_the_evidence_it_was_read_from(self) -> None:
        rows = driver.judge_pass_conditions(
            transcript=self.GOOD_TRANSCRIPT,
            root_disk_digest_before="a" * 64,
            root_disk_digest_after="a" * 64,
            receipt=self.receipt(),
        )
        for row in rows:
            self.assertTrue(row["evidence"].strip())
            self.assertIn(row["verdict"], {"MET", "NOT MET"})


class BoundaryTests(unittest.TestCase):
    """The driver cannot widen what the frozen record allows."""

    def test_it_reads_the_command_line_rather_than_carrying_its_own(self) -> None:
        source = driver.__file__ and pathlib.Path(driver.__file__).read_text(
            encoding="utf-8"
        )
        self.assertNotIn("root=/dev/vda", source)

    def test_it_holds_the_predecessor_seal_by_digest(self) -> None:
        green = REPO / qualification()["predecessor"]["path"]
        self.assertEqual(
            driver.sha256_file(green), qualification()["predecessor"]["sha256"]
        )

    def test_it_refuses_a_writable_attachment_request(self) -> None:
        with self.assertRaises(driver.RefusedError):
            driver.assert_attachment_is_read_only({"attachedReadOnly": False})
        driver.assert_attachment_is_read_only({"attachedReadOnly": True})


class GateTests(unittest.TestCase):
    """Held by the gates that run on every push."""

    def test_the_driver_and_host_are_pinned_by_the_docs_gate(self) -> None:
        smoke = (REPO / "scripts" / "docs-smoke.sh").read_text(encoding="utf-8")
        self.assertIn("scripts/native_shadow_mac3_closed_local_boot_arm64_v1.py", smoke)
        self.assertIn("native/mac3/boole-mac3-closed-local-boot.swift", smoke)

    def test_this_module_stays_registered_in_the_self_test(self) -> None:
        self_test = (REPO / "scripts" / "self-test.sh").read_text(encoding="utf-8")
        self.assertIn(pathlib.Path(__file__).name, self_test)


if __name__ == "__main__":
    unittest.main()
