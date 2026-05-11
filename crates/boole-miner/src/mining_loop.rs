// Mining loop wire-up — composes per-step modules into the full pipeline.
//
//   fetch_head → grind_ticket → announce_ticket → for j ∈ [0, M):
//       emitter.emit  → driver.generate (with_retry)
//                     → ProofIntakeV1
//                     → canonicalizer.canonicalize
//                     → verifier.verify
//                     → grind_share         (until score ≥ MinShareScore)
//                     → grind_submission_pow (until hash < T_submit)
//                     → submit_client.submit
//
// All collaborators are injected via `MiningLoopDeps` so integration tests
// can swap in stubs without touching network or processes. The loop owns
// no I/O.
//
// Stop conditions:
//   - `opts.max_shares` accepted shares submitted
//   - `opts.max_cycles` ticket cycles completed
//   - `opts.cancel.load()` flips to `true`
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use boole_core::{difficulty_weight, Hex32};
use num_traits::One;

use crate::canonicalizer::{Canonicalizer, Target};
use crate::chain_head::{ChainHead, ChainHeadError, ChainHeadFetcher};
use crate::family_v1_lenbound;
use crate::grinder::{grind_share, grind_submission_pow, CounterNonce, GrinderConfig, OsRngNonce};
use crate::llm_driver::{
    with_retry, GenerateResult, ProverDriver, RetryConfig, Sleeper, ThreadSleeper,
};
use crate::local_verify::{Verifier, VerifyResult};
use crate::proof_intake::{
    ProofIntakeV1, ProofTransport, PROOF_BODY_CONTRACT_VERSION, PROOF_CANONICALIZER_VERSION,
};
use crate::proof_package::bppk_canon_hash;
use crate::submit_client::{
    AnnounceTicketInputs, AnnounceTicketResult, SubmitInputs, SubmitResult, Submitter,
};
use crate::target_emitter::{TargetEmitArgs, TargetEmitter};

/// Default prompt cookbook — ports pof's `solver_frontier.py` SYSTEM_PROMPT.
/// The verifier emits a module that imports `Boole.Family.V0Helpers` and
/// inserts the model's response verbatim after `:=`, so the model must
/// produce a term-mode lambda only.
const COOKBOOK: &str = r#"You are a Lean 4 proof engineer for the Boole-v3.1.1 ListInvariantsV0 family.

The verifier elaborates a module of this exact shape:

  import Boole.Family.V0Helpers
  namespace BooleVerifyMod
  open Boole.Family.V0Helpers
  -- (library distractor defs)
  theorem instance_thm : ∀ (xs : List Int), <body> := <YOUR_PROOF>
  end BooleVerifyMod

Your output is inserted verbatim after `:=`. Term mode only — write a lambda
like `fun xs => <term>`. No `by`, no tactics, no `sorry`, no `_` for
universe params, no `import` lines, no `theorem`/`namespace` headers.

## Available helpers (already opened, write unqualified)

  filterByPred (p : Int → Bool) (xs : List Int) : List Int    -- = xs.filter p
  mapAdd       (k : Int)        (xs : List Int) : List Int    -- = xs.map (· + k)
  mapMul       (k : Int)        (xs : List Int) : List Int    -- = xs.map (· * k)
  dedup                         (xs : List Int) : List Int    -- = xs.eraseDups
  sortAsc                       (xs : List Int) : List Int    -- = xs.mergeSort (≤)

## Witness lemmas — choose the one matching the rendered invariant

  -- "every element satisfies p" — chain ends with `filterByPred p`:
  all_filterByPred_self : ∀ (p : Int → Bool) (xs : List Int),
      (filterByPred p xs).all p = true

  -- "is sorted in ascending order" — chain ends with `sortAsc`:
  pairwise_sortAsc : ∀ (xs : List Int), List.Pairwise (· ≤ ·) (sortAsc xs)

  -- "has no duplicates and preserves first-occurrence order" — ends with `dedup`:
  nodup_dedup : ∀ (xs : List Int), List.Nodup (dedup xs)

  -- "equals (xs.filter p, xs.filter (not p))" — universal, no last-op constraint:
  partition_eq_filter_filter : ∀ (p : Int → Bool) (xs : List Int),
      xs.partition p = (xs.filter p, xs.filter (fun x => !(p x)))

