#!/usr/bin/env python3
"""Tests for executing the frozen ext4 root disk plan on the arm64 producer.

The plan says what to write and runs nothing; something has to run it.  What
that something must not do is decide anything: the staging order, the modes, the
ownership, the argv and the environment all come from the plan, and the layer
supplies the bytes.  These tests pin the two halves that can silently disagree
-- the plan's metadata against the layer's contents, and the tools on disk
against the digests the plan froze -- and pin that a disagreement stops the
build with the producer authority's own abort id.

``mke2fs`` is an aarch64 ELF, so nothing here runs it.  The staging tree, the
cross-check and the refusals are all host-testable, and they are what these
tests cover; whether the two independent jobs agree is settled by the byte
comparison, not here.
"""

from __future__ import annotations

import json
import os
import pathlib
import tempfile
import unittest

from scripts import native_shadow_boot_root_disk_arm64_v1 as plan_mod
from scripts import native_shadow_boot_root_disk_execute_arm64_v1 as mod
from scripts.test_native_shadow_boot_initrd_arm64_v1 import SMALL, tar_bytes


LAYER = tar_bytes(SMALL)


def a_plan(scratch: pathlib.Path, **overrides):
    kwargs = {
        "layer": LAYER,
        "mke2fs": str(scratch / "mke2fs"),
        "debugfs": str(scratch / "debugfs"),
        "e2fsck": str(scratch / "e2fsck"),
        "config": str(scratch / "mke2fs.conf"),
        "image": str(scratch / "guest-root-disk"),
        "staging": str(scratch / "staging"),
        "sizeBytes": 64 * 1024 * 1024,
    }
    kwargs.update(overrides)
    return plan_mod.root_disk_plan(**kwargs)


class StagingTreeTests(unittest.TestCase):
    def stage(self, **overrides) -> tuple[pathlib.Path, list[str]]:
        scratch = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))
        plan = a_plan(scratch, **overrides)
        created = mod.stage_tree(plan, LAYER)
        return scratch / "staging", created

    def enterContext(self, cm):  # Python 3.9 has no unittest.enterContext
        result = cm.__enter__()
        self.addCleanup(cm.__exit__, None, None, None)
        return result

    def test_entries_are_created_in_the_order_the_plan_lists(self) -> None:
        scratch = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))
        plan = a_plan(scratch)
        created = mod.stage_tree(plan, LAYER)
        self.assertEqual(created, [row["path"] for row in plan["staging"]["entries"]])

    def test_a_file_gets_the_bytes_the_layer_carries(self) -> None:
        root, _ = self.stage()
        for entry in plan_mod.layer_entries(LAYER):
            if entry["kind"] == "file":
                self.assertEqual((root / entry["name"]).read_bytes(), entry["data"])

    def test_a_file_gets_the_mode_the_plan_froze(self) -> None:
        scratch = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))
        plan = a_plan(scratch)
        mod.stage_tree(plan, LAYER)
        for row in plan["staging"]["entries"]:
            if row["kind"] == "symlink":
                continue
            path = scratch / "staging" / row["path"]
            self.assertEqual(os.stat(path).st_mode & 0o7777, row["mode"])

    def test_a_symlink_points_where_the_plan_says(self) -> None:
        scratch = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))
        plan = a_plan(scratch)
        mod.stage_tree(plan, LAYER)
        rows = [row for row in plan["staging"]["entries"] if row["kind"] == "symlink"]
        self.assertTrue(rows, "the fixture layer carries no symlink to check")
        for row in rows:
            path = scratch / "staging" / row["path"]
            self.assertEqual(os.readlink(path), row["target"])

    def test_times_are_the_canonical_one_not_the_wall_clock(self) -> None:
        scratch = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))
        plan = a_plan(scratch)
        mod.stage_tree(plan, LAYER)
        for row in plan["staging"]["entries"]:
            path = scratch / "staging" / row["path"]
            stamp = os.stat(path, follow_symlinks=False).st_mtime
            self.assertEqual(int(stamp), plan_mod.CANONICAL_MTIME)

    def test_staging_twice_into_the_same_root_is_refused(self) -> None:
        scratch = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))
        plan = a_plan(scratch)
        mod.stage_tree(plan, LAYER)
        with self.assertRaises(mod.RootDiskExecuteError):
            mod.stage_tree(plan, LAYER)


