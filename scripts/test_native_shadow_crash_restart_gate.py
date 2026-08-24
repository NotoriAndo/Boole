"""Contract tests for the closed-local native-shadow crash/restart gate.

The crash/restart driver kills the real production replay node and launcher
processes at durable journal points on Linux CI.  These tests pin the pure
validation logic plus the kill-safety and wiring contracts so the gate cannot
silently regress into name-based kills, skipped scenarios, or a relaxed
redelivery comparison.
"""

import json
import unittest
from pathlib import Path

from scripts import native_shadow_crash_restart_gate as gate


REPO_ROOT = Path(__file__).resolve().parents[1]
DRIVER_PATH = REPO_ROOT / "scripts/native_shadow_crash_restart_gate.py"
MANAGER_GATE_PATH = REPO_ROOT / "scripts/native-shadow-manager-cgroup-gate.sh"
PORTABLE_GATE_PATH = REPO_ROOT / "scripts/native-shadow-portable-rootfs-replay-linux.sh"


def accepted_body(redelivered: bool) -> dict:
    return {
        "schema": "boole.native-shadow.adjudication.v1",
        "outcome": "accepted",
        "reasonCode": "accepted",
        "redelivered": redelivered,
        "evidenceDigest": "a" * 64,
        "receipt": {
            "taskId": "1" * 64,
            "submissionId": "2" * 64,
            "artifactRoot": "3" * 64,
            "checkerHash": "4" * 64,
            "verdict": "accepted",
            "rejectReason": None,
        },
    }


def tampered_body(redelivered: bool) -> dict:
    body = accepted_body(redelivered)
    body["outcome"] = "deterministic_reject"
    body["reasonCode"] = "checker_rejected"
    body["receipt"]["verdict"] = "rejected"
    body["receipt"]["rejectReason"] = "compile-or-hidden-test-failed"
    return body


class CrashRestartResponseContractTests(unittest.TestCase):
    def test_redelivered_flag_is_validated_in_both_directions(self) -> None:
        gate.validate_adjudication_response(
            "accepted", 200, accepted_body(False), expect_redelivered=False
        )
        gate.validate_adjudication_response(
            "accepted", 200, accepted_body(True), expect_redelivered=True
        )
        gate.validate_adjudication_response(
            "tampered", 200, tampered_body(True), expect_redelivered=True
        )
        with self.assertRaises(ValueError):
            gate.validate_adjudication_response(
                "accepted", 200, accepted_body(False), expect_redelivered=True
            )
        with self.assertRaises(ValueError):
            gate.validate_adjudication_response(
                "tampered", 200, tampered_body(True), expect_redelivered=False
            )

    def test_tampered_redelivery_never_becomes_accept(self) -> None:
        drifted = tampered_body(True)
        drifted["outcome"] = "accepted"
        drifted["reasonCode"] = "accepted"
        with self.assertRaises(ValueError):
            gate.validate_adjudication_response(
                "tampered", 200, drifted, expect_redelivered=True
            )

    def test_byte_parity_accepts_only_the_redelivered_flag_flip(self) -> None:
        first = json.dumps(
            {"outcome": "deterministic_reject", "redelivered": False, "evidenceDigest": "a" * 64},
            separators=(",", ":"),
        ).encode("utf-8")
        redelivered = first.replace(b'"redelivered":false', b'"redelivered":true')
        gate.validate_redelivered_byte_parity(first, redelivered)
        with self.assertRaises(ValueError):
            gate.validate_redelivered_byte_parity(first, first)
        mutated = redelivered.replace(b"a" * 64, b"b" * 64)
        with self.assertRaises(ValueError):
            gate.validate_redelivered_byte_parity(first, mutated)
        with self.assertRaises(ValueError):
            gate.validate_redelivered_byte_parity(
                first.replace(b'"redelivered":false', b'"redelivered":true'),
                first,
            )

    def test_identical_redelivered_bodies_must_match_exactly(self) -> None:
        body = json.dumps(
            {"outcome": "accepted", "redelivered": True}, separators=(",", ":")
        ).encode("utf-8")
        gate.validate_identical_redelivery(body, bytes(body))
        with self.assertRaises(ValueError):
            gate.validate_identical_redelivery(body, body + b" ")


