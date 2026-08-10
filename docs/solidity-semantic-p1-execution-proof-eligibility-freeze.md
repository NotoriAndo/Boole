# Solidity semantic P1 — EVM execution-proof task eligibility freeze (v1)

Ceiling label: **SOLIDITY-EVM-EXECUTION-PROOF-P1-MINEABLE-ELIGIBLE = 1396**.

This is an **issuable problem count, not network activation**. Solidity `mineable_now`
stays **0**; this record wires **no** consensus / BF.7 path. It is closed-local,
non-consensus evidence and is **not a public-network / leaderboard / paid-API /
production claim**.

This wave is the **direct successor** to the compile-only Solidity census P0
(`docs/solidity-census-p0-eligibility-freeze.md`), which deferred the **1,670**
`semanticTests` as `DEFERRED-EVM-REQUIRED` — a compile-only family cannot decide a
runtime-semantics answer. Here each `semanticTest` is materialized into a **real EVM
execution case** and its eligibility is decided by whether a compressed zkVM proof of
**correct EVM execution** can be produced for it within a fixed cycle budget.

**Why public expected outputs are not answer leakage.** In P0 the direct-answer route
was answer-leaked: the expected `// ----` line sits next to the input in the public
checkout, so issuing a compile-only task is unfair (all `EXCLUDED-ANSWERED-FIXTURE`).
Here the artifact a miner must produce is a **SP1 compressed STARK proof that the EVM
actually executed the deployed bytecode and produced output O** — not the value O
itself. Knowing O does not shortcut generating a valid execution trace/proof, so the
public expected output is **not** a leaked answer. A fixture is excluded **only** if a
real proof was already generated for it (answer-confirmed), never merely because its
expected output is public.

This document is an **append-only attestation ledger**. The guest/host implementation,
the pinned compiler, the Solidity `semanticTests` corpus, the materialized cases, the
run ledgers, and the representative proof **stay in the git-ignored sandbox**
(`local-docs/solidity-semantic-p1-2026-08-10/`); only content hashes, corpus
fingerprints, conservation identities, and lineage are tracked here so the result
survives outside the sandbox. Future waves append new dated entries below; existing
**merged** entries are never rewritten.

---

## Entry 1 — 2026-08-10 · execution-proof-census-v1 / N = 1396

### Result in one line

The **1,670** frozen `semanticTests` were reduced to **1,474** compile-materialized
execution candidates, each run natively (revm, Cancun) against its author-recorded
`// ----` oracle and measured for SP1 execution cycles. After excluding the one
answer-confirmed proof fixture, host-dependent / non-reconstructible oracles, genuine
execution mismatches, and cases over the 8,000,000-cycle mineable ceiling,
**SOLIDITY-EVM-EXECUTION-PROOF-P1-MINEABLE-ELIGIBLE = 1396**.

### Task unit (fixed BEFORE full execution)

One `semanticTest` `.sol` file **plus its ordered full call bundle** (constructor +
every `// ----` call line, in file order) = **one task**. This unit is fixed and is not
changed after seeing results.

* The isoltest legacy / via-IR "also" dual-run is a **compile-pipeline variant of the
  same semantic task**, not an independent parameter variant — each task uses its single
  author-sanctioned compile variant (see the compile policy below), so it is **not**
  split into two tasks.
* `semanticTests` have **no** general parameter-sweep mechanism; there is no per-file
  fan-out into independent parameter instances. One file → one ordered bundle → one task.

### Pinned execution engine + SP1 proof binding

The guest engine is a **real EVM** (revm), deployer nonce 0, `SpecId::CANCUN`,
`basefee = 0`, coinbase = zero address. It intentionally does **not** reproduce
isoltest `MockedHost` quirks (EOA-CREATE nonce preincrement, `tx.origin = 0x9292`,
coinbase `0x7878`, `basefee = 7`, synthetic blockhash/blobhash/prevrandao/number/
timestamp/gaslimit); host-dependent oracles are bucketed as `UNSUPPORTED-ORACLE` rather
than reproduced, which keeps the guest — and therefore the vk — frozen.

| binding component | value |
| --- | --- |
| guest ELF | `sha256 1599d54fd75ef48742a9ec460628b6caba68d7a4f33a9c615707b713465d37a2` |
| verifying key (`vk_bytes32`) | `0x004a748560e6b44075bd4fc72a0e88bcef34a91c6d3a47b37a4416d13126b207` |
| SP1 proof circuit version | `v6.1.0` |
| SP1 package (`sp1-zkvm` / `sp1-sdk`) | `=6.3.1` |
| revm | `=38.0.0` |
| alloy-primitives | `=1.5.6` |
| guest edition | `2021` |
| target fork | `CANCUN` |

