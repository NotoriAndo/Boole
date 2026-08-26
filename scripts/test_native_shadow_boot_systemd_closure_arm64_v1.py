"""Tests for the systemd guest closure audit.

This is the last of the three input slots the boot-artifact plan scaffold left
null.  It answers one question -- "if this rootfs booted, would PID 1 be real
systemd and would it start the launcher?" -- entirely from files, without
running anything.

The audit deliberately reports two tiers of evidence separately.  Everything
answerable from the tracked source lock can be re-proved by CI.  Everything
read out of the package content store cannot: those bytes are gitignored and
the runner has never seen them.  Averaging the two into one boolean would make
the weaker half look as strong as the stronger half, so the result keeps them
apart and the tests below check that it does.
"""

from __future__ import annotations

import hashlib
import json
import pathlib
import tempfile
import unittest

from scripts import native_shadow_boot_systemd_closure_arm64_v1 as closure


EMPTY_SHA256 = hashlib.sha256(b"").hexdigest()


class ConstantTests(unittest.TestCase):
    def test_the_format_answers_the_scaffold_slot(self):
        # The sealed scaffold named this exact string for its systemdGuestClosure
        # input. If the two ever disagree this audit is answering a slot nothing
        # asked for.
        self.assertEqual(closure.CLOSURE_FORMAT, "systemd-rootfs-closure-authority-v1")
        scaffold = json.loads(
            pathlib.Path(
                "native/containment/"
                "native-shadow-boot-artifact-build-plan-arm64-v1-scaffold.json"
            ).read_text()
        )
        self.assertEqual(
            scaffold["inputs"]["systemdGuestClosure"]["format"],
            closure.CLOSURE_FORMAT,
        )

    def test_the_status_does_not_claim_a_boot(self):
        self.assertEqual(
            closure.RESULT_STATUS,
            "SYSTEMD-GUEST-CLOSURE-AUDITED-NOT-BOOT-AUTHORITY",
        )

    def test_the_source_lock_digest_is_pinned(self):
        self.assertEqual(
            closure.SOURCE_LOCK_SHA256,
            "9eb70e05e0daf8cc56c0741c5c8ca266cad819d059ca28bcadeaecf84c0531cf",
        )

    def test_the_launcher_path_matches_the_sealed_launcher_result(self):
        # The launcher build sealed the guest path it built for. The unit file's
        # ExecStart has to name that same path or the service would start nothing.
        sealed = json.loads(
            pathlib.Path(
                "native/containment/native-shadow-launcher-build-result-arm64-v1.json"
            ).read_text()
        )
        self.assertEqual(closure.LAUNCHER_GUEST_PATH, sealed["launcher"]["guestLogicalPath"])


class LockAuditTests(unittest.TestCase):
    def setUp(self):
        self.lock = closure.load_source_lock(
            pathlib.Path(
                "native/containment/"
                "native-shadow-boot-rootfs-source-lock-arm64-v1.json"
            )
        )

    def test_the_lock_digest_is_checked_not_assumed(self):
        with tempfile.TemporaryDirectory() as tmp:
            bogus = pathlib.Path(tmp) / "lock.json"
            bogus.write_text('{"schema": "x"}\n')
            with self.assertRaises(closure.SystemdClosureError):
                closure.load_source_lock(bogus)

    def test_the_launcher_unit_is_a_tracked_file_with_a_pinned_digest(self):
        unit = closure.tracked_file(self.lock, closure.LAUNCHER_UNIT_PATH)
        self.assertEqual(
            unit["sha256"],
            "126f0d88e24ecc53879aba02ad910d516980b14473ea30ac4ed14e1cd120e0d8",
        )
        self.assertEqual(unit["mode"], "0444")
        self.assertEqual(unit["uid"], 0)
        self.assertEqual(unit["gid"], 0)

    def test_sysusers_and_tmpfiles_are_tracked_with_pinned_digests(self):
        sysusers = closure.tracked_file(self.lock, closure.SYSUSERS_PATH)
        tmpfiles = closure.tracked_file(self.lock, closure.TMPFILES_PATH)
        self.assertEqual(
            sysusers["sha256"],
            "75b1ae8eb024396dcb36d3713435115dba68a8c52b05705ce4fe3b8e7f616445",
        )
        self.assertEqual(
            tmpfiles["sha256"],
            "ad9676f2836b097b48e7955c07c165100b2257010bfdb6b4099396fc68f0d721",
        )

    def test_a_missing_tracked_file_is_an_error_not_a_none(self):
        with self.assertRaises(closure.SystemdClosureError):
            closure.tracked_file(self.lock, "/usr/lib/systemd/system/nothing.service")

    def test_machine_id_is_present_and_empty(self):
        entry = closure.tracked_file(self.lock, "/etc/machine-id")
        # An empty /etc/machine-id is what makes systemd generate a fresh id on
        # first boot instead of every image sharing one identity.
        self.assertEqual(entry["sha256"], EMPTY_SHA256)

    def test_the_enablement_symlink_points_at_the_unit(self):
        link = closure.derived_symlink(self.lock, closure.ENABLEMENT_LINK_PATH)
        self.assertEqual(link["target"], closure.LAUNCHER_UNIT_PATH)

    def test_the_enablement_directory_agrees_with_the_units_wantedby(self):
        # Presence is not enough. A unit enabled into the wrong target's wants
        # directory never starts, and the file would still be "present".
        unit_text = pathlib.Path(
            "native/systemd/boole-native-shadow-launcher.service"
        ).read_text()
        wanted_by = closure.unit_field(unit_text, "WantedBy")
        self.assertEqual(wanted_by, "multi-user.target")
        self.assertEqual(
            closure.ENABLEMENT_LINK_PATH,
            f"/etc/systemd/system/{wanted_by}.wants/"
            "boole-native-shadow-launcher.service",
        )

    def test_the_units_execstart_is_the_sealed_launcher_path(self):
        unit_text = pathlib.Path(
            "native/systemd/boole-native-shadow-launcher.service"
        ).read_text()
        self.assertEqual(
            closure.unit_field(unit_text, "ExecStart"),
            closure.LAUNCHER_GUEST_PATH,
        )

    def test_no_replay_node_service_is_declared_anywhere_in_the_lock(self):
        self.assertEqual(closure.replay_node_references(self.lock), [])

    def test_a_planted_replay_node_unit_is_reported(self):
        poisoned = json.loads(json.dumps(self.lock))
        poisoned["trackedFiles"].append(
            {
                "logicalPath": "/usr/lib/systemd/system/boole-replay-node.service",
                "mode": "0444",
                "sha256": EMPTY_SHA256,
                "sourcePath": "native/systemd/boole-replay-node.service",
                "uid": 0,
                "gid": 0,
            }
        )
        found = closure.replay_node_references(poisoned)
        self.assertEqual(
            found, ["/usr/lib/systemd/system/boole-replay-node.service"]
        )

    def test_the_systemd_logical_roots_are_in_the_closure(self):
        roots = closure.systemd_logical_roots(self.lock)
        for expected in (
            "/etc/systemd",
            "/usr/lib/systemd",
            "/usr/lib/sysusers.d",
            "/usr/lib/tmpfiles.d",
            "/usr/libexec/boole",
        ):
            self.assertIn(expected, roots)


