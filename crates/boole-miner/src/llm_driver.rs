// LLM driver — provider abstraction for proof generation.
//
// Backends supported in this slice:
//   - mock        — in-process canned responses for tests.
//   - claude_cli  — shells out to `claude -p`; uses the CLI's local OAuth
//                   session, no API key required.
//   - agent_cli   — shells out to a configurable autonomous-agent CLI
//                   (Hermes, OpenClaw, OpenCode, etc.) with the prompt
//                   appended as the final argv item.
//
// SDK-based backends (anthropic / openai / google / openai_compat) are
// deferred — they require a TLS HTTP client; the miner's existing
// HttpClient is plaintext-only on purpose.
//
// Each driver accepts a constructed prompt and returns either a candidate
// proof source string or a typed failure (rejected vs error). The retry
// policy lives in `with_retry` rather than each driver, so swapping
// providers does not duplicate retry logic.
//
// Retries fire only on `Error` outcomes; `Rejected` (model returned but
// the response was unusable) is surfaced to the caller without retry —
// retrying with the same prompt will not change the outcome.
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LLMBackend {
    Mock,
    ClaudeCli,
    AgentCli,
}

impl LLMBackend {
    pub fn as_str(&self) -> &'static str {
        match self {
            LLMBackend::Mock => "mock",
            LLMBackend::ClaudeCli => "claude_cli",
            LLMBackend::AgentCli => "agent_cli",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "mock" => Some(LLMBackend::Mock),
            "claude_cli" => Some(LLMBackend::ClaudeCli),
            "agent_cli" => Some(LLMBackend::AgentCli),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    Frontier,
    OpenWeight,
    Hybrid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectionReason {
    EmptyResponse,
    NoProofBlock,
    NonStringResponse,
}

impl RejectionReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            RejectionReason::EmptyResponse => "empty_response",
            RejectionReason::NoProofBlock => "no_proof_block",
            RejectionReason::NonStringResponse => "non_string_response",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenerateResult {
    Solved {
        proof_source: String,
        elapsed: Duration,
        tokens_used: Option<u64>,
    },
    Rejected {
        reason: RejectionReason,
        elapsed: Duration,
    },
    Error {
        cause: String,
        elapsed: Duration,
    },
}

pub trait ProverDriver: Send + Sync {
    fn name(&self) -> &str;
    fn strategy(&self) -> Strategy;
    fn generate(&self, prompt: &str) -> GenerateResult;
}

/// Strip a Lean fenced code block if present, otherwise return the raw text.
/// Empty / whitespace-only is treated as `EmptyResponse`. A fenced block
/// with no body is `NoProofBlock`. Mirrors pof's `extractProofSource`
/// (only `lean` / `lean4` are recognized as language tags).
pub fn extract_proof_source(raw: &str) -> Result<String, RejectionReason> {
    if raw.trim().is_empty() {
        return Err(RejectionReason::EmptyResponse);
    }
    let body: &str = match find_lean_fenced_block(raw) {
        Some(inner) => inner,
        None => raw,
    };
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Err(RejectionReason::NoProofBlock);
    }
    Ok(trimmed.to_string())
}

fn find_lean_fenced_block(raw: &str) -> Option<&str> {
    let start = raw.find("```")?;
    let after_open = &raw[start + 3..];
    let after_lang = if let Some(rest) = after_open.strip_prefix("lean4") {
        rest
    } else if let Some(rest) = after_open.strip_prefix("lean") {
        rest
    } else {
        after_open
    };
    let body_start = after_lang
        .char_indices()
        .find(|(_, c)| !c.is_whitespace())
        .map(|(i, _)| i)
        .unwrap_or(after_lang.len());
    let after_ws = &after_lang[body_start..];
    let close_rel = after_ws.find("```")?;
    Some(&after_ws[..close_rel])
}

// --- Mock driver ----------------------------------------------------------

#[derive(Debug, Clone)]
pub enum MockResponse {
    /// Raw text — passes through `extract_proof_source` to classify.
    Text(String),
    /// Force an `Error` outcome with this cause string.
    Error(String),
}

pub struct MockDriver {
    responses: Mutex<Vec<MockResponse>>,
    cursor: Mutex<usize>,
    latency: Duration,
}

impl MockDriver {
    pub fn new(responses: Vec<MockResponse>) -> Self {
        Self {
            responses: Mutex::new(responses),
            cursor: Mutex::new(0),
            latency: Duration::ZERO,
        }
    }

    pub fn with_latency(responses: Vec<MockResponse>, latency: Duration) -> Self {
        Self {
            responses: Mutex::new(responses),
            cursor: Mutex::new(0),
            latency,
        }
    }
}

impl ProverDriver for MockDriver {
    fn name(&self) -> &str {
        "mock"
    }

    fn strategy(&self) -> Strategy {
        Strategy::Frontier
    }

