use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use boole_core::{parse_biguint_hex, Hex32};

use boole_miner::{
    family_v1_lenbound, run_mining_loop, AcceptingVerifier, AgentRuntimeReport,
    AnnounceTicketInputs, AnnounceTicketResult, ChainHead, DefaultPromptBuilder, FixedChainHead,
    GrinderConfig, MiningEvent, MiningLoopDeps, MiningLoopOptions, MiningLoopOutcome,
    MiningRunContext, MiningRunDriverMode, MiningRunTargetMode, MiningRunVerifierMode, MockDriver,
    MockResponse, PromptBuilder, ProtocolReport, RejectingVerifier, StructuralCanonicalizer,
    StubTargetEmitter, SubmitInputs, SubmitResult, Submitter, Target, Verifier, VerifyReason,
    VerifyResult,
};

fn pk32() -> Hex32 {
    Hex32::from_bytes([1u8; 32])
}

fn c32() -> Hex32 {
    Hex32::from_bytes([2u8; 32])
}

fn easy_head() -> ChainHead {
    ChainHead {
        c: c32(),
        // All four thresholds set to 2^256 - 1 so the grinders always
        // succeed on the first attempt.
        t_ticket: parse_biguint_hex(&"f".repeat(64)).unwrap(),
        t_share: parse_biguint_hex(&"f".repeat(64)).unwrap(),
        t_block: parse_biguint_hex(&"f".repeat(64)).unwrap(),
        t_submit: parse_biguint_hex(&"f".repeat(64)).unwrap(),
        // MinShareScore = 0 so any share score qualifies.
        min_share_score: num_bigint::BigUint::default(),
        m: 1,
        d: 1,
        profile: "v01".to_string(),
        n: None,
    }
}

#[derive(Default)]
struct RecordingSubmitter {
    pub announce_calls: Mutex<u32>,
    pub submit_calls: Mutex<Vec<String>>,
    pub submit_result: Mutex<Option<SubmitResult>>,
    pub announce_result: Mutex<Option<AnnounceTicketResult>>,
}

impl RecordingSubmitter {
    fn with_results(submit: SubmitResult, announce: AnnounceTicketResult) -> Self {
        Self {
            announce_calls: Mutex::new(0),
            submit_calls: Mutex::new(Vec::new()),
            submit_result: Mutex::new(Some(submit)),
            announce_result: Mutex::new(Some(announce)),
        }
    }
}

impl Submitter for RecordingSubmitter {
    fn announce_ticket(&self, _inputs: AnnounceTicketInputs<'_>) -> AnnounceTicketResult {
        *self.announce_calls.lock().unwrap() += 1;
        self.announce_result
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(AnnounceTicketResult::Observed {
                hash_hex: String::new(),
            })
    }

    fn submit(&self, inputs: SubmitInputs<'_>) -> SubmitResult {
        self.submit_calls.lock().unwrap().push(format!(
            "j={} bytes={}B",
            inputs.j_hex,
            inputs.canon_bytes.len()
        ));
        self.submit_result
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(SubmitResult::Accepted {
                share_hash_hex: String::new(),
            })
    }
}

fn make_canned_proof_driver() -> Box<MockDriver> {
    Box::new(MockDriver::new(vec![MockResponse::Text(
        "```lean\nfun xs => nodup_dedup _\n```".to_string(),
    )]))
}

#[test]
fn default_prompt_builder_uses_v1_lenbound_contract_for_v1_profile() {
    let target = Target {
        seed_hex: "00".repeat(32),
        d: 1,
        profile: "v1-lenbound".to_string(),
        n: 1,
        render: "the result length is ≤ input length".to_string(),
    };

    let prompt = DefaultPromptBuilder.build_prompt(&target);

    assert!(prompt.contains("Boole v1 length-bound"));
    assert!(prompt.contains("length_filterByPred_le"));
    assert!(prompt.contains("length_dedup_le"));
    assert!(prompt.contains("proof body only"));
    assert!(prompt.contains("`by` tactic blocks are allowed"));
    assert!(!prompt.contains("ListInvariantsV0 family"));
}

