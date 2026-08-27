"""The image has no place for the kernel filesystems to be mounted. This fixes that.

The one MAC.3 boot got as far as PID 1 and stopped there, because `/proc`,
`/sys` and `/dev` are not in the image and systemd mounts them before it does
anything else. Three names came out of that transcript. Guessing that three is
the whole list is how a second image fails the same way, so the list is taken
from the guest's own systemd instead: the mount table is decoded out of the
`libsystemd-shared` the image ships, the mount units in the image are read for
their `Where=`, and `/etc/fstab` is checked for rather than assumed absent.

What that audit produces is five directories, not three, and the two the
transcript never reached are the interesting half of the answer.

These tests hold three things. The audit is what the required list comes from,
so it is recorded and checked rather than summarised. The sealed records are
left alone -- the closure exception that already names these five as runtime
filesystems is bound by digest, and the new record has to agree with it rather
than restate it. And the builder actually puts the directories in, with the
modes a Debian base system gives them.
"""

from __future__ import annotations

import hashlib
import json
import pathlib
import unittest

import scripts.native_shadow_boot_rootfs_mount_point_audit_arm64_v1 as audit

REPO = pathlib.Path(__file__).resolve().parents[1]
CONTAINMENT = REPO / "native/containment"
RECORD_PATH = CONTAINMENT / "native-shadow-boot-rootfs-runtime-mount-points-arm64-v1.json"
CLOSURE_EXCEPTION_PATH = (
    CONTAINMENT / "native-shadow-boot-rootfs-closure-exception-arm64-v1.json"
)
BOOT_FAILURE_PATH = (
    CONTAINMENT / "native-shadow-mac3-closed-local-boot-result-arm64-v1.json"
)

# Debian and Ubuntu base-files ship exactly these modes, and a mount point's own
# mode is invisible once something is mounted over it, so matching the base
# system is the only reason to prefer one number over another.
EXPECTED_MODES = {
    "dev": "0755",
    "proc": "0555",
    "run": "0755",
    "sys": "0555",
    "tmp": "1777",
}


def document() -> dict:
    return json.loads(RECORD_PATH.read_text(encoding="utf-8"))


