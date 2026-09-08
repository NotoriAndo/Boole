# Boole — current development status

Updated: 2026-09-09. This is the tracked entrypoint for current progress and next
work. Edit it in place; historical experiment results remain in their own records.
Local Master/Execution documents summarize current contracts and milestones and
point here for status. Historical planning text is archived locally; unresolved
requirements have a separate local owner rather than competing current cursors.
Work methods are governed by [the development policy](development-throughput-and-evidence-policy-v1.md).

## Current audit-remediation boundary

The first audit-remediation pass merged as [PR #370](https://github.com/NotoriAndo/Boole/pull/370),
main `374f7bbb943457f2cfebdb90d8f276f705fd5bd9`. The follow-up,
[PR #371](https://github.com/NotoriAndo/Boole/pull/371), addresses the
remaining verified boundaries and records explicit exclusions in
[September audit follow-up](audit-remediation-2026-09.md).

The subsequent independent review of main `ae08171c` produced nine additional
findings. [PR #372](https://github.com/NotoriAndo/Boole/pull/372) closes storage role
collisions and failed append continuation, future-checkpoint trust and equal/shorter
fork convergence, MCP transport replacement/responsiveness, and portable subprocess cleanup. The same
storage pass also fixes bounty create/status/proof publication before durable
audit storage. Independent review is separate from implementation ownership;
the linked follow-up records the tested scope and remaining design limitations.
PR #372 merged to main `a0a8f81c9a1fdc4988d0a4f846e7a682cd1f8199` after its
required checks passed. Integration evidence is the containing PR's protected CI
under the
[development policy](development-throughput-and-evidence-policy-v1.md).
No fresh VM execution or real-model success is claimed; historical execution
evidence below remains preserved.

## Completed development boundary

- N5.3 M1–M6 closed-local foundation: named-network authority, canonical state
  safety, bounded P2P lifecycle, controlled three-node join, third-party Lean
  process isolation and HTTP/P2P verification parity (PR #361–#366).
- Strict MCP native verification (PR #367) and an actual MCP stdio → node HTTP →
  qualified launcher → Linux-contained checker → BF.3 receipt trace (PR #368,
  main `1fba4bf263a8827a028d122a3b574b2fb8fef3cd`).
- Both Linux architectures passed the actual trace. Accepted, tampered, constant
  and empty answers keep their distinct verdicts; MCP restart and redelivery
  do not execute the checker again.
- Mac closed-local VM and curl installation/update/rollback paths and
  non-operational trust-policy/custody rehearsal are implemented. Operational
  release custody values remain deferred.

Evidence: [MCP path](boole-mcp-e2e.md),
[local MVP closeout](verified-answer-local-mvp-closeout.md),
[PR #368](https://github.com/NotoriAndo/Boole/pull/368),
[CI](https://github.com/NotoriAndo/Boole/actions/runs/33954999938).

## Next development boundary

The separately approved [development tuple-task admission](development-tuple-task-admission.md)
extends the fixed task to operator-signed generated problems in the existing
Rust tuple-projection family. It has separate default-disabled Linux binaries,
signature/schema/registry identity and per-task durable state. The profile admits
only bounded typed specifications, not arbitrary source or runtime policy.
Each installed grant still permits one candidate and at most one checker run;
multiple problems use separate grants and state. The containing PR's full CI
checks accepted/incorrect/tampered synthetic answers through actual MCP and the
qualified checker, cross-task rejection and restart/redelivery safety.
No new model execution is included. After this development boundary, the next
evidence is a separately scoped real-model evaluation on new admitted problems,
followed by general-user installation validation when that scope is selected.

Real MCP-client/LLM [canary preparation](boole-mcp-canary-preparation.md) and the
separate development implementation are complete. The newly approved
[direct-client run](boole-direct-model-canary-2026-09-08.md) **passed**:
model → actual MCP → qualified Linux-contained checker → ACCEPT/BF.3 receipt →
model response. There was one model session, one MCP submission, one candidate
and one checker execution, with no operator forwarding or redelivery.
The earlier [operator-assisted result](boole-real-model-canary-2026-09-08.md)
and its blocked CLI attempt remain preserved as separate history.

The fixed-fixture replay grant remains unchanged. The separately approved
implementation adds a default-disabled, Linux-only one-task fresh-answer canary
with an independent development operator signing root, durable candidate binding,
at most one checker execution across restart, and at most one separately signed
identical redelivery. Its task remains the permanently non-issuable historical
task: fresh answer bytes do not mean a novel problem or benchmark result.
PR #375's full CI checks the actual MCP/contained-checker path with synthetic
answers and disposable test keys. That coverage remains separate from the actual
model results above; the successful direct-client run also passed a no-model
approval/dispatch/response regression check before inference.

Both separately approved sessions used existing ChatGPT authentication, not a
new API key. Their isolated Linux installations were removed after evidence
collection, including the development private keys. The direct-client canary
boundary is now closed. Broader task admission, general-user installation and
any additional model execution require their own applicable scope; completing
these capped runs does not grant further model sessions, production keys or
public deployment authority. The existing Mac replay path is unchanged.

## Operational boundaries

No new production release, public testnet/P2P, mining, real payment/wallet movement,
reward, consensus-state or activation authority is granted by these development
results. `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`,
`Base activation=false`, `activationAllowed=false` remain the recorded posture.
Clean-Mac CURL.3 remains deferred/not passed; closed-local evidence does not
silently promote it.
