#!/usr/bin/env python3
"""Run Boole proof-to-block benchmark rows and emit JSON + optional Markdown.

Rows are intentionally command-based so provider/model and agent-runtime
benchmarks stay outside consensus code. Each row's command must print a JSON
object. Secrets are never printed; env values in row config are only presence
checks / explicit safe values supplied by the caller.
"""
from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path
from typing import Any


def load_spec(env_name: str, default: list[dict[str, Any]]) -> list[dict[str, Any]]:
    raw = os.environ.get(env_name)
    if not raw:
        return default
    parsed = json.loads(raw)
    if not isinstance(parsed, list):
        raise SystemExit(f"{env_name} must be a JSON array")
    return parsed


def parse_json_output(stdout: str) -> dict[str, Any] | None:
    candidates = []
    for line in stdout.splitlines():
        s = line.strip()
        if s.startswith("{") and s.endswith("}"):
            candidates.append(s)
    if candidates:
        return json.loads(candidates[-1])
    # Fallback for pretty JSON or logs containing a final object.
    match = re.search(r"(\{[\s\S]*\})\s*$", stdout.strip())
    if match:
        return json.loads(match.group(1))
    return None


def require_env(row: dict[str, Any]) -> str | None:
    missing = [name for name in row.get("requireEnv", []) if not os.environ.get(name)]
    return ",".join(missing) if missing else None


def run_row(row: dict[str, Any], timeout_s: int) -> dict[str, Any]:
    name = row.get("name")
    if not isinstance(name, str) or not name:
        raise SystemExit(f"benchmark row missing name: {row}")
    command = row.get("command")
    if not isinstance(command, list) or not command or not all(isinstance(x, str) for x in command):
        raise SystemExit(f"benchmark row {name} command must be string array")

    missing = require_env(row)
    if missing:
        return {
            "name": name,
            "kind": row.get("kind"),
            "metadata": row.get("metadata"),
            "ok": True,
            "skipped": True,
            "status": "SKIP",
            "reason": "missing_required_env",
            "missingEnv": missing,
            "elapsedMs": 0,
            "score": {"blocksProduced": 0, "replayPass": False},
            "diagnostics": {"verifiedShares": 0},
        }

    env = os.environ.copy()
    for key, value in row.get("env", {}).items():
        if not isinstance(key, str) or not isinstance(value, str):
            raise SystemExit(f"benchmark row {name} env values must be strings")
        env[key] = value

    started = time.time()
    proc = subprocess.run(
        command,
        cwd=Path(__file__).resolve().parents[1],
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=int(row.get("timeoutSec", timeout_s)),
    )
    elapsed_ms = int((time.time() - started) * 1000)
    parsed = parse_json_output(proc.stdout) if proc.stdout else None

    summary = (parsed or {}).get("summary") or {}
    status = (parsed or {}).get("status") or {}
    aggregate = (parsed or {}).get("aggregate") or {}
    blocks = int(status.get("height") or summary.get("blocksMined") or summary.get("blocksProduced") or aggregate.get("blocksProduced") or 0)
    verified = int(summary.get("verifyAccepted") or aggregate.get("verifyAccepted") or summary.get("sharesAccepted") or aggregate.get("sharesAccepted") or 0)
    replay_pass = bool(status.get("replayMatchesRuntime") or (parsed or {}).get("replayMatchesRuntime") or False)
    row_ok = proc.returncode == 0 and parsed is not None and (parsed.get("ok") is True)
    skipped = bool((parsed or {}).get("skipped"))

    return {
        "name": name,
        "kind": row.get("kind"),
        "metadata": row.get("metadata"),
        "ok": row_ok,
        "skipped": skipped,
        "status": "SKIP" if skipped else ("PASS" if row_ok else "FAIL"),
        "exitCode": proc.returncode,
        "elapsedMs": elapsed_ms,
        "score": {"blocksProduced": blocks, "replayPass": replay_pass},
        "diagnostics": {"verifiedShares": verified},
        "result": parsed,
        "stderrTail": proc.stderr[-1200:],
        "stdoutTail": proc.stdout[-1200:],
    }


def block_production_rate_pct(*, blocks_produced: int, generated_attempts: int) -> float:
    if generated_attempts <= 0:
        return 0.0
    return round((blocks_produced / generated_attempts) * 100.0, 2)


def write_markdown(path: str, benchmark: dict[str, Any]) -> None:
    rows = benchmark["leaderboard"]
    lines = [
        f"# {benchmark['title']}",
        "",
        f"- kind: `{benchmark['kind']}`",
        f"- ok: `{str(benchmark['ok']).lower()}`",
        f"- generatedAtUnixMs: `{benchmark['generatedAtUnixMs']}`",
        f"- blockProductionRate: `{benchmark['totals']['blocksProduced']}/{benchmark['totals']['generatedAttempts']} ({benchmark['totals']['blockProductionRatePct']:.2f}%)`",
        "",
        "## Leaderboard",
        "",
    ]
    for idx, row in enumerate(rows, start=1):
        score = row.get("score", {})
        status = "SKIP" if row.get("skipped") else ("PASS" if row.get("ok") else "FAIL")
        lines.extend([
            f"### {idx}. {row['name']}",
            f"- status: `{status}`",
            f"- blocksProduced: `{score.get('blocksProduced', 0)}`",
            f"- blockProduced: `{str(int(score.get('blocksProduced', 0)) > 0).lower()}`",
            f"- replayPass: `{str(score.get('replayPass') is True).lower()}`",
            f"- elapsedMs: `{row.get('elapsedMs', 0)}`",
            "",
        ])
    Path(path).write_text("\n".join(lines), encoding="utf-8")


def main() -> None:
    kind = os.environ.get("BENCHMARK_KIND", "proof-to-block")
    title = os.environ.get("BENCHMARK_TITLE", "Boole Proof-to-Block Benchmark")
    spec_env = os.environ.get("BENCHMARK_SPEC_ENV", "BENCHMARK_SPEC")
    default_spec = json.loads(os.environ.get("BENCHMARK_DEFAULT_SPEC", "[]"))
    timeout_s = int(os.environ.get("BENCHMARK_TIMEOUT_SEC", "300"))

    rows = [run_row(row, timeout_s) for row in load_spec(spec_env, default_spec)]
    leaderboard = sorted(
        rows,
        key=lambda r: (
            0 if r.get("skipped") else 1,
            1 if r.get("ok") else 0,
            int(r.get("score", {}).get("blocksProduced", 0)),
            -int(r.get("elapsedMs", 0)),
        ),
        reverse=True,
    )
    generated_attempts = sum(1 for row in rows if not row.get("skipped"))
    blocks_produced = sum(int(row.get("score", {}).get("blocksProduced", 0)) for row in rows)
    out = {
        "ok": all(row.get("ok") is True for row in rows),
        "kind": kind,
        "title": title,
        "generatedAtUnixMs": int(time.time() * 1000),
        "totals": {
            "generatedAttempts": generated_attempts,
            "blocksProduced": blocks_produced,
            "blockProductionRatePct": block_production_rate_pct(blocks_produced=blocks_produced, generated_attempts=generated_attempts),
        },
        "publicScore": {
            "primaryMetric": "blockProductionRatePct",
            "formula": "blocksProduced / generatedAttempts * 100",
        },
        "rows": rows,
        "leaderboard": leaderboard,
    }
    md_path = os.environ.get("LEADERBOARD_MD")
    if md_path:
        write_markdown(md_path, out)
        out["leaderboardMarkdown"] = md_path
    print(json.dumps(out, separators=(",", ":")))
    if not out["ok"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
