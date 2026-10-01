//! Boole H1 wrappers for `MerkleChip` instructions (child module of `sinsemilla::merkle::tests`; `cfg(test)`).
//! Configuration: the repository's `sinsemilla::merkle::tests::configure::<PallasLookupRangeCheckConfig>` (two
//! Merkle configs over 10 advice columns; the first one is used), Sinsemilla table loaded as the tests do, hash
//! domain `TestHashDomain`, layer index 0 for `hash_layer` (the first layer of the tests' path), K = 11.

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
use crate::ecc::{chip::NonIdentityEccPoint, tests::TestFixedBases};
use crate::sinsemilla::{
    chip::SinsemillaChip,
    merkle::{chip::MerkleChip, MerkleInstructions},
    tests::{TestCommitDomain, TestHashDomain},
    HashDomains, MessagePiece, SinsemillaInstructions,
};
use crate::utilities::{cond_swap::CondSwapInstructions, lookup_range_check::PallasLookupRangeCheckConfig};

type Lookup = PallasLookupRangeCheckConfig;
type MChip = MerkleChip<TestHashDomain, TestCommitDomain, TestFixedBases, Lookup>;
type MCfg = crate::sinsemilla::merkle::chip::MerkleConfig<TestHashDomain, TestCommitDomain, TestFixedBases, Lookup>;
type Cfg = (Io, (MCfg, MCfg));

const PROV: &str = "halo2_gadgets/src/sinsemilla/merkle.rs tests::configure (first Merkle config; TestHashDomain; \
                    K = 11)";

macro_rules! merkle_wrapper {
    ($name:ident, $allow:expr, $vals:ty, |$v:ident, $chip:ident, $io:ident, $l:ident| $body:block) => {
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
                SinsemillaChip::<TestHashDomain, TestCommitDomain, TestFixedBases, Lookup>::load(
                    config.1 .0.sinsemilla_config.clone(),
                    &mut layouter,
                )?;
                let $chip = MChip::construct(config.1 .0.clone());
                let $io = config.0.clone();
                let $v = &self.0;
                let $l = &mut layouter;
                $body
            }
        }
    };
}

fn rand_fp(rng: &mut StdRng, s: usize) -> Fp {
    match s {
        1 => Fp::ZERO,
        2 => -Fp::ONE,
        _ => Fp::random(&mut *rng),
    }
}

merkle_wrapper!(HashLayerW, false, (), |v, chip, io, l| {
    let lr = io.load(l, 0, 2)?;
    let q = TestHashDomain.Q();
    let out = MerkleInstructions::<pallas::Affine, 32, 10, 253>::hash_layer(
        &chip, l.namespace(|| "hash layer"), q, 0, lr[0].clone(), lr[1].clone())?;
    io.expose(l, &[out])
});

#[test]
fn boole_h1_merkle_hash_layer() {
    drive(
        &BSpec { name: "MerkleChip.hash_layer", k: 11,
                 call: "MerkleChip::hash_layer(Q = TestHashDomain.Q(), l = 0, left, right)",
                 rule: "repo-test", provenance: PROV, input_notes: &[] },
        |rng, s| (HashLayerW(()), vec![rand_fp(rng, s), rand_fp(rng, s)]),
    );
}