#[test]
fn default_prompt_builder_keeps_v0_contract_for_v031_profiles() {
    let target = Target {
        seed_hex: "11".repeat(32),
        d: 1,
        profile: "v031-lp".to_string(),
        n: 1,
        render: "synthetic invariant render".to_string(),
    };

    let prompt = DefaultPromptBuilder.build_prompt(&target);

    assert!(prompt.contains("ListInvariantsV0 family"));
    assert!(prompt.contains("fun xs => nodup_dedup _"));
}

#[test]
fn default_prompt_builder_embeds_exact_v1_helper_manifest() {
    let target = Target {
        seed_hex: "22".repeat(32),
        d: 3,
        profile: "v1-lenbound".to_string(),
        n: 2,
        render: "theorem instance_thm : ∀ (xs : List Int), (dedup xs).length ≤ xs.length"
            .to_string(),
    };

    let prompt = DefaultPromptBuilder.build_prompt(&target);
    let manifest = family_v1_lenbound::helper_manifest();

    assert!(prompt.contains(manifest));
    assert!(prompt.contains("Respond with one Lean proof body only"));
    assert!(prompt.contains("`by` tactic blocks are allowed"));
    assert!(!prompt.contains("Respond with a single fenced ```lean block"));
}

#[test]
fn mining_loop_outcome_splits_agent_runtime_from_protocol_report() {
    let agent = AgentRuntimeReport {
        llm_calls: 3,
        llm_solved: 2,
        llm_rejected: 1,
        llm_errored: 0,
    };
    let protocol = ProtocolReport {
        cycles_run: 1,
        tickets_found: 1,
        verify_accepted: 1,
        verify_rejected: 1,
        shares_accepted: 1,
        shares_rejected: 0,
        rate_limited: 0,
        network_errors: 0,
        announce_rejected: 0,
        proposer_shares: 1,
        loop_class: "smoke".to_string(),
        public_scoring_eligible: false,
        ineligibility_reasons: vec!["open_thresholds".to_string()],
    };
    let outcome = MiningLoopOutcome { agent, protocol };

    assert_eq!(outcome.agent.llm_calls, 3);
    assert_eq!(outcome.agent.llm_solved, 2);
    assert_eq!(outcome.protocol.verify_accepted, 1);
    assert_eq!(outcome.protocol.shares_accepted, 1);
    assert!(!outcome.protocol.public_scoring_eligible);
}

#[test]
fn run_mining_loop_returns_split_outcome_reports() {
    let head = easy_head();
    let submitter = Arc::new(RecordingSubmitter::with_results(
        SubmitResult::Accepted {
            share_hash_hex: "abc".to_string(),
        },
        AnnounceTicketResult::Observed {
            hash_hex: "def".to_string(),
        },
    ));

    struct ArcSubmitter(Arc<RecordingSubmitter>);
    impl Submitter for ArcSubmitter {
        fn announce_ticket(&self, inputs: AnnounceTicketInputs<'_>) -> AnnounceTicketResult {
            self.0.announce_ticket(inputs)
        }
        fn submit(&self, inputs: SubmitInputs<'_>) -> SubmitResult {
            self.0.submit(inputs)
        }
    }

    let deps = MiningLoopDeps {
        pk: pk32(),
        chain_head: Box::new(FixedChainHead { head }),
        emitter: Box::new(StubTargetEmitter::new("synthetic invariant render")),
        driver: make_canned_proof_driver(),
        verifier: Box::new(AcceptingVerifier),
        canonicalizer: Box::new(StructuralCanonicalizer),
        submit_client: Box::new(ArcSubmitter(Arc::clone(&submitter))),
        prompt_builder: None,
        log: None,
        sleeper: None,
    };
    let opts = MiningLoopOptions {
        max_cycles: Some(1),
        deterministic_nonces: true,
        ticket_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        share_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        submit_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        ..Default::default()
    };

    let outcome = run_mining_loop(deps, opts);

    assert_eq!(outcome.agent.llm_calls, 1);
    assert_eq!(outcome.agent.llm_solved, 1);
    assert_eq!(outcome.protocol.cycles_run, 1);
    assert_eq!(outcome.protocol.tickets_found, 1);
    assert_eq!(outcome.protocol.verify_accepted, 1);
    assert_eq!(outcome.protocol.shares_accepted, 1);
}

