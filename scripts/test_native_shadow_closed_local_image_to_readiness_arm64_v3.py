#!/usr/bin/env python3
"""Regression gates for the isolated-CLI successor of the PrivateTmp lane."""

from __future__ import annotations

import pathlib
import subprocess
import sys
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts/native_shadow_closed_local_image_to_readiness_arm64_v3.py"
WORKFLOW = ROOT / ".github/workflows/native-shadow-closed-local-image-readiness-arm64-v3.yml"


class ClosedLocalImageIsolatedCliSuccessorTests(unittest.TestCase):
    def test_direct_isolated_cli_can_import_the_private_tmp_successor(self):
        completed = subprocess.run(
            [sys.executable, "-I", "-S", str(SCRIPT), "--help"],
            cwd="/tmp",
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertIn("usage:", completed.stdout)

    def test_manual_workflow_uses_v3_and_keeps_v2_historical(self):
        v2 = (
            ROOT / ".github/workflows/native-shadow-closed-local-image-readiness-arm64-v2.yml"
        ).read_text(encoding="utf-8")
        v3 = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("native_shadow_closed_local_image_to_readiness_arm64_v2.py", v2)
        self.assertNotIn("native_shadow_closed_local_image_to_readiness_arm64_v3.py", v2)
        self.assertIn("native_shadow_closed_local_image_to_readiness_arm64_v3.py", v3)
        self.assertIn("test_native_shadow_closed_local_image_to_readiness_arm64_v3", v3)
        self.assertIn("options: [preflight, build]", v3)


if __name__ == "__main__":
    unittest.main()
