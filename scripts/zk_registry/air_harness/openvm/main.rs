//! Boole AIR extractor for OpenVM (scratch copy of the OpenVM workspace at the census pin; not upstream code).
//!
//! Usage: `boole-air-extract <out-dir> <openvm-repo-root>` writes `manifest.jsonl`, `airs/<air_id>.json`
//! (boole-air-ir/v1) and `rows/<air_id>.json` (boole-air-rows/v1).
//!
//! * AIRs: every AIR of the SDK's standard application VM (`SdkVmConfig::standard()`), from the proving key that
//!   `VirtualMachine::new_with_keygen` builds (the same path as the SDK's app keygen): per AIR, the vkey's
//!   `SymbolicConstraintsDag` (constraint DAG and interactions with the configuration's real bus indices) and
//!   trace widths.  A receive is recorded by the backend as a negated count (`Neg(x)` or `(-1) * x`); the
//!   extractor records it as `receive` with `x` as multiplicity.
//! * Rows: the real VM (metered execution, preflight, `generate_proving_ctx`) on the committed benchmark ELFs
//!   (fibonacci, sha2, keccak256 with the standard configuration; ecrecover, pairing and kitchen-sink with their
//!   own `openvm.toml` configurations).  Rows of another configuration are attributed to a standard AIR whose name,
//!   widths and constraint DAG are identical (bus indices do not enter the rows).
//! * Ratchet mode (`BOOLE_RATCHET_AIRS`, `BOOLE_AIR_SEED`; see `boole_air_ir.rs`): every row of every trace of
//!   the requested AIRs; with a non-zero seed only the straight-line program (other operand values).
use std::{
    collections::{hash_map::DefaultHasher, HashMap, HashSet},
    hash::{Hash, Hasher},
    panic::{catch_unwind, AssertUnwindSafe},
};

mod boole_air_ir;
use boole_air_ir::*;

use openvm_circuit::arch::{
    execution_mode::Segment, instructions::exe::VmExe, PreflightExecutionOutput, Streams, VirtualMachine,
};
use openvm_sdk_config::{SdkVmConfig, SdkVmCpuBuilder, TranspilerConfig};
use openvm_stark_backend::{
    air_builders::symbolic::{symbolic_variable::Entry, SymbolicConstraintsDag, SymbolicExpressionNode},
    keygen::types::MultiStarkProvingKey,
    p3_field::{PrimeCharacteristicRing, PrimeField32},
    p3_matrix::{dense::RowMajorMatrix, Matrix},
    StarkEngine,
};
use openvm_stark_sdk::config::{
    app_params_with_100_bits_security,
    baby_bear_poseidon2::{BabyBearPoseidon2Config as SC, BabyBearPoseidon2CpuEngine, F},
    MAX_APP_LOG_STACKED_HEIGHT,
};
use openvm_transpiler::{elf::Elf, openvm_platform::memory::MEM_SIZE, FromElf};

type Engine = BabyBearPoseidon2CpuEngine;
const P: u64 = 2013265921;

fn engine() -> Engine {
    Engine::new(app_params_with_100_bits_security(MAX_APP_LOG_STACKED_HEIGHT))
}

// ------------------------------------------------------------------------------------------ extraction

struct Extracted {
    doc: AirDoc,
    key: u64,
}

fn part_offsets(cached: &[usize], common: usize) -> Vec<usize> {
    let mut off = vec![];
    let mut k = 0;
    for w in cached.iter().chain(std::iter::once(&common)) {
        off.push(k);
        k += w;
    }
    off
}

/// Content key: name, widths and the constraint DAG (not the interactions: bus indices differ between
/// configurations, rows do not).
fn content_key(name: &str, widths: (&Option<usize>, &[usize], usize), dag: &SymbolicConstraintsDag<F>) -> u64 {
    let mut h = DefaultHasher::new();
    name.hash(&mut h);
    format!("{:?}", widths).hash(&mut h);
    format!("{:?}", dag.constraints).hash(&mut h);
    h.finish()
}

fn extract(pk: &MultiStarkProvingKey<SC>, id: usize) -> Result<Extracted, String> {
    extract_as(pk, id, id, "app-vm", "SdkVmConfig::standard()")
}

