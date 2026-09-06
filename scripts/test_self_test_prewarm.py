#!/usr/bin/env python3
"""Behavioural contract for self-test's prewarm exit-status boundary."""
from __future__ import annotations

import os
import stat
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PREWARM = ROOT / "scripts" / "self-test-prewarm.sh"


class SelfTestPrewarmTests(unittest.TestCase):
    def run_fixture(self, exit_status: int) -> subprocess.CompletedProcess[str]:
        with tempfile.TemporaryDirectory(prefix="boole-prewarm-contract-") as temp:
            binary = Path(temp) / "fixture-bin"
            binary.write_text("#!/usr/bin/env bash\nexit \"${PREWARM_FIXTURE_STATUS}\"\n", encoding="utf-8")
            binary.chmod(binary.stat().st_mode | stat.S_IXUSR)
            environment = os.environ | {"PREWARM_FIXTURE_STATUS": str(exit_status)}
            return subprocess.run(
                [str(PREWARM), str(binary)],
                cwd=ROOT,
                env=environment,
                text=True,
                capture_output=True,
                check=False,
            )

    def test_success_reaches_the_outer_shell_boundary(self) -> None:
        result = self.run_fixture(0)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_nonzero_failure_reaches_the_outer_shell_boundary(self) -> None:
        result = self.run_fixture(1)
        self.assertEqual(result.returncode, 1, result.stderr)

    def test_timeout_status_reaches_the_outer_shell_boundary(self) -> None:
        # A fixture exit of 124 exercises timeout's public exit contract
        # without waiting for the production 60-second safety bound.
        result = self.run_fixture(124)
        self.assertEqual(result.returncode, 124, result.stderr)


if __name__ == "__main__":
    unittest.main()
