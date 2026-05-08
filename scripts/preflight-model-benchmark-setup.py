#!/usr/bin/env python3
"""Build Boole preflight provider/model benchmark specs.

This is intentionally a selector/generator, not an API-key manager. It only
checks credential presence and never prints secret values.
"""
from __future__ import annotations

import argparse
import json
import os
import shlex
import shutil
import subprocess
from dataclasses import dataclass
from typing import Any


@dataclass(frozen=True)
class ModelRow:
    name: str
    provider: str
    backend: str
    model: str = ""
    base_url: str = ""
    api_key_env: str = ""
    kind: str = "provider-model"
    timeout_sec: int = 900

    def to_benchmark_row(self, *, benchmark_command: str = "", ollama_command: str = "", submit_lean_command: str = "", node_url: str = "", artifact_root: str = "") -> dict[str, Any]:
        if self.backend == "mock":
            return {
                "name": self.name,
                "kind": self.kind,
                "command": ["./scripts/boole-miner-smoke.sh"],
                "timeoutSec": self.timeout_sec,
                "metadata": {
                    "provider": self.provider,
                    "backend": self.backend,
                    "model": self.model,
                    "credential": "none_or_oauth",
                },
            }

        if self.provider == "ollama-openai-compatible" and benchmark_command:
            row_name = f"ollama-{slug(self.model)}"
            artifact_dir = os.path.join(artifact_root or os.path.join("artifacts", "model-benchmarks"), row_name)
            command = shlex.split(benchmark_command) + [
                "--target",
                f"ollama:{self.model}",
                "--attempts",
                os.environ.get("TRIALS", "1"),
                "--output-dir",
                artifact_dir,
                "--run-id",
                row_name,
            ]
            if ollama_command:
                command.extend(["--ollama-command", ollama_command])
            if submit_lean_command:
                command.extend(["--submit-lean-command", submit_lean_command])
            if node_url:
                command.extend(["--node-url", node_url])
            return {
                "name": row_name,
                "kind": self.kind,
                "command": command,
                "timeoutSec": self.timeout_sec,
                "metadata": {
                    "provider": "ollama",
                    "backend": "ollama",
                    "model": self.model,
                    "credential": "none_local",
                },
            }

        env = {
            "LLM_PROVIDER_LABEL": self.provider,
            "LLM_BACKEND": self.backend,
            "TRIALS": os.environ.get("TRIALS", "1"),
        }
        if self.model:
            env["LLM_MODEL"] = self.model
        if self.base_url:
            env["LLM_BASE_URL"] = self.base_url
        if self.api_key_env:
            env["LLM_API_KEY_ENV"] = self.api_key_env
        row: dict[str, Any] = {
            "name": self.name,
            "kind": self.kind,
            "command": ["./scripts/provider-model-smoke.sh"],
            "timeoutSec": self.timeout_sec,
            "env": env,
            "metadata": {
                "provider": self.provider,
                "backend": self.backend,
                "model": self.model,
                "credential": self.api_key_env or "none_or_oauth",
            },
        }
        if self.api_key_env:
            row["requireEnv"] = [self.api_key_env]
        return row


def slug(value: str) -> str:
    out = []
    for ch in value.lower():
        if ch.isalnum():
            out.append(ch)
        elif ch in {"-", "_", ".", ":", "/"}:
            out.append("-")
    return "".join(out).strip("-").replace("--", "-") or "model"


def frontier_rows() -> list[ModelRow]:
    return [
        ModelRow("anthropic-claude-opus-4-7-api", "anthropic-api", "anthropic", "claude-opus-4-7", api_key_env="ANTHROPIC_API_KEY"),
        ModelRow("openai-gpt-5-api", "openai-api", "openai", "gpt-5", api_key_env="OPENAI_API_KEY"),
        ModelRow("google-gemini-2-5-pro-api", "google-api", "google", "gemini-2.5-pro", api_key_env="GOOGLE_API_KEY"),
        ModelRow("xai-grok-openai-compat-api", "xai-openai-compatible-api", "openai_compat", "grok-4", "https://api.x.ai/v1", "XAI_API_KEY"),
    ]