fn extract_as(pk: &MultiStarkProvingKey<SC>, id: usize, index: usize, group: &str, config: &str) -> Result<Extracted, String> {
    let apk = &pk.per_air[id];
    let vk = &apk.vk;
    let w = &vk.params.width;
    let dag = &vk.symbolic_constraints;
    let offs = part_offsets(&w.cached_mains, w.common_main);
    let width: usize = w.cached_mains.iter().sum::<usize>() + w.common_main;
    let prep_width = w.preprocessed.unwrap_or(0);
    let mut out = Dag::default();
    let mut map: Vec<usize> = Vec::with_capacity(dag.constraints.nodes.len());
    for node in dag.constraints.nodes.iter() {
        let n = match node {
            SymbolicExpressionNode::Variable(v) => match v.entry {
                Entry::Main { part_index, offset } => Node::Main(offset as u8, offs[part_index] + v.index),
                Entry::Preprocessed { offset } => Node::Prep(offset as u8, v.index),
                Entry::Public => Node::Pub(v.index),
                ref e => return Err(format!("unsupported entry {e:?}")),
            },
            SymbolicExpressionNode::IsFirstRow => Node::First,
            SymbolicExpressionNode::IsLastRow => Node::Last,
            SymbolicExpressionNode::IsTransition => Node::Trans,
            SymbolicExpressionNode::Constant(c) => Node::Const(c.as_canonical_u32() as u64),
            SymbolicExpressionNode::Add { left_idx, right_idx, .. } => Node::Add(map[*left_idx], map[*right_idx]),
            SymbolicExpressionNode::Sub { left_idx, right_idx, .. } => Node::Sub(map[*left_idx], map[*right_idx]),
            SymbolicExpressionNode::Mul { left_idx, right_idx, .. } => Node::Mul(map[*left_idx], map[*right_idx]),
            SymbolicExpressionNode::Neg { idx, .. } => Node::Neg(map[*idx]),
        };
        map.push(out.intern(n));
    }
    let constraints = dag.constraints.constraint_idx.iter().map(|&i| map[i]).collect();
    let mut interactions = vec![];
    for it in dag.interactions.iter() {
        let values = it.message.iter().map(|&i| map[i]).collect();
        // a receive is a negated count (PermutationCheckBus::receive, LookupBus::add_key_with_lookups): `-x` is
        // recorded either as Neg(x) or as (-1) * x
        let nodes = &dag.constraints.nodes;
        let minus_one = |i: usize| matches!(nodes[i], SymbolicExpressionNode::Constant(c) if c == -F::ONE);
        let (dir, mult) = match &nodes[it.count] {
            SymbolicExpressionNode::Neg { idx, .. } => ("receive", map[*idx]),
            SymbolicExpressionNode::Mul { left_idx, right_idx, .. } if minus_one(*left_idx) => ("receive", map[*right_idx]),
            SymbolicExpressionNode::Mul { left_idx, right_idx, .. } if minus_one(*right_idx) => ("receive", map[*left_idx]),
            _ => ("send", map[it.count]),
        };
        interactions.push(Interaction {
            dir,
            kind: None,
            kind_name: String::new(),
            bus: Some(it.bus_index as u32),
            scope: None,
            values,
            mult,
            count_weight: Some(it.count_weight),
        });
    }
    let name = apk.air_name.clone();
    let key = content_key(&name, (&w.preprocessed, &w.cached_mains, w.common_main), dag);
    Ok(Extracted {
        doc: AirDoc {
            zkvm: "openvm".into(),
            release: "v2.0.2".into(),
            commit: std::env::var("BOOLE_COMMIT").unwrap_or_else(|_| "59a69b8b0cbee7011ac978e4cc07707ee3681944".into()),
            field_name: "BabyBear".into(),
            p: P,
            name: name.clone(),
            rust_type: name,
            group: group.into(),
            index,
            width,
            prep_width,
            num_public_values: vk.params.num_public_values,
            dag: out,
            constraints,
            interactions,
            meta: vec![
                ("cached_main_widths".into(), format!("{:?}", w.cached_mains)),
                ("common_main_width".into(), w.common_main.to_string()),
                ("config".into(), config.into()),
            ],
        },
        key,
    })
}

// ------------------------------------------------------------------------------------------ rows

struct Sink {
    docs: HashMap<usize, RowsDoc>,
    all: Option<AllRows>,
}

fn distinct(rows: &[(usize, Vec<u64>)]) -> usize {
    rows.iter().map(|x| &x.1).collect::<HashSet<_>>().len()
}

fn row_of(parts: &[&RowMajorMatrix<F>], i: usize) -> Vec<u64> {
    parts
        .iter()
        .flat_map(|m| {
            let r = m.row_slice(i).expect("row in range");
            r.iter().map(|x| x.as_canonical_u32() as u64).collect::<Vec<_>>()
        })
        .collect()
}

