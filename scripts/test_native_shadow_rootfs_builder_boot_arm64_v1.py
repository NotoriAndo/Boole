#!/usr/bin/env python3
"""Tests for the boot projection of the frozen arm64 rootfs builder."""

from __future__ import annotations

import hashlib
import json
import pathlib
import unittest

from scripts import native_shadow_rootfs_builder_arm64_v1 as arm64
from scripts import native_shadow_rootfs_builder_boot_arm64_v1 as mod


BOOT_LOCK_PATH = pathlib.Path(
    "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v1.json"
)


def boot_lock() -> dict:
    return json.loads(BOOT_LOCK_PATH.read_text(encoding="utf-8"))


def candidate(**overrides) -> dict:
    row = {
        "name": "python3",
        "version": "3.12.3-0ubuntu1",
        "provides": "",
        "multiArch": "allowed",
    }
    row.update(overrides)
    return row


class ProvenanceTests(unittest.TestCase):
    def test_the_projected_builder_digest_equals_the_pin_in_the_sealed_boot_lock(self) -> None:
        """The lock pins the builder it was written for; the projection must be it."""

        self.assertEqual(mod.BUILDER_SHA256, boot_lock()["buildRecipe"]["builderSha256"])

    def test_the_frozen_arm64_builder_bytes_are_pinned(self) -> None:
        raw = pathlib.Path(arm64.__file__).read_bytes()
        self.assertEqual(hashlib.sha256(raw).hexdigest(), mod.ARM64_BUILDER_SHA256)

    def test_the_projection_records_its_own_bytes_separately(self) -> None:
        """The widening is not inside builderSha256, so it gets its own digest."""

        raw = pathlib.Path(mod.__file__).read_bytes()
        self.assertEqual(hashlib.sha256(raw).hexdigest(), mod.BOOT_PROJECTION_SHA256)
        self.assertNotEqual(mod.BOOT_PROJECTION_SHA256, mod.BUILDER_SHA256)

    def test_the_boot_projection_leaves_the_arm64_module_untouched(self) -> None:
        """A widened boot table must never leak into the portable arm64 path."""

        self.assertEqual(len(arm64.__getattr__("EXPECTED_AUTHORITY_FILES")), 6)
        self.assertEqual(len(arm64.__getattr__("REQUIRED_PROVENANCE_CLOSURES")), 3)


class WideningTests(unittest.TestCase):
    def test_the_authority_table_covers_every_tracked_file_in_the_boot_lock(self) -> None:
        table = mod.__getattr__("EXPECTED_AUTHORITY_FILES")
        self.assertEqual(len(table), 10)
        expected = {row["sourcePath"]: row["logicalPath"] for row in boot_lock()["trackedFiles"]}
        self.assertEqual({source: logical for source, logical in table.values()}, expected)

    def test_the_launcher_unit_and_its_supporting_files_are_in_the_table(self) -> None:
        table = mod.__getattr__("EXPECTED_AUTHORITY_FILES")
        for identifier in ("guest-machine-id", "launcher-unit", "sysusers-config", "tmpfiles-config"):
            self.assertIn(identifier, table)

    def test_the_closure_table_covers_every_declared_closure(self) -> None:
        names = mod.__getattr__("REQUIRED_PROVENANCE_CLOSURES")
        self.assertEqual(names, tuple(row["name"] for row in boot_lock()["closureRoots"]))
        roots = mod.__getattr__("EXPECTED_PROVENANCE_CLOSURE_ROOTS")
        for name in names:
            self.assertIn(name, roots)


class ClosureRootOrderTests(unittest.TestCase):
    def test_both_the_declared_and_the_normalized_order_are_recorded(self) -> None:
        """Sorting silently would hide the very difference worth seeing."""

        _, _, record = mod.normalized_runtime_lock(boot_lock())
        rows = {row["name"]: row for row in record["closureRoots"]}
        row = rows["guest-init-and-launcher-file-manifest"]
        self.assertTrue(row["reordered"])
        self.assertNotEqual(row["declaredOrder"], row["normalizedOrder"])
        self.assertEqual(row["normalizedOrder"], sorted(row["declaredOrder"]))
        self.assertEqual(record["reorderedClosures"], ["guest-init-and-launcher-file-manifest"])

    def test_the_normalized_lock_is_canonical_and_sorted(self) -> None:
        lock, raw, _ = mod.normalized_runtime_lock(boot_lock())
        self.assertEqual(mod.canonical_json(lock), raw)
        for closure in lock["closureRoots"]:
            self.assertEqual(closure["logicalRoots"], sorted(closure["logicalRoots"]))

    def test_a_duplicate_logical_root_is_refused_rather_than_silently_deduped(self) -> None:
        lock = boot_lock()
        lock["closureRoots"][0]["logicalRoots"].append(lock["closureRoots"][0]["logicalRoots"][0])
        with self.assertRaises(mod.BootProjectionError):
            mod.normalized_runtime_lock(lock)

    def test_normalizing_never_changes_the_set_of_roots(self) -> None:
        before = {tuple(sorted(row["logicalRoots"])) for row in boot_lock()["closureRoots"]}
        lock, _, _ = mod.normalized_runtime_lock(boot_lock())
        self.assertEqual({tuple(row["logicalRoots"]) for row in lock["closureRoots"]}, before)


