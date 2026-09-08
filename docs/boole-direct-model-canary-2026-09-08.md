# Direct model-client canary — 2026-09-08

Status: **DIRECT MODEL → MCP → CONTAINED CHECKER → RECEIPT: PASS**.

A newly approved bounded session completed the direct path that was not passed
in the earlier [operator-assisted run](boole-real-model-canary-2026-09-08.md).
The model generated one candidate, called the actual MCP tool once, received
ACCEPT and the BF.3 receipt, and reported its submission ID. There was no
operator forwarding, answer editing, second model session or redelivery.
The earlier blocked result remains unchanged.

## Scope and no-model regression check

The user approved one new five-minute session, one candidate and at most one
checker execution. The evaluation retained `gpt-6-astra / xhigh`, existing
ChatGPT authentication and the isolated tool profile from the first run. The
user's global configuration was not modified. No API key, credit purchase or
usage-reset credit was used.

The installed Codex CLI was 0.153.4. A local mock provider emitted a static tool
call into the real CLI; an unrelated loopback HTTP sink stood in for the native
service and returned an explicitly synthetic HTTP 503 response. This exercised
the CLI approval dispatcher, actual MCP process, HTTP forwarding and response
delivery without any inference or checker execution.

- The original configuration reproduced the approval refusal and zero HTTP
  submissions.
- Two dotted-path override variants still produced zero submissions. Quoting
  the dot-containing tool name inside the override path did not establish the
  intended tool approval in this CLI.
- Passing a TOML table with the complete tool name as one literal key produced
  exactly one HTTP submission with unchanged arguments. The CLI received the
  synthetic error body unchanged; it was not misreported as checker ACCEPT.

The working per-command approval setting, combined with an enabled-tool
allowlist containing only `boole.verify_native`, was:

```sh
-c 'mcp_servers.canary.tools={ "boole.verify_native" = { approval_mode = "approve" } }'
```

This authorizes only that already-approved tool in the isolated session, not
all tools or arbitrary shell execution. Per-tool approval settings and exposure
allowlists are separate controls in the
[official MCP configuration](https://learn.chatgpt.com/docs/extend/mcp).
The captured final tool inventory contained native verification and three inert
MCP resource helpers; the server implements no resource-reading endpoint.
Shell, editing, web, hooks, plugins, skills and subagents were unavailable.
As in the earlier run, local model-catalog metadata disabled auxiliary tool
surfaces without replacing model identity or built-in instructions.

## New installation and actual execution

Both host MCP and guest runtime used source
`0ac2f8fa7c4ffbf4e983d94a59134bf0b0bd826b`. The rebuilt host MCP SHA-256 was
`404cc11781f395a76e396083ea2e19651ebd3c767f123a700a361fe45fe2dacf`.
The disposable ARM64 Ubuntu/Lima VM retained the previous plain-mode isolation:
no host folder mounts or SSH-agent/auth sharing, with numeric-loopback native
forwarding only. The separate development seed, run ID, journal ID and epoch
were new; no old execution state or grant was reset or reused.

During preparation, an OS package update changed the concrete signature-tool
binding after resolution. Exact CAS replay rejected the stale runtime plan.
After package installation completed, the two resolutions differed only in
`planSha256`; their package and signature-derived resolution contents matched.
The old resolution was preserved, and the new tool-bound resolution was used
without changing any frozen authority. A diagnostic invocation also refused an
unwritable temporary path; supplying the existing private scratch path fixed it.
Signed resolution, offline rootfs build and independent verification then
returned `EXACT-MATCH`. Both service startup checks passed before inference.

The answer-free public packet retained the same contract, public task and
scaffold, changing only the submission epoch to 21. Its SHA-256 was
`f8fad08ecd101614f0b151d74891b417a6aaeb7106d123d83ce3e79cd4427694`.
No prior accepted candidate or hidden tests were supplied to the model.

The actual session finished within the 300-second cap. The launcher recorded
one peer request at 2026-09-08T12:25:05Z and completion at 12:25:34Z. The CLI's
completed MCP event contains the ACCEPT response and receipt; the model's final
message quotes the same submission ID. Reported usage was 15,302 input tokens
(7,168 cached) and 441 output tokens (including 73 reasoning tokens). These are
client token counts, not a dollar estimate or a claim of one inference request.

## Identity and durable evidence

| Identity | Value |
|---|---|
| Run | `dddd60796d13caa6804ed0432fe53d783714b477bad80b3960c15d97f739db72` |
| Candidate SHA-256 | `726226c2e7349ee0a951d158f2c63ae94e4da220c1788ce205583c88e98169a1` |
| Submission | `e4876c7ed1c03a05b212387d780b71e1282a1d9782fae3a8ec7c34e41cd9a3bd` |
| Evidence | `287b63869d622aa878e3eca4b848f26a51ed1ec73f36aeb90af1635eb47b97a3` |
| Artifact root | `1aca55ea77105e0762b12488e821214117d07a3aeb219de4d2d1585a58a0b0a0` |
| Checker | `fa3fea6534d505a8dcce5eca38ecc2c4a60c5173ff19a310dd82cfd797a11598` |

Both node and launcher budgets contain exactly `open`, `candidate`, `execute`.
Their candidate and submission identities match the CLI call. The five terminal
journal rows carry the same evidence digest as the response, and there is one
launcher peer marker. These cross-checks passed after service shutdown.

Raw mock/actual client events, public input, invocation helpers, public signed
grant, both resolutions, rootfs receipts and runtime journals are retained in
the ignored operator directory `local-docs/boole-direct-model-canary-2026-09-08/`,
with a cross-check script and `verified-result.json`. They are local evidence,
not clean-checkout CI fixtures. The disposable VM and its development private
key were removed after collection; no live listener or recovery installation
remains. Existing user edits were preserved.

## Result boundary

This closes the direct-client delivery canary for the historical permanently
non-issuable task. It does not establish new-problem benchmark performance,
universal mathematical correctness, public mining, payment, rewards, consensus
or activation. `activationAllowed=false` remains unchanged. Redelivery was not
needed or exercised. No further model session is authorized by this completed
one-session allowance. Verified paid-validation buyers/LOIs were not checked.
