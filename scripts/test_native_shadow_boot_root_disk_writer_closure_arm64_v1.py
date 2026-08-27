#!/usr/bin/env python3
"""The writer stops being the guest's own mke2fs, and stays separable from it.

The frozen writer copies each staged file's `st_ctime` into the image, so the
image carries whatever the staging filesystem stamped and two replicas cannot
agree.  The selected replacement clamps that field to a fixed time -- but only
when the library arms a flag, and only `SOURCE_DATE_EPOCH` arms it.  The
variable the plan used to set does not, so swapping the binary without swapping
the variable would reproduce the sealed failure with a newer tool.

Changing the writer opens a hazard the old arrangement did not have.  There is
now more than one build of `libext2fs.so.2` in play, and a loader handed the
wrong one would run the new writer against the old library -- the flag never
armed, the defect back, and nothing in the output saying so.  So the two
closures are described separately and are required to differ in exactly the two
libraries the writer set supplies, and to agree everywhere else.

Nothing here runs mke2fs.  These are properties of the plan and of the
executor's checks, read without producing an image.
"""
from __future__ import annotations

import unittest

from scripts import native_shadow_boot_root_disk_arm64_v1 as plan_mod
from scripts import native_shadow_boot_root_disk_execute_arm64_v1 as execute
from scripts.test_native_shadow_boot_initrd_arm64_v1 import SMALL, tar_bytes

WRITER_SUPPLIED = {"libext2fs.so.2", "libe2p.so.2"}
LAYER = tar_bytes(SMALL)


def plan() -> dict:
    return plan_mod.root_disk_plan(
        layer=LAYER,
        mke2fs="/writer/usr/sbin/mke2fs",
        debugfs="/frozen/usr/sbin/debugfs",
        e2fsck="/frozen/usr/sbin/e2fsck",
        config="/frozen/etc/mke2fs.conf",
        image="/out/root.img",
        staging="/scratch/staging",
        sizeBytes=64 * 1024 * 1024,
    )


class WriterTimeTests(unittest.TestCase):
    """Which variable is set, and why the other one is not enough."""

    def test_the_plan_sets_the_variable_the_selected_build_honours(self) -> None:
        self.assertEqual(plan_mod.WRITER_TIME_ENV, "SOURCE_DATE_EPOCH")
        env = plan()["mke2fs"]["env"]
        self.assertEqual(env[plan_mod.WRITER_TIME_ENV], plan_mod.EXT4_WRITER_TIME)

    def test_the_superseded_variable_is_not_set_as_well(self) -> None:
        # Setting both would read as belt and braces and is the opposite. In the
        # selected build E2FSPROGS_FAKE_TIME sets the time without arming the
        # flag, so a plan carrying it could look correct while producing the old
        # failure. Which binary runs is fixed by its digest, not by a spare
        # variable.
        self.assertNotIn("E2FSPROGS_FAKE_TIME", plan()["mke2fs"]["env"])

    def test_the_fixed_time_is_neither_the_unset_sentinel_nor_a_wall_clock(self) -> None:
        value = int(plan_mod.EXT4_WRITER_TIME)
        self.assertNotEqual(value, 0)
        self.assertLess(value, execute.WALL_CLOCK_LOWER_BOUND)

    def test_the_executor_reads_the_same_variable_the_plan_writes(self) -> None:
        self.assertEqual(
            execute.assert_writer_time(plan()["mke2fs"]["env"]),
            int(plan_mod.EXT4_WRITER_TIME),
        )

    def test_the_executor_refuses_the_unset_sentinel_and_the_wall_clock(self) -> None:
        for bad in ("0", str(execute.WALL_CLOCK_LOWER_BOUND)):
            with self.assertRaises(execute.RootDiskExecuteError):
                execute.assert_writer_time({plan_mod.WRITER_TIME_ENV: bad})

    def test_the_executor_refuses_an_environment_that_only_sets_the_old_variable(
        self,
    ) -> None:
        with self.assertRaises(execute.RootDiskExecuteError) as caught:
            execute.assert_writer_time({"E2FSPROGS_FAKE_TIME": plan_mod.EXT4_WRITER_TIME})
        self.assertIn(execute.ABORT_WRITER_TIME, str(caught.exception))