class UnitFieldTests(unittest.TestCase):
    def test_a_repeated_key_is_refused_rather_than_silently_last_wins(self):
        # systemd's own semantics for a repeated ExecStart are not "the last one
        # wins", so guessing here would make the audit disagree with the thing it
        # claims to audit.
        text = "[Service]\nExecStart=/a\nExecStart=/b\n"
        with self.assertRaises(closure.SystemdClosureError):
            closure.unit_field(text, "ExecStart")

    def test_a_missing_key_is_an_error(self):
        with self.assertRaises(closure.SystemdClosureError):
            closure.unit_field("[Service]\nType=exec\n", "ExecStart")

    def test_surrounding_whitespace_is_stripped(self):
        self.assertEqual(
            closure.unit_field("[Install]\nWantedBy=  multi-user.target  \n", "WantedBy"),
            "multi-user.target",
        )


class PackageAuditTests(unittest.TestCase):
    def test_control_members_of_every_supported_compression_are_read(self):
        # The frozen set is not uniform: 188 packages carry control.tar.zst, two
        # carry control.tar.xz and the kernel modules package carries an
        # uncompressed control.tar. A reader that knows only some of those does
        # not fail loudly, it silently skips packages -- which is how a missing
        # systemd would look exactly like a present one.
        self.assertEqual(
            closure.CONTROL_MEMBER_NAMES,
            ("control.tar", "control.tar.gz", "control.tar.xz", "control.tar.zst"),
        )

    def test_an_unreadable_control_member_is_an_error_not_a_skip(self):
        with self.assertRaises(closure.SystemdClosureError):
            closure.package_identity(b"!<arch>\n")

    def test_the_init_symlink_is_resolved_relative_to_its_own_directory(self):
        # systemd-sysv ships /usr/sbin/init as a RELATIVE symlink to
        # ../lib/systemd/systemd. Joining that onto the wrong base directory
        # lands somewhere else entirely, so the resolution is part of the audit
        # rather than something read off the target string.
        self.assertEqual(
            closure.resolve_link("/usr/sbin/init", "../lib/systemd/systemd"),
            "/usr/lib/systemd/systemd",
        )

    def test_a_link_that_escapes_the_root_is_refused(self):
        with self.assertRaises(closure.SystemdClosureError):
            closure.resolve_link("/usr/sbin/init", "../../../../etc/passwd")

    def test_pid1_is_checked_to_be_an_aarch64_elf(self):
        # Same discipline as the kernel magic: e_machine lives at a defined
        # offset in the ELF header and is read there, not searched for.
        self.assertEqual(closure.ELF_MACHINE_AARCH64, 183)
        self.assertEqual(closure.ELF_MACHINE_OFFSET, 18)

    def test_required_packages_name_systemd_and_its_pid1_provider(self):
        self.assertIn("systemd", closure.REQUIRED_PACKAGES)
        # systemd-sysv is the package that installs /sbin/init pointing at
        # systemd. Without it "PID 1 is systemd" is an assumption, not a fact.
        self.assertIn("systemd-sysv", closure.REQUIRED_PACKAGES)