#[test]
fn test_one_cycle_one_share_pipeline_with_easy_thresholds() {
    let head = easy_head();
    let submitter = Arc::new(RecordingSubmitter::with_results(
        SubmitResult::Accepted {
            share_hash_hex: "abc".to_string(),
        },
        AnnounceTicketResult::Observed {
            hash_hex: "def".to_string(),
        },
    ));

    struct ArcSubmitter(Arc<RecordingSubmitter>);
    impl Submitter for ArcSubmitter {
        fn announce_ticket(&self, inputs: AnnounceTicketInputs<'_>) -> AnnounceTicketResult {
            self.0.announce_ticket(inputs)
        }
        fn submit(&self, inputs: SubmitInputs<'_>) -> SubmitResult {
            self.0.submit(inputs)
        }
    }

    let deps = MiningLoopDeps {
        pk: pk32(),
        chain_head: Box::new(FixedChainHead { head }),
        emitter: Box::new(StubTargetEmitter::new("synthetic invariant render")),
        driver: make_canned_proof_driver(),
        verifier: Box::new(AcceptingVerifier),
        canonicalizer: Box::new(StructuralCanonicalizer),
        submit_client: Box::new(ArcSubmitter(Arc::clone(&submitter))),
        prompt_builder: None,
        log: None,
        sleeper: None,
    };
    let opts = MiningLoopOptions {
        max_cycles: Some(1),
        deterministic_nonces: true,
        ticket_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        share_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        submit_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        ..Default::default()
    };
    let summary = run_mining_loop(deps, opts);
    assert_eq!(summary.protocol.cycles_run, 1);
    assert_eq!(summary.protocol.tickets_found, 1);
    assert_eq!(summary.agent.llm_calls, 1);
    assert_eq!(summary.agent.llm_solved, 1);
    assert_eq!(summary.protocol.verify_accepted, 1);
    assert_eq!(summary.protocol.shares_accepted, 1);
    assert_eq!(*submitter.announce_calls.lock().unwrap(), 1);
    assert_eq!(submitter.submit_calls.lock().unwrap().len(), 1);
}

#[test]
fn test_loop_skips_to_next_j_when_verify_rejects() {
    let mut head = easy_head();
    head.m = 3;
    let submitter = Arc::new(RecordingSubmitter::with_results(
        SubmitResult::Accepted {
            share_hash_hex: "abc".to_string(),
        },
        AnnounceTicketResult::Observed {
            hash_hex: "def".to_string(),
        },
    ));
    struct ArcSubmitter(Arc<RecordingSubmitter>);
    impl Submitter for ArcSubmitter {
        fn announce_ticket(&self, inputs: AnnounceTicketInputs<'_>) -> AnnounceTicketResult {
            self.0.announce_ticket(inputs)
        }
        fn submit(&self, inputs: SubmitInputs<'_>) -> SubmitResult {
            self.0.submit(inputs)
        }
    }
    let driver = Box::new(MockDriver::new(vec![
        MockResponse::Text("```lean\nfun xs => x\n```".to_string()),
        MockResponse::Text("```lean\nfun xs => y\n```".to_string()),
        MockResponse::Text("```lean\nfun xs => z\n```".to_string()),
    ]));
    let deps = MiningLoopDeps {
        pk: pk32(),
        chain_head: Box::new(FixedChainHead { head }),
        emitter: Box::new(StubTargetEmitter::new("render")),
        driver,
        verifier: Box::new(RejectingVerifier::new(VerifyReason::ElaborateFailed)),
        canonicalizer: Box::new(StructuralCanonicalizer),
        submit_client: Box::new(ArcSubmitter(Arc::clone(&submitter))),
        prompt_builder: None,
        log: None,
        sleeper: None,
    };
    let opts = MiningLoopOptions {
        max_cycles: Some(1),
        deterministic_nonces: true,
        ticket_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        share_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        submit_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        ..Default::default()
    };
    let summary = run_mining_loop(deps, opts);
    assert_eq!(summary.protocol.cycles_run, 1);
    assert_eq!(summary.agent.llm_calls, 3);
    assert_eq!(summary.agent.llm_solved, 3);
    assert_eq!(summary.protocol.verify_accepted, 0);
    assert_eq!(summary.protocol.verify_rejected, 3);
    assert_eq!(summary.protocol.shares_accepted, 0);
    assert_eq!(submitter.submit_calls.lock().unwrap().len(), 0);
}

