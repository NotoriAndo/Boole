//! Boole AIR extractor for Pico (a `#[cfg(test)]` module added to a scratch copy of pico-vm at the census pin;
//! not upstream code).  Run: `BOOLE_OUT=<dir> cargo test -p pico-vm --release --lib boole_air_extract::extract
//! -- --nocapture --test-threads 1`.  Writes `manifest.jsonl`, `airs/<index>.json` (boole-air-ir/v1) and
//! `rows/<index>.json` (boole-air-rows/v1).
//!
//! * AIRs: `RiscvChipType::<KoalaBear>::all_chip_variants()` (the enabled machine, in machine order), then the
//!   nine `RecursionChipType::<KoalaBear>` AIRs of `all_chips()`.
//! * Constraints and lookups: pico's own `SymbolicConstraintFolder` (the folder the prover uses to count and
//!   collect them); the lookups are cross-checked against `MetaChip::get_looking` / `get_looked`.
//! * Rows: the real emulator (`RiscvEmulator`), the prover's record complement (`extra_record` per chip in machine
//!   order) and `generate_main` / `generate_preprocessed`, on straight-line programs (ALU, memory, control flow,
//!   precompiles without an in-tree ELF) and the in-tree RV64 ELFs of `perf/bench_data/rv64`; the recursion AIRs
//!   on a linear recursion program run by the recursion `Runtime`.
#![allow(dead_code, clippy::all)]

use crate::{
    boole_air_ir::*,
    chips::gadgets::curves::{
        weierstrass::{bls381::Bls12381, bn254::Bn254, secp256k1::Secp256k1, secp256r1::Secp256r1},
        AffinePoint, EllipticCurve,
    },
    compiler::{
        recursion::{
            instruction as rinstr,
            program::RecursionProgram,
            types::MemAccessKind,
        },
        riscv::{
            compiler::{Compiler, SourceType},
            instruction::Instruction,
            opcode::Opcode,
            program::Program,
        },
    },
    configs::stark_config::KoalaBearPoseidon2,
    emulator::{
        record::RecordBehavior,
        recursion::emulator::{BaseAluOpcode, ExtAluOpcode, Runtime},
        riscv::record::EmulationRecord,
        stdin::EmulatorStdin,
    },
    instances::chiptype::{recursion_chiptype::RecursionChipType, riscv_chiptype::RiscvChipType},
    machine::{
        chip::{ChipBehavior, MetaChip},
        folder::SymbolicConstraintFolder,
        lookup::VirtualPairLookup,
    },
    primitives::consts::{KOALABEAR_S_BOX_DEGREE, MAX_NUM_PVS},
};
use num::{BigUint, One};
use p3_air::{Air, BaseAir, PairCol};
use p3_field::{extension::BinomialExtensionField, FieldAlgebra, FieldExtensionAlgebra, PrimeField32};
use p3_koala_bear::KoalaBear;
use p3_matrix::{dense::RowMajorMatrix, Matrix};
use p3_uni_stark::{Entry, SymbolicExpression as SE};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    panic::{catch_unwind, AssertUnwindSafe},
    sync::Arc,
};

type F = KoalaBear;
const P: u64 = 2130706433;

// ------------------------------------------------------------------------------------------ conversion

fn conv(e: &SE<F>, dag: &mut Dag, memo: &mut HashMap<usize, usize>) -> usize {
    let key = e as *const SE<F> as usize;
    if let Some(&i) = memo.get(&key) {
        return i;
    }
    let n = match e {
        SE::Variable(v) => match v.entry {
            Entry::Main { offset } => Node::Main(offset as u8, v.index),
            Entry::Preprocessed { offset } => Node::Prep(offset as u8, v.index),
            Entry::Public => Node::Pub(v.index),
            ref other => panic!("unsupported entry {other:?}"),
        },
        SE::IsFirstRow => Node::First,
        SE::IsLastRow => Node::Last,
        SE::IsTransition => Node::Trans,
        SE::Constant(c) => Node::Const(c.as_canonical_u32() as u64),
        SE::Add { x, y, .. } => {
            let a = conv(x, dag, memo);
            let b = conv(y, dag, memo);
            Node::Add(a, b)
        }
        SE::Sub { x, y, .. } => {
            let a = conv(x, dag, memo);
            let b = conv(y, dag, memo);
            Node::Sub(a, b)
        }
        SE::Mul { x, y, .. } => {
            let a = conv(x, dag, memo);
            let b = conv(y, dag, memo);
            Node::Mul(a, b)
        }
        SE::Neg { x, .. } => {
            let a = conv(x, dag, memo);
            Node::Neg(a)
        }
    };
    let id = dag.intern(n);
    memo.insert(key, id);
    id
}

