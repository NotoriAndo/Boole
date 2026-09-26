# Native concurrent-candidate convergence — 2026-09-23

Status: **PASS — the preregistered large scenario.** This qualification covers eight
fixed authenticated peers concurrently providing the same valid winning fork.
It is separate from incomplete-fork buffer rejection and repeated losing-fork
polls. It does not qualify eight distinct successful reorgs, continuously
changing adversarial candidates, arbitrary inbound/RPC mixes or public operation.

## Fixed scenario and acceptance criteria

Record source/tree/executable SHA-256, exact command, raw output and resources.
Preserve every failed attempt and these bounds. Use the existing developer Mac
(`Mac16,9`, arm64, 16 logical CPUs, 64GiB RAM), Rust 1.95 debug test executable
and `/usr/bin/time -l`, with no concurrent local build/test load. Compilation is
outside the measured process. Only disposable keys/state and loopback are used;
there is no operator state/key access, non-loopback operation, VM/model/paid API,
release, GitHub publication or activation.

- Build ten empty blocks plus 256 blocks containing 512 owner-signed one-atom
  transfers to distinct recipients. Verify all 131,072 recipient balances,
  131,073 balance entries, sender nonce, issued/locked supply and confirmed count.
- From that common verified head, construct and core-validate a 16-block fork,
  each block containing 512 valid signed owner-to-self transfers with consecutive
  nonces. On the local node instead append one empty block. Assert the candidate
  wins normal cumulative-work/hash fork choice. Retain only the candidate suffix
  and hash vector in the eight TLS fixtures, not eight full chain clones.
- All eight independently pinned numeric `127.0.0.1` peers advertise this same
  candidate and send pages of at most three blocks. Pause each after the client
  requests the final page, proving all eight clients hold the preceding fifteen
  blocks simultaneously. All eight outgoing workers must be active; no journals,
  accounting or head may have changed before release.
- Release all final pages together. The node must converge to the advertised
  head once, with exactly 8,192 additional confirmed self-transfers and the
  candidate's supply/height/nonce, not eight times that accounting. Candidate
  data from a changed local snapshot must be abandoned safely; subsequent rounds
  with that same candidate must match the new head without requesting blocks.
- All eight peer statuses must reach a successful `snapshot_match` within 15
  seconds of starting the network phase. Each peer's first round sends exactly
  sixteen blocks; subsequent rounds send zero blocks. Seven initial snapshot
  races are expected, not evidence of invalid blocks or network authentication
  failure. One initial round completes, the other seven retry after head change.
- After releasing the final pages, 100 direct readiness/resource queries finish
  within ten seconds and observe only the original or the complete candidate
  head. This bound allows the shared validation/publication lock; it is not a
  reserved read-availability guarantee or an HTTP request latency measurement.
  Queries start immediately on release and need not overlap the long validation
  section; a fast observed result cannot establish worst-case lock-wait latency.
- After convergence, stop drains workers within one second. Recheck every
  recipient balance, nonce, supply, locks and confirmed count; pending is empty.
  Save both journal digests/existence. Drop service, node and fixtures, reopen
  independently within 120 seconds and require identical accounting/head/journals.
- Each local template/append finishes within ten seconds. Total scenario is at
  most 900 seconds, canonical history at most 96MiB and process peak RSS at most
  1GiB, including all eight TLS fixtures and simultaneous candidate buffers.

The small routine-CI case uses two funded blocks but the same sixteen-block
fork, eight-peer barrier and exact accounting checks. The large case is ignored
by default. Existing validation, fork choice, mutation barrier, publication and
protocol limits remain unchanged unless an actual failure motivates a separately
recorded RED/fix; no acceptance criterion may be relaxed after measurement.

## Results

### Small harness — PASS before the large run

