#!/usr/bin/env python3
"""The fifth step: one merge path, and a measurement that cannot build an image.

The sealed plan bounded the assembled totals rather than measuring them, and the
operator's instruction is to replace the bound with a measurement taken the way
production will later take it: assemble a temporary tree through the same merge
the production assembler will use, walk it, and compare the walk against what the
builder computed internally.  Two numbers added together would not have been that.

This gate covers the two modules that instruction needs.

``native_shadow_rootfs_builder_boot_arm64_v3`` is where the merge lives.  It is a
projection of the fourth step in the same sense the fourth step is a projection of
the third: the predecessor keeps its bytes and its refusal to merge, and this one
threads the nested tree into ``_assemble_entries`` at the point the boot
projection already reserved for entries the lock does not name.  Because
``build_oci_layout`` calls that same function, production gets the merge by
consuming the module rather than by repeating it -- which is the property
``SharedMaterializationTests`` exists to prove.

``native_shadow_boot_staging_measure_arm64_v1`` is the measurement.  It writes the
assembled table to a directory and reads it back with a walk that knows nothing
about the table, and the two have to agree on every number the plan names.  What
it may not do is produce an image, and ``MeasurementIsNotProductionTests`` is the
part of this gate that says so: the module names no filesystem builder, no initrd
tool and no workflow dispatch, and the one place it is allowed to run a
subprocess refuses anything outside the two replay tools.

Nothing here reads the payload store, so the gate runs where the artifacts are
absent.  The measurement itself is a separate run against the sealed store, and
its result is sealed in a file this gate only reads for shape.
"""

from __future__ import annotations

import hashlib
import inspect
import json
import pathlib
import tempfile
import unittest

from scripts import native_shadow_boot_staging_measure_arm64_v1 as measure
from scripts import native_shadow_rootfs_builder_boot_arm64_v2 as boot_v2
from scripts import native_shadow_rootfs_builder_boot_arm64_v3 as mod


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
SEALED_LOCK_PATH = (
    REPO_ROOT / "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v2.json"
)
MEASUREMENT_PATH = (
    REPO_ROOT
    / "native/containment/native-shadow-boot-staging-tree-measurement-arm64-v1.json"
)


def read_json(path: pathlib.Path) -> dict:
    return json.loads(path.read_bytes().decode("utf-8"))


def sealed_recipe() -> dict:
    return read_json(SEALED_LOCK_PATH)["buildRecipe"]


def tiny_tree() -> dict:
    """A hand-written entry table in the shape the builder produces."""

    return {
        "etc": {"path": "etc", "kind": "directory", "mode": 0o755, "uid": 0, "gid": 0},
        "etc/hosts": {
            "path": "etc/hosts",
            "kind": "file",
            "mode": 0o644,
            "uid": 0,
            "gid": 0,
            "raw": b"127.0.0.1 localhost\n",
        },
        "etc/étc.conf": {
            "path": "etc/étc.conf",
            "kind": "file",
            "mode": 0o644,
            "uid": 0,
            "gid": 0,
            "raw": b"non-ascii so byte order and text order differ\n",
        },
        "etc/localhost": {
            "path": "etc/localhost",
            "kind": "symlink",
            "mode": 0o777,
            "uid": 0,
            "gid": 0,
            "target": "hosts",
            "resolvedTarget": "etc/hosts",
        },
    }


