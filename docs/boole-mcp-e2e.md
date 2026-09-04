# boole-mcp end-to-end smoke (external-user path)

This document captures the external-user end-to-end (e2e) smoke flow for
`boole-mcp`: from a fresh `$HOME`, install the MCP server into an
IDE-compatible settings file, start the server, and drive a zero-cost
fixture mining round-trip through the `boole.mine` and `boole.status`
tools, plus the closed-local `boole.verify_native` bridge when the separate
native verifier service is running.

Scope: closed local smoke; not public-network mining. The `boole.mine`
tool exercised here runs a `MiningLoopOptions { max_cycles: Some(0), ..
}` zero-cycle round-trip: the loop body short-circuits before any
collaborator call, so no real proof work, no Lean elaboration, no
HTTP loopback to a real `boole-node`, and no paid-API calls are made.
The in-process collaborators wired behind `boole.mine` are
`CanonicalProofDriver`, `RejectingVerifier`,
`FamilyV1LengthBoundTargetEmitter`, and `StructuralCanonicalizer`;
with `max_cycles >= 1` they drive a real v1-lenbound instance through
the proof-intake pipeline and report honest counters
(`driver_answered`, `proof_intake_accepted`, `verify_rejected`,
`loop_class = "smoke"`) — still a closed-local smoke with no Lean
toolchain dependency and no paid-API calls. The block-produced
end-state from the master plan §6.5 P2.2 criterion 3 wording maps to
the `{"state":"completed","last_summary":{...}}` envelope returned by
`boole.status` after a successful `boole.mine`.

Supported platforms: macOS and Linux. Instructions in this document
are platform-agnostic except where explicitly noted.

## Prerequisites

- Rust `1.95.0` toolchain (see [docs/install.md](install.md) for the
  full installer including Lean and Ollama optional gates).
- A clean repository checkout of `Boole/`.
- A writable `$HOME` (any IDE-config target directory under `$HOME`
  is created on demand by `boole-mcp install`).

No paid-API credentials are required. No public-network mining is
performed.

Native verification is a separate, optional prerequisite: the node-owned
native service must already be listening on a numeric loopback origin (the
installed default is `http://127.0.0.1:8082`). `boole-mcp` does not start the
checker, own its ledger, or expose it on a remote address.

## Step 1 — build the boole-mcp binary

From the repository root:

```
cargo build --release -p boole-mcp --bin boole-mcp
```

The build embeds the runtime-smoke fixture into the binary via
`include_bytes!`, so the resulting binary has zero filesystem
dependency on `fixtures/` at the user's host.

Verify the binary identifies itself:

```
./target/release/boole-mcp --version
```

Expected (the SHA and build UTC vary per build):

```
boole-mcp 0.1.0 (sha=<12-char-git-sha> build=<iso-8601-utc>)
```

## Step 2 — inspect the planned IDE install

Before mutating any IDE settings file, dry-run the install to see the
exact JSON merge that would be written. Pick the target matching your
IDE:

```
./target/release/boole-mcp install --target claude --dry-run
./target/release/boole-mcp install --target codex --dry-run
./target/release/boole-mcp install --target cursor --dry-run
./target/release/boole-mcp install --target opencode --dry-run
```

The stdout response is a unified envelope:

```
{"ok":true,"version":"v1","command":"install","result":{"dry_run":true,"target":"<ide>","settings_path":"<path>","planned_content":{...}}}
```

`planned_content` shows the post-merge JSON that would be written to
`settings_path`. The merge is idempotent: re-running install on an
already-installed entry is a no-op for `mcpServers.boole` and
preserves every other top-level setting.

Canonical settings paths (relative to `$HOME`):

- `claude` → `.claude/settings.json`
- `codex` → `.codex/config.json`
- `cursor` → `.cursor/mcp.json`
- `opencode` → `.config/opencode/config.json`

## Step 3 — perform the IDE install

Once the dry-run looks correct, drop `--dry-run` to perform the
atomic write (stage to `<file>.json.tmp`, then rename):

```
./target/release/boole-mcp install --target <ide>
```

Expected stdout envelope:

```
{"ok":true,"version":"v1","command":"install","result":{"dry_run":false,"target":"<ide>","settings_path":"<path>"}}
```

Typed errors land on stderr (still unified-envelope):

