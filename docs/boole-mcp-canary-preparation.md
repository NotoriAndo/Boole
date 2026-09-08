# Real-model MCP canary preparation

Status: **PREPARATION COMPLETE; REAL EXECUTION NOT READY** (2026-09-08).

The preparation milestone identifies a client path, exercises the current MCP
transport without a model, and records the missing execution boundary. It does
not claim a real model submission, a new contained-checker run, or a new receipt.
The current cursor is [current development status](current-development-status.md).

## What the existing path actually accepts

The native MCP bridge accepts the exact six fields `schema`, `familyVersion`,
`templateId`, `challengeSha256`, `epoch`, and `rawAnswer`. It sends them to the
separate numeric-loopback native service. Its response contains the native
adjudication and, when present, the BF.3 receipt. `receipt.get` queries the legacy
node and is **not** a retrieval path for this native result. Recover a lost native
response by manually resubmitting the identical six fields; do not regenerate an
answer or infer rejection from a transport failure. See [MCP E2E](boole-mcp-e2e.md).

The installed native service is currently a **fixed-fixture replay** service,
not a general new-answer service. The compiled
[replay grant](../native/containment/native-shadow-closed-local-replay-grant-arm64-v1.json)
fixes four cases, their epochs, raw-answer/source digests and execution caps.
`match_submission` and `build_execution_request` in
[the grant implementation](../crates/boole-native-shadow-protocol/src/closed_local_replay_grant.rs)
require those exact bindings. A different model-generated answer is not permitted
merely because it has the correct six-field JSON shape.

Do not change the frozen grant, copy its accepted answer into a model prompt,
or bypass its digest checks to label a replay as a real solving canary.

## Client and authentication route

The preferred preparation route is an already-authenticated **Codex CLI STDIO
client**, when available. ChatGPT sign-in uses subscription access, while API-key
sign-in uses usage-based access; an authenticated session does not establish a
zero-cost run or authorize additional usage. Check the actual method with
`codex login status` without retaining credential values. See
[OpenAI authentication](https://learn.chatgpt.com/docs/auth).

Prepare an unregistered, disabled server entry until all execution gates pass:

- Run the exact, current `boole-mcp` binary in `stdio` mode.
- Give its child a cleared environment and only explicitly needed variables;
  model credentials belong to the client, not to the MCP child.
- Configure only `boole.verify_native` in `enabled_tools`; exclude mining,
  wallet/payment tools, and legacy receipt lookup from this canary.
- Use a numeric-loopback native origin distinct from the legacy node origin.
- Set the client tool timeout to 150 seconds, above the MCP upstream deadline
  of 120 seconds and its cleanup grace. A client timeout is not a verdict.
- Disable unrelated tools, shell execution, web search, hooks and subagents in
  the actual evaluation profile, and inspect the resulting tool inventory.
  An MCP tool allowlist alone does not disable the client's other tools.

STDIO configuration, environment forwarding, tool allowlists and timeout options
are documented in [OpenAI MCP configuration](https://learn.chatgpt.com/docs/extend/mcp?surface=cli).
Client feature controls are in the
[configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference).
Do not silently modify the user's global client configuration or copy auth files
into a preparation bundle. The local configuration was parsed with `codex mcp
get` using per-command overrides; no model session was started.

## Environment gate

The preparation host had no listener on the native loopback port. Starting an
MCP server does not start the node or qualify its launcher. Select and validate
an explicit install/runtime/journal tuple before a real run.

On macOS, the existing
[`boole-mac-native-shadow-replay-node`](../crates/boole-node/src/bin/boole-mac-native-shadow-replay-node.rs)
requires the install root, private runtime root, journal path, and product/guest
trust roots. The
[installed service](../crates/boole-node/src/native_shadow_replay_service.rs)
opens a verified signed product and uses a persistent private VM. The absence
of Docker or QEMU does not by itself make this Virtualization.framework path
unavailable. A previous boot result does not prove a current serving-ready path.

Keep historical journals and frozen image inputs unchanged. A future canary
needs its own bounded working state and a qualified launcher/checker path;
starting the existing replay service would still leave the new-answer grant
restriction in place.

## Proposed next implementation boundary — not execution approval

Add a separate, default-disabled **one-task fresh-answer canary capability**.
Retain the existing replay capability and its fixed fixtures unchanged. Before
implementation, resolve how the operator's authorization is verified and bound
to the exact task, checker, policy, toolchain, private journal and run identity.
It must not be self-issued by model input or promoted into production authority.

The proposed bounded behavior is:

1. One selected client session and one new answer candidate for one non-issuable
   task. One client session may contain multiple provider inference requests;
   it is not a promise of one billable API call.
2. At first admission, durably bind the exact answer/submission identity before
   checker execution. Enforce at most one checker execution across crash/restart.
3. Permit at most one operator-controlled identical redelivery for result
   recovery, with no second checker execution. Changed-answer retries, exhausted
   attempts and mismatched authority must fail before execution.
4. Preserve qualified containment, byte/resource limits, unknown-outcome
   handling and receipt/evidence identity. No host execution fallback.
5. Keep issuance, public P2P, payment, wallet movement, mining, rewards, consensus
   and activation unavailable. This is a development capability, not a public
   task-admission API.

Required tests include wrong task/checker/authority rejection, second-candidate
rejection, durable single-execution enforcement across restart, exact redelivery,
and contained checker ACCEPT/REJECT through the actual MCP path. Implementing
this changes runtime authority and requires normal behavior tests and full CI;
the documentation-only preparation PR does not implement or authorize it.

## Prompt, run budget and success criteria

Freeze the public input packet before a future model run: global contract,
family manifest, official helper if applicable, output format, then the selected
instance/task specification. Exclude accepted-answer fixtures, hidden tests and
private hints. Do not give the evaluation client repository-wide tools that can
retrieve those answers. Leave task identifiers unresolved until a valid canary
task/authority is selected; fixture IDs are not a new run grant.

Proposed limits are one client session, one candidate, at most two identical
submission calls and one checker execution. Set a five-minute client-session
deadline, with separate bounded cleanup/observation for a possibly in-flight
node execution; killing the client must not fabricate a cancelled node verdict.
Confirm subscription/credit or API spending rules and the applicable approval
at execution time. No new paid API use is authorized by this plan.

Report separately: model produced an answer; MCP transmitted it; intake accepted
it; checker returned ACCEPT or deterministic REJECT; native receipt/evidence was
recorded; and identical redelivery preserved it. A rejected answer can validate
transport and honest rejection but is not a solved-task success. Unknown outcomes
remain unknown. None of these results is a block or reward claim.

## Preparation verification

Against main `81c63213f442d9be593eaf1ccc74c60e0c0b4438`:

- `cargo build --locked --offline -p boole-mcp --bin boole-mcp` passed.
- MCP `native_verify_tool` and `stdio_transport`: **26 passed**. These spawn the
  actual MCP binary, but native responses in this local lane are synthetic.
- Protocol `closed_local_replay_grant::tests`: **12 passed**, including exact
  binding, one-shot use, restart matching and mutation rejection.
- A cleared-environment MCP process initialized and listed its six-field native
  schema, forwarded one dummy request to a dedicated synthetic loopback service,
  and preserved its explicit preparation-only HTTP 503 body. The separate legacy
  trap observed zero connections. All rehearsal processes were stopped.

Real-model calls: **0**. New checker executions: **0**. New native receipts: **0**.
No active client registration, VM boot, production signing or operational state
change was performed. Historical Linux real-trace evidence remains separate.