#[test]
fn test_loop_counts_llm_rejected_when_response_has_no_proof_block() {
    let head = easy_head();
    let submitter = Arc::new(RecordingSubmitter::with_results(
        SubmitResult::Accepted {
            share_hash_hex: "abc".to_string(),
        },
        AnnounceTicketResult::Observed {
            hash_hex: "def".to_string(),
        },
    ));
    struct ArcSubmitter(Arc<RecordingSubmitter>);
    impl Submitter for ArcSubmitter {
        fn announce_ticket(&self, inputs: AnnounceTicketInputs<'_>) -> AnnounceTicketResult {
            self.0.announce_ticket(inputs)
        }
        fn submit(&self, inputs: SubmitInputs<'_>) -> SubmitResult {
            self.0.submit(inputs)
        }
    }
    let driver = Box::new(MockDriver::new(vec![MockResponse::Text(String::new())]));
    let deps = MiningLoopDeps {
        pk: pk32(),
        chain_head: Box::new(FixedChainHead { head }),
        emitter: Box::new(StubTargetEmitter::new("render")),
        driver,
        verifier: Box::new(AcceptingVerifier),
        canonicalizer: Box::new(StructuralCanonicalizer),
        submit_client: Box::new(ArcSubmitter(Arc::clone(&submitter))),
        prompt_builder: None,
        log: None,
        sleeper: None,
    };
    let opts = MiningLoopOptions {
        max_cycles: Some(1),
        deterministic_nonces: true,
        ticket_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        share_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        submit_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        ..Default::default()
    };
    let summary = run_mining_loop(deps, opts);
    assert_eq!(summary.agent.llm_rejected, 1);
    assert_eq!(summary.agent.llm_solved, 0);
    assert_eq!(summary.protocol.shares_accepted, 0);
    assert_eq!(*submitter.announce_calls.lock().unwrap(), 1);
    assert_eq!(submitter.submit_calls.lock().unwrap().len(), 0);
}

#[test]
fn test_loop_records_cycle_complete_and_head_fetched_events() {
    let events: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let events_clone = Arc::clone(&events);
    let log_fn: Box<dyn Fn(&MiningEvent) + Send + Sync> = Box::new(move |e: &MiningEvent| {
        let kind = match e {
            MiningEvent::HeadFetched { .. } => "head_fetched",
            MiningEvent::LoopClassified { .. } => "loop_classified",
            MiningEvent::TicketFound { .. } => "ticket_found",
            MiningEvent::TicketAnnounced { .. } => "ticket_announced",
            MiningEvent::TicketExhausted { .. } => "ticket_exhausted",
            MiningEvent::TargetEmitted { .. } => "target_emitted",
            MiningEvent::LlmOutcome { .. } => "llm_outcome",
            MiningEvent::VerifyOutcome { .. } => "verify_outcome",
            MiningEvent::ShareFound { .. } => "share_found",
            MiningEvent::ShareGrindExhausted { .. } => "share_grind_exhausted",
            MiningEvent::SubmitPowFound { .. } => "submit_pow_found",
            MiningEvent::SubmitPowExhausted { .. } => "submit_pow_exhausted",
            MiningEvent::SubmitOutcome { .. } => "submit_outcome",
            MiningEvent::HeadAdvancedMidCycle { .. } => "head_advanced_mid_cycle",
            MiningEvent::CycleComplete { .. } => "cycle_complete",
            MiningEvent::HeadFetchFailed { .. } => "head_fetch_failed",
        };
        events_clone.lock().unwrap().push(kind.to_string());
    });
    let head = easy_head();
    let submitter = RecordingSubmitter::with_results(
        SubmitResult::Accepted {
            share_hash_hex: "abc".to_string(),
        },
        AnnounceTicketResult::Observed {
            hash_hex: "def".to_string(),
        },
    );
    let deps = MiningLoopDeps {
        pk: pk32(),
        chain_head: Box::new(FixedChainHead { head }),
        emitter: Box::new(StubTargetEmitter::new("render")),
        driver: make_canned_proof_driver(),
        verifier: Box::new(AcceptingVerifier),
        canonicalizer: Box::new(StructuralCanonicalizer),
        submit_client: Box::new(submitter),
        prompt_builder: None,
        log: Some(log_fn),
        sleeper: None,
    };
    let opts = MiningLoopOptions {
        max_cycles: Some(1),
        deterministic_nonces: true,
        ticket_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        share_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        submit_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        ..Default::default()
    };
    let _ = run_mining_loop(deps, opts);
    let events = events.lock().unwrap();
    assert!(events.contains(&"head_fetched".to_string()));
    assert!(events.contains(&"ticket_found".to_string()));
    assert!(events.contains(&"ticket_announced".to_string()));
    assert!(events.contains(&"target_emitted".to_string()));
    assert!(events.contains(&"llm_outcome".to_string()));
    assert!(events.contains(&"verify_outcome".to_string()));
    assert!(events.contains(&"share_found".to_string()));
    assert!(events.contains(&"submit_pow_found".to_string()));
    assert!(events.contains(&"submit_outcome".to_string()));
    assert!(events.contains(&"cycle_complete".to_string()));
}

