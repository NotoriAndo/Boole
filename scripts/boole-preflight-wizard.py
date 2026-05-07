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
from copy import deepcopy
from pathlib import Path
from typing import Any, NamedTuple

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


class ModelTarget(NamedTuple):
    id: str
    title: str
    group: str
    cost: str
    credential: str
    status: str
    action: str
    paid: bool = False
    run_hermes_real: bool = False
    model_preset: str | None = None
    ollama_model: str | None = None
    model_include: str | None = None


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
            "lean": command_ok("lean"),
            "lake": command_ok("lake"),
            "elan": command_ok("elan"),
            "hermes": command_ok("hermes"),
            "claude": command_ok("claude"),
            "codex": command_ok("codex"),
            "opencode": command_ok("opencode"),
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


def model_target_catalog(status: dict[str, Any]) -> list[ModelTarget]:
    commands = status.get("commands", {})
    credentials = status.get("credentials", {})
    ollama_installed = bool(commands.get("ollama"))
    ollama_detected = set(status.get("ollamaModels", []))

    def command_target(target_id: str, title: str, command: str) -> ModelTarget:
        ready = bool(commands.get(command))
        return ModelTarget(
            id=target_id,
            title=title,
            group="agent cli",
            cost="subscription/local",
            credential="OAuth/configured CLI; no API key printed",
            status="ready" if ready else "missing",
            action="ready" if ready else f"install or configure `{command}`",
            run_hermes_real=target_id == "hermes:configured" and ready,
            model_preset="oauth" if ready and target_id in {"claude-code", "codex", "opencode"} else None,
            model_include=target_id.split(":", 1)[0] if ready and target_id in {"claude-code", "codex", "opencode"} else None,
        )

    targets: list[ModelTarget] = [
        ModelTarget(
            id="safe-core",
            title="Safe deterministic core preflight",
            group="core",
            cost="free",
            credential="not needed",
            status="ready",
            action="ready",
        ),
        ModelTarget(
            id="hermes:configured",
            title="Hermes configured agent runtime",
            group="agent cli",
            cost="subscription/local",
            credential="Hermes config; no API key printed",
            status="ready" if commands.get("hermes") else "missing",
            action="ready" if commands.get("hermes") else "install/configure `hermes`",
            run_hermes_real=bool(commands.get("hermes")),
        ),
        command_target("claude-code", "Claude Code CLI runtime", "claude"),
        command_target("codex", "Codex CLI runtime", "codex"),
        command_target("opencode", "OpenCode CLI runtime", "opencode"),
    ]

    for model in ["qwen2.5-coder:7b", "llama3.1:8b"]:
        ready = ollama_installed and model in ollama_detected
        action = "ready" if ready else (f"ollama pull {model}" if ollama_installed else "install Ollama, then pull model")
        targets.append(
            ModelTarget(
                id=f"ollama:{model}",
                title=f"Ollama local model {model}",
                group="local llm",
                cost="free/local compute",
                credential="not needed",
                status="ready" if ready else "missing",
                action=action,
                model_preset="ollama",
                ollama_model=model,
            )
        )
    for model in sorted(ollama_detected):
        target_id = f"ollama:{model}"
        if any(target.id == target_id for target in targets):
            continue
        targets.append(
            ModelTarget(
                id=target_id,
                title=f"Ollama local model {model}",
                group="local llm",
                cost="free/local compute",
                credential="not needed",
                status="ready",
                action="ready",
                model_preset="ollama",
                ollama_model=model,
            )
        )

    frontier_specs = [
        ("openai:gpt-5", "OpenAI GPT-5 API", "OPENAI_API_KEY", "openai-gpt-5-api"),
        ("anthropic:claude-opus-4-7", "Anthropic Claude Opus 4.7 API", "ANTHROPIC_API_KEY", "anthropic-claude-opus-4-7-api"),
        ("google:gemini-2.5-pro", "Google Gemini 2.5 Pro API", "GOOGLE_API_KEY", "google-gemini-2-5-pro-api"),
        ("xai:grok-4", "xAI Grok 4 API", "XAI_API_KEY", "xai-grok-openai-compat-api"),
    ]
    for target_id, title, key, include in frontier_specs:
        present = bool(credentials.get(key))
        targets.append(
            ModelTarget(
                id=target_id,
                title=title,
                group="paid api",
                cost="paid/API",
                credential=f"{key} {'present' if present else 'missing'}",
                status="available" if present else "disabled",
                action="requires --allow-paid-api" if present else f"set {key} and pass --allow-paid-api",
                paid=True,
                model_preset="frontier",
                model_include=include,
            )
        )
    return targets


