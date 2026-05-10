use std::sync::{Arc, Mutex};
use std::time::Duration;

use boole_miner::{
    create_driver, extract_proof_source, with_retry, AgentCliDriver, ClaudeCliDriver,
    DriverConfigError, GenerateResult, LLMBackend, LLMDriverConfig, MockDriver, MockResponse,
    ProcessError, ProcessRunner, ProverDriver, RejectionReason, RetryConfig, Sleeper,
    StdProcessRunner, Strategy,
};

// --- extract_proof_source -------------------------------------------------

#[test]
fn test_extract_proof_source_strips_lean_fence() {
    let raw = "```lean\nexample : True := trivial\n```";
    assert_eq!(
        extract_proof_source(raw).unwrap(),
        "example : True := trivial"
    );
}

#[test]
fn test_extract_proof_source_strips_lean4_fence() {
    let raw = "Here you go:\n```lean4\nby trivial\n```\n";
    assert_eq!(extract_proof_source(raw).unwrap(), "by trivial");
}

#[test]
fn test_extract_proof_source_strips_unlabeled_fence() {
    let raw = "```\nexact rfl\n```";
    assert_eq!(extract_proof_source(raw).unwrap(), "exact rfl");
}

#[test]
fn test_extract_proof_source_returns_raw_text_when_no_fence() {
    let raw = "by trivial";
    assert_eq!(extract_proof_source(raw).unwrap(), "by trivial");
}

#[test]
fn test_extract_proof_source_empty_response_rejected() {
    assert_eq!(
        extract_proof_source(""),
        Err(RejectionReason::EmptyResponse)
    );
    assert_eq!(
        extract_proof_source("   \n\t  \n"),
        Err(RejectionReason::EmptyResponse)
    );
}

#[test]
fn test_extract_proof_source_empty_fence_yields_no_proof_block() {
    assert_eq!(
        extract_proof_source("```lean\n   \n```"),
        Err(RejectionReason::NoProofBlock)
    );
}

// --- MockDriver -----------------------------------------------------------

#[test]
fn test_mock_driver_returns_solved_for_text_response() {
    let driver = MockDriver::new(vec![MockResponse::Text(
        "```lean\nby trivial\n```".to_string(),
    )]);
    let r = driver.generate("prompt");
    match r {
        GenerateResult::Solved { proof_source, .. } => assert_eq!(proof_source, "by trivial"),
        other => panic!("expected Solved, got {other:?}"),
    }
}

#[test]
fn test_mock_driver_returns_error_for_error_response() {
    let driver = MockDriver::new(vec![MockResponse::Error("network down".into())]);
    let r = driver.generate("prompt");
    assert!(matches!(r, GenerateResult::Error { cause, .. } if cause == "network down"));
}

#[test]
fn test_mock_driver_advances_through_responses() {
    let driver = MockDriver::new(vec![
        MockResponse::Error("first".into()),
        MockResponse::Text("```lean\nby rfl\n```".into()),
    ]);
    let r1 = driver.generate("p");
    assert!(matches!(r1, GenerateResult::Error { .. }));
    let r2 = driver.generate("p");
    assert!(matches!(r2, GenerateResult::Solved { .. }));
}

#[test]
fn test_mock_driver_returns_error_when_exhausted() {
    let driver = MockDriver::new(vec![]);
    let r = driver.generate("p");
    match r {
        GenerateResult::Error { cause, .. } => assert!(cause.contains("exhausted")),
        other => panic!("expected Error, got {other:?}"),
    }
}

// --- ProcessRunner mock ---------------------------------------------------

#[derive(Default, Debug)]
struct Capture {
    binary: String,
    args: Vec<String>,
    stdin: Option<Vec<u8>>,
    timeout: Duration,
}

#[derive(Clone)]
struct FakeRunner {
    inner: Arc<Mutex<FakeRunnerInner>>,
}

struct FakeRunnerInner {
    captures: Vec<Capture>,
    next: Vec<Result<Vec<u8>, ProcessError>>,
}

impl FakeRunner {
    fn new(responses: Vec<Result<Vec<u8>, ProcessError>>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(FakeRunnerInner {
                captures: Vec::new(),
                next: responses,
            })),
        }
    }

    fn captured(&self) -> Vec<Capture> {
        let inner = self.inner.lock().unwrap();
        inner
            .captures
            .iter()
            .map(|c| Capture {
                binary: c.binary.clone(),
                args: c.args.clone(),
                stdin: c.stdin.clone(),
                timeout: c.timeout,
            })
            .collect()
    }
}

impl ProcessRunner for FakeRunner {
    fn run(
        &self,
        binary: &str,
        args: &[String],
        stdin_input: Option<&[u8]>,
        timeout: Duration,
    ) -> Result<Vec<u8>, ProcessError> {
        let mut inner = self.inner.lock().unwrap();
        inner.captures.push(Capture {
            binary: binary.to_string(),
            args: args.to_vec(),
            stdin: stdin_input.map(|b| b.to_vec()),
            timeout,
        });
        if inner.next.is_empty() {
            return Err(ProcessError::Io("FakeRunner exhausted".to_string()));
        }
        inner.next.remove(0)
    }
}

