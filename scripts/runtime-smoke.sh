#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

SCENARIO="${SCENARIO:-fixtures/protocol/runtime-smoke/v1.json}"
BLOCK_STORE="${BLOCK_STORE:-/tmp/boole-runtime-smoke.ndjson}"

rm -f "$BLOCK_STORE"
mkdir -p "$(dirname "$BLOCK_STORE")"

if [[ -n "${BOOLE_NODE_BIN:-}" ]]; then
  OUTPUT="$($BOOLE_NODE_BIN runtime-smoke --scenario "$SCENARIO" --block-store "$BLOCK_STORE")"
else
  OUTPUT="$(cargo run -q -p boole-node -- runtime-smoke --scenario "$SCENARIO" --block-store "$BLOCK_STORE")"
fi

python3 - "$OUTPUT" <<'PY' >&2
import json
import sys

raw = sys.argv[1]
out = json.loads(raw)
errors = []

def expect(cond, message):
    if not cond:
        errors.append(message)

expect(out.get("ok") is True, "ok must be true")
expect(out.get("accepted") is True, "accepted must be true")
expect(out.get("storeSize") == 2, "storeSize must be 2 for tracked runtime-smoke fixture")
expect(out.get("replayHeight") == 2, "replayHeight must be 2 for tracked runtime-smoke fixture")
expect(out.get("latestMatchesRuntime") is True, "latestMatchesRuntime must be true")
expect(out.get("replayMatchesRuntime") is True, "replayMatchesRuntime must be true")
expect(out.get("runtimeHead") == out.get("c"), "runtimeHead must equal latest c")
expect(out.get("replayLatestC") == out.get("c"), "replayLatestC must equal latest c")

blocks = out.get("blocks")
expect(isinstance(blocks, list), "blocks must be a list")
if isinstance(blocks, list):
    expect(len(blocks) == 2, "tracked runtime-smoke fixture must produce two blocks")
    if len(blocks) == 2:
        expect(blocks[0].get("height") == 0, "block 0 height must be 0")
        expect(blocks[1].get("height") == 1, "block 1 height must be 1")
        expect(blocks[1].get("prevC") == blocks[0].get("c"), "block 1 prevC must equal block 0 c")
        expect(out.get("height") == blocks[1].get("height"), "output height must be latest block height")
        expect(out.get("c") == blocks[1].get("c"), "output c must be latest block c")

if errors:
    for error in errors:
        print(f"runtime-smoke validation failed: {error}", file=sys.stderr)
    raise SystemExit(1)

print("runtime-smoke: PASS", file=sys.stderr)
PY

printf '%s\n' "$OUTPUT"
