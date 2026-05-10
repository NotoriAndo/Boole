# Benchmark target families

Each Boole benchmark attempt is bound to a **target family** — a versioned identifier that fixes the prompt shape, theorem template, lottery binding, and the canonical proof's required difficulty. The family is recorded as `targetFamily` on every per-row JSON record and on the run summary, so historical leaderboards remain comparable only across runs that share the same family.

A family is **immutable** once a run has used it. Any change that would invalidate a historical canonical proof must instead introduce a new family with a bumped version suffix (`v2`, `v3`, …). Old families remain callable for back-compat smoke runs.

This document is the authoritative list. CI lints `scripts/boole-model-benchmark.py` against the section headers below: every `targetFamily` literal in the script must have a matching `## <family>` section here. Add a new section before adding a new literal.

---

## boole.calibration.pow.v1

**Mode:** `mining` (default for `scripts/boole-model-benchmark.py`).

**Used by:** the public proof-to-block benchmark (`scripts/preflight-model-benchmark.sh` and the long-running `claude/sonnet/gemma` runs under `artifacts/model-benchmarks/`).

**Theorem template:** `theorem boole_benchmark_pow_target_<n> : "<challenge>" = "<challenge>" := <proof_term>`. The lottery sample (`challenge`, `nonce`, `theoremName`) is derived deterministically from `(runId, target, attemptIndex, benchmarkMode, targetFamily)` so that re-running with the same arguments produces byte-identical attempts. Each attempt also embeds the lottery in Lean comment lines so the canonical package binds the lottery to the proof source hash.

**Canonical proof shape:** any term that closes `"<challenge>" = "<challenge>"`. In practice, every challenge is reflexively true, so `rfl` (or any synonym thereof) is canonical. This is the **family's primary known limitation** — see "Known limitations" below.

**Difficulty:** the calibration target lives in `fixtures/protocol/admission/v1.json` (read by `boole-node submit-lean --difficulty-mode fixture`). The family does not pick its own difficulty; the calibration report determines it for the consensus path.

**Verifier hash:** resolved from `fixtures/benchmarks/verifier-hashes.json` via `resolve_verifier_hash()` (Slice S5). The fixture's `active` key names the version a fresh run picks up; the resolved string is recorded on each row as `verifierHash`, alongside the version key as `verifierHashVersion`. Replay/validation pins to the recorded version, so a row written when `active="v0"` resolves to the v0 hash even after `active` is bumped. The legacy `v0` value is preserved byte-identically as `boole-model-benchmark-ollama-v0` so historical NDJSON artifacts replay against the original hash without re-derivation.

### Known limitations
- **`rfl`-trivial canonical proofs.** Because the challenge string equals itself, any model that emits `rfl` (or `Eq.refl _` etc.) produces a valid canonical proof. The intended difficulty is in the **lottery binding** (proof-source-binding into `canon_hash`), not in the proof term. Models that exploit this are still gated by the calibration target and the share/block selection — but the v1 family does not measure proof-search ability per se. Slice **S8 (B1 + B7(b))** introduces `boole.calibration.pow.v2` with non-trivial canonical proofs to address this.
- **Single theorem template per attempt.** All attempts in a run share `theorem boole_benchmark_pow_target_<n>` — the only varying inputs are `<n>` (1-based attempt index) and the embedded lottery sample. A v2 family is free to widen the template space.
- **Verifier-hash naming.** The legacy verifier hash carries the `ollama` substring even though the family is provider-agnostic. Slice S5 moved verifier-hash resolution to `fixtures/benchmarks/verifier-hashes.json` while preserving the legacy `v0` string byte-identically; renaming the v0 entry would invalidate historical rows, so any future provider-agnostic name lands as a separate `v1` entry rather than a v0 rewrite. Downstream tooling that strips the `ollama` substring should treat the legacy string as opaque.

---

## boole.calibration.pow.v2

**Mode:** `mining-v2` (`--benchmark-mode mining-v2`).

