"""The MAC.3 closed-local boot qualification is frozen before it is attempted.

A qualification that is written after the run is not a qualification; it is a
description of what happened. This module holds the record to the shape it has
to have *before* the one allowed attempt: the image it will boot named by the
digest two replicas converged on, the kernel command line fixed, the isolation
stated, the pass conditions enumerated, and -- the part that is easy to leave
out -- the two prerequisites already known to be missing from the image, written
down while a reader can still check that they were known in advance.
"""

from __future__ import annotations

import hashlib
import json
import pathlib
import unittest

REPO = pathlib.Path(__file__).resolve().parents[1]
CONTAINMENT = REPO / "native/containment"
QUALIFICATION_PATH = (
    CONTAINMENT / "native-shadow-mac3-closed-local-boot-qualification-arm64-v1.json"
)
GREEN_PATH = (
    CONTAINMENT / "native-shadow-boot-root-disk-determinism-green-arm64-v1.json"
)


def document() -> dict:
    return json.loads(QUALIFICATION_PATH.read_text(encoding="utf-8"))


def green() -> dict:
    return json.loads(GREEN_PATH.read_text(encoding="utf-8"))


def digest(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


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
        record = document()
        self.assertEqual(record["frozenBefore"], "any qualification run")


class SubjectTests(unittest.TestCase):
    """What gets booted is the artifact two replicas independently reached."""

    def test_the_predecessor_green_record_is_bound_by_digest(self) -> None:
        predecessor = document()["predecessor"]
        self.assertEqual(predecessor["path"], GREEN_PATH.relative_to(REPO).as_posix())
        self.assertEqual(predecessor["sha256"], digest(GREEN_PATH))

    def test_the_kernel_and_root_disk_are_the_converged_ones(self) -> None:
        subject = document()["subject"]
        converged = green()["converged"]
        for role in ("kernel", "rootDisk"):
            self.assertEqual(subject[role]["sha256"], converged[role]["sha256"])
            self.assertEqual(subject[role]["sizeBytes"], converged[role]["sizeBytes"])

    def test_the_initrd_is_named_and_declared_unused_with_a_reason(self) -> None:
        # It exists and was produced by the same run, so silence about it would
        # read as an oversight rather than a decision.
        initrd = document()["subject"]["initrd"]
        self.assertEqual(initrd["sha256"], green()["converged"]["initrd"]["sha256"])
        self.assertFalse(initrd["used"])
        self.assertTrue(initrd["whyUnused"].strip())

    def test_the_digests_are_re_checked_against_the_files_at_boot_time(self) -> None:
        # A record naming a digest proves nothing if the loader reads whatever
        # file is at the path.
        self.assertTrue(document()["subject"]["verifiedImmediatelyBeforeBoot"])


class BootConfigurationTests(unittest.TestCase):
    """One command line, written down before it is used."""

    def test_the_kernel_command_line_is_one_frozen_string(self) -> None:
        command_line = document()["boot"]["kernelCommandLine"]
        self.assertIsInstance(command_line, str)
        self.assertEqual(command_line, command_line.strip())

    def test_it_mounts_the_sealed_root_read_only_and_names_pid_one(self) -> None:
        command_line = document()["boot"]["kernelCommandLine"]
        for token in ("root=/dev/vda", "ro", "init=/usr/lib/systemd/systemd"):
            self.assertIn(token, command_line.split())

    def test_it_gives_the_guest_a_console_to_be_observed_on(self) -> None:
        # Without a console the run produces no evidence either way.
        self.assertIn("console=hvc0", document()["boot"]["kernelCommandLine"].split())

    def test_the_init_path_is_the_real_binary_not_the_symlink(self) -> None:
        # /usr/sbin/init is a symlink; naming the target removes one way for the
        # boot to fail for a reason that has nothing to do with the image.
        command_line = document()["boot"]["kernelCommandLine"]
        self.assertNotIn("init=/usr/sbin/init", command_line)
        self.assertNotIn("init=/sbin/init", command_line)


class IsolationTests(unittest.TestCase):
    """Closed-local means the VM cannot reach the network or the host's files."""

    def test_no_network_device_is_attached(self) -> None:
        self.assertEqual(document()["isolation"]["networkDevices"], 0)

    def test_no_directory_is_shared_with_the_guest(self) -> None:
        isolation = document()["isolation"]
        self.assertEqual(isolation["sharedDirectories"], 0)
        self.assertFalse(isolation["hostFilesystemExposedToGuest"])

    def test_the_sealed_image_is_attached_read_only_and_nothing_writable_is(
        self,
    ) -> None:
        isolation = document()["isolation"]
        self.assertTrue(isolation["rootDiskAttachedReadOnly"])
        self.assertEqual(isolation["writableDisksAttached"], 0)


class SigningTests(unittest.TestCase):
    """A development Mac, and nothing that belongs to a release."""

    def test_the_entitlement_is_carried_by_an_ad_hoc_signature(self) -> None:
        signing = document()["signing"]
        self.assertTrue(signing["adHocOnly"])
        self.assertEqual(signing["entitlement"], "com.apple.security.virtualization")

    def test_no_release_identity_is_used(self) -> None:
        signing = document()["signing"]
        for forbidden in (
            "teamId",
            "developerIdCertificate",
            "provisioningProfile",
            "notarization",
        ):
            self.assertFalse(signing[forbidden], forbidden)


class PassConditionTests(unittest.TestCase):
    """Each condition says how it will be judged, not just what it hopes for."""

    def conditions(self) -> list:
        return document()["passConditions"]

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

    def test_the_image_must_be_byte_unchanged_afterwards(self) -> None:
        # Read-only is a claim about the attachment; this is the check that the
        # claim held, and it is the one condition the guest cannot fake.
        ids = {condition["id"] for condition in self.conditions()}
        self.assertIn("sealed-image-unchanged-after-the-run", ids)

    def test_reaching_userspace_is_required_rather_than_assumed(self) -> None:
        ids = {condition["id"] for condition in self.conditions()}
        self.assertIn("guest-systemd-is-pid-1", ids)

    def test_the_transcript_is_kept_whatever_the_verdict(self) -> None:
        ids = {condition["id"] for condition in self.conditions()}
        self.assertIn("console-transcript-captured-and-hashed", ids)


class KnownGapTests(unittest.TestCase):
    """What is already missing is written down before, not explained after."""

    def gaps(self) -> list:
        return document()["knownAbsentBeforeTheRun"]

    def test_each_gap_names_what_is_missing_and_what_follows_from_it(self) -> None:
        self.assertTrue(self.gaps())
        for gap in self.gaps():
            self.assertTrue(gap["what"].strip())
            self.assertTrue(gap["path"].strip())
            self.assertTrue(gap["consequence"].strip())

    def test_the_absent_account_database_is_among_them(self) -> None:
        paths = {gap["path"] for gap in self.gaps()}
        self.assertIn("/etc/passwd", paths)

    def test_the_absent_runtime_rootfs_is_among_them(self) -> None:
        paths = {gap["path"] for gap in self.gaps()}
        self.assertIn("/var/lib/boole/native-shadow/runtime-rootfs", paths)

    def test_no_gap_is_listed_as_a_pass_condition(self) -> None:
        # Otherwise the run is scored on something already known to be absent.
        conditions = " ".join(
            condition["condition"] for condition in document()["passConditions"]
        )
        for gap in self.gaps():
            self.assertNotIn(gap["path"], conditions)


class BoundaryTests(unittest.TestCase):
    """A boot is not a product, and this record refuses to be read as one."""

    def test_a_pass_would_not_establish_the_launcher_serving(self) -> None:
        not_established = document()["notEstablishedByAPass"]
        joined = " ".join(not_established).lower()
        self.assertIn("launcher", joined)

    def test_a_pass_would_not_reopen_the_release_gates(self) -> None:
        not_established = " ".join(document()["notEstablishedByAPass"]).lower()
        for gate in ("curl.3", "clean-mac", "release", "public mining", "activation"):
            self.assertIn(gate, not_established)

    def test_the_record_claims_no_boot_and_no_activation_before_running(self) -> None:
        record = document()
        self.assertFalse(record["bootableClaim"])
        self.assertFalse(record["activationAllowed"])
        self.assertFalse(record["boundaries"]["guestBootVerified"])

    def test_the_invariants_carry_across_unchanged(self) -> None:
        self.assertEqual(document()["invariants"], green()["invariants"])

    def test_the_attempt_stops_rather_than_relaxing_anything(self) -> None:
        aborts = " ".join(document()["abortConditions"]).lower()
        self.assertIn("second", aborts)
        self.assertIn("team id", aborts)


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
