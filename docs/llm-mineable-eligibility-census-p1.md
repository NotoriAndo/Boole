# LLM-mineable eligibility census P1 — label correction and unified ledger (v1)

Ceiling label: **LLM-MINEABLE-ELIGIBLE = NOT-YET-DETERMINED**.

This ledger exists because the previously accumulated cross-domain number was being
read as something it never measured. Every number sealed before this document counted
**tasks whose already-known expected output can be re-executed and proved**. That is a
real quantity and it is preserved unchanged — but it is **not** a count of problems on
which a language model must produce a *new* answer.

Two labels are therefore separated here, permanently:

* **EXECUTION-PROOF-ELIGIBLE-SUBSET = 12,880** — the integrated confirmed subtotal
  sealed across the EVM, Solidity-semantic, Rust and Ethereum-consensus execution-proof
  records. Renamed, not revised: the underlying per-domain numbers are untouched.
* **LLM-MINEABLE-ELIGIBLE = NOT-YET-DETERMINED** — the count of unique anchors that
  carry *both* a fresh semantic challenge generator *and* an answer-free deterministic
  checker. It is **not yet measured**, and it is explicitly **not recorded as 0** and
  **not assumed equal to 12,880**.

Nothing already sealed is discarded. The frozen corpora, author oracles, verifiers and
SP1 results remain the base material from which LLM problems are generated and against
which candidate answers are checked.

`mineable_now = 0` is unchanged. This record wires **no** consensus / BF.7 / reward /
Base path. It is closed-local evidence and is **not** a public-network, leaderboard,
paid-API or production claim.

This document is an **append-only attestation ledger**. Existing merged entries are
never rewritten, and **no other document is edited by this record** — where an earlier
sealed document uses the old reading of 12,880, the correction lives here rather than in
that document. The census artifacts stay in the git-ignored sandboxes; only content
hashes, conservation identities and lineage are tracked here.

---

## Entry 1 — 2026-08-16 · label-correction-and-chunk-wave-halt / LLM-MINEABLE-ELIGIBLE = NOT-YET-DETERMINED

### Result in one line

The in-flight Ethereum-consensus B-chunk proof wave was **halted by operator directive
before its first proof completed** (0 proofs produced, 0 challenges consumed, 0 corpus
candidates consumed), and the integrated subtotal **12,880 is relabelled
`EXECUTION-PROOF-ELIGIBLE-SUBSET`**, with `LLM-MINEABLE-ELIGIBLE` opened as a separate,
**not-yet-determined** quantity.

### The label correction (no number changes)

| quantity | before this entry | after this entry | value |
| --- | --- | --- | --- |
| integrated confirmed subtotal | read as "problem count" | **EXECUTION-PROOF-ELIGIBLE-SUBSET** | **12,880** (unchanged) |
| LLM problem count | implicitly conflated with the above | **LLM-MINEABLE-ELIGIBLE** | **NOT-YET-DETERMINED** |

The subtotal's composition is restated **verbatim** from the sealed records, under the
corrected label only:

```
EVM P0                            6,767   (docs/evm-census-p0-eligibility-freeze.md)
Solidity-semantic P1              1,408   (docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md, successor)
Rust execution-proof P1           2,499   (docs/rust-execution-proof-p1-eligibility-freeze.md, successor)
Ethereum-consensus P1             2,206   (docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md, monolithic band)
Solidity P0                           0   (docs/solidity-census-p0-eligibility-freeze.md)
zk-native release-audit P0            0   (docs/zk-native-release-audit-census-p0-eligibility-freeze.md)
---------------------------------------
EXECUTION-PROOF-ELIGIBLE-SUBSET  12,880
Lean                    CORPUS-NOT-MATERIALIZED   (contributes no number)
```

**What the corrected label means.** An `EXECUTION-PROOF-ELIGIBLE` row is a task whose
expected output already exists in the frozen corpus and can be re-executed inside a zkVM
and proved. A model that submits that already-published expected output has produced
**no new answer**. Such a row is therefore *not* an LLM mining problem by itself; it is
**source material** from which one may or may not be constructible. Which rows do
convert is the subject of Entry 2 and beyond, and is measured, not assumed.

**What is explicitly not claimed here.** That 12,880 rows convert; that fewer convert;
that any particular domain converts. `LLM-MINEABLE-ELIGIBLE` has no value in this entry.

### Chunk proof wave — halted, preserved, nothing confirmed

The Ethereum-consensus B-chunk calibration wave (7 planned proofs) was stopped on
operator instruction **24 s into proof 1 of 7**, before any proof artifact was written.

| item | value |
| --- | --- |
| proofs completed | **0** |
| proofs generated | **0** |
| challenges issued / consumed | 7 / **0** (all still `ISSUED`) |
| corpus candidate consumption | **0** |
| termination | SIGTERM to the driver and to the prover process group; no partial proof file existed |

Stages that **had** completed before the halt, recorded as work product and **not** as
confirmed numbers:

1. the proof set was frozen from the sealed cover by a pre-registered rule (parent
   `i=1101`, |S| = 7) before any result existed;
2. seven **out-of-corpus** fixtures were built and passed a 7/7 equality gate — byte
   identical execution inputs, distinct task identity and fresh challenges, and exact
   reproduction of the sealed execution values, all five domination axes and the shard
   structure — with **0** corpus candidates consumed;
3. `PROOF-SET-FROZEN.json` was written before any proof was attempted.

**No chunk number is confirmed.** The pre-registered adjudication values of the halted
wave (chunked logical problems 4, issued work-units 20, eligibility 2,210, subtotal
12,884, unsplit 83) are **UNCONFIRMED** and must not be cited. The sealed monolithic
figure (2,206) and the subtotal (12,880) are untouched by the halt.

### Lineage (git-ignored sandbox; hash-pinned here)

Sandbox: `local-docs/ethereum-consensus-proof-p1-2026-08-11/successor-harness/`.

| artifact | role | sha256 |
| --- | --- | --- |
| `chunk-3903/OPERATOR-STOP-3904.json` | halt record + sha256 of all 43 preserved files | `b106270d9077f96d279236d378fcd1a50255398fa990e11211ed588168cd9317` |
| `CHUNK-PROOF-FREEZE-3903.md` | pre-registration (proof set, fixtures, combination rule, caps) | `877f2483ad8acd5edda374106daaa87d45956fd5e8b8481ac84f4e35c5f58b6a` |
| `chunk-3903/FIXTURE-EQUALITY.json` | 7/7 out-of-corpus fixture equality gate | `4756649e9ccbe6f12fecf56c15eb6c8ffd9a4958de7f384f7bcff93d9439321c` |
| `chunk-3903/PROOF-SET-FROZEN.json` | proof list + hashes frozen before proving | `f3398478f974484a25b11d7dcd839cc30734480f857ca4a37a8ce208aeb61394` |
| chunk guest ELF | the chunk program (separate identity from the monolithic guest) | `b92c4265d49bf26b654639131a22b47bf99c9ab057dd2078a34944e8207d5c7c` |
| chunk verifying key | proof-ledger separation from the monolithic vk | `0x003768e58cb3736107f08b785bf2f1591a68b9e10c3914b0eea4f70cd4d1b102` |

### Boundary / non-claims (Entry 1)

* **LLM-MINEABLE-ELIGIBLE = NOT-YET-DETERMINED** — no LLM problem count is stated,
  claimed or implied by this entry, and the absence of a number is not a zero.
* **EXECUTION-PROOF-ELIGIBLE-SUBSET = 12,880** is a rename of an existing sealed
  quantity. No per-domain number is revised, retracted or recomputed here.
* SP1 proof counts, prover CPU time and prover memory are **infrastructure cost**. They
  are never counted as LLM problems and never expressed as LLM difficulty.
* An already-published expected output is **not** an LLM answer; execution-only tasks
  are not counted as LLM problems.
* The halted wave's numbers are unconfirmed; nothing in it may be cited as a result.
* Closed-local validation only — **not** public-network mining, **no** paid-API
  benchmark, **no** leaderboard claim. `mineable_now = 0`.

---

## Entry 2 — 2026-08-16 · halted-wave challenge cancellation / 7 CANCELLED, 0 CONSUMED

### Result in one line

The seven challenges issued by the halted chunk-3903 calibration were transitioned
`ISSUED` → **`CANCELLED-OPERATOR-STOP-3904`**, with **0 consumed**, reissue and reuse
forbidden, and the cancellation lineage recorded in a separate file so it survives even
if the registry later auto-expires.

### Why this entry exists

Directive 3904 halted the chunk wave, but halting a wave does not by itself retire the
challenges it had already handed out. Seven challenges were left sitting at `ISSUED`.
An `ISSUED` challenge is a live obligation: it can be answered later, and a later answer
would attach a result to a wave whose numbers are unconfirmed. They are therefore
cancelled explicitly rather than left to expire.

**They are cancelled, not consumed.** Marking them `CONSUMED` would assert that work was
performed against them. No work was performed against them — the wave stopped at proof
1/7 — so `consumed: false` is recorded for every one, and marking them consumed in future
is forbidden by the record itself.

### What was done

| step | outcome |
| --- | --- |
| prior registry preserved | `challenge-registry.json` re-hashed **after** the write and confirmed byte-unchanged |
| successor written | `challenge-registry-v2.json`, all 7 entries `CANCELLED-OPERATOR-STOP-3904` |
| live mapping | emptied (`current: {}`); the 7 task contracts moved to `revoked` |
| reissue / reuse | `reissue_forbidden: true`, `reuse_forbidden: true` |
| lineage | recorded separately in `CHALLENGE-CANCELLATION-3906.json` |

Each cancelled row carries challenge ID, task contract, sequence number, prior status,
new status, `consumed: false`, cancellation time and reason. The registry carries **no
per-entry issuance timestamp**, so `issued_at_recorded` is `null` and the file mtime is
recorded separately as `issued_at_evidence` — evidence is not silently promoted to a
recorded fact.

### Lineage (git-ignored sandbox; hash-pinned here)

Sandbox: `local-docs/ethereum-consensus-proof-p1-2026-08-11/successor-harness/chunk-3903/`.

| artifact | role | sha256 |
| --- | --- | --- |
| `challenge-registry.json` | prior root, preserved byte-unchanged | `380fc70d9d2801aa02227bd0ee732fa14476e542c08bfcaeb359ea2ac3ab1eae` |
| `challenge-registry-v2.json` | successor root, 7 × `CANCELLED-OPERATOR-STOP-3904` | `996bc567b5af81ac11ed37ec8575f38039e320022d2657c421b63138df436048` |
| `CHALLENGE-CANCELLATION-3906.json` | cancellation lineage, survives registry expiry | `9808a2b004a48dd6a780c4db1f77423799d93f9e064e6882b497533543167e92` |

### Boundary / non-claims (Entry 2)

* 7 cancelled, **0 consumed**. No result, no proof and no number is produced by this entry.
* The halted wave's pre-registered values stay **UNCONFIRMED** and must not be cited.
* `mineable_now = 0`. Closed-local record only.

---

## Entry 3 — 2026-08-16 · first gated LLM family + full bucketing / LLM-TASK-ELIGIBLE = 6,755 anchors

### Result in one line

One LLM problem family passed a 14-gate representative battery, was materialized once
over its whole corpus, and **all 91,328 unified-ledger rows were placed in exactly one
bucket each with conservation PASS** — yielding **LLM-TASK-ELIGIBLE = 6,755 anchors**,
every one of them from the single gated family, with **76,759 rows NEEDS-SPEC** because
no family was gated for their domain.

### Order of operations (this order is the evidence)

1. `LLM-WORK-CONTRACT-V1` (13 clauses, 14-gate battery, 10-bucket vocabulary) was frozen
   in `CONTRACT-FREEZE.json` **before any family gate ran and before any count existed**.
2. The family was built and gated by vertical TDD, one test at a time, RED before GREEN,
   on **3** representatives.
3. `BUCKET-MAP.json` — the rule table mapping each sealed label to a bucket — was written
   **before** the bucketing run, from the semantics of each sealed label.
4. Only then were the corpus and the ledger run through.

At seal time every file bound by `CONTRACT-FREEZE.json` was re-hashed and matched (7/7),
and the ledger re-hashed to its sealed root before any row was read.

### The gated family — `evm-bytecode-synth-v1`

The model is given a frozen EVM anchor (pre-state, block environment, fork) and a fresh
256-bit challenge, and must **write the runtime bytecode** of a challenge-derived account
such that, for every probe word `X` the checker executes,

```
storage[target][slot] == ((X * a) mod 2**256) XOR b
```

with `a`, `b`, `slot` and all 12 probes derived from the challenge, inside a **192-byte**
code limit, executed by the pinned revm 38.0.0 native runner.

The corpus supplies the *environment*; the challenge supplies the *obligation*. The anchor
is therefore never its own answer.

**Why 192 bytes is the load-bearing parameter.** The one answer that needs no computation
is a lookup table of the checker's own probes. Its minimum branchless encoding is 70 bytes
per probe — 880 bytes for 12 — while the honest computing solution is 106 bytes. This was
verified, not asserted: the table is ACCEPTed when the limit is lifted to 4096 and
REJECTed at 192. The size bound is what forces real computation.

**The checker cannot construct an answer.** `checker.py` (shipped) derives every expected
value from the published formula. The existence witness lives in `generator.py`, which is
**not shipped**. Gate G13 enforces this by attribute and source inspection: the shipped
module has no witness constructor, and neither the shipped task nor the checker source
contains the answer or its core.

| gate | what it proves |
| --- | --- |
| G1–G3 | task derives from a frozen anchor; the anchor's own published bytecode is REJECTED |
| G4–G5 | constant answers, four generic programs and the full lookup table are all REJECTED |
| G6–G8 | answer-free checking, stale challenges REJECTED, verdicts are ACCEPT/REJECT only |
| G9–G10 | cross-challenge answers REJECT (5×5 matrix, ACCEPT only on the diagonal); tampered fork / pre-state / probes / size policy / gas limit REJECT as `TASK-BINDING-MISMATCH` |
| G11 | determinism — one distinct verdict across repeats |
| G12 | freshness — 16 epochs give 16 distinct specs and 16 distinct answers |
| G13 | no answer and no way to construct one in shipped material |
| G14 | every submission derivable from shipped material REJECTS, and existence still holds |

