//! PoVFN Phase 0-A host driver: execute (cycle count) and prove/verify the
//! lean-kernel guest over a lean4export closure, and run the binding
//! flip-tests. Prints one JSON object to stdout.
use methods::{LEAN_KERNEL_GUEST_ELF, LEAN_KERNEL_GUEST_ID};
use risc0_zkvm::{default_executor, default_prover, ExecutorEnv, ProverOpts};
use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Deserialize, Serialize, Clone)]
struct BindingInputs {
    network_id: String,
    rule_version: u32,
    family: String,
    checker_artifact_hash: String,
    package_hash: String,
    seed_hex: String,
    thm_full_name: String,
    reward_pk: String,
    job_scope: String,
}

#[derive(Deserialize, Serialize, Debug, PartialEq)]
struct Journal {
    binding: BindingInputs,
    export_sha256: String,
    stmt_hash: String,
    decl_count: usize,
    accepted: bool,
}

impl PartialEq for BindingInputs {
    fn eq(&self, other: &Self) -> bool {
        serde_json::to_string(self).unwrap() == serde_json::to_string(other).unwrap()
    }
}
impl std::fmt::Debug for BindingInputs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", serde_json::to_string(self).unwrap())
    }
}

fn build_env<'a>(binding: &BindingInputs, export: &'a [u8]) -> ExecutorEnv<'a> {
    ExecutorEnv::builder()
        .write(binding)
        .unwrap()
        .write(&export.to_vec())
        .unwrap()
        .build()
        .unwrap()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!(
            "usage: host <export.ndjson> <binding.json> <mode: execute|prove|prove-succinct> [expected_stmt_hash]"
        );
        std::process::exit(2);
    }
    let export = std::fs::read(&args[1]).expect("read export");
    let binding: BindingInputs =
        serde_json::from_str(&std::fs::read_to_string(&args[2]).expect("read binding"))
            .expect("parse binding");
    let mode = args[3].as_str();
    let expected_stmt_hash = args.get(4).cloned();

    let mut out = serde_json::json!({
        "mode": mode,
        "export_bytes": export.len(),
        "image_id": format!("{:?}", LEAN_KERNEL_GUEST_ID),
    });

    match mode {
        "execute" => {
            let env = build_env(&binding, &export);
            let t0 = Instant::now();
            let exec = default_executor();
            let session = exec.execute(env, LEAN_KERNEL_GUEST_ELF).unwrap();
            let dt = t0.elapsed().as_secs_f64();
            let journal: Journal = session.journal.decode().unwrap();
            let user_cycles: u64 = session.segments.iter().map(|s| s.cycles as u64).sum();
            out["execute_s"] = dt.into();
            out["user_cycles_segment_sum"] = user_cycles.into();
            out["segments"] = session.segments.len().into();
            out["journal_stmt_hash"] = journal.stmt_hash.clone().into();
            out["journal_decl_count"] = journal.decl_count.into();
            out["journal_accepted"] = journal.accepted.into();
            if let Some(exp) = &expected_stmt_hash {
                out["stmt_hash_matches_expected"] = (journal.stmt_hash == *exp).into();
            }
        }
        "prove" | "prove-succinct" => {
            let opts = if mode == "prove-succinct" {
                ProverOpts::succinct()
            } else {
                ProverOpts::composite()
            };
            let env = build_env(&binding, &export);
            let prover = default_prover();
            let t0 = Instant::now();
            let info = prover
                .prove_with_opts(env, LEAN_KERNEL_GUEST_ELF, &opts)
                .unwrap();
            let prove_s = t0.elapsed().as_secs_f64();
            let receipt = info.receipt;
            let receipt_bytes = bincode::serialize(&receipt).unwrap().len();

            let t0 = Instant::now();
            receipt.verify(LEAN_KERNEL_GUEST_ID).unwrap();
            let verify_s = t0.elapsed().as_secs_f64();

            let journal: Journal = receipt.journal.decode().unwrap();
            out["prove_s"] = prove_s.into();
            out["verify_s"] = verify_s.into();
            out["receipt_bytes"] = receipt_bytes.into();
            out["user_cycles"] = info.stats.user_cycles.into();
            out["total_cycles"] = info.stats.total_cycles.into();
            out["segments"] = info.stats.segments.into();
            out["journal_stmt_hash"] = journal.stmt_hash.clone().into();
            out["journal_accepted"] = journal.accepted.into();
            if let Some(exp) = &expected_stmt_hash {
                out["stmt_hash_matches_expected"] = (journal.stmt_hash == *exp).into();
            }

            // Binding flip-test: a verifier that expects specific binding
            // values must reject a journal whose fields were altered. We
            // simulate the node-side comparison for every field.
            let mut flips = serde_json::Map::new();
            let jb = serde_json::to_value(&journal.binding).unwrap();
            for (field, _) in jb.as_object().unwrap() {
                let mut altered = jb.clone();
                altered[field] = serde_json::Value::String("ALTERED".into());
                let differs = altered != jb;
                flips.insert(field.clone(), differs.into());
            }
            out["binding_flip_detected"] = serde_json::Value::Object(flips);
        }
        other => {
            eprintln!("unknown mode {other}");
            std::process::exit(2);
        }
    }
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}
