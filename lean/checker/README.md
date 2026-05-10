# BooleCheck — Canonical Lean Proof Checker

This is the reference checker that `boole-lean-runner` invokes via
`lake exec boole_check <proof.lean>`. It is intentionally tiny: it shells
out to the host `lean` executable, forwards the child's stdout/stderr, and
returns 0 if and only if `lean` accepted the proof file.

## Why this directory matters

The SHA-256 of every file the checker depends on is recorded in every
proof package as `checker_artifact_hash`. Operators pin a hash allowlist
via `LeanProofBridgePolicy::allow_checker_artifact_hash(...)`, so any
byte-level modification of this directory invalidates every proof produced
afterwards until operators rotate the allowlist.

The hash inputs, in canonical order, are:

1. `lean-toolchain` — pins the Lean compiler version operators must use.
2. `lakefile.lean` — pins the Lake build configuration.
3. `lake-manifest.json` — pins resolved dependency versions.
4. Every file under `BooleCheck/**` (recursive), sorted by relative path.

Symlinks anywhere inside the package are rejected so an operator cannot
smuggle a file in via a symlink that resolves outside the package.

## Canonical artifact hash

The hash of the files committed to this repo:

```
160009a4f09686c0d264e82261bfd1fa8783f78fb98a8f7783695ccdae217b87
```

Recompute and verify with:

```bash
scripts/verify-checker-artifact-hash.sh
```

## Building

```bash
cd lean/checker
lake build boole_check
```

## Running

```bash
cd lean/checker
lake exec boole_check /path/to/proof.lean
```

The checker exits 0 on accepted proofs and non-zero on every other outcome
(missing argument, lean not on PATH, lean rejected the proof). The Rust
runner is responsible for sandboxing (process group, timeout, rlimits,
output cap, env scrub, `sorry` detection) — this checker is the trust
core, not the sandbox.

## Toolchain

The expected Lean compiler version is pinned in `lean-toolchain`. The
`checker_artifact_hash` covers that file, so any node running a different
toolchain produces a different artifact hash and is rejected by operators
pinning the canonical hash. The compiler binary itself is still installed
on the host PATH (Lake style); the runtime additionally records the
output of `lean --version` and `lake --version` in evidence so a build
that links against an unexpected compiler is detectable post-hoc.