def render_model_picker(catalog: list[ModelTarget]) -> str:
    lines = [
        "\nModel/runtime targets",
        "=====================",
        "Select benchmark targets by number or id. Examples: 1,2 or safe-core,ollama:qwen2.5-coder:7b",
    ]
    for index, target in enumerate(catalog, start=1):
        lines.extend(
            [
                f"[{index}] {target.id} — {target.title}",
                f"    group: {target.group}",
                f"    cost: {target.cost}",
                f"    API key: {target.credential}",
                f"    status: {target.status}",
                f"    action: {target.action}",
            ]
        )
    return "\n".join(lines)


def parse_target_selection(raw: str, catalog: list[ModelTarget]) -> list[str]:
    by_id = {target.id: target.id for target in catalog}
    selected: list[str] = []
    for item in [part.strip() for part in raw.split(",") if part.strip()]:
        if item.isdigit() and 1 <= int(item) <= len(catalog):
            selected.append(catalog[int(item) - 1].id)
        elif item in by_id:
            selected.append(item)
        else:
            raise ValueError(f"unknown target: {item}")
    return selected


def apply_selected_targets(args: argparse.Namespace, catalog: list[ModelTarget]) -> None:
    selected = list(getattr(args, "target", []) or [])
    if not selected:
        return
    by_id = {target.id: target for target in catalog}
    model_presets: set[str] = set()
    for target_id in selected:
        target = by_id.get(target_id)
        if target is None:
            raise SystemExit(f"boole-preflight-wizard: unknown --target {target_id}")
        if target.run_hermes_real:
            args.run_hermes_real = True
        if target.model_preset:
            model_presets.add(target.model_preset)
        if target.ollama_model and target.ollama_model not in args.ollama_model:
            args.ollama_model.append(target.ollama_model)
        if target.model_include and target.model_include not in args.model_include:
            args.model_include.append(target.model_include)
    if "frontier" in model_presets and "ollama" in model_presets:
        args.model_preset = "all"
    elif "frontier" in model_presets:
        args.model_preset = "frontier"
    elif "ollama" in model_presets:
        args.model_preset = "ollama"
    elif "oauth" in model_presets and not args.model_preset:
        args.model_preset = "oauth"


def selected_model_preset(args: argparse.Namespace, preset_name: str) -> str | None:
    return args.model_preset or PRESETS[preset_name]["model_preset"]


def requires_paid_api_confirmation(args: argparse.Namespace, preset_name: str) -> bool:
    if getattr(args, "allow_paid_api", False):
        return False
    paid_targets = [target for target in getattr(args, "target", []) if target.startswith(("openai:", "anthropic:", "google:", "xai:"))]
    return bool(paid_targets) or selected_model_preset(args, preset_name) in {"frontier", "all"}


def redacted_status(status: dict[str, Any]) -> dict[str, Any]:
    redacted = deepcopy(status)
    redacted["credentials"] = {name: "present" if ok else "missing" for name, ok in status.get("credentials", {}).items()}
    return redacted


def purpose_label(args: argparse.Namespace) -> str:
    return getattr(args, "purpose", None) or "github-v0.1"


def benchmark_profile_label(args: argparse.Namespace) -> str:
    return getattr(args, "benchmark_profile", None) or "github-v0.1"


