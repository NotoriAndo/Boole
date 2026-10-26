//! Advisory account state at an exact own finalized header; never inclusion.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NonceState {
    Unavailable,
    NotAdvanced,
    NonceConsumed,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Inclusion {
    Unresolved,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnavailableReason {
    NotChecked,
    NoJournal,
    ForeignMetadata,
    TipChanged,
    JournalUnavailable,
    ProofUnavailable,
    InvalidProof,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NonceObservation {
    pub state: NonceState,
    pub inclusion: Inclusion,
    pub finalized_height: Option<u64>,
    pub finalized_hash: Option<String>,
    pub account_nonce: Option<u64>,
    pub reason: Option<UnavailableReason>,
}
impl NonceObservation {
    pub fn unavailable(reason: UnavailableReason) -> Self {
        Self {
            state: NonceState::Unavailable,
            inclusion: Inclusion::Unresolved,
            finalized_height: None,
            finalized_hash: None,
            account_nonce: None,
            reason: Some(reason),
        }
    }
}

use crate::posting::PostingMetadata;
use anyhow::{ensure, Context, Result};
use boole_derivation::{
    attributes::{hex_bytes, hex_string},
    rlp,
    user_tx::keccak,
};
use boole_devnet_archive::journal::InputJournal;
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    io::Read,
    rc::Rc,
    time::{Duration, Instant},
};
const RESPONSE_CAP: usize = 256 * 1024;
const PROOF_CAP: usize = 128 * 1024;
type TipKey = ([u8; 32], [u8; 32]);
struct FailedObservation {
    tip: TipKey,
    retry_at: Instant,
    reason: UnavailableReason,
}

pub(crate) struct Endpoint {
    url: reqwest::Url,
    client: reqwest::blocking::Client,
    cached: Option<([u8; 32], [u8; 32], u64)>,
    failed: Option<FailedObservation>,
}
impl Endpoint {
    /// RpcPoster already applied the shared literal-loopback URL validation.
    pub(crate) fn new(url: &str) -> Result<Self> {
        let url = reqwest::Url::parse(url)?;
        let ip: std::net::IpAddr = url
            .host_str()
            .context("nonce endpoint host")?
            .trim_matches(['[', ']'])
            .parse()?;
        ensure!(
            ip.is_loopback()
                && ["http", "https"].contains(&url.scheme())
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "nonce endpoint must be literal loopback"
        );
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(1))
            .timeout(Duration::from_secs(3))
            .build()?;
        Ok(Self {
            url,
            client,
            cached: None,
            failed: None,
        })
    }
    fn query(&self, method: &str, params: Value, until: Instant) -> Result<Value> {
        let remaining = until
            .checked_duration_since(Instant::now())
            .context("nonce observation deadline")?;
        let response = self
            .client
            .post(self.url.clone())
            .timeout(remaining)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
            .send()?;
        ensure!(response.status().is_success(), "nonce RPC unavailable");
        let mut raw = vec![];
        response
            .take((RESPONSE_CAP + 1) as u64)
            .read_to_end(&mut raw)?;
        ensure!(
            raw.len() <= RESPONSE_CAP && Instant::now() < until,
            "nonce response budget"
        );
        let value: Value = serde_json::from_slice(&raw)?;
        ensure!(
            value["jsonrpc"] == "2.0" && value["id"] == 1 && value.get("error").is_none(),
            "nonce RPC unavailable"
        );
        value
            .get("result")
            .cloned()
            .context("nonce result unavailable")
    }
    pub(crate) fn observe(
        &mut self,
        account: &str,
        metadata: &PostingMetadata,
        journal: Option<&Rc<RefCell<InputJournal>>>,
    ) -> NonceObservation {
        if !own_metadata(account, metadata) {
            return NonceObservation::unavailable(UnavailableReason::ForeignMetadata);
        }
        let key = journal.and_then(|j| {
            let j = j.borrow();
            j.check().ok()?;
            Some((
                crate::client::fixed(j.hash(j.tip())?).ok()?,
                j.verified_row_digest(j.tip()).ok()?,
            ))
        });
        if let Some(failed) = &self.failed {
            if Some(failed.tip) == key && Instant::now() < failed.retry_at {
                return NonceObservation::unavailable(failed.reason.clone());
            }
        }
        let result = self.observe_inner(account, metadata, journal);
        self.failed = match (&result, key) {
            (
                Err(
                    reason @ (UnavailableReason::ProofUnavailable
                    | UnavailableReason::InvalidProof
                    | UnavailableReason::TipChanged),
                ),
                Some(tip),
            ) => Some(FailedObservation {
                tip,
                retry_at: Instant::now() + Duration::from_secs(1),
                reason: reason.clone(),
            }),
            _ => None,
        };
        result.unwrap_or_else(NonceObservation::unavailable)
    }
    fn observe_inner(
        &mut self,
        account: &str,
        metadata: &PostingMetadata,
        journal: Option<&Rc<RefCell<InputJournal>>>,
    ) -> std::result::Result<NonceObservation, UnavailableReason> {
        use UnavailableReason::*;
        let journal = journal.ok_or(NoJournal)?;
        let (height, hash, row_digest) = {
            let j = journal.borrow();
            j.check().map_err(|_| JournalUnavailable)?;
            (
                j.tip(),
                j.hash(j.tip()).ok_or(JournalUnavailable)?.to_owned(),
                j.verified_row_digest(j.tip())
                    .map_err(|_| JournalUnavailable)?,
            )
        };
        let own = crate::client::fixed::<32>(&hash).map_err(|_| JournalUnavailable)?;
        let until = Instant::now() + Duration::from_secs(3);
        let before = self
            .query("eth_getBlockByNumber", json!(["finalized", false]), until)
            .map_err(|_| ProofUnavailable)?;
        if !same_tip(&before, height, own) {
            return Err(TipChanged);
        }
        let nonce = if let Some((h, d, nonce)) = self
            .cached
            .filter(|(h, d, _)| *h == own && *d == row_digest)
        {
            let _ = (h, d);
            nonce
        } else {
            let row = journal
                .borrow()
                .row(height)
                .map_err(|_| JournalUnavailable)?;
            let raw =
                hex_bytes(&row.header_rlp, "own nonce header").map_err(|_| JournalUnavailable)?;
            if keccak(&raw) != own {
                return Err(JournalUnavailable);
            }
            let decoded = rlp::decode(&raw).map_err(|_| JournalUnavailable)?;
            let fields = decoded.children.ok_or(JournalUnavailable)?;
            let root = fields
                .get(3)
                .and_then(|v| v.bytes.try_into().ok())
                .ok_or(JournalUnavailable)?;
            let proof = self
                .query(
                    "eth_getProof",
                    json!([account,[],{"blockHash":hex_string(&own),"requireCanonical":true}]),
                    until,
                )
                .map_err(|_| ProofUnavailable)?;
            if proof["accountProof"]
                .as_array()
                .is_none_or(|v| v.is_empty())
            {
                return Err(ProofUnavailable);
            }
            verify_account(
                root,
                crate::client::fixed(account).map_err(|_| ForeignMetadata)?,
                &proof,
            )
            .map_err(|_| InvalidProof)?
        };
        let after = self
            .query("eth_getBlockByNumber", json!(["finalized", false]), until)
            .map_err(|_| ProofUnavailable)?;
        if !same_tip(&after, height, own) {
            return Err(TipChanged);
        }
        {
            let j = journal.borrow();
            j.check().map_err(|_| JournalUnavailable)?;
            if j.tip() != height
                || j.hash(height) != Some(hash.as_str())
                || j.verified_row_digest(height)
                    .map_err(|_| JournalUnavailable)?
                    != row_digest
            {
                return Err(JournalUnavailable);
            }
        }
        self.cached = Some((own, row_digest, nonce));
        Ok(NonceObservation {
            state: if nonce > metadata.nonce {
                NonceState::NonceConsumed
            } else {
                NonceState::NotAdvanced
            },
            inclusion: Inclusion::Unresolved,
            finalized_height: Some(height),
            finalized_hash: Some(hash),
            account_nonce: Some(nonce),
            reason: None,
        })
    }
}
fn own_metadata(account: &str, metadata: &PostingMetadata) -> bool {
    metadata.version == 1
        && metadata.chain_id == boole_derivation::config::L1_CHAIN_ID
        && metadata.sender.eq_ignore_ascii_case(account)
        && matches!(metadata.kind, 2 | 3 | 4 | 7)
        && crate::client::fixed::<32>(&metadata.transaction_hash).is_ok()
        && crate::client::fixed::<32>(&metadata.body_digest).is_ok()
}
fn same_tip(value: &Value, height: u64, hash: [u8; 32]) -> bool {
    value["number"]
        .as_str()
        .and_then(|s| s.strip_prefix("0x"))
        .and_then(|s| u64::from_str_radix(s, 16).ok())
        == Some(height)
        && value["hash"]
            .as_str()
            .and_then(|s| crate::client::fixed::<32>(s).ok())
            == Some(hash)
}
fn nibbles(bytes: &[u8]) -> Vec<u8> {
    bytes.iter().flat_map(|b| [b >> 4, b & 15]).collect()
}
fn reference(raw: &[u8], item: &rlp::Item<'_>) -> Result<Vec<u8>> {
    if item.children.is_some() {
        let encoded = &raw[item.span.clone()];
        ensure!(encoded.len() < 32, "oversized embedded trie node");
        Ok(encoded.to_vec())
    } else {
        ensure!(item.bytes.len() == 32, "missing or invalid trie reference");
        Ok(item.bytes.to_vec())
    }
}
/// Verify membership at keccak(address), independent of RPC-declared nonce.
fn verify_account(root: [u8; 32], address: [u8; 20], proof: &Value) -> Result<u64> {
    ensure!(
        proof["address"]
            .as_str()
            .and_then(|s| crate::client::fixed::<20>(s).ok())
            == Some(address),
        "proof address"
    );
    let rows = proof["accountProof"]
        .as_array()
        .context("missing account proof")?;
    ensure!(
        !rows.is_empty() && rows.len() <= 65,
        "account proof node cap"
    );
    let mut total = 0usize;
    let mut nodes = BTreeMap::new();
    for row in rows {
        let bytes = hex_bytes(
            row.as_str().context("account proof node")?,
            "account proof node",
        )?;
        total = total
            .checked_add(bytes.len())
            .context("account proof size")?;
        ensure!(
            !bytes.is_empty() && bytes.len() <= 4096 && total <= PROOF_CAP,
            "account proof byte cap"
        );
        nodes.insert(keccak(&bytes), bytes);
    }
    let key = nibbles(&keccak(&address));
    let mut at = 0;
    let mut link = root.to_vec();
    let mut account = None;
    for _ in 0..65 {
        let raw = if link.len() == 32 {
            nodes
                .get(&<[u8; 32]>::try_from(link.as_slice()).expect("hash"))
                .context("missing linked trie node")?
                .clone()
        } else {
            ensure!(!link.is_empty() && link.len() < 32, "trie link width");
            link.clone()
        };
        let node = rlp::decode(&raw)
            .map_err(anyhow::Error::msg)?
            .children
            .context("trie node list")?;
        match node.len() {
            17 => {
                if at == key.len() {
                    ensure!(
                        node[16].children.is_none() && !node[16].bytes.is_empty(),
                        "missing branch account"
                    );
                    account = Some(node[16].bytes.to_vec());
                    break;
                }
                link = reference(&raw, &node[usize::from(key[at])])?;
                at += 1;
            }
            2 => {
                ensure!(
                    node[0].children.is_none() && !node[0].bytes.is_empty(),
                    "compact trie path"
                );
                let encoded = nibbles(node[0].bytes);
                let flag = encoded[0];
                ensure!(flag <= 3, "compact path flag");
                let skip = if flag & 1 == 0 {
                    ensure!(encoded[1] == 0, "compact path padding");
                    2
                } else {
                    1
                };
                let path = &encoded[skip..];
                ensure!(
                    key.get(at..at + path.len()) == Some(path),
                    "foreign account proof path"
                );
                at += path.len();
                if flag >= 2 {
                    ensure!(
                        at == key.len() && node[1].children.is_none(),
                        "incomplete account leaf"
                    );
                    account = Some(node[1].bytes.to_vec());
                    break;
                }
                ensure!(
                    !path.is_empty() && at < key.len(),
                    "empty/exhausted trie extension"
                );
                link = reference(&raw, &node[1])?;
            }
            _ => anyhow::bail!("account trie node shape"),
        }
    }
    let value = account.context("incomplete account proof")?;
    let decoded = rlp::decode(&value)
        .map_err(anyhow::Error::msg)?
        .children
        .context("account value list")?;
    ensure!(
        decoded.len() == 4 && decoded.iter().all(|v| v.children.is_none()),
        "account field shape"
    );
    let nonce = crate::posting::integer(&decoded[0])?;
    ensure!(
        decoded[1].bytes.len() <= 32
            && decoded[1].bytes.first() != Some(&0)
            && decoded[2].bytes.len() == 32
            && decoded[3].bytes.len() == 32,
        "account field widths"
    );
    ensure!(
        proof["nonce"] == format!("0x{nonce:x}")
            && boole_devnet_archive::input::quantity_bytes(&proof["balance"])? == decoded[1].bytes
            && proof["storageHash"]
                .as_str()
                .and_then(|s| crate::client::fixed::<32>(s).ok())
                .as_ref()
                .map(|v| v.as_slice())
                == Some(decoded[2].bytes)
            && proof["codeHash"]
                .as_str()
                .and_then(|s| crate::client::fixed::<32>(s).ok())
                .as_ref()
                .map(|v| v.as_slice())
                == Some(decoded[3].bytes)
            && proof["storageProof"]
                .as_array()
                .is_some_and(|v| v.is_empty()),
        "RPC account claims differ from proven account"
    );
    Ok(nonce)
}

#[cfg(test)]
mod tests {
    #[test]
    fn t79_pinned_native_proof_membership() -> anyhow::Result<()> {
        let value: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../fixtures/devnet-mcp/nonce-account-proof-mined-v1.json"
        ))?;
        let root = crate::client::fixed(value["header"]["stateRoot"].as_str().unwrap())?;
        let address = crate::client::fixed(value["proof"]["address"].as_str().unwrap())?;
        assert_eq!(super::verify_account(root, address, &value["proof"])?, 0);
        let genesis: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../fixtures/devnet-mcp/nonce-account-proof-genesis-mismatch-v1.json"
        ))?;
        let genesis_root = crate::client::fixed(genesis["header"]["stateRoot"].as_str().unwrap())?;
        assert!(
            super::verify_account(genesis_root, address, &genesis["proof"]).is_err(),
            "do not accept the captured genesis proof whose root differs"
        );
        Ok(())
    }
}
