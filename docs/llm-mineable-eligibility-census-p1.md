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

---

## Entry 12 — MULTI-DOMAIN-LLM-FAMILY-V1: a corrected canonical identity, four calibrated families, and the first reference-LLM-calibrated issuable-template count

Sealed 2026-08-16. Append-only: Entries 1–11 and the sealed figure of 7,954
`CURRENTLY-GATED-LLM-TASK-ANCHORS` are unchanged by this entry.

### 12.1 What this wave did

One wave over the whole `RAW-MATERIAL-LEDGER` of **91,328 rows** (evm, solidity, rust,
zk-native, ethereum-consensus, lean). The object was not to measure proving time or memory
— no SP1 work, no cycle or memory measurement — but to decide, per domain, whether a real
LLM task family can be built from the material at all, and then to count how many distinct
templates such a family can actually issue.

The definition was frozen first, before any adapter was written and before any model call:

> `REFERENCE-LLM-CALIBRATED-ISSUABLE-TEMPLATES-V1` — unique task templates that satisfy a
> fresh semantic challenge, expose no answer or witness, verify deterministically in
> public, reject trivial / fixed / cross-used answers, pass a reference-LLM 12/12 gate, and
> fully verify on a reference instance.

It is used as a versioned `LLM-MINEABLE-ELIGIBLE-V1`. It is **not** an absolute total over
all LLMs or all seeds.

### 12.2 The normalization was defective, and was corrected before the census

The first normalization folded the 91,328 rows onto 77,783 templates. A pre-census gate,
run at the operator's instruction, found that the conservation identity balanced but the
row-level mapping did not survive inspection:

* **49 rows were merged across different task units** by a blind content key
  (`domain + input_digest + oracle_digest`). Eight of them were in ethereum-consensus, the
  only domain that produces the final number: `epoch_processing/inactivity_updates` and
  `epoch_processing/rewards_and_penalties` over the same pre-state both leave that state
  unchanged, so their input and oracle digests collide although they are two different
  handlers. The other 41 were in evm — for example `test_ranges_example.py` merged onto
  `test_labels_example.py`.
* **1,556 rows were dropped.** A fold whose target row was absent from the ledger was
  silently deleted rather than promoted, contradicting the canonicalizer's own documented
  intent.
* **Declared lineage was treated as evidence of identity.** A parent/child or supersedes
  declaration merged rows on its own.

That image is preserved, not deleted, as `SUPERSEDED-NORMALIZATION-DEFECT`
(77,783 templates + 13,545 folded rows), and the census was stopped before it ran.

**`CANONICAL-ISSUANCE-IDENTITY-V2`** replaces it. Identity is bound to intrinsic facts
only, in eight components: `domain`, `family`, `task_kind`, `handler`, `fork`, `revision`
(`source_commit`), `compile_setting` (the consensus preset — `mainnet` and `minimal` are
different settings), and `semantic_locator` (the exact address of the material, path plus
in-file fragment). Equal identity, and nothing else, makes one template.

Explicitly **not** identity: `input_digest`, `oracle_digest` (two handlers that both leave
a pre-state unchanged share both), `duplicate_lineage` (provenance only), `corpus` (a
registration, not a question), `unit` (a granularity label, not a question).

Three rules were pinned as tests before the corrected normalization was written, and all
six tests in `test_identity.py` pass:

1. equal input and oracle digests with a different handler or task kind ⇒ separate
   templates;
2. a lineage declaration alone never merges — only an equal identity may;
3. a fold naming an absent target raises immediately and never drops the row.

**Corrected conservation, re-verified row by row over all 91,328 rows:**

```
91,328 = 87,235 unique templates + 4,093 identical-identity rows
```

Every row resolves to exactly one template; no row appears twice; no row is unmapped.
Per domain: ethereum-consensus 7,111 · evm 20,036 · rust 29,609 · solidity 12,931 ·
zk-native 17,548.

**Lineage re-audit** — every declared fold re-judged under V2, merged only on equal
identity: `cross-corpus-superseded-by`/`supersedes` 1,670 pairs MERGED (same file, same
revision, two corpora); `task-unit-of-anchor` 2,423 MERGED, 81 SEPARATE;
`cross-corpus-parent-of` 3,492 SEPARATE (1,937 identity differs, 1,555 no target row);
`subrow-of-bundle` 5,726 SEPARATE; `intra-corpus-content-clone` 79 SEPARATE;
`intra-corpus-duplicate` 1 SEPARATE. Every separation was on `handler` and
`semantic_locator` — a different address inside the file. All 39 evm rows that V1 had
folded on lineage alone across a task unit are now separate templates.

### 12.3 What was frozen before any model call

Six domain adapters were designed before any result existed. Two were terminated up front
and are **not** counted as measured zeroes: **zk-native — `NO-DETERMINISTIC-FAMILY`**, and
**lean — `CORPUS-NOT-MATERIALIZED`** (no real corpus; probing `lean --version` pulled an
elan Lean 4.33.0 toolchain, which is recorded and was not used as a corpus).

For each of the four remaining families, twelve out-of-corpus representatives (3 patterns ×
4), the rendered prompts, the budget, the fixtures and the PASS criterion were hash-frozen
before any model call. Existence-then-discard ran first: a valid answer was constructed,
verified and discarded, keeping only its digest, its size and the ACCEPT. All four families
froze clean — existence 12/12, adversarial controls 60 rows and 0 ACCEPT each, cross-task
replay PASS, `UNIVERSAL-ANSWER` scan PASS.

The common attack gate rejects empty/no-op, always-return/revert, the public original, a
constant answer, a fixed patch, a universal template, a lookup table, a sibling-challenge
answer, wrong task/seed/source/policy, and cross-task or cross-seed reuse.

### 12.4 Reference-LLM calibration — not re-run for this entry

Local offline **gemma4:26b** only (`5571076f3d70`, 25.8B, Q4_K_M, ollama on loopback, no
network egress), temperature 0, fixed seed 42, exactly 1 attempt, 0 retries, 0 manual
fixes, hidden verification exactly once after commitment. Families were run **sequentially,
one at a time**.

| family | result | ACCEPT |
|---|---|---|
| `rust-op-synth-v1` | `REFERENCE-LLM-CALIBRATED` | 12/12 |
| `consensus-epoch-patch-v1` | `REFERENCE-LLM-CALIBRATED` | 12/12 |
| `evm-op-synth-v1` | `FAMILY-CALIBRATION-FAILED` | 3/12 |
| `solidity-op-synth-v1` | `FAMILY-CALIBRATION-FAILED` | 0/12 |

All four: 0 leak findings, 0 trivial accepts, 0 cross-reuse accepts, 0 infrastructure
errors. These results predate the normalization correction and were **not re-run**; the
correction changed the denominator, not the model evidence. No threshold, prompt, tool,
budget or problem set was changed after results, and no v2 of any family was made.

### 12.5 The census

Only a family that calibrated 12/12 is censused, with no further LLM calls, exactly one
reference seed per template, 0 retries and 0 manual exceptions. Every template lands in
exactly one bucket by first match.

The eligibility rule, precommitted before any verdict: **a template is ELIGIBLE only when
its own material is load-bearing in the challenge.** Seed space and epoch reissues are
never multiplied into the number.

```
ALREADY-COUNTED           7,954      NONDETERMINISTIC            0
FAMILY-UNSUPPORTED       72,170      RESOURCE-EXCEEDED           0
NO-FRESH-INSTANCE         3,042      ORACLE-OR-CHECK-FAILED     33
DUPLICATE                 1,816      ERROR                     180
                                     TRIVIAL-OR-UNIVERSAL        0
                                     ELIGIBLE                2,040
                                     ------------------------------
                                     TOTAL                  87,235
```

Conservation BALANCED overall and per domain. All 2,040 ELIGIBLE templates are
ethereum-consensus; the sealed 7,954 are evm and solidity, so the overlap is zero by
construction — ALREADY-COUNTED is the first bucket.

### 12.6 The number

```
RAW-MATERIAL-LEDGER                                 = 91,328 rows
CANONICAL-ISSUANCE-TEMPLATE (identity v2)           = 87,235 templates
REFERENCE-LLM-CALIBRATED-ISSUABLE-TEMPLATES-V1      =  2,040
LLM-MINEABLE-ELIGIBLE-V1                            =  2,040
new unique increment, no overlap with the sealed 7,954 =  2,040
mineable_now                                        =      0
```

### 12.7 Disclosures

* **`rust-op-synth-v1` calibrated 12/12 and still contributes 0 templates.** It synthesises
  its own function; a rustc UI test file is not read by the challenge, so no rust template's
  material is load-bearing. Under the precommitted claims table those 29,609 templates land
  in `FAMILY-UNSUPPORTED`; `NO-FRESH-INSTANCE` would describe them more precisely. The table
  was **not** edited after results — ELIGIBLE is 0 either way. This is the honest reading of
  a 12/12 family that cannot bind its domain's material.
* **180 ERROR** — all `decode pre` failures on cross-fork `transition/core` and `fork/fork`
  cases, where the stored pre-state is the previous fork's type. Recorded, not hidden.
* **33 ORACLE-OR-CHECK-FAILED** — all `COLLATERAL-DISTURBANCE`: the checker rejected because
  the reference answer perturbed state outside the audited set. The checker working, not a
  leak.
* **3,042 NO-FRESH-INSTANCE** — electra 1,451 and fulu 1,429 are outside the driver's
  supported fork set, and 162 cases have no `pre.ssz_snappy` on disk.
* **Locator-contained templates** — 5,726 solidity and 81 rust templates are sub-row
  locators whose bundle is also a template. V2 keeps them separate because merging them
  would need the lineage declaration V2 forbids as identity evidence. Neither domain
  contributes an ELIGIBLE template, so the final number is unaffected either way.
* **The precommitted census script hash is superseded.** `CENSUS-RULES-PRECOMMIT.json`
  pinned `455331866c1d0377…`; the corrected script is `1dd6e2274ff896e6…`. It changed only
  in what it reads (`templates-v2.jsonl`), how many templates it expects (87,235, not
  77,783), and that ALREADY-COUNTED is resolved through the V2 row-level mapping. Bucket
  order, bucket definitions, the eligibility rule, the dedup key, the per-template budget,
  seeds per template, retries, manual exceptions, LLM calls in census and the supported
  fork set are unchanged.

### 12.8 Not a claim

This is a reference-LLM-calibrated issuable-template count under one frozen local model,
one frozen prompt, one frozen budget and one frozen fixture set. It is **not** an absolute
total over all LLMs or all seeds, and it is not a statement about any model's general
ability. Model-solved representative counts are never mixed with structural census counts.
Closed local, offline, non-consensus. `mineable_now = 0`. No paid API, no other model, no
public benchmark, no public mining, no leaderboard claim.

---

## Entry 13 — 2026-08-17 · RUST-ANCHOR-COUPLED-FRESH-REPAIR-V1 reference calibration / FAMILY-CALIBRATION-FAILED, GEMMA-CALIBRATION = 2/12

Sealed 2026-08-17. Append-only: Entries 1–12 and every figure they sealed are unchanged by
this entry.

### 13.1 Result

```
FAMILY-CALIBRATION-FAILED
GEMMA-CALIBRATION            = 2/12
ACCEPT                       = 2
NO-FORMAL-SUBMISSION         = 10
INFRA-ERROR                  = 0        retries = 0
REFERENCE-INSTANCE-ELIGIBLE  = NOT-CENSUSED
```

The wave's own stop rule required 12/12 before the census could start. It returned 2/12, so
the census over the frozen 29,609-row Rust input was **not started**, and no reference-
instance count exists for this family.

`NO-FORMAL-SUBMISSION` counts episodes that reached the end of the sealed 8-turn budget
without ever emitting a submission in the required response format. It is a count of
episodes, not of wrong answers.

### 13.2 What this is not

* **`REFERENCE-INSTANCE-ELIGIBLE = NOT-CENSUSED` does not mean the Rust problem count is 0.**
  It means the count was never measured. Not measured is not zero, and this entry seals no
  Rust figure of any kind.
* **`LLM-MINEABLE-ELIGIBLE-V1 = 2,040` is unchanged.** No V2 sum is reported, because a V2
  sum required a census that did not run.
* `mineable_now = 0` is unchanged.

### 13.3 The sealed configuration that produced 2/12

Frozen before the model was called, verified identical after the run:

| seal | digest |
| --- | --- |
| policy | `5906169104aa1eb522f19ce439e146ffc48adb5f16557d1e6dbf06e60f1d99e3` |
| prompt | `7d6475fb8cfe0d113f4ccccd8d2a5fb9f8a52423f62eeb1a2b548f974b553de0` |
| calibration fixtures | `1775511652423c3790aefb0f611d921538f387e35dd2a5127c7fb9e78240d220` |
| input freeze listing (29,609 rows) | `58cc58d1c59dc01f609b096b67b5d0abe9f2f9f0f0456a5bc301d109c8989121` |

12 episodes, one attempt each, no retry and no manual repair. No answer text was stored:
every episode record carries `answer_stored: false` and keeps only a digest and a size.

### 13.4 Unused, non-authoritative artifacts

A census driver and an overlap checker exist in the git-ignored sandbox. They were written
before the calibration verdict and **were never executed** — no corpus row was resolved and
no overlap was computed, so they produced no number and carry no authority here. They are
recorded only so their existence cannot later be mistaken for a result, and they are
deliberately excluded from this record's authoritative outputs:

| artifact | sha256 | execution |
| --- | --- | --- |
| `census.py` | `81152c2305b8595fc0dd3b4aad1c97889f0988396718b97497e9a24da28cf3f6` | not executed |
| `overlap.py` | `6b35190110026af25d53c8fb580256d208cf1b9edf391894085f0ac715f712d3` | not executed |

A pure-Python self-check of that unused driver's bookkeeping
(`506034edd70f68ac9266389482ecf57bba1aa63258a9c4ec06b73e93a09b6ac7`) ran in the sandbox
against no corpus row, no compiler and no model; it is non-authoritative for the same
reason. The sandbox marker is `UNUSED-NONAUTHORITATIVE.json`.

### 13.5 Not a claim

A closed local, offline calibration under one frozen local model, one frozen prompt, one
frozen budget and one frozen fixture set. Not a paid API run, not a public benchmark, not
public-network mining, not a leaderboard claim, and not a statement about any model's
general ability. `mineable_now = 0`.

---

## Entry 14 — 2026-08-17 · OPUS5 reference calibration halted, OPUS48 wave pre-registered / MODEL-SUBSTITUTION-HARD-STOP = 1, ADJUDICATED-TASKS = 0

Sealed 2026-08-17, **before the first model call of the wave it registers**. Append-only:
Entries 1–13 and every figure they sealed are unchanged by this entry.

### 14.1 The halted wave

```
MODEL-SUBSTITUTION-HARD-STOP = 1
ADJUDICATED-TASKS            = 0
SCORED-FAMILIES              = 0        EVM, Solidity and Rust: none reached a verdict
RETRIES                      = 0        fixtures swapped = 0
LLM-MINEABLE-ELIGIBLE-V1     = 2,040    unchanged
mineable_now                 = 0        unchanged
```

The runtime, not the harness, replaced the sanctioned model in the middle of a session: a
`model_refusal_fallback` event (direction `retry`, scope `session`) switched the session
away from `claude-opus-5` while the second turn of the first episode was being answered.
Substitution of the sanctioned model is a hard stop under the directive that authorised the
wave, so the wave stopped at that turn. One episode was contacted —
`evm-bytecode-synth-v1 calibration/out-of-corpus/p1-minimal/0` — and none was adjudicated.

The model the runtime switched to was `claude-opus-4-8` — the same model the successor wave
registered in 14.4 targets, by instruction. Nothing produced by that substituted turn is
carried into the successor wave: its reply is preserved as evidence of the stop, is scored
nowhere, and the successor wave begins from fresh sessions with the frozen prompts only.

No family verdict, no per-family count and no diagnostic figure is sealed for that wave. Its
three families remain exactly as Entries 9, 11 and 13 left them.

### 14.2 The one ACCEPT seen before the stop is an observation, not a score

On the first turn of that single episode the frozen harness's own `check` tool returned
ACCEPT. **It is not a result, it is not authoritative, and it is excluded from every count
in this ledger.** It is written down only so that its presence in a preserved transcript can
never later be read as a scored task. Three separate reasons make it unscoreable: it came
from a turn inside an episode that never finished, the episode therefore produced no
end-of-episode verdict, and the wave that produced it was stopped as invalid.

### 14.3 What is preserved, unedited

The halted wave's session record, its output and its stop record are kept append-only in the
git-ignored sandbox (`local-docs/opus5-isolated-reference-calibration-2026-08-17/`, read-only
copy under `opus5-halted/`). Only digests are tracked here.

| preserved artifact | sha256 |
| --- | --- |
| `HARD-STOP.json` — the stop record | `1fcded111efec758eabd21699e72a4850aa36dec12fe3cac8e6a1c2a6c563774` |
| `STAGE-FREEZE.json` — the halted wave's sealed plan | `d586f141ba5e05345a868884eb6cc880279d45b22ae20368ddde432b88f1f059` |
| `STAGE-FREEZE-superseded-01.json` — its predecessor | `50d37f10b47d84bd558569c2d1496815f100321b19a6cd4a076f73d15af73bc8` |
| `HARD-STOP-contestant-session.jsonl` — the runtime's own session transcript | preserved, not digested here (contains the contestant's replies) |

**Disclosure — the drivers were edited in place, the earlier bytes are not retained.** The
transport and the two runners were modified for the successor wave (model id, per-turn
verification against the runtime transcript, refusal recording, output naming). Their
pre-edit digests remain recorded inside the halted wave's own sealed plan, and their current
digests are in 14.4, so the change is visible from both sides; the pre-edit file contents
themselves were not copied before the edit and no longer exist. The frozen harnesses,
fixtures, prompts and checkers were never edited by either wave.

### 14.4 Pre-registration of the successor wave

Registered here **before** any call of it, under the directive that authorised it
(`OPUS48-ISOLATED-REFERENCE-CALIBRATION`). Sealed plan:
`STAGE-FREEZE-OPUS48.json` = `aeea4f94c1c60dab7a21f89cb9338dcd296903c6458f81f0c5ba7659d578c296`.

**Model.** `claude-opus-4-8` is the **first and only** target of this wave — it is not a
fallback for another model, and no other model may answer in its place. The exact model id
is requested, not a shorthand. Every turn is checked against two independent records: the
per-turn model usage the runtime reports back, and the per-turn model field the runtime
writes into its own session transcript. A turn naming any other model is an immediate hard
stop, not a result. No `--fallback-model` is passed. A refusal produced by the sanctioned
model itself is a **model result**: it is recorded as `MODEL-REFUSAL` and is never retried.

**Scope.** Only this Claude Code environment's own model access is used. No external API
key, no separate paid API call, and no other model as a fallback.

**Tasks and budgets — unchanged, carried over from the halted wave's sealed plan.** The
seal's `cascade`, `frozen_inputs` and `budget` blocks were compared byte for byte against the
earlier seal before the new one was written, and are identical:

