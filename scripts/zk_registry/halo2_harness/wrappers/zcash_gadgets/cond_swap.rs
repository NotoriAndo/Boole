//! Boole H1 wrappers for `CondSwapChip` instructions (child module of `utilities::cond_swap`; `cfg(test)`).
//! Configuration: the repository's tests — `swap`: `cond_swap::tests` (5 advice columns,
//! `CondSwapChip::configure`); `mux`: the mux test (equality enabled on every advice column).  K = 3 (the tests use
//! small K; the driver raises K until the layout fits).

use ff::Field;
use halo2_proofs::{
    circuit::{Layouter, SimpleFloorPlanner, Value},
    plonk::{Circuit, ConstraintSystem, Error},
};
use crate::boole_h1::StdRng;

use super::{CondSwapChip, CondSwapConfig, CondSwapInstructions};
use crate::boole_h1::{drive, Fp, Io, Spec as BSpec};

type Cfg = (Io, CondSwapConfig);

fn rand_fp(rng: &mut StdRng, s: usize) -> Fp {
    match s {
        1 => Fp::ZERO,
        2 => -Fp::ONE,
        _ => Fp::random(&mut *rng),
    }
}

struct SwapW(Fp, bool);

impl Circuit<Fp> for SwapW {
    type Config = Cfg;
    type FloorPlanner = SimpleFloorPlanner;
    fn without_witnesses(&self) -> Self {
        unimplemented!("MockProver does not call without_witnesses")
    }
    fn configure(meta: &mut ConstraintSystem<Fp>) -> Cfg {
        let io = Io::configure(meta);
        let advices = [meta.advice_column(), meta.advice_column(), meta.advice_column(), meta.advice_column(),
                       meta.advice_column()];
        // the swapped outputs are copied to the output column: equality on every advice column, as the
        // repository's mux test configures it
        for advice in advices.iter() {
            meta.enable_equality(*advice);
        }
        (io, CondSwapChip::<Fp>::configure(meta, advices))
    }
    fn synthesize(&self, config: Cfg, mut layouter: impl Layouter<Fp>) -> Result<(), Error> {
        let chip = CondSwapChip::<Fp>::construct(config.1.clone());
        let a = config.0.load(&mut layouter, 0, 1)?.remove(0);
        let (x, y) = chip.swap(layouter.namespace(|| "swap"), (a, Value::known(self.0)), Value::known(self.1))?;
        config.0.expose(&mut layouter, &[x, y])
    }
}

#[test]
fn boole_h1_cond_swap_swap() {
    drive(
        &BSpec { name: "CondSwapChip.swap", k: 3,
                 call: "CondSwapChip::swap((a, b), swap); inputs: a and the cells the instruction witnesses for b and \
                        swap",
                 rule: "repo-test",
                 provenance: "halo2_gadgets/src/utilities/cond_swap.rs tests::cond_swap (5 advice columns; equality on every \
                              advice column as in tests::test_mux, so the results can be copied out)",
                 input_notes: &[("swap", "witness b"), ("swap", "swap")] },
        |rng, s| {
            let a = rand_fp(rng, s);
            let b = rand_fp(rng, s + 7);
            (SwapW(b, s % 2 == 1), vec![a])
        },
    );
}

struct MuxW;

impl Circuit<Fp> for MuxW {
    type Config = Cfg;
    type FloorPlanner = SimpleFloorPlanner;
    fn without_witnesses(&self) -> Self {
        unimplemented!("MockProver does not call without_witnesses")
    }
    fn configure(meta: &mut ConstraintSystem<Fp>) -> Cfg {
        let io = Io::configure(meta);
        let advices = [meta.advice_column(), meta.advice_column(), meta.advice_column(), meta.advice_column(),
                       meta.advice_column()];
        for advice in advices.iter() {
            meta.enable_equality(*advice);
        }
        (io, CondSwapChip::<Fp>::configure(meta, advices))
    }
    fn synthesize(&self, config: Cfg, mut layouter: impl Layouter<Fp>) -> Result<(), Error> {
        let chip = CondSwapChip::<Fp>::construct(config.1.clone());
        let c = config.0.load(&mut layouter, 0, 3)?;
        let out = chip.mux(&mut layouter, c[0].clone(), c[1].clone(), c[2].clone())?;
        config.0.expose(&mut layouter, &[out])
    }
}

#[test]
fn boole_h1_cond_swap_mux() {
    drive(
        &BSpec { name: "CondSwapChip.mux", k: 3, call: "CondSwapChip::mux(choice, left, right), choice in {0, 1}",
                 rule: "repo-test",
                 provenance: "halo2_gadgets/src/utilities/cond_swap.rs tests::test_mux (equality on every advice \
                              column)",
                 input_notes: &[] },
        |rng, s| {
            let choice = if s % 2 == 0 { Fp::ZERO } else { Fp::ONE };
            (MuxW, vec![choice, rand_fp(rng, s), rand_fp(rng, s + 3)])
        },
    );
}