def reproduce_command(args: argparse.Namespace, preset_name: str) -> str:
    cmd = ["./scripts/boole-preflight-wizard.py", "--preset", preset_name]
    if getattr(args, "genesis_benchmark", False):
        cmd.append("--genesis-benchmark")
    if getattr(args, "benchmark_profile", None):
        cmd += ["--benchmark-profile", args.benchmark_profile]
    if getattr(args, "purpose", None):
        cmd += ["--purpose", args.purpose]
    if getattr(args, "attempts_per_model", None) is not None:
        cmd += ["--attempts-per-model", str(args.attempts_per_model)]
    model_preset = getattr(args, "model_preset", None)
    if model_preset:
        cmd += ["--model-preset", model_preset]
    for include in getattr(args, "model_include", []):
        cmd += ["--model-include", include]
    for target in getattr(args, "target", []):
        cmd += ["--target", target]
    for model in getattr(args, "ollama_model", []):
        cmd += ["--ollama-model", model]
    if getattr(args, "run_hermes_real", False):
        cmd.append("--run-hermes-real")
    if getattr(args, "allow_paid_api", False):
        cmd.append("--allow-paid-api")
    cmd.append("--yes")
    return " ".join(cmd)


def render_guided_steps(args: argparse.Namespace, preset_name: str, plan: list[list[str]], status: dict[str, Any]) -> str:
    model_preset = selected_model_preset(args, preset_name) or "none"
    paid_state = "enabled" if model_preset in {"frontier", "all"} else "disabled"
    selected_targets = getattr(args, "target", []) or ["safe-core"]
    benchmark_profile = benchmark_profile_label(args)
    purpose = purpose_label(args)
    command_lines = [" ".join(cmd) for cmd in plan]
    missing_required = [name for name in ["cargo", "python3", "lean", "lake"] if not status.get("commands", {}).get(name)]
    ollama_count = len(status.get("ollamaModels", []))
    lines = [
        "\nBoole Guided Preflight",
        "=======================",
        "Step 1/7 — Environment check",
        f"required tooling: {'OK' if not missing_required else 'missing ' + ', '.join(missing_required)}",
        f"ollama models detected: {ollama_count}",
        "API credentials: values hidden; status is present/missing only",
        "Step 2/7 — Run purpose",
        f"purpose: {purpose}",
        "Step 3/7 — Runtime/model selection",
        f"preset: {preset_name}",
        f"selected targets: {', '.join(selected_targets)}",
        f"model preset: {model_preset}",
        f"paid/API model rows: {paid_state}",
        "Step 4/7 — Benchmark profile",
        f"benchmark profile: {benchmark_profile}",
        f"genesis reset: {'enabled' if getattr(args, 'genesis_benchmark', False) else 'disabled'}",
        "Step 5/7 — Safety and cost boundary",
        "safe preset uses no API/OAuth, no wallet seed, no token value, and no public mining",
        "frontier/all model rows require --allow-paid-api before non-interactive execution",
        "Step 6/7 — Execution plan",
    ]
    lines.extend(f"- {line}" for line in command_lines)
    lines.extend(
        [
            "Step 7/7 — Evidence, report, and reproducibility",
            "loop: Agent → Proof → Verifier → Share → Block → Replay",
            f"reproduce command: {reproduce_command(args, preset_name)}",
            "reports: wizard-report.md, wizard-leaderboard.md, wizard-summary.redacted.json",
        ]
    )
    return "\n".join(lines)


def redact_summary(summary: dict[str, Any], evidence_dir: Path) -> dict[str, Any]:
    redacted = deepcopy(summary)
    redacted["evidenceDir"] = "[REDACTED_LOCAL_PATH]"
    if "gitStatus" in redacted:
        redacted["gitStatus"] = "[REDACTED_LOCAL_STATUS]"
    redacted["reportEvidenceDirName"] = evidence_dir.name
    return redacted


def agent_rows(summary: dict[str, Any]) -> list[dict[str, Any]]:
    for check in summary.get("checks", []):
        if check.get("name") == "agent-runtime-benchmark" and isinstance(check.get("rows"), list):
            return check["rows"]
    return []


