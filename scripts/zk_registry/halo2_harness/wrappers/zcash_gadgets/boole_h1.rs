//! Boole H1 wrapper driver (shared by the adapters; injected into scratch copies or vendored crates only).
//!
//! A wrapper circuit allocates the I/O columns first (instance 0; advice 0 for outputs, advice 1 for loaded
//! inputs), then the repository's configuration, assigns inputs (copy-constrained to instance cells), calls the
//! instruction(s) and copies every result cell into the `boole-outputs` region.  `drive` runs a wrapper on sampled
//! inputs and exports each `MockProver` run; it does nothing unless `BOOLE_H1_OUT` is set.

#[allow(unused_imports)]
use std::{boxed::Box, string::{String, ToString}, vec::Vec};
use std::fs;
use std::io::{BufRead, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};

use ff::{Field, PrimeField};
use halo2_proofs::dev::boole_export::ExportField;
use halo2_proofs::{
    circuit::{AssignedCell, Layouter},
    dev::{boole_export, MockProver},
    plonk::{Advice, Circuit, Column, ConstraintSystem, Error, Instance},
};

// BEGIN pasta (adapters over the Pasta curves; replaced for other fields)
use pasta_curves::pallas;

/// The wrappers' field (Pallas base field).
pub type Fp = pallas::Base;

/// Affine coordinates (the identity as (0, 0), as the ECC chip encodes it).
pub fn xy(p: pallas::Affine) -> (Fp, Fp) {
    use pasta_curves::arithmetic::CurveAffine;
    let c = p.coordinates();
    if bool::from(c.is_some()) {
        let c = c.unwrap();
        (*c.x(), *c.y())
    } else {
        (Fp::ZERO, Fp::ZERO)
    }
}
// END pasta

/// The wrappers' sampler: SplitMix64 (no dependency beyond `rand_core`, which every adapter's crate already
/// has), seeded per (seed, sample) so that a run is reproducible.
#[derive(Clone, Debug)]
pub struct StdRng(u64);

impl StdRng {
    pub fn seed_from_u64(seed: u64) -> Self {
        StdRng(seed)
    }
}

/// Marker only: some repository helpers ask for a `CryptoRng` when generating test data.
impl rand::CryptoRng for StdRng {}

impl rand::RngCore for StdRng {
    fn next_u32(&mut self) -> u32 {
        self.next_u64() as u32
    }
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        for chunk in dest.chunks_mut(8) {
            let v = self.next_u64().to_le_bytes();
            chunk.copy_from_slice(&v[..chunk.len()]);
        }
    }
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

/// The wrapper's I/O columns.
#[derive(Clone, Debug)]
pub struct Io {
    pub instance: Column<Instance>,
    pub out: Column<Advice>,
    pub inp: Column<Advice>,
}

impl Io {
    pub fn configure<F: Field>(meta: &mut ConstraintSystem<F>) -> Self {
        let instance = meta.instance_column();
        meta.enable_equality(instance);
        let out = meta.advice_column();
        meta.enable_equality(out);
        let inp = meta.advice_column();
        meta.enable_equality(inp);
        Io { instance, out, inp }
    }

    /// Instance rows `start..start + n` loaded into the input column (copy constraints to the instance cells).
    pub fn load<F: Field>(
        &self,
        layouter: &mut impl Layouter<F>,
        start: usize,
        n: usize,
    ) -> Result<Vec<AssignedCell<F, F>>, Error> {
        layouter.assign_region(
            || "boole-inputs",
            |mut region| {
                (0..n)
                    .map(|i| {
                        region.assign_advice_from_instance(
                            || format!("in{}", start + i),
                            self.instance,
                            start + i,
                            self.inp,
                            i,
                        )
                    })
                    .collect()
            },
        )
    }

    /// Copy constraints from cells the instruction (or a witnessing helper) assigned to instance rows.
    pub fn bind<F: Field>(
        &self,
        layouter: &mut impl Layouter<F>,
        cells: &[AssignedCell<F, F>],
        start: usize,
    ) -> Result<(), Error> {
        for (i, c) in cells.iter().enumerate() {
            layouter.constrain_instance(c.cell(), self.instance, start + i)?;
        }
        Ok(())
    }

    /// The instruction's result cells, copied into the output column (region `boole-outputs`).
    pub fn expose<F: Field>(
        &self,
        layouter: &mut impl Layouter<F>,
        cells: &[AssignedCell<F, F>],
    ) -> Result<(), Error> {
        if cells.is_empty() {
            return Ok(());
        }
        layouter.assign_region(
            || "boole-outputs",
            |mut region| {
                for (i, c) in cells.iter().enumerate() {
                    c.copy_advice(|| format!("out{}", i), &mut region, self.out, i)?;
                }
                Ok(())
            },
        )
    }
}

/// Target description written into every exported document.
pub struct Spec {
    pub name: &'static str,
    pub k: u32,
    pub call: &'static str,
    pub rule: &'static str,
    pub provenance: &'static str,
    /// (region name, annotation) of cells the instruction witnesses from `Value` parameters; they are inputs.
    pub input_notes: &'static [(&'static str, &'static str)],
}