Result: **14 / 14 PASS** on 3 representatives.

### Full materialization — one pass, no retries

All 6,767 EMITTED anchors of the sealed EVM execution census were materialized once. Each
anchor was checked to exist in the frozen corpus (**6,767 / 6,767 present, all unique**),
issued a challenge, and confirmed by a witness that was then discarded.

| bucket | anchors | why |
| --- | --- | --- |
| `LLM-TASK-ELIGIBLE` | **6,755** | witness confirmed an answer exists, then discarded |
| `SOLUTION-EXISTENCE-UNPROVEN` | **12** | 6 frozen senders cannot fund the probe transaction; 6 witness runs faulted |

**Witness answers persisted: 0.**

The 12 are recorded as unproven rather than ineligible: the family covers them, but no
witness confirmed an answer, and an unconfirmed row is never counted as eligible.

### Every row bucketed — conservation PASS

All 91,328 rows were assigned exactly one bucket. Rules that matched no row, rows matched
by no rule, duplicate row identities and any per-unit imbalance were all set to HARD-STOP;
none triggered.

| bucket | rows | anchor | task | subrow |
| --- | --- | --- | --- | --- |
| `LLM-TASK-ELIGIBLE` | 6,755 | 6,755 | 0 | 0 |
| `DUPLICATE` | 1,750 | 1,750 | 0 | 0 |
| `ANSWER-EXPOSED` | 5,728 | 2 | 0 | 5,726 |
| `TRIVIAL-OR-UNIVERSAL-SOLUTION` | 311 | 311 | 0 | 0 |
| `NO-FRESH-SEMANTIC-INSTANCE` | 0 | 0 | 0 | 0 |
| `NO-DETERMINISTIC-CHECKER` | 0 | 0 | 0 | 0 |
| `SOLUTION-EXISTENCE-UNPROVEN` | 12 | 12 | 0 | 0 |
| `FAMILY-UNSUPPORTED` | 0 | 0 | 0 | 0 |
| `NEEDS-SPEC` | 76,759 | 73,328 | 2,504 | 927 |
| `ERROR` | 13 | 13 | 0 | 0 |
| **total** | **91,328** | **82,171** | **2,504** | **6,653** |

Per-corpus conservation reproduced all 11 `(corpus, unit)` group counts exactly.

**Three buckets are empty by construction, not by finding.** `NO-FRESH-SEMANTIC-INSTANCE`,
`NO-DETERMINISTIC-CHECKER` and `FAMILY-UNSUPPORTED` can only be assigned by a gated family
examining a row. One family was gated, and it assigned neither. Their zeros mean *not
measured*, not *none exist*.

**Why 76,759 rows are `NEEDS-SPEC`.** A row leaves `NEEDS-SPEC` only on evidence: either a
gated family measured it, or a sealed family-independent fact already decides it
(duplicate lineage, published answer, universal construction, processing error). No family
was gated outside EVM, so no verdict was invented for those domains. Each such row carries
its sealed label in its reason field, so nothing is lost. In particular, a row previously
sealed as execution-proof `MINEABLE-ELIGIBLE` **stays `NEEDS-SPEC`** — execution-only
problems are not LLM problems.

### Per-domain conversion directions

| domain | direction | status |
| --- | --- | --- |
| EVM | transaction-sequence / bytecode synthesis | **GATED** — 6,755 eligible |
| Solidity | code repair / implementation / property satisfaction | not gated; **buildable offline** (a pinned `solcjs` is present and 1,408 semantic anchors carry runnable cases) |
| Rust | code repair / compile-and-test satisfying patch | not gated; `rustc` present. The corpus is public rustc UI tests whose upstream file *is* the published answer, so a "revert the mutation" conversion would be `ANSWER-EXPOSED` and fail G2 — a conforming family must bind a fresh challenge-derived obligation instead |
| Ethereum-consensus | valid operation-sequence / counterexample / state-transition input synthesis | not gated; needs SSZ container construction and a per-handler obligation derived from the published spec |
| Lean | theorem proving | **no corpus** — the toolchain is present but no Lean corpus was ever materialized, so there are 0 Lean rows to bucket |
| zk-native | counterexample / patch / audit finding | not gated; **blocked on C8** — the audit oracle is human judgement, and no deterministic answer-free checker exists |

### Final numbers, reported separately and never summed

| quantity | value |
| --- | --- |
| RAW-ANCHORS | 82,171 anchor rows (80,501 deduped; 1,670 cross-corpus overlap) + 2,504 task rows + 6,653 subrows = 91,328 ledger rows |
| **LLM-TASK-ELIGIBLE** | **6,755 anchors** · 0 tasks · 0 subrows |
| REFERENCE-LLM-SOLVED | **NOT-MEASURED** — directive item 8 permits approved offline/local models only and no model has been approved. A local offline model (`gemma4:26b`, ollama) is present but unapproved and was **not** run. No paid API, no public benchmark. |
| EXECUTION-PROOF-ELIGIBLE-SUBSET | 12,880 — unchanged, re-derived from the ledger by the verifier |
| DEFERRED · NEEDS-SPEC | 76,759 rows (73,328 anchor · 2,504 task · 927 subrow) |
| dynamic issuance instances per epoch | 6,755 fresh instances per epoch, 256-bit challenge entropy — a **rate**, never added to the anchor count |
| chunk work-units | **0 confirmed** — the halted wave's 7 challenges are cancelled, 0 consumed, and its 87 pre-registered chunking cases stay unconfirmed |

`LLM-MINEABLE-ELIGIBLE` remains **NOT-YET-DETERMINED** as a cross-domain quantity: 6,755 is
what one gated family measured, not a ceiling, and the 76,759 `NEEDS-SPEC` rows are neither
eligible nor ineligible.

### Lineage (git-ignored sandbox; hash-pinned here)

Sandbox: `local-docs/llm-mineable-census-p1-2026-08-16/`.

| artifact | role | sha256 |
| --- | --- | --- |
| `LLM-WORK-CONTRACT-V1.md` | 13 clauses, 14 gates, 10 buckets | `81c43b8f35d780dc5b49a1209a4ed8bf58f582a4b671f229ec0a05f6d559041b` |
| `CONTRACT-FREEZE.json` | contract frozen before any gate or count | `d91422336aa420616475eb9f93101821b43413dde658886e9ded60e6d4f7a10e` |
| `unified-ledger.jsonl` | 91,328 raw-material rows | `4a384f0e68c04d028ca55b970ef1ae6100aa1958f171b080df9484ca38619785` |
| `families/…/checker.py` | shipped checker — no answer, no way to build one | `b24038f83f652f549085347d949e1bdc3a57540d0802dbd93bdfcaafb73fa87a` |
| `families/…/generator.py` | generation side + existence witness — **not shipped** | `c27732e9352ce736ab73fdc24e80597b0004a858ac551b0aa166cf9506d3ff6c` |
| `families/…/test_gate.py` | G1–G14 battery, 14/14 PASS | `bb3708cc4623daaf7545dc6d69db5443a03ffd1d82c3fa2fab6ba5fc529850ce` |
| `families/…/materialization.jsonl` | 6,767 one-pass family verdicts | `abe989edadb9f5370f8a41badff7d1446dfb7b71e89b5b7d848a06cf9272e849` |
| `BUCKET-MAP.json` | bucket rules, written before the run | `5547c761c193d9b5f510e335c46e34cd68ecd7fa04ae9d4cea9b475ca7b8209f` |
| `bucketed-ledger.jsonl` | 91,328 rows, one bucket each | `d7892df472ac74a38499e97fe6d1527562ccd10586eab09516a4a1855cf4b3df` |
| `BUCKETING-RESULT.json` | conservation PASS + all tallies | `297732ebb778c5155cf5ecd5b35d18a6b15622969d39c10eb65fb0b746136543` |
| `CONVERSION-DIRECTIONS.json` | per-domain direction and status | `6b62aaa3975edca99b723c4d07b66f4e7f04cefa4ecd87777be60f5877d8befd` |
| `FINAL-NUMBERS.json` | the seven separated quantities | `40c517b74ec8c5c730b79d8377af1791d47f3e77e072f910af757547fe00a2c7` |

### Correction recorded in this entry (append-only)

`BUCKET-MAP.json` originally justified its Solidity `NEEDS-SPEC` rules with "solc is not
present in the offline sandbox". That was **factually wrong** — a pinned `solcjs` is
present in the sandbox. The justification is corrected to the true one: no Solidity family
was gated in this wave. **No bucket assignment changed**; every affected row was and
remains `NEEDS-SPEC`. A wrong reason was corrected, not a rule.

### Boundary / non-claims (Entry 3)

* **6,755 is one family's measurement, not `LLM-MINEABLE-ELIGIBLE`**, which stays
  NOT-YET-DETERMINED. It is not a ceiling and not a total.
* Zeros in `NO-FRESH-SEMANTIC-INSTANCE`, `NO-DETERMINISTIC-CHECKER` and
  `FAMILY-UNSUPPORTED` mean **not measured**, not "none exist".
* **No LLM was run.** `REFERENCE-LLM-SOLVED` is NOT-MEASURED and is never merged into
  `LLM-TASK-ELIGIBLE`. Existence was confirmed by a constructed witness, which proves a
  valid answer exists — **not** that any model can find it.
* SP1 proof counts and prover CPU/memory remain infrastructure cost; they are never LLM
  problem counts and never LLM difficulty.
* Execution-only tasks are not LLM problems; already-published expected outputs are not
  LLM answers.
* This record wires **no** consensus / BF.7 / reward / Base path. Closed-local, offline,
  non-consensus evidence — **not** public-network mining, **no** paid-API benchmark,
  **no** leaderboard claim. `mineable_now = 0`.

---

## Entry 4 — 2026-08-16 · reference-LLM calibration gate / family = REFERENCE-UNSOLVED, LLM-TASK-ELIGIBLE unchanged at 6,755

### Result in one line

A local, offline model was run once against twelve **out-of-corpus** instances of the
gated family under parameters frozen before any output existed, produced **0 / 12 ACCEPT**,
and the family is therefore recorded **`REFERENCE-UNSOLVED`** — while
**`LLM-TASK-ELIGIBLE` stays exactly 6,755, neither deleted nor reduced**, because the two
facts are separate metrics and are kept separate.

### Why this gate came before the second family

Entry 3 established that 6,755 instances are *well-posed and answerable* — a witness
proves an answer exists. It established nothing about whether a model can find one.
Gating a second family before testing that would repeat the earlier mistake of stacking
eligibility counts on an unmeasured assumption. So the reference-LLM gate ran first.

### Frozen before any model output existed

`RUN-FREEZE.json` was written by `freeze.py`, which refuses to overwrite an existing
freeze, and pins:

| frozen item | value |
| --- | --- |
| model | `gemma4:26b`, 25.8B, Q4_K_M, ollama id `5571076f3d70` |
| weights blob sha256 | `7121486771cbfe218851513210c40b35dbdee93ab1ef43fe36283c883980f0df` |
| runtime | local loopback only, no network egress |
| temperature / seed | `0` / `42` |
| token limit | `num_predict = 2048` |
| attempts / retries / feedback | `1` / `0` / `0` |
| wall-clock limit | 1200 s per instance |
| instances | 12 = 3 patterns × 4, each with its challenge and prompt sha256 |
| pass rule | ≥1 ACCEPT in **each** pattern **and** all adversarial submissions REJECTED |
| failure rule | 6,755 is NOT deleted and NOT reduced; family becomes `REFERENCE-UNSOLVED` |

`run.py` re-verified all six file bindings (**6 / 6 MATCH**) and re-derived every challenge
and prompt digest before contacting the model, with HARD-STOP on any drift. The bindings
were re-verified again at seal time: still **6 / 6 MATCH**.

### The twelve instances are outside the corpus

The anchors were synthesised deterministically from the seed `boole-calibration-3909`, not
drawn from the census corpus. **`corpus_candidates_consumed = 0`** — the sealed 6,755 is
untouched and uncontaminated.

| pattern | frozen pre-state | why |
| --- | --- | --- |
| `p1-minimal` | funded sender only | the bare case |
| `p2-populated` | sender plus unrelated accounts holding balance and storage | irrelevant state must not help |
| `p3-contract-adjacent` | sender plus a **real contract with code and storage** | the model can see published bytecode and may be tempted to copy it |

The family itself — checker, challenge derivation, formula, 192-byte bound — is byte-identical
to the one gated at 14/14. Only the anchors are new.

### What the model was and was not given

The prompt follows the repo's fixed structure: submission contract → family manifest →
official helper surface → output format → instance. Everything above `# INSTANCE` is
**identical across all twelve**. No per-instance hint, no cheat sheet, no worked recipe, no
opcode sequence. No tools, no internet, no compiler, no execution environment, no access to
the witness constructor.

### Measurements

| measurement | value |
| --- | --- |
| ACCEPT | **0 / 12** — `p1-minimal` 0/4, `p2-populated` 0/4, `p3-contract-adjacent` 0/4 |
| REJECT reasons | `MALFORMED-SUBMISSION` 8, `CODE-SIZE-EXCEEDED` 4 |
| solve time | 5.4 s min / 26.7 s max / 293.2 s total |
| generated tokens | 22,528 total; 11 / 12 stopped at the 2,048 cap |
| prompt tokens | 24,173 total |
| adversarial: empty / constant / published-fixture | **36 / 36 REJECTED** |

The checker behaved correctly on every adversarial submission, including the published
contract bytecode visible in the `p3-contract-adjacent` pre-state. The failure is the
model's, not the checker's.

### Diagnosis — truncation or no attempt?

