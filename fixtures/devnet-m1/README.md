# Devnet M1 fixtures

Provenance: S1b local spike, 2026-10-20, op-reth v2.6.0, Foundry v1.8.5.

Genesis files are byte-for-byte copies. Recorded profiles contain all 35 main
and 2 basefee heights from replica A, independently matched by replicas B/C.
Logs reconstruct the ABI envelope with zero padding and txIndex = logIndex.
Recorded parent headers include the previous payload's raw L1-info deposit.
Cases preserve setup, continuation, expected inclusion/status and public test
addresses; no signing keys or Engine credentials are included.

The pure tests compare derivation, decisions, signers and hashes. Env-gated
driver tests replay the recorded roots and run fresh live A/B/C campaigns.
Set BOOLE_DEVNET_OP_RETH and BOOLE_DEVNET_ANVIL to executables matching pins.json.
Absent variables print skipping; mismatched executable digests fail.
These executable pins currently qualify darwin-arm64 only.
