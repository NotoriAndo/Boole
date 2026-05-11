use std::sync::{Arc, Mutex};
use std::time::Duration;

use boole_miner::{
    create_driver, extract_proof_source, with_retry, AgentCliDriver, AnthropicDriver,
    ClaudeCliDriver, DriverConfigError, GenerateResult, GoogleDriver, HttpRunner, HttpRunnerError,
    HttpRunnerResponse, LLMBackend, LLMDriverConfig, MockDriver, MockResponse, OpenAiCompatDriver,
    OpenAiDriver, ProcessError, ProcessRunner, ProverDriver, RejectionReason, RetryConfig, Sleeper,
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

#[test]
fn test_extract_proof_source_rejects_full_theorem_contract_shape() {
    let raw = "```lean\ntheorem BooleVerifyMod.instance_thm : True := by\n  trivial\n```";
    assert_eq!(
        extract_proof_source(raw),
        Err(RejectionReason::ContractFailed)
    );
}

#[test]
fn test_extract_proof_source_rejects_markdown_prose_contract_shape() {
    let raw = "* We need induction.\n* Then use length_dedup_le.";
    assert_eq!(
        extract_proof_source(raw),
        Err(RejectionReason::ContractFailed)
    );
}

#[test]
fn test_extract_proof_source_accepts_by_tactic_body() {
    let raw = "by\n  intro xs\n  exact length_dedup_le xs";
    assert_eq!(extract_proof_source(raw).unwrap(), raw);
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
    let driver =
        ClaudeCliDriver::with_runner("claude", Duration::from_secs(60), Box::new(runner.clone()));
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
    let driver = ClaudeCliDriver::with_runner("claude", Duration::from_secs(5), Box::new(runner));
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
    let driver =
        AgentCliDriver::with_runner("hermes", vec![], Duration::from_secs(5), Box::new(runner));
    match driver.generate("p") {
        GenerateResult::Rejected { reason, .. } => {
            assert_eq!(reason, RejectionReason::EmptyResponse);
        }
        other => panic!("expected Rejected, got {other:?}"),
    }
}

// --- create_driver --------------------------------------------------------

fn driver_cfg(backend: LLMBackend, timeout: Duration) -> LLMDriverConfig {
    LLMDriverConfig {
        backend,
        timeout,
        claude_binary: None,
        agent_command: None,
        agent_args: vec![],
        api_key: None,
        model: None,
        base_url: None,
        max_tokens: None,
    }
}

#[test]
fn test_create_driver_mock_backend_is_not_constructible() {
    let cfg = driver_cfg(LLMBackend::Mock, Duration::from_secs(10));
    match create_driver(&cfg) {
        Err(DriverConfigError::MockNotConstructible) => (),
        Err(other) => panic!("expected MockNotConstructible, got {other:?}"),
        Ok(_) => panic!("expected error"),
    }
}

#[test]
fn test_create_driver_claude_cli_uses_default_binary() {
    let cfg = driver_cfg(LLMBackend::ClaudeCli, Duration::from_secs(120));
    let d = create_driver(&cfg).unwrap();
    assert_eq!(d.name(), "claude_cli");
}

#[test]
fn test_create_driver_agent_cli_requires_command() {
    let cfg = driver_cfg(LLMBackend::AgentCli, Duration::from_secs(60));
    match create_driver(&cfg) {
        Err(DriverConfigError::AgentCommandMissing) => (),
        Err(other) => panic!("expected AgentCommandMissing, got {other:?}"),
        Ok(_) => panic!("expected error"),
    }
}

#[test]
fn test_create_driver_agent_cli_with_command_succeeds() {
    let mut cfg = driver_cfg(LLMBackend::AgentCli, Duration::from_secs(60));
    cfg.agent_command = Some("hermes".to_string());
    cfg.agent_args = vec!["--mode=lean".to_string()];
    let d = create_driver(&cfg).unwrap();
    assert_eq!(d.name(), "agent_cli");
}

#[test]
fn test_create_driver_openai_compat_requires_base_url() {
    let mut cfg = driver_cfg(LLMBackend::OpenAiCompat, Duration::from_secs(60));
    cfg.model = Some("gemma3:27b".to_string());
    match create_driver(&cfg) {
        Err(DriverConfigError::BaseUrlMissing) => (),
        Err(other) => panic!("expected BaseUrlMissing, got {other:?}"),
        Ok(_) => panic!("expected error"),
    }
}

#[test]
fn test_create_driver_openai_compat_requires_model() {
    let mut cfg = driver_cfg(LLMBackend::OpenAiCompat, Duration::from_secs(60));
    cfg.base_url = Some("http://localhost:11434".to_string());
    match create_driver(&cfg) {
        Err(DriverConfigError::ModelMissing("openai_compat")) => (),
        Err(other) => panic!("expected ModelMissing(\"openai_compat\"), got {other:?}"),
        Ok(_) => panic!("expected error"),
    }
}

#[test]
fn test_create_driver_openai_compat_with_required_fields_succeeds() {
    let mut cfg = driver_cfg(LLMBackend::OpenAiCompat, Duration::from_secs(60));
    cfg.base_url = Some("http://localhost:11434".to_string());
    cfg.model = Some("gemma3:27b".to_string());
    let d = create_driver(&cfg).unwrap();
    assert_eq!(d.name(), "openai_compat");
}

#[test]
fn test_llm_backend_parse_round_trips() {
    for s in [
        "mock",
        "claude_cli",
        "agent_cli",
        "openai_compat",
        "anthropic",
        "openai",
        "google",
    ] {
        let b = LLMBackend::parse(s).unwrap();
        assert_eq!(b.as_str(), s);
    }
    assert_eq!(LLMBackend::parse("unknown"), None);
}

#[test]
fn test_create_driver_anthropic_requires_api_key() {
    let mut cfg = driver_cfg(LLMBackend::Anthropic, Duration::from_secs(60));
    cfg.model = Some("claude-opus-4-7".to_string());
    match create_driver(&cfg) {
        Err(DriverConfigError::ApiKeyMissing("anthropic")) => (),
        Err(other) => panic!("expected ApiKeyMissing(\"anthropic\"), got {other:?}"),
        Ok(_) => panic!("expected error"),
    }
}

#[test]
fn test_create_driver_anthropic_requires_model() {
    let mut cfg = driver_cfg(LLMBackend::Anthropic, Duration::from_secs(60));
    cfg.api_key = Some("sk-ant-xxx".to_string());
    match create_driver(&cfg) {
        Err(DriverConfigError::ModelMissing("anthropic")) => (),
        Err(other) => panic!("expected ModelMissing(\"anthropic\"), got {other:?}"),
        Ok(_) => panic!("expected error"),
    }
}

#[test]
fn test_create_driver_anthropic_with_required_fields_succeeds() {
    let mut cfg = driver_cfg(LLMBackend::Anthropic, Duration::from_secs(60));
    cfg.api_key = Some("sk-ant-xxx".to_string());
    cfg.model = Some("claude-opus-4-7".to_string());
    let d = create_driver(&cfg).unwrap();
    assert_eq!(d.name(), "anthropic");
}

#[test]
fn test_create_driver_openai_requires_api_key() {
    let mut cfg = driver_cfg(LLMBackend::OpenAi, Duration::from_secs(60));
    cfg.model = Some("gpt-5".to_string());
    match create_driver(&cfg) {
        Err(DriverConfigError::ApiKeyMissing("openai")) => (),
        Err(other) => panic!("expected ApiKeyMissing(\"openai\"), got {other:?}"),
        Ok(_) => panic!("expected error"),
    }
}

#[test]
fn test_create_driver_openai_with_required_fields_succeeds() {
    let mut cfg = driver_cfg(LLMBackend::OpenAi, Duration::from_secs(60));
    cfg.api_key = Some("sk-xxx".to_string());
    cfg.model = Some("gpt-5".to_string());
    let d = create_driver(&cfg).unwrap();
    assert_eq!(d.name(), "openai");
}

#[test]
fn test_create_driver_google_requires_api_key() {
    let mut cfg = driver_cfg(LLMBackend::Google, Duration::from_secs(60));
    cfg.model = Some("gemini-2.5-pro".to_string());
    match create_driver(&cfg) {
        Err(DriverConfigError::ApiKeyMissing("google")) => (),
        Err(other) => panic!("expected ApiKeyMissing(\"google\"), got {other:?}"),
        Ok(_) => panic!("expected error"),
    }
}

#[test]
fn test_create_driver_google_with_required_fields_succeeds() {
    let mut cfg = driver_cfg(LLMBackend::Google, Duration::from_secs(60));
    cfg.api_key = Some("AIza-xxx".to_string());
    cfg.model = Some("gemini-2.5-pro".to_string());
    let d = create_driver(&cfg).unwrap();
    assert_eq!(d.name(), "google");
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
        .run(
            "/bin/sleep",
            &["5".to_string()],
            None,
            Duration::from_millis(200),
        )
        .unwrap_err();
    assert!(matches!(err, ProcessError::Timeout { .. }), "got {err:?}");
}

// --- HttpRunner mock + OpenAiCompatDriver --------------------------------

#[derive(Default, Debug)]
struct HttpCapture {
    url: String,
    headers: Vec<(String, String)>,
    body: serde_json::Value,
    timeout: Duration,
}

#[derive(Clone)]
struct FakeHttpRunner {
    inner: Arc<Mutex<FakeHttpRunnerInner>>,
}

struct FakeHttpRunnerInner {
    captures: Vec<HttpCapture>,
    next: Vec<Result<HttpRunnerResponse, HttpRunnerError>>,
}

impl FakeHttpRunner {
    fn new(responses: Vec<Result<HttpRunnerResponse, HttpRunnerError>>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(FakeHttpRunnerInner {
                captures: Vec::new(),
                next: responses,
            })),
        }
    }

    fn captured(&self) -> Vec<HttpCapture> {
        let inner = self.inner.lock().unwrap();
        inner
            .captures
            .iter()
            .map(|c| HttpCapture {
                url: c.url.clone(),
                headers: c.headers.clone(),
                body: c.body.clone(),
                timeout: c.timeout,
            })
            .collect()
    }
}