/// Bytes of an `openvm::serde`-serialized integer as one input stream entry (one field element per byte).
fn stream_u64(n: u64) -> Vec<F> {
    n.to_le_bytes().iter().map(|b| F::from_u8(*b)).collect()
}

fn stream_u32(n: u32) -> Vec<F> {
    n.to_le_bytes().iter().map(|b| F::from_u8(*b)).collect()
}

fn elf_exe(config: &SdkVmConfig, elf_path: &str) -> Result<VmExe<F>, String> {
    let bytes = std::fs::read(elf_path).map_err(|e| format!("read {elf_path}: {e}"))?;
    let elf = Elf::decode(&bytes, MEM_SIZE as u32).map_err(|e| format!("decode: {e}"))?;
    VmExe::from_elf(elf, config.transpiler()).map_err(|e| format!("transpile: {e:?}"))
}

#[allow(clippy::too_many_arguments)]
fn run(
    label: &str,
    config: SdkVmConfig,
    exe: Result<VmExe<F>, String>,
    input: Vec<Vec<F>>,
    standard_keys: &HashMap<u64, Vec<usize>>,
    sink: &mut Sink,
    log: &mut String,
    max_segments: usize,
) -> Result<(), String> {
    let exe = exe?;
    let (mut vm, pk) =
        VirtualMachine::new_with_keygen(engine(), SdkVmCpuBuilder, config).map_err(|e| format!("keygen: {e:?}"))?;
    // standard AIR ids of this configuration's AIRs (same name, widths and constraint DAG)
    let mut to_std: HashMap<usize, usize> = HashMap::new();
    for (id, apk) in pk.per_air.iter().enumerate() {
        let w = &apk.vk.params.width;
        let k = content_key(&apk.air_name, (&w.preprocessed, &w.cached_mains, w.common_main), &apk.vk.symbolic_constraints);
        if let Some(ids) = standard_keys.get(&k) {
            if ids.len() == 1 {
                to_std.insert(id, ids[0]);
            }
        }
    }
    let streams: Streams<F> = Streams::new(input);
    let metered_ctx = vm.build_metered_ctx(&exe);
    let (segments, _) = vm
        .metered_interpreter(&exe)
        .map_err(|e| format!("{e:?}"))?
        .execute_metered(streams.clone(), metered_ctx)
        .map_err(|e| format!("metered: {e:?}"))?;
    log.push_str(&format!("{label}: {} segments, {} AIRs mapped to the standard VM\n", segments.len(), to_std.len()));
    let cached = vm.commit_program_on_device(&exe.program);
    vm.load_program(cached);
    let mut interp = vm.preflight_interpreter(&exe).map_err(|e| format!("{e:?}"))?;
    let mut state = Some(vm.create_initial_state(&exe, streams));
    for (si, seg) in segments.iter().enumerate().take(max_segments) {
        let Segment { num_insns, trace_heights, .. } = seg.clone();
        let from = state.take().unwrap();
        vm.transport_init_memory_to_device(&from.memory);
        let PreflightExecutionOutput { system_records, record_arenas, to_state } = vm
            .execute_preflight(&mut interp, from, Some(num_insns), &trace_heights)
            .map_err(|e| format!("preflight: {e:?}"))?;
        state = Some(to_state);
        let ctx = vm.generate_proving_ctx(system_records, record_arenas).map_err(|e| format!("ctx: {e:?}"))?;
        for (air_id, actx) in ctx.per_trace.iter() {
            let Some(&std_id) = to_std.get(air_id) else { continue };
            if sink.all.as_ref().map(|a| !a.wants(std_id, label)).unwrap_or(false) {
                continue;
            }
            let mut parts: Vec<&RowMajorMatrix<F>> = actx.cached_mains.iter().map(|c| &c.trace).collect();
            parts.push(&actx.common_main);
            let height = actx.common_main.height();
            if height == 0 {
                continue;
            }
            if let Some(all) = sink.all.as_mut() {
                let doc = RowsDoc {
                    air_index: std_id,
                    name: pk.per_air[*air_id].air_name.clone(),
                    source: format!("{label} segment {si} (metered execution + preflight + generate_proving_ctx)"),
                    height,
                    width: parts.iter().map(|m| m.width()).sum(),
                    prep_width: 0,
                    public_values: actx.public_values.iter().map(|x| x.as_canonical_u32() as u64).collect(),
                    main_rows: all_rows(height).into_iter().map(|i| (i, row_of(&parts, i))).collect(),
                    prep_rows: vec![],
                };
                all.write(label, &doc);
                continue;
            }
            // tables with full height (range, bitwise, range tuple): the first rows and a random sample
            let idx = select_rows(height, 48, 16, 0x5eed ^ std_id as u64);
            let rows: Vec<(usize, Vec<u64>)> = idx.iter().map(|&i| (i, row_of(&parts, i))).collect();
            if let Some(d) = sink.docs.get(&std_id) {
                if distinct(&rows) <= distinct(&d.main_rows) {
                    continue;
                }
            }
            sink.docs.insert(
                std_id,
                RowsDoc {
                    air_index: std_id,
                    name: pk.per_air[*air_id].air_name.clone(),
                    source: format!("{label} segment {si} (metered execution + preflight + generate_proving_ctx)"),
                    height,
                    width: parts.iter().map(|m| m.width()).sum(),
                    prep_width: 0,
                    public_values: actx.public_values.iter().map(|x| x.as_canonical_u32() as u64).collect(),
                    main_rows: rows,
                    prep_rows: vec![],
                },
            );
        }
    }
    Ok(())
}

