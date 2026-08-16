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
