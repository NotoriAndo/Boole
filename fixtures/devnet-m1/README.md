# Devnet M1 fixtures

Warning: the M1 inbox corrupts items above 65,536 bytes and is unsuitable for M2.

These recordings, including the later v1–v4 budget schedules, are frozen
historical inputs. The [v1.3 reset and live recordings](../devnet-m3/README.md)
supersede them for the active M3 profile. Replaying or regenerating an older
schedule requires its original client revision; no old JSON is rewritten.

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

`s1b-recorded-main-budget-v1.json` applies DEVNET-M1b's D12 cumulative gas budget
to the original recorded L1 inputs. Main heights 1–23 retain their exact S1b
attributes, block hashes and roots; case 9 at height 24 now forwards only its
first two 12,000,000-gas transactions. The missing nonce also changes subsequent
inclusion, which the new fixture records explicitly. Basefee remains unchanged.
The original S1b JSON/genesis files are preserved. The new file includes source
SHA-256, pinned client identities and acceptance provenance: recorded A/B/C
matched at every height, followed by live A/B/C finality/restart/reorg checks
and one further finalized block after each reorg (main 36, basefee 3).

To regenerate an ignored candidate, set `BOOLE_DEVNET_GENERATE_BUDGET` to an
output file and run the env-gated I1 test. I2 can read that candidate via
`BOOLE_DEVNET_BUDGET_CANDIDATE`. Normal tests require the accepted fixture's
recorded/live acceptance flags; candidate generation alone does not accept it.

The M2 v1.1 reset uses `s1b-recorded-schedule-budget-v3.json` for the same
budget-compliant transaction schedule with a config-seeded root v2. Its tracked
generator records the exact M2 config and digest. Original v1 and v2 files are
retained; M1 payload attributes and transaction occurrences are unchanged.
