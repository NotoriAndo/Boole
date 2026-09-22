//! Code-pinned policy for the new, test-only native-coin network.
//! This is separate from all legacy v3 genesis and development-credit rules.

use serde::{Serialize, Serializer};

use crate::native_ledger::{EmissionSchedule, NativeLedger, NativeLedgerError};
use crate::{canonicalize, DifficultyRetargetPolicy, Hex32};

pub const NATIVE_TESTNET_NETWORK_ID: &str = "boole-native-testnet-1";
pub const NATIVE_COIN_UNIT: u128 = 100_000_000;

/// Only compiled constructors can create a policy. It cannot be deserialized
/// from a scenario or changed independently by node operators.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeNetwork {
    schema: &'static str,
    rule_version: u32,
    network_id: &'static str,
    symbol: &'static str,
    decimals: u8,
    #[serde(serialize_with = "serialize_amount")]
    total_supply: u128,
    #[serde(serialize_with = "serialize_amount")]
    initial_reward: u128,
    halving_epoch: u64,
    #[serde(serialize_with = "serialize_amount")]
    minimum_fee: u128,
    reward_maturity: u64,
    initial_target: String,
    retarget: DifficultyRetargetPolicy,
    median_time_past_window: usize,
    max_transfers_per_block: usize,
    max_transfer_bytes: usize,
    max_block_bytes: usize,
    useful_work_rewards: bool,
    reward_allocation: &'static str,
    fee_allocation: &'static str,
    #[serde(serialize_with = "serialize_amount")]
    genesis_issuance: u128,
}

fn serialize_amount<S: Serializer>(amount: &u128, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&amount.to_string())
}

pub fn native_testnet() -> NativeNetwork {
    NativeNetwork {
        schema: "boole.native.genesis.v1",
        rule_version: 1,
        network_id: NATIVE_TESTNET_NETWORK_ID,
        symbol: "tBOOLE",
        decimals: 8,
        total_supply: 1_000_000_000 * NATIVE_COIN_UNIT,
        initial_reward: 50_000 * NATIVE_COIN_UNIT,
        halving_epoch: 10_000,
        minimum_fee: 1_000,
        reward_maturity: 10,
        // An inexpensive but nontrivial hash target for this experimental
        // chain. The target is not a promise of measured wall-clock speed.
        initial_target: format!("0000{}", "f".repeat(60)),
        retarget: DifficultyRetargetPolicy {
            target_block_ms: 60_000,
            retarget_every_blocks: 2,
            max_adjustment_factor: 4,
        },
        median_time_past_window: crate::difficulty::MEDIAN_TIME_PAST_WINDOW,
        max_transfers_per_block: 512,
        max_transfer_bytes: 4_096,
        max_block_bytes: 524_288,
        useful_work_rewards: false,
        reward_allocation: "producer-only",
        fee_allocation: "producer-no-burn",
        genesis_issuance: 0,
    }
}

impl NativeNetwork {
    pub fn network_id(&self) -> &'static str {
        self.network_id
    }

    pub fn decimals(&self) -> u8 {
        self.decimals
    }

    pub fn total_supply(&self) -> u128 {
        self.total_supply
    }

    pub fn minimum_fee(&self) -> u128 {
        self.minimum_fee
    }

    pub fn genesis_hash(&self) -> Hex32 {
        let mut hash = blake3::Hasher::new();
        hash.update(b"boole.native.genesis.v1\0");
        hash.update(&canonicalize(
            &serde_json::to_value(self).expect("native policy"),
        ));
        Hex32::from_bytes(*hash.finalize().as_bytes())
    }

    pub fn initial_target(&self) -> &str {
        &self.initial_target
    }

    pub fn retarget(&self) -> &DifficultyRetargetPolicy {
        &self.retarget
    }

    pub fn median_time_past_window(&self) -> usize {
        self.median_time_past_window
    }

    pub fn max_transfers_per_block(&self) -> usize {
        self.max_transfers_per_block
    }

    pub fn max_transfer_bytes(&self) -> usize {
        self.max_transfer_bytes
    }

    pub fn max_block_bytes(&self) -> usize {
        self.max_block_bytes
    }

    pub fn ledger(&self) -> Result<NativeLedger, NativeLedgerError> {
        NativeLedger::new_with_rules(
            self.network_id,
            EmissionSchedule::new(self.total_supply, self.initial_reward, self.halving_epoch)?,
            self.minimum_fee,
            self.reward_maturity,
        )
    }
}