class CrossCheckTests(unittest.TestCase):
    def enterContext(self, cm):
        result = cm.__enter__()
        self.addCleanup(cm.__exit__, None, None, None)
        return result

    def scratch(self) -> pathlib.Path:
        return pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))

    def test_a_layer_the_plan_was_not_made_from_is_refused(self) -> None:
        scratch = self.scratch()
        plan = a_plan(scratch)
        other = tar_bytes(SMALL[:-1])
        with self.assertRaises(mod.RootDiskExecuteError):
            mod.stage_tree(plan, other)

    def test_a_plan_that_says_it_already_ran_is_refused(self) -> None:
        scratch = self.scratch()
        plan = a_plan(scratch)
        plan["executed"] = True
        with self.assertRaises(mod.RootDiskExecuteError):
            mod.stage_tree(plan, LAYER)

    def test_a_staging_path_that_escapes_the_root_is_refused(self) -> None:
        scratch = self.scratch()
        plan = a_plan(scratch)
        plan["staging"]["entries"][0]["path"] = "../escaped"
        with self.assertRaises(mod.RootDiskExecuteError):
            mod.stage_tree(plan, LAYER)

    def test_an_absolute_staging_path_is_refused(self) -> None:
        scratch = self.scratch()
        plan = a_plan(scratch)
        plan["staging"]["entries"][0]["path"] = "/etc/passwd"
        with self.assertRaises(mod.RootDiskExecuteError):
            mod.stage_tree(plan, LAYER)

    def test_an_entry_owned_by_someone_other_than_root_is_refused(self) -> None:
        scratch = self.scratch()
        plan = a_plan(scratch)
        plan["staging"]["entries"][0]["uid"] = 1000
        with self.assertRaises(mod.RootDiskExecuteError):
            mod.stage_tree(plan, LAYER)


class ToolDigestTests(unittest.TestCase):
    def enterContext(self, cm):
        result = cm.__enter__()
        self.addCleanup(cm.__exit__, None, None, None)
        return result

    def test_a_tool_whose_bytes_differ_stops_the_build(self) -> None:
        scratch = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))
        plan = a_plan(scratch)
        (scratch / "mke2fs").write_bytes(b"not the frozen tool")
        (scratch / "debugfs").write_bytes(b"not the frozen tool either")
        with self.assertRaises(mod.RootDiskExecuteError) as caught:
            mod.assert_tools(plan)
        self.assertIn(mod.ABORT_TOOL_DIGEST_MISMATCH, str(caught.exception))

    def test_a_tool_that_is_not_there_stops_the_build(self) -> None:
        scratch = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))
        plan = a_plan(scratch)
        with self.assertRaises(mod.RootDiskExecuteError) as caught:
            mod.assert_tools(plan)
        self.assertIn(mod.ABORT_TOOL_DIGEST_MISMATCH, str(caught.exception))

    def test_the_expected_digests_come_from_the_plan_not_from_here(self) -> None:
        source = pathlib.Path(
            "scripts/native_shadow_boot_root_disk_execute_arm64_v1.py"
        ).read_text(encoding="utf-8")
        self.assertNotIn(plan_mod.MKE2FS_SHA256, source)
        self.assertNotIn(plan_mod.DEBUGFS_SHA256, source)

    def test_the_abort_id_is_the_producer_authority_s_own(self) -> None:
        document = json.loads(
            pathlib.Path(
                "native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json"
            ).read_text(encoding="utf-8")
        )
        ids = {row["id"] for row in document["abortConditions"]}
        self.assertIn(mod.ABORT_TOOL_DIGEST_MISMATCH, ids)


