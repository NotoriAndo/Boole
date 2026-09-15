//! Native-coin accounting contract for a future, explicitly versioned network.
//!
//! This module is not connected to v3 block validation, the node, or a network
//! preset. The caller must validate block identity, linkage, PoW and reward
//! authorization before applying its accounting inputs. Monetary values passed
//! here are immutable per ledger, not a runtime configuration mechanism for an
//! existing network. There is no import path for legacy development credits.

use std::collections::BTreeMap;

use serde::Deserialize;
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
    #[error("native ledger: transfer expired")]
    Expired,
    #[error("native ledger: expected nonce {expected}, got {actual}")]
    UnexpectedNonce { expected: u64, actual: u64 },
    #[error("native ledger: insufficient balance for amount plus fee")]
    InsufficientBalance,
}

/// Signed payload integers are canonical decimal strings, including nonce and
/// height, so clients never round u128/u64 values through JSON floating point.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TransferPayload {
    schema: String,
    from: String,
    to: String,
    amount: String,
    fee: String,
    nonce: String,
    valid_before: String,
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
}

impl NativeLedger {
    /// Genesis has height zero, no issuance and no preallocated balances.
    pub fn new(network_id: &str, schedule: EmissionSchedule) -> Result<Self, NativeLedgerError> {
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
        })
    }

    pub fn balance(&self, pk: &str) -> u128 {
        self.balances.get(pk).copied().unwrap_or(0)
    }

    pub fn height(&self) -> u64 {
        self.height
    }

    pub fn issued(&self) -> u128 {
        self.issued
    }

    pub fn next_nonce(&self, pk: &str) -> u64 {
        self.next_nonces.get(pk).copied().unwrap_or(0)
    }

    /// Atomically settle accounting inputs from one already-validated block.
    /// Transactions execute in committed order, then scheduled issuance is
    /// credited. Consequently this block's issuance cannot finance its own
    /// transactions. This kernel does not select a reward-maturity policy,
    /// validate PoW/linkage, or decide whether a block is canonical.
    pub fn apply_block(
        &mut self,
        height: u64,
        authenticated_reward_pk: &str,
        transfers: &[SignedEnvelope],
    ) -> Result<(), NativeLedgerError> {
        let mut staged = self.clone();
        staged.apply_block_inner(height, authenticated_reward_pk, transfers)?;
        *self = staged;
        Ok(())
    }

    fn apply_block_inner(
        &mut self,
        height: u64,
        authenticated_reward_pk: &str,
        transfers: &[SignedEnvelope],
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
        for transfer in transfers {
            self.apply_transfer(height, authenticated_reward_pk, transfer)?;
        }
        let emission = self.schedule.emission(height, self.issued)?;
        self.credit(authenticated_reward_pk, emission)?;
        self.issued = self
            .issued
            .checked_add(emission)
            .ok_or(NativeLedgerError::Overflow)?;
        self.height = height;
        Ok(())
    }

    fn apply_transfer(
        &mut self,
        height: u64,
        reward_pk: &str,
        envelope: &SignedEnvelope,
    ) -> Result<(), NativeLedgerError> {
        if envelope.schema != SIGNED_ENVELOPE_SCHEMA {
            return Err(NativeLedgerError::InvalidTransfer);
        }
        if envelope.network_id.as_deref() != Some(&self.network_id) {
            return Err(NativeLedgerError::WrongNetwork);
        }
        let payload: TransferPayload = serde_json::from_value(envelope.payload.clone())
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
        let amount: u128 = decimal(&payload.amount)?;
        let fee: u128 = decimal(&payload.fee)?;
        let nonce: u64 = decimal(&payload.nonce)?;
        let valid_before: u64 = decimal(&payload.valid_before)?;
        if amount == 0 {
            return Err(NativeLedgerError::ZeroAmount);
        }
        if height > valid_before {
            return Err(NativeLedgerError::Expired);
        }
        let expected = self.next_nonce(&payload.from);
        if nonce != expected {
            return Err(NativeLedgerError::UnexpectedNonce {
                expected,
                actual: nonce,
            });
        }
        let debit = amount.checked_add(fee).ok_or(NativeLedgerError::Overflow)?;
        let remaining = self
            .balance(&payload.from)
            .checked_sub(debit)
            .ok_or(NativeLedgerError::InsufficientBalance)?;
        let next_nonce = nonce.checked_add(1).ok_or(NativeLedgerError::Overflow)?;
        self.balances.insert(payload.from.clone(), remaining);
        for (pk, credit) in [(payload.to.as_str(), amount), (reward_pk, fee)] {
            self.credit(pk, credit)?;
        }
        self.next_nonces.insert(payload.from, next_nonce);
        Ok(())
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
