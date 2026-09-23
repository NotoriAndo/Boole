# Native concurrent-peer resource qualification — 2026-09-23

Status: **PASS — the preregistered large scenario.** This record qualifies one synthetic
closed-local scenario, not public connectivity, general denial-of-service
resistance or operational capacity. It follows the independent-worker change
in `bff7bd5`; all non-loopback endpoints remain refused.

## Fixed scenario and criteria

These criteria are recorded before the large run. Failures remain failures;
corrections and retries must preserve their actual outputs and identify the
changed source or infrastructure. The earlier capacity and recent-fork records
remain separate and do not establish concurrent-download capacity.

- One private disposable node directory, deterministic test-only owner/producer,
  and eight distinct ephemeral TLS transport identities. Every bind/connect is
  numeric `127.0.0.1`; no operator key/state, VM, model, paid API or public action.
- Under the unchanged compiled native genesis and validation rules, mine ten
  empty blocks and 256 blocks of 512 valid transfers each to distinct recipients.
  Validate all 131,072 one-atom balances, sender nonce, supply, locked rewards and
  131,073 canonical balance entries using the existing capacity assertions.
- Build an independently valid 33-block competing suffix at the funded head,
  each block holding 512 owner-signed self-transfers. Keep a different single
  empty block on the local canonical branch. A hash-only declaration is not the
  source of this suffix: it is produced and appended through the real core.
- Eight pinned loopback TLS fixtures advertise that same valid competing chain.
  Each serves verified common-prefix hashes and pages of three actual suffix
  blocks. After 30 blocks, each fixture pauses so all eight partial fork buffers
  coexist; no fixture can supply a final winning candidate under the existing
  8MiB round wire budget. The remaining three blocks exceed that budget.
- All eight workers must reach the partial-fork pause; active/peak outbound
  rounds must be exactly eight. Their next oversized remainder must fail the
  round, not adopt or truncate a candidate. Peer keys/worker labels remain eight.
- During the pause, 100 direct ready/resource queries complete within five
  seconds. After rejection, readiness, canonical head, supply, every balance,
  nonce, confirmed count and empty pending queue must be unchanged. A streaming
  digest confirms that both canonical and pending journal bytes/existence are
  unchanged (an unused pending journal may legitimately be absent).
- The concurrent network phase completes within 15 seconds. Stopping the peer
  service completes within one second and drains all outbound rounds. Drop all
  fixture buffers/services, then independently reopen the node within 120 seconds
  and verify the unchanged head/accounting again.
- Each local template construction/durable append remains within ten seconds.
  Entire scenario completes within 900 seconds excluding compilation; canonical
  history stays within 96MiB. Process maximum RSS must be at most 1GiB as measured
  by `/usr/bin/time -l` around the exact debug test executable, not Cargo.
- The process measurement includes the eight local TLS fixture servers and their
  shared valid suffix. It is not a isolated-node RSS measurement. No concurrent
  local build/test load is intentionally scheduled. Temporary state is removed.

The small CI harness uses two funded blocks while retaining the same eight-peer,
30+3-block wire-budget path. The explicit large test uses 256 funded blocks.
Neither test approves nonlocal P2P, changes round/consensus limits or claims the
full 256MiB/100,000-block storage envelope, all traffic mixes or all hardware.

## Environment and execution record

Developer Mac (`Mac16,9`, arm64, 16 logical CPUs, 64GiB RAM), repository debug
profile and `rustc 1.95.0`. Record the exact source, executable SHA-256, command,
unmodified result JSON and OS resource output below after execution.

### Harness preparation, before the large run

The first small test stopped before opening any peer sockets: it assumed an empty
pending journal must already exist and `File::open` returned `NotFound` (test
exit101, 20.31s). The corrected comparison preserves absence as distinct from an
empty file; it does not create a file or relax any node validation. The pause is
placed when the next range request arrives, proving the client has decoded all
30 prior blocks, rather than merely observing server-side writes. The large
qualification has not run at this point and its criteria are unchanged.

The corrected small harness passed before the large run: eight simultaneous
partial forks, 8,188,312 server plaintext bytes per peer before the final page,
472ms network phase, 4,089µs for 100 ready/resource queries, 188µs shutdown,
397ms restart and 21.175s total. All eight peer rounds failed with no successful
round/adoption, and journal digests/accounting were unchanged. These figures are
the small 1,025-balance-entry fixture, not the large qualification or an RSS pass.

### Large attempt 1 — PASS

Preregistration commit `22f9f4e`; executed source
`cf3b9cbd94696822484e0617310e0555a8805dec`, tree
`4204899df57759469a3e0e076ce99d1efc245491`. The debug executable SHA-256 was
`2d03c5247c78b5f599c21104a285e94b3ab3d0dcd05aadad0e57e934791d24f9`, checked
both before and after the run. No runtime source changed during measurement.
The existing peer/HTTP/CLI tests, node all-target clippy, new small harness and
its focused clippy, formatting, docs smoke and diff checks passed beforehand.

Executed at approximately 08:21 UTC, without other local builds/tests:

```sh
/usr/bin/time -l /Users/seoyong/projects/Boole/target/debug/deps/native_capacity-6f26ba504621f258 \
  --ignored --exact native_eight_peer_forks_131072_accounts \
  --nocapture --test-threads=1
```

