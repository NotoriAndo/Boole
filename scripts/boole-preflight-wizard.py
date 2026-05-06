#!/usr/bin/env python3
"""Boole preflight setup wizard.

A Hermes-style local setup runner for solo preflight. It keeps consensus logic in
existing scripts and only orchestrates environment checks, preset selection, and
safe result summarization.
"""
from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]

PRESETS: dict[str, dict[str, Any]] = {
    "safe": {
        "description": "Deterministic core preflight only; no paid API or live stochastic model matrix.",
        "install_claude": False,
        "install_codex": False,
        "run_hermes_real": False,
        "model_preset": None,
    },
    "agent-local": {
        "description": "Install local agent commands and include Hermes real proof-to-block evidence.",
        "install_claude": True,
        "install_codex": True,
        "run_hermes_real": True,
        "model_preset": None,
    },
    "local-models": {
        "description": "Hermes real evidence plus all installed Ollama models.",
        "install_claude": True,
        "install_codex": True,
        "run_hermes_real": True,
        "model_preset": "ollama",
    },
    "frontier": {
        "description": "Hermes real evidence plus frontier API/OAuth model rows.",
        "install_claude": True,
        "install_codex": True,
        "run_hermes_real": True,
        "model_preset": "frontier",
    },
    "everything": {
        "description": "Everything available: Hermes real, API/OAuth rows, and installed Ollama models.",
        "install_claude": True,
        "install_codex": True,
        "run_hermes_real": True,
        "model_preset": "all",
    },
}


def run(cmd: list[str], *, dry_run: bool, capture: bool = False, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str] | None:
    printable = " ".join(cmd)
    print(f"$ {printable}", file=sys.stderr)
    if dry_run:
        return None
    merged_env = os.environ.copy()
    if env:
        merged_env.update(env)
    return subprocess.run(cmd, cwd=ROOT, text=True, capture_output=capture, env=merged_env, check=False)


def command_ok(name: str) -> bool:
    return shutil.which(name) is not None


def ollama_models() -> list[str]:
    if not command_ok("ollama"):
        return []
    try:
        proc = subprocess.run(["ollama", "list"], text=True, capture_output=True, timeout=10)
    except Exception:
        return []
    if proc.returncode != 0:
        return []
    models = []
    for line in proc.stdout.splitlines()[1:]:
        parts = line.split()
        if parts:
            models.append(parts[0])
    return models


def env_status() -> dict[str, Any]:
    return {
        "commands": {
            "cargo": command_ok("cargo"),
            "node": command_ok("node"),
            "npm": command_ok("npm"),
            "python3": command_ok("python3"),
            "hermes": command_ok("hermes"),
            "claude": command_ok("claude"),
            "codex": command_ok("codex"),
            "ollama": command_ok("ollama"),
            "gitleaks": command_ok("gitleaks"),
        },
        "credentials": {
            "ANTHROPIC_API_KEY": bool(os.environ.get("ANTHROPIC_API_KEY")),
            "OPENAI_API_KEY": bool(os.environ.get("OPENAI_API_KEY")),
            "GOOGLE_API_KEY": bool(os.environ.get("GOOGLE_API_KEY")),
            "XAI_API_KEY": bool(os.environ.get("XAI_API_KEY")),
        },
        "ollamaModels": ollama_models(),
    }


def print_status(status: dict[str, Any]) -> None:
    print("\nBoole Preflight Wizard")
    print("=======================")
    print("\nEnvironment")
    for name, ok in status["commands"].items():
        print(f"- {name}: {'OK' if ok else 'missing'}")
    print("\nCredentials")
    for name, ok in status["credentials"].items():
        print(f"- {name}: {'present' if ok else 'missing'}")
    models = status["ollamaModels"]
    print("\nOllama models")
    if models:
        for model in models:
            print(f"- {model}")
    else:
        print("- none detected or ollama unavailable")


def choose_preset() -> str:
    print("\nSelect preset")
    names = list(PRESETS)
    for idx, name in enumerate(names, start=1):
        print(f"{idx}) {name}: {PRESETS[name]['description']}")
    while True:
        raw = input("Preset [1]: ").strip() or "1"
        if raw.isdigit() and 1 <= int(raw) <= len(names):
            return names[int(raw) - 1]
        if raw in PRESETS:
            return raw
        print("Invalid selection")


def model_list(dry_run: bool) -> None:
    if dry_run:
        print("$ ./scripts/preflight-model-benchmark-setup.py --preset all --list", file=sys.stderr)
        return
    subprocess.run([str(ROOT / "scripts/preflight-model-benchmark-setup.py"), "--preset", "all", "--list"], cwd=ROOT, check=False)


def positive_int(raw: str) -> int:
    value = int(raw)
    if value <= 0:
        raise argparse.ArgumentTypeError("must be a positive integer")
    return value