fn ecrecover_input() -> Vec<Vec<F>> {
    use k256::ecdsa::{SigningKey, VerifyingKey};
    use rand_chacha::{rand_core::SeedableRng, ChaCha8Rng};
    use tiny_keccak::{Hasher, Keccak};
    let mut rng = ChaCha8Rng::seed_from_u64(12345);
    let signing_key = SigningKey::random(&mut rng);
    let verifying_key = VerifyingKey::from(&signing_key);
    let mut hasher = Keccak::v256();
    let mut expected_address = [0u8; 32];
    hasher.update(&verifying_key.to_encoded_point(false).as_bytes()[1..]);
    hasher.finalize(&mut expected_address);
    expected_address[..12].fill(0);
    let mut out = vec![expected_address.iter().map(|b| F::from_u8(*b)).collect::<Vec<_>>()];
    for msg in ["Elliptic", "Curve", "Digital", "Signature", "Algorithm"] {
        let mut hasher = Keccak::v256();
        hasher.update(msg.as_bytes());
        let mut prehash = [0u8; 32];
        hasher.finalize(&mut prehash);
        let (signature, recid) = signing_key.sign_prehash_recoverable(&prehash).unwrap();
        let mut input = prehash.to_vec();
        input.extend_from_slice(&[0; 31]);
        input.push(recid.to_byte() + 27u8);
        input.extend_from_slice(signature.to_bytes().as_ref());
        out.push(input.into_iter().map(F::from_u8).collect());
    }
    out
}

// ------------------------------------------------------------------------------------------ main

fn write(path: &str, text: &str) {
    std::fs::write(path, text).unwrap_or_else(|e| panic!("write {path}: {e}"));
}

