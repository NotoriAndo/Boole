#!/usr/bin/env python3
"""Tests for the enumerated ownership/set-id exception in the boot closure.

The sealed image rule is ``ownership: root:root-only``.  The frozen builder
keeps it by refusing any member that arrives otherwise, rather than by silently
rewriting one.  Eleven members of the sealed 191-package closure do arrive
otherwise, so the rewrite is written down instead of assumed -- and everything
outside that written list still meets the original refusal, unchanged.
"""

from __future__ import annotations

import io
import json
import pathlib
import stat
import tarfile
import unittest

from scripts import native_shadow_rootfs_builder_boot_arm64_v1 as boot


EXCEPTION_PATH = pathlib.Path(
    "native/containment/native-shadow-boot-rootfs-closure-exception-arm64-v1.json"
)
SOURCE_LOCK_PATH = pathlib.Path(
    "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v1.json"
)
BUILDER_AUTHORITY_PATH = pathlib.Path(
    "native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json"
)


def document() -> dict:
    return json.loads(EXCEPTION_PATH.read_text(encoding="utf-8"))


def tar_bytes(members: list[tuple[str, int, int, int]]) -> bytes:
    """A tar carrying one regular file per ``(path, uid, gid, mode)`` row."""

    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w") as archive:
        for path, uid, gid, mode in members:
            info = tarfile.TarInfo(f"./{path}")
            info.size = 0
            info.uid = uid
            info.gid = gid
            info.mode = mode
            info.uname = "root"
            info.gname = "shadow" if gid == 42 else "root"
            archive.addfile(info, io.BytesIO(b""))
    return raw.getvalue()


def entries(raw: bytes) -> dict:
    recipe = {"maxEntries": 1000, "maxFileBytes": 1 << 20, "maxTotalBytes": 1 << 24}
    return boot.__getattr__("_tar_entries")(raw, "package deb-test data", recipe)


class DocumentTests(unittest.TestCase):
    def test_the_exception_names_exactly_the_eleven_members_that_block_the_build(self) -> None:
        doc = document()
        self.assertEqual(doc["memberCount"], 11)
        self.assertEqual(len(doc["metadataMembers"]), 11)

    def test_only_four_packages_are_involved(self) -> None:
        packages = sorted({row["package"] for row in document()["metadataMembers"]})
        self.assertEqual(
            packages, ["libpam-modules-bin", "mount", "passwd", "sudo"]
        )

    def test_every_row_records_what_it_changes_away_from(self) -> None:
        for row in document()["metadataMembers"]:
            with self.subTest(path=row["path"]):
                self.assertEqual(row["normalizedUid"], 0)
                self.assertEqual(row["normalizedGid"], 0)
                original = int(row["originalMode"], 8)
                normalized = int(row["normalizedMode"], 8)
                self.assertEqual(normalized, original & 0o1777)
                self.assertTrue(
                    row["originalUid"] != 0
                    or row["originalGid"] != 0
                    or original & (stat.S_ISUID | stat.S_ISGID),
                    "a row that changes nothing does not belong in an exception list",
                )

    def test_the_exception_is_bound_to_the_closure_and_the_rule_it_serves(self) -> None:
        import hashlib

        doc = document()
        self.assertEqual(
            doc["rootfsSourceLockSha256"],
            hashlib.sha256(SOURCE_LOCK_PATH.read_bytes()).hexdigest(),
        )
        self.assertEqual(
            doc["authoritySha256"],
            hashlib.sha256(BUILDER_AUTHORITY_PATH.read_bytes()).hexdigest(),
        )

    def test_the_rule_being_kept_really_says_root_root_only(self) -> None:
        authority = json.loads(BUILDER_AUTHORITY_PATH.read_text(encoding="utf-8"))
        self.assertEqual(authority["determinism"]["ownership"], "root:root-only")

    def test_the_document_says_plainly_that_this_is_not_stock_ubuntu(self) -> None:
        self.assertIn("NOT the bytes Ubuntu ships", document()["boundaries"]["fidelity"])

    def test_enumerating_an_exception_is_not_building_or_booting(self) -> None:
        doc = document()
        self.assertIs(doc["bootableClaim"], False)
        self.assertIs(doc["activationAllowed"], False)
        self.assertIs(doc["guestImageBuilt"], False)


