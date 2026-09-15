//! Hash-only native test-coin blocks. Legacy PersistedBlock/v3 is untouched.
//! Every accepted block independently proves PoW, producer authorization and
//! the network-bound transfers whose accounting it changes.

use num_bigint::BigUint;
use num_traits::Zero;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::native_ledger::{
    validate_native_transfer, NativeLedger, NativeTransferFields, NativeTransferPayload,
};
use crate::native_network::{native_testnet, NativeNetwork};
use crate::signed_envelope::{SignedEnvelope, SIGNED_ENVELOPE_SCHEMA};
use crate::{canonicalize, difficulty_weight, Hex32};

const BLOCK_SCHEMA: &str = "boole.native.block.v1";
const BLOCK_AUTH_SCHEMA: &str = "boole.native.block.authorization.v1";

/// The existing signed-envelope wire spelling, with a mandatory network and
/// a typed payload so duplicate/unknown payload fields are rejected by serde.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeTransfer {
    pub schema: String,
    pub payload: NativeTransferPayload,
    pub pk: String,
    pub signature: String,
    pub network_id: String,
}

impl TryFrom<&SignedEnvelope> for NativeTransfer {
    type Error = anyhow::Error;

    fn try_from(envelope: &SignedEnvelope) -> anyhow::Result<Self> {
        anyhow::ensure!(
            envelope.schema == SIGNED_ENVELOPE_SCHEMA,
            "invalid envelope schema"
        );
        Ok(Self {
            schema: envelope.schema.to_string(),
            payload: serde_json::from_value(envelope.payload.clone())?,
            pk: envelope.pk.clone(),
            signature: envelope.signature.clone(),
            network_id: envelope
                .network_id
                .clone()
                .ok_or_else(|| anyhow::anyhow!("network required"))?,
        })
    }
}

impl NativeTransfer {
    /// Verify immutable wire/signature/fee rules without requiring a funded
    /// account. Durable pending-state recovery uses this before dropping rows
    /// made stale by a legitimate block or reorganization.
    pub fn validated_fields(&self) -> anyhow::Result<NativeTransferFields> {
        let network = native_testnet();
        anyhow::ensure!(
            serde_json::to_vec(self)?.len() <= network.max_transfer_bytes(),
            "transfer too large"
        );
        Ok(validate_native_transfer(
            &self.envelope()?,
            network.network_id(),
            network.minimum_fee(),
        )?)
    }
    pub fn envelope(&self) -> anyhow::Result<SignedEnvelope> {
        anyhow::ensure!(
            self.schema == SIGNED_ENVELOPE_SCHEMA,
            "invalid envelope schema"
        );
        Ok(SignedEnvelope {
            schema: SIGNED_ENVELOPE_SCHEMA,
            payload: serde_json::to_value(&self.payload)?,
            pk: self.pk.clone(),
            signature: self.signature.clone(),
            network_id: Some(self.network_id.clone()),
        })
    }

