# Commit/reveal devnet fixtures

Current protocol: [v1.2](../../docs/devnet-commit-reveal-v1.2.md), reset from genesis.
Every current block carries L1-info followed by the M2ROOT system deposit.
The v1 JSON inputs remain archived byte-for-byte; they are not replayed as v1.1.

## Active v1.2 recordings and shared vectors

- `l2-genesis-v1.2.json`: the previous L2 allocations plus the `0x00` STOP sink
  at `0x42000000000000000000000000000000000000b2`; the old genesis is frozen.
- `root-system-v1.2.json`: independent canonical M2ROOT deposit bytes. Reproduce
  with `python3 -I scripts/devnet/generate_m2root_vector.py`.
- `commit-reveal-v1.2.json`, `adversarial-v1.2.json`, `rounds-v1.2.json`:
  reset recordings of the 84/72/140-height scenarios below. A/B/C block hashes,
  embedded roots and pure-model roots agree at every recorded height.
- `root-binding-v1.2.json`: an actual peer-payload import with a test-only fold
  mutation at height 6. The mutant stays at height 5 while A/C continue; its
  actual Engine process restarts mid-round, re-folds and matches through 28.
- `../devnet-m1/s1b-recorded-schedule-budget-v4.json`: the reset M1 schedule
  with the extra system reservation and M2ROOT. Original raw occurrences remain.

The v1.1 config, event and package vectors still define the unchanged M2 fold,
slots and carriage:

- `config-golden-v1.1.json`: independent canonical config bytes, slot encodings,
  config/slot digests and root-v2 probe vectors for all three profiles.
- `event-golden-v1.1.json`: 47-byte round records with pool_seq, fixed draw and
  union-mapping seeds, status/prune events and next-header priority.
- `package-codec-v1.1.json`: an actual pinned-zstd 1 MiB decoded package with a
  carried size inside STATEMENT_MAX. Generation verifies decoding; derivation
  validates the transmitted bytes and declared codec/length only.

The following v1.1 **node recordings are superseded** by their v1.2 counterparts
and remain byte-for-byte frozen:

- `commit-reveal-v1.1.json`: 84 live and recorded A/B/C heights, v2 pool/package
  bodies, a codec-1 statement package, lane-2 OPEN, 14-part reveal, restart, fresh
  C and finality/reorg.
- `adversarial-v1.1.json`: 72 live and recorded A/B/C heights, commit saturation,
  uncapped reveal, copied parts, forgery/size/window rejection and gas probes.
- `rounds-v1.1.json`: 140 live and recorded A/B/C heights with distinct slot
  windows (48/4 versus 96/8), withholding, closure and later draws.
- `../devnet-m1/s1b-recorded-schedule-budget-v3.json`: original M1 transaction
  occurrences with the same budget schedule, recorded under root v2. Its
  provenance includes the exact M2 config and digest.

Generate the first three using
`BOOLE_DEVNET_ZSTD=<pinned executable> python3 -I crates/boole-derivation/tests/generate_v11_vectors.py`.
Its SHA-256 is recorded in each vector. For node recordings the tracked Rust
integration tests write directly to BOOLE_DEVNET_M2_RECORDING,
BOOLE_DEVNET_I6_RECORDING, BOOLE_DEVNET_I7_RECORDING and
BOOLE_DEVNET_SCHEDULE_RECORDING after comparisons and strict cleanup.
All node recordings retain per-height compared values and replay against the
pure fold. Config and slot digests are included in provenance.

At the previous reset, the v1.1 event/config/recording files superseded event-golden-v1.json,
commit-reveal-v1.json, adversarial-v1.json, rounds-v1.json and the v2 M1 schedule
for that reset. The service parity subset, size summary, inbox genesis, gas
observations and codec executable pin still apply unchanged. The original M1
JSON/genesis files remain unchanged.

## Retained inputs and v1 history

These are derivation and client fixtures, not kernel-certified proofs or production allocations.

- `service-parity-v1.json`: vectors from the registry's real commit, reveal and
  advance API, with a synthetic catalog and injected PASS/FAIL verifier. Source
  and generator SHA-256 values bind provenance. Rust uses D=2 and W+1 maps the
  registry's expiry boundary. No ERROR or CANCEL enters this shared subset.
- `event-golden-v1.json` (**superseded by `event-golden-v1.1.json`**): independently encoded fixed hex for draw, withholding,
  deadline, expiry and prune records, plus the next-header tie order.