| carried over unchanged | value |
| --- | --- |
| stage A, EVM | `p1-minimal/0`, `p2-populated/0`, `p3-contract-adjacent/0` |
| stage A, Solidity | `p1-minimal/0`, `p2-library-interface/0`, `p3-inheritance-modifier/0` |
| stage A, Rust | `R01`, `R05`, `R09` |
| stage A rule | the alphabetically first fixture id of each frozen pattern, one per pattern |
| stop rule | 3/3 ACCEPT in stage A opens the remaining nine of that family; 2/3 or fewer stops that family immediately |
| budget per episode | 8 turns, 8 compile/assemble, 4 public test/check, 24,576 generated tokens, 1,800 s |
| attempts, retries, manual edits, human intervention | 1, 0, 0, 0 |
| max episodes | 36 |

The fixtures, the selection order, the prompts, the tool surface and the budgets are the ones
Gemma was measured under. No fixture may be swapped or added after a result is seen.

**Isolation.** One fresh contestant per task, in a fresh session that is never resumed across
tasks, in a per-task working directory holding the public task prompt and nothing else. The
contestant runs with every built-in tool disabled, no MCP server, no settings file and no
project instructions, so no read, search, shell or web path exists from it to the repository,
the witnesses, the expected values, earlier answers, result logs or this ledger. No Opus 5
transcript, no Gemma answer and no earlier result is placed in front of it.

**Unenforceable axes, recorded rather than invented.** `temperature = UNCONTROLLED` and
`seed = UNCONTROLLED` — this runtime exposes neither. A per-turn output cap is not settable;
the episode-wide 24,576-token budget is still enforced, by the frozen loop for the two
interface families and by the transport for the Rust family.

| driver, at seal time | sha256 |
| --- | --- |
| `opus.py` | `927e0617a0b5129845548a390467934c580b1fbf6283aa13b4d8bccf4c2ec4b8` |
| `test_opus.py` | `d8fe42aa30079f82206ccefbba992228d94c5f8c9eab38e9c967eabca12175b7` |
| `run_interface.py` | `cce597112504d57f2ad81ff5f9bb92076602849466831c41b8c23003778f1bf2` |
| `run_rust.py` | `9d23294bf7b2ac03be54643e18fc43c8abb97165fc032613195e080d13a86cec` |
| `freeze_stage.py` | `295d1efb6bc08eb276de3723a1426eb4314b2c68c6753db67af6bf46b95498bd` |

### 14.5 What the successor wave may report, and what it may not

Results are recorded per model and per family only, as `OPUS48-EVM-SOLVED`,
`OPUS48-SOLIDITY-SOLVED` and `OPUS48-RUST-SOLVED`. A family that fails stage A is reported as
`OPUS48-DIAGNOSTIC = x/3` and never as 0; a family that clears all twelve is
`OPUS48-FAMILY-CALIBRATION-PASS`; anything else is `OPUS48-SOLVED = x/12`, exactly.

These figures are **never** summed or averaged across the three families, **never** added to
the Gemma record of Entries 9, 11 and 13, **never** added to the halted Opus 5 record, and
**never** scaled to the full template count. They are a separate model layer over the same
frozen fixtures, not a revision of any sealed number.

### 14.6 What this entry does not do

* `LLM-MINEABLE-ELIGIBLE-V1 = 2,040` is unchanged, and no successor result may edit it.
* The Gemma results of Entries 9, 11 and 13 are unchanged.
* `mineable_now = 0` is unchanged.
* No full census, no new family, no prompt change and no V2 design follows automatically
  from the successor wave; each would need its own instruction.
* A representative result is never scaled to a whole template count.

### 14.7 Not a claim

A closed local, isolated calibration of frozen fixtures under one runtime's own model access.
Not a paid API benchmark run, not a public benchmark, not public-network mining, not a
leaderboard claim, and not a statement about any model's general ability. `mineable_now = 0`.

---

## Entry 15 — 2026-08-17 · OPUS48 isolated reference calibration executed / EVM 12/12, Solidity 12/12, Rust 12/12, three separate FAMILY-CALIBRATION-PASS results

Sealed 2026-08-17, after the wave pre-registered in Entry 14 ran to completion under the plan
that entry sealed **before** any model call. Append-only: Entries 1–14 and every figure they
sealed are unchanged by this entry.

### 15.1 Result, per model and per family only

```
model                        claude-opus-4-8   (first and only target; not a fallback)
OPUS48-EVM-SOLVED            12/12   OPUS48-FAMILY-CALIBRATION-PASS
OPUS48-SOLIDITY-SOLVED       12/12   OPUS48-FAMILY-CALIBRATION-PASS
OPUS48-RUST-SOLVED           12/12   OPUS48-FAMILY-CALIBRATION-PASS
ADJUDICATED-TASKS            36      episodes contacted 36, verdicts reached 36
MODEL-SUBSTITUTION-HARD-STOP 0       for this wave; the halted wave's 1 stands in Entry 14
LLM-MINEABLE-ELIGIBLE-V1     2,040   unchanged
mineable_now                 0       unchanged
```

Each family cleared stage A 3/3, which opened its remaining nine under the pre-registered
cascade; each then cleared all nine. Stage A opened stage B in all three families, so the
`OPUS48-DIAGNOSTIC = x/3` form is not used by this entry.

These three figures are **not** summed, averaged or combined. There is no "36/36" result in
this ledger: the three families are three separate measurements over three separate frozen
fixture sets, and a single number across them would not name anything real.

### 15.2 Per-task judgement, all 36 episodes

| judgement | count |
| --- | --- |
| MODEL-ANSWERED | 36 / 36 |
| FORMAL-SUBMISSION-EMITTED | 36 / 36 |
| CHECKER ACCEPT | 36 |
| CHECKER REJECT | 0 |
| MODEL-REFUSAL | 0 |
| INFRA-ERROR | 0 |
| FORBIDDEN-ACCESS | NONE |
| CONTAMINATION | NONE |
| retries, manual edits, human interventions | 0, 0, 0 |
| fixtures swapped or added after a result was seen | 0 |

Budget use stayed far inside the pre-registered envelope of 8 turns, 8 compile/assemble,
4 public test/check, 24,576 generated tokens and 1,800 s per episode:

| family | turns | compile/assemble | check | generated tokens | wall clock |
| --- | --- | --- | --- | --- | --- |
| `evm-bytecode-synth-v1` | 2–2 | 0 | 1 | 523–758 | 10.1–15.9 s |
| `solidity-source-synth-v1` | 2–4 | 0–1 | 1 | 2,674–5,529 | 40.3–72.4 s |
| `rust-anchor-coupled-fresh-repair-v1` | 1–1 | 0 | 1 (hidden verify) | 221–483 | 4.6–14.8 s |

No episode hit a budget ceiling. Every interface-family episode ended by submitting
(`ended_by = SUBMIT`), never by exhaustion.

The frozen adversarial controls were rejected in every family that carries them:
`empty`, `constant`, `echo_input`, `hex_only`, `prose`, `revert`, `stop`, `store_zero` on the
EVM family and `empty`, `constant` on the Solidity family — all REJECT, in every episode. The
checkers were therefore not accepting on shape alone in the runs that produced the ACCEPTs.

### 15.3 The model that answered, verified per turn

36 episodes ran in 36 distinct sessions; no session was resumed across tasks. Each turn was
checked against two independent records — the per-turn model usage the runtime reports back,
and the per-turn model field the runtime writes into its own session transcript. Both name
`claude-opus-4-8` for every answering turn of all 36 episodes.

`claude-haiku-4-5-20251001` appears once per session, 36 turns in total, ≤20 output tokens
each: it is the runtime's own first-turn housekeeping (session naming) and produced no part of
any submission. It is recorded here rather than hidden, and it is credited with nothing.

No `model_refusal_fallback` event occurred in any session — the failure that stopped the
previous wave did not recur. No `--fallback-model` was passed. No substitution was detected in
any episode, so no hard stop fired.

Totals across the wave: 74 model calls, 36 housekeeping turns included; `temperature` and
`seed` remain `UNCONTROLLED`, as pre-registered, because this runtime exposes neither.

### 15.4 Isolation, as pre-registered

Each contestant ran with every built-in tool disabled, no MCP server, no settings file and no
project instructions, in a per-task working directory holding the public task prompt and
nothing else. No tool call was available to it, so no read, search, shell or web path existed
from the contestant to the repository, the witnesses, the expected values, earlier answers,
result logs or this ledger. No Opus 5 transcript, no Gemma answer and no earlier result was
placed in front of it. `forbidden_access = NONE` and `contamination = NONE` in all 36 records.

### 15.5 Artifacts

Results stay in the git-ignored sandbox
(`local-docs/opus5-isolated-reference-calibration-2026-08-17/`). Only digests are tracked here.

| artifact | sha256 |
| --- | --- |
| `STAGE-FREEZE-OPUS48.json` — the plan, sealed before the first call | `aeea4f94c1c60dab7a21f89cb9338dcd296903c6458f81f0c5ba7659d578c296` |
| `OPUS48-evm-bytecode-synth-v1.json` — EVM stage A | `1fadbb5a2d9381bcd21fa9cfcf8b6f1c31a96afcf9132082e929477c5c0ad9f9` |
| `OPUS48-evm-stage-b.json` — EVM stage B | `a9e51e03c4c50a4e36dae875493d158e553d52b19767219213b01274f4b90733` |
| `OPUS48-solidity-stage-a.json` | `26977470eaf67a5f5a5793bc7261f8fc5c3c669c71b5a97d9e377281e1b1f49e` |
| `OPUS48-solidity-stage-b.json` | `99d9b39d61104ce51ed199a77991ff07617a63e388b532f37810e9586ea8e1bd` |
| `OPUS48-rust-stage-a.json` | `3ce826fefd1450cd1f3657cc5a5323fdd396d015d7fd807db39e56d498151490` |
| `OPUS48-rust-stage-b.json` | `408721a443043e5afc6557e717e7966998b2ee160aaaa4616041ea63d7682e17` |

The drivers are byte-identical to the digests Entry 14 sealed before the wave began
(`opus.py` `927e0617…`, `test_opus.py` `d8fe42aa…`, `run_interface.py` `cce59711…`,
`run_rust.py` `9d23294b…`, `freeze_stage.py` `295d1efb…`); none was edited during the wave.
Each result file also carries the frozen-input digest its runner verified before the first
call: `e0e194db…` for the EVM family, `bda74097…` for the Solidity family, and prompt
`7d6475fb…` with fixtures `1775511652…` for the Rust family.

### 15.6 What this entry does not do

* `LLM-MINEABLE-ELIGIBLE-V1 = 2,040` is unchanged. A reference-model result does not edit it.
* The Gemma results of Entries 9, 11 and 13 are unchanged, and nothing here is added to them,
  compared away, or used to revise them. `GEMMA-CALIBRATION` figures stand as sealed.
* The halted Opus 5 record of Entry 14 is unchanged; its single pre-stop ACCEPT remains
  unscored.
* `mineable_now = 0` is unchanged. Twelve accepted representatives per family are not blocks,
  not shares, and not issuable templates.
* No result here is scaled to a whole template count. Twelve representatives are twelve
  representatives.
* No full census, no new family, no prompt change and no V2 design follows automatically from
  this entry; each would need its own instruction.

### 15.7 Not a claim

A closed local, isolated calibration of frozen fixtures under one runtime's own model access.
Not a paid API benchmark run, not a public benchmark, not public-network mining, not a
leaderboard claim, and not a statement about any model's general ability. `mineable_now = 0`.

## Entry 16 — 2026-08-17 · All-domain frontier-LLM closure over the deduplicated 87,235 / LLM-MINEABLE-ELIGIBLE-V2 = 10,702, UNRESOLVED = 0

Sealed 2026-08-17. Append-only: Entries 1–15 and every figure they sealed are unchanged by this
entry, including `LLM-MINEABLE-ELIGIBLE-V1 = 2,040`, which this entry does not revise, replace or
deprecate. V2 is a **successor** label measured over a different question and a wider denominator.

### 16.1 Result

```
denominator                        87,235   deduplicated templates
FRONTIER-LLM-CALIBRATED-ISSUABLE   10,702
STRUCTURALLY-INELIGIBLE            72,706
CALIBRATION-FAILED                  3,827
UNRESOLVED                              0
LLM-MINEABLE-ELIGIBLE-V2           10,702
LLM-MINEABLE-ELIGIBLE-V1            2,040   unchanged
model episodes this wave                18   claude-opus-4-8
mineable_now                             0   unchanged
```

**`LLM-MINEABLE-ELIGIBLE-V2 = 10,702` is the number of templates issuable under a family that
passed frontier-LLM calibration at family level. It does not mean a model individually solved
10,702 templates.** Twelve accepted representatives per family remain twelve representatives.

Conservation holds exactly, with every template in exactly one bucket:

```
87,235 = 10,702 + 72,706 + 3,827 + 0
```

| domain | templates | ISSUABLE | INELIGIBLE | CALIBRATION-FAILED |
| --- | --- | --- | --- | --- |
| evm | 20,036 | 6,755 | 13,281 | 0 |
| rust | 29,609 | 708 | 28,901 | 0 |
| ethereum-consensus | 7,111 | 2,040 | 5,071 | 0 |
| solidity | 12,931 | 1,199 | 7,905 | 3,827 |
| zk-native | 17,548 | 0 | 17,548 | 0 |

Duplicate `template_id`s across the settled rows: 0. Per-domain sums agree with the whole. The
issuable figure is the **union over `template_id`**, not a sum of the prior 2,040 and 7,954: the
settlement writes exactly one bucket per template and the duplicate count proves no template was
counted twice. Lean stays outside this denominator, as a zero-row declaration
(`CORPUS-NOT-MATERIALIZED`), and contributes nothing to any figure here.

### 16.2 The four families that reach the count, and the two that do not

No model was re-run for a family whose frontier evidence already existed at the same fingerprint.
Three of the four reuse the Entry 15 OPUS48 results directly; the fourth was measured here.

| family | frontier calibration | source | templates counted |
| --- | --- | --- | --- |
| `evm-bytecode-synth-v1` | 12/12 | Entry 15, reused | 6,755 |
| `solidity-source-synth-v1` | 12/12 | Entry 15, reused | 1,199 |
| `rust-anchor-coupled-fresh-repair-v1` | 12/12 | Entry 15, reused | 708 |
| `consensus-epoch-patch-v1` | 12/12 | measured in this wave, 12 episodes | 2,040 |
| `solidity-diagnostic-mutation-v1` | 0/3, closed at stage A | measured in this wave | 0 |
| `solidity-smt-diagnostic-mutation-v1` | 2/3, closed at stage A | measured in this wave | 0 |

Reuse was not assumed. Each reused pair was re-read and checked mechanically for answering model,
family name, `accepted == episodes`, zero infrastructure errors, zero refusals, and equality of
`fixture_digest` and `prompt_digest` between its two stages — a family whose two stages describe
different cuts is not one 12/12 result. All three passed; verdict `ALL-REUSABLE`.

The sealed Gemma FAIL artifacts of Entries 9, 11 and 13 were read and are quoted, never edited and
never deleted. `GEMMA-CALIBRATION` figures stand exactly as sealed. Both numbers are reported for
every family that carries both; neither replaces the other. Where this entry counts, it counts on
the frontier result, because the question this wave asks is a frontier-LLM question.

The two new Solidity families failed and were closed where they failed. Neither prompt, fixture,
branch nor threshold was touched afterwards, and neither family was re-measured in this wave:

| family | branch | representative | verdict |
| --- | --- | --- | --- |
| `solidity-diagnostic-mutation-v1` | BRANCH-TYPE | `abiEncoder/abi_encodeCall_unitary_tuple_from_assignment.sol` | REJECT / DIAGNOSTIC-MISMATCH |
| | BRANCH-DECL | `inheritance/override/calldata_memory.sol` | REJECT / DIAGNOSTIC-MISMATCH |
| | BRANCH-MODIFIER | `parsing/lexer_numbers_with_underscores_decimal.sol` | REJECT / DIAGNOSTIC-MISMATCH |
| `solidity-smt-diagnostic-mutation-v1` | BRANCH-PREDICATE | `abi/abi_encode_call_simple_1.sol` | ACCEPT |
| | BRANCH-LITERAL | `functions/getters/external_getter_2.sol` | REJECT / DIAGNOSTIC-MISMATCH |
| | BRANCH-ARITH | `operators/shifts/shift_underflow_negative_rvalue.sol` | ACCEPT |

Their 2,391 + 1,436 = 3,827 templates are `CALIBRATION-FAILED`. They are not `STRUCTURALLY-INELIGIBLE`:
the structural gates were not what refused them, and the distinction is kept so a later wave can
tell "the family could not be calibrated" from "no answer-free instance exists here".

### 16.3 The consensus family, measured here

Twelve frozen out-of-corpus representatives, all ACCEPT, 144.9 s wall clock across the family.
Zero answer leaks, zero trivial accepts, zero cross-task reuse accepts, zero infrastructure errors,
zero retries, zero manual fixes — each a pre-registered ceiling of zero, none of them raised after
a result was seen. Every contestant ran with all tools disabled, no MCP server, no settings file
and no system prompt (`forbidden_access = NONE — no tool was available to the contestant`).

`claude-haiku-4-5-20251001` appears once per session at ≤20 output tokens: the runtime's own
session-naming turn. It fed no answer, no tool and no grading, and it is credited with nothing.
No `--fallback-model` was passed and no substitution was detected, so no hard stop fired.

`num_predict = 4096` is recorded with `num_predict_enforced = false`, and `temperature`/`seed`
remain uncontrolled — this contestant CLI exposes none of them, so no claim is made that they were
fixed.

### 16.4 The Rust census, and a correction to this wave's own interim summary

Rust was the one passing family with no census, so its census ran exactly once over all 29,609
templates: 3,178.7 s, 29,609 rows resolved, conservation over the eleven frozen buckets holding.

An interim summary of this wave named four Rust figures — 3,293 non-Rust sources, 10,276 anchors
that do not build, 767 with no crate-root API, and 1,178 candidates — which sum to 15,514 and
therefore left **14,095 templates unnamed**. The operator held the number for that reason before
any sealing. The hold was correct and is recorded here rather than quietly fixed.

The 14,095 were **already in the ledger**, with a per-row reason written at the time each row was
resolved. They are the `GENERATION-FAILED` bucket, and every one of those rows carries the reason
verbatim: `no pattern applies to this anchor's api`. The anchor builds and exposes a bindable
crate-root API, but none of the frozen generator's four structural patterns — `ENUM-FOLD`,
`FN-COMPOSE`, `STRUCT-PROJECT`, `TRAIT-IMPL` — can bind to that API, so no fresh instance can be
cut from it. The omission was in the summary, not in the ledger: the four figures quoted were all
phase-A input properties, and phase A only answers whether an anchor builds and exposes an API. The
split of `builds-with-api` into task-emitting and not is a phase-B fact, and phase B had not been
read when that summary was written. No row moved, and nothing was re-run to establish this.

The full accounting, cross-tabulated read-only between the frozen phase-A manifest and the census
rows already on disk:

| phase A found | count | census bucket |
| --- | --- | --- |
| not a Rust source | 3,293 | NO-ANCHOR-API 3,293 |
| anchor does not build | 10,276 | COMPILE-INCOMPATIBLE 10,265 · RESOURCE-EXCEEDED 11 |
| builds, no bindable crate-root API | 767 | NO-ANCHOR-API 767 |
| builds with a bindable API | 15,273 | GENERATION-FAILED 14,095 · ELIGIBLE 708 · COMPILE-INCOMPATIBLE 361 · ANCHOR-COUPLING-FAILED 107 · DUPLICATE 2 |
| | **29,609** | conservation holds |

