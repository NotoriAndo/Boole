# Native repeated-fork qualification — 2026-09-23

Status: **PASS — the preregistered large scenario.** This is one synthetic closed-local
qualification of unchanged, fully verified losing-fork polls. It is separate from
the earlier [eight-peer buffer test](native-peer-resource-qualification-2026-09.md),
recent-fork adoption and full-pending capacity scenarios. No public operation,
general denial-of-service resistance or full-capacity claim follows.

## Fixed scenario and acceptance criteria

Record exact source, executable SHA-256, command, output and resource result after
execution. Preserve failures and criteria; do not reinterpret a failed run as a
pass. Use the existing developer Mac (`Mac16,9`, arm64, 16 logical CPUs, 64GiB RAM),
Rust 1.95 debug test executable and `/usr/bin/time -l` without concurrent local
build/test load. Compilation is excluded from the measured test process.

- Build one disposable native state from ten empty blocks and 256 blocks of 512
  real signed one-atom transfers to distinct recipients. Verify all 131,072
  recipient balances, sender nonce, issued supply, locked rewards and counts.
- From that common verified head, build an alternative 16-block suffix, each
  containing 512 valid owner-signed self-transfers. Validate it through the core.
  Keep 17 different empty blocks on the local canonical chain and assert the
  alternative loses normal cumulative-work/hash fork choice. Only the suffix
  and hash vector remain in the TLS fixture; discard its full candidate chain.
- One fixed mutually pinned numeric `127.0.0.1` peer advertises that same losing
  head in eight sequential rounds. First round serves all 16 blocks in pages of
  at most three, staying within unchanged 1MiB-message/8MiB-round limits. It must
  complete successfully within ten seconds, with the local chain unchanged.
- The next seven rounds must exchange hello/done only: zero hash, block or pending
  requests. They cannot reuse unverified work or change the local ledger. This
  proves omission of repeated download/validation, not a speedup inferred from
  a noisy timing difference. TLS authentication and readiness checks still run.
- After the first round, 100 direct readiness/resource queries finish within five
  seconds. All eight rounds plus these queries finish within 15 seconds.
- Before/after streaming journal digests and existence match. Canonical head,
  every balance, nonce, confirmed count, issuance and empty pending queue remain
  unchanged. Peer status has at least eight successful rounds and no failures
  before deliberate shutdown; worker count remains one.
- Stop drains outgoing workers within one second. Drop the service, fixture and
  original node, independently reopen within 120 seconds and repeat accounting,
  head and journal checks. This is ordinary genesis replay, not a cached audit.
- Each local template/durable append finishes within ten seconds. The entire
  scenario finishes within 900 seconds; canonical history is at most 96MiB and
  measured process peak RSS is at most 1GiB. RSS includes the TLS fixture.
- Only disposable deterministic test keys/state and loopback sockets are used.
  No actual operator key/funds, non-loopback bind/connect, VM/model/paid API,
  release, GitHub publication or activation occurs.

The small routine-CI harness uses two funded blocks but the same 16-block fork,
17-block canonical continuation and eight polls. The large test is ignored by
default and must be explicitly selected. Previously observed focused wire tests
already distinguish first validation, unchanged polls, both head changes,
restart, invalid signatures and storage loss; this measurement supplements them
with large canonical state, not a new validation bypass.

## Results

### Small harness — PASS before the large run

The two-funded-block scenario passed on its first execution: 1,025 balance
entries, 1,024 confirmed transfers, 29 canonical blocks and 565,202 history bytes.
The first round downloaded all 16 alternative blocks in six requests, took
3,166,316 microseconds and sent 4,364,479 server plaintext bytes. Each of the
next seven rounds requested no hashes/blocks/pending, sending only 250 server
bytes and 266 client bytes. Their 517–525ms times include the fixed 500ms poll
wait, not just computation. Eight rounds took 6.818s; 100 ready/resource queries
took 4,324 microseconds, stop 127 microseconds and independent restart 414ms.
Total was 19.636s; unchanged journals, head and accounting passed. This is the
small fixture, not a large-state or RSS result.

### Large attempt 1 — PASS

Preregistration/runtime commit `d454d55`; executed source
`184965786df8586f677bcb078f8022d10822b3e5`, tree
`952976c0429517d1b142f2e4ae842cca7a410c94`. Exact debug executable SHA-256
`e33dda8e471a348736d14ed8bf990143c11f20c0292718b7581eb341cc746fa1` was checked
before and after execution. Source was clean and unchanged during measurement.
The small harness, native peer17/HTTP3/processCLI3, node all-target clippy,
focused harness clippy, formatting, docs smoke and diff checks passed first.

Executed at approximately 09:10 UTC without other local build/test load:

```sh
/usr/bin/time -l /Users/seoyong/projects/Boole/target/debug/deps/native_capacity-6f26ba504621f258 \
  --ignored --exact native_repeated_losing_fork_131072_accounts \
  --nocapture --test-threads=1
```

