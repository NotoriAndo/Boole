# Boole — current development status

Updated: 2026-09-15. This is the tracked entrypoint for current progress and the
ordered development roadmap. Edit it in place; execution evidence stays in its
own result record. Local Master/Execution documents link here instead of owning
a second current cursor. Work methods follow the
[development policy](development-throughput-and-evidence-policy-v1.md).

## Current boundary

**The prepared development MCP workflow is complete**, merged in
[PR #382](https://github.com/NotoriAndo/Boole/pull/382), main
`649fcff6abd2630efc52543f189bb7ad45e2f802`.
Read-only problem/identity and verifier/candidate status, plus start/status/stop
for an already-provisioned closed-local Lima VM, passed direct-consumer checks
and full CI including both Linux contained-checker architectures.
This is not automatic VM/task provisioning or another actual-model evaluation.
See the [workflow and evidence limits](development-mcp-workflow.md).

**Next in the plan: R1, native test-coin transfer and public-testnet prerequisites.**
Native accounting currently credits rewards and exposes balances; signed
wallet-to-wallet transfer, debit/nonce replay and transfer block inclusion are
not implemented. A verification receipt, mock faucet transaction ID or generic
wallet signature does not close that gap.

This roadmap reconciliation changes documents only. It does not start R1 code,
a VM/model run, public deployment, mining or a wallet transaction.
[Reconciliation findings](development-plan-reconciliation.md) distinguish closed
work, actual gaps, optional designs and checks still needed.

## Ordered roadmap

R1–R4 is the base-network sequence. U1/U2 is the separately gated useful-work
branch; it must not become a prerequisite for ordinary native test-coin
transfers. Demand discovery and conditional research can proceed in parallel.
These IDs group outcomes, not one-PR work units. R1 implementation starts with
the E13/E14 transfer/ledger contract and is packaged into coherent, independently
verified milestones under the development policy; security/transport work can
proceed alongside it without making all of R1 one oversized change.

| ID | Boundary / completion evidence | Dependency and current state |
|---|---|---|
| R1 | Native transfer and launch-critical product/security/operations: signed network-bound transfers, debit/fee/nonce state, block/replay/reorg integration, wallet safety/recovery for the selected testnet UX, secure public P2P/RPC, resource limits and operator recovery/observability. Pin test-only monetary parameters and experiment criteria before running the new network. | **Planned, incomplete.** Start from accepted transfer/monetary structure and the residual scope, not a repeat of completed N5.3 or MCP work. Prove mining → reward → A-to-B transfer → consistent balances in closed-local integration first. |
| R2 | Public-testnet launch readiness: versioned network/genesis and release artifacts, scoped key custody, participant risk notice/onboarding, bootstrap/incident/upgrade/rollback runbook and launch approval. | After the applicable R1 acceptance tests. **Not passed.** Clean-Mac CURL.3 is the last installation-validation step, deferred until a clean machine is available; current-Mac/CI work continues. A supported-Mac public-release claim still needs that evidence, or an explicit narrower platform scope. |
| R3 | Public base-network testnet: independently operated nodes actually mine test coins, send them between wallets, include/confirm transactions and agree on balances/fees/supply; exercise rejection, restart, partitions/rejoin, reorg and operator recovery. | After R2 and explicit public-network/mining/test-wallet scope. **Not started.** Test coins carry no real-money or future-mainnet entitlement. Mock-only accounting does not pass. Useful-work reward remains OFF unless separately authorized. |
| U1 | BF.7: new-rule receipt consensus with `no_protocol_reward`, independent replay and DA recovery. | **HOLD.** Adapter-scoped RP0-MD/supply, BF.6a DA and deterministic resource contracts are prerequisites. Closed-local integration precedes any separately approved public extension. Existing v3 is preserved; this branch does not block R1–R3 base transfers. |
| U2 | BF.8: preregister economic experiments, gather supply/solve/attack/demand evidence including at least four nodes and heterogeneous operators, then accept the evidence-informed Economic ADR. | After the applicable U1 path and preregistered experiment authority. **Not passed.** Additional useful-work reward activation needs its own plan; an ACCEPT/BF.3 receipt is not issuance. |
| R4 | Mainnet / real-value production decision and launch: testnet exit review, final monetary parameters, economic/security assumptions, release/key custody, recovery and any selected real-payment/refund service. | After R3 evidence and applicable economic/release gates. **Not ready; no launch approval.** If useful-work rewards are included, U1/U2 and their activation gates also apply. A base-only scope is a separate explicit decision, not an automatic bypass. |

R1 fixes **testnet** parameters and safety, not final mainnet tokenomics.
Real facilitator/x402 payment, paid escrow, live reputation, an exchange or an
entire marketplace are not blanket entry requirements for R3. If a feature is
exposed, its own security/accounting gates apply; otherwise it stays disabled
or outside the declared scope. Final mainnet constants remain unresolved.
Transfer authorization must not weaken the existing work-session
`canTransfer=false` / `canWithdraw=false` boundary.

Public transport encryption and peer authentication remain required before
public/untrusted participation; the closed-local plaintext/static-peer
implementation is not a public-network transport certificate.

### Parallel work and conditional research

- Demand discovery, external signed work requests/LOIs, supply measurement and
  economic draft/preregistration proceed alongside R1–R3. Actual buyer evidence
  is required by the useful-work activation gate, not fabricated as a base
  testnet prerequisite or claimed from implementation success.
- T0's original experiment is complete. Re-measure own-corpus reuse only after
  the first useful family has at least 50 actual external proofs.
- C1 corpus artifact needs its schema/provenance/real-row evidence; C2 conjecture
  market needs economic and anti-collusion decisions; C3 autoformalization needs
  C1/C2 and a faithfulness-dispute design. Z (proof-of-Lean/aggregation/settlement)
  remains conditional R&D. These are not mandatory work before a base testnet.
- Optional GUI/personas, mnemonic standard, OS keychain, durable unlock sessions,
  broad refactors and extra developer tooling are choices, not automatically
  approved missing features. The selected release still needs safe key handling,
  recoverability and clear user-visible limits.

## Completed major boundaries

| Boundary | Verified scope / evidence |
|---|---|
| L1/SC and N5.3 M1–M6 | Named-network authority, canonical-state durability, bounded P2P lifecycle, controlled three-node join, artifact-only Lean audit/process isolation and HTTP/P2P verification parity. PR #361–#366; [MVP closeout](verified-answer-local-mvp-closeout.md). No public-network claim. |
| BF.0–BF.6a non-consensus foundation | Default-OFF identity/registry/assignment/receipt/store scaffold and commit/reveal, sidecar/CAS/P2P support. This is not BF.7 or reward activation. |
| Strict MCP and real contained trace | PR #367/#368; actual MCP stdio → node → qualified launcher → Linux-contained checker → BF.3 receipt on both architectures, negative controls and restart/redelivery safety. [MCP evidence](boole-mcp-e2e.md). |
| September audit remediation | PR #370/#371/#372, including storage role collisions, durable publication/fencing, checkpoint trust/fork convergence, MCP replacement/responsiveness and portable subprocess cleanup. [Findings and deliberate exclusions](audit-remediation-2026-09.md). Closed findings are not blanket repository security certification. |
| Fresh-answer canary and direct model caller | PR #375 implementation/synthetic CI and the [direct-client run](boole-direct-model-canary-2026-09-08.md): one session, one direct MCP submission/candidate/checker, ACCEPT and model receipt delivery. Earlier [operator-assisted/blocked evidence](boole-real-model-canary-2026-09-08.md) remains separate. Temporary installations and development keys were removed. |
| Generated development tuple tasks | PR #378, separately signed bounded typed specifications, task identities and durable per-task budgets; real MCP/checker synthetic CI on both Linux architectures. [Admission contract](development-tuple-task-admission.md). |
| Three actual new-task model evaluations | PR #379; [3/3 new generated problems](boole-new-task-model-evaluation-2026-09-09.md), independent sessions with one candidate/checker each, direct MCP and receipt delivery, no retry/redelivery/operator forwarding. Disposable VM/key removed. One-family bounded result, not a benchmark. |
| Developer-Mac user trial | PR #381; [one new tuple ACCEPT](boole-local-mcp-trial-2026-09-15.md), original MCP/budgets/terminal evidence agree on one candidate/checker and no redelivery. VM normally stopped; this trial's disk/key/spent state retained. Not clean-Mac evidence or a run of the subsequent PR #382 workflow. |
| Prepared development MCP workflow | PR #382; problem/status reads and prepared-VM control complete. No extra model run; existing trial VM was only queried while stopped, not upgraded or reset. [Workflow](development-mcp-workflow.md). |
| Mac/curl and custody foundations | Closed-local VM, install/update/rollback and non-operational trust-policy/custody rehearsal implemented. **Clean-Mac CURL.3 and operational release custody are not complete.** |

Completed actual-model allowances are exhausted, not recurring permission.
Both original canary sessions used existing ChatGPT authentication, not a new
API key. Further model runs require their own applicable scope/budget.
Historical fixed replay/canary grants remain non-issuable and unchanged.

## Operational boundaries

No new production release, public testnet/P2P, mining, real payment/wallet movement,
reward, consensus-state or activation authority follows from this plan.
`mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`,
`Base activation=false`, `activationAllowed=false` remain the recorded posture.
Those useful-work readiness values do not mean ordinary test-coin transfer
development must wait for BF.7. A proposed network still needs its own scope,
rules and explicit execution authority.

Latest paid verification buyer/LOI counts were not verified in this document
review. No previous count is promoted to a current demand measurement.
