//! Boole H1 wrappers for darkfi's halo2 chips (`src/zk/gadget/*.rs` at the pin), one per chip row: every
//! result-producing instruction of the chip is called once on the wrapper's inputs.  Configurations follow the
//! gadgets' own tests (column layout, equality, K); `IsEqualChip` has no test at the pin and follows `src/zk/vm.rs`
//! (its advice columns have equality enabled there).

use ff::Field;
use halo2_proofs::{
    circuit::{Layouter, SimpleFloorPlanner},
    plonk::{Circuit, ConstraintSystem, Error},
};
use rand::Rng;
use crate::boole_h1::StdRng;

use crate::boole_h1::{drive, Fp, Io, Spec};
use crate::zk::gadget::{
    arithmetic::{ArithChip, ArithConfig, ArithInstruction},
    cond_select::{ConditionalSelectChip, ConditionalSelectConfig},
    is_equal::{AssertEqualChip, AssertEqualConfig, IsEqualChip, IsEqualConfig},
    less_than::{LessThanChip, LessThanConfig},
    native_range_check::{NativeRangeCheckChip, NativeRangeCheckConfig},
    small_range_check::{SmallRangeCheckChip, SmallRangeCheckConfig},
};

fn rand_fp(rng: &mut StdRng, s: usize) -> Fp {
    match s {
        1 => Fp::ZERO,
        2 => -Fp::ONE,
        _ => Fp::random(&mut *rng),
    }
}

macro_rules! wrapper {
    ($name:ident, $cfg:ty, |$meta:ident| $conf:block, |$c:ident, $io:ident, $l:ident| $body:block) => {
        struct $name;
        impl Circuit<Fp> for $name {
            type Config = (Io, $cfg);
            type FloorPlanner = SimpleFloorPlanner;
            type Params = ();
            fn without_witnesses(&self) -> Self {
                $name
            }
            fn configure($meta: &mut ConstraintSystem<Fp>) -> Self::Config {
                let io = Io::configure($meta);
                (io, $conf)
            }
            #[allow(unused_variables)]
            fn synthesize(&self, config: Self::Config, mut layouter: impl Layouter<Fp>) -> Result<(), Error> {
                let $io = config.0.clone();
                let $c = config.1.clone();
                let $l = &mut layouter;
                $body
            }
        }
    };
}

wrapper!(ArithW, ArithConfig, |meta| {
    let advices = [meta.advice_column(), meta.advice_column(), meta.advice_column()];
    for advice in advices.iter() {
        meta.enable_equality(*advice);
    }
    ArithChip::configure(meta, advices[0], advices[1], advices[2])
}, |c, io, l| {
    let chip = ArithChip::<Fp>::construct(c);
    let ab = io.load(l, 0, 2)?;
    let s = chip.add(l.namespace(|| "add"), &ab[0], &ab[1])?;
    let d = chip.sub(l.namespace(|| "sub"), &ab[0], &ab[1])?;
    let m = chip.mul(l.namespace(|| "mul"), &ab[0], &ab[1])?;
    io.expose(l, &[s, d, m])
});

wrapper!(CondSelectW, ConditionalSelectConfig<Fp>, |meta| {
    let advices = [meta.advice_column(), meta.advice_column(), meta.advice_column(), meta.advice_column()];
    ConditionalSelectChip::configure(meta, advices)
}, |c, io, l| {
    let chip = ConditionalSelectChip::<Fp>::construct(c);
    let x = io.load(l, 0, 3)?;
    let out = chip.conditional_select(l, x[0].clone(), x[1].clone(), x[2].clone())?;
    io.expose(l, &[out])
});

wrapper!(IsEqualW, IsEqualConfig<Fp>, |meta| {
    let advices = [meta.advice_column(), meta.advice_column(), meta.advice_column(), meta.advice_column()];
    for advice in advices.iter() {
        meta.enable_equality(*advice);
    }
    IsEqualChip::configure(meta, advices)
}, |c, io, l| {
    let chip = IsEqualChip::<Fp>::construct(c);
    let x = io.load(l, 0, 2)?;
    let out = chip.is_eq_with_output(l, x[0].clone(), x[1].clone())?;
    io.expose(l, &[out])
});

wrapper!(AssertEqualW, AssertEqualConfig<Fp>, |meta| {
    let advices = [meta.advice_column(), meta.advice_column()];
    for advice in advices.iter() {
        meta.enable_equality(*advice);
    }
    AssertEqualChip::configure(meta, advices)
}, |c, io, l| {
    let chip = AssertEqualChip::<Fp>::construct(c);
    let x = io.load(l, 0, 2)?;
    chip.assert_equal(l, x[0].clone(), x[1].clone())?;
    io.expose(l, &[])
});

const LT_WINDOW: usize = 3;
const LT_BITS: usize = 64;

wrapper!(LessThanW, LessThanConfig<LT_WINDOW, LT_BITS>, |meta| {
    let w = meta.advice_column();
    meta.enable_equality(w);
    let a = meta.advice_column();
    let b = meta.advice_column();
    let a_offset = meta.advice_column();
    let z1 = meta.advice_column();
    let z2 = meta.advice_column();
    let k_values_table = meta.lookup_table_column();
    let constants = meta.fixed_column();
    meta.enable_constant(constants);
    LessThanChip::<LT_WINDOW, LT_BITS>::configure(meta, a, b, a_offset, z1, z2, k_values_table)
}, |c, io, l| {
    let chip = LessThanChip::<LT_WINDOW, LT_BITS>::construct(c.clone());
    NativeRangeCheckChip::<LT_WINDOW, LT_BITS>::load_k_table(l, c.k_values_table)?;
    let x = io.load(l, 0, 2)?;
    chip.copy_less_than(l.namespace(|| "a < b"), x[0].clone(), x[1].clone(), 0, true)?;
    io.expose(l, &[])
});

