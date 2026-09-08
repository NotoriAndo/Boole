# New-task direct-model evaluation — 2026-09-09

Status: **3/3 DIRECT MODEL → MCP → CONTAINED CHECKER → RECEIPT: PASS**.

Three newly admitted generated problems in the existing Rust tuple-projection
family each received one fresh model candidate and an ACCEPT receipt. In all
three independent sessions the model called the actual MCP tool and received
the verdict and receipt directly. No answer was forwarded or edited by the
operator, and no second candidate, model-session retry or redelivery was used.

This is the separately approved follow-up to
[development tuple-task admission](development-tuple-task-admission.md),
implemented in PR #378. The earlier
[historical-task direct canary](boole-direct-model-canary-2026-09-08.md) remains
a separate result, not one of these three problems.

## Approved scope and preparation

The user approved three new signed problems, each with one model session, one
candidate and at most one checker execution. Each session had a 300-second
deadline. The retained evaluation profile was `gpt-6-astra / xhigh` through
Codex CLI 0.153.4 and the existing ChatGPT login. There was no new API key,
credit purchase, usage-reset redemption or change to the global configuration.
The account's reported weekly usage was 27% both before and after the run
(rounded account-level values, not a claim of zero usage).

The official distinction between subscription and API-key authentication is
documented in [OpenAI authentication](https://learn.chatgpt.com/docs/auth).
Actual authentication was also checked locally before execution.

Host MCP and guest runtime were built from main
`0a508a6bb67aa97b6e3208647962e42e014d015c`. The host MCP SHA-256 was
`8259a237540bbab37121853fbf6c7085d1842bca2b54ba06cb7b301c6cb51668`.
The disposable ARM64 Ubuntu/Lima VM had no host folder mounts, SSH-agent
forwarding or authentication copying; native HTTP forwarding used numeric
loopback only. The fixed runtime was independently rebuilt and verified as
`EXACT-MATCH`, with layer digest
`sha256:dfeafb2918764736bdcd94d0fd121ed8eee2ef88d0a82e1ef28b3e625723bc0d`.
No frozen checker, rootfs expectation, signature scope or containment policy
was changed.

One guest-only development signing seed issued three distinct grants and
journals with epochs 30–32. The services were stopped between tasks and the
next public grant was explicitly provisioned. A spent task or budget was never
reset or reused. Each grant remained `nonIssuable=true` and
`activationAllowed=false`.

Before inference, each case passed a no-model tool-dispatch preflight: a local
mock provider sent one fixed tool call through the actual CLI and MCP process
to an unrelated loopback HTTP sink. The sink's synthetic 503 response was
delivered unchanged. It did not submit to the checker. Only native verification
and three inert MCP resource helpers were exposed; shell, editing, web, hooks,
plugins, skills and subagents were unavailable. The same literal-key tool
approval setting validated in the earlier direct canary was retained.

Readiness checking was corrected during preparation to accept an empty
startup-created journal and to wait for rootfs qualification after systemd
reported `active`. The immediate second-task probe initially observed no
launcher budget; it did not start a model. A subsequent read confirmed both
fresh `open`-only budgets before inference. The final probe waits at most
60 seconds for those files, then still requires zero candidates/executions.
These were preparation corrections, not candidate or model retries.

## Public problems and actual outcomes

Each packet followed global contract → family manifest → official surface →
output format → instance specification. It contained only the generated public
anchor, exact scaffold, recurrence, constants and submission identity. No prior
answer or hidden test was supplied. Input seeds were fresh and all semantic
fields were bound by the signed task generator.

| Case | Field types | Initial accumulator / multiplier | Coefficients | Result |
|---|---|---|---|---|
| Pair | `i32, bool` | `17 / 3` | `2, -5` | ACCEPT; model received receipt |
| Mixed | `i8, u16, i64, bool` | `-31 / -7` | `-3, 11, 2, -13` | ACCEPT; model received receipt |
| Wide | `u8, i16, u32, i64, u64, bool, i32, bool` | `12345 / 97` | `5, -7, 11, -13, 17, -19, 23, 29` | ACCEPT; model received receipt |

Launcher request markers were recorded at 2026-09-08 23:14:44.986000,
23:16:47.655827 and 23:18:36.161142 UTC, respectively (September 9 in Korea).
All sessions completed within their 300-second bounds. CLI usage totaled
46,690 input tokens (14,464 cached) and 1,835 output tokens (524 reasoning).
These are client token counts, not dollar estimates or a claim of three
underlying inference requests.

## Durable identity cross-checks

| Case | Submission ID | Evidence digest |
|---|---|---|
| Pair | `0534e8463941576be327f9ccbc92ef25c00779224e27744db2efd361dc7a4634` | `0106bdad98a8aee14f209d8f50f6e2f2ee63c1790c8289f24009872f19e36783` |
| Mixed | `c96c04043c238f835fa1c5d06f38dfc4fb6ad357be6a7f8a7033eb027a8e2c6b` | `ae1331932baf2219abc95e3876a65d5785ce75ec0566194a3be890678e628047` |
| Wide | `708d44432042a7551a4924188e499df6921cecb8de42f11ac60333236cf4840f` | `0e6b963e3aae591013752113720d9540640bf0608499ae0e9e65913feeeafd09` |

For every case, both independent budget files contain exactly `open`,
`candidate`, `execute`. The candidate digest and submission identity match the
CLI tool arguments and returned receipt. The five terminal journal rows carry
the same evidence digest as the MCP response, and the model's final message
quotes that receipt's submission ID. Each case has exactly one launcher peer
marker. Task/template/challenge, run/journal, candidate and receipt identities
are distinct across the three cases. Total: three sessions, three MCP calls,
three candidates, three checker executions, zero redeliveries.

The raw client events, answer-free inputs, public grants/materials, rootfs
receipts, per-task runtime journals and cross-check helpers are retained in
the ignored operator directory
`local-docs/boole-new-task-model-evaluation-2026-09-09/`, including
`verified-summary.json`. They are local execution evidence, not CI fixtures.
The development private key was never copied out. After collection, the VM and
its private key were deleted at 2026-09-08 23:19:57 UTC; no live runtime listener
remained. Existing user edits were preserved.

## Result boundary and next step

This demonstrates direct model use of three new generated problems in one
already-supported development family. It does not establish performance on an
independent benchmark, real-world repository tasks, Lean/formal proofs or
arbitrary mathematical statements. It does not promote the family to issuable
supply or establish public mining, rewards, payment, consensus or activation.

The three-session allowance is complete. Further model runs are not included.
The next proposed boundary is clean-Mac general-user installation and lifecycle
validation, including setup, readiness, submission/receipt, restart and
uninstall. This developer-machine VM run does not satisfy clean-Mac CURL.3.
Operational release custody and public deployment remain separate scopes.
Verified paid-validation buyers/LOIs were not checked.