def digest(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


class RecordTests(unittest.TestCase):
    """The five directories, named with the metadata they will be written with."""

    def test_the_record_is_on_disk_and_parses(self) -> None:
        self.assertTrue(RECORD_PATH.is_file())
        self.assertIsInstance(document(), dict)

    def test_it_names_exactly_the_five_runtime_roots(self) -> None:
        self.assertEqual(
            sorted(row["path"] for row in document()["requiredRootDirectories"]),
            ["dev", "proc", "run", "sys", "tmp"],
        )

    def test_every_directory_is_root_owned(self) -> None:
        for row in document()["requiredRootDirectories"]:
            self.assertEqual((row["uid"], row["gid"]), (0, 0), row["path"])
            self.assertEqual(row["kind"], "directory", row["path"])

    def test_the_modes_are_the_base_system_modes(self) -> None:
        modes = {row["path"]: row["mode"] for row in document()["requiredRootDirectories"]}
        self.assertEqual(modes, EXPECTED_MODES)

    def test_tmp_is_sticky_and_world_writable_and_the_others_are_not(self) -> None:
        for row in document()["requiredRootDirectories"]:
            mode = int(row["mode"], 8)
            if row["path"] == "tmp":
                self.assertTrue(mode & 0o1000, "/tmp must carry the sticky bit")
                self.assertEqual(mode & 0o777, 0o777)
            else:
                self.assertFalse(mode & 0o1000, row["path"])
                self.assertFalse(mode & 0o022, row["path"])

    def test_no_directory_carries_a_set_id_bit(self) -> None:
        for row in document()["requiredRootDirectories"]:
            self.assertFalse(int(row["mode"], 8) & 0o6000, row["path"])

    def test_each_directory_says_why_it_is_needed(self) -> None:
        for row in document()["requiredRootDirectories"]:
            self.assertTrue(row["reason"].strip(), row["path"])


class CauseTests(unittest.TestCase):
    """It is tied to the run that found the problem, by digest."""

    def test_the_failed_boot_is_bound_by_its_digest(self) -> None:
        row = document()["cause"]
        self.assertEqual(row["path"], BOOT_FAILURE_PATH.relative_to(REPO).as_posix())
        self.assertEqual(row["sha256"], digest(BOOT_FAILURE_PATH))

    def test_the_three_names_the_transcript_gave_are_among_the_five(self) -> None:
        required = {row["path"] for row in document()["requiredRootDirectories"]}
        failure = json.loads(BOOT_FAILURE_PATH.read_text(encoding="utf-8"))
        for row in failure["foundByThisRun"]:
            self.assertIn(row["path"].lstrip("/"), required)

    def test_the_audit_found_more_than_the_transcript_did(self) -> None:
        # If the audit had returned exactly the three the console named, it
        # would not have been an audit.
        failure = json.loads(BOOT_FAILURE_PATH.read_text(encoding="utf-8"))
        self.assertGreater(
            len(document()["requiredRootDirectories"]), len(failure["foundByThisRun"])
        )


class AuditTests(unittest.TestCase):
    """Where the list came from, kept in a form someone else can redo."""

    def test_the_mount_table_was_read_from_the_image_that_failed(self) -> None:
        source = document()["audit"]["systemd"]["mountTableSource"]
        self.assertEqual(len(source["sha256"]), 64)
        self.assertGreater(source["sizeBytes"], 0)
        self.assertIn("libsystemd-shared", source["guestPath"])

    def test_the_whole_mount_table_is_recorded_not_a_selection(self) -> None:
        table = document()["audit"]["systemd"]["mountTable"]
        self.assertEqual(len(table), document()["audit"]["systemd"]["mountTableEntryCount"])
        self.assertGreater(len(table), 5)

    def test_every_mount_table_target_resolves_to_a_required_root(self) -> None:
        required = {row["path"] for row in document()["requiredRootDirectories"]}
        for row in document()["audit"]["systemd"]["mountTable"]:
            root = audit.top_level_of(row["where"])
            self.assertIn(root, required | {"sys", "proc", "dev", "run"}, row["where"])

    def test_the_mount_units_in_the_image_are_all_read(self) -> None:
        units = document()["audit"]["mountUnits"]
        self.assertTrue(units)
        for row in units:
            self.assertTrue(row["unit"].endswith(".mount"))
            self.assertTrue(row["where"].startswith("/"))
            self.assertIn("enabled", row)

    def test_no_mount_unit_needs_a_directory_the_image_does_not_carry(self) -> None:
        required = {row["path"] for row in document()["requiredRootDirectories"]}
        for row in document()["audit"]["mountUnits"]:
            self.assertIn(audit.top_level_of(row["where"]), required, row["unit"])

    def test_fstab_was_looked_for_rather_than_assumed_absent(self) -> None:
        fstab = document()["audit"]["fstab"]
        self.assertEqual(fstab["guestPath"], "/etc/fstab")
        self.assertFalse(fstab["presentInImage"])

    def test_tmp_is_required_because_nothing_mounts_over_it(self) -> None:
        # `tmp.mount` ships under /usr/share, which is not a unit directory, so
        # /tmp stays a real directory on the root filesystem.
        tmp = document()["audit"]["tmpMount"]
        self.assertFalse(tmp["enabled"])
        self.assertNotIn("/usr/lib/systemd/system/", tmp["guestPath"])


class SealedRecordTests(unittest.TestCase):
    """Nothing sealed is edited; the new record agrees with what is already there."""

    def test_the_closure_exception_is_bound_and_unchanged(self) -> None:
        row = document()["appendOnly"]["recordsLeftByteUnchanged"]
        pinned = {entry["path"]: entry for entry in row}
        path = CLOSURE_EXCEPTION_PATH.relative_to(REPO).as_posix()
        self.assertIn(path, pinned)
        self.assertEqual(pinned[path]["sha256"], digest(CLOSURE_EXCEPTION_PATH))

    def test_the_required_set_equals_the_sealed_runtime_filesystem_roots(self) -> None:
        sealed = json.loads(CLOSURE_EXCEPTION_PATH.read_text(encoding="utf-8"))
        self.assertEqual(
            sorted(sealed["runtimeFilesystemRoots"]),
            sorted(row["path"] for row in document()["requiredRootDirectories"]),
        )

    def test_every_pinned_record_still_hashes_to_what_was_pinned(self) -> None:
        for row in document()["appendOnly"]["recordsLeftByteUnchanged"]:
            path = REPO / row["path"]
            self.assertTrue(path.is_file(), row["path"])
            self.assertEqual(digest(path), row["sha256"], row["path"])


class AuditModuleTests(unittest.TestCase):
    """The audit is code, so a later tree can be put through the same check."""

    def test_it_reads_the_record_rather_than_restating_it(self) -> None:
        self.assertEqual(
            sorted(row["path"] for row in audit.required_root_directories()),
            ["dev", "proc", "run", "sys", "tmp"],
        )

    def test_top_level_of_ignores_the_leading_slash(self) -> None:
        self.assertEqual(audit.top_level_of("/sys/fs/cgroup"), "sys")
        self.assertEqual(audit.top_level_of("proc/sys/fs/binfmt_misc"), "proc")
        self.assertEqual(audit.top_level_of("/run"), "run")

    def test_it_reports_what_a_tree_is_missing(self) -> None:
        self.assertEqual(
            audit.missing_from({"usr", "etc", "var", "dev", "proc"}),
            ("run", "sys", "tmp"),
        )

    def test_a_complete_tree_is_missing_nothing(self) -> None:
        self.assertEqual(audit.missing_from({"dev", "proc", "run", "sys", "tmp"}), ())

    def test_the_tree_that_failed_is_reported_as_missing_all_five(self) -> None:
        observed = set(document()["audit"]["observedTopLevelBefore"])
        self.assertEqual(audit.missing_from(observed), ("dev", "proc", "run", "sys", "tmp"))


class BuilderTests(unittest.TestCase):
    """The builder writes them, as directories, with those modes."""

    def test_it_offers_the_five_as_builder_entries(self) -> None:
        entries = audit.mount_point_entries()
        self.assertEqual(sorted(entries), ["dev", "proc", "run", "sys", "tmp"])

    def test_each_entry_is_a_root_owned_directory(self) -> None:
        for path, entry in audit.mount_point_entries().items():
            self.assertEqual(entry["kind"], "directory", path)
            self.assertEqual(entry["path"], path)
            self.assertEqual((entry["uid"], entry["gid"]), (0, 0), path)

    def test_the_modes_survive_as_numbers(self) -> None:
        entries = audit.mount_point_entries()
        self.assertEqual(entries["tmp"]["mode"], 0o1777)
        self.assertEqual(entries["proc"]["mode"], 0o555)
        self.assertEqual(entries["dev"]["mode"], 0o755)

    def test_the_boot_projection_merges_them_into_the_tree(self) -> None:
        import scripts.native_shadow_rootfs_builder_boot_arm64_v1 as projection

        self.assertEqual(
            sorted(projection.runtime_mount_point_entries()),
            ["dev", "proc", "run", "sys", "tmp"],
        )

    def test_the_merge_is_in_the_source_the_projection_compiles(self) -> None:
        import scripts.native_shadow_rootfs_builder_boot_arm64_v1 as projection

        source = projection._derived_source()
        self.assertIn("runtime_mount_point_entries()", source)
        self.assertEqual(source.count("_ensure_parents(entries)"), 1)

    def _assemble_body(self) -> str:
        import scripts.native_shadow_rootfs_builder_boot_arm64_v1 as projection

        source = projection._derived_source()
        start = source.index("def _assemble_entries(")
        return source[start:source.index("\ndef ", start + 1)]

    def test_the_merge_lands_inside_the_function_that_assembles_the_tree(self) -> None:
        # Present somewhere in the file is not the same as reached while the
        # tree is being built.
        self.assertIn("runtime_mount_point_entries()", self._assemble_body())

    def test_the_merge_runs_before_the_parents_are_filled_in(self) -> None:
        # Order is the whole reason this is not a cosmetic detail: merging first
        # means an entry a package already ships collides loudly, and merging
        # after would silently leave the tree without the directories.
        body = self._assemble_body()
        self.assertLess(
            body.index("runtime_mount_point_entries()"),
            body.index("_ensure_parents(entries)"),
        )

    def test_the_entries_survive_the_builders_own_merge(self) -> None:
        import scripts.native_shadow_rootfs_builder_boot_arm64_v1 as projection

        merge = projection._IMPL["_merge"]
        entries = {"usr": {"path": "usr", "kind": "directory", "mode": 0o755, "uid": 0, "gid": 0}}
        merge(entries, projection.runtime_mount_point_entries(), "runtime mount point")
        self.assertEqual(
            sorted(entries), ["dev", "proc", "run", "sys", "tmp", "usr"]
        )

    def test_a_package_shipping_one_of_them_differently_still_collides(self) -> None:
        import scripts.native_shadow_rootfs_builder_boot_arm64_v1 as projection

        merge = projection._IMPL["_merge"]
        entries = {"tmp": {"path": "tmp", "kind": "file", "mode": 0o644, "uid": 0, "gid": 0}}
        with self.assertRaises(projection.RootfsBuildError):
            merge(entries, projection.runtime_mount_point_entries(), "runtime mount point")


class BoundaryTests(unittest.TestCase):
    """Adding directories to a builder is not a boot, and says so."""

    def test_it_claims_no_boot_and_no_activation(self) -> None:
        record = document()
        self.assertFalse(record["bootableClaim"])
        self.assertFalse(record["activationAllowed"])

    def test_it_does_not_claim_the_next_image_will_boot(self) -> None:
        self.assertFalse(document()["boundaries"]["guestBootVerified"])
        self.assertFalse(document()["boundaries"]["successorImageProduced"])

    def test_the_determinism_result_is_untouched(self) -> None:
        self.assertTrue(document()["determinismResultUnaffected"])


class GateTests(unittest.TestCase):
    """Held by the gates that run on every push."""

    def test_the_record_is_pinned_by_the_docs_gate(self) -> None:
        smoke = (REPO / "scripts" / "docs-smoke.sh").read_text(encoding="utf-8")
        self.assertIn(RECORD_PATH.relative_to(REPO).as_posix(), smoke)

    def test_this_module_stays_registered_in_the_self_test(self) -> None:
        self_test = (REPO / "scripts" / "self-test.sh").read_text(encoding="utf-8")
        self.assertIn(pathlib.Path(__file__).name, self_test)


if __name__ == "__main__":
    unittest.main()
