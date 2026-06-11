# ADR 0001: POFP-v2 canonical-package widening

## Status

Status: Implemented (wire format only — see scope note below).

POFP-v2 is the default canonical package emitted by the Rust Lean proof bridge. It widens the two formerly narrow canonical expression slots to two domain-separated 256-bit opaque digest slots. The change invalidates POFP-v1 proof-package bytes and therefore must still coincide with a chain reset for any network that previously admitted v1 packages.

Scope note: "Implemented" covers the v2 wire format. The live mining loop
does not yet emit Lean-bound v2 packages — it grinds a structural
placeholder. Binding the live canon bytes to Lean evidence is specified in
ADR-0007 and lands in wave N0.

## Context

`canonical_pofp_package_from_lean_result` (in
`crates/boole-node/src/proof_bridge.rs`) emits the canonical bytes that
become `canon_hash = sha256(package)`. Two of the slots in the v1 package
are derived from `stable_u32(...)` and pin only 32 bits each. The
*variable* portion of the package across distinct `LeanCheckResult`
inputs is therefore at most 64 bits.

That makes the effective entropy of `canon_hash` 64 bits even though
sha256 nominally outputs 256. The protocol is still safe in v1 because
the share is bound through:

- **Ticket PoW** binds `(c, pk, n)` via
  `ticket(c, pk, n) < T_ticket` (`hash.rs::ticket`). This is what binds
  `n` to a share, not the submission PoW.
- **Submission PoW** binds `(c, pk, nonceS, canon_hash) < T_submit`
  (`hash.rs::submission_pow_hash`). Submission PoW does **not** include
  `n`.
- **Share hash** is
  `share_hash(c, pk, n, j, canon_hash)`
  (`hash.rs::share_hash`).
- **Pool dedup** is keyed by `(pk, n, j)` per chain head `c`
  (`share_pool.rs::share_key`), not by `share_hash`.

A `canon_hash` collision found in 64-bit space therefore cannot be
replayed against the same `(c, pk, n, j)` slot — the pool would reject
the second insertion as `Duplicate` regardless of `canon_hash`.

The remaining v1 risk surface is forging an *accepted* `LeanCheckResult`
with chosen `canon_hash`. That requires either editing the checker
(rejected by `LeanProofBridgePolicy::allow_checker_artifact_hash` in
`proof_bridge.rs`, where `checker_artifact_hash` covers `lean-toolchain`,
`lakefile.lean`, `lake-manifest.json`, and the recursive `BooleCheck/**`
tree) or breaking the host `lean` binary.

## Decision

Defer the widening to POFP-v2. v2 will widen the two narrow slots to 16
or 32 bytes each so `canon_hash` recovers a full 256-bit collision space
end-to-end and does not have to lean on `(pk, n, j)` for binding.

## Consequences

- v2 changes the wire format. Every previously recorded proof is
  invalidated, so the cutover must coincide with a chain reset.
- v1 is acceptable for testnet and pre-VC review. Production mainnet
  should not ship until v2 lands.
- The SECURITY comment at the top of
  `canonical_pofp_package_from_lean_result` references this ADR and must
  stay in sync if v2 is replanned.
