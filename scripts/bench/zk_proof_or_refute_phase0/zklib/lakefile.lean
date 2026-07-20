import Lake
open Lake DSL

/-!
Protocol-owned ZK primitives library for the zk-proof-or-refute Phase 0
experiment. Self-contained (no mathlib), same pinned toolchain as the
consensus checker (v4.29.1). The consensus `lean/checker` tree is NOT
touched — this is a separate research lib. Domain = algebra/structure of
ZK constructions (no cryptographic-hardness axioms; TB.1 allowlist stays
{propext, Classical.choice, Quot.sound}).
-/
package zklib

@[default_target]
lean_lib «Zk»