The first small execution passed in 18.60s. All eight peers held fifteen blocks
(4,091,490 server plaintext bytes per peer) before final-page release. Each sent
six block pages/sixteen blocks, about 4.36MiB total. Exactly one initial round
completed; seven closed after the competing publication and each next round
completed with one empty pending request and no hash/block requests. All eight
statuses became `snapshot_match` within 4.132s. Stop took 120 microseconds and
workers drained to zero. The 100 immediate post-release queries took 4,737
microseconds; they were not forced to wait behind the subsequent long validation
section, so this is not a worst-case read-latency result.

The adopted state has 28 blocks, 1,025 balance entries and 9,216 confirmed
transfers: the original 1,024 plus 8,192 self-transfers, not eight copies. Sender
nonce, issued/locked amounts, every recipient and empty pending queue matched.
The 4,914,356-byte canonical journal and accounting matched after independent
3.454s replay. No runtime change was needed; this is regression/qualification
coverage of existing behavior, not an invented RED.

### Large attempt 1 — PASS

Preregistration commit `6f94bea`; executed source
`171dabc0a13a0f854c72b0d88a6df29279ca14ce`, tree
`6279e2cb08877ade8cd11dea3c4fbcbf5f38defe`. The clean, unchanged debug
executable SHA-256 was checked before and after measurement:
`fbec8c98fb961970dc35e190f8f6fe7a79b2605e880007c8acc8808f5a1d9e72`.
The small scenario, focused test-target clippy, formatting, docs smoke and diff
checks passed first. No runtime changed in this qualification branch.

Executed at approximately 09:52–09:56 UTC without another local build/test load:

```sh
/usr/bin/time -l /Users/seoyong/projects/Boole/target/debug/deps/native_capacity-6f26ba504621f258 \
  --ignored --exact native_eight_winning_candidates_131072_accounts \
  --nocapture --test-threads=1
```

| Fixed criterion | Observed |
|---|---|
| Eight simultaneous candidates | Each held fifteen blocks / 4,107,917 server plaintext bytes before the final page; peak outgoing workers 8 |
| Initial completion / snapshot races | Exactly 1 / 7; the initial successful round took 7.764632s |
| All peers `snapshot_match` ≤15s | 8.292s; every subsequent round requested zero hashes/blocks |
| Once-only canonical accounting | 282 blocks, 139,264 confirmed transfers = 131,072 original + 8,192 new; 131,073 balances, one sender nonce entry, exact supply/locks |
| 100 post-release queries ≤10s | 4,731 microseconds; not forced into the long critical section, so not a worst-case lock-wait bound |
| Stop ≤1s / drain | 252 microseconds / zero active outgoing workers |
| Independent replay ≤120s | 55.040s; identical head, balances, nonce, supply, locks and journal digests/existence |
| Local template/append ≤10s each | Maximum 197ms / 230ms |
| Canonical history ≤96MiB | 74,336,285 bytes |
| Total ≤900s | 232.459s scenario / 232.50s OS wall time |
| Peak RSS ≤1GiB | 571,146,240 bytes = 544.6875MiB, including all TLS fixtures |

This provides existing-behavior convergence/recovery evidence, not a measured
performance improvement. Seven failed first rounds are the expected stale-local-
snapshot cancellation after another worker wins, not seven invalid candidates.
The subsequent ~516–522ms round times include the 500ms polling delay. Full
history replay still costs 55s here; mixed traffic, distinct competing forks,
worst-case read availability and public operation remain unqualified.

Raw result records (progress chatter omitted; values unmodified):

