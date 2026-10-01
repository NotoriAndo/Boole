//! Boole H1 wrapper for the Orchard Action circuit (child module of `circuit::tests`; `cfg(test)`): the
//! repository's own circuit and instance from `tests::generate_circuit_instance` at the current circuit version
//! (`OrchardCircuitVersion::PostNu6_3`), K = 11.  Its public data are instance cells; the circuit has no result cells.

use super::{generate_circuit_instance, OrchardCircuitVersion, K};
use crate::boole_h1::{drive, Spec};

#[test]
fn boole_h1_orchard_circuit() {
    drive(
        &Spec { name: "Circuit", k: K, call: "orchard::circuit::Circuit (Action circuit, OrchardCircuitVersion::PostNu6_3)",
                rule: "repo-test", provenance: "src/circuit.rs tests::generate_circuit_instance, K = 11",
                input_notes: &[] },
        |rng, _s| {
            let (circuit, instance) = generate_circuit_instance(&mut *rng, OrchardCircuitVersion::PostNu6_3);
            (circuit, instance.to_halo2_instance()[0].to_vec())
        },
    );
}