fn main_inner(out: String, repo: String, rows: bool) {
    std::fs::create_dir_all(format!("{out}/airs")).unwrap();
    std::fs::create_dir_all(format!("{out}/rows")).unwrap();
    let mut log = String::new();
    let config = SdkVmConfig::standard();
    let (_vm, pk) = VirtualMachine::new_with_keygen(engine(), SdkVmCpuBuilder, config.clone()).expect("keygen");
    let mut manifest = String::new();
    let mut keys: HashMap<u64, Vec<usize>> = HashMap::new();
    for id in 0..pk.per_air.len() {
        let name = pk.per_air[id].air_name.clone();
        let r = catch_unwind(AssertUnwindSafe(|| extract(&pk, id)));
        let (status, detail) = match r {
            Ok(Ok(x)) => {
                write(&format!("{out}/airs/{id}.json"), &x.doc.to_json());
                keys.entry(x.key).or_default().push(id);
                ("extracted", format!("{} constraints, {} interactions", x.doc.constraints.len(), x.doc.interactions.len()))
            }
            Ok(Err(e)) => ("failed", e),
            Err(e) => ("failed", format!("panic: {}", panic_message(e.as_ref()))),
        };
        manifest.push_str(&manifest_line(id, &name, &name, "app-vm", status, &detail));
        manifest.push('\n');
    }
    // the leaf aggregation circuit that verifies proofs of this application VM (the SDK's leaf prover:
    // VerifierSubCircuit<MAX_NUM_CHILDREN_LEAF = 4> with continuations, inside InnerCircuit; leaf parameters)
    let leaf = catch_unwind(AssertUnwindSafe(|| {
        use openvm_continuations::circuit::{inner::InnerCircuit, Circuit};
        use openvm_recursion_circuit::system::{VerifierConfig, VerifierSubCircuit};
        let vk = std::sync::Arc::new(pk.get_vk());
        let sub = VerifierSubCircuit::<4>::new_with_options(
            vk,
            VerifierConfig { continuations_enabled: true, ..Default::default() },
        );
        let circuit = InnerCircuit::new(std::sync::Arc::new(sub), None);
        let airs = <InnerCircuit<VerifierSubCircuit<4>> as Circuit<SC>>::airs(&circuit);
        let leaf_engine = Engine::new(openvm_stark_sdk::config::leaf_params_with_100_bits_security());
        leaf_engine.keygen(&airs).0
    }));
    match leaf {
        Ok(leaf_pk) => {
            let base = pk.per_air.len();
            for id in 0..leaf_pk.per_air.len() {
                let name = leaf_pk.per_air[id].air_name.clone();
                let r = catch_unwind(AssertUnwindSafe(|| {
                    extract_as(&leaf_pk, id, base + id, "recursion-leaf", "leaf aggregation circuit (VerifierSubCircuit<4>)")
                }));
                let (status, detail) = match r {
                    Ok(Ok(x)) => {
                        write(&format!("{out}/airs/{}.json", base + id), &x.doc.to_json());
                        ("extracted", format!("{} constraints, {} interactions", x.doc.constraints.len(), x.doc.interactions.len()))
                    }
                    Ok(Err(e)) => ("failed", e),
                    Err(e) => ("failed", format!("panic: {}", panic_message(e.as_ref()))),
                };
                manifest.push_str(&manifest_line(base + id, &name, &name, "recursion-leaf", status, &detail));
                manifest.push('\n');
            }
            log.push_str(&format!("leaf aggregation circuit: {} AIRs (no rows: they need app proofs)\n", leaf_pk.per_air.len()));
        }
        Err(e) => log.push_str(&format!("leaf aggregation circuit: panic: {}\n", panic_message(e.as_ref()))),
    }
    write(&format!("{out}/manifest.jsonl"), &manifest);
    eprintln!("extracted {} AIRs of SdkVmConfig::standard()", pk.per_air.len());
    if rows {
        let mut sink = Sink { docs: HashMap::new(), all: ratchet_airs().map(|a| AllRows::new(&out, a)) };
        let g = format!("{repo}/benchmarks/guest");
        let toml = |name: &str| -> SdkVmConfig {
            let text = std::fs::read_to_string(format!("{g}/{name}/openvm.toml")).expect("openvm.toml");
            SdkVmConfig::from_toml(&text).expect("config")
        };
        let std_cfg = SdkVmConfig::standard();
        let elf = |cfg: &SdkVmConfig, path: String| elf_exe(cfg, &path);
        let runs: Vec<(&str, SdkVmConfig, Result<VmExe<F>, String>, Vec<Vec<F>>, usize)> = vec![
            ("standard-straight-line", std_cfg.clone(), program::standard_program(&std_cfg), vec![], 4),
            ("fibonacci", std_cfg.clone(), elf(&std_cfg, format!("{g}/fibonacci/elf/openvm-fibonacci-program.elf")),
             vec![stream_u64(2000)], 4),
            ("sha2_bench", std_cfg.clone(), elf(&std_cfg, format!("{g}/sha2_bench/elf/openvm-sha2-bench-program.elf")),
             vec![stream_u32(4096)], 4),
            ("keccak256", std_cfg.clone(), elf(&std_cfg, format!("{g}/keccak256/elf/openvm-keccak256-program.elf")),
             vec![], 4),
            ("ecrecover", toml("ecrecover"),
             elf(&toml("ecrecover"), format!("{g}/ecrecover/elf/openvm-ecdsa-recover-key-program.elf")),
             ecrecover_input(), 4),
            ("pairing", toml("pairing"), elf(&toml("pairing"), format!("{g}/pairing/elf/openvm-pairing-program.elf")),
             vec![], 4),
            ("kitchen-sink", toml("kitchen-sink"),
             elf(&toml("kitchen-sink"), format!("{g}/kitchen-sink/elf/openvm-kitchen-sink-program.elf")), vec![], 2),
        ];
        for (label, cfg, exe, input, max_seg) in runs {
            if seed_mix() != 0 && label != "standard-straight-line" {
                continue; // the committed ELFs do not depend on the seed
            }
            let r = catch_unwind(AssertUnwindSafe(|| run(label, cfg, exe, input, &keys, &mut sink, &mut log, max_seg)));
            match r {
                Ok(Ok(())) => {}
                Ok(Err(e)) => log.push_str(&format!("{label}: error: {e}\n")),
                Err(e) => log.push_str(&format!("{label}: panic: {}\n", panic_message(e.as_ref()))),
            }
            eprint!("{log}");
        }
        for (i, d) in sink.docs.iter() {
            write(&format!("{out}/rows/{i}.json"), &d.to_json());
        }
        log.push_str(&format!("rows for {} of {} AIRs\n", sink.docs.len(), pk.per_air.len()));
    }
    write(&format!("{out}/extract.log"), &log);
    eprint!("{log}");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = args.get(1).cloned().expect("usage: boole-air-extract <out-dir> <openvm-repo> [--no-rows]");
    let repo = args.get(2).cloned().expect("usage: boole-air-extract <out-dir> <openvm-repo> [--no-rows]");
    let rows = !args.iter().any(|a| a == "--no-rows");
    std::thread::Builder::new().stack_size(1 << 30).spawn(move || main_inner(out, repo, rows)).unwrap().join().unwrap();
}