| Fixed criterion | Observed |
|---|---|
| Canonical accounting and unchanged journals | All 131,072 one-atom recipients, 131,073 balance entries, sender nonce/count/supply and both journal digests/existence matched |
| First full losing-fork round ≤10s | 4.998709s; 16 blocks in six block requests and eight hash requests |
| Seven repeated polls | Each zero hash/block/pending requests; server/client plaintext 251/267 bytes per poll |
| Eight rounds + queries ≤15s | 8.637s |
| 100 ready/resource queries ≤5s | 4,322 microseconds |
| Stop ≤1s; worker drain | 166 microseconds; active outbound rounds zero |
| Successful/failed rounds before stop | 8 / 0; one peak outbound worker |
| Independent replay ≤120s | 51.791s, identical head/accounting/journals |
| Local template/append ≤10s each | Maximum 197ms / 229ms |
| Canonical history ≤96MiB | 283 blocks / 69,970,748 bytes |
| Total ≤900s | 231.194s scenario; 231.24s OS wall time |
| Process maximum RSS ≤1GiB | 503,398,400 bytes = 480.078125MiB, including the TLS fixture |

Repeated-round 514–525ms times include the fixed 500ms polling delay. No
large-state pre-change timing ratio is claimed: the earlier focused wire RED
directly observed repeated downloads, and this run proves they are absent for
this verified unchanged pair. It does not eliminate the first verification or
qualify continuously changing candidates, eight concurrent successful adoptions,
arbitrary traffic mixes, full configured storage capacity or other hardware.

Raw result records (progress chatter omitted; values unmodified):

```text
repeat-fork-network {"advertised":{"hash":"0000484b57ca8413eea1cd47c9f8d295ac2c376184d4eb2b0593e7105ad16864","height":282},"commonHash":"00009943584cb04f821a9cc0675de0f42b80de834a700e6d4b7ce92d10624b0a","commonHeight":266,"elapsedMs":178540,"fundedBlocks":256,"localHead":"0000fd8d93a6bc92369f6292a2ba580c18206bf19ba1e720ab5678d9756bc667","lookup100Micros":4322,"networkMs":8637,"peersBeforeStop":{"acceptedConnections":0,"activeInboundWorkers":0,"activeOutboundRounds":0,"authenticatedConnections":0,"authenticationFailures":0,"completedInboundRounds":0,"enabled":true,"failedInboundRounds":0,"limits":{"handshakeTimeoutMs":2000,"inboundKeyCooldownMs":500,"maxHandshakesPerSecond":8,"maxInboundWorkers":4,"maxMessageBytes":1048576,"maxOutboundWorkers":8,"maxPeers":8,"maxRoundBlocks":256,"maxRoundBytes":8388608,"maxRoundRequests":64,"roundTimeoutMs":10000},"listenAddress":"127.0.0.1:63175","localPeerId":"34967d7bbbe7c2f1b1d9c0601194764ca16f37721bc88925448c32eb83f06786","peakInboundWorkers":0,"peakOutboundRounds":1,"peers":[{"address":"127.0.0.1:63176","consecutiveFailures":0,"failedRounds":0,"peerId":"7b481d4f42ee7e32bf2ae7d96ac50b66f16b2aa370e0d1301918ca4bdfbf4721","retryDelayMs":500,"state":"local_chain_preferred","successfulRounds":8}],"rejectedConnections":0,"rejectedPeerRounds":0,"running":true},"rounds":[{"blockRequests":6,"elapsedMicros":4998709,"hashRequests":8,"receivedBytes":2178,"round":1,"sentBlocks":16,"sentBytes":4381531},{"blockRequests":0,"elapsedMicros":519818,"hashRequests":0,"receivedBytes":267,"round":2,"sentBlocks":0,"sentBytes":251},{"blockRequests":0,"elapsedMicros":524571,"hashRequests":0,"receivedBytes":267,"round":3,"sentBlocks":0,"sentBytes":251},{"blockRequests":0,"elapsedMicros":513988,"hashRequests":0,"receivedBytes":267,"round":4,"sentBlocks":0,"sentBytes":251},{"blockRequests":0,"elapsedMicros":518662,"hashRequests":0,"receivedBytes":267,"round":5,"sentBlocks":0,"sentBytes":251},{"blockRequests":0,"elapsedMicros":515914,"hashRequests":0,"receivedBytes":267,"round":6,"sentBlocks":0,"sentBytes":251},{"blockRequests":0,"elapsedMicros":518928,"hashRequests":0,"receivedBytes":267,"round":7,"sentBlocks":0,"sentBytes":251},{"blockRequests":0,"elapsedMicros":523515,"hashRequests":0,"receivedBytes":267,"round":8,"sentBlocks":0,"sentBytes":251}],"stopMicros":166}
repeat-fork-result {"elapsedMs":231194,"fundedBlocks":256,"genesisHash":"933a672120674efa9ec6205f344e1830febdec58b0ffcd2603ccc8a9723610c1","head":"0000fd8d93a6bc92369f6292a2ba580c18206bf19ba1e720ab5678d9756bc667","issued":"1415000000000000","maxAppendMs":229,"maxTemplateMs":197,"networkId":"boole-native-testnet-1","resources":{"balanceEntries":131073,"confirmedTransfers":131072,"historyBlocks":283,"historyBytes":69970748,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":1,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0},"restartMs":51791}
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 7 filtered out; finished in 231.23s

      231.24 real       225.72 user         0.60 sys
           503398400  maximum resident set size
                   0  average shared memory size
                   0  average unshared data size
                   0  average unshared stack size
               30948  page reclaims
                   0  page faults
                   0  swaps
                   0  block input operations
                   0  block output operations
                 366  messages sent
                 363  messages received
                   0  signals received
                1285  voluntary context switches
                3487  involuntary context switches
       3538818554000  instructions retired
        977448182516  cycles elapsed
           324305592  peak memory footprint
```