`1,178` was never a verdict. It was an intermediate count printed by phase B before the gates ran:
the number of manifest rows from which the generator could emit a task at all. Under the operator's
naming it is recorded as `RUST-MATERIALIZATION-CANDIDATE = 1,178`, and the frozen gates then settled
it into 708 ELIGIBLE, 361 whose materialised instance failed to compile, 107 that failed anchor
coupling, and 2 duplicates. It is counted from the `builds-with-api` column alone: the global
`COMPILE-INCOMPATIBLE` total of 10,626 includes 10,265 anchors that never built and so never
reached the generator, and those are not candidates.

The Rust figure this entry counts is therefore **708**, not 1,178. The candidate check that was
running when the hold arrived was left to finish on its own — not stopped, not re-run — and its
rows were read exactly as written.

This is also the wave's clearest structural finding: **a family passing calibration does not make
its templates issuable.** `rust-anchor-coupled-fresh-repair-v1` is 12/12 at family level, and 708
of 29,609 templates survive its own census. Its twelve representatives are out-of-corpus fixtures
and cover 0 of the 29,609 manifest rows; that caveat is carried in the calibration authority rather
than dropped.

### 16.5 Freeze order, and the two ledgers

The gap plan mapped all 87,235 templates to exactly one of the nine pre-registered categories, and
was frozen **before** any model call in this wave. No category boundary, representative, prompt or
threshold moved after a result was seen. The settlement applies measured outcomes to that frozen
map; it cannot invent a bucket, and a template whose outcome is missing stays `UNRESOLVED` and is
reported as a shortfall rather than guessed. Shortfalls: none.

The two prior ledgers are used differently and the difference matters. `census-rows.jsonl` is the
only prior ledger keyed by `template_id`, and it is the join the frozen gap plan itself used, so it
settles the three reused families per template. `bucketed-ledger-v2.jsonl` is keyed by `anchor_id`
and **cannot** be joined per template; it is used only as an independent per-domain recount, and the
run stops if the two disagree. They agree: EVM 6,755, Solidity 1,199 on both.

The Rust census ran from a wave-local wrapper that substituted exactly two things — the calibration
gate and the output paths — while importing the policy, generator, checker, extractor and bucket
priority from the sealed sandbox byte-for-byte. After the run, the six sealed decision files were
re-digested and confirmed byte-identical, and the sealed directory was confirmed to have gained no
new file.

One wrapper-level change is recorded because it is visible in the run: a handful of anchors make
rustdoc emit JSON nested deeper than CPython's default recursion limit, which killed the worker
pool outright. The wrapper raises that limit and retries such a parse on a large stack. This is an
interpreter stack limit, not a property of the anchor and not a verdict about it — without the
headroom the row cannot be **read** at all. It cannot move a row between buckets; it only decides
whether the extractor sees the document the toolchain already produced.

### 16.6 Budget

18 model episodes in total: 12 for `consensus-epoch-patch-v1`, 3 for each failed Solidity family,
closed at stage A as pre-registered. The plan's ceiling was 120 episodes beyond which execution had
to stop and report, so no plan-only stop was required. Every episode ran in a fresh isolated
subagent with retries 0 and human edits 0. No model was run per template anywhere in the census.

### 16.7 Artifacts

Results stay in the git-ignored sandbox
(`local-docs/all-domain-frontier-llm-closure-v1-2026-08-17/`). Only digests are tracked here.

| artifact | sha256 |
| --- | --- |
| `INPUT-FREEZE.json` | `a4df1c78dcedbb1f016c313d5e233e897e8279a061c5ddd644d05d90af9cc310` |
| `GAP-PLAN-FREEZE.json` — sealed before the first model call | `895ea960f15e428b6e412ffa585f7b8a91e492a7fefe2f93ca8c1f96d7836381` |
| `GAP-PLAN-ROWS.jsonl` — all 87,235 pre-mapped | `6383b064e3506275d9f6195881734ad8664e9e6831697c096f38c5bcb6a3a4c9` |
| `POOLS.json` | `7b5eb488707e565aa113d1c26c767c0f8803f0cb7f53709e01e0b6a250dc05b0` |
| `FRONTIER-CALIBRATION-consensus-epoch-patch-v1.json` — 12/12 | `f6e2d3f0bdac15c8ad9636dc27e023b5f19f51c4c821e4cd7cde496ef123e362` |
| `FRONTIER-CALIBRATION-solidity-diagnostic-mutation-v1.json` — 0/3 | `4ac242a6d4a91361982009a83309e268daf66745b626756986b5ae898b173405` |
| `FRONTIER-CALIBRATION-solidity-smt-diagnostic-mutation-v1.json` — 2/3 | `20525945821fd5f152a8ee0e68b80d8cdae90c73af74c8031994c3a901701685` |
| `RUST-CALIBRATION-AUTHORITY.json` | `85eb7c9d305f36daa2a8d12428a7e4ed69a0c1b0270ce54143b27045ed3ab753` |
| `REUSE-AUTHORITY.json` — `ALL-REUSABLE` | `1b8d3b2f5e6d8bb843b8394e4a303a49936aab3a09f177b2ad0eef55218ed0d2` |
| `RUST-ANCHOR-API-MANIFEST.jsonl` — 29,609 rows | `ade1e73cdc10c0be844e2e332f4818a61ec5c7b45281e4b9b99f7770de0177e7` |
| `RUST-MANIFEST-FREEZE.json` | `93eaf525c2413cf040f1c5aab0e79430a962a966656e68e15976092e71054c6e` |
| `RUST-CENSUS-ROWS.jsonl` — 29,609 rows | `5cc910e68363bdc6dc95fb55d4c47cea6104366dde66a9116cea9134235ea935` |
| `RUST-CENSUS-RESULT.json` — ELIGIBLE 708 | `e599b26a103c1353826bd838565c5c49b76de7e84d1a3dd1e54df4184e37ab81` |
| `RUST-FULL-ACCOUNTING.json` — the 14,095 accounted for | `708bc87c7136887ee0910414d1cfe3c68ad1331beddac6e092e1fde0e4bd23ef` |
| `CLOSURE-ROWS.jsonl` — 87,235 settled rows | `855c21d685b9675b2d6cbdbfe2d41ca741d55e01d2caf77f059cb3c9dc2c2fe1` |
| `CLOSURE-RESULT.json` | `3689d8c3ee7f3f704d1e4273a4dfe38a8f86e686b70bb4b72bc6bcaf5f4fc218` |

Every row of `CLOSURE-ROWS.jsonl` carries `template_id`, `domain`, `family_version`,
`family_fingerprint`, `source_hash`, `bucket` and `reason`. No answer, witness or expected value is
stored in any artifact above.

### 16.8 What this entry does not do

* `LLM-MINEABLE-ELIGIBLE-V1 = 2,040` is unchanged. V2 does not revise it.
* The Gemma results of Entries 9, 11 and 13 and the halted Opus 5 record of Entry 14 are unchanged.
* `mineable_now = 0` is unchanged. 10,702 issuable templates are not blocks and not shares.
* No SP1 proof, cycle, gas or memory figure was measured, and none is claimed.
* No consensus, reward, Base or real mining path was touched or connected.
* No template was individually solved by a model in the census. The census makes no model call.
* The 3,827 `CALIBRATION-FAILED` templates are not declared unusable — only uncalibrated. A later
  wave may retry them, under its own pre-registered plan.

### 16.9 Not a claim

A closed local, offline, non-consensus structural issuance count under frozen policy, with
family-level frontier calibration on twelve representatives per family. Not a per-template solve
rate, not a paid API benchmark run, not a public benchmark, not public-network mining, and not a
leaderboard claim. `mineable_now = 0`.

## Entry 17 — 2026-08-19 · S-1 semanticTests census under the sealed Solidity family / V3-CANDIDATE = 1,583 of 1,670, model episodes = 0

Sealed 2026-08-19. Append-only: Entries 1–16 and every figure they sealed are unchanged by this
entry. This entry counts **candidates**, not eligibility: `LLM-MINEABLE-ELIGIBLE-V2 = 10,702` is
not revised, and no sealed row moves out of its bucket here.

### 17.1 Question and answer

The 1,670 Solidity semanticTests templates settled in Entry 16 as `STRUCTURALLY-INELIGIBLE`, under
gap-plan category `CORPUS-OR-TOOLCHAIN-UNAVAILABLE` (FRR-P0 class: rung VL1, bridge UNDECIDED,
blocker TOOLCHAIN). The recorded reason — deciding the in-file expectation needs isoltest driving
evmone through evmc — is a fact about the corpus's own expectation comments. But
`solidity-source-synth-v1` (Entry 5 family, frontier-calibrated 12/12 in Entry 15) never reads the
in-file expectation: it appends a challenge-derived `BooleAnswer` obligation to the anchor's frozen
source and needs only compilability under the pinned solc 0.8.36. So the blocker was tested, not
assumed: the family's anchor walk was widened from syntaxTests to semanticTests — the sealed
`materialize.py` procedure verbatim, wider walk root, nothing else changed — and the family's own
published checker decided every row against the family's confirm-and-discard witness.

```
denominator (this census)       1,670   semanticTests anchors — verified 1:1 with the sealed 1,670 stock rows
TASK-EXISTS (ACCEPT)            1,583   labelled V3-CANDIDATE
NO-TASK (REJECT)                   87   all COMPILE-FAILED
conservation                    1,583 + 87 = 1,670
model episodes this census          0
LLM-MINEABLE-ELIGIBLE-V2       10,702   unchanged
mineable_now                        0   unchanged
```

The pre-registered prior band was 1,300–1,600 and the census landed at 1,583. The band was a
prior, not a gate; it decided nothing.

### 17.2 Discipline

Same execution shape as the FRR-P0 survey: a pre-registration frozen **before any run** — walk
rule, identity rule, fail-closed check order B1–B8, control battery, report format, labels — with
the document's sha256 recorded in `W1-FREEZE.json` and re-verified unchanged at seal time.

Fail-closed bindings, checked in frozen order on every run: pinned soljson.js digest; checkout
commit `03fe7dd4` with a clean semanticTests subtree; walk count exactly 1,670; corpus aggregate
digest equal byte-for-byte to the sealed 2026-08-10 input freeze; `GAP-PLAN-ROWS.jsonl` and
`templates-v2.jsonl` digests, with the selector re-run and the template→anchor join verified to be
a bijection onto the walked set; executor binary digest; family-code digests hashed in the
directory actually imported, before import. Seven negative controls ran to 7/7 STOP before the
real run — tampered anchor content, dropped anchor, extra anchor, renamed anchor, tampered
selector file, tampered family code, injected conservation fault — each stopping at its predicted
check with zero output files. The census then ran twice; all three outputs are byte-identical
across runs.

Zero per-row human or model decisions (operator conditions E1/E2): rules were authored at table
level and frozen; every verdict came from the sealed family checker. No fitness-based selection:
all 1,670 rows are registered with their verdicts, rejects included.

### 17.3 The 87 rejects, cross-tabulated

All 87 rejects are `COMPILE-FAILED` — zero harness faults, zero size, probe or execution
failures, zero empty-runtime rows. The frozen report-only diagnostics (properties overlap; the 87
are not a disjoint sum of these rows):

| source property | files | ACCEPT | REJECT |
| --- | ---: | ---: | ---: |
| multi-source marker (`==== Source:` / `==== ExternalSource:`) | 56 | 0 | 56 |
| `// ====` settings block | 386 | 360 | 26 |
| abicoder-v1 / ABIEncoderV2 pragma | 33 | 31 | 2 |

The 56 multi-source rejects are a **file-format shave, not a language verdict**: the family has no
`==== Source: ====` splitter (exactly as in the sealed syntaxTests census), the marker is not
valid Solidity, and the rows record that honestly. The remaining 31 rejects are genuine
0.8.36-pin incompatibilities. Settings blocks are comments to solc: such anchors compile under the
family's frozen settings and their in-file settings are intentionally ignored — part of the frozen
family definition, not an oversight. The `// ----` expectation blocks are likewise comments and
were never read; that is the whole S-1 argument.

Caveats frozen before the run and carried, not fixed: `compile.mjs` pins no `evmVersion`
(inherited family defect; changing it mid-census would alter the family definition and break
comparability with the sealed census), and `nodeid` is walk-root-relative, so issuance-time
challenge derivation must bind the full `anchor_id` — deferred to promotion governance.

### 17.4 What V3-CANDIDATE means, and does not mean

* No sealed ledger moves. The 1,670 remain `STRUCTURALLY-INELIGIBLE` / blocker TOOLCHAIN in every
  sealed artifact (Entry 16, FRR-P0). The candidate label lives in the W1 artifacts only.
* Promotion to `LLM-MINEABLE-ELIGIBLE-V3` is a separate append-only governance decision (ADR 0020
  R3-adjacent routes are named but unratified — R2). One question is recorded for that step rather
  than resolved here: the family's 12/12 frontier calibration (Entry 15) was measured on
  out-of-corpus representatives during the syntaxTests-era wave; whether that family-level
  authority transfers to semanticTests anchors without a fresh calibration wave is exactly the
  decision promotion governance owns.
* 1,583 is not supply, not "solved", not issuable inventory, and not a revision of any V2 figure.

### 17.5 Artifacts

Results stay in the git-ignored sandbox (`local-docs/w1-solidity-semantictests-census-2026-08-19/`).
Only digests are tracked here.

| artifact | sha256 |
| --- | --- |
| `PREREGISTRATION.md` — frozen before any run, unchanged at seal | `c872c14219dfc46f040839b5da30149a75dff62164f0161c6fdba7b9ed9dbd29` |
| `W1-FREEZE.json` | `1f2beff35936c2ca13dbfc51252f5cc0cea0aec2624c10577d64e2e479de7877` |
| `w1_census.py` | `1fd1eafd08937f469d9145d039b3d2f10051f2ed9040f35f14e44c3b706b2b3e` |
| `w1_selftest.py` | `3256d813b21a02f05f8f174769aae2b24cb9d5c896758297135988574910a7b0` |
| `SELFTEST-RESULT.json` — 7/7 STOP | `875f166268428d5795983d0ced1385a26f3f15bd2d5efac534b95525203868fb` |
| `W1-CENSUS-ROWS.jsonl` — 1,670 rows | `c5f58bf91df043b7ee3d1d6b30e36a30db454a3f11c36aa70f80ce1941685a70` |
| `W1-CENSUS-REPORT.json` | `e086ba3b9c9131ecfe71624e5d5c8cb2103527a8cd4fa3029096050965587f89` |
| `W1-CENSUS-REPORT.md` | `97cfdf9dfa35a201712aec2a63bd752fdf40875d5c02fbdcdf00c2a96cf947eb` |
| `W1-SEAL.json` | `1724219984d00d44bec993aad7bd60f39f553d9219e4279b97d80a7d01dd8b26` |

Every row carries `anchor_id`, `nodeid`, `template_id`, `anchor_source_sha256`, `challenge`,
`verdict`, `reason`, `detail` and `label`. No answer, witness or expected value is stored in any
artifact above (the witness is confirm-and-discard, C9).

### 17.6 What this entry does not do

* `LLM-MINEABLE-ELIGIBLE-V1 = 2,040` and `LLM-MINEABLE-ELIGIBLE-V2 = 10,702` are unchanged.
* No promotion decision is taken; no calibration claim is made beyond quoting Entry 15's sealed
  result.
* No model call was made (0 episodes). No isoltest or evmone was built; no in-file expectation was
  decided or read.
* `mineable_now = 0` is unchanged. No consensus, reward, Base or real mining path was touched.
* The 87 rejects are not declared unusable — the 56 multi-source files are a file-format gap that a
  later splitter-bearing family could revisit under its own pre-registration.

### 17.7 Not a claim

A closed local, offline, non-consensus, model-free census under frozen pre-registration. Not a
solve rate, not a paid API benchmark run, not a public benchmark, not public-network mining, and
not a leaderboard claim. `mineable_now = 0`.

## Entry 18 — 2026-08-19 · W2 electra/fulu census under the sealed consensus family / V3-CANDIDATE = 1,581 of 2,880, model episodes = 0

Sealed 2026-08-19 (census: operator order Telegram msg 4070; this docs step: msg 4119).
Append-only: Entries 1–17 and every figure they sealed are unchanged by this entry. Like Entry 17,
this entry counts **candidates**, not eligibility: `LLM-MINEABLE-ELIGIBLE-V2 = 10,702` is not
revised, and no sealed row moves out of its bucket in any sealed artifact.

### 18.1 Question and answer

The 2,880 electra/fulu consensus templates settled in Entry 16 as `STRUCTURALLY-INELIGIBLE`, under
gap-plan category `CORPUS-OR-TOOLCHAIN-UNAVAILABLE`: the sealed driver executed only the forks it
was built for, and electra/fulu containers were not among them. That blocker is a toolchain fact,
not a task-shape fact, so it was tested rather than assumed. The driver was extended to
v1.6.1-faithful electra/fulu containers (`consensus-epoch-exec`, binary digest `7ab25420…`,
corpus pin consensus-specs v1.6.1 commit `5fa6edcc`), the extension was gated **before any census
verdict** — G-SSZ container round-trips 70/70 across mainnet/minimal × electra/fulu, G-POST
post-state checks 8/8, and 35/35 draft probes failing loudly rather than silently — and the sealed
family `consensus-epoch-patch-v1` (Entry 12 identity, frontier-calibrated in Entries 15–16) then
decided every row model-free: reference epoch 1, one seed per template, 120 s budget, zero
retries, zero manual exceptions.

```
denominator (this census)       2,880   all electra/fulu templates of the sealed GAP-PLAN
ELIGIBLE                        1,581   labelled V3-CANDIDATE (electra 781 · fulu 800)
DUPLICATE                       1,036   against sealed material and intra-W2 repeats
NO-FRESH-INSTANCE                 138
ERROR                              98
ORACLE-OR-CHECK-FAILED             27
conservation                    1,581 + 1,036 + 138 + 98 + 27 = 2,880   per-fork balanced
model episodes this census          0
LLM-MINEABLE-ELIGIBLE-V2       10,702   unchanged
mineable_now                        0   unchanged
```

The pre-registered prior band was 1,300–2,300 and the census landed at 1,581. The band was a
prior, not a gate; it decided nothing.

### 18.2 Discipline

Pre-registration frozen before the selftest and before both census runs, its sha256 recorded in
`W2-FREEZE.json` and re-verified unchanged at seal. Baseline selftest green, then all eight
pre-registered negative controls HARD-STOPPED at their predicted check with exit 2 and zero output
files leaked. The census then ran twice; the row files are byte-identical across runs
(`W2-CENSUS-ROWS.jsonl` = `081aadbe…` both times).

Dedup continuity: the W2 walk replayed the sealed census's seen-material map from disk and
reproduced it exactly before issuing any W2 verdict — 1,816 sealed DUPLICATEs, 2,253 first-wins
and 162 sealed-fork NO-FRESH rows re-derived — so no W2 row double-counts sealed material.
`ALREADY-COUNTED = 0` is measured, not assumed. Zero per-row human or model decisions: every
verdict came from the sealed family checker against the family's confirm-and-discard witness, and
no answer, witness or expected post-state is stored in any artifact below.