11 of 12 replies stopped at the frozen token cap, which raises a fair question: were these
correct answers cut off? `analyze.py` answers it with facts independent of the cap, reading
only, changing no gate and no parameter:

| fact | value | what it settles |
| --- | --- | --- |
| reply contained a well-formed `ANSWER:` line | 12 / 12 | not a parsing artifact |
| submission contains `SSTORE` (0x55) anywhere | **0 / 12** | the task is unsatisfiable without writing storage — these are not partial solutions at any length |
| submission embeds challenge constant `A` or `B` | **0 / 12** | a correct answer must carry a per-instance constant; none does |
| body is majority one repeated unit | 11 / 12 | a decoding loop, not an interrupted construction |

The longest submission is 1,022 bytes of which **93 % is the unit `5b6000815260206000f3`
repeated 96 times**, after a memorised compiler-style prologue. The dominant failure mode is
recall-then-loop: the model emitted boilerplate it did not need and never attempted the
required arithmetic.

**The frozen parameters were deliberately NOT changed after these results were seen.** No
token budget was raised, no retry was added, no instance was re-run. Adjusting a gate to
improve its own outcome is the failure this ledger exists to prevent.

### Effect on the sealed numbers — none

| quantity | before | after |
| --- | --- | --- |
| `LLM-TASK-ELIGIBLE` | 6,755 anchors | **6,755 anchors — unchanged** |
| `REFERENCE-LLM-SOLVED` | NOT-MEASURED | **0 / 12 under this frozen regime**, never merged into the above |
| family status | gated 14/14 | gated 14/14 **and** `REFERENCE-UNSOLVED` |
| all ten buckets, conservation | 91,328 rows, PASS | unchanged, not re-run |
| `LLM-MINEABLE-ELIGIBLE` | NOT-YET-DETERMINED | **NOT-YET-DETERMINED** |

The two labels coexist on the same family by design: *the instances are answerable* and
*this model did not answer them* are different claims, and merging them in either direction
would destroy information.

### Sealed digests (Entry 4)

| artifact | what it is | sha256 |
| --- | --- | --- |
| `calibration/RUN-FREEZE.json` | frozen before any output | `8f9340d8d3e934043fa67514334271693bd3dca3a2f482e4f64b8b4c8e5af412` |
| `calibration/anchors.py` | 12 out-of-corpus anchors | `6d9c9e2883e541efd67e82cf057bbd3d4bf825ac48e41214473c37fb6c9a6e83` |
| `calibration/prompt.py` | frozen prompt, no per-instance hint | `5c59e804cdf958772dfdb82304aa6f9132e07cb0e42daf8872ef2e13406a7082` |
| `calibration/freeze.py` | writes the freeze, refuses to rewrite | `06b91843bc7529e97072a15bb23f4be2c828797fe81c3770e397d932a7d9de7c` |
| `calibration/run.py` | binding gate + one attempt per instance | `d6f388528f22f8eb8c8ce99b86fa0bfc3481517d8adde9eccd489ff6a8cd8008` |
| `calibration/CALIBRATION-RESULT.json` | verdicts, times, tokens, adversarial | `7abc610eef524a14f7479c5b5fb1f0b20bad07271fcd2e1409a7f06ede3db9ae` |
| `calibration/analyze.py` | read-only diagnosis | `b1046262148d7f6b2b1c9ee2dcb31ccd59d674861903e2040ff909c692f3eedd` |
| `calibration/DIAGNOSTIC.json` | truncation-vs-no-attempt evidence | `b6091e4324a777aaab1755a1caef27acc842449dd65fc203f932525774766c57` |

### Boundary / non-claims (Entry 4)

* **0 / 12 does not mean the family is unsolvable, and it does not mean this model cannot
  solve it.** It measures one model in one frozen regime — temperature 0, seed 42, 2,048
  tokens, one attempt, no retry, no feedback. A different budget or a different model is a
  different measurement and needs its own freeze.
* **`REFERENCE-UNSOLVED` is not a deletion.** No row was removed, no bucket changed, no
  conservation identity was re-run. `LLM-TASK-ELIGIBLE = 6,755` stands on the witness
  evidence sealed in Entry 3.
* `REFERENCE-LLM-SOLVED` is never merged into `LLM-TASK-ELIGIBLE`, in this entry or any
  future one.
* The model ran **locally over loopback with no network egress**, on operator approval for
  this closed gate only. **No paid API, no public benchmark, no leaderboard claim, not
  public-network mining.**
* This record wires **no** consensus / BF.7 / reward / Base path. `mineable_now = 0`.

---

## Entry 5 — 2026-08-16 · second gated LLM family / LLM-TASK-ELIGIBLE = 6,755 → 7,954 anchors

### Result in one line

A second LLM family, **`solidity-source-synth-v1`**, passed **all 14 gates**, was
materialized once over the full Solidity syntax-test anchor supply, and moved **1,199
anchors** out of `NEEDS-SPEC` into `LLM-TASK-ELIGIBLE` — raising it from **6,755 to
7,954** — while **2,346** examined anchors became `FAMILY-UNSUPPORTED` and total
conservation stayed exactly **91,328 rows**. No model was run on this family, so its
reference status is **`REFERENCE-NOT-MEASURED`**, not solved and not unsolved.

### Why this family, and why it was designed three times

Entry 3 recorded Solidity as "not gated; buildable offline". The reason it was not gated
earlier is that the obvious conversions all fail C1/C2: a Solidity corpus file's published
source *is* the expected artifact, so "repair the mutation" hands the model its own answer.
Two designs were written and killed before any gate ran:

| design | fate | why |
| --- | --- | --- |
| v0 — synthesize a precondition | **REJECTED on inspection** | the weakest precondition is mechanically derivable from the published source, and the SMT verifier is wall-clock bounded, violating C7/G11 determinism |
| v1 — satisfy an arithmetic obligation | **REFUTED by measurement** | measured, not argued: the mechanical construction compiled to **183/179 bytes** against the honest one's **175/175** — only 4–8 bytes apart, so no size bound could separate them |
| v2 — integer-square-root obligation | **GATED** | Solidity has no operator, built-in or precompile for `isqrt`, so the rule cannot be transcribed; the only mechanical answer left is a probe table, and a table is separable by size |

v1 was not discarded quietly. It was measured, it lost, and the measurement is what
selected v2. The rule "if the answer can be transcribed from published material by a fixed
procedure, there is no problem" is the same rule that produced the 2026-07-23 zero-task
outcome, applied to my own designs before the gate rather than after.

### The obligation, and why the rule may be published

Shipped to the model: the anchor's frozen Solidity source, plus the requirement to append a
contract named `BooleAnswer` such that the whole file compiles under the pin and, for every
probe word `X`, the post-state satisfies

```
storage[target][slot] == isqrt((X * a) mod 2**256) XOR b
```

with `a`, `b`, `slot`, `target` and all 24 probes derived from the 256-bit challenge.

Publishing the rule is required, not tolerated: C2, C7 and C13 together forbid a checker
that hides the obligation, because a hidden rule is an answer held in shipped material.
The family is sound because **knowing what the value is does not tell a submitter how to
compute it in EVM code within the size bound** — `isqrt` must be implemented.

### Frozen before any representative ran

| parameter | value |
| --- | --- |
| probes per instance | **24** |
| size bound | **640 bytes** of deployed code |
| compiler | solc 0.8.36, pinned WASM, sha256 `ccb677d54dfab2a9b30084eec6bb396c93eb86d58b42cc00267fd0f54f391f32` |
| compiler settings | `optimizer { enabled: true, runs: 200 }` |
| executor | pinned revm 38.0.0 native runner, SpecId::CANCUN |
| transaction gas limit | 300,000 |
| answer contract name | `BooleAnswer` |
| network | none |

### The size bound was verified, not asserted

The bound is placed by a counting argument: any probe table must embed 24 distinct 256-bit
constants, and the cheapest EVM encoding of a 256-bit constant is `PUSH32` at 33 bytes, so
**no table can be smaller than 24 × 33 = 792 bytes**. 640 sits below that floor and above
the honest construction. The gate then had to confirm it in bytes:

| construction | compiled size | verdict at 640 |
| --- | --- | --- |
| honest Newton iteration | **299 bytes** | ACCEPT |
| probe lookup table | **1,887 bytes** (floor 792) | REJECT · `CODE-SIZE-EXCEEDED` |

G5 also re-ran the table **with the bound lifted** and required ACCEPT. That half is the
load-bearing half: it proves the table is *correct but too big*, so the size bound — not a
bug and not luck — is what stops it. The design stated in advance that if a table had fit
within 640 bytes, v2 would be **recorded as refuted and the bound would not be raised**.
That did not happen; the margin is 2.1× headroom for the honest answer and 152 bytes of
clearance below the table's theoretical floor.

### Gate battery — 14/14 PASS

| # | requirement | outcome |
| --- | --- | --- |
| G1 | correct newly generated answer | ACCEPT |
| G2 | the anchor's own published source | REJECT |
| G3 | empty / whitespace-only answer | REJECT · `EMPTY-ANSWER` |
| G4 | constant answers (0, 1, 42) | REJECT |
| G5 | universal answers + probe table | REJECT (table: `CODE-SIZE-EXCEEDED`; ACCEPT only with the bound lifted) |
| G6 | answer replayed under a re-issued challenge | REJECT |
| G7 | stale / swapped challenge | REJECT · `STALE-CHALLENGE` |
| G8 | another anchor's accepted answer | REJECT |
| G9 | 5×5 challenge matrix | ACCEPT on the diagonal only, 20/20 off-diagonal REJECT |
| G10 | tampered fork, pre-state, probes, anchor source, size policy, gas limit, target | 7/7 REJECT · `TASK-BINDING-MISMATCH` |
| G11 | repeated runs, same task and answer | identical verdict, 3/3, for three different submissions |
| G12 | changed challenge | 16/16 distinct obligations, answers and probe sets; ≥128-bit entropy |
| G13 | answer bytes in shipped task or checker | **0**; the checker holds no answer-constructing function |
| G14 | answer withheld, 12 submissions built only from shipped bytes | 12/12 REJECT |

G14 is the operative one. Its twelve submissions include the published rule transcribed
with the root omitted, the rule with a cheap shift standing in for the root, the rule with
a division standing in for the root, each spec constant stored directly, the challenge
itself, the target address, the first probe, and an empty contract. All twelve are
assembled only from bytes the model already holds. All twelve REJECT.

### Full materialization — one pass, no retries

Every `.sol` anchor under the frozen Solidity syntax-test corpus was materialized once: a
challenge was issued, the witness was compiled against the anchor, size-checked, executed
against all 24 probes, and then discarded.

| outcome | anchors | meaning |
| --- | --- | --- |
| task exists | **1,201** | witness confirmed an answer exists, then discarded |
| `COMPILE-FAILED` | 2,345 | the anchor's own frozen source does not build under the pin |
| `EMPTY-RUNTIME-CODE` | 1 | the appended answer contract compiled to no deployed code |
| **scanned** | **3,547** | 1,201 + 2,346 = 3,547 |

**Witness answers persisted: 0.** Zero anchors failed on size and zero on probe mismatch:
where the anchor built at all, the honest construction fit the bound and satisfied every
probe.

That 2,345 anchors do not compile is expected and is not a defect: the corpus is a
compiler's syntax-error test suite, and most of its files are deliberately invalid.

### Two scanned files are not ledger rows — and were not counted

The disk scan found **3,547** files; the sealed ledger holds **3,545** syntax-test anchors.
The two extra files both produced a valid task, and both were **excluded from every count**:

* `test/libsolidity/syntaxTests/parsing/overloaded_functions.sol`
* `test/libsolidity/syntaxTests/types/functionTypes/function_parameter_return_types_success.sol`

The ledger's row set is sealed and append-only. A row that was never in it does not enter
it because a later pass happened to find the file. The eligible count therefore reports
**1,199**, not 1,201, and the discrepancy is recorded here rather than absorbed.

### Bucketing — rules frozen first, conservation PASS

`BUCKET-MAP-V2.json` was written and hashed **before** the re-bucketing run. It adds rules
only; no `BUCKET-MAP-V1` assignment was revised. Both rules are dictated by the sealed
bucket definitions, not chosen after seeing counts:

* covered anchor with verdict ACCEPT → `LLM-TASK-ELIGIBLE`
* covered anchor with any other verdict → `FAMILY-UNSUPPORTED`
* anchor not covered by this family → bucket unchanged

| bucket | before | after | change |
| --- | --- | --- | --- |
| `LLM-TASK-ELIGIBLE` | 6,755 | **7,954** | +1,199 |
| `FAMILY-UNSUPPORTED` | 0 | **2,346** | +2,346 |
| `NEEDS-SPEC` | 76,759 | **73,214** | −3,545 |
| `ANSWER-EXPOSED` | 5,728 | 5,728 | — |
| `DUPLICATE` | 1,750 | 1,750 | — |
| `TRIVIAL-OR-UNIVERSAL-SOLUTION` | 311 | 311 | — |
| `SOLUTION-EXISTENCE-UNPROVEN` | 12 | 12 | — |
| `NO-FRESH-SEMANTIC-INSTANCE` | 0 | 0 | — |
| `NO-DETERMINISTIC-CHECKER` | 0 | 0 | — |
| `ERROR` | 13 | 13 | — |
| **total** | **91,328** | **91,328** | **0** |

Five hard stops were enforced in code and none triggered: no row assigned twice, no row
unassigned, no change in total rows, no revision of a non-`NEEDS-SPEC` V1 assignment, and
all 11 `(corpus, unit)` group counts reproduced exactly. The independent ledger verifier
re-ran on the v2 ledger: **91,328 rows, 11 groups, conservation PASS**, and
`EXECUTION-PROOF-ELIGIBLE-SUBSET` re-derived unchanged at **12,880**.

**`FAMILY-UNSUPPORTED` is not a verdict of ineligibility.** Its sealed definition is "a
gated family exists for the domain but does not support this row's construct." It records
that *this* family cannot found a task on that anchor under this pin. It does not say no
family ever could.