// ------------------------------------------------------------------------------------------ straight-line program

/// A hand-assembled program for the standard configuration: rv32 division, 256-bit shifts and branches, every
/// modular and Fp2 operation of every configured modulus, and elliptic-curve addition / doubling on every
/// configured curve (operands on the heap, address space 2; pointers in registers x5, x6, x7 advanced after
/// each call).  Instruction encodings follow the transpilers (`from_r_type`, `from_i_type`, the bigint branch).
mod program {
    use super::*;
    use num_bigint::BigUint;
    use openvm_algebra_transpiler::{Fp2Opcode, Rv32ModularArithmeticOpcode};
    use openvm_bigint_transpiler::{Rv32BranchLessThan256Opcode, Rv32Shift256Opcode};
    use openvm_ecc_transpiler::Rv32WeierstrassOpcode;
    use openvm_instructions::{instruction::Instruction, program::Program, LocalOpcode, SystemOpcode, VmOpcode};
    use openvm_rv32im_transpiler::{BaseAluOpcode, BranchLessThanOpcode, DivRemOpcode, ShiftOpcode};
    use std::collections::BTreeMap;
    use strum::EnumCount;

    const A0: u32 = 0x0010_0000;
    const B0: u32 = 0x0020_0000;
    const D0: u32 = 0x0030_0000;
    const STRIDE: u32 = 128;
    const N: usize = 12;

    struct Asm {
        ins: Vec<Instruction<F>>,
        mem: BTreeMap<(u32, u32), u8>,
        slot: u32,
        rng: Rng,
    }

    impl Asm {
        fn reg(&mut self, r: u32, v: u32) {
            for k in 0..4 {
                self.mem.insert((1, 4 * r + k), (v >> (8 * k)) as u8);
            }
        }
        fn heap(&mut self, addr: u32, bytes: &[u8]) {
            for (k, b) in bytes.iter().enumerate() {
                self.mem.insert((2, addr + k as u32), *b);
            }
        }
        fn addi(&mut self, rd: usize, rs1: usize, imm: usize) {
            self.ins.push(Instruction::from_usize(BaseAluOpcode::ADD.global_opcode(), [4 * rd, 4 * rs1, imm, 1, 0]));
        }
        /// Writes the operands of the next slot, runs `op` on (x7 <- x5, x6) and advances the pointers.
        fn call(&mut self, op: Instruction<F>, a: &[u8], b: &[u8]) {
            let s = self.slot;
            self.heap(A0 + STRIDE * s, a);
            self.heap(B0 + STRIDE * s, b);
            self.ins.push(op);
            for r in [5, 6, 7] {
                self.addi(r, r, STRIDE as usize);
            }
            self.slot += 1;
        }
        fn rand_below(&mut self, m: &BigUint) -> BigUint {
            let bytes: Vec<u8> = (0..(m.bits() as usize / 8 + 8)).map(|_| self.rng.next() as u8).collect();
            BigUint::from_bytes_le(&bytes) % m
        }
    }

    fn le(x: &BigUint, n: usize) -> Vec<u8> {
        let mut b = x.to_bytes_le();
        b.resize(n, 0);
        b
    }

    fn r_type(opcode: VmOpcode, rd: usize, rs1: usize, rs2: usize, e: usize) -> Instruction<F> {
        Instruction::from_usize(opcode, [4 * rd, 4 * rs1, 4 * rs2, 1, e])
    }

