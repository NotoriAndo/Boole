mod bounty_client;
mod canonicalizer;
mod chain_head;
pub mod cli;
#[allow(dead_code)]
// N0.4b — the v1-lenbound family generator/renderer moved to boole-core so
// the node can re-derive a share's canonical Lean source (deep_verify_block)
// without depending on boole-miner. Re-exported here as `crate::family_v1_lenbound`
// so existing miner-internal references resolve unchanged.
pub use boole_core::family_v1_lenbound;
mod grinder;
mod http_client;
mod http_runner;
#[allow(dead_code)]
mod llm_driver;
mod local_verify;
mod mining_loop;
mod proof_intake;
mod proof_package;
mod proof_signer;
mod state;
mod submit_client;
mod target_emitter;

pub use bounty_client::{BountyClient, BountyProofInputs, BountyProofResult};
pub use canonicalizer::{
    encode_placeholder_bppk, live_canonicalizer, CanonError, Canonicalizer, LeanBoundCanonicalizer,
    StructuralCanonicalizer, Target,
};
pub use chain_head::{ChainHead, ChainHeadError, ChainHeadFetcher, HttpChainHeadFetcher};
pub use cli::{
    evaluate_paid_api_policy, paid_api_optin_total, PaidApiOptInCounter, PaidApiPolicyError,
    PaidApiPolicyOutcome, EXIT_CODE_POLICY_REFUSED, PAID_LLM_ALLOW_ENV,
};
pub use family_v1_lenbound::helper_manifest as v1_lenbound_helper_manifest;
pub use grinder::{
    grind_share, grind_submission_pow, grind_ticket, CounterNonce, GrindProgress,
    GrindShareOutcome, GrindSubmitOutcome, GrindTicketOutcome, GrinderConfig, NonceSource,
    OsRngNonce,
};
pub use http_client::{percent_encode_component, HttpClient, HttpError, HttpResponse};
pub use http_runner::{HttpRunner, HttpRunnerError, HttpRunnerResponse, ReqwestHttpRunner};
pub use llm_driver::{
    create_driver, with_retry, AgentCliDriver, AnthropicDriver, ClaudeCliDriver, DriverConfigError,
    GenerateResult, GoogleDriver, LLMBackend, LLMDriverConfig, MockDriver, MockResponse,
    OpenAiCompatDriver, OpenAiDriver, ProcessError, ProcessRunner, ProverDriver, RejectionReason,
    RetryConfig, Sleeper, StdProcessRunner, Strategy, ThreadSleeper, ANTHROPIC_API_VERSION,
    ANTHROPIC_DEFAULT_BASE_URL, ANTHROPIC_DEFAULT_MAX_TOKENS, GOOGLE_DEFAULT_BASE_URL,
    GOOGLE_DEFAULT_MAX_TOKENS, OPENAI_COMPAT_DEFAULT_MAX_TOKENS, OPENAI_DEFAULT_BASE_URL,
    OPENAI_DEFAULT_MAX_TOKENS,
};
pub use local_verify::{LeanVerifier, RejectingVerifier, Verifier, VerifyReason, VerifyResult};
// P1.9 — `AcceptingVerifier` is the always-accept bypass stub. Public
// re-export only when `dev-tools` is enabled so a release-mode
// downstream cannot link against the bypass.
#[cfg(feature = "dev-tools")]
pub use local_verify::AcceptingVerifier;
pub use mining_loop::{
    run_mining_loop, AgentRuntimeReport, DefaultPromptBuilder, FixedChainHead, HeadAdvanceReason,
    LlmOutcomeKind, MiningEvent, MiningLoopDeps, MiningLoopOptions, MiningLoopOutcome,
    MiningLoopSummary, MiningRunContext, MiningRunDriverMode, MiningRunTargetMode,
    MiningRunVerifierMode, PromptBuilder, ProtocolReport, BOOLE_PROOF_SUBMISSION_CONTRACT_V1,
};
pub use proof_intake::{
    extract_proof_source, ProofCandidate, ProofEnvelope, ProofIntakeV1, ProofTransport,
    PROOF_BODY_CONTRACT_VERSION, PROOF_CANONICALIZER_VERSION,
};
pub use proof_package::{
    bppk_canon_hash, expr_tag, level_tag, lit_tag, walk_bppk, BppkBuilder, BppkDecodeError,
    BppkWalkResult, FORMAT_VERSION, MAGIC, MAX_WALK_DEPTH,
};
pub use proof_signer::{AgentSigner, KeySigner, ProofSigner};
pub use state::{
    canonical_state_path_with, default_state_path, generate_miner_state, legacy_candidates_with,
    load_state, pubkey_to_address, save_state, signing_key_from_state, state_exists,
    try_migrate_legacy_state_with, update_config, verifying_key_from_state, ConfigPatch,
    DispatcherConfig, LegacyMigration, LlmConfig, MinerState, MinerStateConfig, StateEnv,
    StateError,
};
pub use submit_client::{
    AnnounceTicketInputs, AnnounceTicketResult, SubmitClient, SubmitInputs, SubmitRejectionKind,
    SubmitResult, Submitter,
};
pub use target_emitter::{
    target_seed, FamilyV1LengthBoundTargetEmitter, FixedSeedTargetEmitter, StubTargetEmitter,
    TargetEmitArgs, TargetEmitter,
};