### What this family does not cover

The family's declared coverage is Solidity syntax-test anchors only. Every other Solidity
row keeps its V1 bucket: 1,433 SMT-checker anchors, 553 grammar-node anchors and the
semantic-test rows all remain `NEEDS-SPEC` or their prior bucket. Two gated families now
exist across six domains; four domains remain ungated.

### Updated numbers, reported separately and never summed

| quantity | value |
| --- | --- |
| RAW-ANCHORS | 91,328 ledger rows — unchanged |
| **LLM-TASK-ELIGIBLE** | **7,954 anchors** (6,755 EVM + 1,199 Solidity) · 0 tasks · 0 subrows |
| `FAMILY-UNSUPPORTED` | 2,346 anchors — examined by a gated family, not supported by it |
| REFERENCE-LLM-SOLVED | still **NOT-MEASURED**. `evm-bytecode-synth-v1` is `REFERENCE-UNSOLVED` (Entry 4); `solidity-source-synth-v1` is **`REFERENCE-NOT-MEASURED`** — no model was run on it |
| EXECUTION-PROOF-ELIGIBLE-SUBSET | 12,880 — unchanged, re-derived by the verifier |
| DEFERRED · NEEDS-SPEC | 73,214 rows |
| dynamic issuance per epoch | 7,954 fresh instances per epoch, 256-bit challenge entropy — a **rate**, never added to the anchor count |

`LLM-MINEABLE-ELIGIBLE` remains **NOT-YET-DETERMINED**. 7,954 is what two gated families
measured about problem structure; it is not a claim that any model can solve them.

### No model was run on this family

The approval recorded in Entry 4 covered **one** closed local calibration gate. It was
consumed there. No model — local or otherwise — was run against `solidity-source-synth-v1`,
and none will be without a fresh approval. Running one would also not change the 1,199:
Entry 4's failure rule is symmetric and stands, so a model result can only add a reference
status, never delete a structurally confirmed anchor.

### Lineage (git-ignored sandbox; hash-pinned here)

Root: `local-docs/llm-mineable-census-p1-2026-08-16/`

| artifact | role | sha256 |
| --- | --- | --- |
| `BUCKET-MAP-V2.json` | bucket rules, frozen before the run | `0591d9b68f79eeb3bde9b47929694eb487a437add2164e776db052a09367674a` |
| `BUCKETING-RESULT-V2.json` | move counts, hard stops, conservation | `ca3611ca9a3376bb9a139b014bd78c88a72b3373b20354578ddd96d0bc4bd762` |
| `bucketed-ledger-v2.jsonl` | 91,328 rows, one bucket each | `43afd65c70fb061ebffc2ef5da1e070647fb64b8ae264be60c44f5e8e77539af` |
| `rebucket_v2.py` | append-only re-bucketing with hard stops | `39304dc8fe4002d9fc30e95f127a95f4b9fee6400e8c1d8f429141f099c7e867` |
| `families/solidity-source-synth-v1/DESIGN.md` | pre-registered v2 design and frozen parameters | `21f3270b1a1831eb826d432dab19937e4f0b7af3451ba1c485143274dde3e364` |
| `families/solidity-source-synth-v1/DESIGN-REJECTED-v0.md` | the rejected v0 design, kept as evidence | `e1b2d6a6980485ead270d247df79aa296b4175b4cd2a73dbccca22517f1b6779` |
| `families/solidity-source-synth-v1/checker.py` | shipped answer-free checker | `edcdd45a7e2e5fabc4f2b6b2338fe57369c2fc6dd202f76e29fcdfd27a995f4e` |
| `families/solidity-source-synth-v1/generator.py` | generation side, **not shipped** | `796f39dde24c8bd98e4e76e168fda4daa239d74066f626e1fa1c27763d1bcc1b` |
| `families/solidity-source-synth-v1/test_gate.py` | G1–G14 battery | `208e9119ab39d36d83afb868e94e32dbaa22b046bf80943b087d2837dfdac255` |
| `families/solidity-source-synth-v1/compile.mjs` | pinned-compiler bridge | `e9fefede85cd1e2910f220861b394f601e65653e88ac1a54535cc40076a9649b` |
| `families/solidity-source-synth-v1/materialize.py` | one-pass materialization | `d59494cb2d76fc8ec7c980e9e13bf7a88cfa6df0d31c19a9ace5bed040e51c60` |
| `families/solidity-source-synth-v1/MATERIALIZATION.json` | 3,547 rows with verdicts | `f653da0560be11afe9a349697c78097fd5a2b516f7c68f1728b952216c37296e` |

### Boundary / non-claims (Entry 5)

* **Closed local, offline, non-consensus.** No network, no paid API, no public benchmark,
  no leaderboard claim, no public mining. `mineable_now = 0`. Nothing here is wired to
  consensus, reward, issuance or Base.
* **7,954 is a structural count, not a solved count.** It counts anchors where a
  challenge-derived instance exists and a witness confirmed an answer exists. It says
  nothing about whether any model can produce one.
* **No LLM was run on this family.** `REFERENCE-NOT-MEASURED` means not measured, not
  "expected to pass".
* **The size bound was verified in bytes**, not argued. Had the table fit, v2 would have
  been recorded as refuted rather than rescued by a larger bound.
* **`FAMILY-UNSUPPORTED = 2,346` is not an ineligibility finding**, and the three buckets
  still at zero remain *not measured*, not *none exist*.
* **No sealed number was rewritten.** Entries 1–4 stand as written; this entry adds rows'
  movement out of `NEEDS-SPEC` only, under rules frozen before the run, with conservation
  reproduced by an independent verifier.

---

## Entry 6 — 2026-08-16 · Solidity reference-LLM calibration / family = REFERENCE-UNSOLVED, LLM-TASK-ELIGIBLE unchanged at 7,954

### Result in one line

Under parameters frozen and hashed before any model output, the approved local model
produced **0 ACCEPT out of 12** on out-of-corpus instances of `solidity-source-synth-v1`
(0/4 in each of the three patterns), so the family is recorded **`REFERENCE-UNSOLVED`** —
while **`LLM-TASK-ELIGIBLE` stays exactly 7,954**, with the family's 1,199 anchors neither
deleted nor reduced.

### Frozen before any model output existed

`RUN-FREEZE.json` was written, hashed and bound to the family's files before the first
prompt was sent. It was written **twice, both times before any output existed**: the first
write omitted the file-binding hashes the runner needs to refuse a drifted run. No
parameter differed between the two writes; only the second is sealed. That is recorded in
the file itself rather than left to be inferred.

| frozen item | value |
| --- | --- |
| model | `gemma4:26b`, ollama id `5571076f3d70` |
| weights blob sha256 | `7121486771cbfe218851513210c40b35dbdee93ab1ef43fe36283c883980f0df` (17,987,569,344 bytes — byte-identical to the model used in Entry 4) |
| temperature / seed | 0 / 42 |
| `num_predict` | 4,096 |
| attempts per instance | **1** · retries 0 · feedback rounds 0 |
| wall-clock limit | 1,200 s per instance |
| runtime | ollama on local loopback, no network egress |
| submission interface | Solidity source only — one `BooleAnswer` contract appended to the frozen anchor, compiled by the pin |
| extraction rule | last fenced block; else the whole reply if it declares `contract BooleAnswer`; else empty |
| pass rule | ≥1 ACCEPT in **every** pattern ⇒ `REFERENCE-LLM-CALIBRATED` |
| failure rule | 1,199 is NOT deleted and NOT reduced; family becomes `REFERENCE-UNSOLVED` |

**`num_predict` was raised from the previous gate's 2,048 to 4,096, and the reason was
written into the freeze before any output**: the answer here is Solidity source rather
than a hex string, so identical content costs several times more tokens, and Entry 4's
11/12 cap hits confounded that reading. This is a parameter of a new gate fixed in
advance, not a retroactive loosening of the old one. It was not raised again afterwards.

### The family was not touched

The freeze binds five file hashes and the runner refuses to start if any differ. Three of
them are the artifacts sealed in Entry 5, and they matched exactly:

| file | sha256 | same as Entry 5 |
| --- | --- | --- |
| `checker.py` | `edcdd45a7e2e5fabc4f2b6b2338fe57369c2fc6dd202f76e29fcdfd27a995f4e` | yes |
| `compile.mjs` | `e9fefede85cd1e2910f220861b394f601e65653e88ac1a54535cc40076a9649b` | yes |
| `generator.py` | `796f39dde24c8bd98e4e76e168fda4daa239d74066f626e1fa1c27763d1bcc1b` | yes |

Same checker, same challenge derivation, same isqrt rule, same 640-byte bound. Only the
anchors are new.

### Zero corpus anchors were consumed

The twelve instances are built on anchors written for this gate and present in no corpus:
four `p1-minimal` (a bare file), four `p2-library-interface` (an interface and a library
already declared), four `p3-inheritance-modifier` (an abstract base, a modifier, an event
and a mapping). **None of the 1,199 sealed anchors was issued, consumed or scored.**

**All twelve were confirmed solvable before the freeze.** A witness produced an accepted
answer for 12/12 at 299 bytes against the 640-byte bound, and was discarded. This ordering
is what makes the result attributable: a failure below is the model failing a solvable
instance, not the model meeting an impossible one.

### What the model was and was not given

Given: the submission contract, the family manifest, the helper surface, the output
format, the frozen anchor source, `A`, `B`, `SLOT`, `TARGET`, `SENDER`, the size bound,
the compiler pin, the block environment, the pre-state and all 24 probe words.

The helper surface states that `isqrt(n)` is the largest `r` with `r*r <= n`, that
Solidity has no operator, built-in or precompile for it, and that Solidity 0.8 reverts on
overflow unless the code is inside an `unchecked` block.

Not given: any method for computing an integer square root, any code, any per-instance
hint, any tool, any compiler, any execution environment, any internet access, and no part
of the answer constructor.

### Measurements

| measurement | value |
| --- | --- |
| ACCEPT | **0 / 12** |
| ACCEPT per pattern | `p1-minimal` 0/4 · `p2-library-interface` 0/4 · `p3-inheritance-modifier` 0/4 |
| reject reasons | `COMPILE-FAILED` 9 · `PROBE-MISMATCH` 3 |
| empty submissions | 0 — every reply produced an extractable submission |
| solve time | 6.5 s – 51.2 s per instance; **189.3 s total** |
| tokens | 35,719 prompt · 13,316 generated |
| hit the token cap | **1 / 12** |
| submission size | 297 – 1,482 characters |
| adversarial rejection | **36 / 36 REJECT** — empty, constant and the published anchor source, on all twelve instances |

### Diagnosis — this failure is not the previous failure

A read-only pass over the frozen transcripts. No model call, no prompt edit, no retry, no
parameter change.

| question | answer |
| --- | --- |
| was the output truncated? | **no — 1/12 hit the cap.** The pre-registered 4,096 removed Entry 4's confound |
| did it produce the required contract? | **12/12 declare `contract BooleAnswer`** |
| did it use the calling convention? | **12/12 use `fallback`** |
| did it attempt the actual obligation? | **10/12 contain both a square-root construction and a loop** |
| did it handle the 0.8 overflow rule? | **0/12 use `unchecked`** |

This is a materially different failure from Entry 4. There, 0/12 submissions contained a
storage write or a single challenge constant — nothing was a partial solution at any
length. Here the model consistently produced the right shape: the right contract name, the
right entry point, and in 10 of 12 cases a genuine attempt at the integer square root. It
failed on Solidity-level correctness.

The single most consequential omission is `unchecked`. The rule is `(X * A) mod 2**256`,
and under Solidity 0.8 `X * A` reverts on overflow, which for 256-bit challenge constants
is essentially every probe. A reverted call writes nothing, the slot stays zero, and the
probe mismatches. **0/12 used it**, though the helper surface states the rule — which is
sufficient on its own to account for all three `PROBE-MISMATCH` instances.

The nine compile failures are mostly genuine Solidity and inline-assembly errors: three
attempts at an explicit conversion from `bytes calldata` to `uint256`, three malformed
assembly assignments, one parse error, one stray `^`. **One is not a mathematical failure
at all** — a duplicate SPDX license header, i.e. the model re-emitted a file preamble in a
fragment that is appended to an existing file. That is a packaging mistake, and it is
reported here as a confound of size one rather than folded into the other eight.

**No parameter was changed after seeing this.** No prompt edit, no retry, no larger token
budget, no relaxed size bound.

### Effect on the sealed numbers — none

| quantity | before Entry 6 | after Entry 6 |
| --- | --- | --- |
| `LLM-TASK-ELIGIBLE` | 7,954 anchors | **7,954 anchors — unchanged** |
| of which Solidity | 1,199 | **1,199 — unchanged** |
| `solidity-source-synth-v1` status | gated 14/14, `REFERENCE-NOT-MEASURED` | gated 14/14 **and** `REFERENCE-UNSOLVED` |
| `REFERENCE-LLM-SOLVED` | not measured | **0 instances**, on 12 out-of-corpus instances only |
| all ten buckets, conservation | 91,328 rows, PASS | unchanged, not re-run |

Both gated families are now `REFERENCE-UNSOLVED` under the same local model: 0/12 for EVM
and 0/12 for Solidity, 0/24 overall. Structural eligibility and reference solvability are
tracked separately and are never merged; a model failing a structurally valid problem does
not delete the problem.

### Sealed digests (Entry 6)

Root: `local-docs/llm-mineable-census-p1-2026-08-16/families/solidity-source-synth-v1/calibration/`

