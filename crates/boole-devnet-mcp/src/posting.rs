//! Own posting evidence. These records never decide a consensus outcome.
use anyhow::{ensure, Context, Result};
use boole_derivation::{
    attributes::{hex_bytes, hex_string},
    rlp,
    user_tx::{filter_user, keccak},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostingMetadata {
    pub version: u8,
    pub sender: String,
    pub chain_id: u64,
    pub nonce: u64,
    pub transaction_hash: String,
    pub body_digest: String,
    pub kind: u8,
}

/// The signed bytes stay private until broadcast and are never a tool response.
pub struct SignedPosting {
    pub metadata: PostingMetadata,
    pub(crate) raw: Vec<u8>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredPosting {
    metadata: PostingMetadata,
    signed_transaction: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SendState {
    Intent,
    Prepared,
    Acknowledged,
    Ambiguous,
    NotSent,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StageError {
    PostingRecordDamaged,
}
/// Local retry policy, never an inclusion or current account-state claim.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RebroadcastStop {
    NonceObservedConsumed,
}

pub(crate) fn integer(item: &rlp::Item<'_>) -> Result<u64> {
    ensure!(
        item.children.is_none() && item.bytes.len() <= 8 && item.bytes.first() != Some(&0),
        "posting integer"
    );
    Ok(item.bytes.iter().fold(0, |n, b| (n << 8) | u64::from(*b)))
}

impl SignedPosting {
    fn name(metadata: &PostingMetadata) -> String {
        format!(
            "posting-{}.json",
            metadata.transaction_hash.trim_start_matches("0x")
        )
    }
    pub fn persist(&self, store: &crate::store::Store) -> Result<()> {
        let name = Self::name(&self.metadata);
        if store.path.join(&name).exists() {
            let old = Self::load(store, &self.metadata)?;
            ensure!(old.raw == self.raw, "own signed posting changed");
            std::fs::File::open(store.path.join(&name))?.sync_all()?;
            std::fs::File::open(&store.path)?.sync_all()?;
            return Ok(());
        }
        // Store holds the exclusive client lock. Both the private file and its
        // parent directory are fsynced before this method returns.
        store.write(
            &name,
            &StoredPosting {
                metadata: self.metadata.clone(),
                signed_transaction: hex_string(&self.raw),
            },
        )
    }
    pub(crate) fn load(store: &crate::store::Store, metadata: &PostingMetadata) -> Result<Self> {
        let _: [u8; 32] = crate::client::fixed(&metadata.transaction_hash)?;
        let saved: StoredPosting = store.read(&Self::name(metadata))?;
        ensure!(saved.metadata == *metadata, "own posting reference changed");
        let raw = hex_bytes(&saved.signed_transaction, "own signed posting")?;
        ensure!(raw.first() == Some(&2), "own signed posting type");
        let fields = rlp::decode(&raw[1..])
            .map_err(anyhow::Error::msg)?
            .children
            .context("own posting fields")?;
        let data = fields.get(7).context("own posting calldata")?.bytes;
        ensure!(
            data.get(..4)
                == Some(hex_bytes(boole_derivation::config::POST_SELECTOR, "selector")?.as_slice()),
            "own posting selector"
        );
        let (kind, body) = boole_derivation::inbox::item(&data[4..]).map_err(anyhow::Error::msg)?;
        let body = body.to_vec();
        let fee = fields.get(3).context("own posting fee")?.bytes.to_vec();
        let signed = Self::authenticate(raw, kind, &body, &metadata.sender, &fee)?;
        ensure!(
            signed.metadata == *metadata,
            "own posting metadata differs from signed bytes"
        );
        Ok(signed)
    }
    pub(crate) fn authenticate(
        raw: Vec<u8>,
        kind: u8,
        body: &[u8],
        sender: &str,
        max_fee: &[u8],
    ) -> Result<Self> {
        ensure!(
            raw.len() <= 1024 * 1024 && raw.first() == Some(&2),
            "posting signed transaction type/cap"
        );
        ensure!(matches!(kind, 2 | 3 | 4 | 7), "posting kind");
        let mut budget = u64::MAX;
        let decision = filter_user(&raw, &mut budget);
        ensure!(
            decision.forward
                && decision
                    .sender
                    .as_deref()
                    .is_some_and(|a| a.eq_ignore_ascii_case(sender)),
            "posting signed sender"
        );
        let fields = rlp::decode(&raw[1..])
            .map_err(anyhow::Error::msg)?
            .children
            .context("posting transaction list")?;
        let chain_id = integer(&fields[0])?;
        let nonce = integer(&fields[1])?;
        ensure!(
            fields[2].children.is_none()
                && fields[2].bytes.is_empty()
                && fields[3].children.is_none()
                && fields[3].bytes.len() <= 32
                && fields[3].bytes.first() != Some(&0)
                && fields[3].bytes == max_fee,
            "posting signed fees differ from requested zero priority / maximum fee"
        );
        ensure!(
            chain_id == boole_derivation::config::L1_CHAIN_ID,
            "posting chain id"
        );
        ensure!(
            integer(&fields[4])? == if kind == 4 { 5_100_000 } else { 1_000_000 },
            "posting gas"
        );
        ensure!(
            fields[5].bytes == hex_bytes(boole_derivation::config::INBOX, "inbox")?
                && fields[6].bytes.is_empty()
                && fields[7].bytes == crate::wire::calldata(kind, body)
                && fields[8].children.as_ref().is_some_and(|v| v.is_empty()),
            "posting signed inbox/body/value/access list"
        );
        let metadata = PostingMetadata {
            version: 1,
            sender: decision.sender.context("posting sender")?,
            chain_id,
            nonce,
            transaction_hash: hex_string(&keccak(&raw)),
            body_digest: hex_string(&crate::codec::digest(body)),
            kind,
        };
        Ok(Self { metadata, raw })
    }
}

#[derive(Debug)]
pub struct PostingNotSent;
impl std::fmt::Display for PostingNotSent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("posting preparation or durable intent unavailable; no send attempted")
    }
}
impl std::error::Error for PostingNotSent {}
#[derive(Debug)]
pub struct PostingAmbiguous;
impl std::fmt::Display for PostingAmbiguous {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("posting send outcome unknown; exact assigned hash retained")
    }
}
impl std::error::Error for PostingAmbiguous {}
