# Devnet node durability evidence

`input-auth-v0.json` qualifies the journal's Ethereum input encoding on pinned
Foundry anvil v1.8.5. Three original headers, full typed receipts and transaction
bytes authenticate against the node's keccak hashes and both ordered trie roots.
The probe includes a non-inbox LOG0 before an inbox log and a reverted transaction.
Its tracked producer is `boole-devnet-driver/tests/input_auth_live.rs`; it emits
provenance and writes a candidate only after strict process/log cleanup.

This is an encoding probe, with an explicit synthetic genesis and public devnet
accounts. It is not an L2 round, an archive authority, or a light-client proof.
The portable archive test re-verifies every row without a node. Existing v1.3
consensus goldens and recordings remain unchanged.

M4b2 `audit-receipt-v1.json` is the complete unsigned T81 pure-audit receipt
for `input-auth-v0.json`, the devnet M2 configuration, and the deterministic
two-block in-memory L2 view in `boole-devnet-audit/tests/audit.rs`. The independent
head is the valid L1 ancestor at height 1; the receipt covers height 2. The test
compares every field, including decimal strings and deferred K4/environment
labels. It does not re-record or change any consensus golden or live tape.

The M2 receipt marks verdict, admission and fork checks `not-applicable`. Its
L2 view uses the derived timestamp and gas limit. These diagnostic checks do not
change v1.3 fold bytes or recordings.
