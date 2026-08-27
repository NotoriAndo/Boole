#!/usr/bin/env python3
"""Tests for the read-only comparison of two produced trees."""

from __future__ import annotations

import hashlib
import os
import pathlib
import tempfile
import unittest

from scripts import native_shadow_boot_root_disk_tree_diff_arm64_v1 as mod


def _no_xattrs(path: str) -> dict[str, str]:
    return {}


def _directory(mode: int = 0o755, **extra) -> dict:
    row = {"gid": 0, "kind": "directory", "mode": mode, "uid": 0, "xattrs": {}}
    row.update(extra)
    return row


def _file(data: bytes, mode: int = 0o644, **extra) -> dict:
    row = {
        "gid": 0,
        "kind": "file",
        "mode": mode,
        "mtime": 1,
        "nlink": 1,
        "sha256": hashlib.sha256(data).hexdigest(),
        "size": len(data),
        "uid": 0,
        "xattrs": {},
    }
    row.update(extra)
    return row


BEFORE = {
    "/": _directory(),
    "/etc": _directory(),
    "/etc/hostname": _file(b"boole\n"),
    "/sbin": {
        "gid": 0,
        "kind": "symlink",
        "mode": 0o777,
        "mtime": 1,
        "target": "usr/sbin",
        "uid": 0,
        "xattrs": {},
    },
}

REQUIRED = ("dev", "proc", "run", "sys", "tmp")
MODES = {"dev": 0o755, "proc": 0o555, "run": 0o755, "sys": 0o555, "tmp": 0o1777}


def _after_with_required() -> dict:
    after = dict(BEFORE)
    for name in REQUIRED:
        after["/" + name] = _directory(MODES[name])
    return after


class DiffTreesTest(unittest.TestCase):
    def test_identical_trees_differ_in_nothing(self) -> None:
        found = mod.diff_trees(BEFORE, dict(BEFORE))
        self.assertEqual(found["added"], {})
        self.assertEqual(found["removed"], {})
        self.assertEqual(found["changed"], {})

    def test_an_added_path_is_reported_as_added(self) -> None:
        after = dict(BEFORE)
        after["/tmp"] = _directory(0o1777)
        found = mod.diff_trees(BEFORE, after)
        self.assertEqual(sorted(found["added"]), ["/tmp"])
        self.assertEqual(found["removed"], {})
        self.assertEqual(found["changed"], {})

    def test_a_removed_path_is_reported_as_removed(self) -> None:
        after = dict(BEFORE)
        del after["/etc/hostname"]
        found = mod.diff_trees(BEFORE, after)
        self.assertEqual(sorted(found["removed"]), ["/etc/hostname"])

    def test_changed_content_names_the_field_and_both_sides(self) -> None:
        after = dict(BEFORE)
        after["/etc/hostname"] = _file(b"other\n")
        found = mod.diff_trees(BEFORE, after)
        self.assertEqual(sorted(found["changed"]), ["/etc/hostname"])
        change = found["changed"]["/etc/hostname"]["sha256"]
        self.assertEqual(change["before"], BEFORE["/etc/hostname"]["sha256"])
        self.assertEqual(change["after"], after["/etc/hostname"]["sha256"])

    def test_a_changed_mode_is_reported(self) -> None:
        after = dict(BEFORE)
        after["/etc"] = _directory(0o700)
        self.assertIn("mode", mod.diff_trees(BEFORE, after)["changed"]["/etc"])

    def test_a_changed_owner_is_reported(self) -> None:
        after = dict(BEFORE)
        after["/etc"] = _directory(uid=1000)
        self.assertIn("uid", mod.diff_trees(BEFORE, after)["changed"]["/etc"])

    def test_a_changed_link_target_is_reported(self) -> None:
        after = dict(BEFORE)
        after["/sbin"] = dict(BEFORE["/sbin"], target="usr/bin")
        self.assertIn("target", mod.diff_trees(BEFORE, after)["changed"]["/sbin"])

    def test_a_changed_xattr_is_reported(self) -> None:
        after = dict(BEFORE)
        after["/etc/hostname"] = _file(b"boole\n", xattrs={"security.capability": "01"})
        self.assertIn("xattrs", mod.diff_trees(BEFORE, after)["changed"]["/etc/hostname"])

    def test_a_field_appearing_on_one_side_only_is_reported(self) -> None:
        after = dict(BEFORE)
        without = dict(BEFORE["/etc/hostname"])
        del without["nlink"]
        after["/etc/hostname"] = without
        self.assertIn("nlink", mod.diff_trees(BEFORE, after)["changed"]["/etc/hostname"])