- `size-boundaries-v1.json`: quantile boundary rows from the environment sizing
  summary. These rows do not reproduce the underlying receipt distribution.
- `l1-genesis-v1.json`: the S1 inbox ABI and topic with its size scratch pointer
  placed after copied calldata when necessary. Small calls retain the original
  scratch location and memory cost. Large PART logs therefore preserve every
  byte. The original genesis remains unchanged.
- `commit-reveal-v1.json` (**superseded by `commit-reveal-v1.1.json`**): 84 finalized heights compared on live and recorded
  A/B/C, including a restart during a 14-part reveal and an unfinalized reorg.
  Provenance includes client pins, the L1 genesis and the comparison output hash.
  Gas observations cover every posted kind and the EIP-7623 calldata floor.
- `l1-gas-limits-v1.json`: separate fresh-node probes at 2^24 and 2^24+1 gas.
  Pinned anvil admits and mines both with 21,000 gas used; this configuration
  does not enforce the EIP-7825 transaction gas limit.
- `adversarial-v1.json` and `rounds-v1.json` (**superseded by the corresponding
  `adversarial-v1.1.json` and `rounds-v1.1.json`**): I6's 72 heights and I7's 80
  heights, each compared on live and recorded A/B/C and replayed offline against
  the pure model at every height. The full compared values are in each row.
- `zstd-pin-v1.json`: exact darwin-arm64 codec executable identity. Tests require
  `BOOLE_DEVNET_ZSTD`; the executable is checked before use, with no PATH lookup.

Regenerate the **v1.2 recordings** using the tracked `m2_devnet` integration test
with the pinned client
variables from the M1 README and `BOOLE_DEVNET_ZSTD`. The superseded v1 files
are frozen historical recordings; this command does not reproduce the historical v1/v1.1 clients. Set
`BOOLE_DEVNET_M2_RECORDING`, `BOOLE_DEVNET_I6_RECORDING` and
`BOOLE_DEVNET_I7_RECORDING` to distinct candidate output paths. The tracked
generator constructs the full L1 genesis, source genesis hashes and inbox-change
provenance before writing each file after successful comparisons and strict
cleanup. No metadata injection or private comparison-log digest is required.
For the current v4 M1 schedule use `m1_schedule` and
`BOOLE_DEVNET_SCHEDULE_RECORDING`; the superseded v2/v3 schedules are also frozen.
Accepted fixtures keep the compared attributes, decisions, roots and observations.

The client recording uses a deterministic test generator package and synthetic
configuration identities. Its compressed bytes exercise codec framing, size,
checksum and digest reconstruction only; gate parsing and proof validity belong
to the next milestone. The test calldata batch caller is added to disposable L1
state and calls the inbox once per encoded item.

The separate M1 budget schedule recording defers the third 12M-gas candidate at
height 24 to the beginning of height 25. Every original raw transaction occurrence
is preserved, with new client observations for duplicate and execution-failure
cases. All previous M1 fixtures remain unchanged.

For the root divergence/restart recording, run the tracked `root_binding` test
with `BOOLE_DEVNET_OP_RETH` and `BOOLE_DEVNET_ROOT_RECORDING`. The current
`m2_devnet`/`m1_schedule` producers write candidates only after the same complete
comparisons and strict cleanup; copy accepted files exactly. No metadata is
injected into recordings. Historical v1/v1.1 node recordings are not current
reset replay inputs; use their original revision to reproduce those clients.

Regenerate service parity from the repository root with:
`python3 -I scripts/devnet/generate_service_vectors.py`.
The generator is tracked and resolves the repository root from its own path.
The move changed only `provenance.generator_sha256`: all four cases, all 315
height rows, their event-log digests and every other fixture field stayed
identical. It runs only for explicit regeneration, outside self-test runtime.

The v1.2 test Engine profile is recorded in provenance: forced per-block disk
persistence (`persistence_threshold=0`), its required zero hot-block buffer, and
`suppress_persistence_during_build=true`. The last setting asks the Engine to
defer persistence cycles while a payload build is active on this disposable chain.
Original-profile failed attempts retain their unexpected cache-fallback WARNs;
the accepted recordings still require zero unexpected warnings/errors and
actual restart recovery. No log filter, sleep or block-validity rule changes.