class ProvenanceTests(unittest.TestCase):
    """The successor pins the bytes it was written against."""

    def test_the_predecessor_builder_bytes_are_pinned(self) -> None:
        raw = pathlib.Path(boot_v2.__file__).resolve().read_bytes()
        self.assertEqual(hashlib.sha256(raw).hexdigest(), mod.BOOT_V2_SHA256)

    def test_the_successor_records_its_own_bytes(self) -> None:
        raw = pathlib.Path(mod.__file__).resolve().read_bytes()
        self.assertEqual(hashlib.sha256(raw).hexdigest(), mod.SUCCESSOR_PROJECTION_SHA256)

    def test_the_staging_table_is_inherited_unchanged(self) -> None:
        self.assertEqual(mod.BOOT_AUTHORITY_FILES, boot_v2.BOOT_AUTHORITY_FILES)
        self.assertEqual(len(mod.BOOT_AUTHORITY_FILES), 9)
        self.assertEqual(len(mod._IMPL["EXPECTED_AUTHORITY_FILES"]), 15)
        self.assertEqual(
            mod._IMPL["EXPECTED_AUTHORITY_FILES"],
            boot_v2._IMPL["EXPECTED_AUTHORITY_FILES"],
        )

    def test_the_nested_tree_declaration_is_inherited_unchanged(self) -> None:
        self.assertEqual(mod.NESTED_RUNTIME_TREE, boot_v2.NESTED_RUNTIME_TREE)

    def test_the_predecessor_still_refuses_to_merge(self) -> None:
        """The fourth step said it does not merge, and it still does not."""

        predecessor = boot_v2._derived_source()
        self.assertNotIn('"nested runtime tree"', predecessor)
        self.assertFalse(boot_v2.NESTED_RUNTIME_TREE_ASSEMBLED)


class MergeProjectionTests(unittest.TestCase):
    """The nested tree is threaded through the builder, not merged beside it."""

    def test_the_derived_source_merges_before_parents_are_derived(self) -> None:
        source = mod._derived_source()
        merge = '    _merge(entries, nested_tree, "nested runtime tree")\n'
        self.assertIn(merge, source)
        self.assertLess(source.index(merge), source.index("    _ensure_parents(entries)\n"))

    def test_the_mount_points_are_still_merged_first(self) -> None:
        source = mod._derived_source()
        self.assertLess(
            source.index('_merge(entries, runtime_mount_point_entries(), "runtime mount point")'),
            source.index('_merge(entries, nested_tree, "nested runtime tree")'),
        )

    def test_every_replacement_applied_exactly_as_many_times_as_declared(self) -> None:
        source = boot_v2._derived_source()
        for old, _new, count in mod.REPLACEMENTS:
            self.assertEqual(source.count(old), count, msg=old[:60])

    def test_the_merge_is_optional_so_the_predecessor_build_is_unchanged(self) -> None:
        signature = inspect.signature(mod.__getattr__("_assemble_entries"))
        self.assertIsNone(signature.parameters["nested_tree"].default)
        signature = inspect.signature(mod.__getattr__("build_oci_layout"))
        self.assertIsNone(signature.parameters["nested_tree"].default)


class SharedMaterializationTests(unittest.TestCase):
    """One function assembles the tree, and both callers reach it."""

    def test_both_entry_points_call_the_same_assembler(self) -> None:
        """The named function and the layout builder reach one ``_assemble_entries``.

        Production consumes the merge by consuming ``build_oci_layout``; the
        measurement consumes it by name.  Recording the calls is how this gate
        says the two are the same object rather than two that agree today.
        """

        seen = []

        def recorder(validated, repository_root, artifact_store, launcher_binary=None, nested_tree=None):
            seen.append(nested_tree)
            return {}

        def validator(*args, **kwargs):
            return {"lock": {"buildRecipe": {"canonicalMtime": 0}, "closureRoots": []}}

        assembler = mod._IMPL["_assemble_entries"]
        validate = mod._IMPL["validate_source_lock"]
        manifest = mod._IMPL["_entry_manifest"]
        nested = tiny_tree()
        with tempfile.TemporaryDirectory(prefix="boole-shared-materialize.") as temporary:
            try:
                mod._IMPL["_assemble_entries"] = recorder
                mod._IMPL["validate_source_lock"] = validator
                mod._IMPL["_entry_manifest"] = lambda entries, closures: {"entries": []}
                mod.materialize_staging_tree(
                    validator(), REPO_ROOT, REPO_ROOT, nested_tree=nested
                )
                self.assertEqual(seen, [nested])
                mod.__getattr__("build_oci_layout")(
                    {"closureRoots": [], "buildRecipe": {"canonicalMtime": 0}},
                    b"{}",
                    REPO_ROOT,
                    REPO_ROOT,
                    pathlib.Path(temporary) / "layout",
                    nested_tree=nested,
                )
                self.assertEqual(seen, [nested, nested])
            finally:
                mod._IMPL["_assemble_entries"] = assembler
                mod._IMPL["validate_source_lock"] = validate
                mod._IMPL["_entry_manifest"] = manifest

    def test_a_nested_entry_that_collides_is_refused_in_the_builders_words(self) -> None:
        entries = {"etc": dict(tiny_tree()["etc"], mode=0o700)}
        with self.assertRaises(mod.RootfsBuildError) as caught:
            mod.__getattr__("_merge")(dict(tiny_tree()), entries, "nested runtime tree")
        self.assertIn("rootfs path collision differs (nested runtime tree)", str(caught.exception))

    def test_the_rerooting_is_the_predecessors_and_is_not_repeated_here(self) -> None:
        """The fourth step wrote ``nested_tree_entries`` and its gate covers it."""

        source = pathlib.Path(mod.__file__).read_text(encoding="utf-8")
        self.assertIn("boot_v2.nested_tree_entries(", source)
        self.assertNotIn('guestPrefix"].lstrip', source)
        self.assertNotIn("contentManifestGuestPath\"].lstrip", source)


