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
import re
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


def ollama_version() -> str | None:
    if not command_ok("ollama"):
        return None
    try:
        proc = subprocess.run(["ollama", "--version"], text=True, capture_output=True, timeout=5)
    except Exception:
        return None
    if proc.returncode != 0:
        return None
    output = "\n".join(part for part in [proc.stdout, proc.stderr] if part).strip()
    version_match = re.search(r"(?:client version is|version is|version)\s+([0-9][^\s]*)", output, flags=re.IGNORECASE)
    if version_match:
        return version_match.group(1)
    for line in output.splitlines():
        if line.strip() and "warning" not in line.lower() and "could not connect" not in line.lower():
            return line.strip()
    return None


def ollama_status() -> dict[str, Any]:
    installed = command_ok("ollama")
    version = ollama_version() if installed else None
    endpoint = "http://127.0.0.1:11434"
    if not installed:
        return {"installed": False, "version": None, "endpoint": endpoint, "daemon": False, "models": [], "error": "ollama command missing"}
    try:
        proc = subprocess.run(["ollama", "list"], text=True, capture_output=True, timeout=10)
    except Exception as exc:
        return {"installed": True, "version": version, "endpoint": endpoint, "daemon": False, "models": [], "error": str(exc)}
    if proc.returncode != 0:
        error = (proc.stderr or proc.stdout or "ollama list failed").strip()
        return {"installed": True, "version": version, "endpoint": endpoint, "daemon": False, "models": [], "error": error}
    models = []
    for line in proc.stdout.splitlines()[1:]:
        parts = line.split()
        if parts:
            models.append(parts[0])
    return {"installed": True, "version": version, "endpoint": endpoint, "daemon": True, "models": models, "error": None}


def ollama_models() -> list[str]:
    return list(ollama_status().get("models", []))


def summarize_ollama_readiness(status: dict[str, Any], requested_models: list[str] | None = None) -> dict[str, Any]:
    ollama = status.get("ollama", {})
    commands = status.get("commands", {})
    installed = bool(ollama.get("installed", commands.get("ollama")))
    daemon = bool(ollama.get("daemon", bool(status.get("ollamaModels"))))
    models = sorted(set(ollama.get("models") or status.get("ollamaModels", [])))
    requested = list(dict.fromkeys(requested_models or []))
    missing = [model for model in requested if model not in models]
    fix_commands: list[str] = []
    retry_command = "./scripts/boole-preflight-wizard.py --list-models"
    status_label = "ready"
    if not installed:
        state = "command-missing"
        status_label = "blocked"
        fix_commands.append("Install Ollama from https://ollama.com/download")
    elif not daemon:
        state = "daemon-unreachable"
        status_label = "blocked"
        fix_commands.append("ollama serve")
    elif missing:
        state = "models-missing"
        status_label = "setup-required"
        fix_commands.extend(f"ollama pull {model}" for model in missing)
    else:
        state = "ready"
    if requested:
        retry_command = "./scripts/boole-preflight-wizard.py " + " ".join(f"--target ollama:{model}" for model in requested) + " --preset local-models --yes"
        if state in {"command-missing", "daemon-unreachable"}:
            retry_command = "./scripts/boole-preflight-wizard.py --list-models"
    return {
        "state": state,
        "status": status_label,
        "installed": installed,
        "version": ollama.get("version"),
        "endpoint": ollama.get("endpoint", "http://127.0.0.1:11434"),
        "daemon": "reachable" if daemon else "unreachable",
        "models": models,
        "requestedModels": requested,
        "missingModels": missing,
        "error": ollama.get("error"),
        "fixCommands": fix_commands,
        "retryCommand": retry_command,
    }


