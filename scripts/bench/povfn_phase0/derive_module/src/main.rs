//! Phase 0-A helper: re-derive the canonical v1-lenbound Lean module,
//! expected statement, and canon package for a seed — exactly what the
//! node-side re-verifier does before invoking the pinned checker.
//! Throwaway experiment tool; reads nothing but its arguments.
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        eprintln!("usage: povfn-derive-module <seed_hex> <checker_artifact_hash> <mode: module|statement|canon>");
        std::process::exit(2);
    }
    let seed_hex = &args[1];
    let checker_artifact_hash = &args[2];
    let mode = args[3].as_str();

    let instance = boole_core::family_v1_lenbound::generate_from_hex(seed_hex)
        .expect("bad seed hex");
    let proof = boole_core::family_v1_lenbound::render_canonical_proof(&instance);
    match mode {
        "module" => {
            print!(
                "{}",
                boole_core::family_v1_lenbound::lean_module(&instance, &proof)
            );
        }
        "statement" => {
            println!(
                "∀ (xs : List Int), {}",
                boole_core::family_v1_lenbound::theorem_rhs(&instance)
            );
        }
        "canon" => {
            let verifier_hash = boole_core::lean_bound_verifier_hash("v1-lenbound");
            let source = boole_core::family_v1_lenbound::lean_module(&instance, &proof);
            let package = boole_core::lean_bound_canon_package(
                &verifier_hash,
                checker_artifact_hash,
                &source,
            );
            use sha2::{Digest, Sha256};
            println!("package_hex={}", hex::encode(&package));
            println!("canon_hash={}", hex::encode(Sha256::digest(&package)));
        }
        _ => {
            eprintln!("unknown mode {mode}");
            std::process::exit(2);
        }
    }
}
