import unittest
import json
from pathlib import Path

from scripts import native_shadow_http_replay_gate as gate


class NativeShadowHttpReplayGateTests(unittest.TestCase):
    @staticmethod
    def receipt(verdict: str) -> dict:
        return {
            "taskId": "1" * 64,
            "submissionId": "2" * 64,
            "artifactRoot": "3" * 64,
            "checkerHash": "4" * 64,
            "verdict": verdict,
            "rejectReason": (
                None if verdict == "accepted" else "compile-or-hidden-test-failed"
            ),
        }

    def test_exact_four_case_response_contract(self) -> None:
        gate.validate_case_response(
            "accepted",
            200,
            {
                "schema": "boole.native-shadow.adjudication.v1",
                "outcome": "accepted",
                "reasonCode": "accepted",
                "redelivered": False,
                "evidenceDigest": "a" * 64,
                "receipt": self.receipt("accepted"),
            },
        )
        for case_id in ("tampered", "constant"):
            gate.validate_case_response(
                case_id,
                200,
                {
                    "schema": "boole.native-shadow.adjudication.v1",
                    "outcome": "deterministic_reject",
                    "reasonCode": "compile_or_hidden_test_failed",
                    "redelivered": False,
                    "evidenceDigest": "b" * 64,
                    "receipt": self.receipt("rejected"),
                },
            )
        gate.validate_case_response(
            "empty",
            400,
            {
                "schema": "boole.native-shadow.adjudication-error.v1",
                "outcome": "precheck_reject",
                "reasonCode": "intake_rejected",
            },
        )

    def test_response_contract_rejects_softened_or_retried_results(self) -> None:
        invalid = [
            (
                "accepted",
                200,
                {
                    "schema": "boole.native-shadow.adjudication.v1",
                    "outcome": "deterministic_reject",
                    "reasonCode": "compile_or_hidden_test_failed",
                    "redelivered": False,
                    "evidenceDigest": "a" * 64,
                    "receipt": self.receipt("rejected"),
                },
            ),
            (
                "tampered",
                503,
                {
                    "schema": "boole.native-shadow.adjudication-error.v1",
                    "outcome": "retryable_unavailable",
                    "reasonCode": "containment_environment_unavailable",
                },
            ),
            (
                "empty",
                200,
                {
                    "schema": "boole.native-shadow.adjudication.v1",
                    "outcome": "accepted",
                    "reasonCode": "accepted",
                    "redelivered": False,
                    "evidenceDigest": "a" * 64,
                    "receipt": self.receipt("accepted"),
                },
            ),
        ]
        for case_id, status, body in invalid:
            with self.subTest(case_id=case_id, status=status):
                with self.assertRaisesRegex(ValueError, "HTTP replay matrix"):
                    gate.validate_case_response(case_id, status, body)

    def test_journal_contract_is_four_attempts_and_three_terminal_checker_rows(self) -> None:
        events = []
        for epoch, verdict, reason in (
            (0, "accepted", "accepted"),
            (1, "deterministic_reject", "compile_or_hidden_test_failed"),
            (2, "deterministic_reject", "compile_or_hidden_test_failed"),
        ):
            events.extend(
                [
                    {
                        "kind": "grant_attempt_reserved_v1",
                        "epoch": epoch,
                        "attemptKind": "checker",
                    },
                    {"kind": "bootstrap_v2", "epoch": epoch},
                    {"kind": "in_flight_v3", "epoch": epoch},
                    {
                        "kind": "evidence_v2",
                        "epoch": epoch,
                        "evidenceJson": json.dumps(
                            {"verdict": verdict, "reasonCode": reason}
                        ),
                    },
                    {"kind": "terminal_consumed_v2", "epoch": epoch, "exhausted": True},
                ]
            )
        events.append(
            {
                "kind": "grant_attempt_reserved_v1",
                "epoch": 3,
                "attemptKind": "pre_intake",
            }
        )
        gate.validate_journal_events(events)

        events[-1]["attemptKind"] = "checker"
        with self.assertRaisesRegex(ValueError, "journal"):
            gate.validate_journal_events(events)

    def test_root_owned_install_can_rebind_every_input_path(self) -> None:
        arguments = gate.parse_args(
            [
                "--grant-path",
                "/usr/libexec/boole/native-shadow-http-replay-grant-v1.json",
                "--fixture-directory",
                "/usr/libexec/boole/native-shadow-http-replay-fixtures",
                "--journal-path",
                "/var/lib/boole/native-shadow/node-state/replay-v1.ndjson",
            ]
        )
        self.assertEqual(
            arguments.grant_path,
            Path("/usr/libexec/boole/native-shadow-http-replay-grant-v1.json"),
        )
        self.assertEqual(
            arguments.fixture_directory,
            Path("/usr/libexec/boole/native-shadow-http-replay-fixtures"),
        )
        self.assertEqual(arguments.journal_path, gate.JOURNAL_PATH)


if __name__ == "__main__":
    unittest.main()