def render_leaderboard(summary: dict[str, Any]) -> str:
    rows = agent_rows(summary)
    lines = [
        "# Boole Wizard Leaderboard",
        "",
        "Local agent/runtime rows. Scores are verifier/replay-backed; skipped rows are not failures.",
        "",
        "- Rank key: blocks → verifiedShares → replayPass → status",
        "",
    ]
    if not rows:
        lines.append("No agent-runtime benchmark rows found in this run.")
        return "\n".join(lines) + "\n"
    sorted_rows = sorted(
        rows,
        key=lambda row: (
            int(row.get("score", {}).get("blocks", 0) or 0),
            int(row.get("score", {}).get("verifiedShares", 0) or 0),
            bool(row.get("score", {}).get("replayPass", False)),
            row.get("status") == "PASS",
        ),
        reverse=True,
    )
    for index, row in enumerate(sorted_rows, start=1):
        score = row.get("score", {})
        lines.extend(
            [
                f"## {index}. {row.get('name')}",
                f"- status: {row.get('status')}",
                f"- blocks: {score.get('blocks', 0)}",
                f"- verifiedShares: {score.get('verifiedShares', 0)}",
                f"- replayPass: {score.get('replayPass', False)}",
                "",
            ]
        )
    return "\n".join(lines)


def render_report(summary: dict[str, Any], *, purpose: str, benchmark_profile: str) -> str:
    genesis = summary.get("genesisBenchmark") if isinstance(summary.get("genesisBenchmark"), dict) else {}
    difficulty = genesis.get("difficulty") if isinstance(genesis.get("difficulty"), dict) else {}
    lines = [
        "# Proof-to-Block Benchmark v0.1 Wizard Report",
        "",
        "This is local safe-genesis preflight evidence, not public-network mining and not a token/reward claim.",
        "",
        f"- purpose: {purpose}",
        f"- benchmark profile: {benchmark_profile}",
        f"- phase: {summary.get('phase')}",
        f"- ok: {summary.get('ok')}",
        f"- blocks produced: {genesis.get('blocksProduced')}",
        f"- cases passed: {genesis.get('casesPassed')}/{genesis.get('caseCount')}",
        f"- replay passed: {genesis.get('replayPassed')}",
        f"- invalid accepted: {genesis.get('invalidAccepted')}",
        f"- chain divergence: {genesis.get('chainDivergence')}",
        f"- difficulty mode: {difficulty.get('mode')}",
        f"- retarget: {difficulty.get('retarget')}",
        "",
        "Safe public wording:",
        "",
        f"> Local safe-genesis preflight produced {genesis.get('blocksProduced')} replay-valid blocks, {genesis.get('invalidAccepted')} invalid accepted, {genesis.get('chainDivergence')} divergence.",
        "",
        "Loop:",
        "",
        "```text",
        "Agent → Proof → Verifier → Share → Block → Replay",
        "```",
    ]
    return "\n".join(lines) + "\n"


def write_wizard_reports(summary: dict[str, Any], evidence_dir: Path, *, purpose: str, benchmark_profile: str) -> dict[str, Path]:
    evidence_dir.mkdir(parents=True, exist_ok=True)
    report_path = evidence_dir / "wizard-report.md"
    leaderboard_path = evidence_dir / "wizard-leaderboard.md"
    redacted_path = evidence_dir / "wizard-summary.redacted.json"
    report_path.write_text(render_report(summary, purpose=purpose, benchmark_profile=benchmark_profile), encoding="utf-8")
    leaderboard_path.write_text(render_leaderboard(summary), encoding="utf-8")
    redacted_path.write_text(json.dumps(redact_summary(summary, evidence_dir), indent=2) + "\n", encoding="utf-8")
    return {"report": report_path, "leaderboard": leaderboard_path, "redacted_summary": redacted_path}


def summary_evidence_dir(summary: dict[str, Any]) -> Path | None:
    raw = summary.get("evidenceDir")
    if not isinstance(raw, str) or not raw:
        return None
    return Path(raw)


def build_plan(args: argparse.Namespace, preset_name: str) -> list[list[str]]:
    apply_selected_targets(args, model_target_catalog(env_status()))
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
    if getattr(args, "skip_hardening_checks", False):
        preflight.append("--skip-hardening-checks")
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


