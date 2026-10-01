//! Boole H1 wrapper for qed-it/orchard `CircuitZsa` (child module of `circuit::circuit_zsa::tests`; `cfg(test)`):
//! the repository's circuit and instance from `tests::generate_circuit_instance(is_zatoshi_asset = false, ..)`,
//! run as the tests run it (`circuit.to_zsa()`), K = 11.  Its public data are instance cells; the circuit has no
//! result cells.

use super::{generate_circuit_instance, K};
use crate::boole_h1::{drive_cols, Spec};

#[test]
fn boole_h1_qedit_circuit_zsa() {
    drive_cols(
        &Spec { name: "Circuit:CircuitZsa", k: K,
                call: "CircuitZsa on tests::generate_circuit_instance(is_zatoshi_asset = false)",
                rule: "repo-test", provenance: "src/circuit/circuit_zsa.rs tests (K = 11)", input_notes: &[] },
        |rng, _s| {
            let (circuit, instance) = generate_circuit_instance(false, &mut *rng);
            (circuit.to_zsa().expect("ZSA witnesses are present"),
             instance.to_halo2_instance().iter().map(|c| c.to_vec()).collect())
        },
    );
}
