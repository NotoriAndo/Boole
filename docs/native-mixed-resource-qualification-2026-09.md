# Native mixed input-pressure qualification — 2026-09-23

Status: **PASS — preregistered small and large scenarios, existing runtime unchanged.** This combines specific already
bounded P2P and loopback HTTP inputs. It is not general mixed-traffic, successful
competing-adoption, continuously changing-candidate or public-network acceptance.

After the subsequent native HTTP shutdown correction, the unchanged small and
large scenarios passed again. The [corrected-source result and raw output](native-http-shutdown-drain-2026-09.md#unchanged-large-mixed-resource-follow-up--first-corrected-source-run-pass)
record 234.772s total, 623.875MiB RSS, 3.118s network phase, 39.786ms drained stop,
456µs maximum diagnostic response and 51.871s replay with exact original state.
The original measured source/results below remain unchanged and distinct.
A later [separate held-input shutdown scenario](native-http-shutdown-drain-2026-09.md#large-held-input-result--first-run-pass)
stopped before completing the 8/4/8 inputs, closed all twenty connections in
5.071s and independently replayed unchanged state at 610.375MiB RSS. Its fixed
seven-second held stop criterion does not replace this record's three-second
already-drained stop criterion.

## Why this combination

Separate qualifications exercised eight incomplete fork buffers, distinct valid
fork adoption, HTTP admission and independent diagnostics. Their separate passes
do not establish the resource cost of simultaneous pressure. The selected new
scenario retains the original limits and validation, and runs through the actual
combined native HTTP/P2P service. No new production API or trust shortcut is
required just to expose the state to the test.

Use the same developer Mac (`Mac16,9`, arm64, 16 logical CPUs, 64GiB RAM), Rust
1.95 debug executable, numeric loopback and disposable deterministic spending
keys/random transport keys. No operational keys/funds, VM, model/paid API, public
endpoint, GitHub publication, release or activation. Build outside measurement;
record source/tree/executable SHA-256 before and after. Run the large scenario
alone under `/usr/bin/time -l`, after its small companion passes.

## Fixed scenario and acceptance

1. Build the original ten empty blocks plus 256 blocks containing 512 signed
   one-atom transfers each to distinct recipient addresses. Require exactly
   131,073 canonical balance entries and original accounting. Construct a valid
   33-block competing suffix with 512 owner-to-self transfers per block, then
   retain only its suffix/hash vector and discard the extra complete chain.
   Extend the actual local chain by one empty block. Require its original exact
   height-267 head, accounting and canonical journal bytes.
2. Start the actual `serve_native_node_with_peers` router/service with eight
   mutually pinned TLS peers. Each remote sends three-block pages and pauses
   after the request proving the client received/decoded thirty blocks. All
   eight outgoing rounds must be active simultaneously, each under its 8MiB
   round bound. No incomplete candidate may change canonical state.
3. While these buffers are held, establish four authenticated incoming TLS
   connections, exchange valid native hellos and leave requests unfinished.
   Observe four active incoming workers. A fifth permitted identity must be
   rejected by the worker bound without a native hello, with a rejection
   counter increment; do not weaken TLS or configure an unknown key for this.
4. Admit eight actual `/native/chain` requests, each declaring exactly 8MiB,
   with `Expect: 100-continue`. After the real 100 response, send a valid empty
   array followed by whitespace, withholding the last byte of each body.
   Every client sends 8MiB minus one byte. Kernel socket buffers may retain some
   bytes: this is not an assertion that exactly 64MiB resides in parser heaps.
   At the overlap, diagnostic counters must show eight ordinary requests,
   eight outgoing rounds and four incoming workers within unchanged limits.
5. A ninth ordinary request must receive 429 without a 100 response. Ten
   sequential independent diagnostic requests must each return 200 within one
   second, and retain `authority=local_process_only` and
   `ledgerReadiness=not_checked`; they must not declare canonical readiness or
   include balances/head. Their separate budget and shared connection limits
   remain unchanged. The observed ordinary/incoming/outgoing counts must never
   exceed 8/4/8 during this held-input phase.
6. Complete the eight HTTP bodies. Each must return 200 with `adopted=false`,
   and all ordinary permits must return. Close the four inbound fixture
   connections and require incoming workers to drain. Release the final fork
   pages; each exceeds its existing round budget and must fail without adoption.
   Query ordinary info after pressure drains: exact prior head, resources,
   confirmed count and empty pending, with no journal changes.
7. Complete the network phase within 15 seconds and stop the drained combined
   service within 3 seconds. Join fixture workers; all listeners must be
   reusable. Independently reopen within 120 seconds and verify every recipient,
   owner balance/nonce, issued/locked supply, empty pending and byte-identical
   canonical/pending journals and manifest.

Maximum measured template/durable append/candidate-append phases stay at ten
seconds. Total scenario excluding build stays at 900 seconds, canonical history
at 96MiB and OS peak process RSS at 1GiB. Record timing, held-input byte counts,
diagnostic snapshots, rejection/drain outcomes, exact final accounting and OS
resource output. Keep failed outcomes and criteria unchanged. The small companion
uses two funded blocks (1,025 balance entries), the same full-size competing
suffix and the same 8/4/8 pressure levels and HTTP body sizes.

No success here qualifies arbitrary RPC JSON trees, local trusted-host compromise,
mixed successful fork validation, many independent operators, slower storage,
network-wide availability or the complete R1 launch gate.

## Small companion — first run PASS

The actual combined service passed at 1,025 balances in 21.671s (harness 21.68s).
All eight clients held thirty decoded blocks (8,188,312 fixture wire bytes each),
four incoming connections had authenticated and eight ordinary HTTP requests
held admission after sending 8,388,607 bytes each. The fifth inbound connection
and ninth ordinary request were refused; there was no HTTP 100 response for
the ninth upload. Ten diagnostic responses preserved the non-authoritative
markers, with maximum 442 microseconds under the overlapping 8/4/8 counts.

After completion/closure, all eight empty-chain requests returned adopted=false,
ordinary permits and inbound/outbound workers drained to zero, and all eight
over-budget fork rounds failed at block_download. Network phase was 918ms,
drained service stop 511 microseconds and independent replay 392ms. Exact
13-block/1,024-confirmed/552,737-byte canonical state and empty pending remained;
all journal digests and manifest bytes were unchanged. Maximum template,
canonical append and candidate append were 170, 199 and 177ms. All ports were
released. No production behavior changed, and no failed run preceded this pass.

The fixture can report more than 8MiB **sent** after the final excess page is
released (here 9,007,936 bytes): those include socket-buffered bytes offered to a
client that rejects its remaining round budget. This is not a claim that the
client accepted, decoded or retained the full excess body. Before final release,
all held buffers are below the existing round bound and no candidate is adopted.


## Large qualification — first run PASS

Executed on 2026-09-23, approximately 13:31–13:35 UTC. Source
`460bb191587ccd5787b7757242370d0d78bc245e`, tree
`ef12dd52d6716b54672897bbd356291bc0f04e88`, clean worktree and executable
SHA-256 `8bade0d46ea204fcc64d8d99be1ec9ddb20fb54d68039f5b4fba7ab80dda9352`
were identical before and after the run. No other build/test load was intentionally
run concurrently. Preregistered criteria from `51d6218` were unchanged; the
small companion, targeted test clippy with warnings denied, fmt, docs-smoke and
diff-check had passed before measurement.

All eight outgoing clients simultaneously held thirty decoded blocks, with
8,213,762 fixture wire bytes per peer before release. Four incoming TLS peers
were authenticated and held open. Each of eight admitted HTTP clients had sent
8,388,607 bytes toward its incomplete 8MiB body. The fifth incoming connection
was rejected; the ninth HTTP request received 429 without a 100 response.
Diagnostic snapshots and all ten timed observations retained 8/4/8 ordinary/
incoming/outgoing counts, the unchanged limits and non-authoritative markers.
Maximum timed diagnostic response was 457 microseconds.

The eight completed empty-array requests all returned `adopted=false`. Ordinary
permits and incoming workers drained, then all eight excess fork pages failed
at `block_download`; no candidate was adopted. The network phase took 3.137s,
normal drained service stop 39.025ms and independent reopen 51.652s. Scenario
elapsed time was 234.292s (harness/OS wall 234.33s). Maximum template, canonical
append and candidate append times were 199, 239 and 191ms. OS peak RSS was
651,444,224 bytes (621.265625MiB), under the fixed 1GiB criterion. Both process
and surrounding identity-check command exited 0.

The final original head remained
`000002700a9b5f795da3ff71cad2588c0180e2f6213ba0f0e983ea42b1b63d1b`,
with 267 blocks, 131,073 balance entries, 131,072 confirmed transfers, one nonce
entry, 69,958,251 history bytes, 1,335,000,000,000,000 issued atoms and empty pending.
Every recipient, owner balance/nonce, issued/locked amount, original journal
digest and manifest byte comparison passed before and after independent replay.
All local listeners were released. The network snapshots include expected
failures from deliberately closed incoming rounds and rejected oversized forks;
these are part of the scenario, not suppressed outcomes.

This qualifies exactly this simultaneous incomplete-input/rejection combination.
It does not replace the separate valid competing-adoption measurement, combine
its peak RSS arithmetically with this run, or establish arbitrary mixed traffic,
slow-disk, other-platform, public-network or complete R1 readiness. Runtime code,
limits and validation remain unchanged.

Direct measurement command:

```sh
/usr/bin/time -l /Users/seoyong/projects/Boole/target/debug/deps/native_capacity-6f26ba504621f258 --ignored --exact mixed::native_mixed_p2p_http_pressure_131072_accounts --nocapture --test-threads=1
```

Raw stdout/stderr and source/executable identity checks:

```text
460bb191587ccd5787b7757242370d0d78bc245e
ef12dd52d6716b54672897bbd356291bc0f04e88
8bade0d46ea204fcc64d8d99be1ec9ddb20fb54d68039f5b4fba7ab80dda9352  /Users/seoyong/projects/Boole/target/debug/deps/native_capacity-6f26ba504621f258

running 1 test
test mixed::native_mixed_p2p_http_pressure_131072_accounts ... mixed-progress fundedBlocks=32 elapsedMs=19155
mixed-progress fundedBlocks=64 elapsedMs=38155
mixed-progress fundedBlocks=96 elapsedMs=57359
mixed-progress fundedBlocks=128 elapsedMs=77226
mixed-progress fundedBlocks=160 elapsedMs=97341
mixed-progress fundedBlocks=192 elapsedMs=116740
mixed-progress fundedBlocks=224 elapsedMs=136535
mixed-progress fundedBlocks=256 elapsedMs=156706
mixed-network {"advertised":{"hash":"000092c6b9368f84167d9bdbe28083aaa3c3a54ec6f0886623daef36cd3ca5cc","height":299},"diagnosticMaxMicros":457,"drained":{"authority":"local_process_only","ledgerReadiness":"not_checked","peers":{"acceptedConnections":4,"activeInboundWorkers":0,"activeOutboundRounds":0,"authenticatedConnections":4,"authenticationFailures":0,"completedInboundRounds":0,"enabled":true,"failedInboundRounds":4,"limits":{"handshakeTimeoutMs":2000,"inboundKeyCooldownMs":500,"maxHandshakesPerSecond":8,"maxInboundWorkers":4,"maxMessageBytes":1048576,"maxOutboundWorkers":8,"maxPeers":8,"maxRoundBlocks":256,"maxRoundBytes":8388608,"maxRoundRequests":64,"roundTimeoutMs":10000},"listenAddress":"127.0.0.1:52971","localPeerId":"efabd88e0a93288ea7ad2f2e190740019154d558cfb198d043b97412ab52fea3","peakInboundWorkers":4,"peakOutboundRounds":8,"peers":[{"address":"127.0.0.1:52973","consecutiveFailures":1,"failedRounds":1,"lastFailureStage":"block_download","peerId":"1d33ae201dc1263466f0464d8242cb923274f8febe27a49878f176356da3a55f","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:52974","consecutiveFailures":1,"failedRounds":1,"lastFailureStage":"block_download","peerId":"6eb340d7ae63d38f812ecdffa5f23c07df41624a741112d5f1260d4cc93b12ba","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:52975","consecutiveFailures":1,"failedRounds":1,"lastFailureStage":"block_download","peerId":"e8e5673b3ac8db321daa5d81dd42af0f7ff88169f916553d4fc75ca75b0a9531","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:52976","consecutiveFailures":1,"failedRounds":1,"lastFailureStage":"block_download","peerId":"4633b1419aa4e509d8fd1f193cc0f6cb886ff8cbf278525f87af65e5eb5016ca","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:52977","consecutiveFailures":1,"failedRounds":1,"lastFailureStage":"block_download","peerId":"cb6da12d9db812f7815f38e75a519c02af9430354f47ea2e2a8f0031dffc62e0","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:52978","consecutiveFailures":1,"failedRounds":1,"lastFailureStage":"block_download","peerId":"fab1c99ec4d73a6510802e281d62e31e1617af88b0d68f8cb81b591444e15731","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:52979","consecutiveFailures":1,"failedRounds":1,"lastFailureStage":"block_download","peerId":"d94082c196813eaa1c9907159c8b363e214bae39501c0ca52f3e4ff5bb11835d","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:52980","consecutiveFailures":1,"failedRounds":1,"lastFailureStage":"block_download","peerId":"595afc2b0b289b7dc3df93d6539b4772d5ac5d2a6affcb72351458c92e0001d7","retryDelayMs":500,"state":"retrying","successfulRounds":0}],"rejectedConnections":1,"rejectedPeerRounds":0,"running":true},"rpc":{"activeDiagnostics":1,"activeRequests":0,"diagnosticLimit":2,"requestLimit":8},"schema":"boole.native.diagnostics.v1","stopping":false},"elapsedMs":181817,"finalSentBytesPerPeer":[9034930,9034930,9034930,9034930,9034930,9034930,9034930,9034930],"fundedBlocks":256,"httpBodyBytesSentPerClient":8388607,"localHead":"000002700a9b5f795da3ff71cad2588c0180e2f6213ba0f0e983ea42b1b63d1b","networkMs":3137,"overlap":{"authority":"local_process_only","ledgerReadiness":"not_checked","peers":{"acceptedConnections":4,"activeInboundWorkers":4,"activeOutboundRounds":8,"authenticatedConnections":4,"authenticationFailures":0,"completedInboundRounds":0,"enabled":true,"failedInboundRounds":0,"limits":{"handshakeTimeoutMs":2000,"inboundKeyCooldownMs":500,"maxHandshakesPerSecond":8,"maxInboundWorkers":4,"maxMessageBytes":1048576,"maxOutboundWorkers":8,"maxPeers":8,"maxRoundBlocks":256,"maxRoundBytes":8388608,"maxRoundRequests":64,"roundTimeoutMs":10000},"listenAddress":"127.0.0.1:52971","localPeerId":"efabd88e0a93288ea7ad2f2e190740019154d558cfb198d043b97412ab52fea3","peakInboundWorkers":4,"peakOutboundRounds":8,"peers":[{"address":"127.0.0.1:52973","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"1d33ae201dc1263466f0464d8242cb923274f8febe27a49878f176356da3a55f","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:52974","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"6eb340d7ae63d38f812ecdffa5f23c07df41624a741112d5f1260d4cc93b12ba","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:52975","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"e8e5673b3ac8db321daa5d81dd42af0f7ff88169f916553d4fc75ca75b0a9531","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:52976","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"4633b1419aa4e509d8fd1f193cc0f6cb886ff8cbf278525f87af65e5eb5016ca","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:52977","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"cb6da12d9db812f7815f38e75a519c02af9430354f47ea2e2a8f0031dffc62e0","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:52978","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"fab1c99ec4d73a6510802e281d62e31e1617af88b0d68f8cb81b591444e15731","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:52979","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"d94082c196813eaa1c9907159c8b363e214bae39501c0ca52f3e4ff5bb11835d","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:52980","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"595afc2b0b289b7dc3df93d6539b4772d5ac5d2a6affcb72351458c92e0001d7","retryDelayMs":0,"state":"not_connected","successfulRounds":0}],"rejectedConnections":1,"rejectedPeerRounds":0,"running":true},"rpc":{"activeDiagnostics":1,"activeRequests":8,"diagnosticLimit":2,"requestLimit":8},"schema":"boole.native.diagnostics.v1","stopping":false},"partialBytesPerPeer":[8213762,8213762,8213762,8213762,8213762,8213762,8213762,8213762],"stopMicros":39025}
mixed-result {"elapsedMs":234292,"fundedBlocks":256,"genesisHash":"933a672120674efa9ec6205f344e1830febdec58b0ffcd2603ccc8a9723610c1","head":"000002700a9b5f795da3ff71cad2588c0180e2f6213ba0f0e983ea42b1b63d1b","issued":"1335000000000000","maxAppendMs":239,"maxCandidateAppendMs":191,"maxTemplateMs":199,"networkId":"boole-native-testnet-1","resources":{"balanceEntries":131073,"confirmedTransfers":131072,"historyBlocks":267,"historyBytes":69958251,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":1,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0},"restartMs":51652}
ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 13 filtered out; finished in 234.33s

      234.33 real       235.50 user         0.65 sys
           651444224  maximum resident set size
                   0  average shared memory size
                   0  average unshared data size
                   0  average unshared stack size
               43207  page reclaims
                   0  page faults
                   0  swaps
                   0  block input operations
                   0  block output operations
                6327  messages sent
               10817  messages received
                   0  signals received
                1848  voluntary context switches
                3454  involuntary context switches
       3662687031190  instructions retired
       1019130338967  cycles elapsed
           446563288  peak memory footprint
460bb191587ccd5787b7757242370d0d78bc245e
ef12dd52d6716b54672897bbd356291bc0f04e88
8bade0d46ea204fcc64d8d99be1ec9ddb20fb54d68039f5b4fba7ab80dda9352  /Users/seoyong/projects/Boole/target/debug/deps/native_capacity-6f26ba504621f258
```
