#!/usr/bin/env bash
set -euo pipefail

ORIGINAL_ARGS=("$@")
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

ADDR="${BOOLE_NODE_ADDR:-127.0.0.1:18096}"
SCENARIO="${SCENARIO:-fixtures/protocol/runtime-smoke/v1.json}"
BLOCK_STORE="${BLOCK_STORE:-${TMPDIR:-/tmp}/boole-node-ollama-gemma-smoke.ndjson}"
REWARD_LEDGER="${REWARD_LEDGER:-${TMPDIR:-/tmp}/boole-node-ollama-gemma-smoke-rewards.ndjson}"
TRIALS="${TRIALS:-1}"
OLLAMA_BASE_URL="${OLLAMA_BASE_URL:-http://127.0.0.1:11434}"
OLLAMA_MODEL="${OLLAMA_MODEL:-gemma4:26b}"
PROFILE="${PROFILE:-v031-lp}"
FIXED_SEED="${FIXED_SEED:-b606f7037936d8191ded73d7051fb423e72d2b442b0e868da9e3b11e72c7f764}"
EVIDENCE_DIR="${BOOLE_OLLAMA_GEMMA_EVIDENCE_DIR:-}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --evidence-dir)
      EVIDENCE_DIR="${2:?missing --evidence-dir value}"
      shift 2
      ;;
    -h|--help)
      cat <<'EOF'
Usage: boole-miner-ollama-gemma-smoke.sh [--evidence-dir DIR]

Runs a local controlled Ollama Gemma smoke through boole-miner and boole-node.
This is mock-verifier local smoke evidence, not public mining evidence.
EOF
      exit 0
      ;;
    *)
      printf 'boole-miner-ollama-gemma-smoke: unknown argument: %s\n' "$1" >&2
      exit 64
      ;;
  esac
done

if [[ -n "$EVIDENCE_DIR" && -z "${BOOLE_OLLAMA_GEMMA_CAPTURED:-}" ]]; then
  mkdir -p "$EVIDENCE_DIR"
  set +e
  BOOLE_OLLAMA_GEMMA_CAPTURED=1 "$0" "${ORIGINAL_ARGS[@]}" >"$EVIDENCE_DIR/stdout.json" 2>"$EVIDENCE_DIR/stderr.txt"
  code=$?
  set -e
  python3 - "$EVIDENCE_DIR" "$code" <<'PY'
import datetime
import json
import pathlib
import sys

evidence_dir = pathlib.Path(sys.argv[1])
exit_code = int(sys.argv[2])
stdout_path = evidence_dir / "stdout.json"
stderr_path = evidence_dir / "stderr.txt"
raw_stdout = stdout_path.read_text(encoding="utf-8") if stdout_path.exists() else ""
raw_stderr = stderr_path.read_text(encoding="utf-8") if stderr_path.exists() else ""
try:
    result = json.loads(raw_stdout)
except json.JSONDecodeError:
    result = {"ok": False, "parseError": "stdout_not_json", "stdoutTail": raw_stdout[-1200:]}
summary = {
    "ok": exit_code == 0 and bool(result.get("ok")),
    "kind": "boole-miner-ollama-gemma-evidence",
    "schemaVersion": 1,
    "generatedAt": datetime.datetime.now(datetime.timezone.utc).isoformat().replace("+00:00", "Z"),
    "evidenceDir": "[REDACTED_LOCAL_PATH]",
    "claimBoundary": "local controlled-smoke UX artifact, not public mining evidence",
    "publicMiningEvidence": False,
    "paidApiBenchmark": False,
    "mockVerifier": True,
    "openThresholds": True,
    "redaction": {
        "localPaths": "redacted in summary only; raw stdout/stderr stay local under evidenceDir",
    },
    "result": result,
}
if exit_code != 0:
    summary["exitCode"] = exit_code
if raw_stderr.strip():
    summary["stderrTail"] = raw_stderr[-1200:]