class CommandTests(unittest.TestCase):
    def enterContext(self, cm):
        result = cm.__enter__()
        self.addCleanup(cm.__exit__, None, None, None)
        return result

    def test_the_argv_is_the_plan_s_argv_unchanged(self) -> None:
        scratch = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))
        plan = a_plan(scratch)
        self.assertEqual(mod.mke2fs_command(plan), list(plan["mke2fs"]["argv"]))

    def test_the_environment_is_the_plan_s_and_nothing_inherited(self) -> None:
        scratch = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))
        plan = a_plan(scratch)
        environment = mod.mke2fs_environment(plan)
        self.assertEqual(environment, dict(plan["mke2fs"]["env"]))
        self.assertNotIn("PATH", environment)
        self.assertNotIn("SOURCE_DATE_EPOCH", environment)

    def test_nothing_in_this_module_reaches_the_network(self) -> None:
        source = pathlib.Path(
            "scripts/native_shadow_boot_root_disk_execute_arm64_v1.py"
        ).read_text(encoding="utf-8")
        for forbidden in ("urllib", "socket", "http", "requests"):
            self.assertNotIn(forbidden, source)


class FrozenLoaderTests(unittest.TestCase):
    """The runner is Ubuntu 24.04 arm64 too, and that is exactly the hazard.

    Left alone, the loader would find the runner's own ``libext2fs.so.2`` before
    the frozen one, and the image would be written by a tool the closure never
    pinned.  The plan lists the eight sonames it needs and says which copy wins
    is a run-time fact it cannot settle; settling it means naming the frozen
    loader and the frozen library directory explicitly, and recording what was
    named.
    """

    def enterContext(self, cm):
        result = cm.__enter__()
        self.addCleanup(cm.__exit__, None, None, None)
        return result

    def tree(self) -> pathlib.Path:
        root = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))
        for row in plan_mod.SHARED_LIBRARIES:
            path = root / row["logicalPath"].lstrip("/")
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(row["soname"].encode("utf-8"))
        return root

    def test_the_plan_s_argv_is_kept_whole_after_the_loader(self) -> None:
        scratch = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))
        plan = a_plan(scratch)
        argv = mod.frozen_invocation(plan, self.tree())
        self.assertEqual(argv[-len(plan["mke2fs"]["argv"]) :], plan["mke2fs"]["argv"])

    def test_the_loader_that_runs_it_is_the_one_in_the_tree(self) -> None:
        scratch = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))
        tree = self.tree()
        argv = mod.frozen_invocation(a_plan(scratch), tree)
        self.assertEqual(
            argv[0], str(tree / "usr/lib/aarch64-linux-gnu/ld-linux-aarch64.so.1")
        )

    def test_only_the_frozen_library_directory_is_searched(self) -> None:
        scratch = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))
        tree = self.tree()
        argv = mod.frozen_invocation(a_plan(scratch), tree)
        self.assertEqual(argv[1], "--library-path")
        self.assertEqual(argv[2], str(tree / "usr/lib/aarch64-linux-gnu"))

    def test_a_tree_without_the_loader_is_refused(self) -> None:
        scratch = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))
        empty = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))
        with self.assertRaises(mod.RootDiskExecuteError):
            mod.frozen_invocation(a_plan(scratch), empty)

    def test_a_tree_missing_one_pinned_library_is_refused(self) -> None:
        scratch = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))
        tree = self.tree()
        (tree / plan_mod.SHARED_LIBRARIES[-1]["logicalPath"].lstrip("/")).unlink()
        with self.assertRaises(mod.RootDiskExecuteError) as caught:
            mod.frozen_invocation(a_plan(scratch), tree)
        self.assertIn(plan_mod.SHARED_LIBRARIES[-1]["soname"], str(caught.exception))

    def test_the_copies_that_were_named_are_recorded(self) -> None:
        tree = self.tree()
        resolved = mod.resolved_libraries(tree)
        self.assertEqual(
            sorted(resolved), sorted(row["soname"] for row in plan_mod.SHARED_LIBRARIES)
        )
        for row in resolved.values():
            self.assertEqual(len(row["sha256"]), 64)

    def test_the_soname_list_comes_from_the_plan_not_from_here(self) -> None:
        source = pathlib.Path(
            "scripts/native_shadow_boot_root_disk_execute_arm64_v1.py"
        ).read_text(encoding="utf-8")
        for row in plan_mod.SHARED_LIBRARIES:
            self.assertNotIn(row["soname"], source)


class BoundaryTests(unittest.TestCase):
    def test_running_the_plan_is_still_not_a_boot_claim(self) -> None:
        self.assertFalse(mod.BOOTABLE_CLAIM)
        self.assertFalse(mod.ACTIVATION_ALLOWED)


if __name__ == "__main__":
    unittest.main()
