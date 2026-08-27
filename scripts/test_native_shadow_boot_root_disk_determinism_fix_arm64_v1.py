#!/usr/bin/env python3
"""Tests for the fix the determinism successor pre-registered.

The successor authority was written before this code existed and says what the
fix has to do: hand the ext4 writer a fixed time that is not the library's unset
sentinel, keep the staged inputs' own epoch separate from it, write down which
library files the loader actually used, and run a read-only filesystem check
whose only passing exit code is zero.

Each test here is one of the rejections that record pre-registered.  They are
written as rejections on purpose.  A test that only confirms the happy path
passes just as well after someone removes the guard, and every one of these
guards exists because its absence already produced a wrong answer once.
"""

from __future__ import annotations

import pathlib
import struct
import tempfile
import unittest
import unittest.mock

from scripts import native_shadow_boot_initrd_arm64_v1 as initrd
from scripts import native_shadow_boot_root_disk_arm64_v1 as root_disk
from scripts import native_shadow_boot_root_disk_execute_arm64_v1 as execute
from scripts import native_shadow_boot_root_disk_time_audit_arm64_v1 as audit
from scripts import native_shadow_boot_produce_phase_arm64_v1 as produce


REPO = pathlib.Path(__file__).resolve().parents[1]

BLOCK = root_disk.BLOCK_SIZE
INODE_SIZE = root_disk.INODE_SIZE


def synthetic_image(
    *,
    superblock_times: tuple[int, int, int, int] = (0, 1, 1, 1),
    inode_times: tuple[int, int, int, int] = (0, 1, 0, 1),
    inode_count: int = 4,
) -> bytes:
    """The smallest ext4-shaped bytes the auditor needs to read times out of.

    A real image is a gigabyte and takes a privileged tool to make.  The auditor
    only reads the superblock, the group descriptors and the inode table, so a
    hand-built image with those three is enough to say whether it reads them
    correctly -- and it can carry a wall clock on purpose, which no real produced
    image is allowed to.
    """

    inode_table_block = 3
    blocks = inode_table_block + 4

    superblock = bytearray(1024)
    s_mtime, s_wtime, s_lastcheck, s_mkfs_time = superblock_times
    struct.pack_into("<I", superblock, 0x00, inode_count)  # s_inodes_count
    struct.pack_into("<I", superblock, 0x04, blocks)  # s_blocks_count_lo
    struct.pack_into("<I", superblock, 0x14, 1)  # s_first_data_block
    struct.pack_into("<I", superblock, 0x18, 0)  # s_log_block_size -> 1024 << 0
    struct.pack_into("<I", superblock, 0x20, blocks)  # s_blocks_per_group
    struct.pack_into("<I", superblock, 0x28, inode_count)  # s_inodes_per_group
    struct.pack_into("<I", superblock, 0x2C, s_mtime)
    struct.pack_into("<I", superblock, 0x30, s_wtime)
    struct.pack_into("<H", superblock, 0x38, 0xEF53)  # s_magic
    struct.pack_into("<I", superblock, 0x40, s_lastcheck)
    struct.pack_into("<H", superblock, 0x58, INODE_SIZE)  # s_inode_size
    struct.pack_into("<I", superblock, 0x60, 0x2C2)  # s_feature_incompat (64BIT)
    struct.pack_into("<I", superblock, 0x108, s_mkfs_time)
    struct.pack_into("<H", superblock, 0xFE, 64)  # s_desc_size

    descriptor = bytearray(64)
    struct.pack_into("<I", descriptor, 0x08, inode_table_block)  # bg_inode_table_lo

    i_atime, i_ctime, i_mtime, i_crtime = inode_times
    table = bytearray(INODE_SIZE * inode_count)
    for index in range(inode_count):
        base = index * INODE_SIZE
        struct.pack_into("<I", table, base + 0x08, i_atime)
        struct.pack_into("<I", table, base + 0x0C, i_ctime)
        struct.pack_into("<I", table, base + 0x10, i_mtime)
        struct.pack_into("<H", table, base + 0x80, 0x20)  # i_extra_isize
        struct.pack_into("<I", table, base + 0x90, i_crtime)

    raw = bytearray(blocks * 1024)
    raw[1024 : 1024 + len(superblock)] = superblock
    raw[2048 : 2048 + len(descriptor)] = descriptor
    offset = inode_table_block * 1024
    raw[offset : offset + len(table)] = table
    return bytes(raw)


