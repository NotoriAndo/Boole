# Native transport-key retirement and explicit reenrollment — 2026-09-23

Status: **PASS — preregistered actual process regression; existing runtime unchanged.** This is a
disposable one-host loopback membership/recovery exercise, not external operator
enrollment, public connectivity, operational key custody or R2/R3 completion.

## Selection and fixed scope

The selected initial network uses explicit transport-public-key membership.
The current configuration is fixed at process start: replacing a private-key
file or editing an external list does not revoke a running process's loaded
identity or already authenticated connections. The safe existing workflow is
coordinated normal stop, exact mapping update and restart, with independent
approval of the replacement public key. No hot-reload or automatic enrollment
feature is introduced by this test.

Use three actual `boole-node run-native-local` child processes, independent state
directories and RPC/P2P endpoints, and transport keys generated through the real
`peer-keygen` CLI. Bind only numeric loopback. Spending/producer keys are separate
deterministic fixture keys; no operator vault, private key, funds, VM, paid model,
public endpoint, release or external publication is involved. Preserve old and
new fixture key files and journal evidence throughout the scenario; discard only
the test's owned temporary directory when the entire test exits.

Acceptance is fixed before execution:

1. A/B/C initially authenticate reciprocally and reach the same real ten-block
   history, with rewards owned by a spending key separate from every transport
   identity. Each process reports two approved peers and successful rounds.
2. Stop B, establish an authenticated old-B connection to A, then normally stop
   A/C and verify that connection closes. Restart A/C with each other only.
   Restart old B without changing its state: it cannot authenticate at A/C, and
   its canonical history remains at the old head. Direct old-key probes must
   fail with increased server-side authentication-failure observations.
3. Stop B and generate its replacement transport key at a new path. Start B with
   the new identity and old state before updating A/C: it remains unapproved.
   A/C still propagate and confirm an actual owner-signed transfer while B
   cannot learn that new block through the denied connections.
4. Explicitly update A/C's B public-key mapping and normally restart them. B
   rejoins at the preserved state path, catches up to the intended head and
   reports reciprocal successful rounds. No old transport ID remains configured.
   Retired-key probes remain refused, and resubmitting the same confirmed signed
   transfer does not debit twice. No spending key/address is rotated.
5. Submit a second owner-signed transfer from the new-B process at nonce 1.
   Its new transport identity propagates it to A/C; confirm at height 12. All
   three processes show owner nonce 2, recipient balance 200,000,000 atoms and
   no pending transactions. Stop all nodes normally and independently audit
   the exact intended head: issued/balance total 60,000,000,000,000 atoms, two
   confirmed transfers totaling 200,000,000 amount atoms and 2,000 fee atoms.
   The complete canonical journals agree and retain the initial byte prefix;
   manifests and both old/new key files remain unchanged. All ports are released.

Each observable phase must complete within 15 seconds, each normal process stop
within 3 seconds and the complete small scenario within 120 seconds excluding
compilation. These are focused developer-Mac test bounds, not public latency,
large-state shutdown or adversarial-host guarantees. Preserve every failed
outcome and its classification; do not relax criteria after observing it.

This tests implemented behavior first. A first-pass regression is legitimate
evidence for an existing feature; do not break the implementation to invent RED.
If a behavioral failure appears, reproduce/fix it separately with the normal
RED → GREEN loop and repeat the unchanged acceptance criteria.


## Executed results — PASS, existing runtime unchanged

The first regression passed in 8.364s (test harness 8.37s) on the existing runtime
from `161999c`. Normal stops took 12–20ms; the longest membership phase was
2.074s. No runtime defect was manufactured or claimed. A test-only strengthening
then explicitly required A and C's server-side authentication-failure counters
to increase for the unapproved new identity. That version passed in 6.655s
(harness 6.66s), with normal stops 11–19ms and every phase within 532ms.
Both retained the original 15s/3s/120s criteria and identical deterministic
canonical head `000006c6b50e64fc92c8c5de8bd13f44a212110d2fd3270df7d303487649d769`.
Transport IDs are random disposable fixtures, not enrolled external identities.

All three independent source-preserving audits agreed at height 12: three
balance entries, two confirmed transfers, nonce entries 1, 10,396 canonical
history bytes and empty pending. Issued/balance total was 60,000,000,000,000 atoms,
including 50,000,000,000,000 locked and 10,000,000,000,000 spendable atoms, with
ten pending reward entries. The owner had nonce 2 and balance 49,999,799,998,000
atoms; the recipient held exactly 200,000,000 atoms. Both repeat submissions
stayed confirmed without a third debit. Initial journal prefix, manifests and
old/new transport files were preserved; final ports were reusable.