| artifact | role | sha256 |
| --- | --- | --- |
| `RUN-FREEZE.json` | every parameter, fixed before any output | `9fa8e408d40c5b3f3f1ad207125865b97a5e625132d92b145074cddf4856936b` |
| `CALIBRATION-RESULT.json` | 12 rows, verdicts, timings, tokens | `d8cfbfeb14dab745ba69a13d55e97023368da778e40dc120eeb3a294fb4e7e97` |
| `DIAGNOSTIC.json` | read-only post-hoc diagnosis | `477ed8d327b63b2deb6c2609e825c3aaac466c1b26ef32954a23c4f6b392602a` |
| `anchors.py` | the twelve out-of-corpus anchors | `72fd27584e014f8e3a6510622dadf04c1dc34c99a5dd5a96c4ac92848c22aed6` |
| `prompt.py` | frozen prompt and extraction rule | `1b949a2f0b10496f2175f30cffa7c37e1fc54011f46e7d1f9359d74e0dfc6a40` |
| `freeze.py` | freeze builder with existence hard-stop | `833c149970168134814636c5d43374506b30911f6062a5c24bf260a3c37032af` |
| `run.py` | runner with drift-refusal gate | `0dbae038274fc4e5efc7fedad865a09a54d5c42a59ca7f9670555ad7017515f8` |
| `analyze.py` | read-only diagnosis | `9a92d527e52aa2c8ac0cd2d265529fb5c9d8fcc7134fc1f043e1e9594a7f4663` |

### Boundary / non-claims (Entry 6)

* **Closed local, offline, non-consensus.** One local model over loopback. No paid API, no
  public benchmark, no public mining, no leaderboard claim. `mineable_now = 0`.
* **This measures one model in one frozen regime on twelve instances.** It does not show
  the family is unsolvable, that a stronger model would fail, or that this model would fail
  under different decoding. Those are untested, and "untested" is not "false".
* **`REFERENCE-UNSOLVED` is not a deletion.** No row was removed, no bucket changed, no
  conservation identity re-run. `LLM-TASK-ELIGIBLE = 7,954` stands on witness-confirmed
  structure, which a model result cannot revoke.
* **0 ACCEPT is `REFERENCE-LLM-SOLVED = 0` for these twelve instances only**, and is never
  merged into `LLM-TASK-ELIGIBLE`. Symmetrically, a nonzero count would never have meant
  the model solved 7,954 anchors.
* **The one SPDX-duplicate failure is reported separately** because it is a packaging
  error, not a failure at the problem. Folding it into the mathematical failures would
  overstate the finding by one instance.

---

## Entry 7 — 2026-08-16 · LLM-MINER-INTERFACE-V1 frozen / no episode run, all sealed numbers unchanged

**Directive:** operator msg 3917 — *"새 family를 더 만들기 전에 LLM-MINER-INTERFACE-v1 동결
… 공식 compiler/checker만 도구로 허용 … 시도 횟수·토큰·시간을 전역 고정 … 컴파일 오류를 보고
스스로 수정하는 agentic loop 허용 … 기존 단발 결과는 SINGLE-SHOT-REFERENCE-UNSOLVED로 영구
보존 … 새 인터페이스는 별도 버전으로 같은 대표 24건을 정확히 1회 측정."*

This entry seals an **interface specification only**. **No episode has been run, no model
has been invoked, and no number in this ledger changes.** It is the freeze that must exist
*before* any measurement, sealed while there is still nothing to bias it.

### Why the interface, and not another family

Entries 4 and 6 measured 0 ACCEPT out of 24 under one profile: a single attempt, no tools,
no feedback, the whole program written blind. Entry 6's own diagnosis found that 10 of 12
Solidity submissions attempted the correct mathematics and were lost to language-level
mistakes a compiler names in one second — 0 of 12 used `unchecked`, which alone accounts
for every probe mismatch.

That profile measures something real, but it is not how a coding agent is used. Building
more families on top of a mis-specified miner interface would multiply the mis-specification.

### The prior results are preserved, not superseded

Entries 4 and 6 keep every number exactly as sealed, under a permanent name:

> **`SINGLE-SHOT-REFERENCE-UNSOLVED`** — one attempt, no tools, no feedback. 0/12 EVM,
> 0/12 Solidity, 0/24 overall.

A single-shot number and an agentic number are never averaged, summed, or substituted for
one another. They are two named profiles over the same instances.

### What the interface fixes (`LLM-MINER-INTERFACE-V1`, clauses I1–I9)

| clause | content |
| --- | --- |
| I1 | the miner receives the shipped task and nothing more |
| I2 | never: the answer, the witness, the generator, any expected value, per-instance hints, internet, a human |
| I3 | exhaustive tool surface — `compile` and `check`, the official pinned components, nothing else |
| I4 | global budgets, identical for every instance, fixed before any episode |
| I5 | temperature 0, fixed seed, deterministic pinned tools, full transcript recorded |
| I6 | forbidden: prompt edits after results, re-runs, budget raises after results, bound changes, per-instance adaptation |
| I7 | labels, none of which is ever merged into `LLM-TASK-ELIGIBLE` |
| I8 | exactly one pass over the same 24 sealed representatives; the harness decides the verdict |
| I9 | hard stops that end the run rather than repair it |

**Budgets (I4), fixed here and never tuned per problem:** 1 episode per instance · 8 model
turns · 8 `compile` calls · 4 `check` calls · 24,576 generated tokens · 1,800 s wall clock ·
0 human interventions.

### The checker is an oracle — the bound is stated before the run, not after

Exposing `check` as a tool must not let a miner *construct* an answer instead of computing
one. `check` returns `ACCEPT`, or `REJECT` with its reason and the index of the first
failing probe — **never an expected value, never a count of passing probes**. Each call
therefore yields at most one bit beyond what is already public. Learning one expected
256-bit word by querying costs on the order of `2**255` calls; the budget is 4. The oracle
cannot substitute for solving, and C2/C8/C13 answer-freeness is unaffected.

### An asymmetry between the two families, stated rather than hidden

`compile` exists only for families whose submission is source code. `evm-bytecode-synth-v1`
submits raw bytecode, so under v1 it has the checker and nothing else, and its agentic loop
is genuinely weaker than Solidity's. The two families are **not comparable under v1**;
their results are reported separately and never pooled. Fixing this would mean adding an
assembler to the tool surface, which is outside the operator's stated surface and is
therefore left to a future version rather than added quietly.

### Sealed digest (Entry 7)

| artifact | role | sha256 |
| --- | --- | --- |
| `local-docs/llm-mineable-census-p1-2026-08-16/LLM-MINER-INTERFACE-V1.md` | the frozen interface, clauses I1–I9 | `41febe7f88901f66f73176b92276d2026f4f309cd39900740b46db72a07c4562` |

### What did not change

| quantity | before Entry 7 | after Entry 7 |
| --- | --- | --- |
| `LLM-TASK-ELIGIBLE` | 7,954 anchors | **7,954 — unchanged** |
| `LLM-MINEABLE-ELIGIBLE` | `NOT-YET-DETERMINED` | **`NOT-YET-DETERMINED` — unchanged** |
| Entry 4 / Entry 6 results | 0/24 | **0/24, relabelled `SINGLE-SHOT-REFERENCE-UNSOLVED`, values untouched** |
| all ten buckets, conservation | 91,328 rows, PASS | unchanged, not re-run |

### Boundary / non-claims (Entry 7)

* **Nothing was measured.** This entry seals a specification. No model ran, no instance was
  answered, no anchor was consumed, and no approval to run a model exists at seal time.
* **A frozen interface is not a prediction.** It does not claim the agentic profile will do
  better than 0/24. It claims only that the question will be asked once, under fixed rules
  written down in advance.
* **`LLM-MINEABLE-ELIGIBLE` is not decided by this interface either.** Structural
  eligibility (7,954) and model results stay separate, as in every prior entry.
* **Closed local, offline, non-consensus.** `mineable_now = 0`. No paid API, no public
  benchmark, no public mining, no leaderboard claim.

---

## Entry 8 — 2026-08-16 · Entry 7 correction + LLM-MINER-INTERFACE-V1.1 frozen / still no episode run

**Directive:** operator msg 3919 — *"check가 '처음 틀린 프로브 번호 + 사유'를 주므로 최대 1비트
누출은 아닙니다 … 1비트 누출 주장 철회 … 제한된 적응형 피드백으로 명명 … check 출력은 고정
형식만 허용 … 실제값·기대값·통과 개수·상태값 출력 금지 … 새 calibration challenge 24개를 결과
전에 동결. 이전 challenge 재사용 금지."*

This entry corrects Entry 7 and freezes the corrected interface. **No episode has been run
and no number in this ledger changes.** Entry 7 stays exactly as sealed and is not edited;
the correction lives here, which is what an append-only ledger is for.

### The correction: Entry 7's leakage claim was wrong

Entry 7 stated that each `check` call "yields at most one bit beyond what is already
public". **That claim is withdrawn.** The operator caught the error: the reason code and
the probe index are themselves information, so the leak is not one bit.

The correct accounting, over the closed output format below:

| family | distinct outputs per call | bits per call | ≤ 4 calls |
| --- | --- | --- | --- |
| `solidity-source-synth-v1` | 1 ACCEPT + 7 non-probe reasons + 24 probe indices = **32** | **5.000** | **≤ 20.00 bits** |
| `evm-bytecode-synth-v1` | 1 ACCEPT + 6 non-probe reasons + 12 probe indices = **19** | **4.248** | **≤ 16.99 bits** |

The property is renamed accordingly. It is not a one-bit oracle; it is
**`LIMITED-ADAPTIVE-FEEDBACK`** — a bounded but genuinely adaptive signal that tells the
miner *that* it is wrong and *where* the first divergence is.

**What the corrected bound does and does not establish.** It does not say the feedback is
negligible — it is real help, and it is meant to be. It says only that the feedback cannot
*replace* solving: the obligation is 24 (or 12) expected 256-bit words, i.e. 6,144 (or
3,072) bits, against ≤ 20.00 (or ≤ 16.99) bits of total feedback, which cannot determine
even one word. Answer-freeness holds — for this reason, not the one Entry 7 gave.

### `check` now has a fixed output format, and the stripping is proven

The tool returns exactly `ACCEPT`, or `REJECT(reason_enum, first_probe_index)`, with the
index present only for `PROBE-MISMATCH`.

**Forbidden without exception:** expected values, observed values, any post-state, the
number or fraction of probes passed, the probe *word* rather than its index, and compiler
text, byte counts or any free-form detail.

The reason strings the two shipped checkers return were **already fixed constants, not free
strings**, so no checker was modified — the operator's "이미 enum이면 문서 정정만" case
applies to the enum itself. But the checkers do return extra fields (a compiler detail on
`COMPILE-FAILED`, a size on `CODE-SIZE-EXCEEDED`, the probe word on `PROBE-MISMATCH`), so
the interface layer projects their output onto the closed format and drops the rest, as a
**whitelist** — a field added to a checker later is excluded by default rather than leaking
until someone notices.

A gate battery proves the stripping instead of asserting it: **T1–T9, 14 tests, run once
per family, both PASS** (3 skips on EVM and 1 on Solidity, all of them the deliberate
`compile`-tool asymmetry). T2 is the strongest: it serialises every `check` output and
searches it for every expected word, in decimal and in hex, and requires no match. T9 feeds
the projection a synthetic verdict carrying `detail`, `gas_used`, `post_state` and
`probes_passed`, and requires all four to be dropped.

### Why v1.1 rather than a document-only correction

The enum trigger did not fire. **I8 did**: Entry 7 said the agentic pass would reuse the
challenges of Entries 4 and 6, and the operator forbids that. Replacing a clause is a
version, so the corrected interface is frozen as `LLM-MINER-INTERFACE-V1.1`.

### New challenges, frozen before any result

The 24 representative anchors are unchanged — the same 12 EVM and 12 Solidity out-of-corpus
anchors — but every one is issued a **fresh challenge at epoch 1**, and the freeze records
each anchor's epoch-0 challenge and asserts the new one differs. Under each family's
derivation a new epoch moves the constants, the slot and every probe word, so the required
answer is genuinely different and nothing from the sealed single-shot transcripts carries
over. **Zero corpus anchors are consumed.**

A valid answer was confirmed to exist for all 24 instances at the new challenges, by
witness, and then discarded (C9) — never shipped, never logged.

### One change of mine, disclosed as mine

Entry 7 defined the episode's submission as "the last source passed to `check`, or to
`compile`". That loses an answer written in a turn that called no tool, which would
understate the model for a formatting reason. v1.1 replaces it with a **standing
submission**: the most recent extractable answer from any turn, tool call or not. The
harness still never picks a tool on the model's behalf, so no budget is ever spent by a
formatting accident, and the verdict is still a final harness check outside the budget.
This is mine, made before any result existed, and is not part of the directive.

### Sealed digests (Entry 8)

Root: `local-docs/llm-mineable-census-p1-2026-08-16/`

| artifact | role | sha256 |
| --- | --- | --- |
| `LLM-MINER-INTERFACE-V1.1.md` | the corrected interface, clauses C1–C7 | `f98051921f3e158afef063bf45353a8d57e61f89b6aea55ace6744f0a665f390` |
| `interface-v11/RUN-FREEZE-V11.json` | model, decoding, budgets, both family freezes | `76469541aa3478188c3695fee8abaa15db6c3fc4cf80601ef0158abec055bf28` |
| `interface-v11/FREEZE-evm-bytecode-synth-v1.json` | 12 new challenges, prompt digests, existence | `784118609d0b70839337db88ee31f64e4e86c7a0df77053acc752ee4d8e53611` |
| `interface-v11/FREEZE-solidity-source-synth-v1.json` | 12 new challenges, prompt digests, existence | `bda74097e40f8dc01b770ef4592715e0bb8e9d90d956ab12c8b65ad4cf669d86` |
| `interface-v11/surface.py` | the two tools, closed enum, budget accounting | `9b32183fd5b74c3e4d10195ba35bdd389636e070f00e459c418300f476efab09` |
| `interface-v11/test_surface.py` | T1–T9 stripping and budget battery | `13ef61a3621cf7f03e15a886358ba7d1cf07cbd679ec7ac58881fb25187f129e` |
| `interface-v11/protocol.py` | episode prompt splice and turn protocol | `ca7d228b2858d7ebae3e643afa45f32e0efa350b44c8cba7ff7f1f0c84aa689f` |
| `interface-v11/episode.py` | the agentic loop | `4ba7f1e05456bb8fd4ca37b39c65dbd7924d89e198d085e0a27be6b4b24343c8` |
| `interface-v11/instances.py` | the 24 instances at epoch 1 | `001281f2eac5d431ca7fcfdd9400d48ed2332c7cf136693ae8485c27585237a0` |
| `interface-v11/run_agentic.py` | runner with drift refusal | `ca832361522059d34275c021bca2a4e506b106c77ee849f4f0e8a31d3b1e9f4c` |
| `interface-v11/freeze.py` | per-family freeze builder | `22034dcf6b123dee8d3bb6fecd6564162cb3173c0c7a0313822d9187a66acfb9` |
| `interface-v11/seal.py` | run-freeze builder | `cfc423951c35ca4348f71d2d1a7813a520f073c87bd67b84fafa47ffadb5a7c8` |

