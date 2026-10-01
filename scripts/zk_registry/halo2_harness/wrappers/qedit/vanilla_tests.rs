//! Boole H1 wrapper for qed-it/orchard `CircuitVanilla` (child module of `circuit::circuit_vanilla::tests`;
//! `cfg(test)`): the repository's circuit and instance from `tests::generate_circuit_instance` at
//! `OrchardCircuitVersion::PostNu6_3`, run as the tests run it (`circuit.common_witnesses`), K = 11.  Its public
//! data are instance cells; the circuit has no result cells.

use super::{generate_circuit_instance, OrchardCircuitVersion, K};
use crate::boole_h1::{drive_cols, Spec};

#[test]
fn boole_h1_qedit_circuit_vanilla() {
    drive_cols(
        &Spec { name: "Circuit:CircuitVanilla", k: K,
                call: "CircuitVanilla (OrchardCircuitVersion::PostNu6_3) on tests::generate_circuit_instance",
                rule: "repo-test", provenance: "src/circuit/circuit_vanilla.rs tests (K = 11)", input_notes: &[] },
        |rng, _s| {
            let (circuit, instance) = generate_circuit_instance(&mut *rng, OrchardCircuitVersion::PostNu6_3);
            (circuit.common_witnesses, instance.to_halo2_instance().iter().map(|c| c.to_vec()).collect())
        },
    );
}
