#!/usr/bin/env bash
set -euo pipefail

timeout_seconds="${BOOLE_PREWARM_TIMEOUT_SECONDS:-60}"

if [[ "$#" -eq 0 ]]; then
  printf 'prewarm: expected at least one binary\n' >&2
  exit 2
fi

if ! command -v python3 >/dev/null 2>&1; then
  printf 'prewarm: python3 is required for bounded execution\n' >&2
  exit 127
fi

# Python is already a self-test prerequisite and is portable to macOS, where
# GNU timeout is not shipped. Keep the same timeout exit status and terminate
# the owned process group, including helpers that outlive their direct parent.
exec python3 - "$timeout_seconds" "$@" <<'PY'
import math
import os
import signal
import subprocess
import sys

try:
    timeout = float(sys.argv[1])
    if not math.isfinite(timeout) or timeout <= 0:
        raise ValueError()
except ValueError:
    print("prewarm: timeout seconds must be finite and positive", file=sys.stderr)
    raise SystemExit(2)

for binary in sys.argv[2:]:
    if not os.path.isfile(binary) or not os.access(binary, os.X_OK):
        print(f"prewarm: {binary} is missing or not executable", file=sys.stderr)
        raise SystemExit(1)
    try:
        process = subprocess.Popen(
            [binary, "--help"], start_new_session=True,
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
    except OSError:
        print(f"prewarm: {binary} could not be started", file=sys.stderr)
        raise SystemExit(126)
    try:
        status = process.wait(timeout=timeout)
        if status < 0:
            status = 128 - status
    except subprocess.TimeoutExpired:
        status = 124
    finally:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait()
    if status:
        print(f"prewarm: {binary} --help failed (exit {status})", file=sys.stderr)
        raise SystemExit(status)
PY
