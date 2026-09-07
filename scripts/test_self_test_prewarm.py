#!/usr/bin/env python3
"""Behavioural contract for self-test's prewarm exit-status boundary."""
from __future__ import annotations

import os
import stat
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PREWARM = ROOT / "scripts" / "self-test-prewarm.sh"


class SelfTestPrewarmTests(unittest.TestCase):
    def test_success_needs_no_gnu_timeout_in_path(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp)
            (path / "bash").symlink_to("/bin/bash")
            (path / "python3").symlink_to(sys.executable)
            result = subprocess.run(
                ["/bin/bash", str(PREWARM), "/bin/echo"],
                env=os.environ | {"PATH": temp},
                capture_output=True, text=True, timeout=5,
            )
            self.assertEqual(result.returncode, 0, result.stderr)

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

    def test_real_timeout_kills_the_owned_process_tree(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            binary = Path(temp) / "sleeping fixture"
            marker = Path(temp) / "escaped-child"
            child = f"import pathlib,time; time.sleep(1); pathlib.Path({str(marker)!r}).touch()"
            binary.write_text(
                f"#!{sys.executable}\nimport subprocess,sys,time\n"
                f"subprocess.Popen([sys.executable, '-c', {child!r}])\ntime.sleep(10)\n"
            )
            binary.chmod(0o700)
            started = time.monotonic()
            result = subprocess.run(
                ["/bin/bash", str(PREWARM), str(binary)],
                env=os.environ | {"BOOLE_PREWARM_TIMEOUT_SECONDS": "0.2"},
                capture_output=True, text=True, timeout=3,
            )
            self.assertEqual(result.returncode, 124, result.stderr)
            self.assertLess(time.monotonic() - started, 2)
            time.sleep(1.1)
            self.assertFalse(marker.exists(), "timeout left a child alive")

    def test_invalid_timeout_cannot_disable_the_safety_bound(self) -> None:
        for value in ["0", "-1", "nan", "inf", "oops"]:
            with self.subTest(value=value):
                result = subprocess.run(
                    ["/bin/bash", str(PREWARM), "/bin/echo"],
                    env=os.environ | {"BOOLE_PREWARM_TIMEOUT_SECONDS": value},
                    capture_output=True, text=True, timeout=3,
                )
                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertIn("finite and positive", result.stderr)


if __name__ == "__main__":
    unittest.main()