**Used by:** opt-in proof-to-block runs that exercise structurally
non-`rfl` canonical proofs — the family carrying B1 acceptance evidence
(parity-plan §7 Gate B). v1 stays callable for back-compat smoke runs;
v2 does not replace v1.

**Family origin.** Port of pof's `v031-lp` family (the lp-bearing arm
of `projects/pof/lean/Boole/Family/GenTargetCatalogV031.lean`), narrowed
to **N = 1 lp-only**. Each attempt produces a single `lengthPreserved`
branch over a deterministic chain drawn from the length-preserving op
family `{mapAdd k, mapMul k, sortAsc}`.

**Theorem template:**
```
import Boole.Family.V0Helpers

namespace boole_benchmark_pow_v2_target_<n>

open Boole.Family.V0Helpers

theorem boole_benchmark_pow_v2_target_<n> :
    ∀ (xs : List Int), <chain_expr>.length = xs.length :=
  <PROOF_TERM>

end boole_benchmark_pow_v2_target_<n>
```
where `<chain_expr>` is the chain applied left-to-right as nested
calls (e.g. `sortAsc (mapMul 2 (mapAdd 3 xs))` for chain
`[mapAdd 3, mapMul 2, sortAsc]`). The lottery sample (`challenge`,
`nonce`, `chainExpr`, `chainLen`, `D`) is derived deterministically
from `(runId, target, attemptIndex, benchmarkMode, targetFamily)` so
re-running with the same arguments produces byte-identical attempts;
the lottery is also embedded as `-- lpChainLen: ...`, `-- lpD: ...`
comment lines so the canonical package binds the lottery to the proof
source hash.

