# Solidity census P0 — generative task-family eligibility freeze (v1)

Ceiling label: **SOLIDITY-P0-MINEABLE-ELIGIBLE = 4**.

This is an **issuable problem count, not network activation**. Solidity `mineable_now`
stays **0** until BF.7 consensus activation — this record wires **no** consensus /
BF.7 path. It is closed-local, non-consensus evidence and is **not a public-network
/ leaderboard / paid-API / production claim**.

This document is an **append-only attestation ledger**. The family implementation,
the pinned verifier, the Solidity test corpora, and the run ledgers **stay in the
git-ignored sandbox**
(`local-docs/langspec-universe-p0-2026-07-23/solidity-family-impl/` and
`.../sources/solidity/test/libsolidity/`); only content hashes, corpus fingerprints,
and lineage are tracked here so the result survives outside the sandbox. Future waves
append new dated entries below; existing entries are never rewritten.

---

## Entry 1 — 2026-08-10 · generative-family-v1 / two sub-families / soljson.js 0.8.36

### Why a generative family (not a leaked-answer bank)

The public Solidity compiler tests are used **only as templates**. For each template
we synthesize a **new** problem whose accepted answer is **never** the public/leaked
answer of the source file: the answer must be synthesized fresh and is not copyable
from the source. A template counts toward the ceiling only when a bounded solution
provably exists (non-vacuity by rule), the newly imposed constraint provably rejects
the original public answer, and a fixed offline verifier decides acceptance in a
single compiler run. No per-instance hint, cheat sheet, or per-problem recipe is
issued — only the generated problem is recorded; the answer is never stored.

### Pinned verifier (fixed as the Solidity-P0 verifier)

A prebuilt **pinned soljson.js 0.8.36** (solc-js WASM, `solc@0.8.36`) runs fully
offline under Node `v22.22.0`, including the SMTChecker with Z3 compiled to WASM
in-process. It is fixed **explicitly** as the Solidity-P0 verifier; it is **not**
claimed byte-equal to any other solc release. Templates that do not compile under the
pin are bucketed separately (`INCOMPATIBLE-0.8.36`), never silently counted.

| pinned artifact | sha256 |
| --- | --- |
| `soljson.js` (`solc@0.8.36`) | `ccb677d54dfab2a9b30084eec6bb396c93eb86d58b42cc00267fd0f54f391f32` |

### Two non-overlapping sub-families

**Sub-family A — typed-hole compile-repair (front end).** Over `syntaxTests`. A public
template `function f(T a, T b) ... returns (T) { return <EXPR>; }` (T an integer type)
is retargeted to `bool` and the return expression is punched into a hole. The worker
must synthesize a bounded boolean expression E over both params from the grammar
`! && || == != < <= > >=`. The verifier does **one** front-end compile of `probe(E)`
and accepts only if E compiles clean in the bool context, is inside the grammar, uses
**both** params, and is non-atomic. The **original integer EXPR is rejected** by the
new bool constraint (a type-system certainty, verified once at generation). Non-vacuity
by rule: for two integer params, `a < b` is always a valid bool answer, so a solution
provably exists.

**Sub-family B — bounded expression synthesis (SMTChecker).** Over `smtCheckerTests`. A
pure boolean function `f(bool p1..pn)` (n ≥ 2, single `assert`, in-grammar over params,
single-assignment bool locals inlined) is extracted; a **new** target relation R is
derived deterministically from `sha256` of the source, chosen non-constant, all-vars-
relevant, and **different** from the original truth table. The worker synthesizes a
grammar-bounded E (only `! && || == !=` over params). The verifier does **one** solc
SMTChecker run checking: E ≡ R proved-safe, E and ¬E each satisfiable (neither tautology
nor contradiction), and the **original public assert P is rejected** by R. A host
truth-table cross-check guards the solc↔host verdict mapping. Non-vacuity by rule =
functional completeness of `{!, &&, ||}`.

The two sub-families do not overlap: A is front-end-only over `syntaxTests`; B is
SMTChecker over `smtCheckerTests`.

### Representative gates (RED → GREEN, before the full cascade)

* Sub-family A: 3 representative patterns × 8 rows = **24/24 PASS** (accepts genuine
  bounded bool fills using both params; rejects the original int expr, a literal, a
  single var, an unused-var fill, an out-of-grammar conditional, and arithmetic).
* Sub-family B: 4 representative patterns × 9 rows = **36/36 PASS** (accepts a
  synthesized equivalent of the new R and its reversed-DNF form; rejects the original
  P, the complement, a tautology, a contradiction, a single var, an out-of-grammar
  `^`, and an irrelevant var).

Both sub-families passed, so per the mandate each passing sub-family was run over its
**full** anchor set exactly once and the counts were frozen.

### Conservation identity (full cascade, FROZEN)

Each template yields at most one task; every template lands in exactly one bucket.

* `syntaxTests: 3,547 = 2 COUNTED + 2,322 TEMPLATE-NONCOMPILING + 1,223 SPEC-UNSUPPORTED` (0 INVARIANT-original-accepted-as-bool, 0 WITNESS-REJECTED)
* `smtCheckerTests: 1,435 = 2 COUNTED + 28 INCOMPATIBLE-0.8.36 + 1,405 SPEC-UNSUPPORTED` (0 NONVACUITY-UNRESOLVED, 0 WITNESS-REJECTED)

The `WITNESS-REJECTED = 0` invariant holds on both corpora: for every COUNTED anchor a
bounded solution was realized once (canonical DNF for B, `a < b` for A) and then
**discarded** — only the generated problem is recorded, never the answer.