#[test]
fn test_open_mock_fixed_loop_is_labeled_smoke_and_not_public_scoring_eligible() {
    let head = easy_head();
    let submitter = RecordingSubmitter::with_results(
        SubmitResult::Accepted {
            share_hash_hex: "abc".to_string(),
        },
        AnnounceTicketResult::Observed {
            hash_hex: "def".to_string(),
        },
    );
    let events: Arc<Mutex<Vec<MiningEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let events_clone = Arc::clone(&events);
    let log_fn: Box<dyn Fn(&MiningEvent) + Send + Sync> =
        Box::new(move |e: &MiningEvent| events_clone.lock().unwrap().push(e.clone()));
    let deps = MiningLoopDeps {
        pk: pk32(),
        chain_head: Box::new(FixedChainHead { head }),
        emitter: Box::new(StubTargetEmitter::new("render")),
        driver: make_canned_proof_driver(),
        verifier: Box::new(AcceptingVerifier),
        canonicalizer: Box::new(StructuralCanonicalizer),
        submit_client: Box::new(submitter),
        prompt_builder: None,
        log: Some(log_fn),
        sleeper: None,
    };
    let opts = MiningLoopOptions {
        max_cycles: Some(1),
        deterministic_nonces: true,
        run_context: MiningRunContext {
            verifier_mode: MiningRunVerifierMode::MockAccept,
            driver_mode: MiningRunDriverMode::MockLlmResponse,
            target_mode: MiningRunTargetMode::FixedSeed,
        },
        ticket_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        share_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        submit_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        ..Default::default()
    };

    let summary = run_mining_loop(deps, opts);

    assert_eq!(summary.protocol.loop_class, "smoke");
    assert!(!summary.protocol.public_scoring_eligible);
    assert!(summary
        .protocol
        .ineligibility_reasons
        .contains(&"mock_verifier".to_string()));
    assert!(summary
        .protocol
        .ineligibility_reasons
        .contains(&"mock_llm".to_string()));
    assert!(summary
        .protocol
        .ineligibility_reasons
        .contains(&"fixed_target_seed".to_string()));
    assert!(summary
        .protocol
        .ineligibility_reasons
        .contains(&"open_thresholds".to_string()));
    assert!(events.lock().unwrap().iter().any(|event| matches!(
        event,
        MiningEvent::LoopClassified {
            loop_class,
            public_scoring_eligible: false,
            ineligibility_reasons,
        } if loop_class == "smoke"
            && ineligibility_reasons.contains(&"open_thresholds".to_string())
    )));
}

