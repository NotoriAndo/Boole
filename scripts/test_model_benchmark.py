#!/usr/bin/env python3
"""Regression tests for model benchmark artifact skeleton."""
from __future__ import annotations

import importlib.util
import json
import os
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BENCHMARK_PATH = ROOT / "scripts" / "boole-model-benchmark.py"


def load_benchmark():
    spec = importlib.util.spec_from_file_location("boole_model_benchmark", BENCHMARK_PATH)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class ModelBenchmarkArtifactTests(unittest.TestCase):
    def test_runner_writes_summary_rows_leaderboard_and_replay_report(self) -> None:
        benchmark = load_benchmark()
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            spec_path = tmp_path / "spec.json"
            out_dir = tmp_path / "model-benchmark"
            spec = [
                {
                    "name": "mock-local-model",
                    "kind": "provider-model",
                    "metadata": {"provider": "mock", "backend": "mock", "model": "mock-v0"},
                    "command": [
                        "python3",
                        "-c",
                        "import json; print(json.dumps({'ok': True, 'summary': {'verifyAccepted': 2, 'blocksProduced': 1}, 'safety': {'invalidAccepted': 0, 'chainDivergence': 0, 'replayFailures': 0}, 'replayMatchesRuntime': True}))",
                    ],
                }
            ]
            spec_path.write_text(json.dumps(spec), encoding="utf-8")

            result = benchmark.run_benchmark(spec_path=spec_path, output_dir=out_dir, run_id="test-run")

            self.assertTrue(result["ok"])
            summary = json.loads((out_dir / "benchmark-summary.json").read_text())
            rows = [(json.loads(line)) for line in (out_dir / "benchmark-rows.ndjson").read_text().splitlines()]
            replay = json.loads((out_dir / "replay-report.json").read_text())
            leaderboard = (out_dir / "leaderboard.md").read_text()

            self.assertEqual(summary["benchmark"], "boole-model-proof-to-block")
            self.assertEqual(summary["runId"], "test-run")
            self.assertEqual(summary["artifacts"]["rows"], "benchmark-rows.ndjson")
            self.assertEqual(summary["totals"]["rows"], 1)
            self.assertEqual(summary["totals"]["passed"], 1)
            self.assertEqual(summary["totals"]["blocksProduced"], 1)
            self.assertEqual(summary["totals"]["verifiedShares"], 2)
            self.assertEqual(summary["safety"], {"invalidAccepted": 0, "chainDivergence": 0, "replayFailures": 0})
            self.assertTrue(summary["replayPassed"])
            self.assertEqual(rows[0]["name"], "mock-local-model")
            self.assertEqual(rows[0]["status"], "PASS")
            self.assertEqual(rows[0]["score"], {"blocks": 1, "verifiedShares": 2, "replayPass": True})
            self.assertEqual(rows[0]["metadata"]["model"], "mock-v0")
            self.assertTrue(replay["replayPassed"])
            self.assertIn("# Boole Model Proof-to-Block Benchmark", leaderboard)
            self.assertIn("mock-local-model", leaderboard)
            self.assertIn("verifiedShares", leaderboard)

    def test_missing_required_env_is_recorded_as_skip_without_running_command(self) -> None:
        benchmark = load_benchmark()
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            spec_path = tmp_path / "spec.json"
            out_dir = tmp_path / "model-benchmark"
            spec = [
                {
                    "name": "frontier-row",
                    "kind": "provider-model",
                    "metadata": {"provider": "openai", "model": "gpt-5", "credential": "OPENAI_API_KEY"},
                    "requireEnv": ["BOOLE_TEST_MISSING_API_KEY"],
                    "command": ["python3", "-c", "raise SystemExit('must not run')"],
                }
            ]
            spec_path.write_text(json.dumps(spec), encoding="utf-8")
            old = os.environ.pop("BOOLE_TEST_MISSING_API_KEY", None)
            try:
                result = benchmark.run_benchmark(spec_path=spec_path, output_dir=out_dir, run_id="skip-run")
            finally:
                if old is not None:
                    os.environ["BOOLE_TEST_MISSING_API_KEY"] = old

            self.assertTrue(result["ok"])
            summary = json.loads((out_dir / "benchmark-summary.json").read_text())
            rows = [(json.loads(line)) for line in (out_dir / "benchmark-rows.ndjson").read_text().splitlines()]
            self.assertEqual(summary["totals"]["skipped"], 1)
            self.assertEqual(rows[0]["status"], "SKIP")
            self.assertEqual(rows[0]["reason"], "missing_required_env")
            self.assertEqual(rows[0]["missingEnv"], ["BOOLE_TEST_MISSING_API_KEY"])
            self.assertNotIn("sk-", json.dumps(summary) + json.dumps(rows))


if __name__ == "__main__":
    unittest.main()
