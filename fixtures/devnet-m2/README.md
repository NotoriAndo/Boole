# Commit/reveal devnet fixtures

These are derivation and client fixtures, not kernel-certified proofs or production allocations.

- `service-parity-v1.json`: vectors from the registry's real commit, reveal and
  advance API, with a synthetic catalog and injected PASS/FAIL verifier. Source
  and generator SHA-256 values bind provenance. Rust uses D=2 and W+1 maps the
  registry's expiry boundary. No ERROR or CANCEL enters this shared subset.
- `event-golden-v1.json`: independently encoded fixed hex for draw, withholding,
  deadline, expiry and prune records, plus the next-header tie order.
- `size-boundaries-v1.json`: quantile boundary rows from the environment sizing
  summary. These rows do not reproduce the underlying receipt distribution.
- `l1-genesis-v1.json`: the S1 inbox ABI and topic with its size scratch pointer
  placed after copied calldata when necessary. Small calls retain the original
  scratch location and memory cost. Large PART logs therefore preserve every
  byte. The original genesis remains unchanged.
- `commit-reveal-v1.json`: 84 finalized heights compared on live and recorded
  A/B/C, including a restart during a 14-part reveal and an unfinalized reorg.
  Provenance includes client pins, the L1 genesis and the comparison output hash.
  Gas observations cover every posted kind and the EIP-7623 calldata floor.
- `l1-gas-limits-v1.json`: separate fresh-node probes at 2^24 and 2^24+1 gas.
  Pinned anvil admits and mines both with 21,000 gas used; this configuration
  does not enforce the EIP-7825 transaction gas limit.

The client recording uses a deterministic test generator package and synthetic
configuration identities. Its compressed bytes exercise codec framing, size,
checksum and digest reconstruction only; gate parsing and proof validity belong
to the next milestone. The test calldata batch caller is added to disposable L1
state and calls the inbox once per encoded item.

The separate M1 budget schedule recording defers the third 12M-gas candidate at
height 24 to the beginning of height 25. Every original raw transaction occurrence
is preserved, with new client observations for duplicate and execution-failure
cases. All previous M1 fixtures remain unchanged.
