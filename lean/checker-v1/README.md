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
f9da3a1c5bcb605a26c8f778e2661e471398864c69393079aed461dd1453d7b4
```

Recompute and verify with:

```bash
scripts/verify-checker-artifact-hash.sh
```

## Building the trusted helper

```bash
cd lean/checker
lake build Boole.Family.V0Helpers
```

Production verification does not trust a prebuilt `boole_check` executable or
the package's gitignored helper artifact. `boole-lean-runner` snapshots the
pinned helper source into request-private storage and compiles it there under
the same containment boundary used by the two verification stages.

## Running

```bash
boole-lean-runner consumer API (request-private helper compile -> direct
`lean --run BooleCheck/Main.lean` -> artifact-only
`lean --run BooleCheck/Audit.lean`)
```

The three stages share one outer deadline. The primary process receives only
the snapshotted submitted source; the audit process receives only the sealed
artifact file descriptor. Both use the canonical Lean executable under the
pinned toolchain sysroot, and all children run with process spawning, network
access and package mutation denied. The Rust runner remains responsible for
process-group cleanup, wall-clock containment, rlimits, output caps,
environment scrubbing and forbidden-source-token rejection.

## Toolchain

The expected Lean compiler version is pinned in `lean-toolchain`. The
`checker_artifact_hash` covers that file, so any node running a different
toolchain produces a different artifact hash and is rejected by operators
pinning the canonical hash. The compiler binary itself is still installed
on the host PATH (Lake style); the runtime additionally records the
output of `lean --version` and `lake --version` in evidence so a build
that links against an unexpected compiler is detectable post-hoc.
