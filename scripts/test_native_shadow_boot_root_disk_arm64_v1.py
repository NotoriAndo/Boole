#!/usr/bin/env python3
"""Tests for the ext4 root disk plan the arm64 CI producer will execute."""

from __future__ import annotations

import json
import pathlib
import unittest

from scripts import native_shadow_boot_root_disk_arm64_v1 as mod
from scripts.test_native_shadow_boot_initrd_arm64_v1 import SMALL, tar_bytes


REPO = pathlib.Path(__file__).resolve().parents[1]
CONTAINMENT = REPO / "native/containment"


def sealed_authority() -> dict:
    return json.loads(
        (CONTAINMENT / "native-shadow-boot-image-builder-authority-arm64-v1.json").read_text(
            encoding="utf-8"
        )
    )


def selection_record() -> dict:
    return json.loads(
        (CONTAINMENT / "native-shadow-boot-e2fsprogs-selection-plucky-arm64-v1.json").read_text(
            encoding="utf-8"
        )
    )


LAYER = tar_bytes(SMALL)
IMAGE = "/scratch/guest-root-disk.img"
STAGING = "/staging/root"
MKE2FS = "/tools/mke2fs"
DEBUGFS = "/tools/debugfs"
E2FSCK = "/tools/e2fsck"
CONF = "/tools/etc/mke2fs.conf"
SIZE = 64 * 1024 * 1024


def plan(**overrides):
    kwargs = {
        "layer": LAYER,
        "mke2fs": MKE2FS,
        "debugfs": DEBUGFS,
        "e2fsck": E2FSCK,
        "config": CONF,
        "image": IMAGE,
        "staging": STAGING,
        "sizeBytes": SIZE,
    }
    kwargs.update(overrides)
    return mod.root_disk_plan(**kwargs)


def argv(**overrides):
    return plan(**overrides)["mke2fs"]["argv"]


