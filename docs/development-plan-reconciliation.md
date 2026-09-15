# Development plan reconciliation

Reviewed: 2026-09-15, against main `649fcff` (PR #382).
This is a documentation/code-evidence review, not a fresh runtime or security
certification. The [current status](current-development-status.md) owns the
execution order; this document owns the rationale for this reconciliation.
No runtime, monetary parameters, consensus rules, operational grants or
historical execution results were changed.

## One owner per kind of information

| Document | Role |
|---|---|
| Tracked current-development-status | Current completed boundary, ordered R1–R4 / U1–U2 roadmap, execution limits |
| Local Master | Product invariants, native-transfer/testnet acceptance and BF prerequisite contracts |
| Local Execution | Current cursor, major completions and transition conditions; no repeated canary diary |
| Local remaining-requirements | Original requirement IDs, status, applicable stage and precise remaining scope |
| Local thesis-realization-roadmap / current-product-thesis | Evidence versus claims, parallel demand work and conditional T0/C1/C2/C3/Z research |
| Local Hardening / Production readiness | Maps to the same requirement owners; no second backlog or mandatory all-P3 launch gate |

The local planning files stay in their existing ignored locations; they are
not protocol/release trust roots and were not force-added to Git.
This tracked review and the current-status document carry the durable summary.
Original archived plans, accepted ADR decisions, RP status blocks and experiment
records remain intact. A historical unchecked box is not a current open task.

## Ordering corrections

- Native test-coin transfer is R1 implementation/closed-local integration and
  R3 public-testnet evidence, not a task deferred until real-money payments.
  The observed credit-only implementation means it remains genuinely open.
- R2 checks the declared public release scope after R1. Public P2P encryption
  and peer authentication remain mandatory. Clean-Mac CURL.3 stays the final
  installation check, deferred at the user's request; no clean-Mac PASS is
  inferred from developer-machine or CI success.
- R3 explicitly requires actual mined test-coin transfers and multi-operator
  agreement on balances, nonces, fees and supply, plus attack/recovery cases.
  The historical loopback three-node exercise and mock faucet do not pass it.
- Useful-work U1/BF.7 remains HOLD behind adapter-scoped supply/RP0-MD, DA and
  deterministic resources. Its `no_protocol_reward` receipt consensus does not
  prevent the separate base test-coin transfer path.
- U2/BF.8 follows economic draft/preregistration → experiment → evidence-informed
  Economic ADR Accepted → separate useful-reward activation. The accepted final
  economics is not incorrectly required before the experiment that informs it.
- R4 explicitly owns the mainnet/real-value go/no-go and launch plan, final
  monetary parameters and operational release/custody. Testnet completion is
  not launch authorization; including useful rewards also requires U1/U2.
- Buyer/LOI discovery, external work requests, supply checks and economic drafts
  are parallel work. Final real facilitator/escrow/reputation products and
  C1/C2/C3/Z are not blanket prerequisites for the base testnet.
  No current buyer/LOI count was verified.

## Completion and residual corrections

The following rows cite existing code/tests and completed PR evidence.
They do not claim those Rust/Lean tests were freshly rerun in this docs-only task.

| Requirement / old ambiguity | Current disposition and evidence |
|---|---|
| MCP workflow still under “next development” | **Complete**, PR #382: read-only discovery/status and prepared Lima start/status/stop. [Workflow](development-mcp-workflow.md). Automatic provisioning and a new end-to-end Mac run are not included. |
| Real caller still described as future | **Complete within capped runs**: [direct canary](boole-direct-model-canary-2026-09-08.md), [three new tasks](boole-new-task-model-evaluation-2026-09-09.md), [user trial](boole-local-mcp-trial-2026-09-15.md). No repeat allowance or broader-family claim. |
| W10 native bridge / caller | Closed for the named primitive/trace/caller/discovery boundaries above. Public deployment and reward stay separate. |
| H.1–H.4 / S01, storage S13 | Closed for N5.3 and September fixes. [MVP closeout](verified-answer-local-mvp-closeout.md), [audit results](audit-remediation-2026-09.md). New transfer state must add its own replay/reorg coverage in E13. |
| H.5 / S02 artifact-only audit | Closed, not merely “isolation partial”: [Audit.lean](../lean/checker/BooleCheck/Audit.lean) consumes an artifact and kernel-replays it without parsing submitted source; [real-checker regressions](../crates/boole-lean-runner/tests/real_checker.rs) cover artifact input and exactly-once elaboration side effects. M5/PR #365 plus later cleanup evidence apply. |
| H.6 / S03 setup versus read confinement | Setup failure handling is complete: [runner](../crates/boole-lean-runner/src/lib.rs) propagates seccomp/Landlock construction errors and requires full enforcement. General read confinement is not complete: legacy Landlock handles write/execute rights. Do not generalize the qualified native boundary to every legacy deployment. |
| H.7 / S04 “zero mutation before dedup” | Superseded by the already-adopted M6 contract, not a fresh weakening: [HTTP admission](../crates/boole-node/src/local_node.rs) retains bounded replay tombstones/anti-abuse charges but removes eligible effects; [duplicate-proof regression](../crates/boole-node/tests/no_duplicate_proof_credit.rs) asserts an empty eligible pool and [admission tests](../crates/boole-core/tests/admission_fixtures.rs) preserve valid quota. The old no-mutation recipe is not a new open defect. |
| H.8 / S05 / W05 session policy | Partial: [node session regressions](../crates/boole-node/tests/submit_session_policy.rs) cover signatures, expiry/revocation, replay and recipient binding. [SessionPolicy](../crates/boole-core/src/session_policy.rs) has local allowlists/per-request authorization, but parsing daily_fee_cap is not an atomic daily-spend ledger. W05 owns the remaining policy choice; do not duplicate it as another implementation task. |
| H.9 / W06 wallet-agent trust | Still open, statically confirmed: [CLI](../crates/boole-cli/src/main.rs) retains wallet-agent PATH fallback/override and its spawn does not clear the parent environment; [AgentSigner](../crates/boole-miner/src/proof_signer.rs) also inherits environment. The separate node-child env_clear fix does not close it. No exploit was executed. |
| H.10 / S06 verifier resources | Partial: the node's shared semantic-verifier permit/worker lifecycle is implemented; complete cross-bounty/adapter aggregate memory/queue/resource isolation is not established by that alone. Keep only that residual scope. |
| H.11 / S07 transport | Local MCP limits/responsiveness/discovery reads complete; public encryption/peer identity/HTTPS and the declared RPC exposure remain R1/R2. A 5-second/64-KiB native loopback read limit is not certification of public transport. |
| W01–W04 wallet UX/memory | [CLI wallet enum](../crates/boole-cli/src/main.rs) exposes Init/Address/Sign/Migrate, not mnemonic restore, unlock sessions or change-password. [Wallet agent](../crates/boole-wallet-agent/src/main.rs) and signer use zero-on-drop buffers; mlock/full runtime erasure remain unproved. Recovery/safe handling is a release outcome, while BIP39, a daemon/TTL or OS keychain are design choices. |
| E03/E04 reward recovery | [Runtime](../crates/boole-node/src/runtime.rs) re-derives bounty settlement from canonical block inputs; durability/publication recovery is covered by #362/#372. A specific periodic reconciler/sweeper or extra stored credit fields are not automatically mandatory. Paid bounty expiry/escrow semantics still need a decision and consumer coverage if exposed. |
| E08 “settlement CLI absent” | Existing legacy `chain settlement-report` and read-only reputation export are complete; [CLI regressions](../crates/boole-cli/tests/replay_cli.rs) cover output and audit failure. Future transfer debit/fee/supply and real-payment budget reporting remain separate. |
| E13/E14 native transfer/monetary implementation | Genuinely open: [checked_credit](../crates/boole-core/src/accounting.rs), [reward-store apply](../crates/boole-node/src/reward_store.rs) and the account/wallet CLI show credits/balance/signing, not transfer/debit/block integration. Accepted ADR-0010/0011 structure is not implementation or final numeric policy. |
| S09 storage schema | Explicit node_storage generations and rejection/preservation are complete per #371; new transfer/nonce storage and other wire/migration surfaces remain to be checked. Do not label the entire schema boundary missing. |
| S14 / historical Batch E | All three original requirements have matching implementation/tests: the [miner CLI](../crates/boole-miner/src/cli.rs) dev-tools-gates deterministic nonces, [bounty verifier test](../crates/boole-node/tests/bounty_lean_verifier.rs) rejects a sorry-carrying submission before spawning Lean, and [node CLI](../crates/boole-node/src/main.rs) computes invalidAccepted from verifier/share outcomes with a four-case truth-table test. Closed for that scope, not a claim about every downstream safety metric. |
| D07 / RM2.9 admission reverify | The configured base-lane admission reverify is implemented by the M6 shared HTTP/P2P verifier path. Retire the old “after N0.4” wait; unrelated cookbook/signature/port cleanup remains optional and consumer-dependent. |
| D10 historical preflight/flaky note | Preserve the completed historical preflight; do not carry an old flaky-test claim as a current defect. Public value-flow evidence is the separate N02 milestone. |
| D12 clean-Mac/release | Partial, not complete and not a blocker for all local work. Closed-local installation/update/rollback and rehearsal exist; clean-Mac CURL.3 and operational custody remain distinct release boundaries. |

All 55 prior W/E/S/D requirement IDs remain represented; N01–N03 add the missing
launch-readiness, public-testnet experiment and mainnet decision boundaries.
Completed and superseded rows are explicitly excluded from the open backlog.
Items still labeled “recheck” are not certified bugs and were not falsely closed.

## Removed as current instructions, not erased as history

- Repeating completed N5.3/MCP/canary/model-evaluation milestones.
- Treating all wallet design alternatives, all P3 products, GUI, broad refactors
  or long-term research as required before the first public base testnet.
- Treating a receipt, reward counter, generic signature or
  [mock faucet smoke](../scripts/smoke-testnet-faucet-to-block.sh) as native
  transfer evidence.
- Reusing old SC/N5 waiting language, all-slice full-suite obligations, prose
  digest synchronization or original implementation recipes against the current
  development policy and already-landed behavior.

No source, fixture, user note, archived plan or execution evidence was deleted.
The existing user modification to `tasks/lessons.md` is outside this change.