class VerdictTest(unittest.TestCase):
    def _rows(self):
        return tuple(
            {"gid": 0, "kind": "directory", "mode": format(MODES[name], "04o"), "path": name, "uid": 0}
            for name in REQUIRED
        )

    def test_exactly_the_five_empty_directories_passes(self) -> None:
        found = mod.verdict(diff=mod.diff_trees(BEFORE, _after_with_required()), rows=self._rows())
        self.assertEqual(found["reasons"], [])
        self.assertTrue(found["ok"])

    def test_an_unchanged_tree_fails_because_the_five_are_absent(self) -> None:
        found = mod.verdict(diff=mod.diff_trees(BEFORE, dict(BEFORE)), rows=self._rows())
        self.assertFalse(found["ok"])

    def test_a_sixth_added_path_fails(self) -> None:
        after = _after_with_required()
        after["/surprise"] = _directory()
        found = mod.verdict(diff=mod.diff_trees(BEFORE, after), rows=self._rows())
        self.assertFalse(found["ok"])
        self.assertTrue(any("/surprise" in reason for reason in found["reasons"]))

    def test_a_file_inside_an_added_directory_fails(self) -> None:
        after = _after_with_required()
        after["/tmp/leftover"] = _file(b"x")
        self.assertFalse(mod.verdict(diff=mod.diff_trees(BEFORE, after), rows=self._rows())["ok"])

    def test_any_removal_fails(self) -> None:
        after = _after_with_required()
        del after["/etc/hostname"]
        found = mod.verdict(diff=mod.diff_trees(BEFORE, after), rows=self._rows())
        self.assertFalse(found["ok"])
        self.assertTrue(any("/etc/hostname" in reason for reason in found["reasons"]))

    def test_any_change_to_an_existing_path_fails(self) -> None:
        after = _after_with_required()
        after["/etc/hostname"] = _file(b"other\n")
        found = mod.verdict(diff=mod.diff_trees(BEFORE, after), rows=self._rows())
        self.assertFalse(found["ok"])
        self.assertTrue(any("/etc/hostname" in reason for reason in found["reasons"]))

    def test_an_added_directory_with_the_wrong_mode_fails(self) -> None:
        after = _after_with_required()
        after["/tmp"] = _directory(0o777)
        found = mod.verdict(diff=mod.diff_trees(BEFORE, after), rows=self._rows())
        self.assertFalse(found["ok"])
        self.assertTrue(any("/tmp" in reason for reason in found["reasons"]))

    def test_an_added_directory_owned_by_someone_else_fails(self) -> None:
        after = _after_with_required()
        after["/run"] = _directory(MODES["run"], uid=1000)
        self.assertFalse(mod.verdict(diff=mod.diff_trees(BEFORE, after), rows=self._rows())["ok"])

    def test_an_added_entry_that_is_not_a_directory_fails(self) -> None:
        after = _after_with_required()
        after["/tmp"] = _file(b"", mode=0o1777)
        self.assertFalse(mod.verdict(diff=mod.diff_trees(BEFORE, after), rows=self._rows())["ok"])

    def test_an_added_directory_carrying_an_xattr_fails(self) -> None:
        after = _after_with_required()
        after["/tmp"] = _directory(MODES["tmp"], xattrs={"user.note": "00"})
        self.assertFalse(mod.verdict(diff=mod.diff_trees(BEFORE, after), rows=self._rows())["ok"])

    def test_a_changed_hardlink_grouping_fails(self) -> None:
        found = mod.verdict(
            diff=mod.diff_trees(BEFORE, _after_with_required()),
            rows=self._rows(),
            hardlinkGroups={"before": [["/a", "/b"]], "after": [["/a", "/c"]]},
        )
        self.assertFalse(found["ok"])
        self.assertTrue(any("hard link" in reason for reason in found["reasons"]))

    def test_an_unchanged_hardlink_grouping_passes(self) -> None:
        found = mod.verdict(
            diff=mod.diff_trees(BEFORE, _after_with_required()),
            rows=self._rows(),
            hardlinkGroups={"before": [["/a", "/b"]], "after": [["/a", "/b"]]},
        )
        self.assertTrue(found["ok"])


