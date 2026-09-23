# Native HTTP shutdown drain — 2026-09-23

Status: **Correction, direct regressions, drained mixed pressure and held-input large shutdown PASS.**

## Selected boundary

Static review found that native HTTP awaits Axum's complete graceful drain,
including incomplete request bodies and slow response receivers, before crossing
the existing durable-mutation shutdown barrier. Its ten-second request deadline
does not cover a response after the handler returns. Axum 0.8.9 spawns connection
tasks; merely dropping the outer server future does not demonstrate that those
tasks released connections and the node's state ownership.

The selected requirement is five seconds of graceful HTTP drain after normal
shutdown, followed by closure of this server's remaining client I/O. The server
must still wait for already admitted durable mutations to finish; this is not
permission to interrupt validation or disk publication. New mutation admission
and peer I/O must close when shutdown is requested, as before. No normal request,
body, connection, peer, validation or storage limit may be raised or bypassed.

## Reproduction and verification plan

Final resource-lifetime review (15:29 UTC) identified a possible ordering gap:
the HTTP connection permit may return before the registry drops its duplicate
shutdown descriptor. A targeted real-listener regression will hold the actual
registry mutex while dropping accepted I/O, require the slot to stay occupied,
then release cleanup and verify real EOF and normal next acceptance. This checks
the resource lifetime, not source field order. The test must join cleanup before
asserting failure. Only a reproduced failure will receive a runtime correction;
the 128-connection limit, shutdown deadlines and mutation rules remain unchanged.

1. Through the real public native server, admit eight partial `/native/chain`
   bodies and receive their actual HTTP 100 responses. Keep all clients open,
   request normal shutdown and require server completion within seven seconds
   (five-second drain plus two-second scheduling margin). The current node has
   no running ledger mutation. Verify actual client closure, listener reuse,
   immediate fresh state ownership and exact original canonical/manifest bytes.
   Always close test clients and join the server before reporting a RED result.
2. After that behavior is corrected, exercise an actual large block response
   with a deliberately small TCP receive/send window and an unread body. Do not
   infer socket cleanup merely from a completed server future. Reopen state while
   the old client remains held, then compare exact head/accounting/journal bytes.
3. Verify that prompt normal requests still drain without a mandatory five-second
   sleep, and that already admitted delayed mutations keep ownership/admission
   until completion even if their caller is disconnected. Replay exact IDs and
   accounting afterward. Existing peer shutdown and storage fencing stay intact.

Only disposable numeric-loopback sockets, development identities and temporary
state are in scope. No public listener, external transmission, operator key/fund,
model/VM/paid run, signature shortcut or release is involved. Keep each observed
failure, cause and corrective result below. A local pass does not close R1/R2/R3.

## First reproduction — RED

On the unchanged runtime at `b38b832`, with preregistration `854b850`, the actual
eight-client unfinished-body test exceeded its unchanged seven-second limit:
`elapsedMs=7003 timely=false`, test exit 101 (7.05s harness, 9.92s build).
Closing the fixture clients then allowed the server to join normally; the test
released its listener/state owner and verified unchanged canonical/manifest bytes
before reporting failure. This is not a hung fixture or a changed ledger.

The ranked hypotheses were client-input drain, shutdown-signal delivery and
already admitted state work. This fixture starts no ledger operation, and closing
only the client sockets releases the wait. Static tracing connects that outcome
to the request's ten-second body deadline and unbounded outer graceful drain.
Existing prompt/drained service tests already exercised delivery of the same
normal shutdown signal. No random repeated pass attempt was used.

The prepared correction retains a shutdown handle per already-bounded HTTP
connection, gives it five seconds to drain normally and then shuts down actual
sockets before awaiting Axum's connection tasks. Native mutation admission and
peer I/O still close immediately on the shutdown signal. The existing state
mutation barrier remains after HTTP drain; even a disconnected client cannot
abort an admitted durable write. The optional listener wiring is native-only;
legacy listener behavior is not silently changed or newly qualified here.
Tracking uses one extra shutdown descriptor per admitted connection, bounded
by the existing 128-connection limit. It is not zero-cost: the operating system's
file-descriptor budget remains relevant. Failed registration drops the accepted
connection and its permit, with the existing accept-error backoff rather than a
busy retry. No connection, request or peer limit was increased to compensate.

The unchanged actual eight-client body regression then passed in 5.05s (build
4.07s), reporting `elapsedMs=5003 timely=true`. The five-second drain expired,
actual sockets closed, the server joined, every old held client reached closure,
the listener could be rebound and a fresh native node immediately reopened the
unchanged height-zero state. This is the initial GREEN, not yet the unread-large-
response or already-admitted-mutation verification.

## Unread response and last-owner follow-up

The first unread-response regression exposed an additional last-owner race:
the server completed, but an immediate `NativeNode::open` still found the state
locked. That run failed in 1.58s (build 0.98s). A targeted temporary probe retained
the server's own state reference and reported one remaining owner after drain;
immediate reopen then succeeded. It also exposed a **fixture** error: the test
incorrectly required a minimum four-second wait even when Axum already returned.
That probe failed only that extra assertion in 1.77s (build 3.27s), reporting
zero-millisecond drain and an incomplete 130,500-byte tail from the real
272,026-byte block response. The fixed requirement is a maximum, not a mandatory
sleep. The seven-second maximum, real client closure, incomplete response and
immediate ownership/replay checks remain unchanged; the artificial minimum was
removed. Both failed outcomes are retained, not reported as successful runs.

A proposed unbounded final-owner retry was rejected by safety review before it
was applied. The implemented alternative uses a **one-second cleanup timeout**
after the existing durable-mutation barrier. The server retains its last state
reference, explicitly releases the sole owner before reporting success, and
returns a fixed error if another reference remains. It does not force a lock
off another owner or silently return success. This new cleanup limit does not
preempt already admitted validation/disk work. Temporary probe logs were removed.

The two public-server regressions then passed together in 6.77s (build 3.56s):
the unread 272,026-byte response closed promptly with only 130,500 trailing bytes,
immediate reopen matched all 512 confirmed transfers/head/accounting and source
bytes, and eight incomplete input bodies closed in 5.003s. The response case
does not claim that all bytes reached the client or that a truncated response
is a usable receipt. It verifies closure and durable-state recovery.

The deliberately retained-state-owner test returned the fixed error at 1.000s,
kept the old lock and canonical/manifest bytes intact, then reopened normally
after that owner was deliberately dropped. The actual admitted-block regression
confirmed entry into the existing mutation gate before requesting shutdown,
held the real ledger mutex, and kept seven other HTTP bodies unfinished. After
client-I/O expiry all eight clients were closed, but the admitted block's one
request permit and state ownership remained. The server did not report completion
while that work was delayed. Releasing the delay produced exactly one block and
one 50,000-tBOOLE reward; independent replay and duplicate-block submission
confirmed no extra issuance. This passed in 6.16s (6.016s measured shutdown flow).
The test-only gate observation coordinates a real admitted operation; it does
not replace the block validation/publication path or add a production API.

Focused consumers passed: nine native HTTP integration tests (18.29s), five
native HTTP unit/router tests (17.33s), the unchanged shared connection-cap test
(0.05s) and five existing socket/mutation-lifecycle tests (0.10s). The following
all-target node clippy identified one pre-existing expression in this local
stack's peer-rotation fixture: comparing a JSON value to a freshly allocated
decimal string. Its correction must preserve the wire's string type, not accept
clippy's suggested numeric JSON comparison. This is a fixture/static-check
finding, not a failure of the completed HTTP operating scenarios.

The fixture now computes the expected decimal height once and compares it using
`as_str()`, preserving the existing JSON string contract. Node all-target clippy
then passed in 21.25s. The actual three-process transport-key retirement/re-
enrollment rehearsal passed in 6.300s with normal stops 12–18ms, the exact prior
height-12 canonical head, two confirmed transfers and matching independent audits.
Normal drained shutdown therefore does not impose a mandatory five-second wait.

The unchanged small mixed-pressure companion also passed: 1,025 balances and
8/4/8 outgoing/incoming/HTTP occupancy, rejected excess admission, 440µs maximum
diagnostic response, 916ms network phase, 498µs drained stop, 399ms independent
reopen and 21.800s total. Exact original accounting/head/journal/manifest checks
passed. The full 131,073-balance mixed case then passed alone under OS resource
measurement on the clean committed source, with the original criteria unchanged.
No successful mixed-fork, public-network, arbitrary slow-disk or whole-R1 claim
follows from these direct regressions.

## Unchanged large mixed-resource follow-up — first corrected-source run PASS

