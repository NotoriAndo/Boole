#!/usr/bin/env bash
set -euo pipefail

timeout_seconds="${BOOLE_PREWARM_TIMEOUT_SECONDS:-60}"

if [[ "$#" -eq 0 ]]; then
  printf 'prewarm: expected at least one binary\n' >&2
  exit 2
fi

for bin in "$@"; do
  if [[ ! -x "$bin" ]]; then
    printf 'prewarm: %s is missing or not executable\n' "$bin" >&2
    exit 1
  fi
  # Do not negate this command in an `if !` condition: the failure branch
  # must preserve the command's original exit status (including timeout 124).
  if /usr/bin/env timeout "$timeout_seconds" "$bin" --help >/dev/null 2>&1; then
    :
  else
    status=$?
    printf 'prewarm: %s --help failed (exit %d)\n' "$bin" "$status" >&2
    exit "$status"
  fi
done