fn vpc(v: &p3_air::VirtualPairCol<F>, dag: &mut Dag) -> usize {
    let terms: Vec<(Node, u64)> = v
        .column_weights
        .iter()
        .map(|(c, w)| {
            let node = match c {
                PairCol::Main(i) => Node::Main(0, *i),
                PairCol::Preprocessed(i) => Node::Prep(0, *i),
            };
            (node, w.as_canonical_u32() as u64)
        })
        .collect();
    dag.affine(&terms, v.constant.as_canonical_u32() as u64)
}

fn lookup(l: &VirtualPairLookup<F>, dir: &'static str, dag: &mut Dag) -> Interaction {
    let values = l.values.iter().map(|v| vpc(v, dag)).collect();
    let mult = vpc(&l.mult, dag);
    Interaction {
        dir,
        kind: Some(l.kind as u32),
        kind_name: format!("{:?}", l.kind),
        bus: None,
        scope: Some(format!("{:?}", l.scope).to_lowercase()),
        values,
        mult,
        count_weight: None,
    }
}

fn extract_air<C>(chip: &C, name: &str, group: &str, index: usize, expected: (usize, usize)) -> Result<AirDoc, String>
where
    C: BaseAir<F> + Air<SymbolicConstraintFolder<F>> + ChipBehavior<F>,
{
    let pw = chip.preprocessed_width();
    let w = chip.width();
    let mut b = SymbolicConstraintFolder::<F>::new(pw, w);
    chip.eval(&mut b);
    let cons = b.constraints();
    let mut b2 = SymbolicConstraintFolder::<F>::new(pw, w);
    chip.eval(&mut b2);
    let (looking, looked) = b2.lookups();
    if (looking.len(), looked.len()) != expected {
        return Err(format!(
            "lookup counts ({}, {}) differ from MetaChip ({}, {})",
            looking.len(),
            looked.len(),
            expected.0,
            expected.1
        ));
    }
    let mut dag = Dag::default();
    let mut memo = HashMap::new();
    let constraints = cons.iter().map(|c| conv(c, &mut dag, &mut memo)).collect();
    let mut interactions = vec![];
    for l in looking.iter() {
        interactions.push(lookup(l, "send", &mut dag));
    }
    for l in looked.iter() {
        interactions.push(lookup(l, "receive", &mut dag));
    }
    Ok(AirDoc {
        zkvm: "pico".into(),
        release: "v2.1.2".into(),
        commit: std::env::var("BOOLE_COMMIT").unwrap_or_else(|_| "bfa8f12e6e1a19a56f61dea8d78a5dd3a28de255".into()),
        field_name: "KoalaBear".into(),
        p: P,
        name: name.into(),
        rust_type: name.into(),
        group: group.into(),
        index,
        width: w,
        prep_width: pw,
        num_public_values: MAX_NUM_PVS,
        dag,
        constraints,
        interactions,
        meta: vec![
            ("looking".into(), looking.len().to_string()),
            ("looked".into(), looked.len().to_string()),
            ("direction_convention".into(), "looking = send (+multiplicity), looked = receive (-multiplicity)".into()),
        ],
    })
}

// ------------------------------------------------------------------------------------------ programs

const DATA: u64 = 0x0020_0000;

struct Prog {
    ins: Vec<Instruction>,
    mem: BTreeMap<u64, u64>,
}

impl Prog {
    fn new() -> Self {
        Self { ins: vec![], mem: BTreeMap::new() }
    }
    /// rd = imm (ADD with an immediate, as the pilot programs and the LUI transpilation do).
    fn li(&mut self, rd: u8, v: u64) {
        self.ins.push(Instruction::new(Opcode::ADD, rd, 0, v, false, true));
    }
    fn words(&mut self, addr: u64, ws: &[u64]) {
        for (i, w) in ws.iter().enumerate() {
            self.mem.insert(addr + 8 * i as u64, *w);
        }
    }
    fn ecall(&mut self, code: u32, a0: u64, a1: u64) {
        self.li(5, code as u64);
        self.li(10, a0);
        self.li(11, a1);
        self.ins.push(Instruction::new(Opcode::ECALL, 0, 0, 0, false, false));
    }
    fn finish(mut self) -> Arc<Program> {
        self.li(5, 0);
        self.li(10, 0);
        self.ins.push(Instruction::new(Opcode::ECALL, 0, 0, 0, false, false));
        let mut p = Program::new(self.ins, 0x10000, 0x10000);
        p.memory_image = Arc::new(self.mem);
        Arc::new(p)
    }
}

