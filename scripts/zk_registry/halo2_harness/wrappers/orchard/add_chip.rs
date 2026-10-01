//! Boole H1 wrapper for orchard's `AddChip` (child module of `circuit::gadget::add_chip`; `cfg(test)`).
//! Configuration: as `circuit::Circuit::configure` uses the chip (three advice columns with equality enabled);
//! the chip has no test of its own at the pin.  K = 4.

use ff::Field;
use halo2_proofs::{
    circuit::{Layouter, SimpleFloorPlanner},
    plonk::{Circuit, ConstraintSystem, Error},
};

use super::super::AddInstruction;
use super::{AddChip, AddConfig};
use crate::boole_h1::{drive, Fp, Io, Spec};

struct AddW;

impl Circuit<Fp> for AddW {
    type Config = (Io, AddConfig);
    type FloorPlanner = SimpleFloorPlanner;
    fn without_witnesses(&self) -> Self {
        AddW
    }
    fn configure(meta: &mut ConstraintSystem<Fp>) -> Self::Config {
        let io = Io::configure(meta);
        let advices = [meta.advice_column(), meta.advice_column(), meta.advice_column()];
        for advice in advices.iter() {
            meta.enable_equality(*advice);
        }
        (io, AddChip::configure(meta, advices[0], advices[1], advices[2]))
    }
    fn synthesize(&self, config: Self::Config, mut layouter: impl Layouter<Fp>) -> Result<(), Error> {
        let chip = AddChip::construct(config.1.clone());
        let ab = config.0.load(&mut layouter, 0, 2)?;
        let c = chip.add(layouter.namespace(|| "add"), &ab[0], &ab[1])?;
        config.0.expose(&mut layouter, &[c])
    }
}

#[test]
fn boole_h1_orchard_add_chip() {
    drive(
        &Spec { name: "AddChip", k: 4, call: "AddChip::add(a, b)", rule: "repo-derived",
                provenance: "src/circuit.rs Circuit::configure (AddChip over advice columns with equality)",
                input_notes: &[] },
        |rng, _s| (AddW, std::vec![Fp::random(&mut *rng), Fp::random(&mut *rng)]),
    );
}
