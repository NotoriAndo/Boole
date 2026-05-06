#!/usr/bin/env bash
# Recompute the canonical Lean checker artifact hash and compare it against
# the value pinned in lean/checker/README.md. Fails non-zero if they differ.
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
checker_dir="$repo_root/lean/checker"
readme="$checker_dir/README.md"

if [[ ! -d "$checker_dir" ]]; then
  echo "checker directory missing: $checker_dir" >&2
  exit 2
fi

actual=$(python3 - "$checker_dir" <<'PY'
import hashlib, pathlib, sys
root = pathlib.Path(sys.argv[1])
h = hashlib.sha256()
for rel in ("lakefile.lean", "BooleCheck/Main.lean"):
    data = (root / rel).read_bytes()
    h.update(rel.encode())
    h.update(b"\x00")
    h.update(data)
    h.update(b"\x00")
print(h.hexdigest())
PY
)

pinned=$(grep -Eo '^[a-f0-9]{64}$' "$readme" | head -n1 || true)

if [[ -z "$pinned" ]]; then
  echo "no pinned hash found in $readme" >&2
  echo "actual: $actual" >&2
  exit 3
fi

if [[ "$pinned" != "$actual" ]]; then
  echo "checker artifact hash drift detected" >&2
  echo "  pinned: $pinned" >&2
  echo "  actual: $actual" >&2
  echo "Update lean/checker/README.md or revert the change." >&2
  exit 1
fi

echo "checker artifact hash OK: $actual"
