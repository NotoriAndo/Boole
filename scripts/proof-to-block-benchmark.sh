#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

SMOKE_JSON="$(${ROOT}/scripts/runtime-smoke-all.sh)"

python3 - "$SMOKE_JSON" <<'PY'
import json
import os
import pathlib
import shutil
import subprocess
import sys

smoke = json.loads(sys.argv[1])
root = pathlib.Path.cwd()
block_store_dir = pathlib.Path(os.environ.get("BLOCK_STORE_DIR", "/tmp/boole-runtime-smoke-cases"))
block_store_dir.mkdir(parents=True, exist_ok=True)
node_bin = os.environ.get("BOOLE_NODE_BIN")


def node_cmd(args):
    if node_bin:
        return [node_bin, *args]
    return ["cargo", "run", "-q", "-p", "boole-node", "--", *args]


def write_lean_checker_workspace(workspace: pathlib.Path) -> pathlib.Path:
    if workspace.exists():
        shutil.rmtree(workspace)
    (workspace / "BooleCheck").mkdir(parents=True)
    (workspace / "lakefile.lean").write_text(
        """import Lake
open Lake DSL

package boole_check_fixture

lean_exe boole_check where
  root := `BooleCheck.Main
"""
    )
    (workspace / "BooleCheck" / "Main.lean").write_text(
        """def main (args : List String) : IO UInt32 := do
  let some proofPath := args.head?
    | IO.eprintln "usage: boole_check <proof.lean>"; return 64
  let output ← IO.Process.output {
    cmd := "lean"
    args := #[proofPath]
  }
  if output.exitCode == 0 then
    IO.println "boole_check: accepted"
    return 0
  else
    IO.eprintln output.stderr
    return 1
"""
    )
    proof = workspace / "LeanSubmitProofToBlock.lean"
    proof.write_text(
        """theorem boole_benchmark_submit_lean_valid : 2 + 2 = 4 := by
  decide
"""
    )
    return proof


def run_lean_submit_case():
    workspace = block_store_dir / "lean-submit-proof-to-block-workspace"
    proof = write_lean_checker_workspace(workspace)
    block_store = block_store_dir / "lean-submit-proof-to-block.ndjson"
    if block_store.exists():
        block_store.unlink()

    proc = subprocess.run(
        node_cmd([
            "submit-lean",
            "--proof",
            str(proof),
            "--checker-dir",
            str(workspace),
            "--fixture",
            "fixtures/protocol/admission/v1.json",
            "--block-store",
            str(block_store),
            "--verifier-hash",
            "proof-to-block-benchmark-lean-v0",
        ]),
        cwd=root,
        text=True,
        capture_output=True,
    )
    if proc.returncode != 0:
        print(proc.stderr, file=sys.stderr, end="")
        print(proc.stdout, file=sys.stderr, end="")
        raise SystemExit(proc.returncode)

    out = json.loads(proc.stdout)
    errors = []
    for key in ["ok", "accepted", "shareAccepted", "replayMatchesRuntime"]:
        if out.get(key) is not True:
            errors.append(f"submit-lean output {key} must be true")
    if out.get("invalidAccepted") != 0:
        errors.append("submit-lean invalidAccepted must be 0")
    if out.get("block", {}).get("selectedShares") != 1:
        errors.append("submit-lean block must select exactly one share")
    if out.get("replayLatestC") != out.get("runtimeHead"):
        errors.append("submit-lean replayLatestC must match runtimeHead")
    if errors:
        for error in errors:
            print(f"Lean submit benchmark case failed: {error}", file=sys.stderr)
        raise SystemExit(1)

    block = out["block"]
    print("proof-to-block case lean-submit-proof-to-block: PASS", file=sys.stderr)
    return {
        "name": "lean-submit-proof-to-block",
        "mode": "submit-lean",
        "input": str(proof.relative_to(root)) if proof.is_relative_to(root) else str(proof),
        "ok": True,
        "accepted": True,
        "shareAccepted": True,
        "blockProduced": True,
        "height": block["height"],
        "storeSize": 1,
        "replayHeight": out["replayHeight"],
        "latestMatchesRuntime": out["replayLatestC"] == out["runtimeHead"],
        "replayMatchesRuntime": out["replayMatchesRuntime"],
        "invalidAccepted": out["invalidAccepted"],
        "blockStorePath": out["blockStorePath"],
        "lean": {
            "accepted": out.get("lean", {}).get("accepted"),
            "checker": out.get("lean", {}).get("checker"),
            "verifierHash": out.get("lean", {}).get("verifier_hash"),
        },
        "blocks": [block],
    }


cases = list(smoke.get("cases", []))
cases.append(run_lean_submit_case())
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
invalid_accepted = sum(int(case.get("invalidAccepted", 0)) for case in cases)

out = {
    "ok": smoke.get("ok") is True and replay_failures == 0 and chain_divergence == 0 and invalid_accepted == 0,
    "benchmark": "proof-to-block",
    "version": 0,
    "description": "Seed benchmark derived from checked local runtime-smoke cases plus a real Lean-backed submit-lean proof-to-block case; not a model leaderboard yet.",
    "source": {
        "harness": "scripts/runtime-smoke-all.sh + boole-node submit-lean",
        "manifest": smoke.get("manifest"),
    },
    "summary": {
        "casesPassed": sum(1 for case in cases if case.get("ok") is True and case.get("accepted") is True),
        "caseCount": len(cases),
        "blocksProduced": blocks_produced,
        "replayFailures": replay_failures,
    },
    "safety": {
        "invalidAccepted": invalid_accepted,
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
