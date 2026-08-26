#!/usr/bin/env python3
"""RED tests for reading the produced ext4 root disk back and checking it.

`mke2fs` wrote the image from a staging tree this repository staged, and the
chain from the frozen layer to that tree is already pinned step by step.  What
is not pinned is `mke2fs` itself: nothing so far has opened the image it wrote
and looked.  This module opens it, through the same ext4 driver a boot would
use, and hands what it finds to the verification stage that already exists.

Reusing that stage is the point.  The initrd and the root disk are two
containers for one tree, so they answer to one set of checks, and a second set
written for the second container would be a second thing that can drift.

Mounting needs Linux and root, so what runs on the machine that writes these
tests is the reading, the argv and the refusals.
"""

from __future__ import annotations

import json
import os
import pathlib
import tempfile
import unittest

from scripts import native_shadow_boot_image_verify_arm64_v1 as image_verify
from scripts import native_shadow_boot_produce_phase_arm64_v1 as phase
from scripts import native_shadow_boot_root_disk_readback_arm64_v1 as readback


REPO = pathlib.Path(__file__).resolve().parents[1]
MODULE_SOURCE = (
    REPO / "scripts/native_shadow_boot_root_disk_readback_arm64_v1.py"
).read_text(encoding="utf-8")


class MountTests(unittest.TestCase):
    def test_the_image_is_mounted_read_only(self) -> None:
        argv = readback.mount_argv(
            image=pathlib.Path("/out/disk"), mountpoint=pathlib.Path("/mnt/disk")
        )
        self.assertEqual(argv[0], "mount")
        self.assertNotIn("-w", argv)
        options = argv[argv.index("-o") + 1].split(",")
        self.assertIn("ro", options)

    def test_the_mount_cannot_bring_devices_or_programs_with_it(self) -> None:
        """The image is evidence being read, not a filesystem being used."""

        argv = readback.mount_argv(
            image=pathlib.Path("/out/disk"), mountpoint=pathlib.Path("/mnt/disk")
        )
        options = argv[argv.index("-o") + 1].split(",")
        for option in ("nodev", "noexec", "nosuid"):
            self.assertIn(option, options)

    def test_it_is_mounted_as_the_filesystem_the_plan_wrote(self) -> None:
        argv = readback.mount_argv(
            image=pathlib.Path("/out/disk"), mountpoint=pathlib.Path("/mnt/disk")
        )
        self.assertEqual(argv[argv.index("-t") + 1], "ext4")

    def test_taking_it_down_names_the_same_place(self) -> None:
        mountpoint = pathlib.Path("/mnt/disk")
        self.assertEqual(readback.umount_argv(mountpoint)[-1], str(mountpoint))


class ReadTests(unittest.TestCase):
    """What is read has to be the shape the verification stage already takes."""

    def build(self, root: pathlib.Path) -> None:
        (root / "usr/lib").mkdir(parents=True)
        (root / "usr/lib/file").write_bytes(b"contents")
        (root / "usr/lib/file").chmod(0o644)
        (root / "link").symlink_to("usr/lib/file")

    def test_a_tree_on_disk_reads_like_a_tree_in_an_archive(self) -> None:
        with tempfile.TemporaryDirectory() as scratch:
            root = pathlib.Path(scratch)
            self.build(root)
            tree = readback.tree_from_directory(root)
        self.assertEqual(
            set(tree), {"/usr", "/usr/lib", "/usr/lib/file", "/link"}
        )
        self.assertEqual(tree["/usr"]["kind"], "directory")
        self.assertEqual(tree["/link"]["kind"], "symlink")
        self.assertEqual(tree["/link"]["target"], "usr/lib/file")
        self.assertEqual(tree["/usr/lib/file"]["kind"], "file")
        self.assertEqual(tree["/usr/lib/file"]["mode"], 0o644)

    def test_every_entry_carries_what_the_lock_is_compared_on(self) -> None:
        with tempfile.TemporaryDirectory() as scratch:
            root = pathlib.Path(scratch)
            self.build(root)
            tree = readback.tree_from_directory(root)
        for path, entry in tree.items():
            for field in ("gid", "kind", "mode", "uid"):
                self.assertIn(field, entry, path)
        self.assertIn("sha256", tree["/usr/lib/file"])
        self.assertIn("target", tree["/link"])

    def test_a_symlink_is_never_walked_through(self) -> None:
        """A loop, or an escape to the host, would both come from following one."""

        with tempfile.TemporaryDirectory() as scratch:
            root = pathlib.Path(scratch)
            (root / "real").mkdir()
            (root / "real/leaf").write_bytes(b"leaf")
            (root / "loop").symlink_to(root, target_is_directory=True)
            tree = readback.tree_from_directory(root)
        self.assertEqual(tree["/loop"]["kind"], "symlink")
        self.assertEqual(set(tree), {"/real", "/real/leaf", "/loop"})

    def test_the_digest_is_of_what_the_file_holds(self) -> None:
        with tempfile.TemporaryDirectory() as scratch:
            root = pathlib.Path(scratch)
            self.build(root)
            tree = readback.tree_from_directory(root)
        self.assertEqual(
            tree["/usr/lib/file"]["sha256"], readback.hashlib.sha256(b"contents").hexdigest()
        )


class ChecksTests(unittest.TestCase):
    """One set of checks, two containers."""

    def test_the_checks_are_the_ones_the_verification_stage_already_names(self) -> None:
        self.assertEqual(readback.REQUIRED_CHECKS, tuple(image_verify.REQUIRED_CHECKS))

    def test_no_second_set_of_checks_is_written_here(self) -> None:
        for identifier in image_verify.REQUIRED_CHECKS:
            self.assertNotIn(f'"{identifier}"', MODULE_SOURCE)

    def test_the_launcher_digest_comes_from_the_sealed_build_result(self) -> None:
        sealed = json.loads(
            (
                REPO
                / "native/containment"
                / "native-shadow-launcher-build-result-arm64-v1.json"
            ).read_text(encoding="utf-8")
        )
        self.assertEqual(readback.sealed_launcher_sha256(), sealed["launcher"]["sha256"])

    def test_the_files_it_reads_are_the_ones_the_authority_names(self) -> None:
        outputs = pathlib.Path("/out")
        self.assertEqual(readback.output_paths(outputs), phase.output_paths(outputs))

    def test_no_output_name_is_restated_here(self) -> None:
        for name in phase.output_names():
            self.assertNotIn(name, MODULE_SOURCE)


class RefusalTests(unittest.TestCase):
    def test_reading_the_image_back_refuses_when_the_run_is_not_root(self) -> None:
        if os.geteuid() == 0:
            self.skipTest("this host is root; the refusal cannot be observed here")
        with tempfile.TemporaryDirectory() as scratch:
            with self.assertRaises(readback.RootDiskReadbackError) as caught:
                readback.verify(outputs=pathlib.Path(scratch))
            self.assertIn("root", str(caught.exception).lower())


class BoundaryTests(unittest.TestCase):
    def test_reading_an_image_back_is_not_booting_it(self) -> None:
        self.assertIs(readback.BOOTABLE_CLAIM, False)
        self.assertIs(readback.ACTIVATION_ALLOWED, False)
        self.assertIs(readback.GUEST_BOOT_VERIFIED, False)

    def test_nothing_here_reaches_the_network(self) -> None:
        for forbidden in ("urllib", "socket", "http", "requests"):
            self.assertNotIn(forbidden, MODULE_SOURCE)


if __name__ == "__main__":
    unittest.main()