- `home-not-set` — `$HOME` is unset.
- `settings-not-object` — existing settings file is not a JSON object.
- `settings-parse-failed` — existing settings file is unparseable JSON.
- `mcp-servers-not-object` — existing `mcpServers` key is not an
  object.

In each error case the existing file is left untouched; repair it by
hand and re-run install.

## Step 4 — start the boole-mcp server

For the e2e smoke, run the HTTP server surface directly. Note that an
installed IDE entry uses the stdio transport instead: `boole-mcp
install` registers separate `--node-url http://127.0.0.1:8080` and
`--native-shadow-url http://127.0.0.1:8082` arguments, and the IDE launches
`boole-mcp stdio` as a subprocess speaking JSON-RPC 2.0 over stdin/stdout with
Content-Length framing. The HTTP `serve` surface below exists for curl-driven
smokes like this one; both transports dispatch the same tools:

```
./target/release/boole-mcp serve \
  --node-url http://127.0.0.1:8080 \
  --native-shadow-url http://127.0.0.1:8082 \
  --listen 127.0.0.1:0
```

Equivalent one-line form: `./target/release/boole-mcp serve --node-url
http://127.0.0.1:8080 --native-shadow-url http://127.0.0.1:8082 --listen
127.0.0.1:0`.

The server echoes the resolved bind address to stderr as:

```
boole-mcp listening on http://127.0.0.1:<ephemeral-port>
```

Capture the port for the remaining steps. `--node-url` is required by
the CLI but only consulted by the upstream-proxying tools
(`bounty.list`, `receipt.get`); the in-process mining tools
(`boole.mine`, `boole.status`) do not contact the upstream URL.
`boole.verify_native` never uses that legacy node URL: it accepts only an
`http` numeric-loopback native origin that is distinct from the legacy origin
and has no credentials or extra path. When that native origin is configured,
the MCP HTTP `--listen` address must also be a numeric loopback socket address;
wildcard, non-loopback and hostname listeners are rejected before bind so the
MCP process cannot become an unauthenticated remote-to-loopback submission
bridge. Legacy-only `serve` without `--native-shadow-url` retains its existing
listener behavior.
Ambient proxies and redirects are disabled for this client.

## Step 5 — list available MCP tools

```
curl -s http://127.0.0.1:<port>/mcp/tools | jq .
```

Expected response (order may vary):

```
{"tools":[
  {"name":"bounty.list", ...},
  {"name":"receipt.get", ...},
  {"name":"boole.mine", ...},
  {"name":"boole.status", ...},
  {"name":"boole.verify_native", ...}
]}
```

Each entry carries a `description` string and identical `input_schema` /
`inputSchema` objects for the HTTP-compatibility and MCP stdio spellings.

## Step 6 — invoke boole.status (idle)

Before driving any mining, confirm the session-state read:

```
curl -s -H 'Content-Type: application/json' \
  -d '{"tool":"boole.status","args":{}}' \
  http://127.0.0.1:<port>/mcp/invoke
```

Expected response (HTTP 200):

```
{"state":"idle"}
```

The `idle` envelope is returned when no `boole.mine` invocation has
yet completed in the current `boole-mcp serve` process.

## Step 7 — invoke boole.mine (zero-cycle round-trip)

```
curl -s -H 'Content-Type: application/json' \
  -d '{"tool":"boole.mine","args":{}}' \
  http://127.0.0.1:<port>/mcp/invoke
```

Expected response (HTTP 200):

```
{"cycles_run":0,"tickets_found":0,"shares_accepted":0,"network_errors":0}
```

All four counters are 0 because the round-trip runs with
`max_cycles: Some(0)` — the loop body short-circuits before any
driver/verifier/Lean call, so no real proof work happens. The point of
this step is to verify end-to-end MCP → `MiningLoopDeps` →
`run_mining_loop` plumbing, not to mine a block.

## Step 8 — invoke boole.status (after mine)

```
curl -s -H 'Content-Type: application/json' \
  -d '{"tool":"boole.status","args":{}}' \
  http://127.0.0.1:<port>/mcp/invoke
```

Expected response (HTTP 200):

```
{"state":"completed","last_summary":{"cycles_run":0,"tickets_found":0,"shares_accepted":0,"network_errors":0}}
```