### The episode prompt is a splice, not a rewrite

The episode prompt is built from each family's already-frozen single-shot prompt, replacing
only the submission contract and the output format. The family manifest, the official
helper surface and the instance section are carried over **byte-for-byte**. The two
profiles therefore differ in the interface and in nothing else, so a difference in the
numbers cannot be blamed on a reworded problem statement.

The runner re-derives every challenge and every episode-prompt digest before it starts and
hard-stops on any drift. Both were exercised without invoking the model: 24/24 instances
verified, and a deliberately altered bound file was refused with
`HARD-STOP: interface-v11/protocol.py changed since the freeze`.

### What did not change

| quantity | before Entry 8 | after Entry 8 |
| --- | --- | --- |
| `LLM-TASK-ELIGIBLE` | 7,954 anchors | **7,954 — unchanged** |
| `LLM-MINEABLE-ELIGIBLE` | `NOT-YET-DETERMINED` | **`NOT-YET-DETERMINED` — unchanged** |
| Entries 4 and 6 | 0/24, `SINGLE-SHOT-REFERENCE-UNSOLVED` | **unchanged, values untouched** |
| Entry 7 | sealed | **sealed and unedited; corrected here, not rewritten** |
| budgets | 8 turns · 8 compile · 4 check · 24,576 tokens · 1,800 s | **unchanged** |
| all ten buckets, conservation | 91,328 rows, PASS | unchanged, not re-run |

### Boundary / non-claims (Entry 8)

* **Nothing was measured.** No model ran, no instance was answered, no anchor was consumed.
* **A withdrawn claim stays visible.** Entry 7's wrong sentence is not deleted from the
  ledger; it is contradicted here, with the arithmetic that replaces it.
* **The corrected bound is an upper bound on information, not a hardness proof.** It shows
  the feedback cannot specify the answer. It does not show the problem is hard.
* **The family asymmetry is not repaired.** `compile` exists only for source-code families,
  so `evm-bytecode-synth-v1` is weaker under this interface. EVM and Solidity are tallied
  separately and never pooled.
* **Closed local, offline, non-consensus.** `mineable_now = 0`. No paid API, no public
  benchmark, no public mining, no leaderboard claim.

---

## Entry 9 — 2026-08-16 · agentic reference measurement under LLM-MINER-INTERFACE-V1.1 / EVM 0/12, Solidity 1/12, LLM-TASK-ELIGIBLE unchanged at 7,954

**Directive:** operator msg 3919 — *"그 뒤 gemma4:26b agentic 측정 24건을 승인합니다. EVM 12 /
Solidity 12 별도 집계 · 각 task 정확히 1회 · 동결된 8턴·compile 8·check 4·24,576토큰·1,800초
유지 · 사람·인터넷·정답 구성기·문제별 힌트 금지 · 인프라 오류는 모델 실패와 분리하고 재시도 금지 ·
결과 후 prompt·예산·도구 변경 금지 · LLM-TASK-ELIGIBLE=7,954는 결과와 무관하게 유지 · 이번 실행은
REFERENCE-LLM-SOLVED만 채움 · 보고 후 STOP, 새 family 자동 착수 금지."*

### Result in one line

The frozen agentic interface was run exactly once over each of the 24 representatives.
**EVM `0 / 12`. Solidity `1 / 12`.** Zero infrastructure errors, zero retries. Both families
remain **`AGENTIC-REFERENCE-UNSOLVED`**, because calibration requires at least one ACCEPT in
each of a family's three patterns and Solidity's single ACCEPT sits in one pattern.
`LLM-TASK-ELIGIBLE` stays **7,954**, untouched in either direction.

### A correction to Entry 8, before the results

Entry 8's leakage table is arithmetically wrong for Solidity. It recorded *"1 ACCEPT + 7
non-probe reasons + **24 probe indices** = 32"*, giving 5.000 bits per call and ≤ 20.00 bits
over four calls. **Both families carry 12 probes per instance, not 24.** The frozen surface
was always correct; only Entry 8's description of it was not. The corrected accounting:

| family | distinct outputs per call | bits per call | ≤ 4 calls | Entry 8 said |
| --- | --- | --- | --- | --- |
| `solidity-source-synth-v1` | 1 + 7 + 12 = **20** | **4.322** | **≤ 17.29 bits** | ~~32 / 5.000 / ≤ 20.00~~ |
| `evm-bytecode-synth-v1` | 1 + 6 + 12 = **19** | **4.248** | **≤ 16.99 bits** | 19 / 4.248 / ≤ 16.99 — correct |

Entry 8's companion sentence *"the obligation is 24 (or 12) expected 256-bit words, i.e.
6,144 (or 3,072) bits"* is corrected the same way: the obligation is **12 words = 3,072
bits in both families**.

Both errors ran in the conservative direction — they overstated the feedback *and*
overstated the obligation — so the answer-freeness conclusion is unchanged and slightly
stronger: ≤ 17.29 bits of total feedback against a 3,072-bit obligation still cannot
determine even one word. Entry 8 is not edited; the correction lives here.

**This changes nothing executable.** No prompt, budget, tool, bound or enum was touched —
the run used the same twelve-probe tasks it was frozen with, and the operator's
"결과 후 prompt·예산·도구 변경 금지" is not implicated by fixing a sentence about them. The
post-run audit below confirms every `first_probe_index` the model actually received was in
`0..11`.

### `evm-bytecode-synth-v1` — 0 / 12

| quantity | value |
| --- | --- |
| measured / instances | **12 / 12** |
| NOT-MEASURED (infrastructure) | **0** |
| ACCEPT | **0 / 12** — `p1-minimal` 0/4, `p2-populated` 0/4, `p3-contract-adjacent` 0/4 |
| final reasons | `MALFORMED-SUBMISSION` 6, `CODE-SIZE-EXCEEDED` 6 |
| episode end | `SUBMIT` **12 / 12** — every episode was ended by the model, not by a budget |
| turns · check calls | 51 · 39 (no `compile` tool in this family) |
| tokens | 152,956 generated, 388,008 prompt |
| wall clock | 2,253.0 s total |
| family status | **`AGENTIC-REFERENCE-UNSOLVED`** |

**Not one submission reached the behavioural comparison.** Of 39 in-episode `check` calls,
25 returned `MALFORMED-SUBMISSION`, 12 returned `CODE-SIZE-EXCEEDED` and only **2** ever got
as far as `PROBE-MISMATCH`. Eleven of the twelve final answers exceed the family's
**192-byte** bound, most of them at roughly 2,044 bytes — about **10×** over — and the one
submission that fit the bound (104 bytes) was malformed. The recurring 4,088–4,090 character
length is the per-turn generation cap rendered as hex: the model emitted a maximum-length
byte string rather than a short program.

### `solidity-source-synth-v1` — 1 / 12

| quantity | value |
| --- | --- |
| measured / instances | **12 / 12** |
| NOT-MEASURED (infrastructure) | **0** |
| ACCEPT | **1 / 12** — `p1-minimal` 1/4, `p2-library-interface` 0/4, `p3-inheritance-modifier` 0/4 |
| final reasons | `PROBE-MISMATCH` 10, `COMPILE-FAILED` 1, ACCEPT 1 |
| episode end | `TURN-BUDGET` 11, `SUBMIT` 1 (the accepted one) |
| turns · compile calls · check calls | 92 · 60 · 27 |
| tokens | 52,114 generated, 508,652 prompt |
| wall clock | 788.2 s total |
| family status | **`AGENTIC-REFERENCE-UNSOLVED`** |

The accepted instance is `calibration/out-of-corpus/p1-minimal/2`: one failing compile, one
successful compile, one `check` that returned ACCEPT, then `SUBMIT` — 4 turns, 30.1 s.

### What the agentic loop changed, and what it did not

Two halves of the interface behaved very differently, and the distinction matters more than
the headline count.

**The `compile` half worked, measurably.** In **12 / 12** Solidity episodes the model's
*first* compile attempt failed. In **12 / 12** it subsequently reached a program that
compiles, and **11 / 12** ended holding one. Compiler errors fed back into an edit loop did
what they were supposed to do — that is a real capability difference from the single-shot
profile, where a non-compiling answer was simply the end of the attempt.

**The `check` half converted nothing.** Across both families there were **66** in-episode
`check` calls and exactly **one** ACCEPT — and that one came on the model's *first* check of
that instance, before any adaptive feedback existed. **Zero episodes turned an in-episode
REJECT into a final ACCEPT.** The `LIMITED-ADAPTIVE-FEEDBACK` channel that Entry 8 was
written to bound correctly produced, in this run, **no observed gain at all**. Ten Solidity
episodes received `PROBE-MISMATCH` with a first-divergence index two or three times each and
still finished wrong.

So the single-shot → agentic delta of `0/24 → 1/24` is not evidence that the feedback oracle
helps. On this evidence it is the compiler, not the checker, that the interface added.

### Controls

| control | EVM | Solidity |
| --- | --- | --- |
| adversarial submissions rejected (empty + constant, per instance) | **24 / 24** | **24 / 24** |
| post-run leak audit | **PASS** | **PASS** |
| harness→model messages scanned | 51 | 92 |
| expected words searched × 5 renderings each | 144 | 288 |
| recorded tool results scanned | 39 | 87 |
| `first_probe_index` values, all in range `0..11` | 2 | 25 |

`test_surface.py` proved the projection on constructed submissions *before* the run. The
post-run audit re-proves it on the 24 episodes that actually happened: every message the
harness sent the model was searched for every probe's expected value in decimal, bare hex,
`0x` hex and 32-byte padded hex, and no match exists. Every recorded tool result stayed
inside `ACCEPT | REJECT(reason_enum, first_probe_index)`. The audit script is read-only,
written after the run, and is not part of the frozen surface; both family freezes were
re-verified afterwards and still bind (`GATE-ONLY: 12/12` each).

### Neither budget was the binding constraint

No episode ended on `WALL-CLOCK` or `TOKEN-BUDGET`. The largest single episode generated
20,480 of the 24,576 permitted tokens. EVM episodes ended because the model chose `SUBMIT`
(12/12); Solidity episodes ended on the 8-turn cap (11/12). A larger token or time budget
would not have changed these numbers — which is worth recording precisely because the
budgets are now frozen and cannot be revised.

### Sealed digests (Entry 9)

Root: `local-docs/llm-mineable-census-p1-2026-08-16/interface-v11/`

| artifact | role | sha256 |
| --- | --- | --- |
| `AGENTIC-RESULT-evm-bytecode-synth-v1.json` | 12 EVM rows, per-pattern tally, controls | `cc05b3cd4fa83fe2c64a4065eb1fcea82446d9055e730791fec491d479a70448` |
| `AGENTIC-RESULT-solidity-source-synth-v1.json` | 12 Solidity rows, per-pattern tally, controls | `e8e9e3ce9bbaaee0003a7702a5871cc9872694ce64d94f72f591a9fe4ca92002` |
| `audit_transcripts.py` | post-run leak audit, read-only, not part of the frozen surface | `6c6ac088cca351b067146aeced6bc48e8a501a43754555378f7998a5ac8197ad` |

Both result files record the freeze digests they ran under —
`784118609d0b7083…` (EVM), `bda74097e40f8dc0…` (Solidity), `76469541aa347818…` (run freeze) —
unchanged from Entry 8. 24 full transcripts are retained in the git-ignored sandbox.

### What did not change

| quantity | before Entry 9 | after Entry 9 |
| --- | --- | --- |
| `LLM-TASK-ELIGIBLE` | 7,954 anchors | **7,954 — unchanged, as the directive requires** |
| `LLM-MINEABLE-ELIGIBLE` | `NOT-YET-DETERMINED` | **`NOT-YET-DETERMINED` — unchanged** |
| `REFERENCE-LLM-SOLVED` | NOT-MEASURED | **1 / 24 under the agentic profile — EVM 0/12, Solidity 1/12, never merged into the above** |
| Entries 4 and 6 | 0/24, `SINGLE-SHOT-REFERENCE-UNSOLVED` | **unchanged, values untouched** |
| Entries 7 and 8 | sealed | **sealed and unedited; Entry 8's arithmetic corrected here, not rewritten** |
| budgets, prompt, tools, bounds | frozen in Entry 8 | **unchanged, and now unchangeable — results exist** |
| corpus anchors consumed | 0 | **0 — the 24 representatives are out-of-corpus** |
| all ten buckets, conservation | 91,328 rows, PASS | unchanged, not re-run |

### Boundary / non-claims (Entry 9)

* **1 / 24 is a reference measurement, not a capability claim.** One local model, one
  epoch of challenges, one episode per instance, no retries. It does not bound what this
  model could do at another temperature, another epoch, or with a repaired EVM interface.
* **`AGENTIC-REFERENCE-UNSOLVED` is not a deletion.** No row was removed, no bucket changed,
  no anchor consumed. Structural eligibility and reference solvability stay separate labels.
