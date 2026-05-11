pub mod bounty_client;
pub mod canonicalizer;
pub mod chain_head;
pub mod cli;
pub mod family_v031;
pub mod grinder;
pub mod http_client;
pub mod http_runner;
pub mod llm_driver;
pub mod local_verify;
pub mod mining_loop;
pub mod proof_package;
pub mod state;
pub mod submit_client;
pub mod target_emitter;

pub use bounty_client::{BountyClient, BountyProofInputs, BountyProofResult};
pub use canonicalizer::{encode_placeholder_bppk, Canonicalizer, StructuralCanonicalizer, Target};
pub use chain_head::{ChainHead, ChainHeadError, ChainHeadFetcher, HttpChainHeadFetcher};
pub use grinder::{
    grind_share, grind_submission_pow, grind_ticket, CounterNonce, GrindProgress,
    GrindShareOutcome, GrindSubmitOutcome, GrindTicketOutcome, GrinderConfig, NonceSource,
    OsRngNonce,
};
pub use http_client::{percent_encode_component, HttpClient, HttpError, HttpResponse};
pub use http_runner::{HttpRunner, HttpRunnerError, HttpRunnerResponse, ReqwestHttpRunner};
pub use llm_driver::{
    create_driver, extract_proof_source, with_retry, AgentCliDriver, AnthropicDriver,
    ClaudeCliDriver, DriverConfigError, GenerateResult, GoogleDriver, LLMBackend, LLMDriverConfig,
    MockDriver, MockResponse, OpenAiCompatDriver, OpenAiDriver, ProcessError, ProcessRunner,
    ProverDriver, RejectionReason, RetryConfig, Sleeper, StdProcessRunner, Strategy, ThreadSleeper,
    ANTHROPIC_API_VERSION, ANTHROPIC_DEFAULT_BASE_URL, ANTHROPIC_DEFAULT_MAX_TOKENS,
    GOOGLE_DEFAULT_BASE_URL, GOOGLE_DEFAULT_MAX_TOKENS, OPENAI_COMPAT_DEFAULT_MAX_TOKENS,
    OPENAI_DEFAULT_BASE_URL, OPENAI_DEFAULT_MAX_TOKENS,
};
pub use local_verify::{
    AcceptingVerifier, LeanVerifier, RejectingVerifier, Verifier, VerifyReason, VerifyResult,
};
pub use mining_loop::{
    run_mining_loop, DefaultPromptBuilder, FixedChainHead, LlmOutcomeKind, MiningEvent,
    MiningLoopDeps, MiningLoopOptions, MiningLoopSummary, MiningRunContext, MiningRunDriverMode,
    MiningRunTargetMode, MiningRunVerifierMode, PromptBuilder,
};
pub use proof_package::{
    bppk_canon_hash, walk_bppk, BppkBuilder, BppkDecodeError, BppkWalkResult, FORMAT_VERSION,
    MAGIC, MAX_WALK_DEPTH,
};
pub use state::{
    default_state_path, generate_miner_state, load_state, pubkey_to_address, save_state,
    signing_key_from_state, state_exists, update_config, verifying_key_from_state, ConfigPatch,
    DispatcherConfig, LlmConfig, MinerState, MinerStateConfig, StateError,
};
pub use submit_client::{
    AnnounceTicketInputs, AnnounceTicketResult, SubmitClient, SubmitInputs, SubmitResult, Submitter,
};
pub use target_emitter::{
    target_seed, FamilyV031TargetEmitter, FixedSeedTargetEmitter, StubTargetEmitter,
    TargetEmitArgs, TargetEmitter,
};