// --- ClaudeCliDriver ------------------------------------------------------

#[test]
fn test_claude_cli_driver_pipes_prompt_via_stdin_with_dash_p() {
    let runner = FakeRunner::new(vec![Ok(b"```lean\nby trivial\n```".to_vec())]);
    let driver = ClaudeCliDriver::with_runner(
        "claude",
        Duration::from_secs(60),
        Box::new(runner.clone()),
    );
    let r = driver.generate("PROMPT");
    assert!(matches!(r, GenerateResult::Solved { .. }));
    assert_eq!(driver.name(), "claude_cli");
    assert_eq!(driver.strategy(), Strategy::Frontier);
    let cap = runner.captured();
    assert_eq!(cap.len(), 1);
    assert_eq!(cap[0].binary, "claude");
    assert_eq!(cap[0].args, vec!["-p".to_string()]);
    assert_eq!(cap[0].stdin.as_deref(), Some(b"PROMPT".as_slice()));
}

#[test]
fn test_claude_cli_driver_returns_error_when_runner_fails() {
    let runner = FakeRunner::new(vec![Err(ProcessError::NotFound("claude".to_string()))]);
    let driver =
        ClaudeCliDriver::with_runner("claude", Duration::from_secs(5), Box::new(runner));
    match driver.generate("p") {
        GenerateResult::Error { cause, .. } => assert!(cause.contains("not found"), "got {cause}"),
        other => panic!("expected Error, got {other:?}"),
    }
}

// --- AgentCliDriver -------------------------------------------------------

#[test]
fn test_agent_cli_driver_appends_prompt_as_final_argv_no_stdin() {
    let runner = FakeRunner::new(vec![Ok(b"by trivial".to_vec())]);
    let driver = AgentCliDriver::with_runner(
        "hermes",
        vec!["--solo".to_string(), "--mode=lean".to_string()],
        Duration::from_secs(60),
        Box::new(runner.clone()),
    );
    let r = driver.generate("PROMPT");
    assert!(matches!(r, GenerateResult::Solved { .. }));
    assert_eq!(driver.strategy(), Strategy::Hybrid);
    let cap = runner.captured();
    assert_eq!(cap.len(), 1);
    assert_eq!(cap[0].binary, "hermes");
    assert_eq!(
        cap[0].args,
        vec![
            "--solo".to_string(),
            "--mode=lean".to_string(),
            "PROMPT".to_string(),
        ]
    );
    assert!(cap[0].stdin.is_none());
}

#[test]
fn test_agent_cli_driver_classifies_empty_stdout_as_rejected() {
    let runner = FakeRunner::new(vec![Ok(b"".to_vec())]);
    let driver = AgentCliDriver::with_runner(
        "hermes",
        vec![],
        Duration::from_secs(5),
        Box::new(runner),
    );
    match driver.generate("p") {
        GenerateResult::Rejected { reason, .. } => {
            assert_eq!(reason, RejectionReason::EmptyResponse);
        }
        other => panic!("expected Rejected, got {other:?}"),
    }
}

// --- create_driver --------------------------------------------------------

#[test]
fn test_create_driver_mock_backend_is_not_constructible() {
    let cfg = LLMDriverConfig {
        backend: LLMBackend::Mock,
        timeout: Duration::from_secs(10),
        claude_binary: None,
        agent_command: None,
        agent_args: vec![],
    };
    match create_driver(&cfg) {
        Err(DriverConfigError::MockNotConstructible) => (),
        Err(other) => panic!("expected MockNotConstructible, got {other:?}"),
        Ok(_) => panic!("expected error"),
    }
}

#[test]
fn test_create_driver_claude_cli_uses_default_binary() {
    let cfg = LLMDriverConfig {
        backend: LLMBackend::ClaudeCli,
        timeout: Duration::from_secs(120),
        claude_binary: None,
        agent_command: None,
        agent_args: vec![],
    };
    let d = create_driver(&cfg).unwrap();
    assert_eq!(d.name(), "claude_cli");
}

#[test]
fn test_create_driver_agent_cli_requires_command() {
    let cfg = LLMDriverConfig {
        backend: LLMBackend::AgentCli,
        timeout: Duration::from_secs(60),
        claude_binary: None,
        agent_command: None,
        agent_args: vec![],
    };
    match create_driver(&cfg) {
        Err(DriverConfigError::AgentCommandMissing) => (),
        Err(other) => panic!("expected AgentCommandMissing, got {other:?}"),
        Ok(_) => panic!("expected error"),
    }
}

#[test]
fn test_create_driver_agent_cli_with_command_succeeds() {
    let cfg = LLMDriverConfig {
        backend: LLMBackend::AgentCli,
        timeout: Duration::from_secs(60),
        claude_binary: None,
        agent_command: Some("hermes".to_string()),
        agent_args: vec!["--mode=lean".to_string()],
    };
    let d = create_driver(&cfg).unwrap();
    assert_eq!(d.name(), "agent_cli");
}