class ArchitectureTests(unittest.TestCase):
    def test_the_sealed_boot_closure_is_single_architecture(self) -> None:
        self.assertEqual(mod.concrete_architectures(boot_lock()), ("arm64",))

    def test_a_second_concrete_architecture_stops_the_build(self) -> None:
        """`:any` is only equal to the bare name while one architecture exists."""

        lock = boot_lock()
        lock["ubuntu"]["packages"][0] = dict(lock["ubuntu"]["packages"][0], architecture="amd64")
        with self.assertRaises(mod.BootProjectionError):
            mod.assert_single_architecture(lock)

    def test_an_architecture_disagreeing_with_the_platform_stops_the_build(self) -> None:
        lock = boot_lock()
        lock["platform"] = dict(lock["platform"], debArchitecture="riscv64")
        with self.assertRaises(mod.BootProjectionError):
            mod.assert_single_architecture(lock)


class DependencyScopeTests(unittest.TestCase):
    """The frozen check is right; it was being applied to the wrong string."""

    def test_a_version_range_spanning_two_dependencies_is_not_a_qualifier(self) -> None:
        groups = mod.__getattr__("_split_dependency_groups")(
            "python3 (<< 3.13), python3 (>= 3.12~), libc6 (>= 2.17)"
        )
        self.assertEqual(groups[0], ["python3 (<< 3.13)"])
        self.assertEqual(len(groups), 3)

    def test_a_real_architecture_qualifier_is_still_refused(self) -> None:
        with self.assertRaises(mod.RootfsBuildError):
            mod.__getattr__("_split_dependency_groups")("libc6, gcc [arm64]")

    def test_a_build_profile_is_still_refused(self) -> None:
        with self.assertRaises(mod.RootfsBuildError):
            mod.__getattr__("_split_dependency_groups")("libc6, dpkg-dev <!nocheck>")

    def test_a_qualifier_inside_an_alternative_is_still_refused(self) -> None:
        with self.assertRaises(mod.RootfsBuildError):
            mod.__getattr__("_split_dependency_groups")("libc6 | gcc [arm64]")

    def test_no_dependency_in_the_frozen_index_carries_a_real_qualifier(self) -> None:
        """The narrowed scope drops false positives only; there is nothing else to drop."""

        splitter = mod.__getattr__("_split_dependency_groups")
        for package in boot_lock()["ubuntu"]["packages"]:
            for field in ("depends", "preDepends"):
                splitter(package.get(field, ""))


class MultiArchTests(unittest.TestCase):
    def test_an_any_qualified_dependency_matches_the_single_architecture_provider(self) -> None:
        matches = mod.__getattr__("_dependency_matches")
        self.assertTrue(matches("python3:any", candidate()))

    def test_an_any_qualifier_without_multi_arch_allowed_is_refused(self) -> None:
        matches = mod.__getattr__("_dependency_matches")
        with self.assertRaises(mod.RootfsBuildError):
            matches("python3:any", candidate(multiArch="same"))

    def test_a_native_qualifier_is_still_refused(self) -> None:
        matches = mod.__getattr__("_dependency_matches")
        with self.assertRaises(mod.RootfsBuildError):
            matches("python3:native", candidate())

    def test_only_python3_uses_the_widened_qualifier_in_the_sealed_lock(self) -> None:
        used = set()
        for package in boot_lock()["ubuntu"]["packages"]:
            for field in ("depends", "preDepends"):
                for group in (package.get(field, "") or "").split(","):
                    for alternative in group.split("|"):
                        alternative = alternative.strip()
                        if ":any" in alternative:
                            used.add(alternative)
        self.assertEqual(used, {"python3:any"})


class ValidationTests(unittest.TestCase):
    def test_an_unsorted_lock_is_refused_with_a_pointer_to_the_normalizer(self) -> None:
        lock = boot_lock()
        with self.assertRaises(mod.BootProjectionError) as caught:
            mod.validate_source_lock(
                lock,
                mod.canonical_json(lock),
                pathlib.Path("."),
                None,
                require_complete=False,
            )
        self.assertIn("normalized_runtime_lock", str(caught.exception))


class BoundaryTests(unittest.TestCase):
    def test_projecting_a_builder_is_not_building_an_image(self) -> None:
        self.assertIs(mod.BOOTABLE_CLAIM, False)
        self.assertIs(mod.ACTIVATION_ALLOWED, False)

    def test_the_boot_release_string_is_the_one_the_sealed_lock_carries(self) -> None:
        self.assertEqual(mod.BOOT_SOURCE_LOCK_RELEASE, boot_lock()["release"])
        self.assertIn("NOT-BOOTABLE", mod.BOOT_SOURCE_LOCK_RELEASE)


if __name__ == "__main__":
    unittest.main()