class TraversalTests(unittest.TestCase):
    """The walk knows nothing about the table, and has to agree with it anyway."""

    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="boole-staging-measure.")
        self.addCleanup(self.temporary.cleanup)
        self.root = pathlib.Path(self.temporary.name) / "tree"
        self.entries = tiny_tree()

    def stage(self) -> dict:
        measure.write_staging_tree(self.entries, self.root, mtime=0)
        return measure.traverse_staging_tree(self.root)

    def test_the_walk_reproduces_the_builders_numbers(self) -> None:
        walked = self.stage()
        computed = measure.builder_totals(self.entries)
        measure.assert_measurements_agree(computed, walked)
        self.assertEqual(walked["entries"], 4)
        self.assertEqual(walked["byKind"], {"directory": 1, "file": 2, "symlink": 1})
        self.assertEqual(
            walked["payloadBytes"],
            sum(len(entry["raw"]) for entry in self.entries.values() if entry["kind"] == "file"),
        )

    def test_the_path_manifest_is_the_byte_ordered_path_list(self) -> None:
        """One newline-terminated path per line, ordered by the paths' own bytes.

        The non-ASCII name is in the tree so the encoding is exercised rather than
        assumed; UTF-8 preserves code point order, so the byte key and a plain
        sort agree, and the key is written out because the frozen builder orders
        its layer members the same way.
        """

        walked = self.stage()
        paths = sorted(self.entries, key=lambda value: value.encode("utf-8"))
        expected = hashlib.sha256(
            b"".join(path.encode("utf-8") + b"\n" for path in paths)
        ).hexdigest()
        self.assertEqual(walked["pathManifestSha256"], expected)
        self.assertIn("etc/étc.conf", paths)

    def test_an_extra_file_on_disk_fails_the_comparison(self) -> None:
        walked = self.stage()
        (self.root / "etc" / "extra").write_bytes(b"x")
        again = measure.traverse_staging_tree(self.root)
        with self.assertRaises(measure.StagingMeasurementError):
            measure.assert_measurements_agree(measure.builder_totals(self.entries), again)
        self.assertEqual(walked["entries"] + 1, again["entries"])

    def test_a_symlink_that_leaves_the_tree_is_refused(self) -> None:
        self.entries["etc/escape"] = {
            "path": "etc/escape",
            "kind": "symlink",
            "mode": 0o777,
            "uid": 0,
            "gid": 0,
            "target": "../../outside",
            "resolvedTarget": "outside",
        }
        with self.assertRaises(measure.StagingMeasurementError) as caught:
            self.stage()
        self.assertIn("leaves the staging tree", str(caught.exception))

    def test_the_largest_file_is_named_as_well_as_sized(self) -> None:
        walked = self.stage()
        largest = max(
            (len(entry["raw"]), entry["path"])
            for entry in self.entries.values()
            if entry["kind"] == "file"
        )
        self.assertEqual(walked["largestFileBytes"], largest[0])
        self.assertEqual(walked["largestFilePath"], largest[1])


