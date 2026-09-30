# ZK verification problem registry — production v1

Status: closed-local production tooling and a first wave of problem packages. No problem is issued on any
network, no reward exists, and BF.7 remains HOLD. Protocol-authored problems only: Boole produces the Lean
statement and the model of the real code; miners would only prove.

## Why

The census work of September 28–29 measured the ZK verification population (zkVM chips, zkEVM modules, proof-system
components, verifier contracts, cryptographic primitives, gadget libraries, application circuits, chain
state-transition programs and contracts) and found no item with full machine-checked coverage. A 30-item pilot
turned 17 candidates into non-trivial Lean problems and a strong model proved 15 of them with independent kernel
checks. Hand-authoring problems does not scale, so production uses deterministic generators per family.

## Property templates

- **DET** (implemented): output determinism of a circuit — two constraint-satisfying witnesses with equal inputs
  have equal outputs. No bespoke specification is needed (tier T1: the definition of determinism). A confirmed
  counterexample marks the package `DET-FALSE-CANDIDATE`; such findings stay private.
- Planned: release refinement (REL) and specification-backed templates (zkVM instruction chips against the Sail
  RISC-V model; standard primitives against FIPS / RFC / SEC / EIP) under the three-tier specification rule
  (T1 external standards, T2 published project specifications with statement review, T3 implementation-attached
  text not accepted).

## Circom DET generator

Code: `scripts/zk_registry/` (driver `circom_det.py`, generator 1.3). Pipeline: a pinned, digest-checked circom
compiler compiles a template instantiation without simplification (`--O0`; circom 1: `-f`); `.r1cs` / `.sym` are
read and emitted verbatim as a `ZMod p` R1CS model in Lean; inputs and outputs come from the main component's signal
declarations. Instantiations above 2,000 constraints are `TOO-LARGE` and are decomposed (below).

**Compiler selection.** The newest `pragma circom` in the template's include closure selects its language line; the
pinned release of that line is tried first, then the pinned releases of newer lines: 2.0 → v2.0.9, v2.1.9, v2.2.3;
2.1 → v2.1.9, v2.2.3; 2.2 → v2.2.3. A closure without any pragma uses circom 2's documented default (v2.2.3) and then
circom 1 (npm `circom` 0.5.46, the last circom 1 release), whose R1CS/SYM and wasm witness generator (through its
pinned `circom_runtime`) feed the same pipeline. Pins: v2.2.3 by the release page's published digests; v2.1.9 by the
digest of its arm64 macOS release asset (the release page publishes none; identical R1CS/SYM/wasm to a source build of
the tag on five cross-check circuits); v2.0.9 by a source build of the tag commit (its macOS asset is x86_64), Cargo.lock
registry crates at their locked checksums; circom 1 by the npm tarball integrity and the install's package-lock digest.
Each record names the compiler used and every (include context, compiler) attempt.

**Instantiation rule.** Tiers, first tier with a compilable candidate wins: `parameter-free` → `repo-main`
(`component main`, circomkit `circuits.json` entries, `component main` strings of repository scripts) → `repo-test`
(test wrappers; circomkit `WitnessTester`/`ProofTester` parameters and `component main` strings of JS/TS tests, literal
values or single numeric consts only) → `repo-internal` (literal library calls) → `repo-derived` (chains: a call inside
any template whose parameters are grounded, test wrappers and library dependencies included, with the enclosing
parameters, single-assignment `var` bindings and the values of simple counting `for` loops substituted, repeated until
nothing new is derived) → `documented-default` → `probed` → UNINSTANTIABLE. No parameter is invented: every
non-probed candidate is an expression the repository writes, evaluated with parameters the repository supplies. The
optional `probed` tier applies only when no other tier has a candidate and the template's own top-level asserts bound
every parameter from above (lower bound from asserts, else 1): values from {1, 2, 3, 4, 8, 16, 32, 64} inside the
bounds. Probed records carry `instantiation.rule = probed`, and a counterexample on them is labelled
`not-a-finding`. Within the tier the largest compiled instantiation within the size policy is chosen.

**Statement shape.** DET for every package:

```lean
theorem det [Fact (Nat.Prime p)] :
    ∀ w₁ w₂ : Fin nWires → F, Constraints w₁ → Constraints w₂ →
      (∀ i ∈ Inputs, w₁ i = w₂ i) → ∀ o ∈ Outputs, w₁ o = w₂ o := by
  sorry
```

Templates whose inputs carry tags with a meaning fixed by circom's documentation and circomlib (`binary`; `maxbit`
with a repository-grounded value) are compiled through a generated wrapper whose main inputs are untagged copies (same
names and dimensions; tagged intermediate signals feed the template), and their statement is **DET under input
preconditions**:

```lean
theorem det [Fact (Nat.Prime p)] :
    ∀ w₁ w₂ : Fin nWires → F, Constraints w₁ → Constraints w₂ → Preconditions w₁ → Preconditions w₂ →
      (∀ i ∈ Inputs, w₁ i = w₂ i) → ∀ o ∈ Outputs, w₁ o = w₂ o := by
  sorry
```

