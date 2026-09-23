//! Native-coin accounting for the explicitly versioned native testnet.
//!
//! This module does not reinterpret v3 blocks or credit ledgers. Its native-chain
//! caller must validate block identity, linkage, PoW and reward
//! authorization before applying its accounting inputs. Monetary values passed
//! here are immutable per ledger, not a runtime configuration mechanism for an
//! existing network. There is no import path for legacy development credits.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    signed_envelope::{SignedEnvelope, SIGNED_ENVELOPE_SCHEMA},
    Hex32,
};

pub const NATIVE_TRANSFER_SCHEMA: &str = "boole.transfer.v1";

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NativeLedgerError {
    #[error("native ledger: invalid emission schedule")]
    InvalidSchedule,
    #[error("native ledger: invalid network id")]
    InvalidNetwork,
    #[error("native ledger: invalid public key")]
    InvalidPublicKey,
    #[error("native ledger: expected height {expected}, got {actual}")]
    UnexpectedHeight { expected: u64, actual: u64 },
    #[error("native ledger: arithmetic overflow")]
    Overflow,
    #[error("native ledger: malformed transfer")]
    InvalidTransfer,
    #[error("native ledger: transfer signature is invalid")]
    InvalidSignature,
    #[error("native ledger: transfer is not bound to this network")]
    WrongNetwork,
    #[error("native ledger: transfer signer is not the sender")]
    SenderMismatch,
    #[error("native ledger: amount must be nonzero")]
    ZeroAmount,
    #[error("native ledger: fee is below the network minimum")]
    FeeBelowMinimum,
    #[error("native ledger: transfer expired")]
    Expired,
    #[error("native ledger: expected nonce {expected}, got {actual}")]
    UnexpectedNonce { expected: u64, actual: u64 },
    #[error("native ledger: insufficient balance for amount plus fee")]
    InsufficientBalance,
}

/// Signed payload integers are canonical decimal strings, including nonce and
/// height, so clients never round u128/u64 values through JSON floating point.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeTransferPayload {
    pub schema: String,
    pub from: String,
    pub to: String,
    pub amount: String,
    pub fee: String,
    pub nonce: String,
    pub valid_before: String,
}

/// Parsed fields for admission and account views. This is not an authority to
/// debit: `apply_block` always re-verifies the actual signed envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeTransferFields {
    pub from: String,
    pub to: String,
    pub amount: u128,
    pub fee: u128,
    pub nonce: u64,
    pub valid_before: u64,
}

pub(crate) fn validate_native_transfer(
    envelope: &SignedEnvelope,
    network_id: &str,
    minimum_fee: u128,
) -> Result<NativeTransferFields, NativeLedgerError> {
    if envelope.schema != SIGNED_ENVELOPE_SCHEMA {
        return Err(NativeLedgerError::InvalidTransfer);
    }
    if envelope.network_id.as_deref() != Some(network_id) {
        return Err(NativeLedgerError::WrongNetwork);
    }
    let payload: NativeTransferPayload = serde_json::from_value(envelope.payload.clone())
        .map_err(|_| NativeLedgerError::InvalidTransfer)?;
    if payload.schema != NATIVE_TRANSFER_SCHEMA {
        return Err(NativeLedgerError::InvalidTransfer);
    }
    if payload.from != envelope.pk {
        return Err(NativeLedgerError::SenderMismatch);
    }
    for pk in [&payload.from, &payload.to] {
        Hex32::from_hex(pk).map_err(|_| NativeLedgerError::InvalidPublicKey)?;
    }
    if !envelope
        .verify_strict()
        .map_err(|_| NativeLedgerError::InvalidSignature)?
    {
        return Err(NativeLedgerError::InvalidSignature);
    }
    let fields = NativeTransferFields {
        from: payload.from,
        to: payload.to,
        amount: decimal(&payload.amount)?,
        fee: decimal(&payload.fee)?,
        nonce: decimal(&payload.nonce)?,
        valid_before: decimal(&payload.valid_before)?,
    };
    if fields.amount == 0 {
        return Err(NativeLedgerError::ZeroAmount);
    }
    if fields.fee < minimum_fee {
        return Err(NativeLedgerError::FeeBelowMinimum);
    }
    Ok(fields)
}