* **EVM and Solidity are never pooled.** The `0/12` and the `1/12` are separate results of
  separate families under a deliberately asymmetric interface — EVM has no `compile` tool,
  and its failure profile (formatting and size, not behaviour) is exactly what that
  asymmetry predicts. Repairing it would be a new interface version and a new measurement,
  not a re-run of this one.
* **The feedback oracle showed no measurable benefit here.** Reporting the `0/24 → 1/24`
  delta as evidence that adaptive checking helps would misread this run.
* **No re-run.** These numbers stand as measured. The gate was frozen before results
  existed and is not revisited now that they do.
* **Closed local, offline, non-consensus.** `mineable_now = 0`. No paid API, no public
  benchmark, no public mining, no leaderboard claim.

---

## Entry 10 — 2026-08-16 · Entry 9 wording correction + LLM-MINER-INTERFACE-V1.2 frozen / no episode run, all sealed numbers unchanged

**Directive: operator msg 3923, with the answer format resolved by operator msg 3926.**

### Result in one line

`LLM-MINER-INTERFACE-V1.2` is frozen — a deterministic EVM assembler and an assembly answer
format, with the checker, the 192-byte bound and every budget carried over unchanged — and
**no episode has been run under it at the time of sealing**. Every sealed number in Entries
1–9 stands exactly as sealed. `LLM-TASK-ELIGIBLE` remains **7,954**.

### A correction to Entry 9's wording, required by the operator

Entry 9 wrote that *"a larger token or time budget would not have changed these numbers."*
That overreaches, and the operator corrected it:

> 한 가지 문구만 고치는 게 좋습니다. "예산을 늘려도 결과가 달라지지 않았을 것"보다는 "이번
> 실행에서는 예산 상한이 병목이었다는 증거가 없다"가 정확합니다.

The accurate statement, which replaces it going forward: **in that run there was no evidence
that the budget ceiling was the binding constraint.** All twenty-four episodes ended by
SUBMIT or by turn budget; none hit the token ceiling, and the longest used 309 of its 1,800
seconds. That is an absence of evidence for a budget bottleneck, not evidence of its
absence. Nobody measured what a larger budget would have produced, because nobody ran it.

Entry 9 is **not edited**. Its numbers, its tables and its text stay as sealed; this entry
is the correction, in the append-only form the ledger requires. The correction is to a
sentence of interpretation, not to a measurement — no row, count, verdict or digest changes.

### How Entry 9 is to be read, per the operator

The operator's reading is adopted as the premise of this version, in their words:

* **EVM: 인터페이스 병목이 먼저 발견됨.** 0/12 must not be read as a failure of reasoning.
  All twelve stopped at the format or size step, so what was actually measured was *the
  ability to write raw bytecode without an assembler*.
* **Solidity: 의미적 풀이 능력은 확인됐지만 대표 패턴 전체를 충족하지 못함.** All twelve
  reached a compiling program and one passed.
* **7,954 개: 구조적으로 유효한 문제 수 그대로.**
* **LLM 채굴 가능 수: 아직 미확정.**
* **1/24를 전체 코퍼스에 비례 확대하면 안 됨.** Twelve or twenty-four instances do not
  scale to 91,328 rows, and no entry will multiply them out.

### What v1.2 changes, and what it deliberately does not

The operator required a successor, not a repair: *"기존 v1을 고치지 말고 후속 버전으로 분리해야
합니다."* `interface-v11/` is untouched and its digests still hash as sealed in Entry 8.
v1.2 lives in `interface-v12/` with its own freeze.

| | Entry 9 (v1.1) | Entry 10 (v1.2) |
| --- | --- | --- |
| answer language | hexadecimal runtime bytecode | **EVM assembly, assembled by the harness** |
| tool alongside `check` | `compile` (Solidity only; EVM had none) | **`assemble`, 8 calls — the same slot, the same count** |
| checker | shipped, unmodified | **the same file, unmodified** |
| `MAX_CODE_BYTES` | 192 | **192** |
| turns / check calls / tokens / seconds | 8 / 4 / 24,576 / 1,800 | **8 / 4 / 24,576 / 1,800** |
| model, weights digest, decoding | frozen | **copied verbatim from the v1.1 run freeze** |
| challenges | epoch 1 | **epoch 2, fresh; epochs 0 and 1 asserted different** |
| Solidity | measured 1/12 | **not re-run — "Solidity는 재실행하지 않음"** |

`seal.py` hard-stops if any budget differs from v1.1's, so *"checker와 나머지 예산 유지"* is
enforced by the code and not merely promised by this text. One variable changes: the
assembler.

### The assembler is a translator, and it is gated as one

`assembler.py` maps mnemonics to bytes, encodes `PUSHn` operands, and resolves labels to
byte offsets. It does not choose push widths, does not optimise or reorder, does not know
the family, the formula, the probes, the slot or the challenge, and reports nothing about
correctness. The prompt publishes mnemonic **names only, never opcode numbers**, and that
list is generated from the assembler's own table so prompt and tool cannot drift apart.

Two assertions carry the weight:

* **A9** fails if the assembler's source so much as mentions `expected_value`,
  `reference_answer`, `witness`, `slot`, `probe`, `challenge`, `anchor` or the family name
  outside its docstring.
* **A10** writes a straightforward correct program in assembly, assembles it with this tool,
  and hands the result to the **unchanged** checker. It assembles to **106 bytes against the
  192-byte bound** and the checker returns **ACCEPT**. The program is then discarded (C9):
  never shipped, never logged, never shown to the model.

A10 is what makes the next measurement interpretable. Entry 9's 0/12 had an interface
explanation available; after A10, a 0/12 could not be blamed on the interface again, because
the interface has been demonstrated to reach ACCEPT inside the published bound.

### The answer is assembly (operator msg 3926)

Two readings were possible and the operator chose: *"A로 진행"* — **the model's answer itself
is EVM assembly, which the harness assembles into the runtime bytecode that the unchanged
checker judges.** The family manifest, the official helper surface and the instance block
are carried into the episode prompt **byte for byte** from the frozen single-shot prompt.
Only the submission contract and the output format are replaced, because the answer is no
longer hexadecimal and the old format would contradict the new one. The problem, the
constraints, the formula and the instance data are untouched.

### Feedback accounting, updated for the one new reason code

v1.2 adds exactly one reason, `ASSEMBLE-FAILED`, raised by the surface when the answer does
not assemble. It travels bare — the assembler's error text never rides along with a verdict,
because diagnostics are what `assemble` is for, under `assemble`'s own budget.

| | distinct outputs | bits per call | ≤ 4 calls |
| --- | --- | --- | --- |
| v1.1 EVM (Entry 8) | 1 ACCEPT + 6 non-probe reasons + 12 probe indices = 19 | 4.248 | ≤ 16.99 bits |
| **v1.2 EVM** | 1 ACCEPT + 7 non-probe reasons + 12 probe indices = **20** | **4.322** | **≤ 17.29 bits** |

The obligation is unchanged: 12 expected 256-bit words, **3,072 bits**, against ≤ 17.29 bits
of total feedback. Seventeen bits cannot determine even one 256-bit word.
`LIMITED-ADAPTIVE-FEEDBACK` still names the property correctly.

`MALFORMED-SUBMISSION` is retained in the enum although v1.2 makes it unreachable — the
harness only ever hands the checker `0x`-prefixed assembler output. Keeping it declared
costs 0.074 bits per call and removes a crash path; dropping it would buy a tighter bound
with a hard stop the model could trigger. Stated here rather than done silently.

### The freeze

* **Twelve instances**, the same out-of-corpus representatives as Entries 4 and 9, on
  **fresh epoch-2 challenges**. Epoch 0 (Entry 4) and epoch 1 (Entry 9) are recorded per row
  and asserted different; a collision is a hard stop. **Zero corpus anchors consumed.**
* **Existence through the pipeline, then discarded.** For all twelve, a valid answer was
  confirmed to exist *as assembly, assembled by this tool, inside the 192-byte bound,
  accepted by the unchanged checker* — 106 of 192 bytes in every case — and then dropped
  (C9). v1.1 asked whether a valid bytecode existed; v1.2 asks whether one exists *that this
  assembler can produce*, which is the question a 0/12 must be read against.
* **The prompt is checked for leaks at freeze time.** For every probe, the expected word is
  searched for in the episode prompt in decimal, bare hex and 32-byte padded hex. Any match
  is a hard stop. None matched.
* **Gates, both run before the freeze:** `test_assembler.py` 21/21 PASS (A10 printing
  `assembled 106 bytes, bound 192, checker verdict ACCEPT`) and `test_surface.py` 29/29
  PASS, including that an unassemblable answer yields the bare reason `ASSEMBLE-FAILED` with
  no diagnostics attached, and that the extractor never invents an answer.
* **`run_agentic.py --gate-only`:** freeze binding gate **15/15 MATCH**, 12/12 instances
  re-derived to the frozen challenge and prompt digest, **no model invoked**.
* **The post-run leak audit is frozen too.** Unlike v1.1's, `audit_transcripts.py` is written
  and hashed *before* the run, so it cannot be tailored to the results. It adds a third
  assertion v1.1 did not have: every episode's first message must hash to the freeze's
  prompt digest.

### One leniency that is mine, not the operator's

The answer extractor takes, in order: the last `BEGIN ANSWER` / `END ANSWER` block; an
unterminated `BEGIN ANSWER` to the end of the reply; otherwise the last fenced code block.
It is deterministic and can only ever recover text the model actually wrote — it never
invents, completes or repairs an answer. **This leniency is the harness author's choice, not
an operator instruction**, recorded here so the result is read with it in view. The
reasoning: v1.2 exists to measure whether the model can write a correct program once
hand-encoding is removed, so a marker typo should not decide an instance. It makes the
measurement slightly friendlier to the model than a strict reading would.

### Sealed digests (sha256)

| file | sha256 |
| --- | --- |
| `LLM-MINER-INTERFACE-V1.2.md` | `426d6165…5fd55464` |
| `interface-v12/assembler.py` | `a6efd8ff…343081f7` |
| `interface-v12/surface.py` | `3fbec465…98108619` |
| `interface-v12/protocol.py` | `30d8158b…041f8469` |
| `interface-v12/episode.py` | `c86b653f…74e6181f` |
| `interface-v12/instances.py` | `d8c911c6…38588ad1` |
| `interface-v12/controls.py` | `4c808db1…d1700a3b` |
| `interface-v12/run_agentic.py` | `befcbce7…0cb2ba9c` |
| `interface-v12/test_assembler.py` | `1b0f98c4…90eb9a82` |
| `interface-v12/test_surface.py` | `2ca301e5…070d568a` |
| `interface-v12/audit_transcripts.py` | `3ef27892…5522271c` |
| `interface-v12/freeze.py` | `4a305f8d…def47345` |
| `interface-v12/seal.py` | `57a02c66…000328b8` |
| `interface-v12/FREEZE-evm-bytecode-synth-v1.json` | `e0e194db…5bec837f` |
| `interface-v12/RUN-FREEZE-V12.json` | `534ed579…cd6bce3e` |

Full digests are recorded inside `FREEZE-evm-bytecode-synth-v1.json` and re-verified by the
runner before a single model call is made.

### What did not change

| quantity | before Entry 10 | after Entry 10 |
| --- | --- | --- |
| `LLM-TASK-ELIGIBLE` | 7,954 anchors | **7,954 — unchanged** |
| `LLM-MINEABLE-ELIGIBLE` | `NOT-YET-DETERMINED` | **`NOT-YET-DETERMINED` — unchanged** |
| `REFERENCE-LLM-SOLVED` | 1 / 24 agentic (EVM 0/12, Solidity 1/12) | **unchanged — no episode run in this entry** |
| Entries 4 and 6 | 0/24, `SINGLE-SHOT-REFERENCE-UNSOLVED` | **unchanged, values untouched** |
| Entries 7, 8, 9 | sealed | **sealed and unedited; Entry 9's budget sentence corrected here, not rewritten** |
| `interface-v11/` | sealed in Entry 8 | **not edited; every digest still hashes as sealed** |
| the shipped checker | unmodified | **unmodified — v1.2 adds a tool, it does not touch the judge** |
| `MAX_CODE_BYTES` | 192 | **192** |
| corpus anchors consumed | 0 | **0** |
| all ten buckets, conservation | 91,328 rows, PASS | unchanged, not re-run |

### Boundary / non-claims (Entry 10)

* **This entry seals a freeze, not a result.** No episode has been run under v1.2. Any
  number that appears later belongs to a later entry.
* **The freeze precedes the result, and will not be revisited after it.** Prompt, budget,
  tools and bounds are fixed here. If the next measurement is another 0/12, that is the
  measurement; the interface does not get adjusted afterwards to improve it.
* **v1.2 removes a known bottleneck; it does not predict an outcome.** A10 shows a correct
  program fits comfortably inside the bound through this tool. It says nothing about whether
  the model will write one.
* **No extrapolation.** `LLM-TASK-ELIGIBLE = 7,954` counts structurally valid problems and is
  unchanged by any reference measurement in either direction. The LLM-mineable count stays
  `NOT-YET-DETERMINED`. Entry 9's 1/24 is not multiplied out to the corpus, and neither will
  whatever comes next.
* **EVM and Solidity are never pooled**, and Solidity is not re-run.
* **Closed local, offline, non-consensus.** `mineable_now = 0`. No paid API, no public
  benchmark, no public mining, no leaderboard claim.

---

## Entry 11 — 2026-08-16 · agentic reference measurement under LLM-MINER-INTERFACE-V1.2 / EVM 3/12 at epoch 2, family still AGENTIC-REFERENCE-UNSOLVED

**Directive: operator msg 3928** — *"추가 테스트를 늘리지 말고 PR #139 CI 초록 후 12건을 딱 한 번
측정하면 됩니다."* One measurement, twelve instances, one episode each, no retries, run after
Entry 10 was merged to `main`.

