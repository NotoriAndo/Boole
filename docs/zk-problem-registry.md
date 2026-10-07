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
- **DET-MOD** (implemented for Circom, recovery R1): modular determinism of an instantiation too large for DET. The
  statement keeps the main component's own compiled constraints and replaces every sub-component instance by an
  uninterpreted function of its inputs, shared by both assignments and by all instances of one kind (identical
  compiled sub-circuits). DET-MOD and DET of every sub-component imply DET; DET-MOD is incomplete where the parent
  relies on facts its sub-components enforce, so a DET-MOD counterexample is labelled `not-a-finding` (GATE-FAIL),
  never DET-FALSE-CANDIDATE. DET-MOD packages are counted separately from DET.
- Planned: release refinement (REL) and specification-backed templates (zkVM instruction chips against the Sail
  RISC-V model; standard primitives against FIPS / RFC / SEC / EIP) under the three-tier specification rule
  (T1 external standards, T2 published project specifications with statement review, T3 implementation-attached
  text not accepted).

## Circom DET generator

Code: `scripts/zk_registry/` (driver `circom_det.py`, generator 1.3). Pipeline: a pinned, digest-checked circom
compiler compiles a template instantiation without simplification (`--O0`; circom 1: `-f`); `.r1cs` / `.sym` are
read and emitted verbatim as a `ZMod p` R1CS model in Lean; inputs and outputs come from the main component's signal
declarations. Instantiations above the size policy are `TOO-LARGE` and are decomposed (below). The policy is 2,000
constraints for waves 0–1b and 4,000 for recovery R1 (chosen from a measured size ladder; every record names its
policy in `circuit.size_policy`, and the validator accepts only these two).

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
`not-a-finding`. Within the tier the largest compiled instantiation within the size policy is chosen. Recovery R1
adds **probe-min** (`probe_min` in the wave configuration) for templates whose asserts do not bound every parameter:
per parameter the literal values the repository passes at its call sites come first, then the asserts' lower bound
(else 1) and the next integers; candidates are compiled in ascending order (the template's own file and its nearest
includer) and the first that compiles is used, i.e. the smallest values that satisfy the template's asserts and array
sizes. A compile-time evaluation error (T3001) or a typing error ends the attempts for that candidate or template.
Probe-min records are `rule = probed`, with the same `not-a-finding` label.

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

## AIR DET generator (zkVM chips)

Code: `scripts/zk_registry/air_*.py` (driver `air_det.py`, generator `boole-zk-registry-air-det`) and the Rust
extractors in `scripts/zk_registry/air_harness/` (a standard-library IR writer plus one adapter per zkVM, built in a
scratch copy of the zkVM's workspace at the census pin). Each adapter runs the zkVM's own symbolic builder over every
AIR: SP1 v6.8.1 through a recording builder over p3-uni-stark `SymbolicExpression` (its own `SymbolicAirBuilder`
labels preprocessed cells as main cells; counts are cross-checked against `Chip::num_constraints` / `sends` /
`receives`), Pico v2.1.2 through `SymbolicConstraintFolder` (lookup counts cross-checked against `MetaChip`), OpenVM
v2.0.2 through the keygen vkey DAG of `SdkVmConfig::standard()` and of the leaf aggregation circuit (a negated count is
a receive). Every constraint polynomial (with `when(..)` guards and first / last / transition selectors as factors) and
every bus interaction (direction, bus, values, multiplicity) is written as a hash-consed DAG (`boole-air-ir/v1`). Real
rows come from each zkVM's own trace generation on sample programs (straight-line programs, in-tree ELFs, linear
recursion programs).

Statement: one row window (two rows when a constraint references the next row) over `F = ZMod p` (KoalaBear for SP1
and Pico, BabyBear for OpenVM):

```lean
theorem det [Fact (Nat.Prime p)] :
    ∀ w₁ w₂ : Fin nVars → F, Constraints w₁ → Constraints w₂ → Assumptions w₁ → Assumptions w₂ →
      (∀ i ∈ Fixed, w₁ i = w₂ i) → BusEq (In w₁) (In w₂) → BusEq (Out w₁) (Out w₂) := by
  sorry
```

`Fixed` are the preprocessed cells, public values and row selectors; `In` / `Out` are bus messages `(multiplicity,
values)` and `BusEq` compares contributions (equal multiplicities; equal values where the multiplicity is non-zero).
A per-zkVM bus model (`air_bus.py`) assigns every interaction a role from the bus conventions at the pin: memory
accesses consume the previous state and produce the current one (SP1 and Pico *send* the previous state, OpenVM
receives it), call buses that carry the request and the result in one message are split (the result is an output of
the AIR that serves the call and an input of the caller), Global messages follow their `is_send` / `is_receive`
fields, program and instruction lookups are inputs, and lookups into tables another chip provides (SP1 byte / range,
Pico byte, OpenVM variable range / bitwise / range tuple) are hypotheses `m ≠ 0 → Table values = true` with the table
written in Lean and, independently, in Python. A table AIR's own received lookups and the result of a call an AIR
serves are never assumed. Row selectors are modelled normalized (every constraint is checked to be homogeneous in
each selector). Bus balance (LogUp) is not modelled.

Gates: G-ELAB; G-FID (≥ 10 distinct real windows and ≥ 10 single-cell mutants; Lean's `decide (Constraints w)` and
`decide (Assumptions w)` equal the Python evaluator on every window, and every real window satisfies both);
G-NONVAC (a real window with an active output message satisfies the model); G-TRIV (battery P1 + P2 over the AIR
statement); DET-SEARCH (single-variable, linear-kernel and re-solve searches, confirmed in Python and Lean). A
counterexample on a one-row window is `DET-FALSE-CANDIDATE` (private); on a two-row window it is labelled
not-a-finding (the window omits the row's history) and the record is GATE-FAIL. Size policy: 4,000 constraints and
100,000 expression nodes. `coverage` records public machine-checked artifacts at the same code (sp1-lean, openvm-fv;
pico-fv only for chips unchanged since its pin) so issuance can exclude answered items. Schema
`schema/air_problem.schema.json`; the checker accepts both schemas.

## Noir DET generator (ACIR)

Code: `scripts/zk_registry/noir_*.py` (driver `noir_det.py`, generator `boole-zk-registry-noir-det` 1.0) and the
Rust decoder/executor `scripts/zk_registry/noir_tool/`. It reuses the package format, `problem.json` schema (with
additive enum values), checker, G-ELAB, the TRIV battery and the reporting of the Circom generator; its sources are
hashed separately (`noir_det.generator_info`).

**Population.** Ledger rows with `framework == "noir"`, excluding units ending in `-constraint-site` /
`-interaction-site`, `C2-operation`, `U1-instruction`, and rows whose flags contain any of `test`, `not-counted`,
`deprecated`, `NOT-ITEMIZED`, `ZERO-ITEMS`, `supplementary`, `exported-api-false`, `intrinsic`, `generated`,
`duplicate-of`, `copy-of`, `vendored`, `mapped-to`, `archived`, `excluded`, `secondary`, `program-as-circuit`
(`noir_det.SELECTION_FILTER`). Rows are deduplicated by a content key before packaging: sha256 over the compiler and
the comment-free text of the function, its `impl` header and every declaration of its crate (and path dependencies)
that it reaches by name, transitively. Every row ends in one terminal status: OPEN, GATE-FAIL, DET-FALSE-CANDIDATE
(private), TOO-LARGE, NO-INSTANTIATION, COMPILE-FAIL or NOT-APPLICABLE, each with a reason.

**Compiler selection** (`noir_toolchain.REPO_COMPILERS`, every attempt recorded): aztec-packages uses its
`noir/noir-repo` submodule commit (= tag v1.0.0-beta.25); the noir stdlib the release tag of its pin (v1.0.0-rc.2);
other repositories their toolchain pin (CI `noirup` matrix, README `noirup -v`, a constants file, the noir crates their
Rust code pins, or the aztec-packages release their Noir dependencies are tagged with, whose `noir/noir-repo` source is
built); a repository without any pin gets the newest release published before its pinned commit (recorded as
unpinned). Release assets are checked against the release page digest where one is published (otherwise the asset
digest is recorded); source builds use `cargo build --release --locked -p nargo_cli`. `nargo` runs with `HOME` in
scratch and `GIT_ALLOW_PROTOCOL=file`: git dependencies are fetched beforehand at their tags (sparse for
aztec-packages), so a compile never reaches the network.

**Instantiation** (`noir_instantiation.py`). Library functions get an `#[export]` wrapper appended to their own source
file (so private items and the file's imports resolve; binary crates and contract-crate modules are compiled through a
library copy of the crate) and are compiled by `nargo export`; stdlib items get a wrapper crate that imports the item's
names by public paths; `fn main` of a binary crate is compiled as the repository builds it; private Aztec contract
entrypoints are taken from the compiled contract artifact. The wrapper's parameters are the function's parameters
(`self` and `&mut` parameters by value); it returns the return value followed by the final values of every `&mut`
parameter. Generic parameters (function and `impl`) are assigned by tiers, first tier with a compiling candidate wins:
`parameter-free` → `repo-main` / `repo-test` / `repo-derived` (turbofish arguments of calls and concrete
instantiations of the `impl`'s self type written in the repository, classified by the enclosing function; global
constants substituted) → `probed` (numeric 4, 2, 1; type parameters: the repository's implementors of their trait
bounds, else Field, u32, u8, bool; a counterexample there is labelled not-a-finding). Within a tier the largest
compiled instantiation within the size policy (2,000 flattened ACIR opcodes) is chosen. Unconstrained, comptime,
oracle/builtin declarations and Aztec public/utility functions are NOT-APPLICABLE; functions with closure or slice
parameters, contract-block helpers and private stdlib items have NO-INSTANTIATION.

**ACIR extraction.** `boole-acir-tool` is built as an added member of the noir workspace at the same version (its
`acir`, `acvm`, `noirc_abi` crates and lockfile), so the program is deserialized with the compiler's own serialization
code and printed as serde JSON; `noir_acir.normalize_program` maps every version's shapes (hex or byte field elements,
old/new function inputs, memory operations as expressions with predicates or as `read` flags that hold `Write`) to one
form, and ACIR calls are inlined (callee witnesses after the caller's).

**Model and statement** (`noir_lean_emit.py`, BN254 `ZMod p`). AssertZero as polynomial equations; RANGE as
`val < 2 ^ n`; AND/XOR as `&&&` / `^^^` on `n`-bit operands; hash and curve black boxes as uninterpreted functions
`bb k inputs` shared by both assignments (stated assumption: each is a deterministic function of its inputs, as ACVM
computes it; with a witness predicate the call constrains only when the predicate is non-zero); memory blocks by
`memRun` (initial witnesses, then reads/writes in program order with the index below the block length); Brillig call
outputs are free. Inputs are the parameter witnesses, outputs the return witnesses; the statement is the Circom
template, prefixed by `∀ bb : BlackBox` when black boxes occur.

**Gates.** G-ELAB (shared); G-NONVAC: an execution of the compiled program by the compiler's own ACVM solver (the
`nargo execute` engine, through boole-acir-tool; oracle calls answered with zeros of the declared shapes) on derived
inputs or the repository's Prover.toml is accepted by the Python evaluator and by Lean; G-FID: every real witness (at
least 10, 1 without parameters) and 16 single-witness mutants are evaluated by Lean (`decide`, black boxes as the
table of real values, mutants' new black-box inputs re-solved by the compiler's solver) and by the independent
Python evaluator; G-TRIV: battery P1 (one file, 33 forms) + P2 (7 forms), run when the other gates pass and the
function has outputs; DET search: output mutation, the AssertZero-Jacobian kernel, re-solve from free witnesses
(Brillig outputs first), and re-execution with pseudo-random oracle answers; candidates are confirmed under the real
black-box semantics and in Lean. `crosscheck` compares `nargo execute` with the tool's executor on the same artifact
and inputs; `decompose` splits TOO-LARGE functions into the crate functions they call (ledger rows and content-equal
functions are mapped, the others instantiated, up to 4 levels).