The proof binds to **(guest ELF + vk + SP1 verifier digest)**: a proof accepted under
`vk 0x004a7485…` attests execution of exactly this guest ELF. Changing the engine would
change the ELF and the vk, so the binding is the engine freeze.

### Representative proof (the one `EXCLUDED-PROOF-FIXTURE`)

A real compressed proof was generated for `./array/fixed_arrays_in_constructors.sol`
(canonical exec 688,688 cycles, ≤ 8M, oracle `ACCEPT`). Because a proof already exists
for it, it is answer-confirmed and excluded from the final candidate count — counted in
**exactly one** bucket (`EXCLUDED-PROOF-FIXTURE`), never double-subtracted as both a
fixture and anything else.

| proof artifact (git-ignored) | value |
| --- | --- |
| `zkvm/rep-real/out/proof.bin` | `sha256 96a7ef5793e59644788fc67669d569af60f3426de11269ba131969f1755dbc6d`, 1,273,110 bytes, `compressed` |
| public values length | 541 bytes |
| observed output digest | `94d46fac914e8b030ca80720c6d68f290128748997d2545caa9ee1ce9a2cc1ad` |
| verify | `accepted` |

### Compile policy (Level A)

The pinned compiler is **soljson.js `solc@0.8.36`** (WASM), `sha256
ccb677d54dfab2a9b30084eec6bb396c93eb86d58b42cc00267fd0f54f391f32` — the same pin as the
Solidity census P0. Each test honors its **author-specified** compile settings
(`compileViaYul` / `optimize` / `EVMVersion` / `revertStrings`); via-IR is **not**
applied globally. `EVMVersion` is in-scope iff `cancun` satisfies it. Gas is
**non-gating**: the functional oracle (return / revert / logs) is fork-stable and
asserted; recorded gas values are not.

### Conservation identities (FROZEN)

**Level 0 — corpus → runnable cases** (`file-ledger-step2.json`, over 1,670 `.sol`):

```
1,670 total_files = 1,519 CASES-MATERIALIZED
                  +   109 HARNESS-UNSUPPORTED   (no runnable isoltest harness mapping)
                  +    42 NO-RUNNABLE-CASE      (no constructor/call bundle to execute)
                  +     0 ERROR
```

**Level A — compile classification** (`compile-ledger-v2.json`, over the 1,519 cases):

```
1,519 = 1,474 COMPILE-MATERIALIZED-CANDIDATE
      +    20 EVM-VERSION-OUT-OF-PIN       (author EVMVersion not satisfied by cancun)
      +    23 ABI-ENCODER-V1-REQUIRED      (needs ABIEncoderV1; out of the 0.8.36 default)
      +     2 SOLC-0.8.36-INCOMPATIBLE     (front-end error under the pin)
      +     0 DEFERRED-VIAIR-REINTERPRETATION
      +     0 SOURCE-UNRESOLVED
```

**Level B — execution census** (`case-ledger-v2.json`, over the 1,474 candidates; each
candidate in exactly one primary bucket, precedence
`fixture → oracle-fail → cost → duplicate → eligible`):

```
1,474 = 1,396 MINEABLE-ELIGIBLE            ← N
      +     1 EXCLUDED-PROOF-FIXTURE       (fixed_arrays_in_constructors, real proof exists)
      +     0 DUPLICATE                    (all 1,474 canonical_input byte-distinct)
      +    44 UNSUPPORTED-ORACLE           (30 host-dependent MISMATCH + 14 indexed-dynamic ORACLE_UNSUPPORTED)
      +    12 EXECUTION-MISMATCH           (observed real-EVM output != recorded oracle; follow-up)
      +    21 DEFERRED-HIGH-COST           (> 8,000,000 exec cycles)
      +     0 HARNESS-ERROR
      +     0 TIMEOUT
      +     0 UNRESOLVED
```

Reconciliation with the native-oracle round-trip (`verify-ledger-all.json`):
`ACCEPT 1,418 = N 1,396 + fixture 1 + DEFERRED-HIGH-COST 21 + DUPLICATE 0`. All 21
over-ceiling cases are themselves `ACCEPT`, so none is double-counted against the oracle
buckets. `MISMATCH 42 = UNSUPPORTED-ORACLE-from-mismatch 30 + EXECUTION-MISMATCH 12`;
`ORACLE_UNSUPPORTED 14` are all `UNSUPPORTED-ORACLE`.