fn edge_values() -> Vec<u64> {
    vec![0, 1, 2, u64::MAX, i64::MIN as u64, i64::MAX as u64, 0xFFFF_FFFF, 0x8000_0000, 0x7FFF_FFFF,
         0xFFFF_FFFF_0000_0000, 0x0000_0001_0000_0000, 0x1234_5678_9abc_def0]
}

fn program_core(rng: &mut Rng) -> Arc<Program> {
    let mut p = Prog::new();
    let mut vals = edge_values();
    while vals.len() < 40 {
        vals.push(rng.next());
    }
    let rr = [Opcode::ADD, Opcode::SUB, Opcode::XOR, Opcode::OR, Opcode::AND, Opcode::SLL, Opcode::SRL, Opcode::SRA,
        Opcode::SLT, Opcode::SLTU, Opcode::MUL, Opcode::MULH, Opcode::MULHU, Opcode::MULHSU, Opcode::DIV,
        Opcode::DIVU, Opcode::REM, Opcode::REMU, Opcode::ADDW, Opcode::SUBW, Opcode::SLLW, Opcode::SRLW, Opcode::SRAW,
        Opcode::MULW, Opcode::DIVW, Opcode::DIVUW, Opcode::REMW, Opcode::REMUW];
    for (k, op) in rr.iter().enumerate() {
        for j in 0..14 {
            p.li(10, vals[(j + k) % vals.len()]);
            p.li(11, vals[(3 * j + 2 * k + 1) % vals.len()]);
            let rd = if j == 13 { 0 } else { 12 };
            p.ins.push(Instruction::new(*op, rd, 10, 11, false, false));
        }
    }
    let ri = [Opcode::ADD, Opcode::XOR, Opcode::OR, Opcode::AND, Opcode::SLL, Opcode::SRL, Opcode::SRA, Opcode::SLT,
        Opcode::SLTU, Opcode::ADDW, Opcode::SLLW, Opcode::SRLW, Opcode::SRAW];
    for (k, op) in ri.iter().enumerate() {
        for j in 0..12 {
            p.li(10, vals[(j * 5 + k) % vals.len()]);
            let imm = if matches!(op, Opcode::SLL | Opcode::SRL | Opcode::SRA) {
                (j as u64 * 7) % 64
            } else if matches!(op, Opcode::SLLW | Opcode::SRLW | Opcode::SRAW) {
                (j as u64 * 5) % 32
            } else {
                [0u64, 1, 2047, 0xFFFF_FFFF_FFFF_F800, 5, 1000, u64::MAX, 3, 42, 100, 7, 2046][j]
            };
            p.ins.push(Instruction::new(*op, 13, 10, imm, false, true));
        }
    }
    let base = DATA + 0x1_0000;
    let words: Vec<u64> = (0..16).map(|_| rng.next()).collect();
    p.words(base, &words);
    p.li(20, base);
    let loads = [Opcode::LB, Opcode::LH, Opcode::LW, Opcode::LD, Opcode::LBU, Opcode::LHU, Opcode::LWU];
    for (k, op) in loads.iter().enumerate() {
        let align = match op {
            Opcode::LB | Opcode::LBU => 1,
            Opcode::LH | Opcode::LHU => 2,
            Opcode::LW | Opcode::LWU => 4,
            _ => 8,
        };
        for j in 0..12u64 {
            let off = ((j * 13 + k as u64 * 3) % 64) / align * align;
            let rd = if j == 11 { 0 } else { 14 };
            p.ins.push(Instruction::new(*op, rd, 20, off, false, true));
        }
    }
    let stores = [Opcode::SB, Opcode::SH, Opcode::SW, Opcode::SD];
    for (k, op) in stores.iter().enumerate() {
        let align = match op {
            Opcode::SB => 1,
            Opcode::SH => 2,
            Opcode::SW => 4,
            _ => 8,
        };
        for j in 0..12u64 {
            p.li(15, vals[(j as usize * 3 + k) % vals.len()]);
            let off = ((j * 11 + k as u64 * 5) % 64) / align * align;
            p.ins.push(Instruction::new(*op, 15, 20, off, false, true));
        }
    }
    let br = [Opcode::BEQ, Opcode::BNE, Opcode::BLT, Opcode::BGE, Opcode::BLTU, Opcode::BGEU];
    for (k, op) in br.iter().enumerate() {
        for j in 0..12 {
            let b = vals[(j + 2 * k) % vals.len()];
            let c = if j % 3 == 0 { b } else { vals[(j * 7 + k + 1) % vals.len()] };
            p.li(10, b);
            p.li(11, c);
            p.ins.push(Instruction::new(*op, 10, 11, 8, false, true));
            p.ins.push(Instruction::new(Opcode::ADD, 16, 16, 1, false, true));
        }
    }
    for j in 0..12u64 {
        let rd = if j == 11 { 0 } else { 1 };
        p.ins.push(Instruction::new(Opcode::JAL, rd, 8, 0, true, true));
        p.ins.push(Instruction::new(Opcode::ADD, 16, 16, 1, false, true));
    }
    for j in 0..12u64 {
        let jalr_pc = 0x10000 + 4 * (p.ins.len() as u64 + 1);
        p.li(21, jalr_pc + 8);
        let rd = if j == 11 { 0 } else { 1 };
        p.ins.push(Instruction::new(Opcode::JALR, rd, 21, 0, false, true));
        p.ins.push(Instruction::new(Opcode::ADD, 16, 16, 1, false, true));
    }
    for j in 0..12u64 {
        let u = (j * 0x0098_7000) & 0x7FFF_F000;
        p.ins.push(Instruction::new(Opcode::AUIPC, 17, u, u, true, true));
    }
    p.finish()
}