## gnark DET generator (Go circuits and gadgets)

Code: `scripts/zk_registry/gnark_*.py` (driver `gnark_det.py`, generator `boole-zk-registry-gnark-det` 1.0) and the Go
tool `scripts/zk_registry/gnark_tool/` (`catalog/`: a go/packages + go/types catalog; `harness/`: builders, sampling,
export; `main.go`: the `gnarkx` runner). It reuses the internal R1CS representation, the Lean template, G-ELAB, the
battery, the counterexample search, the package format, the checker and the `problem.json` schema (additive values).

**Population.** Ledger rows with `framework == "gnark"`, with the unit and flag exclusions of the Noir generator
(`gnark_det.SELECTION_FILTER`), deduplicated by a content key (the comment-free declaration, every module function it
reaches through calls, the underlying types its signature reaches, the Go and gnark versions). Every row ends in one
terminal status: OPEN, GATE-FAIL, DET-FALSE-CANDIDATE (private), TOO-LARGE, NO-INSTANTIATION, COMPILE-FAIL or
NOT-APPLICABLE, each with a reason.

**Toolchain.** Each repository is built at its ledger pin with the Go version its `go.mod` requires: the installed Go
when it satisfies the `go` directive, otherwise a digest-checked official release in scratch (`GOTOOLCHAIN=local`;
`GOPATH`, `GOMODCACHE`, `GOCACHE` in scratch; modules from the public proxy only). The wrapper tool is a Go module that
`replace`s the repository module by the checkout (with the repository's own `replace` directives); every module the
repository requires must resolve to the version it pins (deviations are recorded). Targets in `internal` packages are
built inside a copy of the repository module. Where the tool does not build against an older gnark API, a sizing
program inside a copy of the module compiles the circuit with gnark's production builder and records its constraint
count (TOO-LARGE above the policy, otherwise COMPILE-FAIL).

**Instantiation** (`gnark_instantiation.py`). A wrapper circuit per candidate: its secret fields are the target's
circuit inputs (parameters of circuit-variable types: `frontend.Variable`, emulated elements, points, extension-field
elements, bytes, and arrays / slices / pointers of those, and a data receiver); `Define` constructs gadget objects with
the package's in-circuit constructor (parameters supplied by zero-argument providers, recursively by constructors, or by
constants of call sites), calls the target and passes pointers to its results to `harness.Expose`, which copies every
circuit variable of the results into a new wire (a hint) constrained equal to it: those wires are the outputs, all
public and secret variables are the inputs. Choices by tier, first tier with a compiling candidate wins, largest model
within the size policy (2,000 constraints): `parameter-free` → `repo-test` (type arguments of concrete instantiations,
constants and interface implementations at call sites in test files) → `repo-derived` (the same in non-test files) →
`probed` (slice lengths 2 and 4, constants 2 / 8, false / true; a counterexample there is labelled not-a-finding). The
native field is the curve the package's tests compile with (BN254 when they name it or none). Constructors and functions
without circuit-variable inputs or outputs are NOT-APPLICABLE; parameters of function or `any` type, generic types
without a concrete instantiation in the repository and declarations in package `main` have NO-INSTANTIATION.

