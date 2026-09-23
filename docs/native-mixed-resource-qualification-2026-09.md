# Native mixed input-pressure qualification — 2026-09-23

Status: **PREREGISTERED — not yet executed.** This combines specific already
bounded P2P and loopback HTTP inputs. It is not general mixed-traffic, successful
competing-adoption, continuously changing-candidate or public-network acceptance.

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