fn words_of(x: &BigUint, n: usize) -> Vec<u64> {
    let mut d = x.to_u64_digits();
    d.resize(n, 0);
    d
}

fn point_words<E: EllipticCurve>(p: &AffinePoint<E>, n: usize) -> Vec<u64> {
    let mut w = words_of(&p.x, n);
    w.extend(words_of(&p.y, n));
    w
}

fn points<E: EllipticCurve>(n: usize) -> Vec<AffinePoint<E>> {
    let g = E::ec_generator();
    let mut out = vec![g.clone()];
    let mut cur = E::ec_double(&g);
    while out.len() < n {
        out.push(cur.clone());
        cur = E::ec_add(&cur, &g);
    }
    out
}

/// Precompiles without an in-tree ELF (secp256r1, secp256k1 decompress, secp256k1 field ops, uint256 mul).
fn program_precompiles(rng: &mut Rng) -> Arc<Program> {
    let mut p = Prog::new();
    let mut next = DATA;
    let mut alloc = |n: u64| {
        let a = next;
        next += (8 * n + 0xFF) / 0x100 * 0x100;
        a
    };
    const N: usize = 10;
    let r1 = points::<Secp256r1>(N + 1);
    for k in 0..N {
        let (a, b) = (alloc(8), alloc(8));
        p.words(a, &point_words(&r1[k + 1], 4));
        p.words(b, &point_words(&r1[0], 4));
        p.ecall(0x00_01_01_30, a, b);
        let d = alloc(8);
        p.words(d, &point_words(&r1[k], 4));
        p.ecall(0x00_00_01_31, d, 0);
    }
    // decompress: x in the second half of the slice, sign = y mod 2 (LSB rule)
    let xs_k1: Vec<(BigUint, BigUint)> = points::<Secp256k1>(N).into_iter().map(|q| (q.x, q.y)).collect();
    let xs_r1: Vec<(BigUint, BigUint)> = r1.iter().take(N).map(|q| (q.x.clone(), q.y.clone())).collect();
    for (pts, code) in [(xs_k1, 0x00_00_01_0Cu32), (xs_r1, 0x00_00_01_32)] {
        for (x, y) in pts.iter() {
            let s = alloc(8);
            p.words(s + 32, &words_of(x, 4));
            let sign = if (y % BigUint::from(2u32)) == BigUint::one() { 1 } else { 0 };
            p.ecall(code, s, sign);
        }
    }
    // bn254 add / double (the in-tree ELF calls them only a few times)
    let bn = points::<Bn254>(N + 1);
    for k in 0..N {
        let (a, b) = (alloc(8), alloc(8));
        p.words(a, &point_words(&bn[k + 1], 4));
        p.words(b, &point_words(&bn[0], 4));
        p.ecall(0x00_01_01_0E, a, b);
        let d = alloc(8);
        p.words(d, &point_words(&bn[k], 4));
        p.ecall(0x00_00_01_0F, d, 0);
    }
    // fp2 add / sub over bn254 (4 dwords per coordinate) and bls12-381 (6 dwords)
    let bls = points::<Bls12381>(N + 2);
    for k in 0..N {
        for code in [0x00_01_01_29u32, 0x00_01_01_2A] {
            let (x, y) = (alloc(8), alloc(8));
            p.words(x, &point_words(&bn[k], 4));
            p.words(y, &point_words(&bn[k + 1], 4));
            p.ecall(code, x, y);
        }
        for code in [0x00_01_01_23u32, 0x00_01_01_24] {
            let (x, y) = (alloc(12), alloc(12));
            p.words(x, &point_words(&bls[k], 6));
            p.words(y, &point_words(&bls[k + 1], 6));
            p.ecall(code, x, y);
        }
    }
    let k1 = points::<Secp256k1>(N + 2);
    for k in 0..N {
        for code in [0x00_01_01_2Cu32, 0x00_01_01_2D, 0x00_01_01_2E] {
            let (x, y) = (alloc(4), alloc(4));
            p.words(x, &words_of(&k1[k].x, 4));
            p.words(y, &words_of(&k1[k + 1].y, 4));
            p.ecall(code, x, y);
        }
    }
    for k in 0..N {
        let m: BigUint = if k % 3 == 0 {
            BigUint::one() << 256
        } else {
            BigUint::from_slice(&(0..8).map(|_| rng.next() as u32).collect::<Vec<_>>()) | BigUint::one()
        };
        let x = BigUint::from_slice(&(0..8).map(|_| rng.next() as u32).collect::<Vec<_>>()) % &m;
        let y = BigUint::from_slice(&(0..8).map(|_| rng.next() as u32).collect::<Vec<_>>()) % &m;
        let (px, py) = (alloc(4), alloc(8));
        p.words(px, &words_of(&x, 4));
        let mut yw = words_of(&y, 4);
        yw.extend(if k % 3 == 0 { vec![0; 4] } else { words_of(&m, 4) });
        p.words(py, &yw);
        p.ecall(0x00_01_01_1D, px, py);
    }
    p.finish()
}