class ListedMemberTests(unittest.TestCase):
    def test_a_listed_setgid_member_is_admitted_as_root_root_without_the_set_id_bit(self) -> None:
        result = entries(tar_bytes([("usr/sbin/unix_chkpwd", 0, 42, 0o2755)]))
        entry = result["usr/sbin/unix_chkpwd"]
        self.assertEqual(entry["uid"], 0)
        self.assertEqual(entry["gid"], 0)
        self.assertEqual(entry["mode"], 0o755)

    def test_a_listed_setuid_member_loses_only_the_set_id_bit(self) -> None:
        result = entries(tar_bytes([("usr/bin/sudo", 0, 0, 0o4755)]))
        self.assertEqual(result["usr/bin/sudo"]["mode"], 0o755)

    def test_every_listed_member_actually_passes(self) -> None:
        rows = document()["metadataMembers"]
        raw = tar_bytes(
            [
                (row["path"], row["originalUid"], row["originalGid"], int(row["originalMode"], 8))
                for row in rows
            ]
        )
        result = entries(raw)
        self.assertEqual(len(result), len(rows))
        for row in rows:
            with self.subTest(path=row["path"]):
                entry = result[row["path"]]
                self.assertEqual(entry["uid"], 0)
                self.assertEqual(entry["gid"], 0)
                self.assertEqual(entry["mode"], int(row["normalizedMode"], 8))


class UnlistedMemberTests(unittest.TestCase):
    """Everything the list does not name keeps meeting the original refusal."""

    def test_an_unlisted_non_root_member_is_still_refused(self) -> None:
        with self.assertRaises(boot.RootfsBuildError) as caught:
            entries(tar_bytes([("usr/bin/somethingelse", 0, 42, 0o2755)]))
        self.assertIn("is not root:root", str(caught.exception))

    def test_an_unlisted_setuid_member_is_still_refused(self) -> None:
        with self.assertRaises(boot.RootfsBuildError) as caught:
            entries(tar_bytes([("usr/bin/somethingelse", 0, 0, 0o4755)]))
        self.assertIn("set-id mode", str(caught.exception))

    def test_a_listed_path_arriving_with_different_ownership_is_refused(self) -> None:
        """The exception covers eleven known members, not eleven known paths."""

        with self.assertRaises(boot.RootfsBuildError) as caught:
            entries(tar_bytes([("usr/bin/sudo", 0, 42, 0o4755)]))
        self.assertIn("usr/bin/sudo", str(caught.exception))

    def test_a_listed_path_arriving_with_a_wider_mode_is_refused(self) -> None:
        with self.assertRaises(boot.RootfsBuildError) as caught:
            entries(tar_bytes([("usr/bin/sudo", 0, 0, 0o4777)]))
        self.assertIn("usr/bin/sudo", str(caught.exception))

    def test_a_listed_path_gaining_setuid_where_it_had_setgid_is_refused(self) -> None:
        with self.assertRaises(boot.RootfsBuildError) as caught:
            entries(tar_bytes([("usr/sbin/unix_chkpwd", 0, 42, 0o6755)]))
        self.assertIn("usr/sbin/unix_chkpwd", str(caught.exception))

    def test_an_ordinary_member_is_untouched(self) -> None:
        result = entries(tar_bytes([("usr/bin/true", 0, 0, 0o755)]))
        self.assertEqual(result["usr/bin/true"]["mode"], 0o755)


class UsrmergeTests(unittest.TestCase):
    """Ubuntu 24.04 is merged-/usr, so ``lib/x`` and ``usr/lib/x`` are one path."""

    def test_the_merged_roots_are_the_ones_the_record_names(self) -> None:
        self.assertEqual(boot.USRMERGE_ROOTS, document()["usrmergeRoots"])

    def test_a_member_under_a_merged_root_is_relocated(self) -> None:
        self.assertEqual(boot.usrmerge_path("lib/modules/x"), "usr/lib/modules/x")
        self.assertEqual(boot.usrmerge_path("bin/ip"), "usr/bin/ip")
        self.assertEqual(boot.usrmerge_path("sbin/tc"), "usr/sbin/tc")
        self.assertEqual(boot.usrmerge_path("lib"), "usr/lib")

    def test_a_member_outside_the_merged_roots_is_untouched(self) -> None:
        for path in ("etc/passwd", "usr/lib/x", "var/log", "boot/vmlinuz", "libexec/x"):
            with self.subTest(path=path):
                self.assertEqual(boot.usrmerge_path(path), path)

    def test_relocation_moves_paths_and_never_decides_a_collision(self) -> None:
        """Two different members landing on one path is still an error."""

        usrmerge = boot.__getattr__("_usrmerge")
        entries = {
            "lib/x": {"path": "lib/x", "kind": "file", "mode": 0o644, "uid": 0, "gid": 0, "raw": b"a"},
            "usr/lib/x": {"path": "usr/lib/x", "kind": "file", "mode": 0o644, "uid": 0, "gid": 0, "raw": b"b"},
        }
        with self.assertRaises(boot.RootfsBuildError) as caught:
            usrmerge(entries, "package test data")
        self.assertIn("usrmerge path collision differs", str(caught.exception))

    def test_two_identical_directory_stubs_collapse(self) -> None:
        usrmerge = boot.__getattr__("_usrmerge")
        entries = {
            "lib": {"path": "lib", "kind": "directory", "mode": 0o755, "uid": 0, "gid": 0},
            "usr/lib": {"path": "usr/lib", "kind": "directory", "mode": 0o755, "uid": 0, "gid": 0},
        }
        self.assertEqual(sorted(usrmerge(entries, "package test data")), ["usr/lib"])

    def test_a_symlink_pointing_into_a_merged_root_is_relocated_too(self) -> None:
        usrmerge = boot.__getattr__("_usrmerge")
        entries = {
            "usr/bin/x": {
                "path": "usr/bin/x", "kind": "symlink", "mode": 0o777, "uid": 0, "gid": 0,
                "target": "/lib/y", "resolvedTarget": "lib/y",
            }
        }
        moved = usrmerge(entries, "package test data")
        self.assertEqual(moved["usr/bin/x"]["resolvedTarget"], "usr/lib/y")