fn decimal<T: std::str::FromStr>(value: &str) -> Result<T, NativeLedgerError> {
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(NativeLedgerError::InvalidTransfer);
    }
    value
        .parse()
        .map_err(|_| NativeLedgerError::InvalidTransfer)
}

/// Pure schedule inputs. No production or public-testnet values are chosen here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmissionSchedule {
    total_supply: u128,
    initial_reward: u128,
    halving_epoch: u64,
}

impl EmissionSchedule {
    pub fn new(
        total_supply: u128,
        initial_reward: u128,
        halving_epoch: u64,
    ) -> Result<Self, NativeLedgerError> {
        if total_supply == 0 || initial_reward == 0 || halving_epoch == 0 {
            return Err(NativeLedgerError::InvalidSchedule);
        }
        Ok(Self {
            total_supply,
            initial_reward,
            halving_epoch,
        })
    }

    fn emission(self, height: u64, issued: u128) -> Result<u128, NativeLedgerError> {
        let epoch = height / self.halving_epoch;
        let reward = if epoch < u128::BITS as u64 {
            self.initial_reward >> epoch
        } else {
            0
        };
        let remaining = self
            .total_supply
            .checked_sub(issued)
            .ok_or(NativeLedgerError::Overflow)?;
        Ok(reward.min(remaining))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeLedger {
    network_id: String,
    schedule: EmissionSchedule,
    height: u64,
    issued: u128,
    balances: BTreeMap<String, u128>,
    next_nonces: BTreeMap<String, u64>,
    minimum_fee: u128,
    reward_maturity: u64,
    locked_balances: BTreeMap<String, u128>,
    pending_rewards: BTreeMap<u64, (String, u128)>,
}

/// In-process inverse of one verified transition. There is no deserialization
/// or external construction path; only NativeChain retains these values.
#[derive(Debug, Clone)]
pub(crate) struct NativeLedgerUndo {
    height: u64,
    issued: u128,
    balances: BTreeMap<String, Option<u128>>,
    next_nonces: BTreeMap<String, Option<u64>>,
    locked_balances: BTreeMap<String, Option<u128>>,
    pending_rewards: BTreeMap<u64, Option<(String, u128)>>,
}

impl NativeLedgerUndo {
    fn new(ledger: &NativeLedger) -> Self {
        Self {
            height: ledger.height,
            issued: ledger.issued,
            balances: BTreeMap::new(),
            next_nonces: BTreeMap::new(),
            locked_balances: BTreeMap::new(),
            pending_rewards: BTreeMap::new(),
        }
    }

    fn balance(&mut self, ledger: &NativeLedger, pk: &str) {
        self.balances
            .entry(pk.to_string())
            .or_insert_with(|| ledger.balances.get(pk).copied());
    }

    fn locked_balance(&mut self, ledger: &NativeLedger, pk: &str) {
        self.locked_balances
            .entry(pk.to_string())
            .or_insert_with(|| ledger.locked_balances.get(pk).copied());
    }
}

fn restore_entries<K: Ord, V>(map: &mut BTreeMap<K, V>, old: BTreeMap<K, Option<V>>) {
    for (key, value) in old {
        match value {
            Some(value) => {
                map.insert(key, value);
            }
            None => {
                map.remove(&key);
            }
        }
    }
}

/// Non-authoritative reservations for the next block. It cannot be converted
/// into a canonical ledger; fees are reserved, not credited to an unknown
/// future producer, and no new block reward is created.
#[derive(Debug, Clone)]
pub struct NativePendingView {
    ledger: NativeLedger,
    height: u64,
}

/// A verified reservation exclusively borrows its originating view until it is
/// committed or dropped. It cannot be applied to a different/stale view, cloned,
/// deserialized, or used to publish canonical state. Dropping it does nothing.
#[derive(Debug)]
#[must_use = "commit after durable publication, or drop to leave reservations unchanged"]
pub struct NativePendingTransfer<'a> {
    view: &'a mut NativePendingView,
    prepared: PreparedTransfer,
}

impl NativePendingTransfer<'_> {
    /// All fallible accounting and signature checks ran during preparation.
    pub fn commit(self) {
        self.view.ledger.commit_transfer(self.prepared);
    }
}

