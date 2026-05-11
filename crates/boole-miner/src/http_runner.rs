// TLS-capable HTTP runner for outbound LLM provider calls.
//
// `crates/boole-miner/src/http_client.rs` is plaintext-only on purpose —
// it talks to the local dispatcher / boole-node and rolls its own TCP
// stack to avoid dragging a TLS dependency in. LLM provider APIs
// (`anthropic`, `openai`, `openai_compat`/Ollama, `google`) are reachable
// only over TLS, so they live behind a separate runner here.
//
// The runner is split into a trait + production impl so unit tests can
// inject a deterministic fake without standing up a TLS server.
// Mirrors the `ProcessRunner` / `FakeRunner` split already used by
// `claude_cli` / `agent_cli` in `llm_driver.rs`.
use std::time::Duration;

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRunnerResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

#[derive(Debug, Error)]
pub enum HttpRunnerError {
    #[error("request to {url} timed out after {ms}ms")]
    Timeout { url: String, ms: u128 },
    #[error("network error: {0}")]
    Network(String),
    #[error("malformed response: {0}")]
    BadResponse(String),
}

pub trait HttpRunner: Send + Sync {
    fn post_json(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: &serde_json::Value,
        timeout: Duration,
    ) -> Result<HttpRunnerResponse, HttpRunnerError>;
}

pub struct ReqwestHttpRunner;

impl HttpRunner for ReqwestHttpRunner {
    fn post_json(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: &serde_json::Value,
        timeout: Duration,
    ) -> Result<HttpRunnerResponse, HttpRunnerError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| HttpRunnerError::Network(format!("client build: {e}")))?;
        let mut req = client.post(url).json(body);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let resp = req.send().map_err(|e| {
            if e.is_timeout() {
                HttpRunnerError::Timeout {
                    url: url.to_string(),
                    ms: timeout.as_millis(),
                }
            } else {
                HttpRunnerError::Network(e.to_string())
            }
        })?;
        let status = resp.status().as_u16();
        let body = resp
            .bytes()
            .map_err(|e| HttpRunnerError::Network(format!("read body: {e}")))?
            .to_vec();
        Ok(HttpRunnerResponse { status, body })
    }
}
