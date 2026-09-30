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

Code: `scripts/zk_registry/` (driver `circom_det.py`). Pipeline: pinned circom release (digest-checked) compiles a
template instantiation with `--O0`; `.r1cs` / `.sym` are read and emitted verbatim as a `ZMod p` R1CS model in Lean;
inputs and outputs come from the template's signal declarations. Instantiation parameters are taken, in order, from
the repository's `component main`, its tests, literal calls in the library, derivation from enclosing calls, or a
documented default; otherwise the template is `UNINSTANTIABLE`. Instantiations above 2,000 constraints are
`TOO-LARGE` (decomposition candidates).

Gates per package: G-ELAB (statement elaborates), G-NONVAC (a real witness from circom's own generator satisfies
the model), G-FID (every sampled witness is evaluated by Lean and by an independent Python R1CS oracle), G-TRIV
(the standardized automation battery must not close the statement), and a sound counterexample search.

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
