# Public counter identity corpus

These 32 cases (16 families, two parameter values each) come only from
`scripts/zk_registry/generate_public_counter_corpus.py` and the pinned public
Lean 4 Init library. No local catalog proof, known solution, export, receipt or
old golden is an input. The generator preregisters source hashes before Lean
runs and seals every export hash before counter execution. Both public source
commits and exact generation tool identities are retained in the manifest and
preregistration. The compressed archive contains only generated public exports.

Families cover universes, lambda/identity, products/projections, conjunction,
disjunction, recursion, imported theorems, lists, quotients, function equality,
a new inductive, sized natural/string literals, arithmetic, bitwise primitives
and nested applications. Some parameter-independent templates intentionally
share bytes; these are 32 cases, not 32 distinct proof shapes.

The aarch64 run accepts all 32 cases in both kernels. Its 18,732 nanoda
per-declaration vectors exercise all three call kinds and all seven instrumented
event slots (0..6). Slot 7 is reserved and has no producer. Identity includes
full deterministic per-declaration vector hashes and typed outcomes. This is
counter architecture coverage, not real-workload timing prediction, cap
calibration, package eligibility or an x86 production attestation. The x86 CI
job is advisory, has never run, and its 120-minute fit remains unmeasured.