const RC_WINDOW: usize = 3;
const RC_BITS: usize = 64;

wrapper!(NativeRangeW, NativeRangeCheckConfig<RC_WINDOW, RC_BITS>, |meta| {
    let w = meta.advice_column();
    meta.enable_equality(w);
    let z = meta.advice_column();
    let table_column = meta.lookup_table_column();
    let constants = meta.fixed_column();
    meta.enable_constant(constants);
    NativeRangeCheckChip::<RC_WINDOW, RC_BITS>::configure(meta, z, table_column)
}, |c, io, l| {
    let chip = NativeRangeCheckChip::<RC_WINDOW, RC_BITS>::construct(c.clone());
    NativeRangeCheckChip::<RC_WINDOW, RC_BITS>::load_k_table(l, c.k_values_table)?;
    let x = io.load(l, 0, 1)?;
    chip.copy_range_check(l.namespace(|| "copy and range check"), x[0].clone())?;
    io.expose(l, &[])
});

wrapper!(SmallRangeW, SmallRangeCheckConfig, |meta| {
    let z = meta.advice_column();
    SmallRangeCheckChip::<Fp>::configure(meta, z, 2)
}, |c, io, l| {
    let chip = SmallRangeCheckChip::<Fp>::construct(c);
    let x = io.load(l, 0, 1)?;
    chip.small_range_check(l.namespace(|| "boolean check"), x[0].clone())?;
    io.expose(l, &[])
});

const PROV: &str = "src/zk/gadget tests (column layout, equality and K of the chip's own test circuit)";

pub fn run(name: &str) {
    match name {
        "Chip:ArithChip" => drive(
            &Spec { name: "Chip:ArithChip", k: 4, call: "ArithChip::{add, sub, mul}(a, b)", rule: "repo-test",
                    provenance: "src/zk/gadget/arithmetic.rs tests (3 advice columns with equality, K = 4)",
                    input_notes: &[] },
            |rng, s| (ArithW, vec![rand_fp(rng, s), rand_fp(rng, s + 5)]),
        ),
        "Chip:ConditionalSelectChip" => drive(
            &Spec { name: "Chip:ConditionalSelectChip", k: 4,
                    call: "ConditionalSelectChip::conditional_select(a, b, cond), cond in {0, 1}", rule: "repo-test",
                    provenance: "src/zk/gadget/cond_select.rs tests (4 advice columns, K = 4)", input_notes: &[] },
            |rng, s| {
                let cond = if s % 2 == 0 { Fp::ZERO } else { Fp::ONE };
                (CondSelectW, vec![rand_fp(rng, s), rand_fp(rng, s + 5), cond])
            },
        ),
        "Chip:IsEqualChip" => drive(
            &Spec { name: "Chip:IsEqualChip", k: 4, call: "IsEqualChip::is_eq_with_output(a, b)",
                    rule: "repo-derived",
                    provenance: "src/zk/vm.rs ZkCircuit::configure (IsEqualChip over advice columns with equality); \
                                 no test of the chip at the pin", input_notes: &[] },
            |rng, s| {
                let a = rand_fp(rng, s);
                let b = if s % 2 == 0 { a } else { rand_fp(rng, s + 5) };
                (IsEqualW, vec![a, b])
            },
        ),
        "Chip:AssertEqualChip" => drive(
            &Spec { name: "Chip:AssertEqualChip", k: 4, call: "AssertEqualChip::assert_equal(a, b) (no results)",
                    rule: "repo-derived",
                    provenance: "src/zk/vm.rs ZkCircuit::configure (advice columns with equality); no test of the \
                                 chip at the pin", input_notes: &[] },
            |rng, s| {
                let a = rand_fp(rng, s);
                (AssertEqualW, vec![a, a])
            },
        ),
        "Chip:LessThanChip" => drive(
            &Spec { name: "Chip:LessThanChip", k: 5,
                    call: "LessThanChip::<3, 64>::copy_less_than(a, b, 0, strict = true) (no results)",
                    rule: "repo-test",
                    provenance: "src/zk/gadget/less_than.rs tests (WINDOW_SIZE 3, NUM_OF_BITS 64, K = 5)",
                    input_notes: &[] },
            |rng, s| {
                let a: u64 = if s == 1 { 0 } else { rng.gen_range(0..u64::MAX / 2) };
                let b: u64 = a + 1 + rng.gen_range(0..u64::MAX / 4);
                (LessThanW, vec![Fp::from(a), Fp::from(b)])
            },
        ),
        "Chip:NativeRangeCheckChip" => drive(
            &Spec { name: "Chip:NativeRangeCheckChip", k: 6,
                    call: "NativeRangeCheckChip::<3, 64>::copy_range_check(value) (no results)", rule: "repo-test",
                    provenance: "src/zk/gadget/native_range_check.rs tests (WINDOW_SIZE 3, NUM_BITS 64, K = 6)",
                    input_notes: &[] },
            |rng, s| {
                let v: u64 = match s { 1 => 0, 2 => u64::MAX, _ => rng.gen() };
                (NativeRangeW, vec![Fp::from(v)])
            },
        ),
        "Chip:SmallRangeCheckChip" => drive(
            &Spec { name: "Chip:SmallRangeCheckChip", k: 3,
                    call: "SmallRangeCheckChip::small_range_check(value), range 2 (no results)", rule: "repo-test",
                    provenance: "src/zk/gadget/small_range_check.rs tests (range 2, K = 3)", input_notes: &[] },
            |_rng, s| (SmallRangeW, vec![Fp::from((s % 2) as u64)]),
        ),
        other => panic!("unknown target {}", other),
    }
    let _ = PROV;
}
