//! Pure devnet v0 derivation. No I/O enters this crate.
#[cfg(test)]
extern crate self as boole_derivation;
pub mod attributes;
pub mod config;
pub mod inbox;
#[cfg(test)]
mod m2_legacy_tests;
#[cfg(test)]
#[path = "../tests/support/m3.rs"]
mod m3_scenario;
pub mod rlp;
pub mod system;
pub mod user_tx;

pub use attributes::{
    check_l1_link, derive, derive_with_m2, derive_with_m3, Derived, InboxLog, InputError, L1Block,
    ParentHeader,
};

pub mod m2;
pub mod m3;