class WriterTimeTests(unittest.TestCase):
    """RED 1 and 2: the sentinel is refused and a fixed non-zero time is required."""

    def test_the_writer_is_no_longer_handed_the_unset_sentinel(self) -> None:
        value = root_disk.mke2fs_env(config="/x")[root_disk.WRITER_TIME_ENV]
        self.assertNotEqual(value, "0")

    def test_the_writer_time_is_fixed_and_positive(self) -> None:
        value = root_disk.mke2fs_env(config="/x")[root_disk.WRITER_TIME_ENV]
        self.assertGreater(int(value), 0)
        self.assertEqual(value, root_disk.EXT4_WRITER_TIME)

    def test_an_environment_without_a_writer_time_is_refused(self) -> None:
        with self.assertRaises(execute.RootDiskExecuteError):
            execute.assert_writer_time({"LC_ALL": "C", "TZ": "UTC"})

    def test_an_environment_that_pins_the_sentinel_is_refused(self) -> None:
        with self.assertRaises(execute.RootDiskExecuteError):
            execute.assert_writer_time({root_disk.WRITER_TIME_ENV: "0"})

    def test_an_environment_that_pins_only_the_superseded_name_is_refused(self) -> None:
        """A fixed non-zero time under the wrong name is the worst case here.

        It looks like the fix from the outside -- a real timestamp, pinned, not
        the sentinel -- and the writer reads it, stores it and leaves the clamp
        unarmed, so the staged st_ctime goes into the image exactly as before.
        Nothing downstream would show it: the two replicas would simply differ
        again, and the sentinel test above would have passed.
        """

        with self.assertRaises(execute.RootDiskExecuteError):
            execute.assert_writer_time(
                {root_disk.SUPERSEDED_WRITER_TIME_ENV: root_disk.EXT4_WRITER_TIME}
            )

    def test_the_source_epoch_stays_what_the_staged_inputs_mean(self) -> None:
        self.assertEqual(initrd.CANONICAL_MTIME, 0)
        self.assertNotEqual(str(initrd.CANONICAL_MTIME), root_disk.EXT4_WRITER_TIME)

    def test_the_two_times_are_the_only_values_an_image_may_carry(self) -> None:
        self.assertEqual(
            sorted(root_disk.ALLOWED_TIMESTAMPS),
            sorted({initrd.CANONICAL_MTIME, int(root_disk.EXT4_WRITER_TIME)}),
        )


class SuperblockTimeTests(unittest.TestCase):
    """RED 3: a superblock time outside the closed set is refused."""

    def test_an_image_whose_times_are_the_two_allowed_values_passes(self) -> None:
        report = audit.audit(synthetic_image())
        self.assertEqual(report["violations"], [])
        self.assertTrue(report["passed"])

    def test_a_superblock_time_outside_the_closed_set_is_refused(self) -> None:
        raw = synthetic_image(superblock_times=(0, 1, 1, 7))
        report = audit.audit(raw)
        self.assertFalse(report["passed"])
        self.assertIn("s_mkfs_time", [row["field"] for row in report["violations"]])

    def test_every_superblock_time_field_is_actually_read(self) -> None:
        fields = set()
        for index, name in enumerate(("s_mtime", "s_wtime", "s_lastcheck", "s_mkfs_time")):
            times = [0, 1, 1, 1]
            times[index] = 7
            report = audit.audit(synthetic_image(superblock_times=tuple(times)))
            fields.update(row["field"] for row in report["violations"])
        self.assertEqual(fields, {"s_mtime", "s_wtime", "s_lastcheck", "s_mkfs_time"})


