#![allow(dead_code)]
use anyhow::{anyhow, Result};
use boole_derivation::{m2::*, m3::ForkTable, InboxLog, L1Block};
use boole_gate_supervisor::rebuild::History;
use std::collections::BTreeMap;
#[path = "../../../boole-derivation/tests/support/m3.rs"]
pub mod scenario;
#[derive(Clone)]
pub struct Archive {
    pub rows: BTreeMap<u64, (L1Block, Vec<InboxLog>)>,
    pub reads: Vec<u64>,
}
impl History for Archive {
    fn origin(&mut self, h: u64) -> Result<(L1Block, Vec<InboxLog>)> {
        self.reads.push(h);
        self.rows
            .get(&h)
            .cloned()
            .ok_or_else(|| anyhow!("missing L1 bytes"))
    }
}
pub fn fixture() -> (Config, State, Key, Archive) {
    fixture_with(scenario::m3_cfg(), None)
}
pub fn fixture_with(c: Config, tag: Option<u8>) -> (Config, State, Key, Archive) {
    fixture_with_fuel(c, tag, 777)
}
pub fn fixture_with_fuel(c: Config, tag: Option<u8>, fuel: u64) -> (Config, State, Key, Archive) {
    fixture_with_open_packages(c, tag, fuel, &[])
}
pub fn fixture_with_open_packages(
    c: Config,
    tag: Option<u8>,
    fuel: u64,
    pkgs: &[Hash],
) -> (Config, State, Key, Archive) {
    fixture_with_content(c, tag, fuel, pkgs, None, None)
}
pub fn fixture_with_content(
    c: Config,
    tag: Option<u8>,
    fuel: u64,
    pkgs: &[Hash],
    real_package: Option<Vec<u8>>,
    real_raw: Option<Vec<u8>>,
) -> (Config, State, Key, Archive) {
    fixture_with_layout(c, tag, fuel, pkgs, real_package, real_raw, &[], &[], None)
}
/// Independent OPEN/REVEAL encoder; the public fold decides which wire inputs
/// are accepted before real reconstruction sees them.
#[allow(clippy::too_many_arguments)]
pub fn fixture_with_layout(
    c: Config,
    tag: Option<u8>,
    fuel: u64,
    pkgs: &[Hash],
    real_package: Option<Vec<u8>>,
    real_raw: Option<Vec<u8>>,
    model: &[u8],
    required: &[Hash],
    body_override: Option<&[u8]>,
) -> (Config, State, Key, Archive) {
    use scenario::*;
    let kind = if real_package.is_some() { 1 } else { 2 };
    let package = real_package.unwrap_or_else(|| {
        if tag.is_some() {
            boole_gate_supervisor::stub::package(10000)
        } else {
            b"generator-v1".to_vec()
        }
    });
    let root = pool_leaf(c.chain_id, kind, required, &package, &[9; 32]);
    let mut reveal = problem_body(0, 1, 0, required, [9; 32], &[], &package);
    reveal[10] = kind;
    let mut prepared = State::new(&c, 30_000_000).unwrap();
    run(
        &mut prepared,
        &c,
        vec![input(8, pool(&c, root, 1, 3, 1), 0)],
    );
    until(&mut prepared, &c, 4);
    run(&mut prepared, &c, vec![input(9, reveal.clone(), 0)]);
    let p = *prepared.problems.keys().next().unwrap();
    let raw = real_raw.unwrap_or_else(|| {
        tag.map(|t| boole_gate_supervisor::stub::export(t, fuel))
            .unwrap_or_else(|| vec![42; 8])
    });
    let transport = if tag.is_some() {
        boole_gate_supervisor::raw_frame::encode(&raw).unwrap()
    } else {
        raw.clone()
    };
    let (mut ab, _, _) = body(&c, p, [6; 20], [7; 32], &transport, pkgs);
    let q = 151 + 32 * pkgs.len();
    ab[q + 2..q + 10].copy_from_slice(&(raw.len() as u64).to_be_bytes());
    ab[q + 10..q + 42].copy_from_slice(&hash(&raw));
    ab[q + 1] = c.slot_table[0].lane;
    ab[q + 84..q + 86].copy_from_slice(&((40 + model.len()) as u16).to_be_bytes());
    ab[q + 87] = model.len() as u8;
    ab.splice(q + 88..q + 88, model.iter().copied());
    if let Some(body) = body_override {
        ab = body.to_vec();
    }
    let ch = commit_hash(c.chain_id, p, cancel_hash(&[8; 32]), body_digest(&ab));
    let mut s = State::new(&c, 30_000_000).unwrap();
    let mut rows = BTreeMap::new();
    for h in 1..=40 {
        let good = part(ch, 0, &[], &transport);
        let mut garbage = good.clone();
        *garbage.last_mut().unwrap() ^= 1;
        let items = match h {
            1 => vec![input(8, pool(&c, root, 1, 3, 1), 0)],
            5 => vec![input(9, reveal.clone(), 0)],
            6 => vec![input(2, commit(p, ch), 0)],
            15 => vec![input(4, good.clone(), 0)],
            16 => vec![input(3, open(&ab, cancel_hash(&[8; 32])), 0)],
            17 => vec![input(4, garbage, 0), input(4, good, 1)],
            _ => vec![],
        };
        let block = block(h);
        let logs = items
            .iter()
            .map(|i| {
                let raw = [vec![i.kind], i.body.clone()].concat();
                let mut abi = vec![0; 64];
                abi[31] = 32;
                abi[56..64].copy_from_slice(&(raw.len() as u64).to_be_bytes());
                abi.extend(raw);
                InboxLog {
                    address: boole_derivation::config::INBOX.into(),
                    topics: vec![boole_derivation::config::posted_topic()],
                    block_hash: block.hash.clone(),
                    removed: false,
                    transaction_index: i.transaction_index,
                    log_index: i.log_index,
                    data: boole_derivation::attributes::hex_string(&abi),
                }
            })
            .collect();
        if h <= 17 {
            s = s
                .step_with_verdicts(&c, &block, &items, &[], &ForkTable::genesis(&c).unwrap())
                .unwrap()
                .state;
        }
        rows.insert(h, (block, logs));
    }
    if body_override.is_none() {
        assert_eq!(s.candidates[&(p, ch)].status(), Status::Checking);
    }
    (
        c,
        s,
        (p, ch),
        Archive {
            rows,
            reads: vec![],
        },
    )
}
impl boole_devnet_driver::l1::L1Source for Archive {
    fn finalized(&mut self) -> Result<u64> {
        Ok(*self.rows.keys().next_back().unwrap())
    }
    fn block(&mut self, h: u64) -> Result<L1Block> {
        self.rows
            .get(&h)
            .map(|r| r.0.clone())
            .ok_or_else(|| anyhow!("missing height"))
    }
    fn block_by_hash(&mut self, hash: &str) -> Result<L1Block> {
        self.rows
            .values()
            .find(|r| r.0.hash == hash)
            .map(|r| r.0.clone())
            .ok_or_else(|| anyhow!("missing anchored block"))
    }
    fn inbox_logs(&mut self, hash: &str) -> Result<Vec<InboxLog>> {
        self.rows
            .values()
            .find(|r| r.0.hash == hash)
            .map(|r| r.1.clone())
            .ok_or_else(|| anyhow!("missing anchored logs"))
    }
}