class CaseSensitivityTests(unittest.TestCase):
    """A case-folding filesystem would silently lose entries, so it is refused."""

    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="boole-staging-case.")
        self.addCleanup(self.temporary.cleanup)
        self.root = pathlib.Path(self.temporary.name)

    def test_the_probe_reports_this_directory(self) -> None:
        self.assertIsInstance(measure.filesystem_is_case_sensitive(self.root), bool)

    def test_a_case_folding_directory_is_refused_before_anything_is_written(self) -> None:
        original = measure.filesystem_is_case_sensitive
        try:
            measure.filesystem_is_case_sensitive = lambda path: False
            with self.assertRaises(measure.StagingMeasurementError) as caught:
                measure.assert_case_sensitive(self.root)
            self.assertIn("case-sensitive", str(caught.exception))
        finally:
            measure.filesystem_is_case_sensitive = original

    def test_a_case_sensitive_directory_passes(self) -> None:
        original = measure.filesystem_is_case_sensitive
        try:
            measure.filesystem_is_case_sensitive = lambda path: True
            measure.assert_case_sensitive(self.root)
        finally:
            measure.filesystem_is_case_sensitive = original


class LimitTests(unittest.TestCase):
    """The three limits come from the sealed lock, and excess fails immediately."""

    def test_the_limits_are_the_sealed_recipes_own_numbers(self) -> None:
        recipe = sealed_recipe()
        self.assertEqual(recipe["maxEntries"], 200000)
        self.assertEqual(recipe["maxFileBytes"], 536870912)
        self.assertEqual(recipe["maxTotalBytes"], 2147483648)

    def test_a_measurement_within_the_limits_passes(self) -> None:
        measure.assert_within_limits(
            sealed_recipe(),
            {"entries": 10, "payloadBytes": 10, "largestFileBytes": 10},
        )

    def test_too_many_entries_fails(self) -> None:
        with self.assertRaises(measure.StagingMeasurementError) as caught:
            measure.assert_within_limits(
                sealed_recipe(),
                {"entries": 200001, "payloadBytes": 10, "largestFileBytes": 10},
            )
        self.assertIn("entry limit", str(caught.exception))

    def test_too_many_payload_bytes_fails(self) -> None:
        with self.assertRaises(measure.StagingMeasurementError) as caught:
            measure.assert_within_limits(
                sealed_recipe(),
                {"entries": 10, "payloadBytes": 2147483649, "largestFileBytes": 10},
            )
        self.assertIn("total byte limit", str(caught.exception))

    def test_one_file_over_the_single_file_limit_fails(self) -> None:
        with self.assertRaises(measure.StagingMeasurementError) as caught:
            measure.assert_within_limits(
                sealed_recipe(),
                {"entries": 10, "payloadBytes": 10, "largestFileBytes": 536870913},
            )
        self.assertIn("single file limit", str(caught.exception))

    def test_nothing_is_truncated_or_excluded_to_fit(self) -> None:
        source = pathlib.Path(measure.__file__).read_text(encoding="utf-8")
        for forbidden in ("truncate", "[:limit]", "islice", "del entries["):
            self.assertNotIn(forbidden, source)


