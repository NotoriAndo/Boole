//! Core protocol types and deterministic state transition logic for Boole.
//!
//! This crate is intentionally small at first. It will grow by matching
//! TypeScript-generated golden fixtures from `/Users/seoyong/projects/pof`.

pub mod admission;
pub mod block;
pub mod block_builder;
pub mod bounty_ledger;
pub mod bounty_registry;
pub mod config;
pub mod family_manifest;
pub mod hash;
pub mod rate_limiter;
pub mod rejection_log;
pub mod replay;
pub mod share_pool;
pub mod submission_pow;
pub mod validator;
pub mod work_manifest;

pub use admission::{
    admit_submission, admit_submission_json, admit_submission_typed, check_admission_ticket,
    AdmissionDecision, AdmissionDeps, AdmissionError, AdmissionStatus, RejectionReason,
    TicketAdmissionResult, TicketRejectReason,
};
pub use block::PersistedBlock;
pub use block_builder::{
    build_block_selection, BlockBuilderConfig, BuildSelectionResult, BuiltBlockSelection,
    CandidateShare,
};
pub use bounty_ledger::BountyEventLedger;
pub use bounty_registry::{
    Bounty, BountyRegistry, BountyVerifier, CreateBountyInput, SubmitProofInput, SubmitProofResult,
    UpdateStatusInput,
};
pub use config::{
    calibration_policy, calibration_thresholds, hex_to_biguint, validate_calibration_report,
    CalibrationPolicy, CalibrationReport, CalibrationThresholds,
};
pub use family_manifest::{parse_family_manifest, FamilyManifest, FamilyManifestParseResult};
pub use hash::{
    block_hash, difficulty_weight, digest_to_biguint, h_protocol, min_share_score,
    parse_biguint_hex, share_hash, share_score, submission_pow_hash, submission_pow_ok, ticket,
    Hex32, TicketResult,
};
pub use rate_limiter::{
    rate_limit_result_json, RateLimitRejectReason, RateLimitResult, RateLimiter,
};
pub use rejection_log::{
    json_rejection_line, reason_key, reason_key_typed, rejection_event_from_json,
    rejection_event_json, rejection_event_line, LoggedRejectionReason, RejectionEvent,
    RingRejectionLogger,
};
pub use replay::{
    compute_block_credits, replay_blocks, PersistedCredit, PersistedRewardEvent, ReplayResult,
};
pub use share_pool::{AcceptResult, PoolShare, SharePool, SharePoolRejectReason};
pub use submission_pow::{
    check_submission_pow, check_submission_pow_json, SubmissionPowRejectReason, SubmissionPowResult,
};
pub use validator::{
    decode_detail_from_json, decode_detail_json, validate_proof_package,
    validate_proof_package_json, validate_proof_package_with_policy, validation_reason_from_json,
    validation_reason_json, DecodeDetail, ValidationReason, ValidationResult,
};
pub use work_manifest::{bounty_to_work_manifest, BountyFixture, WorkManifest, WorkVerifier};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex32_roundtrip_accepts_lowercase_32_bytes() {
        let input = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
        let parsed = Hex32::from_hex(input).expect("valid hex32");
        assert_eq!(parsed.to_hex(), input);
    }
}
