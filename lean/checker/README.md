# BooleCheck — Canonical Lean Proof Checker

This is the reference checker that `boole-lean-runner` invokes via
`lake exec boole_check <proof.lean>`. It is intentionally tiny: it shells
out to the host `lean` executable, forwards the child's stdout/stderr, and
returns 0 if and only if `lean` accepted the proof file.

## Why this directory matters

The SHA-256 of `lakefile.lean` and `BooleCheck/Main.lean` (concatenated
with NUL separators in that order) is recorded in every proof package as
`checker_artifact_hash`. Operators pin a hash allowlist via
`LeanProofBridgePolicy::allow_checker_artifact_hash(...)`, so any byte-level
modification of this directory invalidates every proof produced afterwards
until operators rotate the allowlist.

## Canonical artifact hash

The hash of the files committed to this repo:

```
a91261b9957ea0cae0a37a090ad6fc90852e701d4788f3fdf83552ab27668239
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

The repository does not pin a `lean-toolchain` here. Operators are
expected to install a Lean 4 toolchain on their PATH; the
`checker_artifact_hash` covers only the Lean source, not the compiler
binary. A future change may pin the toolchain so the proof package can
attest to Lean version determinism.
