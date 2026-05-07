#!/usr/bin/env python3
"""Regression tests for preflight wizard/script orchestration."""
from __future__ import annotations

import argparse
import importlib.util
import json
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

    def test_frontier_or_everything_requires_explicit_paid_api_confirmation(self) -> None:
        wizard = load_wizard()
        safe_args = argparse.Namespace(model_preset=None, allow_paid_api=False)
        frontier_args = argparse.Namespace(model_preset=None, allow_paid_api=False)
        override_args = argparse.Namespace(model_preset="frontier", allow_paid_api=False)
        allowed_args = argparse.Namespace(model_preset="frontier", allow_paid_api=True)
        self.assertFalse(wizard.requires_paid_api_confirmation(safe_args, "safe"))
        self.assertTrue(wizard.requires_paid_api_confirmation(frontier_args, "frontier"))
        self.assertTrue(wizard.requires_paid_api_confirmation(frontier_args, "everything"))
        self.assertTrue(wizard.requires_paid_api_confirmation(override_args, "safe"))
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
                        {"name": "hermes-agent-cli-mock-verify", "status": "PASS", "score": {"verifiedShares": 1, "blocks": 1, "replayPass": True}},
                        {"name": "openclaw-opencode-agent-cli-mock-verify", "status": "SKIP", "score": {"verifiedShares": 0, "blocks": 0, "replayPass": False}},
                    ],
                }
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
        self.assertIn("verifiedShares", leaderboard)
        self.assertNotIn("/Users/", json.dumps(redacted))
        self.assertEqual(redacted["evidenceDir"], "[REDACTED_LOCAL_PATH]")


if __name__ == "__main__":
    unittest.main()