class WriterClosureTests(unittest.TestCase):
    """What the writer links against, and where each library comes from."""

    def test_the_writer_closure_is_exactly_what_the_writer_links(self) -> None:
        sonames = {row["soname"] for row in plan_mod.WRITER_LIBRARIES}
        self.assertEqual(sonames, set(plan_mod.WRITER_NEEDED))
        self.assertEqual(len(plan_mod.WRITER_LIBRARIES), len(plan_mod.WRITER_NEEDED))

    def test_the_writer_set_supplies_exactly_the_exact_version_libraries(self) -> None:
        supplied = {
            row["soname"]
            for row in plan_mod.WRITER_LIBRARIES
            if row["origin"] == plan_mod.ORIGIN_WRITER_SET
        }
        self.assertEqual(supplied, WRITER_SUPPLIED)

    def test_every_writer_set_library_is_pinned_by_digest(self) -> None:
        for row in plan_mod.WRITER_LIBRARIES:
            if row["origin"] != plan_mod.ORIGIN_WRITER_SET:
                self.assertNotIn("sha256", row, row["soname"])
                continue
            self.assertRegex(row["sha256"], r"^[0-9a-f]{64}$")
            self.assertGreater(row["sizeBytes"], 0)
            self.assertEqual(row["package"], "libext2fs2t64")

    def test_the_rest_of_the_closure_comes_from_the_frozen_guest(self) -> None:
        frozen = [
            row
            for row in plan_mod.WRITER_LIBRARIES
            if row["origin"] == plan_mod.ORIGIN_FROZEN_GUEST
        ]
        self.assertEqual(len(frozen), len(plan_mod.WRITER_NEEDED) - len(WRITER_SUPPLIED))
        checker = {row["soname"] for row in plan_mod.SHARED_LIBRARIES}
        for row in frozen:
            self.assertIn(row["soname"], checker, row["soname"])

    def test_the_two_closures_differ_in_exactly_the_writer_set_libraries(self) -> None:
        writer = {row["soname"]: row for row in plan_mod.WRITER_LIBRARIES}
        checker = {row["soname"]: row for row in plan_mod.SHARED_LIBRARIES}
        differing = {
            soname
            for soname, row in writer.items()
            if row["origin"] == plan_mod.ORIGIN_WRITER_SET
        }
        self.assertEqual(differing, WRITER_SUPPLIED)
        for soname in set(writer) - differing:
            self.assertEqual(writer[soname]["logicalPath"], checker[soname]["logicalPath"])

    def test_the_checker_closure_is_left_alone(self) -> None:
        # debugfs and e2fsck stay on the frozen build, so their closure is the
        # one that was pinned before the writer was chosen.
        self.assertEqual(len(plan_mod.SHARED_LIBRARIES), 8)
        self.assertIn("libss.so.2", {row["soname"] for row in plan_mod.SHARED_LIBRARIES})
        self.assertNotIn("libss.so.2", set(plan_mod.WRITER_NEEDED))

    def test_the_plan_carries_both_closures_under_their_own_names(self) -> None:
        document = plan()
        self.assertEqual(document["sharedLibraries"], plan_mod.SHARED_LIBRARIES)
        self.assertEqual(document["writerLibraries"], plan_mod.WRITER_LIBRARIES)


class WriterToolTests(unittest.TestCase):
    """Which binary writes, and which binaries judge."""

    def test_the_writer_is_the_selected_build_and_the_checkers_are_not(self) -> None:
        tools = plan()["tools"]
        self.assertEqual(tools["mke2fs"]["sha256"], plan_mod.MKE2FS_SHA256)
        self.assertEqual(tools["mke2fs"]["packageSha256"], plan_mod.WRITER_PACKAGE_SHA256)
        self.assertNotEqual(
            tools["mke2fs"]["packageSha256"], plan_mod.E2FSPROGS_PACKAGE_SHA256
        )
        for checker in ("debugfs", "e2fsck"):
            self.assertNotEqual(tools[checker]["sha256"], plan_mod.MKE2FS_SHA256)
        self.assertEqual(
            tools["e2fsck"]["packageSha256"], plan_mod.E2FSPROGS_PACKAGE_SHA256
        )

    def test_the_writer_digest_is_the_one_the_static_read_accepted(self) -> None:
        self.assertEqual(
            plan_mod.MKE2FS_SHA256,
            "d20ac1862b68136bd1ffc03b19d3be24733d714345c2e1531178d13d3ddb5945",
        )
        self.assertEqual(plan_mod.MKE2FS_SIZE_BYTES, 199208)

    def test_the_ext4_argument_vector_is_unchanged(self) -> None:
        # The feature set and the file layout are this argument vector. A new
        # writer is allowed to change which bytes the timestamps hold; it is not
        # allowed to change what filesystem is being asked for.
        argv = plan()["mke2fs"]["argv"]
        for flag, value in (
            ("-t", "ext4"),
            ("-b", str(plan_mod.BLOCK_SIZE)),
            ("-I", str(plan_mod.INODE_SIZE)),
            ("-m", str(plan_mod.RESERVED_BLOCK_PERCENT)),
            ("-U", plan_mod.EXT4_UUID),
        ):
            self.assertEqual(argv[argv.index(flag) + 1], value)
        self.assertIn(f"hash_seed={plan_mod.EXT4_HASH_SEED},root_owner=0:0", argv)


