#!/usr/bin/env python3
"""Behavior tests for the reversible lane's bounded snapshot retry wrapper."""

from __future__ import annotations

import pathlib
import subprocess
import sys
import tempfile
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
WRAPPER = ROOT / "scripts/native_shadow_bounded_snapshot_retry_v1.py"
RETRYABLE = "snapshot response status is not 200"
CI = ROOT / ".github/workflows/ci.yml"


class BoundedSnapshotRetryTests(unittest.TestCase):
    def run_wrapper(self, program: str, state: pathlib.Path) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [
                sys.executable,
                "-I",
                "-S",
                str(WRAPPER),
                "--delay-seconds",
                "0",
                "--",
                sys.executable,
                "-I",
                "-S",
                "-c",
                program,
                str(state),
            ],
            cwd=ROOT,
            text=True,
            capture_output=True,
            check=False,
        )

    def test_two_transient_snapshot_failures_then_success(self) -> None:
        program = (
            "import pathlib,sys; p=pathlib.Path(sys.argv[1]); "
            "n=int(p.read_text()) if p.exists() else 0; p.write_text(str(n+1)); "
            f"print({RETRYABLE!r}, file=sys.stderr) if n < 2 else print('ok'); "
            "raise SystemExit(1 if n < 2 else 0)"
        )
        with tempfile.TemporaryDirectory() as scratch:
            state = pathlib.Path(scratch) / "attempts"
            completed = self.run_wrapper(program, state)
            self.assertEqual(completed.returncode, 0, completed.stderr)
            self.assertEqual(state.read_text(), "3")
            self.assertIn("bounded snapshot retry 2/3", completed.stderr)
            self.assertIn("bounded snapshot retry 3/3", completed.stderr)

    def test_non_snapshot_failure_is_never_retried(self) -> None:
        program = (
            "import pathlib,sys; p=pathlib.Path(sys.argv[1]); "
            "n=int(p.read_text()) if p.exists() else 0; p.write_text(str(n+1)); "
            "print('frozen digest differs', file=sys.stderr); raise SystemExit(7)"
        )
        with tempfile.TemporaryDirectory() as scratch:
            state = pathlib.Path(scratch) / "attempts"
            completed = self.run_wrapper(program, state)
            self.assertEqual(completed.returncode, 7)
            self.assertEqual(state.read_text(), "1")
            self.assertNotIn("bounded snapshot retry", completed.stderr)

    def test_three_snapshot_failures_preserve_the_final_failure(self) -> None:
        program = (
            "import pathlib,sys; p=pathlib.Path(sys.argv[1]); "
            "n=int(p.read_text()) if p.exists() else 0; p.write_text(str(n+1)); "
            f"print({RETRYABLE!r}, file=sys.stderr); raise SystemExit(9)"
        )
        with tempfile.TemporaryDirectory() as scratch:
            state = pathlib.Path(scratch) / "attempts"
            completed = self.run_wrapper(program, state)
            self.assertEqual(completed.returncode, 9)
            self.assertEqual(state.read_text(), "3")
            self.assertEqual(completed.stderr.count(RETRYABLE), 3)

    def test_observed_snapshot_502_is_retryable_but_404_is_not(self) -> None:
        retryable_program = (
            "import pathlib,sys; p=pathlib.Path(sys.argv[1]); "
            "n=int(p.read_text()) if p.exists() else 0; p.write_text(str(n+1)); "
            "print('urllib.error.HTTPError: HTTP Error 502: Bad Gateway', file=sys.stderr); "
            "raise SystemExit(1)"
        )
        permanent_program = retryable_program.replace("502: Bad Gateway", "404: Not Found")
        with tempfile.TemporaryDirectory() as scratch:
            base = pathlib.Path(scratch)
            retryable = self.run_wrapper(retryable_program, base / "retryable")
            permanent = self.run_wrapper(permanent_program, base / "permanent")
            self.assertEqual(retryable.returncode, 1)
            self.assertEqual((base / "retryable").read_text(), "3")
            self.assertEqual(permanent.returncode, 1)
            self.assertEqual((base / "permanent").read_text(), "1")

    def test_rootfs_harness_generic_snapshot_fetch_failure_is_retryable(self) -> None:
        program = (
            "import pathlib,sys; p=pathlib.Path(sys.argv[1]); "
            "n=int(p.read_text()) if p.exists() else 0; p.write_text(str(n+1)); "
            "print('native-shadow rootfs acquisition failed: artifact fetch failed: "
            "https://snapshot.ubuntu.com/ubuntu/frozen.deb', file=sys.stderr); "
            "raise SystemExit(2)"
        )
        with tempfile.TemporaryDirectory() as scratch:
            state = pathlib.Path(scratch) / "attempts"
            completed = self.run_wrapper(program, state)
            self.assertEqual(completed.returncode, 2)
            self.assertEqual(state.read_text(), "3")

    def test_required_ci_uses_the_same_bound_for_three_snapshot_consumers(self) -> None:
        text = CI.read_text(encoding="utf-8")
        wrapper = "native_shadow_bounded_snapshot_retry_v1.py"
        self.assertEqual(text.count(wrapper), 3)
        self.assertIn(
            f"{wrapper} -- sudo ./scripts/native-shadow-portable-rootfs-replay-linux.sh",
            text,
        )
        self.assertIn(
            f"{wrapper} -- sudo ./scripts/native-shadow-portable-rootfs-replay-linux-arm64.sh",
            text,
        )
        self.assertIn(
            f"{wrapper} -- python3 scripts/native_shadow_boot_ci_payload_acquire_arm64_v1.py acquire",
            text,
        )

    def test_empty_command_is_refused(self) -> None:
        completed = subprocess.run(
            [sys.executable, "-I", "-S", str(WRAPPER), "--delay-seconds", "0", "--"],
            cwd=ROOT,
            text=True,
            capture_output=True,
            check=False,
        )
        self.assertNotEqual(completed.returncode, 0)


if __name__ == "__main__":
    unittest.main()
