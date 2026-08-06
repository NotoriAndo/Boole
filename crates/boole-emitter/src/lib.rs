//! `boole-emitter` — the domain-common emitter contract (`EmitterResultV1` /
//! `EmitterOutputV1`) and the per-domain converters that expand one p2 census
//! anchor into 0..N normalized task records or exactly one deterministic
//! `zero_reason`.
//!
//! This crate is a NON-consensus, offline census utility. It never touches
//! admission, replay, block building, or reward paths and produces no on-chain
//! artifact. Derived identities reuse boole-core's BLAKE3 `h_protocol` so the
//! contract's digests are byte-faithful to the protocol hashing convention;
//! source/file/text evidence uses SHA-256.
//!
//! In this slice the crate is a **common contract checker and zero-result
//! generator**, not a "problem generator": it validates the canonical-JSON /
//! digest / XOR invariants and converts the Rust representative anchor into its
//! deterministic zero result.
//!
//! Frozen specification:
//! `local-docs/mineable-eligible-census-p0/EMITTER-CONTRACT-v1_2-final.md`.

pub mod canonical_json;
pub mod digest;
pub mod identity;
pub mod result;
pub mod rust_domain;