#[derive(Debug)]
struct PreparedTransfer {
    balances: BTreeMap<String, u128>,
    sender: String,
    next_nonce: u64,
}

impl NativePendingView {
    pub fn push(&mut self, envelope: &SignedEnvelope) -> Result<(), NativeLedgerError> {
        self.prepare(envelope)?.commit();
        Ok(())
    }

    /// Prepare only the affected accounts; the rest of the ledger is neither
    /// copied nor mutated. The exclusive borrow prevents intervening changes.
    pub fn prepare(
        &mut self,
        envelope: &SignedEnvelope,
    ) -> Result<NativePendingTransfer<'_>, NativeLedgerError> {
        let prepared = self.ledger.prepare_transfer(self.height, None, envelope)?;
        Ok(NativePendingTransfer {
            view: self,
            prepared,
        })
    }

    pub fn available_balance(&self, pk: &str) -> u128 {
        self.ledger.spendable_balance(pk)
    }

    pub fn next_nonce(&self, pk: &str) -> u64 {
        self.ledger.next_nonce(pk)
    }
}

impl NativeLedger {
    /// Genesis has height zero, no issuance and no preallocated balances.
    pub fn new(network_id: &str, schedule: EmissionSchedule) -> Result<Self, NativeLedgerError> {
        Self::new_with_rules(network_id, schedule, 0, 0)
    }

    /// Construct an immutable accounting policy. Named-network callers must
    /// obtain these rules from their code-pinned genesis, not runtime knobs.
    pub fn new_with_rules(
        network_id: &str,
        schedule: EmissionSchedule,
        minimum_fee: u128,
        reward_maturity: u64,
    ) -> Result<Self, NativeLedgerError> {
        if network_id.is_empty()
            || network_id.len() > 128
            || !network_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        {
            return Err(NativeLedgerError::InvalidNetwork);
        }
        Ok(Self {
            network_id: network_id.to_string(),
            schedule,
            height: 0,
            issued: 0,
            balances: BTreeMap::new(),
            next_nonces: BTreeMap::new(),
            minimum_fee,
            reward_maturity,
            locked_balances: BTreeMap::new(),
            pending_rewards: BTreeMap::new(),
        })
    }

    pub fn balance(&self, pk: &str) -> u128 {
        self.balances.get(pk).copied().unwrap_or(0)
    }

    /// Balance available at the current applied height. A reward maturing in
    /// the next block becomes available when that block's transition begins.
    pub fn spendable_balance(&self, pk: &str) -> u128 {
        self.balance(pk) - self.locked_balance(pk)
    }

    pub fn locked_balance(&self, pk: &str) -> u128 {
        self.locked_balances.get(pk).copied().unwrap_or(0)
    }

    pub fn height(&self) -> u64 {
        self.height
    }

    pub fn issued(&self) -> u128 {
        self.issued
    }

    /// Stored map entries, including accounts whose current balance is zero.
    /// This is a resource count, not a count of unique people or active wallets.
    pub fn balance_entry_count(&self) -> usize {
        self.balances.len()
    }

    pub fn nonce_entry_count(&self) -> usize {
        self.next_nonces.len()
    }

    pub fn next_nonce(&self, pk: &str) -> u64 {
        self.next_nonces.get(pk).copied().unwrap_or(0)
    }

