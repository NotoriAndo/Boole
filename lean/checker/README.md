# BooleCheck — Canonical Lean Proof Checker

This is the reference checker that `boole-lean-runner` invokes via
`lake exec boole_check <proof.lean> <maxHeartbeats> <maxRecDepth>`. It is
intentionally tiny: it shells out to the host `lean` executable (forwarding
the committed step budget as `-DmaxHeartbeats=<n> -DmaxRecDepth=<n>` —
SC.9a / ADR-0016), forwards the child's stdout/stderr, and returns 0 if and
only if `lean` accepted the proof file within that budget.

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
4. `Boole/Family/V0Helpers.lean` — the helper surface proof files import
   (`import Boole.Family.V0Helpers`); pinned explicitly because it lives
   outside `BooleCheck/`.
5. Every file under `BooleCheck/**` (recursive), sorted by relative path.

Symlinks anywhere inside the package are rejected so an operator cannot
smuggle a file in via a symlink that resolves outside the package.

## Canonical artifact hash

The hash of the files committed to this repo:

```
1dd3055acb05142816f2082f0b3ad000c49513c3a2401572ec68703542042be1
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
lake exec boole_check /path/to/proof.lean 400000 512
```

The checker exits 0 on accepted proofs and non-zero on every other outcome
(missing argument, lean not on PATH, lean rejected the proof, committed
step budget exhausted). The two trailing args are the committed step
budget (`maxHeartbeats` in Lean's thousands unit, `maxRecDepth`); when
omitted (manual use) the inner `lean` falls back to its own defaults —
`boole-lean-runner`, the only production caller, always passes them so the
verdict is a pure function of (proof bytes, this checker, committed
budget). The Rust runner is responsible for sandboxing (process group,
wall-clock containment, rlimits, output cap, env scrub, `sorry` detection)
— this checker is the trust core, not the sandbox.

## Toolchain

The expected Lean compiler version is pinned in `lean-toolchain`. The
`checker_artifact_hash` covers that file, so any node running a different
toolchain produces a different artifact hash and is rejected by operators
pinning the canonical hash. The compiler binary itself is still installed
on the host PATH (Lake style); the runtime additionally records the
output of `lean --version` and `lake --version` in evidence so a build
that links against an unexpected compiler is detectable post-hoc.