class ResultShapeTests(unittest.TestCase):
    def setUp(self):
        self.result = closure.build_result(
            lock_audit=closure.LockAudit(
                launcherUnitSha256="126f0d88",
                sysusersSha256="75b1ae8e",
                tmpfilesSha256="ad9676f2",
                machineIdEmpty=True,
                enablementTarget=closure.LAUNCHER_UNIT_PATH,
                execStart=closure.LAUNCHER_GUEST_PATH,
                wantedBy="multi-user.target",
                replayNodeReferences=[],
            ),
            package_audit=closure.PackageAudit(
                packages=[{"name": "systemd", "version": "255.4-1ubuntu8",
                           "sha256": "306d4824"}],
                pid1Path="/usr/lib/systemd/systemd",
                pid1ProvidedBy="systemd",
                pid1Sha256="ab970cc6",
                pid1Machine="aarch64",
                initLinkPath="/usr/sbin/init",
                initLinkTarget="../lib/systemd/systemd",
                initLinkResolvesTo="/usr/lib/systemd/systemd",
                initLinkProvidedBy="systemd-sysv",
            ),
        )

    def test_the_two_evidence_tiers_stay_separate(self):
        self.assertIn("lockAudit", self.result)
        self.assertIn("packageAudit", self.result)

    def test_the_package_tier_declares_that_ci_cannot_reprove_it(self):
        self.assertIs(self.result["packageAudit"]["reproducibleInCi"], False)

    def test_the_lock_tier_declares_that_ci_can_reprove_it(self):
        self.assertIs(self.result["lockAudit"]["reproducibleInCi"], True)

    def test_only_the_closure_boundary_flips(self):
        boundaries = self.result["boundaries"]
        self.assertIs(boundaries["systemdGuestClosureAudited"], True)
        for name in (
            "bootAuthority",
            "guestBootVerified",
            "guestImageBuilt",
            "initrdBuilt",
            "launcherDeployedIntoGuest",
            "rootDiskBuilt",
            "runtimeCompatibilityVerified",
        ):
            self.assertIs(boundaries[name], False, name)

    def test_the_result_refuses_to_claim_a_boot_or_activation(self):
        self.assertIs(self.result["bootableClaim"], False)
        self.assertIs(self.result["activationAllowed"], False)

    def test_a_replay_node_reference_refuses_to_produce_a_result(self):
        with self.assertRaises(closure.SystemdClosureError):
            closure.build_result(
                lock_audit=closure.LockAudit(
                    launcherUnitSha256="126f0d88",
                    sysusersSha256="75b1ae8e",
                    tmpfilesSha256="ad9676f2",
                    machineIdEmpty=True,
                    enablementTarget=closure.LAUNCHER_UNIT_PATH,
                    execStart=closure.LAUNCHER_GUEST_PATH,
                    wantedBy="multi-user.target",
                    replayNodeReferences=["/usr/lib/systemd/system/x.service"],
                ),
                package_audit=closure.PackageAudit(
                    packages=[],
                    pid1Path="/usr/lib/systemd/systemd",
                    pid1ProvidedBy="systemd",
                    pid1Sha256="ab970cc6",
                    pid1Machine="aarch64",
                    initLinkPath="/usr/sbin/init",
                    initLinkTarget="../lib/systemd/systemd",
                    initLinkResolvesTo="/usr/lib/systemd/systemd",
                    initLinkProvidedBy="systemd-sysv",
                ),
            )

    def test_a_non_empty_machine_id_refuses_to_produce_a_result(self):
        with self.assertRaises(closure.SystemdClosureError):
            closure.build_result(
                lock_audit=closure.LockAudit(
                    launcherUnitSha256="126f0d88",
                    sysusersSha256="75b1ae8e",
                    tmpfilesSha256="ad9676f2",
                    machineIdEmpty=False,
                    enablementTarget=closure.LAUNCHER_UNIT_PATH,
                    execStart=closure.LAUNCHER_GUEST_PATH,
                    wantedBy="multi-user.target",
                    replayNodeReferences=[],
                ),
                package_audit=closure.PackageAudit(
                    packages=[],
                    pid1Path="/usr/lib/systemd/systemd",
                    pid1ProvidedBy="systemd",
                    pid1Sha256="ab970cc6",
                    pid1Machine="aarch64",
                    initLinkPath="/usr/sbin/init",
                    initLinkTarget="../lib/systemd/systemd",
                    initLinkResolvesTo="/usr/lib/systemd/systemd",
                    initLinkProvidedBy="systemd-sysv",
                ),
            )


class NoOverclaimTests(unittest.TestCase):
    def test_the_module_never_says_a_system_booted(self):
        text = pathlib.Path(
            "scripts/native_shadow_boot_systemd_closure_arm64_v1.py"
        ).read_text().lower()
        for phrase in (
            "boots successfully",
            "boot verified",
            "bootable image",
            "successfully booted",
        ):
            self.assertNotIn(phrase, text, phrase)


if __name__ == "__main__":
    unittest.main()
