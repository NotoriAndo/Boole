#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

export RUNTIME_SMOKE_CASES="${RUNTIME_SMOKE_CASES:-fixtures/protocol/runtime-smoke/cases.v1.json}"
export BLOCK_STORE_DIR="${BLOCK_STORE_DIR:-/tmp/boole-runtime-smoke-cases}"
mkdir -p "$BLOCK_STORE_DIR"

python3 <<'PY'
import json
import os
import pathlib
import subprocess
import sys

root = pathlib.Path.cwd()
manifest_path_raw = os.environ["RUNTIME_SMOKE_CASES"]
manifest_path = pathlib.Path(manifest_path_raw)
if not manifest_path.is_absolute():
    manifest_path = root / manifest_path

block_store_dir = pathlib.Path(os.environ["BLOCK_STORE_DIR"])
block_store_dir.mkdir(parents=True, exist_ok=True)

manifest = json.loads(manifest_path.read_text())
if manifest.get("domain") != "runtime-smoke":
    raise SystemExit(f"runtime-smoke manifest domain mismatch: {manifest.get('domain')!r}")

node_bin = os.environ.get("BOOLE_NODE_BIN")

def node_cmd(args):
    if node_bin:
        return [node_bin, *args]
    return ["cargo", "run", "-q", "-p", "boole-node", "--", *args]

def expect(cond, errors, message):
    if not cond:
        errors.append(message)

def validate_case(case, out):
    errors = []
    expected_store_size = int(case["expectedStoreSize"])
    expected_replay_height = int(case.get("expectedReplayHeight", expected_store_size))

    expect(out.get("ok") is True, errors, "ok must be true")
    expect(out.get("accepted") is True, errors, "accepted must be true")
    expect(out.get("storeSize") == expected_store_size, errors, f"storeSize must be {expected_store_size}")
    expect(out.get("replayHeight") == expected_replay_height, errors, f"replayHeight must be {expected_replay_height}")
    expect(out.get("latestMatchesRuntime") is True, errors, "latestMatchesRuntime must be true")
    expect(out.get("replayMatchesRuntime") is True, errors, "replayMatchesRuntime must be true")
    expect(out.get("runtimeHead") == out.get("c"), errors, "runtimeHead must equal latest c")
    expect(out.get("replayLatestC") == out.get("c"), errors, "replayLatestC must equal latest c")

    blocks = out.get("blocks")
    expect(isinstance(blocks, list), errors, "blocks must be a list")
    if isinstance(blocks, list):
        expect(len(blocks) == expected_store_size, errors, f"blocks length must be {expected_store_size}")
        for index, block in enumerate(blocks):
            expect(block.get("height") == index, errors, f"block {index} height must be {index}")
            if index > 0:
                expect(block.get("prevC") == blocks[index - 1].get("c"), errors, f"block {index} prevC must equal previous c")
        if blocks:
            expect(out.get("height") == blocks[-1].get("height"), errors, "output height must be latest block height")
            expect(out.get("c") == blocks[-1].get("c"), errors, "output c must be latest block c")

    if errors:
        for error in errors:
            print(f"runtime-smoke case {case['name']} failed: {error}", file=sys.stderr)
        raise SystemExit(1)

cases = []
for case in manifest.get("cases", []):
    name = case["name"]
    mode = case["mode"]
    input_path = case["input"]
    expected_store_size = int(case["expectedStoreSize"])
    block_store = block_store_dir / f"{name}.ndjson"
    if block_store.exists():
        block_store.unlink()

    if mode == "scenario":
        args = ["runtime-smoke", "--scenario", input_path, "--block-store", str(block_store)]
    elif mode == "fixture":
        args = ["runtime-smoke", "--fixture", input_path, "--block-store", str(block_store)]
    else:
        raise SystemExit(f"unsupported runtime-smoke case mode: {mode}")

    proc = subprocess.run(node_cmd(args), cwd=root, text=True, capture_output=True)
    if proc.returncode != 0:
        print(proc.stderr, file=sys.stderr, end="")
        print(proc.stdout, file=sys.stderr, end="")
        raise SystemExit(proc.returncode)
    out = json.loads(proc.stdout)
    validate_case(case, out)
    print(f"runtime-smoke case {name}: PASS", file=sys.stderr)
    cases.append({
        "name": name,
        "mode": mode,
        "input": input_path,
        "expectedStoreSize": expected_store_size,
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

if not cases:
    raise SystemExit("runtime-smoke manifest must contain at least one case")

manifest_display = manifest_path_raw if not pathlib.Path(manifest_path_raw).is_absolute() else str(manifest_path.relative_to(root))
print(json.dumps({
    "ok": True,
    "manifest": manifest_display,
    "caseCount": len(cases),
    "cases": cases,
}, separators=(",", ":")))
print("runtime-smoke-all: PASS", file=sys.stderr)
PY