class InodeTimeTests(unittest.TestCase):
    """RED 4: an inode time outside the closed set is refused."""

    def test_an_inode_time_outside_the_closed_set_is_refused(self) -> None:
        report = audit.audit(synthetic_image(inode_times=(0, 1, 0, 9)))
        self.assertFalse(report["passed"])
        self.assertIn("i_crtime", [row["field"] for row in report["violations"]])

    def test_every_inode_time_field_is_actually_read(self) -> None:
        fields = set()
        for index, name in enumerate(("i_atime", "i_ctime", "i_mtime", "i_crtime")):
            times = [0, 1, 0, 1]
            times[index] = 9
            report = audit.audit(synthetic_image(inode_times=tuple(times)))
            fields.update(row["field"] for row in report["violations"])
        self.assertEqual(fields, {"i_atime", "i_ctime", "i_mtime", "i_crtime"})

    def test_the_audit_reads_every_inode_rather_than_a_sample(self) -> None:
        report = audit.audit(synthetic_image(inode_count=4))
        self.assertEqual(report["inodesRead"], 4)


class WallClockTests(unittest.TestCase):
    """RED 5: a surviving wall clock is refused, and named as one."""

    def test_a_wall_clock_in_the_superblock_is_refused_and_named(self) -> None:
        report = audit.audit(synthetic_image(superblock_times=(0, 1787745904, 1, 1)))
        self.assertFalse(report["passed"])
        row = next(r for r in report["violations"] if r["field"] == "s_wtime")
        self.assertTrue(row["looksLikeAWallClock"])

    def test_a_wall_clock_in_an_inode_is_refused_and_named(self) -> None:
        report = audit.audit(synthetic_image(inode_times=(0, 1787745901, 0, 1)))
        self.assertFalse(report["passed"])
        row = next(r for r in report["violations"] if r["field"] == "i_ctime")
        self.assertTrue(row["looksLikeAWallClock"])

    def test_the_wall_clock_boundary_sits_above_every_allowed_value(self) -> None:
        self.assertGreater(root_disk.WALL_CLOCK_LOWER_BOUND, max(root_disk.ALLOWED_TIMESTAMPS))

    def test_a_small_wrong_value_is_still_refused_even_though_it_is_no_clock(self) -> None:
        report = audit.audit(synthetic_image(superblock_times=(0, 1, 1, 2)))
        self.assertFalse(report["passed"])
        row = next(r for r in report["violations"] if r["field"] == "s_mkfs_time")
        self.assertFalse(row["looksLikeAWallClock"])


def evidence(*, sha256: str = "a" * 64, directory: str = "/frozen/lib") -> dict:
    """A complete loader-evidence block, so each test can spoil exactly one thing."""

    libraries = {
        row["soname"]: {
            "package": row["package"],
            "path": None if directory is None else f"{directory}/{row['soname']}",
            "sha256": sha256,
            "sizeBytes": 1,
        }
        for row in root_disk.SHARED_LIBRARIES
    }
    loader = next(name for name in libraries if name.startswith("ld-"))
    return {
        "libraries": libraries,
        "libraryPath": str(directory),
        "loader": dict(libraries[loader], soname=loader),
        "tree": "/frozen",
    }


