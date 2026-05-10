use std::sync::{Arc, Mutex};

use boole_core::{parse_biguint_hex, Hex32};

use boole_miner::{
    run_mining_loop, AcceptingVerifier, AnnounceTicketInputs, AnnounceTicketResult, ChainHead,
    FixedChainHead, GrinderConfig, MiningEvent, MiningLoopDeps, MiningLoopOptions, MockDriver,
    MockResponse, RejectingVerifier, StructuralCanonicalizer, StubTargetEmitter, SubmitInputs,
    SubmitResult, Submitter, VerifyReason,
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
    assert_eq!(summary.cycles_run, 1);
    assert_eq!(summary.tickets_found, 1);
    assert_eq!(summary.llm_calls, 1);
    assert_eq!(summary.llm_solved, 1);
    assert_eq!(summary.verify_accepted, 1);
    assert_eq!(summary.shares_accepted, 1);
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
    assert_eq!(summary.cycles_run, 1);
    assert_eq!(summary.llm_calls, 3);
    assert_eq!(summary.llm_solved, 3);
    assert_eq!(summary.verify_accepted, 0);
    assert_eq!(summary.verify_rejected, 3);
    assert_eq!(summary.shares_accepted, 0);
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
    assert_eq!(summary.llm_rejected, 1);
    assert_eq!(summary.llm_solved, 0);
    assert_eq!(summary.shares_accepted, 0);
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
    assert_eq!(summary.shares_accepted, 2);
    assert!(summary.cycles_run >= 1);
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
    assert_eq!(summary.network_errors, 1);
    assert_eq!(summary.shares_accepted, 0);
    assert_eq!(submitter.submit_calls.lock().unwrap().len(), 0);
}