Each witness ignores everything BEFORE the last op: `nodup_dedup _` proves
`List.Nodup (dedup _)` no matter what the inner chain is. The v0.2+ generator
guarantees the last op of every chain is the witness op for its invariant
class, so a single witness application closes the goal.

## Canonical single-conjunct proofs (v01 / v02 / v03 with N=1)

  every element satisfies …          : fun xs => all_filterByPred_self _ _
  is sorted in ascending order       : fun xs => pairwise_sortAsc _
  has no duplicates …                : fun xs => nodup_dedup _
  equals (xs.filter …, xs.filter …)  : fun xs => partition_eq_filter_filter _ _

## v0.3+ compound goals (only when N >= 2)

Goal shape: `∀ xs, P_1 (chain_1 xs) ∧ P_2 (chain_2 xs) ∧ … ∧ P_N (chain_N xs)`.
Close with an anonymous-constructor product over the matching witness lemmas:

  fun xs => ⟨w_1 _, w_2 _, …, w_N _⟩

`A ∧ B ∧ C` is right-associative; `⟨…⟩` flattens through the nested `And`
automatically. Apply exactly N witnesses, left-to-right matching the conjunct
order in the rendered description.

## v0.3.1 `lengthPreserved` branches ("the result has the same length as the input")

No single witness lemma; compose one length lemma per op via `Eq.trans`:

  length_mapAdd  : (mapAdd k xs).length = xs.length
  length_mapMul  : (mapMul k xs).length = xs.length
  length_sortAsc : (sortAsc xs).length = xs.length

Read INSIDE-OUT — outermost op leads, then peel via `.trans`:

  -- chain = mapAdd k1 ▷ mapMul k2 ▷ sortAsc   (sortAsc outermost)
  fun xs =>
    (length_sortAsc _).trans
      ((length_mapMul k2 _).trans (length_mapAdd k1 xs))

Single-op chain needs no `.trans`:  `fun xs => length_mapAdd 3 xs`.
Two-op chain (mapMul outermost):     `fun xs => (length_mapMul k _).trans (length_mapAdd k xs)`.

In an N-ary conjunction with mixed invariants, each lengthPreserved branch
takes its own composition closure as one slot of the `⟨…⟩` product.

## Forbidden — these names do NOT exist in core Lean 4 stdlib

  List.Nodup.dedup, List.sorted_sort, List.nodup_dedup,
  List.sorted_merge_sort, List.forall_filter, List.sorted_sort_ascending,
  ListInvariantsV0.dedup_preserves_order

If you need a fallback, try the real core lemmas:
  List.all_filter, List.pairwise_mergeSort, List.partition_eq_filter_filter,
  List.mem_eraseDups, List.eraseDups_cons.

## Output format — STRICT

