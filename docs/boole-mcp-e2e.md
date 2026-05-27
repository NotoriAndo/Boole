# boole-mcp end-to-end smoke (external-user path)

This document captures the external-user end-to-end (e2e) smoke flow for
`boole-mcp`: from a fresh `$HOME`, install the MCP server into an
IDE-compatible settings file, start the server, and drive a zero-cost
fixture mining round-trip through the `boole.mine` and `boole.status`
tools.

Scope: closed local smoke; not public-network mining. The `boole.mine`
tool exercised here runs a `MiningLoopOptions { max_cycles: Some(0), ..
}` zero-cycle round-trip on in-process mocks (`MockDriver`,
`RejectingVerifier`, `StubTargetEmitter`, `StructuralCanonicalizer`),
so no real proof work, no Lean elaboration, no HTTP loopback to a
real `boole-node`, and no paid-API calls are made. The block-produced
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

For the e2e smoke, run the server directly (the IDE invokes the same
command via `mcpServers.boole.command` once installed):

```
./target/release/boole-mcp serve --node-url http://127.0.0.1:8080 --listen 127.0.0.1:0
```

The server echoes the resolved bind address to stderr as:

```
boole-mcp listening on http://127.0.0.1:<ephemeral-port>
```

Capture the port for the remaining steps. `--node-url` is required by
the CLI but only consulted by the upstream-proxying tools
(`bounty.list`, `receipt.get`); the in-process mining tools
(`boole.mine`, `boole.status`) do not contact the upstream URL.

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
  {"name":"boole.status", ...}
]}
```

Each entry carries a `description` string and an `input_schema`
object.

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
