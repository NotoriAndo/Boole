//! Derived identity digests (EMITTER-CONTRACT-v1_2-final §3 / §4).
//!
//! This slice materializes only the **anchor-binding** identity — the digest
//! that binds one census anchor to its upstream source coordinates. The
//! task-path identities (`problem_digest`, `emitter_task_key`,
//! `verification_binding_digest`, `variant_free_key`) are only meaningful once a
//! converter emits a non-empty `tasks[]`; the Rust representative anchor emits a
//! zero result, so they are deferred until a task-producing converter exists
//! rather than added as untested speculative code.

use crate::digest::{protocol_digest, push_field, Digest};

/// Domain separator for `anchor_binding_digest` (§3).
pub const ANCHOR_BINDING_DOMAIN: &[u8] = b"boole.emitter-output.anchor-binding.v1";

/// Compute the Rust-domain `anchor_binding_digest` (§3).
///
/// Preimage (each component `push_field`-framed, in this exact order):
/// `domain_tag="rust", source_commit, source_path, line_start (u64_le), kind,
/// raw(semantic_digest), identity`.
pub fn rust_anchor_binding_digest(
    source_commit: &str,
    source_path: &str,
    line_start: u64,
    kind: &str,
    semantic_digest_raw: &[u8; 32],
    identity: &str,
) -> Digest {
    let mut buf = Vec::new();
    push_field(&mut buf, b"rust");
    push_field(&mut buf, source_commit.as_bytes());
    push_field(&mut buf, source_path.as_bytes());
    push_field(&mut buf, &line_start.to_le_bytes());
    push_field(&mut buf, kind.as_bytes());
    push_field(&mut buf, semantic_digest_raw);
    push_field(&mut buf, identity.as_bytes());
    protocol_digest(ANCHOR_BINDING_DOMAIN, &buf)
}
