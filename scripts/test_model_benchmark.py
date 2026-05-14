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
    def test_closed_mining_productivity_v1_scenario_uses_calibrated_difficulty_without_rate_limits(self) -> None:
        scenario_path = ROOT / "fixtures" / "benchmarks" / "closed-mining-productivity-v1" / "scenario.json"
        scenario = json.loads(scenario_path.read_text())
        config_fixture = json.loads((ROOT / "fixtures" / "protocol" / "config" / "v1.json").read_text())
        valid_report = next(case["report"] for case in config_fixture["cases"] if case["name"] == "valid")

        self.assertEqual(scenario["version"], 1)
        self.assertEqual(scenario["source"]["claimBoundary"], "closed local mining productivity benchmark; not public-network mining")
        self.assertEqual(scenario["source"]["difficultySource"], "fixtures/protocol/config/v1.json#cases[name=valid]")
        self.assertEqual(scenario["source"]["intendedMinerProfile"], "v1-lenbound")
        self.assertEqual(scenario["source"]["targetEmitter"], "FamilyV1LengthBoundTargetEmitter")
        self.assertTrue(scenario["source"]["requiresBooleMinerStartProfile"])
        self.assertFalse(scenario["source"]["forBooleModelBenchmarkDefaultMining"])
        self.assertNotIn("runtime-smoke", scenario["source"].get("difficultySource", ""))
        self.assertEqual(scenario["genesisC"], "0000000000000000000000000000000000000000000000000000000000000000")
        self.assertEqual(scenario["steps"], [])

        cfg = scenario["cfg"]
        for key in ["T_submit", "T_share", "T_block", "T_ticket"]:
            self.assertEqual(cfg[key], valid_report[key])
        for key in ["MinShareScoreMultiplier", "L", "D_max", "EMAWindow", "M"]:
            self.assertEqual(cfg[key], valid_report[key])
        self.assertEqual(cfg["K_max"], 100000)
        self.assertEqual(cfg["SharePoolGlobalCap"], 100000)
        self.assertEqual(cfg["ShareCapPerPK_Block"], 100000)
        self.assertEqual(cfg["perIpRateLimitPer60s"], 100000)
        self.assertEqual(cfg["provenance"], "closed-mining-productivity-v1")

    def test_controlled_model_mining_fixture_freezes_public_safe_schema(self) -> None:
        fixture = json.loads((ROOT / "fixtures" / "benchmarks" / "controlled-model-mining" / "v1-summary.json").read_text())
        self.assertEqual(fixture["benchmark"], "controlled-local-model-mining-v1")
        self.assertEqual(fixture["claimBoundary"], "controlled local mining benchmark, not public-network mining")
        self.assertFalse(fixture["publicMiningEvidence"])
        self.assertEqual(fixture["primaryMetric"], "blocksProduced")
        self.assertEqual(fixture["attemptHierarchy"], [
            "generatedAttempts",
            "proofIntakeAccepted",
            "verifierAccepted",
            "verifiedShares",
            "blocksProduced",
            "replayPassed",
        ])
        models = fixture["models"]
        self.assertEqual(
            {model["model"] for model in models},
            {"ollama:gemma4:26b", "claude-cli:claude-sonnet-4-6", "claude-cli:claude-opus-4-7"},
        )
        for model in models:
            self.assertIn("generatedAttempts", model)
            self.assertIn("proofIntakeAccepted", model)
            self.assertIn("verifierAccepted", model)
            self.assertIn("verifiedShares", model)
            self.assertIn("blocksProduced", model)
            self.assertIn("replayFailures", model)
            self.assertIn("invalidAccepted", model)
            self.assertIn("timeouts", model)
            self.assertLessEqual(model["blocksProduced"], model["verifiedShares"])
            self.assertLessEqual(model["verifiedShares"], model["verifierAccepted"])
            self.assertLessEqual(model["verifierAccepted"], model["proofIntakeAccepted"])
            self.assertLessEqual(model["proofIntakeAccepted"], model["generatedAttempts"])
        self.assertEqual(fixture["totals"]["invalidAccepted"], 0)
        self.assertEqual(fixture["totals"]["replayFailures"], 0)

    def test_self_test_smoke_artifacts_freeze_public_claim_boundary(self) -> None:
        proof_script = (ROOT / "scripts" / "proof-to-block-benchmark.sh").read_text(encoding="utf-8")
        mining_script = (ROOT / "scripts" / "local-mining-smoke.sh").read_text(encoding="utf-8")
        self_test = (ROOT / "scripts" / "self-test.sh").read_text(encoding="utf-8")

        self.assertIn('"claimBoundary": "closed local/fixture validation; not public-network mining"', proof_script)
        self.assertIn('"publicMiningEvidence": False', proof_script)
        self.assertIn('"publicScoringEligible": False', proof_script)
        self.assertIn('"ineligibilityReasons": [', proof_script)

        self.assertIn('"claimBoundary": "controlled local smoke; not public-network mining"', mining_script)
        self.assertIn('"publicMiningEvidence": False', mining_script)
        self.assertIn('"publicScoringEligible": False', mining_script)
        self.assertIn('"ineligibilityReasons": [', mining_script)

        self.assertIn('"claimBoundary": benchmark.get("claimBoundary")', self_test)
        self.assertIn('"publicMiningEvidence": benchmark.get("publicMiningEvidence")', self_test)
        self.assertIn('"claimBoundary": mining.get("claimBoundary")', self_test)
        self.assertIn('"publicMiningEvidence": mining.get("publicMiningEvidence")', self_test)

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
            # B4: the mock command surfaces `replayMatchesRuntime: True`, so
            # `score_from_result` records `replayInvoked: True` alongside
            # `replayPass: True`.
            self.assertEqual(rows[0]["score"], {"blocksProduced": 1, "replayPass": True, "replayInvoked": True})
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
                    # B4: one row exercised submit-lean, so the attempt
                    # hierarchy reports one row that invoked replay.
                    "replayInvoked": 1,
                },
            )
            self.assertEqual(summary["safety"]["invalidAccepted"], 0)
            self.assertEqual(rows[0]["status"], "ACCEPTED")
            self.assertTrue(rows[0]["accepted"])
            self.assertTrue(rows[0]["verifier"]["invoked"])
            self.assertEqual(rows[0]["verifier"]["command"], "submit-lean")
            self.assertEqual(rows[0]["verifier"]["exitCode"], 0)
            self.assertEqual(rows[0]["score"], {"blocksProduced": 1, "replayPass": True, "replayInvoked": True})
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
            verifier_hash_fixture = json.loads(
                (ROOT / "fixtures/benchmarks/verifier-hashes.json").read_text(encoding="utf-8")
            )
            expected_verifier_hash = verifier_hash_fixture["versions"][verifier_hash_fixture["active"]]
            self.assertEqual(invocation["verifierHash"], expected_verifier_hash)
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
                self.assertEqual(rows[0]["score"], {"blocksProduced": 1, "replayPass": True, "replayInvoked": True})
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
                # The /ticket request must mirror pof TicketBody = {c, pk, n} exactly:
                # no payload wrapper, no submit-shaped extras (j, nonceS, bytes, ...). Boole's
                # tightened /ticket contract rejects extra fields with HTTP 400 unexpected_field,
                # so a regression here would 400 every live --use-node-ticket benchmark run.
                ticket_body = requests[0]["body"]
                self.assertEqual(set(ticket_body.keys()), {"c", "pk", "n"})
                self.assertEqual(ticket_body["c"], "00" * 32)
                self.assertEqual(ticket_body["pk"], "11" * 32)
                self.assertEqual(ticket_body["n"], "1")
                node_http = rows[0]["verifier"]["nodeHttp"]
                self.assertTrue(node_http["ticketInvoked"])
                self.assertTrue(node_http["ticket"]["valid"])
                self.assertEqual(node_http["ticket"]["hashHex"], "cd" * 32)
                self.assertTrue(rows[0]["miningPath"]["targetIssued"])
                self.assertEqual(rows[0]["score"], {"blocksProduced": 1, "replayPass": True, "replayInvoked": True})
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
            # Timeout rows never reach the verifier — replayInvoked is False
            # so they cannot vacuously pass the summary aggregation.
            self.assertEqual(rows[0]["score"], {"blocksProduced": 0, "replayPass": True, "replayInvoked": False})
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
                    "claude-cli:claude-sonnet-4-6",
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
            self.assertEqual({row["model"] for row in rows}, {"claude-sonnet-4-6"})
            self.assertTrue(all(row["generatedAttempt"] is True for row in rows))
            self.assertTrue(all(row["status"] == "REJECTED" for row in rows))
            self.assertTrue(all(row["accepted"] is False for row in rows))
            self.assertTrue(all(row["candidateSha256"] for row in rows))
            self.assertIn("-p", invocation)
            self.assertIn("--model", invocation)
            self.assertIn("claude-sonnet-4-6", invocation)

    def test_extractor_handles_thinking_prompt_echo_and_final_proof_line(self) -> None:
        benchmark = load_benchmark()
        raw = "\x1b[?25lThinking...\nI should follow the instruction: do not include explanations, JSON, markdown fences, by, sorry, or admit.\n\x1b[?25h\nrfl\n"

        candidate, extraction, reason = benchmark.extract_proof_term_candidate(raw)

        self.assertIsNone(reason)
        self.assertEqual(candidate, "rfl")
        self.assertEqual(extraction["format"], "final-proof-line")
        self.assertIn("strip-ansi", extraction["normalization"])
        self.assertIn("last-proof-line", extraction["normalization"])

    def test_extractor_strips_think_blocks_then_extracts_final_proof_line(self) -> None:
        """Gemma-style CoT: <think>...</think> block recalls forbidden tokens; clean final answer follows.

        Pre-S2 behavior: the raw forbidden-token check on the un-stripped text would trigger the fallback
        path, and the legacy `ollama-final-line` label leaked the provider name into a provider-agnostic
        codepath. Post-S2: normalize_model_output strips <think> blocks (records `strip-think`), the
        forbidden-token check runs on the FINAL candidate only, and the format label is provider-neutral.
        """
        benchmark = load_benchmark()
        raw = (
            "<think>\n"
            "The user asked me to prove this. They explicitly said: do not use sorry or admit.\n"
            "Let me try rfl.\n"
            "</think>\n"
            "\n"
            "rfl\n"
        )

        candidate, extraction, reason = benchmark.extract_proof_term_candidate(raw)

        self.assertIsNone(reason, msg=f"expected reach-verifier but got rejection: {reason} (extraction={extraction})")
        self.assertEqual(candidate, "rfl")
        self.assertIn("strip-think", extraction["normalization"])
        # Format label must NOT leak the provider name now that the lift is provider-agnostic.
        self.assertNotEqual(extraction["format"], "ollama-final-line")

    def test_extractor_does_not_reject_when_forbidden_token_only_in_think_block(self) -> None:
        """Forbidden-token check is final-candidate-only: prompt-recall inside <think> must not trip rejection
        when the extracted candidate is clean."""
        benchmark = load_benchmark()
        raw = "<think>do not use sorry; do not use admit</think>\nEq.refl 1\n"

        candidate, extraction, reason = benchmark.extract_proof_term_candidate(raw)

        self.assertIsNone(reason, msg=f"expected reach-verifier but got rejection: {reason} (extraction={extraction})")
        self.assertEqual(candidate, "Eq.refl 1")
        self.assertIn("strip-think", extraction["normalization"])

    def test_extractor_still_rejects_when_final_candidate_contains_forbidden_token(self) -> None:
        """Final-candidate-only validation must still reject when the EXTRACTED candidate itself is forbidden."""
        benchmark = load_benchmark()
        raw = "<think>thinking...</think>\nsorry\n"

        candidate, extraction, reason = benchmark.extract_proof_term_candidate(raw)

        self.assertIsNone(candidate)
        self.assertEqual(reason, "candidate-forbidden-token")

    def test_node_url_fetches_current_head_and_passes_it_to_submit_lean(self) -> None:
        requests: list[dict[str, object]] = []
        node_head = "ab" * 32

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self) -> None:  # noqa: N802 - stdlib handler hook
                requests.append({"method": "GET", "path": self.path})
                if self.path != "/head":
                    self.send_response(404)
                    self.end_headers()
                    return
                payload = json.dumps({"ok": True, "c": node_head}).encode("utf-8")
                self.send_response(200)
                self.send_header("content-type", "application/json")
                self.send_header("content-length", str(len(payload)))
                self.end_headers()
                self.wfile.write(payload)

            def do_POST(self) -> None:  # noqa: N802 - stdlib handler hook
                length = int(self.headers.get("content-length", "0"))
                body = json.loads(self.rfile.read(length).decode("utf-8"))
                requests.append({"method": "POST", "path": self.path, "body": body})
                payload = json.dumps({"ok": True, "accepted": True, "shareHash": "cd" * 32, "block": None, "replayMatchesRuntime": True, "invalidAccepted": 0}).encode("utf-8")
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
                fake_ollama.write_text("#!/usr/bin/env python3\nprint('rfl')\n", encoding="utf-8")
                fake_ollama.chmod(0o755)
                submit_args_log = tmp_path / "submit-args.json"
                fake_submit = tmp_path / "fake-submit-lean.py"
                fake_submit.write_text(
                    "#!/usr/bin/env python3\n"
                    "import json, pathlib, sys\n"
                    "args = sys.argv[1:]\n"
                    f"pathlib.Path({str(submit_args_log)!r}).write_text(json.dumps(args))\n"
                    "head = args[args.index('--head-c') + 1]\n"
                    "print(json.dumps({'ok': True, 'command': 'submit-lean', 'accepted': True, 'shareAccepted': True, 'replayMatchesRuntime': True, 'invalidAccepted': 0, 'canonTag': 0, 'submissionBody': {'c': head, 'pk': '11'*32, 'n': '1', 'j': '0', 'nonceS': '2', 'bytes': '504f4650'}, 'block': None}))\n",
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
                        "node-head-aligned-run",
                    ],
                    cwd=ROOT,
                    text=True,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    check=False,
                )

                self.assertEqual(proc.returncode, 0, proc.stderr + proc.stdout)
                submit_args = json.loads(submit_args_log.read_text())
                rows = [json.loads(line) for line in (out_dir / "benchmark-rows.ndjson").read_text().splitlines()]
                self.assertIn("--head-c", submit_args)
                self.assertEqual(submit_args[submit_args.index("--head-c") + 1], node_head)
                self.assertEqual([request["path"] for request in requests], ["/head", "/submit"])
                self.assertEqual(requests[1]["body"]["body"]["c"], node_head)
                self.assertTrue(rows[0]["verifier"]["nodeHead"]["invoked"])
                self.assertEqual(rows[0]["verifier"]["nodeHead"]["c"], node_head)
        finally:
            server.shutdown()
            server.server_close()

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