def oauth_rows() -> list[ModelRow]:
    rows = []
    if shutil.which("claude"):
        rows.append(ModelRow("claude-cli-oauth-default", "claude-cli-oauth", "claude_cli", "claude-cli-default", timeout_sec=600))
    else:
        rows.append(ModelRow("claude-cli-oauth-default", "claude-cli-oauth", "claude_cli", "claude-cli-default", timeout_sec=600))
    return rows


def ollama_models() -> list[str]:
    if not shutil.which("ollama"):
        return []
    try:
        proc = subprocess.run(["ollama", "list"], text=True, capture_output=True, timeout=10)
    except Exception:
        return []
    if proc.returncode != 0:
        return []
    names = []
    for line in proc.stdout.splitlines()[1:]:
        parts = line.split()
        if parts:
            names.append(parts[0])
    return names


def ollama_rows(models: list[str] | None = None) -> list[ModelRow]:
    selected = models if models is not None else ollama_models()
    return [
        ModelRow(
            name=f"ollama-{slug(model)}-openai-compat",
            provider="ollama-openai-compatible",
            backend="openai_compat",
            model=model,
            base_url=os.environ.get("OLLAMA_BASE_URL", "http://127.0.0.1:11434/v1"),
            timeout_sec=900,
        )
        for model in selected
    ]


def build_rows(preset: str, ollama_model_args: list[str]) -> list[ModelRow]:
    rows: list[ModelRow] = []
    if preset in {"frontier", "all"}:
        rows.extend(frontier_rows())
    if preset in {"oauth", "all"}:
        rows.extend(oauth_rows())
    if preset in {"ollama", "all"}:
        rows.extend(ollama_rows(ollama_model_args or None))
    if preset == "mock":
        rows.append(ModelRow("mock-model-transport", "mock", "mock", "mock", timeout_sec=300))
    return rows


def main() -> None:
    parser = argparse.ArgumentParser(description="Generate Boole provider/model benchmark specs for preflight.")
    parser.add_argument("--preset", choices=["mock", "frontier", "oauth", "ollama", "all"], default="all")
    parser.add_argument("--ollama-model", action="append", default=[], help="Specific Ollama model to include; repeatable. Defaults to all installed models.")
    parser.add_argument("--include", action="append", default=[], help="Only include rows whose name contains this substring; repeatable.")
    parser.add_argument("--output", help="Write benchmark spec JSON to this path.")
    parser.add_argument("--benchmark-command", default="", help="Override Ollama rows with this runner command, e.g. python3 scripts/boole-model-benchmark.py.")
    parser.add_argument("--ollama-command", default="", help="Ollama command override forwarded to benchmark-command Ollama rows.")
    parser.add_argument("--submit-lean-command", default="", help="submit-lean command override forwarded to benchmark-command Ollama rows.")
    parser.add_argument("--node-url", default="", help="Local node URL forwarded to benchmark-command Ollama rows for controlled node HTTP submit evidence.")
    parser.add_argument("--artifact-root", default="", help="Artifact root for benchmark-command per-model outputs.")
    parser.add_argument("--print-spec", action="store_true", help="Print the benchmark spec JSON array.")
    parser.add_argument("--list", action="store_true", help="Print a safe human-readable model list with credential presence only.")
    args = parser.parse_args()

    rows = build_rows(args.preset, args.ollama_model)
    if args.include:
        rows = [row for row in rows if any(term.lower() in row.name.lower() for term in args.include)]

    spec = [
        row.to_benchmark_row(
            benchmark_command=args.benchmark_command,
            ollama_command=args.ollama_command,
            submit_lean_command=args.submit_lean_command,
            node_url=args.node_url,
            artifact_root=args.artifact_root,
        )
        for row in rows
    ]

    if args.list:
        for row in rows:
            credential = row.api_key_env
            present = bool(os.environ.get(credential)) if credential else True
            print(f"{row.name}\tprovider={row.provider}\tbackend={row.backend}\tmodel={row.model}\tcredential={credential or 'none_or_oauth'}\tpresent={str(present).lower()}")

    if args.output:
        with open(args.output, "w", encoding="utf-8") as f:
            json.dump(spec, f, indent=2)
            f.write("\n")

    if args.print_spec or not args.output and not args.list:
        print(json.dumps(spec, separators=(",", ":")))


if __name__ == "__main__":
    main()
