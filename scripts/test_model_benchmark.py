#!/usr/bin/env python3
"""Regression tests for model benchmark artifact skeleton."""
from __future__ import annotations

import importlib.util
import json
import os
import subprocess
import tempfile
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
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
            self.assertEqual(summary["totals"]["generatedAttempts"], 0)
            self.assertEqual(summary["totals"]["blockProductionRatePct"], 0.0)
            self.assertEqual(summary["publicScore"]["primaryMetric"], "blockProductionRatePct")
            self.assertEqual(summary["diagnostics"]["verifiedShares"], 2)
            self.assertEqual(summary["safety"], {"invalidAccepted": 0, "chainDivergence": 0, "replayFailures": 0})
            self.assertTrue(summary["replayPassed"])
            self.assertEqual(rows[0]["name"], "mock-local-model")
            self.assertEqual(rows[0]["status"], "PASS")
            self.assertEqual(rows[0]["score"], {"blocksProduced": 1, "replayPass": True})
            self.assertEqual(rows[0]["diagnostics"]["verifiedShares"], 2)
            self.assertEqual(rows[0]["metadata"]["model"], "mock-v0")
            self.assertTrue(replay["replayPassed"])
            self.assertIn("# Boole Model Proof-to-Block Benchmark", leaderboard)
            self.assertIn("mock-local-model", leaderboard)
            self.assertIn("blockProductionRate", leaderboard)
            self.assertIn("blocksProduced", leaderboard)
            self.assertNotIn("verifiedShares", leaderboard)
            self.assertNotIn("accepted:", leaderboard)

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
                "print('True.intro')\n",
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
            self.assertEqual(summary["totals"]["blocksProduced"], 0)
            self.assertEqual(summary["totals"]["blockProductionRatePct"], 0.0)
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
                "print('True.intro')\n",
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
                    "--benchmark-mode",
                    "smoke",
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

            self.assertTrue(Path(invocation["proof"]).is_absolute())
            self.assertTrue(Path(invocation["blockStore"]).is_absolute())
            self.assertTrue(Path(invocation["proof"]).exists())
            self.assertTrue(summary["ok"])
            self.assertEqual(summary["diagnostics"]["accepted"], 1)
            self.assertEqual(summary["diagnostics"]["verifiedShares"], 1)
            self.assertEqual(summary["totals"]["blocksProduced"], 1)
            self.assertEqual(summary["totals"]["generatedAttempts"], 1)
            self.assertEqual(summary["totals"]["blockProductionRatePct"], 100.0)
            self.assertEqual(
                summary["attemptHierarchy"],
                {
                    "generatedAttempts": 1,
                    "verifierAccepted": 1,
                    "verifiedShares": 1,
                    "blocksProduced": 1,
                },
            )
            self.assertEqual(summary["safety"]["invalidAccepted"], 0)
            self.assertEqual(rows[0]["status"], "ACCEPTED")
            self.assertTrue(rows[0]["accepted"])
            self.assertTrue(rows[0]["verifier"]["invoked"])
            self.assertEqual(rows[0]["verifier"]["command"], "submit-lean")
            self.assertEqual(rows[0]["verifier"]["exitCode"], 0)
            self.assertEqual(rows[0]["score"], {"blocksProduced": 1, "replayPass": True})
            self.assertEqual(rows[0]["diagnostics"], {"verifiedShares": 1})
            self.assertEqual(
                rows[0]["miningPath"],
                {
                    "targetIssued": True,
                    "modelGenerated": True,
                    "candidateWrapped": True,
                    "submitLeanInvoked": True,
                    "verifierAccepted": True,
                    "canonicalPackageSubmitted": True,
                    "shareAccepted": True,
                    "blockProduced": True,
                    "replayPassed": True,
                },
            )
            self.assertIn("theorem boole_benchmark_true : True", invocation["proofText"])
            self.assertIn("True.intro", invocation["proofText"])
            self.assertEqual(rows[0]["candidateMode"], "proof-term")
            self.assertEqual(rows[0]["candidateExtraction"]["format"], "raw")
            self.assertEqual(invocation["verifierHash"], "boole-model-benchmark-ollama-v0")
            self.assertRegex(invocation["requiredCheckerArtifactHash"], r"^[0-9a-f]{64}$")

    def test_generated_attempt_can_be_submitted_through_local_node_http_path(self) -> None:
        requests: list[dict] = []

        class Handler(BaseHTTPRequestHandler):
            def do_POST(self) -> None:  # noqa: N802 - stdlib handler hook
                length = int(self.headers.get("content-length", "0"))
                body = json.loads(self.rfile.read(length).decode("utf-8"))
                requests.append({"path": self.path, "body": body})
                response = {
                    "ok": True,
                    "accepted": True,
                    # Real boole-node /submit returns shareHash/block evidence, not an explicit
                    # shareAccepted/blockProduced boolean pair.
                    "block": {"height": 7, "selectedShares": 1},
                    "replayMatchesRuntime": True,
                    "invalidAccepted": 0,
                    "runtimeHead": "node-head",
                    "replayLatestC": "node-head",
                    "shareHash": "ab" * 32,
                }
                payload = json.dumps(response).encode("utf-8")
                self.send_response(200)
                self.send_header("content-type", "application/json")
                self.send_header("content-length", str(len(payload)))
                self.end_headers()
                self.wfile.write(payload)

            def log_message(self, _format: str, *_args: object) -> None:
                return

        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            with tempfile.TemporaryDirectory() as tmp:
                tmp_path = Path(tmp)
                fake_ollama = tmp_path / "fake-ollama.py"
                fake_ollama.write_text("#!/usr/bin/env python3\nprint('True.intro')\n", encoding="utf-8")
                fake_ollama.chmod(0o755)
                fake_submit = tmp_path / "fake-submit-lean.py"
                fake_submit.write_text(
                    "#!/usr/bin/env python3\n"
                    "import json\n"
                    "print(json.dumps({'ok': True, 'command': 'submit-lean', 'accepted': True, 'shareAccepted': True, 'replayMatchesRuntime': True, 'invalidAccepted': 0, 'canonTag': 0, 'submissionBody': {'c': '00'*32, 'pk': '11'*32, 'n': '1', 'j': '0', 'nonceS': '2', 'bytes': '504f4650'}, 'block': None}))\n",
                    encoding="utf-8",
                )
                fake_submit.chmod(0o755)
                out_dir = tmp_path / "model-benchmark"
                node_url = f"http://127.0.0.1:{server.server_port}"

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
                        "--node-url",
                        node_url,
                        "--attempts",
                        "1",
                        "--output-dir",
                        str(out_dir),
                        "--run-id",
                        "node-path-run",
                        "--benchmark-mode",
                        "smoke",
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
                self.assertEqual(len(requests), 1)
                self.assertEqual(requests[0]["path"], "/submit")
                self.assertEqual(requests[0]["body"]["canonTag"], 0)
                self.assertEqual(requests[0]["body"]["body"]["bytes"], "504f4650")
                self.assertEqual(summary["totals"]["blocksProduced"], 1)
                self.assertEqual(rows[0]["verifier"]["nodeHttp"]["url"], node_url)
                self.assertTrue(rows[0]["verifier"]["nodeHttp"]["invoked"])
                self.assertTrue(rows[0]["verifier"]["nodeHttp"]["accepted"])
                self.assertEqual(rows[0]["score"], {"blocksProduced": 1, "replayPass": True})
                self.assertTrue(rows[0]["miningPath"]["blockProduced"])
        finally:
            server.shutdown()
            server.server_close()

    def test_node_ticket_mode_requests_ticket_before_submit_and_records_evidence(self) -> None:
        requests: list[dict[str, object]] = []

        class Handler(BaseHTTPRequestHandler):
            def do_POST(self) -> None:  # noqa: N802 - http.server API
                body = json.loads(self.rfile.read(int(self.headers.get("content-length", "0"))))
                requests.append({"path": self.path, "body": body})
                if self.path == "/ticket":
                    response = {"ok": True, "hashHex": "cd" * 32, "valid": True}
                elif self.path == "/submit":
                    response = {
                        "ok": True,
                        "accepted": True,
                        "block": {"height": 0, "selectedShares": 1},
                        "replayMatchesRuntime": True,
                        "invalidAccepted": 0,
                        "shareHash": "ef" * 32,
                    }
                else:
                    self.send_response(404)
                    self.end_headers()
                    return
                payload = json.dumps(response).encode("utf-8")
                self.send_response(200)
                self.send_header("content-type", "application/json")
                self.send_header("content-length", str(len(payload)))
                self.end_headers()
                self.wfile.write(payload)

            def log_message(self, _format: str, *_args: object) -> None:
                return

        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            with tempfile.TemporaryDirectory() as tmp:
                tmp_path = Path(tmp)
                fake_ollama = tmp_path / "fake-ollama.py"
                fake_ollama.write_text("#!/usr/bin/env python3\nprint('True.intro')\n", encoding="utf-8")
                fake_ollama.chmod(0o755)
                fake_submit = tmp_path / "fake-submit-lean.py"
                fake_submit.write_text(
                    "#!/usr/bin/env python3\n"
                    "import json\n"
                    "print(json.dumps({'ok': True, 'command': 'submit-lean', 'accepted': True, 'shareAccepted': True, 'replayMatchesRuntime': True, 'invalidAccepted': 0, 'canonTag': 0, 'submissionBody': {'c': '00'*32, 'pk': '11'*32, 'n': '1', 'j': '0', 'nonceS': '2', 'bytes': '504f4650'}, 'block': None}))\n",
                    encoding="utf-8",
                )
                fake_submit.chmod(0o755)
                out_dir = tmp_path / "model-benchmark"
                node_url = f"http://127.0.0.1:{server.server_port}"

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
                        "--node-url",
                        node_url,
                        "--use-node-ticket",
                        "--attempts",
                        "1",
                        "--output-dir",
                        str(out_dir),
                        "--run-id",
                        "node-ticket-path-run",
                        "--benchmark-mode",
                        "smoke",
                    ],
                    cwd=ROOT,
                    text=True,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    check=False,
                )

                self.assertEqual(proc.returncode, 0, proc.stderr + proc.stdout)
                rows = [json.loads(line) for line in (out_dir / "benchmark-rows.ndjson").read_text().splitlines()]
                self.assertEqual([request["path"] for request in requests], ["/ticket", "/submit"])
                self.assertEqual(requests[0]["body"]["payload"]["c"], "00" * 32)
                self.assertEqual(requests[0]["body"]["payload"]["pk"], "11" * 32)
                self.assertEqual(requests[0]["body"]["payload"]["n"], "1")
                node_http = rows[0]["verifier"]["nodeHttp"]
                self.assertTrue(node_http["ticketInvoked"])
                self.assertTrue(node_http["ticket"]["valid"])
                self.assertEqual(node_http["ticket"]["hashHex"], "cd" * 32)
                self.assertTrue(rows[0]["miningPath"]["targetIssued"])
                self.assertEqual(rows[0]["score"], {"blocksProduced": 1, "replayPass": True})
        finally:
            server.shutdown()
            server.server_close()

    def test_default_model_benchmark_uses_mining_target_not_true_smoke(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            fake_ollama = tmp_path / "fake-ollama.py"
            fake_ollama.write_text(
                "#!/usr/bin/env python3\n"
                "print('rfl')\n",
                encoding="utf-8",
            )
            fake_ollama.chmod(0o755)
            verifier_log = tmp_path / "verifier-invocations.ndjson"
            fake_submit = tmp_path / "fake-submit-lean.py"
            fake_submit.write_text(
                "#!/usr/bin/env python3\n"
                "import json, pathlib, sys\n"
                "args = sys.argv[1:]\n"
                "proof = pathlib.Path(args[args.index('--proof') + 1])\n"
                "text = proof.read_text()\n"
                f"with pathlib.Path({str(verifier_log)!r}).open('a') as f:\n"
                "    f.write(json.dumps({'proofText': text}) + '\\n')\n"
                "print(json.dumps({'ok': True, 'command': 'submit-lean', 'accepted': False, 'shareAccepted': False, 'replayMatchesRuntime': True, 'invalidAccepted': 0}))\n",
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
                    "default-mining-target-run",
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
            proof_text = json.loads(verifier_log.read_text().splitlines()[0])["proofText"]
            self.assertEqual(summary["benchmarkMode"], "mining")
            self.assertEqual(summary["targetFamily"], "boole.calibration.pow.v1")
            self.assertEqual(rows[0]["benchmarkMode"], "mining")
            self.assertEqual(rows[0]["targetFamily"], "boole.calibration.pow.v1")
            self.assertNotIn("boole_benchmark_true", proof_text)
            self.assertNotIn(": True", proof_text)
            self.assertIn("boole_benchmark_pow_target", proof_text)

    def test_multi_attempt_mining_benchmark_binds_unique_lottery_samples(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            fake_ollama = tmp_path / "fake-ollama.py"
            fake_ollama.write_text(
                "#!/usr/bin/env python3\n"
                "print('rfl')\n",
                encoding="utf-8",
            )
            fake_ollama.chmod(0o755)
            verifier_log = tmp_path / "verifier-invocations.ndjson"
            fake_submit = tmp_path / "fake-submit-lean.py"
            fake_submit.write_text(
                "#!/usr/bin/env python3\n"
                "import hashlib, json, pathlib, sys\n"
                "args = sys.argv[1:]\n"
                "proof = pathlib.Path(args[args.index('--proof') + 1])\n"
                "text = proof.read_text()\n"
                "digest = hashlib.sha256(text.encode()).hexdigest()\n"
                f"with pathlib.Path({str(verifier_log)!r}).open('a') as f:\n"
                "    f.write(json.dumps({'proofText': text, 'digest': digest}) + '\\n')\n"
                "print(json.dumps({'ok': True, 'command': 'submit-lean', 'accepted': True, 'shareAccepted': True, 'replayMatchesRuntime': True, 'invalidAccepted': 0, 'shareHash': digest, 'block': None}))\n",
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
                    "3",
                    "--output-dir",
                    str(out_dir),
                    "--run-id",
                    "unique-lottery-run",
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
            invocations = [json.loads(line) for line in verifier_log.read_text().splitlines()]
            self.assertEqual(summary["diagnostics"]["uniqueCandidates"], 3)
            self.assertEqual(summary["diagnostics"]["uniqueShares"], 3)
            self.assertEqual(summary["diagnostics"]["uniqueShareRatePct"], 100.0)
            self.assertEqual(len({row["lotterySample"]["challenge"] for row in rows}), 3)
            self.assertEqual(len({row["lotterySample"]["nonce"] for row in rows}), 3)
            self.assertEqual(len({row["candidateSha256"] for row in rows}), 3)
            self.assertEqual(len({item["digest"] for item in invocations}), 3)

    def test_explicit_smoke_mode_is_segregated_from_public_mining_target(self) -> None:
        benchmark = load_benchmark()
        prompt = benchmark.model_proof_term_prompt(benchmark_mode="smoke", attempt_context=benchmark.attempt_context("smoke-run", "ollama:test", 0, benchmark_mode="smoke"))
        candidate = benchmark.wrap_proof_term_candidate("True.intro", benchmark_mode="smoke", attempt_context=benchmark.attempt_context("smoke-run", "ollama:test", 0, benchmark_mode="smoke"))
        self.assertIn("boole_benchmark_true", prompt)
        self.assertIn("Valid example response: `True.intro`", prompt)
        self.assertIn("theorem boole_benchmark_true : True", candidate)

    def test_full_theorem_output_is_rejected_before_verifier_in_proof_term_mode(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            fake_ollama = tmp_path / "fake-ollama.py"
            fake_ollama.write_text(
                "#!/usr/bin/env python3\n"
                "print('```lean')\n"
                "print('theorem arbitrary : True := by trivial')\n"
                "print('```')\n",
                encoding="utf-8",
            )
            fake_ollama.chmod(0o755)
            fake_submit = tmp_path / "fake-submit-lean.py"
            fake_submit.write_text(
                "#!/usr/bin/env python3\n"
                "raise SystemExit('verifier must not be invoked for wrong candidate shape')\n",
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
                    "full-theorem-rejected-run",
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
            self.assertEqual(summary["totals"]["rejected"], 1)
            self.assertEqual(summary["totals"]["generatedAttempts"], 0)
            self.assertEqual(rows[0]["status"], "REJECTED")
            self.assertEqual(rows[0]["reason"], "candidate-shape-invalid")
            self.assertFalse(rows[0]["generatedAttempt"])
            self.assertFalse(rows[0]["verifier"]["invoked"])
            self.assertEqual(rows[0]["candidateExtraction"]["format"], "fenced-lean")

    def test_submit_lean_receives_absolute_paths_for_relative_output_dir(self) -> None:
        with tempfile.TemporaryDirectory(dir=ROOT) as tmp:
            tmp_path = Path(tmp)
            fake_ollama = tmp_path / "fake-ollama.py"
            fake_ollama.write_text(
                "#!/usr/bin/env python3\n"
                "print('True.intro')\n",
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
                "checker_dir = pathlib.Path(args[args.index('--checker-dir') + 1])\n"
                f"pathlib.Path({str(verifier_log)!r}).write_text(json.dumps({{'proof': str(proof), 'blockStore': str(block_store), 'checkerDir': str(checker_dir)}}))\n"
                "if not proof.is_absolute() or not block_store.is_absolute() or not checker_dir.is_absolute():\n"
                "    print(json.dumps({'ok': False, 'accepted': False, 'error': 'relative-path'}))\n"
                "    raise SystemExit(1)\n"
                "print(json.dumps({'ok': True, 'command': 'submit-lean', 'accepted': True, 'shareAccepted': True, 'replayMatchesRuntime': True, 'invalidAccepted': 0, 'block': {'height': 0, 'selectedShares': 1}}))\n",
                encoding="utf-8",
            )
            fake_submit.chmod(0o755)
            relative_out = Path(tmp_path.name) / "relative-model-benchmark"

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
                    str(relative_out),
                    "--run-id",
                    "relative-submit-path-run",
                ],
                cwd=ROOT,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )

            self.assertEqual(proc.returncode, 0, proc.stderr + proc.stdout)
            invocation = json.loads(verifier_log.read_text())
            self.assertTrue(Path(invocation["proof"]).is_absolute())
            self.assertTrue(Path(invocation["blockStore"]).is_absolute())
            self.assertTrue(Path(invocation["checkerDir"]).is_absolute())

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
            self.assertEqual(rows[0]["score"], {"blocksProduced": 0, "replayPass": True})
            self.assertEqual(rows[0]["diagnostics"]["verifiedShares"], 0)
            self.assertEqual(rows[0]["safety"], {"invalidAccepted": 0, "chainDivergence": 0, "replayFailures": 0})

    def test_ollama_target_streams_rows_before_all_attempts_finish(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            counter = tmp_path / "count.txt"
            rows_path = tmp_path / "model-benchmark" / "benchmark-rows.ndjson"
            progress_path = tmp_path / "model-benchmark" / "progress.json"
            fake_ollama = tmp_path / "fake-stream-check-ollama.py"
            fake_ollama.write_text(
                "#!/usr/bin/env python3\n"
                "import os, pathlib, sys\n"
                f"counter = pathlib.Path({str(counter)!r})\n"
                "count = int(counter.read_text()) if counter.exists() else 0\n"
                "count += 1\n"
                "counter.write_text(str(count))\n"
                "rows = pathlib.Path(os.environ['BOOLE_TEST_ROWS_PATH'])\n"
                "progress = pathlib.Path(os.environ['BOOLE_TEST_PROGRESS_PATH'])\n"
                "if count == 2 and (not rows.exists() or len(rows.read_text().splitlines()) != 1 or not progress.exists()):\n"
                "    print('missing streaming checkpoint after first attempt', file=sys.stderr)\n"
                "    raise SystemExit(42)\n"
                "print('True.intro')\n",
                encoding="utf-8",
            )
            fake_ollama.chmod(0o755)
            out_dir = tmp_path / "model-benchmark"
            env = os.environ.copy()
            env["BOOLE_TEST_ROWS_PATH"] = str(rows_path)
            env["BOOLE_TEST_PROGRESS_PATH"] = str(progress_path)

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
                    "stream-run",
                ],
                cwd=ROOT,
                env=env,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=30,
            )

            self.assertEqual(proc.returncode, 0, proc.stderr)
            rows = [json.loads(line) for line in rows_path.read_text().splitlines()]
            progress = json.loads(progress_path.read_text())
            self.assertEqual(len(rows), 2)
            self.assertEqual(progress["completedAttempts"], 2)
            self.assertEqual(progress["totalAttempts"], 2)
            self.assertEqual(progress["totals"]["rejected"], 2)

    def test_claude_cli_target_records_generated_attempt_rows(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            fake_claude = tmp_path / "fake-claude.py"
            invocation_log = tmp_path / "claude-invocation.json"
            fake_claude.write_text(
                "#!/usr/bin/env python3\n"
                "import json, pathlib, sys\n"
                f"pathlib.Path({str(invocation_log)!r}).write_text(json.dumps(sys.argv[1:]))\n"
                "print('True.intro')\n",
                encoding="utf-8",
            )
            fake_claude.chmod(0o755)
            out_dir = tmp_path / "model-benchmark"

            proc = subprocess.run(
                [
                    "python3",
                    str(BENCHMARK_PATH),
                    "--target",
                    "claude-cli:sonnet",
                    "--claude-command",
                    str(fake_claude),
                    "--attempts",
                    "2",
                    "--output-dir",
                    str(out_dir),
                    "--run-id",
                    "claude-cli-run",
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
            invocation = json.loads(invocation_log.read_text())

            self.assertTrue(summary["ok"])
            self.assertEqual(summary["totals"]["rows"], 2)
            self.assertEqual(summary["totals"]["generatedAttempts"], 2)
            self.assertEqual(summary["totals"]["blocksProduced"], 0)
            self.assertEqual(summary["totals"]["blockProductionRatePct"], 0.0)
            self.assertEqual(summary["totals"]["rejected"], 2)
            self.assertEqual(summary["safety"]["invalidAccepted"], 0)
            self.assertEqual({row["provider"] for row in rows}, {"claude-cli"})
            self.assertEqual({row["model"] for row in rows}, {"sonnet"})
            self.assertTrue(all(row["generatedAttempt"] is True for row in rows))
            self.assertTrue(all(row["status"] == "REJECTED" for row in rows))
            self.assertTrue(all(row["accepted"] is False for row in rows))
            self.assertTrue(all(row["candidateSha256"] for row in rows))
            self.assertIn("-p", invocation)
            self.assertIn("--model", invocation)
            self.assertIn("sonnet", invocation)

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
