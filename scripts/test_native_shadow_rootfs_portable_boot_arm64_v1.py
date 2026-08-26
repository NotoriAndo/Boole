#!/usr/bin/env python3
"""Tests for the boot projection of the frozen arm64 portable authority."""

from __future__ import annotations

import hashlib
import json
import pathlib
import shutil
import unittest

from scripts import native_shadow_rootfs_builder_boot_arm64_v1 as builder
from scripts import native_shadow_rootfs_portable_arm64_v1 as portable
from scripts import native_shadow_rootfs_portable_boot_arm64_v1 as mod


BOOT_LOCK_PATH = pathlib.Path(
    "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v1.json"
)
CAS = pathlib.Path("local-docs/native-shadow-runtime-rootfs-source-lock-v1/cas")


def boot_lock() -> dict:
    return json.loads(BOOT_LOCK_PATH.read_text(encoding="utf-8"))


def tool(name: str):
    found = shutil.which(name)
    return pathlib.Path(found).resolve() if found else None


class ProvenanceTests(unittest.TestCase):
    def test_the_frozen_portable_authority_bytes_are_pinned(self) -> None:
        raw = pathlib.Path(portable.__file__).read_bytes()
        self.assertEqual(hashlib.sha256(raw).hexdigest(), mod.ARM64_PORTABLE_SHA256)

    def test_the_projection_records_its_own_bytes(self) -> None:
        raw = pathlib.Path(mod.__file__).read_bytes()
        self.assertEqual(hashlib.sha256(raw).hexdigest(), mod.BOOT_PROJECTION_SHA256)

    def test_the_boot_projection_leaves_the_portable_module_untouched(self) -> None:
        self.assertIn("PORTABLE", portable.__getattr__("SOURCE_LOCK_RELEASE"))
        self.assertNotEqual(portable.__getattr__("SOURCE_LOCK_RELEASE"), mod.SOURCE_LOCK_RELEASE)


class ReleaseTests(unittest.TestCase):
    def test_the_accepted_release_is_the_one_the_sealed_boot_lock_carries(self) -> None:
        self.assertEqual(mod.SOURCE_LOCK_RELEASE, boot_lock()["release"])

    def test_the_schema_is_unchanged_because_only_the_release_differs(self) -> None:
        self.assertEqual(mod.SOURCE_LOCK_SCHEMA, boot_lock()["schema"])

    def test_the_portable_release_is_no_longer_accepted_here(self) -> None:
        """Widening the accepted release must not mean accepting both."""

        lock = boot_lock()
        lock["release"] = portable.__getattr__("SOURCE_LOCK_RELEASE")
        with self.assertRaises(mod.PortableAuthorityError):
            mod.materialize_runtime_lock(
                lock, builder.canonical_json(lock), pathlib.Path("/"), pathlib.Path("/")
            )


class IdentityTests(unittest.TestCase):
    """The projection changes which lock is accepted, not what is checked."""

    def test_a_non_canonical_lock_is_still_refused(self) -> None:
        lock = boot_lock()
        with self.assertRaises(mod.PortableAuthorityError):
            mod.materialize_runtime_lock(
                lock, b"{}\n", pathlib.Path("/"), pathlib.Path("/")
            )

    def test_an_activatable_lock_is_still_refused(self) -> None:
        lock = boot_lock()
        lock["activationAllowed"] = True
        with self.assertRaises(mod.PortableAuthorityError):
            mod.materialize_runtime_lock(
                lock, builder.canonical_json(lock), pathlib.Path("/"), pathlib.Path("/")
            )

    def test_a_lock_without_the_declared_tool_roles_is_still_refused(self) -> None:
        lock = boot_lock()
        lock["buildRecipe"]["zstdToolRole"] = "not-zstd"
        with self.assertRaises(mod.PortableAuthorityError):
            mod.materialize_runtime_lock(
                lock, builder.canonical_json(lock), pathlib.Path("/"), pathlib.Path("/")
            )


class AliasTests(unittest.TestCase):
    def test_the_x86_only_lib64_alias_stays_removed(self) -> None:
        paths = {row["logicalPath"] for row in mod.__getattr__("PORTABLE_V2_DERIVED_ENTRIES")}
        self.assertNotIn("/lib64", paths)
        self.assertIn("/lib", paths)


class EndToEndTests(unittest.TestCase):
    """Runs only where the acquired closure is on disk; CI proves this separately."""

    def setUp(self) -> None:
        self.gpgv = tool("gpgv")
        self.zstd = tool("zstd")
        if not (CAS / "sha256").is_dir():
            self.skipTest("acquired artifact store is not present on this host")
        if self.gpgv is None or self.zstd is None:
            self.skipTest("gpgv and zstd are not both present on this host")

    def test_the_sealed_boot_lock_passes_the_projected_builder(self) -> None:
        lock = boot_lock()
        runtime, receipt = mod.materialize_runtime_lock(
            lock, builder.canonical_json(lock), self.gpgv, self.zstd
        )
        self.assertIs(receipt["activationAllowed"], False)
        normalized, raw, record = builder.normalized_runtime_lock(runtime)
        self.assertEqual(record["reorderedClosures"], ["guest-init-and-launcher-file-manifest"])
        builder.validate_source_lock(
            normalized, raw, pathlib.Path("."), CAS, require_complete=True
        )


class BoundaryTests(unittest.TestCase):
    def test_binding_replay_tools_is_not_booting_anything(self) -> None:
        self.assertIs(mod.BOOTABLE_CLAIM, False)
        self.assertIs(mod.ACTIVATION_ALLOWED, False)


if __name__ == "__main__":
    unittest.main()
