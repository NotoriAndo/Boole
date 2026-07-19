//! PoVFN Phase 0-A zkVM guest: kernel-check a lean4export dependency
//! closure with the vendored Rust Lean kernel checker (nanoda_lib) and
//! commit the binding journal. Any check failure panics, so no proof can
//! be produced for an invalid input (fail-closed).
use risc0_zkvm::guest::env;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Cursor;

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

#[derive(Serialize)]
struct Journal {
    binding: BindingInputs,
    export_sha256: String,
    stmt_hash: String,
    decl_count: usize,
    accepted: bool,
}

const NANODA_CONFIG: &str = r#"{
    "export_file_path": null,
    "permitted_axioms": ["propext", "Classical.choice", "Quot.sound"],
    "unpermitted_axiom_hard_error": true,
    "num_threads": 1,
    "nat_extension": true,
    "string_extension": true,
    "pp_declars": null,
    "pp_output_path": null,
    "print_axioms": false,
    "print_success_message": false
}"#;

fn main() {
    let binding: BindingInputs = env::read();
    let export_bytes: Vec<u8> = env::read();

    let export_sha256 = hex_str(&Sha256::digest(&export_bytes));

    // P-stage binding: the target theorem must be present; commit the
    // structural hash of its statement (type DAG).
    let stmt_hash = stmt_hash::statement_hash(&export_bytes, &binding.thm_full_name)
        .expect("export parse failed")
        .expect("target theorem not present in export");

    // K-stage: full kernel check of every declaration in the closure.
    let cfg: nanoda_lib::util::Config =
        serde_json::from_str(NANODA_CONFIG).expect("config parse");
    let (export_file, skipped) = cfg
        .to_export_file_from_reader(Cursor::new(&export_bytes[..]))
        .expect("export parse (nanoda)");
    assert!(skipped.is_empty(), "unpermitted axioms skipped");
    export_file.check_all_declars();
    let decl_count = export_file.declars.len();

    env::commit(&Journal {
        binding,
        export_sha256,
        stmt_hash,
        decl_count,
        accepted: true,
    });
}

fn hex_str(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}