The change adds this regression and operator/participant guidance only. It does
not alter production trust, signature, monetary, file-durability or fork-choice
logic. The TDD method influenced the result by testing the real existing process
boundary first, without adding a new mechanism merely to produce a code change.

Command (compilation excluded from scenario time):

```sh
CARGO_TARGET_DIR=/Users/seoyong/projects/Boole/target cargo test -p boole-node --test native_peer_rotation -- --nocapture --test-threads=1
```

Raw strengthened-regression output:

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.80s
     Running tests/native_peer_rotation.rs (/Users/seoyong/projects/Boole/target/debug/deps/native_peer_rotation-5c23921ecc6a47a1)

running 1 test
test stopped_configuration_revokes_old_transport_key_and_explicit_reenrollment_preserves_ledger ... native-peer-rotation-phase process-ready elapsedMs=28
native-peer-rotation-phase process-ready elapsedMs=29
native-peer-rotation-phase process-ready elapsedMs=29
native-peer-rotation-phase head-agreement elapsedMs=72
native-peer-rotation-phase reciprocal-membership elapsedMs=439
native-peer-rotation-stop elapsedMs=19
native-peer-rotation-phase old-key-live-authenticated-connection elapsedMs=10
native-peer-rotation-stop elapsedMs=11
native-peer-rotation-stop elapsedMs=16
native-peer-rotation-phase process-ready elapsedMs=36
native-peer-rotation-phase process-ready elapsedMs=35
native-peer-rotation-phase reciprocal-membership elapsedMs=490
native-peer-rotation-phase process-ready elapsedMs=36
native-peer-rotation-phase unapproved-identity-refused elapsedMs=0
native-peer-rotation-phase retired-key-authentication-failure elapsedMs=0
native-peer-rotation-phase retired-key-authentication-failure elapsedMs=0
native-peer-rotation-stop elapsedMs=17
native-peer-rotation-phase process-ready elapsedMs=35
native-peer-rotation-phase unapproved-identity-refused elapsedMs=0
native-peer-rotation-phase replacement-key-server-authentication-refusal elapsedMs=0
native-peer-rotation-phase healthy-peer-transfer-propagation elapsedMs=393
native-peer-rotation-phase head-agreement elapsedMs=253
native-peer-rotation-stop elapsedMs=16
native-peer-rotation-stop elapsedMs=16
native-peer-rotation-phase process-ready elapsedMs=35
native-peer-rotation-phase process-ready elapsedMs=35
native-peer-rotation-phase head-agreement elapsedMs=524
native-peer-rotation-phase reciprocal-membership elapsedMs=520
native-peer-rotation-phase retired-key-authentication-failure elapsedMs=0
native-peer-rotation-phase retired-key-authentication-failure elapsedMs=0
native-peer-rotation-phase replacement-peer-transfer-propagation elapsedMs=510
native-peer-rotation-phase head-agreement elapsedMs=431
native-peer-rotation-phase reciprocal-membership elapsedMs=532
native-peer-rotation-stop elapsedMs=17
native-peer-rotation-stop elapsedMs=16
native-peer-rotation-stop elapsedMs=16
native-peer-rotation-result {"audit":{"accounting":{"balanceAtoms":"60000000000000","issuedAtoms":"60000000000000","lockedAtoms":"50000000000000","pendingRewardEntries":10,"spendableAtoms":"10000000000000","supplyCapAtoms":"100000000000000000"},"confirmedTransfers":{"amountAtoms":"200000000","count":2,"feeAtoms":"2000"},"genesisHash":"933a672120674efa9ec6205f344e1830febdec58b0ffcd2603ccc8a9723610c1","headHash":"000006c6b50e64fc92c8c5de8bd13f44a212110d2fd3270df7d303487649d769","height":"12","networkId":"boole-native-testnet-1","resources":{"balanceEntries":3,"confirmedTransfers":2,"historyBlocks":12,"historyBytes":10396,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":1,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0},"schema":"boole.native.audit.v1","scope":"confirmed_canonical"},"closedOldConnection":true,"elapsedMs":6655,"explicitReenrollment":true,"head":"000006c6b50e64fc92c8c5de8bd13f44a212110d2fd3270df7d303487649d769","ownerPk":"6738a145e4568df7963726a3920ba15b79873afd1d59783a006bec94202cb89e","preservedOriginalKey":true,"replacementPeerId":"5ef8e857c1cdbb051b8f31a3d5e2252836db548d2424a540a4bdf510fab2fae1","retiredPeerId":"02e215b2334ad0dea45c4b9ea37625e36015493d61db8e299d7d461504bf01ca"}
ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 6.66s
```