class LoaderEvidenceTests(unittest.TestCase):
    """RED 6, 7 and 8: the loader's real inputs are recorded and bounded."""

    FROZEN = pathlib.Path("/frozen")

    def test_evidence_missing_a_pinned_library_is_refused(self) -> None:
        spoiled = evidence()
        spoiled["libraries"].pop("libext2fs.so.2")
        with self.assertRaises(execute.RootDiskExecuteError) as caught:
            execute.assert_loader_evidence(spoiled, tree=self.FROZEN)
        self.assertIn("libext2fs.so.2", str(caught.exception))

    def test_evidence_with_no_loader_at_all_is_refused(self) -> None:
        spoiled = evidence()
        spoiled.pop("loader")
        with self.assertRaises(execute.RootDiskExecuteError):
            execute.assert_loader_evidence(spoiled, tree=self.FROZEN)

    def test_evidence_without_a_path_is_refused(self) -> None:
        with self.assertRaises(execute.RootDiskExecuteError) as caught:
            execute.assert_loader_evidence(evidence(directory=None), tree=self.FROZEN)
        self.assertIn("no path", str(caught.exception))

    def test_a_library_outside_the_frozen_tree_is_refused(self) -> None:
        spoiled = evidence()
        spoiled["libraries"]["libc.so.6"]["path"] = "/usr/lib/aarch64-linux-gnu/libc.so.6"
        with self.assertRaises(execute.RootDiskExecuteError) as caught:
            execute.assert_loader_evidence(spoiled, tree=self.FROZEN)
        self.assertIn("outside the frozen tree", str(caught.exception))

    def test_a_loader_outside_the_frozen_tree_is_refused(self) -> None:
        spoiled = evidence()
        spoiled["loader"]["path"] = "/usr/lib/aarch64-linux-gnu/ld-linux-aarch64.so.1"
        with self.assertRaises(execute.RootDiskExecuteError) as caught:
            execute.assert_loader_evidence(spoiled, tree=self.FROZEN)
        self.assertIn("outside the frozen tree", str(caught.exception))

    def test_a_digest_that_is_not_a_digest_is_refused(self) -> None:
        with self.assertRaises(execute.RootDiskExecuteError):
            execute.assert_loader_evidence(evidence(sha256="not-a-digest"), tree=self.FROZEN)

    def test_a_prefix_of_the_tree_path_is_not_inside_the_tree(self) -> None:
        with self.assertRaises(execute.RootDiskExecuteError):
            execute.assert_loader_evidence(
                evidence(directory="/frozen-elsewhere/lib"), tree=self.FROZEN
            )

    def test_evidence_entirely_inside_the_frozen_tree_is_accepted(self) -> None:
        self.assertTrue(execute.assert_loader_evidence(evidence(), tree=self.FROZEN))


class FilesystemCheckTests(unittest.TestCase):
    """RED 9: the check is read-only, forced, and passes only on zero."""

    def test_the_check_is_forced_and_never_repairs(self) -> None:
        argv = root_disk.e2fsck_argv(e2fsck="/frozen/e2fsck", image="/out/img")
        self.assertEqual(argv[0], "/frozen/e2fsck")
        self.assertIn("-f", argv)
        self.assertIn("-n", argv)
        for option in ("-p", "-y", "-a"):
            self.assertNotIn(option, argv)

    def test_only_a_zero_exit_is_a_pass(self) -> None:
        self.assertTrue(execute.fsck_passed(0))
        for code in (1, 2, 4, 8, 16, 32, 128):
            self.assertFalse(execute.fsck_passed(code), code)

    def test_a_result_with_no_check_in_it_is_refused(self) -> None:
        with self.assertRaises(execute.RootDiskExecuteError):
            execute.assert_fsck_ran({"image": {"sha256": "e" * 64}})

    def test_a_check_that_ran_and_failed_is_refused(self) -> None:
        with self.assertRaises(execute.RootDiskExecuteError):
            execute.assert_fsck_ran({"fsck": {"exitCode": 4, "passed": False}})

    def test_a_check_that_ran_and_passed_is_accepted(self) -> None:
        self.assertTrue(execute.assert_fsck_ran({"fsck": {"exitCode": 0, "passed": True}}))


