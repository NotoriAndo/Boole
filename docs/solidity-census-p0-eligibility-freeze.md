# Solidity census P0 — generative task-family eligibility freeze (v1)

Ceiling label: **SOLIDITY-P0-MINEABLE-ELIGIBLE = 0**.

This is an **issuable problem count, not network activation**. Solidity `mineable_now`
stays **0**; this record wires **no** consensus / BF.7 path. It is closed-local,
non-consensus evidence and is **not a public-network / leaderboard / paid-API /
production claim**.

A **0 here does not mean Solidity is permanently un-mineable.** It means that the
**current frozen P0 corpus**, run through **this compiler-only A/B generative family**,
yields exactly **0** genuinely non-trivial issuable problems. `semanticTests` (1,670) and
the successor tests (709) are **follow-on scope, not discarded**. The family is **not**
redesigned in this wave.

This document is an **append-only attestation ledger**. The family implementation, the
pinned verifier, the Solidity test corpora, and the run ledgers **stay in the git-ignored
sandbox** (`local-docs/langspec-universe-p0-2026-07-23/solidity-family-impl/` and
`.../sources/solidity/test/libsolidity/`); only content hashes, corpus fingerprints, and
lineage are tracked here so the result survives outside the sandbox. Future waves append
new dated entries below; existing **merged** entries are never rewritten.

---

## Entry 1 — 2026-08-10 · generative-family-v1 / trivial-construction finalization / N = 0

### Result in one line

Two non-overlapping compiler-only sub-families (A: typed-hole compile-repair over
`syntaxTests`; B: bounded expression synthesis over `smtCheckerTests`) each produced
**2** candidate anchors. A mandatory **trivial-construction gate** (operator msgs 3722 /
3724) then rejected **all 4**: each candidate is solvable by a **mechanically generated**
answer, so none represents genuine work. Final **SOLIDITY-P0-MINEABLE-ELIGIBLE = 0**.

### Why a generative family (not a leaked-answer bank)

The public Solidity compiler tests carry their expected answer co-located in the public
GPL checkout, so a **direct-answer** family issues **0** fair tasks (every test anchor is
`EXCLUDED-ANSWERED-FIXTURE`; retired in
`local-docs/langspec-universe-p0-2026-07-23/DECISION-solidity-direct-answer-family-NO-GO.md`).
This wave instead used the tests **only as templates**: for each template a **new**
problem is synthesized whose accepted answer is **never** the public/leaked answer of the
source file. A template could count toward the ceiling only if a bounded solution provably
exists, the new constraint provably rejects the original public answer, a fixed offline
verifier decides acceptance in a single compiler run, **and** the accepted answer is not
mechanically constructible from the task statement.

### Pinned verifier (fixed as the Solidity-P0 verifier)

A prebuilt **pinned soljson.js 0.8.36** (solc-js WASM, `solc@0.8.36`) runs fully offline
under Node `v22.22.0`, including the SMTChecker with Z3 compiled to WASM in-process. It is
fixed **explicitly** as the Solidity-P0 verifier; it is **not** claimed byte-equal to any
other solc release. Templates that do not compile under the pin are bucketed separately,
never silently counted.

| pinned artifact | sha256 |
| --- | --- |
| `soljson.js` (`solc@0.8.36`) | `ccb677d54dfab2a9b30084eec6bb396c93eb86d58b42cc00267fd0f54f391f32` |

### The two sub-families and their 2 + 2 candidate anchors

**Sub-family A — typed-hole compile-repair (front end), over `syntaxTests`.** A template
`function f(T a, T b) ... returns (T) { return <EXPR>; }` (T an integer type) is retargeted
to `bool` and the return expression is punched into a hole; the worker must fill a bounded
boolean expression E over both params from the grammar `! && || == != < <= > >=`. The
front-end verifier accepts E iff it compiles clean as `bool`, is in-grammar, uses **both**
params, and is non-atomic. The full cascade over 3,547 templates produced **2** candidate
anchors.

