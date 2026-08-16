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
