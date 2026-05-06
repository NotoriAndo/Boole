#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

MINER_ROOT="${BOOLE_MINER_ROOT:-$(cd "$ROOT/../pof/boole-miner" && pwd)}"
ADDR="${BOOLE_NODE_ADDR:-127.0.0.1:18120}"
SCENARIO="${SCENARIO:-fixtures/protocol/runtime-smoke/v1.json}"
BLOCK_STORE="${BLOCK_STORE:-${TMPDIR:-/tmp}/boole-node-provider-model-smoke.ndjson}"
TRIALS="${TRIALS:-1}"
LLM_BACKEND="${LLM_BACKEND:-mock}"
LLM_MODEL="${LLM_MODEL:-}"
LLM_BASE_URL="${LLM_BASE_URL:-}"
LLM_API_KEY_ENV="${LLM_API_KEY_ENV:-}"
LLM_PROVIDER_LABEL="${LLM_PROVIDER_LABEL:-$LLM_BACKEND}"
FIXED_SEED="${FIXED_SEED:-b606f7037936d8191ded73d7051fb423e72d2b442b0e868da9e3b11e72c7f764}"
FIXED_RENDER="${FIXED_RENDER:-Given xs : List Int with |xs| = 7 and 1 library distractors, apply [multiply each element by 2]; prove: the result equals (xs.filter (x is odd), xs.filter (not (x is odd))).}"
STATE_DIR="$(mktemp -d "${TMPDIR:-/tmp}/boole-provider-model-smoke.XXXXXX")"
STATE="$STATE_DIR/state.json"
RESULTS_JSONL="$STATE_DIR/results.jsonl"
rm -f "$BLOCK_STORE"

json_skip() {
  local reason="$1"
  python3 - "$reason" "$LLM_PROVIDER_LABEL" "$LLM_BACKEND" "$LLM_MODEL" <<'PY'
import json, sys
reason, provider, backend, model = sys.argv[1:5]
print(json.dumps({
    "ok": True,
    "kind": "boole-provider-model-smoke",
    "skipped": True,
    "reason": reason,
    "provider": provider,
    "backend": backend,
    "model": model,
}, separators=(",", ":")))
PY
}

case "$LLM_BACKEND" in
  mock|anthropic|openai|google|claude_cli|openai_compat) ;;
  *)
    printf 'boole-provider-model-smoke: unsupported LLM_BACKEND=%s\n' "$LLM_BACKEND" >&2
    exit 64
    ;;
esac

if [[ "$LLM_BACKEND" == "claude_cli" ]] && ! command -v claude >/dev/null 2>&1; then
  json_skip "claude_cli_not_found"
  rm -rf "$STATE_DIR"
  exit 0
fi

if [[ -n "$LLM_API_KEY_ENV" && -z "${!LLM_API_KEY_ENV:-}" ]]; then
  json_skip "missing_api_key_env"
  rm -rf "$STATE_DIR"
  exit 0
fi

