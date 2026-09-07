#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

source "$ROOT/scripts/smoke-lifecycle.sh"
ADDR="${BOOLE_NODE_ADDR:-127.0.0.1:$((20000 + ($$ % 20000)))}"
SCENARIO="${SCENARIO:-fixtures/protocol/runtime-smoke/v1.json}"
BLOCK_STORE="$(smoke_fresh_path "${BLOCK_STORE:-$SMOKE_WORK_DIR/blocks.ndjson}")"
REWARD_LEDGER="$(smoke_fresh_path "${REWARD_LEDGER:-$SMOKE_WORK_DIR/rewards.ndjson}")"
TRIALS="${TRIALS:-1}"
LLM_BACKEND="${LLM_BACKEND:-mock}"
LLM_MODEL="${LLM_MODEL:-}"
LLM_BASE_URL="${LLM_BASE_URL:-}"
LLM_API_KEY_ENV="${LLM_API_KEY_ENV:-}"
LLM_PROVIDER_LABEL="${LLM_PROVIDER_LABEL:-$LLM_BACKEND}"
PROFILE="${PROFILE:-v1-lenbound}"
FIXED_SEED="${FIXED_SEED:-b606f7037936d8191ded73d7051fb423e72d2b442b0e868da9e3b11e72c7f764}"
STATE_DIR="$SMOKE_WORK_DIR/state"
STATE="$STATE_DIR/state.json"
RESULTS_JSONL="$STATE_DIR/results.jsonl"
mkdir -p "$STATE_DIR"
MINER_INIT_OUT="$SMOKE_WORK_DIR/miner-init.out"

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
  exit 0
fi

if [[ -n "$LLM_API_KEY_ENV" && -z "${!LLM_API_KEY_ENV:-}" ]]; then
  json_skip "missing_api_key_env"
  exit 0
fi

if [[ "$LLM_BACKEND" == "openai_compat" ]]; then
  if [[ -z "$LLM_MODEL" || -z "$LLM_BASE_URL" ]]; then
    printf 'boole-provider-model-smoke: openai_compat requires LLM_MODEL and LLM_BASE_URL\n' >&2
    exit 64
  fi
  if [[ "$LLM_BASE_URL" == http://127.0.0.1:* || "$LLM_BASE_URL" == http://localhost:* ]]; then
    if ! curl --connect-timeout 3 --max-time 10 -fsS "${LLM_BASE_URL%/}/models" >/dev/null 2>&1; then
      json_skip "openai_compat_endpoint_not_ready"
      exit 0
    fi
  fi
fi

# Build before the node readiness budget starts.
NODE_BIN="$(smoke_build_binary boole-node boole-node)"
MINER_BIN="$(smoke_build_binary boole-miner boole-miner --features boole-miner/dev-tools)"
smoke_prewarm_binary "$NODE_BIN"
smoke_prewarm_binary "$MINER_BIN"
"$NODE_BIN" run-local \
  --addr "$ADDR" \
  --scenario "$SCENARIO" \
  --block-store "$BLOCK_STORE" \
  --reward-store "$REWARD_LEDGER" \
  --allow-anonymous-submit \
  >"$SMOKE_WORK_DIR/node.out" \
  2>"$SMOKE_WORK_DIR/node.err" &
PID=$!
smoke_register_child "$PID"

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

init_args=(init --state "$STATE" --dispatcher-url "http://$ADDR" --llm-backend "$LLM_BACKEND" --force)
if [[ -n "$LLM_MODEL" ]]; then init_args+=(--llm-model "$LLM_MODEL"); fi
if [[ -n "$LLM_BASE_URL" ]]; then init_args+=(--llm-base-url "$LLM_BASE_URL"); fi

# P1.10d — the `--llm-api-key` argv flag was removed. Pass the secret
# via the BOOLE_LLM_API_KEY env var so it never lands in argv.
init_env=()
if [[ -n "$LLM_API_KEY_ENV" ]]; then
  init_env+=(BOOLE_LLM_API_KEY="${!LLM_API_KEY_ENV}")
elif [[ "$LLM_BACKEND" == "openai_compat" ]]; then
  init_env+=(BOOLE_LLM_API_KEY=sk-no-key)
fi

env "${init_env[@]}" "$MINER_BIN" "${init_args[@]}" >"$MINER_INIT_OUT"

success=0
for trial in $(seq 1 "$TRIALS"); do
  out="$SMOKE_WORK_DIR/miner-start.${trial}.out"
  set +e
  "$MINER_BIN" start \
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
    "miner": "boole-miner provider/model + FamilyV1LenboundTargetEmitter + StructuralCanonicalizer (proof-intake canonicalization) + AcceptingVerifier (--mock-verify-accept)",
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

smoke_stop_and_wait "$PID"
SMOKE_CHILD_PIDS=""
printf 'boole-provider-model-smoke: PASS provider=%s model=%s\n' "$LLM_PROVIDER_LABEL" "$LLM_MODEL" >&2