fn json_str(s: &str) -> String {
    let mut out = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push(' '),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn parse_fp<F: PrimeField>(hex: &str) -> Option<F> {
    let mut be = [0u8; 32];
    if hex.len() != 64 {
        return None;
    }
    for i in 0..32 {
        be[i] = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok()?;
    }
    let mut repr = <F as PrimeField>::Repr::default();
    for (i, b) in be.iter().rev().enumerate() {
        repr.as_mut()[i] = *b;
    }
    Option::from(F::from_repr(repr))
}

/// Runs the wrapper on `BOOLE_H1_SAMPLES` sampled inputs (`gen(rng, sample)` gives the circuit and the instance
/// column) and writes one document per sample to `$BOOLE_H1_OUT/<name>/sample_<k>.json`.  With
/// `BOOLE_H1_MUTANTS=1` it reruns sample 0 and evaluates `mutants.txt` (`column row hex` per line) with
/// `MockProver::verify` on the overridden advice cell, writing `mutants.out`.
pub fn drive<F, C, G>(spec: &Spec, gen: G)
where
    F: ExportField,
    C: Circuit<F>,
    G: Fn(&mut StdRng, usize) -> (C, Vec<F>),
{
    drive_cols(spec, |rng, s| {
        let (c, inst) = gen(rng, s);
        (c, vec![inst])
    })
}

/// `drive` for circuits with any number of instance columns (the repository's own circuits).
pub fn drive_cols<F, C, G>(spec: &Spec, gen: G)
where
    F: ExportField,
    C: Circuit<F>,
    G: Fn(&mut StdRng, usize) -> (C, Vec<Vec<F>>),
{
    let out = match std::env::var("BOOLE_H1_OUT") {
        Ok(v) => v,
        Err(_) => return,
    };
    if let Ok(only) = std::env::var("BOOLE_H1_ONLY") {
        if !only.split(',').any(|x| x == spec.name) {
            return;
        }
    }
    let samples: usize = std::env::var("BOOLE_H1_SAMPLES").ok().and_then(|v| v.parse().ok()).unwrap_or(16);
    let seed: u64 = std::env::var("BOOLE_H1_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    let mutate = std::env::var("BOOLE_H1_MUTANTS").map(|v| v == "1").unwrap_or(false);
    let dir = format!("{}/{}", out, spec.name);
    fs::create_dir_all(&dir).unwrap();
    let notes: Vec<String> =
        spec.input_notes.iter().map(|(r, a)| format!("[{},{}]", json_str(r), json_str(a))).collect();
    let count = if mutate { 1 } else { samples };
    for s in 0..count {
        let mut rng = StdRng::seed_from_u64(seed.wrapping_mul(1_000_003).wrapping_add(s as u64));
        let (circuit, inst) = gen(&mut rng, s);
        boole_export::reset_notes();
        let mut k = spec.k;
        let run = loop {
            let r = catch_unwind(AssertUnwindSafe(|| MockProver::run(k, &circuit, inst.clone())));
            match r {
                Ok(Err(Error::NotEnoughRowsAvailable { .. })) | Ok(Err(Error::InstanceTooLarge)) if k < spec.k + 4 => {
                    k += 1;
                    boole_export::reset_notes();
                }
                other => break other,
            }
        };
        let mut prover = match run {
            Ok(Ok(p)) => p,
            Ok(Err(e)) => {
                fs::write(format!("{}/sample_{:03}.err", dir, s), format!("MockProver::run error: {:?}", e)).unwrap();
                continue;
            }
            Err(_) => {
                fs::write(format!("{}/sample_{:03}.err", dir, s), "panic during synthesis").unwrap();
                continue;
            }
        };
        if mutate {
            let path = format!("{}/mutants.txt", dir);
            let f = fs::File::open(&path).unwrap();
            let mut outf = fs::File::create(format!("{}/mutants.out", dir)).unwrap();
            for line in std::io::BufReader::new(f).lines() {
                let line = line.unwrap();
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() != 3 {
                    continue;
                }
                let col: usize = parts[0].parse().unwrap();
                let row: usize = parts[1].parse().unwrap();
                let v: F = parse_fp(parts[2]).expect("mutant value");
                // a panic inside verify (e.g. an assertion on the mutated values) is recorded, not hidden
                let verdict = match prover.boole_verify_override(col, row, v) {
                    Some(true) => "ACCEPT",
                    Some(false) => "REJECT",
                    None => "PANIC",
                };
                writeln!(outf, "{} {} {}", col, row, verdict).unwrap();
            }
            continue;
        }
        let meta = format!(
            "{{\"target\":{},\"k_requested\":{},\"call\":{},\"rule\":{},\"provenance\":{},\"input_notes\":[{}],\"sample\":{},\"seed\":{}}}",
            json_str(spec.name),
            spec.k,
            json_str(spec.call),
            json_str(spec.rule),
            json_str(spec.provenance),
            notes.join(","),
            s,
            seed
        );
        let doc = prover.boole_export(&meta);
        fs::write(format!("{}/sample_{:03}.json", dir, s), doc).unwrap();
    }
}
