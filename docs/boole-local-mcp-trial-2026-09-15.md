# Developer-Mac MCP trial — 2026-09-15

Status: **DIRECT MCP SUBMISSION ACCEPTED / DURABLE EVIDENCE CROSS-CHECKED / VM STOPPED**.

The user completed the local walkthrough on an existing developer Mac. A newly
prepared development tuple-projection problem received one model-generated
candidate through the actual `boole.verify_native` MCP tool. The returned
adjudication and BF.3 receipt were accepted. A subsequent no-inference closeout
matched the original client event against the guest's two durable budgets and
terminal journal, then normally stopped the dedicated VM without deleting its
disk, grant or spent state.

This is a one-problem developer-machine use trial, separate from the earlier
[three-problem evaluation](boole-new-task-model-evaluation-2026-09-09.md).
It is not a benchmark, a clean-Mac installation test, a signed Mac product
release, public mining, a reward or activation result.

## Preparation and user handoff

The separate preparation used source commit
`7811fc784c1838d710f6a8b082f742b2b12ecc11` and a dedicated ARM64 Lima/VZ VM,
`boole-mcp-dev-20260914`. The Linux-only
[development task-admission](development-tuple-task-admission.md) node and
launcher retained the existing checker, signature and containment checks.
Preparation records retain the independently checked rootfs `EXACT-MATCH`,
loopback-only service, no host folder/SSH-agent/credential sharing, and unused
budgets before submission.

The new problem was `LocalPracticeTuple(i16, u32, bool)`, with coefficients
`7, -4, 13`, initial accumulator `41`, multiplier `-9` and epoch `914`.
It used a new development grant and separate durable state, not one of the
previous three evaluation grants. This profile permits one candidate and at
most one checker execution. No redelivery permission was issued.

The first message in the solving conversation omitted the public problem file.
The client stopped without submitting. The user then supplied its exact local
path. The same conversation read the public specification and scaffold, formed
one candidate, called the MCP tool directly and displayed the returned receipt.
The operator did not forward or edit the candidate. This handoff demonstrates
the current manual public-file requirement; it is not an automatic problem
discovery flow.

## Execution and independent closeout checks

The actual MCP call started at `2026-09-14T15:05:01.802Z` (September 15 in Korea)
and completed in approximately `29.284` seconds. The original completed-tool
event contains both submitted arguments and the actual MCP response; it is not
a receipt reconstructed from the assistant's final prose.

- Exactly one native submission call exists in the solving conversation.
- Node and launcher budgets each contain exactly `open`, `candidate`, `execute`.
  Both bind the same signed-grant digest, candidate digest and submission ID.
- The guest's installed public grant, signature and public key match the
  operator's public copies byte-for-byte. No private key was exported.
- The five journal records are `grant_attempt_reserved_v1`, `bootstrap_v2`,
  `in_flight_v3`, `evidence_v2`, `terminal_consumed_v2`. The final record is
  exhausted; no retry, rollback or redelivery row exists.
- SHA-256 recomputed over the exact durable `evidenceJson` bytes matches the
  journal and the client response's `evidenceDigest`. Its candidate, submission,
  checker, verdict and reason bindings agree with the original MCP call/receipt.
- The launcher journal has one active-execution peer marker. Its systemd result
  is success, exit status zero and restart count zero. Its inactive state after
  the call is the intended one-shot launcher exit, not a failed checker.
- The model's final message contains the same submission ID as the tool result.

The audit retains the receipt's `taskId` and `artifactRoot` exactly as returned;
it does not independently recompute those two BLAKE3-derived mappings. It does
recompute the candidate, signed-grant and durable-evidence SHA-256 bindings.

| Result | Value |
|---|---|
| MCP submissions / candidates / checker executions | `1 / 1 / 1` |
| Redeliveries / closeout model calls / closeout submissions | `0 / 0 / 0` |
| Outcome / receipt verdict | `accepted / accepted` |
| Submission ID | `2a9d594b347f2d5995e52b82ae349dce6adf78454638d6789a4130016ac9bcfc` |
| Candidate digest | `41bbfc187ee56a2015aaaaf0d39139104b0092c1181392807fdbec3040d08012` |
| Evidence digest | `ac68a43057f4cd36722412d5831afa6b570e969c7aa0223feeb581db08c06a8e` |
| Task ID | `886978aec343c0c720d0f78a0fd89af7fc89490fa8a00024f51507b41c23f209` |
| Artifact root | `eb03fe66a73e6940ab43f9171b84dccee4fe6db2395f082a45a48dfb8ed08ad5` |
| Checker hash | `fa3fea6534d505a8dcce5eca38ecc2c4a60c5173ff19a310dd82cfd797a11598` |

## Shutdown and evidence location

After capture and verification, the prepared `stop.sh` stopped the node/launcher
services and normally shut down the VM at approximately
`2026-09-14T15:16:09Z`. Post-stop checks confirmed `Stopped`, no listener on host
TCP port `8082`, and the retained VM disk. No VM, grant, journal, development key
or user file was deleted. The development private key remains inside the stopped
VM; this is preservation, not key destruction. The spent grant must not be
reset or treated as fresh after a later start.

The private local evidence directory is
`local-docs/boole-mcp-dev-20260914/closeout-20260915/`. It contains selected original
client events, exact adjudication text, submitted arguments, the guest snapshot
with raw-file bytes/hashes/metadata and service logs, `verified-result.json`, and
`shutdown.json`. Its sibling `closeout-20260915.py` documents extraction and
cross-checks. It never submits or starts a service; output files are created
exclusively to avoid rewriting earlier evidence. The preparation-time
`prepared.json` and readiness records remain unchanged and continue to describe
the earlier zero-submission state, not the final result.

The closeout preserved the pre-existing `tasks/lessons.md` edit and did not
change MCP settings. Further problems need their own unused grants and applicable
model-execution scope. Automatic VM/problem/receipt workflows remain proposed
follow-up work, not an implemented result of this closeout. Clean-Mac CURL.3
and all existing operational holds remain unchanged.
