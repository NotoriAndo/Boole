// LLM driver — provider abstraction for proof generation.
//
// Backends supported:
//   - mock          — in-process canned responses for tests.
//   - claude_cli    — shells out to `claude -p`; uses the CLI's local OAuth
//                     session, no API key required.
//   - agent_cli     — shells out to a configurable autonomous-agent CLI
//                     (Hermes, OpenClaw, OpenCode, etc.) with the prompt
//                     appended as the final argv item.
//   - openai_compat — POSTs to a self-hosted or proxied
//                     `{base_url}/v1/chat/completions` (Ollama, vLLM,
//                     LM Studio, llama.cpp, DeepSeek, etc.). TLS via
//                     `http_runner::ReqwestHttpRunner`. Includes the
//                     `think: false` Ollama extension so reasoning-mode
//                     models route their output to `content` rather than
//                     a non-standard `reasoning` field.
//   - anthropic     — direct Anthropic API: `POST /v1/messages` with
//                     `x-api-key` + `anthropic-version` headers. Parses
//                     `content[*].text` (concatenating all text blocks).
//   - openai        — direct OpenAI API: thin wrapper around the
//                     openai_compat driver pinned to
//                     `https://api.openai.com`. Strategy::Frontier.
//   - google        — direct Gemini API: `POST /v1beta/models/{model}:generateContent`
//                     with `x-goog-api-key`. Parses
//                     `candidates[0].content.parts[*].text`.
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

use crate::http_runner::{HttpRunner, HttpRunnerError, ReqwestHttpRunner};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LLMBackend {
    Mock,
    ClaudeCli,
    AgentCli,
    OpenAiCompat,
    Anthropic,
    OpenAi,
    Google,
}

impl LLMBackend {
    pub fn as_str(&self) -> &'static str {
        match self {
            LLMBackend::Mock => "mock",
            LLMBackend::ClaudeCli => "claude_cli",
            LLMBackend::AgentCli => "agent_cli",
            LLMBackend::OpenAiCompat => "openai_compat",
            LLMBackend::Anthropic => "anthropic",
            LLMBackend::OpenAi => "openai",
            LLMBackend::Google => "google",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "mock" => Some(LLMBackend::Mock),
            "claude_cli" => Some(LLMBackend::ClaudeCli),
            "agent_cli" => Some(LLMBackend::AgentCli),
            "openai_compat" => Some(LLMBackend::OpenAiCompat),
            "anthropic" => Some(LLMBackend::Anthropic),
            "openai" => Some(LLMBackend::OpenAi),
            "google" => Some(LLMBackend::Google),
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

// --- OpenAI-compat driver (Ollama / vLLM / LM Studio / DeepSeek / …) -----

/// Default `max_tokens` for openai_compat, mirroring pof's TS miner. Higher
/// than the 2k frontier-API default because reasoning-mode models (Gemma 3/4,
/// DeepSeek-R1, Qwen3-thinking) burn ~1k–4k tokens on chain-of-thought even
/// with `think: false` — 2k truncates before usable content emits.
pub const OPENAI_COMPAT_DEFAULT_MAX_TOKENS: u32 = 8192;

pub struct OpenAiCompatDriver {
    base_url: String,
    api_key: String,
    model: String,
    max_tokens: u32,
    timeout: Duration,
    http: Box<dyn HttpRunner>,
}

impl OpenAiCompatDriver {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        max_tokens: u32,
        timeout: Duration,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            model: model.into(),
            max_tokens,
            timeout,
            http: Box::new(ReqwestHttpRunner),
        }
    }

    pub fn with_runner(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        max_tokens: u32,
        timeout: Duration,
        http: Box<dyn HttpRunner>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            model: model.into(),
            max_tokens,
            timeout,
            http,
        }
    }
}

impl ProverDriver for OpenAiCompatDriver {
    fn name(&self) -> &str {
        "openai_compat"
    }

    fn strategy(&self) -> Strategy {
        Strategy::OpenWeight
    }