**N is a strict lower bound.** Every case counted in N has a confirmed author-oracle
round-trip **and** executes within the 8M mineable cycle ceiling. The 12
`EXECUTION-MISMATCH` are excluded conservatively and flagged for forensic follow-up —
some (ABI length-diff, empty-revert) may prove to be oracle-recording artifacts rather
than engine defects; none is counted toward N.

### The three exclusion families, by fingerprint

**`UNSUPPORTED-ORACLE` (44).** The author oracle depends on isoltest `MockedHost` state
the frozen real-EVM guest does not reproduce, or cannot be faithfully reconstructed by
the comparator:

* block / tx host globals — `blobhash`, `blockhash`, `prevrandao`, `block.basefee`,
  `block.coinbase`, `block.difficulty`, `block.gaslimit`, `block.number`,
  `block.timestamp`, `tx.gasprice`, `tx.origin` (17 files);
* account code / balance preconditions — `codehash`, `codehash_assembly`,
  `codebalance_assembly` (3);
* deployer-nonce CREATE address & cross-contract emitter identity —
  `create_random`, `event_emit_from_other_contract`,
  `selfdestruct_post_cancun_redeploy` (3);
* isoltest framework `account()` derivation — `isoltestTesting/account` (1);
* anonymous events (no topic0 to reconstruct) — `event_anonymous*` (3),
  `optimize_memory_store_multi_block_bugreport` (1);
* sub-4-byte malformed revert (engine emits exactly 3 bytes; the comparator's
  4-byte-selector un-padding cannot reconstruct the recorded word) —
  `malformed_panic_2`, `malformed_panic_3` (2);
* indexed dynamic-type event params (topic = keccak(value), token stream incomplete) —
  14 `ORACLE_UNSUPPORTED`.

**`EXECUTION-MISMATCH` (12).** Host-independent, faithfully-materialized cases where the
observed real-EVM output disagrees with the recorded oracle:
`abicoder/calldataDecoding/array/*` (4), `abicoder/cleanup/{dynamic_array,static_array,
struct}_v2` (3), `revertStrings/empty_v2`, `smoke/basic`, `smoke/fallback`,
`fallback/short_data_calls_fallback`, `inlineAssembly/keccak_optimization_bug_string`.

**`DEFERRED-HIGH-COST` (21).** Over the 8,000,000-cycle mineable ceiling (11 hit the 16M
measurement cap): the seven `array/array_storage_*` + `array_function_pointers`, two
`storage/storage_boundary_*`, `storageLayoutSpecifier/{dynamic_array,mapping}_storage_end`
at the cap; then `abi_encode_calldata_slice` (15.37M), `array_copy_including_array`
(14.59M), `storage_boundary_struct_array_packed` (14.45M), `static_array_copy_cleanup`
(12.96M), `bytes_storage_to_storage` (12.09M), `deposit_contract` (11.96M),
`failed_create` (11.73M), `storage_boundary_struct_array_multislot` (11.31M),
`abicoder/cleanup/intx_v2` (10.68M), `precompiles_ignoring_trailing_input` (8.70M).

### Cycle-ceiling boundary (no ambiguity)

The largest mineable-eligible case is **7,308,504** cycles
(`abicoder/cleanup/bytesx_v2.sol`) — **691,496** cycles below the ceiling. The smallest
deferred case is **8,700,111** cycles. The gap across the 8M line is **~1.39M cycles**,
far larger than the ≤ ~1,000-cycle drift between placeholder and real header digests
used in the batch cycle measurement, so no case sits close enough to the ceiling for
that drift to change its bucket.

### Corpus fingerprint (git-ignored corpus, hash-pinned here)

| corpus | files | aggregate sha256 |
| --- | ---: | --- |
| `test/libsolidity/semanticTests` | 1,670 | `f0af98e63cde61a6399929f38daa70e694aa929f65c28e7071c624ddf9661f28` |

Method (`input-freeze.json`): for each `.sol` file emit `<sha256(bytes)>  ./<relpath>`,
sort the lines by relpath, join with `\n` and a trailing `\n`, then `sha256` the whole
manifest. Reproduced byte-exact at freeze time.

### Census artifact hashes (git-ignored; hash-pinned here)