**Canonical proof shape:** composition of `length_*` lemmas via
`Eq.trans` (the same shape probed by pof's `V0ProbeV031.lean`):

- `chainLen = 1`: `fun xs => length_<op_0> <args_0> xs`.
- `chainLen ≥ 2`: `fun xs => (length_<op_{n-1}> _).trans
  ((length_<op_{n-2}> _).trans (... (length_<op_0> <args_0> xs)))`,
  reading outermost-to-innermost.

Argument formatting: `mapAdd k → "(k : Int)"`, `mapMul k → "(k : Int)"`,
`sortAsc →` (no argument). The final-position lemma takes `xs`
directly; non-final positions take `_` (placeholder).

**Lean library.** `Boole.Family.V0Helpers` ships in
`lean/checker/Boole/Family/V0Helpers.lean` and is declared as a lake
`lean_lib «Boole»` of the `boole_check` package, so submitted v2
modules resolve `import Boole.Family.V0Helpers` against the package's
olean cache when `boole-lean-runner` invokes `lake exec boole_check
<proof.lean>`. The library exports the three reducible defs
(`mapAdd`, `mapMul`, `sortAsc`) and the three length lemmas
(`length_mapAdd`, `length_mapMul`, `length_sortAsc`); it intentionally
omits the v0.2 truthy lemmas and `dedup` since lp-only does not use
them.

**Why non-`rfl`-equivalent (Gate B-CI binding criterion).** Kernel
reduction of `(mapAdd k xs).length` does not simplify to `xs.length`
without invoking `List.length_map`; the reducible `mapAdd` definition
unfolds to `xs.map (fun x => x + k)`, but the resulting `length` only
reduces under the named lemma. The minimum proof term therefore
contains `length_<op>` at least once — `rfl` alone fails to typecheck.
For `chainLen ≥ 2` the proof additionally needs `Eq.trans`. Because
chains differ per challenge (op kinds, op order, op arguments), the
canonical proof differs structurally per challenge.

**Difficulty.** Chain length is `(D % 6) + 1 ∈ [1, 6]`, where `D` is
read from the first cursor byte of a sha256-derived seed
(`sha256("<runId>|<target>|<attemptIndex>|<benchmarkMode>|<targetFamily>|v031-lp-cursor")`).
The same calibrated admission target as v1 governs the consensus
path; the family does not pick its own difficulty.

**Verifier hash.** Same resolver as v1
(`fixtures/benchmarks/verifier-hashes.json` via `resolve_verifier_hash()`).
v2 reuses the submit-lean / admission / share pipeline byte-for-byte,
so no new entry is required. A future change that demands a
v2-specific hash lands as `active: "v1"` in the fixture per the
Slice-S5 contract; the existing resolver picks it up without code
changes.

**Acceptance criteria (Gate B).**
- **CI portion:** v2 family loads, generates 5 deterministic
  challenges, each has a non-`rfl`-equivalent canonical proof.
  Asserted via fixture-driven test in
  `scripts/test_model_benchmark.py::MiningV2FamilyTests`
  (no live LLM call).
- **Live portion:** 50-attempt v2 run produces non-zero
  `blocksProduced` for at least one model AND a per-model spread of
  ≥10 percentage points across opus / sonnet / gemma. Run after S8
  lands; failures trigger a follow-up bug, not CI red.

### Known limitations
- **N = 1 lp-only narrowing.** The pof v0.3.1 schema supports N≥2
  parallel branches mixing the `lengthPreserved` invariant with the
  v0.2 witness invariants (`allSatisfy`, `sortedAsc`, `dedupFirst`,
  `partitionEq`). v2 ships only the lp arm so the Lean port stays
  small; future v3 may broaden to compound conjunctions once the
  v0.2 `truthy_*` lemmas are also ported.
- **Closed length-preserving op family.** `{mapAdd, mapMul, sortAsc}`
  exhausts the available proof primitives — any chain combination
  closes by `Eq.trans` over `length_*`. Models that learn the
  composition pattern at all generalize across the family.
  Differentiation between **frontier** models is unlikely to come
  from chain length alone; the family's spread targets the gap
  between frontier and smaller / open-weight models.
- **Cursor reads beyond the chain are no-op-consumed.** The pof
  generator advances the cursor through `pred / lenRaw / noiseRaw`
  positions after the chain. v2 also consumes those bytes (with
  results ignored) so the cursor model stays byte-compatible if the
  family is ever extended to N≥2 mixed branches.

---

## boole.calibration.pow.v3

**Mode:** `mining-v3` (`--benchmark-mode mining-v3`, opt-in for now;
the production-default mode pinned in CI remains `mining` for v1 byte
freeze. v3 ships alongside v1 and v2 and stays callable without
disturbing them.)

**Family origin.** Direct port of pof's full v0.3.1 single-branch
generator (`projects/pof/lean/Boole/Family/GenTargetCatalogV031.lean`
+ `ListInvariantsV031.lean`), narrowed to N=1 (single invariant per
attempt). Each generated instance carries one of five invariant
classes — `allSatisfy p`, `sortedAsc`, `dedupFirst`, `partitionEq p`,
`lengthPreserved` — drawn uniformly via `genInvClassV031`. Cursor
read order is byte-identical to pof's `genBranchV031`: invClass
selector → optional predicate sub-cursor → body ops → trailing
`pred` / `lenRaw` / `noiseRaw` (forward-compat). Boole prepends a
single `D` byte (also matches v2) so the seed determines difficulty
without requiring an external parameter.

**Theorem template (per invariant).** Let `body_expr` be the body
chain applied to `xs`; the witness op (where applicable) is appended
at the chain end so `result_expr = (witness_op body_expr)`. The
shipping signature is always
`theorem boole_benchmark_pow_v3_target_<n> : ∀ (xs : List Int), <RHS>`
where `<RHS>` is selected by invariant class:

| Invariant       | Result expr                          | Theorem RHS                                                                                |
|-----------------|--------------------------------------|--------------------------------------------------------------------------------------------|
| `allSatisfy p`  | `(filterByPred <pBool> body_expr)`   | `(<result>).all <pBool> = true`                                                            |
| `sortedAsc`     | `(sortAsc body_expr)`                | `List.Pairwise (· ≤ ·) <result>`                                                            |
| `dedupFirst`    | `(dedup body_expr)`                  | `List.Nodup <result>`                                                                       |
| `partitionEq p` | `body_expr` (no witness op)          | `(<result>).partition <pBool> = ((<result>).filter <pBool>, (<result>).filter <pBoolNot>)` |
| `lengthPreserved` | `body_expr` (lp 3-set body only)   | `(<result>).length = xs.length`                                                             |

**Canonical proof shape (per invariant).**

| Invariant       | Canonical proof term (after `fun xs =>`)                                            |
|-----------------|-------------------------------------------------------------------------------------|
| `allSatisfy p`  | `all_filterByPred_self <pBool> <body_expr>`                                         |
| `sortedAsc`     | `pairwise_sortAsc <body_expr>`                                                      |
| `dedupFirst`    | `nodup_dedup <body_expr>`                                                           |
| `partitionEq p` | `partition_eq_filter_filter <pBool> <body_expr>`                                    |
| `lengthPreserved` | (same as v2) `(length_<lastOp> _).trans (... (length_<firstOp> <args> xs))`       |

When the body chain is empty (`chainLen = 1` for witness branches
whose `bodyLen = chainLen − 1 = 0`), `body_expr = "xs"` and the
witness lemma is applied to `xs` directly.

**Pred → Bool function rendering.** Each `Pred` kind renders as a
canonical `Int → Bool` Lean source expression (matches pof's
`Pred.toBoolFn`):

| Pred kind  | toBoolFn                                                  |
|------------|-----------------------------------------------------------|
| `even`     | `(fun x : Int => x % 2 == 0)`                             |
| `odd`      | `(fun x : Int => x % 2 != 0)`                             |
| `ltK k`    | `(fun x : Int => decide (x < (k : Int)))`                 |
| `gtK k`    | `(fun x : Int => decide ((k : Int) < x))`                 |
| `eqK k`    | `(fun x : Int => x == (k : Int))`                         |
| `modK k r` | `(fun x : Int => x % (k : Int) == (r : Int))`             |

`toBoolNot p = (fun y : Int => !(<p.toBoolFn> y))`.

**Lean library.** All proofs ride the same
`Boole.Family.V0Helpers` namespace as v2. v3 additionally references:

- `@[reducible] def filterByPred (p : Int → Bool) (xs : List Int) := xs.filter p`
- `@[reducible] def dedup (xs : List Int) := xs.eraseDups`
- `theorem all_filterByPred_self (p) (xs) : (filterByPred p xs).all p = true`
- `theorem nodup_dedup (xs) : List.Nodup (dedup xs)`
- `theorem pairwise_sortAsc (xs) : List.Pairwise (· ≤ ·) (sortAsc xs)`
- `theorem partition_eq_filter_filter (p) (xs) : xs.partition p = (xs.filter p, xs.filter (fun x => !(p x)))`

These are byte-identical ports of the corresponding lemmas from
pof's `projects/pof/lean/Boole/Family/V0Helpers.lean`. The Lean
toolchain (4.29.1) and stdlib lemma surface (`List.all_filter`,
`Bool.not_or_self`, `List.eraseDups_cons`, `List.pairwise_mergeSort`,
`List.partition_eq_filter_filter`) match between projects, so the
proof bodies copy verbatim.

**Why non-rfl-equivalent (per branch).**

- `allSatisfy p`: closing `(filterByPred p xs).all p = true` requires
  the kernel to reduce `xs.filter p |>.all p` and rewrite under
  `Bool.not_or_self`. Bare `rfl` fails — `simp [List.all_filter]` is
  needed.
- `sortedAsc`: `List.Pairwise (· ≤ ·) (sortAsc xs)` is a
  `mergeSort`-postcondition theorem; not a kernel reduction.
- `dedupFirst`: `List.Nodup (dedup xs)` is closed by induction with a
  decreasing-on-filter measure. `rfl` cannot close it.
- `partitionEq p`: `xs.partition p = (filter p, filter ¬p)` requires
  `List.partition_eq_filter_filter` plus a function-extensionality
  rewrite. Not `rfl`.
- `lengthPreserved`: same argument as v2 — `(mapAdd k xs).length`
  doesn't reduce to `xs.length` without `List.length_map`.

**Difficulty.** Per-attempt difficulty is driven by the chain length
`D` (cursor-derived, `(D % 6) + 1` ∈ [1, 6]) and the invariant
class. The `lengthPreserved` branch composes `D` length lemmas via
`Eq.trans` and is the same difficulty knob v2 ships. The four
witness branches close with a single witness-lemma application
regardless of `D` — body chain depth manifests only in the body
expression the model has to track, not in the proof composition.
Future tuning may rebalance the 5-way invariant distribution if one
class proves trivially solvable in live runs.

**Verifier hash.** Stays pinned to `verifier-hashes.json`'s `active`
entry (v0 today). A future live-run slice that produces 50-attempt
v3 evidence will bump `active` to a v3-anchored hash; the
version-keyed resolver from Slice S5 picks it up automatically with
no code change.

**Acceptance criteria.**

- Gate B-CI (deterministic): mode dispatch, attempt-context shape,
  per-invariant canonical-proof witness lemma name, 5-way coverage
  in 50 deterministic samples, wrap output, prompt content,
  argparse choice, and a v3-specific golden-fixture replay
  regression. Asserted via
  `scripts/test_model_benchmark.py::MiningV3FamilyTests`.
- Lean elaboration smoke: one wrapped candidate per invariant class
  must elaborate cleanly through `lake exec boole_check` (exit 0).
- v2 byte-frozen: v2 fixture replay still Green; v3 ships in
  parallel without disturbing it.

**Known limitations.**

- **N=1 only.** N≥2 conjunction goals + anonymous-constructor proof
  composition are deferred to a future S8c slice.
- **Single-template-per-attempt** (inherits from v1 / v2). The
  attempt advertises one theorem statement; a multi-template menu
  is a separate B-track follow-up.
- **No live evidence (yet).** B1+B7(b) live-evidence (≥10pp
  per-model spread on a 50-attempt run) is a separate Gate B-Live
  artifact, not gated by CI for S8b.
- **Witness-only closure for the four truthy branches.** Each
  invariant has exactly one canonical witness lemma; v3 does not
  yet exercise multi-step rewriting proofs. Future families could
  drop the witness scaffold and force the model to discover the
  proof structure itself.

**Forward-compat cursor positions.** Like v2, v3 advances the cursor
through the legacy `pred` / `lenRaw` / `noiseRaw` positions even
though N=1 doesn't reference them, so the cursor stays
byte-compatible with a future N≥2 v3 extension.

---

## boole.smoke.true.v1

**Mode:** `smoke` (`--benchmark-mode smoke`, opt-in).

**Used by:** smoke / probe scripts and the test corpus. **Not** a public mining score and **not** counted on the leaderboard alongside mining-mode rows.

**Theorem template:** `theorem boole_benchmark_true : True := <proof_term>`.

**Canonical proof shape:** `True.intro` (or any term inhabiting `True`).

**Difficulty:** smoke mode uses the `preflight-easy` calibration overrides (see `boole-node submit-lean --difficulty-mode preflight-easy`) so that any valid `True` term clears the targets. This is intentional — the family exists to verify the wiring (model → extractor → submit-lean → admission → block), not to measure model capability.

### Known limitations
- **Trivially passable.** Every model that emits `True.intro` produces a block, so the family does not differentiate models. It exists for plumbing tests only.
- **Segregated from public scores.** Code paths that surface a public score guard against `benchmarkMode == "smoke"`, but consumers reading `targetFamily` directly should still skip smoke rows when computing leaderboards.