if [[ "$LLM_BACKEND" == "openai_compat" ]]; then
  if [[ -z "$LLM_MODEL" || -z "$LLM_BASE_URL" ]]; then
    printf 'boole-provider-model-smoke: openai_compat requires LLM_MODEL and LLM_BASE_URL\n' >&2
    exit 64
  fi
  if [[ "$LLM_BASE_URL" == http://127.0.0.1:* || "$LLM_BASE_URL" == http://localhost:* ]]; then
    if ! curl -fsS "${LLM_BASE_URL%/}/models" >/dev/null 2>&1; then
      json_skip "openai_compat_endpoint_not_ready"
      rm -rf "$STATE_DIR"
      exit 0
    fi
  fi
fi

cargo run -q -p boole-node -- run-local \
  --addr "$ADDR" \
  --scenario "$SCENARIO" \
  --block-store "$BLOCK_STORE" \
  --max-requests 120 \
  >/tmp/boole-node-provider-model-smoke.out \
  2>/tmp/boole-node-provider-model-smoke.err &
PID=$!
trap 'kill "$PID" >/dev/null 2>&1 || true; rm -rf "$STATE_DIR"; rm -f /tmp/boole-node-provider-model-smoke.out /tmp/boole-node-provider-model-smoke.err /tmp/boole-provider-model-smoke-init.out /tmp/boole-provider-model-smoke-start.*.out' EXIT

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

init_args=(--state "$STATE" init --dispatcher-url "http://$ADDR" --llm-backend "$LLM_BACKEND" --force)
if [[ -n "$LLM_MODEL" ]]; then init_args+=(--llm-model "$LLM_MODEL"); fi
if [[ -n "$LLM_BASE_URL" ]]; then init_args+=(--llm-base-url "$LLM_BASE_URL"); fi
if [[ -n "$LLM_API_KEY_ENV" ]]; then init_args+=(--llm-api-key "${!LLM_API_KEY_ENV}"); fi
if [[ "$LLM_BACKEND" == "openai_compat" && -z "$LLM_API_KEY_ENV" ]]; then init_args+=(--llm-api-key sk-no-key); fi

(
  cd "$MINER_ROOT"
  npx tsx src/cli.ts "${init_args[@]}" >/tmp/boole-provider-model-smoke-init.out
)

success=0
for trial in $(seq 1 "$TRIALS"); do
  out="/tmp/boole-provider-model-smoke-start.${trial}.out"
  set +e
  (
    cd "$MINER_ROOT"
    npx tsx src/cli.ts --state "$STATE" start \
      --max-shares 1 \
      --max-cycles 1 \
      --profile v01 \
      --difficulty 0 \
      --fixed-target-seed-hex "$FIXED_SEED" \
      --fixed-target-render "$FIXED_RENDER" \
      >"$out"
  )
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
if not match:
    raise SystemExit(1)
s = json.loads(match.group(1))
raise SystemExit(0 if s.get("sharesAccepted") == 1 and s.get("verifyAccepted") == 1 and s.get("networkErrors") == 0 else 1)
PY
  then
    success=1
    break
  fi
done

python3 - "$ADDR" "$RESULTS_JSONL" "$success" "$LLM_PROVIDER_LABEL" "$LLM_BACKEND" "$LLM_MODEL" <<'PY'
import http.client, json, os, sys
addr, results_path, success_raw, provider, backend, model = sys.argv[1:7]
rows = [json.loads(line) for line in open(results_path)] if os.path.exists(results_path) else []
host, port_raw = addr.rsplit(":", 1)
conn = http.client.HTTPConnection(host, int(port_raw), timeout=2)
conn.request("GET", "/status")
res = conn.getresponse(); status = json.loads(res.read().decode())
verify_accepted = sum(1 for r in rows if (r.get("summary") or {}).get("verifyAccepted") == 1)
shares_accepted = sum(1 for r in rows if (r.get("summary") or {}).get("sharesAccepted") == 1)
ok = success_raw == "1" and status.get("height", 0) >= 1 and status.get("replayMatchesRuntime") is True
out = {
    "ok": ok,
    "kind": "boole-provider-model-smoke",
    "provider": provider,
    "backend": backend,
    "model": model,
    "miner": "boole-miner provider/model + real Lean verifier + LakeCanonicalizer/boole_emit POFP canon",
    "node": "boole-node run-local",
    "trials": len(rows),
    "aggregate": {"verifyAccepted": verify_accepted, "sharesAccepted": shares_accepted},
    "rows": rows,
    "status": status,
}
print(json.dumps(out, separators=(",", ":")))
if not ok:
    raise SystemExit("boole-provider-model-smoke: no proof-to-block success")
PY

kill "$PID" >/dev/null 2>&1 || true
wait "$PID" 2>/dev/null || true
printf 'boole-provider-model-smoke: PASS provider=%s model=%s\n' "$LLM_PROVIDER_LABEL" "$LLM_MODEL" >&2
