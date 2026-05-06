#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

CONFIG="${PREFLIGHT_CONFIG:-fixtures/testnet/closed-preflight.v1.json}"
EVIDENCE_DIR="${PREFLIGHT_EVIDENCE_DIR:-}"
RUN_HERMES_REAL="${RUN_HERMES_REAL_PREFLIGHT:-0}"

usage() {
  cat <<'EOF'
Usage: phase7-solo-preflight.sh [--config PATH] [--evidence-dir DIR] [--run-hermes-real]

Runs the local Phase 7.0 solo preflight evidence gate and writes captured JSON,
stderr, and git metadata into an evidence directory. The summary JSON is printed
to stdout; progress goes to stderr.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --config)
      CONFIG="${2:?missing --config value}"
      shift 2
      ;;
    --evidence-dir)
      EVIDENCE_DIR="${2:?missing --evidence-dir value}"
      shift 2
      ;;
    --run-hermes-real)
      RUN_HERMES_REAL=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      printf 'phase7-solo-preflight: unknown argument: %s\n' "$1" >&2
      usage >&2
      exit 64
      ;;
  esac
done

if [[ ! -f "$CONFIG" ]]; then
  printf 'phase7-solo-preflight: missing config %s\n' "$CONFIG" >&2
  exit 66
fi

if [[ -z "$EVIDENCE_DIR" ]]; then
  stamp="$(date -u +%Y%m%dT%H%M%SZ)"
  EVIDENCE_DIR="$ROOT/artifacts/preflight/$stamp"
fi
mkdir -p "$EVIDENCE_DIR"

CONFIG_ABS="$(python3 - "$CONFIG" <<'PY'
from pathlib import Path
import sys
print(Path(sys.argv[1]).resolve())
PY
)"
cp "$CONFIG_ABS" "$EVIDENCE_DIR/config.json"

git rev-parse HEAD > "$EVIDENCE_DIR/git-head.txt"
git status --short --branch --untracked-files=all > "$EVIDENCE_DIR/git-status.txt"
git log --oneline -5 > "$EVIDENCE_DIR/git-log.txt"

run_json_check() {
  local name="$1"
  shift
  local out="$EVIDENCE_DIR/${name}.json"
  local err="$EVIDENCE_DIR/${name}.stderr.txt"
  printf 'phase7 preflight check %s: RUN\n' "$name" >&2
  if "$@" >"$out" 2>"$err"; then
    cat "$err" >&2
    printf 'phase7 preflight check %s: PASS\n' "$name" >&2
  else
    local status=$?
    printf 'phase7 preflight check %s: FAIL\n' "$name" >&2
    cat "$err" >&2 || true
    cat "$out" >&2 || true
    return "$status"
  fi
  python3 - "$out" "$name" <<'PY'
import json, sys
path, name = sys.argv[1:3]
try:
    data = json.load(open(path))
except Exception as err:
    raise SystemExit(f"{name}: output is not valid JSON: {err}")
if data.get("ok") is not True:
    raise SystemExit(f"{name}: ok is not true")
PY
}

run_json_check runtime-smoke-all ./scripts/runtime-smoke-all.sh
run_json_check proof-to-block-benchmark ./scripts/proof-to-block-benchmark.sh
run_json_check local-mining-smoke ./scripts/local-mining-smoke.sh
run_json_check boole-agent-mine-fake ./scripts/boole-agent-mine.sh --runtime fake
run_json_check boole-agent-mine-hermes-mock ./scripts/boole-agent-mine.sh --runtime hermes --verify mock
LEADERBOARD_MD="$EVIDENCE_DIR/agent-runtime-leaderboard.md" run_json_check agent-runtime-benchmark ./scripts/agent-runtime-benchmark.sh

if [[ "$RUN_HERMES_REAL" == "1" ]]; then
  TRIALS="${HERMES_REAL_PREFLIGHT_TRIALS:-1}" run_json_check boole-agent-mine-hermes-real ./scripts/boole-agent-mine.sh --runtime hermes --verify real
