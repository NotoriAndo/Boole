# ADR 0007: Live Lean-evidence binding for mined blocks

## Status

Status: Accepted (2026-06-11). Implementation lands across wave N0 (N0.1–N0.4);
nothing in this ADR is a claim that the binding is already live.

## Context

The live mining loop today grinds a **structural placeholder**: the default
canonicalizer is `StructuralCanonicalizer`
(`crates/boole-miner/src/canonicalizer/structural.rs`) and the canon hash is
`proof_package::bppk_canon_hash` (`crates/boole-miner/src/proof_package.rs`).
Lean verification and replay live in separate sub-pipelines. Consequently the
system's defining property — "Lean-verified work becomes the block" — holds in
fixtures and the bounty path, but **not** on the live block-mining path.

ADR-0001 implemented the POFP-v2 *wire format*; it is easy to misread its
"Implemented" status as "live blocks are Lean-bound". They are not. This ADR
records the four design decisions that close that gap, so the decisions live
in tracked docs rather than operator-local planning notes.

A forward-looking constraint also shapes decision (d): the persisted evidence
is the raw material of a future verified-reasoning **corpus product**
(statement, proof, premise-DAG, difficulty grading). The schema must allow
additive evolution without consensus breaks.

## Decisions

### (a) `LeanBoundCanonicalizer` scope — the miner renders; the node verifies

A new `Canonicalizer` impl (`LeanBoundCanonicalizer`) builds the canonical
bytes from the **rendered canonical proof**
(`family_v1_lenbound::render_canonical_proof`) plus an injected
checker-artifact evidence hash, producing a POFP-v2-shaped package that
`boole_core::validate_proof_package_with_policy` accepts. The miner does NOT
execute Lean; Lean execution remains node-side (`LeanRunner` via the proof
bridge). Rationale: generation and judging stay separated — a miner that runs
its own checker drifts toward self-grading; the miner binary also keeps its
zero-Lean-host-dependency property.

### (b) `bppk_*` placeholder fate — out of the live path, gated to test/fixture builds only

The live submit path must contain no `bppk_*` placeholder after N0.3: the
default `MiningLoopDeps.canonicalizer` switches to `LeanBoundCanonicalizer`,
and the `bppk_*` code is gated behind
`#[cfg(any(test, feature = "bppk-fixtures"))]` so it remains available to
test/fixture builds only. Full deletion is deferred to a post-N0 cleanup
slice; the binding invariant is "absent from the live path", not "absent from
the tree".

### (c) Replay compatibility — keep accepting pre-N0 blocks, with an explicit claim boundary

The validator and `replay_blocks` continue to accept blocks mined before the
N0 fusion (structural-placeholder era). The claim boundary is explicit: the
**Lean-bound guarantee applies only to blocks mined after N0 lands**; earlier
blocks carry structural validation only. `deep_verify_block` reports such
blocks as **legacy-skip** — a distinct, counted outcome, not a failure.
Invalidating the existing closed-local chain would break replay determinism
and force regeneration of every smoke/benchmark fixture for zero security
benefit. Whether a future network restarts from a clean genesis is an N5
(canonical genesis) decision, not this one.

### (d) Block deep-verify entry point and persisted evidence schema

A new **`deep_verify_block`** entry point is added
(`deep_verify_bounty_events` is bounty-JSONL-only and takes no blocks). The
persisted block evidence must be sufficient to re-run
`lake exec boole_check` and recompute the identical canon hash **from the
block alone**. Minimum field set:

1. the rendered canonical proof source (`lean_source`),
2. `checker_artifact_hash`,
3. `verifier_hash`.

The evidence is persisted as a **schema-versioned evidence object** so future
corpus-product fields (proposition statement, premise-DAG keys, grading
metadata) can be added without a consensus break. Like
`promoted_bounty_shares`, the evidence object lives **outside `block_hash`**;
it is a mirror, not the source of truth. The binding invariant: everything
`deep_verify_block` uses to re-verify must be canon-bound — the canon bytes
are PoW-ground and the checker-artifact hash is bound inside the canon per
decision (a) — or be a direct mirror of canon-bound data. Premise-DAG and
reuse-count data are derived side indexes built off-chain from the evidence;
they are NOT block fields.

## Consequences

- N0.2 implements (a); N0.3 implements (b); N0.4 implements (d); (c) shapes
  the N0.4 test matrix (legacy-skip rows) and the replay contract.
- `local-mining-smoke` and `proof-to-block-benchmark` must stay green across
  the N0.3 canonicalizer switch; fixture expectations may need regeneration
  in the same slice.
- ADR-0001's status is cross-referenced to this ADR so "Implemented" cannot
  be read as "live binding done".
- Any future field added to the evidence object bumps its schema version;
  removing or reinterpreting an existing field requires a new ADR.

## Claim boundary

Closed local validation only. Nothing here claims public-network mining,
public scoring, or that live blocks are Lean-bound before N0 closure.