class DerivedUsrmergeSymlinkTests(unittest.TestCase):
    """``base-files`` is not in this closure, so nothing ships ``/bin`` or ``/sbin``.

    The sealed lock derives ``/lib`` -- 347 binaries name their ELF interpreter
    under it -- but stops there, and the kernel hands PID 1 to ``/sbin/init``.
    ``/bin`` completes the same layout; it does not conjure the shell that
    closure never had, which :class:`ShellAbsenceTests` pins down separately.
    """

    def test_the_record_names_the_symlinks_the_closure_is_missing(self) -> None:
        rows = document()["derivedUsrmergeSymlinks"]
        self.assertEqual([row["logicalPath"] for row in rows], ["/bin", "/sbin"])

    def test_each_added_symlink_matches_the_shape_the_lock_already_uses(self) -> None:
        sealed = json.loads(SOURCE_LOCK_PATH.read_text(encoding="utf-8"))
        lib = next(
            row for row in sealed["derivedEntries"] if row["logicalPath"] == "/lib"
        )
        for row in document()["derivedUsrmergeSymlinks"]:
            with self.subTest(path=row["logicalPath"]):
                self.assertEqual(row["kind"], lib["kind"])
                self.assertEqual(row["mode"], lib["mode"])
                self.assertEqual(row["uid"], lib["uid"])
                self.assertEqual(row["gid"], lib["gid"])
                self.assertEqual(row["target"], f"usr{row['logicalPath']}")

    def test_lib64_is_not_added_because_arm64_does_not_use_it(self) -> None:
        paths = [row["logicalPath"] for row in document()["derivedUsrmergeSymlinks"]]
        self.assertNotIn("/lib64", paths)

    def test_normalising_the_lock_adds_them_and_says_so(self) -> None:
        sealed = json.loads(SOURCE_LOCK_PATH.read_text(encoding="utf-8"))
        result, _, record = boot.normalized_runtime_lock(sealed)
        paths = [row["logicalPath"] for row in result["derivedEntries"]]
        self.assertIn("/bin", paths)
        self.assertIn("/sbin", paths)
        self.assertEqual(record["addedDerivedEntries"], ["/bin", "/sbin"])

    def test_the_sealed_bytes_on_disk_are_not_touched(self) -> None:
        before = SOURCE_LOCK_PATH.read_bytes()
        boot.normalized_runtime_lock(json.loads(before.decode("utf-8")))
        self.assertEqual(SOURCE_LOCK_PATH.read_bytes(), before)

    def test_the_derived_entries_stay_sorted_by_logical_path(self) -> None:
        sealed = json.loads(SOURCE_LOCK_PATH.read_text(encoding="utf-8"))
        result, _, _ = boot.normalized_runtime_lock(sealed)
        paths = [row["logicalPath"] for row in result["derivedEntries"]]
        self.assertEqual(paths, sorted(paths))

    def test_a_lock_that_already_derives_the_path_differently_is_refused(self) -> None:
        sealed = json.loads(SOURCE_LOCK_PATH.read_text(encoding="utf-8"))
        sealed["derivedEntries"].append(
            {"gid": 0, "kind": "symlink", "logicalPath": "/bin", "mode": "0777",
             "target": "usr/local/bin", "uid": 0}
        )
        with self.assertRaises(boot.BootProjectionError):
            boot.normalized_runtime_lock(sealed)

    def test_a_lock_that_already_derives_the_path_identically_is_left_alone(self) -> None:
        sealed = json.loads(SOURCE_LOCK_PATH.read_text(encoding="utf-8"))
        sealed["derivedEntries"].append(
            {"gid": 0, "kind": "symlink", "logicalPath": "/bin", "mode": "0777",
             "target": "usr/bin", "uid": 0}
        )
        result, _, record = boot.normalized_runtime_lock(sealed)
        paths = [row["logicalPath"] for row in result["derivedEntries"]]
        self.assertEqual(paths.count("/bin"), 1)
        self.assertEqual(record["addedDerivedEntries"], ["/sbin"])


