//! boole-acir-tool: decode and execute compiled Noir artifacts with the `acir` / `acvm` crates of the
//! compiler version that produced them.  It is built inside a checkout of that noir version (see
//! `zk_registry.noir_toolchain.build_acir_tool`), so the serialization format and the solver are the
//! ones `nargo` itself uses.  No proof is written; the tool only reads circuits and computes witnesses.
//!
//! Commands (all output is JSON on stdout or in the named file):
//!
//! * `decode <artifact.json> [--contract-fn NAME]`
//!       the ACIR functions (serde JSON of `Circuit`), the ABI and Brillig statistics;
//! * `execute <artifact.json> <inputs.jsonl> <out.jsonl> [--contract-fn NAME]`
//!       one execution per input line (ABI inputs as JSON, or `{"__prover_toml__": path}`), with the ACVM solver loop of
//!       `nargo::ops::execute_program` (Brillig calls, ACIR calls, foreign calls); foreign calls are
//!       answered with zero values of the shapes declared by the Brillig `ForeignCall` opcode, or with
//!       pseudo-random values within the declared bit sizes (`--oracle-seed N`, N > 0); `print` gets an
//!       empty answer.  Oracle answers are unconstrained prover hints, so any answer is a legitimate
//!       execution;
//! * `bbeval <artifact.json> <requests.jsonl> <out.jsonl> [--contract-fn NAME]`
//!       re-solve single opcodes (black-box calls) on given input values with the ACVM solver;
//! * `witness <witness.gz>`
//!       decode a witness stack written by `nargo execute`.
#![allow(clippy::all)]

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{BufRead, BufReader, Write};

use acvm::acir::brillig::{ForeignCallParam, ForeignCallResult};
use acvm::acir::circuit::{Circuit, Program};
use acvm::acir::native_types::{Witness, WitnessMap, WitnessStack};
use acvm::pwg::{ACVMStatus, ACVM};
use acvm::{AcirField, FieldElement};
use base64::Engine;
use bn254_blackbox_solver::Bn254BlackBoxSolver;
use noirc_abi::input_parser::Format;
use noirc_abi::Abi;
use serde_json::{json, Value};

type F = FieldElement;

fn die(msg: String) -> ! {
    eprintln!("boole-acir-tool: {msg}");
    std::process::exit(2);
}

fn solver() -> Bn254BlackBoxSolver {
    #[cfg(feature = "bn_bool")]
    {
        Bn254BlackBoxSolver(false)
    }
    #[cfg(not(feature = "bn_bool"))]
    {
        Bn254BlackBoxSolver
    }
}

fn deserialize_stack(bytes: &[u8]) -> WitnessStack<F> {
    #[cfg(feature = "ws_tryfrom")]
    {
        WitnessStack::<F>::try_from(bytes).unwrap_or_else(|e| die(format!("witness stack: {e:?}")))
    }
    #[cfg(not(feature = "ws_tryfrom"))]
    {
        WitnessStack::<F>::deserialize(bytes).unwrap_or_else(|e| die(format!("witness stack: {e:?}")))
    }
}

struct Artifact {
    noir_version: Value,
    abi_json: Value,
    abi: Abi,
    program: Program<F>,
}

fn load_artifact(path: &str, contract_fn: Option<&str>) -> Artifact {
    let text = fs::read_to_string(path).unwrap_or_else(|e| die(format!("{path}: {e}")));
    let v: Value = serde_json::from_str(&text).unwrap_or_else(|e| die(format!("{path}: {e}")));
    let (abi_json, bytecode) = match contract_fn {
        Some(name) => {
            let f = v["functions"]
                .as_array()
                .and_then(|fs| fs.iter().find(|f| f["name"] == name))
                .unwrap_or_else(|| die(format!("contract function {name} not found")));
            (f["abi"].clone(), f["bytecode"].clone())
        }
        None => (v["abi"].clone(), v["bytecode"].clone()),
    };
    let abi: Abi = serde_json::from_value(abi_json.clone()).unwrap_or_else(|e| die(format!("abi: {e}")));
    let b64 = bytecode.as_str().unwrap_or_else(|| die("bytecode is not a string".into()));
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).unwrap_or_else(|e| die(format!("base64: {e}")));
    let program = Program::<F>::deserialize_program(&bytes).unwrap_or_else(|e| die(format!("program: {e}")));
    Artifact { noir_version: v["noir_version"].clone(), abi_json, abi, program }
}