### 18.3 Artifacts

Results stay in the git-ignored sandbox (`local-docs/w2-consensus-electra-fulu-census-2026-08-19/`).
Only digests are tracked here.

| artifact | sha256 |
| --- | --- |
| `PREREGISTRATION.md` — frozen before any run, unchanged at seal | `c7d65e478b289fdd462c6a8a9c92feaeb3dbd9cbd31ea6a79682812456950699` |
| `W2-FREEZE.json` | `1a24105153a6e48b83e870130c1e96d171a99f11711f48cc3f93c763ede3c509` |
| `w2_census.py` | `34f8dc2e9cbd817a74d0cd795f2a9a323f2111ece7b5d4b059269f21240badd3` |
| `w2_selftest.py` | `8ce5fe46eee61f00859189cd75a5169298546c88c7ad9a32ee1f9614a18c0d5c` |
| `w2_driver_gates.py` | `e160f828572c8d81c2a8f283fd56bcfeacb483982b12620276aa1091e8ff8a25` |
| `fam_consensus_w2.py` — sealed family, W2 anchor walk | `abd0421d5e7dbb46ce2be162fa39ca8d3273b4d623d1d3a06050a7bfe1cad8b0` |
| `witness_consensus_w2.py` — confirm-and-discard author witness | `973ca76b22d51f92961907a46d52b67d9e9694acb2c64749958126b4ac964f51` |
| `consensus-epoch-exec` — driver binary, electra/fulu extension | `7ab25420be6a165c2234cdeda7a4f69d1784e9682c56decbaedbfb4060def9a1` |
| `DRIVER-DECISION.md` | `1baf13d42f73b40d7a86b1fc9c300cff92c1760a59e32d96ad200278f181e9a7` |
| `DRIVER-GATES.json` — G-SSZ 70/70, G-POST 8/8 | `bf368eb7a98da5a9635849b62c577cd1b5edccf77ddce2e5f936cc8965ffd90e` |
| `W2-SELFTEST.json` — 8/8 STOP | `2e4a08f4081b7c05fa1c74b2c489613429a2a946ec6906fb6d8a2e64cdb5cbeb` |
| `W2-CENSUS-ROWS.jsonl` — 2,880 rows | `081aadbec6ba13c6135f9911ab394a4c3d095a67f8433dc6e23d102238c7449f` |
| `W2-CENSUS-REPORT.json` | `0189ebd2c28ab679088fe322fe32d88345d33cbe473e418c5dbf932d3f652020` |
| `W2-CENSUS-REPORT.md` | `068317b18d89944b7c242bf8a04a9f050cf2517978b735900c77e7214ad8b95b` |
| `W2-SEAL.json` | `8880aadddaa148715b985de3705c7396bb8879b51db84972005814e8b573c790` |

### 18.4 What this entry does not do

* `LLM-MINEABLE-ELIGIBLE-V1 = 2,040` and `LLM-MINEABLE-ELIGIBLE-V2 = 10,702` are unchanged.
* No promotion decision is taken here. The consensus family's sealed 12/12 (Entries 15–16) was
  measured on sealed-fork representatives, not on electra/fulu; whether that authority covers the
  new forks is exactly the question the promotion wave of Entry 19 answers by fresh measurement.
* No model call was made (0 episodes). The 98 ERROR and 27 ORACLE-OR-CHECK-FAILED rows are
  registered with per-row reasons, not retried and not excluded from the denominator.
* W2b — cross-fork decode of the ~204 previous-fork pre-states — remains deferred and unrun.
* `mineable_now = 0` is unchanged. No consensus, reward, Base or real mining path was touched.

### 18.5 Not a claim

A closed local, offline, non-consensus, model-free census under frozen pre-registration. Not a
solve rate, not a paid API benchmark run, not a public benchmark, not public-network mining, and
not a leaderboard claim. `mineable_now = 0`.

## Entry 19 — 2026-08-19 · V3 promotion calibration wave, both pools 12/12 / LLM-MINEABLE-ELIGIBLE-V3 = 13,866

Sealed 2026-08-19 (wave: operator order Telegram msg 4114; this docs step: msg 4119). Append-only:
Entries 1–18 and every figure they sealed are unchanged, including `LLM-MINEABLE-ELIGIBLE-V1 =
2,040` and `LLM-MINEABLE-ELIGIBLE-V2 = 10,702`. V3 is a **successor** label: the V2 count plus two
candidate pools promoted as whole blocks, each after passing a fresh frontier calibration on
representatives drawn from the pool itself.

### 19.1 Question and result

Entry 17 recorded the promotion question rather than resolving it: the Solidity family's 12/12
(Entry 15) was measured on syntaxTests-era representatives, and whether that family-level
authority transfers to semanticTests anchors was deferred to promotion governance. The same
question applied to W2: the consensus family's 12/12 (Entry 16) was measured on sealed-fork
representatives, not electra/fulu. This wave answered both by fresh in-pool measurement instead of
a transfer argument — twelve representatives per pool, drawn deterministically from the pool
itself, one episode per representative on `claude-opus-4-8` under the sealed Entry 14–16
isolated-contestant protocol.

```
W1  solidity-source-synth-v1 over semanticTests   12/12 ACCEPT   FRONTIER-LLM-CALIBRATED
W2  consensus-epoch-patch-v1 over electra/fulu    12/12 ACCEPT   FRONTIER-LLM-CALIBRATED

LLM-MINEABLE-ELIGIBLE-V2               10,702   unchanged
W1 pool promoted (Entry 17)             1,583
W2 pool promoted (Entry 18)             1,581
LLM-MINEABLE-ELIGIBLE-V3               13,866   = 10,702 + 1,583 + 1,581
model episodes this wave                   24   claude-opus-4-8
mineable_now                                0   unchanged
```

**`LLM-MINEABLE-ELIGIBLE-V3 = 13,866` counts templates issuable under a family that passed
frontier-LLM calibration at family level over the pool in question. It does not mean a model
solved 13,866 templates — or 3,164, or any number beyond the 24 accepted representatives.** The
union arithmetic is exact: both pools lie wholly inside Entry 16's `STRUCTURALLY-INELIGIBLE`
bucket (the 1,670 semanticTests rows and the 2,880 electra/fulu rows), the W2 census measured
`ALREADY-COUNTED = 0` against sealed material, and Entry 17 verified the W1 walk 1:1 against the
sealed stock rows — so no template is counted twice.

### 19.2 Pre-registration, frozen before any model call

`PROMO-FREEZE.json` + `PREREGISTRATION.md` were written and digest-bound before the harness
selftest and before the first model call, adopting the operator-approved decisions verbatim: pass
threshold kept at 12/12 (D1), a fresh W1 calibration rather than a transfer argument (D2), one
single pre-registered 24-episode wave (D3).

