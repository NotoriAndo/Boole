#!/usr/bin/env python3
"""Regression gates for the read-only PrivateTmp successor image lane."""

from __future__ import annotations

import hashlib
import pathlib
import types
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]

from scripts import native_shadow_closed_local_image_to_readiness_arm64_v1 as v1
from scripts import native_shadow_closed_local_image_to_readiness_arm64_v2 as v2
from scripts.test_native_shadow_closed_local_image_to_readiness_arm64_v1 import (
    exact_development_readback_tree,
)


class ClosedLocalImagePrivateTmpSuccessorTests(unittest.TestCase):
    def test_predecessor_is_hash_pinned_and_not_edited(self):
        path = ROOT / "scripts/native_shadow_closed_local_image_to_readiness_arm64_v1.py"
        self.assertEqual(path.stat().st_size, v2.PREDECESSOR_SIZE_BYTES)
        self.assertEqual(hashlib.sha256(path.read_bytes()).hexdigest(), v2.PREDECESSOR_SHA256)

    def test_successor_adds_exactly_one_sticky_private_tmp_directory(self):
        relay = b"arm64-linux-relay"
        predecessor = v1._development_mac4_entries(ROOT, relay)
        successor = v2._successor_mac4_entries(ROOT, relay)
        self.assertEqual(set(successor) - set(predecessor), {v2.PRIVATE_TMP_PATH})
        self.assertEqual(
            successor[v2.PRIVATE_TMP_PATH],
            {
                "path": v2.PRIVATE_TMP_PATH,
                "kind": "directory",
                "mode": 0o1777,
                "uid": 0,
                "gid": 0,
            },
        )
        self.assertEqual(
            {path: row for path, row in successor.items() if path != v2.PRIVATE_TMP_PATH},
            predecessor,
        )

    def test_readback_rejects_missing_or_nonsticky_private_tmp(self):
        exact = exact_development_readback_tree()
        mounted = "/" + v2.PRIVATE_TMP_PATH
        for replacement in (None, {"kind": "directory", "mode": 0o777, "uid": 0, "gid": 0}):
            observed = dict(exact)
            if replacement is None:
                observed.pop(mounted, None)
            else:
                observed[mounted] = replacement

            class Delegate:
                def read_tree(self, _mountpoint):
                    return observed

            module = types.SimpleNamespace(HostReadbackEffects=Delegate)
            effects = v2.SuccessorAutoclearReadbackEffects(module)
            with self.assertRaisesRegex(
                v1.ClosedLocalImageError, "MAC.4 private temporary directory differs"
            ):
                effects.read_tree(pathlib.Path("mounted-root"))

    def test_successor_patch_scope_restores_all_predecessor_globals(self):
        original_entries = v1._development_mac4_entries
        original_paths = v1.MAC4_OVERLAY_PATHS
        original_effects = v1.DevelopmentAutoclearReadbackEffects
        with v2._successor_contract():
            self.assertIs(v1._development_mac4_entries, v2._successor_mac4_entries)
            self.assertEqual(v1.MAC4_OVERLAY_PATHS, (*original_paths, v2.PRIVATE_TMP_PATH))
            self.assertIs(v1.DevelopmentAutoclearReadbackEffects, v2.SuccessorAutoclearReadbackEffects)
        self.assertIs(v1._development_mac4_entries, original_entries)
        self.assertEqual(v1.MAC4_OVERLAY_PATHS, original_paths)
        self.assertIs(v1.DevelopmentAutoclearReadbackEffects, original_effects)

    def test_new_manual_workflow_consumes_v2_without_mutating_v1(self):
        old = (ROOT / ".github/workflows/native-shadow-closed-local-image-readiness-arm64.yml").read_text()
        new = (ROOT / ".github/workflows/native-shadow-closed-local-image-readiness-arm64-v2.yml").read_text()
        self.assertIn("options: [preflight, build]", old)
        self.assertIn("native_shadow_closed_local_image_to_readiness_arm64_v1.py", old)
        self.assertNotIn("native_shadow_closed_local_image_to_readiness_arm64_v2.py", old)
        self.assertIn("options: [preflight, build]", new)
        self.assertIn("native_shadow_closed_local_image_to_readiness_arm64_v2.py", new)
        self.assertIn("test_native_shadow_closed_local_image_to_readiness_arm64_v2", new)


if __name__ == "__main__":
    unittest.main()