class VersionMixingTests(unittest.TestCase):
    """The one failure this change introduces, and the check that catches it."""

    def _resolved(self) -> tuple:
        writer = {}
        checker = {}
        for row in plan_mod.SHARED_LIBRARIES:
            checker[row["soname"]] = {
                "path": f"/frozen{row['logicalPath']}",
                "sha256": "0" * 63 + "1",
                "sizeBytes": 1,
            }
        for row in plan_mod.WRITER_LIBRARIES:
            if row["origin"] == plan_mod.ORIGIN_WRITER_SET:
                resolved = {
                    "path": f"/writer{row['logicalPath']}",
                    "sha256": row["sha256"],
                    "sizeBytes": row["sizeBytes"],
                }
            else:
                resolved = dict(checker[row["soname"]])
                resolved["path"] = f"/writer{row['logicalPath']}"
            writer[row["soname"]] = resolved
        return writer, checker

    def test_a_matching_pair_of_closures_is_accepted(self) -> None:
        writer, checker = self._resolved()
        execute.assert_no_version_mixing(writer, checker)

    def test_the_old_library_under_the_new_writer_is_refused(self) -> None:
        # The failure worth naming: the new mke2fs resolves the frozen
        # libext2fs, the flag is never armed, and the image silently carries
        # staging times again.
        writer, checker = self._resolved()
        writer["libext2fs.so.2"] = dict(checker["libext2fs.so.2"])
        with self.assertRaises(execute.RootDiskExecuteError) as caught:
            execute.assert_no_version_mixing(writer, checker)
        self.assertIn("libext2fs.so.2", str(caught.exception))

    def test_a_shared_library_that_is_not_shared_is_refused(self) -> None:
        # Everything outside the writer set has to be the same bytes in both
        # closures. A writer-side libc that differs means the writer tree was
        # built from something other than the frozen guest.
        writer, checker = self._resolved()
        writer["libc.so.6"] = dict(writer["libc.so.6"], sha256="f" * 64)
        with self.assertRaises(execute.RootDiskExecuteError) as caught:
            execute.assert_no_version_mixing(writer, checker)
        self.assertIn("libc.so.6", str(caught.exception))

    def test_a_missing_library_is_refused_rather_than_skipped(self) -> None:
        writer, checker = self._resolved()
        del writer["libuuid.so.1"]
        with self.assertRaises(execute.RootDiskExecuteError):
            execute.assert_no_version_mixing(writer, checker)

    def test_the_abort_is_named_so_a_reader_can_find_it(self) -> None:
        writer, checker = self._resolved()
        writer["libe2p.so.2"] = dict(writer["libe2p.so.2"], sha256="e" * 64)
        with self.assertRaises(execute.RootDiskExecuteError) as caught:
            execute.assert_no_version_mixing(writer, checker)
        self.assertIn(execute.ABORT_LIBRARY_VERSION_MIXED, str(caught.exception))


class AssumptionTests(unittest.TestCase):
    """What the plan still does not know."""

    def test_the_writer_time_assumption_names_what_it_replaces(self) -> None:
        rows = {row["id"]: row for row in plan_mod.UNVERIFIED_ASSUMPTIONS}
        self.assertIn("writer-honours-source-date-epoch", rows)
        row = rows["writer-honours-source-date-epoch"]
        self.assertEqual(row["onMismatch"], "abort-never-relax")
        self.assertIn("E2FSPROGS_FAKE_TIME", row["detail"])
        self.assertNotIn("fake-time-honoured-by-this-build", rows)

    def test_the_loader_assumption_now_covers_two_closures(self) -> None:
        rows = {row["id"]: row for row in plan_mod.UNVERIFIED_ASSUMPTIONS}
        self.assertIn("loader-resolves-only-frozen-libraries", rows)
        self.assertIn("writer", rows["loader-resolves-only-frozen-libraries"]["detail"])


if __name__ == "__main__":
    unittest.main()