class ReadTreeTest(unittest.TestCase):
    def test_it_records_kinds_modes_owners_contents_and_links(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = pathlib.Path(raw)
            (root / "etc").mkdir()
            (root / "etc/hostname").write_bytes(b"boole\n")
            os.chmod(root / "etc/hostname", 0o644)
            os.symlink("usr/sbin", root / "sbin")
            found = mod.read_tree(root, xattrs=_no_xattrs)
        entries = found["entries"]
        self.assertEqual(entries["/"]["kind"], "directory")
        self.assertEqual(entries["/etc"]["kind"], "directory")
        self.assertEqual(entries["/etc/hostname"]["kind"], "file")
        self.assertEqual(
            entries["/etc/hostname"]["sha256"], hashlib.sha256(b"boole\n").hexdigest()
        )
        self.assertEqual(entries["/etc/hostname"]["mode"], 0o644)
        self.assertEqual(entries["/sbin"]["kind"], "symlink")
        self.assertEqual(entries["/sbin"]["target"], "usr/sbin")

    def test_a_symlink_to_a_directory_is_described_and_not_walked(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = pathlib.Path(raw)
            (root / "real").mkdir()
            (root / "real/inside").write_bytes(b"x")
            os.symlink("real", root / "link")
            found = mod.read_tree(root, xattrs=_no_xattrs)
        self.assertIn("/real/inside", found["entries"])
        self.assertNotIn("/link/inside", found["entries"])

    def test_directories_carry_no_size_or_link_count(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = pathlib.Path(raw)
            (root / "etc").mkdir()
            found = mod.read_tree(root, xattrs=_no_xattrs)
        self.assertNotIn("nlink", found["entries"]["/etc"])
        self.assertNotIn("size", found["entries"]["/etc"])

    def test_hard_linked_files_are_grouped(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = pathlib.Path(raw)
            (root / "one").write_bytes(b"x")
            os.link(root / "one", root / "two")
            found = mod.read_tree(root, xattrs=_no_xattrs)
        self.assertEqual(found["hardlinkGroups"], [["/one", "/two"]])

    def test_files_with_one_name_form_no_group(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = pathlib.Path(raw)
            (root / "one").write_bytes(b"x")
            found = mod.read_tree(root, xattrs=_no_xattrs)
        self.assertEqual(found["hardlinkGroups"], [])

    def test_extended_attributes_are_recorded_as_hex(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = pathlib.Path(raw)
            (root / "one").write_bytes(b"x")
            found = mod.read_tree(root, xattrs=lambda path: {"user.note": "ff00"})
        self.assertEqual(found["entries"]["/one"]["xattrs"], {"user.note": "ff00"})

    def test_an_entry_that_is_not_a_file_directory_or_link_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = pathlib.Path(raw)
            os.mkfifo(root / "pipe")
            with self.assertRaises(mod.RootDiskTreeDiffError):
                mod.read_tree(root, xattrs=_no_xattrs)


class InitrdComparisonTest(unittest.TestCase):
    def _write_pair(self, root: pathlib.Path, later_rows: list[dict]) -> tuple:
        from scripts.native_shadow_boot_initrd_arm64_v1 import initrd_bytes
        from scripts.test_native_shadow_boot_initrd_arm64_v1 import tar_bytes

        base = [
            {"path": "etc", "kind": "directory", "mode": 0o755},
            {"path": "etc/hostname", "kind": "file", "mode": 0o644, "raw": b"boole\n"},
            {"path": "usr", "kind": "directory", "mode": 0o755},
        ]
        sides = []
        for name, rows in (("earlier", base), ("later", base + later_rows)):
            outputs = root / name
            outputs.mkdir()
            target = mod.resolve_outputs(outputs, "initrd")
            target.write_bytes(initrd_bytes(tar_bytes(rows)))
            sides.append(outputs)
        return tuple(sides)

    def _required_rows(self) -> list[dict]:
        return [
            {"path": name, "kind": "directory", "mode": MODES[name]} for name in REQUIRED
        ]

    def test_two_real_archives_differing_by_the_five_directories_pass(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            earlier, later = self._write_pair(pathlib.Path(raw), self._required_rows())
            found = mod.compare_initrds(
                before=mod.resolve_outputs(earlier, "initrd"),
                after=mod.resolve_outputs(later, "initrd"),
            )
        self.assertEqual(found["verdict"]["reasons"], [])
        self.assertTrue(found["verdict"]["ok"])
        self.assertEqual(found["addedPaths"], ["/" + name for name in REQUIRED])
        self.assertEqual(found["removedPaths"], [])
        self.assertEqual(found["changedPaths"], {})
        self.assertEqual(found["container"], "initrd")

    def test_a_sixth_directory_in_the_later_archive_fails(self) -> None:
        extra = self._required_rows() + [
            {"path": "srv", "kind": "directory", "mode": 0o755}
        ]
        with tempfile.TemporaryDirectory() as raw:
            earlier, later = self._write_pair(pathlib.Path(raw), extra)
            found = mod.compare_initrds(
                before=mod.resolve_outputs(earlier, "initrd"),
                after=mod.resolve_outputs(later, "initrd"),
            )
        self.assertFalse(found["verdict"]["ok"])

    def test_a_changed_file_in_the_later_archive_fails(self) -> None:
        from scripts.native_shadow_boot_initrd_arm64_v1 import initrd_bytes
        from scripts.test_native_shadow_boot_initrd_arm64_v1 import tar_bytes

        with tempfile.TemporaryDirectory() as raw:
            earlier, later = self._write_pair(pathlib.Path(raw), self._required_rows())
            rows = [
                {"path": "etc", "kind": "directory", "mode": 0o755},
                {"path": "etc/hostname", "kind": "file", "mode": 0o644, "raw": b"other\n"},
                {"path": "usr", "kind": "directory", "mode": 0o755},
            ] + self._required_rows()
            mod.resolve_outputs(later, "initrd").write_bytes(initrd_bytes(tar_bytes(rows)))
            found = mod.compare_initrds(
                before=mod.resolve_outputs(earlier, "initrd"),
                after=mod.resolve_outputs(later, "initrd"),
            )
        self.assertFalse(found["verdict"]["ok"])
        self.assertIn("/etc/hostname", found["changedPaths"])

    def test_trees_without_extended_attributes_still_compare(self) -> None:
        before = {"/etc": {"gid": 0, "kind": "directory", "mode": 0o755, "uid": 0}}
        after = dict(before)
        for name in REQUIRED:
            after["/" + name] = {
                "gid": 0,
                "kind": "directory",
                "mode": MODES[name],
                "uid": 0,
            }
        rows = tuple(
            {"gid": 0, "kind": "directory", "mode": format(MODES[name], "04o"), "path": name, "uid": 0}
            for name in REQUIRED
        )
        found = mod.verdict(diff=mod.diff_trees(before, after), rows=rows)
        self.assertEqual(found["reasons"], [])
        self.assertTrue(found["ok"])


class InitrdAccountingTest(unittest.TestCase):
    """Every byte of difference between two archives, accounted for."""

    BASE = [
        {"path": "etc", "kind": "directory", "mode": 0o755},
        {"path": "etc/hostname", "kind": "file", "mode": 0o644, "raw": b"boole\n"},
        {"path": "usr", "kind": "directory", "mode": 0o755},
        {"path": "usr/bin", "kind": "directory", "mode": 0o755},
    ]

    def _rows(self):
        return tuple(
            {"gid": 0, "kind": "directory", "mode": format(MODES[name], "04o"), "path": name, "uid": 0}
            for name in REQUIRED
        )

    def _archive(self, rows, *, sort: bool = True) -> bytes:
        from scripts.native_shadow_boot_initrd_arm64_v1 import initrd_bytes
        from scripts.test_native_shadow_boot_initrd_arm64_v1 import tar_bytes

        return initrd_bytes(tar_bytes(rows, sort=sort))

    def _required(self):
        return [{"path": name, "kind": "directory", "mode": MODES[name]} for name in REQUIRED]

    def test_five_inserted_directories_account_for_the_whole_difference(self) -> None:
        found = mod.initrd_record_accounting(
            before=self._archive(self.BASE),
            after=self._archive(self.BASE + self._required()),
            rows=self._rows(),
        )
        self.assertEqual(found["reasons"], [])
        self.assertTrue(found["ok"])
        self.assertEqual(found["addedRecords"], sorted(REQUIRED))
        self.assertEqual(found["removedRecords"], [])
        self.assertEqual(found["fieldDifferences"], {})
        self.assertTrue(found["byteAccounting"]["balanced"])

    def test_the_added_records_explain_the_size_growth_exactly(self) -> None:
        before = self._archive(self.BASE)
        after = self._archive(self.BASE + self._required())
        found = mod.initrd_record_accounting(before=before, after=after, rows=self._rows())
        accounting = found["byteAccounting"]
        self.assertEqual(accounting["beforeBytes"], len(before))
        self.assertEqual(accounting["afterBytes"], len(after))
        self.assertEqual(
            accounting["afterBytes"] - accounting["beforeBytes"],
            accounting["addedRecordBytes"],
        )

    def test_every_shared_record_is_renumbered_by_the_insertions_before_it(self) -> None:
        found = mod.initrd_record_accounting(
            before=self._archive(self.BASE),
            after=self._archive(self.BASE + self._required()),
            rows=self._rows(),
        )
        self.assertTrue(found["renumbering"]["consistent"])
        # /usr sorts after all five, so it moves by the full five.
        self.assertEqual(found["renumbering"]["shifts"]["usr"], 5)
        # /etc sorts before every one of them except dev, so it moves by one.
        self.assertEqual(found["renumbering"]["shifts"]["etc"], 1)

    def test_a_changed_file_is_reported_and_fails(self) -> None:
        changed = [
            dict(row, raw=b"other\n") if row["path"] == "etc/hostname" else row
            for row in self.BASE
        ]
        found = mod.initrd_record_accounting(
            before=self._archive(self.BASE),
            after=self._archive(changed + self._required()),
            rows=self._rows(),
        )
        self.assertFalse(found["ok"])
        self.assertIn("etc/hostname", found["fieldDifferences"])

    def test_a_changed_mode_is_reported_and_fails(self) -> None:
        changed = [
            dict(row, mode=0o600) if row["path"] == "etc/hostname" else row
            for row in self.BASE
        ]
        found = mod.initrd_record_accounting(
            before=self._archive(self.BASE),
            after=self._archive(changed + self._required()),
            rows=self._rows(),
        )
        self.assertFalse(found["ok"])
        self.assertIn("mode", found["fieldDifferences"]["etc/hostname"])

    def test_a_sixth_added_record_fails(self) -> None:
        extra = self._required() + [{"path": "srv", "kind": "directory", "mode": 0o755}]
        found = mod.initrd_record_accounting(
            before=self._archive(self.BASE),
            after=self._archive(self.BASE + extra),
            rows=self._rows(),
        )
        self.assertFalse(found["ok"])
        self.assertIn("srv", found["addedRecords"])

    def test_two_identical_archives_fail_because_the_five_are_absent(self) -> None:
        found = mod.initrd_record_accounting(
            before=self._archive(self.BASE),
            after=self._archive(self.BASE),
            rows=self._rows(),
        )
        self.assertFalse(found["ok"])

    def test_the_writer_refuses_to_emit_an_unsorted_layer_at_all(self) -> None:
        from scripts.native_shadow_boot_initrd_arm64_v1 import InitrdBuildError

        later = self.BASE + self._required()
        reordered = sorted(later, key=lambda row: row["path"], reverse=True)
        with self.assertRaises(InitrdBuildError):
            self._archive(reordered, sort=False)

    def test_a_record_numbered_out_of_step_is_caught(self) -> None:
        from scripts.native_shadow_boot_initrd_arm64_v1 import _record, parse_newc

        later = self._archive(self.BASE + self._required())
        out = b"".join(
            _record(
                ino=row["ino"] + 1 if row["name"] == "usr" else row["ino"],
                mode=row["mode"],
                nlink=row["nlink"],
                name=row["name"],
                data=row["data"],
            )
            for row in parse_newc(later)
        )
        found = mod.initrd_record_accounting(
            before=self._archive(self.BASE), after=out, rows=self._rows()
        )
        self.assertFalse(found["ok"])
        self.assertFalse(found["renumbering"]["consistent"])
        self.assertIn("usr", found["renumbering"]["inconsistentRecords"])

    def test_a_removed_record_fails(self) -> None:
        without = [row for row in self.BASE if row["path"] != "usr/bin"]
        found = mod.initrd_record_accounting(
            before=self._archive(self.BASE),
            after=self._archive(without + self._required()),
            rows=self._rows(),
        )
        self.assertFalse(found["ok"])
        self.assertEqual(found["removedRecords"], ["usr/bin"])


class ResolveOutputsTest(unittest.TestCase):
    def test_the_names_come_from_the_producer_rather_than_from_here(self) -> None:
        from scripts import native_shadow_boot_root_disk_readback_arm64_v1 as readback

        outputs = pathlib.Path("/somewhere/boot-outputs")
        for key in ("root-disk", "initrd"):
            self.assertEqual(
                mod.resolve_outputs(outputs, key), readback.output_paths(outputs)[key]
            )

    def test_a_name_the_producer_does_not_write_is_refused(self) -> None:
        with self.assertRaises(mod.RootDiskTreeDiffError):
            mod.resolve_outputs(pathlib.Path("/somewhere"), "not-a-produced-file")


class DocumentTest(unittest.TestCase):
    def test_the_document_makes_no_boot_claim(self) -> None:
        document = mod.document(
            subject={"before": {"name": "a", "sha256": "0" * 64}, "after": {"name": "b", "sha256": "1" * 64}},
            container="root-disk",
            diff=mod.diff_trees(BEFORE, _after_with_required()),
            verdict={"ok": True, "reasons": []},
        )
        self.assertFalse(document["activationAllowed"])
        self.assertFalse(document["bootableClaim"])
        self.assertFalse(document["guestBootVerified"])
        self.assertEqual(document["container"], "root-disk")
        self.assertTrue(document["verdict"]["ok"])


if __name__ == "__main__":
    unittest.main()
