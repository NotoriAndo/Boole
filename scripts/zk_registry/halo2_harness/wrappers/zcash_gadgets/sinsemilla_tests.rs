//! Boole H1 wrappers for `SinsemillaChip` instructions (child module of `sinsemilla::tests`; `cfg(test)`).
//! Configuration: the repository's `sinsemilla::tests::configure::<PallasLookupRangeCheckConfig>` (ECC chip plus
//! two Sinsemilla configs over 10 advice columns, generator table loaded by `SinsemillaChip::load`), hash domain
//! `TestHashDomain` (personalization "MerkleCRH"), message shape of `tests::synthesize`: pieces of 10, 250 and 250
//! bits; K = 11.

use ff::Field;
use group::{Curve, Group};
use halo2_proofs::{
    circuit::{Layouter, SimpleFloorPlanner, Value},
    plonk::{Circuit, ConstraintSystem, Error},
};
use pasta_curves::pallas;
use rand::Rng;
use crate::boole_h1::StdRng;

use crate::boole_h1::{drive, xy, Fp, Io, Spec as BSpec};
use crate::ecc::{chip::EccChip, tests::TestFixedBases, CircuitVersion, EccInstructions};
use crate::sinsemilla::{chip::SinsemillaChip, MessagePiece, SinsemillaInstructions};
use crate::utilities::lookup_range_check::PallasLookupRangeCheckConfig;

use super::{TestCommitDomain, TestHashDomain, Q};

type Lookup = PallasLookupRangeCheckConfig;
type SChip = SinsemillaChip<TestHashDomain, TestCommitDomain, TestFixedBases, Lookup>;
type Cfg = (Io, super::EccSinsemillaConfig<Lookup>);

const PROV: &str = "halo2_gadgets/src/sinsemilla.rs tests::configure / tests::synthesize (TestHashDomain, \
                    message pieces of 10 + 250 + 250 bits, K = 11)";

macro_rules! sinsemilla_wrapper {
    ($name:ident, $allow:expr, $vals:ty, |$v:ident, $ecc:ident, $chip:ident, $io:ident, $l:ident| $body:block) => {
        struct $name($vals);
        impl Circuit<Fp> for $name {
            type Config = Cfg;
            type FloorPlanner = SimpleFloorPlanner;
            fn without_witnesses(&self) -> Self {
                unimplemented!("MockProver does not call without_witnesses")
            }
            fn configure(meta: &mut ConstraintSystem<Fp>) -> Cfg {
                let io = Io::configure(meta);
                (io, super::configure::<Lookup>(meta, $allow))
            }
            #[allow(unused_variables)]
            fn synthesize(&self, config: Cfg, mut layouter: impl Layouter<Fp>) -> Result<(), Error> {
                let $ecc = EccChip::construct(config.1 .0.clone(), CircuitVersion::AnchoredBase);
                SChip::load(config.1 .1.clone(), &mut layouter)?;
                let $chip = SChip::construct(config.1 .1.clone());
                let $io = config.0.clone();
                let $v = &self.0;
                let $l = &mut layouter;
                $body
            }
        }
    };
}

pub(crate) fn bits_to_fp(bits: &[bool]) -> Fp {
    let mut acc = Fp::ZERO;
    let mut pow = Fp::ONE;
    for b in bits {
        if *b {
            acc += pow;
        }
        pow = pow.double();
    }
    acc
}

pub(crate) fn sample_pieces(rng: &mut StdRng, s: usize) -> Vec<Vec<bool>> {
    [10usize, 250, 250]
        .iter()
        .map(|n| (0..*n).map(|_| match s { 1 => false, 2 => true, _ => rng.gen::<bool>() }).collect())
        .collect()
}

fn piece_inst(pieces: &[Vec<bool>]) -> Vec<Fp> {
    pieces.iter().map(|b| bits_to_fp(b)).collect()
}

