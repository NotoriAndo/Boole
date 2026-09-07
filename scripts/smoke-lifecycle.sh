#!/usr/bin/env bash
# Source from a smoke running with set -euo pipefail. Only this invocation's
# private directory is removed. Explicit output paths must be new and are kept.
SMOKE_WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/boole-smoke.XXXXXX")"
SMOKE_CHILD_PIDS=""

smoke_fresh_path() {
  if [[ -e "$1" || -L "$1" ]]; then
    printf 'smoke refuses existing output path: %s\n' "$1" >&2
    return 1
  fi
  printf '%s\n' "$1"
}

smoke_build_binary() {
  # Cargo's artifact path honors target-dir configuration. Never execute a
  # potentially stale ROOT/target/debug binary after building elsewhere.
  local smoke_binary="$1" smoke_package="$2"
  shift 2
  cargo build -q --locked -p "$smoke_package" --bin "$smoke_binary" --message-format=json "$@" |
    python3 -c '
import json, sys
name = sys.argv[1]
executable = None
for line in sys.stdin:
    row = json.loads(line)
    if row.get("reason") == "compiler-message":
        print(row.get("message", {}).get("rendered", ""), file=sys.stderr, end="")
    if row.get("reason") == "compiler-artifact" and row.get("target", {}).get("name") == name and row.get("executable"):
        executable = row["executable"]
if executable is None:
    raise SystemExit("cargo did not return the requested smoke executable")
print(executable)
' "$smoke_binary"
}

smoke_prewarm_binary() {
  # First execution on macOS can include code-signature/cache preparation.
  # Pay this bounded cost before starting any HTTP readiness/faucet clock.
  python3 -c 'import subprocess, sys; subprocess.run([sys.argv[1], "--help"], check=True, timeout=60, stdout=subprocess.DEVNULL)' "$1"
}

smoke_register_child() {
  case "$1" in ''|*[!0-9]*) return 1 ;; esac
  SMOKE_CHILD_PIDS="$SMOKE_CHILD_PIDS $1"
}

smoke_stop_and_wait() {
  local smoke_pid="$1"
  kill -TERM "$smoke_pid" >/dev/null 2>&1 || true
  local smoke_tick
  for smoke_tick in {1..100}; do
    if ! kill -0 "$smoke_pid" >/dev/null 2>&1; then
      wait "$smoke_pid"
      return $?
    fi
    sleep 0.05
  done
  printf 'smoke child did not stop within 5 seconds: %s\n' "$smoke_pid" >&2
  kill -KILL "$smoke_pid" >/dev/null 2>&1 || true
  # Do not turn failed cleanup into an unbounded wait.
  return 1
}

smoke_cleanup() {
  local smoke_pid
  for smoke_pid in $SMOKE_CHILD_PIDS; do
    smoke_stop_and_wait "$smoke_pid" >/dev/null 2>&1 || true
  done
  # mktemp returned this exact private path; never remove caller overrides.
  rm -rf -- "$SMOKE_WORK_DIR"
}
trap smoke_cleanup EXIT
