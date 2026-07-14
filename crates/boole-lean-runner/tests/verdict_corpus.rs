//! SC.9c / ADR-0016 (a)(a-1) — cross-platform verdict corpus.
//!
//! The verdict is a pure function of (proof bytes, pinned checker,
//! committed step budget). This test runs a FIXED corpus of sources under
//! FIXED budgets through the canonical checker and compares the resulting
//! verdict tuples byte-for-byte against the committed golden fixture. CI
//! runs it as four concrete jobs (Linux/macOS × debug/release) behind one
//! required aggregate `verdict-corpus` status: a platform- or
//! profile-divergent verdict is a fork vector and must never merge.
//!
//! Wall-clock containment is deliberately NOT in the corpus: it is an
//! availability condition, machine-dependent by definition, and the
//! three-state contract (`budget_verdict.rs`) already pins that it can
//! never masquerade as a verdict.

use boole_lean_runner::{LeanRunner, LeanRunnerConfig, LeanVerdict};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

fn canonical_checker_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crate has workspace root")
        .join("lean")
        .join("checker")
}

fn golden_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crate has workspace root")
        .join("fixtures")
        .join("verdict-corpus")
        .join("golden.json")
}

fn lake_and_lean_available() -> bool {
    Command::new("lake")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
        && Command::new("lean")
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
}

struct CorpusCase {
    id: &'static str,
    source: &'static str,
    max_heartbeats: u64,
    max_rec_depth: u64,
}

/// The fixed corpus. Every case sits FAR from its budget boundary so the
/// verdict has margin on any conforming Lean build of the pinned githash;
/// the point is cross-platform identity, not boundary probing.
const CORPUS: &[CorpusCase] = &[
    CorpusCase {
        id: "accept_trivial_decide",
        source: "theorem corpus_accept : 1 + 1 = 2 := by decide\n",
        max_heartbeats: 400_000,
        max_rec_depth: 512,
    },
    CorpusCase {
        id: "reject_false_statement",
        source: "theorem corpus_false : 1 + 1 = 3 := by decide\n",
        max_heartbeats: 400_000,
        max_rec_depth: 512,
    },
    CorpusCase {
        id: "budget_exceeded_heartbeats",
        source: "theorem corpus_burn : (List.range 400).foldl Nat.add 0 = 79800 := by decide\n",
        max_heartbeats: 1,
        max_rec_depth: 512,
    },
    CorpusCase {
        id: "budget_exceeded_rec_depth",
        source: "theorem corpus_deep : (List.range 400).foldl Nat.add 0 = 79800 := by decide\n",
        max_heartbeats: 400_000,
        max_rec_depth: 512,
    },
    CorpusCase {
        id: "intake_rejects_budget_override_unlock",
        source: "set_option maxHeartbeats 0 in\ntheorem corpus_unlock : 1 + 1 = 2 := by decide\n",
        max_heartbeats: 400_000,
        max_rec_depth: 512,
    },
    CorpusCase {
        id: "intake_rejects_rec_depth_override",
        source: "set_option maxRecDepth 100000 in\ntheorem corpus_rd : 1 + 1 = 2 := by decide\n",
        max_heartbeats: 400_000,
        max_rec_depth: 512,
    },
    CorpusCase {
        id: "intake_rejects_sorry",
        source: "theorem corpus_sorry : 1 + 1 = 2 := by sorry\n",
        max_heartbeats: 400_000,
        max_rec_depth: 512,
    },
];

fn run_case(case: &CorpusCase) -> serde_json::Value {
    let dir = std::env::temp_dir().join(format!(
        "boole-verdict-corpus-{}-{}",
        std::process::id(),
        case.id
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("corpus tmp dir");
    let proof = dir.join("Proof.lean");
    std::fs::write(&proof, case.source).expect("write corpus proof");

    let runner = LeanRunner::new(
        LeanRunnerConfig::new("verdict-corpus")
            .with_package_dir(canonical_checker_dir())
            .with_max_heartbeats(case.max_heartbeats)
            .with_max_rec_depth(case.max_rec_depth)
            // Generous wall clock: containment must never fire here.
            .with_timeout_ms(300_000),
    );
    let entry = match runner.check_file(&proof) {
        Ok(result) => match result.verdict {
            LeanVerdict::Accepted => serde_json::json!({
                "id": case.id, "verdict": "accepted"
            }),
            LeanVerdict::DeterministicReject { reason } => serde_json::json!({
                "id": case.id, "verdict": "deterministic_reject", "reason": reason
            }),
            LeanVerdict::RetryableUnavailable { reason } => serde_json::json!({
                "id": case.id, "verdict": "retryable_unavailable", "reason": reason
            }),
        },
        // Pre-spawn intake rejection (forbidden token) — deterministic,
        // recorded by the token the message names.
        Err(err) => {
            let msg = err.to_string();
            let token = ["maxHeartbeats", "maxRecDepth", "sorry"]
                .iter()
                .find(|token| msg.contains(**token))
                .copied()
                .unwrap_or("other");
            serde_json::json!({
                "id": case.id, "verdict": "intake_rejected", "token": token
            })
        }
    };
    let _ = std::fs::remove_dir_all(&dir);
    entry
}

#[test]
fn verdict_corpus_is_identical_across_platforms_and_profiles() {
    if !lake_and_lean_available() {
        eprintln!("skipping verdict corpus: lake/lean unavailable");
        return;
    }
    let entries: Vec<serde_json::Value> = CORPUS.iter().map(run_case).collect();
    let actual = serde_json::to_string_pretty(&serde_json::json!({
        "schema": "boole.verdict-corpus.v1",
        "entries": entries,
    }))
    .expect("serialize corpus")
        + "\n";
    let digest = hex::encode(Sha256::digest(actual.as_bytes()));
    eprintln!("verdict corpus digest: {digest}");

    if std::env::var("BOOLE_REGEN_VERDICT_CORPUS").is_ok() {
        std::fs::create_dir_all(golden_path().parent().expect("fixture dir")).expect("mkdir");
        std::fs::write(golden_path(), &actual).expect("write golden");
        eprintln!("regenerated {}", golden_path().display());
        return;
    }
    let golden = std::fs::read_to_string(golden_path()).unwrap_or_else(|err| {
        panic!(
            "committed golden verdict corpus missing at {} ({err}) — regenerate with \
             BOOLE_REGEN_VERDICT_CORPUS=1 and commit the result",
            golden_path().display()
        )
    });
    assert_eq!(
        actual, golden,
        "verdict corpus diverged from the committed golden — a platform/profile-dependent \
         verdict is a fork vector (ADR-0016 (a)); if the divergence is an INTENDED checker or \
         budget change, regenerate the golden with BOOLE_REGEN_VERDICT_CORPUS=1 and land it \
         with the change"
    );
}