fn elf(path: &str) -> Option<Arc<Program>> {
    let bytes = std::fs::read(path).ok()?;
    let c = Compiler::new(SourceType::RISCV, &bytes).ok()?;
    Some(c.compile())
}

fn stdin_with(writes: &[Vec<u8>], u64s: &[u64]) -> EmulatorStdin<Program, Vec<u8>> {
    let mut b = EmulatorStdin::<Program, Vec<u8>>::new_builder::<KoalaBearPoseidon2>();
    for v in u64s {
        b.write(v);
    }
    for w in writes {
        b.write(w);
    }
    b.finalize().0
}

// ------------------------------------------------------------------------------------------ rows

struct Sink {
    docs: HashMap<usize, RowsDoc>,
}

fn distinct(rows: &[(usize, Vec<u64>)]) -> usize {
    rows.iter().map(|x| &x.1).collect::<HashSet<_>>().len()
}

fn mrows(m: &RowMajorMatrix<F>, idx: &[usize]) -> Vec<(usize, Vec<u64>)> {
    idx.iter()
        .filter(|&&i| i < m.height())
        .map(|&i| (i, m.row_slice(i).iter().map(|x| x.as_canonical_u32() as u64).collect()))
        .collect()
}

fn run_riscv(
    metas: &[MetaChip<F, RiscvChipType<F>>],
    program: Arc<Program>,
    stdin: EmulatorStdin<Program, Vec<u8>>,
    label: &str,
    sink: &mut Sink,
    log: &mut String,
) {
    let r = catch_unwind(AssertUnwindSafe(|| crate::chips::tests::test_rv64_emulate(program.clone(), stdin)));
    let mut records = match r {
        Ok(r) => r,
        Err(e) => {
            log.push_str(&format!("{label}: emulation panic: {}\n", panic_message(e.as_ref())));
            return;
        }
    };
    log.push_str(&format!("{label}: {} records\n", records.len()));
    // the prover's complement step (complement_record_static): each chip's extra record is appended in order
    for rec in records.iter_mut() {
        for c in metas.iter() {
            if c.is_active(rec) {
                let mut extra = EmulationRecord::default();
                c.extra_record(rec, &mut extra);
                rec.append(&mut extra);
            }
        }
    }
    let preps: Vec<Option<RowMajorMatrix<F>>> = metas.iter().map(|c| c.generate_preprocessed(&program)).collect();
    for (ri, rec) in records.iter().enumerate() {
        let pv: Vec<u64> = rec.public_values::<F>().iter().map(|x| x.as_canonical_u32() as u64).collect();
        for (i, c) in metas.iter().enumerate() {
            if !c.is_active(rec) {
                continue;
            }
            // enough distinct rows already (ByteChip::generate_main is O(2^17 x lookups): do not repeat it)
            if sink.docs.get(&i).map(|d| distinct(&d.main_rows) >= 40).unwrap_or(false) {
                continue;
            }
            let t = match catch_unwind(AssertUnwindSafe(|| c.generate_main(rec, &mut EmulationRecord::default()))) {
                Ok(t) => t,
                Err(e) => {
                    log.push_str(&format!("{label}#{ri} {}: generate_main panic: {}\n", c.name(), panic_message(e.as_ref())));
                    continue;
                }
            };
            if t.height() == 0 {
                continue;
            }
            let idx = select_rows(t.height(), 48, 16, 0x5eed ^ i as u64);
            let main_rows = mrows(&t, &idx);
            if let Some(d) = sink.docs.get(&i) {
                if distinct(&main_rows) <= distinct(&d.main_rows) {
                    continue;
                }
            }
            let prep_rows = preps[i].as_ref().map(|pm| mrows(pm, &idx)).unwrap_or_default();
            sink.docs.insert(
                i,
                RowsDoc {
                    air_index: i,
                    name: c.name(),
                    source: format!("{label} record {ri} (RiscvEmulator + extra_record complement + generate_main)"),
                    height: t.height(),
                    width: t.width(),
                    prep_width: c.preprocessed_width(),
                    public_values: pv.clone(),
                    main_rows,
                    prep_rows,
                },
            );
        }
    }
}

