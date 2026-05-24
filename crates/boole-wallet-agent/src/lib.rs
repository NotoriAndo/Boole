//! Library facet of `boole-wallet-agent`.
//!
//! The crate's primary surface is the `boole-wallet-agent` binary
//! (`src/main.rs`); this file exists so cargo recognises the crate as
//! a library target, which is the only way a sibling crate can declare
//! it as a dev-dependency and inherit cargo's `CARGO_BIN_EXE_boole-
//! wallet-agent` env var inside that crate's integration tests.
//!
//! The single public constant below is the AEAD additional-data tag
//! that the binary binds every vault to. Re-exporting it from the
//! library lets a future façade (`boole-cli wallet ...`) seal a vault
//! in-process if and when an in-process API replaces the spawn-the-
//! binary contract — without that, the binary would be the sole owner
//! of the tag and consumers would have to copy the string verbatim,
//! which is a drift hazard.

/// AEAD additional-data tag bound into every vault sealed by
/// `boole-wallet-agent`. Any other consumer must bind a different tag;
/// mixing vault files across consumers fails at decryption.
pub const VAULT_AAD: &[u8] = b"boole-wallet-agent.v1";
