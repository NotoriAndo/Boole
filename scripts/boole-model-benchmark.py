#!/usr/bin/env python3
"""Create Boole model Proof-to-Block benchmark artifact bundles.

This is a non-consensus runner skeleton. It executes command-based benchmark
rows, records pass/skip/fail rows, and writes stable artifacts that later live
Ollama/frontier runners can fill with real proof-attempt data.
"""
from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]


def now_ms() -> int:
    return int(time.time() * 1000)


def default_run_id() -> str:
    return time.strftime("%Y%m%dT%H%M%SZ", time.gmtime())


def load_spec(path: Path) -> list[dict[str, Any]]:
    parsed = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(parsed, list):
        raise SystemExit("benchmark spec must be a JSON array")
    for row in parsed:
        if not isinstance(row, dict):
            raise SystemExit("benchmark spec rows must be JSON objects")
    return parsed


def parse_json_output(stdout: str) -> dict[str, Any] | None:
    candidates: list[str] = []
    for line in stdout.splitlines():
        stripped = line.strip()
        if stripped.startswith("{") and stripped.endswith("}"):
            candidates.append(stripped)
    if candidates:
        return json.loads(candidates[-1])
    match = re.search(r"(\{[\s\S]*\})\s*$", stdout.strip())
    if match:
        return json.loads(match.group(1))
    return None


def missing_env(row: dict[str, Any]) -> list[str]:
    return [name for name in row.get("requireEnv", []) if isinstance(name, str) and not os.environ.get(name)]


def score_from_result(parsed: dict[str, Any] | None) -> dict[str, Any]:
    data = parsed or {}
    summary = data.get("summary") or {}
    status = data.get("status") or {}
    aggregate = data.get("aggregate") or {}
    safety = data.get("safety") or {}
    blocks = int(
        status.get("height")
        or summary.get("blocksMined")
        or summary.get("blocksProduced")
        or aggregate.get("blocksProduced")
        or 0
    )
    verified = int(
        summary.get("verifyAccepted")
        or aggregate.get("verifyAccepted")
        or summary.get("sharesAccepted")
        or aggregate.get("sharesAccepted")
        or 0
    )
    replay_pass = bool(
        status.get("replayMatchesRuntime")
        or data.get("replayMatchesRuntime")
        or safety.get("replayMatchesRuntime")
        or False
    )
    return {"blocks": blocks, "verifiedShares": verified, "replayPass": replay_pass}


def safety_from_result(parsed: dict[str, Any] | None) -> dict[str, int]:
    safety = (parsed or {}).get("safety") or {}
    summary = (parsed or {}).get("summary") or {}
    return {
        "invalidAccepted": int(safety.get("invalidAccepted") or summary.get("invalidAccepted") or 0),
        "chainDivergence": int(safety.get("chainDivergence") or summary.get("chainDivergence") or 0),
        "replayFailures": int(safety.get("replayFailures") or summary.get("replayFailures") or 0),
    }


def validate_row(row: dict[str, Any]) -> tuple[str, list[str]]:
    name = row.get("name")
    command = row.get("command")
    if not isinstance(name, str) or not name:
        raise SystemExit(f"benchmark row missing name: {row}")
    if not isinstance(command, list) or not command or not all(isinstance(part, str) for part in command):
        raise SystemExit(f"benchmark row {name} command must be a non-empty string array")
    return name, command


def run_row(row: dict[str, Any], timeout_s: int) -> dict[str, Any]:
    name, command = validate_row(row)
    required_missing = missing_env(row)
    base = {
        "name": name,
        "kind": row.get("kind"),
        "metadata": row.get("metadata"),
    }
    if required_missing:
        return {
            **base,
            "ok": True,
            "skipped": True,
            "status": "SKIP",
            "reason": "missing_required_env",
            "missingEnv": required_missing,
            "elapsedMs": 0,
            "score": {"blocks": 0, "verifiedShares": 0, "replayPass": False},
            "safety": {"invalidAccepted": 0, "chainDivergence": 0, "replayFailures": 0},
        }

    env = os.environ.copy()
    for key, value in (row.get("env") or {}).items():
        if not isinstance(key, str) or not isinstance(value, str):
            raise SystemExit(f"benchmark row {name} env values must be strings")
        env[key] = value

    started = time.time()
    proc = subprocess.run(
        command,
        cwd=ROOT,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=int(row.get("timeoutSec", timeout_s)),
        check=False,
    )
    elapsed_ms = int((time.time() - started) * 1000)
    parsed = parse_json_output(proc.stdout) if proc.stdout else None
    row_ok = proc.returncode == 0 and parsed is not None and parsed.get("ok") is True
    skipped = bool((parsed or {}).get("skipped"))
    return {
        **base,
        "ok": row_ok,
        "skipped": skipped,
        "status": "SKIP" if skipped else ("PASS" if row_ok else "FAIL"),
        "exitCode": proc.returncode,
        "elapsedMs": elapsed_ms,
        "score": score_from_result(parsed),
        "safety": safety_from_result(parsed),
        "result": parsed,
        "stderrTail": proc.stderr[-1200:],
        "stdoutTail": proc.stdout[-1200:],
    }


def leaderboard_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return sorted(
        rows,
        key=lambda row: (
            0 if row.get("skipped") else 1,
            1 if row.get("ok") else 0,
            int(row.get("score", {}).get("blocks", 0)),
            int(row.get("score", {}).get("verifiedShares", 0)),
            -int(row.get("elapsedMs", 0)),
        ),
        reverse=True,
    )


