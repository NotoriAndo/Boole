# Boole — current development status

Updated: 2026-09-07. This is the tracked entrypoint for current progress and next
work. Edit it in place; historical experiment results remain in their own records.
Local Master/Execution documents retain detailed plans and point here for status.
Work methods are governed by [the development policy](development-throughput-and-evidence-policy-v1.md).

## Current audit-remediation boundary

The first audit-remediation pass merged as [PR #370](https://github.com/NotoriAndo/Boole/pull/370),
main `374f7bbb943457f2cfebdb90d8f276f705fd5bd9`. The follow-up,
[PR #371](https://github.com/NotoriAndo/Boole/pull/371), addresses the
remaining verified boundaries and records explicit exclusions in
[September audit follow-up](audit-remediation-2026-09.md). Required integration
evidence is the containing PR's protected CI under the
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

After the audit follow-up's required CI/main integration, prepare—but do not execute—one real
MCP-client/LLM canary through the existing verification path.
First identify the available client, its authentication/billing path and the
node/launcher environment; prepare a bounded end-to-end run and safe local
rehearsal. A test driver is already proven; a real model acting as caller has
not yet been demonstrated by this milestone. Do not assume a specific paid model
or require a new paid API before checking available integration paths.

The current instruction does not itself approve a paid model call. Any later
canary run must use the existing applicable approval, or obtain only the missing
scope/budget approval.

## Operational boundaries

No new production release, public testnet/P2P, mining, real payment/wallet movement,
reward, consensus-state or activation authority is granted by these development
results. `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`,
`Base activation=false`, `activationAllowed=false` remain the recorded posture.
Clean-Mac CURL.3 remains deferred/not passed; closed-local evidence does not
silently promote it.