    fn generate(&self, _prompt: &str) -> GenerateResult {
        let started = Instant::now();
        if !self.latency.is_zero() {
            thread::sleep(self.latency);
        }
        let mut cursor = self.cursor.lock().expect("MockDriver cursor poisoned");
        let responses = self
            .responses
            .lock()
            .expect("MockDriver responses poisoned");
        let i = *cursor;
        *cursor += 1;
        if i >= responses.len() {
            return GenerateResult::Error {
                cause: "MockDriver exhausted".to_string(),
                elapsed: started.elapsed(),
            };
        }
        match &responses[i] {
            MockResponse::Error(cause) => GenerateResult::Error {
                cause: cause.clone(),
                elapsed: started.elapsed(),
            },
            MockResponse::Text(text) => classify_response(text, started.elapsed(), None),
        }
    }
}

// --- Process-runner abstraction ------------------------------------------

#[derive(Debug, Error)]
pub enum ProcessError {
    #[error("command '{0}' not found on PATH")]
    NotFound(String),
    #[error("command '{command}' timed out after {ms}ms")]
    Timeout { command: String, ms: u128 },
    #[error("command '{command}' exited with code {code}: {stderr}")]
    Exit {
        command: String,
        code: String,
        stderr: String,
    },
    #[error("io: {0}")]
    Io(String),
}

pub trait ProcessRunner: Send + Sync {
    fn run(
        &self,
        binary: &str,
        args: &[String],
        stdin_input: Option<&[u8]>,
        timeout: Duration,
    ) -> Result<Vec<u8>, ProcessError>;
}

pub struct StdProcessRunner;

impl ProcessRunner for StdProcessRunner {
    fn run(
        &self,
        binary: &str,
        args: &[String],
        stdin_input: Option<&[u8]>,
        timeout: Duration,
    ) -> Result<Vec<u8>, ProcessError> {
        let mut cmd = Command::new(binary);
        cmd.args(args);
        if stdin_input.is_some() {
            cmd.stdin(Stdio::piped());
        }
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => ProcessError::NotFound(binary.to_string()),
            _ => ProcessError::Io(e.to_string()),
        })?;
        if let Some(input) = stdin_input {
            if let Some(mut stdin) = child.stdin.take() {
                if let Err(e) = stdin.write_all(input) {
                    let _ = child.kill();
                    return Err(ProcessError::Io(e.to_string()));
                }
            }
        }
        let deadline = Instant::now() + timeout;
        loop {
            match child
                .try_wait()
                .map_err(|e| ProcessError::Io(e.to_string()))?
            {
                Some(status) => {
                    let output = child
                        .wait_with_output()
                        .map_err(|e| ProcessError::Io(e.to_string()))?;
                    if !status.success() {
                        let stderr = String::from_utf8_lossy(&output.stderr)
                            .chars()
                            .take(500)
                            .collect();
                        let code = match status.code() {
                            Some(c) => c.to_string(),
                            None => "signal".to_string(),
                        };
                        return Err(ProcessError::Exit {
                            command: binary.to_string(),
                            code,
                            stderr,
                        });
                    }
                    return Ok(output.stdout);
                }
                None => {
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(ProcessError::Timeout {
                            command: binary.to_string(),
                            ms: timeout.as_millis(),
                        });
                    }
                    thread::sleep(Duration::from_millis(50));
                }
            }
        }
    }
}

// --- Claude CLI driver ----------------------------------------------------

pub struct ClaudeCliDriver {
    binary: String,
    timeout: Duration,
    runner: Box<dyn ProcessRunner>,
}

impl ClaudeCliDriver {
    pub fn new(binary: impl Into<String>, timeout: Duration) -> Self {
        Self {
            binary: binary.into(),
            timeout,
            runner: Box::new(StdProcessRunner),
        }
    }

    pub fn with_runner(
        binary: impl Into<String>,
        timeout: Duration,
        runner: Box<dyn ProcessRunner>,
    ) -> Self {
        Self {
            binary: binary.into(),
            timeout,
            runner,
        }
    }
}

impl ProverDriver for ClaudeCliDriver {
    fn name(&self) -> &str {
        "claude_cli"
    }

    fn strategy(&self) -> Strategy {
        Strategy::Frontier
    }

    fn generate(&self, prompt: &str) -> GenerateResult {
        let started = Instant::now();
        let args = vec!["-p".to_string()];
        match self
            .runner
            .run(&self.binary, &args, Some(prompt.as_bytes()), self.timeout)
        {
            Ok(stdout) => {
                let text = String::from_utf8_lossy(&stdout).into_owned();
                classify_response(&text, started.elapsed(), None)
            }
            Err(err) => GenerateResult::Error {
                cause: err.to_string(),
                elapsed: started.elapsed(),
            },
        }
    }
}

// --- Agent CLI driver -----------------------------------------------------

