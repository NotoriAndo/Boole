#!/usr/bin/env python3
"""Regression gates for the isolated-CLI successor of the PrivateTmp lane."""

from __future__ import annotations

import contextlib
import pathlib
import subprocess
import sys
import unittest
from unittest import mock


ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts/native_shadow_closed_local_image_to_readiness_arm64_v3.py"
WORKFLOW = ROOT / ".github/workflows/native-shadow-closed-local-image-readiness-arm64-v3.yml"

from scripts import native_shadow_closed_local_image_to_readiness_arm64_v3 as v3


class ClosedLocalImageIsolatedCliSuccessorTests(unittest.TestCase):
    def test_build_publishes_the_decorated_result_exactly_once(self):
        result = pathlib.Path("/isolated/result.json")
        base = {"status": "IMAGES-BUILT", "effects": {"imagesCreated": 1}}
        published = []

        def fake_predecessor_build(**kwargs):
            v3.v2.v1._publish_result(kwargs["result"], base)
            return dict(base)

        def fake_real_publish(path, document):
            published.append((path, dict(document)))

        with (
            mock.patch.object(
                v3.v2, "_successor_contract", return_value=contextlib.nullcontext()
            ),
            mock.patch.object(v3.v2.v1, "build", side_effect=fake_predecessor_build),
            mock.patch.object(v3.v2.v1, "_publish_result", side_effect=fake_real_publish),
        ):
            document = v3.build(result=result, run_label="gh-1-1-r1")

        self.assertEqual(len(published), 1)
        self.assertEqual(published[0][0], result)
        self.assertEqual(published[0][1], document)
        self.assertEqual(document["privateTmpSuccessor"]["path"], "/var/tmp")

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

    def test_manual_workflow_rebuilds_the_sealed_launcher_from_its_historical_tree(self):
        workflow = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn(
            "ref: ff7982e9fd4583a4a7dd22825f647cea170cc3fe",
            workflow,
        )
        self.assertIn("path: _historical-launcher-v2", workflow)
        self.assertIn(
            '"$GITHUB_WORKSPACE/_historical-launcher-v2/scripts/'
            'native_shadow_launcher_emit_arm64_v2.py"',
            workflow,
        )
        self.assertIn("shutil.rmtree", workflow)
        cleanup = workflow.index("shutil.rmtree")
        copy_current = workflow.index('sudo cp -a -- "$GITHUB_WORKSPACE/." "$repo"')
        self.assertLess(cleanup, copy_current)


if __name__ == "__main__":
    unittest.main()