Entries 1–10 stay exactly as sealed and are **not edited**. Two wording corrections the
operator required on Entry 10 are recorded here as corrections, in the same append-only way
Entry 8 corrected Entry 7 and Entry 10 corrected Entry 9. Both are about *how the experiment
is described*, not about what was executed: the frozen files, the freeze digests and the run
are all unaffected by them, which is exactly why editing the sealed text would have been the
wrong repair — it would have broken the hash freeze to fix a sentence.

### Correction A — what A10 is evidence of

Entry 10 introduced the pre-run existence gate as *"A10, the assertion that matters"* and
leaned on it as the thing that would make a repeat 0/12 interpretable. The operator's
correction, adopted verbatim as the reading of record:

> A10은 "유효한 답을 192바이트 안에서 표현할 수 있다"는 증거입니다. "모델에게 쉽다"는 증거는 아닙니다.

A10 assembled a correct program to 106 bytes against the 192-byte bound and the unchanged
checker returned ACCEPT. That establishes **expressibility**: a valid answer *can be written*
in this assembly language, assembled by this tool, inside this bound, and accepted by this
judge. It establishes nothing about difficulty for a model — the program was written by the
harness author with the answer in hand, then discarded (C9). Any sentence that slides from
"a valid answer fits" to "the task is easy" is not supported by A10 and is not made here.

### Correction B — the changed variable, stated accurately

Entry 10 says *"One variable changes: the assembler"* (and, in `LLM-MINER-INTERFACE-V1.2.md`
D1/D7, *"v1.2 changes exactly one thing"* and *"v1.1 and v1.2 differ in the interface
alone"*). That is imprecise, because the challenges are new as well. The operator's correction,
adopted verbatim as the phrasing of record:

> fresh challenge도 바뀌므로 엄밀히는 변수 하나만 바뀐 실험은 아닙니다. "의도적으로 바꾼 인터페이스
> 요소는 어셈블러뿐이며, 챌린지는 같은 규칙으로 새로 발급했다"가 정확합니다.

So, for the record and for every future citation of this comparison: **the only interface
element deliberately changed is the assembler; the challenges were newly issued under the same
rules** (epoch 2, same generator, same twelve out-of-corpus anchors, asserted different from
epochs 0 and 1). This is not a one-variable experiment. The v1.1 → v1.2 comparison below is
read with that in view, and the strong part of it — the reject reasons moving off the format
step entirely — is a comparison of *failure modes*, which fresh challenges do not manufacture.

### The measurement

`LLM-MINER-INTERFACE-V1.2`, `evm-bytecode-synth-v1`, epoch-2 challenges, one episode per
instance, no retry, verdict decided at the end by the unchanged checker on the last
extractable answer (C4). Model, weights digest, decoding parameters and every budget carried
verbatim from the v1.1 run freeze; `seal.py` hard-stops on any budget drift and did not fire.
The runner re-verified all 15 frozen digests (`15/15 MATCH`) before the first model call.

| # | anchor (pattern/n) | verdict | reason | turns | assemble | check | s | tok | ended_by |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `p1-minimal/0` | **REJECT** | PROBE-MISMATCH | 7 | 2 | 4 | 20.4 | 849 | SUBMIT |
| 2 | `p1-minimal/1` | **REJECT** | PROBE-MISMATCH | 8 | 5 | 3 | 200.1 | 13464 | TURN-BUDGET |
| 3 | `p1-minimal/2` | **REJECT** | PROBE-MISMATCH | 8 | 5 | 2 | 55.4 | 4006 | TURN-BUDGET |
| 4 | `p1-minimal/3` | **REJECT** | PROBE-MISMATCH | 8 | 5 | 3 | 21.5 | 1252 | TURN-BUDGET |
| 5 | `p2-populated/0` | **REJECT** | PROBE-MISMATCH | 8 | 4 | 3 | 110.3 | 7984 | TURN-BUDGET |
| 6 | `p2-populated/1` | **REJECT** | PROBE-MISMATCH | 8 | 4 | 3 | 54.1 | 3839 | TURN-BUDGET |
| 7 | `p2-populated/2` | **ACCEPT** | — | 4 | 2 | 1 | 14.4 | 891 | SUBMIT |
| 8 | `p2-populated/3` | **ACCEPT** | — | 8 | 3 | 3 | 32.9 | 2336 | SUBMIT |
| 9 | `p3-contract-adjacent/0` | **ACCEPT** | — | 8 | 5 | 2 | 81.7 | 5147 | TURN-BUDGET |
| 10 | `p3-contract-adjacent/1` | **REJECT** | PROBE-MISMATCH | 8 | 4 | 3 | 79.2 | 5657 | TURN-BUDGET |
| 11 | `p3-contract-adjacent/2` | **REJECT** | PROBE-MISMATCH | 8 | 4 | 2 | 21.3 | 1358 | TURN-BUDGET |
| 12 | `p3-contract-adjacent/3` | **REJECT** | ASSEMBLE-FAILED | 8 | 4 | 3 | 99.0 | 6396 | TURN-BUDGET |

| | value |
| --- | --- |
| instances / measured / not measured (infrastructure) | 12 / 12 / **0** |
| accepted | **3** |
| reject reasons | PROBE-MISMATCH 8, ASSEMBLE-FAILED 1 |
| per pattern | p1-minimal **0/4**, p2-populated **2/4**, p3-contract-adjacent **1/4** |
| `family_status` | **`AGENTIC-REFERENCE-UNSOLVED`** |
| totals | 91 turns, 47 assemble calls, 32 check calls, 790.3 s, 53,179 generated tokens, 508,269 prompt tokens |
| adversarial controls | **96 / 96 REJECT** |

The family stays `AGENTIC-REFERENCE-UNSOLVED`. The calibration bar set in Entry 3 requires at
least one ACCEPT in **every** representative pattern, and `p1-minimal` is 0/4. Three accepted
instances do not clear it, and the bar is not lowered now that a result exists.

### v1.1 → v1.2: what moved, and what that is worth

| | Entry 9 (v1.1, epoch 1) | Entry 11 (v1.2, epoch 2) |
| --- | --- | --- |
| EVM accepted | 0 / 12 | **3 / 12** |
| reject reasons | MALFORMED-SUBMISSION 6, CODE-SIZE-EXCEEDED 6 | PROBE-MISMATCH 8, **ASSEMBLE-FAILED 1** |
| instances that reached execution | **0 / 12** | **11 / 12** |
| turns / tool calls | 51 turns, 0 compile, 39 check | 91 turns, 47 assemble, 32 check |
| generated tokens | 152,956 | 53,179 |
| wall clock | 2,253.0 s | 790.3 s |
| infrastructure `NOT-MEASURED` | 0 | 0 |

The load-bearing row is the third. Under v1.1 not one of the twelve answers survived the
format and size step, so the checker never executed a single program: the measurement could
not see the reasoning step at all. Under v1.2, eleven of twelve reached execution and were
judged on what the program actually computed. The format wall went from 12/12 to 1/12.

That is a statement about the interface, and it is the one thing this pair of runs supports
well. The 0 → 3 change in accepted instances is weaker evidence, because the challenges are
new (Correction B) and twelve instances is a small sample.

### What the tool logs show, including the parts that cut against a clean story

* **The assembler functioned as a repair loop, not as a one-shot translator.** The first
  `assemble` call failed in **5 / 12** episodes. All **12 / 12** eventually reached a program
  that assembled, and **11 / 12** ended holding one. Removing hand-encoding did not make the
  model's first attempt syntactically correct; it made the mistake *recoverable inside the
  episode*, which hand-encoded hex never was.
* **`check` converted a REJECT into an ACCEPT exactly once.** In `p2-populated/3` the
  sequence was PROBE-MISMATCH → PROBE-MISMATCH → ACCEPT. Entry 9 recorded zero such
  conversions. One conversion in twelve episodes is a single observation, not a demonstrated
  capability, and `LIMITED-ADAPTIVE-FEEDBACK` still bounds the channel at ≤ 17.29 bits per
  instance against a 3,072-bit obligation.
* **One ACCEPT was not confirmed by the model's own `check` calls.** In
  `p3-contract-adjacent/0` both of the episode's `check` calls returned ASSEMBLE-FAILED, yet
  the final harness verdict is ACCEPT: the model's last extractable answer did assemble and
  did pass, and under C4 the harness — not the model's in-episode calls — decides. The
  episode therefore ended without the model knowing it had succeeded. Disclosed because it
  makes the ACCEPT look less deliberate than the bare 3/12 suggests.
* **`p3-contract-adjacent/3` regressed at the end.** It had two assembling programs judged
  PROBE-MISMATCH, then finished on one that no longer assembled, so its recorded reason is
  ASSEMBLE-FAILED. That is the single ASSEMBLE-FAILED in the result table; it is not an
  instance that never got off the ground.
* **Adversarial controls: 96 / 96 REJECT**, all eight control shapes on all twelve instances,
  including `hex_only` — the v1.1 answer shape, which under v1.2 does not assemble. No
  degenerate submission was accepted.

### Budget

Neither run hit a ceiling. In this run the longest episode used 200.1 s of the 1,800 s
allowance and no episode reached the 24,576-token ceiling; episodes ended by SUBMIT (3) or by
the 8-turn limit (9). Carrying forward the wording the operator required in Entry 10: this is
**no evidence that the budget ceiling was the binding constraint in this run** — an absence of
evidence for a budget bottleneck, not evidence of its absence. It is not claimed that a larger
budget would have produced the same numbers.

### Post-run leak audit

`audit_transcripts.py`, written and hashed **before** the run, re-run unchanged afterwards:

```
POST-RUN-LEAK-AUDIT-V1.2  evm-bytecode-synth-v1
episodes 12 | prompts matching the freeze 12/12
harness messages scanned 79 | expected words searched 144 x 5 renderings
tool results scanned 79 | first_probe_index values 15, all in range
PASS
```

Every episode ran on the frozen prompt, no expected value reached the model in any rendering
(assembler error text included, since it reaches the model through that same channel), and
every tool result stayed inside `ACCEPT | REJECT(reason_enum, first_probe_index)`.

### Sealed digests (sha256)

| artefact | sha256 |
| --- | --- |
| `interface-v12/AGENTIC-RESULT-evm-bytecode-synth-v1.json` | `982415d472b00e9606a5875f624b22ce4024c434d2832f55ddb97a2cad72f60d` |
| `interface-v12/FREEZE-evm-bytecode-synth-v1.json` | `e0e194db2f128ef3f55f5272217d7948982fa7c1932219f78819af1e5bec837f` |
| `interface-v12/RUN-FREEZE-V12.json` | `534ed579e6b67ed79f3b9e24f9c91abbf55654a7eb6635bf66b3fc9fcd6bce3e` |
| `interface-v12/run-v12.log` | `a022a8fa1dcb5f55d09511c9e3f8a533ae03bf0461925324f8c0269d6e3e1b7b` |
| 12 transcripts, concatenated in sorted order (238,198 bytes) | `ca4d52e60a86402fb058cf0a2987ddd50984aa5ca9ee3be2217e97e7a296c1de` |

The result file records the freeze digests it was produced under, so the run cannot be
re-attributed to a different freeze after the fact.

### What did not change

| quantity | before Entry 11 | after Entry 11 |
| --- | --- | --- |
| `CURRENTLY-GATED-LLM-TASK-ANCHORS` | 7,954 | **7,954 — unchanged** |
| `RAW-MATERIAL-LEDGER` | 91,328 rows | **91,328 rows — unchanged** |
| `LLM-MINEABLE-ELIGIBLE` | `NOT-YET-DETERMINED` | **`NOT-YET-DETERMINED` — unchanged** |
| `evm-bytecode-synth-v1` family status | `AGENTIC-REFERENCE-UNSOLVED` | **`AGENTIC-REFERENCE-UNSOLVED` — unchanged** |
| Entry 9 Solidity result | 1 / 12 agentic | **unchanged — not re-run, per operator msg 3923** |
| Entries 1–10 | sealed | **sealed and unedited; Entry 10's two phrasings corrected here, not rewritten** |
| `interface-v11/` | sealed in Entry 8 | **not edited; every digest still hashes as sealed** |
| the shipped checker | unmodified | **unmodified — v1.2 added a tool, it never touched the judge** |
| `MAX_CODE_BYTES` | 192 | **192** |
| corpus anchors consumed | 0 | **0** |
| all ten buckets, conservation | 91,328 rows, PASS | unchanged, not re-run |

### Boundary / non-claims (Entry 11)

* **This fills `REFERENCE-LLM-SOLVED` for twelve EVM instances at epoch 2, and nothing else.**
* **No extrapolation, in either direction.** 3/12 is not multiplied out to 91,328 rows, and it
  is not read as a property of EVM bytecode synthesis in general or of language models in
  general. It is twelve instances, one model, one decoding setting, one run.
* **The operator's 0/12 conditional did not fire.** Operator msg 3928 stated that another
  0/12 would let raw-hex writing be excluded as the cause, with the explicit caveat against
  widening that to all of EVM or all models. The result is 3/12, so that conditional does not
  apply; what the run does show is the failure mode moving off the format step (12/12 → 1/12).
  The caveat is kept in force regardless of which branch fired.
* **This is not a one-variable experiment.** The only interface element deliberately changed
  is the assembler; the challenges were newly issued under the same rules.
* **A10 shows expressibility, not ease.** A valid answer can be written in this assembly,
  inside 192 bytes, and accepted by the unchanged checker. Nothing about model difficulty
  follows from it.
* **The calibration bar is not lowered after seeing the result.** Every representative pattern
  needs at least one ACCEPT; `p1-minimal` is 0/4; the family stays unsolved.
* **EVM and Solidity are tallied separately and never pooled**, and Entry 9's v1.1 numbers are
  preserved as measured — v1.2 results are kept in their own directory and their own entry,
  never merged into v1.1's.
* **The interface is not revised now that a result exists.** Prompt, budget, tools and bounds
  were fixed in Entry 10 and stay fixed.
* **Closed local, offline, non-consensus.** `mineable_now = 0`. No paid API, no public
  benchmark, no public mining, no leaderboard claim.
