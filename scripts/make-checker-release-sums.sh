#!/usr/bin/env bash
# SC.9b / ADR-0016 (a-2) — regenerate lean/checker/SHA256SUMS, the minimal
# release channel artifact (P3.6 subset: git tag + SHA256SUMS). It covers
# RELEASE-MANIFEST.json plus every checker source file the artifact hash
# pins, so an operator who downloads a tagged checker can verify it
# byte-for-byte before trusting the network's checker pin.
#
# Run after ANY change to the checker sources or the release manifest, then
# re-run scripts/verify-checker-artifact-hash.sh; the repo test
# `preset_pin_matches_released_checker_toolchain_manifest` enforces that
# the SUMS, the manifest, the compiled preset pin, and the sources agree.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CHECKER_DIR="$ROOT/lean/checker"

FILES=(
  "RELEASE-MANIFEST.json"
  "lean-toolchain"
  "lakefile.lean"
  "lake-manifest.json"
  "BooleCheck/Main.lean"
  "BooleCheck/Audit.lean"
  "Boole/Family/V0Helpers.lean"
)

cd "$CHECKER_DIR"
: > SHA256SUMS
for rel in "${FILES[@]}"; do
  python3 - "$rel" <<'EOF' >> SHA256SUMS
import hashlib, pathlib, sys
rel = sys.argv[1]
digest = hashlib.sha256(pathlib.Path(rel).read_bytes()).hexdigest()
print(f"{digest}  {rel}")
EOF
done

echo "wrote $CHECKER_DIR/SHA256SUMS:"
cat SHA256SUMS
