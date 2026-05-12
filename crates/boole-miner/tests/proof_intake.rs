use boole_miner::{
    extract_proof_source, ProofEnvelope, ProofIntakeV1, ProofTransport, RejectionReason,
};

#[test]
fn proof_intake_v1_accepts_declared_answer_channel_only() {
    let envelope = ProofEnvelope {
        answer: "by\n  intro xs\n  exact length_dedup_le xs".to_string(),
        stdout: Some("WARNING: compacted context; not part of answer".to_string()),
        stderr: Some("debug trace must not enter proof".to_string()),
    };

    let candidate = ProofIntakeV1::extract(envelope).expect("valid answer channel");

    assert_eq!(
        candidate.proof_source,
        "by\n  intro xs\n  exact length_dedup_le xs"
    );
    assert_eq!(candidate.contract_version, "boole-proof-body-v1");
    assert_eq!(
        candidate.canonicalizer_version,
        "boole-proof-canonicalizer-v1"
    );
    assert!(!candidate.proof_source.contains("WARNING"));
    assert!(!candidate.proof_source.contains("debug trace"));
}

#[test]
fn proof_intake_v1_rejects_warning_contamination_inside_answer_channel() {
    let envelope = ProofEnvelope {
        answer: "WARNING: compacted context\nby\n  trivial".to_string(),
        stdout: None,
        stderr: None,
    };

    assert_eq!(
        ProofIntakeV1::extract(envelope).map(|c| c.proof_source),
        Err(RejectionReason::ContractFailed)
    );
}

#[test]
fn proof_intake_v1_rejects_missing_answer_channel_even_if_stdout_has_proof() {
    let envelope = ProofEnvelope {
        answer: String::new(),
        stdout: Some("by\n  trivial".to_string()),
        stderr: None,
    };

    assert_eq!(
        ProofIntakeV1::extract(envelope).map(|c| c.proof_source),
        Err(RejectionReason::EmptyResponse)
    );
}

#[test]
fn proof_transport_plain_text_is_the_shared_legacy_answer_envelope() {
    let candidate = ProofTransport::PlainText("```lean\nby trivial\n```".to_string())
        .into_envelope()
        .and_then(ProofIntakeV1::extract)
        .expect("legacy plain text should still use the common intake");

    assert_eq!(candidate.proof_source, "by trivial");
}

#[test]
fn extract_proof_source_uses_common_intake_versions() {
    assert_eq!(
        extract_proof_source("by trivial"),
        Ok("by trivial".to_string())
    );
}

#[test]
fn proof_intake_v1_rejects_bare_top_level_tactic_shapes() {
    for source in [
        "apply length_dedup_le",
        "rw [length_sortAsc]",
        "intro xs",
        "have h := length_dedup_le xs",
        "exact length_dedup_le xs",
        "calc\n  xs.length ≤ xs.length := by exact Nat.le_refl _",
    ] {
        assert_eq!(
            extract_proof_source(source),
            Err(RejectionReason::ContractFailed),
            "bare top-level theorem-body tactic should be rejected: {source:?}"
        );
    }
}

#[test]
fn proof_intake_v1_accepts_slot_level_by_and_fun_shapes() {
    assert_eq!(
        extract_proof_source("by\n  intro xs\n  exact length_dedup_le xs"),
        Ok("by\n  intro xs\n  exact length_dedup_le xs".to_string())
    );
    assert_eq!(
        extract_proof_source("fun xs => length_dedup_le xs"),
        Ok("fun xs => length_dedup_le xs".to_string())
    );
}
