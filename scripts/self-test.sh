#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/boole-self-test.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

run_logged() {
  local name="$1"
  shift
  local log="$TMP_DIR/${name}.log"
  printf 'self-test check %s: RUN\n' "$name" >&2
  if "$@" >"$log" 2>&1; then
    printf 'self-test check %s: PASS\n' "$name" >&2
  else
    local status=$?
    printf 'self-test check %s: FAIL\n' "$name" >&2
    cat "$log" >&2
    return "$status"
  fi
}

run_capture_json() {
  local name="$1"
  local out="$2"
  shift 2
  local err="$TMP_DIR/${name}.err"
  printf 'self-test check %s: RUN\n' "$name" >&2
  if "$@" >"$out" 2>"$err"; then
    cat "$err" >&2
    printf 'self-test check %s: PASS\n' "$name" >&2
  else
    local status=$?
    printf 'self-test check %s: FAIL\n' "$name" >&2
    cat "$err" >&2
    cat "$out" >&2
    return "$status"
  fi
}

run_logged cargo-fmt cargo fmt --all --check
run_logged cargo-clippy cargo clippy --workspace --all-targets -- -D warnings
run_logged rust-parity ./scripts/check-rust-parity.sh

SMOKE_JSON="$TMP_DIR/runtime-smoke-all.json"
BENCH_JSON="$TMP_DIR/proof-to-block-benchmark.json"
run_capture_json runtime-smoke-all "$SMOKE_JSON" ./scripts/runtime-smoke-all.sh
run_capture_json proof-to-block-benchmark "$BENCH_JSON" ./scripts/proof-to-block-benchmark.sh
MINING_JSON="$TMP_DIR/local-mining-smoke.json"
run_capture_json local-mining-smoke "$MINING_JSON" ./scripts/local-mining-smoke.sh
run_logged git-diff-check git diff --check

GITLEAKS_STATUS="skipped"
if command -v gitleaks >/dev/null 2>&1; then
  run_logged gitleaks gitleaks detect --redact --verbose --no-banner
  GITLEAKS_STATUS="pass"
fi

python3 - "$SMOKE_JSON" "$BENCH_JSON" "$MINING_JSON" "$GITLEAKS_STATUS" <<'PY'
import json
import sys

smoke = json.load(open(sys.argv[1]))
benchmark = json.load(open(sys.argv[2]))
mining = json.load(open(sys.argv[3]))
gitleaks_status = sys.argv[4]

cases = smoke.get("cases", [])
summary = benchmark.get("summary", {})
safety = benchmark.get("safety", {})

checks = [
    {"name": "cargo-fmt", "ok": True},
    {"name": "cargo-clippy", "ok": True},
    {"name": "rust-parity", "ok": True},
    {
        "name": "runtime-smoke-all",
        "ok": smoke.get("ok") is True,
        "caseCount": smoke.get("caseCount"),
        "casesPassed": sum(1 for case in cases if case.get("ok") is True and case.get("accepted") is True),
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
        "miner": mining.get("miner"),
        "blocksMined": mining.get("blocksMined"),
        "finalHeight": mining.get("finalHead", {}).get("height"),
    },
    {"name": "git-diff-check", "ok": True},
    {"name": "gitleaks", "ok": gitleaks_status in {"pass", "skipped"}, "status": gitleaks_status},
]

out = {
    "ok": all(check.get("ok") is True for check in checks),
    "checks": checks,
}
print(json.dumps(out, separators=(",", ":")))
if not out["ok"]:
    raise SystemExit(1)
PY

printf 'self-test: PASS\n' >&2
