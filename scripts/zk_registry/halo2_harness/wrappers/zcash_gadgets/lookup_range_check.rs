//! Boole H1 wrappers for `short_range_check` of both lookup range-check configs (child module of
//! `utilities::lookup_range_check`; `cfg(test)`).  Configuration: the repository's
//! `tests::MyShortRangeCheckCircuit::configure` (one running-sum column, a 10-bit table column, a constants column;
//! the 4/5-bit variant adds its tag column), table loaded by `load_range_check_table`, `num_bits = 6` (a value of the
//! tests' `short_range_check` cases), K = 11.  The instruction has no results.

use ff::Field;
use halo2_proofs::{
    circuit::{Layouter, SimpleFloorPlanner},
    plonk::{Circuit, ConstraintSystem, Error},
};
use rand::Rng;
use crate::boole_h1::StdRng;

use super::{LookupRangeCheck, PallasLookupRangeCheck4_5BConfig, PallasLookupRangeCheckConfig};
use crate::boole_h1::{drive, Fp, Io, Spec as BSpec};

const NUM_BITS: usize = 6;

macro_rules! short_wrapper {
    ($name:ident, $cfg:ty) => {
        struct $name;
        impl Circuit<Fp> for $name {
            type Config = (Io, $cfg);
            type FloorPlanner = SimpleFloorPlanner;
            fn without_witnesses(&self) -> Self {
                unimplemented!("MockProver does not call without_witnesses")
            }
            fn configure(meta: &mut ConstraintSystem<Fp>) -> Self::Config {
                let io = Io::configure(meta);
                let running_sum = meta.advice_column();
                let table_idx = meta.lookup_table_column();
                let constants = meta.fixed_column();
                meta.enable_constant(constants);
                (io, <$cfg>::configure(meta, running_sum, table_idx))
            }
            fn synthesize(&self, config: Self::Config, mut layouter: impl Layouter<Fp>) -> Result<(), Error> {
                config.1.load_range_check_table(&mut layouter)?;
                let e = config.0.load(&mut layouter, 0, 1)?.remove(0);
                let lk = config.1.clone();
                layouter.assign_region(
                    || "short range check",
                    |mut region| {
                        let element = e.copy_advice(|| "element", &mut region, lk.config().running_sum, 0)?;
                        lk.short_range_check(&mut region, element, NUM_BITS)
                    },
                )?;
                config.0.expose(&mut layouter, &[])
            }
        }
    };
}

short_wrapper!(ShortW, PallasLookupRangeCheckConfig);
short_wrapper!(Short45W, PallasLookupRangeCheck4_5BConfig);

fn element(rng: &mut StdRng, s: usize) -> Fp {
    match s {
        1 => Fp::ZERO,
        2 => Fp::from((1u64 << NUM_BITS) - 1),
        _ => Fp::from(rng.gen_range(0..(1u64 << NUM_BITS))),
    }
}

#[test]
fn boole_h1_lookup_short_range_check() {
    drive(
        &BSpec { name: "LookupRangeCheckConfig.short_range_check", k: 11,
                 call: "LookupRangeCheckConfig::short_range_check(element, num_bits = 6) (no results)",
                 rule: "repo-test",
                 provenance: "halo2_gadgets/src/utilities/lookup_range_check.rs tests::MyShortRangeCheckCircuit",
                 input_notes: &[] },
        |rng, s| (ShortW, vec![element(rng, s)]),
    );
}

#[test]
fn boole_h1_lookup_4_5b_short_range_check() {
    drive(
        &BSpec { name: "LookupRangeCheck4_5BConfig.short_range_check", k: 11,
                 call: "LookupRangeCheck4_5BConfig::short_range_check(element, num_bits = 6) (no results)",
                 rule: "repo-test",
                 provenance: "halo2_gadgets/src/utilities/lookup_range_check.rs tests::MyShortRangeCheckCircuit",
                 input_notes: &[] },
        |rng, s| (Short45W, vec![element(rng, s)]),
    );
}
