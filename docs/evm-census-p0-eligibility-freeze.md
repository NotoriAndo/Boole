# EVM census P0 — case-task-binding eligibility freeze (v1.5)

Ceiling label: **EVM-P0-MINEABLE-ELIGIBLE = 6,767**.

This is an **issuable problem count, not network activation**. EVM `mineable_now`
stays **0** until BF.7 consensus activation — this record wires **no** consensus /
BF.7 path. It is closed-local, non-consensus evidence and is **not a public-network
/ leaderboard / paid-API / production claim**.

This document is an **append-only attestation ledger**. The ledger and proof
**originals stay in the git-ignored sandbox**
(`local-docs/evm-zkvm-feasibility/freeze-records/stage1-7306-run/genesis-p0-case-task-binding-design-v3_1/emitter-impl/`);
only content hashes and lineage are tracked here so the result survives outside the
sandbox. Future waves append new dated entries below; existing entries are never
rewritten.

---

## Entry 1 — 2026-08-10 · stage1-7306-run / design-v3_1 / S-CONTRACT-FREEZE-v1.5

### Frozen contract (S-CONTRACT-FREEZE-v1.5, Accepted)

Per-case admission identity uses **BLAKE3** over length-prefixed raw digests
(`pf(x) = struct.pack("<Q", len(x)) + x`; `raw(hex)`), with three frozen admission
domain strings. `pv#1` is the **per-case** `task_contract_admission_digest`
(8-field preimage: family, policy, eligible-ledger root, oracle-ledger root,
canonical-anchor root, 4-way pairing, per-case `case_admission_digest`, and the
resource-policy admission-contract digest `acd` appended as the last field). The
retired v1.3 global `task_contract_digest` is not used.

