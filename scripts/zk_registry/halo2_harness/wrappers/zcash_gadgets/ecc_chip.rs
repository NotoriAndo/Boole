//! Boole H1 wrappers for `EccChip` instructions (child module of `ecc::chip`; `cfg(test)`).
//! Configuration: the repository's `ecc::tests::MyEccCircuit::configure` (10 advice columns, a 10-bit lookup
//! table, 8 Lagrange-coefficient columns, one constants column, `PallasLookupRangeCheckConfig`), fixed bases
//! `ecc::tests::TestFixedBases`, `CircuitVersion::AnchoredBase` (the tests' default), K = 11.

use ff::Field;
use group::{prime::PrimeCurveAffine, Curve, Group};
use halo2_proofs::{
    circuit::{Layouter, SimpleFloorPlanner, Value},
    plonk::{Circuit, ConstraintSystem, Error},
};
use pasta_curves::pallas;
use rand::RngCore;
use crate::boole_h1::StdRng;

use super::{EccChip, EccConfig, EccScalarFixedShort, ScalarVar};
use crate::boole_h1::{drive, xy, Fp, Io, Spec};
use crate::ecc::tests::{BaseField, FullWidth, Short, TestFixedBases};
use crate::ecc::{BaseFitsInScalarInstructions, CircuitVersion, EccInstructions};
use crate::utilities::lookup_range_check::{LookupRangeCheck, PallasLookupRangeCheckConfig};

type Lookup = PallasLookupRangeCheckConfig;
type Chip = EccChip<TestFixedBases, Lookup>;
type Cfg = (Io, EccConfig<TestFixedBases, Lookup>);

const PROV: &str = "halo2_gadgets/src/ecc.rs tests::MyEccCircuit::configure (K = 11 lookup table; fixed bases \
                    ecc::tests::TestFixedBases; CircuitVersion::AnchoredBase)";

fn configure_ecc(meta: &mut ConstraintSystem<Fp>) -> Cfg {
    let io = Io::configure(meta);
    let advices = [
        meta.advice_column(),
        meta.advice_column(),
        meta.advice_column(),
        meta.advice_column(),
        meta.advice_column(),
        meta.advice_column(),
        meta.advice_column(),
        meta.advice_column(),
        meta.advice_column(),
        meta.advice_column(),
    ];
    let lookup_table = meta.lookup_table_column();
    let lagrange_coeffs = [
        meta.fixed_column(),
        meta.fixed_column(),
        meta.fixed_column(),
        meta.fixed_column(),
        meta.fixed_column(),
        meta.fixed_column(),
        meta.fixed_column(),
        meta.fixed_column(),
    ];
    let constants = meta.fixed_column();
    meta.enable_constant(constants);
    let range_check = Lookup::configure(meta, advices[9], lookup_table);
    let ecc = Chip::configure(meta, advices, lagrange_coeffs, range_check);
    (io, ecc)
}

macro_rules! ecc_wrapper {
    ($name:ident, $vals:ty, |$v:ident, $chip:ident, $io:ident, $l:ident| $body:block) => {
        struct $name($vals);
        impl Circuit<Fp> for $name {
            type Config = Cfg;
            type FloorPlanner = SimpleFloorPlanner;
            fn without_witnesses(&self) -> Self {
                unimplemented!("MockProver does not call without_witnesses")
            }
            fn configure(meta: &mut ConstraintSystem<Fp>) -> Cfg {
                configure_ecc(meta)
            }
            #[allow(unused_variables)]
            fn synthesize(&self, config: Cfg, mut layouter: impl Layouter<Fp>) -> Result<(), Error> {
                let $chip = Chip::construct(config.1.clone(), CircuitVersion::AnchoredBase);
                config.1.lookup_config.load_range_check_table(&mut layouter)?;
                let $io = config.0.clone();
                let $v = &self.0;
                let $l = &mut layouter;
                $body
            }
        }
    };
}

fn rand_point(rng: &mut StdRng) -> pallas::Affine {
    pallas::Point::random(&mut *rng).to_affine()
}

fn rand_base(rng: &mut StdRng) -> Fp {
    Fp::random(&mut *rng)
}

// ------------------------------------------------------------------------------------------------ add

ecc_wrapper!(AddW, (pallas::Affine, pallas::Affine), |v, chip, io, l| {
    let p = chip.witness_point(l, Value::known(v.0))?;
    let q = chip.witness_point(l, Value::known(v.1))?;
    io.bind(l, &[p.x(), p.y(), q.x(), q.y()], 0)?;
    let r = chip.add(l, &p, &q)?;
    io.expose(l, &[r.x(), r.y()])
});

