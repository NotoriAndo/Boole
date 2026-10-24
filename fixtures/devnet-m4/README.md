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