class ArgvTests(unittest.TestCase):
    def test_the_frozen_uuid_is_passed_not_generated(self) -> None:
        """A generated UUID differs per run and reseeds every metadata checksum."""

        row = argv()
        self.assertIn("-U", row)
        self.assertEqual(row[row.index("-U") + 1], "00000000-0000-4000-8000-000000000001")

    def test_the_frozen_directory_hash_seed_is_passed(self) -> None:
        extended = argv()[argv().index("-E") + 1]
        self.assertIn("hash_seed=00000000-0000-4000-8000-000000000002", extended)

    def test_the_root_directory_is_owned_by_root_not_the_invoking_user(self) -> None:
        extended = argv()[argv().index("-E") + 1]
        self.assertIn("root_owner=0:0", extended)

    def test_the_pinned_tool_path_is_used_verbatim(self) -> None:
        self.assertEqual(argv()[0], MKE2FS)

    def test_the_image_and_block_count_close_the_argv(self) -> None:
        row = argv()
        self.assertEqual(row[-2], IMAGE)
        self.assertEqual(int(row[-1]), SIZE // mod.BLOCK_SIZE)

    def test_the_block_size_and_inode_size_are_pinned(self) -> None:
        row = argv()
        self.assertEqual(row[row.index("-b") + 1], str(mod.BLOCK_SIZE))
        self.assertEqual(row[row.index("-I") + 1], str(mod.INODE_SIZE))

    def test_the_inode_count_is_pinned_rather_than_derived_from_the_ratio(self) -> None:
        row = argv()
        self.assertGreater(int(row[row.index("-N") + 1]), len(mod.layer_entries(LAYER)))

    def test_the_staging_tree_is_the_population_source(self) -> None:
        row = argv()
        self.assertEqual(row[row.index("-d") + 1], STAGING)


class EnvironmentTests(unittest.TestCase):
    def test_the_time_knob_this_build_of_mke2fs_actually_reads_is_set(self) -> None:
        """Both variables reach this writer and only one of them arms the clamp.

        The statement this test used to make was true of the writer it was
        written about: 1.47.0 has no SOURCE_DATE_EPOCH at all, so
        E2FSPROGS_FAKE_TIME was the only knob there was.  The selected build
        reads SOURCE_DATE_EPOCH first -- it stores the fixed time and arms the
        flag mke2fs branches on -- and keeps E2FSPROGS_FAKE_TIME as a fallback
        that stores the time and leaves the flag clear.  So the superseded name
        is not a second way of asking for the same thing; setting it would put
        the writer back on the branch that copies each staged file's own
        st_ctime, which is the sealed failure with a newer binary.
        """

        env = plan()["mke2fs"]["env"]
        self.assertEqual(env[mod.WRITER_TIME_ENV], mod.EXT4_WRITER_TIME)
        self.assertNotIn(mod.SUPERSEDED_WRITER_TIME_ENV, env)

    def test_the_time_knob_is_not_the_value_the_library_treats_as_unset(self) -> None:
        """Setting it to zero is how the first pair of builds got two wall clocks."""

        self.assertNotEqual(plan()["mke2fs"]["env"][mod.WRITER_TIME_ENV], "0")

    def test_the_config_comes_from_the_frozen_package_not_the_runner(self) -> None:
        """mke2fs reads its feature defaults from this file."""

        self.assertEqual(plan()["mke2fs"]["env"]["MKE2FS_CONFIG"], CONF)

    def test_locale_and_timezone_are_pinned(self) -> None:
        env = plan()["mke2fs"]["env"]
        self.assertEqual(env["TZ"], "UTC")
        self.assertEqual(env["LC_ALL"], "C")


class StagingTests(unittest.TestCase):
    def test_staging_uses_a_filesystem_whose_readdir_order_is_creation_order(self) -> None:
        """mke2fs walks the staging tree with readdir and never sorts it."""

        staging = plan()["staging"]
        self.assertEqual(staging["filesystem"], "tmpfs")
        self.assertEqual(staging["path"], STAGING)

    def test_staging_entries_are_created_in_logical_path_byte_order(self) -> None:
        paths = [row["path"] for row in plan()["staging"]["entries"]]
        self.assertEqual(paths, sorted(paths, key=lambda value: value.encode("utf-8")))

    def test_every_staged_entry_is_owned_by_root(self) -> None:
        for row in plan()["staging"]["entries"]:
            self.assertEqual(row["uid"], 0)
            self.assertEqual(row["gid"], 0)

    def test_every_staged_entry_carries_the_canonical_timestamp(self) -> None:
        for row in plan()["staging"]["entries"]:
            self.assertEqual(row["mtime"], 0)

    def test_the_staged_kinds_and_modes_match_the_layer(self) -> None:
        rows = {row["path"]: row for row in plan()["staging"]["entries"]}
        self.assertEqual(rows["etc"]["kind"], "directory")
        self.assertEqual(rows["lib"]["kind"], "symlink")
        self.assertEqual(rows["lib"]["target"], "usr/lib")
        self.assertEqual(rows["usr/bin/true"]["kind"], "file")
        self.assertEqual(rows["usr/bin/true"]["mode"], 0o755)


class SizeTests(unittest.TestCase):
    def test_the_size_must_be_stated_and_is_never_guessed(self) -> None:
        """A content-derived size drifts whenever a package changes by one byte."""

        with self.assertRaises(TypeError):
            mod.root_disk_plan(
                layer=LAYER,
                mke2fs=MKE2FS,
                debugfs=DEBUGFS,
                config=CONF,
                image=IMAGE,
                staging=STAGING,
            )

    def test_a_size_that_is_not_a_whole_number_of_blocks_is_rejected(self) -> None:
        with self.assertRaises(mod.RootDiskPlanError):
            plan(sizeBytes=SIZE + 1)

    def test_a_size_too_small_for_the_content_is_rejected(self) -> None:
        with self.assertRaises(mod.RootDiskPlanError):
            plan(sizeBytes=mod.BLOCK_SIZE * 8)

    def test_the_required_size_is_reported_so_the_pin_can_be_chosen(self) -> None:
        self.assertGreater(plan()["sizeBytes"]["required"], 0)
        self.assertEqual(plan()["sizeBytes"]["pinned"], SIZE)


class ToolPinTests(unittest.TestCase):
    def test_the_inspector_is_still_the_one_the_sealed_authority_named(self) -> None:
        """The writer moved and the inspector did not, and that is deliberate.

        Keeping the sealed inspector is what makes the check independent: an
        image written by the new e2fsprogs is read back by a build that knows
        nothing about it, so a defect shared by writer and checker cannot agree
        with itself into a pass.
        """

        by_role = {row["role"]: row for row in sealed_authority()["toolBinaries"]}
        self.assertEqual(by_role["ext4-image-inspector"]["sha256"], mod.DEBUGFS_SHA256)

    def test_the_writer_is_the_replacement_the_selection_record_measured(self) -> None:
        """Not the sealed writer any more -- and not an unrecorded one either.

        The sealed authority's writer is the binary whose defect is sealed in
        the hard-stop record, so the plan pinning it again would mean nothing
        had changed.  The digest it points at instead has to be the build the
        append-only selection record read and returned FIXED for, or the swap
        is to something nobody measured.
        """

        by_role = {row["role"]: row for row in sealed_authority()["toolBinaries"]}
        self.assertNotEqual(by_role["ext4-image-writer"]["sha256"], mod.MKE2FS_SHA256)

        positive = selection_record()["controls"]["positive"]
        self.assertEqual(positive["verdict"], "FIXED")
        self.assertEqual(positive["writer"]["sha256"], mod.MKE2FS_SHA256)
        self.assertEqual(positive["writer"]["sizeBytes"], mod.MKE2FS_SIZE_BYTES)

    def test_the_plan_records_the_digests_the_producer_must_check(self) -> None:
        tools = plan()["tools"]
        self.assertEqual(tools["mke2fs"]["sha256"], mod.MKE2FS_SHA256)
        self.assertEqual(tools["debugfs"]["sha256"], mod.DEBUGFS_SHA256)

    def test_debugfs_stays_an_inspector_and_never_writes_the_image(self) -> None:
        """The sealed authority gives debugfs that role; the plan must not widen it."""

        self.assertEqual(plan()["tools"]["debugfs"]["role"], "ext4-image-inspector")
        flat = json.dumps(plan()["mke2fs"])
        self.assertNotIn(DEBUGFS, flat)


class DeterminismTests(unittest.TestCase):
    def test_the_same_layer_produces_the_same_plan(self) -> None:
        self.assertEqual(mod.canonical_json(plan()), mod.canonical_json(plan()))

    def test_the_plan_names_the_risks_the_byte_comparison_has_to_catch(self) -> None:
        """These are assumptions until two arm64 jobs agree; naming them is the point."""

        ids = {row["id"] for row in plan()["unverifiedAssumptions"]}
        self.assertIn("writer-honours-source-date-epoch", ids)
        self.assertIn("staging-readdir-order-is-creation-order", ids)
        self.assertIn("loader-resolves-only-frozen-libraries", ids)
        for row in plan()["unverifiedAssumptions"]:
            self.assertEqual(row["onMismatch"], "abort-never-relax")


class SharedLibraryTests(unittest.TestCase):
    def test_every_needed_soname_has_a_frozen_provider(self) -> None:
        """Read from the two ELF headers; matched against the 191 frozen packages."""

        sonames = {row["soname"] for row in mod.SHARED_LIBRARIES}
        self.assertEqual(
            sonames,
            {
                "ld-linux-aarch64.so.1",
                "libblkid.so.1",
                "libc.so.6",
                "libcom_err.so.2",
                "libe2p.so.2",
                "libext2fs.so.2",
                "libss.so.2",
                "libuuid.so.1",
            },
        )
        for row in mod.SHARED_LIBRARIES:
            self.assertTrue(row["package"])

    def test_libe2p_comes_from_the_ext2fs_package_not_one_of_its_own(self) -> None:
        """There is no libe2p2t64 in this release; looking for one finds nothing."""

        rows = {row["soname"]: row for row in mod.SHARED_LIBRARIES}
        self.assertEqual(rows["libe2p.so.2"]["package"], "libext2fs2t64")

    def test_the_providers_are_named_in_the_plan(self) -> None:
        self.assertEqual(plan()["sharedLibraries"], mod.SHARED_LIBRARIES)


class BoundaryTests(unittest.TestCase):
    def test_planning_an_image_is_not_building_one(self) -> None:
        self.assertIs(mod.BOOTABLE_CLAIM, False)
        self.assertIs(mod.ACTIVATION_ALLOWED, False)
        self.assertIs(plan()["executed"], False)

    def test_the_module_cannot_run_the_tools_it_plans(self) -> None:
        self.assertFalse(hasattr(mod, "subprocess"))
        self.assertFalse(hasattr(mod, "os"))


if __name__ == "__main__":
    unittest.main()