class ShellAbsenceTests(unittest.TestCase):
    """Adding ``/bin`` fixes the layout, not the missing interpreter behind it.

    No package among the sealed 191 provides a shell, so the members whose
    shebang names one cannot run however the path resolves.  The record has to
    say that itself, or the ``/bin`` row reads like a repair it is not.
    """

    SHELL_PACKAGES = frozenset(
        {"bash", "dash", "busybox", "busybox-static", "busybox-initramfs",
         "ksh", "mksh", "posh", "zsh", "yash"}
    )

    def closure_packages(self) -> set[str]:
        sealed = json.loads(SOURCE_LOCK_PATH.read_text(encoding="utf-8"))
        return {package["name"] for package in sealed["ubuntu"]["packages"]}

    def test_the_closure_really_ships_no_shell(self) -> None:
        self.assertEqual(self.closure_packages() & self.SHELL_PACKAGES, set())

    def test_the_record_says_the_closure_ships_no_shell(self) -> None:
        note = document()["boundaries"]["shellAbsence"]
        for name in ("dash", "bash", "busybox"):
            with self.subTest(shell=name):
                self.assertIn(name, note)

    def test_the_record_says_those_members_cannot_run(self) -> None:
        self.assertIn("cannot run", document()["boundaries"]["shellAbsence"])

    def test_the_bin_row_does_not_read_as_a_repair_of_them(self) -> None:
        note = document()["rationale"]["derivedUsrmergeSymlinks"]
        self.assertIn("does not supply a shell", note)
        self.assertIn("shellAbsence", note)

    def test_the_bin_row_still_gives_its_own_reason(self) -> None:
        note = document()["rationale"]["derivedUsrmergeSymlinks"]
        self.assertIn("/sbin/init", note)

    def test_confirming_the_boot_path_is_left_to_a_verify_step(self) -> None:
        note = document()["boundaries"]["shellAbsence"]
        self.assertIn("verify", note)


class DanglingSymlinkTests(unittest.TestCase):
    def test_a_mask_pointing_at_dev_null_is_allowed(self) -> None:
        allowed = boot.__getattr__("_dangling_allowed")
        self.assertTrue(allowed("usr/lib/systemd/system/hwclock.service", "dev/null"))

    def test_the_other_runtime_filesystems_are_allowed_too(self) -> None:
        allowed = boot.__getattr__("_dangling_allowed")
        for root in document()["runtimeFilesystemRoots"]:
            with self.subTest(root=root):
                self.assertTrue(allowed("usr/lib/systemd/system/x.service", f"{root}/something"))

    def test_a_named_dangling_symlink_is_allowed_at_its_recorded_target_only(self) -> None:
        allowed = boot.__getattr__("_dangling_allowed")
        for row in document()["danglingSymlinks"]:
            with self.subTest(path=row["path"]):
                self.assertTrue(allowed(row["path"], row["resolvedTarget"]))
                self.assertFalse(allowed(row["path"], "usr/lib/somewhere-else"))

    def test_an_unnamed_dangling_symlink_is_still_refused(self) -> None:
        allowed = boot.__getattr__("_dangling_allowed")
        self.assertFalse(allowed("usr/bin/whatever", "usr/bin/missing"))

    def test_every_recorded_dangling_symlink_gives_a_reason(self) -> None:
        rows = document()["danglingSymlinks"]
        self.assertEqual(len(rows), document()["danglingSymlinkCount"])
        for row in rows:
            with self.subTest(path=row["path"]):
                self.assertTrue(row["reason"].strip())
                self.assertNotIn(row["resolvedTarget"].split("/", 1)[0], document()["runtimeFilesystemRoots"])


class ProjectionTests(unittest.TestCase):
    def test_the_projection_still_pins_the_frozen_builder_digest(self) -> None:
        self.assertEqual(boot.BUILDER_SHA256, boot.ARM64_BUILDER_SHA256)

    def test_the_exception_table_is_read_from_the_sealed_file_not_restated(self) -> None:
        """A second copy of a sealed fact can drift from it, invisibly."""

        table = boot.METADATA_EXCEPTIONS
        rows = document()["metadataMembers"]
        self.assertEqual(len(table), len(rows))
        for row in rows:
            self.assertIn(row["path"], table)

    def test_the_other_refusals_in_the_same_loop_are_untouched(self) -> None:
        raw = io.BytesIO()
        with tarfile.open(fileobj=raw, mode="w") as archive:
            info = tarfile.TarInfo("./etc/ld.so.preload")
            info.size = 0
            archive.addfile(info, io.BytesIO(b""))
        with self.assertRaises(boot.RootfsBuildError) as caught:
            entries(raw.getvalue())
        self.assertIn("forbidden rootfs path", str(caught.exception))


if __name__ == "__main__":
    unittest.main()
