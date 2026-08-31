#!/usr/bin/env python3
"""High-value workflow contracts for the reversible ARM64 image lane."""

from __future__ import annotations

import pathlib
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github/workflows/native-shadow-closed-local-image-readiness-arm64.yml"
SEALED_PRODUCTION = ROOT / ".github/workflows/native-shadow-successor-produce-arm64-v5.yml"


class ClosedLocalWorkflowContractTests(unittest.TestCase):
    def test_lane_is_manual_read_only_and_has_no_production_authority_surface(self):
        text = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("workflow_dispatch:", text)
        self.assertIn("options: [preflight, build]", text)
        self.assertIn("permissions:\n  contents: read", text)
        self.assertNotIn("contents: write", text)
        self.assertNotIn("production-authority-arm64-v7", text)
        self.assertNotIn("git tag", text)
        self.assertNotIn("git update-ref", text)
        self.assertNotIn("activationAllowed=true", text)

    def test_build_is_two_replicas_of_the_same_backend_then_compares_all_images(self):
        text = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("matrix:\n        replica:", text)
        self.assertIn("'[1, 2]'", text)
        self.assertIn("max-parallel: 1", text)
        self.assertIn(
            "native_shadow_closed_local_image_to_readiness_arm64_v1.py", text
        )
        self.assertIn('"$script" "$MODE"', text)
        self.assertIn("IPAddressDeny=any", text)
        for name in ("guest-kernel", "guest-initrd", "guest-root-disk"):
            self.assertIn(f'cmp -- "$left/{name}" "$right/{name}"', text)

    def test_historical_production_lane_remains_locked_and_separate(self):
        production = SEALED_PRODUCTION.read_text(encoding="utf-8")
        development = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("production-authority-arm64-v7", production)
        self.assertIn("options: [rehearsal, production]", production)
        self.assertNotIn("closed-local-image-to-readiness", production)
        self.assertNotIn("production-authority-arm64-v7", development)

    def test_root_owned_results_are_checked_with_root_visibility(self):
        text = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn('sudo test -f "$result"', text)
        self.assertIn('sudo test ! -L "$result"', text)
        self.assertIn('sudo test -f "$outputs/$name"', text)
        self.assertIn('sudo test ! -L "$outputs/$name"', text)
        self.assertIn('sudo test ! -e "$outputs"', text)
        self.assertNotIn('\n          test -f "$result"', text)
        self.assertNotIn('\n            test ! -e "$outputs"', text)


if __name__ == "__main__":
    unittest.main()
