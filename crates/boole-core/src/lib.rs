//! Core protocol types and deterministic state transition logic for Boole.
//!
//! This crate is intentionally small at first. It will grow by matching
//! TypeScript-generated golden fixtures from `legacy-pof`.

pub mod admission;
pub mod admission_types;
pub mod agent_events;
pub mod block;
pub mod block_builder;
pub mod bounty_ledger;
pub mod bounty_promotion;
pub mod bounty_proof_verifier;
pub mod bounty_registry;
pub mod bounty_side_pool;
pub mod canonical_json;
pub mod config;
pub mod difficulty;
pub mod family_manifest;
pub mod family_manifest_registry;
pub mod family_v1_lenbound;
pub mod hash;
pub mod lean_bound_canon;
pub mod paths;
pub mod rate_limiter;
pub mod receipt;
pub mod rejection_log;
pub mod replay;
pub(crate) mod replay_evidence;
pub mod session_policy;
pub mod share_pool;
pub mod signed_envelope;
pub mod submission_pow;
pub mod submit_receipt_audit;
pub mod telemetry;
pub mod validator;
pub mod vault;
pub mod work_manifest;

pub use admission::{
    admit_parsed_submission_typed, admit_submission, admit_submission_json, admit_submission_typed,
    check_admission_ticket, parse_submission_body, AdmissionDeps, AdmissionParsedDeps,
};
pub use admission_types::{
    AdmissionDecision, AdmissionError, AdmissionStatus, ParsedSubmission, RejectionReason,
    SubmitPowRejectReason, TicketAdmissionResult, TicketRejectReason,
};
pub use agent_events::{
    agent_passport_events_for_receipt, AgentPassportEvent, AGENT_PASSPORT_EVENT_SCHEMA,
};
pub use block::{PersistedBlock, SelectedShareEvidence};
pub use block_builder::{
    build_block_selection, BlockBuilderConfig, BuildSelectionResult, BuiltBlockSelection,
    CandidateShare, PromotedBountyCredit, PromotedBountySelection, PromotedBountyShare,
};
pub use bounty_ledger::{validate_bounty_ledger_event, BountyEventLedger};
pub use bounty_promotion::{select_promoted_bounty_selection, select_promoted_bounty_shares};
pub use bounty_proof_verifier::{BountyProofVerifier, VerifyOutcome};
pub use bounty_registry::{
    bounties_from_list, Bounty, BountyList, BountyRegistry, BountyVerifier, CreateBountyInput,
    SubmitProofInput, SubmitProofResult, UpdateStatusInput,
};
pub use bounty_side_pool::{BountyShare, BountySidePool};
pub use canonical_json::canonicalize;
pub use config::{
    calibration_policy, calibration_thresholds, hex_to_biguint, parse_decimal_nanos,
    validate_calibration_report, CalibrationPolicy, CalibrationReport, CalibrationThresholds,
};
pub use difficulty::{
    expected_retarget_difficulty_for_height, retarget_t_block, validate_retargeted_difficulty,
    verify_block_ts_median_time_past, DifficultyEvidence, DifficultyRetargetPolicy,
    MEDIAN_TIME_PAST_WINDOW,
};
pub use family_manifest::{
    parse_family_manifest, verify_family_manifest_signature, FamilyCaps, FamilyManifest,
    FamilyManifestParseResult,
};
pub use family_manifest_registry::FamilyManifestRegistry;
pub use hash::{
    block_hash, difficulty_weight, digest_to_biguint, h_protocol, min_share_score,
    parse_biguint_hex, share_hash, share_score, submission_pow_hash, submission_pow_ok, ticket,
    Hex32, Hex64, TicketResult,
};
pub use lean_bound_canon::{lean_bound_canon_package, lean_bound_verifier_hash};
pub use rate_limiter::{
    rate_limit_result_json, RateLimitRejectReason, RateLimitResult, RateLimiter,
};
pub use receipt::{ReceiptCommitment, ReceiptCommitmentInput};
pub use rejection_log::{
    json_rejection_line, reason_key, reason_key_typed, rejection_event_from_json,
    rejection_event_json, rejection_event_line, LoggedRejectionReason, RejectionEvent,
    RingRejectionLogger,
};
pub use replay::{
    compute_block_credits, compute_block_reward_credits, replay_blocks,
    replay_blocks_with_retarget, PersistedCredit, PersistedRewardEvent, ReplayResult,
};
pub use session_policy::{SessionPolicy, SessionState, SignerRequest};
pub use share_pool::{AcceptResult, PoolShare, SharePool, SharePoolRejectReason};
pub use signed_envelope::{
    canonical_payload_hash_hex, signing_digest_hex, verify_signature,
    verify_signature_with_network, SignedEnvelope, SigningKeyV2, SIGNED_ENVELOPE_SCHEMA,
};
pub use submission_pow::{
    check_submission_pow, check_submission_pow_json, check_submission_pow_with_policy,
    SubmissionPowRejectReason, SubmissionPowResult,
};
pub use submit_receipt_audit::{
    audit_submit_receipt_lineages, audit_submit_receipts, SubmitReceipt, SubmitReceiptAuditReport,
    SubmitReceiptLineage, SubmitReceiptReputationDelta, SubmitReceiptSettlementChecks,
    SubmitReceiptSettlementReport,
};
pub use validator::{
    decode_detail_from_json, decode_detail_json, validate_proof_package,
    validate_proof_package_json, validate_proof_package_shape, validate_proof_package_with_policy,
    validation_reason_from_json, validation_reason_json, DecodeDetail, ValidationReason,
    ValidationResult,
};
pub use vault::{EncryptedVault, VaultError, VaultParams};
pub use work_manifest::{
    bounty_to_work_manifest, work_manifests_from_list, BountyFixture, WorkManifest,
    WorkManifestList, WorkVerifier,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex32_roundtrip_accepts_lowercase_32_bytes() {
        let input = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
        let parsed = Hex32::from_hex(input).expect("valid hex32");
        assert_eq!(parsed.to_hex(), input);
    }

    #[test]
    fn hex64_roundtrip_accepts_lowercase_64_bytes_and_rejects_uppercase() {
        let input = "00".repeat(64);
        let parsed = Hex64::from_hex(&input).expect("valid hex64");
        assert_eq!(parsed.to_hex(), input);
        assert!(Hex64::from_hex(&"A".repeat(128)).is_err());
    }
}
