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

## Limits

- Closed-local artifacts; no registry service, issuance, receipt or reward path is wired.
- Pilot statements and proofs were produced by agents of one model family; independence of statement authorship
  from solving is not established.
- Content-identical copies across repositories must be deduplicated by content hash before packaging.