    fn generate(&self, prompt: &str) -> GenerateResult {
        let started = Instant::now();
        let url = format!(
            "{}/v1/chat/completions",
            self.base_url.trim_end_matches('/')
        );
        let auth = format!("Bearer {}", self.api_key);
        // Body mirrors pof llmDriver.ts:241-246. `think: false` is the
        // Ollama extension that disables reasoning-mode CoT scratchpad;
        // servers that don't recognize it ignore it per OpenAI spec
        // forward-compat.
        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "messages": [{"role": "user", "content": prompt}],
            "think": false,
        });
        let resp = match self.http.post_json(
            &url,
            &[("authorization", auth.as_str())],
            &body,
            self.timeout,
        ) {
            Ok(r) => r,
            Err(err) => {
                return GenerateResult::Error {
                    cause: render_http_runner_error(&err),
                    elapsed: started.elapsed(),
                }
            }
        };
        if resp.status < 200 || resp.status >= 300 {
            let snippet = String::from_utf8_lossy(&resp.body)
                .chars()
                .take(500)
                .collect::<String>();
            return GenerateResult::Error {
                cause: format!("openai_compat HTTP {}: {}", resp.status, snippet),
                elapsed: started.elapsed(),
            };
        }
        let payload: serde_json::Value = match serde_json::from_slice(&resp.body) {
            Ok(v) => v,
            Err(err) => {
                return GenerateResult::Error {
                    cause: format!("openai_compat: malformed JSON: {err}"),
                    elapsed: started.elapsed(),
                }
            }
        };
        let text = payload
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        let tokens = payload
            .get("usage")
            .and_then(|u| u.get("completion_tokens"))
            .and_then(|n| n.as_u64());
        classify_response(&text, started.elapsed(), tokens)
    }
}

fn render_http_runner_error(err: &HttpRunnerError) -> String {
    err.to_string()
}

// --- Anthropic direct-API driver -----------------------------------------

pub const ANTHROPIC_DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
pub const ANTHROPIC_API_VERSION: &str = "2023-06-01";
pub const ANTHROPIC_DEFAULT_MAX_TOKENS: u32 = 8192;

pub struct AnthropicDriver {
    base_url: String,
    api_key: String,
    model: String,
    max_tokens: u32,
    timeout: Duration,
    http: Box<dyn HttpRunner>,
}

impl AnthropicDriver {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        max_tokens: u32,
        timeout: Duration,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            model: model.into(),
            max_tokens,
            timeout,
            http: Box::new(ReqwestHttpRunner),
        }
    }

    pub fn with_runner(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        max_tokens: u32,
        timeout: Duration,
        http: Box<dyn HttpRunner>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            model: model.into(),
            max_tokens,
            timeout,
            http,
        }
    }
}

impl ProverDriver for AnthropicDriver {
    fn name(&self) -> &str {
        "anthropic"
    }

    fn strategy(&self) -> Strategy {
        Strategy::Frontier
    }

    fn generate(&self, prompt: &str) -> GenerateResult {
        let started = Instant::now();
        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "messages": [{"role": "user", "content": prompt}],
        });
        let resp = match self.http.post_json(
            &url,
            &[
                ("x-api-key", self.api_key.as_str()),
                ("anthropic-version", ANTHROPIC_API_VERSION),
            ],
            &body,
            self.timeout,
        ) {
            Ok(r) => r,
            Err(err) => {
                return GenerateResult::Error {
                    cause: render_http_runner_error(&err),
                    elapsed: started.elapsed(),
                }
            }
        };
        if resp.status < 200 || resp.status >= 300 {
            let snippet = String::from_utf8_lossy(&resp.body)
                .chars()
                .take(500)
                .collect::<String>();
            return GenerateResult::Error {
                cause: format!("anthropic HTTP {}: {}", resp.status, snippet),
                elapsed: started.elapsed(),
            };
        }
        let payload: serde_json::Value = match serde_json::from_slice(&resp.body) {
            Ok(v) => v,
            Err(err) => {
                return GenerateResult::Error {
                    cause: format!("anthropic: malformed JSON: {err}"),
                    elapsed: started.elapsed(),
                }
            }
        };
        // Concatenate `text` from every `text` block. Anthropic returns
        // multiple content blocks for tool-use / structured output; for our
        // single-turn message-only requests there is typically one, but we
        // fold across all of them defensively.
        let text = payload
            .get("content")
            .and_then(|c| c.as_array())
            .map(|arr| {
                arr.iter()
                    .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
                    .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();
        let tokens = payload
            .get("usage")
            .and_then(|u| u.get("output_tokens"))
            .and_then(|n| n.as_u64());
        classify_response(&text, started.elapsed(), tokens)
    }
}