fn hex(f: &F) -> String {
    f.to_hex()
}

fn map_json(m: &WitnessMap<F>) -> Value {
    let mut out = serde_json::Map::new();
    for (w, val) in m.clone().into_iter() {
        out.insert(w.witness_index().to_string(), Value::String(hex(&val)));
    }
    Value::Object(out)
}

fn parse_map(v: &Value) -> WitnessMap<F> {
    let mut m = BTreeMap::new();
    if let Some(obj) = v.as_object() {
        for (k, val) in obj {
            let idx: u32 = k.parse().unwrap_or_else(|_| die(format!("bad witness index {k}")));
            let s = val.as_str().unwrap_or_else(|| die("witness value must be a hex string".into()));
            let f = F::from_hex(s).unwrap_or_else(|| die(format!("bad hex {s}")));
            m.insert(Witness(idx), f);
        }
    }
    WitnessMap::from(m)
}

// ------------------------------------------------------------------------------------------ foreign calls

fn collect_foreign_shapes(v: &Value, out: &mut HashMap<String, Vec<Value>>) {
    match v {
        Value::Object(o) => {
            if let Some(fc) = o.get("ForeignCall") {
                if let Some(name) = fc["function"].as_str() {
                    let types = fc["destination_value_types"].as_array().cloned().unwrap_or_default();
                    out.entry(name.to_string()).or_insert(types);
                }
            }
            for (_, x) in o {
                collect_foreign_shapes(x, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| collect_foreign_shapes(x, out)),
        _ => {}
    }
}

/// Bit size of a Brillig simple value type (`Field` -> 253, `{"Integer": "U32"}` -> 32, a number -> itself).
fn bit_size(t: &Value) -> u32 {
    let s = t.get("Simple").unwrap_or(t);
    if let Some(n) = s.as_u64() {
        return n as u32;
    }
    if let Some(txt) = s.as_str() {
        return if txt == "Field" { 253 } else { txt.trim_start_matches('U').parse().unwrap_or(253) };
    }
    if let Some(i) = s.get("Integer") {
        if let Some(txt) = i.as_str() {
            return txt.trim_start_matches('U').parse().unwrap_or(64);
        }
        if let Some(n) = i.as_u64() {
            return n as u32;
        }
    }
    253
}

/// Oracle answers: zero values (seed 0) or pseudo-random values within each declared bit size (seed > 0).
struct Answers {
    seed: u64,
    state: u64,
}

impl Answers {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.state ^= self.state >> 12;
        self.state ^= self.state << 25;
        self.state ^= self.state >> 27;
        self.state.wrapping_mul(0x2545F4914F6CDD1D)
    }

    fn value(&mut self, bits: u32) -> F {
        if self.seed == 0 {
            return F::zero();
        }
        let b = bits.min(120);
        let v = (self.next() as u128) << 64 | self.next() as u128;
        let v = if b >= 128 { v } else { v & ((1u128 << b) - 1) };
        F::from(v)
    }

    fn fill(&mut self, t: &Value, out: &mut Vec<F>) {
        if let Some(a) = t.get("Array") {
            let size = a["size"].as_u64().unwrap_or(0);
            let inner = a["value_types"].as_array().cloned().unwrap_or_default();
            for _ in 0..size {
                for it in &inner {
                    self.fill(it, out);
                }
            }
        } else if t.get("Vector").is_some() {
        } else {
            out.push(self.value(bit_size(t)));
        }
    }

    fn answer(&mut self, name: &str, shapes: &HashMap<String, Vec<Value>>) -> ForeignCallResult<F> {
        if name == "print" {
            return ForeignCallResult { values: vec![] };
        }
        let types = shapes.get(name).cloned().unwrap_or_default();
        let mut values = Vec::new();
        for t in &types {
            if t.get("Array").is_some() || t.get("Vector").is_some() {
                let mut v = Vec::new();
                self.fill(t, &mut v);
                values.push(ForeignCallParam::Array(v));
            } else {
                values.push(ForeignCallParam::Single(self.value(bit_size(t))));
            }
        }
        ForeignCallResult { values }
    }
}

// ------------------------------------------------------------------------------------------ execution

struct Exec<'a> {
    program: &'a Program<F>,
    solver: &'a Bn254BlackBoxSolver,
    shapes: &'a HashMap<String, Vec<Value>>,
    calls: Vec<Value>,
    oracles: Vec<String>,
    answers: Answers,
}

