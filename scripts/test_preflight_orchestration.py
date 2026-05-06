#!/usr/bin/env python3
"""Regression tests for preflight wizard/script orchestration."""
from __future__ import annotations

import argparse
import importlib.util
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


if __name__ == "__main__":
    unittest.main()