class ReplayInvokedAndTargetFamilyDocTests(unittest.TestCase):
    """B4 + B7(a) regression tests.

    B4: per-row score carries `replayInvoked: bool`; summary `replayPassed`
    is `null` when no active row invoked replay (vacuous), `True/False`
    otherwise. The leaderboard renders the null state as the em-dash so
    "no evidence" reads as "no evidence" instead of "passed".

    B7(a): every `targetFamily` value referenced in the benchmark script
    has a corresponding section in `docs/benchmark-target-families.md`.
    The lint catches "added a new family without writing the section yet."
    """

    def test_summary_replayPassed_is_null_when_no_row_invoked_replay(self) -> None:
        """All-rejected run (fake ollama emits raw text accepted as a candidate
        but no submit-lean configured → verifier never invoked) must surface
        as `replayPassed: null`, not `True` (the legacy vacuous-pass bug)."""
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
                    "no-replay-invoked-run",
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
            self.assertIsNone(
                summary["replayPassed"],
                msg=f"replayPassed must be null when no row invoked replay, got {summary['replayPassed']!r}",
            )
            for row in rows:
                self.assertIs(
                    row["score"].get("replayInvoked"),
                    False,
                    msg=f"rejected row must carry replayInvoked: false, got score={row.get('score')}",
                )
            replay = json.loads((out_dir / "replay-report.json").read_text())
            self.assertIsNone(replay["replayPassed"])
            for row in replay["rows"]:
                self.assertIs(
                    row.get("replayInvoked"),
                    False,
                    msg=f"replay-report row must carry replayInvoked: false, got {row}",
                )

    def test_summary_replayPassed_is_true_when_invoked_row_passes(self) -> None:
        """Verified row that calls submit-lean with replayMatchesRuntime=True must surface as
        `replayPassed: True` AND per-row `score.replayInvoked: True`."""
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            fake_ollama = tmp_path / "fake-ollama.py"
            fake_ollama.write_text("#!/usr/bin/env python3\nprint('True.intro')\n", encoding="utf-8")
            fake_ollama.chmod(0o755)
            fake_submit = tmp_path / "fake-submit-lean.py"
            fake_submit.write_text(
                "#!/usr/bin/env python3\n"
                "import json\n"
                "print(json.dumps({'ok': True, 'command': 'submit-lean', 'accepted': True, 'shareAccepted': True, 'replayMatchesRuntime': True, 'invalidAccepted': 0, 'block': {'height': 0, 'selectedShares': 1}}))\n",
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
                    "replay-invoked-run",
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
            self.assertIs(summary["replayPassed"], True)
            self.assertIs(rows[0]["score"]["replayInvoked"], True)
            self.assertIs(rows[0]["score"]["replayPass"], True)

    def test_leaderboard_renders_em_dash_when_replayPassed_is_null(self) -> None:
        """When `replayPassed` is null, the leaderboard must render `—` rather than `none`/`null`/`true`."""
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
                    "leaderboard-em-dash-run",
                ],
                cwd=ROOT,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )
            self.assertEqual(proc.returncode, 0, proc.stderr + proc.stdout)
            leaderboard = (out_dir / "leaderboard.md").read_text()
            self.assertIn("- replayPassed: `—`", leaderboard)
            # The legacy stringification of None ("none") must NOT appear on the
            # replayPassed line — the em-dash is the contract.
            self.assertNotIn("- replayPassed: `none`", leaderboard)
            self.assertNotIn("- replayPassed: `null`", leaderboard)
            self.assertNotIn("- replayPassed: `true`", leaderboard)
            self.assertNotIn("- replayPassed: `false`", leaderboard)

    def test_every_target_family_value_has_a_doc_section(self) -> None:
        """B7(a) lint: every `targetFamily` literal in the script must have a
        section header in `docs/benchmark-target-families.md`. The doc fence
        guards against silently shipping a new family without prose."""
        import re

        script_text = BENCHMARK_PATH.read_text(encoding="utf-8")
        # Scrape every `boole.<dotted>.vN` literal occurring in the script —
        # this catches the constants AND any `f"-- targetFamily: ..."` strings.
        family_re = re.compile(r"\"(boole\.[a-z][a-z0-9.]*\.v\d+)\"")
        families = sorted(set(family_re.findall(script_text)))
        self.assertTrue(families, msg="no targetFamily literals found in benchmark script")

        doc_path = ROOT / "docs" / "benchmark-target-families.md"
        self.assertTrue(doc_path.is_file(), msg=f"missing target-family doc at {doc_path}")
        doc_text = doc_path.read_text(encoding="utf-8")
        for family in families:
            # Section header form: a `##` line containing the literal.
            section_re = re.compile(rf"^##[^\n]*\b{re.escape(family)}\b", re.MULTILINE)
            self.assertRegex(
                doc_text,
                section_re,
                msg=(
                    f"docs/benchmark-target-families.md missing section header for "
                    f"targetFamily literal {family!r}; add a `## {family}` section "
                    f"documenting the family and its known limitations."
                ),
            )