**Sub-family B — bounded expression synthesis (SMTChecker), over `smtCheckerTests`.** A
pure boolean function `f(bool p1..pn)` is extracted; a **new** target relation R is derived
deterministically from `sha256` of the source, chosen non-constant, all-vars-relevant, and
different from the original truth table. The worker must synthesize a grammar-bounded E
(`! && || == !=`) SMT-equivalent to R. The full cascade over 1,435 templates produced **2**
candidate anchors.

The two sub-families do not overlap (A is front-end-only over `syntaxTests`; B is
SMTChecker over `smtCheckerTests`), and the 4 candidate anchors are distinct by source
sha and by semantic signature (4/4).

### The decisive trivial-construction gate (operator msgs 3722 / 3724)

For each candidate the accepted answer was **mechanically generated** and submitted to the
family verifier. If any mechanical answer is accepted within the allowed grammar/size, the
candidate is `INELIGIBLE-TRIVIAL-CONSTRUCTION`.

**Sub-family B — from the target truth table, build standard DNF, standard CNF, and a
deterministic (Quine–McCluskey) minimization; submit each.** Both anchors: **all three
accept**.

| anchor (git-ignored) | R (membership) | DNF | CNF | QM-min | verdict |
| --- | --- | --- | --- | --- | --- |
| `smtCheckerTests/types/bool_simple_2.sol` | `[0,1,1,1]` | ACCEPT | ACCEPT `(x \|\| y)` | ACCEPT `(y \|\| x)` | TRIVIAL |
| `smtCheckerTests/types/bool_simple_3.sol` | `[1,0,1,1]` | ACCEPT | ACCEPT `(x \|\| !y)` | ACCEPT `(!y \|\| x)` | TRIVIAL |

**Sub-family A — a single fixed expression solves every anchor.** The witness `p0 < p1`
(the family's own non-vacuity witness) is accepted for **both** anchors, so no per-instance
synthesis exists — a strictly stronger triviality than B's per-instance construction.

| anchor (git-ignored) | params | universal witness | accepts | verdict |
| --- | --- | --- | --- | --- |
| `syntaxTests/parsing/overloaded_functions.sol` | `(a,b)` | `a < b` | yes | TRIVIAL |
| `syntaxTests/types/functionTypes/function_parameter_return_types_success.sol` | `(x,y)` | `x < y` | yes | TRIVIAL |

Sub-family A's reject/accept battery still passes **16/16** (original public int expr,
literal, single var, unused-var fill, arithmetic, and out-of-grammar conditional are all
rejected; genuine bounded bool fills accepted) — i.e. A's *battery* is sound, but the
*trivial-construction bar* excludes it regardless.

**SMT decisions are clean:** across every SMTChecker run in the finalization, solc **hard
errors = 0** and **unknown / timeout family = 0**. No `unknown`/timeout/hard-error result
was ever treated as a pass.

**All 4 candidate anchors → `INELIGIBLE-TRIVIAL-CONSTRUCTION`. MINEABLE-ELIGIBLE = 0.**

### Conservation identities (FROZEN)

**Stage A — the full frozen anchor ledger, by anchor kind (from
`census-results.json`):**

```
12,931 = 6,652 test-file bundle
        + 5,726 public-answer row  (single expectation line inside a bundle)
        +   288 doc-grammar anchor  (ANTLR docs grammar production)
        +   214 compiler-source anchor  (whole .cpp/.h under libsolidity/)
        +    51 doc-prose anchor  (RST documentation subsection)
```

**Stage B — the 6,652 test-file bundle, by P0 mineability bucket (each anchor in exactly
one bucket):**

```
6,652 =     0 MINEABLE-ELIGIBLE
        + 2,322 NO-CLEAN-COMPILER-BASE       (A: template front-end error under the pin — mostly negative tests)
        +    28 SOLC-0.8.36-INCOMPATIBLE     (B: smtCheckerTests front-end error under the pin)
        + 2,628 NO-FRESH-INSTANCE            (A 1,223 + B 1,405: no template shape to generate a fresh problem)
        + 1,670 DEFERRED-EVM-REQUIRED        (semanticTests: need EVM execution)
        +     4 INELIGIBLE-TRIVIAL-CONSTRUCTION  (A 2 + B 2: mechanical answer accepts)
```