    pub fn id(&self) -> Hex32 {
        let mut hash = blake3::Hasher::new();
        hash.update(b"boole.native.transfer.id.v1\0");
        hash.update(&canonicalize(
            &serde_json::to_value(self).expect("native transfer"),
        ));
        Hex32::from_bytes(*hash.finalize().as_bytes())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeBlockHeader {
    pub schema: String,
    pub network_id: String,
    pub genesis_hash: String,
    pub height: u64,
    pub previous_hash: String,
    pub producer_pk: String,
    pub reward_pk: String,
    pub timestamp_ms: u64,
    pub target: String,
    pub transfers_root: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeBlock {
    pub header: NativeBlockHeader,
    /// Canonical decimal u64 text: a JSON client must not round nonce bits.
    pub nonce: String,
    pub transfers: Vec<NativeTransfer>,
    pub producer_signature: String,
}

fn header_hasher(header: &NativeBlockHeader) -> blake3::Hasher {
    let mut hash = blake3::Hasher::new();
    hash.update(b"boole.native.block.v1\0");
    hash.update(&canonicalize(
        &serde_json::to_value(header).expect("native header"),
    ));
    hash
}

fn hash_nonce(prefix: &blake3::Hasher, nonce: u64) -> Hex32 {
    let mut hash = prefix.clone();
    hash.update(&nonce.to_le_bytes());
    Hex32::from_bytes(*hash.finalize().as_bytes())
}

fn transfers_root(transfers: &[NativeTransfer]) -> String {
    let mut hash = blake3::Hasher::new();
    hash.update(b"boole.native.transfers.v1\0");
    hash.update(&canonicalize(
        &serde_json::to_value(transfers).expect("native transfers"),
    ));
    Hex32::from_bytes(*hash.finalize().as_bytes()).to_hex()
}

impl NativeBlock {
    pub fn hash(&self) -> anyhow::Result<Hex32> {
        let nonce = self.nonce.parse::<u64>()?;
        anyhow::ensure!(nonce.to_string() == self.nonce, "noncanonical block nonce");
        Ok(hash_nonce(&header_hasher(&self.header), nonce))
    }

    pub fn authorization_payload(&self) -> anyhow::Result<Value> {
        Ok(json!({"schema": BLOCK_AUTH_SCHEMA, "blockHash": self.hash()?.to_hex()}))
    }

    /// Attach an externally produced owner/miner signature; the node never
    /// needs custody of either the producer or the cold reward key.
    pub fn authorize(mut self, envelope: &SignedEnvelope) -> anyhow::Result<Self> {
        anyhow::ensure!(
            envelope.schema == SIGNED_ENVELOPE_SCHEMA
                && envelope.network_id.as_deref() == Some(self.header.network_id.as_str())
                && envelope.pk == self.header.producer_pk
                && envelope.payload == self.authorization_payload()?,
            "producer authorization does not match the block"
        );
        anyhow::ensure!(
            envelope.verify_strict().map_err(anyhow::Error::msg)?,
            "invalid producer signature"
        );
        self.producer_signature = envelope.signature.clone();
        Ok(self)
    }

    fn verify_authorization(&self) -> anyhow::Result<()> {
        self.clone().authorize(&SignedEnvelope {
            schema: SIGNED_ENVELOPE_SCHEMA,
            payload: self.authorization_payload()?,
            pk: self.header.producer_pk.clone(),
            signature: self.producer_signature.clone(),
            network_id: Some(self.header.network_id.clone()),
        })?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeBlockTemplate {
    pub header: NativeBlockHeader,
    pub transfers: Vec<NativeTransfer>,
}

impl NativeBlockTemplate {
    /// Bounded actual BLAKE3 work. The returned block is unsigned until the
    /// producer calls `authorize`; no node state changes during mining.
    pub fn mine(&self, start_nonce: u64, attempts: u64) -> anyhow::Result<Option<NativeBlock>> {
        let target = Hex32::from_hex(&self.header.target)?;
        let prefix = header_hasher(&self.header);
        for offset in 0..attempts {
            let Some(nonce) = start_nonce.checked_add(offset) else {
                break;
            };
            if hash_nonce(&prefix, nonce) < target {
                return Ok(Some(NativeBlock {
                    header: self.header.clone(),
                    nonce: nonce.to_string(),
                    transfers: self.transfers.clone(),
                    producer_signature: String::new(),
                }));
            }
        }
        Ok(None)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeChain {
    network: NativeNetwork,
    blocks: Vec<NativeBlock>,
    ledger: NativeLedger,
    cumulative_work: BigUint,
}

/// A fully verified successor ready for durable publication. Private fields
/// prevent a storage caller from constructing a claimed balance transition.
#[derive(Debug)]
pub struct PreparedNativeBlock {
    block: NativeBlock,
    ledger: NativeLedger,
    cumulative_work: BigUint,
}

impl PreparedNativeBlock {
    pub fn block(&self) -> &NativeBlock {
        &self.block
    }
}

impl NativeChain {
    pub fn new() -> anyhow::Result<Self> {
        let network = native_testnet();
        Ok(Self {
            ledger: network.ledger()?,
            network,
            blocks: Vec::new(),
            cumulative_work: BigUint::zero(),
        })
    }

    pub fn replay(blocks: &[NativeBlock]) -> anyhow::Result<Self> {
        let mut chain = Self::new()?;
        for block in blocks {
            chain.append(block.clone())?;
        }
        Ok(chain)
    }

    pub fn ledger(&self) -> &NativeLedger {
        &self.ledger
    }

    pub fn blocks(&self) -> &[NativeBlock] {
        &self.blocks
    }

    pub fn cumulative_work(&self) -> &BigUint {
        &self.cumulative_work
    }

    /// Same work ordering and lowest-hash tie break as the legacy chain,
    /// but every weight here is derived from validated native block targets.
    pub fn outranks(&self, other: &Self) -> bool {
        self.cumulative_work > other.cumulative_work
            || (self.cumulative_work == other.cumulative_work
                && self.head_hash() < other.head_hash())
    }

    pub fn head_hash(&self) -> Hex32 {
        self.blocks
            .last()
            .map(|block| block.hash().expect("verified block"))
            .unwrap_or_else(|| self.network.genesis_hash())
    }

    pub fn next_target(&self) -> anyhow::Result<String> {
        let Some(last) = self.blocks.last() else {
            return Ok(self.network.initial_target().to_string());
        };
        let policy = self.network.retarget();
        let completed = self.ledger.height();
        if completed < policy.retarget_every_blocks
            || !completed.is_multiple_of(policy.retarget_every_blocks)
        {
            return Ok(last.header.target.clone());
        }
        let window_len = usize::try_from(policy.retarget_every_blocks)?;
        let first = &self.blocks[self.blocks.len() - window_len];
        let current = BigUint::from_bytes_be(Hex32::from_hex(&last.header.target)?.as_bytes());
        let actual = last
            .header
            .timestamp_ms
            .saturating_sub(first.header.timestamp_ms)
            .max(1);
        let expected = policy
            .target_block_ms
            .checked_mul(policy.retarget_every_blocks - 1)
            .ok_or_else(|| anyhow::anyhow!("retarget duration overflow"))?;
        let target = crate::retarget_t_block(&current, actual, expected, policy)?;
        let limit =
            BigUint::from_bytes_be(Hex32::from_hex(self.network.initial_target())?.as_bytes());
        Ok(format!("{:064x}", target.min(limit)))
    }

    pub fn minimum_timestamp_ms(&self) -> anyhow::Result<u64> {
        let start = self
            .blocks
            .len()
            .saturating_sub(self.network.median_time_past_window());
        let mut timestamps: Vec<u64> = self.blocks[start..]
            .iter()
            .map(|block| block.header.timestamp_ms)
            .collect();
        timestamps.sort_unstable();
        let median = timestamps.get(timestamps.len() / 2).copied().unwrap_or(0);
        median
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("timestamp overflow"))
    }

    pub fn template(
        &self,
        producer_pk: &str,
        reward_pk: &str,
        timestamp_ms: u64,
        transfers: &[NativeTransfer],
    ) -> anyhow::Result<NativeBlockTemplate> {
        anyhow::ensure!(
            timestamp_ms >= self.minimum_timestamp_ms()?,
            "timestamp does not exceed median-time-past"
        );
        Hex32::from_hex(producer_pk)?;
        Hex32::from_hex(reward_pk)?;
        let height = self
            .ledger
            .height()
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("height overflow"))?;
        let envelopes = self.transfer_envelopes(transfers)?;
        self.ledger
            .clone()
            .apply_block(height, reward_pk, &envelopes)?;
        Ok(NativeBlockTemplate {
            header: NativeBlockHeader {
                schema: BLOCK_SCHEMA.to_string(),
                network_id: self.network.network_id().to_string(),
                genesis_hash: self.network.genesis_hash().to_hex(),
                height,
                previous_hash: self.head_hash().to_hex(),
                producer_pk: producer_pk.to_string(),
                reward_pk: reward_pk.to_string(),
                timestamp_ms,
                target: self.next_target()?,
                transfers_root: transfers_root(transfers),
            },
            transfers: transfers.to_vec(),
        })
    }

    fn transfer_envelopes(
        &self,
        transfers: &[NativeTransfer],
    ) -> anyhow::Result<Vec<SignedEnvelope>> {
        anyhow::ensure!(
            transfers.len() <= self.network.max_transfers_per_block(),
            "too many transfers"
        );
        transfers
            .iter()
            .map(|transfer| {
                anyhow::ensure!(
                    serde_json::to_vec(transfer)?.len() <= self.network.max_transfer_bytes(),
                    "transfer too large"
                );
                transfer.envelope()
            })
            .collect()
    }

    pub fn append(&mut self, block: NativeBlock) -> anyhow::Result<()> {
        let prepared = self.prepare(block)?;
        self.commit(prepared)
    }

    /// Validate without changing this chain or copying its full block history.
    /// Storage publishes the verified block before calling `commit`.
    pub fn prepare(&self, block: NativeBlock) -> anyhow::Result<PreparedNativeBlock> {
        anyhow::ensure!(
            serde_json::to_vec(&block)?.len() <= self.network.max_block_bytes(),
            "block too large"
        );
        let header = &block.header;
        anyhow::ensure!(
            header.schema == BLOCK_SCHEMA
                && header.network_id == self.network.network_id()
                && header.genesis_hash == self.network.genesis_hash().to_hex(),
            "foreign native genesis or block schema"
        );
        anyhow::ensure!(
            header.height
                == self
                    .ledger
                    .height()
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("height overflow"))?
                && header.previous_hash == self.head_hash().to_hex(),
            "native block does not extend this head"
        );
        Hex32::from_hex(&header.producer_pk)?;
        Hex32::from_hex(&header.reward_pk)?;
        anyhow::ensure!(
            header.timestamp_ms >= self.minimum_timestamp_ms()?,
            "timestamp does not exceed median-time-past"
        );
        anyhow::ensure!(
            header.target == self.next_target()?,
            "unexpected native block target"
        );
        let target = Hex32::from_hex(&header.target)?;
        anyhow::ensure!(block.hash()? < target, "insufficient native block work");
        anyhow::ensure!(
            header.transfers_root == transfers_root(&block.transfers),
            "transfer commitment mismatch"
        );
        block.verify_authorization()?;
        let envelopes = self.transfer_envelopes(&block.transfers)?;
        let work = difficulty_weight(&BigUint::from_bytes_be(target.as_bytes()))?;
        let mut ledger = self.ledger.clone();
        ledger.apply_block(header.height, &header.reward_pk, &envelopes)?;
        Ok(PreparedNativeBlock {
            block,
            ledger,
            cumulative_work: &self.cumulative_work + work,
        })
    }

    pub fn commit(&mut self, prepared: PreparedNativeBlock) -> anyhow::Result<()> {
        anyhow::ensure!(
            prepared.block.header.previous_hash == self.head_hash().to_hex(),
            "prepared native block is stale"
        );
        self.ledger = prepared.ledger;
        self.cumulative_work = prepared.cumulative_work;
        self.blocks.push(prepared.block);
        Ok(())
    }
}