fn point_pair(rng: &mut StdRng, s: usize) -> (pallas::Affine, pallas::Affine) {
    let p = rand_point(rng);
    let q = rand_point(rng);
    match s {
        1 => (p, p),
        2 => (p, -p),
        3 => (pallas::Affine::identity(), q),
        4 => (p, pallas::Affine::identity()),
        5 => (pallas::Affine::identity(), pallas::Affine::identity()),
        _ => (p, q),
    }
}

fn point_inst(ps: &[pallas::Affine]) -> Vec<Fp> {
    ps.iter().flat_map(|p| {
        let (x, y) = xy(*p);
        vec![x, y]
    }).collect()
}

#[test]
fn boole_h1_ecc_add() {
    drive(
        &Spec { name: "EccChip.add", k: 11, call: "EccChip::add(&p, &q) on witnessed points p, q (identity allowed)",
                rule: "repo-test", provenance: PROV, input_notes: &[] },
        |rng, s| {
            let (p, q) = point_pair(rng, s);
            (AddW((p, q)), point_inst(&[p, q]))
        },
    );
}

// ------------------------------------------------------------------------------------------------ add_incomplete

ecc_wrapper!(AddIncW, (pallas::Affine, pallas::Affine), |v, chip, io, l| {
    let p = chip.witness_point_non_id(l, Value::known(v.0))?;
    let q = chip.witness_point_non_id(l, Value::known(v.1))?;
    io.bind(l, &[p.x(), p.y(), q.x(), q.y()], 0)?;
    let r = chip.add_incomplete(l, &p, &q)?;
    io.expose(l, &[r.x(), r.y()])
});

#[test]
fn boole_h1_ecc_add_incomplete() {
    drive(
        &Spec { name: "EccChip.add_incomplete", k: 11,
                call: "EccChip::add_incomplete(&p, &q) on witnessed non-identity points with distinct x",
                rule: "repo-test", provenance: PROV, input_notes: &[] },
        |rng, _s| {
            let p = rand_point(rng);
            let q = rand_point(rng);
            (AddIncW((p, q)), point_inst(&[p, q]))
        },
    );
}

// ------------------------------------------------------------------------------------------------ constrain_equal

ecc_wrapper!(EqW, pallas::Affine, |v, chip, io, l| {
    let p = chip.witness_point(l, Value::known(*v))?;
    let q = chip.witness_point(l, Value::known(*v))?;
    io.bind(l, &[p.x(), p.y(), q.x(), q.y()], 0)?;
    chip.constrain_equal(l, &p, &q)?;
    io.expose(l, &[])
});

#[test]
fn boole_h1_ecc_constrain_equal() {
    drive(
        &Spec { name: "EccChip.constrain_equal", k: 11,
                call: "EccChip::constrain_equal(&p, &q) on witnessed points (no results)",
                rule: "repo-test", provenance: PROV, input_notes: &[] },
        |rng, s| {
            let p = if s == 1 { pallas::Affine::identity() } else { rand_point(rng) };
            (EqW(p), point_inst(&[p, p]))
        },
    );
}

// ------------------------------------------------------------------------------------------------ extract_p

ecc_wrapper!(ExtractW, pallas::Affine, |v, chip, io, l| {
    let p = chip.witness_point(l, Value::known(*v))?;
    io.bind(l, &[p.x(), p.y()], 0)?;
    let x = Chip::extract_p(&p);
    io.expose(l, &[x])
});

#[test]
fn boole_h1_ecc_extract_p() {
    drive(
        &Spec { name: "EccChip.extract_p", k: 11, call: "EccChip::extract_p(&p) on a witnessed point",
                rule: "repo-test", provenance: PROV, input_notes: &[] },
        |rng, s| {
            let p = if s == 1 { pallas::Affine::identity() } else { rand_point(rng) };
            (ExtractW(p), point_inst(&[p]))
        },
    );
}

// ------------------------------------------------------------------------------------------------ witness_point_from_constant

ecc_wrapper!(ConstW, pallas::Affine, |v, chip, io, l| {
    let p = chip.witness_point_from_constant(l, *v)?;
    io.expose(l, &[p.x(), p.y()])
});