sinsemilla_wrapper!(HashW, false, Vec<Vec<bool>>, |v, ecc, chip, io, l| {
    let mut pieces = vec![];
    for (i, bits) in v.iter().enumerate() {
        let bits: Vec<Value<bool>> = bits.iter().map(|b| Value::known(*b)).collect();
        pieces.push(MessagePiece::from_bitstring(chip.clone(), l.namespace(|| format!("piece {}", i)), &bits)?);
    }
    let cells: Vec<_> = pieces.iter().map(|p| p.inner().cell_value()).collect();
    io.bind(l, &cells, 0)?;
    let msg: Vec<_> = pieces.iter().map(|p| p.inner()).collect();
    let (point, zs) = chip.hash_to_point(l.namespace(|| "hash"), *Q, msg.into())?;
    let mut outs = vec![point.x(), point.y()];
    for z in zs {
        outs.extend(z.into_iter());
    }
    io.expose(l, &outs)
});

#[test]
fn boole_h1_sinsemilla_hash_to_point() {
    drive(
        &BSpec { name: "SinsemillaChip.hash_to_point", k: 11,
                 call: "SinsemillaChip::hash_to_point(Q = TestHashDomain.Q(), [10-bit, 250-bit, 250-bit pieces]); \
                        outputs: the point and the running sums",
                 rule: "repo-test", provenance: PROV, input_notes: &[] },
        |rng, s| {
            let pieces = sample_pieces(rng, s);
            let inst = piece_inst(&pieces);
            (HashW(pieces), inst)
        },
    );
}

sinsemilla_wrapper!(HashPrivW, true, (Vec<Vec<bool>>, pallas::Affine), |v, ecc, chip, io, l| {
    let mut pieces = vec![];
    for (i, bits) in v.0.iter().enumerate() {
        let bits: Vec<Value<bool>> = bits.iter().map(|b| Value::known(*b)).collect();
        pieces.push(MessagePiece::from_bitstring(chip.clone(), l.namespace(|| format!("piece {}", i)), &bits)?);
    }
    let cells: Vec<_> = pieces.iter().map(|p| p.inner().cell_value()).collect();
    io.bind(l, &cells, 0)?;
    let q = ecc.witness_point_non_id(l, Value::known(v.1))?;
    io.bind(l, &[q.x(), q.y()], cells.len())?;
    let msg: Vec<_> = pieces.iter().map(|p| p.inner()).collect();
    let (point, zs) = chip.hash_to_point_with_private_init(l.namespace(|| "hash"), &q, msg.into())?;
    let mut outs = vec![point.x(), point.y()];
    for z in zs {
        outs.extend(z.into_iter());
    }
    io.expose(l, &outs)
});

#[test]
fn boole_h1_sinsemilla_hash_to_point_with_private_init() {
    drive(
        &BSpec { name: "SinsemillaChip.hash_to_point_with_private_init", k: 11,
                 call: "SinsemillaChip::hash_to_point_with_private_init(&Q witnessed, [10-bit, 250-bit, 250-bit \
                        pieces]) with allow_init_from_private_point (tests::configure(meta, true)); outputs: the point \
                        and the running sums",
                 rule: "repo-test", provenance: PROV, input_notes: &[] },
        |rng, s| {
            let pieces = sample_pieces(rng, s);
            let q = if s == 3 { *Q } else { pallas::Point::random(&mut *rng).to_affine() };
            let mut inst = piece_inst(&pieces);
            let (x, y) = xy(q);
            inst.push(x);
            inst.push(y);
            (HashPrivW((pieces, q)), inst)
        },
    );
}

sinsemilla_wrapper!(ExtractW, false, pallas::Affine, |v, ecc, chip, io, l| {
    let p = ecc.witness_point_non_id(l, Value::known(*v))?;
    io.bind(l, &[p.x(), p.y()], 0)?;
    let x = SChip::extract(&p);
    io.expose(l, &[x])
});

#[test]
fn boole_h1_sinsemilla_extract() {
    drive(
        &BSpec { name: "SinsemillaChip.extract", k: 11, call: "SinsemillaChip::extract(&p) on a witnessed point",
                 rule: "repo-test", provenance: PROV, input_notes: &[] },
        |rng, _s| {
            let p = pallas::Point::random(&mut *rng).to_affine();
            let (x, y) = xy(p);
            (ExtractW(p), vec![x, y])
        },
    );
}