The `completed` envelope reflects the protocol counters from the most
recent `boole.mine` invocation in the current `boole-mcp serve`
process. The slot is wiped when the process exits.

## Native verification — strict six-field bridge

With the native service running, submit exactly the six fields owned by that
service:

```
curl -s -H 'Content-Type: application/json' \
  -d '{"tool":"boole.verify_native","args":{"schema":"boole.native-shadow.submission.v1","familyVersion":"<family>","templateId":"<64-lowercase-hex>","challengeSha256":"<64-lowercase-hex>","epoch":0,"rawAnswer":"```rust\\n<answer>\\n```"}}' \
  http://127.0.0.1:<port>/mcp/invoke
```

Missing, extra, duplicated or wrongly typed fields are rejected before any
upstream request; duplicate keys are detected by reparsing the original JSON,
not inferred from a generic JSON value that overwrote one occurrence. This MCP
precheck deliberately validates only the exact six keys and their JSON types.
A negative integral `epoch` is therefore forwarded, while a fractional value
is rejected as the wrong JSON type. The native service remains the authority
for the schema value, family and challenge identity, digest syntax/meaning,
epoch policy, raw answer format and every content/length bound; MCP neither duplicates nor
weakens those decisions. A shape-valid request is sent exactly once to
`POST /native-shadow/submissions`. The native JSON response is returned as a
whole, including `outcome`, `reasonCode`, `redelivered`, `evidenceDigest` and
the BF.3 `receipt`; it is not converted into the legacy `/receipts/{id}`
`ReceiptCommitment` vocabulary.

On the HTTP surface, the native HTTP status and JSON body are both preserved.
On stdio, the JSON body is preserved and the upstream status is represented
only by MCP's success/error class (`isError`); no status field is injected into
or removed from the native JSON.

The client accepts at most 64 KiB of response data and waits 120 seconds, just
beyond the verifier's frozen 115-second outer deadline. It follows no redirect
and performs no automatic retry. If any response cannot be forwarded after the
request may have reached the service—including a connection loss, oversized
body or invalid JSON—the MCP response says only that the outcome is unknown,
with the transport problem as non-verdict detail. Resubmit the identical six
fields manually: the native service can return its durable terminal result with
`redelivered: true` without a second checker execution, even if the MCP process
restarted in between.

That last property is backed by two adjoining layers rather than a mock being
called a real checker. The MCP transport E2E observes the exact request,
ambiguous disconnect, process restart, manual replay and unchanged response.
The native service's own router and crash/restart E2E tests independently prove
that the same endpoint commits terminal ACCEPT/reject evidence durably and does
not launch the checker again on redelivery. Together they cover the boundary;
the MCP fixture alone is not evidence of a real checker execution.

## Transcript capture

The transcripts for this smoke are captured under
`tests/fixtures/boole-mcp-e2e/` (added in a follow-up slice; for now,
re-run the curl commands and verify the responses match this
document).

## Boundary statements

- This is closed local smoke; not public-network mining.
- No paid-API calls are made; no public scoring is claimed.
- The `boole.mine` round-trip uses in-process mocks; no real proof
  artifact is produced.
- `boole.verify_native` is the only tool in this document that can run a real
  checker, and it does so only through the node-owned isolated loopback service.
  It never calls the legacy `/receipts` route or wallet, payment, block or
  reward code.
- The MCP install surface does not exfiltrate any key material;
  signing isolation lives in `boole-wallet-agent`, not `boole-mcp`.

## Cross-reference

- Master plan §6.5 P2.1 / P2.2 (closure criteria for boole-mcp).
- [docs/install.md](install.md) — full installer flow (Lean, Rust,
  Ollama optional gates).
- `crates/boole-mcp/src/main.rs` — `serve`, `install` subcommands.
- `crates/boole-mcp/src/lib.rs` — `RUNTIME_SMOKE_FIXTURE_BYTES`,
  `build_in_process_mining_deps`.
- `crates/boole-mcp/tests/mining_tool_surface.rs` —
  `tools_endpoint_now_lists_boole_mine_and_boole_status`,
  `invoke_boole_mine_zero_cycle_returns_protocol_summary_envelope_200`,
  `invoke_boole_status_returns_idle_envelope_200`,
  `invoke_boole_status_after_mine_returns_completed_envelope_200`.
