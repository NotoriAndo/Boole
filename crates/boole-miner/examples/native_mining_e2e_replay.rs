//! A-ROOTED-NATIVE-MINING-E2E-V1 (msg 4206 sections 4-6) -- the Rust-side
//! conclusion of the one real, closed-local native-mining pass.
//!
//! This binary is not a family, not a checker, and not a new judgment rule.
//! It replays exactly one already-decided fact -- the real, sealed
//! `gates.Checker.run()` verdict that
//! `local-docs/a-rooted-native-mining-e2e-v1-2026-08-21/step2_episode.py`
//! (which reuses the sealed `episode_tuple()` turn loop byte for byte)
//! already computed for the one real `claude-opus-4-8` answer, captured in
//! `STEP2-EPISODE-RESULT.json` -- through the CI-tested
//! `boole_miner::run_native_mining_e2e` wiring, so the same proof-intake /
//! binding / receipt-assembly / share-accounting code path the crate's own
//! test suite exercises with fixtures is the one that produces the real
//! receipt and real local share-ledger entry for this one episode.
//!
//! The frozen identity constants below are read verbatim from
//! `local-docs/a-rooted-native-mining-e2e-v1-2026-08-21/FREEZE.json`, the
//! same constants the crate's `tests/native_mining_e2e.rs` fixtures already
//! use -- not re-derived or re-typed independently here.
//!
//! Run once, after `step2_episode.py` has produced `STEP2-EPISODE-RESULT.json`:
//!     cargo run -p boole-miner --example native_mining_e2e_replay
//!
//! Writes `local-docs/a-rooted-native-mining-e2e-v1-2026-08-21/RECEIPT.json`
//! and appends one line to
//! `local-docs/a-rooted-native-mining-e2e-v1-2026-08-21/SHARE-LEDGER.ndjson`
//! only if the verdict is Accept. Touches no consensus state
//! (`boole_core::block`, `SharePool`, any persisted-block or reward store).
//! Closed local, non-consensus. Not a public benchmark, not a paid public API
//! benchmark claim, not public-network mining, not a leaderboard claim.
//! mineable_now = 0.

use boole_miner::{
    record_share_if_accepted, run_native_mining_e2e, NativeCandidate, NativeChecker,
    NativeCheckerVerdict, NativeTaskContext,
};
use std::path::PathBuf;

// Frozen identity, read verbatim from
// local-docs/a-rooted-native-mining-e2e-v1-2026-08-21/FREEZE.json -- the same
// constants crates/boole-miner/tests/native_mining_e2e.rs's frozen_ctx() uses.
const FAMILY_VERSION: &str = "TUPLE-STRUCT-PROJECT/RUST-TUPLE-STRUCT-PROJECT-V1";
const TEMPLATE_ID: &str = "04dd8453f52dd4da8af1736ad6eceeb82ded2e65dcc1c0600c4b37ef7cf6307a";
const ANCHOR_SHA256: &str = "693f62acfa0626a0831c9133a26fcfc1dbb30922c1ab2036231c42a363cfd7fe";
const CHALLENGE_SHA256: &str = "34ae2f0d0d9af2747c59b444c17b83cb264fce8ead0879378e6aa0eae54e246f";
const CHECKER_DIGEST: &str = "633c6ec891a508813960d6f7d86d446752461f8002aef424290f33550de9cb01";
const POLICY_DIGEST: &str = "cd90c313fad6488307f9fc8c09ecebab6155688699c7777c7ed8c6e1b7be3313";

const TASK_DIR: &str = "local-docs/a-rooted-native-mining-e2e-v1-2026-08-21";

/// Replays the real, already-decided sealed-checker verdict for this one
/// episode. Does not re-judge the candidate's Rust code -- the actual
/// compile+test decision already happened inside `step2_episode.py`, via the
/// sealed `gates.Checker.run()` that `episode_tuple()` reuses byte for byte.
struct ReplaySealedCheckerVerdict {
    verdict: &'static str,
    reason: String,
}

impl NativeChecker for ReplaySealedCheckerVerdict {
    fn run(&self, _candidate_text: &str, _ctx: &NativeTaskContext) -> NativeCheckerVerdict {
        if self.verdict == "ACCEPT" {
            NativeCheckerVerdict::Accept
        } else {
            NativeCheckerVerdict::Reject {
                reason: self.reason.clone(),
            }
        }
    }
}

