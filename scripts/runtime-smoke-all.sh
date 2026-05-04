#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

BLOCK_STORE_DIR="${BLOCK_STORE_DIR:-/tmp/boole-runtime-smoke-cases}"
mkdir -p "$BLOCK_STORE_DIR"

run_node() {
  if [[ -n "${BOOLE_NODE_BIN:-}" ]]; then
    "$BOOLE_NODE_BIN" "$@"
  else
    cargo run -q -p boole-node -- "$@"
  fi
}

run_case() {
  local name="$1"
  local mode="$2"
  local input_path="$3"
  local expected_store_size="$4"
  local block_store="$BLOCK_STORE_DIR/${name}.ndjson"
  rm -f "$block_store"

  if [[ "$mode" == "scenario" ]]; then
    run_node runtime-smoke --scenario "$input_path" --block-store "$block_store"
  elif [[ "$mode" == "fixture" ]]; then
    run_node runtime-smoke --fixture "$input_path" --block-store "$block_store"
  else
    echo "unsupported runtime-smoke case mode: $mode" >&2
    return 1
  fi > "$BLOCK_STORE_DIR/${name}.json"

  python3 - "$name" "$mode" "$expected_store_size" "$BLOCK_STORE_DIR/${name}.json" <<'PY' >&2
import json
import sys

name, mode, expected_store_size_raw, path = sys.argv[1:]
expected_store_size = int(expected_store_size_raw)
out = json.load(open(path))
errors = []

def expect(cond, message):
    if not cond:
        errors.append(message)

expect(out.get("ok") is True, "ok must be true")
expect(out.get("accepted") is True, "accepted must be true")
expect(out.get("storeSize") == expected_store_size, f"storeSize must be {expected_store_size}")
expect(out.get("replayHeight") == expected_store_size, f"replayHeight must be {expected_store_size}")
expect(out.get("latestMatchesRuntime") is True, "latestMatchesRuntime must be true")
expect(out.get("replayMatchesRuntime") is True, "replayMatchesRuntime must be true")
expect(out.get("runtimeHead") == out.get("c"), "runtimeHead must equal latest c")
expect(out.get("replayLatestC") == out.get("c"), "replayLatestC must equal latest c")

blocks = out.get("blocks")
expect(isinstance(blocks, list), "blocks must be a list")
if isinstance(blocks, list):
    expect(len(blocks) == expected_store_size, f"blocks length must be {expected_store_size}")
    for index, block in enumerate(blocks):
        expect(block.get("height") == index, f"block {index} height must be {index}")
        if index > 0:
            expect(block.get("prevC") == blocks[index - 1].get("c"), f"block {index} prevC must equal previous c")
    if blocks:
        expect(out.get("height") == blocks[-1].get("height"), "output height must be latest block height")
        expect(out.get("c") == blocks[-1].get("c"), "output c must be latest block c")

if errors:
    for error in errors:
        print(f"runtime-smoke case {name} failed: {error}", file=sys.stderr)
    raise SystemExit(1)

print(f"runtime-smoke case {name}: PASS", file=sys.stderr)
PY
}

run_case "runtime-smoke-multistep" "scenario" "fixtures/protocol/runtime-smoke/v1.json" 2
run_case "admission-fixture-compat" "fixture" "fixtures/protocol/admission/v1.json" 1

python3 - "$BLOCK_STORE_DIR/runtime-smoke-multistep.json" "$BLOCK_STORE_DIR/admission-fixture-compat.json" <<'PY'
import json
import sys

case_specs = [
    ("runtime-smoke-multistep", "scenario", sys.argv[1]),
    ("admission-fixture-compat", "fixture", sys.argv[2]),
]
cases = []
for name, mode, path in case_specs:
    out = json.load(open(path))
    cases.append({
        "name": name,
        "mode": mode,
        "ok": out["ok"],
        "accepted": out["accepted"],
        "height": out["height"],
        "storeSize": out["storeSize"],
        "replayHeight": out["replayHeight"],
        "latestMatchesRuntime": out["latestMatchesRuntime"],
        "replayMatchesRuntime": out["replayMatchesRuntime"],
        "blockStorePath": out["blockStorePath"],
        "blocks": out["blocks"],
    })
print(json.dumps({"ok": True, "caseCount": len(cases), "cases": cases}, separators=(",", ":")))
PY

echo "runtime-smoke-all: PASS" >&2
