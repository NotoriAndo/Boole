#!/usr/bin/env python3
"""Regression tests for preflight wizard/script orchestration."""
from __future__ import annotations

import argparse
import importlib.util
import json
import os
import stat
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
WIZARD_PATH = ROOT / "scripts" / "boole-preflight-wizard.py"
PREFLIGHT_PATH = ROOT / "scripts" / "phase7-solo-preflight.sh"


def load_wizard():
    spec = importlib.util.spec_from_file_location("boole_preflight_wizard", WIZARD_PATH)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class PreflightOrchestrationTests(unittest.TestCase):
    def test_doctor_reports_lean_toolchain_and_gates(self) -> None:
        wizard = load_wizard()
        status = wizard.env_status()
        for command in ["lean", "lake", "elan", "gitleaks"]:
            self.assertIn(command, status["commands"])

    def test_wizard_plan_passes_hardening_control_to_preflight(self) -> None:
        wizard = load_wizard()
        args = argparse.Namespace(
            install_claude=False,
            install_codex=False,
            evidence_dir=None,
            genesis_benchmark=False,
            attempts_per_model=None,
            run_hermes_real=False,
            model_preset=None,
            model_include=[],
            ollama_model=[],
            skip_hardening_checks=True,
        )
        plan = wizard.build_plan(args, "safe")
        self.assertEqual(plan[-1][0], "./scripts/phase7-solo-preflight.sh")
        self.assertIn("--skip-hardening-checks", plan[-1])

    def test_agent_mine_missing_runtime_skip_matches_contract_fixture(self) -> None:
        proc = subprocess.run(
            ["./scripts/boole-agent-mine.sh", "--runtime", "codex", "--agent-command", "/tmp/boole-missing-codex-runtime"],
            cwd=ROOT,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=30,
        )
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        actual = json.loads(proc.stdout)
        expected = json.loads((ROOT / "fixtures" / "protocol" / "agent-slash-mine" / "v1-missing-runtime-skip.json").read_text())
        self.assertEqual(actual, expected)

    def test_slash_command_templates_preserve_thin_safe_claim_boundary(self) -> None:
        template_paths = [
            ROOT / "templates" / "agent-slash-commands" / "claude" / "boole" / "mine.md",
            ROOT / "templates" / "agent-slash-commands" / "codex" / "boole-mine.md",
        ]
        for path in template_paths:
            text = path.read_text()
            self.assertIn("scripts/boole-agent-mine.sh", text)
            self.assertIn("deterministic verifier/canonicalizer/node replay decides acceptance", text)
            self.assertIn("not public mining", text)
            self.assertNotIn("wallet key", text.lower())

    def test_agent_slash_installer_writes_rendered_templates_and_refuses_overwrite(self) -> None:
        expected = json.loads((ROOT / "fixtures" / "protocol" / "agent-slash-mine" / "v1-install-claude-codex.json").read_text())
        with tempfile.TemporaryDirectory(dir="/tmp") as td:
            tmp = Path(td)
            claude_dir = tmp / "claude-commands"
            codex_dir = tmp / "codex-prompts"
            claude = subprocess.run(
                [
                    "./scripts/install-agent-slash-commands.sh",
                    "--profile",
                    "claude",
                    "--target-dir",
                    str(claude_dir),
                    "--boole-root",
                    str(ROOT),
                    "--force",
                ],
                cwd=ROOT,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=30,
            )
            self.assertEqual(claude.returncode, 0, claude.stdout + claude.stderr)
            codex = subprocess.run(
                [
                    "./scripts/install-agent-slash-commands.sh",
                    "--profile",
                    "codex",
                    "--target-dir",
                    str(codex_dir),
                    "--boole-root",
                    str(ROOT),
                    "--codex-args",
                    "--verify mock",
                    "--force",
                ],
                cwd=ROOT,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=30,
            )
            self.assertEqual(codex.returncode, 0, codex.stdout + codex.stderr)

            claude_mine = (claude_dir / "boole" / "mine.md").read_text()
            codex_mine = (codex_dir / "boole-mine.md").read_text()
            actual = {
                "claudeMineHasBooleRoot": f"{ROOT}/scripts/boole-agent-mine.sh --runtime claude-code" in claude_mine,
                "codexMineHasBooleRoot": f"{ROOT}/scripts/boole-agent-mine.sh --runtime codex --verify mock" in codex_mine,
                "placeholdersRemaining": [token for token in ["__BOOLE_ROOT__", "__BOOLE_ARGS__"] if token in claude_mine + codex_mine],
                "claimBoundaryPresent": "not public mining" in claude_mine and "not public mining" in codex_mine,
                "writtenBasenames": sorted(Path(path).name for path in json.loads(claude.stdout)["written"] + json.loads(codex.stdout)["written"]),
            }
            self.assertEqual(actual, expected)

            refuse = subprocess.run(
                ["./scripts/install-agent-slash-commands.sh", "--profile", "claude", "--target-dir", str(claude_dir), "--boole-root", str(ROOT)],
                cwd=ROOT,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=30,
            )
            self.assertEqual(refuse.returncode, 73)
            self.assertIn("refusing to overwrite", refuse.stderr)

    def test_agent_smoke_scripts_use_isolated_reward_stores(self) -> None:
        for script in [
            ROOT / "scripts" / "boole-miner-agent-cli-smoke.sh",
            ROOT / "scripts" / "boole-miner-hermes-cli-smoke.sh",
            ROOT / "scripts" / "boole-miner-opencode-cli-smoke.sh",
        ]:
            text = script.read_text()
            self.assertIn("REWARD_STORE", text, script.name)
            self.assertIn("--reward-store", text, script.name)
            self.assertIn("--max-requests 10", text, script.name)
            self.assertIn("wait \"$PID\" >/dev/null 2>&1 || true", text, script.name)

    def test_agent_mine_evidence_dir_writes_redacted_local_claim_boundary(self) -> None:
        expected = json.loads((ROOT / "fixtures" / "protocol" / "agent-slash-mine" / "v1-evidence-summary.json").read_text())
        with tempfile.TemporaryDirectory(dir="/tmp") as td:
            evidence_dir = Path(td) / "slash-evidence"
            proc = subprocess.run(
                [
                    "./scripts/boole-agent-mine.sh",
                    "--runtime",
                    "codex",
                    "--agent-command",
                    "/tmp/boole-missing-codex-runtime",
                    "--evidence-dir",
                    str(evidence_dir),
                ],
                cwd=ROOT,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=30,
            )
            self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
            self.assertTrue((evidence_dir / "stdout.json").is_file())
            self.assertTrue((evidence_dir / "stderr.txt").is_file())
            summary = json.loads((evidence_dir / "summary.json").read_text())
            self.assertEqual(summary["evidenceDir"], "[REDACTED_LOCAL_PATH]")
            summary["generatedAt"] = "deterministic-test"
            self.assertEqual(summary, expected)
            self.assertEqual(json.loads(proc.stdout), json.loads((evidence_dir / "stdout.json").read_text()))

    def test_ollama_gemma_smoke_evidence_capture_has_local_claim_boundary(self) -> None:
        text = (ROOT / "scripts" / "boole-miner-ollama-gemma-smoke.sh").read_text()
        self.assertIn("--evidence-dir", text)
        self.assertIn("BOOLE_OLLAMA_GEMMA_EVIDENCE_DIR", text)
        self.assertIn("stdout.json", text)
        self.assertIn("stderr.txt", text)
        self.assertIn("summary.json", text)
        self.assertIn("local controlled-smoke UX artifact, not public mining evidence", text)
        self.assertIn("publicMiningEvidence", text)
        self.assertIn("mockVerifier", text)

    def test_phase7_preflight_has_named_s7_5_hardening_gate(self) -> None:
        text = PREFLIGHT_PATH.read_text()
        self.assertIn("s7.5-hardening", text)
        for test_name in [
            "biguint_score",
            "store_fixtures",
            "local_node",
            "real_checker",
            "proof_package_bridge",
            "submit_lean_cli",
            "runtime_global_cap",
        ]:
            self.assertIn(test_name, text)

    def test_phase7_preflight_accepts_local_benchmark_command_overrides(self) -> None:
        text = PREFLIGHT_PATH.read_text()
        self.assertIn("--model-benchmark-command)", text)
        self.assertIn("--ollama-command)", text)
        self.assertIn("--submit-lean-command)", text)
        self.assertIn("--node-url)", text)
        self.assertIn("--use-node-ticket)", text)
        self.assertIn('MODEL_BENCHMARK_ARGS+=(--benchmark-command "$MODEL_BENCHMARK_COMMAND")', text)
        self.assertIn('MODEL_BENCHMARK_ARGS+=(--ollama-command "$OLLAMA_COMMAND")', text)
        self.assertIn('MODEL_BENCHMARK_ARGS+=(--submit-lean-command "$SUBMIT_LEAN_COMMAND")', text)
        self.assertIn('MODEL_BENCHMARK_ARGS+=(--node-url "$NODE_URL")', text)
        self.assertIn('MODEL_BENCHMARK_ARGS+=(--use-node-ticket)', text)
    def test_guided_flow_renders_seven_step_onboarding_contract(self) -> None:
        wizard = load_wizard()
        args = argparse.Namespace(
            preset="safe",
            purpose="github-v0.1",
            benchmark_profile="github-v0.1",
            genesis_benchmark=True,
            attempts_per_model=None,
            model_preset=None,
            model_include=[],
            ollama_model=[],
            run_hermes_real=False,
            install_claude=False,
            install_codex=False,
            evidence_dir=None,
            skip_hardening_checks=False,
            allow_paid_api=False,
            dry_run=True,
        )
        plan = wizard.build_plan(args, "safe")
        rendered = wizard.render_guided_steps(args, "safe", plan, wizard.env_status())
        for step in range(1, 8):
            self.assertIn(f"Step {step}/7", rendered)
        self.assertIn("Agent → Proof → Verifier → Share → Block → Replay", rendered)
        self.assertIn("benchmark profile: github-v0.1", rendered)
        self.assertIn("paid/API model rows: disabled", rendered)
        self.assertIn("reproduce command", rendered)

    def test_model_picker_renders_detailed_hermes_style_targets(self) -> None:
        wizard = load_wizard()
        status = {
            "commands": {
                "cargo": True,
                "python3": True,
                "lean": True,
                "lake": True,
                "hermes": True,
                "claude": True,
                "codex": False,
                "ollama": True,
            },
            "credentials": {
                "ANTHROPIC_API_KEY": False,
                "OPENAI_API_KEY": False,
                "GOOGLE_API_KEY": False,
                "XAI_API_KEY": False,
            },
            "ollamaModels": ["qwen2.5-coder:7b"],
        }
        catalog = wizard.model_target_catalog(status)
        rendered = wizard.render_model_picker(catalog)
        self.assertIn("[1] safe-core", rendered)
        self.assertIn("cost: free", rendered)
        self.assertIn("API key: not needed", rendered)
        self.assertIn("status: ready", rendered)
        self.assertIn("ollama:qwen2.5-coder:7b", rendered)
        self.assertIn("action: ready", rendered)
        self.assertIn("hermes:configured", rendered)
        self.assertIn("openai:gpt-5", rendered)
        self.assertIn("OPENAI_API_KEY missing", rendered)
        self.assertIn("status: disabled", rendered)

    def test_recovery_guidance_explains_status_why_fix_and_retry(self) -> None:
        wizard = load_wizard()
        status = {
            "commands": {
                "cargo": True,
                "python3": True,
                "lean": True,
                "lake": True,
                "ollama": True,
                "hermes": False,
            },
            "credentials": {
                "OPENAI_API_KEY": False,
                "ANTHROPIC_API_KEY": False,
                "GOOGLE_API_KEY": False,
                "XAI_API_KEY": False,
            },
            "ollamaModels": [],
            "ollama": {
                "installed": True,
                "daemon": False,
                "models": [],
                "error": "connection refused",
            },
        }
        rendered = wizard.render_recovery_guidance(status, targets=["ollama:qwen2.5-coder:7b", "hermes:configured"])
        self.assertIn("Diagnostics and recovery", rendered)
        self.assertIn("status: blocked", rendered)
        self.assertIn("why: Boole can run local model rows only when the Ollama daemon is reachable", rendered)
        self.assertIn("fix: ollama serve", rendered)
        self.assertIn("retry: ./scripts/boole-preflight-wizard.py --list-models", rendered)
        self.assertNotIn("target: ollama:qwen2.5-coder:7b", rendered)
        self.assertNotIn("fix: ollama pull qwen2.5-coder:7b", rendered)
        self.assertIn("target: hermes:configured", rendered)
        self.assertIn("fix: install/configure `hermes`", rendered)
        self.assertNotIn("sk-", rendered)

    def test_guided_flow_embeds_recovery_guidance_for_selected_missing_target(self) -> None:
        wizard = load_wizard()
        status = {
            "commands": {"cargo": True, "python3": True, "lean": True, "lake": True, "ollama": True},
            "credentials": {"OPENAI_API_KEY": False, "ANTHROPIC_API_KEY": False, "GOOGLE_API_KEY": False, "XAI_API_KEY": False},
            "ollamaModels": [],
            "ollama": {"installed": True, "daemon": True, "models": [], "error": None},
        }
        args = argparse.Namespace(
            preset="local-models",
            purpose="local-validation",
            benchmark_profile="local-llm",
            genesis_benchmark=False,
            attempts_per_model=None,
            model_preset="ollama",
            model_include=[],
            ollama_model=["qwen2.5-coder:7b"],
            target=["ollama:qwen2.5-coder:7b"],
            run_hermes_real=False,
            allow_paid_api=False,
            dry_run=True,
        )
        plan = [["./scripts/phase7-solo-preflight.sh", "--run-model-benchmark", "--model-preset", "ollama", "--ollama-model", "qwen2.5-coder:7b"]]
        rendered = wizard.render_guided_steps(args, "local-models", plan, status)
        self.assertIn("Diagnostics and recovery", rendered)
        self.assertIn("target: ollama:qwen2.5-coder:7b", rendered)
        self.assertIn("status: setup-required", rendered)
        self.assertIn("fix: ollama pull qwen2.5-coder:7b", rendered)
        self.assertIn("retry: ./scripts/boole-preflight-wizard.py --target ollama:qwen2.5-coder:7b --preset local-models --yes", rendered)

    def test_ollama_readiness_splits_install_daemon_model_states(self) -> None:
        wizard = load_wizard()
        status = {
            "commands": {"ollama": True},
            "ollamaModels": [],
            "ollama": {"installed": True, "daemon": False, "models": [], "error": "connection refused"},
        }
        readiness = wizard.summarize_ollama_readiness(status, requested_models=["qwen2.5-coder:7b"])
        self.assertEqual(readiness["state"], "daemon-unreachable")
        self.assertEqual(readiness["daemon"], "unreachable")
        self.assertIn("qwen2.5-coder:7b", readiness["missingModels"])
        self.assertIn("ollama serve", readiness["fixCommands"])
        rendered = wizard.render_ollama_readiness(readiness)
        self.assertIn("Ollama readiness", rendered)
        self.assertIn("status: blocked", rendered)
        self.assertIn("daemon: unreachable", rendered)
        self.assertIn("fix: ollama serve", rendered)
        self.assertIn("retry: ./scripts/boole-preflight-wizard.py --list-models", rendered)

    def test_model_picker_marks_ollama_daemon_unreachable_before_pull(self) -> None:
        wizard = load_wizard()
        status = {
            "commands": {"ollama": True},
            "credentials": {},
            "ollamaModels": [],
            "ollama": {"installed": True, "daemon": False, "models": [], "error": "connection refused"},
        }
        rendered = wizard.render_model_picker(wizard.model_target_catalog(status))
        self.assertIn("ollama:qwen2.5-coder:7b", rendered)
        self.assertIn("status: blocked", rendered)
        self.assertIn("action: start Ollama daemon with `ollama serve`", rendered)
        self.assertNotIn("action: ollama pull qwen2.5-coder:7b", rendered)

    def test_target_selection_maps_to_preflight_flags(self) -> None:
        wizard = load_wizard()
        args = argparse.Namespace(
            install_claude=False,
            install_codex=False,
            evidence_dir=None,
            genesis_benchmark=True,
            attempts_per_model=None,
            run_hermes_real=False,
            model_preset=None,
            model_include=[],
            ollama_model=[],
            target=["safe-core", "hermes:configured", "ollama:qwen2.5-coder:7b"],
            skip_hardening_checks=False,
        )
        wizard.env_status = lambda: {
            "commands": {"hermes": True, "ollama": True},
            "credentials": {},
            "ollamaModels": ["qwen2.5-coder:7b"],
            "ollama": {"installed": True, "daemon": True, "models": ["qwen2.5-coder:7b"], "error": None},
        }
        plan = wizard.build_plan(args, "safe")
        preflight = plan[-1]
        self.assertIn("--genesis-benchmark", preflight)
        self.assertIn("--run-hermes-real", preflight)
        self.assertIn("--run-model-benchmark", preflight)
        self.assertIn("--model-preset", preflight)
        self.assertIn("ollama", preflight)
        self.assertIn("--ollama-model", preflight)
        self.assertIn("qwen2.5-coder:7b", preflight)

    def test_wizard_plan_forwards_local_benchmark_runner_commands(self) -> None:
        wizard = load_wizard()
        args = argparse.Namespace(
            install_claude=False,
            install_codex=False,
            evidence_dir=None,
            genesis_benchmark=False,
            attempts_per_model=2,
            run_hermes_real=False,
            model_preset="ollama",
            model_include=[],
            ollama_model=["qwen2.5-coder:7b"],
            target=["ollama:qwen2.5-coder:7b"],
            skip_hardening_checks=False,
            model_benchmark_command="/tmp/fake-model-benchmark.py",
            ollama_command="/tmp/fake-ollama.py",
            submit_lean_command="/tmp/fake-submit-lean.py",
            node_url="http://127.0.0.1:8765",
            use_node_ticket=True,
        )
        wizard.env_status = lambda: {
            "commands": {"ollama": True},
            "credentials": {},
            "ollamaModels": ["qwen2.5-coder:7b"],
            "ollama": {"installed": True, "daemon": True, "models": ["qwen2.5-coder:7b"], "error": None},
        }
        plan = wizard.build_plan(args, "safe")
        preflight = plan[-1]
        self.assertIn("--run-model-benchmark", preflight)
        self.assertIn("--model-preset", preflight)
        self.assertIn("ollama", preflight)
        self.assertIn("--ollama-model", preflight)
        self.assertIn("qwen2.5-coder:7b", preflight)
        self.assertIn("--model-benchmark-command", preflight)
        self.assertIn("/tmp/fake-model-benchmark.py", preflight)
        self.assertIn("--ollama-command", preflight)
        self.assertIn("/tmp/fake-ollama.py", preflight)
        self.assertIn("--submit-lean-command", preflight)
        self.assertIn("/tmp/fake-submit-lean.py", preflight)
        self.assertIn("--node-url", preflight)
        self.assertIn("http://127.0.0.1:8765", preflight)
        self.assertIn("--use-node-ticket", preflight)

    def test_frontier_or_everything_requires_explicit_paid_api_confirmation(self) -> None:
        wizard = load_wizard()
        safe_args = argparse.Namespace(model_preset=None, target=[], allow_paid_api=False)
        frontier_args = argparse.Namespace(model_preset=None, target=[], allow_paid_api=False)
        override_args = argparse.Namespace(model_preset="frontier", target=[], allow_paid_api=False)
        target_args = argparse.Namespace(model_preset=None, target=["openai:gpt-5"], allow_paid_api=False)
        allowed_args = argparse.Namespace(model_preset="frontier", target=["openai:gpt-5"], allow_paid_api=True)
        self.assertFalse(wizard.requires_paid_api_confirmation(safe_args, "safe"))
        self.assertTrue(wizard.requires_paid_api_confirmation(frontier_args, "frontier"))
        self.assertTrue(wizard.requires_paid_api_confirmation(frontier_args, "everything"))
        self.assertTrue(wizard.requires_paid_api_confirmation(override_args, "safe"))
        self.assertTrue(wizard.requires_paid_api_confirmation(target_args, "safe"))
        self.assertFalse(wizard.requires_paid_api_confirmation(allowed_args, "safe"))

    def test_wizard_writes_redacted_report_and_leaderboard_from_summary(self) -> None:
        wizard = load_wizard()
        summary = {
            "ok": True,
            "phase": "7.0-solo-preflight",
            "evidenceDir": "SHOULD_BE_OVERRIDDEN",
            "genesisBenchmark": {
                "benchmark": "proof-to-block-genesis-preflight",
                "blocksProduced": 17,
                "casesPassed": 7,
                "caseCount": 7,
                "replayPassed": True,
                "invalidAccepted": 0,
                "chainDivergence": 0,
                "difficulty": {"mode": "epoch-retarget-v0", "retarget": "enabled"},
            },
            "checks": [
                {
                    "name": "agent-runtime-benchmark",
                    "ok": True,
                    "rows": [
                        {"name": "hermes-agent-cli-mock-verify", "status": "PASS", "score": {"blocksProduced": 1, "replayPass": True}, "diagnostics": {"verifiedShares": 1}},
                        {"name": "openclaw-opencode-agent-cli-mock-verify", "status": "SKIP", "score": {"blocksProduced": 0, "replayPass": False}, "diagnostics": {"verifiedShares": 0}},
                    ],
                },
                {
                    "name": "provider-model-live-benchmark",
                    "ok": True,
                    "rows": [
                        {"name": "ollama-qwen2.5-coder-7b", "status": "ACCEPTED", "provider": "ollama", "model": "qwen2.5-coder:7b", "generatedAttempt": True, "accepted": True, "score": {"blocksProduced": 1, "replayPass": True}, "diagnostics": {"verifiedShares": 1}},
                        {"name": "ollama-llama3.2", "status": "REJECTED", "provider": "ollama", "model": "llama3.2", "generatedAttempt": True, "accepted": False, "score": {"blocksProduced": 0, "replayPass": True}, "diagnostics": {"verifiedShares": 0}},
                    ],
                },
            ],
        }
        with tempfile.TemporaryDirectory() as tmp:
            evidence_dir = Path(tmp)
            paths = wizard.write_wizard_reports(summary, evidence_dir, purpose="github-v0.1", benchmark_profile="github-v0.1")
            report = paths["report"].read_text()
            leaderboard = paths["leaderboard"].read_text()
            redacted = json.loads(paths["redacted_summary"].read_text())
        self.assertIn("Proof-to-Block Benchmark v0.1", report)
        self.assertIn("local safe-genesis preflight", report)
        self.assertIn("17", report)
        self.assertIn("0 invalid accepted", report)
        self.assertIn("hermes-agent-cli-mock-verify", leaderboard)
        self.assertNotIn("verifiedShares", leaderboard)
        self.assertIn("ollama-qwen2.5-coder-7b", leaderboard)
        self.assertIn("Local model-generated proof attempts", report)
        self.assertIn("provider-model-live-benchmark", report)
        self.assertIn("provider-model-live-benchmark", leaderboard)
        self.assertNotIn("/Users/", json.dumps(redacted))
        self.assertEqual(redacted["evidenceDir"], "[REDACTED_LOCAL_PATH]")

    def test_wizard_runs_local_model_benchmark_smoke_with_fake_commands(self) -> None:
        with tempfile.TemporaryDirectory(dir="/tmp") as td:
            tmp = Path(td)
            evidence_dir = tmp / "evidence"
            fake_ollama_log = tmp / "fake-ollama-invocations.ndjson"
            fake_submit_log = tmp / "fake-submit-lean-invocations.ndjson"

            fake_ollama = tmp / "fake-ollama.py"
            fake_ollama.write_text(
                f"""#!/usr/bin/env python3
import json
import sys
from pathlib import Path
Path({str(fake_ollama_log)!r}).open("a", encoding="utf-8").write(json.dumps({{"argv": sys.argv[1:]}}) + "\\n")
print("True.intro")
""",
                encoding="utf-8",
            )
            fake_submit = tmp / "fake-submit-lean.py"
            fake_submit.write_text(
                f"""#!/usr/bin/env python3
import hashlib
import json
import sys
from pathlib import Path
proof = Path(sys.argv[sys.argv.index("--proof") + 1]) if "--proof" in sys.argv else Path(sys.argv[1])
Path({str(fake_submit_log)!r}).open("a", encoding="utf-8").write(json.dumps({{"argv": sys.argv[1:], "proof": str(proof)}}) + "\\n")
print(json.dumps({{
    "ok": True,
    "accepted": True,
    "shareAccepted": True,
    "replayMatchesRuntime": True,
    "invalidAccepted": 0,
    "block": {{"height": 1, "hash": "fake-block-hash"}},
    # Mirrors the v0 entry in fixtures/benchmarks/verifier-hashes.json. If the
    # fixture's active version is ever bumped past v0 and this echo needs to
    # match it, switch this fake to read --verifier-hash from argv (see the
    # fake-submit-lean in test_model_benchmark.py:test_ollama_generated_…).
    "verifierHash": "boole-model-benchmark-ollama-v0",
    "checkerArtifactHash": hashlib.sha256(proof.read_bytes()).hexdigest(),
    "elapsedMs": 1,
}}))
""",
                encoding="utf-8",
            )
            for script in [fake_ollama, fake_submit]:
                script.chmod(script.stat().st_mode | stat.S_IXUSR)

            env = os.environ.copy()
            env["BOOLE_TEST_FAST_PREFLIGHT"] = "1"
            proc = subprocess.run(
                [
                    "python3",
                    "scripts/boole-preflight-wizard.py",
                    "--preset",
                    "safe",
                    "--yes",
                    "--evidence-dir",
                    str(evidence_dir),
                    "--genesis-benchmark",
                    "--model-preset",
                    "ollama",
                    "--ollama-model",
                    "qwen2.5-coder:fake",
                    "--attempts-per-model",
                    "1",
                    "--model-benchmark-command",
                    "python3 scripts/boole-model-benchmark.py",
                    "--ollama-command",
                    str(fake_ollama),
                    "--submit-lean-command",
                    str(fake_submit),
                    "--skip-hardening-checks",
                ],
                cwd=ROOT,
                env=env,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=180,
            )
            self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
            self.assertTrue(fake_ollama_log.exists(), proc.stdout + proc.stderr)
            self.assertTrue(fake_submit_log.exists(), proc.stdout + proc.stderr)
            ollama_calls = [json.loads(line) for line in fake_ollama_log.read_text(encoding="utf-8").splitlines()]
            submit_calls = [json.loads(line) for line in fake_submit_log.read_text(encoding="utf-8").splitlines()]
            self.assertTrue(any("qwen2.5-coder:fake" in " ".join(call["argv"]) for call in ollama_calls))
            self.assertTrue(any(call["proof"].endswith(".lean") for call in submit_calls))

            model_artifact_dir = evidence_dir / "model-benchmark-artifacts" / "ollama-qwen2-5-coder-fake"
            for name in ["benchmark-summary.json", "benchmark-rows.ndjson", "replay-report.json", "leaderboard.md"]:
                self.assertTrue((model_artifact_dir / name).exists(), f"missing {name}\nstdout={proc.stdout}\nstderr={proc.stderr}")

            wizard_summary = json.loads((evidence_dir / "wizard-summary.redacted.json").read_text(encoding="utf-8"))
            provider_check = next(check for check in wizard_summary["checks"] if check["name"] == "provider-model-live-benchmark")
            self.assertTrue(provider_check["ok"])
            self.assertEqual(provider_check["rows"][0]["metadata"]["provider"], "ollama")
            self.assertEqual(provider_check["rows"][0]["metadata"]["model"], "qwen2.5-coder:fake")
            self.assertTrue(provider_check["rows"][0]["generatedAttempt"])
            self.assertTrue(provider_check["rows"][0]["accepted"])
            self.assertEqual(provider_check["rows"][0]["score"], {"blocksProduced": 1, "replayPass": True})
            self.assertEqual(provider_check["rows"][0]["diagnostics"]["verifiedShares"], 1)
            self.assertEqual(wizard_summary["genesisBenchmark"]["invalidAccepted"], 0)
            self.assertIn("Local model proof-attempt rows", (evidence_dir / "wizard-leaderboard.md").read_text(encoding="utf-8"))
            self.assertIn("qwen2.5-coder:fake", (evidence_dir / "wizard-leaderboard.md").read_text(encoding="utf-8"))
            self.assertIn("generated attempts: 1", (evidence_dir / "wizard-report.md").read_text(encoding="utf-8"))


if __name__ == "__main__":
    unittest.main()
