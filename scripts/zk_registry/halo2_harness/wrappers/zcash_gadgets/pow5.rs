//! Boole H1 wrappers for `Pow5Chip` instructions (child module of `poseidon::pow5`; `cfg(test)`).
//! Configuration: the repository's `poseidon::pow5::tests::MyHashCircuit::configure` (3 state columns, one
//! partial S-box column, 3 + 3 round-constant columns, constants in `rc_b[0]`), spec `P128Pow5T3` (the tests'
//! `OrchardNullifier`), domain `ConstantLength<2>`, K = 6 (the tests' value).

use ff::Field;
use halo2_proofs::{
    circuit::{Layouter, SimpleFloorPlanner},
    plonk::{Circuit, ConstraintSystem, Error},
};
use crate::boole_h1::StdRng;

use super::{Pow5Chip, Pow5Config, StateWord};
use crate::boole_h1::{drive, Fp, Io, Spec as BSpec};
use crate::poseidon::{
    primitives::{Absorbing, ConstantLength, P128Pow5T3},
    PaddedWord, PoseidonInstructions, PoseidonSpongeInstructions,
};

type Cfg = (Io, Pow5Config<Fp, 3, 2>);
type Chip = Pow5Chip<Fp, 3, 2>;
type Dom = ConstantLength<2>;

const PROV: &str = "halo2_gadgets/src/poseidon/pow5.rs tests::MyHashCircuit::configure (P128Pow5T3, \
                    ConstantLength<2>, K = 6)";

fn configure_pow5(meta: &mut ConstraintSystem<Fp>) -> Cfg {
    let io = Io::configure(meta);
    let state = [meta.advice_column(), meta.advice_column(), meta.advice_column()];
    let partial_sbox = meta.advice_column();
    let rc_a = [meta.fixed_column(), meta.fixed_column(), meta.fixed_column()];
    let rc_b = [meta.fixed_column(), meta.fixed_column(), meta.fixed_column()];
    meta.enable_constant(rc_b[0]);
    (io, Chip::configure::<P128Pow5T3>(meta, state, partial_sbox, rc_a, rc_b))
}

macro_rules! pow5_wrapper {
    ($name:ident, $vals:ty, |$v:ident, $chip:ident, $io:ident, $l:ident| $body:block) => {
        struct $name($vals);
        impl Circuit<Fp> for $name {
            type Config = Cfg;
            type FloorPlanner = SimpleFloorPlanner;
            fn without_witnesses(&self) -> Self {
                unimplemented!("MockProver does not call without_witnesses")
            }
            fn configure(meta: &mut ConstraintSystem<Fp>) -> Cfg {
                configure_pow5(meta)
            }
            #[allow(unused_variables)]
            fn synthesize(&self, config: Cfg, mut layouter: impl Layouter<Fp>) -> Result<(), Error> {
                let $chip = Chip::construct(config.1.clone());
                let $io = config.0.clone();
                let $v = &self.0;
                let $l = &mut layouter;
                $body
            }
        }
    };
}

fn rand_fp(rng: &mut StdRng, s: usize, i: usize) -> Fp {
    match s {
        1 => Fp::ZERO,
        2 => Fp::from(i as u64),
        3 => -Fp::ONE,
        _ => Fp::random(&mut *rng),
    }
}

pow5_wrapper!(PermuteW, (), |v, chip, io, l| {
    let cells = io.load(l, 0, 3)?;
    let state = [StateWord(cells[0].clone()), StateWord(cells[1].clone()), StateWord(cells[2].clone())];
    let out = <Chip as PoseidonInstructions<Fp, P128Pow5T3, 3, 2>>::permute(&chip, l, &state)?;
    io.expose(l, &[out[0].0.clone(), out[1].0.clone(), out[2].0.clone()])
});

#[test]
fn boole_h1_pow5_permute() {
    drive(
        &BSpec { name: "Pow5Chip.permute", k: 6, call: "Pow5Chip::permute(&state) for P128Pow5T3 (width 3, rate 2)",
                 rule: "repo-test", provenance: PROV, input_notes: &[] },
        |rng, s| ((PermuteW(())), (0..3).map(|i| rand_fp(rng, s, i)).collect()),
    );
}

pow5_wrapper!(InitW, (), |v, chip, io, l| {
    let out = <Chip as PoseidonSpongeInstructions<Fp, P128Pow5T3, Dom, 3, 2>>::initial_state(&chip, l)?;
    io.expose(l, &[out[0].0.clone(), out[1].0.clone(), out[2].0.clone()])
});

#[test]
fn boole_h1_pow5_initial_state() {
    drive(
        &BSpec { name: "Pow5Chip.initial_state", k: 6,
                 call: "Pow5Chip::initial_state() for P128Pow5T3, domain ConstantLength<2> (no inputs)",
                 rule: "repo-test", provenance: PROV, input_notes: &[] },
        |_rng, _s| (InitW(()), vec![]),
    );
}

pow5_wrapper!(AddInputW, (), |v, chip, io, l| {
    let cells = io.load(l, 0, 5)?;
    let state = [StateWord(cells[0].clone()), StateWord(cells[1].clone()), StateWord(cells[2].clone())];
    let mut input = Absorbing::init_empty();
    input.absorb(PaddedWord::Message(cells[3].clone())).unwrap();
    input.absorb(PaddedWord::Message(cells[4].clone())).unwrap();
    let out = <Chip as PoseidonSpongeInstructions<Fp, P128Pow5T3, Dom, 3, 2>>::add_input(&chip, l, &state, &input)?;
    io.expose(l, &[out[0].0.clone(), out[1].0.clone(), out[2].0.clone()])
});

#[test]
fn boole_h1_pow5_add_input() {
    drive(
        &BSpec { name: "Pow5Chip.add_input", k: 6,
                 call: "Pow5Chip::add_input(&state, [message word, message word]) for P128Pow5T3, ConstantLength<2>",
                 rule: "repo-test", provenance: PROV, input_notes: &[] },
        |rng, s| (AddInputW(()), (0..5).map(|i| rand_fp(rng, s, i)).collect()),
    );
}

pow5_wrapper!(GetOutputW, (), |v, chip, io, l| {
    let cells = io.load(l, 0, 3)?;
    let state = [StateWord(cells[0].clone()), StateWord(cells[1].clone()), StateWord(cells[2].clone())];
    let mut out = <Chip as PoseidonSpongeInstructions<Fp, P128Pow5T3, Dom, 3, 2>>::get_output(&state);
    let mut words = vec![];
    while let Some(w) = out.squeeze() {
        words.push(w.0);
    }
    io.expose(l, &words)
});

#[test]
fn boole_h1_pow5_get_output() {
    drive(
        &BSpec { name: "Pow5Chip.get_output", k: 6, call: "Pow5Chip::get_output(&state) (the rate part of the state)",
                 rule: "repo-test", provenance: PROV, input_notes: &[] },
        |rng, s| (GetOutputW(()), (0..3).map(|i| rand_fp(rng, s, i)).collect()),
    );
}