impl HttpRunner for FakeHttpRunner {
    fn post_json(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: &serde_json::Value,
        timeout: Duration,
    ) -> Result<HttpRunnerResponse, HttpRunnerError> {
        let mut inner = self.inner.lock().unwrap();
        inner.captures.push(HttpCapture {
            url: url.to_string(),
            headers: headers
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            body: body.clone(),
            timeout,
        });
        if inner.next.is_empty() {
            return Err(HttpRunnerError::Network(
                "FakeHttpRunner exhausted".to_string(),
            ));
        }
        inner.next.remove(0)
    }
}

fn ok_chat_completion(content: &str, completion_tokens: Option<u64>) -> HttpRunnerResponse {
    let usage = match completion_tokens {
        Some(n) => serde_json::json!({"completion_tokens": n}),
        None => serde_json::Value::Null,
    };
    let payload = serde_json::json!({
        "choices": [{"message": {"role": "assistant", "content": content}}],
        "usage": usage,
    });
    HttpRunnerResponse {
        status: 200,
        body: serde_json::to_vec(&payload).unwrap(),
    }
}

#[test]
fn test_openai_compat_driver_posts_to_v1_chat_completions_with_bearer_auth() {
    let http = FakeHttpRunner::new(vec![Ok(ok_chat_completion(
        "```lean\nby trivial\n```",
        Some(42),
    ))]);
    let driver = OpenAiCompatDriver::with_runner(
        "http://localhost:11434",
        "sk-secret",
        "gemma3:27b",
        4096,
        Duration::from_secs(30),
        Box::new(http.clone()),
    );
    let r = driver.generate("PROMPT");
    assert_eq!(driver.name(), "openai_compat");
    assert_eq!(driver.strategy(), Strategy::OpenWeight);
    match r {
        GenerateResult::Solved {
            proof_source,
            tokens_used,
            ..
        } => {
            assert_eq!(proof_source, "by trivial");
            assert_eq!(tokens_used, Some(42));
        }
        other => panic!("expected Solved, got {other:?}"),
    }
    let cap = http.captured();
    assert_eq!(cap.len(), 1);
    assert_eq!(cap[0].url, "http://localhost:11434/v1/chat/completions");
    assert!(
        cap[0]
            .headers
            .iter()
            .any(|(k, v)| k == "authorization" && v == "Bearer sk-secret"),
        "expected Bearer auth header, got {:?}",
        cap[0].headers
    );
    assert_eq!(cap[0].body["model"], "gemma3:27b");
    assert_eq!(cap[0].body["max_tokens"], 4096);
    assert_eq!(cap[0].body["think"], false);
    assert_eq!(cap[0].body["messages"][0]["role"], "user");
    assert_eq!(cap[0].body["messages"][0]["content"], "PROMPT");
}

