import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from scripts import native_shadow_canary_gate as gate


class FreshAnswerCanaryGateTests(unittest.TestCase):
    def test_cross_task_probe_does_not_resubmit_the_same_historical_task(self):
        first = {"familyVersion": "family", "templateId": "template", "challengeSha256": "challenge", "epoch": 10, "rawAnswer": "first"}
        current = dict(first, epoch=11, rawAnswer="second")
        self.assertIsNone(gate.cross_task_probe(first, current))
        self.assertIsNone(gate.cross_task_probe(None, current))
        current["challengeSha256"] = "different-challenge"
        probe = gate.cross_task_probe(first, current)
        self.assertEqual(probe, dict(first, epoch=11))
        self.assertEqual(first["epoch"], 10)

    def test_development_answers_follow_each_new_task_without_fixture_answers(self):
        task = {"scaffold": "prefix\n    todo!()\nsuffix\n", "constants": {"a0": 17, "mul": 3, "coeffs": [2, -5]}}
        answer = gate.development_answer(task, "accepted")
        self.assertIn("let mut acc = 17i64", answer)
        self.assertIn("wrapping_mul(3i64)", answer)
        self.assertIn("(item.1 as i64).wrapping_mul(-5i64)", answer)
        self.assertNotIn("todo!", answer)
        self.assertIn("prefix", answer)
        other = dict(task, constants={"a0": -2, "mul": 7, "coeffs": [3]})
        self.assertNotEqual(answer, gate.development_answer(other, "accepted"))
        self.assertIn("0i64", gate.development_answer(task, "rejected"))
        self.assertNotEqual(answer, gate.development_answer(task, "tampered"))

    def test_synthetic_answers_change_both_digests_without_changing_the_scaffold(self):
        root = Path(__file__).resolve().parents[1]
        for case in ("accepted", "constant"):
            raw = (root / "fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history" / ("replay-" + case + ".raw.txt")).read_text()
            fresh = gate.synthetic_answer(raw, case)
            source = raw.split("```rust", 1)[1].split("```", 1)[0].strip()
            fresh_source = fresh.split("```rust", 1)[1].split("```", 1)[0].strip()
            self.assertNotEqual(hashlib.sha256(raw.encode()).digest(), hashlib.sha256(fresh.encode()).digest())
            self.assertNotEqual(hashlib.sha256(source.encode()).digest(), hashlib.sha256(fresh_source.encode()).digest())
            self.assertEqual(source.split("// <<< ACFR-PATCH-BEGIN >>>")[0], fresh_source.split("// <<< ACFR-PATCH-BEGIN >>>")[0])
            self.assertEqual(source.split("// <<< ACFR-PATCH-END >>>")[1], fresh_source.split("// <<< ACFR-PATCH-END >>>")[1])

    def test_budget_gate_rejects_second_execution_or_wrong_private_journal(self):
        grant = {"journalId": "2" * 64}
        rows = [
            {"event": "open", "journal_id": "2" * 64, "role": "node"},
            {"event": "candidate"}, {"event": "execute"}, {"event": "redeliver"},
        ]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "budget.jsonl"
            path.write_text("\n".join(json.dumps(row) for row in rows) + "\n")
            gate.require_budget(path, grant, "node", 1, 1)
            rows.append({"event": "execute"})
            path.write_text("\n".join(json.dumps(row) for row in rows) + "\n")
            with self.assertRaisesRegex(ValueError, "sequence"):
                gate.require_budget(path, grant, "node", 1, 1)
            path.write_text("\n".join(json.dumps(row) for row in rows[:-1]) + "\n")
            with self.assertRaisesRegex(ValueError, "binding"):
                gate.require_budget(path, {"journalId": "3" * 64}, "node", 1, 1)


if __name__ == "__main__":
    unittest.main()
