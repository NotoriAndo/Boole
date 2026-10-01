//! Boole H1 wrapper for scroll-tech/poseidon-circuit `SeptidonChip` (chip row; child module of
//! `poseidon::septidon::instruction`; `cfg(test)`): the chip's permutation instruction over BN254 `Fr` with the
//! spec the crate hashes with (`<Fr as Hashable>::SpecType`), configured through `PermuteChip::configure` (which
//! enables equality on the initial and final state cells), K = 5 (the septidon test's value).

use halo2_proofs::halo2curves::{bn256::Fr, group::ff::Field};
use halo2_proofs::{
    circuit::{Layouter, SimpleFloorPlanner},
    plonk::{Circuit, ConstraintSystem, Error},
};

use super::super::super::{PermuteChip, PoseidonInstructions, StateWord};
use super::super::SeptidonChip;
use crate::boole_h1::{drive, Io, Spec};
use crate::hash::Hashable;

type S = <Fr as Hashable>::SpecType;

struct PermuteW;

impl Circuit<Fr> for PermuteW {
    type Config = (Io, SeptidonChip);
    type FloorPlanner = SimpleFloorPlanner;
    fn without_witnesses(&self) -> Self {
        PermuteW
    }
    fn configure(meta: &mut ConstraintSystem<Fr>) -> Self::Config {
        let io = Io::configure(meta);
        (io, <SeptidonChip as PermuteChip<Fr, S, 3, 2>>::configure(meta))
    }
    fn synthesize(&self, config: Self::Config, mut layouter: impl Layouter<Fr>) -> Result<(), Error> {
        let chip = <SeptidonChip as PermuteChip<Fr, S, 3, 2>>::construct(config.1.clone());
        let c = config.0.load(&mut layouter, 0, 3)?;
        let st = [StateWord(c[0].clone()), StateWord(c[1].clone()), StateWord(c[2].clone())];
        let out = <SeptidonChip as PoseidonInstructions<Fr, S, 3, 2>>::permute(&chip, &mut layouter, &st)?;
        config.0.expose(&mut layouter, &[out[0].0.clone(), out[1].0.clone(), out[2].0.clone()])
    }
}

#[test]
fn boole_h1_scroll_septidon_chip() {
    drive(
        &Spec { name: "SeptidonChip", k: 5,
                call: "SeptidonChip::permute(&state) over BN254 Fr, spec <Fr as Hashable>::SpecType",
                rule: "repo-test", provenance: "poseidon-circuit/src/poseidon/septidon/tests.rs (K = 5)",
                input_notes: &[] },
        |rng, s| (PermuteW, (0..3).map(|i| if s == 1 { Fr::from(i as u64) } else { Fr::random(&mut *rng) }).collect()),
    );
}
