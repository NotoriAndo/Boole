#!/usr/bin/env python3
"""Tests for the separate verification stage that reads the produced images."""

from __future__ import annotations

import hashlib
import json
import pathlib
import unittest

from scripts import native_shadow_boot_image_verify_arm64_v1 as mod
from scripts.native_shadow_boot_initrd_arm64_v1 import initrd_bytes
from scripts.test_native_shadow_boot_initrd_arm64_v1 import tar_bytes


LAUNCHER = b"\x7fELF-launcher-bytes"
LAUNCHER_SHA = hashlib.sha256(LAUNCHER).hexdigest()
UNIT = b"[Unit]\nDescription=boole\n"
UNIT_SHA = hashlib.sha256(UNIT).hexdigest()

ROWS = [
    {"path": "etc", "kind": "directory", "mode": 0o755},
    {"path": "etc/systemd", "kind": "directory", "mode": 0o755},
    {"path": "etc/systemd/system", "kind": "directory", "mode": 0o755},
    {"path": "etc/systemd/system/multi-user.target.wants", "kind": "directory", "mode": 0o755},
    {
        "path": "etc/systemd/system/multi-user.target.wants/boole-native-shadow-launcher.service",
        "kind": "symlink",
        "mode": 0o777,
        "target": "/usr/lib/systemd/system/boole-native-shadow-launcher.service",
    },
    {"path": "sbin", "kind": "symlink", "mode": 0o777, "target": "usr/sbin"},
    {"path": "usr", "kind": "directory", "mode": 0o755},
    {"path": "usr/lib", "kind": "directory", "mode": 0o755},
    {"path": "usr/lib/systemd", "kind": "directory", "mode": 0o755},
    {"path": "usr/lib/systemd/system", "kind": "directory", "mode": 0o755},
    {
        "path": "usr/lib/systemd/system/boole-native-shadow-launcher.service",
        "kind": "file",
        "mode": 0o444,
        "raw": UNIT,
    },
    {"path": "usr/lib/systemd/systemd", "kind": "file", "mode": 0o755, "raw": b"\x7fELF-systemd"},
    {"path": "usr/libexec", "kind": "directory", "mode": 0o755},
    {"path": "usr/libexec/boole", "kind": "directory", "mode": 0o755},
    {
        "path": "usr/libexec/boole/boole-native-shadow-launcher",
        "kind": "file",
        "mode": 0o755,
        "raw": LAUNCHER,
    },
]

EXPECTATIONS = {
    "files": {
        "/usr/lib/systemd/system/boole-native-shadow-launcher.service": {
            "gid": 0,
            "mode": 0o444,
            "sha256": UNIT_SHA,
            "uid": 0,
        }
    },
    "symlinks": {
        "/etc/systemd/system/multi-user.target.wants/boole-native-shadow-launcher.service": {
            "gid": 0,
            "mode": 0o777,
            "target": "/usr/lib/systemd/system/boole-native-shadow-launcher.service",
            "uid": 0,
        }
    },
}

# The arm64 Linux `Image` header: 64 bytes, with the magic at offset 0x38.
KERNEL = bytearray(b"\x00" * 4096)
KERNEL[0x38:0x3C] = b"ARM\x64"


def tree(rows=None):
    return mod.tree_from_initrd(initrd_bytes(tar_bytes(rows if rows is not None else ROWS)))


def report(rows=None, **overrides):
    kwargs = {
        "tree": tree(rows),
        "expectations": EXPECTATIONS,
        "launcherSha256": LAUNCHER_SHA,
        "kernel": bytes(KERNEL),
    }
    kwargs.update(overrides)
    return mod.verify_tree(**kwargs)


def check(name, rows=None, **overrides):
    return {row["id"]: row for row in report(rows, **overrides)["checks"]}[name]


class KernelTests(unittest.TestCase):
    def test_an_arm64_image_is_recognised_by_its_header_magic(self) -> None:
        found = mod.kernel_report(bytes(KERNEL))
        self.assertTrue(found["ok"])
        self.assertEqual(found["architecture"], "aarch64")

    def test_a_kernel_without_the_magic_is_rejected(self) -> None:
        self.assertFalse(mod.kernel_report(b"\x00" * 4096)["ok"])

    def test_a_still_compressed_kernel_is_rejected(self) -> None:
        """v1 froze `kernelDecompression: gzip`, so the output must not be a gzip member."""

        self.assertFalse(mod.kernel_report(b"\x1f\x8b" + bytes(KERNEL))["ok"])

    def test_a_truncated_kernel_is_rejected_rather_than_read_past_the_end(self) -> None:
        self.assertFalse(mod.kernel_report(b"\x00" * 8)["ok"])


class TreeTests(unittest.TestCase):
    def test_paths_are_reported_as_absolute_logical_paths(self) -> None:
        self.assertIn("/usr/libexec/boole/boole-native-shadow-launcher", tree())

    def test_file_digests_are_computed_from_the_archive_content(self) -> None:
        row = tree()["/usr/libexec/boole/boole-native-shadow-launcher"]
        self.assertEqual(row["sha256"], LAUNCHER_SHA)

    def test_symlink_targets_are_carried(self) -> None:
        row = tree()["/sbin"]
        self.assertEqual(row["kind"], "symlink")
        self.assertEqual(row["target"], "usr/sbin")