`EXCLUDED-REPRESENTATIVE-FIXTURE = 0`, `DUPLICATE = 0`, `ERROR = 0` (all zero, hence
omitted from the identity). The 4 trivial anchors are exactly the cascade `COUNTED` set
(A 2 + B 2) reclassified by the gate; `MINEABLE-ELIGIBLE` therefore drops to 0.

**Successor tests (out of this denominator):**

```
709 SUCCESSOR-OUT-OF-SCOPE = 7,361 (current source test/*.sol) − 6,652 (frozen P0 bundle)
```

recorded separately, **not** mixed into the 6,652 denominator.

### `semanticTests` reconciliation (1,498 vs 1,670)

`semanticTests` is **1,670** `.sol` files. The earlier figure **1,498** is the **subset**
whose expectation is a concrete return / gas / event oracle; the remaining **172** are
`void f()->` or `-> FAILURE`-only. This is a reclassification (subset vs whole corpus),
**not** a unit change:

```
1,670 = 1,498 (concrete return / gas / event)  +  172 (void f()-> or -> FAILURE-only)
```

All **1,670** need EVM execution to decide a runtime-semantics answer, so all are
`DEFERRED-EVM-REQUIRED` (a compile-only P0 cannot decide them); they are follow-on
EVM-execution family candidates, not discarded.

### Reconciliation with the earlier 5,450 expectation

`5,450` was the count of test **bundles carrying a strict non-trivial public oracle**
(2,590 `syntaxTests` + 1,498 `semanticTests` + 1,362 `smtCheckerTests`; the other 1,202
are trivially-matchable/empty) — the naive "issuable if we could use the public oracle"
expectation. Final **N = 0** differs from 5,450 for two independent reasons, neither of
which is oracle *absence*:

1. **Direct-answer route is answer-leaked.** All 5,450 strict-oracle bundles are
   `EXCLUDED-ANSWERED-FIXTURE` — the expected `// ----` answer is co-located with the
   input in the public checkout, so issuing them is not fair. Direct issuance = 0.
2. **Generative route is trivial / non-generatable.** Using the tests as templates, the
   only anchors for which a fresh, original-rejecting problem could be generated (4) are
   trivially constructible; the rest are non-compiling (2,322), have no fresh-instance
   shape (2,628), or require EVM execution (1,670). Generative issuance = 0.

So `5,450 → 0` is entirely (1) leakage on the direct route + (2) triviality /
non-generatability on the generative route.

### No answer / accepting-expression is stored in any issued task

The **final emitted task ledger has 0 rows** (`final_task_ledger.json` = `[]`). With no
task issued, no task can store an answer or an accepting expression — the property holds
vacuously. The `trivial_construction_evidence.json` ledger *does* record the mechanical
accepting expressions, but that artifact is **exclusion evidence** (why each anchor was
rejected); it is **not** an issued task and is never handed to any worker. Target
relations R are recorded only as **membership vectors**, never as a formula.

### CI scope (what green actually attests)

CI (`self-test`, `supply-chain`, `verdict-corpus`) does **not** recompute the 6,652-anchor
cascade or re-derive N. It verifies that **this freeze record is intact and the repository
has no regression** (docs-smoke pins the identity and labels; the sandbox artifacts are
hashed here). The N = 0 result is established by the git-ignored sandbox run recorded
above, not by CI.

### Corpus fingerprints (git-ignored corpora, hash-pinned here)