#[test]
fn test_loop_stops_at_max_shares() {
    let mut head = easy_head();
    head.m = 5;
    let submitter = Arc::new(RecordingSubmitter::with_results(
        SubmitResult::Accepted {
            share_hash_hex: "abc".to_string(),
        },
        AnnounceTicketResult::Observed {
            hash_hex: "def".to_string(),
        },
    ));
    struct ArcSubmitter(Arc<RecordingSubmitter>);
    impl Submitter for ArcSubmitter {
        fn announce_ticket(&self, inputs: AnnounceTicketInputs<'_>) -> AnnounceTicketResult {
            self.0.announce_ticket(inputs)
        }
        fn submit(&self, inputs: SubmitInputs<'_>) -> SubmitResult {
            self.0.submit(inputs)
        }
    }
    let driver = Box::new(MockDriver::new(vec![
        MockResponse::Text("```lean\nfun xs => x\n```".to_string()),
        MockResponse::Text("```lean\nfun xs => y\n```".to_string()),
        MockResponse::Text("```lean\nfun xs => z\n```".to_string()),
        MockResponse::Text("```lean\nfun xs => w\n```".to_string()),
        MockResponse::Text("```lean\nfun xs => v\n```".to_string()),
    ]));
    let deps = MiningLoopDeps {
        pk: pk32(),
        chain_head: Box::new(FixedChainHead { head }),
        emitter: Box::new(StubTargetEmitter::new("render")),
        driver,
        verifier: Box::new(AcceptingVerifier),
        canonicalizer: Box::new(StructuralCanonicalizer),
        submit_client: Box::new(ArcSubmitter(Arc::clone(&submitter))),
        prompt_builder: None,
        log: None,
        sleeper: None,
    };
    let opts = MiningLoopOptions {
        max_cycles: Some(10),
        max_shares: Some(2),
        deterministic_nonces: true,
        ticket_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        share_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        submit_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        ..Default::default()
    };
    let summary = run_mining_loop(deps, opts);
    assert_eq!(summary.protocol.shares_accepted, 2);
    assert!(summary.protocol.cycles_run >= 1);
}

#[test]
fn test_loop_aborts_when_announce_rejected() {
    let head = easy_head();
    let submitter = Arc::new(RecordingSubmitter::with_results(
        SubmitResult::Accepted {
            share_hash_hex: "abc".to_string(),
        },
        AnnounceTicketResult::Rejected {
            status: 400,
            error: "bad".to_string(),
            reason: None,
        },
    ));
    struct ArcSubmitter(Arc<RecordingSubmitter>);
    impl Submitter for ArcSubmitter {
        fn announce_ticket(&self, inputs: AnnounceTicketInputs<'_>) -> AnnounceTicketResult {
            self.0.announce_ticket(inputs)
        }
        fn submit(&self, inputs: SubmitInputs<'_>) -> SubmitResult {
            self.0.submit(inputs)
        }
    }
    let deps = MiningLoopDeps {
        pk: pk32(),
        chain_head: Box::new(FixedChainHead { head }),
        emitter: Box::new(StubTargetEmitter::new("render")),
        driver: make_canned_proof_driver(),
        verifier: Box::new(AcceptingVerifier),
        canonicalizer: Box::new(StructuralCanonicalizer),
        submit_client: Box::new(ArcSubmitter(Arc::clone(&submitter))),
        prompt_builder: None,
        log: None,
        sleeper: None,
    };
    let opts = MiningLoopOptions {
        max_cycles: Some(1),
        deterministic_nonces: true,
        ticket_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        share_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        submit_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        ..Default::default()
    };
    let summary = run_mining_loop(deps, opts);
    assert_eq!(summary.protocol.announce_rejected, 1);
    assert_eq!(summary.protocol.network_errors, 0);
    assert_eq!(summary.protocol.shares_accepted, 0);
    assert_eq!(submitter.submit_calls.lock().unwrap().len(), 0);
}

// Returns one ChainHead the first N fetches and a different one (advanced
// `c`) afterwards. Lets the test simulate the dispatcher promoting an
// accepted share to a block mid-cycle, which advances the chain head.
struct AdvanceAfterChainHead {
    pre_advance: ChainHead,
    post_advance: ChainHead,
    advance_after: u32,
    fetches: Mutex<u32>,
}

impl boole_miner::ChainHeadFetcher for AdvanceAfterChainHead {
    fn fetch_head(&self) -> Result<ChainHead, boole_miner::ChainHeadError> {
        let mut n = self.fetches.lock().unwrap();
        *n += 1;
        let count = *n;
        Ok(if count <= self.advance_after {
            self.pre_advance.clone()
        } else {
            self.post_advance.clone()
        })
    }
}

