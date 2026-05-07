#!/usr/bin/env python3
"""Regression tests for model benchmark artifact skeleton."""
from __future__ import annotations

import importlib.util
import json
import os
import subprocess
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

    def test_ollama_target_records_generated_attempt_rows(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            fake_ollama = tmp_path / "fake-ollama.py"
            fake_ollama.write_text(
                "#!/usr/bin/env python3\n"
                "import sys\n"
                "print('theorem boole_benchmark_true : True := by trivial')\n",
                encoding="utf-8",
            )
            fake_ollama.chmod(0o755)
            out_dir = tmp_path / "model-benchmark"

            proc = subprocess.run(
                [
                    "python3",
                    str(BENCHMARK_PATH),
                    "--target",
                    "ollama:qwen2.5-coder:7b",
                    "--ollama-command",
                    str(fake_ollama),
                    "--attempts",
                    "2",
                    "--output-dir",
                    str(out_dir),
                    "--run-id",
                    "ollama-run",
                ],
                cwd=ROOT,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )

            self.assertEqual(proc.returncode, 0, proc.stderr + proc.stdout)
            summary = json.loads((out_dir / "benchmark-summary.json").read_text())
            rows = [json.loads(line) for line in (out_dir / "benchmark-rows.ndjson").read_text().splitlines()]
            leaderboard = (out_dir / "leaderboard.md").read_text()

            self.assertTrue(summary["ok"])
            self.assertEqual(summary["totals"]["rows"], 2)
            self.assertEqual(summary["totals"]["generatedAttempts"], 2)
            self.assertEqual(summary["totals"]["rejected"], 2)
            self.assertEqual(summary["safety"]["invalidAccepted"], 0)
            self.assertEqual({row["provider"] for row in rows}, {"ollama"})
            self.assertEqual({row["model"] for row in rows}, {"qwen2.5-coder:7b"})
            self.assertTrue(all(row["generatedAttempt"] is True for row in rows))
            self.assertTrue(all(row["status"] == "REJECTED" for row in rows))
            self.assertTrue(all(row["accepted"] is False for row in rows))
            self.assertTrue(all(row["invalidAccepted"] is False for row in rows))
            self.assertTrue(all(row["candidateSha256"] for row in rows))
            self.assertIn("ollama:qwen2.5-coder:7b", leaderboard)
            self.assertIn("Local model-generated proof attempts", leaderboard)

    def test_missing_ollama_command_records_setup_required_without_failing_run(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            out_dir = tmp_path / "model-benchmark"
            missing_command = tmp_path / "missing-ollama"

            proc = subprocess.run(
                [
                    "python3",
                    str(BENCHMARK_PATH),
                    "--target",
                    "ollama:qwen2.5-coder:7b",
                    "--ollama-command",
                    str(missing_command),
                    "--attempts",
                    "1",
                    "--output-dir",
                    str(out_dir),
                    "--run-id",
                    "missing-ollama-run",
                ],
                cwd=ROOT,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )

            self.assertEqual(proc.returncode, 0, proc.stderr + proc.stdout)
            summary = json.loads((out_dir / "benchmark-summary.json").read_text())
            rows = [json.loads(line) for line in (out_dir / "benchmark-rows.ndjson").read_text().splitlines()]

            self.assertTrue(summary["ok"])
            self.assertEqual(summary["totals"]["setupRequired"], 1)
            self.assertEqual(summary["totals"]["generatedAttempts"], 0)
            self.assertEqual(summary["safety"]["invalidAccepted"], 0)
            self.assertEqual(rows[0]["status"], "SETUP_REQUIRED")
            self.assertEqual(rows[0]["reason"], "ollama-command-not-found")
            self.assertFalse(rows[0]["generatedAttempt"])
            self.assertFalse(rows[0]["invalidAccepted"])

    def test_ollama_generated_candidate_is_submitted_to_verifier_path(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            fake_ollama = tmp_path / "fake-ollama.py"
            fake_ollama.write_text(
                "#!/usr/bin/env python3\n"
                "print('theorem boole_benchmark_true : True := by trivial')\n",
                encoding="utf-8",
            )
            fake_ollama.chmod(0o755)
            verifier_log = tmp_path / "verifier-invocation.json"
            fake_submit = tmp_path / "fake-submit-lean.py"
            fake_submit.write_text(
                "#!/usr/bin/env python3\n"
                "import json, pathlib, sys\n"
                "args = sys.argv[1:]\n"
                "proof = pathlib.Path(args[args.index('--proof') + 1])\n"
                "block_store = pathlib.Path(args[args.index('--block-store') + 1])\n"
                "verifier_hash = args[args.index('--verifier-hash') + 1]\n"
                "required_hash = args[args.index('--require-checker-artifact-hash') + 1]\n"
                "block_store.write_text('{\\\"height\\\":0}\\n')\n"
                f"pathlib.Path({str(verifier_log)!r}).write_text(json.dumps({{'proof': str(proof), 'proofText': proof.read_text(), 'blockStore': str(block_store), 'verifierHash': verifier_hash, 'requiredCheckerArtifactHash': required_hash}}))\n"
                "print(json.dumps({'ok': True, 'command': 'submit-lean', 'accepted': True, 'shareAccepted': True, 'replayMatchesRuntime': True, 'invalidAccepted': 0, 'block': {'height': 0, 'selectedShares': 1}, 'replayHeight': 1, 'runtimeHead': '0xabc', 'replayLatestC': '0xabc', 'blockStorePath': str(block_store), 'lean': {'accepted': True, 'verifier_hash': verifier_hash}}))\n",
                encoding="utf-8",
            )
            fake_submit.chmod(0o755)
            out_dir = tmp_path / "model-benchmark"

            proc = subprocess.run(
                [
                    "python3",
                    str(BENCHMARK_PATH),
                    "--target",
                    "ollama:qwen2.5-coder:7b",
                    "--ollama-command",
                    str(fake_ollama),
                    "--submit-lean-command",
                    str(fake_submit),
                    "--attempts",
                    "1",
                    "--output-dir",
                    str(out_dir),
                    "--run-id",
                    "submit-candidate-run",
                ],
                cwd=ROOT,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )

            self.assertEqual(proc.returncode, 0, proc.stderr + proc.stdout)
            summary = json.loads((out_dir / "benchmark-summary.json").read_text())
            rows = [json.loads(line) for line in (out_dir / "benchmark-rows.ndjson").read_text().splitlines()]
            invocation = json.loads(verifier_log.read_text())

            self.assertTrue(summary["ok"])
            self.assertEqual(summary["totals"]["accepted"], 1)
            self.assertEqual(summary["totals"]["verifiedShares"], 1)
            self.assertEqual(summary["totals"]["blocksProduced"], 1)
            self.assertEqual(summary["safety"]["invalidAccepted"], 0)
            self.assertEqual(rows[0]["status"], "ACCEPTED")
            self.assertTrue(rows[0]["accepted"])
            self.assertTrue(rows[0]["verifier"]["invoked"])
            self.assertEqual(rows[0]["verifier"]["command"], "submit-lean")
            self.assertEqual(rows[0]["verifier"]["exitCode"], 0)
            self.assertEqual(rows[0]["score"], {"blocks": 1, "verifiedShares": 1, "replayPass": True})
            self.assertIn("theorem boole_benchmark_true", invocation["proofText"])
            self.assertEqual(invocation["verifierHash"], "boole-model-benchmark-ollama-v0")
            self.assertRegex(invocation["requiredCheckerArtifactHash"], r"^[0-9a-f]{64}$")

    def test_ollama_timeout_records_rejected_row_without_crashing(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            fake_ollama = tmp_path / "fake-slow-ollama.py"
            fake_ollama.write_text(
                "#!/usr/bin/env python3\n"
                "import time\n"
                "time.sleep(2)\n",
                encoding="utf-8",
            )
            fake_ollama.chmod(0o755)
            out_dir = tmp_path / "model-benchmark"

            proc = subprocess.run(
                [
                    "python3",
                    str(BENCHMARK_PATH),
                    "--target",
                    "ollama:slow-model",
                    "--ollama-command",
                    str(fake_ollama),
                    "--attempts",
                    "1",
                    "--timeout-sec",
                    "1",
                    "--output-dir",
                    str(out_dir),
                    "--run-id",
                    "timeout-run",
                ],
                cwd=ROOT,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=10,
            )

            self.assertEqual(proc.returncode, 0, proc.stderr)
            summary = json.loads((out_dir / "benchmark-summary.json").read_text())
            rows = [json.loads(line) for line in (out_dir / "benchmark-rows.ndjson").read_text().splitlines()]
            self.assertTrue(summary["ok"])
            self.assertEqual(summary["totals"]["rejected"], 1)
            self.assertEqual(rows[0]["status"], "REJECTED")
            self.assertEqual(rows[0]["reason"], "ollama-timeout")
            self.assertEqual(rows[0]["score"], {"blocks": 0, "verifiedShares": 0, "replayPass": True})
            self.assertEqual(rows[0]["safety"], {"invalidAccepted": 0, "chainDivergence": 0, "replayFailures": 0})

    def test_missing_ollama_model_records_setup_required_without_auto_pull(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            fake_ollama = tmp_path / "fake-ollama.py"
            fake_ollama.write_text(
                "#!/usr/bin/env python3\n"
                "import sys\n"
                "sys.stderr.write(\"model 'qwen2.5-coder:7b' not found, try pulling it first\\n\")\n"
                "raise SystemExit(1)\n",
                encoding="utf-8",
            )
            fake_ollama.chmod(0o755)
            out_dir = tmp_path / "model-benchmark"

            proc = subprocess.run(
                [
                    "python3",
                    str(BENCHMARK_PATH),
                    "--target",
                    "ollama:qwen2.5-coder:7b",
                    "--ollama-command",
                    str(fake_ollama),
                    "--attempts",
                    "1",
                    "--output-dir",
                    str(out_dir),
                    "--run-id",
                    "missing-model-run",
                ],
                cwd=ROOT,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )

            self.assertEqual(proc.returncode, 0, proc.stderr + proc.stdout)
            summary = json.loads((out_dir / "benchmark-summary.json").read_text())
            rows = [json.loads(line) for line in (out_dir / "benchmark-rows.ndjson").read_text().splitlines()]

            self.assertTrue(summary["ok"])
            self.assertEqual(summary["totals"]["setupRequired"], 1)
            self.assertEqual(summary["safety"]["invalidAccepted"], 0)
            self.assertEqual(rows[0]["status"], "SETUP_REQUIRED")
            self.assertEqual(rows[0]["reason"], "ollama-model-missing")
            self.assertFalse(rows[0]["generatedAttempt"])
            self.assertNotIn("ollama pull", rows[0].get("stdoutTail", ""))


if __name__ == "__main__":
    unittest.main()