| corpus | files | fingerprint sha256 |
| --- | ---: | --- |
| `test/libsolidity/syntaxTests` | 3,547 | `ec0fdc6289a5390e3a050ee4bbe8d72b48a004aae90883763b6af1335a08c318` |
| `test/libsolidity/smtCheckerTests` | 1,435 | `b05d389dc6e334baa66bdb11e0174bdff22894178fbba5b8fe0e0dd2431d180e` |
| `test/libsolidity/semanticTests` | 1,670 | `e0b9d4c315d6b0e12b6a17fff53f776b49eff3d03cd7af0a9598e52592145dcb` |

Each fingerprint is `sha256` over the sorted `relpath\tsha256(bytes)` manifest of every
`.sol` file in the corpus, so the exact anchor set is reproducible.

### Final task ledger + exclusion evidence (hash-only; originals stay in the sandbox)

| artifact | rows | bytes | sha256 |
| --- | ---: | ---: | --- |
| `final_task_ledger.json` (emitted tasks) | 0 | 2 | `4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945` |
| `trivial_construction_evidence.json` (4 excluded) | 4 | — | `b70f055b99decacdc9d5e9b144cb125805f15f1d0907cc0543b80b21648790f2` |
| `cascade_a_counted.json` (A candidates) | 2 | 728 | `55d8d12d9bcab72baf487cb2e0d14dbf0894bea87a967540b7a779325fc96ca7` |
| `cascade_b_counted.json` (B candidates) | 2 | 612 | `89157af29a97716cacf362d49c5b94b19299ca3dde049a55bdcc6d8024cb1699` |

Cascade **summary** JSONs carry a per-run `elapsed_ms`, so their whole-file digests are
intentionally not pinned; the deterministic bucket counts are frozen in the Stage B
identity above (A: 2 / 2,322 / 1,223; B: 2 / 28 / 1,405).

### Family implementation + classifier hashes (git-ignored; hash-pinned here)

| module | role | sha256 |
| --- | --- | --- |
| `solc_pinned.mjs` | pinned solc driver | `3a9faf68101a22ac1c0a56e93142e35ff415e9096ef54069cd8b009d39f1c60a` |
| `subfamily_a.mjs` | A generator / verifier | `6b9598624de769a6fe0aea563d6c06a757d50b68f1cf74452760c3681b6447c4` |
| `subfamily_b.mjs` | B generator / verifier | `de6bfee64d207db1bf44a515fd0ee7b25e4c8a98d3f4c823fa307140d156072b` |
| `cascade_a.mjs` | A full cascade | `95fd25f52d74dbe8e456f67260317c9a51a0bf9119c8d96d6ef81bc19fe49775` |
| `cascade_b.mjs` | B full cascade | `6db8e41901c0547980b562f331e3332246c5506f6e73dd23c0fb32fbd726945b` |
| `verify_finalization_gate.mjs` | trivial-construction classifier | `78d9e33c7cc3ad83fc2b4ae08d96dcad0e2c3e8683d3c68a207887b3e5253b12` |
| `finalize_n0.mjs` | N=0 ledger + digest emitter | `a1e909fbdaac9a51987f281615aae3b1f558870c22391c7c308ff983146c46c8` |

### Boundary / non-claims

* **SOLIDITY-P0-MINEABLE-ELIGIBLE = 0** is the issuable problem count under this frozen P0
  corpus and this compiler-only A/B family — **not** a claim that Solidity is permanently
  un-mineable. `mineable_now` stays 0.
* `semanticTests` (1,670) and the successor tests (709) are **follow-on scope**, not
  discarded. A genuine positive count would require a **redesigned** family that forces a
  difficulty floor (e.g. a size-bounded B where the DNF/CNF/minimized answer is out of
  bounds, or a target-matching A), which is explicitly **out of scope this wave**.
* No consensus / BF.7 / mining / reward / Base / paid-API / public-mining change was made;
  the verifier is a pinned offline solc, connected to no consensus path.
* Family implementation, pinned verifier, corpora, and run ledgers live only in the
  git-ignored sandbox; this record carries hashes, corpus fingerprints, and lineage so the
  freeze is durable in-tree — closed-local validation only.
* This record is **not a public-network / leaderboard / paid-API / production claim**.