impl<'a> Exec<'a> {
    fn run(&mut self, func: usize, init: WitnessMap<F>, path: Vec<Value>) -> Result<WitnessMap<F>, String> {
        let program: &'a Program<F> = self.program;
        let circuit: &'a Circuit<F> = &program.functions[func];
        let mut acvm = ACVM::new(
            self.solver,
            &circuit.opcodes,
            init,
            &program.unconstrained_functions,
            &circuit.assert_messages,
        );
        let mut seq = 0usize;
        loop {
            match acvm.solve() {
                ACVMStatus::Solved => break,
                ACVMStatus::InProgress => return Err("solver stopped in progress".into()),
                ACVMStatus::Failure(e) => return Err(format!("{e}")),
                ACVMStatus::RequiresForeignCall(fc) => {
                    self.oracles.push(fc.function.clone());
                    let ans = self.answers.answer(&fc.function, self.shapes);
                    acvm.resolve_pending_foreign_call(ans);
                }
                ACVMStatus::RequiresAcirCall(info) => {
                    let callee = info.id.as_usize();
                    let ip = acvm.instruction_pointer();
                    let mut sub = path.clone();
                    sub.push(json!([func, ip, seq]));
                    seq += 1;
                    let solved = self.run(callee, info.initial_witness, sub.clone())?;
                    let mut outs = Vec::new();
                    for w in program.functions[callee].return_values.indices() {
                        match solved.get_index(w) {
                            Some(v) => outs.push(*v),
                            None => return Err(format!("missing return witness {w} of function {callee}")),
                        }
                    }
                    self.calls.push(json!({"path": sub, "callee": callee, "witness": map_json(&solved)}));
                    acvm.resolve_pending_acir_call(outs);
                }
            }
        }
        Ok(acvm.finalize())
    }
}

fn shapes_of(program: &Program<F>) -> HashMap<String, Vec<Value>> {
    let mut shapes = HashMap::new();
    let v = serde_json::to_value(&program.unconstrained_functions).unwrap_or(Value::Null);
    collect_foreign_shapes(&v, &mut shapes);
    shapes
}

fn cmd_execute(art: &Artifact, inputs: &str, out: &str, seed: u64) {
    let shapes = shapes_of(&art.program);
    let bb = solver();
    let rdr = BufReader::new(fs::File::open(inputs).unwrap_or_else(|e| die(format!("{inputs}: {e}"))));
    let mut w = fs::File::create(out).unwrap_or_else(|e| die(format!("{out}: {e}")));
    for line in rdr.lines() {
        let line = line.unwrap_or_else(|e| die(format!("{e}")));
        if line.trim().is_empty() {
            continue;
        }
        let res = (|| -> Result<Value, String> {
            // a line {"__prover_toml__": "<path>"} reads the inputs from a Prover.toml (repository inputs)
            let v: Value = serde_json::from_str(&line).map_err(|e| format!("input line: {e}"))?;
            let input_map = match v.get("__prover_toml__").and_then(|p| p.as_str()) {
                Some(p) => {
                    let t = fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))?;
                    Format::Toml.parse(&t, &art.abi).map_err(|e| format!("input: {e}"))?
                }
                None => Format::Json.parse(&line, &art.abi).map_err(|e| format!("input: {e}"))?,
            };
            let init = art.abi.encode(&input_map, None).map_err(|e| format!("encode: {e}"))?;
            let answers = Answers { seed, state: seed.wrapping_mul(0x9E3779B97F4A7C15) | 1 };
            let mut ex = Exec { program: &art.program, solver: &bb, shapes: &shapes, calls: vec![], oracles: vec![], answers };
            let main = ex.run(0, init, vec![])?;
            Ok(json!({"ok": true, "witness": map_json(&main), "calls": ex.calls, "oracles": ex.oracles}))
        })();
        let rec = res.unwrap_or_else(|e| json!({"ok": false, "error": e}));
        writeln!(w, "{}", rec).unwrap();
    }
}

