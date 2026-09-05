import json
import unittest
from pathlib import Path

from scripts import native_shadow_mcp_real_trace_gate as gate


class NativeShadowMcpRealTraceGateTests(unittest.TestCase):
    @staticmethod
    def adjudication(verdict: str, redelivered: bool) -> dict:
        accepted = verdict == "accepted"
        return {
            "schema": "boole.native-shadow.adjudication.v1",
            "outcome": "accepted" if accepted else "deterministic_reject",
            "reasonCode": "accepted" if accepted else "checker_rejected",
            "redelivered": redelivered,
            "evidenceDigest": "a" * 64,
            "receipt": {
                "taskId": "1" * 64,
                "submissionId": "2" * 64,
                "artifactRoot": "3" * 64,
                "checkerHash": "4" * 64,
                "verdict": verdict,
                "rejectReason": (
                    None if accepted else "compile-or-hidden-test-failed"
                ),
            },
        }

    def test_accept_and_reject_survive_mcp_restart_as_the_same_terminal_result(self) -> None:
        for verdict in ("accepted", "rejected"):
            first = self.adjudication(verdict, False)
            replay = self.adjudication(verdict, True)
            gate.validate_terminal_pair(verdict, first, replay)

            replay["evidenceDigest"] = "b" * 64
            with self.assertRaisesRegex(ValueError, "terminal result drifted"):
                gate.validate_terminal_pair(verdict, first, replay)

    def test_three_checker_terminals_and_one_precheck_are_not_extended_by_replay(self) -> None:
        events = []
        for epoch, verdict, reason in (
            (0, "accepted", "accepted"),
            (1, "deterministic_reject", "checker_rejected"),
            (2, "deterministic_reject", "checker_rejected"),
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
        snapshot = "\n".join(json.dumps(event, sort_keys=True) for event in events) + "\n"
        gate.validate_journal_snapshot(snapshot)
        gate.require_journal_unchanged(snapshot, snapshot)

        with self.assertRaisesRegex(ValueError, "journal changed"):
            gate.require_journal_unchanged(snapshot, snapshot + "{}\n")

    def test_legacy_node_counter_must_remain_zero(self) -> None:
        gate.require_no_legacy_node_contact(0)
        with self.assertRaisesRegex(ValueError, "legacy node"):
            gate.require_no_legacy_node_contact(1)

    def test_both_named_linux_authorities_supply_the_same_four_case_shape(self) -> None:
        root = Path(__file__).resolve().parents[1]
        fixtures = root / "fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history"
        for grant_name in (
            "native-shadow-closed-local-replay-grant-v1.json",
            "native-shadow-closed-local-replay-grant-arm64-v1.json",
        ):
            cases = gate._load_inputs(
                root / "native/containment" / grant_name,
                fixtures,
            )
            self.assertEqual(
                [case_id for case_id, _ in cases],
                ["accepted", "tampered", "constant", "empty"],
            )
            self.assertTrue(all(len(arguments) == 6 for _, arguments in cases))


if __name__ == "__main__":
    unittest.main()