def render_leaderboard(summary: dict[str, Any], rows: list[dict[str, Any]]) -> str:
    lines = [
        "# Boole Model Proof-to-Block Benchmark",
        "",
        f"- runId: `{summary['runId']}`",
        f"- ok: `{str(summary['ok']).lower()}`",
        f"- verifiedShares: `{summary['totals']['verifiedShares']}`",
        f"- blocksProduced: `{summary['totals']['blocksProduced']}`",
        f"- replayPassed: `{str(summary['replayPassed']).lower()}`",
        f"- invalidAccepted: `{summary['safety']['invalidAccepted']}`",
        "",
        "## Leaderboard",
        "",
    ]
    for idx, row in enumerate(rows, start=1):
        score = row.get("score", {})
        lines.extend(
            [
                f"### {idx}. {row['name']}",
                f"- status: `{row.get('status')}`",
                f"- blocks: `{score.get('blocks', 0)}`",
                f"- verifiedShares: `{score.get('verifiedShares', 0)}`",
                f"- replayPass: `{str(score.get('replayPass') is True).lower()}`",
                f"- elapsedMs: `{row.get('elapsedMs', 0)}`",
                "",
            ]
        )
    return "\n".join(lines)


def summarize(rows: list[dict[str, Any]], run_id: str, generated_at_ms: int) -> dict[str, Any]:
    safety = {
        "invalidAccepted": sum(int(row.get("safety", {}).get("invalidAccepted", 0)) for row in rows),
        "chainDivergence": sum(int(row.get("safety", {}).get("chainDivergence", 0)) for row in rows),
        "replayFailures": sum(int(row.get("safety", {}).get("replayFailures", 0)) for row in rows),
    }
    active_rows = [row for row in rows if not row.get("skipped")]
    replay_passed = bool(active_rows) and all(row.get("score", {}).get("replayPass") is True for row in active_rows)
    ok = all(row.get("ok") is True for row in rows) and safety == {"invalidAccepted": 0, "chainDivergence": 0, "replayFailures": 0}
    return {
        "ok": ok,
        "benchmark": "boole-model-proof-to-block",
        "version": 0,
        "runId": run_id,
        "generatedAtUnixMs": generated_at_ms,
        "totals": {
            "rows": len(rows),
            "passed": sum(1 for row in rows if row.get("status") == "PASS"),
            "skipped": sum(1 for row in rows if row.get("status") == "SKIP"),
            "failed": sum(1 for row in rows if row.get("status") == "FAIL"),
            "blocksProduced": sum(int(row.get("score", {}).get("blocks", 0)) for row in rows),
            "verifiedShares": sum(int(row.get("score", {}).get("verifiedShares", 0)) for row in rows),
        },
        "safety": safety,
        "replayPassed": replay_passed,
        "artifacts": {
            "summary": "benchmark-summary.json",
            "rows": "benchmark-rows.ndjson",
            "leaderboard": "leaderboard.md",
            "replayReport": "replay-report.json",
        },
    }


def write_artifacts(output_dir: Path, summary: dict[str, Any], rows: list[dict[str, Any]]) -> None:
    output_dir.mkdir(parents=True, exist_ok=True)
    ordered = leaderboard_rows(rows)
    (output_dir / "benchmark-summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    with (output_dir / "benchmark-rows.ndjson").open("w", encoding="utf-8") as f:
        for row in rows:
            f.write(json.dumps(row, separators=(",", ":")) + "\n")
    replay_report = {
        "runId": summary["runId"],
        "replayPassed": summary["replayPassed"],
        "replayFailures": summary["safety"]["replayFailures"],
        "chainDivergence": summary["safety"]["chainDivergence"],
        "rows": [
            {"name": row["name"], "status": row.get("status"), "replayPass": row.get("score", {}).get("replayPass")}
            for row in rows
        ],
    }
    (output_dir / "replay-report.json").write_text(json.dumps(replay_report, indent=2) + "\n", encoding="utf-8")
    (output_dir / "leaderboard.md").write_text(render_leaderboard(summary, ordered), encoding="utf-8")


def run_benchmark(*, spec_path: Path, output_dir: Path, run_id: str | None = None, timeout_s: int = 300) -> dict[str, Any]:
    run_id = run_id or default_run_id()
    generated_at_ms = now_ms()
    rows = [run_row(row, timeout_s=timeout_s) for row in load_spec(spec_path)]
    summary = summarize(rows, run_id, generated_at_ms)
    write_artifacts(output_dir, summary, rows)
    return {"ok": summary["ok"], "runId": run_id, "artifactDir": str(output_dir), "summary": summary}


def main(argv: list[str] | None = None) -> None:
    parser = argparse.ArgumentParser(description="Run Boole model Proof-to-Block benchmark rows and write artifacts.")
    parser.add_argument("--spec", required=True, help="Benchmark row spec JSON array path.")
    parser.add_argument("--output-dir", help="Artifact output directory. Defaults to artifacts/model-benchmarks/<run-id>.")
    parser.add_argument("--run-id", help="Stable run id for reproducible tests/evidence.")
    parser.add_argument("--timeout-sec", type=int, default=300)
    args = parser.parse_args(argv)

    run_id = args.run_id or default_run_id()
    output_dir = Path(args.output_dir) if args.output_dir else ROOT / "artifacts" / "model-benchmarks" / run_id
    result = run_benchmark(spec_path=Path(args.spec), output_dir=output_dir, run_id=run_id, timeout_s=args.timeout_sec)
    print(json.dumps(result, separators=(",", ":")))
    if not result["ok"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