    fn shifted(base: VmOpcode, idx: usize, count: usize) -> VmOpcode {
        VmOpcode::from_usize(base.as_usize() + idx * count)
    }

    fn inv(x: &BigUint, p: &BigUint) -> BigUint {
        x.modpow(&(p - 2u32), p)
    }

    /// A point of y^2 = x^3 + a x + b over F_p with p = 3 mod 4, then multiples (affine arithmetic).
    fn points(p: &BigUint, a: &BigUint, b: &BigUint, n: usize) -> Vec<(BigUint, BigUint)> {
        let mut x = BigUint::from(1u32);
        let e = (p + 1u32) >> 2;
        let g = loop {
            let rhs = (&x * &x * &x + a * &x + b) % p;
            let y = rhs.modpow(&e, p);
            if (&y * &y) % p == rhs && y != BigUint::from(0u32) {
                break (x.clone(), y);
            }
            x += 1u32;
        };
        let dbl = |(x1, y1): &(BigUint, BigUint)| {
            let l = ((BigUint::from(3u32) * x1 * x1 + a) % p) * inv(&((BigUint::from(2u32) * y1) % p), p) % p;
            let x3 = (&l * &l + p + p - x1 - x1) % p;
            let y3 = (&l * ((x1 + p - &x3) % p) + p - y1) % p;
            (x3, y3)
        };
        let add = |(x1, y1): &(BigUint, BigUint), (x2, y2): &(BigUint, BigUint)| {
            let l = ((y2 + p - y1) % p) * inv(&((x2 + p - x1) % p), p) % p;
            let x3 = (&l * &l + p + p - x1 - x2) % p;
            let y3 = (&l * ((x1 + p - &x3) % p) + p - y1) % p;
            (x3, y3)
        };
        let mut out = vec![g.clone(), dbl(&g)];
        while out.len() < n {
            let next = add(out.last().unwrap(), &g);
            out.push(next);
        }
        out
    }