fn recursion_rows(metas: &[MetaChip<F, RecursionChipType<F>>], offset: usize, sink: &mut Sink, log: &mut String) {
    type EF = BinomialExtensionField<F, 4>;
    let mut ins = vec![];
    let mut rng = Rng(0x0dec_0ded);
    let n = 24u32;
    ins.push(rinstr::mem(MemAccessKind::Write, 1, 0, 0));
    ins.push(rinstr::mem(MemAccessKind::Write, n + 2, 1, 1));
    for i in 2..=n {
        let op = [BaseAluOpcode::AddF, BaseAluOpcode::SubF, BaseAluOpcode::MulF, BaseAluOpcode::AddF][(i % 4) as usize];
        ins.push(rinstr::base_alu(op, 2, i, i - 2, i - 1));
    }
    let mut addr = 1000u32;
    for _ in 0..48 {
        let a: [F; 4] = core::array::from_fn(|_| F::from_canonical_u32((rng.next() % P) as u32));
        let b: [F; 4] = core::array::from_fn(|_| F::from_canonical_u32((rng.next() % P) as u32));
        ins.push(rinstr::mem_ext(MemAccessKind::Write, 1, addr, EF::from_base_slice(&a)));
        ins.push(rinstr::mem_ext(MemAccessKind::Write, 1, addr + 1, EF::from_base_slice(&b)));
        ins.push(rinstr::ext_alu(ExtAluOpcode::MulE, 1, addr + 2, addr, addr + 1));
        addr += 3;
    }
    let mut base = 5000u32;
    for _ in 0..12 {
        for j in 0..16u32 {
            ins.push(rinstr::mem(MemAccessKind::Write, 1, base + j, (rng.next() % P) as u32));
        }
        let input: [u32; 16] = core::array::from_fn(|j| base + j as u32);
        let output: [u32; 16] = core::array::from_fn(|j| base + 16 + j as u32);
        // output multiplicity 1: active output writes (no later read in this sample program)
        ins.push(rinstr::poseidon2([1; 16], output, input));
        base += 32;
    }
    // selects (Select), bit decompositions (MemoryVar through HintBits) and exp-reverse-bits (ExpReverseBitsLen)
    {
        use crate::compiler::recursion::{
            instruction::{HintBitsInstr, Instruction as RInstr},
            types::{Address, SelectInstr, SelectIo},
        };
        let a = |x: u32| Address(F::from_canonical_u32(x));
        for k in 0..24u32 {
            ins.push(rinstr::mem(MemAccessKind::Write, 1, base, k % 2));
            ins.push(rinstr::mem(MemAccessKind::Write, 1, base + 1, (rng.next() % P) as u32));
            ins.push(rinstr::mem(MemAccessKind::Write, 1, base + 2, (rng.next() % P) as u32));
            ins.push(RInstr::Select(SelectInstr {
                addrs: SelectIo { bit: a(base), out1: a(base + 3), out2: a(base + 4), in1: a(base + 1), in2: a(base + 2) },
                mult1: F::ONE,
                mult2: F::ONE,
            }));
            base += 5;
        }
        for _ in 0..6u32 {
            ins.push(rinstr::mem(MemAccessKind::Write, 0, base, (rng.next() % 65536) as u32));
            ins.push(RInstr::HintBits(HintBitsInstr {
                output_addrs_mults: (1..=16).map(|i| (a(base + i), F::ONE)).collect(),
                input_addr: a(base),
            }));
            base += 17;
        }
        for k in 0..24u32 {
            let len = 4 + (k % 5);
            ins.push(rinstr::mem(MemAccessKind::Write, 1, base, (rng.next() % P) as u32));
            for i in 0..len {
                ins.push(rinstr::mem(MemAccessKind::Write, 1, base + 1 + i, (rng.next() % 2) as u32));
            }
            let exp: Vec<F> = (0..len).map(|i| F::from_canonical_u32(base + 1 + i)).collect();
            ins.push(rinstr::exp_reverse_bits_len(1, F::from_canonical_u32(base), exp,
                                                  F::from_canonical_u32(base + 1 + len)));
            base += len + 2;
        }
    }
    let total = base as usize + 64;
    let program = RecursionProgram { instructions: ins, total_memory: total, traces: vec![], shape: None };
    let r = catch_unwind(AssertUnwindSafe(|| {
        let mut rt = Runtime::<F, EF, _, _, KOALABEAR_S_BOX_DEGREE>::new(
            Arc::new(program.clone()),
            KoalaBearPoseidon2::default().perm.clone(),
        );
        rt.run().map(|_| rt.record).map_err(|e| format!("{e}"))
    }));
    let record = match r {
        Ok(Ok(rec)) => rec,
        Ok(Err(e)) => {
            log.push_str(&format!("recursion runtime error: {e}\n"));
            return;
        }
        Err(e) => {
            log.push_str(&format!("recursion runtime panic: {}\n", panic_message(e.as_ref())));
            return;
        }
    };
    let pv: Vec<u64> = record.public_values::<F>().iter().map(|x| x.as_canonical_u32() as u64).collect();
    for (k, c) in metas.iter().enumerate() {
        let i = offset + k;
        let t = match catch_unwind(AssertUnwindSafe(|| c.generate_main(&record, &mut Default::default()))) {
            Ok(t) if t.height() > 0 => t,
            Ok(_) => continue,
            Err(e) => {
                log.push_str(&format!("recursion {}: generate_main panic: {}\n", c.name(), panic_message(e.as_ref())));
                continue;
            }
        };
        let prep = c.generate_preprocessed(&program);
        let idx = select_rows(t.height(), 48, 16, 0x5eed ^ i as u64);
        sink.docs.insert(
            i,
            RowsDoc {
                air_index: i,
                name: c.name(),
                source: "linear recursion program (base / extension ALU, memory, Poseidon2, select, hint bits, \
                         exp-reverse-bits) + generate_main"
                    .into(),
                height: t.height(),
                width: t.width(),
                prep_width: c.preprocessed_width(),
                public_values: pv.clone(),
                main_rows: mrows(&t, &idx),
                prep_rows: prep.as_ref().map(|pm| mrows(pm, &idx)).unwrap_or_default(),
            },
        );
    }
}

