# Closed-local real-model canary — 2026-09-08

Status: **MODEL CANDIDATE ACCEPTED VIA OPERATOR MCP DELIVERY; DIRECT CLIENT DELIVERY NOT PASSED**.

This executed record preserves both outcomes. A real model generated one new
answer for the historical, permanently non-issuable tuple-struct task. The
Codex client's tool call was blocked before transmission by its approval
configuration. Without another model call or any answer editing, the operator
forwarded the identical six-field submission through the actual MCP binary to
the Linux-contained checker. It returned ACCEPT and a durable BF.3 receipt.
This is not an uninterrupted model-client → MCP success claim.

## Scope and installation

The user approved actual execution after the development implementation in
[PR #375](https://github.com/NotoriAndo/Boole/pull/375). Source was main
`3c63c130f509a0f52a51f75c9c30ca9ca760f5fc` for the guest installation, with no
product edits. The limits were
one five-minute client session, one candidate and at most one checker execution;
no new paid API route, public server, operational key or activation was used.

A disposable Lima 2.2.0 VZ VM ran Ubuntu 24.04 ARM64 with 4 CPUs, 8 GiB RAM and
a 30 GiB virtual disk. Plain mode disabled host folder mounts, SSH-agent sharing
and dynamic forwarding. The native service was forwarded only from host
127.0.0.1:8082 to guest 127.0.0.1:8082. No model credential was copied into the VM.
The existing ARM64 acquisition, signed resolution, offline build and independent
verification commands returned `EXACT-MATCH` against the frozen rootfs output.
The current development binaries were built with the ARM64 authority and
`fresh-answer-canary` features; a separate root-owned development seed signed
one epoch-20 grant. Production/frozen replay signing inputs were unchanged.

Initial launcher startup refused a missing installed `/opt/boole` toolchain
path. The operator installed the exact toolchain already present in the verified
rootfs, as the CI gate does, then startup qualification succeeded. No candidate
was admitted in that failed installation attempt; its service log is retained.

The host MCP executable was the previously prepared build
`boole-mcp 0.1.0 (sha=81c63213f442 build=2026-09-08T06:53:41Z)`, SHA-256
`c21ae10c772550374cd63d06f1540bc187c577532b025686105db090cf7a5992`.
The MCP crate's source had not changed between that revision and the guest
revision. This identifies the binary actually used, rather than attributing
the separately built guest MCP binary to the host delivery.

## Client preflight and actual session

Three local mock-provider request captures made no inference calls. The first
exposed unwanted auxiliary tools; the final inventory had only native verification
as an operational MCP tool, plus inert MCP resource helpers. The MCP server has
no resource-reading implementation. Shell, file editing, web, hooks, plugins,
skills and subagents were unavailable. A per-command model catalog disabled the
selected model's default code-mode, editing and delegation surfaces; its model
identity and built-in instructions were retained. User/global config was not edited.

The public packet followed global contract → family manifest → official surface
→ output format → instance specification. It included the task scaffold and
public constants, but no accepted-answer fixture, hidden tests or private hints.
Its SHA-256 was
`907a6726c4b0a1945f0a56772228bcbe6a30c06b1d1dfb9ee902f1abc6e2ddaa`.

The actual session used the user's configured `gpt-6-astra`, `xhigh`, existing
ChatGPT sign-in and subscription path. CLI usage reported 15,070 input tokens
and 585 output tokens (including 237 reasoning tokens), with no cached input.
No API key, credit purchase or usage-reset credit was used. These are client
token counts, not a dollar-cost estimate or a claim of one inference request.

The model generated one candidate and attempted one `boole.verify_native` call.
The CLI returned `MCP tool call requires approval, but approval policy is never`.
The model stopped without retrying; no node candidate or execution event existed
at that point. An MCP enabled-tool allowlist did not establish tool approval.
This was a client configuration failure, not a checker rejection.

The operator extracted the tool arguments directly from that completed event,
asserted there was exactly one such call, and transmitted those exact arguments
through a new cleared-environment MCP process. No second model session or
changed-answer retry occurred. The original blocked attempt remains recorded.

## Verified result

The actual MCP delivery began at 2026-09-08T11:57:54Z; the launcher completed at
11:58:23Z. The response was `accepted`, `redelivered=false`, with no legacy-node
connections. Both private budget logs contain exactly `open`, `candidate`,
`execute`; the candidate digests agree. The node verdict journal contains the
five expected reservation/bootstrap/in-flight/evidence/terminal rows, and the
service log contains exactly one active-execution peer marker.

| Identity | Value |
|---|---|
| Run | `217f2fafff798457c2cd1169470d0fc191b67ddb9757e5ee89fcc3d24a7cf4a3` |
| Candidate SHA-256 | `8a918c39dd9052aec147b40c5112c50ca5119101cc0bcd63bc79588678473201` |
| Submission | `cd9e6114258b2a0289c9cd6015dccd6caba55a7b12727366b24490896063e4a5` |
| Evidence | `95e3c57cfdd763f2e9d7924434f92877ed84edfa36bdcfa8ba3996a5fa0570eb` |
| Task | `11e3c69ebf043c405f0c6c554dd3edec2c63bf4d1b3dfa8d27aa6b0fd5434b35` |
| Artifact root | `32d6bad33f0981a1cc82d87528dc65e6f5c0f07ba8eb4244cb6d397d38dc03c5` |
| Checker | `fa3fea6534d505a8dcce5eca38ecc2c4a60c5173ff19a310dd82cfd797a11598` |

Raw client events, the exact submitted candidate, local inventory captures,
public grant/signature/key, rootfs receipts, budget/verdict journals and service
logs are retained in the ignored operator evidence directory
`local-docs/boole-real-model-canary-2026-09-08/`. They are not clean-checkout CI
fixtures and are not claimed to be available from the repository alone.
The VM and development private key were removed after evidence collection;
there is no live canary listener or retained recovery installation.

## Limits and next boundary

This is a real model-generated answer accepted by the historical semantic
checker through operator-assisted MCP delivery. It is neither a novel-problem
benchmark nor a mathematical proof for all inputs, public mining, block, share,
payment, reward or consensus result. Issuance and activation remain disabled.
Signed redelivery was not needed or exercised in this actual run; its synthetic
CI coverage remains separate.

Direct client delivery still needs explicit per-tool approval configuration
and a no-model dispatch preflight, followed by another bounded model session
only within new applicable session/usage approval. The completed one-session
allowance is not silently reset. A future run must not reuse these exhausted
state identities or delete journals to make the original grant reusable.
Verified paid-validation buyers/LOIs were not checked in this execution.