fi

python3 - "$EVIDENCE_DIR" "$CONFIG" "$RUN_HERMES_REAL" <<'PY'
import json
import pathlib
import sys

evidence_dir = pathlib.Path(sys.argv[1])
config_path = sys.argv[2]
run_hermes_real = sys.argv[3] == "1"

def load(name):
    return json.loads((evidence_dir / f"{name}.json").read_text())

runtime = load("runtime-smoke-all")
benchmark = load("proof-to-block-benchmark")
mining = load("local-mining-smoke")
fake = load("boole-agent-mine-fake")
hermes = load("boole-agent-mine-hermes-mock")
agent_bench = load("agent-runtime-benchmark")

summary = benchmark.get("summary", {})
safety = benchmark.get("safety", {})
checks = [
    {
        "name": "runtime-smoke-all",
        "ok": runtime.get("ok") is True,
        "caseCount": runtime.get("caseCount"),
        "casesPassed": sum(1 for case in runtime.get("cases", []) if case.get("ok") is True and case.get("accepted") is True),
        "replayFailures": sum(1 for case in runtime.get("cases", []) if case.get("replayMatchesRuntime") is not True),
    },
    {
        "name": "proof-to-block-benchmark",
        "ok": benchmark.get("ok") is True,
        "casesPassed": summary.get("casesPassed"),
        "blocksProduced": summary.get("blocksProduced"),
        "replayFailures": summary.get("replayFailures"),
        "invalidAccepted": safety.get("invalidAccepted"),
        "chainDivergence": safety.get("chainDivergence"),
    },
    {
        "name": "local-mining-smoke",
        "ok": mining.get("ok") is True,
        "blocksMined": mining.get("blocksMined"),
        "finalHeight": mining.get("finalHead", {}).get("height"),
    },
    {
        "name": "boole-agent-mine-fake",
        "ok": fake.get("ok") is True,
        "height": fake.get("status", {}).get("height"),
        "replayMatchesRuntime": fake.get("status", {}).get("replayMatchesRuntime"),
    },
    {
        "name": "boole-agent-mine-hermes-mock",
        "ok": hermes.get("ok") is True,
        "height": hermes.get("status", {}).get("height"),
        "replayMatchesRuntime": hermes.get("status", {}).get("replayMatchesRuntime"),
    },
    {
        "name": "agent-runtime-benchmark",
        "ok": agent_bench.get("ok") is True,
        "rows": [
            {"name": row.get("name"), "status": row.get("status"), "ok": row.get("ok"), "score": row.get("score")}
            for row in agent_bench.get("rows", [])
        ],
    },
]

if run_hermes_real:
    real = load("boole-agent-mine-hermes-real")
    checks.append({
        "name": "boole-agent-mine-hermes-real",
        "ok": real.get("ok") is True,
        "aggregate": real.get("aggregate"),
        "height": real.get("status", {}).get("height"),
        "replayMatchesRuntime": real.get("status", {}).get("replayMatchesRuntime"),
    })

def check_ok(check):
    if check.get("ok") is not True:
        return False
    if "replayMatchesRuntime" in check and check.get("replayMatchesRuntime") is not True:
        return False
    if check.get("name") == "proof-to-block-benchmark":
        return check.get("invalidAccepted") == 0 and check.get("chainDivergence") == 0 and check.get("replayFailures") == 0
    return True

out = {
    "ok": all(check_ok(check) for check in checks),
    "phase": "7.0-solo-preflight",
    "config": config_path,
    "evidenceDir": str(evidence_dir),
    "gitHead": (evidence_dir / "git-head.txt").read_text().strip(),
    "checks": checks,
}
(evidence_dir / "summary.json").write_text(json.dumps(out, indent=2, sort_keys=True) + "\n")
print(json.dumps(out, separators=(",", ":")))
if not out["ok"]:
    raise SystemExit(1)
PY

printf 'phase7-solo-preflight: PASS evidence=%s\n' "$EVIDENCE_DIR" >&2