// ------------------------------------------------------------------------------------------ main

fn write(path: &str, text: &str) {
    std::fs::write(path, text).unwrap_or_else(|e| panic!("write {path}: {e}"));
}

fn run(out: String) {
    std::fs::create_dir_all(format!("{out}/airs")).unwrap();
    std::fs::create_dir_all(format!("{out}/rows")).unwrap();
    let mut manifest = String::new();
    let mut log = String::new();
    let mut record = |index: usize, name: &str, group: &str, r: std::thread::Result<Result<AirDoc, String>>| {
        let (status, detail) = match r {
            Ok(Ok(doc)) => {
                write(&format!("{out}/airs/{index}.json"), &doc.to_json());
                ("extracted", format!("{} constraints, {} interactions", doc.constraints.len(), doc.interactions.len()))
            }
            Ok(Err(e)) => ("failed", e),
            Err(e) => ("failed", format!("panic: {}", panic_message(e.as_ref()))),
        };
        manifest.push_str(&manifest_line(index, name, name, group, status, &detail));
        manifest.push('\n');
    };
    let variants = RiscvChipType::<F>::all_chip_variants();
    let metas = RiscvChipType::<F>::all_chips();
    let mut index = 0;
    for (v, m) in variants.iter().zip(metas.iter()) {
        let name = ChipBehavior::<F>::name(v);
        let exp = (m.get_looking().len(), m.get_looked().len());
        let r = catch_unwind(AssertUnwindSafe(|| extract_air(v, &name, "riscv", index, exp)));
        record(index, &name, "riscv", r);
        index += 1;
    }
    let rec_offset = index;
    let rmetas = RecursionChipType::<F>::all_chips();
    let rvariants: Vec<RecursionChipType<F>> = recursion_variants();
    for (v, m) in rvariants.iter().zip(rmetas.iter()) {
        let name = format!("Recursion{}", ChipBehavior::<F>::name(v));
        let exp = (m.get_looking().len(), m.get_looked().len());
        let r = catch_unwind(AssertUnwindSafe(|| extract_air(v, &name, "recursion", index, exp)));
        record(index, &name, "recursion", r);
        index += 1;
    }
    drop(record);
    write(&format!("{out}/manifest.jsonl"), &manifest);

    let mut sink = Sink { docs: HashMap::new() };
    let mut rng = Rng(0x5eed_5eed_1234_5678);
    run_riscv(&metas, program_core(&mut rng), stdin_with(&[], &[]), "core", &mut sink, &mut log);
    run_riscv(&metas, program_precompiles(&mut rng), stdin_with(&[], &[]), "precompiles", &mut sink, &mut log);
    let bench = std::env::var("BOOLE_BENCH").unwrap_or_else(|_| "../perf/bench_data/rv64".into());
    let elfs: Vec<(&str, Vec<Vec<u8>>, Vec<u64>)> = vec![
        ("fib-elf", vec![], vec![10]),
        ("sha2-elf", vec![vec![3u8; 32], vec![1u8; 32]], vec![]),
        ("tiny-keccak-elf", vec![vec![0x61u8, 0x62, 0x63]], vec![]),
        ("k256-elf", vec![], vec![]),
        ("bls12381-elf-ws-add-double-decompress", vec![], vec![]),
        ("bn254-elf-ws-add-double", vec![], vec![]),
        ("bn254-fp-elf", vec![], vec![]),
        ("bls12381-fp-elf", vec![], vec![]),
    ];
    for (name, writes, u64s) in elfs {
        match elf(&format!("{bench}/{name}")) {
            Some(prog) => run_riscv(&metas, prog, stdin_with(&writes, &u64s), name, &mut sink, &mut log),
            None => log.push_str(&format!("{name}: could not load\n")),
        }
    }
    recursion_rows(&rmetas, rec_offset, &mut sink, &mut log);
    for (i, d) in sink.docs.iter() {
        write(&format!("{out}/rows/{i}.json"), &d.to_json());
    }
    log.push_str(&format!("extracted {index} AIRs; rows for {} AIRs\n", sink.docs.len()));
    write(&format!("{out}/extract.log"), &log);
    eprint!("{log}");
}