def run_plan(plan: list[list[str]], dry_run: bool, *, purpose: str = "github-v0.1", benchmark_profile: str = "github-v0.1") -> int:
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
            print(
                "safePublicClaim: "
                f"local safe-genesis preflight produced {genesis.get('blocksProduced')} replay-valid blocks, "
                f"{genesis.get('invalidAccepted')} invalid accepted, {genesis.get('chainDivergence')} divergence"
            )
        for check in last_summary.get("checks", []):
            print(f"- {check.get('name')}: ok={check.get('ok')}")
        evidence_dir = summary_evidence_dir(last_summary)
        if evidence_dir:
            paths = write_wizard_reports(last_summary, evidence_dir, purpose=purpose, benchmark_profile=benchmark_profile)
            print("\nWizard reports")
            print("--------------")
            for name, path in paths.items():
                print(f"{name}: {path}")
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
    parser.add_argument("--skip-hardening-checks", action="store_true", help="Skip the S7.5 hardening regression gate inside phase7-solo-preflight.sh.")
    parser.add_argument("--attempts-per-model", type=positive_int, help="Attempts/trials per live provider model row in the optional model benchmark.")
    parser.add_argument("--install-claude", action="store_true", help="Install Claude Code command templates regardless of preset.")
    parser.add_argument("--install-codex", action="store_true", help="Install Codex prompt templates regardless of preset.")
    parser.add_argument("--run-hermes-real", action="store_true", help="Include Hermes real proof-to-block row regardless of preset.")
    parser.add_argument("--model-preset", choices=["mock", "frontier", "oauth", "ollama", "all"], help="Override preset model benchmark selection.")
    parser.add_argument("--model-include", action="append", default=[], help="Filter model benchmark rows by substring; repeatable.")
    parser.add_argument("--ollama-model", action="append", default=[], help="Specific Ollama model to include; repeatable.")
    parser.add_argument("--target", action="append", default=[], help="Hermes-style model/runtime target id to include; repeatable. Example: --target safe-core --target ollama:qwen2.5-coder:7b")
    parser.add_argument("--purpose", choices=["github-v0.1", "local-validation", "vc-demo", "tester-onboarding"], default="github-v0.1", help="Human-facing run purpose shown in the guided plan and report.")
    parser.add_argument("--benchmark-profile", choices=["github-v0.1", "safe-genesis", "local-llm", "frontier-api"], default="github-v0.1", help="Report profile label; does not enable paid/API rows by itself.")
    parser.add_argument("--allow-paid-api", action="store_true", help="Explicitly allow frontier/all model benchmark rows that may use paid API credentials.")
    args = parser.parse_args()

    status = env_status()
    print_status(status)
    if args.doctor:
        return
    if args.list_models:
        print(render_model_picker(model_target_catalog(status)))
        return

    preset = args.preset or choose_preset()
    if not args.target and not args.preset and not args.yes:
        catalog = model_target_catalog(status)
        print(render_model_picker(catalog))
        raw = input("Select benchmark targets [1]: ").strip() or "1"
        try:
            args.target = parse_target_selection(raw, catalog)
        except ValueError as exc:
            print(f"boole-preflight-wizard: {exc}", file=sys.stderr)
            raise SystemExit(2) from exc
    print(f"\nSelected preset: {preset}")
    print(PRESETS[preset]["description"])
    if requires_paid_api_confirmation(args, preset):
        print(
            "boole-preflight-wizard: frontier/all model rows may use paid API credentials; "
            "rerun with --allow-paid-api after choosing provider/models/cost budget.",
            file=sys.stderr,
        )
        raise SystemExit(2)
    plan = build_plan(args, preset)
    print(render_guided_steps(args, preset, plan, status))

    if not args.dry_run and not args.yes and not args.preset:
        answer = input("\nRun this plan? [y/N]: ").strip().lower()
        if answer not in {"y", "yes"}:
            print("Cancelled")
            return

    sys.stdout.flush()
    raise SystemExit(run_plan(plan, args.dry_run, purpose=purpose_label(args), benchmark_profile=benchmark_profile_label(args)))


if __name__ == "__main__":
    main()