**Commitment model** (decision (b), sound modelling of the challenge). gnark's range checks and lookups use a
commitment (Fiat–Shamir challenge) when the builder implements `frontend.Committer`, and emulated arithmetic checks
every multiplication by a random evaluation of a polynomial identity; a challenge modelled as a free wire would make
DET spuriously false. The model builder (a wrapper around gnark's `r1cs` builder, the extension point gnark documents)
implements `frontend.Rangechecker` by gnark's own non-commitment checker (`rangecheck.New` on a view without
`Committer`, i.e. bit decomposition: the predicate `value < 2^bits` itself), and implements `Committer` twice: in a
*symbolic* compile the commitment is a challenge wire, and the challenge-dependent part of the system must be a
polynomial identity of degree D in it (every challenge-dependent wire is defined by its own constraint; checked by the
Go tool and again in Python); the model is then the common pre-commitment system plus the challenge-dependent parts of
D + 1 *constant* compiles at distinct fixed challenges 2, 3, ..., which is equivalent to the identity holding for
every challenge (the intended semantics; no fact the gadget enforces is assumed). Checks that are not polynomial
identities (log-derivative lookups: `logderivlookup`, the byte tables of `uints`, GKR) are not modelled and the row is
COMPILE-FAIL with that reason. Every package records the production constraint count (gnark's own builder) next to the
model's.

**Statement.** The Circom template over gnark's compiled constraint system (`F = ZMod p`, p the native field).
Outputs that are emulated field elements are compared by value, since gnark keeps them in non-canonical limb form:
`EmulatedOutputs` lists (limb wires, limb width, modulus) and the conclusion is
`(∀ o ∈ Outputs, w₁ o = w₂ o) ∧ ∀ g ∈ EmulatedOutputs, emValue w₁ g.1 g.2.1 % g.2.2 = emValue w₂ g.1 g.2.1 % g.2.2`;
without emulated outputs the statement is exactly the Circom one. Hint outputs are free wires (unconstrained prover
inputs); `statement.assumptions` states the range-check and commitment model.

**Gates** (cheapest first). G-ELAB; G-NONVAC: a solution of gnark's solver for the compiled system(s) on a sampled
assignment (gnark-crypto domain samplers for curve points, GT elements and bytes; profiles for native variables) is
accepted by the Python evaluator and by Lean; G-FID: every real witness (at least 10) and 16 single-wire mutants are
evaluated by Lean (`decide (Constraints w)`) and the Python R1CS evaluator, the emulated output values are computed in
Lean and in Python, every real witness is a solution of gnark's solver, and gnark's test engine's verdict on each
assignment is recorded; then the counterexample search (output mutation, linear kernel, re-solve, under the
statement's output relation) and the battery (P2 first, then P1 unless P2 closed the statement). `decompose` maps the
callees of TOO-LARGE records to wave records or new records of exported module functions (up to 4 levels).

## ZoKrates DET generator

Code: `scripts/zk_registry/zokrates_*.py` (driver `zokrates_det.py`, generator `boole-zk-registry-zokrates-det`
1.0). It reuses the Circom R1CS path directly: ZoKrates's own `--r1cs` export (recent releases) is byte-identical
to circom's iden3 format, so the unmodified circom `.r1cs` parser and Python R1CS evaluator, the Lean template,
G-ELAB, the TRIV battery, the counterexample search, the package format, the checker and the `problem.json`
schema (additive `"zokrates"` values) all apply unchanged; only the model header text and the generics-grounding
planner are ZoKrates-specific. ZoKrates has no tag/precondition system, so every statement is the plain DET form.

**Population.** Ledger rows with `framework == "zokrates"`, with the same unit and flag exclusions as the other
generators (`zokrates_det.SELECTION_FILTER`), deduplicated by a content key (the pinned compiler version and the
comment-free declaration text). Every row ends in one terminal status: OPEN, GATE-FAIL, DET-FALSE-CANDIDATE
(private), TOO-LARGE, NO-INSTANTIATION, COMPILE-FAIL or NOT-APPLICABLE, each with a reason.

**Compiler selection.** One pinned ZoKrates per repository, each a source build (`zokrates_toolchain.build_source`;
Cargo.lock at the pinned tag is the reproducibility pin): the zokrates/zokrates stdlib pins its own commit, exactly
tag 0.8.8 (`cargo build --release --locked -p zokrates_cli`); ethereum-oasis-op/baseline's `lib/circuits` pins
ZoKrates 0.6.1 (its own `zok6.Dockerfile`: `FROM zokrates/zokrates:0.6.1`), which has no release asset for this
host and needs a nightly-only Rust feature (`#![feature(box_patterns, box_syntax)]`) its crate still implements;
built from an already-installed stable rustc with `RUSTC_BOOTSTRAP=1` (a documented escape hatch; no toolchain is
installed by the build). 0.8.8 emits the iden3 `.r1cs`/`.wtns` directly with `--r1cs`/`--circom-witness`
(`zokrates_r1cs.build_native`); 0.6.1 predates that exporter, so the compiler's own human-readable `.ztf`
constraint listing and a plain-text witness are parsed and renumbered into the same wire layout
(`zokrates_legacy.build_model`: 0 the constant one, then outputs, public inputs, private inputs, internal wires,
exactly circom's convention). Both compilers accept the same flattened positional `-a v0 v1 ...` argument
encoding for every ABI shape (field, bool, uN, array, struct), confirmed on real compiles of both, so one sampler
serves both (`zokrates_witness.py`, reusing `witness.py`'s strategies, one artificial signal per flattened ABI
leaf). ZoKrates's own directive/solver outputs (division, bit-decomposition, ...) are never exported as R1CS
wires unless a later real constraint also mentions them: ZoKrates's own writer allocates a wire only for a
variable some constraint uses, so unconstrained hint outputs are free in the model automatically, with no
bookkeeping needed.

**Instantiation** (`zokrates_source.py`, `zokrates_instantiation.py`). A ZoKrates function's only shape parameters
are its generic constants (`def f<N, P>(...)`; everything else is a witness value, sampled, not fixed here).
`main` with no generics compiles as the repository's own file; every other declaration (including an overloaded
`cast`-style function, disambiguated by its ledger `name@L<line>` symbol against the exact source line) gets a
generated wrapper that imports it under a fixed alias (copying the exact import line the original file used for
any struct type its substituted signature names) and calls it with the chosen generics. Tiers, first tier with a
compiling candidate wins: `parameter-free` → `repo-call` (an explicit turbofish `name::<1, 2>(...)` call site in
the same file, or an array-literal argument whose length grounds a generic named in that parameter's type) →
`probed` (the function's own `assert(G1 == K * G2)` relation, when stated, grounds one generic from the other
instead of probing independently — the stdlib's bit-width casts all state one; otherwise small values
{1,2,3,4,8,16,32,64}, respecting an `assert` bound on a generic when the source states one; a counterexample
there is labelled not-a-finding). Within a tier the largest compiled candidate within the size policy (2,000
constraints) is kept.

**Statement.** The Circom template over the compiled R1CS (`F = ZMod p`, p the ZoKrates curve's scalar field);
inputs are the wrapper's flattened arguments, outputs its flattened return value, both in ABI declaration order.
`statement.assumptions` states field primality and that ZoKrates directive outputs with no constraint are free.

**Gates**, identical order and budgets to Circom: G-ELAB; G-NONVAC / G-FID (the compiler's own solver on sampled
flattened arguments, accepted by the Python evaluator and by Lean's `decide (Constraints w)` on every real witness
and 16 single-wire mutants); G-TRIV (battery P2 then P1); a sound counterexample search.

## halo2 DET generator (circuits and gadgets over the concrete layout)

Code: `scripts/zk_registry/halo2_*.py` (driver `halo2_det.py`, generator `boole-zk-registry-halo2-det` 1.0) and
`scripts/zk_registry/halo2_harness/` (Rust: `export/` the MockProver exporters, `wrappers/` the wrapper circuits per
adapter). It reuses the Circom statement template, G-ELAB, the TRIV battery, the package format, the checker, the
`problem.json` schema (additive `"halo2"` values), the AIR term printer and the AIR counterexample search.

**Population.** Ledger rows with `framework == "halo2"`, with the unit and flag exclusions of the other generators
(`halo2_det.SELECTION_FILTER`), located by `git show` at their pins and deduplicated by a content key (symbol, census
line and the comment-free text of the file). The `plonky3-AIR+halo2` rows are OpenVM chips (AIRs, extracted in wave
Z0 through OpenVM's keygen) and its verifier contracts, not halo2 circuits; they are not taken. Every row ends in one
terminal status: OPEN, GATE-FAIL, DET-FALSE-CANDIDATE (private), TOO-LARGE, NO-INSTANTIATION, COMPILE-FAIL or
NOT-APPLICABLE, each with a reason (`halo2_targets.py` holds every rule: proof-system, transcript, commitment-scheme,
security-parameter and native or Solidity verifier code is NOT-APPLICABLE; witnessing instructions, whose result is a
prover-supplied `Value` by design, and configuration / table-loading functions are NOT-APPLICABLE; in-circuit SNARK
verification (aggregation, recursion), generic circuit builders and interpreter circuits have NO-INSTANTIATION).

**Extraction.** One adapter per repository build. The `halo2_proofs` the repository builds against gets the exporter
in a scratch copy (`src/dev/boole_export.rs`, plus one call in `MockProver::assign_advice` that records the region and
annotation of every advice assignment); it reads the prover after `MockProver::run`: the gates and lookups after
selector compression (as keygen builds them), all fixed / advice / instance values, the permutation cycles and the
regions. Two MockProver lines are covered: zcash 0.3 (zcash/halo2, orchard, darkfi's fork, the qed-it forks) and
scroll-tech v1.0 (the 2022-09 privacy-scaling-explorations fork; multi-phase, challenge-dependent constraints are
rejected). A path dependency is instrumented in place; a crates.io or git dependency is replaced through `[patch]` by
an instrumented copy of the locked source. The wrappers are `#[test]`s injected as child modules of the chip or of the
chip's own test module, so they reuse the repository's test configuration (columns, equality, fixed bases, hash
domains, K), or a small vendored crate (a repository's gadget sources verbatim, or a runner that depends on the
checkout) built with the repository's Cargo.lock. Toolchain: the pinned rustup toolchain when installed, otherwise
the installed stable (recorded as a deviation).

**Wrappers and I/O.** A wrapper allocates its I/O columns first, loads its inputs from instance cells (or binds the
cells an instruction witnesses to instance cells), calls the instruction and copies every result cell into the region
`boole-outputs`. Cells an instruction witnesses from its `Value` parameters (e.g. the second element and the flag of
a conditional swap) are inputs named by (region, annotation). A chip row exercises every result-producing
instruction of the chip on the wrapper's inputs; a whole-circuit row runs the repository's own circuit and instance
from its tests (no result cells: it is measured, and TOO-LARGE above the policy or GATE-FAIL with no outputs).

**Model and statement.** The flattened circuit over the concrete layout of `MockProver::run` at the recorded k: every
gate polynomial at every row of the domain (rotations modulo n, fixed cells and compressed selectors substituted as
constants, instances that fold to 0 dropped, constant-false items recorded), every lookup at every usable row as
membership of the input tuple in the table over the usable rows (fixed tables are constants: a one-column table equal
to {0, .., m-1} is `x.val < m`, others are `Table<k>` lists; advice-defined tables are tuples of model terms, exact,
no hypothesis), every permutation cycle as equalities (fixed cells as constants). Variables are the advice and
instance cells the constraints reference; unreferenced cells are free. `Inputs` are the instance cells and the named
witnessed cells, `Outputs` the cells of `boole-outputs`. Fields: Pallas base (zcash line), BN254 `Fr` and the Pasta
field (scroll line), as the circuit is defined over. The statement is the Circom template over `nWires` cells. Size
policy: 2,000 items (gate instances + copies + lookups).

**Gates** (cheapest first). One sample sizes the layout (TOO-LARGE stops there); then 16 sampled inputs (boundary
values and random) whose layouts must hash equal; G-ELAB; G-NONVAC (a MockProver-verified run satisfies the Python
evaluator and Lean); G-FID (at least 10 distinct real witnesses, 1 for input-free wrappers, and 16 single-cell advice
mutants: Lean's `decide (Constraints w)`, the independent Python evaluator and `MockProver::verify` with the cell
overridden must agree on every mutant; a panic of `verify` is recorded and counted as a rejection); DET-SEARCH (the
shared single-variable, linear-kernel and re-solve searches, fixed-table lookups as exact predicates; skipped for
advice-defined tables); G-TRIV (battery P2, then P1). `decompose` maps every named region of a TOO-LARGE layout to the
packaged wave records whose wrapper layout has the same region (the instruction that assigns it), marking children of
the same gadget source; no new package and no compositional statement is made.

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

## Wave 1b (generator 1.3 over the unpackaged items)

The four generator improvements above were applied to the 756 UNINSTANTIABLE and TOO-LARGE records of waves 0 and 1
whose outcome they can change, the 611 TOO-LARGE or guard-stopped instantiations were decomposed, and battery P2 was
run on all 367 OPEN packages. Wave-0/1 packages are not modified; superseding records are kept separately.

| waves 0 + 1 records (1,767) | before | after |
|---|---:|---:|
| OPEN | 367 | 303 |
| GATE-FAIL | 191 | 379 |
| DET-FALSE-CANDIDATE (private) | 28 | 29 |
| TOO-LARGE | 392 | 583 |
| UNINSTANTIABLE | 788 | 472 |
| ERROR | 1 | 1 |

- Compiler selection compiled 173 of 208 records (T2046 136 of 159, circom 1 37 of 45); parameter derivation 112 of
  457 ungrounded records plus 8 probed; tag preconditions 23 of 69 (all `binary`; 46 carry project-defined tags).
- Battery P2 closed 133 of the 367 OPEN packages (`grind` / `simp_all` after flattening the input/output lists), which
  became GATE-FAIL.
- Decomposition produced 247 sub-component packages (one per template content): 130 OPEN, 110 GATE-FAIL,
  6 DET-FALSE-CANDIDATE (private), 1 ERROR.

Waves 0, 1 and 1b together: **433 OPEN** DET packages (DET truth unknown for each).

## Recovery R1 (Circom)

The loss classes of the effective Circom state (latest record per item over waves 0, 1, 1b and battery P2; 1,767
templates) were re-run with the R1 generator into a separate collection (`packages/recovery-r1/circom`); every new
record carries `supersedes`, and new packages are deduplicated by (prime, template content, arguments) against all
existing packages (1 duplicate, not packaged again). Gates as in production.

- **Size cap 4,000 constraints** (was 2,000), from a 13-point ladder of TOO-LARGE instantiations (2.2k–55k): G-ELAB
  stays cheap (≤ 92 s); G-FID grows about linearly (2,000 s and 17.8 GB at 55k); the battery bounds memory: 8–10 GB up
  to 4k, 11.5–13.4 GB at 4.7–5.4k, and from 8k a battery file reaches its 16 GB guard. Rule: the largest band whose Lean
  peak stays within 64 GB / 6 concurrent Lean processes and whose package cost stays ≤ 10 minutes.
- **DET** (430 items): OPEN 303 → 403 (+66 from TOO-LARGE, +34 probed), TOO-LARGE 583 → 518, no-instantiation 339 → 0
  (114 compile at no probed value), no-candidate-compiles 38 → 36 classified (17 PLONK custom gates, 6 circom 1 vs. a
  very old pinned circomlib, 4 indexed function-call arguments, 3 genuine T2046, 5 false asserts, 1 include outside
  the repository; 4 fixed by materializing declared circomlib dependencies). Circom DET OPEN with decomposition:
  433 → 533.
- **DET-MOD** over the 518 TOO-LARGE parents: **95 OPEN**, 77 GATE-FAIL (4 DET-MOD counterexamples, not findings),
  30 TOO-LARGE, 316 not built (157 parents above 300,000 constraints, 134 without a reliable mapping — mostly
  anonymous sub-components —, 25 other). The mapping (unoptimized compile, `.sym` hierarchy, input/output directions
  from the declaring templates) keeps exactly the constraints over main-component signals and direct sub-component
  inputs/outputs; parents it cannot map are not packaged.
- DET-FALSE-CANDIDATE among the new DET records (private): 8 in total, 6 of them on probed instantiations
  (`not-a-finding`).

## Wave Z0 (zkVM AIRs: SP1 v6.8.1, Pico v2.1.2, OpenVM v2.0.2)

All AIRs of the three census pins were extracted (0 failures): SP1 122 RISC-V AIRs (supervisor and user variants,
precompiles) and 20 recursion AIRs (compress and wrap machines); Pico 45 RISC-V AIRs and 9 recursion AIRs; OpenVM the
72 AIRs of `SdkVmConfig::standard()` and the 42 AIRs of the leaf aggregation circuit. Real rows came from each zkVM's
trace generation on straight-line programs, in-tree ELFs and linear recursion programs (none for the OpenVM leaf
aggregation AIRs, which need application proofs).

| zkVM | AIRs | OPEN |
|---|---:|---:|
| SP1 v6.8.1 | 142 | 59 |
| Pico v2.1.2 | 54 | 40 |
| OpenVM v2.0.2 | 114 | 56 |
| total | 310 | 155 |

The remaining 155 AIRs are 151 GATE-FAIL and 4 DET-FALSE-CANDIDATE; candidates stay private and are not broken down
by zkVM in tracked documents.

GATE-FAIL is mostly AIRs without an active real row (user-mode, trap and page-permission AIRs, the leaf aggregation
AIRs, lookup tables) and 79 closures by the automation battery; 6 two-row windows were refuted by a window
counterexample (not a finding). 43 OPEN packages carry `coverage: partial` (sp1-lean, openvm-fv, and pico-fv for chips
unchanged since its pin), so issuance can exclude them. Four bus-model corrections were found by the wave's own
counterexample searches (SHA-256 compress chain start, initial memory content on the Global bus, recursion hint
memory, deep sharing in the evaluation harness) and the affected AIRs were regenerated. DET truth is unknown for every
OPEN package.

## Wave N1 (Noir functions of the frozen ledger)

2,197 filtered ledger rows (aztec-packages 1,472, noir stdlib 456, z-imburse 142, payy 67, zkemail.nr 35, self 19,
garaga 6; the 300 provekit rows all carry the `test` flag) were located in their pinned sources (2,197 of 2,197) and
deduplicated by content to 2,184 functions (13 `aztec_sublib` copies of aztec-nr functions). Compilers: v1.0.0-beta.25
(aztec-packages submodule), v1.0.0-rc.2 (stdlib), v1.0.0-beta.14 (payy), v1.0.0-beta.5 (zkemail.nr), v1.0.0-beta.16
(garaga), a source build of aztec-packages-v0.67.0's `noir/noir-repo` (z-imburse) and v1.0.0-beta.26 (self, unpinned).

| status | functions |
|---|---:|
| OPEN | 423 |
| GATE-FAIL (734 closed by the automation battery) | 1,067 |
| DET-FALSE-CANDIDATE (private) | 24 |
| TOO-LARGE | 190 |
| NO-INSTANTIATION | 289 |
| COMPILE-FAIL | 152 |
| NOT-APPLICABLE | 39 |
| total | 2,184 |

- 334 of the 423 OPEN packages contain at least one Brillig call (unconstrained hints, free in the model); 62 use
  uninterpreted black boxes and 97 memory blocks. 98 OPEN packages are on probed instantiations.
- Main losses: 201 functions take a closure parameter (no ABI type), 190 compile above 2,000 flattened opcodes, and
  152 do not compile (names unresolved in instantiated generic code, compiler panics, one repository crate that does
  not compile at its pin). Aztec public and utility functions, unconstrained functions and files outside their crate's
  module tree are NOT-APPLICABLE.
- `nargo execute` and the tool's executor gave identical witnesses on all 36 pairs that both executed (stdlib
  wrappers and aztec-packages kernel / rollup mains on their own Prover.toml).
- Decomposition of the 190 TOO-LARGE functions mapped 304 callee edges to wave records and produced 17 new records
  (1 OPEN).
- 70 stdlib functions carry `coverage: partial` (Lampe Hoare-triple proofs for the v1.0.0-beta.19 copy, function text
  identical at the pin; not a DET statement over ACIR). DET truth is unknown for every OPEN package.

## Recovery R1 (Noir)

Generator 1.1 regenerated all 631 wave-N1 loss records (190 TOO-LARGE, 289 NO-INSTANTIATION, 152 COMPILE-FAIL) into
a separate collection (`packages/recovery-r1/noir`, `noir_det.py recover`); each new record carries `supersedes` and
`evidence.supersedes_record`, the wave-N1 indexes are unchanged, and packages whose Lean model equals an existing
package are listed as `same-model-as` duplicates.

- Size cap 3,000 flattened opcodes (was 2,000), from a size ladder over the TOO-LARGE functions: up to 3,005 opcodes
  every sampled package stayed inside the wave-N1 envelope (gate time ≤ 2,635 s, peak Lean RSS ≤ 12 GB, no memory
  kill); at 3.4–4.8 k four of five took longer or used 13–14 GiB, and from 9.3 k the battery hit the 16 GiB kill.
  Lean concurrency followed a 24 GB budget (2 processes, 8 workers). Records carry `evidence.size_band`.
- Non-ABI parameter types are built inside the wrapper from ABI inputs (struct fields, references through locals,
  function-typed fields from the crate's struct literals, vectors from arrays); closure parameters take a function
  from a repository call site; integer parameters the compiler needs as constants take a call-site literal;
  implementors also come from derives, the Aztec note / event macros and, as a fallback, test code. Code that takes
  an Aztec `PublicContext` is NOT-APPLICABLE (public execution, AVM bytecode).
- No TOO-LARGE program contains an ACIR `Call` opcode (every function is inlined), so no compositional DET-MOD
  template was shipped.

| outcome of the 631 records | records |
|---|---:|
| OPEN (32 new distinct models; 5 duplicate existing models) | 37 |
| GATE-FAIL | 90 |
| DET-FALSE-CANDIDATE (private) | 2 |
| NOT-APPLICABLE (public-execution code) | 90 |
| TOO-LARGE (above 3,000) | 163 |
| NO-INSTANTIATION | 83 |
| COMPILE-FAIL | 166 |

Remaining reasons: compiler internal errors 51, contract-block items of contract crates 28, a crate that does not
compile at its pin 16, stdlib types in non-public modules 36, closures without a usable call site 16, compile-time
arguments without a call-site literal 16, and instantiation errors of the chosen candidates.

## Battery P3 (mechanical propagation), Noir

Code: `scripts/zk_registry/mech_acir.py` (`boole-zk-mech-acir` 1.0; `run`, `solve`, `summary`). A deterministic
solver over the package's own ACIR: the conjuncts are re-rendered and must equal `Model.lean`, then determined wires
are propagated in dependency order (constants, linear steps, mixed-radix bit splits, is-zero gadgets, the
field-to-integer cut with the comparison against p, euclidean division, black-box and AND/XOR congruence, memory in
list mode; Brillig outputs stay free). DETERMINED packages get a generated `Solution.lean` (per-step theorems with
exact `linear_combination` certificates) checked by the production checker; STUCK packages record the undetermined
cone. Run over the 461 effective OPEN Noir packages (wave N1, decomposition, recovery R1; latest record per item):

| outcome | packages |
|---|---:|
| MECH-SOLVED (checker PASS) | 354 |
| MECH-STUCK (all Brillig-dependent) | 107 |
| MECH-PROOF-FAIL / MECH-ERROR | 0 |

- Acceptance: calibration items 3 and 4 PASS; none of the 26 DET-FALSE-CANDIDATE packages is DETERMINED.
- Every OPEN package without a Brillig call is solved; 259 of the calibration's 322 eligible packages are solved.
- Main STUCK classes: a hint multiplied by another undetermined wire 60, unmatched euclidean-division variants 23,
  memory with undetermined index or contents 7, other 17. DET truth stays unknown for STUCK packages; proofs and
  results stay in the operator's local workspace (`MECH-P3-NOIR-REPORT.md`, not tracked).

## Wave G1 (gnark circuits and gadgets of the frozen ledger)

599 filtered ledger rows (gnark std 562 at `cfc7b2f9`, 37 application circuit types of 13 repositories) were located in
their pinned sources (599 of 599) and deduplicated by content to 589 (9 `sw_grumpkin` methods that are textual copies of
the `sw_bls12377` ones, and one duplicated lighter-prover circuit). Go: the installed 1.25.7 where the `go` directive
allows it, otherwise the digest-checked official 1.26.8; gnark std compiled from its checkout with every pinned module
at its version.

| status | records |
|---|---:|
| OPEN | 76 |
| GATE-FAIL (75 closed by the automation battery) | 125 |
| DET-FALSE-CANDIDATE (private) | 2 |
| TOO-LARGE | 258 |
| NO-INSTANTIATION | 36 |
| COMPILE-FAIL | 48 |
| NOT-APPLICABLE | 44 |
| total | 589 |

- All 76 OPEN packages are gnark std gadgets (emulated and native algebra, emulated arithmetic, bits, comparison,
  uints, bitslice, conversion, Poseidon2); 66 contain at least one gadget hint (free in the model), 37 have emulated
  outputs (compared modulo their modulus), 9 hold a commitment challenge at D + 1 points, 5 are on probed instantiations.
- Commitment model: 138 records take a commitment; 99 TOO-LARGE records fit the size policy in gnark's production
  system but not in the model (bit-decomposition range checks and the D + 1 copies of the multiplication checks); 9 rows
  whose checks are log-derivative lookups are COMPILE-FAIL (not modelled).
- Application circuits (36 distinct): 6 TOO-LARGE, 1 NO-INSTANTIATION (package `main`), 29 COMPILE-FAIL (probed slice lengths and
  zero-valued shape fields, modules that do not build at their pin, and repositories pinning an older gnark: the
  wrapper tool would have resolved a newer gnark, so only production sizes are recorded there).
- Decomposition of the 258 TOO-LARGE records: 1,008 callee edges, 602 to wave records, 28 new records (0 OPEN).
- Coverage: the 6 light-protocol v2 circuit rows carry `coverage: partial` (in-repository Lean verification of the
  extracted circuit, not a DET statement over the compiled R1CS); no public machine-checked artifact of gnark std was
  found. DET truth is unknown for every OPEN package.

## Wave K1 (ZoKrates functions of the frozen ledger)

242 filtered ledger rows (`zokrates/zokrates` stdlib 160 at tag 0.8.8, `ethereum-oasis-op/baseline`
`lib/circuits` 82 pinning ZoKrates 0.6.1) were located in their pinned sources (242 of 242) and
deduplicated by content to 242 (no duplicates). Both compilers are source builds: 0.8.8 with the
installed stable rustc; 0.6.1 (no release asset for this host) with an already-installed stable rustc and
`RUSTC_BOOTSTRAP=1` (its crate's nightly-only `box_patterns`/`box_syntax` feature, later removed from
rustc outright, is still implemented and accepted this way). 0.6.1 predates ZoKrates's `--r1cs` exporter,
so its own `.ztf` constraint listing and a plain-text witness are parsed and renumbered into the same
wire layout `zokrates_r1cs`'s native path uses.

| status | records |
|---|---:|
| OPEN | 83 |
| GATE-FAIL (53 closed by the automation battery) | 59 |
| DET-FALSE-CANDIDATE | 0 |
| TOO-LARGE | 48 |
| COMPILE-FAIL | 52 |
| total | 242 |

- By repository: `zokrates/zokrates` 160 rows -> 67 OPEN, 51 GATE-FAIL, 39 TOO-LARGE, 3 COMPILE-FAIL;
  `ethereum-oasis-op/baseline` 82 rows -> 16 OPEN, 8 GATE-FAIL, 9 TOO-LARGE, 49 COMPILE-FAIL.
- Rules of the 142 OPEN/GATE-FAIL records: `parameter-free` 103, `probed` 37 (29 OPEN, `zokrates/zokrates`
  only — 0.6.1 predates generics), `repo-call` 2. 40 of the 83 OPEN packages are library-function wrapper
  packages (not a bare `main`).
- Main losses: `ethereum-oasis-op/baseline`'s 49 COMPILE-FAIL are a relative import that does not resolve
  under the repository's own pinned tree (19, a pre-existing inconsistency in its `lib/circuits` sources),
  a stdlib-relative import naming a path this 0.6.1 stdlib pin does not have (18, e.g. a path later
  reorganized, or a pre-rename `.code` path), and parse errors, curve mismatches or unresolved identifiers
  (12). `zokrates/zokrates`'s 3 COMPILE-FAIL are `cast<N, P>`-style conversions with no generic value that
  both compiles and resolves the intended overload.
- DET-FALSE-CANDIDATE: 0 (the counterexample search ran on every candidate that reached it and confirmed
  none).
- A disk-safety guard (`SIZE_GUARD`) was added after an early trial on `mimcSponge.zok`'s `main<N>` probed
  tier produced an 11 GiB `.r1cs` at `N` = 32 before the guard existed; the guard stops probing a
  strictly larger generic value once a candidate is already far over the size policy. Full report:
  `local-docs/zk-production-v1-2026-09-29/WAVE-K1-REPORT.md` (operator's local workspace, not tracked).

## Wave H1 (halo2 circuits and gadgets of the frozen ledger)

187 filtered ledger rows of 21 repository pins (zcash/halo2 `halo2_gadgets` 0.5.0 41 and `halo2_proofs` 19,
axiom-crypto and scroll-tech snark-verifier 38 + 33, nebrazkp/upa 17, darkfi 8, and 15 more repositories) were located
at their pins (187 of 187) and are 187 distinct by content. The 125 `plonky3-AIR+halo2` rows (OpenVM chips and
verifier contracts) are not halo2 circuits and were not taken. Seven adapters were built (zcash `halo2_gadgets`,
darkfi's gadgets, orchard, qed-it/halo2, qed-it/orchard, scroll-tech/poseidon-circuit, scroll-tech/mpt-circuit).

| status | records |
|---|---:|
| OPEN | 14 |
| GATE-FAIL (18 closed by the automation battery) | 26 |
| DET-FALSE-CANDIDATE | 0 |
| TOO-LARGE | 4 |
| NO-INSTANTIATION | 24 |
| COMPILE-FAIL | 12 |
| NOT-APPLICABLE | 107 |
| total | 187 |

- OPEN: 12 `halo2_gadgets` 0.5.0 instructions (ECC addition, incomplete addition, variable-base, fixed-base,
  short and base-field-element scalar multiplication, the Poseidon permutation, Sinsemilla and Merkle
  `hash_to_point` with and without private initial point, Merkle `hash_layer`; 14 to 1,670 items, 7 with lookups
  into the 10-bit range table and the Sinsemilla generator table) and scroll-tech/poseidon-circuit `Pow5Chip` and
  `SeptidonChip` (BN254). The scroll `Pow5Chip` model is identical to the zcash `Pow5Chip.permute` model (same
  model hash; recorded in `DEDUP.jsonl`), so the wave adds 13 distinct OPEN problems. 10 OPEN packages carry
  `coverage: partial` (zcash/ironwood, a Lean 4 hand port of the 0.5.0 chips at Orchard parameters; not a DET
  statement over the layout); no row is dropped because of coverage.
- GATE-FAIL: 18 closures by battery P2 (`grind` / `simp_all`: copies, projections, conditional swap and mux, the
  darkfi arithmetic / select / is-equal chips, orchard `AddChip`) and 8 instructions or chips without result cells
  (assertions and range checks; DET vacuous). Every G-FID passed on the packaged models (Lean, the Python evaluator
  and `MockProver::verify` agreed on all mutants) except a boolean range check with two distinct inputs.
- TOO-LARGE (measured): the Orchard Action circuit (12,779 items), qed-it's `EccChip` test circuit (10,654) and its
  vanilla / ZSA action circuits (12,779 / 14,657). `decompose` maps their 156 named regions to wave records (98
  mapped, 26 to the same gadget source).
- NO-INSTANTIATION: in-circuit SNARK verification (aggregation, recursion, the halo2 loader; 14), generic circuit
  builders and frameworks (7) and interpreter circuits (zkas VM, Vamp-IR backend, ezkl graph; 3). NOT-APPLICABLE:
  native and Solidity verifiers, proof-system components and security parameters (95), witnessing instructions (6)
  and configuration / table loading (6).
- COMPILE-FAIL: scroll-tech/mpt-circuit (a dependency needs the removed nightly feature `slice_group_by`; the pinned
  nightly is not installed) and 11 rows on halo2 lines without an exporter adaptation (PSE v0.3.0, summa-dev,
  scroll-tech develop, zkonduit, axiom, midnight).
- DET truth is unknown for every OPEN package. Full report: `local-docs/zk-production-v1-2026-09-29/WAVE-H1-REPORT.md`
  (operator's local workspace, not tracked).

## Battery P3 (mechanical propagation), AIR and halo2

Code: `scripts/zk_registry/mech_air.py` (tool `boole-zk-registry-mech-p3` 1.0; `python3 -m zk_registry.mech_air run`
over the wave indexes, `solve` for one package) and `scripts/test_zk_registry_mech_air.py`. A deterministic program,
not a model: it reads a package's own IR (the AIR DAG, the halo2 layout export), first regenerates `Model.lean` from it
with the production emitters and requires byte identity, then propagates "equal in both windows" from the `Fixed`
cells (AIR) or `Inputs` (halo2). Input messages give their values only in a context where their multiplicity is known
non-zero; each distinct output multiplicity is decided in the base context and its message values in the context
`m ≠ 0`; a guard is peeled only when known non-zero. Rules: a single unknown cell occurring affinely with a constant
coefficient; limb / bit decomposition by uniqueness over ranged cells; a ranged carry expression as a virtual digit;
functional table rows; small constant-coefficient linear systems. Ranges come only from `x * (x - 1) = 0`, table
lookups as the Lean table definitions state them, and halo2 range tables. A DETERMINED package gets an emitted
`Solution.lean` (statement byte-identical, helper theorems above the doc comment) judged by the production checker:
MECH-SOLVED (PASS), MECH-STUCK (reason), MECH-PROOF-FAIL, MECH-ERROR.

| OPEN packages | MECH-SOLVED | MECH-STUCK | MECH-PROOF-FAIL | MECH-ERROR |
|---|---:|---:|---:|---:|
| SP1 v6.8.1 (59) | 10 | 49 | 0 | 0 |
| Pico v2.1.2 (40) | 7 | 33 | 0 | 0 |
| OpenVM v2.0.2 (56) | 4 | 52 | 0 | 0 |
| halo2 H1 (14; 13 distinct models) | 3 (2 distinct) | 11 | 0 | 0 |
| total (169) | 24 | 145 | 0 | 0 |

- STUCK classes: coupled unknowns, no fact with a single unknown cell (field arithmetic with quotient / carry limbs,
  ECC slopes, running sums, hash rounds) 101; constraint guarded by a factor that may be zero (opcode flags,
  selectors) 29; an output tied to no known input on some branch (input or output multiplicity left free, or no fact)
  12; unknown occurring non-linearly 3. DET truth is unknown for every STUCK package.
- Acceptance: the wave's DET-FALSE-CANDIDATE packages all come out STUCK (the engine claims no false statement).
  12 of the 24 solved packages carry `coverage: partial`. Engine time is under 10 s for all packages; a check takes
  11–65 s at ~7 GB peak RSS. The registry status of the packages is unchanged.

## Battery P3 (mechanical propagation), R1CS family

Code: `scripts/zk_registry/mech_r1cs.py` (`solve` for one package, `batch` over an item list with the production
checker) and `scripts/test_zk_registry_mech_r1cs.py`. A deterministic program, not a model: the R1CS-family packages
keep no constraint system besides `Model.lean`, so it parses the model back (every constraint is re-rendered with the
generator's printer and must equal its text), propagates "equal in both assignments" from the inputs and `w 0`, and
for a DETERMINED package writes `Solution.lean` (statement byte-identical, helpers above the doc comment) for the
production checker. Rules: `lin` (one unknown wire, linear with a constant coefficient once known wires are
substituted), `bits` (bit-decomposition uniqueness: boolean unknowns with coefficients `s * 2^e`, `2^n < p`), `call`
(DET-MOD sub-component outputs once their inputs are known) — the core rules —, and two counted separately: `isz` (the
IsZero pair `X * (β v) = γ o + K`, `(r X) * (δ o) = 0` fixes `o`) and `elim` (reduced row echelon form of the
difference system over the constraints affine in the unknowns, with `b² = b` for booleans; unit rows and boolean rows
with power-of-two coefficients). Every step is one `linear_combination` over an exact integer identity checked by the
program before emission (non-unit coefficients and mod-p residues through `(p : F) = 0`); bit vectors go through one
helper lemma. Run over the effective OPEN packages (latest record per item; probed instantiations included):

| OPEN packages | MECH-SOLVED (core rules / + IsZero / + elimination) | MECH-STUCK | MECH-PROOF-FAIL | MECH-ERROR |
|---|---:|---:|---:|---:|
| Circom DET (533) | 358 (243 / 61 / 54) | 158 | 17 | 0 |
| Circom DET-MOD (95) | 91 (91 / 0 / 0) | 3 | 1 | 0 |
| gnark G1 (76) | 45 (38 / 3 / 4) | 31 | 0 | 0 |
| ZoKrates K1 (83) | 70 (70 / 0 / 0) | 11 | 2 | 0 |
| total (787) | 564 (442 / 64 / 58) | 203 | 20 | 0 |

- Acceptance: calibration items 1 and 2 PASS; none of the 49 known-false packages of these pools (DET-FALSE-CANDIDATE,
  DET-MOD records refuted by the search) is DETERMINED, under every rule.
- STUCK classes (first frontier class per package): a lone unknown occurring squared, no decomposition pinning it
  (big-integer carries and quotients, bit extraction without a full decomposition, path indices) 107; a
  lone unknown with a value-dependent coefficient (inversion, curve addition and doubling, selectors and decoders,
  integer division) 56; a bit decomposition with `2^n ≥ p` (`Num2Bits_strict`-style aliasing, 256-bit unpacking, field
  to integer casts) 36; coupled unknowns only 4. The 20 MECH-PROOF-FAIL checks exceeded 22 GB of Lean memory (16 large
  elimination systems of the big-integer multiplication circuits, 4 other long models); no proof failed otherwise.
  Solving takes under 33 s per package; a check takes 33 s median, 149 s p90 (898 s max) at 7.1 GB median, 11.6 GB p90
  peak RSS.
- ZoKrates K1 packages pin a lake-manifest digest whose file was removed with the wave's scratch; they were checked in
  the same Lean v4.33.1 / Mathlib `0df444a3` environment with that digest substituted (recorded per result), all other
  checks unchanged. Concurrent Lean processes followed a 24 GB budget by measured RSS (3, 2 and 1 by size band); a check
  above its band's guard was retried alone with a 22 GB guard, and MECH-PROOF-FAIL counts the checks that still
  exceeded it. DET truth stays unknown for STUCK packages; the registry status of the packages is unchanged. Proofs and
  results stay in the operator's local workspace (`MECH-P3-R1CS-REPORT.md`, not tracked).

## Ratchet problems (verified circuit optimization)

Code: `scripts/zk_registry/ratchet.py` (problem builder, candidate pipeline, statement generator, final check) with
`ratchet_sim.js` (simulation runner), schema `schema/ratchet_problem.schema.json`, tests
`scripts/test_zk_registry_ratchet.py` (offline fixtures; a live class re-runs the pipeline with circom, node and Lean
when their paths are configured).

**Definition.** A ratchet problem is a Circom reference of the registry with its current **record**. A submission is
a circuit with fewer non-linear constraints (the candidate) plus a Lean proof that it is equivalent to the reference;
an accepted submission becomes the next record (the next rung). The reference meaning is the registry package's DET
model of the reference (its `--O0` constraint system, the model file byte for byte). The reference's own DET must be
machine-checked — the battery P3 proof, a battery closure re-checked as a proof, or (method `det-problem`) an accepted
proof of the reference's issued DET problem, so a solved DET problem unlocks the reference's ratchet — by the production checker within
the proof limits (20,000 MB Lean memory, 30 minutes), and is recorded in the problem (`det`: method, model digest,
proof digest, checker verdict); DET of the reference and the equivalence imply DET of every accepted candidate.
Eligible references: DET machine-checked as above, a record with at least one non-linear constraint, `--O0` within
4,000 constraints, a compiler the registry pins (the release binaries v2.1.9 and v2.2.3, the v2.0.9 source build, or
circom 1), no tagged-input wrapper. A reference without outputs (an assertion circuit) is eligible too; its
equivalence is about the set of accepted inputs. The problem package holds `problem.json` (status `OPEN`), the reference model file and the
reference main texts; it pins a repository snapshot by manifest digest.

**Metric.** The record and every candidate are scored by the number of non-linear constraints of the `--O2` compile
with every main input public: constraints `A * B = C` whose A and B both contain a non-constant wire. Linear
constraints are free (they are substituted away when the template is embedded in a larger circuit), so compressing
linear constraints into quadratic ones never counts; a candidate counts only with strictly fewer non-linear
constraints than the record. Totals are reported alongside. circom writes the `--O2` constraint set in a
run-dependent order, so every `--O2` R1CS is canonicalized (terms by wire, constraints sorted; header, wire numbering
and labels unchanged) before it is counted, digested or modelled.

**Old compilers.** The registry's v2.0.9 source build (digest-pinned) has `--O2` and is used like the releases. circom 1
(the pinned npm package 0.5.46) has no `--O2`: its record is its only optimization, the full constraint reduction (no
`-f`), which eliminates internal signals through linear constraints and never removes a main input or output, public
or private; circom 1 mains keep the template's own public and private inputs (no `{public [..]}`, no `pragma`), and its
reduced R1CS is canonicalized too. The reference and every candidate use exactly the same pinned compiler, flags and
prime. The non-linear count is not independent of the optimization level: linear substitution can make one factor of
a product constant, and the product becomes linear. Compiled at `--O1` as well, 545 of 664 references (the 458 RT1
problems and 206 v2.0.9 references) have the same non-linear count as at `--O2`; on the others `--O2` is lower by a
median of 3 constraints (1.2%; max 255) and never higher. So the comparison rests on equal treatment (same toolchain,
its most simplifying level) rather than on level-invariance, and records of different compilers are never compared.

**Admissibility of a candidate.** It declares a template with the reference's name and number of parameters (the
harness writes `component main {public [<reference inputs>]} = <reference call>;`); it has no `component main` and no
`pragma custom_templates`, and `pragma circom` at most the pinned compiler; it includes only bare `<name>.circom`
files next to it or files of the pinned repository snapshot (the repository's files at the ledger pin that the include
closures of its references reach, plus every `.circom` file of its circomlib dependency; manifest-checked); it is compiled by
the reference's pinned compiler (digest-checked) with the record's flags and prime; its main inputs and outputs equal
the reference's (prime, names, order).

**Statement** (generated per candidate, both directions of the input–output relation, over the reference's field):

```lean
theorem equiv [Fact (Nat.Prime Ref.p)] :
    ∀ x y : List Ref.F,
      (∃ w : Fin Ref.nWires → Ref.F, Ref.Constraints w ∧ Ref.Inputs.map w = x ∧ Ref.Outputs.map w = y) ↔
      (∃ w : Fin Cand.nWires → Cand.F, Cand.Constraints w ∧ Cand.Inputs.map w = x ∧ Cand.Outputs.map w = y) := by
  sorry
```

`Ref` is written out as the registry model's namespace and `Cand` is the model of the candidate's canonical `--O2`
R1CS (production emitter); the doc comment records the reference, record and candidate digests and counts.

**Checks.** `ratchet count` (admissibility, compile, metric), `ratchet simulate` (a screen, not a proof: both
circuits' own wasm witness generators on the same structured edge cases and seeded random vectors, each witness
re-checked against its R1CS; the candidate must reject exactly what the reference rejects and give the same outputs
elsewhere, and every candidate output must occur in a constraint; the battery P3 propagation on the candidate is
reported as an informational DET screen), `ratchet statement` (elaboration and the checker package, status
`CANDIDATE`), and the final `ratchet check`: the candidate pipeline (strictly fewer non-linear constraints) followed by
the unchanged production checker (`check.py`, which reads ratchet checker packages through their schema version; DET
packages validate as before). Candidates and proofs are never stored in the repository.

**Pilot results** (closed-local, one model family, small samples; not a benchmark or public claim):

| Pilot | Targets | Outcome |
|---|---|---|
| P0 (circomlib v2.0.5 gadgets, total-constraint record) | 6 | 1 proved (`MultiMux4(2)`, 34 → 30) |
| P1 (application circuits) | 6 | 2 proved (−2.7%, −50.2%) + 1 metric artifact (fewer constraints in total, more non-linear: the reason for the non-linear metric, now a regression test) |
| P2 (all eligible application circuits, non-linear metric) | 43 | 26 FOUND by the simulation screen (Wilson 95% about 45–74%; 14,133 → 7,586 non-linear over them); 6 of 10 sampled proved (about 31–83%); the 4 failures were proof cost, none shown non-equivalent |

**Limits.** The simulation screen does not detect under-constraint: a hint assigned with `<--` that the witness
generator fills correctly passes it, and only the proof decides (the DET screen is a hint, not a criterion). Proof
memory is the bottleneck: equivalence proofs of references with thousands of constraints or 254-bit coefficients
exceeded 16–20 GB of Lean memory or 30 minutes in P2. Lower-bound arguments for records without a candidate are
informal.

### Noir references

Code: `scripts/zk_registry/ratchet_noir.py` (problem builder, candidate pipeline, statement generator, simulation
screen, final check), schema `schema/ratchet_noir_problem.schema.json` (`zk-registry-ratchet-noir-problem/v1`; the
production checker reads its CANDIDATE packages like the Circom ones), tests
`scripts/test_zk_registry_ratchet_noir.py` (offline fixtures from a toy crate compiled by nargo v1.0.0-beta.25; a live
class re-runs build, count, simulation and the final check with the pinned nargo, boole-acir-tool and Lean when their
paths are configured).

**Reference.** A Noir registry package whose DET is machine-checked as for Circom (battery P3 proof, or a battery
closure re-checked as a proof, production checker within 20,000 MB / 30 minutes) and that the registry compiled
through its `#[export]` wrapper: library functions (the wrapper appended to the function's own file in a copy of the
crate) and standard-library items (a wrapper crate). The reference meaning is the registry's DET model, byte for byte;
the problem pins the exact wrapper text the registry appended (`reference/append.nr`) and the reference program as the
harness rebuilds it (`reference/program.json`: noir version, ABI, bytecode), whose decoded ACIR and ABI must equal the
registry's. Binary mains and contract entrypoints (no wrapper call to replace) and source-built compilers are not
references.

**Metric.** A cost vector of the flattened ACIR, chosen for embedding: `nonlinear` (degree-2 terms of AssertZero
opcodes, i.e. multiplication gates), `range_bits` (RANGE), `logic_bits` (AND / XOR), `black_box` (enabled calls per
black-box function) and `memory` (initialized elements plus operations). Free, and reported: linear-only AssertZero
opcodes (a caller substitutes them; the wrapper's copies are among them), as in the Circom non-linear metric; the
RANGE checks the wrapper's ABI types put on its parameter witnesses (`abi_range_bits`: a caller's values are already
typed, and they are identical in every candidate); and Brillig calls (unconstrained hints cost nothing in proving). A
backend gate count is not used: no proving backend is pinned with the registry's compilers, and a gate count would
charge the linear opcodes too. A candidate counts only if no priced component is larger than the record's and at least
one is smaller: it is then no more expensive under every backend whose cost grows with these components, whatever
their weights (lookup-based range and logic gates included), and trading one component for another (multiplications
for range bits) never counts. `priced`, the sum, is a summary for size bands and reports.

**Candidate and admissibility.** `Candidate.nr` declares `fn boole_ratchet_candidate` (constrained, top level) plus
helper items. The harness takes the reference's wrapper text, replaces its one reference call by
`boole_ratchet_candidate(<the same arguments>)` (a method call passes its receiver first, as `&mut` when the wrapper
binds it mutably) and appends the candidate after it, in the reference's file of the crate snapshot or in the
standard-library wrapper crate. The snapshot is manifest-checked: the Nargo.toml and `.nr` files of the reference
crates and their path dependencies at the ledger pin, and the git dependencies at their tags in nargo's cache layout
(a standard-library snapshot only names the pinned nargo, which embeds the library). The program is compiled by the
reference's pinned nargo (digest-checked) with `nargo export --silence-warnings`; its ABI, input witnesses and number
of return witnesses must equal the reference's. Not admitted: an `unconstrained` or `comptime` candidate function,
`fn main`, `mod`, `contract`, `boole_det_*` names, and attributes other than `inline_always`, `no_predicates`, `fold`,
`allow` and `derive` (so no new oracle, foreign or builtin declaration and no extra export). Unconstrained helpers are
allowed: their outputs are free in the model, so the proof has to pin them down.

**Statement** (generated per candidate):

```lean
theorem equiv [Fact (Nat.Prime Ref.p)] :
    ∀ (bb : ℕ → List Ref.F → List Ref.F) (x y : List Ref.F),
      (∃ w : Fin Ref.nWires → Ref.F, Ref.Constraints bb w ∧ Ref.Inputs.map w = x ∧ Ref.Outputs.map w = y) ↔
      (∃ w : Fin Cand.nWires → Cand.F, Cand.Constraints bb w ∧ Cand.Inputs.map w = x ∧ Cand.Outputs.map w = y) := by
  sorry
```

Black boxes stay uninterpreted and shared: the candidate model numbers its black boxes like the reference's (keys new
in the candidate after them), so the equivalence holds for every interpretation, in particular for the functions ACVM
computes, and DET of the reference gives DET of the candidate per interpretation. The binder is dropped when neither
model has black boxes, and a side without black boxes takes no `bb`.

**Checks.** `ratchet_noir count` (admissibility, compile, cost), `simulate` (a screen: both programs through the
pinned executor, boole-acir-tool `execute` with the compiler's ACVM solver and oracle calls answered with zeros, on
structured inputs per ABI type and seeded random inputs; the candidate must fail exactly where the reference fails and
return the same values elsewhere; every candidate witness is re-checked against its decoded ACIR by the Python
evaluator and every return witness must occur in a constrained opcode; battery P3 propagation is reported as an
informational DET screen), `statement` (elaboration and the checker package) and the final `check` (the pipeline, then
the unchanged production checker). Integer-typed parameters are range-checked by the wrapper identically in both
programs, so values outside their types fail in both; the screen draws ABI-typed values only.

**Wave RT-N1** (closed-local packages, no solving): 490 problems from the 2,201 effective Noir records (aztec-packages
280, noir stdlib 170, payy 30, zkemail.nr 9, garaga 1); DET evidence: battery P3 proof 258, battery closure re-checked
as a proof 232 (all 366 re-checks PASS); every reference rebuilt to the registry ACIR and ABI. Exclusions: no
machine-checked DET 1,077 (by status: COMPILE-FAIL 170, TOO-LARGE 165, GATE-FAIL 393, NOT-APPLICABLE 129, OPEN with
battery P3 STUCK 107, NO-INSTANTIATION 87, DET-FALSE-CANDIDATE 26), free record 511 (222 with only the wrapper's ABI
range checks, 289 with only linear opcodes and Brillig calls), duplicate models 110, source-built compiler 12
(z-imburse), binary main 1. 284 records have non-linear terms; the others are range, logic, black-box or memory cost
only. Priced records range from 1 to 32,695 (median 35).

**Decomposition by instances** (`noir_callees.py`, waves RT-N2 / RT-G2). A TOO-LARGE reference is not a ratchet
reference, but the functions it compiles can be. The parent is recompiled exactly as the registry compiled it (its
wrapper and pinned nargo; the ACIR digest is compared) with `--show-monomorphized`: every constrained function instance
reachable from the entry point (ACIR inlines all of them; calls into unconstrained functions are hints) is a callee
instance, identified by its printed body and everything it calls with ids renamed. An instance is located in the
parent's crate and path dependencies by name and parameter names (the standard library and git dependencies are other
repositories; items of `quote` blocks are skipped); its generic arguments are recovered by unifying the declared types
with the instance's (structs are printed as their field tuples; a tuple-printed argument is named by the structs of the
same shape, ranked by the field names the instance binds; an undetermined numeric generic is guessed from the
instance's literals). A candidate counts only when an `#[export]` wrapper with those arguments prints the very same
instance. Confirmed instances within the size policy (3,000 flattened opcodes) whose model no registry package has
are packaged by the DET generator (all gates; rule `decomposition`, provenance `caller instance:` with the parents), run
through battery P3 and the ratchet builder. The ratchet problem records `reference.decomposition`: the parents and the
meaning — the callee as instantiated; an equivalent cheaper candidate can replace it at its call sites without changing
a parent's input-output relation (no statement about the parent's DET, cost or proof).

**Wave RT-N2** (closed-local, no solving): the 165 TOO-LARGE functions (143 recompiled; 21 z-imburse functions skipped
for their source-built compiler and one contract entrypoint) compile 1,728 distinct callee instances: 478 not in the
parent's crates (standard library, git dependencies, closures), 301 existing records, 111 data movement only and 42
without a priced cost, 100 above 3,000 opcodes, 449 without a confirmed instantiation (346 failing the instance
comparison or the wrapper compile, 103 without a determined assignment), 18 with an existing or repeated model, and
229 new packages (`packages/decomp-n2`: OPEN 72, GATE-FAIL 157; no DET-FALSE-CANDIDATE). Battery P3 solved 60 of the
72 OPEN packages (12 STUCK); 48 battery closures were re-checked as proofs (all PASS). **108 new Noir ratchet problems**
(aztec-packages 106, payy 2) from 88 parents; DET evidence battery P3 proof 60, battery closure 48; priced records 1 to
3,046 (median 63), 47 with non-linear terms. Exclusions: no machine-checked DET 121 (GATE-FAIL failing fidelity or
non-vacuity 83, battery not closing 26, OPEN with P3 STUCK 12).

**Limits.** The screen does not see an under-constrained Brillig hint that the hint code fills correctly, nor negative
values of signed parameters; only the proof decides. Uninterpreted black boxes forbid replacing a hash or curve call
by arithmetic that computes the same function. A trade between components (fewer multiplications for more range bits)
can be a real backend saving but does not count.

### gnark references

Code: `scripts/zk_registry/ratchet_gnark.py` (problem builder, module snapshot, candidate pipeline, statement generator,
simulation screen, final check) with the harness runner `gnark_tool/harness/simulate.go` (`gnarkx simulate`), schema
`schema/ratchet_gnark_problem.schema.json` (`zk-registry-ratchet-gnark-problem/v1`; the production checker reads its
CANDIDATE packages like the others), tests `scripts/test_zk_registry_ratchet_gnark.py` (offline fixtures from a toy
gadget compiled by the harness with Go 1.25.7 against the gnark v0.16.3 module snapshot; a live class re-runs build,
count, simulation and the final check with the pinned Go, a gnark snapshot and Lean when their paths are configured).

**Reference.** A gnark registry package whose DET is machine-checked as for Circom (battery P3 proof, or a battery
closure re-checked as a proof, production checker within 20,000 MB / 30 minutes). The reference meaning is the
registry's DET model, byte for byte: the R1CS gnark's builder compiles from the registry's wrapper circuit (range
checks through gnark's non-commitment checker, a commitment held at D + 1 fixed challenges), emulated outputs compared
by value modulo their modulus. The registry's harness result must assemble to the registry R1CS and re-render to the
model file, and the reference must rebuild from the problem's module snapshot with the pinned toolchain to the same
R1CS and wrapper I/O. The problem package holds `problem.json`, the model file and the registry's wrapper circuit
(`reference/wrapper.go`).

**Metric.** The record is the number of non-linear constraints of that model (`A * B = C` with a non-constant wire in
both factors): the Circom rule, over the statement's own constraint system. Linear constraints are free: a caller
substitutes them, gnark itself keeps linear combinations symbolic until an assertion, and the wrapper's exposure
constraints are linear and the same in every candidate. Commitments follow the registry's sound model: a
challenge-dependent check counts once per challenge point (D + 1 copies), so the model of a commitment-based circuit is
itself a commitment-free R1CS of that size, and moving checks behind a commitment never buys a count that a
commitment-free circuit could not reach. Hint outputs are free wires: a hint saves constraints only where the
remaining constraints still pin the outputs down (the proof decides; the screen flags an output fixed only by a free
wire). Range checks are priced as gnark's non-commitment checker (bit decomposition, one booleanity constraint per
bit) in the record and in every candidate; gnark's production count (log-derivative range checks sharing a lookup
table with the embedding circuit, its Groth16 commitment) depends on the caller, so it is reported, not scored. A
candidate counts only with strictly fewer non-linear constraints than the record.

**Candidate and admissibility.** `Candidate.go` is a file of the reference's own Go package (its unexported
identifiers are in scope) declaring `BooleRatchetCandidate`: a method of the reference's receiver type (methods and
circuit `Define`) or a function, with the reference's signature. The harness adds it to the package of the module
snapshot through a Go build overlay (the snapshot is never written) and renames the one reference call of the
registry wrapper to it (same receiver, type arguments and arguments). The snapshot is manifest-checked: the repository
module at the ledger pin (go.mod, go.sum and every non-test .go file) with `vendor/` from `go mod vendor` (the
dependency versions the repository pins). The build uses the pinned Go toolchain (the official release of the
registry's Go version; `bin/go` and the compiler digest-checked) with `-mod=vendor`, no network, no cgo and a shared
content-addressed build cache. Not admitted: another package clause; zero or several `BooleRatchetCandidate`; a method
where the reference is a function or the reverse; imports outside a fixed list of the Go standard library, the
repository module (not the harness) and the vendored dependencies (so no `unsafe`, `reflect`, `os` or cgo); `//go:`
directives and build constraints. The wrapper's public and secret variables, output paths and emulated output groups
must equal the reference's; a commitment must fit the registry's commitment model (one, a polynomial identity in the
challenge; log-derivative lookups are not modelled and are rejected); the candidate model has at most 4,000
constraints. `init` functions are allowed (hint registration); they run only in the candidate's own build.

**Statement** (generated per candidate):

```lean
theorem equiv [Fact (Nat.Prime Ref.p)] :
    ∀ (x y : List Ref.F) (z : List ℕ),
      (∃ w : Fin Ref.nWires → Ref.F, Ref.Constraints w ∧ Ref.Inputs.map w = x ∧ Ref.Outputs.map w = y ∧
         Ref.EmulatedOutputs.map (fun g => Ref.emValue w g.1 g.2.1 % g.2.2) = z) ↔
      (∃ w : Fin Cand.nWires → Cand.F, Cand.Constraints w ∧ Cand.Inputs.map w = x ∧ Cand.Outputs.map w = y ∧
         Cand.EmulatedOutputs.map (fun g => Cand.emValue w g.1 g.2.1 % g.2.2) = z) := by
  sorry
```

`Outputs` are the native outputs; without emulated outputs the `z` part is dropped (the Circom statement). Emulated
values are compared modulo their modulus because gnark keeps emulated elements in non-canonical limb form; DET of the
reference (under the same output relation) and the equivalence give DET of the candidate.

**Checks.** `ratchet_gnark count` (admissibility, build, compile, metric), `simulate` (a screen: both wrappers through
gnark's own solver, the harness `simulate` runner, on structured edge vectors (every input leaf at 0, 1, 2, its maximum
or half its bound; each input field at its maximum with the others zero and the reverse) and seeded random vectors
with mixed value profiles; curve points and similar inputs are valid samples of their domain; the candidate must be
solvable exactly where the reference is and give the same outputs, emulated ones by value; the solver witnesses of the
first 64 vectors are re-checked against both models by the Python evaluator; the reference is rebuilt and compared
with its model digest; an output fixed only by a free wire is a mismatch; battery P3 propagation is reported as an
informational DET screen), `statement` (elaboration and the checker package) and the final `check` (the pipeline, then
the unchanged production checker).

**Wave RT-G1** (closed-local packages, no solving): 92 problems from the 617 gnark records of the registry
(wave G1 and its 28 decomposition children), all gnark std gadgets at v0.16.3 (`cfc7b2f9`); DET evidence: battery P3
proof 43, battery closure re-checked as a proof 49 (all 49 re-checks PASS; every DET check within 10.4 GB and 81 s); every reference
rebuilt from the module snapshot (2,358 files, 29 MB) with the official Go 1.25.7 to the registry R1CS (the registry
used a Homebrew build of the same version). Exclusions: no machine-checked DET 480 (by status: TOO-LARGE 258,
NOT-APPLICABLE 53, COMPILE-FAIL 48, NO-INSTANTIATION 37, OPEN with battery P3 STUCK 31, GATE-FAIL 51 (42 failing the
fidelity or non-vacuity gates; 9 wrappers without exposed results: 7 assertion gadgets whose vacuous DET was never
proved and 2 that accept every input), DET-FALSE-CANDIDATE 2), linear record 42 (battery closures of additions,
negations, selections, copies and constants), duplicate models 3. Records range from 2 to 1,537 non-linear
constraints (bands 1-31 / 32-99 / 100-499 / 500-999 / 1000+: 42 / 11 / 12 / 16 / 11); 27 have emulated outputs and
none a commitment; in the 43 records with range checks the bit decompositions (23,842 bits) dominate the count.

**Decomposition by instances** (`gnark_callees.py`, `gnark_tool/callees`, wave RT-G2): the TOO-LARGE record's function
at its recorded type arguments is the root of the call graph of the instantiated code (go/ssa with generics
instantiated, Rapid Type Analysis: static calls, closures and interface calls resolved by the types the code creates);
every reachable function of the module is a callee instance with the type arguments of its instance. Exported ones
outside internal packages become generator rows with those type arguments forced (tier `decomposition`, provenance
`caller instance:`; other parameters by the usual tiers, a probed slice length or constant keeps the `probed` label)
under the gnark ratchet's 4,000-constraint size policy (`max_constraints`; wave G1 used 2,000; references are rebuilt
under their own policy). Problems record `reference.decomposition` with the meaning stated for Noir.

**Wave RT-G2** (closed-local, no solving): 252 of the 258 TOO-LARGE records (the 6 application circuits of other
modules are skipped) reach 2,028 distinct callee instances: 1,219 unexported, 9 in internal packages, 395 existing
records and 405 new generator rows (`packages/decomp-g2`: OPEN 114, GATE-FAIL 128, NOT-APPLICABLE 61,
NO-INSTANTIATION 59, TOO-LARGE 34, COMPILE-FAIL 9; no DET-FALSE-CANDIDATE). Battery P3 solved 90 of the 114 OPEN
packages (24 STUCK); 6 battery closures were re-checked as proofs (all PASS). **89 new gnark ratchet problems** from
165 parents, mostly emulated-field arithmetic at the curve fields the pairings and curves use (`math/emulated` 74,
`sw_emulated` 8, `uints` 5, `sw_grumpkin` 2); DET evidence battery P3 proof 83, battery closure 6; records 3 to 3,069
non-linear constraints (median 381), 68 with emulated outputs, 3 on probed instantiations. Exclusions: no
machine-checked DET 240 (NOT-APPLICABLE 61, NO-INSTANTIATION 59, GATE-FAIL 53, TOO-LARGE 34, OPEN with P3 STUCK 24,
COMPILE-FAIL 9), linear record 73, model equal to an existing package 3.

**Limits.** The screen does not see an under-constrained hint that the solver fills correctly, and emulated inputs are
sampled canonical; only the proof decides. Pricing range checks as bit decomposition overstates their production cost
(gnark's log-derivative argument), equally for the record and every candidate; a candidate that trades range checks
for multiplications is scored by the model count. The pinned toolchain is a darwin-arm64 Go release; another platform
needs its own release of the same version and a rebuild check.

### zkVM AIR references

Code: `scripts/zk_registry/ratchet_air.py` (problem builder, candidate pipeline: overlay rules, build and extraction,
admissibility, statement generator, simulation screen, final check), schema `schema/ratchet_air_problem.schema.json`
(`zk-registry-ratchet-air-problem/v1`; the production checker reads its CANDIDATE packages like the others), the
extractors' ratchet mode (`air_harness/`: `BOOLE_RATCHET_AIRS`, `BOOLE_AIR_SEED`; unset, the registry behaviour is
unchanged), tests `scripts/test_zk_registry_ratchet_air.py` (offline fixtures of a toy SP1-style AIR; a live class
re-runs build, extraction and the screen on a real problem when its paths are configured).

**Reference.** A wave Z0 AIR package whose DET is machine-checked as for Circom and Noir (battery P3 proof, or a
battery closure re-checked as a proof; production checker within 20,000 MB / 30 minutes), with a one-row window and
real rows (G-FID PASS). A one-row AIR constrains every row on its own, so an equivalence of row windows is an
equivalence of traces row by row; a two-row window relation does not compose to the trace, so two-row AIRs are not
references. The reference meaning is the registry DET model (one row window of the extracted constraint and
interaction DAG with the bus model's roles), byte for byte; the problem pins the registry IR, the machine record
(every AIR's IR content digest at the pin) and the reference's bus contributions on the sample programs. The workspace
snapshot is the zkVM's tracked files at the census pin with the registry extractor installed (manifest-checked,
symbolic links pinned); toolchains as in wave Z0 (installed rustup toolchains, deviations recorded).

**Metric.** A proving-cost vector of the extracted AIR: `main_columns` (trace width: committed and low-degree-extended
cells per row), `interactions` (bus sends and receives, table lookups included: LogUp terms per row), `constraints`,
`constraints_deg_ge[k]` for every k ≥ 2 (cumulative, so lowering a degree never makes a component larger; the largest
k is the maximum degree, which sets the quotient domain) and `interaction_degree` (the maximum degree of a
multiplicity or message value). A candidate counts only if no component is larger than the record's and at least one
is smaller: it is then no more expensive under every prover cost that grows with these components (commitment, LogUp
and quotient work alike), and trading columns for higher-degree constraints or for lookups never counts. Preprocessed
columns are fixed by admissibility. `priced` (columns + interactions + constraints) is a summary.

**Candidate and admissibility.** A modified chip: an overlay of `.rs` files (the chip's `eval` and, as needed, its
trace generation) on the snapshot, under the problem's editable roots (SP1 `crates/core/machine/src/`,
`crates/recursion/machine/src/`; Pico `vm/src/chips/`; OpenVM `crates/vm/src/system/`, `crates/circuits/`,
`extensions/*/circuit/src/`), never the extractor, manifests, build scripts or toolchain files, with no `unsafe`,
reflection, `transmute`, `include!`, `env!`, `std::env` / `fs` / `process` / `net` / `io`, `cfg`, `asm!`, `extern`,
linkage attributes, `static mut` or `thread_local` in added lines, and no deletions. The pipeline synchronizes a build
tree with the snapshot plus the overlay (changed files only, with fresh modification times, so cargo rebuilds exactly
the affected crates), builds offline with the snapshot's locked dependencies and the recorded toolchain, and extracts
every AIR with the same harness. Every other AIR must extract to the reference build's IR; the target keeps its name,
position, rust type, field, preprocessed width, public value count, fixed variables (preprocessed cells, public values
and row selectors used) and one-row window, within the size policy; its input and output messages stay the same
(order, direction, bus, scope, number of values, count weight and role: the bus model's roles, with the
negative-multiplicity rule inherited from the matching reference message) and its table lookups go only into tables
the reference uses (dropping or adding lookups is allowed; each is an interaction).

**Statement** (generated per candidate, over the zkVM's field):

```lean
theorem equiv [Fact (Nat.Prime Ref.p)] :
    ∀ (f : List Ref.F) (x y : List Ref.Msg),
      (∃ w : Fin Ref.nVars → Ref.F, Ref.Constraints w ∧ Ref.Assumptions w ∧ Ref.Fixed.map w = f ∧
        Ref.BusEq (Ref.In w) x ∧ Ref.BusEq (Ref.Out w) y) ↔
      (∃ w : Fin Cand.nVars → Cand.F, Cand.Constraints w ∧ Cand.Assumptions w ∧ Cand.Fixed.map w = f ∧
        Cand.BusEq (Cand.In w) x ∧ Cand.BusEq (Cand.Out w) y) := by
  sorry
```

`Cand` is the candidate's model (production emitter, same bus model); each side keeps its own table-lookup
assumptions inside its window relation (the tables are written in Lean), and messages compare by contribution (equal
multiplicities, equal values where the multiplicity is non-zero). DET of the reference and the equivalence imply DET
of the candidate.

**Checks.** `ratchet_air count` (source rules, build, extraction, admissibility, cost), `simulate` (a screen: the
reference and candidate builds' own trace generation in the extractors' ratchet mode, every row of every dumped trace
of the target on the sample programs at seed 0 and on N random executions, the generated programs with other seeds;
every candidate row must satisfy the candidate's constraints and lookups and read the reference's preprocessed and
public values, and each trace must make the same multiset of active input and output contributions; the reference
build must reproduce the problem's recorded contributions; the registry's DET counterexample search runs on the
candidate model from its real rows, and a confirmed counterexample is a mismatch because the reference's DET is
proved; battery P3 propagation is reported as an informational DET screen; dumped traces are bounded per problem, at
most about 2^21 cells per trace and 2^22 per sample program), `statement` and the final `check` (the pipeline, then
the unchanged production checker).

**Wave RT-Z1** (closed-local packages, no solving): 43 problems from the 310 AIR records of wave Z0 (SP1 23, Pico
13, OpenVM 7); DET evidence: battery P3 proof 20, battery closure re-checked as a proof 23 (all 25 re-checks PASS);
every reference re-extracted from its workspace snapshot to the registry IR byte for byte, and every dumped reference
row satisfies the reference model. Exclusions: no machine-checked DET 210 (by status: OPEN with battery P3 STUCK 134,
GATE-FAIL not closed by the battery 72, DET-FALSE-CANDIDATE 4), battery-closed but without real rows 42 (GATE-FAIL:
user-mode, trap and page-permission AIRs and others the sample programs do not exercise), two-row window 9, no trace
within the screen's trace limits 3 (two lookup tables and one wide Pico AIR), duplicate models 3 (SP1 wrap-machine
copies). 7 references have no outputs (lookup and program tables: the equivalence is about the accepted inputs), 14
carry `coverage: partial`. Priced records range from 2 to 1,256 (median 38); 28 have degree-3 constraints.

**Limits.** The screen sees only rows the sample programs exercise and cannot see an under-constrained column that the
trace generation fills correctly unless the DET search finds a counterexample; only the proof decides. The relation is
per row: bus balance (LogUp) is not modelled, the lookup tables are taken as written, and a candidate that changes
the trace layout across rows (two events per row, a different interface) or uses the next row is out of scope.
Several AIRs can come from one Rust chip type (SP1's supervisor and user variants), and every other AIR must stay
unchanged. The extracted symbolic record is the meaning; the source rules forbid the usual means (reflection, `cfg`,
`unsafe`) of a prover-side evaluation that differs from it.

### ZoKrates references

Code: `scripts/zk_registry/ratchet_zokrates.py`, shared checker plumbing `ratchet_native.py`, schema
`schema/ratchet_zokrates_problem.schema.json` (`zk-registry-ratchet-zokrates-problem/v1`), tests
`scripts/test_zk_registry_ratchet_zokrates.py` and `test_zk_registry_ratchet_native.py`. OPEN problems contain no
answer/statement; the candidate pipeline creates a CANDIDATE checker package for the unchanged production checker.

**Reference and metric.** An effective OPEN K1 DET package with accepted evidence produced by `ratchet.det_record`
(`mech-p3`, `battery-closure` or `det-problem`), bound to the exact reference Lean module and theorem. The reference
wrapper is rebuilt with a digest-pinned ZoKrates 0.6.1 or 0.8.8 binary in its source/stdlib snapshot. The actual
`A*B-C` polynomials, wire count and input/output wire numbering must reproduce the DET model. Nonzero field-scalar
normalisation permits different factor spellings of the same equation (including `0*0=C` versus `1*0=C`); this is
not a comparison of counts alone. The record is the native compile's number of nonlinear R1CS constraints using
the shared Circom/gnark classifier. Linear constraints are free; all counts are reported. A zero nonlinear
record is excluded. The native model cap is 4,000 constraints.

**Candidate/admissibility.** One complete UTF-8 `.zok` main program, compiled at the reference's relative directory
in an isolated verified snapshot. Every filesystem import must resolve inside `repo/` or `stdlib/`; absolute,
escaped/symlink-escaped, missing or unrecognised imports are rejected before compilation. Native `EMBED` imports
are allowed. ZoKrates snapshots reject all symbolic links before copying, because the regular-file manifest
does not pin their targets. Snapshot files, compiler binary and stdlib manifest are pinned; a candidate supplies no alternate
compiler or filesystem dependency. Field, scalar input/output names/order/types and public/private input
partition must match the reference ABI. Strictly fewer nonlinear constraints are required by the final pipeline.

**Statement** (both native families use the same input/output-relation shape):

```lean
theorem equiv [Fact (Nat.Prime Ref.p)] :
    ∀ x y : List Ref.F,
      (∃ w : Fin Ref.nWires → Ref.F,
        Ref.Constraints w ∧ Ref.Inputs.map w = x ∧ Ref.Outputs.map w = y) ↔
      (∃ w : Fin Cand.nWires → Cand.F,
        Cand.Constraints w ∧ Cand.Inputs.map w = x ∧ Cand.Outputs.map w = y) := by
  sorry
```

The reference model is copied byte-identically; the candidate is emitted by the production R1CS printer. The
two directions preserve the accepted input domain as well as outputs, including malicious/non-generator
witnesses. Simulation is only a screen: typed edge/seeded inputs, both pinned native witness solvers, independent
R1CS rechecks and comparison of solver acceptance/output vectors. Reference rebuild digest/I/O must still match.
An honest witness screen is not a DET proof or equivalence proof.

**Tools.** With `PYTHONPATH=scripts`, `python3 -m zk_registry.ratchet_zokrates` provides `build`, `count`,
`simulate`, `statement`, `check`. `build` takes `--registry`, `--snapshot`, `--det-record`, `--env-pins`,
`--config` (the compiler pin), `--tools` and `--out`. Candidate commands take `--problem`, `--candidate`,
`--tools`, `--out`, optional `--snapshot`; `simulate --n N` screens N vectors; `statement` and `check` take
`--lean-env`, and `check` additionally `--solution`. `tools/compilers.json` binds each supported version to an
exact binary and style. Candidate `check` recompiles/counts, rejects NOT-SMALLER, generates/elaborates the
statement, validates the bound import digests, then calls the unchanged production checker. Raw proof checking
alone does not establish the separate native cost/admissibility gate.

**RT-K1 qualification.** The private wave builds every reference with accepted P3 DET evidence and a positive
record; old K1 `/tmp` binaries were lost, so the wave explicitly pins replacement official 0.8.8 / source-built
0.6.1 tools and requires exact polynomial/I/O reproduction. The P3 lake-manifest loss/substitution is retained in
DET provenance; it is not permission to forge an environment digest. Private wave reports record counts,
exclusion reasons, size bands, source/tool hashes and reference-as-candidate controls. No upstream defect
fixtures are committed.

### halo2 references

Code: `scripts/zk_registry/ratchet_halo2.py`, shared `ratchet_native.py`, schema
`schema/ratchet_halo2_problem.schema.json` (`zk-registry-ratchet-halo2-problem/v1`), tests
`scripts/test_zk_registry_ratchet_halo2.py` and the shared native tests. DET evidence is bound exactly as for
ZoKrates. A snapshot of the pinned H1 workspace includes the unchanged wrapper/exporter instrumentation and a
locked offline dependency graph. Rustc and Cargo executable SHA256s, lockfile, wrapper/exporter digests and the
snapshot manifest are explicit; the rebuilt native model/I/O/cost must reproduce the reference's DET layout.

**Metric.** Component-wise concrete MockProver-layout vector: domain rows `2^k`, advice columns, fixed columns,
nonzero gate instances after fixed-cell/compressed-selector substitution, copy equalities, lookup instances,
and cumulative `degree_ge[d]` gate counts for every degree d ≥ 2. No component grows and at least one shrinks.
These capture domain/column storage, constraint/permutation/lookup work and quotient-degree pressure without
choosing an arbitrary weight that conceals a tradeoff. This is a concrete layout proxy, not measured prover
time, recursive-verifier cost or gas. `priced = rows*(advice+fixed)+gates+copies+lookups` is informational only;
variables/nodes/max_degree are also reported. A lower priced sum with any larger component is rejected.

**Candidate.** A source overlay under the selected chip source path(s), using the existing AIR overlay verifier:
only `.rs`, no changed manifests/build scripts/toolchain/extractor/wrapper files, symlink changes or added unsafe,
reflection, environment, filesystem/process/network I/O, configuration or linkage escapes. No changed other
file is accepted. Builds are offline/locked with the exact Rust tools. The H1 native test must export every
requested sample, the independent flattened model must accept every native witness, and the actual model and
fixed/structural layout must be witness independent. Sparse zero-valued instance serialization is normalised
using the model's instance-cell support; this never changes its polynomials, fixed values or I/O relation.

The field, instance-column count and exact ordered input/output cell coordinate names are fixed. Consequently
moving public I/O to different coordinates is out of scope even if a more general relabelling proof could be
possible. Other internal rows/columns/gates may change through the allowed source overlay. The fixed H1 wrapper
defines the operation; this is not equivalence of every operation of the entire library. The source rules
inherit the AIR pipeline's syntactic safety restrictions, not an assertion of a general Rust sandbox.

**Pipeline and proof.** Same five commands and equivalence statement as ZoKrates, with native halo2 models from
`halo2_ir` / `halo2_lean_emit`. `build --config` carries `target` and `build` configuration; the latter pins Rust
tools, offline command, editable/protected files and harness/lock digests. The tools directory supplies
`rust-toolchains.json` and an isolated offline Cargo cache. `--build` permits a reusable local build tree.
Simulation runs both source trees on the same H1 seeded input stream, independently rechecks all gates/copies/
lookups and compares input/output vectors. A generated constraint relation quantifies over all advice and
instance assignments, not just the MockProver witnesses. `check` first enforces the component-wise reduction,
then the unchanged checker enforces the theorem text/type, imported-file digests, standard axioms and kernel
replay. No sorry or new axiom is allowed in an accepted solution.

**Scope.** H1 P3 solved only three references; the native layouts are small, and multiple source operations can
instantiate the same constraint model. The private RT-H1 report makes model multiplicity and all exclusions
explicit. Synthetic offline tests cover metric tradeoffs, degree growth, sparse zeros, DET/model binding,
candidate import integrity and malformed records; native controls and Lean checks are separate evidence.

## Limits

- Closed-local artifacts; no registry service, issuance, receipt or reward path is wired.
- Pilot statements and proofs were produced by agents of one model family; independence of statement authorship
  from solving is not established.
- Content-identical copies across repositories must be deduplicated by content hash before packaging.