fn cmd_bbeval(art: &Artifact, requests: &str, out: &str) {
    let rdr = BufReader::new(fs::File::open(requests).unwrap_or_else(|e| die(format!("{requests}: {e}"))));
    let mut w = fs::File::create(out).unwrap_or_else(|e| die(format!("{out}: {e}")));
    let solver = solver();
    for line in rdr.lines() {
        let line = line.unwrap_or_else(|e| die(format!("{e}")));
        if line.trim().is_empty() {
            continue;
        }
        let req: Value = serde_json::from_str(&line).unwrap_or_else(|e| die(format!("request: {e}")));
        let fi = req["function"].as_u64().unwrap_or(0) as usize;
        let oi = req["opcode"].as_u64().unwrap_or(0) as usize;
        let circuit = &art.program.functions[fi];
        let opcodes = vec![circuit.opcodes[oi].clone()];
        let init = parse_map(&req["values"]);
        let mut acvm = ACVM::new(&solver, &opcodes, init, &art.program.unconstrained_functions, &circuit.assert_messages);
        let rec = match acvm.solve() {
            ACVMStatus::Solved => json!({"ok": true, "witness": map_json(&acvm.finalize())}),
            ACVMStatus::Failure(e) => json!({"ok": false, "error": format!("{e}")}),
            _ => json!({"ok": false, "error": "opcode needs a call"}),
        };
        writeln!(w, "{}", rec).unwrap();
    }
}

fn cmd_decode(art: &Artifact) {
    let functions = serde_json::to_value(&art.program.functions).unwrap_or_else(|e| die(format!("{e}")));
    let brillig: Vec<Value> = art
        .program
        .unconstrained_functions
        .iter()
        .map(|b| json!({"opcodes": b.bytecode.len()}))
        .collect();
    let shapes = shapes_of(&art.program);
    let mut oracle_names: Vec<&String> = shapes.keys().collect();
    oracle_names.sort();
    let out = json!({"noir_version": art.noir_version, "abi": art.abi_json, "functions": functions,
                     "unconstrained_functions": brillig, "oracles": oracle_names});
    println!("{}", out);
}

fn cmd_witness(path: &str) {
    let bytes = fs::read(path).unwrap_or_else(|e| die(format!("{path}: {e}")));
    let mut stack = deserialize_stack(&bytes);
    let mut items: Vec<Value> = Vec::new();
    while let Some(it) = stack.pop() {
        items.push(json!({"index": it.index, "witness": map_json(&it.witness)}));
    }
    items.reverse();
    println!("{}", json!({"stack": items}));
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let contract_fn = args.iter().position(|a| a == "--contract-fn").map(|i| args[i + 1].clone());
    let seed: u64 = args.iter().position(|a| a == "--oracle-seed").map(|i| args[i + 1].parse().unwrap_or(0)).unwrap_or(0);
    let pos: Vec<&String> = {
        let mut v = Vec::new();
        let mut skip = false;
        for a in &args[1..] {
            if skip {
                skip = false;
                continue;
            }
            if a == "--contract-fn" || a == "--oracle-seed" {
                skip = true;
                continue;
            }
            v.push(a);
        }
        v
    };
    match pos.first().map(|s| s.as_str()) {
        Some("decode") => cmd_decode(&load_artifact(pos[1], contract_fn.as_deref())),
        Some("execute") => cmd_execute(&load_artifact(pos[1], contract_fn.as_deref()), pos[2], pos[3], seed),
        Some("bbeval") => cmd_bbeval(&load_artifact(pos[1], contract_fn.as_deref()), pos[2], pos[3]),
        Some("witness") => cmd_witness(pos[1]),
        _ => die("usage: decode|execute|bbeval|witness ...".into()),
    }
}
