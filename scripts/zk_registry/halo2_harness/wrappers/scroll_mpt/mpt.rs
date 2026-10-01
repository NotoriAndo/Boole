//! Boole H1 wrapper for scroll-tech/mpt-circuit `TestCircuit` (the row): the repository's own circuit as
//! `tests::mock_prove` runs it (N_ROWS = 8 * 256 + 1, K = 14) on the trace of its
//! `existing_account_balance_update` test (`MPTProofType::BalanceChanged`).  Its witness is the trace; it has no
//! instance cells and no result cells; the run measures the circuit's layout.

use halo2_mpt_circuits::{serde::SMTTrace, MPTProofType, TestCircuit};

use crate::boole_h1::{drive_cols, Spec};

const N_ROWS: usize = 8 * 256 + 1;

pub fn run(name: &str) {
    match name {
        "TestCircuit" => drive_cols(
            &Spec { name: "TestCircuit", k: 14,
                    call: "TestCircuit::new(8 * 256 + 1, [(BalanceChanged, traces/existing_account_balance_update.json)])",
                    rule: "repo-test", provenance: "src/tests.rs mock_prove / existing_account_balance_update (K = 14)",
                    input_notes: &[] },
            |_rng, _s| {
                let trace: SMTTrace =
                    serde_json::from_str(include_str!("traces/existing_account_balance_update.json")).unwrap();
                (TestCircuit::new(N_ROWS, vec![(MPTProofType::BalanceChanged, trace)]), vec![])
            },
        ),
        other => panic!("unknown target {}", other),
    }
}