    pub fn standard_program(config: &SdkVmConfig) -> Result<VmExe<F>, String> {
        let mut m = Asm { ins: vec![], mem: BTreeMap::new(), slot: 0, rng: Rng(0x0e0e_5eed ^ seed_mix()) };
        m.reg(5, A0);
        m.reg(6, B0);
        m.reg(7, D0);
        // rv32 division: registers x10..x29 hold edge and random words
        let edge = [0u32, 1, 2, u32::MAX, 0x8000_0000, 0x7FFF_FFFF, 3, 0xFFFF_FFFE];
        for r in 10..30u32 {
            let v = if (r as usize - 10) < edge.len() { edge[r as usize - 10] } else { m.rng.next() as u32 };
            m.reg(r, v);
        }
        for (k, op) in [DivRemOpcode::DIV, DivRemOpcode::DIVU, DivRemOpcode::REM, DivRemOpcode::REMU].iter().enumerate() {
            for j in 0..N {
                let rs1 = 10 + (j + k) % 20;
                let rs2 = 10 + (3 * j + 2 * k + 1) % 20;
                m.ins.push(r_type(op.global_opcode(), 30, rs1, rs2, 1));
            }
        }
        // 256-bit shifts and branches
        for sh in [ShiftOpcode::SLL, ShiftOpcode::SRL, ShiftOpcode::SRA] {
            for _ in 0..N {
                let a: Vec<u8> = (0..32).map(|_| m.rng.next() as u8).collect();
                let mut b = vec![0u8; 32];
                b[0] = m.rng.next() as u8;
                m.call(r_type(Rv32Shift256Opcode(sh).global_opcode(), 7, 5, 6, 2), &a, &b);
            }
        }
        for br in [BranchLessThanOpcode::BLT, BranchLessThanOpcode::BLTU, BranchLessThanOpcode::BGE, BranchLessThanOpcode::BGEU] {
            for j in 0..N {
                let a: Vec<u8> = (0..32).map(|_| m.rng.next() as u8).collect();
                let b: Vec<u8> = if j % 4 == 0 { a.clone() } else { (0..32).map(|_| m.rng.next() as u8).collect() };
                // offset 4: taken and not taken both continue with the next instruction
                let op = Instruction::from_usize(Rv32BranchLessThan256Opcode(br).global_opcode(), [4 * 5, 4 * 6, 4, 1, 2]);
                m.call(op, &a, &b);
            }
        }
        // modular arithmetic, every modulus
        if let Some(modular) = &config.modular {
            for (idx, p) in modular.supported_moduli.iter().enumerate() {
                let nb = if p.bits() > 256 { 48 } else { 32 };
                let cnt = Rv32ModularArithmeticOpcode::COUNT;
                for op in [Rv32ModularArithmeticOpcode::ADD, Rv32ModularArithmeticOpcode::SUB,
                           Rv32ModularArithmeticOpcode::MUL, Rv32ModularArithmeticOpcode::DIV,
                           Rv32ModularArithmeticOpcode::IS_EQ] {
                    for j in 0..N {
                        let x = m.rand_below(p);
                        let mut y = m.rand_below(p);
                        if op == Rv32ModularArithmeticOpcode::DIV && y == BigUint::from(0u32) {
                            y = BigUint::from(1u32);
                        }
                        if op == Rv32ModularArithmeticOpcode::IS_EQ && j % 3 == 0 {
                            y = x.clone();
                        }
                        let code = shifted(op.global_opcode(), idx, cnt);
                        // IS_EQ writes a register (x8); the others write the heap through x7
                        let instr = if op == Rv32ModularArithmeticOpcode::IS_EQ {
                            r_type(code, 8, 5, 6, 2)
                        } else {
                            r_type(code, 7, 5, 6, 2)
                        };
                        m.call(instr, &le(&x, nb), &le(&y, nb));
                    }
                }
            }
        }
        if let Some(fp2) = &config.fp2 {
            for (idx, (_, p)) in fp2.supported_moduli.iter().enumerate() {
                let nb = if p.bits() > 256 { 48 } else { 32 };
                let cnt = Fp2Opcode::COUNT;
                for op in [Fp2Opcode::ADD, Fp2Opcode::SUB, Fp2Opcode::MUL, Fp2Opcode::DIV] {
                    for _ in 0..N {
                        let (x0, x1, y0, y1) = (m.rand_below(p), m.rand_below(p), m.rand_below(p), m.rand_below(p));
                        let y1 = if y0 == BigUint::from(0u32) && y1 == BigUint::from(0u32) { BigUint::from(1u32) } else { y1 };
                        let mut a = le(&x0, nb);
                        a.extend(le(&x1, nb));
                        let mut b = le(&y0, nb);
                        b.extend(le(&y1, nb));
                        m.call(r_type(shifted(op.global_opcode(), idx, cnt), 7, 5, 6, 2), &a, &b);
                    }
                }
            }
        }
        if let Some(ecc) = &config.ecc {
            for (idx, c) in ecc.supported_curves.iter().enumerate() {
                let nb = if c.modulus.bits() > 256 { 48 } else { 32 };
                let pts = points(&c.modulus, &c.a, &c.b, N + 2);
                let cnt = Rv32WeierstrassOpcode::COUNT;
                for j in 0..N {
                    let (p1, p2) = (&pts[j + 1], &pts[0]);
                    let mut a = le(&p1.0, nb);
                    a.extend(le(&p1.1, nb));
                    let mut b = le(&p2.0, nb);
                    b.extend(le(&p2.1, nb));
                    m.call(r_type(shifted(Rv32WeierstrassOpcode::EC_ADD_NE.global_opcode(), idx, cnt), 7, 5, 6, 2), &a, &b);
                    let p3 = &pts[j];
                    let mut d = le(&p3.0, nb);
                    d.extend(le(&p3.1, nb));
                    m.call(r_type(shifted(Rv32WeierstrassOpcode::EC_DOUBLE.global_opcode(), idx, cnt), 7, 5, 6, 2), &d, &[]);
                }
            }
        }
        // SHA-256 / SHA-512 compression: rd = output, rs1 = previous state, rs2 = message block
        if config.sha2.is_some() {
            for (op, state_len, block_len) in [(openvm_sha2_transpiler::Rv32Sha2Opcode::SHA256, 32usize, 64usize),
                                               (openvm_sha2_transpiler::Rv32Sha2Opcode::SHA512, 64, 128)] {
                for _ in 0..N {
                    let st: Vec<u8> = (0..state_len).map(|_| m.rng.next() as u8).collect();
                    let blk: Vec<u8> = (0..block_len).map(|_| m.rng.next() as u8).collect();
                    m.call(r_type(op.global_opcode(), 7, 5, 6, 2), &st, &blk);
                }
            }
        }
        m.ins.push(Instruction::from_isize(SystemOpcode::TERMINATE.global_opcode(), 0, 0, 0, 0, 0));
        let mut exe = VmExe::new(Program::from_instructions(&m.ins));
        exe.init_memory = m.mem;
        Ok(exe)
    }
}