| Fixed criterion | Observed |
|---|---|
| 131,073 balances / 131,072 confirmed transfers | Exact match, all recipient balances checked |
| Eight simultaneous partial forks | 30 blocks each; 8,213,762 server plaintext bytes per peer before final page |
| Oversized remainder refused, no partial fork adoption | All eight rounds failed; journals/head/accounting unchanged |
| 100 direct ready/resource queries <5s | 4.433ms |
| Network phase <15s / stop <1s | 474ms / 188µs |
| Independent reopen <120s | 51.784s, exact head/accounting/digest match |
| Local template / append <10s each | Maxima 196ms / 233ms |
| Entire scenario <900s | 231.984s internal; 232.03s OS wall time |
| Canonical journal ≤96MiB | 69,958,251 bytes |
| Process maximum RSS ≤1GiB | 578,715,648 bytes = 551.90625MiB |

Unmodified network result (addresses and public keys are ephemeral test fixtures):

```json
{"advertised":{"hash":"000092c6b9368f84167d9bdbe28083aaa3c3a54ec6f0886623daef36cd3ca5cc","height":299},"commonHash":"00009943584cb04f821a9cc0675de0f42b80de834a700e6d4b7ce92d10624b0a","commonHeight":266,"elapsedMs":179331,"fundedBlocks":256,"localHead":"000002700a9b5f795da3ff71cad2588c0180e2f6213ba0f0e983ea42b1b63d1b","lookup100Micros":4433,"networkMs":474,"partialBytesPerPeer":[8213762,8213762,8213762,8213762,8213762,8213762,8213762,8213762],"peers":{"acceptedConnections":0,"activeInboundWorkers":0,"activeOutboundRounds":0,"authenticatedConnections":0,"authenticationFailures":0,"completedInboundRounds":0,"enabled":true,"failedInboundRounds":0,"limits":{"handshakeTimeoutMs":2000,"inboundKeyCooldownMs":500,"maxHandshakesPerSecond":8,"maxInboundWorkers":4,"maxMessageBytes":1048576,"maxOutboundWorkers":8,"maxPeers":8,"maxRoundBlocks":256,"maxRoundBytes":8388608,"maxRoundRequests":64,"roundTimeoutMs":10000},"listenAddress":"127.0.0.1:56459","localPeerId":"4108a54a6528cc872ff109bfc1d1e0eddd48b02b05a6499762ed184099292eda","peakInboundWorkers":0,"peakOutboundRounds":8,"peers":[{"address":"127.0.0.1:56460","consecutiveFailures":1,"failedRounds":1,"peerId":"e91e41b95b82534df03a9deb9c3c23c8fb9179a225ef323cf12a6a58fbc2b0d3","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:56461","consecutiveFailures":1,"failedRounds":1,"peerId":"ff8857582f38858bdd2c5c2c5affcdb3a38c7e3df6c973712704f1189e133984","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:56462","consecutiveFailures":1,"failedRounds":1,"peerId":"b6c3d36a1aac3827fbcc16b1cbcc78e6f2c046dbfa418548af339625efa8a127","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:56463","consecutiveFailures":1,"failedRounds":1,"peerId":"80aaed1410585c9d263ec51ff72aae0956b82ef49db889ea637ce7e8649f5f4e","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:56464","consecutiveFailures":1,"failedRounds":1,"peerId":"289bbab84c8cb9efee93d2f607015784333bd480e0c08964cb90b2555725d1e5","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:56465","consecutiveFailures":1,"failedRounds":1,"peerId":"cade3af520c5d9e9a560458295ce48f3028c2e1226ad9f16efe336ae7805f9ca","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:56466","consecutiveFailures":1,"failedRounds":1,"peerId":"80540c11a4a2bc5955f6a1c6f638c2eaa7c285644fa492dcac80d245a69da2d1","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:56467","consecutiveFailures":1,"failedRounds":1,"peerId":"cb3e4aa3eec45f57f984087b4476bc481063c6cbd4ead0af99547f5d54d66096","retryDelayMs":500,"state":"retrying","successfulRounds":0}],"rejectedConnections":0,"rejectedPeerRounds":0,"running":false},"stopMicros":188}
```

Unmodified final result:

```json
{"elapsedMs":231984,"fundedBlocks":256,"genesisHash":"933a672120674efa9ec6205f344e1830febdec58b0ffcd2603ccc8a9723610c1","head":"000002700a9b5f795da3ff71cad2588c0180e2f6213ba0f0e983ea42b1b63d1b","issued":"1335000000000000","maxAppendMs":233,"maxTemplateMs":196,"networkId":"boole-native-testnet-1","resources":{"balanceEntries":131073,"confirmedTransfers":131072,"historyBlocks":267,"historyBytes":69958251,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":1,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0},"restartMs":51784}
```

Complete OS resource output; command exit code **0**:

```text
      232.03 real       233.34 user         0.60 sys
           578715648  maximum resident set size
                   0  average shared memory size
                   0  average unshared data size
                   0  average unshared stack size
               42308  page reclaims
                   0  page faults
                   0  swaps
                   0  block input operations
                   0  block output operations
                4784  messages sent
                4728  messages received
                   0  signals received
                1561  voluntary context switches
                2360  involuntary context switches
       3642693987385  instructions retired
       1008536757434  cycles elapsed
           371802928  peak memory footprint
```

Temporary state was cleaned normally. The result is a bounded incomplete-fork
buffer/rejection scenario with one canonical state owner. It does not qualify
eight simultaneous successful large-state adoptions, inbound/outbound mixed
traffic, malicious local filesystems, slower hardware or public operation.
