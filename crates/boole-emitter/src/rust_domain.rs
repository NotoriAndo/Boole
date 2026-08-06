//! The Rust-domain representative-anchor converter
//! (EMITTER-CONTRACT-v1_2-final §3 / §10).
//!
//! Rust census anchors are entries from the rust-lang/reference mdBook — written
//! language-specification rules, not auditable Rust implementations. A Reference
//! rule IS a specification, but no **runnable task contract** (statement, oracle,
//! work-product contract) has been materialized from it. So the honest, byte-
//! deterministic conversion of one such anchor is a single zero result with code
//! `TASK-CONTRACT-UNMATERIALIZED` and Stage A `TASK-CONTRACT-MISSING` — NOT a
//! task, and NOT the claim "Rust has zero problems".

use serde::Deserialize;

use crate::canonical_json::to_canonical_bytes;
use crate::digest::sha256_digest;
use crate::identity::rust_anchor_binding_digest;
use crate::result::{
    AnchorBinding, AnchorRef, EmitterResultV1, MissingPart, StageA, ZeroReason, ZeroReasonCode,
    EMITTER_RESULT_SCHEMA,
};

/// A single Rust census anchor (subset of the census record consumed here).
#[derive(Clone, Debug, Deserialize)]
pub struct RustAnchor {
    pub identity: String,
    pub kind: String,
    pub source_commit: String,
    pub source_path: String,
    pub line_start: u64,
    /// `"sha256:<64 lowercase hex>"`.
    pub semantic_digest: String,
    pub anchor_ready: bool,
    pub strict_ready: bool,
}

/// The converter output for one anchor: the result envelope plus its Stage A
/// classification (a census-side judgement, not a `zero_reason` code).
#[derive(Clone, Debug)]
pub struct AnchorConversion {
    pub result: EmitterResultV1,
    pub stage_a: StageA,
}

/// Errors from converting a Rust anchor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RustConvertError {
    /// `semantic_digest` was not `"sha256:<64 lowercase hex>"`.
    BadSemanticDigest,
}

fn parse_sha256_prefixed(value: &str) -> Option<[u8; 32]> {
    let hex_part = value.strip_prefix("sha256:")?;
    if hex_part.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    hex::decode_to_slice(hex_part, &mut out).ok()?;
    Some(out)
}

/// Convert one Rust Reference anchor into its deterministic zero result.
pub fn convert_rust_anchor(anchor: &RustAnchor) -> Result<AnchorConversion, RustConvertError> {
    let semantic_raw = parse_sha256_prefixed(&anchor.semantic_digest)
        .ok_or(RustConvertError::BadSemanticDigest)?;

    let anchor_binding_digest = rust_anchor_binding_digest(
        &anchor.source_commit,
        &anchor.source_path,
        anchor.line_start,
        &anchor.kind,
        &semantic_raw,
        &anchor.identity,
    );

    // What a runnable task contract would need — and what the Reference anchor
    // does not supply.
    let missing = vec![
        MissingPart::Statement,
        MissingPart::Oracle,
        MissingPart::WorkProductContract,
    ];

    // Deterministic evidence for the absence: the anchor's binding identity and
    // the observed non-materialization, in canonical JSON, then SHA-256.
    let evidence_value = serde_json::json!({
        "anchor_binding_digest": anchor_binding_digest.hex,
        "identity": anchor.identity,
        "missing": ["statement", "oracle", "work_product_contract"],
        "observed": {
            "anchor_ready": anchor.anchor_ready,
            "strict_ready": anchor.strict_ready,
        },
        "source_commit": anchor.source_commit,
        "source_path": anchor.source_path,
    });
    let evidence_json = serde_json::to_string(&evidence_value).expect("evidence value serializes");
    let evidence_bytes =
        to_canonical_bytes(&evidence_json).expect("evidence is canonical-json representable");
    let evidence_digest = sha256_digest(&evidence_bytes);

    let zero_reason = ZeroReason {
        code: ZeroReasonCode::TaskContractUnmaterialized,
        evidence_digest,
        evidence_scope: "rust reference anchor: no materialized statement, oracle, or \
                         work-product contract"
            .to_owned(),
        missing: Some(missing),
    };

    let result = EmitterResultV1 {
        schema: EMITTER_RESULT_SCHEMA,
        anchor_ref: AnchorRef {
            domain: "rust".to_owned(),
            identity_kind: anchor.kind.clone(),
            identity_value: anchor.identity.clone(),
        },
        anchor_binding: AnchorBinding {
            anchor_binding_digest,
        },
        tasks: Vec::new(),
        zero_reason: Some(zero_reason),
    };

    Ok(AnchorConversion {
        result,
        // The anchor's only blocker is the unmaterialized task contract, so it
        // is TASK-CONTRACT-MISSING and is NOT promoted to
        // RUNNABLE-TASK-EMITTER-PRESENT.
        stage_a: StageA::TaskContractMissing,
    })
}
