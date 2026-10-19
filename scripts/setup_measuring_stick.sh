#!/usr/bin/env bash
set -euo pipefail
script_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
exec python3 -I -B -c 'import sys, runpy; sys.path.insert(0, sys.argv.pop(1)); runpy.run_module("zk_registry.measuring_sticks", run_name="__main__")' "$script_root" "$@"
