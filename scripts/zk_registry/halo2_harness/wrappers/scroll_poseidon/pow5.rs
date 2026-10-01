//! Boole H1 wrapper for scroll-tech/poseidon-circuit `Pow5Chip` (chip row; child module of `poseidon::pow5`;
//! `cfg(test)`): the chip's permutation instruction, configured as `poseidon::pow5::tests::PermuteCircuit` does
//! (3 state columns, one partial S-box column, 3 + 3 round-constant columns), spec `P128Pow5T3` over the Pasta
//! field of the tests, K = 6.

use halo2_proofs::halo2curves::group::ff::Field;
use halo2_proofs::{
    circuit::{Layouter, SimpleFloorPlanner},
    plonk::{Circuit, ConstraintSystem, Error},
};
use poseidon_base::primitives::{pasta::Fp, P128Pow5T3};

use super::{PoseidonInstructions, Pow5Chip, Pow5Config, StateWord};
use crate::boole_h1::{drive, Io, Spec};

type S = P128Pow5T3<Fp>;

struct PermuteW;

impl Circuit<Fp> for PermuteW {
    type Config = (Io, Pow5Config<Fp, 3, 2>);
    type FloorPlanner = SimpleFloorPlanner;
    fn without_witnesses(&self) -> Self {
        PermuteW
    }
    fn configure(meta: &mut ConstraintSystem<Fp>) -> Self::Config {
        let io = Io::configure(meta);
        let state = [meta.advice_column(), meta.advice_column(), meta.advice_column()];
        let partial_sbox = meta.advice_column();
        let rc_a = [meta.fixed_column(), meta.fixed_column(), meta.fixed_column()];
        let rc_b = [meta.fixed_column(), meta.fixed_column(), meta.fixed_column()];
        (io, Pow5Chip::configure::<S>(meta, state, partial_sbox, rc_a, rc_b))
    }
    fn synthesize(&self, config: Self::Config, mut layouter: impl Layouter<Fp>) -> Result<(), Error> {
        let chip = Pow5Chip::construct(config.1.clone());
        let c = config.0.load(&mut layouter, 0, 3)?;
        let st = [StateWord(c[0].clone()), StateWord(c[1].clone()), StateWord(c[2].clone())];
        let out = <Pow5Chip<Fp, 3, 2> as PoseidonInstructions<Fp, S, 3, 2>>::permute(&chip, &mut layouter, &st)?;
        config.0.expose(&mut layouter, &[out[0].0.clone(), out[1].0.clone(), out[2].0.clone()])
    }
}

#[test]
fn boole_h1_scroll_pow5_chip() {
    drive(
        &Spec { name: "Pow5Chip", k: 6, call: "Pow5Chip::permute(&state) for P128Pow5T3 over the Pasta field",
                rule: "repo-test", provenance: "poseidon-circuit/src/poseidon/pow5.rs tests::PermuteCircuit (K = 6)",
                input_notes: &[] },
        |rng, s| (PermuteW, (0..3).map(|i| if s == 1 { Fp::from(i as u64) } else { Fp::random(&mut *rng) }).collect()),
    );
}