pub struct AgentCliDriver {
    command: String,
    args: Vec<String>,
    timeout: Duration,
    runner: Box<dyn ProcessRunner>,
}

impl AgentCliDriver {
    pub fn new(command: impl Into<String>, args: Vec<String>, timeout: Duration) -> Self {
        Self {
            command: command.into(),
            args,
            timeout,
            runner: Box::new(StdProcessRunner),
        }
    }

    pub fn with_runner(
        command: impl Into<String>,
        args: Vec<String>,
        timeout: Duration,
        runner: Box<dyn ProcessRunner>,
    ) -> Self {
        Self {
            command: command.into(),
            args,
            timeout,
            runner,
        }
    }
}

impl ProverDriver for AgentCliDriver {
    fn name(&self) -> &str {
        "agent_cli"
    }

    fn strategy(&self) -> Strategy {
        Strategy::Hybrid
    }

    fn generate(&self, prompt: &str) -> GenerateResult {
        let started = Instant::now();
        let mut argv = self.args.clone();
        argv.push(prompt.to_string());
        match self.runner.run(&self.command, &argv, None, self.timeout) {
            Ok(stdout) => {
                let text = String::from_utf8_lossy(&stdout).into_owned();
                classify_response(&text, started.elapsed(), None)
            }
            Err(err) => GenerateResult::Error {
                cause: err.to_string(),
                elapsed: started.elapsed(),
            },
        }
    }
}

// --- Driver factory -------------------------------------------------------

#[derive(Debug, Clone)]
pub struct LLMDriverConfig {
    pub backend: LLMBackend,
    pub timeout: Duration,
    pub claude_binary: Option<String>,
    pub agent_command: Option<String>,
    pub agent_args: Vec<String>,
}

#[derive(Debug, Error)]
pub enum DriverConfigError {
    #[error("mock backend must be instantiated with MockDriver::new(...)")]
    MockNotConstructible,
    #[error("agent_command is required for backend=agent_cli")]
    AgentCommandMissing,
}

/// Build a live driver from a config. Returns an error for the `Mock`
/// backend (use `MockDriver::new` directly) and for `AgentCli` without a
/// command. Default timeouts: 120s for `claude_cli`, 300s for `agent_cli`
/// (agents may run tools).
pub fn create_driver(cfg: &LLMDriverConfig) -> Result<Box<dyn ProverDriver>, DriverConfigError> {
    match cfg.backend {
        LLMBackend::Mock => Err(DriverConfigError::MockNotConstructible),
        LLMBackend::ClaudeCli => {
            let binary = cfg
                .claude_binary
                .clone()
                .unwrap_or_else(|| "claude".to_string());
            Ok(Box::new(ClaudeCliDriver::new(binary, cfg.timeout)))
        }
        LLMBackend::AgentCli => {
            let command = cfg
                .agent_command
                .clone()
                .ok_or(DriverConfigError::AgentCommandMissing)?;
            Ok(Box::new(AgentCliDriver::new(
                command,
                cfg.agent_args.clone(),
                cfg.timeout,
            )))
        }
    }
}

// --- Retry wrapper --------------------------------------------------------

pub trait Sleeper: Send + Sync {
    fn sleep(&self, duration: Duration);
}

pub struct ThreadSleeper;

impl Sleeper for ThreadSleeper {
    fn sleep(&self, duration: Duration) {
        thread::sleep(duration);
    }
}

#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_attempts: u32,
    pub initial_backoff: Duration,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_backoff: Duration::from_secs(1),
        }
    }
}

/// Run a generator with retry. Retries fire only on `Error` outcomes —
/// `Rejected` is surfaced immediately (retrying with the same prompt
/// will not change the model's output). Backoff doubles each retry.
pub fn with_retry(
    driver: &dyn ProverDriver,
    prompt: &str,
    cfg: &RetryConfig,
    sleeper: &dyn Sleeper,
) -> GenerateResult {
    let mut last: Option<GenerateResult> = None;
    let attempts = cfg.max_attempts.max(1);
    for i in 0..attempts {
        let r = driver.generate(prompt);
        let is_error = matches!(r, GenerateResult::Error { .. });
        last = Some(r);
        if !is_error {
            return last.expect("set above");
        }
        if i + 1 < attempts {
            let factor = 1u64 << i;
            sleeper.sleep(cfg.initial_backoff.saturating_mul(factor as u32));
        }
    }
    last.expect("at least one attempt")
}

// --- Helpers --------------------------------------------------------------

fn classify_response(text: &str, elapsed: Duration, tokens_used: Option<u64>) -> GenerateResult {
    match extract_proof_source(text) {
        Ok(proof_source) => GenerateResult::Solved {
            proof_source,
            elapsed,
            tokens_used,
        },
        Err(reason) => GenerateResult::Rejected { reason, elapsed },
    }
}