* **Draw, deterministic and skip-aware**: score = sha256("V3-PROMOTION-CALIBRATION-2026-08-19 |
  pool | template_id"), ascending. W2 stratified electra 6 + fulu 6 with the episode order
  interleaved e,f,e,f,… so stage A covers both forks; W1 uniform first 12. The replacement rule
  was pre-registered and the full score walks frozen; zero skips were needed in either pool.
* **Fresh epochs, census never reused**: W1 census epoch 0 → calibration epoch 1; W2 census epoch
  1 → calibration epoch 2. Per representative, the gates re-derive the sealed census binding (W1:
  epoch-0 challenge and anchor-source digest; W2: epoch-1 witness commitment and test seed from
  the pinned pre-state material) and assert the calibration-epoch instance differs.
* **Instances materialised at freeze**: per representative, the prompt digest, an author-witness
  confirmation at the calibration epoch (ACCEPT, confirm-and-discard — no witness stored), and
  every adversarial control REJECT (W2: A1/A4/A5 static + A2/A3 dynamic; W1: empty + constant).
* **Budgets**, identical axis-by-axis across both harness surfaces and verified equal at freeze:
  8 turns, 8 build/compile calls, 4 checks, 24,576 generated tokens, 1,800 s wall clock.
* **Component table**: all 50 files imported, read or executed — including the wave's own six
  scripts — digest-pinned in the freeze and re-verified at every run gate and once more at seal
  (50/50 MATCH). Sealed continuity pins bind `opus.py` (`927e0617…`), both census row files
  (`c5f58bf9…`, `081aadbe…`), the W2 driver (`7ab25420…`), the Solidity family toolchain (pinned
  solc 0.8.36 + native exec) and `templates-v2.jsonl` (`44607581…`) to previously sealed digests.

One process fact is recorded rather than hidden: the first freeze attempt HARD-STOPPED before
sealing anything — the template→anchor join read `source_path` at the wrong nesting and could
seat no W2 representative. The loader was corrected to the sealed census's join and the freeze
re-run from scratch. The fail-closed design left no partial artifact, and nothing had been frozen
or measured before the fix.

### 19.3 Harness negative controls, run before any episode

Baseline first: both runners' full gate stacks green with no fault injected. Then each
pre-registered control had to stop exactly where predicted: NC1 tampered wave challenge module,
NC2 tampered `opus.py` transport, NC3 tampered W2 census rows, NC4 tampered W1 census rows — all
HARD-STOP exit 2 at the component gate; NC5 draw drift → walk-prefix gate; NC6 prompt drift →
prompt re-derivation gate; NC7 a doctored runtime transcript naming `claude-sonnet-5` must raise
the substitution hard stop while a clean `claude-opus-4-8` transcript passes; NC8 episode mode
under any fault refuses (exit 3) before any gate or model call and writes nothing. Faults perturb
only hashed byte-streams or the draw tag — never a file on disk — and the selftest verified zero
new files leaked. Result: 11/11 checks PASS (`PROMO-SELFTEST.json`).

### 19.4 Results in detail

| pool | stage A | full | leaks | trivial | cross-reuse | infra errors | adversarial controls | wall | cost |
| --- | --- | --- | ---: | ---: | ---: | ---: | --- | ---: | ---: |
| W2 consensus-epoch-patch-v1 | 3/3 | 12/12 | 0 | 0 | 0 | 0 | 5 per episode, all REJECT | 149.1 s | $0.59 |
| W1 solidity-source-synth-v1 | 3/3 | 12/12 | 0 | 0 | 0 | 0 | 2 per episode, all REJECT | 705.6 s | $2.34 |

One fresh isolated contestant per episode, no retries, no hints, no per-instance patches, no
fixture swaps. Every W2 episode ended by SUBMIT in 2 turns; W1 episodes took 2–4 turns with at
most 2 compile calls. The W2 COLLATERAL-DISTURBANCE possibility disclosed at freeze did not occur
— all 12 verdicts are straight ACCEPT. Scans: W2 ran the sealed `calibrate.py`
leak/trivial/cross-reuse scans verbatim; W1 ran the pool-adapted equivalents pre-registered at
freeze (expected probe **values** as the secret set, since probe inputs are published in this
family; trivial vs the constant control; cross-reuse against the 11 sibling instances — 0 hits).
Total cost $2.93 informational against the approved ~$5 ceiling, subscription-billed CLI, no API
key, no paid-API run.

### 19.5 Model evidence

Every answering turn was checked twice, independently: the CLI's own per-turn summary and the
runtime's session transcript. Across all 24 episodes the transcript model set is exactly
`[claude-opus-4-8]` (24/24), `substitution_detected` is null everywhere, no `--fallback-model` was
passed, and zero refusals occurred. `claude-haiku-4-5-20251001` appears once per session at ≤25
output tokens: the runtime's own session-naming turn, feeding no answer, no tool and no grading,
credited with nothing (Entry 16 precedent). Every contestant ran with all tools disabled, no MCP
server, no settings sources and an empty system prompt (`forbidden_access = NONE`). Disclosed
unchanged from the sealed precedent: temperature/seed remain uncontrolled by this contestant CLI,
and the per-turn `num_predict = 4096` is recorded as not enforced by the transport.

### 19.6 What promotion changes, and what it does not

* The V3-CANDIDATE labels of Entries 17–18 resolve: both pools are promoted **as whole blocks**
  into the V3 count. In the V3 view the 3,164 promoted templates are issuable; in every earlier
  sealed artifact (Entry 16 settlement, FRR-P0 rows, W1/W2 census artifacts) they keep the labels
  they were sealed with — the promotion lives in this entry, append-only, editing nothing.
* Entry 17's issuance caveat carries to V3 unchanged and unresolved: `nodeid` is
  walk-root-relative, so issuance-time challenge derivation must bind the full `anchor_id`.
  Issuance governance, not calibration, owns that step.
* V1 = 2,040 and V2 = 10,702 remain quotable exactly as sealed; V3 supersedes neither
  retroactively.
* `mineable_now = 0` is unchanged. No consensus, BF.7, reward, Base or real mining path was wired.

### 19.7 Artifacts

Results stay in the git-ignored sandbox (`local-docs/v3-promotion-calibration-2026-08-19/`).
Only digests are tracked here. The 24 per-episode transcripts (`transcripts/w2/w2-00…11.json`,
`transcripts/w1/w1-00…11.json`) are digest-bound row-by-row inside the two run results.

| artifact | sha256 |
| --- | --- |
| `PREREGISTRATION.md` — frozen before selftest and any model call | `f23d314f970c745e9b7c8284e9caff4020268374697e6ac3f185cfca32813899` |
| `PROMO-FREEZE.json` — 50-file component table, 24 reps, walks | `5ae6f8a56a6024ac4adb627ddf92ed45f6ed378553358623775007fcc9761605` |
| `promo_common.py` | `f57f473760301f234eaf4bef3a559b183c6c045b3dac5a09ce9dd8278125ee47` |
| `freeze_promotion.py` | `f31fe32528ffff6755bb5780c4fe8e32fcd50b695350f5a6835245a2e0b1ff6b` |
| `promo_selftest.py` | `4c69e4c024fc101168b8c5444a9aae3522c6d75e15e9853a48775ddc99003561` |
| `run_w2_promotion.py` | `58a9ac7c3d47cce7969c778c808520ceac127fbbe3e91e2e2446f2f1b34afde5` |
| `run_w1_promotion.py` | `d1332a2f8c4287c4ef60f41afd2bc6bdcd0350357ac139f1a9eae750ac64aa7c` |
| `seal_promotion.py` | `57be5be2a00c1660ba5ca18815c194b4b219a5823ec2628c139e379ad04c6117` |
| `PROMO-SELFTEST.json` — 11/11 PASS | `0fc50ff9d6313ba8faafeb0be9c01d53a986fccb4d2f83b8308683c6d7ccacba` |
| `PROMOTION-RUN-w2-consensus-epoch-patch-v1.json` — 12/12 | `c57eb321029daa725f7679ab0c263b87778be4028dc9dfef0277fb7af18d718b` |
| `PROMOTION-RUN-w1-solidity-source-synth-v1.json` — 12/12 | `14bf0fb2fc2782077f252856e8cc5ed03a4c56be987255d32da5d0114b38b571` |
| `PROMOTION-CALIBRATION-SEAL.json` | `cd1c6c4c2131837779a44c2efb48d66d10c78f507fc1ae8a46bd4c185800af29` |

No answer, witness or expected value is stored in any artifact above; per-episode rows carry
`answer_sha256` and byte counts, never answer bytes.

### 19.8 Not a claim

Family-level calibration on 12 representatives per pool, closed local, offline, non-consensus.
Not a per-template solve rate, not a paid API benchmark, not a public benchmark, not
public-network mining, and not a leaderboard claim. V3 is an issuable-count ceiling under the
frozen protocol, not a prediction. `mineable_now = 0`.

## Entry 20 — 2026-08-19 · W2b cross-fork decode census over the 98 W2 ERROR rows / W2B-CANDIDATE = 97 of 98, model episodes = 0

Sealed 2026-08-19 (census: operator order Telegram msg 4121; this docs step: msg 4124).
Append-only: Entries 1–19 and every figure they sealed are unchanged, including
`LLM-MINEABLE-ELIGIBLE-V3 = 13,866`. This entry counts **candidates** under a new label,
`W2B-CANDIDATE`; it promotes nothing and it re-buckets no sealed row.

### 20.1 Question and answer

Entry 18 deferred one measured question: the sealed W2 census left ~204 fork/transition rows
whose `pre.ssz_snappy` is the PREVIOUS fork's container (fork tests start from the pre-fork
state; transition tests from the pre-transition fork's state), and cross-fork decoding was
pre-registered as W2b, explicitly out of W2's scope. Measured against the sealed
`W2-CENSUS-ROWS.jsonl` (`081aadbe…`), the ~204 decompose exactly: 204 fork/transition rows =
106 `DUPLICATE` + 98 `ERROR`. The dedup key is the sha256 of the raw pre-state bytes and does
not depend on any decode fork, so the 106 DUPLICATEs are settled by W2 and out of scope. The 98
ERROR rows — every note starting `driver: decode pre:`, none outside fork/transition
directories — are the entire W2b scope.

Decode rule (the single delta from W2): each row is decoded at the previous fork of its
directory fork — electra-dir rows as **deneb** (an arm the W2 driver carries bit-for-bit from
the sealed census driver), fulu-dir rows as **electra** (the v161 arm gated in Entry 18,
G-SSZ 70/70, G-POST 8/8). Zero new Rust, family, witness or control code exists in this wave:
the walk calls the sealed W2 modules and pinned W2 driver binary (`7ab25420…`) verbatim, and
the only changed input is the anchor's `fork` field. The electra-form published rule
degenerates exactly on deneb instances because the driver reports per-validator
effective-balance limits for every arm (fork-wide 32 ETH on pre-electra arms). The rejected
alternative — upgrading each pre-state to the directory fork — would have required upgrade
functions neither driver calls and a fulu upgrade module the vendored crate does not have.
Parameters otherwise sealed-verbatim: reference epoch 1 (safe: W2 issued no instance for these
anchors — every one ERRORED before any witness commitment existed), one seed per row, 120 s
budget, zero retries, zero manual exceptions.

```
denominator (this census)          98   all ERROR rows of the sealed W2 census
ELIGIBLE                           97   labelled W2B-CANDIDATE
                                        electra-dir→deneb 46 of 47 · fulu-dir→electra 51 of 51
ORACLE-OR-CHECK-FAILED              1   COLLATERAL-DISTURBANCE (minimal/electra transition,
                                        one-fourth slashed active validators pre-fork)
conservation                       97 + 1 = 98   per-group balanced (20 + 20 + 27 + 31)
model episodes this census          0
LLM-MINEABLE-ELIGIBLE-V3       13,866   unchanged
mineable_now                        0   unchanged
```

The pre-registered prior band was 70–98 and the census landed at 97. The one
ORACLE-OR-CHECK-FAILED row is exactly the loss mode disclosed at freeze: a transition
pre-state carrying validators whose effective balance moves on any epoch run, which no patch
can prevent — registered honestly, not retried, not excluded from the denominator.

### 20.2 Discipline

Pre-registration frozen before the selftest and before both census runs, its sha256 recorded
in `W2B-FREEZE.json` and re-verified unchanged at seal. Smoke evidence (one sample per group
decoding at the previous fork and refusing at the directory fork) was taken before the freeze
and marked as evidence, not verdicts. Baseline selftest green, then all eleven pre-registered
negative controls HARD-STOPPED at their predicted check with exit 2 and zero output files
leaked. The census then ran twice; the row files are byte-identical across runs
(`W2B-CENSUS-ROWS.jsonl` = `1e26dfac…` both times).

Two dedup walks were replayed from disk before any W2b verdict and reproduced exactly: the
sealed census walk (1,816 DUPLICATE / 2,253 first-wins / 162 sealed-fork NO-FRESH) and the W2
walk continuing it (1,036 DUPLICATE / 1,706 first-wins / 138 NO-FRESH). Every scope digest is
owned by its own template_id in the continued seen-material map — the 98 were first-wins in W2
that ERRORED before materialising — so the W2b walk cannot double-count by construction, and
`DUPLICATE = NO-FRESH-INSTANCE = ALREADY-COUNTED = 0` is measured, not assumed. A per-row
refail gate (B9) re-drove every DIRECTORY fork first and demanded the sealed W2 decode failure
reproduce; 98/98 still refuse, so no W2b row contradicts a sealed W2 row. Zero per-row human or
model decisions; no answer, witness or expected post-state is stored in any artifact below.

### 20.3 Artifacts

Results stay in the git-ignored sandbox (`local-docs/w2b-crossfork-decode-census-2026-08-19/`).
Only digests are tracked here.

| artifact | sha256 |
| --- | --- |
| `PREREGISTRATION.md` — frozen before any run, unchanged at seal | `654203908e9661ef0259328a4f0baa4b6a7f5a46836ac5817b5689befab6c7b4` |
| `W2B-FREEZE.json` — pins incl. all 98 per-row pre-state digests | `2dc547f482e4dc0dec80f6c745321b9c1f3b284df860b6d4c02ef4992df18e5a` |
| `w2b_freeze.py` | `9c971f2ce06610c365b7a12c239109206e45446c5852faa16da3168d52a06dc5` |
| `w2b_census.py` | `b4cbd1f2003fbe64778de66ac28fd7846774c7cdfb7595039b84c31173281852` |
| `w2b_selftest.py` | `002b1267910fd4439b75e6ca722d81fb047484227c92066fe6bfebab7a6fd65e` |
| `w2b_seal.py` | `d5806b127491b6feb9f1b753a6f588420f6cacf4e64ea84840abdab06e0f9e0e` |
| `W2B-SELFTEST.json` — baseline green, 11/11 STOP | `966c111df3b63c05d0a9767a0573adbfde336eeec482ca1ae1654ea0c658e57a` |
| `W2B-CENSUS-ROWS.jsonl` — 98 rows (run 2 byte-identical) | `1e26dface2b01262bd28c3e7ec2ab70afe7a7164d68be8bf705fe5cbcc759754` |
| `W2B-CENSUS-REPORT.json` | `d7c5749d2728438537157e3e302f64b613475208fcf7c4d6e56cf0a49a416b1f` |
| `W2B-CENSUS-REPORT.md` | `987917575d97625a4b46adf0f1d23ac0863f087ae02e4a091dc9b2d27b1024f3` |
| `run1.log` | `91250408399433886f1efb1e40dbdce3c6a7fefff21bfd81a5c386b22d291e87` |
| `run2.log` | `e5f9aeb8e0ea108aed16c839ca12782d587be5f46333623bf31938118f2e4903` |
| `W2B-SEAL.json` | `7a09f210c6c81418452f4ae8d55cfed6758a7bd836f1f20587ed06d826587438` |

### 20.4 What this entry does not do

* `LLM-MINEABLE-ELIGIBLE-V1 = 2,040`, `V2 = 10,702` and `V3 = 13,866` are unchanged.
  W2B-CANDIDATE is not added to any of them.
* A template is never counted twice. These 98 template_ids sit in the sealed W2 census as
  ERROR and were therefore **not** part of the 1,581 promoted in Entry 19; any future
  promotion would re-decide those same templates under separate governance plus a
  reference-LLM calibration wave, never stack on a count that contains them.
* No promotion decision is taken here. Whether the family's calibrated authority covers
  deneb-decoded fork/transition instances is a fresh measurement question, exactly as
  electra/fulu coverage was in Entry 19.
* The sealed W2 ERROR rows keep their labels in every sealed artifact; the cross-fork
  decision lives in this entry and the W2b sandbox, append-only, editing nothing.
* No model call was made (0 episodes). `mineable_now = 0` is unchanged. No consensus,
  reward, Base or real mining path was touched.

### 20.5 Not a claim

A closed local, offline, non-consensus, model-free census under frozen pre-registration. Not a
solve rate, not a paid API benchmark run, not a public benchmark, not public-network mining,
and not a leaderboard claim. `mineable_now = 0`.

## Entry 21 — 2026-08-20 · W2b promotion calibration wave, 12/12 / LLM-MINEABLE-ELIGIBLE-V4 = 13,963

Sealed 2026-08-20 (wave: operator order Telegram msg 4129; this docs step: msg 4133). Append-only:
Entries 1–20 and every figure they sealed are unchanged, including `LLM-MINEABLE-ELIGIBLE-V1 =
2,040`, `V2 = 10,702` and `V3 = 13,866`. V4 is a **successor** label: the V3 count plus the one
candidate pool of Entry 20, promoted as a whole block after passing a fresh frontier calibration
on representatives drawn from the pool itself.

### 21.1 Question and result

Entry 20 counted candidates and took no promotion decision: whether the consensus family's
calibrated authority covers deneb-decoded fork/transition instances was recorded as a fresh
measurement question, exactly as electra/fulu coverage was before Entry 19 (the Entry 16
fingerprint rule: a 12/12 measured on a different cut never transfers). Entry 20.4 named the
path verbatim — "separate governance plus a reference-LLM calibration wave, never stack on a
count that contains them." This wave is that measurement: twelve representatives drawn
deterministically from the 97-row pool itself, stratified over its two decode arms, one episode
per representative on `claude-opus-4-8` under the sealed Entry 14–16 isolated-contestant
protocol.

```
W2b consensus-epoch-patch-v1 over cross-fork decode   12/12 ACCEPT   FRONTIER-LLM-CALIBRATED
     (electra-dir @deneb 6/6 · fulu-dir @electra 6/6)

LLM-MINEABLE-ELIGIBLE-V3               13,866   unchanged
W2b pool promoted (Entry 20)               97
LLM-MINEABLE-ELIGIBLE-V4               13,963   = 13,866 + 97
model episodes this wave                   12   claude-opus-4-8
mineable_now                                0   unchanged
```

**`LLM-MINEABLE-ELIGIBLE-V4 = 13,963` counts templates issuable under a family that passed
frontier-LLM calibration at family level over the pool in question. It does not mean a model
solved 13,963 templates — or 97, or any number beyond the 12 accepted representatives.** The
union arithmetic is exact: the 98 W2b template_ids sit in the sealed W2 census as ERROR and were
therefore not part of the 1,581 promoted in Entry 19 — Entry 20 measured this via the continued
seen-material dedup walk (`ALREADY-COUNTED = 0`) — so no template is counted twice. The one
ORACLE-OR-CHECK-FAILED row of Entry 20 stays out: 97 are promoted, not 98.

### 21.2 Pre-registration, frozen before any model call

`PROMO-FREEZE.json` + `PREREGISTRATION.md` were written and digest-bound before the harness
selftest and before the first model call, adopting the operator-approved decisions verbatim:
pass threshold kept at 12/12 (D1), a single pre-registered 12-episode wave under a $2 ceiling
(D2), successor-figure naming deferred to the docs step (D3 — resolved here as V4 by operator
order msg 4133).

* **Draw, deterministic and skip-aware**: score = sha256("W2B-PROMOTION-CALIBRATION-2026-08-20 |
  w2b-crossfork-decode | template_id"), ascending. Stratified electra-dir 6 + fulu-dir 6 with
  the episode order interleaved e,f,e,f,… so stage A covers both decode arms (deneb, electra,
  deneb). The replacement rule was pre-registered and the full score walk frozen; zero skips
  were needed.
* **Fresh epoch, census never reused**: W2b census epoch 1 → calibration epoch 2. Per
  representative, the gates re-derive the sealed epoch-1 census binding (witness commitment and
  test seed from the pinned pre-state material, which must equal the census-pinned digest) and
  assert the calibration-epoch instance differs.
* **Instances materialised at freeze**: per representative, the prompt digest, an author-witness
  confirmation at the calibration epoch (ACCEPT, confirm-and-discard — no witness stored), and
  every adversarial control REJECT (A1/A4/A5 static + A2/A3 dynamic).
* **Budgets**, sealed Entry 16 values verified against the wave surface at freeze: 8 turns, 8
  build calls, 4 checks, 24,576 generated tokens, 1,800 s wall clock.
* **Component table**: all 35 files imported, read or executed — including the wave's own six
  scripts — digest-pinned in the freeze and re-verified at the run gate and once more at seal
  (35/35 MATCH). Ten sealed continuity pins bind `opus.py` (`927e0617…`), the sealed wave loop
  and controls, the W2 family/witness modules and driver (`7ab25420…`), both census row files
  (`081aadbe…`, `1e26dfac…`) and the W2b census freeze and seal (`2dc547f4…`, `7a09f210…`) to
  previously sealed digests.
* **Zero new harness code**: the sealed multi-domain wave loop, the sealed W2 family and the
  sealed Entry 15 transport are imported, not copied; the only substitution anywhere is the
  episode chat hook. Pre-freeze smoke evidence (free, no writes, no model): the first candidate
  of each arm materialised end-to-end — the first prompt ever rendered at a deneb-decoded task,
  since the census never rendered prompts.

### 21.3 Harness negative controls, run before any episode

Baseline first: the runner's full gate stack green with no fault injected. Then each
pre-registered control had to stop exactly where predicted: NC1 tampered wave challenge module,
NC2 tampered `opus.py` transport, NC3 tampered W2b census rows, NC4 tampered W2 census rows —
all HARD-STOP exit 2 at the component gate; NC5 draw drift → walk-prefix gate; NC6 prompt drift
→ prompt re-derivation gate; NC7 a doctored runtime transcript naming `claude-sonnet-5` must
raise the substitution hard stop while a clean `claude-opus-4-8` transcript passes; NC8 episode
mode under any fault refuses (exit 3) before any gate or model call and writes nothing. Faults
perturb only hashed byte-streams or the draw tag — never a file on disk — and the selftest
verified zero new files leaked. Result: 10/10 checks PASS (`PROMO-SELFTEST.json`).

### 21.4 Results in detail

| pool | stage A | full | leaks | trivial | cross-reuse | infra errors | adversarial controls | wall | cost |
| --- | --- | --- | ---: | ---: | ---: | ---: | --- | ---: | ---: |
| W2b consensus-epoch-patch-v1 (cross-fork decode) | 3/3 | 12/12 | 0 | 0 | 0 | 0 | 5 per episode, all REJECT | 133.4 s | $0.54 |

One fresh isolated contestant per episode, no retries, no hints, no per-instance patches, no
fixture swaps. Every episode ended by SUBMIT within 2 turns (one in a single turn), used at most
1 check call and 0 build calls, and generated 447–732 tokens. Both decode arms scored 6/6. The
COLLATERAL-DISTURBANCE possibility disclosed at freeze did not occur — all 12 verdicts are
straight ACCEPT. The sealed `calibrate.py` leak/trivial/cross-reuse scans ran verbatim on every
episode: 0 findings. Cost $0.5409 informational against the approved $2 ceiling,
subscription-billed CLI, no API key, no paid-API run.

### 21.5 Model evidence

Every answering turn was checked twice, independently: the CLI's own per-turn summary and the
runtime's session transcript. Across all 12 episodes the runtime transcript was found and its
model set is exactly `[claude-opus-4-8]` (12/12), every answering turn is credited to
`claude-opus-4-8`, `substitution_detected` is null everywhere, no `--fallback-model` was passed,
and zero refusals occurred. `claude-haiku-4-5-20251001` appears once per session at 17 output
tokens: the runtime's own session-naming turn, feeding no answer, no tool and no grading,
credited with nothing (Entry 16 precedent). Every contestant ran with all tools disabled, no MCP
server, no settings sources and an empty system prompt (`forbidden_access = NONE`). Disclosed
unchanged from the sealed precedent: temperature/seed remain uncontrolled by this contestant
CLI, and the per-turn `num_predict = 4096` is recorded as not enforced by the transport.

### 21.6 What promotion changes, and what it does not

* The W2B-CANDIDATE label of Entry 20 resolves: the 97-row pool is promoted **as a whole block**
  into the V4 count. In the V4 view the 97 promoted templates are issuable; in every earlier
  sealed artifact (the W2 census ERROR rows, the W2b census artifacts) they keep the labels they
  were sealed with — the promotion lives in this entry, append-only, editing nothing.
* Entry 19's issuance caveat carries to V4 unchanged and unresolved: issuance-time challenge
  derivation must bind the full `anchor_id`. Issuance governance, not calibration, owns that
  step.
* V1 = 2,040, V2 = 10,702 and V3 = 13,866 remain quotable exactly as sealed; V4 supersedes none
  of them retroactively.
* `mineable_now = 0` is unchanged. No consensus, BF.7, reward, Base or real mining path was
  wired.

### 21.7 Artifacts

Results stay in the git-ignored sandbox (`local-docs/w2b-promotion-calibration-2026-08-20/`).
Only digests are tracked here. The 12 per-episode transcripts (`transcripts/w2b/w2b-00…11.json`)
are digest-bound row-by-row inside the run result.

| artifact | sha256 |
| --- | --- |
| `PREREGISTRATION.md` — frozen before selftest and any model call | `f64990171e2fbf2b853007a582df9d11cc1e5328a0d3bea573035b2e0ad547a7` |
| `PROMO-FREEZE.json` — 35-file component table, 12 reps, walk | `e60f36869e56dfaa76e84c09f64babacf5076e4f547a4ce5115a5ecdf3ce61eb` |
| `promo_common.py` | `0e7608900ec78778d2ca7c56bcc79aaf03441b088f023168f22d8334ab60ffc2` |
| `freeze_promotion.py` | `56d6d6db30b9cd7a71f16bd61dd21c735bb933647576c3a808e4a6a9f09fbdbf` |
| `promo_selftest.py` | `141777998a5535f0b1e1a00ea99caa0e8ac7e0a9397b6df7d66f504a572916f3` |
| `run_w2b_promotion.py` | `56ff5b5c79a75297f5abb78ceda5fe2d86f65c6290b7cffdecbb5c51eefb6fe0` |
| `seal_promotion.py` | `02bc610fa18dd841ddf15111cac7b573b384db4497a4e7912fd5e1a8dfe17c4b` |
| `design_smoke.py` — pre-freeze smoke, writes nothing | `b55aec2aef183591ebc5bc534060a3f0a0e707637084e29a454bd2d5bae01747` |
| `PROMO-SELFTEST.json` — 10/10 PASS | `b02ed8095bda546dfc74d9b3bb4143c3d56e98b5d4748a837218a3aa4a1186d5` |
| `PROMOTION-RUN-w2b-consensus-epoch-patch-v1.json` — 12/12 | `99f08987b6e00740cf73c875b74810999dc9b45e6991618e44f863cb1b66e52d` |
| `PROMOTION-CALIBRATION-SEAL.json` | `176aeb916b50a50076ed8cdff5fab23d6ab2ddba0b5baed929738b3881386e0b` |

No answer, witness or expected value is stored in any artifact above; per-episode rows carry
`answer_sha256` and byte counts, never answer bytes.

### 21.8 Not a claim

Family-level calibration on 12 representatives, closed local, offline, non-consensus. Not a
per-template solve rate, not a paid API benchmark, not a public benchmark, not public-network
mining, and not a leaderboard claim. V4 is an issuable-count ceiling under the frozen protocol,
not a prediction. `mineable_now = 0`.

## Entry 22 — 2026-08-20 · ANCHOR-COUPLING-AUDIT-P0 material-projection decomposition of V4 / MATERIAL-PROJECTION-UNIQUE = 2,398, MATERIAL-PROJECTION-DUPLICATE = 2,028

Sealed 2026-08-20 (operator order Telegram msg 4143, chat_id 1311067056, opened
ANCHOR-COUPLING-AUDIT-P0; corrected msg 4150, 4157, 4159; this seal ordered by msg 4161, same
chat). Append-only: Entries 1-21 and every figure they sealed are unchanged, including
`LLM-MINEABLE-ELIGIBLE-V4 = 13,963` (Entry 21). This entry adds no row, drops no row, and does
not change V4 — it decomposes the already-sealed V4 total by asking, for the A-rooted strata
that make up part of V4, how many rows are built from genuinely distinct real anchor material
versus how many repeat material another row already used.

### 22.1 Question and result

ANCHOR-COUPLING-AUDIT-P0 classified each of V4's four task families as **A** (a row's required
answer/verdict is genuinely coupled to its anchor's real content — swapping the anchor changes
the required answer) or **H** (structurally/cosmetically determined — swapping the anchor does
not). Result, unchanged since the original pass: Ethereum-consensus and Rust are **A**; EVM and
Solidity are **H**. This entry does not re-run that classification (22.5); it answers a
follow-up question raised across four correction rounds (msg 4150, 4157, 4159, 4161): of the
4,426 A-rooted rows (Consensus 3,718 + Rust 708), how many are drawn from distinct real anchor
material, and how many repeat material another row already drew from?

```
A-ROOTED-RAW (Consensus 3,718 + Rust 708, both A, unchanged)      = 4,426
H-SYNTHETIC (EVM + Solidity, both H, unchanged)                   = 9,537

MATERIAL-PROJECTION-UNIQUE    (Consensus 1,690 + Rust 708)        = 2,398
MATERIAL-PROJECTION-DUPLICATE (Consensus 2,028 + Rust 0)          = 2,028
  check: 2,398 + 2,028 = 4,426 = A-ROOTED-RAW                              [OK]

U (void/infrastructure)                                            = 0

V4 = MATERIAL-PROJECTION-UNIQUE + H-SYNTHETIC + MATERIAL-PROJECTION-DUPLICATE + U
   = 2,398 + 9,537 + 2,028 + 0
   = 13,963   [OK, = Entry 21's V4, unchanged]
```

### 22.2 Methodology

MATERIAL-PROJECTION-UNIQUE/-DUPLICATE come from a TEMPLATE-CONTRACT-DIGEST: a per-row digest
built from each row's real anchor content only — no `task_seed`, no `EXT_SEED`, no target/
audit-index selection, nothing seed-derived. Consensus:
`sha256({generator_checker_revision, fork, preset, validator_count, constants, all_validators:
[[i, balance, effective_balance, limit-or-null], ...] for every real validator})`, reading the
full validator range per anchor (not the family's own `CANDIDATE_WINDOW=64`). Rust:
`sha256({generator_checker_revision, pattern, api, anchor_source_fingerprint})`, where
`anchor_source_fingerprint` is a direct hash of the real `.rs` source bytes. Two rows fold to
MATERIAL-PROJECTION-DUPLICATE only on a byte-identical digest match — narrower than "the whole
real state file is identical," it means "every state value this family's own verifier reads is
identical." Names corrected from the working labels `A-ROOTED-UNIQUE`/
`EXACT-CONTRACT-DUPLICATE` (operator order, msg 4161) because the old names implied "the
anchor's full identity," which the digest does not capture — only the specific material this
family's verifier reads.

`generator_checker_revision` is a real content hash — `sha256(sha256(module source bytes) +
sha256(verifier binary bytes))` — not a hand-written label; two pools sharing the same code and
the same binary (`consensus_w2`, `consensus_w2b`, confirmed by direct import inspection:
`w2b_census.py` imports the identical `fam_consensus_w2` module) therefore get the identical
fingerprint, so genuine cross-pool duplicates fold correctly. This closes a bug an
operator-independent recalculation found (msg 4159): the first pass's hand-written per-pool
label text kept 15 genuine cross-pool duplicate groups artificially split into 30,
undercounting MATERIAL-PROJECTION-DUPLICATE by 15 and overcounting -UNIQUE by 15 (round-1
figures 1,705 distinct / 2,013 duplicate / 2,413 unique are superseded by this entry). A second
bug in the same pass — the Rust manifest join keyed by `anchor_sha256` alone, losing lineage
for 178 of 25,965 distinct hashes that carry more than one manifest line — is fixed by keying
on `(anchor_sha256, template_id)`; it affected 1 of 708 rows' manifest lineage but not the
final count for this data.

Largest duplicate group, corrected (msg 4161 — the first pass misreported this as 62): 324
groups of size > 1 exist in total (309 single-pool, 15 cross-pool). The largest **single-pool**
group is 150 rows (`consensus_w2`, digest prefix `b125d655…`); the largest **cross-pool** group
is 159 rows (`consensus_w2` + `consensus_w2b`, digest prefix `54ac4f7c…`). All 15 cross-pool
groups are `consensus_w2`/`consensus_w2b` only — `consensus_existing` shares neither code nor
binary with either and never cross-pool-merges.

### 22.3 Real SHA pin for generator/checker/dependency code and input manifests

`IMPLEMENTATION-SHA-MANIFEST-2026-08-20.json` (built by `pin_implementation_manifest.py`,
computed directly from bytes on disk, no hand-typed hash) pins the sha256 of every generator/
checker source file, verifier binary, and input census/manifest file each family/pool's digest
computation reads. It independently confirms the same finding at the file level:
`consensus_w2` and `consensus_w2b` pin to the identical `fam_consensus_w2.py` hash
(`abd0421d…`) and identical verifier-binary hash (`7ab25420…`); `consensus_existing` pins to
different hashes for both (`fam_consensus.py` = `10e2e0a3…`, its binary = `a4346aa4…`). This is
the artifact this entry's numbers are pinned against — if any of these files change on disk,
the numbers above are no longer backed by the same pinned inputs.

### 22.4 Duplicate definition, fixed

"Duplicate" is fixed to one of two definitions, both kept, neither overriding the other:

```
ISSUED-PROBLEM-COUNT (every row distinct once anchor_id/epoch/seed differ) = 4,426
RAW-MATERIAL-DIVERSITY (MATERIAL-PROJECTION-UNIQUE, this entry)            = 2,398
```

This entry's conservation table (22.1) uses the second definition — raw-material diversity —
because the audit's question is about anchor-content diversity, not issuance volume. The
issued-problem count stays a separate figure and is not merged into or subtracted from V4.

### 22.5 What this entry does not do

* Does not re-run A/H classification. Consensus and Rust stay A; EVM and Solidity stay H —
  unchanged from the original ANCHOR-COUPLING-AUDIT-P0 pass and every correction round since.
* Does not change `LLM-MINEABLE-ELIGIBLE-V4 = 13,963` (Entry 21) or any earlier V1/V2/V3
  figure. This is a decomposition of the existing A-rooted portion of V4, not a new count.
* Does not add, drop, or re-derive any row from any family's census.
* Does not resolve or start the Solidity "A method" family design the same operator order
  (msg 4161) raised — direct investigation found the "3,827" figure cited there is a
  different, already-closed pair of families (`solidity-diagnostic-mutation-v1` +
  `solidity-smt-diagnostic-mutation-v1`, both CALIBRATION-FAILED and never materialized), not
  the 2,782-row H-classified `solidity-source-synth-v1` family this audit scored. That scope
  question is open and unresolved as of this entry.
* `mineable_now = 0` is unchanged. No consensus, BF.7, reward, Base or real mining path is
  wired by this entry.

### 22.6 Artifacts

Results stay in the git-ignored sandbox (`local-docs/anchor-coupling-audit-p0-2026-08-20/`).
Only digests are tracked here.

| artifact | sha256 |
| --- | --- |
| `template_contract_digest_consensus.py` | `d3be9f0ad7995edf7bb5fca889201e1812fec822615211fe93515053a7ac96c7` |
| `template_contract_digest_rust.py` | `7fb67abfe63ce4a12faf906b70ba7afbc2237952493d6d669d9667f720d88986` |
| `TEMPLATE-CONTRACT-DIGEST-consensus.jsonl` (3,718 rows, 0 void) | `a5bb6feaab722d1a9c01c99754a8e3303bc90218e2f85e2560bdac07820564e4` |
| `TEMPLATE-CONTRACT-DIGEST-rust.jsonl` (708 rows, 0 void) | `bd9e2d4ad16eb8cc4fdaede0cf291f76e4d697092ebfbe9181b2108f0d848d93` |
| `pin_implementation_manifest.py` | `23e28918cd325cbe46042893fe3ca9c72ce5c95715cee6815d38aa0c15e1ad4c` |
| `IMPLEMENTATION-SHA-MANIFEST-2026-08-20.json` | `aa25c9195fdcd867d041ade8029bba078172a92426ec91ccb26b1e8c58567076` |
| `final_conservation_check.py` — standalone recheck cited by 22.1 | `b7691fc9f465b2a6ece6d5e54eb5604553dfa2976116a957dd44443cce4c516e` |
| `TEMPLATE-CONTRACT-DIGEST-CORRECTION-2026-08-20.md` — full writeup | `d9959cecb9a6f00ecae41295918dda2c65ac229abb39bc7fac6b620894e28b87` |

No answer, witness, or expected value is stored in any artifact above.

### 22.7 Not a claim

Closed local decomposition of an already-sealed count, offline, non-consensus. Not a public
benchmark, not a paid-API benchmark, not public-network mining, not a leaderboard claim, and
not a re-measurement of family-level LLM calibration (that stays Entry 14-21's).
`V4 = 13,963` is unchanged; this entry only reports how much of its A-rooted portion is unique
real material versus repeated real material. `mineable_now = 0`.

## Entry 23 — 2026-08-20 · ANCHOR-COUPLING-V2 Solidity successor wave / syntax ACCEPT 1/3 · REJECT 2/3, SMT 0/3 not tabulated, V4 unchanged

Sealed 2026-08-20 (operator order Telegram msg 4164, chat_id 1311067056, ordered an A-method
successor design for the two Entry 16 `CALIBRATION-FAILED` Solidity pools; msg 4166 ordered the
SMT run halted mid-wave; msg 4169 ordered this entry sealed). Append-only: Entries 1-22 and every
figure they sealed are unchanged, including `CALIBRATION-FAILED = 3,827` (Entry 16) and
`LLM-MINEABLE-ELIGIBLE-V4 = 13,963` (Entry 21). This entry resolves the scope question Entry 22
§22.5 left open — a v2 successor attempt was designed and run against exactly the two families
Entry 16 closed — and adds no row and changes no count.

### 23.1 Root cause and fix (syntax pool only)

`solidity-diagnostic-mutation-v1`'s `cleanSource()` never applied the preamble
(`pragma solidity >=0.0;` + SPDX line) the real isoltest harness always applies before compiling
(`test/libsolidity/util/Common.cpp::withPreamble()`), so the family's own compiles ran on
unpreambled source and could emit diagnostics no correctly-preambled compile — and therefore no
in-tree expectation block — ever produces. Fixed by applying the same `withPreamble()` logic the
sibling SMT family already used. Verified: 19/19 regression, 0 out of 1,199 out-of-vocabulary
diagnostics across the full calibration-pool sweep. `solidity-smt-diagnostic-mutation-v1` carried
no code change — its one Entry-16 miss (`functions/getters/external_getter_2.sol`,
BRANCH-LITERAL) was judged a model-reasoning miss, not a family bug.

### 23.2 Structural gate (zero model calls, both pools independently)

Four checks per pool — anchor-swap changes the required answer set both directions, no answer
exposure, deterministic checker, wrong-answer rejection — all four passed for both pools before
any model call. `STRUCTURAL-GATE-v2-2026-08-20.json`, sha256
`b7772f77fab82292e1700f43b5f65d587fb756e149a7fdf37a1d3d3c7ebf2477`.

### 23.3 Result — syntax pool

```
solidity-diagnostic-mutation-v1 (wave ANCHOR-COUPLING-V2)
STAGE-A                 = 1/3
ACCEPT                  = 1
REJECT                  = 2   reason DIAGNOSTIC-MISMATCH (both)
FAMILY-CALIBRATION-FAILED, closed at Stage A
census_permitted        = false
```

| branch | representative | verdict |
| --- | --- | --- |
| BRANCH-TYPE | `abiEncoder/abi_encodeCall_unitary_tuple_from_assignment_expression.sol` | REJECT / DIAGNOSTIC-MISMATCH |
| BRANCH-DECL | `inheritance/override/calldata_memory_interface_struct.sol` | REJECT / DIAGNOSTIC-MISMATCH |
| BRANCH-MODIFIER | `parsing/while_loop.sol` | ACCEPT |

Per protocol: not re-prompted, re-fixtured or re-measured in this wave.
`FRONTIER-CALIBRATION-v2-solidity-diagnostic-mutation-v1.json`, sha256
`5a6b0d2783007aa9ba0adf6aaee81036067fa0c5c4f176ee6a633e2e64c16ac3`.

### 23.4 Result — SMT pool

```
solidity-smt-diagnostic-mutation-v1 (wave ANCHOR-COUPLING-V2)
STAGE-A                 = 0/3   halted by operator instruction (msg 4166) before episode 1
                                 returned a verdict
ACCEPT                   = 0
REJECT                   = 0
NOT-TABULATED            = true
census_permitted         = false
```

Prompt digests were frozen (`CALIBRATION-FREEZE-v2-solidity-smt-diagnostic-mutation-v1.json`,
sha256 `1033976c006047714b9f4a03d47b9bfa1541196e08ec0d5c43970a9e434fbf4b`) before the halt; no
episode completed, no result file exists, and none of this wave's SMT activity is counted as a
verdict of any kind — measured 0, not a 0/3 fail.

### 23.5 Pool-accounting cross-check (operator order, msg 4166)

The syntax family draws its twelve calibration representatives from `syntax_calibration_pool`
(1,199 anchors already sealed `LLM-TASK-ELIGIBLE` under `solidity-source-synth-v1`), not from
`syntax_census_pool` (2,391, this family's own target) — a free draw that costs the 2,391 count
nothing. Checked directly, not assumed: the real syntaxTests corpus is 3,547 files; the two pools
are disjoint (0 overlap) and their union is exactly those 3,547 files; the calibration pool's
1,199 anchors match the sealed ledger's 1,199 `solidity-source-synth-v1` rows by exact set
equality; the apparent 1,199 + 2,391 = 3,590 vs 3,547 gap (43) is fifteen files that legitimately
emit more than one template row (vector-N sub-instances), which never caused double-counting here
because the syntax pool never reached its own census step (23.3). Conservation holds; no
correction to Entry 16 is needed.

### 23.6 What this entry does not do

* `CALIBRATION-FAILED = 3,827` (Entry 16) is unchanged — this is a new, independent attempt
  against the same two families, not a correction of Entry 16's figure.
* `LLM-MINEABLE-ELIGIBLE-V4 = 13,963` (Entry 21) is unchanged. No row is added, no row is
  dropped.
* No census ran for either pool (`census_permitted = false` both).
* Neither family is re-measured in this wave, win or lose — the fail-fast, no-same-wave-retry
  rule (operator order, msg 4164) applies to both: the syntax pool because it failed, the SMT
  pool because the operator ordered it stopped before measurement.
* Model episodes this wave: 3 (`claude-opus-4-8`, syntax pool only). SMT: 0.

### 23.7 Not a claim

Closed local, offline, non-consensus calibration attempt over pre-registered fixtures. Not a
per-template solve rate, not a paid API benchmark, not a public benchmark, not public-network
mining, and not a leaderboard claim. `mineable_now = 0`.

## Entry 24 — 2026-08-20 · RUST-TUPLE-STRUCT-PROJECT-V1 pre-registration / Stage A not started, pending a separate operator approval

Sealed 2026-08-20, **before the first model call of the wave it registers** (operator order,
Telegram msg 4176, tag `[RUST-TUPLE-STRUCT-PROJECT-V1]`; this pre-registration step ordered
separately by msg 4181). Append-only: Entries 1–23 and every figure they sealed are unchanged by
this entry, including `LLM-MINEABLE-ELIGIBLE-V4 = 13,963` (Entry 21). This entry adds no row and
changes no count.

### 24.1 What this entry is, and is not

This registers the frozen plan for a new Rust family, `TUPLE-STRUCT-PROJECT`, over a 320-anchor
population drawn from the read-only 14,095-anchor API-shape survey (`local-docs/rust-14095-
recovery-triage-v2-2026-08-20/`). Steps 1–3 below (freeze, structural gate, representative
freeze) are complete, real and mechanical — zero model calls, zero cost. **Steps 4–6 (Stage A's
3 episodes, Stage B's 9, and the one-shot 320-row census) are not started.** Per msg 4181, Stage
A's first real model call requires a further, separate, explicit "진행" (proceed) message from
the operator; this entry does not authorise it and nothing in this wave has run against the paid
model harness yet.

### 24.2 Selection: 672 → 320, every exclusion reason recorded

Of 672 anchors declaring at least one tuple struct, 320 are eligible under
`any_tuple_struct_in_anchor_eligible` (≥1 declared tuple struct with a valid identifier, no
stripped field, every field an int/bool primitive, 1–8 fields) and 352 are blocked. Re-derived
mechanically over all 352 blocked rows (not hand-summarized), cross-validated by independently
re-classifying the 320 already-eligible rows first (0 mismatches):

```
eligible (P1_TUPLE_STRUCT_ELIGIBLE)   320
blocked, non_primitive_field_type     336
blocked, zero_fields                   15
blocked, has_stripped_field             1
blocked, total                        352
672 = 320 + 352, confirmed by set union/overlap check (0 overlap)
```

Every one of the 320 IDs is independently set-equal (0 symmetric difference) between
`TRIAGE-V2-CLASSIFICATION.jsonl`'s `shape_class == P1_TUPLE_STRUCT_ELIGIBLE` filter and this
wave's own `ANCHOR-SOURCE-DIGESTS.json`. Per msg 4176, the unrelated 825-row
`STRUCTURALLY-INELIGIBLE` grade in the same triage file (825/825 zero-method traits, checked
exhaustively) is relabeled `CURRENT-DESIGN-INELIGIBLE / FUTURE-FAMILY-RESEARCH` — the mechanical
finding is unchanged, only the old label's implied permanence is corrected. That relabel lives in
the gitignored `local-docs/` triage artifact, not in this ledger, and changes no `docs/` figure.

### 24.3 Structural gate (Step 2), all seven checks PASS, one run

| check | result |
| --- | --- |
| a. anchor-swap changes the required answer set (3 pairs) | PASS — all REJECT |
| b. cross-anchor patch reuse rejected (3 pairs) | PASS — all REJECT |
| c. negative controls (18 probes × 6 anchors = 108 runs) | PASS — zero ACCEPT |
| d. no leakage of hidden test/reference-patch text into the prompt (6 anchors) | PASS |
| e. checker determinism (3 anchors, run twice) | PASS — fully consistent |
| f. author-witness ACCEPTs, then discarded (`answer_stored: false`) | PASS — 6/6 ACCEPT |
| g. zero duplication vs. `LLM-MINEABLE-ELIGIBLE-V4` (13,963) | PASS — 0 `template_id` hits, 0 `answer_sha256` hits, across 54,769 scanned artifact files |

Six coupling controls per sample pair (`anchor-removed`, `anchor-substituted`,
`challenge-substituted`, `anchor-result-ignored`, `cross-anchor-patch-reuse`,
`shadow-redefinition`) were run against all three sample pairs — 18 runs, 18/18 REJECT, real
compiler errors recorded per run (e.g. `E0432 unresolved import`, `E0609 no field`, `E0308
mismatched types`).

The first Step 2 attempt picked a sample anchor that turned out to be a genuine rustc
`tests/ui/...` UI-test fixture (its entire purpose is to be a compile-error case, e.g. carrying a
`//~^ ERROR` annotation); it failed check (f) because it cannot compile standalone. This was
root-caused by reading the anchor source directly. `pick_sample()` was corrected to require
standalone compile health as a precondition — a refinement of this wave's own new
sample-selection code, not a redesign of the frozen generator/checker/coupling design being
gated — and Step 2 was re-run once, cleanly, to the PASS above.

### 24.4 Disclosed finding: population compile-health caps the ceiling at 219/320

Independent of the structural gate, a full mechanical scan of all 320 anchors (each built
standalone, zero appended solution module, under the pinned toolchain; 29.3 s total) found:

```
standalone_compiles_ok      219 / 320   (68.4%)
standalone_compile_broken   101 / 320   (31.6%)
```

The broken 101 are overwhelmingly rustc `tests/ui/...` negative/UI-test fixtures whose entire
purpose in the rustc corpus is to be compile-ERROR test cases, not extensible modules. A broken
anchor can never host an appended solution and can never ACCEPT under this family, regardless of
model performance — **this caps Step 6's realistic recovery ceiling at ≤219/320 (68.4%),
independent of Stage A/B outcomes.** It also reflects a real gap in the earlier TRIAGE-V2
`CLEAN-A-CANDIDATE` grading (Part A of this project), which graded `shape_class` from the sealed
rustdoc manifest but never re-verified standalone anchor-compile health. The full 101-ID list is
recorded in `STEP2-GATE-RESULT.json["population_compile_health"]["broken_template_ids"]`.

### 24.5 12 representatives frozen (Step 3), Stage A vs. Stage B split

Among the 219 compile-healthy anchors there are exactly 10 distinct structural buckets (bucketed
by field count × per-field primitive category — signed, unsigned, bool):

```
(7 fields, [b,u,u,u,u,u,u])  x1     (2 fields, [u,u])   x14
(4 fields, [i,i,i,i])        x3     (1 field,  [b])     x10
(3 fields, [i,i,i])          x1     (1 field,  [i])     x73
(3 fields, [u,u,u])          x5     (1 field,  [u])     x103
(2 fields, [i,i])            x6
(2 fields, [i,u])            x3
```

12 representatives = one from each of the 10 real buckets, plus 2 clearly labeled *replicate*
picks from the two largest buckets (single-signed, single-unsigned), never presented as an
11th/12th novel branch. **Stage A (3, rep_id 1–3)**: the three most structurally distant buckets
— the 7-field anchor (`tests/ui/consts/issue-94371.rs`, the only one of its size), the one
2-field mixed-sign anchor (`tests/ui/splat/splat-method-tuple-simple.rs`), and a single-field
bool anchor (`tests/mir-opt/copy-prop/custom_move_arg.rs`, rarest primitive, 10/219). **Stage B
(9, rep_id 4–12)**: the remaining 7 distinct buckets plus the 2 replicates. Every representative
carries `template_id`, `anchor_sha256`, `task_seed`, `challenge_sha256` and `answer_sha256`
(digest only, `answer_stored: false`) frozen in `STEP3-FREEZE-REPRESENTATIVES.json`.

### 24.6 Pass criteria, budget, isolation — frozen before any model call

```
stage A     3 representatives, 1 episode each, no retry, 3/3 ACCEPT required to open stage B
stage B     remaining 9 representatives, 1 episode each, no retry, 12/12 ACCEPT required (total)
on failure  seal FAMILY-CALIBRATION-FAILED at the failing stage; no redesign, no retry this wave
step 6      only if stage A and stage B both pass; one mechanical pass over all 320, zero
            additional model calls
```

Budget per episode (reused verbatim from the sealed OPUS48 Rust wave, Entry 15's
`calibration.BUDGET`): 8 turns, 8 compile/assemble, 4 public test/check, 24,576 generated tokens,
1,800 s wall clock, 1 attempt, 0 retries, 0 manual edits. Max real model calls this wave: 12
(Stage A 3 + Stage B 9 + Step 6 census 0). Toolchain pin: `rustc`/`cargo` 1.99.0-nightly,
`rustc_commit_hash = e7795af6d2449fb05a6393c3320ced873a999eb3` (2026-07-22), host
`aarch64-apple-darwin` — the judge compiler is an isolated rust-lang CI build of the exact commit
the corpus is checked out at, never the user-global toolchain. Isolation model: the sanctioned
`Contestant` harness (`claude-opus-4-8`, first and only target, not a fallback), one fresh session
per task, no MCP server, no project instructions, no earlier answer or expected value visible to
it — same isolation discipline as Entry 14 §14.4. `P5_NARY_PRIMITIVE_FN` (47 rows) and the
`RESEARCH-CANDIDATE` pool (12,897 rows) are out of scope and untouched this wave.

### 24.7 Evidence bundle, all digests independently re-verified against the files on disk

All seven files below live in the gitignored `local-docs/rust-tuple-struct-project-v1-2026-08-
20/` sandbox; only their digests are tracked here, re-hashed directly from disk immediately
before this entry was written (not copied from any script's self-report):

| evidence file | sha256 |
| --- | --- |
| `STEP1-FREEZE.json` — population/generator/checker/prompt/toolchain/budget freeze | `8b2c686fa3c157df130513befee25d3bbda5e4d0824b8b73b712539c567f1068` |
| `STEP2-GATE-RESULT.json` — seven structural-gate checks + compile-health disclosure | `2a44996fc2431eac9f6c5d065eaef4068de128d3e719fff8c02da2ac07bc48e4` |
| `STEP3-FREEZE-REPRESENTATIVES.json` — 12 representatives, Stage A/B split | `d8db1c5bd6ca630f9df8f3f4cafe781d065d29b1e04f56076367dffa69669031` |
| `TUPLE-STRUCT-DESCRIPTORS.json` — tuple-field extraction for all 320 | `973712927fd6f583e5f3716f3b36e2d7d7c7e33d4cb8bdbd0de8f61b6c86ec36` |
| `ANCHOR-SOURCE-DIGESTS.json` — 320 candidate IDs + real anchor source sha256 | `2122fae94828858f93f8f66701efaffe414919a2d67b0fef0e48bb0d4484d35f` |
| `P1-EXCLUSION-REASONS.json` — 672→320 formula + per-row reason for all 352 blocked | `acc24da2daf6715cd00ace9962edcf8f46c920963712d3c75a1a998307606a42` |
| `PRE-REGISTRATION-MANIFEST.json` — maps the operator's 6 requested evidence items to the files above, with internal cross-checks | `91d18c4221e8a4bba5e3be0cd207d53c30d931190a67717c167b4cdc3b3d1b2e` |

`wave_policy_digest` (common to every file above): `cd90c313fad6488307f9fc8c09ecebab6155688699c7777c7ed8c6e1b7be3313`.

### 24.8 What this entry does not do

* `LLM-MINEABLE-ELIGIBLE-V4 = 13,963` (Entry 21) is unchanged. No row is added, no row is
  dropped.
* Entries 1–23 and every figure they sealed are unchanged.
* No model call has been made under this wave. `ADJUDICATED-TASKS = 0` for this wave.
* This entry does not authorise Stage A. Per msg 4181, a further, separate, direct operator
  "진행" message is required before the first real model call, and none has arrived yet.
* No full census, no new family beyond `TUPLE-STRUCT-PROJECT`, and no V5 promotion follows
  automatically from this entry; each would need its own instruction and its own seal.

### 24.9 Not a claim

A closed local, offline, non-consensus pre-registration of a frozen plan, zero model calls
issued to assemble it. Not a paid API benchmark, not a public benchmark, not public-network
mining, and not a leaderboard claim. `mineable_now = 0`.

## Entry 25 — 2026-08-21 · RUST-TUPLE-STRUCT-PROJECT-V1 Stage A+B (12/12), Step 6 census, Step 7 dedup / `LLM-MINEABLE-ELIGIBLE-V5 = 14,160`

Sealed 2026-08-21, after four separate, direct operator "진행" (proceed) messages — Stage A
(Telegram msg 4184), Stage B (msg 4187), the Step 6 census (msg 4190), and the Step 7 dedup +
this entry (msg 4193) — each authorising exactly one stage, none auto-advancing to the next.
Append-only: Entries 1–24 and every figure they sealed are unchanged, including
`LLM-MINEABLE-ELIGIBLE-V4 = 13,963` (Entry 21). This entry adds one new figure,
`LLM-MINEABLE-ELIGIBLE-V5 = 14,160`, and changes no earlier one.

### 25.1 What this entry is, and is not

This closes the wave Entry 24 pre-registered: real Stage A (3 episodes) and Stage B (9
episodes) model calls against the frozen `TUPLE-STRUCT-PROJECT` family, a real zero-model-call
mechanical census of all 320 frozen anchors, and a real zero-model-call dedup pass over the
census's issuable rows. It reports a candidate-eligibility count after dedup, not a solve rate:
the 12/12 ACCEPT below covers 12 representatives only; the other 308 anchors were never shown to
a model, at any stage.

### 25.2 Stage A — 3/3 ACCEPT, `claude-opus-4-8`, $0.068118

Representatives 1–3 (the 7-field bucket, the 2-field mixed-sign bucket, the rarest single-bool
bucket), one fresh isolated episode each, no retry, no fixture swap, no manual edit. Per-turn
model verification (two independent records — runtime `model_usage` and the runtime's own
session-transcript `models` field) showed only `claude-opus-4-8` on every answering turn; the
one expected `claude-haiku-4-5-20251001` turn per episode is session-naming housekeeping only
(≤20 output tokens, non-scored, same precedent as Entry 15). 3/3 ACCEPT, zero leaks (all four
`leak_scan` booleans `true` on every row), zero model substitution.

### 25.3 Stage B — 9/9 ACCEPT, combined 12/12, $0.192051 (Stage B) / $0.260169 (combined)

Representatives 4–12 (the remaining 7 structural buckets plus the 2 labelled replicates), same
isolation and no-retry discipline as Stage A. 9/9 ACCEPT, same dual per-turn model verification
clean on all 9. Combined family result: **12/12 ACCEPT**, combined cost $0.260169 — the wave's
full and final real-model spend, exactly the 12-call budget frozen in Entry 24 §24.6, 0 calls
remaining.

### 25.4 Step 6 — 320-row mechanical census, zero model calls, zero cost

Using the generator's own author-witness reference patch (the same mechanism validated by Entry
24 §24.3 check f) against all 320 frozen anchors, exactly once, sorted `template_id` order, no
retry:

```
ISSUABLE            199 / 320   (matches ≤219/320 ceiling from Entry 24 §24.4)
COMPILE_BROKEN      101 / 320   (identical set to Entry 24 §24.4's disclosed 101 broken IDs)
WITNESS_REJECT       20 / 320   (compiles standalone, but the reference patch itself fails
                                 the checker — new finding this step)
GENERATION_FAILED      0
A_BINDING_FAILED       0
V4_DUPLICATE            0
320 = 199 + 101 + 20 + 0 + 0 + 0, conservation HOLDS
```

Before the census ran, a drift gate re-hashed the 12 sources frozen at Step 1 (all match) and an
extended drift gate re-hashed the 320-row population digest, `TOOLCHAIN_PIN`, `CORPUS_COMMIT`
and the two installed compiler binaries (all match). Checker determinism was re-confirmed on 16
real re-run pairs (8 accept-path, 8 wrong-patch-path), 16/16 agree. Zero duplication against
`LLM-MINEABLE-ELIGIBLE-V4` on both axes: 0 `template_id` hits, 0 `answer_sha256` hits across
54,776 scanned artifact files.

### 25.5 Step 7 — canonical-identity dedup over the 199 `ISSUABLE` rows

The operator ordered a canonical template-contract digest per row, adapting this ledger's own
`CANONICAL-ISSUANCE-IDENTITY-V2` scheme (§12.2 above: identity is bound to intrinsic facts only).
For this family the digest is `family + generator_sha256 + checker_sha256 + corpus_commit
(revision) + compile_setting (toolchain pin) + anchor_sha256 + semantic_locator (source path +
declaration line) + struct_shape (name, field count, ordered field types)` — explicitly
**excluding** `template_id`, `task_seed`, `challenge_sha256` and `answer_sha256`, none of which
are intrinsic to the task.

Grouping all 199 `ISSUABLE` rows by this digest finds exactly one duplicate group, size 3:

```
RAW-ISSUABLE-ROWS         199
UNIQUE-NEW-ISSUABLE       197
INTERNAL-DUPLICATE-EXCESS   2
```

The one group — `template_id`s `4ce20a79…b914`, `7747a8f2…96a4`, `8f4994b7…0508` — shares an
identical `anchor_sha256`, the same source file (`tests/ui/layout/randomize.rs:17`) and the same
single-field struct `TransparentWrapper(u16)`. It is registered three times under three
different `template_id`s in the frozen 320-row population; this is a population-list artifact
from `TRIAGE-V2-CLASSIFICATION.jsonl`, not a checker or generator defect (deterministic
same-input-same-seed-same-answer is the expected, correct behaviour). Full corrected conservation:

```
320 = 197 (UNIQUE-NEW-ISSUABLE) + 2 (INTERNAL-DUPLICATE-EXCESS)
    + 101 (STANDALONE-COMPILE-FAILED) + 20 (ORACLE-OR-CHECK-FAILED) + 0 (other)
```

Holds.

### 25.6 Semantic duplication vs. V4 — none possible for this family

`CANONICAL-ISSUANCE-IDENTITY-V2` (§12.2) treats `family` as a required-equal identity
component: two rows in different families are never the same template, whatever anchor they
share. `TUPLE-STRUCT-PROJECT` first appears in this ledger at Entry 24 (line 3433) — zero hits
anywhere earlier in this document, and `LLM-MINEABLE-ELIGIBLE-V4 = 13,963` has been unchanged
since Entry 21, through Entries 22 and 23. A direct content scan (bypassing `.gitignore`
filtering) of 47,077 external `local-docs/` artifact files, excluding this wave's own directory,
found zero occurrences of the family label anywhere else. Under the identity definition already
governing this ledger, a family that has never before been issued cannot semantically collide
with any existing V4 row — so no exhaustive per-row comparison against all 13,963 V4 entries was
needed to reach this conclusion, and none was fabricated.

### 25.7 `LLM-MINEABLE-ELIGIBLE-V5 = 14,160`

```
V4 (Entry 21, unchanged through Entry 24)          13,963
+ UNIQUE-NEW-ISSUABLE (this wave, §25.5)              197
= V5                                               14,160
```

**197 is a candidate-eligibility count, not a solve count.** It means: of the 320 frozen
`TUPLE-STRUCT-PROJECT` anchors, 197 distinct (dedup'd) tasks can mechanically be generated,
compiled and checked to a real ACCEPT via the generator's own reference patch, with zero
duplication (internal or against V4). It does **not** mean a model solved 197, or 199, or 320
of anything — the only model-answered result in this wave is Stage A+B's 12/12 on 12
representatives (§25.2–25.3).

### 25.8 Evidence bundle, all digests re-verified against the files on disk immediately before this entry was written

All files live in the gitignored `local-docs/rust-tuple-struct-project-v1-2026-08-20/` sandbox;
only their digests are tracked here:

| evidence file | sha256 |
| --- | --- |
| `STEP4-STAGE-A-RESULT.json` — 3/3 ACCEPT, $0.068118 | `653855aa5e1d08b88713df0c4da36b804fbbb4c1bd9a7cc23e3853b64f491ab4` |
| `STEP5-STAGE-B-RESULT.json` — 9/9 ACCEPT, combined 12/12, $0.260169 | `95101b08e5af355792a196726bd39a35f16e1ae3bf8242f16e67ae57e0db4a4f` |
| `STEP6-CENSUS-RESULT.json` — 320-row census, buckets 199/101/20/0/0/0 | `a9b345226974afbedb78d0b788ee5987c33fd72a859ff56bdd1441f1b4826e1f` |
| `STEP7-DEDUP-RESULT.json` — canonical-identity dedup, 197 unique + 2 excess, V4 semantic check | `3f519d169cc113b3eef3b3ee0282738a20756b43dc32b58b2f6a532c2c7f6c2c` |

`wave_policy_digest` (common to every prior file in this wave, unchanged since Entry 24):
`cd90c313fad6488307f9fc8c09ecebab6155688699c7777c7ed8c6e1b7be3313`.

### 25.9 What this entry does not do

* `LLM-MINEABLE-ELIGIBLE-V4 = 13,963` (Entry 21) is unchanged. `V5 = 14,160` is a new, separate
  figure that supersedes nothing.
* Entries 1–24 and every figure they sealed are unchanged.
* Does not claim a model solved 197, 199 or 320 anchors. `ADJUDICATED-TASKS` for this wave is
  12 (Stage A 3 + Stage B 9), not 197 and not 320.
* Does not resolve, promote or retire the `4ce20a79…` / `7747a8f2…` / `8f4994b7…` triple beyond
  recording it as one dedup group; the frozen `TRIAGE-V2-CLASSIFICATION.jsonl` population list
  itself is unchanged by this entry.
* `P5_NARY_PRIMITIVE_FN` (47 rows) and `RESEARCH-CANDIDATE` (12,897 rows) remain out of scope
  and untouched.

### 25.10 Not a claim

Stage A/B are real, costed `claude-opus-4-8` episodes (12 total, $0.260169) under a closed
local, offline, non-consensus, sandboxed harness — not a public benchmark, not public-network
mining, not a leaderboard claim. Step 6 and Step 7 issued zero model calls. `mineable_now = 0`.

## Entry 26 — 2026-08-21 · zk-native investigation closure — `NO-CLEAN-A-ROOTED-FAMILY-UNDER-P0`

Sealed 2026-08-21 under Telegram msg 4206 §2 (chat_id 1311067056), which orders the new-domain /
new-family / new-raw-material investigation ended and recorded, append-only, with no new
dependency downloads, repo expansion or family research after this entry. Zero model calls, zero
paid API calls anywhere in this entry's four source investigations. Append-only: Entries 1–25 and
every figure they sealed (including `LLM-MINEABLE-ELIGIBLE-V5 = 14,160`, Entry 25) are unchanged.
**zk-native's contribution to every eligibility figure in this ledger remains 0, before and after
this entry.**

### 26.1 What this entry is, and is not

This is the closing record of a four-step, closed-local, read-only exploratory investigation
(2026-08-21) into whether any real, compilable zk-native (SNARK/STARK-tooling) source material on
this machine could seed a deterministic, checkable LLM task family — a question separate from,
and later than, this ledger's own 2026-08-16 domain-level ruling (Entry 3, Entry 12) that
zk-native is `NO-DETERMINISTIC-FAMILY` at the census level because no answer-free deterministic
checker exists for a human-judged audit-finding domain. This entry's four artifacts asked a
narrower, more concrete question — given the zk-native-tagged repos actually present in this
machine's dependency caches, is there any subset that (a) is a real, unmodified, currently
buildable Rust project and (b) contains changed source with a genuine, non-mechanical, non-leaked
design space — and answers it, honestly, in the negative. **This entry does not claim zk-native
families are permanently impossible; it records that none was found under this pass's scope
(P0), using the material and constraints available on this machine on this date.**

### 26.2 Cache-inventory correction — 1 buildable file corrected to 28 files / 2 real projects

The first pass (`zk-native-repo-context-p0-2026-08-21`) classified this machine's 16,763-file
zk-native-tagged audit pool using only the default `~/.cargo` cache and found exactly **1**
fully buildable file. A follow-up pass (`zk-native-cache-union-correction-p0-2026-08-21`) redid
the same classification against the *union* of every real dependency cache actually present on
this machine, to check whether the first count was an undercount artifact of looking in only one
place. It was:

```
COMPLETE-BUILD-CONTEXT   28 files (up from 1), across only 2 distinct real projects:
  keep-starknet-strange/garaga  tools/garaga_rs                        27 files
  succinctlabs/sp1              examples/untrusted_program/jit-program  1 file
SNAPSHOT-PRESENT-DEPENDENCY-MISSING   13,550
TOOLCHAIN-MISMATCH / FILE-ONLY-NO-BUILD-CONTEXT / UNRESOLVED     0 / 0 / 0
16,763 = 972 (no compiler for this language) + 2,213 (byte-identical duplicate)
       + 13,578 (unique real Rust/Go projects); confirmed twice
```

The honest attribution, stated explicitly in that pass's own record so this is not
overstated here: the growth from 1→28 is a **methodology fix** (checking a
`~/.cargo/git/checkouts` folder that was always present but never scanned by the first pass), not
new material from the newly-unioned cache sources — the union added two genuinely new cache
roots (an `evm-zkvm-feasibility` cache and an `ethereum-consensus` vendor tree), both fully
fingerprinted and wired into the matching logic, and both resolved **zero** net-new dependencies.
Two unrelated parser bugs (a Go multi-block `go.mod` reader, and a `parse_cargo_config_vendor()`
git-only-vendor miss) were found and fixed during this pass; both confirmed non-material to every
bucket count. The correction does not change any sealed eligibility figure in this ledger — the
sealed zk-native-domain count was, and remains, 0.

### 26.3 Build-unit smoke — 2/2 `cargo check --offline --frozen` pass

Both of the 2 real projects the correction identified were then, for the first time, actually
built (front-end parse + type-check only, no LLVM codegen, no final binary) exactly once each,
strictly sequentially, `--jobs 1`:

```
keep-starknet-strange/garaga  tools/garaga_rs                          rc=0, 13.0s, PASS
succinctlabs/sp1              examples/untrusted_program/jit-program   rc=0, 0.03s, PASS
```

Both `cargo metadata`/`cargo check` invocations used `--offline --frozen`, which hard-error
rather than silently reach the network — zero network access during either of the 2 authorized
runs. The SP1 unit's PASS carries one important scope caveat, disclosed before execution and
confirmed true after: `cargo check` skips full codegen and LLVM inline-assembly validation, and
this crate's `asm!()` block uses RISC-V-specific mnemonics meaningful only for the real, unavailable,
out-of-scope `riscv64im-succinct-zkvm-elf` target — so this rc=0 certifies the file type-checks
for the host target, not that it would actually assemble, link or run for its real target. Full
immutability verification (cargo registry, cargo git cache, both extracted source trees, both
archive files) confirmed byte-for-byte unchanged, before vs. after.

**Rustup network-access incident — disclosed in full, not hidden.** Before this task's own
`FREEZE.json` was written and before either of its 2 authorized executions, a diagnostic
`rustc --version` run inside the SP1 snapshot directory (to determine "the exact toolchain that
will be used") triggered rustup's own toolchain-file resolution logic — a layer entirely separate
from, and not gated by, Cargo's `--offline`/`--frozen`/`CARGO_NET_OFFLINE` flags. Seeing 3
components in that directory's `rust-toolchain.toml` (`llvm-tools`, `rustc-dev`, `clippy`) not yet
installed locally, rustup performed one real network sync, silently upgrading the machine-shared
`~/.rustup` toolchain `stable-aarch64-apple-darwin` from **1.97.1 to 1.98.0** and adding those 3
components plus 2 bundled ones (`rust-src`, `rust-std-wasm32-unknown-unknown`). This did not touch
`~/.cargo`, any archive file, any repo file, or any `local-docs/` file — only the separate,
machine-global `~/.rustup` shim toolchain, which is used solely by this exploratory
investigation's own scripts and is explicitly distinct from the pinned, isolated CI toolchain
(`toolchain/ci-e7795af6`, Entry 24/25) that the actual `RUST-TUPLE-STRUCT-PROJECT-V1` mining wave
(Entry 24/25, Entry 27 below) uses — the two are not the same toolchain and this incident did not
touch the one the mining wave depends on. Mitigated for the remainder of the task via an explicit
`RUSTUP_TOOLCHAIN=stable-aarch64-apple-darwin` pin, which bypasses toolchain-file resolution
entirely; no further network activity was observed or expected afterward, and none was.

### 26.4 Garaga-27 family design — all 27 anchors `NO-DETERMINISTIC-FAMILY`

The 27 changed source files inside garaga's `tools/garaga_rs` (the only build unit with more than
1 anchor) were then read and hand-graded, one by one, against this ledger's existing family-design
rulebook, for whether any could become a real checkable/mineable LLM task. Recommendation:
**TERMINATE.** All 27 classify `NO-DETERMINISTIC-FAMILY`:

```
clippy-lint autofixes                                          6
pure module-registration wiring                                 2
pyo3's own PyObject -> Py<PyAny> upstream rename                10
mixed rename + wiring                                            1
pure FFI-marshaling wrapper boilerplate                          4
relocated real logic into a non-anchor auto-generated file,
  with a same-file old-vs-new equivalence test already present    2
------------------------------------------------------------------
26 mechanical/leaked, each with exactly one already-correct
  answer sitting in the archived target snapshot                26
```

The 1 remaining anchor (`algebra/g2point.rs`, real elliptic-curve group-law logic) is leaked
twice over in the same snapshot: an unchanged sibling file (`g1point.rs`) implements the
near-identical pattern one type parameter away, and the repo's own Python reference module
(`hydra/garaga/points.py`) already implements the same class with the same methods. The closest
near-miss, `tools/garaga_rs/src/calldata/signatures.rs` (RSA-2048 witness-generation, genuinely
new and substantially untested), still fails on two independent grounds: it is the only anchor of
its kind in this build unit, so "changing which anchor is used changes the required answer" (a
required family property) cannot be demonstrated with a population of one; and the same repo
snapshot's `hydra/garaga/rsa_rns.py` spells out the exact algorithm and its own limb/modulus
parameters, freely readable. Per this ledger's own default rule — when genuinely uncertain
whether a candidate satisfies the family-design bar, default to `NO-DETERMINISTIC-FAMILY` rather
than force-fit a family of one — `signatures.rs` is recorded as `NO-DETERMINISTIC-FAMILY`, not as
a one-member family. What a future, differently-scoped attempt at this exact anchor would need is
recorded as diagnostic-only in the underlying artifact; it is not proposed, designed or
implemented here.

### 26.5 Conclusion — `NO-CLEAN-A-ROOTED-FAMILY-UNDER-P0`

Across both the 2026-08-16 census-level ruling (Entry 3, Entry 12: no deterministic
answer-free checker exists for the human-judged zk-native audit domain) and this
2026-08-21 build-context-grounded exploratory pass (28 real buildable files across 2 real
projects, both compile cleanly, 27-anchor hand-grading of the one multi-anchor unit finds 26
mechanical-or-leaked and 1 near-miss still disqualified on n=1-population and cross-language
leakage grounds), the honest, current-scope conclusion is: **no clean, anchor-rooted zk-native
family was found under this investigation's P0 constraints.** This is a scoped negative result,
not a claim that such a family can never exist — a differently-scoped future attempt (a
multi-anchor build unit, or a checker design that does not exact-match a leaked reference
serialization) is explicitly left open as unexplored, not foreclosed. Per this entry's own
governing instruction, no further dependency download, repo expansion or family research
follows from this conclusion in this task. **zk-native's contribution to
`LLM-MINEABLE-ELIGIBLE-V5` is 0. `LLM-MINEABLE-ELIGIBLE-V5 = 14,160` (Entry 25) is unchanged.**

### 26.6 Evidence bundle, all digests re-verified against the files on disk immediately before this entry was written

All files live in the gitignored `local-docs/zk-native-*-2026-08-21/` sandboxes; only their
digests are tracked here:

| evidence file | sha256 |
| --- | --- |
| `zk-native-repo-context-p0-2026-08-21/RESULT.json` — first pass, 1 buildable file, default cache only | `bc117a8630dea40a8a4e6c32f06745df9479daaed2e1f172986e3f73d8d02d22` |
| `zk-native-cache-union-correction-p0-2026-08-21/RESULT.json` — corrected pass, 28 files / 2 projects, methodology-fix attribution | `c1654763430d391b8ce8a999b91fa444a6a9551ef202c90b5d9b6e6065806716` |
| `zk-native-cache-union-correction-p0-2026-08-21/SUMMARY.md` | `c69d2625f875d0ffaa64f1d0a21b05184e0215db65e91b54f9d9fda2efe772b1` |
| `zk-native-build-unit-smoke-p1-2026-08-21/RESULT.json` — 2/2 `cargo check` pass, immutability confirmed | `f5d675f5cc41b223b2790431fb7fb90205c220f5160f4475330618c89fda9a02` |
| `zk-native-build-unit-smoke-p1-2026-08-21/FREEZE.json` — rustup network-access incident, full disclosure | `9bc53cf775ae6d2cdb504f19425f9ddbdb5c54c4647d46628be442c480ab9b8c` |
| `zk-native-garaga-27-family-design-p0-2026-08-21/RESULT.json` — 27/27 `NO-DETERMINISTIC-FAMILY`, TERMINATE | `8882199a5854046cd81c6bfdd84699510b19b72194e7276093f10d9697007371` |

### 26.7 What this entry does not do

* Does not change any earlier entry or figure. Entries 1–25 and `LLM-MINEABLE-ELIGIBLE-V5 =
  14,160` (Entry 25) are unchanged.
* Does not claim zk-native families are permanently impossible — see §26.1 and §26.5.
* Does not implement any family, checker or generator for zk-native material.
* Does not authorize, propose or perform any further dependency download, repo expansion, cache
  scan or family-design pass on zk-native material — this investigation thread is closed.
* Does not touch `docs/zk-native-release-audit-census-p0-eligibility-freeze.md` or its sealed
  `ZK-NATIVE-RELEASE-AUDIT-P0-MINEABLE-ELIGIBLE = 0` figure.

### 26.8 Not a claim

All four source investigations are closed-local, offline, read-only or `--offline --frozen`
compile-only passes — zero model calls, zero paid API calls, zero network access during any
authorized step (the one disclosed rustup network event happened before, not during, an
authorized step, and did not touch this ledger's mining-wave toolchain). Not a public benchmark,
not public-network mining, not a leaderboard claim. `mineable_now = 0`.

## Entry 27 — 2026-08-21 · A-rooted native mining E2E v1 — one real episode, `CLOSED-LOCAL-NATIVE-MINING-E2E-GREEN`

Sealed 2026-08-21 under Telegram msg 4206 §§4–6 (chat_id 1311067056). This entry is not a new
census and does not expand `LLM-MINEABLE-ELIGIBLE-V5`. Its purpose is narrower: run one already
sealed, already-mineable Rust-tuple task through the real problem-issuance → LLM-answer →
ProofIntake → Canonicalizer → frozen native checker → node verifier → local share-accounting
path, exactly once, and record the outcome — success or failure — as it actually happened. No new
family, no new checker logic, no new hint, no per-instance exception was created; the sealed
`RUST-TUPLE-STRUCT-PROJECT-V1` generator, prompt and checker (Entry 24/25) were reused byte for
byte. `LLM-MINEABLE-ELIGIBLE-V5 = 14,160` (Entry 25) is unchanged; no V6 figure is created here.

### 27.1 Selected template

Selected under the pre-registered SHA-256 selection rule (§ STEP0, `local-docs/a-rooted-native-mining-e2e-v1-2026-08-21/STEP0-SELECTION-RESULT.json`) from the 197 `UNIQUE-NEW-ISSUABLE`
Rust-tuple templates, excluding the 12 already-used Stage A/B representatives and the internal
3-duplicate group:

```
template_id   04dd8453f52dd4da8af1736ad6eceeb82ded2e65dcc1c0600c4b37ef7cf6307a
source_path   tests/ui/consts/transmute-const.rs
struct_name   Foo(u32)
family        TUPLE-STRUCT-PROJECT / RUST-TUPLE-STRUCT-PROJECT-V1
anchor_sha256 693f62acfa0626a0831c9133a26fcfc1dbb30922c1ab2036231c42a363cfd7fe
```

### 27.2 Real model execution

Exactly 1 real, paid `claude-opus-4-8` call, in a single fresh isolated session, 0 retries, 0
fallback/substitution, 0 human edits, per msg 4206 §5's own authorization:

```
model        claude-opus-4-8 (sanctioned only; independently re-verified against the
             runtime's own session transcript — substitution_detected: null, no stray model,
             no permission denials, num_turns == 1)
session_id   966cdfd6-e7ad-4224-9316-11c383ab143e
turns        1 (model answered directly — no COMPILE/PUBLIC-TEST actions used)
cost_usd     0.02066  (cap was $0.10 — well under)
wall_time    6.1s
tokens       input 2 / output 210
```

The model was given the anchor + statement + scaffold only — no correct answer, no author
witness, no hidden expected value was shown to it. Its own answer (`raw_final_reply`, captured
verbatim, sha256 `003857c61993cf754bca5b99af2ec0d51028b03085653cea9ed8bca108e7d38a`) is the one and
only candidate used downstream — nothing was substituted, edited or re-run.

### 27.3 Path verdicts — all six `CLOSED-LOCAL-NATIVE-MINING-E2E-GREEN` requirements met

```
driver_answered            true   (model produced a FINAL action with real Rust code)
proof_intake_accepted      true   (candidate parsed, template/challenge/checker/policy binding matched)
verify_accepted            true   (frozen sealed checker + Rust node-verifier binding both ACCEPT)
canonical_receipt_generated true  (RECEIPT.json written, binds family/version, template_id,
                                    anchor digest, challenge/epoch, candidate digest, checker
                                    digest, policy digest, verdict)
node_verifier_ACCEPT       true   (binding_accepted: true -- an independent recheck inside
                                    boole_miner::run_native_mining_e2e, not a trusted self-report
                                    of the Python-side verdict)
local_dev_share_recorded   true   (one SHARE-LEDGER.ndjson entry appended)
```

`mineable_now = 0` throughout — no block, reward or consensus state was written or read by any
step of this episode. No SP1/ZK proof was generated or used anywhere in this scope, per msg 4206
§6's explicit prohibition.

### 27.4 Reject-path test coverage (unchanged, pre-existing, GREEN)

The 5 reject-path tests msg 4206 §4 requires were written and GREEN in an earlier segment of this
same task, in `crates/boole-miner/tests/native_mining_e2e.rs`, run against fixtures — the sealed
checker/binding logic under adversarial mutation, not a re-judgment of the one real answer above
(there is exactly one real accept-path case in this episode; the reject paths are exercised with
constructed fixtures, as msg 4206 §4 itself specifies):

```
accept_path_full_pipeline_accepts                       PASS  (fixture ACCEPT candidate, full path)
one_bit_answer_mutation_is_rejected                      PASS  (flipped bit -> checker/verify REJECT)
candidate_reused_for_different_task_is_rejected          PASS  (mismatched template_id -> REJECT)
candidate_reused_for_different_challenge_is_rejected     PASS  (mismatched challenge_sha256 -> REJECT)
checker_or_policy_digest_mutation_is_rejected             PASS  (mismatched checker/policy digest -> REJECT)
```

Re-confirmed GREEN this segment via `cargo test -p boole-miner --test native_mining_e2e` alongside
the new example binary addition, with no regressions to the crate's other 28 lib tests.
`cargo fmt --check` and both CI-identical clippy invocations (`--workspace --all-targets`,
default features; and `--features boole-node/dev-mock-payment,boole-miner/dev-tools`) are clean.

### 27.5 Evidence bundle, all digests re-verified against the files on disk immediately before this entry was written

All source files live in the gitignored `local-docs/a-rooted-native-mining-e2e-v1-2026-08-21/`
sandbox; only their digests are tracked here:

| evidence file | sha256 |
| --- | --- |
| `FREEZE.json` — frozen template identity, budget, digests, authorization | `f2d8294c082a450b131fa50111ae252f03c866fb673eaef9140972d04031b710` |
| `STEP0-SELECTION-RESULT.json` — SHA-256 selection over the 197 UNIQUE-NEW-ISSUABLE set | `1b9fad93ecc88c7fd737624dd089e0642e2cc2c44eb56bf59975054568726a15` |
| `STEP2-EPISODE-RESULT.json` — the one real `claude-opus-4-8` episode, verdict ACCEPT | `bc90995c973769e16339103b60778c3fc50e7d9ba5f46a78c81b653127b08393` |
| `RECEIPT.json` — canonical receipt from `boole_miner::run_native_mining_e2e` | `02a9ffa7d0fc910040dfbd72bcf59d03f2608d15e48d7eebfa8297eff03fe452` |
| `SHARE-LEDGER.ndjson` — local dev share-accounting entry | `39ba4d482b0bce5c4651d6baf3db16e721b8ae49881c54a54dbd7077b6e1e301` |

### 27.6 What this entry does not do

* Does not create a V6 eligibility figure. `LLM-MINEABLE-ELIGIBLE-V5 = 14,160` (Entry 25) is
  unchanged.
* Does not re-run the census, does not re-execute any of the other 196 `UNIQUE-NEW-ISSUABLE`
  templates, does not investigate another domain, does not compare models, does not run a
  performance benchmark.
* Does not touch consensus state — no block was built, no reward was paid, no persisted-chain or
  `SharePool` state was written. `mineable_now = 0`.
* Does not generate or use an SP1/ZK proof.
* Does not introduce a new family, new checker logic, new hint, or per-instance exception. The
  sealed `RUST-TUPLE-STRUCT-PROJECT-V1` generator, prompt template and checker (Entry 24/25) were
  reused byte for byte, verified via a 10-source drift gate immediately before the real call.

### 27.7 Not a claim

One real, costed `claude-opus-4-8` episode ($0.02066, 6.1s, 1 turn) under a closed local,
offline, non-consensus, sandboxed harness — not a public benchmark, not a paid public API
benchmark claim, not public-network mining, not a leaderboard claim. `mineable_now = 0`.

## Entry 28 — 2026-08-21 · Entry 27 scope correction — `MINER-LOCAL-NATIVE-WIRING-GREEN`; real `boole-node` checker re-execution `NOT-RUN`

This append-only correction preserves Entry 27 byte for byte and narrows only the claim that its
recorded execution can support. A read-only inspection of the landed implementation found that
Entry 27's phrases "node verifier" and "independent recheck inside `boole_miner`" overstate what
ran. No model, checker, census, proof or mining execution was run for this correction.

### 28.1 What Entry 27 did establish

The one real model answer did traverse the following closed-local, miner-owned path:

```
real LLM response
  -> family-specific NativeProofIntake
  -> template / challenge / checker / policy binding checks
  -> verdict already produced by the frozen external Python checker
  -> boole-miner NativeReceipt assembly
  -> local non-consensus SHARE-LEDGER.ndjson append
```

The raw model response was preserved and hashed, the frozen task identity was checked, the
external checker had actually compiled and tested the candidate, and the miner-side wiring bound
those facts into a local receipt and ledger row. The five constructed reject-path tests remain
valid evidence for that miner-local intake/binding/wiring surface. The correct completion label is
therefore **`MINER-LOCAL-NATIVE-WIRING-GREEN`**.

### 28.2 What Entry 27 did not establish

The landed Rust replay binary explicitly carries forward the verdict already written by the
external Python checker; it does not invoke that checker again. In
`boole_miner::run_native_mining_e2e`, `verify_accepted` is assigned the existing
`checker_accepted` value after the binding checks. The `boole-node` process, its HTTP server and
its state were not called anywhere in the episode. Consequently, Entry 27 did **not** establish:

* receipt or raw-answer admission by the actual `boole-node` binary;
* node-owned lookup of a pinned family, anchor, checker, policy and toolchain;
* independent node execution of the real checker against the submitted raw answer;
* a receipt or shadow-evidence object issued from a node-owned verdict; or
* any `SharePool`, block, reward, P2P, BF.7 or consensus connection.

The authoritative status is:

```
REAL-BOOLE-NODE-SEMANTIC-RECHECK = NOT-RUN
NODE-ISSUED-NATIVE-EVIDENCE      = NOT-RUN
CONSENSUS / BLOCK / REWARD       = NOT-WIRED
```

The earlier `node_verifier_ACCEPT = true` field is retained as historical evidence but must be
read only as "the miner-local wiring accepted the already-produced checker verdict after binding
checks." It must not be cited as an independent node judgment.

### 28.3 Successor boundary

The authorized successor design is `NATIVE-SUBMISSION-SHADOW-ADMISSION-V1`, specified in
`docs/native-submission-shadow-verification-v1.md`. It requires the node to receive the raw answer
and independently execute a tracked, byte-pinned checker before the node can issue shadow
evidence. It is default-OFF and non-consensus. A receipt-only input or a pass-through of the
miner's verdict is a hard failure, not a fallback.

This correction does not retract the actual model answer, the external checker's ACCEPT, the
miner-local receipt, the local ledger row or their preserved hashes. It corrects only which
component made which judgment.

### 28.4 Numbers and claim boundary — unchanged

```
LLM-MINEABLE-ELIGIBLE-V5 = 14,160
mineable_now              = 0
```

No V6 is created, no eligibility row is added or removed, and Entries 1–27 remain unchanged.
This is closed-local code-reality correction, not public-network mining, not a public or paid-API
benchmark, not a leaderboard claim, and not evidence of consensus activation.
