# Boole — current development status

Updated: 2026-09-08. This is the tracked entrypoint for current progress and next
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

Real MCP-client/LLM [canary preparation](boole-mcp-canary-preparation.md) and the
separate development implementation are complete. The
[2026-09-08 actual run](boole-real-model-canary-2026-09-08.md) generated one real
model candidate and obtained an actual contained-checker ACCEPT and BF.3 receipt
through operator-assisted MCP delivery. The direct CLI tool call was blocked
before transmission by its approval configuration; it is **not passed** as an
uninterrupted client path. The identical answer was forwarded without another
model call or editing. Both budgets confirm one candidate and one execution.

The fixed-fixture replay grant remains unchanged. The separately approved
implementation adds a default-disabled, Linux-only one-task fresh-answer canary
with an independent development operator signing root, durable candidate binding,
at most one checker execution across restart, and at most one separately signed
identical redelivery. Its task remains the permanently non-issuable historical
task: fresh answer bytes do not mean a novel problem or benchmark result.
PR #375's full CI checks the actual MCP/contained-checker path with synthetic
answers and disposable test keys. That coverage remains separate from the actual
model result and its client-delivery limitation above.

The approved one-session run used existing ChatGPT authentication, not a new API
key. Its isolated Linux installation was qualified and removed after evidence
collection, including the development private key. The next direct-client
boundary is explicit per-tool approval plus a no-model dispatch preflight, then
a separately authorized bounded session and fresh installation. The completed
session cap does not authorize another model run. Production keys, public
deployment and the existing Mac replay path remain outside this work.

## Operational boundaries

No new production release, public testnet/P2P, mining, real payment/wallet movement,
reward, consensus-state or activation authority is granted by these development
results. `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`,
`Base activation=false`, `activationAllowed=false` remain the recorded posture.
Clean-Mac CURL.3 remains deferred/not passed; closed-local evidence does not
silently promote it.