#[test]
fn boole_h1_ecc_witness_point_from_constant() {
    drive(
        &Spec { name: "EccChip.witness_point_from_constant", k: 11,
                call: "EccChip::witness_point_from_constant(G) with G the Pallas generator (the tests' base point)",
                rule: "repo-test", provenance: PROV, input_notes: &[] },
        |_rng, _s| (ConstW(pallas::Point::generator().to_affine()), vec![]),
    );
}

// ------------------------------------------------------------------------------------------------ mul_sign

ecc_wrapper!(MulSignW, (Fp, pallas::Affine), |v, chip, io, l| {
    let sign = io.load(l, 0, 1)?.remove(0);
    let p = chip.witness_point(l, Value::known(v.1))?;
    io.bind(l, &[p.x(), p.y()], 1)?;
    let r = chip.mul_sign(l, &sign, &p)?;
    io.expose(l, &[r.x(), r.y()])
});

#[test]
fn boole_h1_ecc_mul_sign() {
    drive(
        &Spec { name: "EccChip.mul_sign", k: 11, call: "EccChip::mul_sign(&sign, &p), sign in {1, -1}",
                rule: "repo-test", provenance: PROV, input_notes: &[] },
        |rng, s| {
            let sign = if s % 2 == 0 { Fp::ONE } else { -Fp::ONE };
            let p = if s == 2 { pallas::Affine::identity() } else { rand_point(rng) };
            let mut inst = vec![sign];
            inst.extend(point_inst(&[p]));
            (MulSignW((sign, p)), inst)
        },
    );
}

// ------------------------------------------------------------------------------------------------ mul

ecc_wrapper!(MulW, (Fp, pallas::Affine), |v, chip, io, l| {
    let alpha = io.load(l, 0, 1)?.remove(0);
    let p = chip.witness_point_non_id(l, Value::known(v.1))?;
    io.bind(l, &[p.x(), p.y()], 1)?;
    let scalar = chip.scalar_var_from_base(l, &alpha)?;
    let (r, s) = chip.mul(l, &scalar, &p)?;
    let s_cell = match s {
        ScalarVar::BaseFieldElem(c) => c,
        ScalarVar::FullWidth => unreachable!(),
    };
    io.expose(l, &[r.x(), r.y(), s_cell])
});

#[test]
fn boole_h1_ecc_mul() {
    drive(
        &Spec { name: "EccChip.mul", k: 11,
                call: "EccChip::mul(ScalarVar::BaseFieldElem(alpha), &p) on a witnessed non-identity point p",
                rule: "repo-test", provenance: PROV, input_notes: &[] },
        |rng, s| {
            let alpha = match s {
                1 => Fp::ZERO,
                2 => Fp::ONE,
                3 => -Fp::ONE,
                _ => rand_base(rng),
            };
            let p = rand_point(rng);
            let mut inst = vec![alpha];
            inst.extend(point_inst(&[p]));
            (MulW((alpha, p)), inst)
        },
    );
}

// ------------------------------------------------------------------------------------------------ mul_fixed

ecc_wrapper!(MulFixedW, pallas::Scalar, |v, chip, io, l| {
    let scalar = chip.witness_scalar_fixed(l, Value::known(*v))?;
    let (r, scalar) = chip.mul_fixed(l, &scalar, &FullWidth::from_pallas_generator())?;
    let windows: Vec<_> = scalar.windows.clone().expect("windows witnessed by mul_fixed").into_iter().collect();
    io.bind(l, &windows, 0)?;
    io.expose(l, &[r.x(), r.y()])
});

#[test]
fn boole_h1_ecc_mul_fixed() {
    drive(
        &Spec { name: "EccChip.mul_fixed", k: 11,
                call: "EccChip::mul_fixed(&scalar, FullWidth::from_pallas_generator()); inputs: the 85 3-bit windows \
                       the instruction witnesses",
                rule: "repo-test", provenance: PROV, input_notes: &[] },
        |rng, s| {
            let scalar = match s {
                1 => pallas::Scalar::ZERO,
                2 => -pallas::Scalar::ONE,
                _ => pallas::Scalar::random(&mut *rng),
            };
            (MulFixedW(scalar), scalar_windows(scalar))
        },
    );
}

/// The 85 little-endian 3-bit windows of a full-width scalar (as the chip witnesses them).
fn scalar_windows(s: pallas::Scalar) -> Vec<Fp> {
    use ff::PrimeFieldBits;
    let bits: Vec<bool> = s.to_le_bits().iter().by_vals().take(255).collect();
    (0..85)
        .map(|w| {
            let mut v = 0u64;
            for b in 0..3 {
                if bits.get(3 * w + b).copied().unwrap_or(false) {
                    v |= 1 << b;
                }
            }
            Fp::from(v)
        })
        .collect()
}