#[test]
fn test_llm_backend_parse_round_trips() {
    for s in ["mock", "claude_cli", "agent_cli"] {
        let b = LLMBackend::parse(s).unwrap();
        assert_eq!(b.as_str(), s);
    }
    assert_eq!(LLMBackend::parse("unknown"), None);
}

// --- with_retry ----------------------------------------------------------

struct RecordingSleeper {
    durations: Mutex<Vec<Duration>>,
}

impl RecordingSleeper {
    fn new() -> Self {
        Self {
            durations: Mutex::new(Vec::new()),
        }
    }

    fn durations(&self) -> Vec<Duration> {
        self.durations.lock().unwrap().clone()
    }
}

impl Sleeper for RecordingSleeper {
    fn sleep(&self, duration: Duration) {
        self.durations.lock().unwrap().push(duration);
    }
}

#[test]
fn test_with_retry_returns_first_success_without_sleeping() {
    let driver = MockDriver::new(vec![MockResponse::Text("by rfl".to_string())]);
    let sleeper = RecordingSleeper::new();
    let r = with_retry(
        &driver,
        "p",
        &RetryConfig {
            max_attempts: 3,
            initial_backoff: Duration::from_secs(1),
        },
        &sleeper,
    );
    assert!(matches!(r, GenerateResult::Solved { .. }));
    assert!(sleeper.durations().is_empty());
}

#[test]
fn test_with_retry_retries_on_error_with_exponential_backoff() {
    let driver = MockDriver::new(vec![
        MockResponse::Error("net1".into()),
        MockResponse::Error("net2".into()),
        MockResponse::Text("```lean\nby rfl\n```".into()),
    ]);
    let sleeper = RecordingSleeper::new();
    let r = with_retry(
        &driver,
        "p",
        &RetryConfig {
            max_attempts: 3,
            initial_backoff: Duration::from_secs(1),
        },
        &sleeper,
    );
    match r {
        GenerateResult::Solved { proof_source, .. } => assert_eq!(proof_source, "by rfl"),
        other => panic!("expected Solved, got {other:?}"),
    }
    assert_eq!(
        sleeper.durations(),
        vec![Duration::from_secs(1), Duration::from_secs(2)]
    );
}

#[test]
fn test_with_retry_does_not_retry_on_rejected() {
    let driver = MockDriver::new(vec![MockResponse::Text("".into())]);
    let sleeper = RecordingSleeper::new();
    let r = with_retry(
        &driver,
        "p",
        &RetryConfig {
            max_attempts: 5,
            initial_backoff: Duration::from_millis(10),
        },
        &sleeper,
    );
    assert!(matches!(r, GenerateResult::Rejected { .. }));
    assert!(sleeper.durations().is_empty());
}

// --- StdProcessRunner real-process smoke ---------------------------------

#[test]
fn test_std_process_runner_executes_real_binary_and_returns_stdout() {
    // Use /bin/echo because PATH lookup of "echo" varies; /bin/echo is
    // present on every macOS / Linux dev box.
    let runner = StdProcessRunner;
    let out = runner
        .run(
            "/bin/echo",
            &["hello".to_string(), "world".to_string()],
            None,
            Duration::from_secs(5),
        )
        .expect("/bin/echo must exist");
    assert_eq!(String::from_utf8_lossy(&out).trim(), "hello world");
}

#[test]
fn test_std_process_runner_reports_not_found_for_missing_binary() {
    let runner = StdProcessRunner;
    let err = runner
        .run(
            "/nonexistent/boole-miner-test-binary",
            &[],
            None,
            Duration::from_secs(2),
        )
        .unwrap_err();
    assert!(matches!(err, ProcessError::NotFound(_)), "got {err:?}");
}

#[test]
fn test_std_process_runner_kills_long_running_child_on_timeout() {
    let runner = StdProcessRunner;
    let err = runner
        .run("/bin/sleep", &["5".to_string()], None, Duration::from_millis(200))
        .unwrap_err();
    assert!(matches!(err, ProcessError::Timeout { .. }), "got {err:?}");
}

#[test]
fn test_with_retry_returns_last_error_when_all_attempts_fail() {
    let driver = MockDriver::new(vec![
        MockResponse::Error("a".into()),
        MockResponse::Error("b".into()),
        MockResponse::Error("c".into()),
    ]);
    let sleeper = RecordingSleeper::new();
    let r = with_retry(
        &driver,
        "p",
        &RetryConfig {
            max_attempts: 3,
            initial_backoff: Duration::from_millis(10),
        },
        &sleeper,
    );
    match r {
        GenerateResult::Error { cause, .. } => assert_eq!(cause, "c"),
        other => panic!("expected Error, got {other:?}"),
    }
    // 2 sleeps between 3 attempts.
    assert_eq!(sleeper.durations().len(), 2);
}
