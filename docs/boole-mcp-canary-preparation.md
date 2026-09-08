# Real-model MCP canary preparation

Status: **DEVELOPMENT CAPABILITY AND DIRECT-CLIENT CANARY COMPLETE** (2026-09-08).

The first approved [actual run](boole-real-model-canary-2026-09-08.md)
obtained ACCEPT for a real model-generated candidate through operator-assisted
MCP delivery. Direct client transmission was blocked by its approval settings
and did not pass. A separately approved
[direct-client run](boole-direct-model-canary-2026-09-08.md) subsequently passed
without operator forwarding after a no-model dispatch regression check. The
original preparation results below remain historical; they are not current
actual-run counts.

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
- Explicitly configure approval for that single allowed tool and exercise a
  no-model dispatch preflight. `enabled_tools` controls exposure, not approval;
  `approval_policy=never` alone can prevent the permitted MCP call from running.
  The direct-run record includes the tested per-tool literal-key override for
  Codex CLI 0.153.4; do not assume dotted override paths preserve dots in tool names.
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

## Adopted development implementation — not model execution approval

The subsequent user approval selected a separate development-only operator
signing root. The default-disabled `fresh-answer-canary` Cargo feature supplies
separate Linux node/launcher binaries; existing replay binaries, fixed grants and
Mac VM behavior are unchanged. The new binaries refuse non-Linux execution.
Model input cannot issue this authority or promote it into production authority.

The implemented runtime bounds, with a proposed future client limit, are:

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

The signed grant fixes the run/journal IDs, epoch (outside replay epochs 0–3),
task, checker/artifact/release, policy, toolchain and intake/registry bindings.
The selected task is the existing permanently non-issuable historical task.
“Fresh” describes answer bytes, not a novel problem or a new benchmark task.
The public packet must still omit the known accepted answer.

The independent Ed25519 public key is provisioned as a root-owned read-only
`operator-public-key.bin` under
`/usr/share/boole/native-shadow/development-canary-v1`, alongside `grant.json`
and `grant.sig`. The directory is root:root 0555 and files are single-link regular
0444 files; descriptor-relative traversal rejects symlinks and ownership drift.
The grant digest includes its signing domain, public key and exact payload, so
key rotation cannot silently reuse old private state. The model never supplies
the trust root. This is not a production release key or activation signature.

Offline `boole-canary-authority` commands are:

```text
create-grant KEY_FILE RUN_ID JOURNAL_ID EPOCH OUTPUT_DIRECTORY
create-redelivery KEY_FILE GRANT_DIRECTORY CANDIDATE_DIGEST SUBMISSION_DIGEST OUTPUT_DIRECTORY
```

The tool requires an existing owner-only 0600 32-byte signing seed and an existing
0700 output directory; it neither generates keys nor installs files, and refuses
output replacement. No secret is printed or passed as an environment value.
Root/operator provisioning is separate from model/client work. Redelivery needs
its own domain-separated `redelivery.json`/`redelivery.sig`, bound to the verified
grant and exact candidate/submission, installed with the same public-file rules.

Private pre-provisioned state lives under
`/var/lib/boole/native-shadow/canary-node/<journalId>` (node-owned 0700) and
`canary-launcher/<journalId>` (root-owned 0700), below root-owned parents.
Node and launcher independently fsync and lock their one-execution budgets;
the node retains the durable verdict journal in its private directory. Partial
tails, empty existing files, wrong authority, changed candidates and ambiguous
in-flight recovery fail closed. Operator-signed terminal redelivery spends its
budget durably before returning and cannot re-execute. A crash can consume an
allowance without delivering a result; these are at-most-once guarantees, not
guarantees of eventual success. Deleting/recreating state is not a recovery path.

Focused behavior tests cover wrong task/checker/key, signer rotation, changed
candidates, corrupted/concurrent journals, single execution across restart and
separately signed redelivery. The containing PR's full Linux CI adds actual
MCP → HTTP → qualified contained checker ACCEPT/REJECT, terminal crash recovery
and in-flight fail-closed cases. It uses synthetic answers and disposable test
keys only. This does not establish a real-model solving result or a ready local
installation. Qualified containment and frozen authority checks remain required.

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
