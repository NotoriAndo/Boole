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
            group: "app-vm".into(),
            index: id,
            width,
            prep_width,
            num_public_values: vk.params.num_public_values,
            dag: out,
            constraints,
            interactions,
            meta: vec![
                ("cached_main_widths".into(), format!("{:?}", w.cached_mains)),
                ("common_main_width".into(), w.common_main.to_string()),
                ("config".into(), "SdkVmConfig::standard()".into()),
            ],
        },
        key,
    })
}

// ------------------------------------------------------------------------------------------ rows

struct Sink {
    docs: HashMap<usize, RowsDoc>,
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

#[allow(clippy::too_many_arguments)]
fn run(
    label: &str,
    config: SdkVmConfig,
    elf_path: &str,
    input: Vec<Vec<F>>,
    standard_keys: &HashMap<u64, Vec<usize>>,
    sink: &mut Sink,
    log: &mut String,
    max_segments: usize,
) -> Result<(), String> {
    let bytes = std::fs::read(elf_path).map_err(|e| format!("read {elf_path}: {e}"))?;
    let elf = Elf::decode(&bytes, MEM_SIZE as u32).map_err(|e| format!("decode: {e}"))?;
    let exe = VmExe::from_elf(elf, config.transpiler()).map_err(|e| format!("transpile: {e:?}"))?;
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
            let mut parts: Vec<&RowMajorMatrix<F>> = actx.cached_mains.iter().map(|c| &c.trace).collect();
            parts.push(&actx.common_main);
            let height = actx.common_main.height();
            if height == 0 {
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
    write(&format!("{out}/manifest.jsonl"), &manifest);
    eprintln!("extracted {} AIRs of SdkVmConfig::standard()", pk.per_air.len());
    if rows {
        let mut sink = Sink { docs: HashMap::new() };
        let g = format!("{repo}/benchmarks/guest");
        let toml = |name: &str| -> SdkVmConfig {
            let text = std::fs::read_to_string(format!("{g}/{name}/openvm.toml")).expect("openvm.toml");
            SdkVmConfig::from_toml(&text).expect("config")
        };
        let runs: Vec<(&str, SdkVmConfig, String, Vec<Vec<F>>, usize)> = vec![
            ("fibonacci", SdkVmConfig::standard(), format!("{g}/fibonacci/elf/openvm-fibonacci-program.elf"),
             vec![stream_u64(2000)], 4),
            ("sha2_bench", SdkVmConfig::standard(), format!("{g}/sha2_bench/elf/openvm-sha2-bench-program.elf"),
             vec![stream_u32(4096)], 4),
            ("keccak256", SdkVmConfig::standard(), format!("{g}/keccak256/elf/openvm-keccak256-program.elf"), vec![], 4),
            ("ecrecover", toml("ecrecover"), format!("{g}/ecrecover/elf/openvm-ecdsa-recover-key-program.elf"),
             ecrecover_input(), 4),
            ("pairing", toml("pairing"), format!("{g}/pairing/elf/openvm-pairing-program.elf"), vec![], 4),
            ("kitchen-sink", toml("kitchen-sink"), format!("{g}/kitchen-sink/elf/openvm-kitchen-sink-program.elf"),
             vec![], 2),
        ];
        for (label, cfg, elf, input, max_seg) in runs {
            let r = catch_unwind(AssertUnwindSafe(|| run(label, cfg, &elf, input, &keys, &mut sink, &mut log, max_seg)));
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
