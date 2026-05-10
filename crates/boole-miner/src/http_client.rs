// Synchronous HTTP/1.1 client used by the miner's submit / chain-head /
// bounty clients. Mirrors the raw-TcpStream pattern in
// crates/boole-cli/src/main.rs (http_post / http_get / parse_http_response)
// so the miner doesn't pull in a separate HTTP dependency.
//
// Only http:// URLs are supported. The dispatcher is normally local or
// reached via a TLS-terminating proxy; supporting https:// here would
// require a TLS stack that the miner does not need today.
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use thiserror::Error;

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

#[derive(Debug, Error)]
pub enum HttpError {
    #[error("only http:// URLs are supported, got {0}")]
    UnsupportedScheme(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("malformed HTTP response: {0}")]
    Malformed(&'static str),
    #[error("malformed status code: {0}")]
    BadStatus(String),
}

#[derive(Debug, Clone)]
pub struct HttpClient {
    base_url: String,
    timeout: Duration,
}

impl HttpClient {
    pub fn new(base_url: impl Into<String>, timeout: Duration) -> Self {
        let mut base = base_url.into();
        while base.ends_with('/') {
            base.pop();
        }
        Self {
            base_url: base,
            timeout,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn post_json(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<HttpResponse, HttpError> {
        let body_str = serde_json::to_string(body)
            .map_err(|_| HttpError::Malformed("failed to serialize request body"))?;
        let (host_port, full_path) = parse_url(&self.base_url, path)?;
        let mut stream = TcpStream::connect(&host_port)?;
        stream.set_read_timeout(Some(self.timeout))?;
        stream.set_write_timeout(Some(self.timeout))?;
        let request = format!(
            "POST {full_path} HTTP/1.1\r\nHost: {host_port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body_str}",
            body_str.len()
        );
        stream.write_all(request.as_bytes())?;
        let mut buffer = Vec::new();
        stream.read_to_end(&mut buffer)?;
        parse_http_response(&buffer)
    }

    pub fn get(&self, path: &str) -> Result<HttpResponse, HttpError> {
        let (host_port, full_path) = parse_url(&self.base_url, path)?;
        let mut stream = TcpStream::connect(&host_port)?;
        stream.set_read_timeout(Some(self.timeout))?;
        stream.set_write_timeout(Some(self.timeout))?;
        let request = format!(
            "GET {full_path} HTTP/1.1\r\nHost: {host_port}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(request.as_bytes())?;
        let mut buffer = Vec::new();
        stream.read_to_end(&mut buffer)?;
        parse_http_response(&buffer)
    }
}

fn parse_url(base_url: &str, path: &str) -> Result<(String, String), HttpError> {
    let stripped = base_url
        .strip_prefix("http://")
        .ok_or_else(|| HttpError::UnsupportedScheme(base_url.to_string()))?;
    let (host_port, base_path) = match stripped.find('/') {
        Some(idx) => (&stripped[..idx], &stripped[idx..]),
        None => (stripped, ""),
    };
    let full_path = if base_path.is_empty() {
        path.to_string()
    } else {
        format!("{}{}", base_path.trim_end_matches('/'), path)
    };
    Ok((host_port.to_string(), full_path))
}

fn parse_http_response(buffer: &[u8]) -> Result<HttpResponse, HttpError> {
    let header_end = buffer
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or(HttpError::Malformed("missing header terminator"))?;
    let header_text = std::str::from_utf8(&buffer[..header_end])
        .map_err(|_| HttpError::Malformed("non-utf8 headers"))?;
    let mut lines = header_text.split("\r\n");
    let status_line = lines
        .next()
        .ok_or(HttpError::Malformed("missing status line"))?;
    let mut parts = status_line.split_whitespace();
    let _ = parts.next();
    let status_str = parts
        .next()
        .ok_or(HttpError::Malformed("missing status code"))?;
    let status: u16 = status_str
        .parse()
        .map_err(|_| HttpError::BadStatus(status_str.to_string()))?;
    let body = buffer[header_end + 4..].to_vec();
    Ok(HttpResponse { status, body })
}

/// Percent-encode a path segment per RFC 3986. Mirrors `encodeURIComponent`
/// with the same un-reserved set (`A-Za-z0-9 - _ . ! ~ * ' ( )`).
pub fn percent_encode_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_url_strips_trailing_slash_in_constructor() {
        let c = HttpClient::new("http://localhost:8080/", Duration::from_secs(1));
        assert_eq!(c.base_url(), "http://localhost:8080");
    }

    #[test]
    fn parse_url_preserves_base_path() {
        let (hp, fp) = parse_url("http://localhost:8080/api", "/head").unwrap();
        assert_eq!(hp, "localhost:8080");
        assert_eq!(fp, "/api/head");
    }

    #[test]
    fn parse_url_rejects_https() {
        let err = parse_url("https://example.com", "/head").unwrap_err();
        assert!(matches!(err, HttpError::UnsupportedScheme(_)));
    }

    #[test]
    fn percent_encode_known_cases() {
        assert_eq!(percent_encode_component("plain-id_42"), "plain-id_42");
        assert_eq!(percent_encode_component("a/b"), "a%2Fb");
        assert_eq!(percent_encode_component("a b"), "a%20b");
        assert_eq!(percent_encode_component("a:b"), "a%3Ab");
    }

    #[test]
    fn parse_response_extracts_status_and_body() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"ok\":true}";
        let r = parse_http_response(raw).unwrap();
        assert_eq!(r.status, 200);
        assert_eq!(r.body, b"{\"ok\":true}");
    }

    #[test]
    fn parse_response_rejects_missing_terminator() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n";
        assert!(matches!(
            parse_http_response(raw),
            Err(HttpError::Malformed(_))
        ));
    }
}
