# Boole — current development status

Updated: 2026-09-23. This is the tracked entrypoint for current progress and the
ordered development roadmap. Edit it in place; execution evidence stays in its
own result record. Local Master/Execution documents link here instead of owning
a second current cursor. Work methods follow the
[development policy](development-throughput-and-evidence-policy-v1.md).

## Current boundary

**R1: the 1-billion-cap native test-coin path and secure local peers are connected.**
The operator approved producer-only base issuance for the new base testnet,
with all transfer fees to the producer and useful-work rewards remaining OFF.
The operator selected a test-only 1,000,000,000 tBOOLE ceiling, 50,000 initial
block reward, 10,000-block halving, 8 decimals and 60-second target; the compiled
policy also fixes the minimum fee and reward maturity. The new
`boole-native-testnet-1` binds these values into its own genesis. Actual mined
and owner-signed blocks now drive durable balances, mempool, replay/reorg,
bounded loopback RPC and encrypted-vault CLI transfers. Two independent local
nodes now automatically synchronize over mutually pinned TLS and propagate
signed transfers; the CLI preserves transactions before broadcast and retries
the same nonce. See the
[contract, tests and limits](native-transfer-ledger-contract.md).

**Next: R1 large-state resource limits and broader abuse/operations readiness.**
The foreground native node remains separate
from the legacy credit-only node. TLS identity, incremental sync, bounded pending
pull and local partition/rejoin are implemented; public listeners remain refused.
Bounded offline export/import now covers long-fork recovery beyond the online
suffix and manual RPC caps, with source preservation, expected-head binding and
independent validation. A 1,025-block recovery preserves orphaned transfers.
Pending admission now retains verified reservations and IDs, stages only affected
accounts and publishes after durable append. Full 512-transaction queues,
16,384-account fixtures and failure/reorg/restart paths pass focused checks.
Stopped-node `native-audit` now independently replays canonical history, checks
balance/issuance/reward-lock conservation and reports exact decimal totals without
repairing source journals. An optional independently retained head detects a
valid-prefix rollback; a self-consistent audit alone cannot prove latest history.
Boot/audit now refuse a missing canonical file instead of treating existing state
as empty genesis. Ambiguous old or interrupted initialization is preserved and
requires a fresh recovery directory. See the
[accounting audit workflow and limits](native-transfer-ledger-contract.md#offline-canonical-accounting-audit).
Storage bytes/limits and canonical account/transaction counts are now observable
through `native info`. A [preregistered developer-Mac scenario](native-capacity-qualification-2026-09.md)
passes at 131,073 balance entries and ~67MiB history, including a full queue and
two reopens. Initial maximum RSS was 327.125MiB; requalification after the recent
fork changes measured 291.71875MiB under the same criteria. Full replay still
takes about 52–53s;
recent fork recovery now reuses only locally verified inverse history, validates
every new suffix and rejects out-of-window live forks. Independent replay parity,
same-hash tampering, pending nonce dependencies and all six reorg publication
failure boundaries pass direct checks. The prior large-state live-fork baseline
took 58.049s and failed its fixed 10s bound; the
[corrected qualification](native-recent-fork-qualification-2026-09.md) passed at
3.251s, with 51.869s restart and 551.0625MiB peak RSS. This uses more peak memory
than the baseline and does not remove whole-history cloning/writing or startup.
Larger histories, concurrency/abuse and other hardware remain.
Encrypted owner-wallet backup/restore, bounded vault/KDF/file handling, isolated
agent environment/lifetime/output and native stdin passphrases now pass direct
recovery tests, including a restored-wallet on-chain transfer. This is the selected
local-vault path, not operational custody or an OS-keychain/mnemonic implementation.
Saved signed transfers can also be inspected offline for their exact ID and
public fields without a node, wallet, password or broadcast. The report explicitly
checks only signature/format, not balance, current nonce/expiry or chain status.
Public/untrusted participation still needs the remaining operational acceptance,
public RPC scope and R2/R3 release/launch decisions.
Existing v3 data and verification rules remain unchanged; R1 is not complete.

Under the operator's twelve-hour autonomous-development delegation on September
23, the selected initial external-participant policy is an explicit node-key
allowlist, not open enrollment. TLS 1.3 mutual raw-public-key authentication uses
separate mode-0600 transport keys, bounded incremental sync and partition/recovery
tests. The CLI exposes peer status, resource counters, cooldown and retry backoff.
Each configured peer now has its own bounded outgoing worker (at most eight),
so one stalled authenticated peer does not serialize every other peer's I/O.
Focused tests cover healthy progress during a stall, eight stalled rounds with
shutdown and concurrent duplicate-transfer pulls. A subsequent
[eight-peer resource qualification](native-peer-resource-qualification-2026-09.md)
passed 131,073 canonical balances plus eight simultaneous incomplete fork buffers
at 551.90625MiB process RSS, with budget rejection, unchanged journals/accounting
and 51.784s restart. A subsequent
[same-candidate convergence qualification](native-peer-convergence-qualification-2026-09.md)
passed with all eight peers simultaneously holding fifteen blocks before final
release: one 16-block adoption, seven stale-snapshot retries, no duplicate
accounting and all peers matching within 8.292s at 544.6875MiB peak RSS. Full
independent replay took 55.040s. Distinct competing forks, mixed traffic and
worst-case shared-lock read latency remain unqualified. All non-loopback
listeners/endpoints are still refused; an explicit exposure option is not implemented.
Each outgoing worker also remembers at most one fully verified losing fork for
the exact unchanged local/remote heads, avoiding repeated suffix downloads and
revalidation. Head changes/restart invalidate that memory; malformed candidates
do not create it, and readiness remains checked. First verification and changing
candidate costs remain; this is not a general CPU-abuse guarantee.
A [preregistered 131,073-balance repeated-fork scenario](native-peer-repeat-qualification-2026-09.md)
passed: first full candidate 4.999s, seven following polls without data requests,
8.637s network phase, 480.078125MiB peak RSS and unchanged accounting/journals.
Local native HTTP also acquires its eight request slots before body decoding,
retaining the same slot through actual node work even after a caller timeout.
Raw slow-upload and delayed-mutation tests cover early 429, slot return and
one durable block/reward after eight timed-out duplicate submissions. This bounds
admission, not total memory or guaranteed read availability under saturation.
Input-reflecting HTTP error bodies are also limited to 4KiB, preserving status
and replacing oversized diagnostics with a short code.
Exact already-known transfers still repeat immutable signature checks. A
[developer retry baseline](native-known-transfer-qualification-2026-09.md)
preserved state/journals but took 2.749s for 8,192 retries against a fixed 1s
criterion. The proposed shortcut is on safety-review hold and was not applied;
field-binding, conflicting-signature and storage-loss regression checks pass on
the unchanged validation path.
Tests remain
closed-local and public operation still requires R2/R3 launch authority.

The completed MCP workflow remains recorded below. Only disposable closed-local
tests mined and moved test coins; no new VM/model run, public deployment,
operator mining or operator wallet transaction was executed.
[Reconciliation findings](development-plan-reconciliation.md) preserve the
earlier roadmap review; this status owns the current implementation cursor.

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
| R1 | Native transfer and launch-critical product/security/operations: signed network-bound transfers, debit/fee/nonce state, block/replay/reorg integration, wallet safety/recovery for the selected testnet UX, secure public P2P/RPC, resource limits and operator recovery/observability. Pin test-only monetary parameters and experiment criteria before running the new network. | **In progress, incomplete.** Native issuance/transfer, owner-vault recovery, mutually pinned TLS, incremental sync, local rejoin and bounded offline long-fork recovery pass direct tests. Broader fault/abuse acceptance, public RPC scope and large-state storage readiness remain. |
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
| R1 native transfer integration | Compiled test-only monetary policy, actual PoW/owner-signed transfer blocks, durable pool/replay/reorg, bounded loopback RPC and owner-vault CLI/outbox. [Contract and automated closed-local evidence](native-transfer-ledger-contract.md). Not public P2P, public mining or R1 completion. |
| R1 secure peer integration | Mutually pinned TLS, bounded incremental block sync/pending pull, local partition/rejoin, transport-key CLI and resource/status/shutdown guards. [Contract and automated evidence](native-transfer-ledger-contract.md#mutually-authenticated-native-peers). Closed-local only; wider operations acceptance remains. |
| R1 encrypted wallet recovery | Authenticated no-overwrite backup/restore, unchanged v1/default KDF, bounded secret/file/agent processing, explicit stdin signing and a restored-owner on-chain transfer. [Contract and limitations](native-transfer-ledger-contract.md#encrypted-owner-vault-backup-and-restore). Disposable local evidence, not operational key custody. |
| R1 offline node recovery | Source-preserving new-file export, expected-head/full-validation import, 1,025-block long-fork recovery and orphan requeue, bounded stable files/manifest and normal fork choice. [Workflow and limits](native-transfer-ledger-contract.md#bounded-offline-chain-recovery). Not unlimited history or production-scale storage. |
| R1 offline accounting diagnostics | Confirmed canonical supply/balance/lock and gross transfer/fee audit, source-preserving replay, expected-head check and missing-history fail-closed boot. [Workflow and limits](native-transfer-ledger-contract.md#offline-canonical-accounting-audit). Unsigned local diagnostics, not finality or latest-state attestation. |
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