| artifact | role | sha256 |
| --- | --- | --- |
| `cases.jsonl` | 1,519 materialized author cases | `f3197b5ee069a8e3692acaf325e64c693647cb9de323e3fe8ea6d0064dd2fa1c` |
| `file-ledger-step2.json` | Level 0 ledger (1,670 → 1,519) | `11bdf6a505c4ddc8fd055d85b2536a31fb9a819ce681663bc42eeeb5b5891e8c` |
| `compile-artifacts-v2.json` | per-test author-sanctioned bytecode | `6e76dbbec1c15ae20a5715a18fa24457e2744a3c17bd90fbb4fce12f588c706d` |
| `compile-ledger-v2.json` | Level A ledger (1,519 → 1,474) | `2a9b0c68f3d1a01754acd54c7d231a49ccefb9f10f22a821b8ddec96b8fe9a39` |
| `zkvm/materialize.mjs` | case → canonical_input materializer | `ae808fae7e12205ec217cc174bada1704f9b83d037e6388d6550a12f1365ba2e` |
| `zkvm/host/src/main.rs` | native exec + batch cycle + prove host | `b0d0070286cade0aa53d2196c6a9a49b22bd8a775981bc25b6498685f2d6c0f0` |
| `zkvm/verify.mjs` | author-oracle canonicalizer | `cae1904ddba4c8bc5782543c92ba723361320b9a8872a024e1043640d9af60dc` |
| `zkvm/census/run-input-all.json` | 1,474 canonical inputs | `59d3ad4b4530c592036557270aa060f4e0d950555569d42705b5602c2a91c36d` |
| `zkvm/census/run-output-all.json` | native exec observed outputs | `377ea65a05d12ce763bc9510c553d01e221e74e4967b56f0ee9c467f723f031e` |
| `zkvm/census/verify-ledger-all.json` | oracle round-trip verdicts | `2e03a1fcedef120b9a46a1acc09c9cc1eee7ae9e41b2b9e7bb423a602f6033da` |
| `zkvm/census/cycles-all.json` | per-case SP1 exec cycles | `6ccaf0af9026c7857ed90e28c9617d469e30ae1953a0bfa507f93be8a3f76e3b` |
| `build_case_ledger.mjs` | Level B bucket assignment + N | `d15c5bb9bbd6f5cd64972c9e4a35a8035f97d022d3e8a434f7eff00fd88cb04f` |
| `case-ledger-v2.json` | Level B ledger (1,474 → N = 1,396) | `aa35e87e7ac70a5d283b95be770d4e91fad5a50b7d703c0e080fbb521fa8cad4` |
| `input-freeze.json` | wave input freeze | `c656160522d8f00321013664bda705955ae90624e6c1a47fb5a92de1d795632e` |

Guest ELF `sha256 1599d54f…` and representative proof `sha256 96a7ef57…` are pinned in
the binding tables above.

### Resource policy (FROZEN)

Execute cycle ceiling **8,000,000** (over → `DEFERRED-HIGH-COST`); proof size limit
4,194,304 bytes; representative proof wall limit 4 h / 48 GiB; **network 0; retries 0**.

### CI scope (what green actually attests)

CI (`self-test`, `supply-chain`) does **not** recompute the 1,474-case census or
re-derive N. It verifies that **this freeze record is intact and the repository has no
regression**; the sandbox artifacts are hashed here. The N = 1,396 result is established
by the git-ignored sandbox run recorded above, not by CI.

### Boundary / non-claims

* **SOLIDITY-EVM-EXECUTION-PROOF-P1-MINEABLE-ELIGIBLE = 1396** is the issuable problem
  count under this frozen `semanticTests` corpus, this pinned compiler, and this frozen
  execution-proof guest — **not** a claim that these tasks are activated on any network.
  `mineable_now` stays 0.
* Public expected outputs are **not** answer leakage: the mined artifact is a proof of
  correct EVM execution, not the output value. Only the one case with an actual generated
  proof is excluded as answer-confirmed.
* The 21 `DEFERRED-HIGH-COST` and 12 `EXECUTION-MISMATCH` cases are **follow-on scope**,
  not discarded: a higher cycle budget (or split execution) could admit the former, and
  forensic review could reclassify some of the latter.
* No consensus / BF.7 / mining / reward / Base / paid-API change was made; the engine is
  an offline revm guest connected to no consensus path.
* Guest/host implementation, pinned compiler, corpus, materialized cases, run ledgers,
  and the representative proof live only in the git-ignored sandbox; this record carries
  hashes, the corpus fingerprint, conservation identities, and lineage so the freeze is
  durable in-tree — closed-local validation only.
* This record is **not a public-network / leaderboard / paid-API / production claim**.
