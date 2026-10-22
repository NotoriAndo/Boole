use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use boole_derivation::ParentHeader;
use ring::hmac;
use serde_json::{json, Value};
use std::{
    net::IpAddr,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// A transport response that may recover without changing the request.
#[derive(Debug)]
pub struct TransientRpcError;
impl std::fmt::Display for TransientRpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("temporary RPC unavailability")
    }
}
impl std::error::Error for TransientRpcError {}

/// Bounded retries apply only to transport/body failures and HTTP 429/5xx.
/// Format, consensus and application errors retain their original fail-closed path.
pub fn retry_rpc<T>(mut read: impl FnMut() -> Result<T>) -> Result<T> {
    for delay in [100, 300] {
        match read() {
            Err(e)
                if e.is::<TransientRpcError>()
                    || e.downcast_ref::<reqwest::Error>()
                        .is_some_and(|e| e.is_connect() || e.is_timeout() || e.is_body()) =>
            {
                std::thread::sleep(Duration::from_millis(delay));
            }
            result => return result,
        }
    }
    read()
}

pub trait Engine {
    fn head(&mut self) -> Result<ParentHeader>;
    fn header(&mut self, _height: u64) -> Result<ParentHeader> {
        bail!("Engine node source does not support anchored restart")
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Value>;
}

/// Literal loopback hosts only; redirects, proxy routing and URL credentials
/// are disabled so Engine JWTs cannot leave the local endpoint.
#[derive(Clone)]
pub struct Rpc {
    url: reqwest::Url,
    client: reqwest::blocking::Client,
    key: Option<hmac::Key>,
}
impl Rpc {
    pub fn new(url: &str) -> Result<Self> {
        let url = reqwest::Url::parse(url)?;
        let host = url.host_str().unwrap_or("").trim_matches(['[', ']']);
        let ip: IpAddr = host
            .parse()
            .context("RPC host must be a literal loopback IP")?;
        if !ip.is_loopback()
            || !["http", "https"].contains(&url.scheme())
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            bail!("RPC URL must be loopback without credentials/query/fragment");
        }
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(15))
            .build()?;
        Ok(Self {
            url,
            client,
            key: None,
        })
    }
    pub fn with_jwt(mut self, secret: &[u8]) -> Result<Self> {
        if secret.len() != 32 {
            bail!("JWT secret must have exactly 32 bytes");
        }
        self.key = Some(hmac::Key::new(hmac::HMAC_SHA256, secret));
        Ok(self)
    }
    pub fn call(&self, method: &str, params: Value) -> Result<Value> {
        let mut request = self
            .client
            .post(self.url.clone())
            .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}));
        if let Some(key) = &self.key {
            let iat = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
            let message = format!(
                "{}.{}",
                URL_SAFE_NO_PAD.encode(br#"{"alg":"HS256","typ":"JWT"}"#),
                URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json!({"iat":iat}))?)
            );
            let token = format!(
                "{}.{}",
                message,
                URL_SAFE_NO_PAD.encode(hmac::sign(key, message.as_bytes()).as_ref())
            );
            request = request.bearer_auth(token);
        }
        let response = request
            .send()
            .with_context(|| format!("{method} transport"))?;
        let status = response.status();
        if status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(TransientRpcError.into());
        }
        let body: Value = response
            .json()
            .with_context(|| format!("{method}: malformed JSON response (HTTP {status})"))?;
        if !status.is_success() || body.get("error").is_some() {
            bail!("{method}: HTTP {status}: {body}");
        }
        if body["jsonrpc"] != "2.0" || body["id"] != 1 {
            bail!("{method}: inconsistent JSON-RPC response");
        }
        body.get("result")
            .cloned()
            .ok_or_else(|| anyhow!("{method}: missing result"))
    }
}

pub struct RpcEngine {
    pub node: Rpc,
    auth: Rpc,
}
impl RpcEngine {
    pub fn new(engine_rpc: &str, node_rpc: &str, jwt_secret: &Path) -> Result<Self> {
        let secret = std::fs::read_to_string(jwt_secret).context("read JWT secret")?;
        let secret = secret.trim();
        let secret = boole_derivation::attributes::hex_bytes(
            &format!("0x{}", secret.strip_prefix("0x").unwrap_or(secret)),
            "JWT secret",
        )?;
        let auth = Rpc::new(engine_rpc)?.with_jwt(&secret)?;
        let node = Rpc::new(node_rpc)?;
        if node.call("eth_chainId", json!([]))?
            != format!("0x{:x}", boole_derivation::config::L2_CHAIN_ID)
        {
            bail!("unexpected L2 chain ID");
        }
        Ok(Self { node, auth })
    }
}
impl RpcEngine {
    fn read_header(&self, tag: Value) -> Result<ParentHeader> {
        let block = self
            .node
            .call("eth_getBlockByNumber", json!([tag, false]))?;
        let mut parent: ParentHeader =
            serde_json::from_value(block.clone()).context("decode L2 head")?;
        if parent.number > 0 {
            let tx = block["transactions"][0]
                .as_str()
                .ok_or_else(|| anyhow!("head missing L1-info transaction"))?;
            let raw = self.node.call("eth_getRawTransactionByHash", json!([tx]))?;
            parent.l1_info = Some(
                raw.as_str()
                    .ok_or_else(|| anyhow!("missing raw L1-info transaction"))?
                    .to_owned(),
            );
        }
        Ok(parent)
    }
}
impl Engine for RpcEngine {
    fn head(&mut self) -> Result<ParentHeader> {
        self.read_header(json!("latest"))
    }
    fn header(&mut self, height: u64) -> Result<ParentHeader> {
        self.read_header(json!(format!("0x{height:x}")))
    }
    fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.auth.call(method, params)
    }
}