    pub fn pending_view(&self) -> Result<NativePendingView, NativeLedgerError> {
        let mut ledger = self.clone();
        let height = self
            .height
            .checked_add(1)
            .ok_or(NativeLedgerError::Overflow)?;
        ledger.unlock_rewards(height)?;
        Ok(NativePendingView { ledger, height })
    }

    /// Atomically settle accounting inputs from one already-validated block.
    /// Transactions execute in committed order, then scheduled issuance is
    /// credited. Consequently this block's issuance cannot finance its own
    /// transactions. Base issuance matures at creation height + the immutable
    /// maturity distance; ordinary credits and fees have no such lock. The
    /// kernel does not validate PoW/linkage or decide canonicality.
    pub fn apply_block(
        &mut self,
        height: u64,
        authenticated_reward_pk: &str,
        transfers: &[SignedEnvelope],
    ) -> Result<(), NativeLedgerError> {
        let (staged, _) = self.prepare_block(height, authenticated_reward_pk, transfers)?;
        *self = staged;
        Ok(())
    }

    pub(crate) fn prepare_block(
        &self,
        height: u64,
        authenticated_reward_pk: &str,
        transfers: &[SignedEnvelope],
    ) -> Result<(Self, NativeLedgerUndo), NativeLedgerError> {
        let mut staged = self.clone();
        let mut undo = NativeLedgerUndo::new(self);
        staged.apply_block_inner(height, authenticated_reward_pk, transfers, &mut undo)?;
        Ok((staged, undo))
    }

    pub(crate) fn undo_block(&mut self, undo: NativeLedgerUndo) -> Result<(), NativeLedgerError> {
        let expected = undo
            .height
            .checked_add(1)
            .ok_or(NativeLedgerError::Overflow)?;
        if self.height != expected {
            return Err(NativeLedgerError::UnexpectedHeight {
                expected,
                actual: self.height,
            });
        }
        restore_entries(&mut self.balances, undo.balances);
        restore_entries(&mut self.next_nonces, undo.next_nonces);
        restore_entries(&mut self.locked_balances, undo.locked_balances);
        restore_entries(&mut self.pending_rewards, undo.pending_rewards);
        self.height = undo.height;
        self.issued = undo.issued;
        Ok(())
    }

    fn apply_block_inner(
        &mut self,
        height: u64,
        authenticated_reward_pk: &str,
        transfers: &[SignedEnvelope],
        undo: &mut NativeLedgerUndo,
    ) -> Result<(), NativeLedgerError> {
        let expected = self
            .height
            .checked_add(1)
            .ok_or(NativeLedgerError::Overflow)?;
        if height != expected {
            return Err(NativeLedgerError::UnexpectedHeight {
                expected,
                actual: height,
            });
        }
        Hex32::from_hex(authenticated_reward_pk)
            .map_err(|_| NativeLedgerError::InvalidPublicKey)?;
        for (unlock_height, (pk, amount)) in self.pending_rewards.range(..=height) {
            undo.locked_balance(self, pk);
            undo.pending_rewards
                .insert(*unlock_height, Some((pk.clone(), *amount)));
        }
        self.unlock_rewards(height)?;
        for transfer in transfers {
            let prepared =
                self.prepare_transfer(height, Some(authenticated_reward_pk), transfer)?;
            for pk in prepared.balances.keys() {
                undo.balance(self, pk);
            }
            undo.next_nonces
                .entry(prepared.sender.clone())
                .or_insert_with(|| self.next_nonces.get(&prepared.sender).copied());
            self.commit_transfer(prepared);
        }
        let emission = self.schedule.emission(height, self.issued)?;
        if emission != 0 {
            undo.balance(self, authenticated_reward_pk);
        }
        self.credit(authenticated_reward_pk, emission)?;
        if emission != 0 && self.reward_maturity != 0 {
            let unlock_height = height
                .checked_add(self.reward_maturity)
                .ok_or(NativeLedgerError::Overflow)?;
            let locked = self
                .locked_balance(authenticated_reward_pk)
                .checked_add(emission)
                .ok_or(NativeLedgerError::Overflow)?;
            undo.locked_balance(self, authenticated_reward_pk);
            undo.pending_rewards
                .entry(unlock_height)
                .or_insert_with(|| self.pending_rewards.get(&unlock_height).cloned());
            self.locked_balances
                .insert(authenticated_reward_pk.to_string(), locked);
            self.pending_rewards.insert(
                unlock_height,
                (authenticated_reward_pk.to_string(), emission),
            );
        }
        self.issued = self
            .issued
            .checked_add(emission)
            .ok_or(NativeLedgerError::Overflow)?;
        self.height = height;
        Ok(())
    }