class InspectorStaysAnInspectorTests(unittest.TestCase):
    """RED 12: debugfs is never given the writer's job."""

    @staticmethod
    def plan() -> dict:
        return root_disk.root_disk_plan(
            layer=initrd_layer(),
            mke2fs="/frozen/mke2fs",
            debugfs="/frozen/debugfs",
            e2fsck="/frozen/e2fsck",
            config="/frozen/mke2fs.conf",
            image="/out/img",
            staging="/scratch/staging",
            sizeBytes=root_disk.BLOCK_SIZE * 4096,
        )

    def test_the_plan_still_calls_debugfs_an_inspector(self) -> None:
        plan = self.plan()
        self.assertEqual(plan["tools"]["debugfs"]["role"], "ext4-image-inspector")
        self.assertEqual(plan["tools"]["mke2fs"]["role"], "ext4-image-writer")
        self.assertEqual(plan["tools"]["e2fsck"]["role"], "ext4-image-read-only-checker")

    def test_the_plan_builds_no_argv_for_the_inspector(self) -> None:
        plan = self.plan()
        self.assertIn("argv", plan["mke2fs"])
        self.assertIn("argv", plan["e2fsck"])
        self.assertNotIn("debugfs", {key for key in plan if isinstance(plan[key], dict) and "argv" in plan[key]})

    def test_the_checker_never_writes_either(self) -> None:
        argv = root_disk.e2fsck_argv(e2fsck="/frozen/e2fsck", image="/out/img")
        self.assertNotIn("-w", argv)


class ProducedEvidenceTests(unittest.TestCase):
    """RED 6 to 9 at the artifact level: the uploaded result carries the evidence.

    The executor already settled all of this and the produce phase then kept only
    the image digest, so `PRODUCE-RESULT.json` -- the file a reader actually gets
    -- said nothing about which libraries wrote the image.  That is the gap the
    sealed record had to leave open as an unsettled assumption.
    """

    def result(self, **overrides) -> dict:
        row = {
            "fsck": {"exitCode": 0, "passed": True},
            "image": {"sha256": "f" * 64},
            "loaderEvidence": evidence(),
            "timeAudit": {"passed": True},
            "toolDigests": {"mke2fs": "a" * 64},
            "writerTime": 1,
        }
        row.update(overrides)
        return row

    def test_the_kept_evidence_is_exactly_what_the_executor_settled(self) -> None:
        kept = produce.root_disk_evidence(self.result())
        self.assertEqual(
            sorted(kept),
            ["fsck", "loaderEvidence", "timeAudit", "toolDigests", "writerTime"],
        )

    def test_a_result_with_no_checker_verdict_is_an_error_not_an_absent_key(self) -> None:
        row = self.result()
        row.pop("fsck")
        with self.assertRaises(produce.ProducePhaseError) as caught:
            produce.root_disk_evidence(row)
        self.assertIn("fsck", str(caught.exception))

    def test_a_result_with_no_loader_evidence_is_refused(self) -> None:
        row = self.result()
        row.pop("loaderEvidence")
        with self.assertRaises(produce.ProducePhaseError):
            produce.root_disk_evidence(row)

    def test_a_result_with_no_writer_time_is_refused(self) -> None:
        row = self.result()
        row.pop("writerTime")
        with self.assertRaises(produce.ProducePhaseError):
            produce.root_disk_evidence(row)

    def test_the_executor_returns_every_field_the_produce_phase_keeps(self) -> None:
        """Neither side may quietly stop carrying one of them."""

        source = (
            REPO / "scripts/native_shadow_boot_root_disk_execute_arm64_v1.py"
        ).read_text(encoding="utf-8")
        for name in produce.ROOT_DISK_EVIDENCE_FIELDS:
            self.assertIn(f'"{name}": ', source, name)