// --- OpenAI direct-API driver --------------------------------------------

pub const OPENAI_DEFAULT_BASE_URL: &str = "https://api.openai.com";
pub const OPENAI_DEFAULT_MAX_TOKENS: u32 = 8192;

/// Direct OpenAI API driver. Wire-format identical to `openai_compat`
/// (same `/v1/chat/completions` endpoint, same Bearer auth), so we
/// delegate to `OpenAiCompatDriver` and only override `name()` /
/// `strategy()` for telemetry.
pub struct OpenAiDriver {
    inner: OpenAiCompatDriver,
}

impl OpenAiDriver {
    pub fn new(
        api_key: impl Into<String>,
        model: impl Into<String>,
        max_tokens: u32,
        timeout: Duration,
    ) -> Self {
        Self {
            inner: OpenAiCompatDriver::new(
                OPENAI_DEFAULT_BASE_URL,
                api_key,
                model,
                max_tokens,
                timeout,
            ),
        }
    }

    pub fn with_runner(
        api_key: impl Into<String>,
        model: impl Into<String>,
        max_tokens: u32,
        timeout: Duration,
        http: Box<dyn HttpRunner>,
    ) -> Self {
        Self {
            inner: OpenAiCompatDriver::with_runner(
                OPENAI_DEFAULT_BASE_URL,
                api_key,
                model,
                max_tokens,
                timeout,
                http,
            ),
        }
    }

    pub fn with_base_url(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        max_tokens: u32,
        timeout: Duration,
    ) -> Self {
        Self {
            inner: OpenAiCompatDriver::new(base_url, api_key, model, max_tokens, timeout),
        }
    }
}

impl ProverDriver for OpenAiDriver {
    fn name(&self) -> &str {
        "openai"
    }

    fn strategy(&self) -> Strategy {
        Strategy::Frontier
    }

    fn generate(&self, prompt: &str) -> GenerateResult {
        self.inner.generate(prompt)
    }
}

// --- Google Gemini direct-API driver -------------------------------------

pub const GOOGLE_DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com";
pub const GOOGLE_DEFAULT_MAX_TOKENS: u32 = 8192;

pub struct GoogleDriver {
    base_url: String,
    api_key: String,
    model: String,
    max_tokens: u32,
    timeout: Duration,
    http: Box<dyn HttpRunner>,
}

impl GoogleDriver {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        max_tokens: u32,
        timeout: Duration,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            model: model.into(),
            max_tokens,
            timeout,
            http: Box::new(ReqwestHttpRunner),
        }
    }

    pub fn with_runner(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        max_tokens: u32,
        timeout: Duration,
        http: Box<dyn HttpRunner>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            model: model.into(),
            max_tokens,
            timeout,
            http,
        }
    }
}

impl ProverDriver for GoogleDriver {
    fn name(&self) -> &str {
        "google"
    }

    fn strategy(&self) -> Strategy {
        Strategy::Frontier
    }