def build_plan(args: argparse.Namespace, preset_name: str) -> list[list[str]]:
    preset = PRESETS[preset_name]
    plan: list[list[str]] = []
    if preset["install_claude"] or args.install_claude:
        plan.append(["./scripts/install-agent-slash-commands.sh", "--profile", "claude", "--target-dir", ".claude/commands", "--force"])
    if preset["install_codex"] or args.install_codex:
        plan.append(["./scripts/install-agent-slash-commands.sh", "--profile", "codex", "--target-dir", ".codex/prompts", "--force"])

    preflight = ["./scripts/phase7-solo-preflight.sh"]
    if args.evidence_dir:
        preflight += ["--evidence-dir", args.evidence_dir]
    if args.genesis_benchmark:
        preflight.append("--genesis-benchmark")
    if args.attempts_per_model is not None:
        preflight += ["--attempts-per-model", str(args.attempts_per_model)]
    if preset["run_hermes_real"] or args.run_hermes_real:
        preflight.append("--run-hermes-real")
    model_preset = args.model_preset or preset["model_preset"]
    if model_preset:
        preflight += ["--run-model-benchmark", "--model-preset", model_preset]
    for include in args.model_include:
        preflight += ["--model-include", include]
    for model in args.ollama_model:
        preflight += ["--ollama-model", model]
    plan.append(preflight)
    return plan


def summarize_preflight(stdout: str) -> dict[str, Any] | None:
    candidates = [line.strip() for line in stdout.splitlines() if line.strip().startswith("{") and line.strip().endswith("}")]
    if not candidates:
        return None
    try:
        return json.loads(candidates[-1])
    except json.JSONDecodeError:
        return None


def run_plan(plan: list[list[str]], dry_run: bool) -> int:
    last_summary = None
    for cmd in plan:
        proc = run(cmd, dry_run=dry_run, capture=not dry_run)
        if proc is None:
            continue
        if proc.stderr:
            print(proc.stderr, file=sys.stderr, end="")
        if proc.stdout:
            print(proc.stdout, end="")
        if proc.returncode != 0:
            print(f"boole-preflight-wizard: command failed with exit {proc.returncode}: {' '.join(cmd)}", file=sys.stderr)
            return proc.returncode
        if "phase7-solo-preflight.sh" in cmd[0]:
            last_summary = summarize_preflight(proc.stdout)
    if last_summary:
        print("\nWizard summary")
        print("--------------")
        print(f"ok: {last_summary.get('ok')}")
        print(f"evidenceDir: {last_summary.get('evidenceDir')}")
        genesis = last_summary.get("genesisBenchmark")
        if isinstance(genesis, dict):
            print(f"genesisBenchmark: {genesis.get('benchmark')} mode={genesis.get('genesisMode')} replayPassed={genesis.get('replayPassed')}")
        for check in last_summary.get("checks", []):
            print(f"- {check.get('name')}: ok={check.get('ok')}")
    return 0


def main() -> None:
    parser = argparse.ArgumentParser(description="Hermes-style Boole preflight setup wizard.")
    parser.add_argument("--preset", choices=list(PRESETS), help="Non-interactive preset.")
    parser.add_argument("--dry-run", action="store_true", help="Print commands without executing.")
    parser.add_argument("--doctor", action="store_true", help="Only print environment status.")
    parser.add_argument("--list-models", action="store_true", help="List generated model rows and credential presence.")
    parser.add_argument("--yes", action="store_true", help="Do not prompt before executing in interactive mode.")
    parser.add_argument("--evidence-dir", help="Evidence directory to pass to phase7-solo-preflight.sh.")
    parser.add_argument("--genesis-benchmark", action="store_true", help="Run a clean genesis-reset preflight benchmark and record reproducibility metadata.")
    parser.add_argument("--attempts-per-model", type=positive_int, help="Attempts/trials per live provider model row in the optional model benchmark.")
    parser.add_argument("--install-claude", action="store_true", help="Install Claude Code command templates regardless of preset.")
    parser.add_argument("--install-codex", action="store_true", help="Install Codex prompt templates regardless of preset.")
    parser.add_argument("--run-hermes-real", action="store_true", help="Include Hermes real proof-to-block row regardless of preset.")
    parser.add_argument("--model-preset", choices=["mock", "frontier", "oauth", "ollama", "all"], help="Override preset model benchmark selection.")
    parser.add_argument("--model-include", action="append", default=[], help="Filter model benchmark rows by substring; repeatable.")
    parser.add_argument("--ollama-model", action="append", default=[], help="Specific Ollama model to include; repeatable.")
    args = parser.parse_args()

    status = env_status()
    print_status(status)
    if args.doctor:
        return
    if args.list_models:
        print("\nModel rows")
        model_list(args.dry_run)
        return

    preset = args.preset or choose_preset()
    print(f"\nSelected preset: {preset}")
    print(PRESETS[preset]["description"])
    plan = build_plan(args, preset)
    print("\nPlan")
    for cmd in plan:
        print("- " + " ".join(cmd))

    if not args.dry_run and not args.yes and not args.preset:
        answer = input("\nRun this plan? [y/N]: ").strip().lower()
        if answer not in {"y", "yes"}:
            print("Cancelled")
            return

    raise SystemExit(run_plan(plan, args.dry_run))


if __name__ == "__main__":
    main()