/// The raw recursion chip values in `RecursionChipType::all_chips()` order (MetaChip keeps them private).
fn recursion_variants() -> Vec<RecursionChipType<F>> {
    use crate::chips::chips::{
        alu_base::BaseAluChip, alu_ext::ExtAluChip, batch_fri::BatchFRIChip, exp_reverse_bits::ExpReverseBitsLenChip,
        public_values::PublicValuesChip, recursion_memory::{constant::MemoryConstChip, variable::MemoryVarChip},
        select::SelectChip,
    };
    vec![
        RecursionChipType::MemoryConst(MemoryConstChip::default()),
        RecursionChipType::MemoryVar(MemoryVarChip::default()),
        RecursionChipType::Select(SelectChip::default()),
        RecursionChipType::ExpReverseBitsLen(ExpReverseBitsLenChip::default()),
        RecursionChipType::BaseAlu(BaseAluChip::default()),
        RecursionChipType::ExtAlu(ExtAluChip::default()),
        RecursionChipType::BatchFRI(BatchFRIChip::default()),
        RecursionChipType::PublicValues(PublicValuesChip::default()),
        RecursionChipType::Poseidon2(Default::default()),
    ]
}

#[test]
fn extract() {
    let out = std::env::var("BOOLE_OUT").expect("BOOLE_OUT");
    std::thread::Builder::new().stack_size(1 << 30).spawn(move || run(out)).unwrap().join().unwrap();
}

#[allow(unused)]
fn _keep(_: VirtualPairLookup<F>) {}