    fn generate(&self, prompt: &str) -> GenerateResult {
        let started = Instant::now();
        // Gemini's REST surface bakes the model into the URL path:
        //   /v1beta/models/{model}:generateContent
        // The model id may contain ':' (e.g. `gemini-2.5-pro`), so we do not
        // URL-encode the path beyond what reqwest already handles.
        let url = format!(
            "{}/v1beta/models/{}:generateContent",
            self.base_url.trim_end_matches('/'),
            self.model
        );
        let body = serde_json::json!({
            "contents": [{"parts": [{"text": prompt}]}],
            "generationConfig": {"maxOutputTokens": self.max_tokens},
        });
        let resp = match self.http.post_json(
            &url,
            &[("x-goog-api-key", self.api_key.as_str())],
            &body,
            self.timeout,
        ) {
            Ok(r) => r,
            Err(err) => {
                return GenerateResult::Error {
                    cause: render_http_runner_error(&err),
                    elapsed: started.elapsed(),
                }
            }
        };
        if resp.status < 200 || resp.status >= 300 {
            let snippet = String::from_utf8_lossy(&resp.body)
                .chars()
                .take(500)
                .collect::<String>();
            return GenerateResult::Error {
                cause: format!("google HTTP {}: {}", resp.status, snippet),
                elapsed: started.elapsed(),
            };
        }
        let payload: serde_json::Value = match serde_json::from_slice(&resp.body) {
            Ok(v) => v,
            Err(err) => {
                return GenerateResult::Error {
                    cause: format!("google: malformed JSON: {err}"),
                    elapsed: started.elapsed(),
                }
            }
        };
        // Concat all `text` parts of the first candidate. Gemini may emit
        // multiple parts per candidate (e.g. for function calls or
        // structured output) — for a plain text request there is usually
        // one, but folding is robust.
        let text = payload
            .get("candidates")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("content"))
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();
        let tokens = payload
            .get("usageMetadata")
            .and_then(|u| u.get("candidatesTokenCount"))
            .and_then(|n| n.as_u64());
        classify_response(&text, started.elapsed(), tokens)
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
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Error)]
pub enum DriverConfigError {
    #[error("mock backend must be instantiated with MockDriver::new(...)")]
    MockNotConstructible,
    #[error("agent_command is required for backend=agent_cli")]
    AgentCommandMissing,
    #[error("base_url is required for backend=openai_compat")]
    BaseUrlMissing,
    #[error("model is required for backend={0}")]
    ModelMissing(&'static str),
    #[error("api_key is required for backend={0}")]
    ApiKeyMissing(&'static str),
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
        LLMBackend::OpenAiCompat => {
            let base_url = cfg
                .base_url
                .clone()
                .ok_or(DriverConfigError::BaseUrlMissing)?;
            let model = cfg
                .model
                .clone()
                .ok_or(DriverConfigError::ModelMissing("openai_compat"))?;
            let api_key = cfg
                .api_key
                .clone()
                .unwrap_or_else(|| "sk-no-key".to_string());
            let max_tokens = cfg.max_tokens.unwrap_or(OPENAI_COMPAT_DEFAULT_MAX_TOKENS);
            Ok(Box::new(OpenAiCompatDriver::new(
                base_url,
                api_key,
                model,
                max_tokens,
                cfg.timeout,
            )))
        }
        LLMBackend::Anthropic => {
            let api_key = cfg
                .api_key
                .clone()
                .ok_or(DriverConfigError::ApiKeyMissing("anthropic"))?;
            let model = cfg
                .model
                .clone()
                .ok_or(DriverConfigError::ModelMissing("anthropic"))?;
            let base_url = cfg
                .base_url
                .clone()
                .unwrap_or_else(|| ANTHROPIC_DEFAULT_BASE_URL.to_string());
            let max_tokens = cfg.max_tokens.unwrap_or(ANTHROPIC_DEFAULT_MAX_TOKENS);
            Ok(Box::new(AnthropicDriver::new(
                base_url,
                api_key,
                model,
                max_tokens,
                cfg.timeout,
            )))
        }
        LLMBackend::OpenAi => {
            let api_key = cfg
                .api_key
                .clone()
                .ok_or(DriverConfigError::ApiKeyMissing("openai"))?;
            let model = cfg
                .model
                .clone()
                .ok_or(DriverConfigError::ModelMissing("openai"))?;
            let max_tokens = cfg.max_tokens.unwrap_or(OPENAI_DEFAULT_MAX_TOKENS);
            // Allow base_url override for Azure OpenAI / proxy deployments.
            Ok(Box::new(match cfg.base_url.clone() {
                Some(b) => OpenAiDriver::with_base_url(b, api_key, model, max_tokens, cfg.timeout),
                None => OpenAiDriver::new(api_key, model, max_tokens, cfg.timeout),
            }))
        }
        LLMBackend::Google => {
            let api_key = cfg
                .api_key
                .clone()
                .ok_or(DriverConfigError::ApiKeyMissing("google"))?;
            let model = cfg
                .model
                .clone()
                .ok_or(DriverConfigError::ModelMissing("google"))?;
            let base_url = cfg
                .base_url
                .clone()
                .unwrap_or_else(|| GOOGLE_DEFAULT_BASE_URL.to_string());
            let max_tokens = cfg.max_tokens.unwrap_or(GOOGLE_DEFAULT_MAX_TOKENS);
            Ok(Box::new(GoogleDriver::new(
                base_url,
                api_key,
                model,
                max_tokens,
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