class MeasurementIsNotProductionTests(unittest.TestCase):
    """Measurement mode cannot reach the tools that would make an image."""

    def source(self) -> str:
        return pathlib.Path(measure.__file__).read_text(encoding="utf-8")

    def test_no_image_tool_is_named_outside_the_refusal_list(self) -> None:
        source = self.source()
        for tool in measure.FORBIDDEN_EXECUTABLES:
            self.assertEqual(
                source.count(tool),
                1,
                msg=f"{tool} appears outside FORBIDDEN_EXECUTABLES",
            )

    def test_the_production_phase_module_is_not_imported(self) -> None:
        source = self.source()
        self.assertNotIn("native_shadow_boot_produce_phase", source)
        self.assertNotIn("root_disk", source)
        self.assertNotIn("initrd", source)
        self.assertNotIn("kernel_extract", source)

    def test_a_forbidden_tool_is_refused_before_it_is_run(self) -> None:
        for tool in measure.FORBIDDEN_EXECUTABLES:
            with self.assertRaises(measure.StagingMeasurementError) as caught:
                measure.assert_replay_tool(pathlib.Path("/usr/sbin") / tool)
            self.assertIn("measurement mode", str(caught.exception))

    def test_only_the_two_replay_tools_are_allowed(self) -> None:
        self.assertEqual(sorted(measure.ALLOWED_REPLAY_TOOLS), ["gpgv", "zstd"])
        with self.assertRaises(measure.StagingMeasurementError):
            measure.assert_replay_tool(pathlib.Path("/bin/tar"))

    def test_the_module_claims_nothing_it_did_not_do(self) -> None:
        self.assertFalse(measure.IMAGE_PRODUCED_CLAIM)
        self.assertFalse(measure.SERVING_CLAIM)
        self.assertFalse(measure.BOOT_CLAIM)
        self.assertFalse(mod.BOOTABLE_CLAIM)
        self.assertFalse(mod.ACTIVATION_ALLOWED)


class SealedMeasurementTests(unittest.TestCase):
    """The sealed result says what was measured and what it does not license."""

    def setUp(self) -> None:
        if not MEASUREMENT_PATH.exists():
            self.skipTest("the measurement has not been taken yet")
        self.record = read_json(MEASUREMENT_PATH)

    def test_the_schema_and_the_refusals_are_on_the_record(self) -> None:
        self.assertEqual(self.record["schema"], measure.MEASUREMENT_SCHEMA)
        self.assertFalse(self.record["imageProduced"])
        self.assertFalse(self.record["servingClaim"])
        self.assertFalse(self.record["bootClaim"])
        self.assertTrue(self.record["imageProductionPreconditionsMet"])

    def test_both_measurements_are_recorded_and_agree(self) -> None:
        builder = self.record["builderInternal"]
        walked = self.record["independentTraversal"]
        for key in ("entries", "byKind", "payloadBytes", "largestFileBytes", "pathManifestSha256"):
            self.assertEqual(builder[key], walked[key], msg=key)

    def test_the_totals_are_inside_the_sealed_limits(self) -> None:
        recipe = sealed_recipe()
        walked = self.record["independentTraversal"]
        self.assertLessEqual(walked["entries"], recipe["maxEntries"])
        self.assertLessEqual(walked["payloadBytes"], recipe["maxTotalBytes"])
        self.assertLessEqual(walked["largestFileBytes"], recipe["maxFileBytes"])

    def test_the_nested_content_manifest_is_the_one_the_launcher_reads(self) -> None:
        nested = self.record["nestedContentManifest"]
        self.assertEqual(nested["sha256"], mod.NESTED_RUNTIME_TREE["contentManifestSha256"])
        self.assertEqual(nested["sizeBytes"], mod.NESTED_RUNTIME_TREE["contentManifestSizeBytes"])
        self.assertEqual(nested["guestPath"], mod.NESTED_RUNTIME_TREE["contentManifestGuestPath"])

    def test_the_measured_total_supersedes_the_declared_bound(self) -> None:
        self.assertFalse(mod.NESTED_RUNTIME_TREE["layerSizeBytesIsAMeasuredTotal"])
        self.assertTrue(self.record["payloadBytesIsAMeasuredTotal"])
        self.assertGreater(
            self.record["independentTraversal"]["payloadBytes"],
            mod.NESTED_RUNTIME_TREE["layerSizeBytes"],
        )

    def test_no_collision_duplicate_or_escape_was_found(self) -> None:
        walked = self.record["independentTraversal"]
        self.assertEqual(walked["pathCollisions"], 0)
        self.assertEqual(walked["duplicatePaths"], 0)
        self.assertEqual(walked["symlinkEscapes"], 0)


if __name__ == "__main__":
    unittest.main()