#[test]
fn test_openai_compat_driver_strips_trailing_slash_from_base_url() {
    let http = FakeHttpRunner::new(vec![Ok(ok_chat_completion("by rfl", None))]);
    let driver = OpenAiCompatDriver::with_runner(
        "http://localhost:11434/",
        "sk",
        "m",
        2048,
        Duration::from_secs(10),
        Box::new(http.clone()),
    );
    let _ = driver.generate("p");
    let cap = http.captured();
    assert_eq!(cap[0].url, "http://localhost:11434/v1/chat/completions");
}

#[test]
fn test_openai_compat_driver_returns_error_on_http_5xx() {
    let http = FakeHttpRunner::new(vec![Ok(HttpRunnerResponse {
        status: 503,
        body: b"upstream unavailable".to_vec(),
    })]);
    let driver = OpenAiCompatDriver::with_runner(
        "http://localhost:11434",
        "sk",
        "m",
        2048,
        Duration::from_secs(10),
        Box::new(http),
    );
    match driver.generate("p") {
        GenerateResult::Error { cause, .. } => {
            assert!(cause.contains("503"), "got {cause}");
            assert!(cause.contains("upstream unavailable"), "got {cause}");
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn test_openai_compat_driver_returns_error_on_network_failure() {
    let http = FakeHttpRunner::new(vec![Err(HttpRunnerError::Timeout {
        url: "http://localhost:11434/v1/chat/completions".to_string(),
        ms: 30_000,
    })]);
    let driver = OpenAiCompatDriver::with_runner(
        "http://localhost:11434",
        "sk",
        "m",
        2048,
        Duration::from_secs(30),
        Box::new(http),
    );
    match driver.generate("p") {
        GenerateResult::Error { cause, .. } => {
            assert!(cause.contains("timed out"), "got {cause}");
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn test_openai_compat_driver_classifies_empty_content_as_rejected() {
    let http = FakeHttpRunner::new(vec![Ok(ok_chat_completion("", None))]);
    let driver = OpenAiCompatDriver::with_runner(
        "http://localhost:11434",
        "sk",
        "m",
        2048,
        Duration::from_secs(10),
        Box::new(http),
    );
    match driver.generate("p") {
        GenerateResult::Rejected { reason, .. } => {
            assert_eq!(reason, RejectionReason::EmptyResponse);
        }
        other => panic!("expected Rejected, got {other:?}"),
    }
}

#[test]
fn test_openai_compat_driver_falls_back_to_legacy_choice_text() {
    let payload = serde_json::json!({
        "choices": [{"text": "```lean\nby rfl\n```"}],
        "usage": {"completion_tokens": 7},
    });
    let http = FakeHttpRunner::new(vec![Ok(HttpRunnerResponse {
        status: 200,
        body: serde_json::to_vec(&payload).unwrap(),
    })]);
    let driver = OpenAiCompatDriver::with_runner(
        "http://localhost:11434",
        "sk",
        "m",
        2048,
        Duration::from_secs(10),
        Box::new(http),
    );
    match driver.generate("p") {
        GenerateResult::Solved {
            proof_source,
            tokens_used,
            ..
        } => {
            assert_eq!(proof_source, "by rfl");
            assert_eq!(tokens_used, Some(7));
        }
        other => panic!("expected Solved, got {other:?}"),
    }
}

#[test]
fn test_openai_compat_driver_falls_back_to_ollama_reasoning_when_content_empty() {
    let payload = serde_json::json!({
        "choices": [{"message": {
            "role": "assistant",
            "content": "",
            "reasoning": "scratchpad before answer\n```lean\nby trivial\n```"
        }}],
        "usage": {"completion_tokens": 99},
    });
    let http = FakeHttpRunner::new(vec![Ok(HttpRunnerResponse {
        status: 200,
        body: serde_json::to_vec(&payload).unwrap(),
    })]);
    let driver = OpenAiCompatDriver::with_runner(
        "http://localhost:11434",
        "sk",
        "m",
        2048,
        Duration::from_secs(10),
        Box::new(http),
    );
    match driver.generate("p") {
        GenerateResult::Solved {
            proof_source,
            tokens_used,
            ..
        } => {
            assert_eq!(proof_source, "by trivial");
            assert_eq!(tokens_used, Some(99));
        }
        other => panic!("expected Solved, got {other:?}"),
    }
}

#[test]
fn test_openai_compat_driver_returns_error_on_malformed_json() {
    let http = FakeHttpRunner::new(vec![Ok(HttpRunnerResponse {
        status: 200,
        body: b"not json".to_vec(),
    })]);
    let driver = OpenAiCompatDriver::with_runner(
        "http://localhost:11434",
        "sk",
        "m",
        2048,
        Duration::from_secs(10),
        Box::new(http),
    );
    match driver.generate("p") {
        GenerateResult::Error { cause, .. } => {
            assert!(cause.contains("malformed JSON"), "got {cause}");
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

// --- AnthropicDriver -----------------------------------------------------

fn ok_anthropic(content: &str, output_tokens: Option<u64>) -> HttpRunnerResponse {
    let usage = match output_tokens {
        Some(n) => serde_json::json!({"input_tokens": 10, "output_tokens": n}),
        None => serde_json::Value::Null,
    };
    let payload = serde_json::json!({
        "id": "msg_x",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": content}],
        "usage": usage,
    });
    HttpRunnerResponse {
        status: 200,
        body: serde_json::to_vec(&payload).unwrap(),
    }
}

#[test]
fn test_anthropic_driver_posts_to_v1_messages_with_x_api_key() {
    let http = FakeHttpRunner::new(vec![Ok(ok_anthropic("```lean\nby trivial\n```", Some(57)))]);
    let driver = AnthropicDriver::with_runner(
        "https://api.anthropic.com",
        "sk-ant-secret",
        "claude-opus-4-7",
        4096,
        Duration::from_secs(30),
        Box::new(http.clone()),
    );
    let r = driver.generate("PROMPT");
    assert_eq!(driver.name(), "anthropic");
    assert_eq!(driver.strategy(), Strategy::Frontier);
    match r {
        GenerateResult::Solved {
            proof_source,
            tokens_used,
            ..
        } => {
            assert_eq!(proof_source, "by trivial");
            assert_eq!(tokens_used, Some(57));
        }
        other => panic!("expected Solved, got {other:?}"),
    }
    let cap = http.captured();
    assert_eq!(cap.len(), 1);
    assert_eq!(cap[0].url, "https://api.anthropic.com/v1/messages");
    assert!(
        cap[0]
            .headers
            .iter()
            .any(|(k, v)| k == "x-api-key" && v == "sk-ant-secret"),
        "expected x-api-key header, got {:?}",
        cap[0].headers
    );
    assert!(
        cap[0]
            .headers
            .iter()
            .any(|(k, v)| k == "anthropic-version" && v == "2023-06-01"),
        "expected anthropic-version header, got {:?}",
        cap[0].headers
    );
    assert_eq!(cap[0].body["model"], "claude-opus-4-7");
    assert_eq!(cap[0].body["max_tokens"], 4096);
    assert_eq!(cap[0].body["messages"][0]["role"], "user");
    assert_eq!(cap[0].body["messages"][0]["content"], "PROMPT");
}

#[test]
fn test_anthropic_driver_concatenates_multiple_text_blocks() {
    let payload = serde_json::json!({
        "content": [
            {"type": "text", "text": "```lean\n"},
            {"type": "text", "text": "by rfl\n```"},
        ],
        "usage": {"output_tokens": 9},
    });
    let http = FakeHttpRunner::new(vec![Ok(HttpRunnerResponse {
        status: 200,
        body: serde_json::to_vec(&payload).unwrap(),
    })]);
    let driver = AnthropicDriver::with_runner(
        "https://api.anthropic.com",
        "k",
        "m",
        2048,
        Duration::from_secs(10),
        Box::new(http),
    );
    match driver.generate("p") {
        GenerateResult::Solved { proof_source, .. } => assert_eq!(proof_source, "by rfl"),
        other => panic!("expected Solved, got {other:?}"),
    }
}

#[test]
fn test_anthropic_driver_returns_error_on_4xx() {
    let http = FakeHttpRunner::new(vec![Ok(HttpRunnerResponse {
        status: 401,
        body: b"{\"error\":\"invalid_api_key\"}".to_vec(),
    })]);
    let driver = AnthropicDriver::with_runner(
        "https://api.anthropic.com",
        "bad",
        "m",
        2048,
        Duration::from_secs(10),
        Box::new(http),
    );
    match driver.generate("p") {
        GenerateResult::Error { cause, .. } => {
            assert!(cause.contains("401"), "got {cause}");
            assert!(cause.contains("invalid_api_key"), "got {cause}");
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn test_anthropic_driver_classifies_empty_content_as_rejected() {
    let payload = serde_json::json!({
        "content": [],
        "usage": {"output_tokens": 0},
    });
    let http = FakeHttpRunner::new(vec![Ok(HttpRunnerResponse {
        status: 200,
        body: serde_json::to_vec(&payload).unwrap(),
    })]);
    let driver = AnthropicDriver::with_runner(
        "https://api.anthropic.com",
        "k",
        "m",
        2048,
        Duration::from_secs(10),
        Box::new(http),
    );
    match driver.generate("p") {
        GenerateResult::Rejected { reason, .. } => {
            assert_eq!(reason, RejectionReason::EmptyResponse);
        }
        other => panic!("expected Rejected, got {other:?}"),
    }
}

// --- OpenAiDriver --------------------------------------------------------

#[test]
fn test_openai_driver_pins_to_api_openai_com_with_bearer_auth() {
    let http = FakeHttpRunner::new(vec![Ok(ok_chat_completion(
        "```lean\nby trivial\n```",
        Some(31),
    ))]);
    let driver = OpenAiDriver::with_runner(
        "sk-openai-secret",
        "gpt-5",
        4096,
        Duration::from_secs(30),
        Box::new(http.clone()),
    );
    let r = driver.generate("PROMPT");
    assert_eq!(driver.name(), "openai");
    assert_eq!(driver.strategy(), Strategy::Frontier);
    match r {
        GenerateResult::Solved {
            proof_source,
            tokens_used,
            ..
        } => {
            assert_eq!(proof_source, "by trivial");
            assert_eq!(tokens_used, Some(31));
        }
        other => panic!("expected Solved, got {other:?}"),
    }
    let cap = http.captured();
    assert_eq!(cap.len(), 1);
    assert_eq!(cap[0].url, "https://api.openai.com/v1/chat/completions");
    assert!(
        cap[0]
            .headers
            .iter()
            .any(|(k, v)| k == "authorization" && v == "Bearer sk-openai-secret"),
        "expected Bearer auth header, got {:?}",
        cap[0].headers
    );
    assert_eq!(cap[0].body["model"], "gpt-5");
}

#[test]
fn test_openai_driver_returns_error_on_5xx() {
    let http = FakeHttpRunner::new(vec![Ok(HttpRunnerResponse {
        status: 500,
        body: b"server boom".to_vec(),
    })]);
    let driver =
        OpenAiDriver::with_runner("sk", "gpt-5", 2048, Duration::from_secs(10), Box::new(http));
    match driver.generate("p") {
        GenerateResult::Error { cause, .. } => {
            assert!(cause.contains("500"), "got {cause}");
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

// --- GoogleDriver --------------------------------------------------------

fn ok_gemini(parts: Vec<&str>, candidates_tokens: Option<u64>) -> HttpRunnerResponse {
    let usage = match candidates_tokens {
        Some(n) => serde_json::json!({"promptTokenCount": 12, "candidatesTokenCount": n}),
        None => serde_json::Value::Null,
    };
    let parts_json: Vec<serde_json::Value> = parts
        .iter()
        .map(|t| serde_json::json!({"text": t}))
        .collect();
    let payload = serde_json::json!({
        "candidates": [{
            "content": {"parts": parts_json, "role": "model"},
            "finishReason": "STOP",
        }],
        "usageMetadata": usage,
    });
    HttpRunnerResponse {
        status: 200,
        body: serde_json::to_vec(&payload).unwrap(),
    }
}

#[test]
fn test_google_driver_posts_to_generate_content_with_x_goog_api_key() {
    let http = FakeHttpRunner::new(vec![Ok(ok_gemini(
        vec!["```lean\nby trivial\n```"],
        Some(64),
    ))]);
    let driver = GoogleDriver::with_runner(
        "https://generativelanguage.googleapis.com",
        "AIza-secret",
        "gemini-2.5-pro",
        4096,
        Duration::from_secs(30),
        Box::new(http.clone()),
    );
    let r = driver.generate("PROMPT");
    assert_eq!(driver.name(), "google");
    assert_eq!(driver.strategy(), Strategy::Frontier);
    match r {
        GenerateResult::Solved {
            proof_source,
            tokens_used,
            ..
        } => {
            assert_eq!(proof_source, "by trivial");
            assert_eq!(tokens_used, Some(64));
        }
        other => panic!("expected Solved, got {other:?}"),
    }
    let cap = http.captured();
    assert_eq!(cap.len(), 1);
    assert_eq!(
        cap[0].url,
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:generateContent"
    );
    assert!(
        cap[0]
            .headers
            .iter()
            .any(|(k, v)| k == "x-goog-api-key" && v == "AIza-secret"),
        "expected x-goog-api-key header, got {:?}",
        cap[0].headers
    );
    assert_eq!(cap[0].body["contents"][0]["parts"][0]["text"], "PROMPT");
    assert_eq!(cap[0].body["generationConfig"]["maxOutputTokens"], 4096);
}

#[test]
fn test_google_driver_concatenates_multiple_parts() {
    let http = FakeHttpRunner::new(vec![Ok(ok_gemini(
        vec!["```lean\n", "by rfl\n", "```"],
        Some(7),
    ))]);
    let driver = GoogleDriver::with_runner(
        "https://generativelanguage.googleapis.com",
        "k",
        "gemini-2.5-pro",
        2048,
        Duration::from_secs(10),
        Box::new(http),
    );
    match driver.generate("p") {
        GenerateResult::Solved { proof_source, .. } => assert_eq!(proof_source, "by rfl"),
        other => panic!("expected Solved, got {other:?}"),
    }
}

#[test]
fn test_google_driver_returns_error_on_4xx() {
    let http = FakeHttpRunner::new(vec![Ok(HttpRunnerResponse {
        status: 403,
        body: b"PERMISSION_DENIED".to_vec(),
    })]);
    let driver = GoogleDriver::with_runner(
        "https://generativelanguage.googleapis.com",
        "bad",
        "gemini-2.5-pro",
        2048,
        Duration::from_secs(10),
        Box::new(http),
    );
    match driver.generate("p") {
        GenerateResult::Error { cause, .. } => {
            assert!(cause.contains("403"), "got {cause}");
            assert!(cause.contains("PERMISSION_DENIED"), "got {cause}");
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn test_google_driver_classifies_empty_candidates_as_rejected() {
    let payload = serde_json::json!({
        "candidates": [{
            "content": {"parts": [], "role": "model"},
            "finishReason": "MAX_TOKENS",
        }],
    });
    let http = FakeHttpRunner::new(vec![Ok(HttpRunnerResponse {
        status: 200,
        body: serde_json::to_vec(&payload).unwrap(),
    })]);
    let driver = GoogleDriver::with_runner(
        "https://generativelanguage.googleapis.com",
        "k",
        "gemini-2.5-pro",
        2048,
        Duration::from_secs(10),
        Box::new(http),
    );
    match driver.generate("p") {
        GenerateResult::Rejected { reason, .. } => {
            assert_eq!(reason, RejectionReason::EmptyResponse);
        }
        other => panic!("expected Rejected, got {other:?}"),
    }
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