class ReplicaEvidenceTests(unittest.TestCase):
    """The compare stage refuses a replica that cannot show what it did.

    The executor stops a bad run inside the job, so this exists for the other
    case: a result that never ran a check and reads like one that passed.
    """

    def result(self, **overrides) -> dict:
        row = {
            "rootDisk": {"sha256": "f" * 64},
            "rootDiskEvidence": {
                "fsck": {"exitCode": 0, "passed": True},
                "loaderEvidence": evidence(),
                "timeAudit": {"passed": True, "violationCount": 0},
                "toolDigests": {"mke2fs": "a" * 64},
                "writerTime": 1,
            },
        }
        row["rootDiskEvidence"].update(overrides)
        return row

    def test_a_replica_that_proved_itself_is_accepted(self) -> None:
        summary = produce.assert_replica_evidence(self.result())
        self.assertEqual(summary["fsckExitCode"], 0)
        self.assertEqual(summary["writerTime"], 1)
        self.assertEqual(summary["librariesRecorded"], len(root_disk.SHARED_LIBRARIES))

    def test_a_result_with_no_evidence_block_at_all_is_refused(self) -> None:
        with self.assertRaises(produce.ProducePhaseError):
            produce.assert_replica_evidence({"rootDisk": {"sha256": "f" * 64}})

    def test_a_non_zero_checker_exit_is_refused(self) -> None:
        with self.assertRaises(produce.ProducePhaseError):
            produce.assert_replica_evidence(self.result(fsck={"exitCode": 1}))

    def test_a_failed_time_audit_is_refused(self) -> None:
        with self.assertRaises(produce.ProducePhaseError):
            produce.assert_replica_evidence(
                self.result(timeAudit={"passed": False, "violationCount": 3})
            )

    def test_a_sentinel_writer_time_is_refused(self) -> None:
        with self.assertRaises(produce.ProducePhaseError):
            produce.assert_replica_evidence(self.result(writerTime=0))

    def test_a_library_outside_the_frozen_tree_is_refused_here_too(self) -> None:
        spoiled = evidence()
        spoiled["libraries"]["libc.so.6"]["path"] = "/usr/lib/aarch64-linux-gnu/libc.so.6"
        with self.assertRaises(execute.RootDiskExecuteError):
            produce.assert_replica_evidence(self.result(loaderEvidence=spoiled))


class CheckerBindingTests(unittest.TestCase):
    """RED 11 adjacent: the checker is bound without editing a sealed authority."""

    def test_the_checker_comes_from_the_package_the_writer_came_from(self) -> None:
        authority = produce.builder_authority()
        packages = {row["packageSha256"] for row in authority["toolBinaries"]}
        self.assertEqual(packages, {root_disk.E2FSPROGS_PACKAGE_SHA256})

    def test_the_sealed_authority_still_pins_only_the_two_original_tools(self) -> None:
        """Binding the checker must not have rewritten the record of the first pair."""

        authority = produce.builder_authority()
        self.assertEqual(
            sorted(row["role"] for row in authority["toolBinaries"]),
            ["ext4-image-inspector", "ext4-image-writer"],
        )

    def test_the_checker_path_sits_inside_the_frozen_tree(self) -> None:
        tools = produce.tool_paths(pathlib.Path("/frozen"))
        self.assertEqual(tools["e2fsck"], "/frozen/usr/sbin/e2fsck")
        self.assertTrue(tools["mke2fs"].startswith("/frozen/"))

    def test_a_tool_from_some_other_package_is_refused(self) -> None:
        authority = {
            "toolBinaries": [
                {"role": "ext4-image-writer", "memberPath": "./usr/sbin/mke2fs", "packageSha256": "0" * 64},
                {"role": "ext4-image-inspector", "memberPath": "./usr/sbin/debugfs", "packageSha256": "0" * 64},
            ]
        }
        with self.assertRaises(produce.ProducePhaseError) as caught:
            produce.tool_paths(pathlib.Path("/frozen"), authority=authority)
        self.assertIn("frozen e2fsprogs package", str(caught.exception))


def initrd_layer() -> bytes:
    """A minimal tar layer, borrowed from the plan module's own test fixture."""

    import io
    import tarfile

    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w") as archive:
        for name in ("usr", "usr/bin"):
            info = tarfile.TarInfo(name)
            info.type = tarfile.DIRTYPE
            info.mode = 0o755
            info.mtime = 0
            archive.addfile(info)
        payload = b"boole"
        info = tarfile.TarInfo("usr/bin/tool")
        info.size = len(payload)
        info.mode = 0o755
        info.mtime = 0
        archive.addfile(info, io.BytesIO(payload))
    return buffer.getvalue()