```text
candidate-network {"advertised":{"hash":"0000484b57ca8413eea1cd47c9f8d295ac2c376184d4eb2b0593e7105ad16864","height":282},"commonHash":"00009943584cb04f821a9cc0675de0f42b80de834a700e6d4b7ce92d10624b0a","commonHeight":266,"elapsedMs":176515,"fundedBlocks":256,"heldBytesPerPeer":[4107917,4107917,4107917,4107917,4107917,4107917,4107917,4107917],"initialHead":"000002700a9b5f795da3ff71cad2588c0180e2f6213ba0f0e983ea42b1b63d1b","lookup100Micros":4731,"networkMs":8292,"peersBeforeStop":{"acceptedConnections":0,"activeInboundWorkers":0,"activeOutboundRounds":0,"authenticatedConnections":0,"authenticationFailures":0,"completedInboundRounds":0,"enabled":true,"failedInboundRounds":0,"limits":{"handshakeTimeoutMs":2000,"inboundKeyCooldownMs":500,"maxHandshakesPerSecond":8,"maxInboundWorkers":4,"maxMessageBytes":1048576,"maxOutboundWorkers":8,"maxPeers":8,"maxRoundBlocks":256,"maxRoundBytes":8388608,"maxRoundRequests":64,"roundTimeoutMs":10000},"listenAddress":"127.0.0.1:52714","localPeerId":"bcb19d41129f1ac39137ef0db1be02f5862233df3029f56fa6b95bbfab9e5d14","peakInboundWorkers":0,"peakOutboundRounds":8,"peers":[{"address":"127.0.0.1:52715","consecutiveFailures":0,"failedRounds":1,"peerId":"110db6215d1a94d8d9fdfa957fe9187f47d5457e5567052d85964b401ea07443","retryDelayMs":500,"state":"snapshot_match","successfulRounds":1},{"address":"127.0.0.1:52716","consecutiveFailures":0,"failedRounds":1,"peerId":"fd34f541b78981c8ac54b79dc7caba2f5f75a9f9f7ef917033d2978148c7c77b","retryDelayMs":500,"state":"snapshot_match","successfulRounds":1},{"address":"127.0.0.1:52717","consecutiveFailures":0,"failedRounds":1,"peerId":"3de5cc8218e263ead63ebfbd88ca8c164e5796b2186bd6ff1e9a8f7b8b53b179","retryDelayMs":500,"state":"snapshot_match","successfulRounds":1},{"address":"127.0.0.1:52718","consecutiveFailures":0,"failedRounds":1,"peerId":"db7f88d6f312dd065b7048a8c35923d69cd7f736c88e410ece3142dfc6697479","retryDelayMs":500,"state":"snapshot_match","successfulRounds":1},{"address":"127.0.0.1:52719","consecutiveFailures":0,"failedRounds":0,"peerId":"6e05d165134c742a6d1ede8ef02aa2407c73c96b5feae57c06e775655b367eb3","retryDelayMs":500,"state":"snapshot_match","successfulRounds":2},{"address":"127.0.0.1:52720","consecutiveFailures":0,"failedRounds":1,"peerId":"58808e2ed274c634bb0ef6254dce98cc7c0b702c69fc8f8f34622db32f052d0f","retryDelayMs":500,"state":"snapshot_match","successfulRounds":1},{"address":"127.0.0.1:52721","consecutiveFailures":0,"failedRounds":1,"peerId":"de7b00b0d7dfa0cd671fd0524732b4601b71716554b2f8a3459a2270c9196cec","retryDelayMs":500,"state":"snapshot_match","successfulRounds":1},{"address":"127.0.0.1:52722","consecutiveFailures":0,"failedRounds":1,"peerId":"6f9a04919f73ee75f1e2142dc87977cf66f6200e54a639a111ec76dc71704b19","retryDelayMs":500,"state":"snapshot_match","successfulRounds":1}],"rejectedConnections":0,"rejectedPeerRounds":0,"running":true},"rounds":[[{"blockRequests":6,"completed":false,"elapsedMicros":7772060,"hashRequests":9,"peer":0,"pendingRequests":0,"receivedBytes":2294,"round":1,"sentBlocks":16,"sentBytes":4381734},{"blockRequests":0,"completed":true,"elapsedMicros":518593,"hashRequests":0,"peer":0,"pendingRequests":1,"receivedBytes":412,"round":2,"sentBlocks":0,"sentBytes":406}],[{"blockRequests":6,"completed":false,"elapsedMicros":7771527,"hashRequests":9,"peer":1,"pendingRequests":0,"receivedBytes":2294,"round":1,"sentBlocks":16,"sentBytes":4381734},{"blockRequests":0,"completed":true,"elapsedMicros":516515,"hashRequests":0,"peer":1,"pendingRequests":1,"receivedBytes":412,"round":2,"sentBlocks":0,"sentBytes":406}],[{"blockRequests":6,"completed":false,"elapsedMicros":7771286,"hashRequests":9,"peer":2,"pendingRequests":0,"receivedBytes":2294,"round":1,"sentBlocks":16,"sentBytes":4381734},{"blockRequests":0,"completed":true,"elapsedMicros":517065,"hashRequests":0,"peer":2,"pendingRequests":1,"receivedBytes":412,"round":2,"sentBlocks":0,"sentBytes":406}],[{"blockRequests":6,"completed":false,"elapsedMicros":7771001,"hashRequests":9,"peer":3,"pendingRequests":0,"receivedBytes":2294,"round":1,"sentBlocks":16,"sentBytes":4381734},{"blockRequests":0,"completed":true,"elapsedMicros":518621,"hashRequests":0,"peer":3,"pendingRequests":1,"receivedBytes":412,"round":2,"sentBlocks":0,"sentBytes":406}],[{"blockRequests":6,"completed":true,"elapsedMicros":7764632,"hashRequests":9,"peer":4,"pendingRequests":1,"receivedBytes":2455,"round":1,"sentBlocks":16,"sentBytes":4381889},{"blockRequests":0,"completed":true,"elapsedMicros":522215,"hashRequests":0,"peer":4,"pendingRequests":1,"receivedBytes":412,"round":2,"sentBlocks":0,"sentBytes":406}],[{"blockRequests":6,"completed":false,"elapsedMicros":7770679,"hashRequests":9,"peer":5,"pendingRequests":0,"receivedBytes":2294,"round":1,"sentBlocks":16,"sentBytes":4381734},{"blockRequests":0,"completed":true,"elapsedMicros":517843,"hashRequests":0,"peer":5,"pendingRequests":1,"receivedBytes":412,"round":2,"sentBlocks":0,"sentBytes":406}],[{"blockRequests":6,"completed":false,"elapsedMicros":7769962,"hashRequests":9,"peer":6,"pendingRequests":0,"receivedBytes":2294,"round":1,"sentBlocks":16,"sentBytes":4381734},{"blockRequests":0,"completed":true,"elapsedMicros":515672,"hashRequests":0,"peer":6,"pendingRequests":1,"receivedBytes":412,"round":2,"sentBlocks":0,"sentBytes":406}],[{"blockRequests":6,"completed":false,"elapsedMicros":7769874,"hashRequests":9,"peer":7,"pendingRequests":0,"receivedBytes":2294,"round":1,"sentBlocks":16,"sentBytes":4381734},{"blockRequests":0,"completed":true,"elapsedMicros":516484,"hashRequests":0,"peer":7,"pendingRequests":1,"receivedBytes":412,"round":2,"sentBlocks":0,"sentBytes":406}]],"stopMicros":252}
candidate-result {"elapsedMs":232459,"fundedBlocks":256,"genesisHash":"933a672120674efa9ec6205f344e1830febdec58b0ffcd2603ccc8a9723610c1","head":"0000484b57ca8413eea1cd47c9f8d295ac2c376184d4eb2b0593e7105ad16864","issued":"1410000000000000","maxAppendMs":230,"maxTemplateMs":197,"networkId":"boole-native-testnet-1","resources":{"balanceEntries":131073,"confirmedTransfers":139264,"historyBlocks":282,"historyBytes":74336285,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":1,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0},"restartMs":55040}
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 9 filtered out; finished in 232.50s

      232.50 real       231.69 user         0.64 sys
           571146240  maximum resident set size
                   0  average shared memory size
                   0  average unshared data size
                   0  average unshared stack size
               37223  page reclaims
                   0  page faults
                   0  swaps
                   0  block input operations
                   0  block output operations
                2523  messages sent
                2662  messages received
                   0  signals received
                1540  voluntary context switches
                4345  involuntary context switches
       3613822126283  instructions retired
       1002875175653  cycles elapsed
           325386936  peak memory footprint
```