class CheckTests(unittest.TestCase):
    def test_pid_one_is_systemd(self) -> None:
        self.assertTrue(check("pid1-is-systemd")["ok"])

    def test_a_tree_without_systemd_fails_the_pid_one_check(self) -> None:
        rows = [row for row in ROWS if row["path"] != "usr/lib/systemd/systemd"]
        self.assertFalse(check("pid1-is-systemd", rows)["ok"])

    def test_the_launcher_digest_must_equal_the_sealed_build_result(self) -> None:
        self.assertTrue(check("launcher-digest-matches-seal")["ok"])
        self.assertFalse(check("launcher-digest-matches-seal", launcherSha256="00" * 32)["ok"])

    def test_a_missing_launcher_fails_rather_than_passes_vacuously(self) -> None:
        rows = [
            row for row in ROWS if row["path"] != "usr/libexec/boole/boole-native-shadow-launcher"
        ]
        self.assertFalse(check("launcher-digest-matches-seal", rows)["ok"])

    def test_the_launcher_service_is_enabled_through_the_wants_symlink(self) -> None:
        self.assertTrue(check("launcher-service-is-enabled")["ok"])

    def test_an_enablement_symlink_pointing_elsewhere_fails(self) -> None:
        rows = [dict(row) for row in ROWS]
        for row in rows:
            if row["path"].endswith("multi-user.target.wants/boole-native-shadow-launcher.service"):
                row["target"] = "/usr/lib/systemd/system/other.service"
        self.assertFalse(check("launcher-service-is-enabled", rows)["ok"])

    def test_a_replay_node_anywhere_in_the_tree_is_a_failure(self) -> None:
        self.assertTrue(check("replay-node-absent")["ok"])
        rows = ROWS + [{"path": "usr/bin/boole-replay-node", "kind": "file", "raw": b"x"}]
        self.assertFalse(check("replay-node-absent", rows)["ok"])

    def test_modes_owners_and_digests_are_compared_against_the_lock(self) -> None:
        self.assertTrue(check("modes-owners-and-paths-match-the-lock")["ok"])

    def test_a_changed_mode_fails_the_lock_comparison(self) -> None:
        rows = [dict(row) for row in ROWS]
        for row in rows:
            if row["path"] == "usr/lib/systemd/system/boole-native-shadow-launcher.service":
                row["mode"] = 0o644
        self.assertFalse(check("modes-owners-and-paths-match-the-lock", rows)["ok"])

    def test_a_missing_expected_file_fails_the_lock_comparison(self) -> None:
        rows = [
            row
            for row in ROWS
            if row["path"] != "usr/lib/systemd/system/boole-native-shadow-launcher.service"
        ]
        self.assertFalse(check("modes-owners-and-paths-match-the-lock", rows)["ok"])


class ReportTests(unittest.TestCase):
    def test_the_report_covers_every_check_the_operator_named(self) -> None:
        ids = {row["id"] for row in report()["checks"]}
        self.assertEqual(ids, set(mod.REQUIRED_CHECKS))

    def test_the_report_passes_only_when_every_check_passes(self) -> None:
        self.assertTrue(report()["passed"])
        self.assertFalse(report(launcherSha256="00" * 32)["passed"])

    def test_a_run_given_no_kernel_fails_instead_of_skipping_the_check(self) -> None:
        """A check that is absent reads the same as a check that passed."""

        self.assertFalse(check("kernel-is-arm64", kernel=None)["ok"])

    def test_a_failing_report_raises_rather_than_being_read_past(self) -> None:
        with self.assertRaises(mod.ImageVerifyError):
            mod.assert_passed(report(launcherSha256="00" * 32))

    def test_the_root_disk_is_inspected_with_debugfs_in_its_sealed_role(self) -> None:
        """v1 gives debugfs the inspector role; verification is what that role is for."""

        argv = mod.debugfs_argv(debugfs="/tools/debugfs", image="/img", command_file="/cmds")
        self.assertEqual(argv[0], "/tools/debugfs")
        self.assertNotIn("-w", argv)
        self.assertIn("/img", argv)

    def test_the_debugfs_commands_only_read(self) -> None:
        commands = mod.debugfs_commands(["/usr/libexec/boole/boole-native-shadow-launcher"])
        for line in commands.splitlines():
            self.assertIn(line.split()[0], mod.DEBUGFS_READ_COMMANDS)


class LockTests(unittest.TestCase):
    def test_the_expectations_come_from_the_sealed_lock(self) -> None:
        lock = json.loads(
            pathlib.Path(
                "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v1.json"
            ).read_text(encoding="utf-8")
        )
        found = mod.expectations_from_lock(lock)
        self.assertEqual(len(found["files"]), 10)
        self.assertIn("/etc/machine-id", found["files"])
        self.assertIn(
            "/etc/systemd/system/multi-user.target.wants/boole-native-shadow-launcher.service",
            found["symlinks"],
        )
        for row in found["files"].values():
            self.assertEqual(row["uid"], 0)
            self.assertEqual(row["gid"], 0)


class BoundaryTests(unittest.TestCase):
    def test_verifying_an_image_is_not_booting_one(self) -> None:
        self.assertIs(mod.BOOTABLE_CLAIM, False)
        self.assertIs(mod.ACTIVATION_ALLOWED, False)
        self.assertIs(report()["guestBootVerified"], False)


if __name__ == "__main__":
    unittest.main()