| frozen field | value |
| --- | --- |
| admission domain (case) | `boole.emitter-output.case-admission.v1` |
| admission domain (contract / pv#1) | `boole.emitter-output.task-contract-admission.v1` |
| admission domain (family) | `boole.emitter-output.family-binding.v1` |
| `family_binding` | `54da764baa871e286ad8c17858ef1f84a922b55c893d5d71377a0c0edab48199` |
| `policy_digest` | `ee9ea351d7529b3a2929dbfcf267d394400cb9d6086a4ee36c5920637a824c83` |
| `acd` (resource-policy admission contract) | `60e3f0bd80c0d97e4c0a3f6701b8cbfa201fb31d6852506f43907c9bc538e269` |
| `canonical_input_root` | `be29a5646ff32345a7a0cc96ccbaaebcf9b601314fa4b5867866179deaa9f074` |
| `canonical_anchor_root` | `c27de7be67837737176c665ff35437d139c1085b374dc7699d015abd1cd68a2e` |
| `eligible_ledger_root` | `278bea159605bb74b30767272c9a62f23cd7921e0e0a2a2939caedf0669d50c9` |
| `oracle_ledger_root` | `d9513adf18256b60c2fa25d2149dafbd96376e4e5330baf6151b4dbc8bb3f3e3` |
| `pairing_4way` | `5266d47a4ce24c6672f9c758f7f2a36c078249982756a5e3ef0e092d935796cf` |
| `fork_or_rule_digest` (Cancun exec) | `9e69ea039851cd44ab42db9125a69327633e012bfcf03fbc146c7201271e355b` |
| frozen verifying key `vk.bytes32()` | `0x0055a63544a5bf8ea7b944c2208fa9d011312da36a70afda0d73222ff2302eae` |
| frozen guest ELF sha256 | `0983d5fe6dd205a6487e5ddb5a5850031b69ce43192090376cc6c5816320c1fb` |
| SP1 tooling | `sp1-sdk` 6.3.1, circuit `SP1_CIRCUIT_VERSION` v6.1.0 |

The canonical input root (`be29a564…`) and the 4-way pairing (`5266d47a…`) were
reused as-is; no S-INPUT re-run or regeneration was performed. The per-case
author-oracle digests and the oracle-ledger root were not changed; zero-elision is
applied only at the comparison stage.

### Conservation identity (final ledger, FROZEN)

**6,855 = 6,767 emitted + 79 duplicate + 7 deferred-lossy + 2 deferred-provenance.**

The emitter was re-run exactly once, in the order
`problem_digest → emitter_task_key → verification_binding → case_admission → pv#1`.
The ledger was frozen only because this identity held. Independent audit cross-check:
the emitter-dedup survivor set and the audit grouping
`(canonical_input_bytes, oracle_semantic_digest, source_material=source_sha256)`
min-nodeid survivors are byte-identical (survivors and non-survivors both match); all
79 duplicates are content-clones. The six frozen roots plus the 3-way/4-way pairing
reproduced (reproduction gate PASS). Distinct `pv#1` over the emitted set = 6,767.

### Ledger hashes (hash-only; originals stay in the sandbox)

| artifact | rows | bytes | sha256 |
| --- | ---: | ---: | --- |
| `final_task_ledger_v1_5.jsonl` | 6,767 | 12,408,217 | `c1a1a71a73f40331221efc486cc3f8f4a614b387f9c5b9ed670961d88de6f6fd` |
| `final_duplicate_ledger_v1_5.jsonl` | 79 | 29,981 | `8ee8f002d42de0f8524bf4017c33bc3547f8ae94b39cc793f4305c436fb24a5a` |
| `final_ledger_summary_v1_5.json` | — | 1,885 | `3eecd80bc74d5411cd547633ec87f8af0ee22450be047d7f29417ebcf8f394ac` |
| `final_eligibility_census_v1_5.json` | — | 1,108 | `cd4ff4f4aae0c0b074127f99246b8f5dc19159bd2a9b19c55afc10b9cd452e1f` |
| `S10-FIXTURE-DESIGNATION.json` | — | 3,098 | `cee0030a387c98e812831a3dca9a2fd2cb1419ec9ac8a1675bc5f6afbe5ae2a3` |

### S9 general verifier

Verifies any task in the final ledger (and the designated validation fixture) by
recomputing `pv#1` from the task's own bindings under the frozen v1.5 contract.
Focused tests: admission layer 10/10 and proof layer 11/11. It rejects a wrong
task / input / oracle / fork / policy / acd, a cross-case `pv#1`, and (proof layer)
a wrong vk / wrong proof and a wrong committed `pv#3`/`pv#4`/`pv#5`, including a
cross-case committed pv (a content-clone's per-nodeid oracle echo mismatches). No
consensus / BF.7 wiring.

### S10 — one positive compressed proof (validation-only)

Fixture designated **before** proof generation: the first (nodeid sort order) of the
79 duplicate non-survivors with complete resource-policy + input + oracle +
source-material bindings; it is **not** one of the 6,767 emitted tasks.

* fixture nodeid: `tests/byzantium/eip196_ec_add_mul/test_ecmul.py::test_invalid[fork_Cancun-state_test-not_on_curve_0_3_no_scalar-ecmul]`
* content-clone of survivor: `…test_invalid[fork_Cancun-state_test-not_on_curve_0_3-length_80-ecmul]`
* source material: `tests/byzantium/eip196_ec_add_mul/test_ecmul.py` @ commit `2282c757b3699d506de112b8a48b6b538df7ed1f` (whole-source-file-bytes sha256)

A compressed STARK proof was generated **exactly once, 0 retries**, within the
existing 4-hour / 48 GiB limits (prove ≈ 27.8 s; pure crypto verify ≈ 0.029 s). The
new verifier ACCEPTs it and every negative rejection row passes.

| artifact | bytes | sha256 |
| --- | ---: | --- |
| `proof-s10_ecmul.bin` (compressed STARK) | 1,273,368 | `c0b77cb07fd9ab8518046776e6f1505c4b860df2bfdb97c620df85c247d8a3fb` |
| `pv-s10_ecmul.bin` (committed public values) | 799 | `bcc314742faffcb76e15cde66a2ec8dc6e7c21baf211e57d42fc484a1dd7a1f3` |
| `s10_ecmul_guest_input.json` | 1,371 | `a0e3d3e4053073da40365c1c7c9f24f5e4f60fc500508af47234f7a0da9d02f9` |

Committed public values (192-byte header = 6 × 32-byte digests):

| pv | name | value |
| --- | --- | --- |
| pv#1 | task_contract (task-global anchor, echoed) | `a71a7d71daef9bca8ceca270eae0fa180430d5ca2e0de84435e79ba87a1197f3` |
| pv#2 | case_or_batch_root | `2231e1cbe63e4bf4ebe0f02fa256faef829f70cfbb68e01968d87ef1f6eb3e95` |
| pv#3 | fork (Cancun) | `9e69ea039851cd44ab42db9125a69327633e012bfcf03fbc146c7201271e355b` |
| pv#4 | canonical_input = sha256(canonical_input) | `2231e1cbe63e4bf4ebe0f02fa256faef829f70cfbb68e01968d87ef1f6eb3e95` |
| pv#5 | author_oracle (per-nodeid measurement echo) | `2a9a087b4c57e0f9af42299201e26d9755b4e25957f420cbaba8858b644d57f1` |
| pv#6 | observed_accounts (revm execution) | `0da60342084674604d082aac1d523d5a9959cb8c0cc10394ba99e83744121696` |

The verifier re-derives vk from the frozen ELF and enforces it equals the frozen
`vk.bytes32()`, crypto-verifies the compressed STARK, and checks pv#4 ==
sha256(canonical_input). A byte-flipped ELF (wrong vk) and a tampered committed pv
(wrong proof) both REJECT.

### Final full-eligibility census (6,767)

Deterministic completeness over all 6,767 emitted tasks on nine axes — task id,
input, author oracle, fork, policy, vk / verifier, resource eligibility, source
material, partition — all 6,767 / 6,767 complete. Distinct task ids = distinct
`pv#1` = 6,767. Global gates (eligible-ledger, oracle-ledger, and frozen guest ELF
digests) match. `certified = true`, `mineable_now = 0`. No per-problem proof was
generated.

### Boundary / non-claims

* **EVM-P0-MINEABLE-ELIGIBLE = 6,767** is the issuable problem count, not network
  activation. `mineable_now stays 0` until BF.7 consensus activation.
* No consensus / BF.7 / mining / reward / Base / paid-API change was made; the S9
  verifier is not connected to any consensus path.
* Originals live only in the git-ignored sandbox; this record carries hashes and
  lineage so the freeze is durable in-tree — closed-local validation only.
* This record is **not a public-network / leaderboard / paid-API / production claim**.