// ------------------------------------------------------------------------------------------------ mul_fixed_short

ecc_wrapper!(MulFixedShortW, (Fp, Fp), |v, chip, io, l| {
    let ms = io.load(l, 0, 2)?;
    let scalar = EccScalarFixedShort { magnitude: ms[0].clone(), sign: ms[1].clone(), running_sum: None };
    let (r, _) = chip.mul_fixed_short(l, &scalar, &Short)?;
    io.expose(l, &[r.x(), r.y()])
});

fn magnitude_sign(rng: &mut StdRng, s: usize) -> (Fp, Fp) {
    let m = match s {
        1 => 0u64,
        2 => u64::MAX,
        3 => 1,
        _ => rng.next_u64(),
    };
    let sign = if s % 2 == 0 { Fp::ONE } else { -Fp::ONE };
    (Fp::from(m), sign)
}

#[test]
fn boole_h1_ecc_mul_fixed_short() {
    drive(
        &Spec { name: "EccChip.mul_fixed_short", k: 11,
                call: "EccChip::mul_fixed_short((magnitude, sign), Short) with magnitude < 2^64, sign in {1, -1}",
                rule: "repo-test", provenance: PROV, input_notes: &[] },
        |rng, s| {
            let (m, sign) = magnitude_sign(rng, s);
            (MulFixedShortW((m, sign)), vec![m, sign])
        },
    );
}

// ------------------------------------------------------------------------------------------------ mul_fixed_base_field_elem

ecc_wrapper!(MulFixedBaseW, Fp, |v, chip, io, l| {
    let e = io.load(l, 0, 1)?.remove(0);
    let r = chip.mul_fixed_base_field_elem(l, e, &BaseField)?;
    io.expose(l, &[r.x(), r.y()])
});

#[test]
fn boole_h1_ecc_mul_fixed_base_field_elem() {
    drive(
        &Spec { name: "EccChip.mul_fixed_base_field_elem", k: 11,
                call: "EccChip::mul_fixed_base_field_elem(e, BaseField)",
                rule: "repo-test", provenance: PROV, input_notes: &[] },
        |rng, s| {
            let e = match s {
                1 => Fp::ZERO,
                2 => -Fp::ONE,
                _ => rand_base(rng),
            };
            (MulFixedBaseW(e), vec![e])
        },
    );
}

// ------------------------------------------------------------------------------------------------ scalar_fixed_from_signed_short

ecc_wrapper!(SignedShortW, (Fp, Fp), |v, chip, io, l| {
    let ms = io.load(l, 0, 2)?;
    let s = chip.scalar_fixed_from_signed_short(l, (ms[0].clone(), ms[1].clone()))?;
    let mut cells = vec![s.magnitude.clone(), s.sign.clone()];
    if let Some(rs) = s.running_sum.clone() {
        cells.extend(rs.into_iter());
    }
    io.expose(l, &cells)
});

#[test]
fn boole_h1_ecc_scalar_fixed_from_signed_short() {
    drive(
        &Spec { name: "EccChip.scalar_fixed_from_signed_short", k: 11,
                call: "EccChip::scalar_fixed_from_signed_short((magnitude, sign)); outputs: magnitude, sign and the \
                       running sum when present",
                rule: "repo-test", provenance: PROV, input_notes: &[] },
        |rng, s| {
            let (m, sign) = magnitude_sign(rng, s);
            (SignedShortW((m, sign)), vec![m, sign])
        },
    );
}

// ------------------------------------------------------------------------------------------------ scalar_var_from_base

ecc_wrapper!(VarFromBaseW, Fp, |v, chip, io, l| {
    let e = io.load(l, 0, 1)?.remove(0);
    let s = chip.scalar_var_from_base(l, &e)?;
    let c = match s {
        ScalarVar::BaseFieldElem(c) => c,
        ScalarVar::FullWidth => unreachable!(),
    };
    io.expose(l, &[c])
});

#[test]
fn boole_h1_ecc_scalar_var_from_base() {
    drive(
        &Spec { name: "EccChip.scalar_var_from_base", k: 11, call: "EccChip::scalar_var_from_base(&e)",
                rule: "repo-test", provenance: PROV, input_notes: &[] },
        |rng, _s| {
            let e = rand_base(rng);
            (VarFromBaseW(e), vec![e])
        },
    );
}