class ProductionGateTests(unittest.TestCase):
    """The one production pair is not spent on a cause already known to be open.

    A second pass over the frozen `mke2fs` found that the `-d` copy path
    overwrites `i_ctime` from the staging filesystem, which no fixed writer time
    reaches.  The audit above would catch such an image on both replicas, so the
    fault is loud rather than silent.  The reason to refuse the dispatch anyway is
    that the record allows one pair and forbids retrying a pair that produced a
    result -- spending it on a known outcome is spending it for nothing.
    """

    def test_the_live_record_currently_blocks_production(self) -> None:
        with self.assertRaises(produce.ProducePhaseError):
            produce.assert_production_unblocked()

    def test_the_refusal_names_the_cause_so_it_can_be_looked_up(self) -> None:
        with self.assertRaises(produce.ProducePhaseError) as caught:
            produce.assert_production_unblocked()
        self.assertIn("staged-inode-ctime-is-not-fs-now", str(caught.exception))

    def test_a_record_with_nothing_open_lets_production_start(self) -> None:
        """The gate opens by closing the cause, not by deleting the gate."""

        self.assertTrue(produce.assert_production_unblocked({"productionReadiness": {}}))
        self.assertTrue(
            produce.assert_production_unblocked({"productionReadiness": {"blocked": False}})
        )

    def test_a_record_that_blocks_without_naming_a_cause_still_blocks(self) -> None:
        with self.assertRaises(produce.ProducePhaseError):
            produce.assert_production_unblocked({"productionReadiness": {"blocked": True}})

    def test_the_gate_reads_the_successor_record_and_not_a_copy_of_it(self) -> None:
        self.assertTrue(produce.SUCCESSOR_AUTHORITY_PATH.is_file())
        self.assertIn(
            "successor-authority", produce.SUCCESSOR_AUTHORITY_PATH.name
        )

    def test_the_gate_is_wired_into_the_entry_point_the_dispatch_actually_calls(
        self,
    ) -> None:
        """A gate that exists and is never called is not a gate.

        `scripts/native-shadow-boot-produce-arm64.sh` -- the script the workflow
        runs under sudo -- invokes this module's `produce` subcommand, so that is
        the function the dispatch reaches.  Running as root is checked first, so
        this stands the effective UID at 0 to get past it and asserts the refusal
        that follows is the one naming the open cause.
        """

        with unittest.mock.patch.object(produce.os, "geteuid", return_value=0):
            with self.assertRaises(produce.ProducePhaseError) as caught:
                produce.produce(
                    scratch=pathlib.Path("/nonexistent/scratch"),
                    outputs=pathlib.Path("/nonexistent/outputs"),
                    gpgv=pathlib.Path("/nonexistent/gpgv"),
                    zstd=pathlib.Path("/nonexistent/zstd"),
                    launcher=pathlib.Path("/nonexistent/launcher"),
                )
        self.assertIn("staged-inode-ctime-is-not-fs-now", str(caught.exception))

    def test_the_gate_refuses_before_anything_is_written(self) -> None:
        """Refusing after the output directory exists would leave a half-run behind."""

        with tempfile.TemporaryDirectory() as scratch:
            outputs = pathlib.Path(scratch) / "outputs"
            with unittest.mock.patch.object(produce.os, "geteuid", return_value=0):
                with self.assertRaises(produce.ProducePhaseError):
                    produce.produce(
                        scratch=pathlib.Path(scratch),
                        outputs=outputs,
                        gpgv=pathlib.Path("/nonexistent/gpgv"),
                        zstd=pathlib.Path("/nonexistent/zstd"),
                        launcher=pathlib.Path("/nonexistent/launcher"),
                    )
            self.assertFalse(outputs.exists())


if __name__ == "__main__":
    unittest.main()