Executed on 2026-09-23, approximately 14:56–15:00 UTC, with source
`ecb549cb4cf646648eee269304898a8c2c53f7a5`, tree
`51c917e9d761a5c3c67d49f3f78963540c9d8aeb` and executable SHA-256
`60b09f7048081a83eebcb05f1a3cd1091588ae566dec977f73dfd646971a9f50`.
Clean status, source/tree and executable identity matched before/after. No other
build or test was intentionally run concurrently. This is a follow-up after a
runtime correction, not an unchanged-product retry after an acceptance failure.
The [original mixed criteria](native-mixed-resource-qualification-2026-09.md#fixed-scenario-and-acceptance)
remain 900s total, 1GiB peak RSS, 15s network phase, 3s drained stop, 120s replay,
1s per diagnostic response and unchanged 8/4/8 admission limits.

All eight outgoing clients held thirty decoded blocks, 8,213,762 fixture wire
bytes each; four authenticated inbound requests and eight unfinished HTTP bodies
overlapped. Each HTTP client sent 8,388,607 bytes. The fifth inbound and ninth
ordinary request were rejected, the latter without a 100 response. All ten
diagnostic responses retained non-authoritative markers and 8/4/8 occupancy;
maximum response was 456 microseconds. Completing the empty-array HTTP bodies
returned `adopted=false`, and releasing the excess fork pages produced eight
expected `block_download` failures without any candidate adoption.

Network input/rejection took 3.118s; drained stop took 39.786ms. Independent
reopen took 51.871s, total scenario 234.772s (harness/OS wall 234.81s), with
654,180,352 bytes (623.875MiB) maximum RSS. Maximum template/canonical append/
candidate append were 198/233/196ms. Both test and outer measurement/identity
command exited 0. All listeners were reusable.

The exact original height-267 head
`000002700a9b5f795da3ff71cad2588c0180e2f6213ba0f0e983ea42b1b63d1b`
and every accounting/recipient/nonce check remained unchanged: 131,073 balances,
131,072 confirmed transfers, 69,958,251 history bytes, 1,335,000,000,000,000 issued
atoms and empty pending. Journal digests and manifest bytes matched before/after
independent replay. This qualifies the same fixed incomplete-input combination
on the corrected source; it is not arbitrary successful mixed traffic or R1 PASS.

## Held mixed-input shutdown — preregistered follow-up

The earlier mixed scenario completes HTTP bodies and rejects final excess peer
pages before stopping. It therefore does not prove shutdown while all input
resources remain held. This next test-only variation uses the same actual
combined server, deterministic canonical chain and fixed 8/4/8 overlap. It does
not change the original qualification's criteria or replace its result.

- Use the same small 1,025-balance and large 131,073-balance setup, eight paused
  thirty-block candidates, four authenticated incoming waits and eight HTTP
  clients each withholding the last byte of an 8MiB body. Observe exact 8/4/8
  counters, unchanged limits, excess refusal and ten non-authoritative diagnostic
  replies each within one second, as in the original scenario.
- While all inputs remain incomplete, request normal shutdown. The combined
  server must return success within seven seconds (five-second client-I/O drain
  plus two-second scheduling margin), without fixture clients being dropped or
  their remaining body/page being sent first. No already admitted ledger mutation
  exists in this particular scenario; the separate delayed-block regression
  above owns that distinct durable-work boundary.
- Require actual HTTP closure and TLS EOF/connection-closure errors, not timeout,
  on all eight outgoing and four inbound peer connections. Release fixture
  workers only to observe their already-closed connections, not to send the
  missing candidate pages. Join all workers and rebind all listeners. Keep
  client handles alive through immediate fresh state ownership.
- Independently reopen within 120 seconds and preserve exact original head,
  every recipient/owner balance and nonce, issuance/locks, empty pending, journal
  digests and manifest bytes. Total scenario remains 900 seconds, each measured
  template/append phase ten seconds, history 96MiB and OS peak RSS 1GiB. The whole
  network-and-shutdown phase stays within 15 seconds. Build outside measurement,
  run the large case alone, and record source/tree/executable identity before/after.
- First run the small existing-behavior regression; a first PASS is legitimate.
  Keep failed outcomes and fix proven harness/product defects without relaxing
  criteria. On a shutdown deadline failure, close fixture clients and join the
  actual server before asserting failure. This is not new public exposure,
  signature relaxation, paid work or an overall R1 acceptance claim.

Status: **small held-input, original-fixture regression and large held-input measurement PASS.**

The first small held-input run passed in 26.365s (26.37s harness, 10.63s build).
With the exact 8/4/8 overlap still held, normal stop took 5.007066s and the entire
network/shutdown phase 5.545s. All eight HTTP, four incoming TLS and eight outgoing
TLS connections reached actual closure; timeout was not accepted as closure.
Outgoing sent bytes stayed at the exact paused 8,188,312 bytes per peer, proving
the fixture did not send missing pages to make shutdown finish. The original
HTTP clients never supplied their final bytes. Old client handles and the Tokio
runtime remained alive during fresh ownership/replay, which took 409ms.
Exact original height-13 head, 1,025 balances/1,024 confirmed transfers, 552,737
history bytes, issuance/locks and journal/manifest comparisons passed. Maximum
diagnostic response was 435µs; template/append/candidate append 171/204/178ms.

The shared fixture's original completion-before-stop path then passed unchanged
acceptance in 21.628s (21.63s harness): 912ms network, 550µs drained stop, 446µs
maximum diagnostic response and 396ms replay, with the same original state.
Focused `boole-node --test native_capacity` clippy with warnings denied passed
(7.47s), as did fmt, docs-smoke and diff-check. No production code or limit changed,
and neither test required a corrective retry or artificial RED.

### Large held-input result — first run PASS

The large run executed approximately 15:15–15:19 UTC on September 23 using
source `eb97bb092a755279d21cec4f29f6612cf8fdc089`, tree
`5c5bd5ae1420e608a29eba493c886a0b1ed05797`, executable SHA-256
`366d3bdc75c770d6f366b1e228a0d93ff3bfb235962a81e83a4eef11ab9c3174`.
These identities matched before/after. No other build/test was intentionally
run concurrently. The root feature checkout had only the explicitly excluded
pre-existing `tasks/lessons.md` user edit; source/test/dependency diff was empty
and that user file's bytes also matched before/after. The whole root worktree
was not clean, and the user's edit was neither staged nor used as test source.

Exactly eight thirty-block outgoing buffers (8,213,762 bytes each), four
authenticated incomplete incoming requests and eight incomplete HTTP bodies
(8,388,607 bytes sent each) were held when normal shutdown was requested. Excess
admission was refused; all ten diagnostics retained 8/4/8 occupancy and the
non-authoritative markers, with maximum response 438µs. The server returned in
5.071433s. All eight HTTP and twelve TLS connections closed without completing
the missing input. Every outgoing fixture sent exactly its original paused byte
count, and every listener could be rebound after workers joined. Network plus
shutdown took 7.082s, within the fixed 15s criterion.

The old HTTP/incoming client handles and runtime remained alive while a fresh
node independently replayed the same state in 51.803s. Exact original height-267
head `000002700a9b5f795da3ff71cad2588c0180e2f6213ba0f0e983ea42b1b63d1b`,
131,073 balance entries, 131,072 confirmed transfers, 69,958,251 history bytes,
1,335,000,000,000,000 issued atoms, every recipient/owner/nonce/lock and empty
pending matched. Journal digests and manifest bytes were unchanged. Maximum
template/append/candidate append were 209/234/192ms.

Total scenario was 239.702s (harness 239.74s, OS wall 239.75s), maximum RSS
640,024,576 bytes (610.375MiB). Test and enclosing measurement/identity commands
exited 0. All preregistered criteria passed on the first run; no production
behavior or limit was changed. This is a fixed input-only shutdown qualification,
not successful competing mutation under load, arbitrary slow storage, public
availability, hard real-time whole-process shutdown or R1 completion.

## Connection-slot lifetime correction — RED then GREEN

The final review's real-listener test reproduced the cleanup-order gap on the
unchanged production source from `2a01c61`. With a one-slot listener and one real
accepted connection, holding the actual socket-registry lock made I/O destruction
wait while its shutdown descriptor remained retained. The connection permit was
already available: the test failed with `HTTP connection slot returned while its
shutdown socket was still retained` (0.01s test, 8.15s build). The fixture then
released the registry, joined cleanup, verified actual client EOF and accepted a
replacement normally before reporting RED. This demonstrates premature resource
admission, not a measured 128-connection memory/descriptor exhaustion incident.

The minimal correction drops the socket lease before the connection permit.
The real stream still drops first, and no connection limit, timer, protocol,
signature, state-mutation rule or public API changes. The test-only registry
coordination holds the real mutex; it does not substitute a mock socket/destructor.
Both connection-cap tests passed (0.26s), as did all five lifecycle tests (0.09s).
The first downstream command then named a nonexistent `native_rpc` integration
target and exited 101 without running that target. Correcting only the command
to the actual `native_http` target ran nine tests successfully (18.36s); the five
native HTTP unit/router tests also passed (17.33s), including six-second delayed
durable mutation and the explicit one-second retained-owner failure.

The same small held-input consumer passed in 26.320s: held stop 5.005700s,
network/stop 5.577s, maximum diagnostic response 472µs and independent replay
409ms. All twenty connections actually closed, missing inputs stayed unsent,
and exact original head/accounting/files matched. Targeted node lib/native HTTP/
capacity clippy with warnings denied passed in 2.49s; fmt/docs-smoke/diff passed.
The unchanged large held-input qualification below also passed. It used the
existing 900s total, 1GiB RSS, 15s network-and-stop, 7s held-stop, 120s replay and
exact state/file criteria above, alone with committed source/tree/executable
identities recorded before and after. Prior large results remain tied to their
original source; they are not overwritten.

### Large held-input follow-up after slot-lifetime correction — PASS

The first corrected-source run, approximately 15:35–15:39 UTC, used source
`df6a32c1ebc380c0a2e203481fe9a93238a96c28`, tree
`6bd1c7a1279ff2880cf2b3cd86f0e2f7d5378ed7` and executable SHA-256
`91ef1bcf2e4b2b72e77ec8c2ecea464d328d6e78a6eabcf86c6628f97cca168c`.
All matched before/after; runtime/test/dependency diffs were empty. The only
worktree change was the excluded pre-existing `tasks/lessons.md` edit, whose
bytes also matched. No other build/test was intentionally run concurrently.

The original exact 8/4/8 overlap, excess refusal and non-authoritative diagnostics
passed; maximum diagnostic response was 449µs. Normal stop while inputs remained
held took 5.071302s, with the entire network/stop phase 7.093s. Eight HTTP and
twelve TLS connections actually closed without sending any missing body/page;
each outgoing fixture's final bytes equaled its paused 8,213,762 bytes. All
workers joined and listeners were reusable. Old HTTP/inbound clients and their
runtime remained alive during independent fresh ownership/replay (51.647s).

All original head/accounting/journal/manifest checks passed: height 267, head
`000002700a9b5f795da3ff71cad2588c0180e2f6213ba0f0e983ea42b1b63d1b`,
131,073 balances, 131,072 confirmed transfers, 69,958,251 history bytes and
1,335,000,000,000,000 issued atoms. Total scenario was 238.814s (harness/OS wall
238.86s), with 651,952,128 bytes (621.75MiB) peak RSS. Maximum template/append/
candidate append were 201/232/192ms. Both test and enclosing identity/measurement
command exited 0. This requalifies only the same fixed input-only combination,
not successful mixed mutations, arbitrary CPU/disk shutdown time, public
availability or complete R1. Its RSS differs from the prior run; no generalized
memory or speed improvement is claimed.

Raw corrected-source large outcome:

```text
df6a32c1ebc380c0a2e203481fe9a93238a96c28
6bd1c7a1279ff2880cf2b3cd86f0e2f7d5378ed7
 M tasks/lessons.md
91ef1bcf2e4b2b72e77ec8c2ecea464d328d6e78a6eabcf86c6628f97cca168c  target/debug/deps/native_capacity-6f26ba504621f258
0e071475840f2ec9b36549507c18004847923eeb121b9baf86b6338ab76ff793  tasks/lessons.md

running 1 test
test mixed::native_mixed_shutdown_131072_accounts ... mixed-progress fundedBlocks=32 elapsedMs=19208
mixed-progress fundedBlocks=64 elapsedMs=38293
mixed-progress fundedBlocks=96 elapsedMs=57550
mixed-progress fundedBlocks=128 elapsedMs=77465
mixed-progress fundedBlocks=160 elapsedMs=97660
mixed-progress fundedBlocks=192 elapsedMs=117138
mixed-progress fundedBlocks=224 elapsedMs=137026
mixed-progress fundedBlocks=256 elapsedMs=157261
boole-node: native HTTP drain expired; closing remaining client sockets
mixed-network {"advertised":{"hash":"000092c6b9368f84167d9bdbe28083aaa3c3a54ec6f0886623daef36cd3ca5cc","height":299},"diagnosticMaxMicros":449,"drained":{"closedHttp":8,"closedInbound":4,"closedOutbound":8,"completedMissingInputs":false},"elapsedMs":186343,"finalSentBytesPerPeer":[8213762,8213762,8213762,8213762,8213762,8213762,8213762,8213762],"fundedBlocks":256,"httpBodyBytesSentPerClient":8388607,"localHead":"000002700a9b5f795da3ff71cad2588c0180e2f6213ba0f0e983ea42b1b63d1b","networkMs":7093,"overlap":{"authority":"local_process_only","ledgerReadiness":"not_checked","peers":{"acceptedConnections":4,"activeInboundWorkers":4,"activeOutboundRounds":8,"authenticatedConnections":4,"authenticationFailures":0,"completedInboundRounds":0,"enabled":true,"failedInboundRounds":0,"limits":{"handshakeTimeoutMs":2000,"inboundKeyCooldownMs":500,"maxHandshakesPerSecond":8,"maxInboundWorkers":4,"maxMessageBytes":1048576,"maxOutboundWorkers":8,"maxPeers":8,"maxRoundBlocks":256,"maxRoundBytes":8388608,"maxRoundRequests":64,"roundTimeoutMs":10000},"listenAddress":"127.0.0.1:55883","localPeerId":"5827ab087acf684a8167e7a4848db47971a71a2f1ae13cf3bd09921a8d1b8b25","peakInboundWorkers":4,"peakOutboundRounds":8,"peers":[{"address":"127.0.0.1:55885","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"632b1291c27c9bfa98a9272088c6a556aebc2ceefe48893d8b7a066eaea30593","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:55886","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"629eccdef084b6cd40fca2d68775c3c1edced625dd736285f2abf6d15fc648b1","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:55887","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"1b40d5e2f4f4bfab4acf9ad0a15ebadf00b5832966c5f91d789ffb14bb92d4b7","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:55888","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"69ca916b9325ff68e1aa800dba542bf7d7b94c36e0bd869818bde2281b3aa824","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:55889","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"1c65bce086c41ae269903ac567dd75cd99a8283e43f89a4cb5d98d0bb92e8a73","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:55890","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"1e6560d4bf5033d1820a56cbcea1ac512c5c93f4db1b7ac09166f545fe94a283","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:55891","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"a800e6fbe416277b22a55e804d52a10c84bc75c87801b9ee6ec8f716ba4fdb6b","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:55892","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"e014b988d5ee4b291f54707a27d1520c2630c48701bb8e40c8303dfbde24d3ce","retryDelayMs":0,"state":"not_connected","successfulRounds":0}],"rejectedConnections":1,"rejectedPeerRounds":0,"running":true},"rpc":{"activeDiagnostics":1,"activeRequests":8,"diagnosticLimit":2,"requestLimit":8},"schema":"boole.native.diagnostics.v1","stopping":false},"partialBytesPerPeer":[8213762,8213762,8213762,8213762,8213762,8213762,8213762,8213762],"stopMicros":5071302,"stopWithHeldInputs":true}
mixed-result {"elapsedMs":238814,"fundedBlocks":256,"genesisHash":"933a672120674efa9ec6205f344e1830febdec58b0ffcd2603ccc8a9723610c1","head":"000002700a9b5f795da3ff71cad2588c0180e2f6213ba0f0e983ea42b1b63d1b","issued":"1335000000000000","maxAppendMs":232,"maxCandidateAppendMs":192,"maxTemplateMs":201,"networkId":"boole-native-testnet-1","resources":{"balanceEntries":131073,"confirmedTransfers":131072,"historyBlocks":267,"historyBytes":69958251,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":1,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0},"restartMs":51647,"stopWithHeldInputs":true}
ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 15 filtered out; finished in 238.86s

      238.86 real       234.82 user         0.63 sys
           651952128  maximum resident set size
                   0  average shared memory size
                   0  average unshared data size
                   0  average unshared stack size
               41850  page reclaims
                   0  page faults
                   0  swaps
                   0  block input operations
                   0  block output operations
                5771  messages sent
                7790  messages received
                   0  signals received
                1791  voluntary context switches
                3004  involuntary context switches
       3647249080318  instructions retired
       1015225931950  cycles elapsed
           438895528  peak memory footprint
df6a32c1ebc380c0a2e203481fe9a93238a96c28
6bd1c7a1279ff2880cf2b3cd86f0e2f7d5378ed7
 M tasks/lessons.md
91ef1bcf2e4b2b72e77ec8c2ecea464d328d6e78a6eabcf86c6628f97cca168c  target/debug/deps/native_capacity-6f26ba504621f258
0e071475840f2ec9b36549507c18004847923eeb121b9baf86b6338ab76ff793  tasks/lessons.md
```

### Connection-cap RED

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 8.15s
     Running unittests src/lib.rs (target/debug/deps/boole_node-9cd49e707f28be6c)

running 1 test
test local_node::tests::active_connection_cap_includes_socket_cleanup_waiters ...
thread 'local_node::tests::active_connection_cap_includes_socket_cleanup_waiters' (8651739) panicked at crates/boole-node/src/local_node.rs:7602:9:
HTTP connection slot returned while its shutdown socket was still retained
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
FAILED

failures:

failures:
    local_node::tests::active_connection_cap_includes_socket_cleanup_waiters

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 315 filtered out; finished in 0.01s

error: test failed, to rerun pass `-p boole-node --lib`
```

### Connection-cap/lifecycle GREEN and incorrect target command

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 3.24s
     Running unittests src/lib.rs (target/debug/deps/boole_node-9cd49e707f28be6c)

running 2 tests
test local_node::tests::active_connection_cap_includes_socket_cleanup_waiters ... ok
test local_node::tests::active_connection_cap_releases_only_when_connection_io_drops ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 314 filtered out; finished in 0.26s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.10s
     Running unittests src/lib.rs (target/debug/deps/boole_node-9cd49e707f28be6c)

running 5 tests
test p2p_lifecycle::tests::dropping_a_lease_removes_only_its_socket ... ok
test p2p_lifecycle::tests::request_stop_wakes_sockets_before_an_earlier_mutation_finishes ... ok
test p2p_lifecycle::tests::stop_closes_every_registered_socket_and_rejects_late_registration ... ok
test p2p_lifecycle::tests::stop_waits_for_an_earlier_mutation_and_rejects_every_later_one ... ok
test p2p_lifecycle::tests::stop_wakes_a_lifecycle_wait_without_polling_the_full_duration ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 311 filtered out; finished in 0.09s

error: no test target named `native_rpc` in `boole-node` package
```

### Correct target, held-input small consumer and static checks

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 3.80s
     Running tests/native_http.rs (target/debug/deps/native_http-1b121ee1a1d92267)

running 9 tests
test native_http_reports_pinned_network_and_refuses_browser_cross_origin_and_public_bind ... ok
test native_json_errors_do_not_reflect_large_unknown_field_names_into_responses ... ok
test native_peer_rpc_exposes_the_bounded_outbound_failure_stage ... ok
test native_request_limit_applies_before_reading_another_large_json_body ... ok
test native_rpc_and_secure_peers_share_the_same_durable_state_and_shutdown_boundary ... ok
test native_shutdown_closes_an_unread_large_response_and_releases_state_ownership ... native-http-shutdown-response elapsedMs=0 timely=true responseBytes=272026 trailingBytes=130500
ok
test native_shutdown_closes_unfinished_http_bodies_before_the_request_deadline ... boole-node: native HTTP drain expired; closing remaining client sockets
native-http-shutdown-body elapsedMs=5003 timely=true
ok
test stalled_native_request_bodies_expire_and_return_all_admission_slots ... ok
test two_independent_rpc_nodes_mine_transfer_and_rejoin_with_identical_accounting ... ok

test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 18.36s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.09s
     Running unittests src/lib.rs (target/debug/deps/boole_node-9cd49e707f28be6c)

running 5 tests
test native_http::tests::diagnostics_do_not_confer_readiness_or_bypass_their_own_limit_and_shutdown_boundary ... ok
test native_http::tests::process_diagnostics_remain_available_while_all_state_requests_wait_on_the_ledger ... ok
test native_http::tests::shutdown_disconnects_clients_but_waits_for_an_already_admitted_block ... boole-node: native HTTP drain expired; closing remaining client sockets
native-http-shutdown-mutation elapsedMs=6020 closedClients=8 admittedBlockPreserved=true
ok
test native_http::tests::shutdown_state_owner_cleanup_is_bounded_and_never_forces_a_retained_owner ... native-http-state-owner-timeout elapsedMs=1000 retainedLockPreserved=true
ok
test native_http::tests::timed_out_http_callers_do_not_release_admission_while_their_actual_mutations_wait ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 311 filtered out; finished in 17.33s

   Compiling boole-node v0.1.0 (/Users/seoyong/projects/Boole/crates/boole-node)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 1.21s
     Running tests/native_capacity.rs (target/debug/deps/native_capacity-6f26ba504621f258)

running 1 test
test mixed::small_mixed_shutdown_closes_held_inputs_and_replays_unchanged ... boole-node: native HTTP drain expired; closing remaining client sockets
mixed-network {"advertised":{"hash":"0000238874213f313ec8275b19912f2e9e9630a5481480e8dc3288e937507042","height":45},"diagnosticMaxMicros":472,"drained":{"closedHttp":8,"closedInbound":4,"closedOutbound":8,"completedMissingInputs":false},"elapsedMs":25899,"finalSentBytesPerPeer":[8188312,8188312,8188312,8188312,8188312,8188312,8188312,8188312],"fundedBlocks":2,"httpBodyBytesSentPerClient":8388607,"localHead":"00009efa3c0a5376c32ed2818d39c89193e09180d72fbfe155a586df1f5f1887","networkMs":5577,"overlap":{"authority":"local_process_only","ledgerReadiness":"not_checked","peers":{"acceptedConnections":4,"activeInboundWorkers":4,"activeOutboundRounds":8,"authenticatedConnections":4,"authenticationFailures":0,"completedInboundRounds":0,"enabled":true,"failedInboundRounds":0,"limits":{"handshakeTimeoutMs":2000,"inboundKeyCooldownMs":500,"maxHandshakesPerSecond":8,"maxInboundWorkers":4,"maxMessageBytes":1048576,"maxOutboundWorkers":8,"maxPeers":8,"maxRoundBlocks":256,"maxRoundBytes":8388608,"maxRoundRequests":64,"roundTimeoutMs":10000},"listenAddress":"127.0.0.1:55202","localPeerId":"cb0eb27075e4eb10dba200341fe8f870e692d57b4fb4d4e5e60fe94ed6934014","peakInboundWorkers":4,"peakOutboundRounds":8,"peers":[{"address":"127.0.0.1:55204","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"670c5cd0edce534783576fd2cff3c7659e068bbfb2e83c90c9a603cf19a34d95","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:55205","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"7c9ff9759530df5fec1830be5f63f3370a7f85537e2bdae6dffb05d241969d7e","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:55206","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"d46ea191ecb42cc58c1975d6318bf1451c794a1b38fac989506696436a182d0e","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:55207","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"b4dfc248eb019ca676a73de860aa5309fa0f8ae227f83104057d3b55252e74c1","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:55208","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"8ad14995c02e8020843969b0046e22343ccd949eb6ce46b1d9c416c3dd5f4cd9","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:55209","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"f47ac7b315d35647816e300599c7350cbfb0fdb291a70feeee3b751b223f0e52","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:55210","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"509ef0ccc5a1cc13538ca1ab81bedd3ec2399d16e80beb3c8ff59c10f216b856","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:55211","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"cbbbc92df2ae2c6aab92bc078c4115a4c68011d4918f74c75dc2bd9b88af65c3","retryDelayMs":0,"state":"not_connected","successfulRounds":0}],"rejectedConnections":1,"rejectedPeerRounds":0,"running":true},"rpc":{"activeDiagnostics":1,"activeRequests":8,"diagnosticLimit":2,"requestLimit":8},"schema":"boole.native.diagnostics.v1","stopping":false},"partialBytesPerPeer":[8188312,8188312,8188312,8188312,8188312,8188312,8188312,8188312],"stopMicros":5005700,"stopWithHeldInputs":true}
mixed-result {"elapsedMs":26320,"fundedBlocks":2,"genesisHash":"933a672120674efa9ec6205f344e1830febdec58b0ffcd2603ccc8a9723610c1","head":"00009efa3c0a5376c32ed2818d39c89193e09180d72fbfe155a586df1f5f1887","issued":"65000000000000","maxAppendMs":202,"maxCandidateAppendMs":177,"maxTemplateMs":171,"networkId":"boole-native-testnet-1","resources":{"balanceEntries":1025,"confirmedTransfers":1024,"historyBlocks":13,"historyBytes":552737,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":1,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0},"restartMs":409,"stopWithHeldInputs":true}
ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 15 filtered out; finished in 26.32s

    Checking boole-node v0.1.0 (/Users/seoyong/projects/Boole/crates/boole-node)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.49s
docs-smoke: PASS
```

## Raw outcomes and follow-up checks

### Initial unfinished-body RED

```text
Compiling boole-node v0.1.0 (/private/tmp/boole-r1-http-drain-worktree.kl1exv/crates/boole-node)
   Compiling boole-testkit v0.1.0 (/private/tmp/boole-r1-http-drain-worktree.kl1exv/crates/boole-testkit)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 9.92s
     Running tests/native_http.rs (/Users/seoyong/projects/Boole/target/debug/deps/native_http-1b121ee1a1d92267)

running 1 test
test native_shutdown_closes_unfinished_http_bodies_before_the_request_deadline ... native-http-shutdown-body elapsedMs=7003 timely=false

thread 'native_shutdown_closes_unfinished_http_bodies_before_the_request_deadline' (8538557) panicked at crates/boole-node/tests/native_http.rs:285:5:
shutdown waited beyond its 5s client-I/O drain window plus scheduling margin
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
FAILED

failures:

failures:
    native_shutdown_closes_unfinished_http_bodies_before_the_request_deadline

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 7 filtered out; finished in 7.05s

error: test failed, to rerun pass `-p boole-node --test native_http`
```

### Unread-response ownership failure

```text
Finished `test` profile [unoptimized + debuginfo] target(s) in 0.98s
     Running tests/native_http.rs (/Users/seoyong/projects/Boole/target/debug/deps/native_http-1b121ee1a1d92267)

running 1 test
test native_shutdown_closes_an_unread_large_response_and_releases_state_ownership ...
thread 'native_shutdown_closes_an_unread_large_response_and_releases_state_ownership' (8558701) panicked at crates/boole-node/tests/native_http.rs:396:36:
unread response must not retain state ownership: state directory is already locked by another process: /var/folders/rz/vz57tt8555zcz4r6hr4cbqv80000gn/T/boole-native-http-shutdown-response-1790174074259729000
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
FAILED

failures:

failures:
    native_shutdown_closes_an_unread_large_response_and_releases_state_ownership

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 8 filtered out; finished in 1.58s

error: test failed, to rerun pass `-p boole-node --test native_http`
```

### Temporary diagnostic probe and fixture-minimum failure

```text
Finished `test` profile [unoptimized + debuginfo] target(s) in 3.27s
     Running tests/native_http.rs (/Users/seoyong/projects/Boole/target/debug/deps/native_http-1b121ee1a1d92267)

running 1 test
test native_shutdown_closes_an_unread_large_response_and_releases_state_ownership ... [DEBUG-native-http-drain] remaining state owners=1
[DEBUG-native-http-drain] response server elapsedMs=0 timely=true bodyBytes=272026
native-http-shutdown-response elapsedMs=0 timely=true responseBytes=272026 trailingBytes=130500

thread 'native_shutdown_closes_an_unread_large_response_and_releases_state_ownership' (8561547) panicked at crates/boole-node/tests/native_http.rs:437:5:
fixture did not hold the response drain
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
FAILED

failures:

failures:
    native_shutdown_closes_an_unread_large_response_and_releases_state_ownership

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 8 filtered out; finished in 1.77s

error: test failed, to rerun pass `-p boole-node --test native_http`
```

### Bounded correction: public-server GREEN

```text
Finished `test` profile [unoptimized + debuginfo] target(s) in 3.56s
     Running tests/native_http.rs (/Users/seoyong/projects/Boole/target/debug/deps/native_http-1b121ee1a1d92267)

running 2 tests
test native_shutdown_closes_an_unread_large_response_and_releases_state_ownership ... native-http-shutdown-response elapsedMs=0 timely=true responseBytes=272026 trailingBytes=130500
ok
test native_shutdown_closes_unfinished_http_bodies_before_the_request_deadline ... boole-node: native HTTP drain expired; closing remaining client sockets
native-http-shutdown-body elapsedMs=5003 timely=true
ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 7 filtered out; finished in 6.77s
```

### Retained-owner deadline

```text
Finished `test` profile [unoptimized + debuginfo] target(s) in 5.54s
     Running unittests src/lib.rs (/Users/seoyong/projects/Boole/target/debug/deps/boole_node-9cd49e707f28be6c)

running 1 test
test native_http::tests::shutdown_state_owner_cleanup_is_bounded_and_never_forces_a_retained_owner ... native-http-state-owner-timeout elapsedMs=1000 retainedLockPreserved=true
ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 313 filtered out; finished in 1.04s
```

### Already-admitted durable block

```text
Finished `test` profile [unoptimized + debuginfo] target(s) in 2.83s
     Running unittests src/lib.rs (/Users/seoyong/projects/Boole/target/debug/deps/boole_node-9cd49e707f28be6c)

running 1 test
test native_http::tests::shutdown_disconnects_clients_but_waits_for_an_already_admitted_block ... boole-node: native HTTP drain expired; closing remaining client sockets
native-http-shutdown-mutation elapsedMs=6016 closedClients=8 admittedBlockPreserved=true
ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 314 filtered out; finished in 6.16s
```

### Focused consumers and original clippy finding

```text
Finished `test` profile [unoptimized + debuginfo] target(s) in 2.56s
     Running tests/native_http.rs (/Users/seoyong/projects/Boole/target/debug/deps/native_http-1b121ee1a1d92267)

running 9 tests
test native_http_reports_pinned_network_and_refuses_browser_cross_origin_and_public_bind ... ok
test native_json_errors_do_not_reflect_large_unknown_field_names_into_responses ... ok
test native_peer_rpc_exposes_the_bounded_outbound_failure_stage ... ok
test native_request_limit_applies_before_reading_another_large_json_body ... ok
test native_rpc_and_secure_peers_share_the_same_durable_state_and_shutdown_boundary ... ok
test native_shutdown_closes_an_unread_large_response_and_releases_state_ownership ... native-http-shutdown-response elapsedMs=0 timely=true responseBytes=272026 trailingBytes=130500
ok
test native_shutdown_closes_unfinished_http_bodies_before_the_request_deadline ... boole-node: native HTTP drain expired; closing remaining client sockets
native-http-shutdown-body elapsedMs=5003 timely=true
ok
test stalled_native_request_bodies_expire_and_return_all_admission_slots ... ok
test two_independent_rpc_nodes_mine_transfer_and_rejoin_with_identical_accounting ... ok

test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 18.29s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.08s
     Running unittests src/lib.rs (/Users/seoyong/projects/Boole/target/debug/deps/boole_node-9cd49e707f28be6c)

running 5 tests
test native_http::tests::diagnostics_do_not_confer_readiness_or_bypass_their_own_limit_and_shutdown_boundary ... ok
test native_http::tests::process_diagnostics_remain_available_while_all_state_requests_wait_on_the_ledger ... ok
test native_http::tests::shutdown_disconnects_clients_but_waits_for_an_already_admitted_block ... boole-node: native HTTP drain expired; closing remaining client sockets
native-http-shutdown-mutation elapsedMs=6014 closedClients=8 admittedBlockPreserved=true
ok
test native_http::tests::shutdown_state_owner_cleanup_is_bounded_and_never_forces_a_retained_owner ... native-http-state-owner-timeout elapsedMs=1000 retainedLockPreserved=true
ok
test native_http::tests::timed_out_http_callers_do_not_release_admission_while_their_actual_mutations_wait ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 310 filtered out; finished in 17.33s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.09s
     Running unittests src/lib.rs (/Users/seoyong/projects/Boole/target/debug/deps/boole_node-9cd49e707f28be6c)

running 1 test
test local_node::tests::active_connection_cap_releases_only_when_connection_io_drops ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 314 filtered out; finished in 0.05s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.07s
     Running unittests src/lib.rs (/Users/seoyong/projects/Boole/target/debug/deps/boole_node-9cd49e707f28be6c)

running 5 tests
test p2p_lifecycle::tests::dropping_a_lease_removes_only_its_socket ... ok
test p2p_lifecycle::tests::request_stop_wakes_sockets_before_an_earlier_mutation_finishes ... ok
test p2p_lifecycle::tests::stop_closes_every_registered_socket_and_rejects_late_registration ... ok
test p2p_lifecycle::tests::stop_waits_for_an_earlier_mutation_and_rejects_every_later_one ... ok
test p2p_lifecycle::tests::stop_wakes_a_lifecycle_wait_without_polling_the_full_duration ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 310 filtered out; finished in 0.10s

    Checking boole-core v0.1.0 (/private/tmp/boole-r1-http-drain-worktree.kl1exv/crates/boole-core)
    Checking boole-native-shadow-protocol v0.1.0 (/private/tmp/boole-r1-http-drain-worktree.kl1exv/crates/boole-native-shadow-protocol)
    Checking boole-lean-runner v0.1.0 (/private/tmp/boole-r1-http-drain-worktree.kl1exv/crates/boole-lean-runner)
    Checking boole-p2p v0.1.0 (/private/tmp/boole-r1-http-drain-worktree.kl1exv/crates/boole-p2p)
    Checking boole-evm-adapter v0.1.0 (/private/tmp/boole-r1-http-drain-worktree.kl1exv/crates/boole-evm-adapter)
    Checking boole-node v0.1.0 (/private/tmp/boole-r1-http-drain-worktree.kl1exv/crates/boole-node)
    Checking boole-testkit v0.1.0 (/private/tmp/boole-r1-http-drain-worktree.kl1exv/crates/boole-testkit)
error: this creates an owned instance just for comparison
   --> crates/boole-node/tests/native_peer_rotation.rs:207:63
    |
207 |                 info["headHash"] == head && info["height"] == height.to_string()
    |                                                               ^^^^^^^^^^^^^^^^^^ help: try: `height`
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/rust-1.95.0/index.html#cmp_owned
    = note: `-D clippy::cmp-owned` implied by `-D warnings`
    = help: to override `-D warnings` add `#[allow(clippy::cmp_owned)]`

error: could not compile `boole-node` (test "native_peer_rotation") due to 1 previous error
warning: build failed, waiting for other jobs to finish...
```

### Corrected clippy, real process rotation and small mixed pressure

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 21.25s
   Compiling boole-node v0.1.0 (/private/tmp/boole-r1-http-drain-worktree.kl1exv/crates/boole-node)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.89s
     Running tests/native_peer_rotation.rs (/Users/seoyong/projects/Boole/target/debug/deps/native_peer_rotation-5c23921ecc6a47a1)

running 1 test
test stopped_configuration_revokes_old_transport_key_and_explicit_reenrollment_preserves_ledger ... native-peer-rotation-phase process-ready elapsedMs=36
native-peer-rotation-phase process-ready elapsedMs=70
native-peer-rotation-phase process-ready elapsedMs=70
native-peer-rotation-phase head-agreement elapsedMs=1
native-peer-rotation-phase reciprocal-membership elapsedMs=410
native-peer-rotation-stop elapsedMs=16
native-peer-rotation-phase old-key-live-authenticated-connection elapsedMs=19
native-peer-rotation-stop elapsedMs=12
native-peer-rotation-stop elapsedMs=17
native-peer-rotation-phase process-ready elapsedMs=33
native-peer-rotation-phase process-ready elapsedMs=32
native-peer-rotation-phase reciprocal-membership elapsedMs=483
native-peer-rotation-phase process-ready elapsedMs=30
native-peer-rotation-phase unapproved-identity-refused elapsedMs=0
native-peer-rotation-phase retired-key-authentication-failure elapsedMs=0
native-peer-rotation-phase retired-key-authentication-failure elapsedMs=0
native-peer-rotation-stop elapsedMs=16
native-peer-rotation-phase process-ready elapsedMs=35
native-peer-rotation-phase unapproved-identity-refused elapsedMs=0
native-peer-rotation-phase replacement-key-server-authentication-refusal elapsedMs=0
native-peer-rotation-phase healthy-peer-transfer-propagation elapsedMs=396
native-peer-rotation-phase head-agreement elapsedMs=276
native-peer-rotation-stop elapsedMs=18
native-peer-rotation-stop elapsedMs=17
native-peer-rotation-phase process-ready elapsedMs=36
native-peer-rotation-phase process-ready elapsedMs=35
native-peer-rotation-phase head-agreement elapsedMs=471
native-peer-rotation-phase reciprocal-membership elapsedMs=68
native-peer-rotation-phase retired-key-authentication-failure elapsedMs=0
native-peer-rotation-phase retired-key-authentication-failure elapsedMs=0
native-peer-rotation-phase replacement-peer-transfer-propagation elapsedMs=444
native-peer-rotation-phase head-agreement elapsedMs=448
native-peer-rotation-phase reciprocal-membership elapsedMs=486
native-peer-rotation-stop elapsedMs=16
native-peer-rotation-stop elapsedMs=16
native-peer-rotation-stop elapsedMs=16
native-peer-rotation-result {"audit":{"accounting":{"balanceAtoms":"60000000000000","issuedAtoms":"60000000000000","lockedAtoms":"50000000000000","pendingRewardEntries":10,"spendableAtoms":"10000000000000","supplyCapAtoms":"100000000000000000"},"confirmedTransfers":{"amountAtoms":"200000000","count":2,"feeAtoms":"2000"},"genesisHash":"933a672120674efa9ec6205f344e1830febdec58b0ffcd2603ccc8a9723610c1","headHash":"000006c6b50e64fc92c8c5de8bd13f44a212110d2fd3270df7d303487649d769","height":"12","networkId":"boole-native-testnet-1","resources":{"balanceEntries":3,"confirmedTransfers":2,"historyBlocks":12,"historyBytes":10396,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":1,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0},"schema":"boole.native.audit.v1","scope":"confirmed_canonical"},"closedOldConnection":true,"elapsedMs":6300,"explicitReenrollment":true,"head":"000006c6b50e64fc92c8c5de8bd13f44a212110d2fd3270df7d303487649d769","ownerPk":"6738a145e4568df7963726a3920ba15b79873afd1d59783a006bec94202cb89e","preservedOriginalKey":true,"replacementPeerId":"3f09627a6cc7a70faea30a64263e996cffcd460aee96892320858080e6950c77","retiredPeerId":"2ef8f75d1fbb7f9839a28208c859bf36b347faa2305db941edab6d8610200bd8"}
ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 6.30s

   Compiling boole-node v0.1.0 (/private/tmp/boole-r1-http-drain-worktree.kl1exv/crates/boole-node)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 1.39s
     Running tests/native_capacity.rs (/Users/seoyong/projects/Boole/target/debug/deps/native_capacity-6f26ba504621f258)

running 1 test
test mixed::small_mixed_p2p_http_pressure_preserves_state_and_diagnostics ... mixed-network {"advertised":{"hash":"0000238874213f313ec8275b19912f2e9e9630a5481480e8dc3288e937507042","height":45},"diagnosticMaxMicros":440,"drained":{"authority":"local_process_only","ledgerReadiness":"not_checked","peers":{"acceptedConnections":4,"activeInboundWorkers":0,"activeOutboundRounds":0,"authenticatedConnections":4,"authenticationFailures":0,"completedInboundRounds":0,"enabled":true,"failedInboundRounds":4,"limits":{"handshakeTimeoutMs":2000,"inboundKeyCooldownMs":500,"maxHandshakesPerSecond":8,"maxInboundWorkers":4,"maxMessageBytes":1048576,"maxOutboundWorkers":8,"maxPeers":8,"maxRoundBlocks":256,"maxRoundBytes":8388608,"maxRoundRequests":64,"roundTimeoutMs":10000},"listenAddress":"127.0.0.1:64513","localPeerId":"7123b8762614eff919fae17b68ec1d1d7a357b70cb61f4e4ed93ea72099484ed","peakInboundWorkers":4,"peakOutboundRounds":8,"peers":[{"address":"127.0.0.1:64515","consecutiveFailures":1,"failedRounds":1,"lastFailureStage":"block_download","peerId":"9b0e4117c92abc3fb41b73199bb0e07a918f6a468c1fa2fca595e482af4d3c11","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:64516","consecutiveFailures":1,"failedRounds":1,"lastFailureStage":"block_download","peerId":"c29d1215729e0cbea8e555a1906503cbcca9b60d831627607f4c59491f4ad884","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:64517","consecutiveFailures":1,"failedRounds":1,"lastFailureStage":"block_download","peerId":"7bbd2ffa3590a19d44384d4bd8b2a44db3e129205dbd27b5484ebcc0de14d842","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:64518","consecutiveFailures":1,"failedRounds":1,"lastFailureStage":"block_download","peerId":"b3feff5c7e7ffe92dc30e5cb52bc4a32693ee28e396b6fa31fdf413638af250e","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:64519","consecutiveFailures":1,"failedRounds":1,"lastFailureStage":"block_download","peerId":"e0a55c86b5730af552dc9066e41080ff1795f34347bb3eb03261cc9341abd0a7","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:64520","consecutiveFailures":1,"failedRounds":1,"lastFailureStage":"block_download","peerId":"7e6c5a044eba486ce2be92cdd9c822dffaea77a89361bc904cfa534db7c0196c","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:64521","consecutiveFailures":1,"failedRounds":1,"lastFailureStage":"block_download","peerId":"a115a60f4ff8721859de43fcc9f56ace97dcfdce6d4c117da7c97e9d85c57ccf","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:64522","consecutiveFailures":1,"failedRounds":1,"lastFailureStage":"block_download","peerId":"2a6a57fdb29aea0c4b2a21977d55bce08b7cd0febc6c02ec3ddf9438358302f1","retryDelayMs":500,"state":"retrying","successfulRounds":0}],"rejectedConnections":1,"rejectedPeerRounds":0,"running":true},"rpc":{"activeDiagnostics":1,"activeRequests":0,"diagnosticLimit":2,"requestLimit":8},"schema":"boole.native.diagnostics.v1","stopping":false},"elapsedMs":21391,"finalSentBytesPerPeer":[9007936,9007936,9007936,9007936,9007936,9007936,9007936,9007936],"fundedBlocks":2,"httpBodyBytesSentPerClient":8388607,"localHead":"00009efa3c0a5376c32ed2818d39c89193e09180d72fbfe155a586df1f5f1887","networkMs":916,"overlap":{"authority":"local_process_only","ledgerReadiness":"not_checked","peers":{"acceptedConnections":4,"activeInboundWorkers":4,"activeOutboundRounds":8,"authenticatedConnections":4,"authenticationFailures":0,"completedInboundRounds":0,"enabled":true,"failedInboundRounds":0,"limits":{"handshakeTimeoutMs":2000,"inboundKeyCooldownMs":500,"maxHandshakesPerSecond":8,"maxInboundWorkers":4,"maxMessageBytes":1048576,"maxOutboundWorkers":8,"maxPeers":8,"maxRoundBlocks":256,"maxRoundBytes":8388608,"maxRoundRequests":64,"roundTimeoutMs":10000},"listenAddress":"127.0.0.1:64513","localPeerId":"7123b8762614eff919fae17b68ec1d1d7a357b70cb61f4e4ed93ea72099484ed","peakInboundWorkers":4,"peakOutboundRounds":8,"peers":[{"address":"127.0.0.1:64515","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"9b0e4117c92abc3fb41b73199bb0e07a918f6a468c1fa2fca595e482af4d3c11","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:64516","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"c29d1215729e0cbea8e555a1906503cbcca9b60d831627607f4c59491f4ad884","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:64517","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"7bbd2ffa3590a19d44384d4bd8b2a44db3e129205dbd27b5484ebcc0de14d842","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:64518","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"b3feff5c7e7ffe92dc30e5cb52bc4a32693ee28e396b6fa31fdf413638af250e","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:64519","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"e0a55c86b5730af552dc9066e41080ff1795f34347bb3eb03261cc9341abd0a7","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:64520","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"7e6c5a044eba486ce2be92cdd9c822dffaea77a89361bc904cfa534db7c0196c","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:64521","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"a115a60f4ff8721859de43fcc9f56ace97dcfdce6d4c117da7c97e9d85c57ccf","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:64522","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"2a6a57fdb29aea0c4b2a21977d55bce08b7cd0febc6c02ec3ddf9438358302f1","retryDelayMs":0,"state":"not_connected","successfulRounds":0}],"rejectedConnections":1,"rejectedPeerRounds":0,"running":true},"rpc":{"activeDiagnostics":1,"activeRequests":8,"diagnosticLimit":2,"requestLimit":8},"schema":"boole.native.diagnostics.v1","stopping":false},"partialBytesPerPeer":[8188312,8188312,8188312,8188312,8188312,8188312,8188312,8188312],"stopMicros":498}
mixed-result {"elapsedMs":21800,"fundedBlocks":2,"genesisHash":"933a672120674efa9ec6205f344e1830febdec58b0ffcd2603ccc8a9723610c1","head":"00009efa3c0a5376c32ed2818d39c89193e09180d72fbfe155a586df1f5f1887","issued":"65000000000000","maxAppendMs":205,"maxCandidateAppendMs":180,"maxTemplateMs":172,"networkId":"boole-native-testnet-1","resources":{"balanceEntries":1025,"confirmedTransfers":1024,"historyBlocks":13,"historyBytes":552737,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":1,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0},"restartMs":399}
ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 13 filtered out; finished in 21.80s
```

### Large mixed-resource follow-up and exact source/executable identities

Command: `/usr/bin/time -l /Users/seoyong/projects/Boole/target/debug/deps/native_capacity-6f26ba504621f258 --ignored --exact mixed::native_mixed_p2p_http_pressure_131072_accounts --nocapture --test-threads=1`

```text
ecb549cb4cf646648eee269304898a8c2c53f7a5
51c917e9d761a5c3c67d49f3f78963540c9d8aeb
60b09f7048081a83eebcb05f1a3cd1091588ae566dec977f73dfd646971a9f50  /Users/seoyong/projects/Boole/target/debug/deps/native_capacity-6f26ba504621f258

running 1 test
test mixed::native_mixed_p2p_http_pressure_131072_accounts ... mixed-progress fundedBlocks=32 elapsedMs=19192
mixed-progress fundedBlocks=64 elapsedMs=38216
mixed-progress fundedBlocks=96 elapsedMs=57459
mixed-progress fundedBlocks=128 elapsedMs=77345
mixed-progress fundedBlocks=160 elapsedMs=97470
mixed-progress fundedBlocks=192 elapsedMs=116899
mixed-progress fundedBlocks=224 elapsedMs=136756
mixed-progress fundedBlocks=256 elapsedMs=156935
mixed-network {"advertised":{"hash":"000092c6b9368f84167d9bdbe28083aaa3c3a54ec6f0886623daef36cd3ca5cc","height":299},"diagnosticMaxMicros":456,"drained":{"authority":"local_process_only","ledgerReadiness":"not_checked","peers":{"acceptedConnections":4,"activeInboundWorkers":0,"activeOutboundRounds":0,"authenticatedConnections":4,"authenticationFailures":0,"completedInboundRounds":0,"enabled":true,"failedInboundRounds":4,"limits":{"handshakeTimeoutMs":2000,"inboundKeyCooldownMs":500,"maxHandshakesPerSecond":8,"maxInboundWorkers":4,"maxMessageBytes":1048576,"maxOutboundWorkers":8,"maxPeers":8,"maxRoundBlocks":256,"maxRoundBytes":8388608,"maxRoundRequests":64,"roundTimeoutMs":10000},"listenAddress":"127.0.0.1:49290","localPeerId":"b597500b3fc4d3355ea89fe739b8eb37155eba89d3c3dd1074f54f4a35556389","peakInboundWorkers":4,"peakOutboundRounds":8,"peers":[{"address":"127.0.0.1:49292","consecutiveFailures":1,"failedRounds":1,"lastFailureStage":"block_download","peerId":"f86716024d2d7da1f3027c11853751df03b314569f1f37514ae6dc3894b29b5c","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:49293","consecutiveFailures":1,"failedRounds":1,"lastFailureStage":"block_download","peerId":"ec3b6349b36903916226ebb4734849149bc14de2059770d5724c6cb5af89569a","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:49295","consecutiveFailures":1,"failedRounds":1,"lastFailureStage":"block_download","peerId":"1ac87474f3d34a6d56f1279c5a9abbc5bedc007ecaca9131e177183272e205dc","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:49296","consecutiveFailures":1,"failedRounds":1,"lastFailureStage":"block_download","peerId":"7f70d9af1d25b9d9dacdf279aac23be84fafd2fbfff4d9d5bac4651518989989","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:49297","consecutiveFailures":1,"failedRounds":1,"lastFailureStage":"block_download","peerId":"221a936ccddf7068d26090d6a990883bf9f4d9bcca0a52f3b03d283d9a42bda5","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:49298","consecutiveFailures":1,"failedRounds":1,"lastFailureStage":"block_download","peerId":"0a96a8b3e063f4eecf384d9d7cabf7e00fa1dc8956b6eb5797f6b0bbf52354a3","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:49299","consecutiveFailures":1,"failedRounds":1,"lastFailureStage":"block_download","peerId":"c24c0c21a34355dab82ef13ab287929f396d5c5d6a219b223b8c1889104342f1","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:49300","consecutiveFailures":1,"failedRounds":1,"lastFailureStage":"block_download","peerId":"1781555fe7597926da1849c29327aabcee5a24139f5f9a47f32311f3eea9abc4","retryDelayMs":500,"state":"retrying","successfulRounds":0}],"rejectedConnections":1,"rejectedPeerRounds":0,"running":true},"rpc":{"activeDiagnostics":1,"activeRequests":0,"diagnosticLimit":2,"requestLimit":8},"schema":"boole.native.diagnostics.v1","stopping":false},"elapsedMs":182078,"finalSentBytesPerPeer":[9034930,9034930,9034930,9034930,9034930,9034930,9034930,9034930],"fundedBlocks":256,"httpBodyBytesSentPerClient":8388607,"localHead":"000002700a9b5f795da3ff71cad2588c0180e2f6213ba0f0e983ea42b1b63d1b","networkMs":3118,"overlap":{"authority":"local_process_only","ledgerReadiness":"not_checked","peers":{"acceptedConnections":4,"activeInboundWorkers":4,"activeOutboundRounds":8,"authenticatedConnections":4,"authenticationFailures":0,"completedInboundRounds":0,"enabled":true,"failedInboundRounds":0,"limits":{"handshakeTimeoutMs":2000,"inboundKeyCooldownMs":500,"maxHandshakesPerSecond":8,"maxInboundWorkers":4,"maxMessageBytes":1048576,"maxOutboundWorkers":8,"maxPeers":8,"maxRoundBlocks":256,"maxRoundBytes":8388608,"maxRoundRequests":64,"roundTimeoutMs":10000},"listenAddress":"127.0.0.1:49290","localPeerId":"b597500b3fc4d3355ea89fe739b8eb37155eba89d3c3dd1074f54f4a35556389","peakInboundWorkers":4,"peakOutboundRounds":8,"peers":[{"address":"127.0.0.1:49292","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"f86716024d2d7da1f3027c11853751df03b314569f1f37514ae6dc3894b29b5c","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:49293","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"ec3b6349b36903916226ebb4734849149bc14de2059770d5724c6cb5af89569a","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:49295","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"1ac87474f3d34a6d56f1279c5a9abbc5bedc007ecaca9131e177183272e205dc","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:49296","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"7f70d9af1d25b9d9dacdf279aac23be84fafd2fbfff4d9d5bac4651518989989","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:49297","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"221a936ccddf7068d26090d6a990883bf9f4d9bcca0a52f3b03d283d9a42bda5","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:49298","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"0a96a8b3e063f4eecf384d9d7cabf7e00fa1dc8956b6eb5797f6b0bbf52354a3","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:49299","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"c24c0c21a34355dab82ef13ab287929f396d5c5d6a219b223b8c1889104342f1","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:49300","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"1781555fe7597926da1849c29327aabcee5a24139f5f9a47f32311f3eea9abc4","retryDelayMs":0,"state":"not_connected","successfulRounds":0}],"rejectedConnections":1,"rejectedPeerRounds":0,"running":true},"rpc":{"activeDiagnostics":1,"activeRequests":8,"diagnosticLimit":2,"requestLimit":8},"schema":"boole.native.diagnostics.v1","stopping":false},"partialBytesPerPeer":[8213762,8213762,8213762,8213762,8213762,8213762,8213762,8213762],"stopMicros":39786}
mixed-result {"elapsedMs":234772,"fundedBlocks":256,"genesisHash":"933a672120674efa9ec6205f344e1830febdec58b0ffcd2603ccc8a9723610c1","head":"000002700a9b5f795da3ff71cad2588c0180e2f6213ba0f0e983ea42b1b63d1b","issued":"1335000000000000","maxAppendMs":233,"maxCandidateAppendMs":196,"maxTemplateMs":198,"networkId":"boole-native-testnet-1","resources":{"balanceEntries":131073,"confirmedTransfers":131072,"historyBlocks":267,"historyBytes":69958251,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":1,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0},"restartMs":51871}
ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 13 filtered out; finished in 234.81s

      234.81 real       235.95 user         0.65 sys
           654180352  maximum resident set size
                   0  average shared memory size
                   0  average unshared data size
                   0  average unshared stack size
               42601  page reclaims
                   0  page faults
                   0  swaps
                   0  block input operations
                   0  block output operations
                6327  messages sent
               10810  messages received
                   0  signals received
                1768  voluntary context switches
                3408  involuntary context switches
       3662673365824  instructions retired
       1018928527756  cycles elapsed
           453952472  peak memory footprint
ecb549cb4cf646648eee269304898a8c2c53f7a5
51c917e9d761a5c3c67d49f3f78963540c9d8aeb
60b09f7048081a83eebcb05f1a3cd1091588ae566dec977f73dfd646971a9f50  /Users/seoyong/projects/Boole/target/debug/deps/native_capacity-6f26ba504621f258
```

### First held-input small regression

```text
   Compiling ring v0.17.14
   Compiling boole-core v0.1.0 (/Users/seoyong/projects/Boole/crates/boole-core)
   Compiling boole-native-shadow-protocol v0.1.0 (/Users/seoyong/projects/Boole/crates/boole-native-shadow-protocol)
   Compiling boole-evm-adapter v0.1.0 (/Users/seoyong/projects/Boole/crates/boole-evm-adapter)
   Compiling rustls v0.23.45
   Compiling rustls-webpki v0.103.15
   Compiling boole-testkit v0.1.0 (/Users/seoyong/projects/Boole/crates/boole-testkit)
   Compiling boole-p2p v0.1.0 (/Users/seoyong/projects/Boole/crates/boole-p2p)
   Compiling boole-node v0.1.0 (/Users/seoyong/projects/Boole/crates/boole-node)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 10.63s
     Running tests/native_capacity.rs (target/debug/deps/native_capacity-6f26ba504621f258)

running 1 test
test mixed::small_mixed_shutdown_closes_held_inputs_and_replays_unchanged ... boole-node: native HTTP drain expired; closing remaining client sockets
mixed-network {"advertised":{"hash":"0000238874213f313ec8275b19912f2e9e9630a5481480e8dc3288e937507042","height":45},"diagnosticMaxMicros":435,"drained":{"closedHttp":8,"closedInbound":4,"closedOutbound":8,"completedMissingInputs":false},"elapsedMs":25944,"finalSentBytesPerPeer":[8188312,8188312,8188312,8188312,8188312,8188312,8188312,8188312],"fundedBlocks":2,"httpBodyBytesSentPerClient":8388607,"localHead":"00009efa3c0a5376c32ed2818d39c89193e09180d72fbfe155a586df1f5f1887","networkMs":5545,"overlap":{"authority":"local_process_only","ledgerReadiness":"not_checked","peers":{"acceptedConnections":4,"activeInboundWorkers":4,"activeOutboundRounds":8,"authenticatedConnections":4,"authenticationFailures":0,"completedInboundRounds":0,"enabled":true,"failedInboundRounds":0,"limits":{"handshakeTimeoutMs":2000,"inboundKeyCooldownMs":500,"maxHandshakesPerSecond":8,"maxInboundWorkers":4,"maxMessageBytes":1048576,"maxOutboundWorkers":8,"maxPeers":8,"maxRoundBlocks":256,"maxRoundBytes":8388608,"maxRoundRequests":64,"roundTimeoutMs":10000},"listenAddress":"127.0.0.1:52062","localPeerId":"926f78ee3dd521114d277210f06bc89fe74c0db2fa153f78ef8d7cdb603ef138","peakInboundWorkers":4,"peakOutboundRounds":8,"peers":[{"address":"127.0.0.1:52064","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"c15b3b6ca05e92771bba89f0470b79c85e84f99f27a346bcfaf647837b9a73e5","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:52065","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"2ce156ca0401c1ec08dff75d30482d0298efab84e1069c8d7a503b72d5658618","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:52066","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"e94aad8680cf2c44914f0571effc9d79bbd37a75ba71c50a549e854c945c0591","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:52067","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"a4ea514419428e6f8f131d5276978f41816a856354e8fd5e3ff7cf3373ecf2d5","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:52068","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"48ee0ea4f35b035ec498683ffb7f5600d38c13e0f006619e3d022cb713ce4267","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:52069","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"67ed9a373bd30c4c06b2c58008e5423ded2c7829e9d40a9f114f236d6b4b1561","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:52070","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"012a7ea596e207b2f073c726cacc295c779688023b363b8cc04aac08a2474ff0","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:52071","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"7970f91663bba852214da3664f92abd1234b55cee3c83b4b54574815f2ae593a","retryDelayMs":0,"state":"not_connected","successfulRounds":0}],"rejectedConnections":1,"rejectedPeerRounds":0,"running":true},"rpc":{"activeDiagnostics":1,"activeRequests":8,"diagnosticLimit":2,"requestLimit":8},"schema":"boole.native.diagnostics.v1","stopping":false},"partialBytesPerPeer":[8188312,8188312,8188312,8188312,8188312,8188312,8188312,8188312],"stopMicros":5007066,"stopWithHeldInputs":true}
mixed-result {"elapsedMs":26365,"fundedBlocks":2,"genesisHash":"933a672120674efa9ec6205f344e1830febdec58b0ffcd2603ccc8a9723610c1","head":"00009efa3c0a5376c32ed2818d39c89193e09180d72fbfe155a586df1f5f1887","issued":"65000000000000","maxAppendMs":204,"maxCandidateAppendMs":178,"maxTemplateMs":171,"networkId":"boole-native-testnet-1","resources":{"balanceEntries":1025,"confirmedTransfers":1024,"historyBlocks":13,"historyBytes":552737,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":1,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0},"restartMs":409,"stopWithHeldInputs":true}
ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 15 filtered out; finished in 26.37s
```

### Shared original fixture and focused static checks

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.08s
     Running tests/native_capacity.rs (target/debug/deps/native_capacity-6f26ba504621f258)

running 1 test
test mixed::small_mixed_p2p_http_pressure_preserves_state_and_diagnostics ... mixed-network {"advertised":{"hash":"0000238874213f313ec8275b19912f2e9e9630a5481480e8dc3288e937507042","height":45},"diagnosticMaxMicros":446,"drained":{"authority":"local_process_only","ledgerReadiness":"not_checked","peers":{"acceptedConnections":4,"activeInboundWorkers":0,"activeOutboundRounds":0,"authenticatedConnections":4,"authenticationFailures":0,"completedInboundRounds":0,"enabled":true,"failedInboundRounds":4,"limits":{"handshakeTimeoutMs":2000,"inboundKeyCooldownMs":500,"maxHandshakesPerSecond":8,"maxInboundWorkers":4,"maxMessageBytes":1048576,"maxOutboundWorkers":8,"maxPeers":8,"maxRoundBlocks":256,"maxRoundBytes":8388608,"maxRoundRequests":64,"roundTimeoutMs":10000},"listenAddress":"127.0.0.1:52263","localPeerId":"dd3c38b95bdba69f64a0dff96ca91e0a9cb800f6eacf0133c96024bdbffae065","peakInboundWorkers":4,"peakOutboundRounds":8,"peers":[{"address":"127.0.0.1:52265","consecutiveFailures":1,"failedRounds":1,"lastFailureStage":"block_download","peerId":"fd062616053a796add040f5c632d5e0f61736f80945a32b805b84d874e830ad7","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:52266","consecutiveFailures":1,"failedRounds":1,"lastFailureStage":"block_download","peerId":"e6b7aadd43df631d934b28876776792f7bc8608bf39464358538635cf07f57fd","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:52267","consecutiveFailures":1,"failedRounds":1,"lastFailureStage":"block_download","peerId":"2a3730b14daf631bab1fe7e584e76ccdddfa2cfe5f222934709de57d5bb24a22","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:52268","consecutiveFailures":1,"failedRounds":1,"lastFailureStage":"block_download","peerId":"1887eedaa7763bf65bbecf44382bbb9411de2f4ad3746a1c9eebee21d22d0871","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:52269","consecutiveFailures":1,"failedRounds":1,"lastFailureStage":"block_download","peerId":"a65db44f4062d33919c1ce860c6f0d5ee2d6a54c100b7fe5523f4d16f2c45518","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:52270","consecutiveFailures":1,"failedRounds":1,"lastFailureStage":"block_download","peerId":"7cca1b0ae9d06b65475273142586de25a0031e058611f9574b5c5fee26248482","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:52271","consecutiveFailures":1,"failedRounds":1,"lastFailureStage":"block_download","peerId":"b32a4c4b626e7fc160ef4919a5cde631f0eb7b28f3e4845a932b049aa235c28e","retryDelayMs":500,"state":"retrying","successfulRounds":0},{"address":"127.0.0.1:52272","consecutiveFailures":1,"failedRounds":1,"lastFailureStage":"block_download","peerId":"4798cb1245a012115032586b6605fc1bac529869c4499e4ab5d2e326e41c5837","retryDelayMs":500,"state":"retrying","successfulRounds":0}],"rejectedConnections":1,"rejectedPeerRounds":0,"running":true},"rpc":{"activeDiagnostics":1,"activeRequests":0,"diagnosticLimit":2,"requestLimit":8},"schema":"boole.native.diagnostics.v1","stopping":false},"elapsedMs":21222,"finalSentBytesPerPeer":[9007936,9007936,9007936,9007936,9007936,9007936,9007936,9007936],"fundedBlocks":2,"httpBodyBytesSentPerClient":8388607,"localHead":"00009efa3c0a5376c32ed2818d39c89193e09180d72fbfe155a586df1f5f1887","networkMs":912,"overlap":{"authority":"local_process_only","ledgerReadiness":"not_checked","peers":{"acceptedConnections":4,"activeInboundWorkers":4,"activeOutboundRounds":8,"authenticatedConnections":4,"authenticationFailures":0,"completedInboundRounds":0,"enabled":true,"failedInboundRounds":0,"limits":{"handshakeTimeoutMs":2000,"inboundKeyCooldownMs":500,"maxHandshakesPerSecond":8,"maxInboundWorkers":4,"maxMessageBytes":1048576,"maxOutboundWorkers":8,"maxPeers":8,"maxRoundBlocks":256,"maxRoundBytes":8388608,"maxRoundRequests":64,"roundTimeoutMs":10000},"listenAddress":"127.0.0.1:52263","localPeerId":"dd3c38b95bdba69f64a0dff96ca91e0a9cb800f6eacf0133c96024bdbffae065","peakInboundWorkers":4,"peakOutboundRounds":8,"peers":[{"address":"127.0.0.1:52265","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"fd062616053a796add040f5c632d5e0f61736f80945a32b805b84d874e830ad7","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:52266","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"e6b7aadd43df631d934b28876776792f7bc8608bf39464358538635cf07f57fd","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:52267","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"2a3730b14daf631bab1fe7e584e76ccdddfa2cfe5f222934709de57d5bb24a22","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:52268","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"1887eedaa7763bf65bbecf44382bbb9411de2f4ad3746a1c9eebee21d22d0871","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:52269","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"a65db44f4062d33919c1ce860c6f0d5ee2d6a54c100b7fe5523f4d16f2c45518","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:52270","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"7cca1b0ae9d06b65475273142586de25a0031e058611f9574b5c5fee26248482","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:52271","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"b32a4c4b626e7fc160ef4919a5cde631f0eb7b28f3e4845a932b049aa235c28e","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:52272","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"4798cb1245a012115032586b6605fc1bac529869c4499e4ab5d2e326e41c5837","retryDelayMs":0,"state":"not_connected","successfulRounds":0}],"rejectedConnections":1,"rejectedPeerRounds":0,"running":true},"rpc":{"activeDiagnostics":1,"activeRequests":8,"diagnosticLimit":2,"requestLimit":8},"schema":"boole.native.diagnostics.v1","stopping":false},"partialBytesPerPeer":[8188312,8188312,8188312,8188312,8188312,8188312,8188312,8188312],"stopMicros":550,"stopWithHeldInputs":false}
mixed-result {"elapsedMs":21628,"fundedBlocks":2,"genesisHash":"933a672120674efa9ec6205f344e1830febdec58b0ffcd2603ccc8a9723610c1","head":"00009efa3c0a5376c32ed2818d39c89193e09180d72fbfe155a586df1f5f1887","issued":"65000000000000","maxAppendMs":199,"maxCandidateAppendMs":178,"maxTemplateMs":173,"networkId":"boole-native-testnet-1","resources":{"balanceEntries":1025,"confirmedTransfers":1024,"historyBlocks":13,"historyBytes":552737,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":1,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0},"restartMs":396,"stopWithHeldInputs":false}
ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 15 filtered out; finished in 21.63s

    Checking ring v0.17.14
    Checking boole-core v0.1.0 (/Users/seoyong/projects/Boole/crates/boole-core)
    Checking boole-native-shadow-protocol v0.1.0 (/Users/seoyong/projects/Boole/crates/boole-native-shadow-protocol)
    Checking boole-lean-runner v0.1.0 (/Users/seoyong/projects/Boole/crates/boole-lean-runner)
    Checking boole-evm-adapter v0.1.0 (/Users/seoyong/projects/Boole/crates/boole-evm-adapter)
    Checking rustls-webpki v0.103.15
    Checking rustls v0.23.45
    Checking boole-p2p v0.1.0 (/Users/seoyong/projects/Boole/crates/boole-p2p)
    Checking boole-testkit v0.1.0 (/Users/seoyong/projects/Boole/crates/boole-testkit)
    Checking boole-node v0.1.0 (/Users/seoyong/projects/Boole/crates/boole-node)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.47s
docs-smoke: PASS
```

### First large held-input shutdown measurement and identity checks

Command: `/usr/bin/time -l /Users/seoyong/projects/Boole/target/debug/deps/native_capacity-6f26ba504621f258 --ignored --exact mixed::native_mixed_shutdown_131072_accounts --nocapture --test-threads=1`

```text
 M tasks/lessons.md
eb97bb092a755279d21cec4f29f6612cf8fdc089
5c5bd5ae1420e608a29eba493c886a0b1ed05797
366d3bdc75c770d6f366b1e228a0d93ff3bfb235962a81e83a4eef11ab9c3174  target/debug/deps/native_capacity-6f26ba504621f258
0e071475840f2ec9b36549507c18004847923eeb121b9baf86b6338ab76ff793  tasks/lessons.md

running 1 test
test mixed::native_mixed_shutdown_131072_accounts ... mixed-progress fundedBlocks=32 elapsedMs=19257
mixed-progress fundedBlocks=64 elapsedMs=38398
mixed-progress fundedBlocks=96 elapsedMs=57737
mixed-progress fundedBlocks=128 elapsedMs=77842
mixed-progress fundedBlocks=160 elapsedMs=98068
mixed-progress fundedBlocks=192 elapsedMs=117624
mixed-progress fundedBlocks=224 elapsedMs=137600
mixed-progress fundedBlocks=256 elapsedMs=157922
boole-node: native HTTP drain expired; closing remaining client sockets
mixed-network {"advertised":{"hash":"000092c6b9368f84167d9bdbe28083aaa3c3a54ec6f0886623daef36cd3ca5cc","height":299},"diagnosticMaxMicros":438,"drained":{"closedHttp":8,"closedInbound":4,"closedOutbound":8,"completedMissingInputs":false},"elapsedMs":187080,"finalSentBytesPerPeer":[8213762,8213762,8213762,8213762,8213762,8213762,8213762,8213762],"fundedBlocks":256,"httpBodyBytesSentPerClient":8388607,"localHead":"000002700a9b5f795da3ff71cad2588c0180e2f6213ba0f0e983ea42b1b63d1b","networkMs":7082,"overlap":{"authority":"local_process_only","ledgerReadiness":"not_checked","peers":{"acceptedConnections":4,"activeInboundWorkers":4,"activeOutboundRounds":8,"authenticatedConnections":4,"authenticationFailures":0,"completedInboundRounds":0,"enabled":true,"failedInboundRounds":0,"limits":{"handshakeTimeoutMs":2000,"inboundKeyCooldownMs":500,"maxHandshakesPerSecond":8,"maxInboundWorkers":4,"maxMessageBytes":1048576,"maxOutboundWorkers":8,"maxPeers":8,"maxRoundBlocks":256,"maxRoundBytes":8388608,"maxRoundRequests":64,"roundTimeoutMs":10000},"listenAddress":"127.0.0.1:53066","localPeerId":"98bd8dc9100974def01ce85f6e70f8867efe511b6ce5ca2c6ec5a56caf8da949","peakInboundWorkers":4,"peakOutboundRounds":8,"peers":[{"address":"127.0.0.1:53068","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"1afa7af6c84c534c52585457cee8e85a5492d2640ec0be34bbf4208974edf77b","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:53069","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"d4bf8f3042cc15bfb9cf1f2236f9935cae04a733ca6a7a08bf5b468dd6c1b8bc","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:53070","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"86f125697a8cf67d25461576bd1fdfd5990e10d4d6d5617052bdf7e7b43af7c6","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:53071","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"de03d68c2c9904ca827f292a13918a60c3ff15336eecca5cad442640e251c4cd","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:53072","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"8ea0c2486ccce6dd1dbff0c4b3a03b9f36b25925f3ab06bfd593118a99c6953f","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:53073","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"b824249f2c72bf31905c132d7e05b9307deaf3dd7a87b0df6da293b454a1f975","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:53074","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"f0aed3b191712479f3187a48892c7d5b27c2103e8b2b2fa7e3c96373637913d7","retryDelayMs":0,"state":"not_connected","successfulRounds":0},{"address":"127.0.0.1:53075","consecutiveFailures":0,"failedRounds":0,"lastFailureStage":null,"peerId":"8030107bc5fec5b0991772a4d8ed9327302db6b2f5287c2d4ba29871fc8a3849","retryDelayMs":0,"state":"not_connected","successfulRounds":0}],"rejectedConnections":1,"rejectedPeerRounds":0,"running":true},"rpc":{"activeDiagnostics":1,"activeRequests":8,"diagnosticLimit":2,"requestLimit":8},"schema":"boole.native.diagnostics.v1","stopping":false},"partialBytesPerPeer":[8213762,8213762,8213762,8213762,8213762,8213762,8213762,8213762],"stopMicros":5071433,"stopWithHeldInputs":true}
mixed-result {"elapsedMs":239702,"fundedBlocks":256,"genesisHash":"933a672120674efa9ec6205f344e1830febdec58b0ffcd2603ccc8a9723610c1","head":"000002700a9b5f795da3ff71cad2588c0180e2f6213ba0f0e983ea42b1b63d1b","issued":"1335000000000000","maxAppendMs":234,"maxCandidateAppendMs":192,"maxTemplateMs":209,"networkId":"boole-native-testnet-1","resources":{"balanceEntries":131073,"confirmedTransfers":131072,"historyBlocks":267,"historyBytes":69958251,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":1,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0},"restartMs":51803,"stopWithHeldInputs":true}
ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 15 filtered out; finished in 239.74s

      239.75 real       235.71 user         0.63 sys
           640024576  maximum resident set size
                   0  average shared memory size
                   0  average unshared data size
                   0  average unshared stack size
               39948  page reclaims
                   0  page faults
                   0  swaps
                   0  block input operations
                   0  block output operations
                5771  messages sent
                7793  messages received
                   0  signals received
                1732  voluntary context switches
                2983  involuntary context switches
       3647336256193  instructions retired
       1017367569113  cycles elapsed
           427295656  peak memory footprint
 M tasks/lessons.md
eb97bb092a755279d21cec4f29f6612cf8fdc089
5c5bd5ae1420e608a29eba493c886a0b1ed05797
366d3bdc75c770d6f366b1e228a0d93ff3bfb235962a81e83a4eef11ab9c3174  target/debug/deps/native_capacity-6f26ba504621f258
0e071475840f2ec9b36549507c18004847923eeb121b9baf86b6338ab76ff793  tasks/lessons.md
```