    fn unlock_rewards(&mut self, height: u64) -> Result<(), NativeLedgerError> {
        while self
            .pending_rewards
            .first_key_value()
            .is_some_and(|(unlock_height, _)| *unlock_height <= height)
        {
            let (_, (pk, amount)) = self.pending_rewards.pop_first().expect("pending reward");
            let remaining = self
                .locked_balance(&pk)
                .checked_sub(amount)
                .ok_or(NativeLedgerError::Overflow)?;
            if remaining == 0 {
                self.locked_balances.remove(&pk);
            } else {
                self.locked_balances.insert(pk, remaining);
            }
        }
        Ok(())
    }

    fn prepare_transfer(
        &self,
        height: u64,
        reward_pk: Option<&str>,
        envelope: &SignedEnvelope,
    ) -> Result<PreparedTransfer, NativeLedgerError> {
        let payload = validate_native_transfer(envelope, &self.network_id, self.minimum_fee)?;
        if height > payload.valid_before {
            return Err(NativeLedgerError::Expired);
        }
        let expected = self.next_nonce(&payload.from);
        if payload.nonce != expected {
            return Err(NativeLedgerError::UnexpectedNonce {
                expected,
                actual: payload.nonce,
            });
        }
        let debit = payload
            .amount
            .checked_add(payload.fee)
            .ok_or(NativeLedgerError::Overflow)?;
        if debit > self.spendable_balance(&payload.from) {
            return Err(NativeLedgerError::InsufficientBalance);
        }
        let remaining = self
            .balance(&payload.from)
            .checked_sub(debit)
            .ok_or(NativeLedgerError::InsufficientBalance)?;
        let next_nonce = payload
            .nonce
            .checked_add(1)
            .ok_or(NativeLedgerError::Overflow)?;
        let mut balances = BTreeMap::new();
        balances.insert(payload.from.clone(), remaining);
        self.stage_credit(&mut balances, &payload.to, payload.amount)?;
        if let Some(reward_pk) = reward_pk {
            self.stage_credit(&mut balances, reward_pk, payload.fee)?;
        }
        Ok(PreparedTransfer {
            balances,
            sender: payload.from,
            next_nonce,
        })
    }

    fn stage_credit(
        &self,
        balances: &mut BTreeMap<String, u128>,
        pk: &str,
        amount: u128,
    ) -> Result<(), NativeLedgerError> {
        if amount != 0 {
            let balance = balances
                .get(pk)
                .copied()
                .unwrap_or_else(|| self.balance(pk))
                .checked_add(amount)
                .ok_or(NativeLedgerError::Overflow)?;
            balances.insert(pk.to_string(), balance);
        }
        Ok(())
    }

    fn commit_transfer(&mut self, prepared: PreparedTransfer) {
        for (pk, balance) in prepared.balances {
            self.balances.insert(pk, balance);
        }
        self.next_nonces
            .insert(prepared.sender, prepared.next_nonce);
    }

    fn credit(&mut self, pk: &str, amount: u128) -> Result<(), NativeLedgerError> {
        if amount != 0 {
            let balance = self
                .balance(pk)
                .checked_add(amount)
                .ok_or(NativeLedgerError::Overflow)?;
            self.balances.insert(pk.to_string(), balance);
        }
        Ok(())
    }
}