#[test]
fn test_loop_breaks_inner_when_head_advances_after_accept() {
    // M=4 cycle, all submits Accepted. After the *first* fetch (cycle start)
    // the head advances. The loop should observe this on the post-submit
    // re-fetch, log HeadAdvancedMidCycle, and break out of the j loop —
    // i.e. exactly one LLM call instead of four.
    let mut head_a = easy_head();
    head_a.m = 4;
    let mut head_b = head_a.clone();
    head_b.c = Hex32::from_bytes([7u8; 32]);

    let submitter = Arc::new(RecordingSubmitter::with_results(
        SubmitResult::Accepted {
            share_hash_hex: "abc".to_string(),
        },
        AnnounceTicketResult::Observed {
            hash_hex: "def".to_string(),
        },
    ));
    struct ArcSubmitter(Arc<RecordingSubmitter>);
    impl Submitter for ArcSubmitter {
        fn announce_ticket(&self, inputs: AnnounceTicketInputs<'_>) -> AnnounceTicketResult {
            self.0.announce_ticket(inputs)
        }
        fn submit(&self, inputs: SubmitInputs<'_>) -> SubmitResult {
            self.0.submit(inputs)
        }
    }

    let events: Arc<Mutex<Vec<MiningEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let events_clone = Arc::clone(&events);
    let log_fn: Box<dyn Fn(&MiningEvent) + Send + Sync> =
        Box::new(move |e: &MiningEvent| events_clone.lock().unwrap().push(e.clone()));

    let deps = MiningLoopDeps {
        pk: pk32(),
        chain_head: Box::new(AdvanceAfterChainHead {
            pre_advance: head_a,
            post_advance: head_b,
            advance_after: 1,
            fetches: Mutex::new(0),
        }),
        emitter: Box::new(StubTargetEmitter::new("render")),
        driver: make_canned_proof_driver(),
        verifier: Box::new(AcceptingVerifier),
        canonicalizer: Box::new(StructuralCanonicalizer),
        submit_client: Box::new(ArcSubmitter(Arc::clone(&submitter))),
        prompt_builder: None,
        log: Some(log_fn),
        sleeper: None,
    };
    let opts = MiningLoopOptions {
        max_cycles: Some(1),
        deterministic_nonces: true,
        ticket_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        share_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        submit_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        ..Default::default()
    };
    let summary = run_mining_loop(deps, opts);

    assert_eq!(
        summary.protocol.shares_accepted, 1,
        "exactly one share accepted"
    );
    assert_eq!(
        summary.agent.llm_calls, 1,
        "inner j loop broke after first accept"
    );
    assert_eq!(submitter.submit_calls.lock().unwrap().len(), 1);
    let evs = events.lock().unwrap();
    let advances: Vec<_> = evs
        .iter()
        .filter_map(|e| match e {
            MiningEvent::HeadAdvancedMidCycle {
                new_c_hex, reason, ..
            } => Some((new_c_hex.clone(), *reason)),
            _ => None,
        })
        .collect();
    assert_eq!(
        advances.len(),
        1,
        "exactly one HeadAdvancedMidCycle event was emitted"
    );
    let (new_c, reason) = &advances[0];
    assert_eq!(*reason, boole_miner::HeadAdvanceReason::SubmitAccepted);
    assert_eq!(
        new_c.as_deref(),
        Some("0707070707070707070707070707070707070707070707070707070707070707"),
        "SubmitAccepted re-fetch surfaces the new c"
    );
}

