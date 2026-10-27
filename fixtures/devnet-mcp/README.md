# Own finalized account proof fixtures

These are disposable loopback Anvil v1.8.5 captures, not consensus goldens or
release authority. Each JSON records the binary digest from
[`devnet-m1/pins.json`](../devnet-m1/pins.json), the tracked genesis digest,
startup arguments, exact header and account proof. No transaction was broadcast.

- `nonce-account-proof-genesis-mismatch-v1.json` preserves the initial response:
  Anvil returned a proof, but its root differs from the captured genesis
  stateRoot. The verifier must refuse it and keep the observation unavailable.
- `nonce-account-proof-mined-v1.json` follows `anvil_mine(["0x10", 12])`.
  Finalized height 8 and bootstrap headers 0–8 are retained. The proof verifies
  against that exact header's stateRoot and proves account nonce zero.

Both requests use `eth_getProof(address, [], {blockHash, requireCanonical:true})`.
Bootstrap headers are installed before the test observation; the observation
itself requests only `finalized` and the exact hash-bound proof. There is no
numeric-height, latest/pending, archive or provider fallback. The valid capture
tests a multi-node trie path; independent synthetic fixtures cover advanced
nonces, successful/reverted original receipts, foreign accounts, stale tips and
forked-out proofs. The observation always leaves inclusion unresolved; see the
[archive contract](../../docs/devnet-archive-v0.md).
