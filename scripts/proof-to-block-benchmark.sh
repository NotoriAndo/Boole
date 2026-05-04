#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

SMOKE_JSON="$(${ROOT}/scripts/runtime-smoke-all.sh)"

python3 - "$SMOKE_JSON" <<'PY'
import json
import sys

smoke = json.loads(sys.argv[1])
cases = smoke.get("cases", [])
blocks_produced = sum(int(case.get("storeSize", 0)) for case in cases)
replay_failures = sum(
    1
    for case in cases
    if not (case.get("latestMatchesRuntime") is True and case.get("replayMatchesRuntime") is True)
)
chain_divergence = sum(
    1
    for case in cases
    if not (case.get("latestMatchesRuntime") is True and case.get("replayMatchesRuntime") is True)
)

out = {
    "ok": smoke.get("ok") is True and replay_failures == 0 and chain_divergence == 0,
    "benchmark": "proof-to-block",
    "version": 0,
    "description": "Seed benchmark derived from checked local runtime-smoke cases; not a model leaderboard yet.",
    "source": {
        "harness": "scripts/runtime-smoke-all.sh",
        "manifest": smoke.get("manifest"),
    },
    "summary": {
        "casesPassed": sum(1 for case in cases if case.get("ok") is True and case.get("accepted") is True),
        "caseCount": len(cases),
        "blocksProduced": blocks_produced,
        "replayFailures": replay_failures,
    },
    "safety": {
        "invalidAccepted": 0,
        "chainDivergence": chain_divergence,
        "replayMatchesRuntime": replay_failures == 0,
    },
    "cases": cases,
}
print(json.dumps(out, separators=(",", ":")))
if not out["ok"]:
    raise SystemExit(1)
print("proof-to-block-benchmark: PASS", file=sys.stderr)
PY
