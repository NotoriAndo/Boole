from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from scripts import native_shadow_installed_mac_crash_restart_e2e_v1 as crash


ROOT = Path(__file__).resolve().parents[1]


class InstalledMacCrashRestartE2ETests(unittest.TestCase):
    def test_script_can_be_invoked_directly_from_the_repository(self) -> None:
        completed = subprocess.run(
            [
                sys.executable,
                str(ROOT / "scripts/native_shadow_installed_mac_crash_restart_e2e_v1.py"),
                "--help",
            ],
            cwd=ROOT,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            check=False,
        )
        self.assertEqual(completed.returncode, 0, completed.stderr)

    def test_terminal_redelivery_allows_only_the_one_flag_delta(self) -> None:
        first = b'{"outcome":"deterministic_reject","redelivered":false,"receipt":"same"}'
        second = b'{"outcome":"deterministic_reject","redelivered":true,"receipt":"same"}'
        crash.validate_flag_only_redelivery(first, second)
        with self.assertRaisesRegex(ValueError, "beyond its flag"):
            crash.validate_flag_only_redelivery(first, second.replace(b"same", b"drift"))

    def test_journal_contracts_distinguish_terminal_from_unresolved(self) -> None:
        def row(kind: str, epoch: int) -> dict[str, object]:
            return {"kind": kind, "epoch": epoch}

        terminal = [
            row(kind, epoch)
            for epoch in (0, 1)
            for kind in (
                "grant_attempt_reserved_v1",
                "bootstrap_v2",
                "in_flight_v3",
                "evidence_v2",
                "terminal_consumed_v2",
            )
        ]
        crash.require_terminal_journal(terminal)
        unresolved = terminal[:3]
        crash.require_unresolved_journal(unresolved)
        with self.assertRaisesRegex(ValueError, "terminal journal drifted"):
            crash.require_terminal_journal(unresolved)

    def test_controller_child_requires_one_direct_member_of_the_node_group(self) -> None:
        runtime = Path("/private/tmp/fixed-runtime")
        expected = str(runtime / "active-controller" / "host-controller")
        table = [
            (42, 1, 42, "/unrelated"),
            (101, 100, 100, expected + " --controller-stdio"),
            (102, 100, 999, expected + " --wrong-group"),
        ]
        with mock.patch.object(crash, "process_table", return_value=table):
            self.assertEqual(crash.controller_child(100, runtime), 101)

    def test_process_and_boundary_contracts_are_fail_closed(self) -> None:
        source = (ROOT / "scripts/native_shadow_installed_mac_crash_restart_e2e_v1.py").read_text(
            encoding="utf-8"
        )
        self.assertIn("os.killpg(process.pid, signal.SIGKILL)", source)
        self.assertNotIn("pkill", source)
        self.assertGreaterEqual(source.count("finally:\n"), 5)
        self.assertIn("cleanup_process_group(second)", source)
        self.assertIn("cleanup_process_group(first)", source)
        self.assertIn("CONTROLLER_EXECUTION_MARKER", source)
        self.assertIn("UNRESOLVED_IN_FLIGHT_MESSAGE", source)
        for boundary in (
            '"production": False',
            '"testnet": False',
            '"mining": False',
            '"reward": False',
            '"consensus": False',
            '"p2p": False',
            '"activationAllowed": False',
        ):
            self.assertIn(boundary, source)

    def test_cleanup_targets_only_the_owned_process_group(self) -> None:
        process = mock.Mock(pid=2345)
        process.wait.return_value = -9
        with (
            mock.patch.object(crash.os, "killpg") as killpg,
            mock.patch.object(crash, "process_table", return_value=[]),
        ):
            crash.cleanup_process_group(process)
        killpg.assert_called_once_with(2345, crash.signal.SIGKILL)
        process.wait.assert_called_once_with(timeout=10)

    def test_result_writer_is_create_then_atomic_replace(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            target = Path(temporary) / "result.json"
            crash._write_json_atomic(target, {"ok": True})
            self.assertEqual(json.loads(target.read_text(encoding="utf-8")), {"ok": True})
            self.assertFalse(target.with_name("result.json.tmp").exists())

    def test_host_controller_requires_the_inherited_lifetime_lease_and_audits_execution(self) -> None:
        swift = (ROOT / "native/mac4/boole-mac4-auth-channel.swift").read_text(
            encoding="utf-8"
        )
        rust = (
            ROOT / "crates/boole-node/src/native_shadow_mac4_controller.rs"
        ).read_text(encoding="utf-8")
        self.assertIn("--controller-stdio requires the inherited runtime lease", swift)
        self.assertIn(crash.CONTROLLER_EXECUTION_MARKER, swift)
        self.assertIn('CONTROLLER_RUNTIME_LEASE_BASENAME: &str = ".controller-runtime.lock"', rust)
        self.assertIn('.arg("--runtime-lease-fd")', rust)
        self.assertIn("recover_stale_controller_runtime(runtime_root, &lease)?", rust)


if __name__ == "__main__":
    unittest.main()
