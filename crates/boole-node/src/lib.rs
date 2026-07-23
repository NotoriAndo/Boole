mod block_store;
mod block_verifier;
mod bounty_catalog_store;
mod bounty_event_store;
mod checker_pin;
mod checkpoint;
mod deep_verify;
mod durability;
mod family_manifest_store;
mod http_error;
mod lean_bounty_verifier;
mod local_node;
#[allow(dead_code)]
mod nonce_ledger;
mod p2p_egress;
mod p2p_ingress;
mod proof_bridge;
mod proof_dedup_ledger;
mod receipt_store;
mod reputation_store;
mod reward_store;
mod runtime;
mod runtime_smoke;
mod session_store;
mod signed_nonce_ledger;
mod state_dir;
mod useful_work_store;
mod work_manifest_store;

pub use block_store::FileBlockStore;
pub use block_verifier::{
    reverify_block_selected_shares, reverify_candidate_chain_selected_shares,
    reverify_share_evidence, verify_lean_bound_share_evidence, BlockReverifyOutcome,
    ShareEvidenceVerdict,
};
pub use bounty_catalog_store::load_bounties_from_path;
pub use bounty_event_store::FileBountyEventLedger;
pub use checkpoint::{
    build_verified_checkpoint, checkpoint_path_for, checkpoint_skip_decision,
    checkpoint_survives_boot, checkpoint_survives_reorg, read_checkpoint,
    validate_or_discard_checkpoint_at_boot, write_checkpoint, CheckpointIdentity,
    CheckpointSkipDecision, VerifiedPrefixCheckpoint,
};
pub use deep_verify::{
    deep_verify_block, deep_verify_bounty_events, DeepVerifyBlockReport, DeepVerifyDivergence,
    DeepVerifyError, DeepVerifyReport,
};
pub use family_manifest_store::{load_family_manifest_registry_from_dir, FamilyManifestStoreError};
pub use lean_bounty_verifier::LeanBountyVerifier;
pub use local_node::{
    serve_local_node, serve_local_node_with_disk_full_sentinel, serve_local_node_with_os_signals,
    serve_local_node_with_os_signals_and_p2p, serve_local_node_with_p2p,
    serve_local_node_with_shutdown, LocalNodeConfig, DEFAULT_ROUTE_TIMEOUT,
    MAX_CONCURRENT_REQUESTS, MAX_HTTP_BODY_BYTES, PROOF_ROUTE_BODY_BYTES, PROOF_ROUTE_TIMEOUT,
};
pub use p2p_ingress::{P2pConfig, DEFAULT_P2P_RATE_LIMIT_PER_60S};
pub use proof_bridge::{
    canonical_pofp_package_from_lean_result, canonical_pofp_package_from_lean_result_and_source,
    LeanProofBridge, LeanProofBridgePolicy, ProofSubmissionTemplate,
};
pub use receipt_store::FileReceiptStore;
pub use reputation_store::{
    FileReputationLedger, PersistedReputationEvent, REPUTATION_EVENT_SCHEMA,
};
pub use reward_store::{verify_ledger_matches_replay, FileRewardLedger};
pub use runtime::{ReorgOutcome, RuntimeAdmissionState, RuntimeConfig, UsefulBaseMode};
pub use runtime_smoke::{
    run_runtime_smoke, run_runtime_smoke_multi_scenario, run_runtime_smoke_scenario,
    run_runtime_smoke_scenario_file, RuntimeSmokeBlockOutput, RuntimeSmokeInput,
    RuntimeSmokeMultiScenario, RuntimeSmokeOutput, RuntimeSmokeScenario, RuntimeSmokeStep,
};
pub use session_store::FileSessionStore;
pub use state_dir::{
    acquire as acquire_state_dir, ensure_manifest, StateDirError, StateDirGuard, StateManifest,
};
pub use useful_work_store::{
    FileUsefulWorkStore, RewardRecord, UsefulWorkEvent, USEFUL_WORK_STORE_FILE,
};
pub use work_manifest_store::load_work_manifests_from_path;