def render_ollama_readiness(readiness: dict[str, Any]) -> str:
    lines = [
        "",
        "Ollama readiness",
        "================",
        f"status: {readiness['status']}",
        f"state: {readiness['state']}",
        f"installed: {'yes' if readiness['installed'] else 'no'}",
        f"version: {readiness.get('version') or 'unknown'}",
        f"endpoint: {readiness.get('endpoint')}",
        f"daemon: {readiness['daemon']}",
        "models: " + (", ".join(readiness.get("models", [])) if readiness.get("models") else "none detected"),
    ]
    if readiness.get("requestedModels"):
        lines.append("requested models: " + ", ".join(readiness["requestedModels"]))
    if readiness.get("missingModels"):
        lines.append("missing models: " + ", ".join(readiness["missingModels"]))
    if readiness.get("error"):
        lines.append(f"last error: {readiness['error']}")
    fixes = readiness.get("fixCommands") or ["none"]
    for command in fixes:
        lines.append(f"fix: {command}")
    lines.append(f"retry: {readiness['retryCommand']}")
    return "\n".join(lines)


def env_status() -> dict[str, Any]:
    ollama = ollama_status()
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
        "ollamaModels": ollama["models"],
        "ollama": ollama,
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
    print(render_ollama_readiness(summarize_ollama_readiness(status)))
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
    ollama_readiness = summarize_ollama_readiness(status)
    ollama_installed = bool(ollama_readiness["installed"])
    ollama_daemon = ollama_readiness["daemon"] == "reachable"
    ollama_detected = set(ollama_readiness["models"])

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
        ready = ollama_installed and ollama_daemon and model in ollama_detected
        if ready:
            status_label = "ready"
            action = "ready"
        elif not ollama_installed:
            status_label = "blocked"
            action = "install Ollama, then pull model"
        elif not ollama_daemon:
            status_label = "blocked"
            action = "start Ollama daemon with `ollama serve`"
        else:
            status_label = "setup-required"
            action = f"ollama pull {model}"
        targets.append(
            ModelTarget(
                id=f"ollama:{model}",
                title=f"Ollama local model {model}",
                group="local llm",
                cost="free/local compute",
                credential="not needed",
                status=status_label,
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


def recovery_items(status: dict[str, Any], targets: list[str] | None = None) -> list[dict[str, str]]:
    commands = status.get("commands", {})
    credentials = status.get("credentials", {})
    ollama = status.get("ollama", {})
    ollama_installed = bool(ollama.get("installed", commands.get("ollama")))
    ollama_daemon = bool(ollama.get("daemon", bool(status.get("ollamaModels"))))
    ollama_models_detected = set(ollama.get("models") or status.get("ollamaModels", []))
    selected = targets or []
    items: list[dict[str, str]] = []

    required_missing = [name for name in ["cargo", "python3", "lean", "lake"] if not commands.get(name)]
    if required_missing:
        items.append(
            {
                "target": "required tooling",
                "status": "blocked",
                "why": "Boole cannot run the verifier/replay preflight without Rust, Python, Lean, and Lake.",
                "fix": "rerun ./install.sh --yes, or install the missing commands: " + ", ".join(required_missing),
                "retry": "./scripts/boole-preflight-wizard.py --doctor",
            }
        )

    if any(target.startswith("ollama:") for target in selected):
        if not ollama_installed:
            items.append(
                {
                    "target": "ollama",
                    "status": "blocked",
                    "why": "Boole can run local model rows only when the Ollama command is installed.",
                    "fix": "Install Ollama from https://ollama.com/download, then pull the selected model.",
                    "retry": "./scripts/boole-preflight-wizard.py --list-models",
                }
            )
        elif not ollama_daemon:
            items.append(
                {
                    "target": "ollama",
                    "status": "blocked",
                    "why": "Boole can run local model rows only when the Ollama daemon is reachable.",
                    "fix": "ollama serve",
                    "retry": "./scripts/boole-preflight-wizard.py --list-models",
                }
            )
        for target in selected:
            if target.startswith("ollama:"):
                model = target.split(":", 1)[1]
                if ollama_daemon and model not in ollama_models_detected:
                    items.append(
                        {
                            "target": target,
                            "status": "setup-required",
                            "why": "The selected local model is not installed, so Boole cannot create a model benchmark row for it yet.",
                            "fix": f"ollama pull {model}",
                            "retry": f"./scripts/boole-preflight-wizard.py --target {target} --preset local-models --yes",
                        }
                    )

    if "hermes:configured" in selected and not commands.get("hermes"):
        items.append(
            {
                "target": "hermes:configured",
                "status": "blocked",
                "why": "Hermes agent-runtime rows require the Hermes CLI to be installed and configured locally.",
                "fix": "install/configure `hermes`",
                "retry": "./scripts/boole-preflight-wizard.py --target hermes:configured --preset agent-local --yes",
            }
        )

    cli_targets = [("claude-code", "claude"), ("codex", "codex"), ("opencode", "opencode")]
    for target, command in cli_targets:
        if target in selected and not commands.get(command):
            items.append(
                {
                    "target": target,
                    "status": "blocked",
                    "why": f"The {target} row needs the `{command}` CLI to be installed and logged in before Boole can run it.",
                    "fix": f"install or configure `{command}`",
                    "retry": f"./scripts/boole-preflight-wizard.py --target {target} --preset agent-local --yes",
                }
            )

    api_keys = {
        "openai:": "OPENAI_API_KEY",
        "anthropic:": "ANTHROPIC_API_KEY",
        "google:": "GOOGLE_API_KEY",
        "xai:": "XAI_API_KEY",
    }
    for target in selected:
        for prefix, key in api_keys.items():
            if target.startswith(prefix):
                missing = not credentials.get(key)
                items.append(
                    {
                        "target": target,
                        "status": "blocked" if missing else "needs-confirmation",
                        "why": "Frontier/API rows can cost money and must be explicitly approved; API key values are never printed or stored.",
                        "fix": f"set {key} in your shell" if missing else "choose provider/model/attempt budget, then add --allow-paid-api",
                        "retry": f"./scripts/boole-preflight-wizard.py --target {target} --preset frontier --allow-paid-api --yes",
                    }
                )
                break

    return items


def render_recovery_guidance(status: dict[str, Any], targets: list[str] | None = None) -> str:
    items = recovery_items(status, targets)
    lines = ["", "Diagnostics and recovery", "========================"]
    if not items:
        lines.append("status: ready")
        lines.append("why: required local tooling and selected targets look ready for the requested wizard path")
        lines.append("fix: none")
        lines.append("retry: ./scripts/boole-preflight-wizard.py --preset safe --genesis-benchmark --yes")
        return "\n".join(lines)
    for item in items:
        lines.extend(
            [
                f"target: {item['target']}",
                f"status: {item['status']}",
                f"why: {item['why']}",
                f"fix: {item['fix']}",
                f"retry: {item['retry']}",
                "",
            ]
        )
    return "\n".join(lines).rstrip()


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
    if getattr(args, "model_benchmark_command", None):
        cmd += ["--model-benchmark-command", args.model_benchmark_command]
    if getattr(args, "ollama_command", None):
        cmd += ["--ollama-command", args.ollama_command]
    if getattr(args, "submit_lean_command", None):
        cmd += ["--submit-lean-command", args.submit_lean_command]
    if getattr(args, "node_url", None):
        cmd += ["--node-url", args.node_url]
    if getattr(args, "use_node_ticket", False):
        cmd.append("--use-node-ticket")
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
    guidance = render_recovery_guidance(status, targets=selected_targets)
    if "status: ready" not in guidance:
        lines.append(guidance)
    return "\n".join(lines)


def redact_summary(summary: dict[str, Any], evidence_dir: Path) -> dict[str, Any]:
    redacted = deepcopy(summary)
    redacted["evidenceDir"] = "[REDACTED_LOCAL_PATH]"
    if "gitStatus" in redacted:
        redacted["gitStatus"] = "[REDACTED_LOCAL_STATUS]"
    redacted["reportEvidenceDirName"] = evidence_dir.name
    return redacted


def check_rows(summary: dict[str, Any], name: str) -> list[dict[str, Any]]:
    for check in summary.get("checks", []):
        if check.get("name") == name and isinstance(check.get("rows"), list):
            return check["rows"]
    return []


def agent_rows(summary: dict[str, Any]) -> list[dict[str, Any]]:
    return check_rows(summary, "agent-runtime-benchmark")


def model_rows(summary: dict[str, Any]) -> list[dict[str, Any]]:
    return check_rows(summary, "provider-model-live-benchmark")


def block_score(row: dict[str, Any]) -> int:
    score = row.get("score", {})
    return int(score.get("blocksProduced", score.get("blocks", 0)) or 0)


def sorted_benchmark_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return sorted(
        rows,
        key=lambda row: (
            block_score(row),
            bool(row.get("score", {}).get("replayPass", False)),
            row.get("status") in {"PASS", "ACCEPTED"},
        ),
        reverse=True,
    )


def append_leaderboard_rows(lines: list[str], rows: list[dict[str, Any]]) -> None:
    for index, row in enumerate(sorted_benchmark_rows(rows), start=1):
        score = row.get("score", {})
        provider = row.get("provider")
        model = row.get("model")
        lines.extend(
            [
                f"## {index}. {row.get('name')}",
                f"- status: {row.get('status')}",
                f"- blocksProduced: {block_score(row)}",
                f"- replayPass: {score.get('replayPass', False)}",
            ]
        )
        if provider:
            lines.append(f"- provider: {provider}")
        if model:
            lines.append(f"- model: {model}")
        if "generatedAttempt" in row:
            lines.append(f"- generatedAttempt: {row.get('generatedAttempt')}")
        lines.append("")


def render_leaderboard(summary: dict[str, Any]) -> str:
    agents = agent_rows(summary)
    models = model_rows(summary)
    lines = [
        "# Boole Wizard Leaderboard",
        "",
        "Local agent/runtime and model proof-attempt rows. Scores are verifier/replay-backed; skipped or rejected rows are not runner failures.",
        "",
        "- Rank key: blocksProduced → replayPass → status",
        "",
        "# Agent/runtime rows",
        "",
    ]
    if agents:
        append_leaderboard_rows(lines, agents)
    else:
        lines.extend(["No agent-runtime benchmark rows found in this run.", ""])
    lines.extend(["# Local model proof-attempt rows", "", "- check: provider-model-live-benchmark", ""])
    if models:
        append_leaderboard_rows(lines, models)
    else:
        lines.extend(["No local model proof-attempt rows found in this run.", ""])
    return "\n".join(lines)


def render_report(summary: dict[str, Any], *, purpose: str, benchmark_profile: str) -> str:
    genesis = summary.get("genesisBenchmark") if isinstance(summary.get("genesisBenchmark"), dict) else {}
    difficulty = genesis.get("difficulty") if isinstance(genesis.get("difficulty"), dict) else {}
    models = model_rows(summary)
    generated_attempts = sum(1 for row in models if row.get("generatedAttempt"))
    accepted_models = sum(1 for row in models if row.get("status") == "ACCEPTED" or row.get("accepted") is True)
    rejected_models = sum(1 for row in models if row.get("status") == "REJECTED" or row.get("accepted") is False)
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
        "Local model-generated proof attempts:",
        "- check: provider-model-live-benchmark",
        f"- rows: {len(models)}",
        f"- generated attempts: {generated_attempts}",
        f"- accepted: {accepted_models}",
        f"- rejected: {rejected_models}",
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
    if getattr(args, "model_benchmark_command", None):
        preflight += ["--model-benchmark-command", args.model_benchmark_command]
    if getattr(args, "ollama_command", None):
        preflight += ["--ollama-command", args.ollama_command]
    if getattr(args, "submit_lean_command", None):
        preflight += ["--submit-lean-command", args.submit_lean_command]
    if getattr(args, "node_url", None):
        preflight += ["--node-url", args.node_url]
    if getattr(args, "use_node_ticket", False):
        preflight.append("--use-node-ticket")
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
    parser.add_argument("--model-benchmark-command", help="Override model benchmark runner command for local/fake Ollama tests; forwarded to phase7-solo-preflight.sh.")
    parser.add_argument("--ollama-command", help="Override Ollama command for local/fake benchmark attempts; forwarded without starting a daemon or pulling models.")
    parser.add_argument("--submit-lean-command", help="Override submit-lean verifier command for local/fake generated proof verification.")
    parser.add_argument("--node-url", help="Forward local node URL to optional controlled model benchmark /submit path.")
    parser.add_argument("--use-node-ticket", action="store_true", help="Forward local node /ticket observation before /submit in the optional controlled model benchmark path.")
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
        print(render_recovery_guidance(status))
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