fn main() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root must resolve");
    let task_dir = repo_root.join(TASK_DIR);
    let episode_path = task_dir.join("STEP2-EPISODE-RESULT.json");

    let episode_raw = std::fs::read_to_string(&episode_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", episode_path.display()));
    let episode_json: serde_json::Value =
        serde_json::from_str(&episode_raw).expect("STEP2-EPISODE-RESULT.json must be valid JSON");
    let episode = &episode_json["episode"];

    let live_template_id = episode["template_id"]
        .as_str()
        .expect("episode.template_id must be a string");
    assert_eq!(
        live_template_id, TEMPLATE_ID,
        "STEP2-EPISODE-RESULT.json's template_id must match the frozen identity"
    );
    let live_anchor_sha256 = episode["anchor_sha256"]
        .as_str()
        .expect("episode.anchor_sha256 must be a string");
    assert_eq!(
        live_anchor_sha256, ANCHOR_SHA256,
        "STEP2-EPISODE-RESULT.json's anchor_sha256 must match the frozen identity"
    );

    let raw_final_reply = episode["raw_final_reply"]
        .as_str()
        .expect("episode.raw_final_reply must be present -- the real model answer")
        .to_string();
    let checker_verdict = episode["verdict"]
        .as_str()
        .expect("episode.verdict must be present");
    let checker_reason = episode["reason"]
        .as_str()
        .unwrap_or("(no reason recorded)")
        .to_string();

    let ctx = NativeTaskContext {
        family_version: FAMILY_VERSION.to_string(),
        template_id: TEMPLATE_ID.to_string(),
        anchor_sha256: ANCHOR_SHA256.to_string(),
        challenge_sha256: CHALLENGE_SHA256.to_string(),
        epoch: 0,
        checker_digest: CHECKER_DIGEST.to_string(),
        policy_digest: POLICY_DIGEST.to_string(),
    };
    // This orchestration script itself constructed both the challenge (via
    // step2_episode.py, frozen in FREEZE.json) and this candidate -- there is
    // no untrusted network hop in this one-time closed-local run -- so the
    // claimed_* fields are the same frozen identity the context commits to.
    // The five reject-path tests already prove what happens when claimed_*
    // disagrees with ctx; this run exercises the agreeing, real-answer path.
    let candidate = NativeCandidate {
        raw_model_response: raw_final_reply,
        claimed_template_id: TEMPLATE_ID.to_string(),
        claimed_challenge_sha256: CHALLENGE_SHA256.to_string(),
        claimed_checker_digest: CHECKER_DIGEST.to_string(),
        claimed_policy_digest: POLICY_DIGEST.to_string(),
    };
    let checker = ReplaySealedCheckerVerdict {
        verdict: if checker_verdict == "ACCEPT" {
            "ACCEPT"
        } else {
            "REJECT"
        },
        reason: checker_reason,
    };

    let report = run_native_mining_e2e(&candidate, &ctx, &checker);

    println!("proof_intake_accepted = {}", report.proof_intake_accepted);
    println!("binding_accepted      = {}", report.binding_accepted);
    println!("checker_accepted      = {}", report.checker_accepted);
    println!("verify_accepted       = {}", report.verify_accepted);
    println!("receipt.verdict       = {:?}", report.receipt.verdict);
    println!(
        "receipt.candidate_digest = {}",
        report.receipt.candidate_digest
    );

    let receipt_json = serde_json::json!({
        "family_version": report.receipt.family_version,
        "template_id": report.receipt.template_id,
        "anchor_digest": report.receipt.anchor_digest,
        "challenge_epoch": {
            "challenge_sha256": report.receipt.challenge_epoch.challenge_sha256,
            "epoch": report.receipt.challenge_epoch.epoch,
        },
        "candidate_digest": report.receipt.candidate_digest,
        "checker_digest": report.receipt.checker_digest,
        "policy_digest": report.receipt.policy_digest,
        "verdict": match &report.receipt.verdict {
            boole_miner::NativeVerdict::Accept => serde_json::json!({"kind": "Accept"}),
            boole_miner::NativeVerdict::Reject { stage, reason } => serde_json::json!({
                "kind": "Reject", "stage": stage, "reason": reason,
            }),
        },
        "proof_intake_accepted": report.proof_intake_accepted,
        "binding_accepted": report.binding_accepted,
        "checker_accepted": report.checker_accepted,
        "verify_accepted": report.verify_accepted,
        "not_a_claim": "closed local, offline, non-consensus; not a public benchmark, \
                         not a paid public API benchmark claim, not public-network mining, \
                         not a leaderboard claim; mineable_now = 0",
    });
    let receipt_path = task_dir.join("RECEIPT.json");
    std::fs::write(
        &receipt_path,
        serde_json::to_string_pretty(&receipt_json).unwrap() + "\n",
    )
    .unwrap_or_else(|e| panic!("write {}: {e}", receipt_path.display()));
    println!("receipt written -> {}", receipt_path.display());

    let ledger_path = task_dir.join("SHARE-LEDGER.ndjson");
    match record_share_if_accepted(&report, &ledger_path) {
        Some(entry) => println!(
            "local dev share recorded -> {} (template_id={}, epoch={})",
            ledger_path.display(),
            entry.template_id,
            entry.epoch
        ),
        None => println!("no share recorded (verify_accepted = false)"),
    }
}