class VerifierHashVersioningTests(unittest.TestCase):
    """B5 regression tests.

    The benchmark driver no longer hardcodes `boole-model-benchmark-ollama-v0`.
    Instead, `fixtures/benchmarks/verifier-hashes.json` holds a version-keyed
    map plus an `active` pointer; new runs read `active`, and historical rows
    persist their `verifierHashVersion` so a replay resolves to the original
    hash even after `active` is bumped to a successor.

    The legacy `v0` value MUST stay byte-identical to the original literal so
    historical `benchmark-rows.ndjson` artifacts replay without re-derivation.
    """

    FIXTURE_PATH = ROOT / "fixtures" / "benchmarks" / "verifier-hashes.json"
    LEGACY_V0_HASH = "boole-model-benchmark-ollama-v0"

    def test_verifier_hashes_fixture_exists_with_active_and_legacy_v0_preserved(self) -> None:
        """The fixture file must exist, point `active` at a real entry, and
        keep the v0 string byte-identical to the legacy literal."""
        self.assertTrue(
            self.FIXTURE_PATH.is_file(),
            msg=f"missing verifier-hash fixture at {self.FIXTURE_PATH}",
        )
        data = json.loads(self.FIXTURE_PATH.read_text(encoding="utf-8"))
        self.assertIn("active", data, msg="fixture missing 'active' key")
        self.assertIn("versions", data, msg="fixture missing 'versions' map")
        self.assertIn(
            data["active"],
            data["versions"],
            msg=f"'active' = {data['active']!r} not present in versions {sorted(data['versions'])}",
        )
        self.assertEqual(
            data["versions"].get("v0"),
            self.LEGACY_V0_HASH,
            msg=(
                "v0 value must stay byte-identical to the legacy literal so "
                "historical rows replay against the original hash."
            ),
        )

    def test_load_verifier_hashes_returns_normalized_shape(self) -> None:
        """`load_verifier_hashes()` reads the fixture and returns
        `{"active": str, "versions": {str: str}}`."""
        benchmark = load_benchmark()
        loaded = benchmark.load_verifier_hashes()
        self.assertIsInstance(loaded, dict)
        self.assertIn("active", loaded)
        self.assertIn("versions", loaded)
        self.assertIsInstance(loaded["versions"], dict)
        for key, value in loaded["versions"].items():
            self.assertIsInstance(key, str)
            self.assertIsInstance(value, str)

    def test_resolve_verifier_hash_returns_active_by_default(self) -> None:
        """With no explicit version, `resolve_verifier_hash` returns
        `(active, versions[active])`."""
        benchmark = load_benchmark()
        hashes = {"active": "v1", "versions": {"v0": "hash-a", "v1": "hash-b"}}
        version, value = benchmark.resolve_verifier_hash(hashes=hashes)
        self.assertEqual(version, "v1")
        self.assertEqual(value, "hash-b")

    def test_resolve_verifier_hash_with_explicit_version_pins_to_recorded(self) -> None:
        """Replay of a historical row must resolve to the recorded version's
        hash regardless of the current `active`. This is the headline B5 spec."""
        benchmark = load_benchmark()
        hashes = {"active": "v1", "versions": {"v0": "hash-a", "v1": "hash-b"}}
        version, value = benchmark.resolve_verifier_hash(version="v0", hashes=hashes)
        self.assertEqual(version, "v0")
        self.assertEqual(
            value,
            "hash-a",
            msg=(
                "active='v1' but the row recorded verifierHashVersion='v0' — "
                "resolver must pin to the recorded version, not jump to active."
            ),
        )

    def test_resolve_verifier_hash_unknown_version_raises(self) -> None:
        """Unknown version → typed error, not a silent fallback."""
        benchmark = load_benchmark()
        hashes = {"active": "v0", "versions": {"v0": "hash-a"}}
        with self.assertRaises(KeyError) as ctx:
            benchmark.resolve_verifier_hash(version="v99", hashes=hashes)
        self.assertIn("v99", str(ctx.exception))
        self.assertIn("unknown verifier hash version", str(ctx.exception))

    def test_benchmark_row_carries_verifier_hash_version(self) -> None:
        """Every row produced by the driver records both `verifierHash`
        (resolved string) AND `verifierHashVersion` (the lookup key) so
        replay/validation can re-resolve the hash from the recorded version."""
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            fake_ollama = tmp_path / "fake-ollama.py"
            fake_ollama.write_text(
                "#!/usr/bin/env python3\n"
                "print('True.intro')\n",
                encoding="utf-8",
            )
            fake_ollama.chmod(0o755)
            fake_submit = tmp_path / "fake-submit-lean.py"
            fake_submit.write_text(
                "#!/usr/bin/env python3\n"
                "import json, pathlib, sys\n"
                "args = sys.argv[1:]\n"
                "block_store = pathlib.Path(args[args.index('--block-store') + 1])\n"
                "verifier_hash = args[args.index('--verifier-hash') + 1]\n"
                "block_store.write_text('{\\\"height\\\":0}\\n')\n"
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
                    "verifier-hash-version-run",
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
            verifier = rows[0]["verifier"]
            fixture = json.loads(self.FIXTURE_PATH.read_text(encoding="utf-8"))
            active_version = fixture["active"]
            active_hash = fixture["versions"][active_version]
            self.assertEqual(
                verifier["verifierHashVersion"],
                active_version,
                msg="row must record the active version key so future replays pin correctly",
            )
            self.assertEqual(
                verifier["verifierHash"],
                active_hash,
                msg="row's verifierHash must equal versions[active]",
            )


class B6TimeoutErgonomicsTests(unittest.TestCase):
    """Slice B6 regression tests for timeout ergonomics.

    Four user-visible changes:
      1. --timeout-sec default 300 → 600 (frontier-model cold-start
         latency rarely exceeds 600s).
      2. --timeout-sec 0 = no per-attempt timeout (already plumbed via
         timeout_s=None; pin the contract).
      3. New --max-run-seconds N wall-clock cap: cooperative; on trip
         finalize summary with rows-so-far + runTerminationReason.
      4. Leaderboard + summary expose attempt-latency p50/p90/p99 so
         timeout-induced rejections surface visibly.
    """

    def test_b6_default_per_attempt_timeout_is_600_seconds(self) -> None:
        """argparse --timeout-sec default must be 600s. The previous
        300s default silently rejected frontier-model attempts on cold
        start. We verify via --help so the change is visible to anyone
        reading the CLI surface."""
        proc = subprocess.run(
            ["python3", str(BENCHMARK_PATH), "--help"],
            cwd=ROOT,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        self.assertEqual(proc.returncode, 0, msg=proc.stderr)
        self.assertIn(
            "--timeout-sec", proc.stdout,
            msg="--timeout-sec flag missing from --help output",
        )
        self.assertIn(
            "default: 600", proc.stdout,
            msg=(
                "--timeout-sec default must surface as 600 in --help "
                f"(stdout: {proc.stdout!r})"
            ),
        )

    def test_b6_zero_timeout_propagates_as_none_to_run_benchmark(self) -> None:
        """`--timeout-sec 0` must propagate as `timeout_s=None` to
        `run_benchmark`. The contract was implicit before; pin it so a
        future refactor can't silently re-introduce a non-zero floor."""
        benchmark = load_benchmark()
        captured: dict = {}

        def fake_run_benchmark(**kwargs):
            captured.update(kwargs)
            return {"ok": True, "runId": kwargs.get("run_id"), "summary": {"ok": True}}

        with tempfile.TemporaryDirectory() as tmp:
            spec_path = Path(tmp) / "spec.json"
            spec_path.write_text("[]", encoding="utf-8")
            original = benchmark.run_benchmark
            benchmark.run_benchmark = fake_run_benchmark
            try:
                benchmark.main([
                    "--spec", str(spec_path),
                    "--output-dir", tmp,
                    "--run-id", "b6-zero-timeout",
                    "--timeout-sec", "0",
                ])
            finally:
                benchmark.run_benchmark = original

        self.assertIn("timeout_s", captured)
        self.assertIsNone(
            captured["timeout_s"],
            msg=f"--timeout-sec 0 must propagate as None, got {captured['timeout_s']!r}",
        )

    def test_b6_max_run_seconds_argparse_accepts_non_negative_int(self) -> None:
        """--max-run-seconds must parse non-negative ints; --max-run-seconds 0
        means no cap (same convention as --timeout-sec 0); negatives reject."""
        # 0 (no cap) parses OK
        proc_zero = subprocess.run(
            ["python3", str(BENCHMARK_PATH), "--help"],
            cwd=ROOT,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        self.assertEqual(proc_zero.returncode, 0)
        self.assertIn(
            "--max-run-seconds", proc_zero.stdout,
            msg="--max-run-seconds flag must appear in --help output",
        )

        # -1 must reject
        proc_neg = subprocess.run(
            [
                "python3", str(BENCHMARK_PATH),
                "--target", "ollama:test",
                "--max-run-seconds", "-1",
            ],
            cwd=ROOT,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        self.assertNotEqual(
            proc_neg.returncode, 0,
            msg=f"--max-run-seconds -1 must reject; stdout={proc_neg.stdout!r} stderr={proc_neg.stderr!r}",
        )

    def test_b6_max_run_seconds_stops_launching_attempts_and_writes_partial_summary(self) -> None:
        """When the wall-clock cap fires, run_benchmark must stop
        launching new attempts, write a summary with the rows it has,
        record runTerminationReason='max-run-seconds', and exit 0
        (the run terminated by operator design, not failure).

        Cooperative model: in-flight attempts are not killed; we only
        stop *launching* new ones. We verify by feeding a 5-row spec
        whose rows each sleep for 0.6s, with --max-run-seconds 1: the
        first row must complete, but the 5-row total cannot.
        """
        benchmark = load_benchmark()
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            spec_path = tmp_path / "spec.json"
            out_dir = tmp_path / "out"
            spec = [
                {
                    "name": f"slow-row-{i}",
                    "kind": "provider-model",
                    "metadata": {"provider": "mock", "backend": "mock", "model": f"mock-{i}"},
                    "command": [
                        "python3",
                        "-c",
                        "import time, json; time.sleep(0.6); print(json.dumps({'ok': True, 'summary': {'verifyAccepted': 0, 'blocksProduced': 0}, 'safety': {'invalidAccepted': 0, 'chainDivergence': 0, 'replayFailures': 0}, 'replayMatchesRuntime': True}))",
                    ],
                }
                for i in range(5)
            ]
            spec_path.write_text(json.dumps(spec), encoding="utf-8")

            result = benchmark.run_benchmark(
                spec_path=spec_path,
                output_dir=out_dir,
                run_id="b6-cap",
                max_run_seconds=1,
            )

            self.assertTrue(result["ok"] in (True, False))  # row outcome unrelated
            summary = json.loads((out_dir / "benchmark-summary.json").read_text())
            rows = [
                json.loads(line)
                for line in (out_dir / "benchmark-rows.ndjson").read_text().splitlines()
                if line.strip()
            ]

        self.assertEqual(
            summary.get("runTerminationReason"), "max-run-seconds",
            msg=f"summary missing runTerminationReason='max-run-seconds': {summary!r}",
        )
        self.assertLess(
            len(rows), 5,
            msg=f"cap should stop the 5-row run early; got {len(rows)} rows",
        )
        self.assertGreaterEqual(
            len(rows), 1,
            msg=f"at least one row should complete before the cap fires; got {len(rows)}",
        )

    def test_b6_summary_includes_latency_distribution_with_p50_p90_p99(self) -> None:
        """summarize() must emit `latencyDistribution: {p50Ms, p90Ms,
        p99Ms, sampleCount}` computed from rows with skipped=False and
        elapsedMs>0. Linear-interpolation contract (numpy default,
        type-7) so 10 rows with elapsedMs ∈ {100..1000} yield
        p50=550, p90=910, p99=991."""
        benchmark = load_benchmark()
        rows = [
            {
                "name": f"row-{i}",
                "ok": True,
                "skipped": False,
                "elapsedMs": (i + 1) * 100,  # 100, 200, …, 1000
                "score": {"blocksProduced": 0, "replayPass": True, "replayInvoked": False},
                "safety": {"invalidAccepted": 0, "chainDivergence": 0, "replayFailures": 0},
            }
            for i in range(10)
        ]
        summary = benchmark.summarize(rows, run_id="b6-quantile", generated_at_ms=0)
        self.assertIn(
            "latencyDistribution", summary,
            msg=f"summary missing latencyDistribution: {summary!r}",
        )
        ld = summary["latencyDistribution"]
        self.assertEqual(ld["sampleCount"], 10)
        self.assertEqual(ld["p50Ms"], 550)
        self.assertEqual(ld["p90Ms"], 910)
        self.assertEqual(ld["p99Ms"], 991)

    def test_b6_latency_distribution_excludes_skipped_and_zero_elapsed_rows(self) -> None:
        """skipped=True rows produced no work; elapsedMs==0 rows are
        early-exit placeholders. Both must be excluded from the p*
        sample set."""
        benchmark = load_benchmark()
        rows = [
            {"name": "skip", "ok": True, "skipped": True, "elapsedMs": 9999, "score": {"blocksProduced": 0}, "safety": {"invalidAccepted": 0, "chainDivergence": 0, "replayFailures": 0}},
            {"name": "zero", "ok": True, "skipped": False, "elapsedMs": 0, "score": {"blocksProduced": 0}, "safety": {"invalidAccepted": 0, "chainDivergence": 0, "replayFailures": 0}},
            {"name": "real-1", "ok": True, "skipped": False, "elapsedMs": 100, "score": {"blocksProduced": 0}, "safety": {"invalidAccepted": 0, "chainDivergence": 0, "replayFailures": 0}},
            {"name": "real-2", "ok": True, "skipped": False, "elapsedMs": 200, "score": {"blocksProduced": 0}, "safety": {"invalidAccepted": 0, "chainDivergence": 0, "replayFailures": 0}},
        ]
        summary = benchmark.summarize(rows, run_id="b6-filter", generated_at_ms=0)
        ld = summary["latencyDistribution"]
        self.assertEqual(ld["sampleCount"], 2)
        # Two-sample linear interp: p50 = 150, p90 = 190, p99 = 199.
        self.assertEqual(ld["p50Ms"], 150)
        self.assertEqual(ld["p90Ms"], 190)
        self.assertEqual(ld["p99Ms"], 199)

    def test_b6_leaderboard_renders_latency_p50_p90_p99(self) -> None:
        """The rendered leaderboard markdown must surface the three
        latency quantiles in the top-level summary block so a reader
        can immediately spot timeout-induced rejection patterns."""
        benchmark = load_benchmark()
        rows = [
            {
                "name": f"row-{i}",
                "ok": True,
                "skipped": False,
                "elapsedMs": (i + 1) * 100,
                "score": {"blocksProduced": 0, "replayPass": True, "replayInvoked": False},
                "safety": {"invalidAccepted": 0, "chainDivergence": 0, "replayFailures": 0},
            }
            for i in range(10)
        ]
        summary = benchmark.summarize(rows, run_id="b6-render", generated_at_ms=0)
        rendered = benchmark.render_leaderboard(summary, benchmark.leaderboard_rows(rows))
        self.assertIn("p50Ms", rendered, msg="leaderboard missing p50Ms line")
        self.assertIn("p90Ms", rendered, msg="leaderboard missing p90Ms line")
        self.assertIn("p99Ms", rendered, msg="leaderboard missing p99Ms line")
        self.assertIn(
            "550", rendered,
            msg=f"leaderboard missing p50 value 550: {rendered!r}",
        )
        self.assertIn(
            "910", rendered,
            msg=f"leaderboard missing p90 value 910: {rendered!r}",
        )
        self.assertIn(
            "991", rendered,
            msg=f"leaderboard missing p99 value 991: {rendered!r}",
        )


class S24aBalancePollingTests(unittest.TestCase):
    """S24a — `--measure-reward` polls `/account/{prover_pk}/balance` before
    and after each `/submit` so each row carries the **delta** in earned
    share reward. Aggregation produces per-model `cumulativeShareReward`,
    closing the gap that `verifier-pass-rate ≠ economic distribution`.
    """

    def test_measure_reward_records_balance_delta_in_each_row_and_summary(self) -> None:
        # The mock node returns:
        #   GET  /account/<pk>/balance → {balance: <step * 5>}
        #   POST /submit               → standard accepted+block response
        # The script polls before and after /submit on every attempt; the
        # delta lands as `attemptShareReward`. Two attempts → before=0,
        # after=5; before=5, after=10 → cumulative=10.
        balance_steps = {"count": 0}
        balance_path_seen: list[str] = []

        prover_pk = "11" * 32

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self) -> None:  # noqa: N802 - http.server API
                if self.path.startswith("/account/") and self.path.endswith("/balance"):
                    balance_path_seen.append(self.path)
                    payload = json.dumps(
                        {
                            "ok": True,
                            "pk": prover_pk,
                            "balance": str(balance_steps["count"] * 5),
                            "asOfHeight": balance_steps["count"],
                            "asOfC": "0" * 64,
                        }
                    ).encode("utf-8")
                    self.send_response(200)
                    self.send_header("content-type", "application/json")
                    self.send_header("content-length", str(len(payload)))
                    self.end_headers()
                    self.wfile.write(payload)
                    return
                self.send_response(404)
                self.end_headers()

            def do_POST(self) -> None:  # noqa: N802 - http.server API
                length = int(self.headers.get("content-length", "0"))
                _ = self.rfile.read(length)
                # Each /submit advances the synthetic balance so the next
                # /account read returns a higher number (= +5 per attempt).
                balance_steps["count"] += 1
                response = {
                    "ok": True,
                    "accepted": True,
                    "block": {"height": balance_steps["count"], "selectedShares": 1},
                    "replayMatchesRuntime": True,
                    "invalidAccepted": 0,
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
                fake_ollama.write_text(
                    "#!/usr/bin/env python3\nprint('True.intro')\n",
                    encoding="utf-8",
                )
                fake_ollama.chmod(0o755)
                fake_submit = tmp_path / "fake-submit-lean.py"
                fake_submit.write_text(
                    "#!/usr/bin/env python3\n"
                    "import json\n"
                    f"print(json.dumps({{'ok': True, 'command': 'submit-lean', 'accepted': True, 'shareAccepted': True, 'replayMatchesRuntime': True, 'invalidAccepted': 0, 'canonTag': 0, 'submissionBody': {{'c': '00'*32, 'pk': '{prover_pk}', 'n': '1', 'j': '0', 'nonceS': '2', 'bytes': '504f4650'}}, 'block': None}}))\n",
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
                        "--measure-reward",
                        "--prover-pk",
                        prover_pk,
                        "--attempts",
                        "2",
                        "--output-dir",
                        str(out_dir),
                        "--run-id",
                        "s24a-balance",
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
                rows = [
                    json.loads(line)
                    for line in (out_dir / "benchmark-rows.ndjson").read_text().splitlines()
                ]
                self.assertEqual(len(rows), 2, f"expected 2 attempt rows, got: {rows}")
                # Before/after balance + delta land on every attempt row.
                for idx, row in enumerate(rows):
                    verifier = row["verifier"]
                    self.assertIn("accountBalanceBefore", verifier, msg=f"row {idx} missing accountBalanceBefore: {verifier}")
                    self.assertIn("accountBalanceAfter", verifier, msg=f"row {idx} missing accountBalanceAfter: {verifier}")
                    self.assertIn("attemptShareReward", verifier, msg=f"row {idx} missing attemptShareReward: {verifier}")
                # Per-attempt deltas: attempt 1 sees 0→5 (delta=5); attempt 2 sees 5→10 (delta=5).
                self.assertEqual(rows[0]["verifier"]["accountBalanceBefore"], "0")
                self.assertEqual(rows[0]["verifier"]["accountBalanceAfter"], "5")
                self.assertEqual(rows[0]["verifier"]["attemptShareReward"], "5")
                self.assertEqual(rows[1]["verifier"]["accountBalanceBefore"], "5")
                self.assertEqual(rows[1]["verifier"]["accountBalanceAfter"], "10")
                self.assertEqual(rows[1]["verifier"]["attemptShareReward"], "5")
                # Mock server saw at least 4 balance GETs (before+after × 2 attempts).
                self.assertGreaterEqual(len(balance_path_seen), 4, balance_path_seen)
                # Cumulative reward exposed on the per-target summary block.
                summary = json.loads((out_dir / "benchmark-summary.json").read_text())
                # The summary structure carries per-target aggregates under "targets" or similar.
                # We assert the field appears somewhere reachable from the top.
                summary_text = json.dumps(summary)
                self.assertIn("cumulativeShareReward", summary_text, msg=f"summary missing cumulativeShareReward: {summary}")
                self.assertIn('"10"', summary_text, msg=f"summary missing cumulative value '10': {summary}")
        finally:
            server.shutdown()
            server.server_close()

    def test_block_producing_attempt_captures_proposer_pk_was_proposer_and_bonus(self) -> None:
        """S24b — when an attempt's /submit response surfaces a `block.height`,
        the runner GETs `<node>/block/<height>` to capture the proposer pk and
        record `wasProposer` plus a `proposerBonusEarned` decimal string.
        Cumulative aggregation lands on the summary as `proposerBonusCumulative`.
        """
        prover_pk = "11" * 32
        other_pk = "22" * 32
        # Block 1: prover IS proposer (bonus 1).
        # Block 2: other_pk proposes (bonus 0). Cumulative bonus = 1.
        proposer_by_height = {1: prover_pk, 2: other_pk}
        balance_steps = {"count": 0, "height": 0}
        block_paths_seen: list[str] = []

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self) -> None:  # noqa: N802 - http.server API
                if self.path.startswith("/account/") and self.path.endswith("/balance"):
                    payload = json.dumps(
                        {
                            "ok": True,
                            "pk": prover_pk,
                            "balance": str(balance_steps["count"]),
                            "asOfHeight": balance_steps["height"],
                            "asOfC": "0" * 64,
                        }
                    ).encode("utf-8")
                    self.send_response(200)
                    self.send_header("content-type", "application/json")
                    self.send_header("content-length", str(len(payload)))
                    self.end_headers()
                    self.wfile.write(payload)
                    return
                if self.path.startswith("/block/"):
                    block_paths_seen.append(self.path)
                    raw = self.path.rsplit("/", 1)[1]
                    try:
                        h = int(raw)
                    except ValueError:
                        self.send_response(400)
                        self.end_headers()
                        return
                    if h not in proposer_by_height:
                        self.send_response(404)
                        self.end_headers()
                        return
                    payload = json.dumps(
                        {
                            "ok": True,
                            "height": h,
                            "c": "ab" * 32,
                            "block": {
                                "height": h,
                                "proposerPk": proposer_by_height[h],
                                "selectedSharePks": [prover_pk],
                                "selectedShareHashes": ["cd" * 32],
                            },
                        }
                    ).encode("utf-8")
                    self.send_response(200)
                    self.send_header("content-type", "application/json")
                    self.send_header("content-length", str(len(payload)))
                    self.end_headers()
                    self.wfile.write(payload)
                    return
                self.send_response(404)
                self.end_headers()

            def do_POST(self) -> None:  # noqa: N802 - http.server API
                length = int(self.headers.get("content-length", "0"))
                _ = self.rfile.read(length)
                h = balance_steps["height"] + 1
                balance_steps["height"] = h
                # Each /submit credits the prover's account: +1 share + (+1 if proposer).
                if proposer_by_height[h] == prover_pk:
                    balance_steps["count"] += 2
                else:
                    balance_steps["count"] += 1
                response = {
                    "ok": True,
                    "accepted": True,
                    "shareAccepted": True,
                    "block": {"height": h, "selectedShares": 1},
                    "replayMatchesRuntime": True,
                    "invalidAccepted": 0,
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
                fake_ollama.write_text(
                    "#!/usr/bin/env python3\nprint('True.intro')\n",
                    encoding="utf-8",
                )
                fake_ollama.chmod(0o755)
                fake_submit = tmp_path / "fake-submit-lean.py"
                fake_submit.write_text(
                    "#!/usr/bin/env python3\n"
                    "import json\n"
                    f"print(json.dumps({{'ok': True, 'command': 'submit-lean', 'accepted': True, 'shareAccepted': True, 'replayMatchesRuntime': True, 'invalidAccepted': 0, 'canonTag': 0, 'submissionBody': {{'c': '00'*32, 'pk': '{prover_pk}', 'n': '1', 'j': '0', 'nonceS': '2', 'bytes': '504f4650'}}, 'block': None}}))\n",
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
                        "--measure-reward",
                        "--prover-pk",
                        prover_pk,
                        "--attempts",
                        "2",
                        "--output-dir",
                        str(out_dir),
                        "--run-id",
                        "s24b-proposer",
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
                rows = [
                    json.loads(line)
                    for line in (out_dir / "benchmark-rows.ndjson").read_text().splitlines()
                ]
                self.assertEqual(len(rows), 2)
                # Both attempts saw a block, so /block/<height> should have been
                # GET'd at least twice (once per block-producing attempt).
                self.assertGreaterEqual(len(block_paths_seen), 2, block_paths_seen)
                v1 = rows[0]["verifier"]
                v2 = rows[1]["verifier"]
                self.assertEqual(v1.get("proposerPk"), prover_pk, v1)
                self.assertTrue(v1.get("wasProposer"), v1)
                self.assertEqual(v1.get("proposerBonusEarned"), "1", v1)
                self.assertEqual(v2.get("proposerPk"), other_pk, v2)
                self.assertFalse(v2.get("wasProposer"), v2)
                self.assertEqual(v2.get("proposerBonusEarned"), "0", v2)
                # Summary aggregates the cumulative bonus (= 1 across the run).
                summary = json.loads((out_dir / "benchmark-summary.json").read_text())
                summary_text = json.dumps(summary)
                self.assertIn("proposerBonusCumulative", summary_text, msg=f"summary missing proposerBonusCumulative: {summary}")
                self.assertIn('"1"', summary_text, msg=f"summary missing cumulative bonus '1': {summary}")
        finally:
            server.shutdown()
            server.server_close()

    def test_bounty_mode_posts_proof_and_records_per_family_credit(self) -> None:
        """S24c — `--bounty-id <id>` makes each attempt POST `/bounties/<id>/proof`
        after running the share submission. The row carries `bountyAccepted`,
        `bountyFamilyId`, `bountyCreditEarned`; the summary aggregates
        `bountyFamilyCreditsByFamily = {familyId: cumulativeCredit}`. Two
        attempts with one accept (reward "50") and one reject must yield
        cumulative "50" under "test.alpha"."""
        prover_pk = "11" * 32
        bounty_id = "bounty-1"
        family_id = "test.alpha"
        bounty_calls: list[dict[str, Any]] = []

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self) -> None:  # noqa: N802 - http.server API
                self.send_response(404)
                self.end_headers()

            def do_POST(self) -> None:  # noqa: N802 - http.server API
                length = int(self.headers.get("content-length", "0"))
                body_raw = self.rfile.read(length)
                if self.path == f"/bounties/{bounty_id}/proof":
                    body = json.loads(body_raw.decode("utf-8")) if body_raw else {}
                    bounty_calls.append(body)
                    accepted = len(bounty_calls) == 1
                    reward = "50"
                    response = {
                        "ok": True,
                        "accepted": accepted,
                        "duplicate": False,
                        "bounty": {
                            "id": bounty_id,
                            "domain": family_id,
                            "reward": reward,
                            "status": "solved" if accepted else "open",
                            "verifier": {"kind": "always-accept"},
                            "problemHash": "ab" * 32,
                        },
                    }
                    payload = json.dumps(response).encode("utf-8")
                    self.send_response(200)
                    self.send_header("content-type", "application/json")
                    self.send_header("content-length", str(len(payload)))
                    self.end_headers()
                    self.wfile.write(payload)
                    return
                # Default /submit path (no block produced — bounty mode is
                # orthogonal to share submission for this slice).
                response = {
                    "ok": True,
                    "accepted": True,
                    "shareAccepted": True,
                    "block": None,
                    "replayMatchesRuntime": True,
                    "invalidAccepted": 0,
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
                fake_ollama.write_text(
                    "#!/usr/bin/env python3\nprint('True.intro')\n",
                    encoding="utf-8",
                )
                fake_ollama.chmod(0o755)
                fake_submit = tmp_path / "fake-submit-lean.py"
                fake_submit.write_text(
                    "#!/usr/bin/env python3\n"
                    "import json\n"
                    f"print(json.dumps({{'ok': True, 'command': 'submit-lean', 'accepted': True, 'shareAccepted': True, 'replayMatchesRuntime': True, 'invalidAccepted': 0, 'canonTag': 0, 'submissionBody': {{'c': '00'*32, 'pk': '{prover_pk}', 'n': '1', 'j': '0', 'nonceS': '2', 'bytes': '504f4650'}}, 'block': None}}))\n",
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
                        "--prover-pk",
                        prover_pk,
                        "--bounty-id",
                        bounty_id,
                        "--attempts",
                        "2",
                        "--output-dir",
                        str(out_dir),
                        "--run-id",
                        "s24c-bounty",
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
                rows = [
                    json.loads(line)
                    for line in (out_dir / "benchmark-rows.ndjson").read_text().splitlines()
                ]
                self.assertEqual(len(rows), 2)
                self.assertEqual(len(bounty_calls), 2, bounty_calls)
                # Each POST body must carry a hex32 proofHash + the prover pk.
                for call in bounty_calls:
                    self.assertIn("proofHash", call)
                    self.assertIn("prover", call)
                    self.assertEqual(call["prover"], prover_pk)
                    self.assertEqual(len(call["proofHash"]), 64)
                # Per-attempt bounty fields.
                v1 = rows[0]["verifier"]
                v2 = rows[1]["verifier"]
                self.assertTrue(v1.get("bountyAccepted"), v1)
                self.assertEqual(v1.get("bountyFamilyId"), family_id, v1)
                self.assertEqual(v1.get("bountyCreditEarned"), "50", v1)
                self.assertFalse(v2.get("bountyAccepted"), v2)
                self.assertEqual(v2.get("bountyFamilyId"), family_id, v2)
                self.assertEqual(v2.get("bountyCreditEarned"), "0", v2)
                # Summary aggregates per-family bounty credits.
                summary = json.loads((out_dir / "benchmark-summary.json").read_text())
                summary_text = json.dumps(summary)
                self.assertIn("bountyFamilyCreditsByFamily", summary_text, msg=f"summary missing per-family bounty rollup: {summary}")
                # The aggregator must register family_id with cumulative "50".
                self.assertIn(family_id, summary_text, msg=f"family id missing: {summary}")
                self.assertIn('"50"', summary_text, msg=f"expected cumulative '50': {summary}")
        finally:
            server.shutdown()
            server.server_close()

    def test_bounty_mode_unique_proof_hash_per_attempt(self) -> None:
        """The runner must derive a unique proofHash per attempt so a duplicate
        POST never short-circuits the second attempt's verifier dispatch."""
        prover_pk = "11" * 32
        bounty_id = "bounty-1"
        seen: list[str] = []

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self) -> None:  # noqa: N802
                self.send_response(404)
                self.end_headers()

            def do_POST(self) -> None:  # noqa: N802
                length = int(self.headers.get("content-length", "0"))
                body_raw = self.rfile.read(length)
                if self.path == f"/bounties/{bounty_id}/proof":
                    body = json.loads(body_raw.decode("utf-8"))
                    seen.append(body["proofHash"])
                    response = {
                        "ok": True,
                        "accepted": True,
                        "duplicate": False,
                        "bounty": {"id": bounty_id, "domain": "test.alpha", "reward": "1", "status": "open", "verifier": {"kind": "x"}, "problemHash": "00" * 32},
                    }
                    payload = json.dumps(response).encode("utf-8")
                    self.send_response(200)
                    self.send_header("content-length", str(len(payload)))
                    self.end_headers()
                    self.wfile.write(payload)
                    return
                response = {"ok": True, "accepted": True, "shareAccepted": True, "block": None, "replayMatchesRuntime": True, "invalidAccepted": 0, "shareHash": "ab" * 32}
                payload = json.dumps(response).encode("utf-8")
                self.send_response(200)
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
                    f"print(json.dumps({{'ok': True, 'command': 'submit-lean', 'accepted': True, 'shareAccepted': True, 'replayMatchesRuntime': True, 'invalidAccepted': 0, 'canonTag': 0, 'submissionBody': {{'c': '00'*32, 'pk': '{prover_pk}', 'n': '1', 'j': '0', 'nonceS': '2', 'bytes': '504f4650'}}, 'block': None}}))\n",
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
                        "--prover-pk",
                        prover_pk,
                        "--bounty-id",
                        bounty_id,
                        "--attempts",
                        "3",
                        "--output-dir",
                        str(out_dir),
                        "--run-id",
                        "s24c-uniq",
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
                self.assertEqual(len(seen), 3, seen)
                self.assertEqual(len(set(seen)), 3, f"proofHash collisions across attempts: {seen}")
        finally:
            server.shutdown()
            server.server_close()

    def test_economic_spread_metrics_appear_in_summary_and_leaderboard(self) -> None:
        """S24d — when 2+ targets have measured share-reward, the summary
        gains `rewardDistribution.economicSpread = {targetCount, min, max,
        range, spreadPct, minTarget, maxTarget}` and the rendered leaderboard
        surfaces a one-line economic-spread summary."""
        benchmark = load_benchmark()
        rows = [
            {
                "name": "ollama:model-A attempt 1",
                "target": "ollama:model-A",
                "ok": True,
                "status": "ACCEPTED",
                "score": {"blocksProduced": 1, "replayPass": True, "replayInvoked": True},
                "diagnostics": {"verifiedShares": 1},
                "safety": {"invalidAccepted": 0, "chainDivergence": 0, "replayFailures": 0},
                "verifier": {
                    "attemptShareReward": "10",
                    "proposerBonusEarned": "1",
                    "bountyId": "b-1",
                    "bountyAccepted": True,
                    "bountyFamilyId": "test.alpha",
                    "bountyCreditEarned": "30",
                },
            },
            {
                "name": "ollama:model-B attempt 1",
                "target": "ollama:model-B",
                "ok": True,
                "status": "ACCEPTED",
                "score": {"blocksProduced": 0, "replayPass": True, "replayInvoked": True},
                "diagnostics": {"verifiedShares": 0},
                "safety": {"invalidAccepted": 0, "chainDivergence": 0, "replayFailures": 0},
                "verifier": {
                    "attemptShareReward": "2",
                    "proposerBonusEarned": "0",
                    "bountyId": "b-1",
                    "bountyAccepted": False,
                    "bountyFamilyId": "test.alpha",
                    "bountyCreditEarned": "0",
                },
            },
        ]
        summary = benchmark.summarize(rows, "test-run", 0)
        rd = summary.get("rewardDistribution") or {}
        spread = rd.get("economicSpread")
        self.assertIsNotNone(spread, f"summary missing economicSpread: {summary}")
        self.assertEqual(spread["targetCount"], 2, spread)
        self.assertEqual(spread["minShareReward"], "2", spread)
        self.assertEqual(spread["maxShareReward"], "10", spread)
        self.assertEqual(spread["rangeShareReward"], "8", spread)
        # (max - min) / max * 100 = 80.0
        self.assertAlmostEqual(spread["spreadPct"], 80.0, places=2)
        self.assertEqual(spread["minTarget"], "ollama:model-B", spread)
        self.assertEqual(spread["maxTarget"], "ollama:model-A", spread)

        rendered = benchmark.render_leaderboard(summary, benchmark.leaderboard_rows(rows))
        self.assertIn("economicSpread", rendered, msg=f"leaderboard missing economicSpread:\n{rendered}")
        self.assertIn("80.00", rendered, msg=f"leaderboard missing spreadPct value:\n{rendered}")

    def test_preflight_node_passes_when_all_routes_respond_200(self) -> None:
        """S24e — `--preflight-node` exits 0 when /head, /account/<pk>/balance,
        and /bounties/<id> all answer 200. Output is a JSON status block on
        stdout suitable for ingestion by the bash preflight wrapper."""
        prover_pk = "11" * 32
        bounty_id = "bounty-x"

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self) -> None:  # noqa: N802
                if self.path == "/head":
                    payload = json.dumps({"ok": True, "height": 0, "c": "0" * 64}).encode()
                    self._send(200, payload)
                    return
                if self.path.startswith("/account/") and self.path.endswith("/balance"):
                    payload = json.dumps({"ok": True, "pk": prover_pk, "balance": "0", "asOfHeight": 0, "asOfC": "0" * 64}).encode()
                    self._send(200, payload)
                    return
                if self.path == f"/bounties/{bounty_id}":
                    payload = json.dumps({"ok": True, "bounty": {"id": bounty_id, "domain": "test.alpha", "reward": "1", "status": "open"}}).encode()
                    self._send(200, payload)
                    return
                self.send_response(404)
                self.end_headers()

            def _send(self, status: int, payload: bytes) -> None:
                self.send_response(status)
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
            node_url = f"http://127.0.0.1:{server.server_port}"
            proc = subprocess.run(
                [
                    "python3",
                    str(BENCHMARK_PATH),
                    "--preflight-node",
                    "--node-url",
                    node_url,
                    "--prover-pk",
                    prover_pk,
                    "--bounty-id",
                    bounty_id,
                ],
                cwd=ROOT,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )
            self.assertEqual(proc.returncode, 0, proc.stderr + proc.stdout)
            payload = json.loads(proc.stdout.strip().splitlines()[-1])
            self.assertTrue(payload["ok"], payload)
            routes = {p["route"]: p for p in payload["probes"]}
            self.assertIn("/head", routes)
            self.assertIn(f"/account/{prover_pk}/balance", routes)
            self.assertIn(f"/bounties/{bounty_id}", routes)
            for probe in payload["probes"]:
                self.assertTrue(probe["ok"], probe)
                self.assertEqual(probe["status"], 200, probe)
        finally:
            server.shutdown()
            server.server_close()

    def test_preflight_node_fails_when_account_route_missing(self) -> None:
        """When the account-balance route 404s, preflight must exit non-zero
        with the failing route surfaced — the bash kickoff wrapper relies on
        the typed exit to short-circuit before kickoff."""
        prover_pk = "22" * 32

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self) -> None:  # noqa: N802
                if self.path == "/head":
                    payload = json.dumps({"ok": True, "height": 0, "c": "0" * 64}).encode()
                    self.send_response(200)
                    self.send_header("content-length", str(len(payload)))
                    self.end_headers()
                    self.wfile.write(payload)
                    return
                # Every other route 404s — including /account.
                self.send_response(404)
                self.end_headers()

            def log_message(self, _format: str, *_args: object) -> None:
                return

        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            node_url = f"http://127.0.0.1:{server.server_port}"
            proc = subprocess.run(
                [
                    "python3",
                    str(BENCHMARK_PATH),
                    "--preflight-node",
                    "--node-url",
                    node_url,
                    "--prover-pk",
                    prover_pk,
                ],
                cwd=ROOT,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )
            self.assertNotEqual(proc.returncode, 0, "preflight must fail when /account 404s")
            self.assertIn("/account/", proc.stderr + proc.stdout, "failing route must surface")
        finally:
            server.shutdown()
            server.server_close()

    def test_economic_spread_is_null_when_only_one_measured_target(self) -> None:
        """A single-target run cannot have economic spread between models —
        emit `economicSpread = null` so consumers don't read 0%/0 as 'all
        models tied'."""
        benchmark = load_benchmark()
        rows = [
            {
                "name": "ollama:solo attempt 1",
                "target": "ollama:solo",
                "ok": True,
                "status": "ACCEPTED",
                "score": {"blocksProduced": 1, "replayPass": True, "replayInvoked": True},
                "diagnostics": {"verifiedShares": 1},
                "safety": {"invalidAccepted": 0, "chainDivergence": 0, "replayFailures": 0},
                "verifier": {"attemptShareReward": "5", "proposerBonusEarned": "0"},
            },
        ]
        summary = benchmark.summarize(rows, "test-run", 0)
        rd = summary.get("rewardDistribution") or {}
        self.assertIsNone(rd.get("economicSpread"), rd)

    def test_measure_reward_requires_node_url_and_prover_pk(self) -> None:
        """Without --node-url, --measure-reward has no node to poll → exit 2.
        Without --prover-pk, the script cannot construct the balance URL → exit 2."""
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            fake_ollama = tmp_path / "fake-ollama.py"
            fake_ollama.write_text("#!/usr/bin/env python3\nprint('True.intro')\n", encoding="utf-8")
            fake_ollama.chmod(0o755)
            out_dir = tmp_path / "out"

            proc = subprocess.run(
                [
                    "python3",
                    str(BENCHMARK_PATH),
                    "--target",
                    "ollama:qwen2.5-coder:7b",
                    "--ollama-command",
                    str(fake_ollama),
                    "--measure-reward",
                    "--prover-pk",
                    "11" * 32,
                    "--attempts",
                    "1",
                    "--output-dir",
                    str(out_dir),
                    "--run-id",
                    "s24a-needs-node",
                    "--benchmark-mode",
                    "smoke",
                ],
                cwd=ROOT,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )
            self.assertNotEqual(proc.returncode, 0, "must reject --measure-reward without --node-url")
            self.assertIn("--node-url", proc.stderr + proc.stdout)


if __name__ == "__main__":
    unittest.main()
