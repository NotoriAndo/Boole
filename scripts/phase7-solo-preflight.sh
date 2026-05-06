#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

CONFIG="${PREFLIGHT_CONFIG:-fixtures/testnet/closed-preflight.v1.json}"
EVIDENCE_DIR="${PREFLIGHT_EVIDENCE_DIR:-}"
RUN_HERMES_REAL="${RUN_HERMES_REAL_PREFLIGHT:-0}"
RUN_MODEL_BENCHMARK="${RUN_MODEL_BENCHMARK_PREFLIGHT:-0}"
MODEL_BENCHMARK_PRESET="${MODEL_BENCHMARK_PRESET:-all}"
MODEL_BENCHMARK_ARGS=()
GENESIS_BENCHMARK="${GENESIS_BENCHMARK_PREFLIGHT:-0}"
ATTEMPTS_PER_MODEL="${ATTEMPTS_PER_MODEL:-}"

usage() {
  cat <<'EOF'
Usage: phase7-solo-preflight.sh [--config PATH] [--evidence-dir DIR] [--run-hermes-real] [--run-model-benchmark] [--model-preset mock|frontier|oauth|ollama|all] [--model-include TERM] [--ollama-model MODEL] [--genesis-benchmark] [--attempts-per-model N]

Runs the local Phase 7.0 solo preflight evidence gate and writes captured JSON,
stderr, and git metadata into an evidence directory. The summary JSON is printed
to stdout; progress goes to stderr. --genesis-benchmark uses a clean evidence
root and records reproducibility metadata for a genesis-reset benchmark run.
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
    --run-model-benchmark)
      RUN_MODEL_BENCHMARK=1
      shift
      ;;
    --model-preset)
      MODEL_BENCHMARK_PRESET="${2:?missing --model-preset value}"
      shift 2
      ;;
    --model-include)
      MODEL_BENCHMARK_ARGS+=(--include "${2:?missing --model-include value}")
      shift 2
      ;;
    --ollama-model)
      MODEL_BENCHMARK_ARGS+=(--ollama-model "${2:?missing --ollama-model value}")
      shift 2
      ;;
    --genesis-benchmark)
      GENESIS_BENCHMARK=1
      shift
      ;;
    --attempts-per-model)
      ATTEMPTS_PER_MODEL="${2:?missing --attempts-per-model value}"
      if ! [[ "$ATTEMPTS_PER_MODEL" =~ ^[1-9][0-9]*$ ]]; then
        printf 'phase7-solo-preflight: --attempts-per-model must be a positive integer\n' >&2
        exit 64
      fi
      shift 2
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
  if [[ "$GENESIS_BENCHMARK" == "1" ]]; then
    EVIDENCE_DIR="$ROOT/artifacts/preflight-genesis/$stamp"
  else
    EVIDENCE_DIR="$ROOT/artifacts/preflight/$stamp"
  fi
fi

if [[ "$GENESIS_BENCHMARK" == "1" ]]; then
  python3 - "$ROOT" "$EVIDENCE_DIR" <<'PY'
import pathlib
import shutil
import sys
root = pathlib.Path(sys.argv[1]).resolve()
evidence = pathlib.Path(sys.argv[2]).resolve()
safe_roots = [root / "artifacts", pathlib.Path("/tmp").resolve()]
if evidence == root or evidence == pathlib.Path("/"):
    raise SystemExit(f"refusing unsafe genesis benchmark reset path: {evidence}")
if not any(evidence == safe or safe in evidence.parents for safe in safe_roots):
    raise SystemExit(f"refusing genesis benchmark reset outside artifacts/ or /tmp: {evidence}")
if evidence.exists():
    shutil.rmtree(evidence)
evidence.mkdir(parents=True, exist_ok=False)
PY
else
  mkdir -p "$EVIDENCE_DIR"
fi

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

if [[ "$RUN_MODEL_BENCHMARK" == "1" ]]; then
  if [[ -n "$ATTEMPTS_PER_MODEL" ]]; then
    TRIALS="$ATTEMPTS_PER_MODEL" run_json_check provider-model-live-benchmark ./scripts/preflight-model-benchmark.sh \
      --preset "$MODEL_BENCHMARK_PRESET" \
      ${MODEL_BENCHMARK_ARGS[@]+"${MODEL_BENCHMARK_ARGS[@]}"} \
      --output-spec "$EVIDENCE_DIR/provider-model-live-spec.json" \
      --leaderboard-md "$EVIDENCE_DIR/provider-model-live-leaderboard.md" \
      --attempts-per-model "$ATTEMPTS_PER_MODEL"
  else
    run_json_check provider-model-live-benchmark ./scripts/preflight-model-benchmark.sh \
      --preset "$MODEL_BENCHMARK_PRESET" \
      ${MODEL_BENCHMARK_ARGS[@]+"${MODEL_BENCHMARK_ARGS[@]}"} \
      --output-spec "$EVIDENCE_DIR/provider-model-live-spec.json" \
      --leaderboard-md "$EVIDENCE_DIR/provider-model-live-leaderboard.md"
  fi
fi

python3 - "$EVIDENCE_DIR" "$CONFIG" "$RUN_HERMES_REAL" "$RUN_MODEL_BENCHMARK" "$GENESIS_BENCHMARK" "${ATTEMPTS_PER_MODEL:-}" <<'PY'
import json
import pathlib
import sys

