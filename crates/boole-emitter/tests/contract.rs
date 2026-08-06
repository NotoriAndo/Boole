//! Common emitter-contract checks (EMITTER-CONTRACT-v1_2-final §1/§3/§10):
//! derived-digest determinism and ordering sensitivity, the XOR invariant, and
//! the Rust representative anchor's deterministic zero path.

use boole_emitter::digest::{sha256_digest, DigestAlg};
use boole_emitter::identity::rust_anchor_binding_digest;
use boole_emitter::result::{
    AnchorBinding, AnchorRef, EmitterOutputV1, EmitterResultV1, MissingPart, StageA, XorViolation,
    ZeroReason, ZeroReasonCode, EMITTER_RESULT_SCHEMA,
};
use boole_emitter::rust_domain::{convert_rust_anchor, RustAnchor};

const REP_ANCHOR_JSON: &str = include_str!("fixtures/rust_representative_anchor.json");

fn sample_semantic() -> [u8; 32] {
    [7u8; 32]
}

// ----- derived digest: determinism + ordering sensitivity (§3) -----

#[test]
fn anchor_binding_digest_is_deterministic() {
    let sem = sample_semantic();
    let a = rust_anchor_binding_digest(
        "commit",
        "src/x.md",
        96,
        "rust_rule",
        &sem,
        "attributes.meta",
    );
    let b = rust_anchor_binding_digest(
        "commit",
        "src/x.md",
        96,
        "rust_rule",
        &sem,
        "attributes.meta",
    );
    assert_eq!(a, b);
    assert_eq!(a.algorithm, DigestAlg::Blake3);
    assert_eq!(a.hex.len(), 64);
    assert!(a
        .hex
        .bytes()
        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
}

#[test]
fn anchor_binding_digest_is_field_order_sensitive() {
    let sem = sample_semantic();
    // Swapping the source_commit and source_path VALUES must change the digest:
    // push_field framing keeps the two components distinguishable.
    let a = rust_anchor_binding_digest("aaa", "bbb", 1, "rust_rule", &sem, "id");
    let b = rust_anchor_binding_digest("bbb", "aaa", 1, "rust_rule", &sem, "id");
    assert_ne!(a, b);
}

#[test]
fn anchor_binding_digest_line_start_matters() {
    let sem = sample_semantic();
    let a = rust_anchor_binding_digest("c", "p", 96, "rust_rule", &sem, "id");
    let b = rust_anchor_binding_digest("c", "p", 97, "rust_rule", &sem, "id");
    assert_ne!(a, b);
}

#[test]
fn sha256_digest_raw_roundtrips() {
    let d = sha256_digest(b"boole");
    assert_eq!(d.algorithm, DigestAlg::Sha256);
    assert_eq!(hex::encode(d.raw()), d.hex);
}

// ----- XOR invariant (§1) -----

fn zero_result() -> EmitterResultV1 {
    EmitterResultV1 {
        schema: EMITTER_RESULT_SCHEMA,
        anchor_ref: AnchorRef {
            domain: "rust".into(),
            identity_kind: "rust_rule".into(),
            identity_value: "id".into(),
        },
        anchor_binding: AnchorBinding {
            anchor_binding_digest: sha256_digest(b"anchor"),
        },
        tasks: Vec::new(),
        zero_reason: Some(ZeroReason {
            code: ZeroReasonCode::TaskContractUnmaterialized,
            evidence_digest: sha256_digest(b"evidence"),
            evidence_scope: "scope".into(),
            missing: Some(vec![MissingPart::Statement]),
        }),
    }
}

#[test]
fn xor_ok_when_only_zero_reason() {
    assert_eq!(zero_result().validate_xor(), Ok(()));
}

#[test]
fn xor_ok_when_only_tasks() {
    let mut r = zero_result();
    r.zero_reason = None;
    r.tasks = vec![EmitterOutputV1 {
        emitter_task_key: sha256_digest(b"k"),
        problem_digest: sha256_digest(b"p"),
    }];
    assert_eq!(r.validate_xor(), Ok(()));
}

#[test]
fn xor_rejects_both_present() {
    let mut r = zero_result();
    r.tasks = vec![EmitterOutputV1 {
        emitter_task_key: sha256_digest(b"k"),
        problem_digest: sha256_digest(b"p"),
    }];
    assert_eq!(r.validate_xor(), Err(XorViolation::Both));
}

#[test]
fn xor_rejects_neither_present() {
    let mut r = zero_result();
    r.zero_reason = None;
    assert_eq!(r.validate_xor(), Err(XorViolation::Neither));
}

// ----- Rust representative anchor: deterministic zero path (§3/§10) -----

#[test]
fn rust_representative_anchor_is_task_contract_unmaterialized() {
    let anchor: RustAnchor =
        serde_json::from_str(REP_ANCHOR_JSON).expect("fixture parses as a RustAnchor");
    let conv = convert_rust_anchor(&anchor).expect("representative anchor converts");

    // Zero path, XOR holds.
    assert!(conv.result.tasks.is_empty());
    assert_eq!(conv.result.validate_xor(), Ok(()));
    assert_eq!(conv.result.schema, EMITTER_RESULT_SCHEMA);

    // The corrected code + missing contract parts (msg 3573).
    let zr = conv
        .result
        .zero_reason
        .as_ref()
        .expect("zero_reason present");
    assert_eq!(zr.code, ZeroReasonCode::TaskContractUnmaterialized);
    assert_eq!(
        zr.missing.as_deref(),
        Some(
            &[
                MissingPart::Statement,
                MissingPart::Oracle,
                MissingPart::WorkProductContract
            ][..]
        )
    );
    assert_eq!(zr.evidence_digest.algorithm, DigestAlg::Sha256);
    assert_eq!(zr.evidence_digest.hex.len(), 64);

    // Stage A must NOT be promoted.
    assert_eq!(conv.stage_a, StageA::TaskContractMissing);

    // anchor_binding_digest is the deterministic §3 digest of the anchor.
    let mut sem = [0u8; 32];
    hex::decode_to_slice(
        anchor.semantic_digest.strip_prefix("sha256:").unwrap(),
        &mut sem,
    )
    .unwrap();
    let expected = rust_anchor_binding_digest(
        &anchor.source_commit,
        &anchor.source_path,
        anchor.line_start,
        &anchor.kind,
        &sem,
        &anchor.identity,
    );
    assert_eq!(conv.result.anchor_binding.anchor_binding_digest, expected);
    assert_eq!(
        conv.result.anchor_binding.anchor_binding_digest.algorithm,
        DigestAlg::Blake3
    );
}

#[test]
fn rust_representative_zero_result_serializes_with_corrected_names() {
    let anchor: RustAnchor = serde_json::from_str(REP_ANCHOR_JSON).unwrap();
    let conv = convert_rust_anchor(&anchor).unwrap();
    let json = serde_json::to_string(&conv.result).unwrap();
    // The frozen wire names, not the retired SPEC-UNMATERIALIZED.
    assert!(json.contains("TASK-CONTRACT-UNMATERIALIZED"));
    assert!(!json.contains("SPEC-UNMATERIALIZED"));
    assert!(json.contains("work_product_contract"));
    assert!(json.contains("boole.emitter-result.v1"));
}
