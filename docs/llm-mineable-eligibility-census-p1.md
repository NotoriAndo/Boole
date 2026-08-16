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