class CrashRestartJournalContractTests(unittest.TestCase):
    @staticmethod
    def rows(kinds_with_epochs) -> str:
        return "\n".join(
            json.dumps({"kind": kind, "epoch": epoch}) for kind, epoch in kinds_with_epochs
        )

    def test_two_case_terminal_journal_shape_is_exact(self) -> None:
        exact = []
        for epoch in (0, 1):
            exact.extend(
                (kind, epoch)
                for kind in (
                    "grant_attempt_reserved_v1",
                    "bootstrap_v2",
                    "in_flight_v3",
                    "evidence_v2",
                    "terminal_consumed_v2",
                )
            )
        gate.require_two_case_terminal_journal(gate.journal_kinds(self.rows(exact)))
        with self.assertRaises(ValueError):
            gate.require_two_case_terminal_journal(
                gate.journal_kinds(self.rows(exact + [("in_flight_v3", 0)]))
            )
        with self.assertRaises(ValueError):
            gate.require_two_case_terminal_journal(
                gate.journal_kinds(self.rows(exact[:-1]))
            )

    def test_unresolved_in_flight_journal_shape_is_exact(self) -> None:
        stuck = [
            ("grant_attempt_reserved_v1", 0),
            ("bootstrap_v2", 0),
            ("in_flight_v3", 0),
        ]
        gate.require_unresolved_in_flight_journal(gate.journal_kinds(self.rows(stuck)))
        with self.assertRaises(ValueError):
            gate.require_unresolved_in_flight_journal(
                gate.journal_kinds(self.rows(stuck + [("evidence_v2", 0)]))
            )
        with self.assertRaises(ValueError):
            gate.require_unresolved_in_flight_journal(
                gate.journal_kinds(self.rows(stuck[:-1]))
            )
        with self.assertRaises(ValueError):
            gate.require_unresolved_in_flight_journal([])


class CrashRestartKillSafetyContractTests(unittest.TestCase):
    def test_signals_flow_through_one_verified_identity_call_site(self) -> None:
        driver = DRIVER_PATH.read_text(encoding="utf-8")
        self.assertIn("def verified_unit_main_pid", driver)
        self.assertIn("def deliver_verified_signal", driver)
        self.assertIn("cgroup.procs", driver)
        self.assertIn("/proc/", driver)
        # A stale MainPID must never be signalled: the one os.kill call site
        # re-verifies pid, start time, and unit cgroup membership first.
        self.assertEqual(driver.count("os.kill("), 1)
        call_site = driver.index("os.kill(")
        verify_site = driver.index("def deliver_verified_signal")
        self.assertLess(verify_site, call_site)
        for forbidden in ("pkill", "killall", "kill -9", "kill -KILL"):
            self.assertNotIn(forbidden, driver)

    def test_fail_closed_message_and_markers_are_pinned(self) -> None:
        self.assertEqual(
            gate.UNRESOLVED_IN_FLIGHT_MESSAGE,
            "native-shadow replay route remains closed while durable InFlight "
            "rows are unresolved",
        )
        driver = DRIVER_PATH.read_text(encoding="utf-8")
        for marker in (
            "native-shadow-crash-restart-case:terminal-redelivery-across-node-kill:PASS",
            "native-shadow-crash-restart-case:unresolved-inflight-fail-closed:PASS",
            "native-shadow-crash-restart-evidence:",
            "native-shadow-crash-restart-gate:PASS",
        ):
            self.assertIn(marker, driver)


class CrashRestartGateWiringTests(unittest.TestCase):
    def test_manager_gate_runs_crash_phase_after_http_matrix(self) -> None:
        manager = MANAGER_GATE_PATH.read_text(encoding="utf-8")
        self.assertIn('sudo python3 "$crash_gate_source"', manager)
        for marker in (
            "native-shadow-crash-restart-case:terminal-redelivery-across-node-kill:PASS",
            "native-shadow-crash-restart-case:unresolved-inflight-fail-closed:PASS",
            "native-shadow-crash-restart-gate:PASS",
            "native-shadow production crash/restart replay gate: PASS",
        ):
            self.assertIn(marker, manager)
        self.assertLess(
            manager.index("native-shadow production HTTP replay gate: PASS"),
            manager.index("run_crash_restart_replay_gate() {"),
        )
        self.assertRegex(
            manager,
            r"run_closed_local_replay_gate\n\s*run_crash_restart_replay_gate\n\s*exit 0",
        )

    def test_portable_gate_budget_covers_crash_scenarios(self) -> None:
        portable = PORTABLE_GATE_PATH.read_text(encoding="utf-8")
        self.assertIn("1200s", portable)
        self.assertNotIn(" 600s", portable)


if __name__ == "__main__":
    unittest.main()