#[test]
fn test_loop_breaks_inner_when_submit_returns_stale_c() {
    // Even without a live re-fetch, an explicit StaleC rejection should
    // trip the inner break (and emit HeadAdvancedMidCycle{StaleCRejection}).
    let mut head = easy_head();
    head.m = 4;

    let submitter = Arc::new(RecordingSubmitter::with_results(
        SubmitResult::Rejected {
            status: 422,
            error: "not_accepted".to_string(),
            reason: Some(
                "Rejected { status: UnprocessableEntity, error: SharePool { reason: StaleC }, \
                 rejection: SharePool { detail: StaleC } }"
                    .to_string(),
            ),
            field: None,
            detail: None,
        },
        AnnounceTicketResult::Observed {
            hash_hex: "def".to_string(),
        },
    ));
    struct ArcSubmitter(Arc<RecordingSubmitter>);
    impl Submitter for ArcSubmitter {
        fn announce_ticket(&self, inputs: AnnounceTicketInputs<'_>) -> AnnounceTicketResult {
            self.0.announce_ticket(inputs)
        }
        fn submit(&self, inputs: SubmitInputs<'_>) -> SubmitResult {
            self.0.submit(inputs)
        }
    }

    let events: Arc<Mutex<Vec<MiningEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let events_clone = Arc::clone(&events);
    let log_fn: Box<dyn Fn(&MiningEvent) + Send + Sync> =
        Box::new(move |e: &MiningEvent| events_clone.lock().unwrap().push(e.clone()));

    let deps = MiningLoopDeps {
        pk: pk32(),
        chain_head: Box::new(FixedChainHead { head }),
        emitter: Box::new(StubTargetEmitter::new("render")),
        driver: make_canned_proof_driver(),
        verifier: Box::new(AcceptingVerifier),
        canonicalizer: Box::new(StructuralCanonicalizer),
        submit_client: Box::new(ArcSubmitter(Arc::clone(&submitter))),
        prompt_builder: None,
        log: Some(log_fn),
        sleeper: None,
    };
    let opts = MiningLoopOptions {
        max_cycles: Some(1),
        deterministic_nonces: true,
        ticket_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        share_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        submit_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        ..Default::default()
    };
    let summary = run_mining_loop(deps, opts);

    assert_eq!(summary.protocol.shares_rejected, 1);
    assert_eq!(
        summary.agent.llm_calls, 1,
        "broke after first stale_c rejection"
    );
    let evs = events.lock().unwrap();
    let advances: Vec<_> = evs
        .iter()
        .filter_map(|e| match e {
            MiningEvent::HeadAdvancedMidCycle {
                new_c_hex, reason, ..
            } => Some((new_c_hex.clone(), *reason)),
            _ => None,
        })
        .collect();
    assert_eq!(advances.len(), 1);
    let (new_c, reason) = &advances[0];
    assert_eq!(*reason, boole_miner::HeadAdvanceReason::StaleCRejection);
    assert!(
        new_c.is_none(),
        "StaleCRejection has no fresh c to surface, got {new_c:?}"
    );
}

struct ArtifactRejectingVerifier {
    artifact_path: PathBuf,
}

impl Verifier for ArtifactRejectingVerifier {
    fn verify(
        &self,
        _seed_hex: &str,
        _d: u32,
        _proof_source: &str,
        _n: Option<u32>,
    ) -> VerifyResult {
        VerifyResult {
            accepted: false,
            reason: VerifyReason::ElaborateFailed,
            elapsed: Duration::from_millis(7),
            stderr_tail: "synthetic lean error".to_string(),
            attempt_artifact_path: Some(self.artifact_path.clone()),
        }
    }
}

#[test]
fn test_verify_outcome_threads_attempt_artifact_path() {
    let artifact_path = PathBuf::from("/tmp/boole-test-attempt-artifact");
    let events: Arc<Mutex<Vec<MiningEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let events_clone = Arc::clone(&events);
    let log_fn: Box<dyn Fn(&MiningEvent) + Send + Sync> =
        Box::new(move |e: &MiningEvent| events_clone.lock().unwrap().push(e.clone()));
    let submitter = RecordingSubmitter::with_results(
        SubmitResult::Accepted {
            share_hash_hex: "abc".to_string(),
        },
        AnnounceTicketResult::Observed {
            hash_hex: "def".to_string(),
        },
    );

    let deps = MiningLoopDeps {
        pk: pk32(),
        chain_head: Box::new(FixedChainHead { head: easy_head() }),
        emitter: Box::new(StubTargetEmitter::new("render")),
        driver: make_canned_proof_driver(),
        verifier: Box::new(ArtifactRejectingVerifier {
            artifact_path: artifact_path.clone(),
        }),
        canonicalizer: Box::new(StructuralCanonicalizer),
        submit_client: Box::new(submitter),
        prompt_builder: None,
        log: Some(log_fn),
        sleeper: None,
    };
    let opts = MiningLoopOptions {
        max_cycles: Some(1),
        deterministic_nonces: true,
        ticket_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        share_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        submit_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        ..Default::default()
    };

    let summary = run_mining_loop(deps, opts);

    assert_eq!(summary.protocol.verify_rejected, 1);
    let evs = events.lock().unwrap();
    assert!(evs.iter().any(|event| matches!(
        event,
        MiningEvent::VerifyOutcome {
            accepted: false,
            attempt_artifact_path: Some(path),
            ..
        } if path == &artifact_path
    )));
}