(evidence_dir / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
  cat "$EVIDENCE_DIR/stdout.json"
  exit "$code"
fi

STATE_DIR="$(mktemp -d "${TMPDIR:-/tmp}/boole-miner-ollama-gemma-state.XXXXXX")"
STATE="$STATE_DIR/state.json"
RESULTS_JSONL="$STATE_DIR/results.jsonl"
rm -f "$BLOCK_STORE" "$REWARD_LEDGER"

command -v ollama >/dev/null 2>&1 || {
  printf 'boole-miner-ollama-gemma-smoke: SKIP ollama not found on PATH\n' >&2
  printf '{"ok":true,"kind":"boole-miner-ollama-gemma-smoke","skipped":true,"reason":"ollama_not_found","model":"%s"}\n' "$OLLAMA_MODEL"
  rm -rf "$STATE_DIR"
  exit 0
}

if ! curl -fsS "${OLLAMA_BASE_URL%/}/v1/models" >/dev/null 2>&1; then
  printf 'boole-miner-ollama-gemma-smoke: SKIP ollama OpenAI-compatible endpoint not ready\n' >&2
  printf '{"ok":true,"kind":"boole-miner-ollama-gemma-smoke","skipped":true,"reason":"ollama_endpoint_not_ready","model":"%s"}\n' "$OLLAMA_MODEL"
  rm -rf "$STATE_DIR"
  exit 0
fi

cargo run -q -p boole-node -- run-local \
  --addr "$ADDR" \
  --scenario "$SCENARIO" \
  --block-store "$BLOCK_STORE" \
  --reward-store "$REWARD_LEDGER" \
  --max-requests 80 \
  >/tmp/boole-node-ollama-gemma-smoke.out \
  2>/tmp/boole-node-ollama-gemma-smoke.err &
PID=$!
trap 'kill "$PID" >/dev/null 2>&1 || true; rm -rf "$STATE_DIR"; rm -f /tmp/boole-node-ollama-gemma-smoke.out /tmp/boole-node-ollama-gemma-smoke.err /tmp/boole-miner-ollama-gemma-smoke-init.out /tmp/boole-miner-ollama-gemma-smoke-start.*.out "$REWARD_LEDGER"' EXIT

python3 - "$ADDR" <<'PY'
import http.client, sys, time
host, port_raw = sys.argv[1].rsplit(":", 1)
last = None
for _ in range(80):
    try:
        conn = http.client.HTTPConnection(host, int(port_raw), timeout=1)
        conn.request("GET", "/head")
        res = conn.getresponse(); res.read()
        if res.status == 200:
            raise SystemExit(0)
    except OSError as err:
        last = err; time.sleep(0.05)
raise SystemExit(f"boole-node did not become ready: {last}")
PY

cargo run -q -p boole-miner -- init \
  --state "$STATE" \
  --dispatcher-url "http://$ADDR" \
  --llm-backend openai_compat \
  --llm-base-url "$OLLAMA_BASE_URL" \
  --llm-model "$OLLAMA_MODEL" \
  --llm-api-key sk-no-key \
  --force >/tmp/boole-miner-ollama-gemma-smoke-init.out

success=0
for trial in $(seq 1 "$TRIALS"); do
  out="/tmp/boole-miner-ollama-gemma-smoke-start.${trial}.out"
  set +e
  cargo run -q -p boole-miner -- start \
    --state "$STATE" \
    --max-shares 1 \
    --max-cycles 1 \
    --profile "$PROFILE" \
    --difficulty 0 \
    --fixed-target-seed-hex "$FIXED_SEED" \
    --mock-verify-accept \
    >"$out"
  code=$?
  set -e
  python3 - "$trial" "$code" "$out" >>"$RESULTS_JSONL" <<'PY'
import json, os, re, sys
trial = int(sys.argv[1]); code = int(sys.argv[2]); path = sys.argv[3]
raw = open(path).read() if os.path.exists(path) else ''
match = re.search(r"summary:\s*(\{[\s\S]*\})\s*$", raw)
summary = json.loads(match.group(1)) if match else None
print(json.dumps({"trial": trial, "exitCode": code, "summary": summary, "tail": raw[-1200:]}, separators=(",", ":")))
PY
  if python3 - "$out" <<'PY'
import json, re, sys
raw = open(sys.argv[1]).read()
match = re.search(r"summary:\s*(\{[\s\S]*\})\s*$", raw)
if not match: raise SystemExit(1)
s = json.loads(match.group(1))
raise SystemExit(0 if s.get("sharesAccepted") == 1 and s.get("verifyAccepted") == 1 and s.get("networkErrors") == 0 else 1)
PY
  then
    success=1
    break
  fi
done

python3 - "$ADDR" "$RESULTS_JSONL" "$success" "$OLLAMA_MODEL" <<'PY'
import http.client, json, os, sys
addr, results_path, success_raw, model = sys.argv[1:5]
rows = [json.loads(line) for line in open(results_path)] if os.path.exists(results_path) else []
host, port_raw = addr.rsplit(":", 1)
conn = http.client.HTTPConnection(host, int(port_raw), timeout=2)
conn.request("GET", "/status")
res = conn.getresponse(); status = json.loads(res.read().decode())
verify_accepted = sum(1 for r in rows if (r.get("summary") or {}).get("verifyAccepted") == 1)
shares_accepted = sum(1 for r in rows if (r.get("summary") or {}).get("sharesAccepted") == 1)
ok = success_raw == "1" and status.get("height", 0) >= 1 and status.get("replayMatchesRuntime") is True
out = {"ok": ok, "kind": "boole-miner-ollama-gemma-smoke", "provider": "ollama-openai-compatible", "model": model, "miner": "boole-miner openai_compat + FamilyV031TargetEmitter + StructuralCanonicalizer (placeholder POFP canon) + AcceptingVerifier (--mock-verify-accept)", "node": "boole-node run-local", "trials": len(rows), "aggregate": {"verifyAccepted": verify_accepted, "sharesAccepted": shares_accepted}, "rows": rows, "status": status}
print(json.dumps(out, separators=(",", ":")))
if not ok:
    raise SystemExit("boole-miner-ollama-gemma-smoke: no proof-to-block success")
PY

kill "$PID" >/dev/null 2>&1 || true
wait "$PID" 2>/dev/null || true
printf 'boole-miner-ollama-gemma-smoke: PASS\n' >&2