Respond with a single fenced ```lean block containing ONLY the proof term.
No prose before or after the fence. Example:

```lean
fun xs => nodup_dedup _
```
"#;

pub trait PromptBuilder: Send + Sync {
    fn build_prompt(&self, target: &Target) -> String;
}

pub struct DefaultPromptBuilder;

impl PromptBuilder for DefaultPromptBuilder {
    fn build_prompt(&self, target: &Target) -> String {
        if target.profile == "v1-lenbound" {
            return format!(
                "## Boole v1 length-bound proof task\n\
                 You are proving a Boole calibration length-bound target.\n\
                 Contract: prove that the rendered chain never increases list length.\n\n\
                 ## Official helper surface\n{}\n\n\
                 ## Output format — STRICT\n\
                 Respond with one Lean proof body only for the theorem body slot.\n\
                 `by` tactic blocks are allowed. Do not restate the theorem.\n\
                 Do not include Markdown fences, prose, imports, namespace declarations, `sorry`, or `admit`.\n\n\
                 ## This instance\nProfile: {}, D={}, N={}.\nRendered description:\n{}",
                family_v1_lenbound::helper_manifest(),
                target.profile,
                target.d,
                target.n,
                target.render
            );
        }

        format!(
            "{COOKBOOK}\n## This instance\nProfile: {}, D={}, N={}.\nRendered description:\n{}",
            target.profile, target.d, target.n, target.render
        )
    }
}

pub struct MiningLoopDeps {
    /// Miner's own ed25519 public key as Hex32 (used in every grinder).
    pub pk: Hex32,
    pub chain_head: Box<dyn ChainHeadFetcher>,
    pub emitter: Box<dyn TargetEmitter>,
    pub driver: Box<dyn ProverDriver>,
    pub verifier: Box<dyn Verifier>,
    pub canonicalizer: Box<dyn Canonicalizer>,
    pub submit_client: Box<dyn Submitter>,
    pub prompt_builder: Option<Box<dyn PromptBuilder>>,
    pub log: Option<LogSink>,
    pub sleeper: Option<Box<dyn Sleeper>>,
}

/// Mining loop event sink. Boxed `Fn(&MiningEvent)` is shared across the
/// async-style boundary inside `run_mining_loop`, so it must be `Send + Sync`.
pub type LogSink = Box<dyn Fn(&MiningEvent) + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MiningRunVerifierMode {
    #[default]
    RealVerifier,
    MockAccept,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MiningRunDriverMode {
    #[default]
    RealLlmOrAgent,
    MockLlmResponse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MiningRunTargetMode {
    #[default]
    ChainDerived,
    FixedSeed,
    Stub,
}

#[derive(Debug, Clone, Default)]
pub struct MiningRunContext {
    pub verifier_mode: MiningRunVerifierMode,
    pub driver_mode: MiningRunDriverMode,
    pub target_mode: MiningRunTargetMode,
}

impl MiningRunContext {
    fn is_mock_verifier(&self) -> bool {
        self.verifier_mode == MiningRunVerifierMode::MockAccept
    }

    fn is_mock_driver(&self) -> bool {
        self.driver_mode == MiningRunDriverMode::MockLlmResponse
    }

    fn is_fixed_or_stub_target(&self) -> bool {
        matches!(
            self.target_mode,
            MiningRunTargetMode::FixedSeed | MiningRunTargetMode::Stub
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct MiningLoopOptions {
    pub max_shares: Option<u64>,
    pub max_cycles: Option<u64>,
    pub ticket_grind: GrinderConfig,
    pub share_grind: GrinderConfig,
    pub submit_grind: GrinderConfig,
    pub llm_retry: RetryConfig,
    pub run_context: MiningRunContext,
    /// Optional cancel flag — flip to `true` to stop the loop after the
    /// next checkpoint.
    pub cancel: Option<Arc<AtomicBool>>,
    /// Use `CounterNonce` instead of `OsRngNonce` for the ticket / share /
    /// submit-PoW grinders. Tests set this so a fixed `(c, pk, T_*)` pair
    /// produces a deterministic outcome.
    pub deterministic_nonces: bool,
}

#[derive(Debug, Clone)]
pub enum MiningEvent {
    HeadFetched {
        c_hex: String,
        m: u32,
    },
    LoopClassified {
        loop_class: String,
        public_scoring_eligible: bool,
        ineligibility_reasons: Vec<String>,
    },
    TicketFound {
        n_hex: String,
        hashes_attempted: u64,
        elapsed_ms: u128,
    },
    TicketAnnounced {
        result: AnnounceTicketResult,
    },
    TicketExhausted {
        hashes_attempted: u64,
    },
    TargetEmitted {
        j_index: u32,
        seed_hex: String,
    },
    LlmOutcome {
        j_index: u32,
        outcome: LlmOutcomeKind,
        elapsed_ms: u128,
        reason: Option<String>,
        proof_contract_version: &'static str,
        canonicalizer_version: &'static str,
        model_specific_overrides: bool,
    },
    VerifyOutcome {
        j_index: u32,
        accepted: bool,
        reason: String,
        elapsed_ms: u128,
        attempt_artifact_path: Option<PathBuf>,
    },
    ShareFound {
        j_hex: String,
        is_proposer: bool,
        hashes_attempted: u64,
    },
    ShareGrindExhausted {
        j_index: u32,
        hashes_attempted: u64,
    },
    SubmitPowFound {
        nonce_s_hex: String,
        hashes_attempted: u64,
    },
    SubmitPowExhausted {
        hashes_attempted: u64,
    },
    SubmitOutcome {
        result: SubmitResult,
    },
    HeadAdvancedMidCycle {
        old_c_hex: String,
        /// Fresh `c` from a successful re-fetch after `SubmitAccepted`. `None`
        /// for `StaleCRejection` since the dispatcher only signaled staleness
        /// without surfacing the new head.
        new_c_hex: Option<String>,
        reason: HeadAdvanceReason,
    },
    CycleComplete {
        cycle: u64,
    },
    HeadFetchFailed {
        error: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadAdvanceReason {
    /// `/head` re-fetched after an Accepted submit reported a different `c`.
    SubmitAccepted,
    /// Dispatcher rejected with a StaleC-flavored reason mid-cycle.
    StaleCRejection,
}

impl HeadAdvanceReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            HeadAdvanceReason::SubmitAccepted => "submit_accepted",
            HeadAdvanceReason::StaleCRejection => "stale_c_rejection",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmOutcomeKind {
    Answered,
    IntakeRejected,
    Rejected,
    Error,
}

impl LlmOutcomeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            LlmOutcomeKind::Answered => "answered",
            LlmOutcomeKind::IntakeRejected => "intake_rejected",
            LlmOutcomeKind::Rejected => "rejected",
            LlmOutcomeKind::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentRuntimeReport {
    pub driver_calls: u64,
    pub driver_answered: u64,
    pub driver_rejected: u64,
    pub driver_errored: u64,
    pub proof_intake_accepted: u64,
    pub proof_intake_rejected: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProtocolReport {
    pub cycles_run: u64,
    pub tickets_found: u64,
    pub verify_accepted: u64,
    pub verify_rejected: u64,
    pub shares_accepted: u64,
    pub shares_rejected: u64,
    pub rate_limited: u64,
    pub network_errors: u64,
    /// Dispatcher returned a protocol rejection on `announce_ticket`
    /// (e.g. 4xx). Distinct from `network_errors`, which tracks transport
    /// failures, so operational alerts on transport health can ignore
    /// protocol-level rejections.
    pub announce_rejected: u64,
    pub proposer_shares: u64,
    pub loop_class: String,
    pub public_scoring_eligible: bool,
    pub ineligibility_reasons: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MiningLoopOutcome {
    pub agent: AgentRuntimeReport,
    pub protocol: ProtocolReport,
}

/// Compatibility alias for callers while the report split is adopted.
pub type MiningLoopSummary = MiningLoopOutcome;

fn aborted(cancel: Option<&Arc<AtomicBool>>) -> bool {
    cancel.map(|c| c.load(Ordering::SeqCst)).unwrap_or(false)
}

#[derive(Debug, Clone)]
struct LoopClassification {
    loop_class: String,
    public_scoring_eligible: bool,
    ineligibility_reasons: Vec<String>,
}

fn classify_loop(head: &ChainHead, context: &MiningRunContext) -> LoopClassification {
    let mut reasons = Vec::new();
    if context.is_mock_verifier() {
        reasons.push("mock_verifier".to_string());
    }
    if context.is_mock_driver() {
        reasons.push("mock_llm".to_string());
    }
    if context.is_fixed_or_stub_target() {
        reasons.push(if context.target_mode == MiningRunTargetMode::Stub {
            "stub_target".to_string()
        } else {
            "fixed_target_seed".to_string()
        });
    }
    if has_open_thresholds(head) {
        reasons.push("open_thresholds".to_string());
    }
    if head.min_share_score == num_bigint::BigUint::default() {
        reasons.push("open_min_share_score".to_string());
    }

    let public_scoring_eligible = reasons.is_empty();
    let loop_class = if !public_scoring_eligible {
        "smoke"
    } else {
        "public_mining"
    };

    LoopClassification {
        loop_class: loop_class.to_string(),
        public_scoring_eligible,
        ineligibility_reasons: reasons,
    }
}

fn has_open_thresholds(head: &ChainHead) -> bool {
    [&head.t_ticket, &head.t_share, &head.t_block, &head.t_submit]
        .iter()
        .any(|threshold| {
            difficulty_weight(threshold)
                .map(|w| w <= num_bigint::BigUint::one())
                .unwrap_or(true)
        })
}

/// Drive one cycle of the mining loop: head → ticket → M targets → submit.
/// Returns `None` if the run should stop (cancel / max-shares / max-cycles).
pub fn run_mining_loop(deps: MiningLoopDeps, opts: MiningLoopOptions) -> MiningLoopSummary {
    let log: Box<dyn Fn(&MiningEvent)> = match deps.log {
        Some(l) => Box::new(move |e: &MiningEvent| l(e)),
        None => Box::new(|_| {}),
    };
    let prompt_builder: Box<dyn PromptBuilder> = deps
        .prompt_builder
        .unwrap_or_else(|| Box::new(DefaultPromptBuilder));
    let sleeper: Box<dyn Sleeper> = deps.sleeper.unwrap_or_else(|| Box::new(ThreadSleeper));

    let max_shares = opts.max_shares.unwrap_or(u64::MAX);
    let max_cycles = opts.max_cycles.unwrap_or(u64::MAX);

    let mut summary = MiningLoopSummary::default();

    while summary.protocol.cycles_run < max_cycles && summary.protocol.shares_accepted < max_shares
    {
        if aborted(opts.cancel.as_ref()) {
            break;
        }

        let head = match deps.chain_head.fetch_head() {
            Ok(h) => h,
            Err(err) => {
                log(&MiningEvent::HeadFetchFailed {
                    error: render_chain_head_error(&err),
                });
                summary.protocol.network_errors += 1;
                summary.protocol.cycles_run += 1;
                log(&MiningEvent::CycleComplete {
                    cycle: summary.protocol.cycles_run,
                });
                continue;
            }
        };
        log(&MiningEvent::HeadFetched {
            c_hex: head.c.to_hex(),
            m: head.m,
        });
        let classification = classify_loop(&head, &opts.run_context);
        // The summary always reflects the classification of the most recent
        // cycle so it agrees with the per-cycle `LoopClassified` event stream;
        // consumers that need per-cycle history should diff the events.
        summary.protocol.loop_class = classification.loop_class.clone();
        summary.protocol.public_scoring_eligible = classification.public_scoring_eligible;
        summary.protocol.ineligibility_reasons = classification.ineligibility_reasons.clone();
        log(&MiningEvent::LoopClassified {
            loop_class: classification.loop_class,
            public_scoring_eligible: classification.public_scoring_eligible,
            ineligibility_reasons: classification.ineligibility_reasons,
        });

        let ticket_outcome = grind_ticket_with_source(
            &head.c,
            &deps.pk,
            &head.t_ticket,
            opts.ticket_grind,
            opts.deterministic_nonces,
        );
        let ticket = match ticket_outcome {
            Some(t) => t,
            None => {
                log(&MiningEvent::TicketExhausted {
                    hashes_attempted: opts.ticket_grind.max_attempts.unwrap_or(0),
                });
                summary.protocol.cycles_run += 1;
                log(&MiningEvent::CycleComplete {
                    cycle: summary.protocol.cycles_run,
                });
                break;
            }
        };
        summary.protocol.tickets_found += 1;
        log(&MiningEvent::TicketFound {
            n_hex: ticket.nonce.to_hex(),
            hashes_attempted: ticket.hashes_attempted,
            elapsed_ms: ticket.elapsed_ms,
        });

        // Announce so the dispatcher's per-pk ceiling can accept M shares.
        let announce = deps.submit_client.announce_ticket(AnnounceTicketInputs {
            c_hex: &head.c.to_hex(),
            pk_hex: &deps.pk.to_hex(),
            n_hex: &ticket.nonce.to_hex(),
        });
        log(&MiningEvent::TicketAnnounced {
            result: announce.clone(),
        });
        match &announce {
            AnnounceTicketResult::Rejected { .. } => {
                summary.protocol.announce_rejected += 1;
                summary.protocol.cycles_run += 1;
                log(&MiningEvent::CycleComplete {
                    cycle: summary.protocol.cycles_run,
                });
                continue;
            }
            AnnounceTicketResult::NetworkError { .. } => {
                summary.protocol.network_errors += 1;
                summary.protocol.cycles_run += 1;
                log(&MiningEvent::CycleComplete {
                    cycle: summary.protocol.cycles_run,
                });
                continue;
            }
            _ => {}
        }

        let m = head.m;
        for j_index in 0..m {
            if aborted(opts.cancel.as_ref()) || summary.protocol.shares_accepted >= max_shares {
                break;
            }

            // Emit target.
            let target = match deps.emitter.emit(&TargetEmitArgs {
                c: &head.c,
                pk: &deps.pk,
                n: &ticket.nonce,
                j_index,
                d: head.d,
                profile: head.profile.clone(),
                n_param: head.n,
            }) {
                Ok(t) => t,
                Err(err) => {
                    summary.agent.driver_errored += 1;
                    log(&MiningEvent::LlmOutcome {
                        j_index,
                        outcome: LlmOutcomeKind::Error,
                        elapsed_ms: 0,
                        reason: Some(format!("emitter: {err}")),
                        proof_contract_version: PROOF_BODY_CONTRACT_VERSION,
                        canonicalizer_version: PROOF_CANONICALIZER_VERSION,
                        model_specific_overrides: false,
                    });
                    continue;
                }
            };
            log(&MiningEvent::TargetEmitted {
                j_index,
                seed_hex: target.seed_hex.clone(),
            });

            // LLM with retry.
            let prompt = prompt_builder.build_prompt(&target);
            summary.agent.driver_calls += 1;
            let llm = with_retry(
                deps.driver.as_ref(),
                &prompt,
                &opts.llm_retry,
                sleeper.as_ref(),
            );
            let proof_source = match &llm {
                GenerateResult::Answered {
                    answer, elapsed, ..
                } => {
                    summary.agent.driver_answered += 1;
                    let candidate = match ProofTransport::PlainText(answer.clone())
                        .into_envelope()
                        .and_then(ProofIntakeV1::extract)
                    {
                        Ok(candidate) => candidate,
                        Err(reason) => {
                            summary.agent.proof_intake_rejected += 1;
                            log(&MiningEvent::LlmOutcome {
                                j_index,
                                outcome: LlmOutcomeKind::IntakeRejected,
                                elapsed_ms: elapsed.as_millis(),
                                reason: Some(reason.as_str().to_string()),
                                proof_contract_version: PROOF_BODY_CONTRACT_VERSION,
                                canonicalizer_version: PROOF_CANONICALIZER_VERSION,
                                model_specific_overrides: false,
                            });
                            continue;
                        }
                    };
                    summary.agent.proof_intake_accepted += 1;
                    log(&MiningEvent::LlmOutcome {
                        j_index,
                        outcome: LlmOutcomeKind::Answered,
                        elapsed_ms: elapsed.as_millis(),
                        reason: None,
                        proof_contract_version: candidate.contract_version,
                        canonicalizer_version: candidate.canonicalizer_version,
                        model_specific_overrides: false,
                    });
                    candidate.proof_source
                }
                GenerateResult::Rejected { reason, elapsed } => {
                    summary.agent.driver_rejected += 1;
                    log(&MiningEvent::LlmOutcome {
                        j_index,
                        outcome: LlmOutcomeKind::Rejected,
                        elapsed_ms: elapsed.as_millis(),
                        reason: Some(reason.as_str().to_string()),
                        proof_contract_version: PROOF_BODY_CONTRACT_VERSION,
                        canonicalizer_version: PROOF_CANONICALIZER_VERSION,
                        model_specific_overrides: false,
                    });
                    continue;
                }
                GenerateResult::Error { cause, elapsed } => {
                    summary.agent.driver_errored += 1;
                    log(&MiningEvent::LlmOutcome {
                        j_index,
                        outcome: LlmOutcomeKind::Error,
                        elapsed_ms: elapsed.as_millis(),
                        reason: Some(cause.clone()),
                        proof_contract_version: PROOF_BODY_CONTRACT_VERSION,
                        canonicalizer_version: PROOF_CANONICALIZER_VERSION,
                        model_specific_overrides: false,
                    });
                    continue;
                }
            };

            // Canonicalize before verifier admission so verifier and share
            // hashing are downstream of the same intake-normalized proof body.
            let canon_bytes = match deps.canonicalizer.canonicalize(&proof_source, &target) {
                Ok(b) => b,
                Err(err) => {
                    summary.protocol.network_errors += 1; // track as generic failure
                    log(&MiningEvent::LlmOutcome {
                        j_index,
                        outcome: LlmOutcomeKind::Error,
                        elapsed_ms: 0,
                        reason: Some(format!("canonicalize: {err}")),
                        proof_contract_version: PROOF_BODY_CONTRACT_VERSION,
                        canonicalizer_version: PROOF_CANONICALIZER_VERSION,
                        model_specific_overrides: false,
                    });
                    continue;
                }
            };
            let canon_hash = bppk_canon_hash(&canon_bytes);

            // Verify.
            let verify: VerifyResult =
                deps.verifier
                    .verify(&target.seed_hex, target.d, &proof_source, head.n);
            log(&MiningEvent::VerifyOutcome {
                j_index,
                accepted: verify.accepted,
                reason: verify.reason.as_str().to_string(),
                elapsed_ms: verify.elapsed.as_millis(),
                attempt_artifact_path: verify.attempt_artifact_path.clone(),
            });
            if !verify.accepted {
                summary.protocol.verify_rejected += 1;
                continue;
            }
            summary.protocol.verify_accepted += 1;

            // Grind share.
            let share = grind_share_with_source(
                &head.c,
                &deps.pk,
                &ticket.nonce,
                &canon_hash,
                &head.min_share_score,
                Some(&head.t_block),
                opts.share_grind,
                opts.deterministic_nonces,
            );
            let share = match share {
                Some(s) => s,
                None => {
                    log(&MiningEvent::ShareGrindExhausted {
                        j_index,
                        hashes_attempted: opts.share_grind.max_attempts.unwrap_or(0),
                    });
                    continue;
                }
            };
            log(&MiningEvent::ShareFound {
                j_hex: share.j.to_hex(),
                is_proposer: share.is_proposer,
                hashes_attempted: share.hashes_attempted,
            });
            if share.is_proposer {
                summary.protocol.proposer_shares += 1;
            }

            // Grind submission PoW.
            let submit_pow = grind_submit_pow_with_source(
                &head.c,
                &deps.pk,
                &canon_hash,
                &head.t_submit,
                opts.submit_grind,
                opts.deterministic_nonces,
            );
            let submit_pow = match submit_pow {
                Some(p) => p,
                None => {
                    log(&MiningEvent::SubmitPowExhausted {
                        hashes_attempted: opts.submit_grind.max_attempts.unwrap_or(0),
                    });
                    continue;
                }
            };
            log(&MiningEvent::SubmitPowFound {
                nonce_s_hex: submit_pow.nonce_s.to_hex(),
                hashes_attempted: submit_pow.hashes_attempted,
            });

            // Submit.
            let submit_result = deps.submit_client.submit(SubmitInputs {
                c_hex: &head.c.to_hex(),
                pk_hex: &deps.pk.to_hex(),
                n_hex: &ticket.nonce.to_hex(),
                j_hex: &share.j.to_hex(),
                nonce_s_hex: &submit_pow.nonce_s.to_hex(),
                canon_bytes: &canon_bytes,
            });
            log(&MiningEvent::SubmitOutcome {
                result: submit_result.clone(),
            });
            let mut head_advanced: Option<HeadAdvanceReason> = None;
            match &submit_result {
                SubmitResult::Accepted { .. } => {
                    summary.protocol.shares_accepted += 1;
                    // The dispatcher may have promoted this share to a block,
                    // advancing `c`. Re-fetch /head; if `c` changed, the
                    // remaining j's in this cycle would all submit against a
                    // stale `c` (StaleC). Break out and start a fresh cycle.
                    if let Ok(fresh) = deps.chain_head.fetch_head() {
                        if fresh.c != head.c {
                            head_advanced = Some(HeadAdvanceReason::SubmitAccepted);
                            log(&MiningEvent::HeadAdvancedMidCycle {
                                old_c_hex: head.c.to_hex(),
                                new_c_hex: Some(fresh.c.to_hex()),
                                reason: HeadAdvanceReason::SubmitAccepted,
                            });
                        }
                    }
                }
                SubmitResult::Rejected { reason, detail, .. } => {
                    summary.protocol.shares_rejected += 1;
                    let mentions_stale_c = reason
                        .as_deref()
                        .map(|r| r.contains("StaleC"))
                        .unwrap_or(false)
                        || detail
                            .as_deref()
                            .map(|d| d.contains("StaleC"))
                            .unwrap_or(false);
                    if mentions_stale_c {
                        head_advanced = Some(HeadAdvanceReason::StaleCRejection);
                        log(&MiningEvent::HeadAdvancedMidCycle {
                            old_c_hex: head.c.to_hex(),
                            new_c_hex: None,
                            reason: HeadAdvanceReason::StaleCRejection,
                        });
                    }
                }
                SubmitResult::RateLimited { .. } => summary.protocol.rate_limited += 1,
                SubmitResult::NetworkError { .. } => summary.protocol.network_errors += 1,
            }
            if head_advanced.is_some() {
                break;
            }
        }

        summary.protocol.cycles_run += 1;
        log(&MiningEvent::CycleComplete {
            cycle: summary.protocol.cycles_run,
        });
    }

    summary
}

fn render_chain_head_error(err: &ChainHeadError) -> String {
    err.to_string()
}

// --- Small wrappers to pick a NonceSource at runtime --------------------

struct TicketGrindLocal {
    nonce: Hex32,
    hashes_attempted: u64,
    elapsed_ms: u128,
}

fn grind_ticket_with_source(
    c: &Hex32,
    pk: &Hex32,
    t_ticket: &num_bigint::BigUint,
    config: GrinderConfig,
    deterministic: bool,
) -> Option<TicketGrindLocal> {
    if deterministic {
        let mut src = CounterNonce::new(0);
        crate::grinder::grind_ticket(c, pk, t_ticket, &mut src, config, None).map(|o| {
            TicketGrindLocal {
                nonce: o.nonce,
                hashes_attempted: o.hashes_attempted,
                elapsed_ms: o.elapsed_ms,
            }
        })
    } else {
        let mut src = OsRngNonce;
        crate::grinder::grind_ticket(c, pk, t_ticket, &mut src, config, None).map(|o| {
            TicketGrindLocal {
                nonce: o.nonce,
                hashes_attempted: o.hashes_attempted,
                elapsed_ms: o.elapsed_ms,
            }
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn grind_share_with_source(
    c: &Hex32,
    pk: &Hex32,
    n: &Hex32,
    canon_hash: &Hex32,
    min_share_score: &num_bigint::BigUint,
    t_block: Option<&num_bigint::BigUint>,
    config: GrinderConfig,
    deterministic: bool,
) -> Option<crate::grinder::GrindShareOutcome> {
    if deterministic {
        let mut src = CounterNonce::new(0);
        grind_share(
            c,
            pk,
            n,
            canon_hash,
            min_share_score,
            t_block,
            &mut src,
            config,
            None,
        )
    } else {
        let mut src = OsRngNonce;
        grind_share(
            c,
            pk,
            n,
            canon_hash,
            min_share_score,
            t_block,
            &mut src,
            config,
            None,
        )
    }
}

fn grind_submit_pow_with_source(
    c: &Hex32,
    pk: &Hex32,
    canon_hash: &Hex32,
    t_submit: &num_bigint::BigUint,
    config: GrinderConfig,
    deterministic: bool,
) -> Option<crate::grinder::GrindSubmitOutcome> {
    if deterministic {
        let mut src = CounterNonce::new(0);
        grind_submission_pow(c, pk, canon_hash, t_submit, &mut src, config, None)
    } else {
        let mut src = OsRngNonce;
        grind_submission_pow(c, pk, canon_hash, t_submit, &mut src, config, None)
    }
}

// --- Stub chain head fetcher ---------------------------------------------

/// Fixed-head fetcher: returns a pre-built `ChainHead` for every call.
/// Useful in integration tests where the dispatcher isn't running.
pub struct FixedChainHead {
    pub head: ChainHead,
}

impl ChainHeadFetcher for FixedChainHead {
    fn fetch_head(&self) -> Result<ChainHead, ChainHeadError> {
        Ok(self.head.clone())
    }
}
