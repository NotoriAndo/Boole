// HTTP chain-head fetcher: maps GET /head on the dispatcher to the
// ChainHead shape the mining loop consumes.
//
// Wire format mirrors dispatcher/src/httpServer.ts GET /head body:
//   c, T_ticket, T_share, T_block, T_submit (hex),
//   MinShareScoreMultiplier, M, K_max, L, D_max (numbers),
//   provenance (string).
//
// The miner's D (instance difficulty) is per-cycle policy, NOT a
// chain-head value, so it is supplied by the caller. Same for profile
// and N until the dispatcher exposes them.
use std::time::Duration;

use num_bigint::BigUint;
use serde_json::Value;
use thiserror::Error;

use boole_core::{min_share_score, parse_biguint_hex, Hex32};

use crate::http_client::{HttpClient, HttpError};

#[derive(Debug, Clone)]
pub struct ChainHead {
    pub c: Hex32,
    pub t_ticket: BigUint,
    pub t_share: BigUint,
    pub t_block: BigUint,
    pub t_submit: BigUint,
    pub min_share_score: BigUint,
    pub m: u32,
    pub d: u32,
    pub profile: String,
    pub n: Option<u32>,
}

#[derive(Debug, Error)]
pub enum ChainHeadError {
    #[error("http: {0}")]
    Http(#[from] HttpError),
    #[error("/head returned {0}")]
    Status(u16),
    #[error("/head response not valid JSON")]
    BadJson,
    #[error("/head response missing field {0}")]
    MissingField(&'static str),
    #[error("/head response field {field} has invalid value: {detail}")]
    InvalidField {
        field: &'static str,
        detail: String,
    },
}

/// Trait the mining loop consumes. Lets tests stub out network without
/// standing up a TcpListener for each scenario.
pub trait ChainHeadFetcher: Send + Sync {
    fn fetch_head(&self) -> Result<ChainHead, ChainHeadError>;
}

#[derive(Debug, Clone)]
pub struct HttpChainHeadFetcher {
    http: HttpClient,
    d: u32,
    profile: String,
    n: Option<u32>,
}

impl ChainHeadFetcher for HttpChainHeadFetcher {
    fn fetch_head(&self) -> Result<ChainHead, ChainHeadError> {
        HttpChainHeadFetcher::fetch_head(self)
    }
}

impl HttpChainHeadFetcher {
    pub fn new(base_url: impl Into<String>, d: u32, profile: impl Into<String>) -> Self {
        Self::with_timeout(base_url, Duration::from_secs(10), d, profile, None)
    }

    pub fn with_timeout(
        base_url: impl Into<String>,
        timeout: Duration,
        d: u32,
        profile: impl Into<String>,
        n: Option<u32>,
    ) -> Self {
        Self {
            http: HttpClient::new(base_url, timeout),
            d,
            profile: profile.into(),
            n,
        }
    }

    pub fn fetch_head(&self) -> Result<ChainHead, ChainHeadError> {
        let res = self.http.get("/head")?;
        if res.status != 200 {
            return Err(ChainHeadError::Status(res.status));
        }
        let body: Value = serde_json::from_slice(&res.body).map_err(|_| ChainHeadError::BadJson)?;
        let obj = body.as_object().ok_or(ChainHeadError::BadJson)?;

        let c_hex = obj
            .get("c")
            .and_then(Value::as_str)
            .ok_or(ChainHeadError::MissingField("c"))?;
        let c = Hex32::from_hex(c_hex).map_err(|e| ChainHeadError::InvalidField {
            field: "c",
            detail: e.to_string(),
        })?;

        let t_ticket = parse_hex_field(obj, "T_ticket")?;
        let t_share = parse_hex_field(obj, "T_share")?;
        let t_block = parse_hex_field(obj, "T_block")?;
        let t_submit = parse_hex_field(obj, "T_submit")?;

        let multiplier_nanos = obj
            .get("MinShareScoreMultiplier")
            .and_then(Value::as_u64)
            .ok_or(ChainHeadError::MissingField("MinShareScoreMultiplier"))?;
        let m = obj
            .get("M")
            .and_then(Value::as_u64)
            .ok_or(ChainHeadError::MissingField("M"))? as u32;

        let mss = min_share_score(&t_share, multiplier_nanos).map_err(|e| {
            ChainHeadError::InvalidField {
                field: "T_share",
                detail: e.to_string(),
            }
        })?;

        Ok(ChainHead {
            c,
            t_ticket,
            t_share,
            t_block,
            t_submit,
            min_share_score: mss,
            m,
            d: self.d,
            profile: self.profile.clone(),
            n: self.n,
        })
    }
}

fn parse_hex_field(
    obj: &serde_json::Map<String, Value>,
    field: &'static str,
) -> Result<BigUint, ChainHeadError> {
    let s = obj
        .get(field)
        .and_then(Value::as_str)
        .ok_or(ChainHeadError::MissingField(field))?;
    parse_biguint_hex(s).map_err(|e| ChainHeadError::InvalidField {
        field,
        detail: e.to_string(),
    })
}
