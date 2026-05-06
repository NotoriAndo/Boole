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
PINNED = ("lean-toolchain", "lakefile.lean", "lake-manifest.json")

entries = []
for rel in PINNED:
    path = root / rel
    if not path.is_file():
        sys.exit(f"missing pinned checker file: {rel}")
    entries.append((rel, path.read_bytes()))

boole_check = root / "BooleCheck"
if boole_check.exists():
    for path in sorted(boole_check.rglob("*")):
        if path.is_symlink():
            sys.exit(f"symlink not allowed: {path.relative_to(root)}")
        if path.is_file():
            rel = path.relative_to(root).as_posix()
            entries.append((rel, path.read_bytes()))

entries.sort(key=lambda item: item[0])

h = hashlib.sha256()
for rel, data in entries:
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