evidence_dir = pathlib.Path(sys.argv[1])
config_path = sys.argv[2]
run_hermes_real = sys.argv[3] == "1"
run_model_benchmark = sys.argv[4] == "1"
genesis_benchmark = sys.argv[5] == "1"
attempts_per_model = int(sys.argv[6]) if sys.argv[6] else None

def load(name):
    return json.loads((evidence_dir / f"{name}.json").read_text())

def sha256_file(path):
    import hashlib
    p = pathlib.Path(path)
    if not p.is_absolute():
        p = pathlib.Path.cwd() / p
    return hashlib.sha256(p.read_bytes()).hexdigest()

def safe_hash(path):
    try:
        return sha256_file(path)
    except Exception:
        return None

def genesis_metadata():
    copied_config = evidence_dir / "config.json"
    config = json.loads(copied_config.read_text())
    scenario = config.get("scenario")
    runtime_cases = config.get("runtimeSmokeCases")
    verifier = config.get("verifier", {}) if isinstance(config.get("verifier"), dict) else {}
    safety = config.get("safetyInvariants", {}) if isinstance(config.get("safetyInvariants"), dict) else {}
    genesis_hash = "0000000000000000000000000000000000000000000000000000000000000000"
    return {
        "benchmark": "proof-to-block-genesis-preflight",
        "version": 1,
        "genesisMode": "reset",
        "genesisHash": genesis_hash,
        "chainId": config.get("chainId"),
        "configHash": sha256_file(copied_config),
        "scenario": scenario,
        "scenarioHash": safe_hash(scenario) if scenario else None,
        "runtimeSmokeCases": runtime_cases,
        "runtimeSmokeCasesHash": safe_hash(runtime_cases) if runtime_cases else None,
        "calibrationHash": sha256_file(copied_config),
        "verifierLabel": verifier.get("label"),
        "canonicalizer": verifier.get("canonicalizer"),
        "agentOutputTrust": verifier.get("agentOutputTrust"),
        "attemptsPerModel": attempts_per_model,
        "replayFromGenesis": True,
        "requiredSafety": {
            "invalidAccepted": safety.get("invalidAccepted", 0),
            "chainDivergence": safety.get("chainDivergence", 0),
            "replayRequired": safety.get("replayRequired", True),
        },
    }

def difficulty_summary_from_benchmark(benchmark):
    blocks = []
    for case in benchmark.get("cases", []):
        blocks.extend(case.get("blocks", []) or [])
    if not blocks:
        return None
    first = blocks[0]
    targets = {
        "tBlock": sorted({block.get("tBlock") for block in blocks if block.get("tBlock")}),
        "tShare": sorted({block.get("tShare") for block in blocks if block.get("tShare")}),
        "difficultyWeight": sorted({block.get("difficultyWeight") for block in blocks if block.get("difficultyWeight")}),
        "difficultyEpoch": sorted({block.get("difficultyEpoch") for block in blocks if block.get("difficultyEpoch") is not None}),
    }
    return {
        "mode": "static-calibrated",
        "retarget": "not-enabled",
        "blockCount": len(blocks),
        "difficultyEpoch": first.get("difficultyEpoch"),
        "tBlock": first.get("tBlock"),
        "tShare": first.get("tShare"),
        "difficultyWeight": first.get("difficultyWeight"),
        "uniqueTargets": targets,
    }

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

if run_model_benchmark:
    model_bench = load("provider-model-live-benchmark")
    checks.append({
        "name": "provider-model-live-benchmark",
        "ok": model_bench.get("ok") is True,
        "leaderboardMarkdown": model_bench.get("leaderboardMarkdown"),
        "rows": [
            {
                "name": row.get("name"),
                "status": row.get("status"),
                "ok": row.get("ok"),
                "skipped": row.get("skipped"),
                "score": row.get("score"),
                "metadata": row.get("metadata"),
            }
            for row in model_bench.get("rows", [])
        ],
    })

def check_ok(check):
    if check.get("ok") is not True:
        return False
    if "replayMatchesRuntime" in check and check.get("replayMatchesRuntime") is not True:
        return False
    if check.get("name") == "proof-to-block-benchmark":
        return check.get("invalidAccepted") == 0 and check.get("chainDivergence") == 0 and check.get("replayFailures") == 0
    return True

genesis = genesis_metadata() if genesis_benchmark else None
if genesis is not None:
    genesis.update({
        "replayPassed": summary.get("replayFailures") == 0,
        "invalidAccepted": safety.get("invalidAccepted"),
        "chainDivergence": safety.get("chainDivergence"),
        "blocksProduced": summary.get("blocksProduced"),
        "casesPassed": summary.get("casesPassed"),
        "caseCount": summary.get("caseCount"),
        "difficulty": difficulty_summary_from_benchmark(benchmark),
    })
    (evidence_dir / "genesis-benchmark.json").write_text(json.dumps(genesis, indent=2, sort_keys=True) + "\n")

out = {
    "ok": all(check_ok(check) for check in checks),
    "phase": "7.0-solo-preflight",
    "config": config_path,
    "evidenceDir": str(evidence_dir),
    "gitHead": (evidence_dir / "git-head.txt").read_text().strip(),
    "checks": checks,
}
if genesis is not None:
    out["genesisBenchmark"] = genesis
(evidence_dir / "summary.json").write_text(json.dumps(out, indent=2, sort_keys=True) + "\n")
print(json.dumps(out, separators=(",", ":")))
if not out["ok"]:
    raise SystemExit(1)
PY

printf 'phase7-solo-preflight: PASS evidence=%s\n' "$EVIDENCE_DIR" >&2