**SOLIDITY-P0-MINEABLE-ELIGIBLE = 4 = N_A(2) + N_B(2).**

### The four counted templates

| sub-family | source template (git-ignored) | source sha256 | new constraint |
| --- | --- | --- | --- |
| A | `syntaxTests/parsing/overloaded_functions.sol` (`fun(uint256 a, uint256 b) → uint256`, `a + b`) | `fbfdea6e4376f0f4993a3319e2e831faa7cb4d9fa814e1b24a453b4fea19ad7f` | bool-retarget |
| A | `syntaxTests/types/functionTypes/function_parameter_return_types_success.sol` (`…to_uint256(uint256 x, uint256 y) → uint256`, `x + y`) | `fc78080b264fedcdf5f6ab57d356256e89b2461ff7cbacc3338ae77918190f16` | bool-retarget |
| B | `smtCheckerTests/types/bool_simple_2.sol` (`f(bool x, bool y)`, P `(x == y)`) | `7c6ba094a96ba940e06f8f169e20814aeb58603c71402c52dd6ccce660a58ed5` | new R `((!x && y) || (x && !y) || (x && y))` |
| B | `smtCheckerTests/types/bool_simple_3.sol` (`f(bool x, bool y)`, P `((!(x && y)) || ((x || y)))`) | `e7bb285ff299743724d1b2ecd45992618847c4b3746182c4e25f6ca0dd1f69b2` | new R `((!x && !y) || (x && !y) || (x && y))` |

### semanticTests deferral (adjustment 2)

`semanticTests` (**1,670** `.sol` files) require EVM execution to decide a
runtime-semantics answer, so they are **not** compiler-only problems. They are
recorded as `semanticTests deferred as DEFERRED-EVM-REQUIRED` and are **not** counted
here. This is the interpretation that bounds the count: A draws only on `syntaxTests`
and B only on `smtCheckerTests`. A possible honest broadening — reusing `semanticTests`
as compile-repair templates for sub-family A, whose verification is compile-only — was
**not** adopted unilaterally; it is left as a future appendable entry pending operator
decision.

### Corpus fingerprints (git-ignored corpora, hash-pinned here)

| corpus | files | fingerprint sha256 |
| --- | ---: | --- |
| `test/libsolidity/syntaxTests` | 3,547 | `ec0fdc6289a5390e3a050ee4bbe8d72b48a004aae90883763b6af1335a08c318` |
| `test/libsolidity/smtCheckerTests` | 1,435 | `b05d389dc6e334baa66bdb11e0174bdff22894178fbba5b8fe0e0dd2431d180e` |

Each fingerprint is `sha256` over the sorted `relpath\tsha256(bytes)` manifest of every
`.sol` file in the corpus, so the exact anchor set the cascade ran over is reproducible.

### Ledger hashes (hash-only; originals stay in the sandbox)

| artifact | rows | bytes | sha256 |
| --- | ---: | ---: | --- |
| `cascade_a_counted.json` | 2 | 728 | `55d8d12d9bcab72baf487cb2e0d14dbf0894bea87a967540b7a779325fc96ca7` |
| `cascade_a_summary.json` | — | 621 | `ed16cc6427fc6b1c57ad8f38eb2ab703d9dfe1e909fdfc8efa133a4732bc58c9` |
| `cascade_b_counted.json` | 2 | 612 | `89157af29a97716cacf362d49c5b94b19299ca3dde049a55bdcc6d8024cb1699` |
| `cascade_b_summary.json` | — | 608 | `e32737b521a0d6b5659b77d5b8abc4149b7ba3f477cbb4259d7700ce9ca149f3` |

### Family implementation hashes (git-ignored; hash-pinned here)

| module | sha256 |
| --- | --- |
| `solc_pinned.mjs` | `3a9faf68101a22ac1c0a56e93142e35ff415e9096ef54069cd8b009d39f1c60a` |
| `subfamily_a.mjs` | `6b9598624de769a6fe0aea563d6c06a757d50b68f1cf74452760c3681b6447c4` |
| `subfamily_b.mjs` | `de6bfee64d207db1bf44a515fd0ee7b25e4c8a98d3f4c823fa307140d156072b` |
| `cascade_a.mjs` | `95fd25f52d74dbe8e456f67260317c9a51a0bf9119c8d96d6ef81bc19fe49775` |
| `cascade_b.mjs` | `6db8e41901c0547980b562f331e3332246c5506f6e73dd23c0fb32fbd726945b` |
| `test_subfamily_a.mjs` | `b5c5c14d0b75029f8bc714993d5d6bc70a9889e9ae3bd222ccf925c0ee6331c0` |
| `test_subfamily_b.mjs` | `6ab4f9b5090464f88ac68beea91fdecf99a30110e29f8c4ccd51be494547d052` |

### Boundary / non-claims

* **SOLIDITY-P0-MINEABLE-ELIGIBLE = 4** is the issuable problem count, not network
  activation. `mineable_now stays 0` until BF.7 consensus activation.
* The count is a **floor under this family definition**, not a universe bound: it is
  bounded by the `semanticTests` deferral and by the rigor bar (non-vacuity-by-rule +
  original-rejection + single-compile decision). Broadening is an appendable future
  entry, not a rewrite of this one.
* No consensus / BF.7 / mining / reward / paid-API change was made; the verifier is a
  pinned offline solc and is not connected to any consensus path.
* Family implementation, pinned verifier, corpora, and run ledgers live only in the
  git-ignored sandbox; this record carries hashes, corpus fingerprints, and lineage so
  the freeze is durable in-tree — closed-local validation only.
* This record is **not a public-network / leaderboard / paid-API / production claim**.