with `Preconditions w := (∀ i ∈ BinaryInputs, w i = 0 ∨ w i = 1) ∧ (∀ i ∈ MaxbitInputs<n>, (w i).val < 2 ^ n)` over
literal lists of the tagged main input wires (`statement.preconditions`, `instantiation.tag_wrapper` in the record).
Samplers and the boundary grid stay inside the preconditions, counterexamples must satisfy them, and G-FID also
compares Lean's `decide (Preconditions w)` with the Python evaluation. Project-defined tags (e.g. `uint64`,
`sub_order_bj_p`) and valued tags without a value remain `UNINSTANTIABLE` with the reason.

**Decomposition.** For every TOO-LARGE instantiation (and every one whose sizing compile was stopped by the resource
guard), the concrete sub-component instantiations it uses are harvested from the source with the parent's parameters,
bindings and loop values substituted. Children are identified by (prime, template content, parameter values); the
content hash covers the normalized declarations a template reaches through its include closure. A child equal to an
existing package is only mapped to it; the others are sized, children above 2,000 constraints are decomposed in turn
(up to 4 levels), and per template content the largest harvested instantiation within the size policy is packaged
(`instantiation.rule = decomposition`, with parents, depth, content hash and variant count); every parent → child edge
is recorded. No compositional statement is made.

Gates per package: G-ELAB (statement elaborates), G-NONVAC (a real witness from circom's own generator satisfies
the model), G-FID (every sampled witness is evaluated by Lean and by an independent Python R1CS oracle), G-TRIV
(the standardized automation battery must not close the statement), and a sound counterexample search. The battery
is the TRIV-P1 battery (11 tactics × V0 as stated / V1 unfold / V2 intros+unfold, 33 forms) plus P2 (7 forms, aimed at
linear and copy circuits whose proofs need the literal `Inputs`/`Outputs` lists case-split: V3 introduces the
hypotheses, flattens `∀ i ∈ [..]` into conjunctions and unfolds the constraints, then `simp` / `simp_all` / `decide` /
`omega` / `grind`; V4 also splits the goal and runs `simp_all` / `grind` on every goal), 200,000 heartbeats per form.
`battery-p2` re-runs P2 alone on packaged statements; a closure supersedes an OPEN record with GATE-FAIL.

Package: `problem.json` (identities, instantiation provenance, specification tier and hash, circuit pins and sizes,
statement, gate results, checker metadata, environment pins, generator source hash) validated against
`schema/problem.schema.json`; `Statement.lean` (theorem `det` with `sorry`); `ZkDet/<Ident>/Model.lean`; `evidence/`.
The checker (`check.py` with `lean/ZkReplay.lean`) verifies statement bytes, imported-file hashes, forbidden
constructs, `#print axioms` ⊆ {`propext`, `Classical.choice`, `Quot.sound`}, an independent kernel replay and the
elaborated statement type.

Tests: `scripts/test_zk_registry_*.py` (offline, fixture-based), registered in `scripts/self-test.sh`.

## Wave 0 (circomlib v2.0.5, 106 templates)

| Status | Count |
|---|---:|
| OPEN | 48 |
| GATE-FAIL (20 closed by the automation battery, 1 fidelity) | 21 |
| DET-FALSE-CANDIDATE (private; all match publicly reported issues) | 5 |
| TOO-LARGE | 19 |
| UNINSTANTIABLE | 13 |

DET truth is unknown for every OPEN package. Several OPEN packages for linear or copy circuits are likely easy; the
battery should be strengthened in the next revision. Packages and the wave report are kept in the operator's local
workspace, not in this repository.

## Wave 1 (all other Circom templates in the frozen ledger)

1,876 ledger rows from 49 repositories were deduplicated by normalized source content to 1,661 distinct templates
(215 in-wave duplicates mapped to their canonical template; none matched a wave-0 circomlib template).

| Status | Count |
|---|---:|
| OPEN | 319 |
| GATE-FAIL (127 closed by the automation battery) | 170 |
| DET-FALSE-CANDIDATE (private) | 23 |
| TOO-LARGE | 373 |
| UNINSTANTIABLE | 775 |
| ERROR (unsupported bus-typed output) | 1 |

Main reasons for UNINSTANTIABLE: parameters that cannot be determined from the repository, sub-component signal
accesses rejected by circom 2.2.3, circom 1 sources, tagged inputs (not packaged, because a wrapper would drop the
tag precondition and could yield spurious counterexamples) and custom gates that have no R1CS form. Every row is
recorded with its reason. Generator fixes made for this wave: multi-repository include paths and prime rules,
includer contexts, custom-template pragmas and custom-gate rejection, a compile-output size guard and sizing budget,
and Lean recursion limits (regression tests in `scripts/test_zk_registry_*.py`).

Waves 0 and 1 together: **367 OPEN** DET packages.

## Limits

- Closed-local artifacts; no registry service, issuance, receipt or reward path is wired.
- Pilot statements and proofs were produced by agents of one model family; independence of statement authorship
  from solving is not established.
- Content-identical copies across repositories must be deduplicated by content hash before packaging.
