# Benchmark target families

Each Boole benchmark attempt is bound to a **target family** — a versioned identifier that fixes the prompt shape, theorem template, lottery binding, and canonical proof contract. The family is recorded as `targetFamily` on every per-row JSON record and run summary so historical artifacts can be interpreted against the family that produced them.

This document lists **active** benchmark families only. Deprecated experimental families and their fixtures are intentionally removed from the active runner/test surface instead of kept as aliases.

---

## boole.calibration.pow.v1

**Mode:** `mining` (default for `scripts/boole-model-benchmark.py`).

**Used by:** the local proof-to-block benchmark and preflight scripts. Public/API benchmark runs still require explicit approval before execution.

**Theorem template:**

```lean
theorem boole_benchmark_pow_target_<n> : "<challenge>" = "<challenge>" :=
  <PROOF_TERM>
```

The lottery sample (`challenge`, `nonce`, `theoremName`) is derived deterministically from `(runId, target, attemptIndex, benchmarkMode, targetFamily)`. Each attempt also embeds the lottery in Lean comment lines so the canonical package binds the lottery to the proof source hash.

**Canonical proof shape:** any term that closes `"<challenge>" = "<challenge>"`. In practice, `rfl` is the minimal proof term. The proof term is not treated as model-solved evidence until it passes ProofIntake, canonicalization, verifier, and share/block gates.

**Difficulty:** the calibration target lives in `fixtures/protocol/admission/v1.json` and is read by `boole-node submit-lean --difficulty-mode fixture`. The target family does not pick its own consensus difficulty.

**Verifier hash:** resolved from `fixtures/benchmarks/verifier-hashes.json` via `resolve_verifier_hash()`. Replay/validation pins to the recorded verifier hash version.

**Known limitation:** this active calibration family is useful for deterministic protocol plumbing and local smoke evidence, not for claiming proof-search ability by itself. Public claims must remain evidence-gated and distinguish smoke/local mining from public/API benchmark results.

---

## boole.smoke.true.v1

**Mode:** `smoke`.

**Used by:** pipeline-only benchmark checks that validate runner wiring, row writing, verifier plumbing, timeout handling, and report shape without exercising the mining target family.

**Theorem template:**

```lean
theorem boole_benchmark_true : True :=
  <PROOF_TERM>
```

**Canonical proof shape:** `True.intro` or another proof term accepted by Lean for `True`.

**Known limitation:** this family is smoke-only. It must not be used for public model ranking, public mining claims, or proof-search productivity claims.