fn bits_to_fp(bits: &[bool]) -> Fp {
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

fn sample_pieces(rng: &mut StdRng, s: usize) -> Vec<Vec<bool>> {
    [10usize, 250, 250]
        .iter()
        .map(|n| (0..*n).map(|_| match s { 1 => false, 2 => true, _ => rng.gen::<bool>() }).collect())
        .collect()
}

merkle_wrapper!(HashW, false, Vec<Vec<bool>>, |v, chip, io, l| {
    let mut pieces = vec![];
    for (i, bits) in v.iter().enumerate() {
        let bits: Vec<Value<bool>> = bits.iter().map(|b| Value::known(*b)).collect();
        pieces.push(MessagePiece::from_bitstring(chip.clone(), l.namespace(|| format!("piece {}", i)), &bits)?);
    }
    let cells: Vec<_> = pieces.iter().map(|p| p.inner().cell_value()).collect();
    io.bind(l, &cells, 0)?;
    let msg: Vec<_> = pieces.iter().map(|p| p.inner()).collect();
    let (point, zs) = chip.hash_to_point(l.namespace(|| "hash"), TestHashDomain.Q(), msg.into())?;
    let mut outs = vec![point.x(), point.y()];
    for z in zs {
        outs.extend(z.into_iter());
    }
    io.expose(l, &outs)
});

#[test]
fn boole_h1_merkle_hash_to_point() {
    drive(
        &BSpec { name: "MerkleChip.hash_to_point", k: 11,
                 call: "MerkleChip::hash_to_point(Q = TestHashDomain.Q(), [10-bit, 250-bit, 250-bit pieces]) \
                        (delegates to SinsemillaChip); outputs: the point and the running sums",
                 rule: "repo-test", provenance: PROV, input_notes: &[] },
        |rng, s| {
            let pieces = sample_pieces(rng, s);
            let inst = pieces.iter().map(|b| bits_to_fp(b)).collect();
            (HashW(pieces), inst)
        },
    );
}

merkle_wrapper!(HashPrivW, true, (Vec<Vec<bool>>, pallas::Affine), |v, chip, io, l| {
    let mut pieces = vec![];
    for (i, bits) in v.0.iter().enumerate() {
        let bits: Vec<Value<bool>> = bits.iter().map(|b| Value::known(*b)).collect();
        pieces.push(MessagePiece::from_bitstring(chip.clone(), l.namespace(|| format!("piece {}", i)), &bits)?);
    }
    let cells: Vec<_> = pieces.iter().map(|p| p.inner().cell_value()).collect();
    io.bind(l, &cells, 0)?;
    let qc = io.load(l, cells.len(), 2)?;
    let q = NonIdentityEccPoint::from_coordinates_unchecked(qc[0].clone().into(), qc[1].clone().into());
    let msg: Vec<_> = pieces.iter().map(|p| p.inner()).collect();
    let (point, zs) = chip.hash_to_point_with_private_init(l.namespace(|| "hash"), &q, msg.into())?;
    let mut outs = vec![point.x(), point.y()];
    for z in zs {
        outs.extend(z.into_iter());
    }
    io.expose(l, &outs)
});

#[test]
fn boole_h1_merkle_hash_to_point_with_private_init() {
    drive(
        &BSpec { name: "MerkleChip.hash_to_point_with_private_init", k: 11,
                 call: "MerkleChip::hash_to_point_with_private_init(&Q from input cells, [10-bit, 250-bit, 250-bit pieces]) \
                        with allow_init_from_private_point (tests::configure(meta, true)); outputs: the point and the \
                        running sums",
                 rule: "repo-test", provenance: PROV, input_notes: &[] },
        |rng, s| {
            let pieces = sample_pieces(rng, s);
            let q = pallas::Point::random(&mut *rng).to_affine();
            let mut inst: Vec<Fp> = pieces.iter().map(|b| bits_to_fp(b)).collect();
            let (x, y) = xy(q);
            inst.push(x);
            inst.push(y);
            (HashPrivW((pieces, q)), inst)
        },
    );
}

merkle_wrapper!(ExtractW, false, (), |v, chip, io, l| {
    let pc = io.load(l, 0, 2)?;
    let p = NonIdentityEccPoint::from_coordinates_unchecked(pc[0].clone().into(), pc[1].clone().into());
    let x = MChip::extract(&p);
    io.expose(l, &[x])
});

#[test]
fn boole_h1_merkle_extract() {
    drive(
        &BSpec { name: "MerkleChip.extract", k: 11,
                 call: "MerkleChip::extract(&p) on a point from input cells (delegates to SinsemillaChip)",
                 rule: "repo-test", provenance: PROV, input_notes: &[] },
        |rng, _s| {
            let p = pallas::Point::random(&mut *rng).to_affine();
            let (x, y) = xy(p);
            (ExtractW(()), vec![x, y])
        },
    );
}

merkle_wrapper!(MuxW, false, (), |v, chip, io, l| {
    let c = io.load(l, 0, 3)?;
    let out = chip.mux(l, c[0].clone(), c[1].clone(), c[2].clone())?;
    io.expose(l, &[out])
});

#[test]
fn boole_h1_merkle_mux() {
    drive(
        &BSpec { name: "MerkleChip.mux", k: 11,
                 call: "MerkleChip::mux(choice, left, right), choice in {0, 1} (delegates to CondSwapChip)",
                 rule: "repo-test", provenance: PROV, input_notes: &[] },
        |rng, s| {
            let choice = if s % 2 == 0 { Fp::ZERO } else { Fp::ONE };
            (MuxW(()), vec![choice, rand_fp(rng, s), rand_fp(rng, s)])
        },
    );
}

merkle_wrapper!(SwapW, false, (Fp, bool), |v, chip, io, l| {
    let a = io.load(l, 0, 1)?.remove(0);
    let (x, y) = chip.swap(l.namespace(|| "swap"), (a, Value::known(v.0)), Value::known(v.1))?;
    io.expose(l, &[x, y])
});

#[test]
fn boole_h1_merkle_swap() {
    drive(
        &BSpec { name: "MerkleChip.swap", k: 11,
                 call: "MerkleChip::swap((a, b), swap) (delegates to CondSwapChip); inputs: a and the cells the \
                        instruction witnesses for b and swap",
                 rule: "repo-test", provenance: PROV, input_notes: &[("swap", "witness b"), ("swap", "swap")] },
        |rng, s| {
            let a = rand_fp(rng, s);
            let b = rand_fp(rng, s + 7);
            (SwapW((b, s % 2 == 1)), vec![a])
        },
    );
}
