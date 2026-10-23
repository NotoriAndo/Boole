//! Commit/reveal consensus fold. Inputs are finalized headers and decoded inbox bytes only.
use crate::L1Block;
use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub type Hash = [u8; 32];
pub type Key = (Hash, Hash);
pub const PART_SIZE: usize = 122_880;
pub const MAX_PARTS: u16 = 128;
pub const Z_CAP: usize = PART_SIZE * MAX_PARTS as usize;
pub const RAW_CAP: u64 = 134_217_728;

/// Untrusted configuration values. `State::new` validates and pins them before folding.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub chain_id: u64,
    pub d: u64,
    pub w: u64,
    pub w_slack_min: u64,
    pub p_blk: u64,
    pub ratio_max: u64,
    pub k_max: usize,
    pub c_commit: u64,
    pub q_max: u64,
    pub v_per_deposit: u64,
    pub verdict_gas: u64,
    pub l1_gas_limit: u64,
    pub pool_limit: usize,
    pub slot_table: Vec<SlotParams>,
    pub statement_raw_max: usize,
    pub slot_leaves_max: u32,
    pub pool_lead: u64,
    pub pool_max: u32,
    pub statement_max: usize,
    pub trace_embargo_max: u32,
    pub schema_id: Hash,
    pub base_root: Hash,
    pub pkg_table: Vec<Hash>,
    pub env_allowed: BTreeSet<Vec<Hash>>,
    pub incompatible: BTreeSet<(Hash, Hash)>,
    pub terms_table: BTreeMap<u8, Hash>,
    pub pool_key: [u8; 20],
    /// Omitted entirely for the frozen v1.2 profile, including its digest bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub m3: Option<crate::m3::M3Config>,
}
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlotParams {
    pub lane: u8,
    pub solve_deadline: u64,
    pub announce_gap: u64,
    pub reveal_grace: u64,
}
impl Config {
    pub fn slot_table_digest(&self) -> Hash {
        let mut bytes = vec![self.slot_table.len() as u8];
        for s in &self.slot_table {
            bytes.push(s.lane);
            bytes.extend(s.solve_deadline.to_be_bytes());
            bytes.extend(s.announce_gap.to_be_bytes());
            bytes.extend(s.reveal_grace.to_be_bytes());
        }
        sha(&[b"BOOLE-M2/SLOTS/v1", &bytes])
    }
    /// Normative compact JSON in declaration order; collections are ordered.
    pub fn digest(&self) -> Hash {
        sha(&[
            if self.m3.is_some() {
                b"BOOLE-M2/CONFIG/v2"
            } else {
                b"BOOLE-M2/CONFIG/v1"
            },
            &serde_json::to_vec(self).expect("integer config serializes"),
        ])
    }

    /// Explicit devnet identities; these do not attest to a kernel environment.
    pub fn devnet() -> Self {
        let mut pkg_table = vec![
            sha(&[b"BOOLE-DEVNET/EVM/v1"]),
            sha(&[b"BOOLE-DEVNET/RISCV/v1"]),
        ];
        pkg_table.sort();
        Self {
            chain_id: 720926,
            d: 9,
            w: 64,
            w_slack_min: 16,
            p_blk: 6,
            ratio_max: 16,
            k_max: 4,
            c_commit: 937,
            q_max: 8,
            v_per_deposit: 128,
            verdict_gas: 1_000_000,
            l1_gas_limit: 30_000_000,
            pool_limit: 131_072,
            slot_table: vec![
                SlotParams {
                    lane: 2,
                    solve_deadline: 48,
                    announce_gap: 4,
                    reveal_grace: 4,
                },
                SlotParams {
                    lane: 1,
                    solve_deadline: 96,
                    announce_gap: 8,
                    reveal_grace: 4,
                },
            ],
            statement_raw_max: 1_048_576,
            slot_leaves_max: 1_048_576,
            pool_lead: 2,
            pool_max: 65_536,
            statement_max: 122_880,
            trace_embargo_max: 2_628_000,
            schema_id: sha(&[b"EENV2-canonical-closure-v2"]),
            base_root: sha(&[b"BOOLE-DEVNET/BASE/v1"]),
            env_allowed: [vec![], vec![pkg_table[0]], vec![pkg_table[1]]].into(),
            incompatible: [(pkg_table[0], pkg_table[1])].into(),
            pkg_table,
            terms_table: [(
                1,
                sha(&[b"Option A placeholder: proofs CC0-1.0; granted trace CC-BY-4.0"]),
            )]
            .into(),
            pool_key: [
                0x1a, 0x64, 0x2f, 0x0e, 0x3c, 0x3a, 0xf5, 0x45, 0xe7, 0xac, 0xbd, 0x38, 0xb0, 0x72,
                0x51, 0xb3, 0x99, 0x09, 0x14, 0xf1,
            ],
            m3: None,
        }
    }
    pub fn lean_testnet(chain_id: u64) -> Self {
        let mut c = Self::devnet();
        c.chain_id = chain_id;
        c.d = 96;
        c.w = 300;
        c.w_slack_min = 64;
        c.c_commit = 1875;
        c.v_per_deposit = 256;
        c.l1_gas_limit = 60_000_000;
        c.slot_table = vec![
            SlotParams {
                lane: 2,
                solve_deadline: 1800,
                announce_gap: 300,
                reveal_grace: 25,
            },
            SlotParams {
                lane: 1,
                solve_deadline: 7200,
                announce_gap: 3600,
                reveal_grace: 25,
            },
            SlotParams {
                lane: 2,
                solve_deadline: 1800,
                announce_gap: 300,
                reveal_grace: 25,
            },
        ];
        c.pool_lead = 7200;
        c
    }
    pub fn production(chain_id: u64) -> Self {
        let mut c = Self::lean_testnet(chain_id);
        c.pool_lead = 50400;
        c
    }
    pub fn validate(&self, l2_gas_limit: u64) -> Result<(), u8> {
        if self.d < 2 {
            return Err(1);
        }
        let posting = 129u64
            .checked_add(self.p_blk.checked_sub(1).ok_or(2u8)?)
            .and_then(|x| x.checked_div(self.p_blk))
            .ok_or(2u8)?;
        if self
            .d
            .checked_add(posting)
            .and_then(|x| x.checked_add(self.w_slack_min))
            .is_none_or(|x| self.w < x)
        {
            return Err(2);
        }
        if self
            .q_max
            .checked_mul(self.v_per_deposit)
            .is_none_or(|x| self.c_commit > x)
        {
            return Err(3);
        }
        if self.m3.is_none()
            && self
                .q_max
                .checked_mul(self.verdict_gas)
                .and_then(|x| x.checked_add(crate::config::L1INFO_GAS))
                .and_then(|x| x.checked_add(crate::config::M2ROOT_GAS))
                .and_then(|x| x.checked_add(16_777_216))
                .is_none_or(|x| x > l2_gas_limit)
        {
            return Err(4);
        }
        let problem_size = self
            .pkg_table
            .len()
            .checked_mul(32)
            .and_then(|n| n.checked_add(1 + 54 + 32 * depth(self.pool_max)))
            .and_then(|n| n.checked_add(self.statement_max))
            .and_then(|n| n.checked_add(31))
            .map(|n| n / 32 * 32)
            .and_then(|n| n.checked_add(68));
        if self.pool_limit < 123_236 || problem_size.is_none_or(|n| n > self.pool_limit) {
            return Err(5);
        }
        if !(1..=3).contains(&self.slot_table.len())
            || self.slot_table.iter().any(|s| ![1, 2].contains(&s.lane))
            || self.k_max > 4
            || !ascending(&self.pkg_table)
            || self.env_allowed.iter().any(|p| {
                !self.valid_pkgs(p)
                    || self
                        .incompatible
                        .iter()
                        .any(|(a, b)| p.contains(a) && p.contains(b))
            })
        {
            return Err(6);
        }
        if self.pool_lead == 0
            || self.slot_table.iter().any(|s| {
                [s.solve_deadline, s.announce_gap, s.reveal_grace]
                    .iter()
                    .any(|n| !(1..=(1u64 << 32)).contains(n))
            })
        {
            return Err(7);
        }
        if self.statement_max == 0
            || self.statement_max > self.statement_raw_max
            || self.statement_raw_max > (1 << 27)
            || self.slot_leaves_max < self.pool_max
            || self.slot_leaves_max > (1 << 20)
        {
            return Err(8);
        }
        if let Some(m3) = &self.m3 {
            m3.validate(self, l2_gas_limit)?;
        }
        Ok(())
    }
    fn valid_pkgs(&self, p: &[Hash]) -> bool {
        p.len() <= self.k_max
            && ascending(p)
            && p.iter().all(|x| self.pkg_table.binary_search(x).is_ok())
    }
}
fn ascending<T: Ord>(v: &[T]) -> bool {
    v.windows(2).all(|p| p[0] < p[1])
}
fn sha(parts: &[&[u8]]) -> Hash {
    let mut s = Sha256::new();
    for p in parts {
        s.update(p);
    }
    s.finalize().into()
}
pub fn body_digest(b: &[u8]) -> Hash {
    sha(&[b"BOOLE-M2/BODY/v1", b])
}
pub fn cancel_hash(s: &Hash) -> Hash {
    sha(&[b"BOOLE-M2/CANCEL/v1", s])
}
pub fn commit_hash(chain: u64, p: Hash, c: Hash, b: Hash) -> Hash {
    sha(&[b"BOOLE-M2/COMMIT/v2", &chain.to_be_bytes(), &p, &c, &b])
}
pub fn env_id(schema: Hash, base: Hash, p: &[Hash]) -> Hash {
    let mut b = Vec::new();
    for x in p {
        b.extend(x);
    }
    sha(&[b"BOOLE-M2/ENV/v1", &schema, &base, &[p.len() as u8], &b])
}
pub fn part_leaf(solver: &[u8; 20], salt: &Hash, index: u16, data: &[u8]) -> Hash {
    sha(&[
        &[0],
        b"BOOLE-M2/PART/v1",
        solver,
        salt,
        &index.to_be_bytes(),
        &sha(&[data]),
    ])
}
pub fn pool_leaf(
    chain: u64,
    kind: u8,
    req: &[Hash],
    codec: u8,
    raw_len: u32,
    package: &[u8],
    salt: &Hash,
) -> Hash {
    let mut b = Vec::new();
    for p in req {
        b.extend(p);
    }
    sha(&[
        &[0],
        b"BOOLE-M2/POOL-LEAF/v2",
        &chain.to_be_bytes(),
        &[kind, req.len() as u8],
        &b,
        &[codec],
        &raw_len.to_be_bytes(),
        &sha(&[package]),
        salt,
    ])
}
fn node(a: &Hash, b: &Hash) -> Hash {
    sha(&[&[1], a, b])
}
pub fn merkle_root(leaves: &[Hash]) -> Hash {
    if leaves.is_empty() {
        return [0; 32];
    }
    let mut row = leaves.to_vec();
    row.resize(row.len().next_power_of_two(), [0; 32]);
    while row.len() > 1 {
        row = row.chunks_exact(2).map(|p| node(&p[0], &p[1])).collect();
    }
    row[0]
}
pub fn merkle_path(leaves: &[Hash], mut index: usize) -> Vec<Hash> {
    if index >= leaves.len() {
        return vec![];
    }
    let mut row = leaves.to_vec();
    row.resize(row.len().next_power_of_two(), [0; 32]);
    let mut path = Vec::new();
    while row.len() > 1 {
        path.push(row[index ^ 1]);
        index /= 2;
        row = row.chunks_exact(2).map(|p| node(&p[0], &p[1])).collect();
    }
    path
}
fn depth(n: u32) -> usize {
    if n <= 1 {
        0
    } else {
        (32 - (n - 1).leading_zeros()) as usize
    }
}
fn verify(mut leaf: Hash, mut index: u32, path: &[Hash], root: Hash) -> bool {
    for p in path {
        leaf = if index & 1 == 0 {
            node(&leaf, p)
        } else {
            node(p, &leaf)
        };
        index >>= 1;
    }
    leaf == root
}
// An overflowing deadline is unreachable; it must never wrap into the past.
#[allow(clippy::manual_saturating_arithmetic)]
fn plus(a: u64, b: u64) -> u64 {
    a.checked_add(b).unwrap_or(u64::MAX)
}

#[derive(Debug, Clone)]
pub struct Input {
    pub transaction_index: u64,
    pub log_index: u64,
    pub kind: u8,
    pub body: Vec<u8>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Status {
    Committed = 1,
    Open = 2,
    Checking = 3,
    Expired = 4,
    Cancelled = 5,
    Passed = 6,
    Accepted = 7,
    Rejected = 8,
    Void = 9,
    Stale = 10,
    Superseded = 11,
}
#[derive(Debug, Clone)]
pub struct OpenRecord {
    pub open_pos: (u64, u64, u64),
    pub done_pos: Option<(u64, u64, u64)>,
    pub export_digest: Hash,
    pub queue_index: Option<u64>,
    pub poster: [u8; 20],
    pub h_o: u64,
    pub solver: [u8; 20],
    pub salt: Hash,
    pub n: u16,
    pub z_len: u32,
    pub raw_len: u64,
    pub data_root: Hash,
    pub body_digest: Hash,
    pub env_id: Hash,
    pub mode: u8,
    pub bitmap: [u64; 2],
    pub h_r: Option<u64>,
}
#[derive(Debug, Clone)]
pub struct LiveCandidate {
    pub h_c: u64,
    pub transaction_index: u64,
    pub log_index: u64,
    pub t: Option<Hash>,
    pub poster: [u8; 20],
    pub status: Status,
    pub open: Option<OpenRecord>,
}
#[derive(Debug, Clone)]
pub enum Candidate {
    Live(Box<LiveCandidate>),
    Tombstone(Status),
}
impl Candidate {
    pub fn status(&self) -> Status {
        match self {
            Self::Live(l) => l.status,
            Self::Tombstone(s) => *s,
        }
    }
    pub fn live(&self) -> Option<&LiveCandidate> {
        if let Self::Live(x) = self {
            Some(x)
        } else {
            None
        }
    }
}
#[derive(Debug, Clone)]
pub struct Pool {
    pub slot: u8,
    pub root: Hash,
    pub start: u64,
    pub count: u32,
    pub unused: BTreeSet<u32>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundStatus {
    Drawn,
    Open,
    Closed,
    Withheld,
}
#[derive(Debug, Clone)]
pub struct Round {
    pub pool_seq: u32,
    pub leaf_index: u32,
    pub instance_seed: Hash,
    pub h_d: u64,
    pub status: RoundStatus,
    pub problem: Option<Hash>,
}
#[derive(Debug, Clone)]
pub struct Slot {
    pub round: u32,
    pub scheduled: Option<u64>,
    pub active: Option<(u8, u32)>,
}
#[derive(Debug, Clone)]
pub struct Problem {
    pub reveal_pos: (u64, u64, u64),
    pub winner: Option<Hash>,
    pub voided: bool,
    pub slot: u8,
    pub id: Hash,
    pub required_pkgs: Vec<Hash>,
    pub h_v: u64,
    pub closed: Option<u64>,
    pub pool_seq: u32,
    pub leaf_index: u32,
    pub instance_seed: Hash,
    pub epoch: u32,
}
/// Consensus state. Re-pose admission belongs to the fold alone.
/// ```compile_fail
/// use boole_derivation::m2::{Config, State};
/// let mut state = State::new(&Config::devnet(), 30_000_000).unwrap();
/// state.enqueue_repose([0; 32]);
/// ```
#[derive(Debug, Clone)]
pub struct State {
    pub(crate) height: u64,
    config: Config,
    l2_gas_limit: u64,
    pub root: Hash,
    pub candidates: BTreeMap<Key, Candidate>,
    pub pools: BTreeMap<u32, Pool>,
    pub rounds: BTreeMap<(u8, u32), Round>,
    pub slots: Vec<Slot>,
    pub problems: BTreeMap<Hash, Problem>,
    pub m3: Option<Box<crate::m3::M3State>>,
    opened: BTreeMap<Hash, Key>,
    expiry: BTreeSet<(u64, u64, Hash, Key)>,
    repose: Vec<VecDeque<Problem>>,
}
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FoldError {
    #[error(transparent)]
    M3(#[from] crate::m3::Error),
    #[error("M2 height must be previous + 1 (expected {expected:?}, got {actual})")]
    Height { expected: Option<u64>, actual: u64 },
    #[error("M2 startup invariant S{0}")]
    Startup(u8),
    #[error("M2 configuration differs from the validated state configuration")]
    ConfigurationChanged,
}
pub struct Step {
    pub state: State,
    pub codes: Vec<u8>,
    pub events: Vec<u8>,
    pub verdicts: Vec<crate::m3::IndexedVerdict>,
    pub payments: Vec<crate::m3::RewardPayment>,
}
impl State {
    pub fn new(c: &Config, l2_gas_limit: u64) -> Result<Self, FoldError> {
        c.validate(l2_gas_limit).map_err(FoldError::Startup)?;
        Ok(Self {
            config: c.clone(),
            l2_gas_limit,
            height: 0,
            root: c.digest(),
            candidates: BTreeMap::new(),
            pools: BTreeMap::new(),
            rounds: BTreeMap::new(),
            slots: (0..c.slot_table.len())
                .map(|_| Slot {
                    round: 0,
                    scheduled: None,
                    active: None,
                })
                .collect(),
            problems: BTreeMap::new(),
            m3: c.m3.as_ref().map(|_| Box::default()),
            opened: BTreeMap::new(),
            expiry: BTreeSet::new(),
            repose: vec![VecDeque::new(); c.slot_table.len()],
        })
    }
    pub fn height(&self) -> u64 {
        self.height
    }
    pub fn validate_next(&self, c: &Config, l1: &L1Block) -> Result<(), FoldError> {
        c.validate(self.l2_gas_limit).map_err(FoldError::Startup)?;
        if c != &self.config {
            return Err(FoldError::ConfigurationChanged);
        }
        let expected = self.height.checked_add(1);
        if expected != Some(l1.number) {
            return Err(FoldError::Height {
                expected,
                actual: l1.number,
            });
        }
        Ok(())
    }
    pub fn priority(&self, p: Hash) -> Vec<Hash> {
        let mut v: Vec<_> = self
            .candidates
            .iter()
            .filter_map(|((pid, ch), c)| {
                let l = c.live()?;
                if *pid == p {
                    Some((l.h_c, l.t.unwrap_or([0; 32]), *ch))
                } else {
                    None
                }
            })
            .collect();
        v.sort();
        v.into_iter().map(|x| x.2).collect()
    }
    /// M3 seam only: queue a known problem for an epoch-incremented re-pose.
    pub(crate) fn enqueue_repose(&mut self, p: Hash) -> bool {
        let Some(p) = self.problems.get(&p) else {
            return false;
        };
        let Some(queue) = self.repose.get_mut(p.slot as usize) else {
            return false;
        };
        if queue.iter().any(|x| x.id == p.id) {
            return false;
        }
        queue.push_back(p.clone());
        true
    }
    pub fn step(&self, c: &Config, l1: &L1Block, items: &[Input]) -> Result<Step, FoldError> {
        if c.m3.is_some() {
            return Err(
                crate::m3::Error::Format("M3 requires explicit verdicts and fork table").into(),
            );
        }
        self.step_inner(c, l1, items, None)
    }
    fn step_inner(
        &self,
        c: &Config,
        l1: &L1Block,
        items: &[Input],
        m3_input: Option<(&[crate::m3::IndexedVerdict], &crate::m3::ForkTable)>,
    ) -> Result<Step, FoldError> {
        self.validate_next(c, l1)?;
        let mut s = self.clone();
        let h = l1.number;
        let mut events = Vec::new();
        if let Some((_, forks)) = m3_input {
            crate::m3::activate(&mut s, h, forks, &mut events)?;
        }
        let mix = hex::decode(l1.mix_hash.strip_prefix("0x").unwrap_or(""))
            .ok()
            .and_then(|v| Hash::try_from(v).ok())
            .unwrap_or([0; 32]);
        for ((_, ch), r) in &mut s.candidates {
            if let Candidate::Live(l) = r {
                if l.h_c.checked_add(1) == Some(h) && l.t.is_none() {
                    l.t = Some(sha(&[b"BOOLE-M2/TIE/v1", ch, &mix]));
                }
            }
        }
        let verdicts = if let Some((records, forks)) = m3_input {
            crate::m3::apply_due(&mut s, c, h, records, forks, &mut events)?
        } else {
            Vec::new()
        };
        s.slot_events(c, h, mix, &mut events);
        s.expiry.clear();
        for (k, r) in &s.candidates {
            if let Candidate::Live(l) = r {
                if matches!(l.status, Status::Committed | Status::Open) {
                    s.expiry
                        .insert((plus(l.h_c, c.w), l.h_c, l.t.unwrap_or([0; 32]), *k));
                }
            }
        }
        let mut expired: Vec<_> = s.expiry.iter().take_while(|x| x.0 < h).copied().collect();
        // Problem barriers are independent; emit each problem's expiry index in order.
        expired.sort_by_key(|x| (x.3 .0, x.0, x.1, x.2, x.3 .1));
        for x in expired {
            s.candidates
                .insert(x.3, Candidate::Tombstone(Status::Expired));
            s.expiry.remove(&x);
            events.push(2);
            events.extend(x.3 .0);
            events.extend(x.3 .1);
            events.push(Status::Expired as u8);
        }
        for p in s.problems.values() {
            if p.closed
                .and_then(|v| v.checked_add(c.w))
                .and_then(|v| v.checked_add(1))
                == Some(h)
            {
                let keys: Vec<_> = s
                    .candidates
                    .iter()
                    .filter(|(k, r)| k.0 == p.id && matches!(r, Candidate::Tombstone(_)))
                    .map(|(k, _)| *k)
                    .collect();
                let count = u32::try_from(keys.len()).unwrap_or(u32::MAX);
                for k in keys {
                    s.candidates.remove(&k);
                    if s.opened.get(&k.1) == Some(&k) {
                        s.opened.remove(&k.1);
                    }
                }
                events.push(4);
                events.extend(p.id);
                events.extend(count.to_be_bytes());
            }
        }
        let mut sorted: Vec<_> = items.iter().collect();
        sorted.sort_by_key(|x| (x.transaction_index, x.log_index));
        let mut codes = Vec::new();
        let mut accepted = 0;
        for x in sorted {
            let code = s.item(c, h, x, &mut accepted);
            codes.push(code);
            if (2..=15).contains(&x.kind) {
                events.push(1);
                events.extend(x.transaction_index.to_be_bytes());
                events.extend(x.log_index.to_be_bytes());
                events.extend([x.kind, code]);
            }
            if code == 0x83 && c.m3.is_some() {
                let ch: Hash = x.body[21..53].try_into().expect("accepted part header");
                let key = s.opened[&ch];
                crate::m3::complete(&mut s, key, h, &mut events)?;
            }
        }
        if c.m3.is_some() {
            crate::m3::settle(&mut s, c, h, &mut events)?;
        }
        let payments = if c.m3.is_some() {
            crate::m3::vest(&mut s, c, h, &mut events)?
        } else {
            Vec::new()
        };
        let l1hash = hex::decode(l1.hash.strip_prefix("0x").unwrap_or("")).unwrap_or_default();
        s.root = sha(&[
            if c.m3.is_some() {
                b"BOOLE-M2/ROOT/v3"
            } else {
                b"BOOLE-M2/ROOT/v2"
            },
            &self.root,
            &h.to_be_bytes(),
            &l1hash,
            &sha(&[&events]),
        ]);
        s.height = h;
        Ok(Step {
            state: s,
            codes,
            events,
            verdicts,
            payments,
        })
    }
    pub fn step_with_verdicts(
        &self,
        c: &Config,
        l1: &L1Block,
        items: &[Input],
        verdicts: &[crate::m3::IndexedVerdict],
        forks: &crate::m3::ForkTable,
    ) -> Result<Step, FoldError> {
        if c.m3.is_none() {
            return Err(crate::m3::Error::ProfileRequired.into());
        }
        self.validate_next(c, l1)?;
        forks.validate(c).map_err(FoldError::Startup)?;
        forks.validate_history(self)?;
        self.step_inner(c, l1, items, Some((verdicts, forks)))
    }
    pub(crate) fn round_event(
        e: &mut Vec<u8>,
        slot: u8,
        round: u32,
        event: u8,
        pool_seq: u32,
        index: u32,
        p: Option<Hash>,
    ) {
        e.extend([3, slot]);
        e.extend(round.to_be_bytes());
        e.push(event);
        e.extend(pool_seq.to_be_bytes());
        e.extend(index.to_be_bytes());
        e.extend(p.unwrap_or([0; 32]));
    }
    fn slot_events(&mut self, c: &Config, h: u64, mix: Hash, e: &mut Vec<u8>) {
        let Some(announcement) = h.checked_sub(1) else {
            return;
        };
        for slot in 0..self.slots.len() {
            let sn = slot as u8;
            let params = &c.slot_table[slot];
            if let Some(key) = self.slots[slot].active {
                if let Some(r) = self.rounds.get_mut(&key) {
                    let end = match r.status {
                        RoundStatus::Drawn if plus(plus(r.h_d, 1), params.reveal_grace) < h => {
                            Some((RoundStatus::Withheld, 0xc2))
                        }
                        RoundStatus::Open
                            if r.problem
                                .and_then(|p| self.problems.get(&p))
                                .is_some_and(|p| plus(p.h_v, params.solve_deadline) < h) =>
                        {
                            Some((RoundStatus::Closed, 0xc3))
                        }
                        _ => None,
                    };
                    if let Some((status, code)) = end {
                        r.status = status;
                        if let Some(p) = r.problem {
                            if let Some(p) = self.problems.get_mut(&p) {
                                p.closed = Some(h);
                            }
                        }
                        Self::round_event(e, sn, key.1, code, r.pool_seq, r.leaf_index, r.problem);
                        self.slots[slot].active = None;
                        self.slots[slot].scheduled = h.checked_add(params.announce_gap);
                    }
                }
            }
            if self.slots[slot].active.is_some()
                || self.slots[slot].scheduled.is_none_or(|d| d >= h)
            {
                continue;
            }
            let Some(round) = self.slots[slot].round.checked_add(1) else {
                continue;
            };
            if let Some(old) = self.repose[slot].pop_front() {
                let Some(epoch) = old.epoch.checked_add(1) else {
                    continue;
                };
                let pool = &self.pools[&old.pool_seq];
                let id = problem_id(
                    c.chain_id,
                    pool.root,
                    old.leaf_index,
                    old.instance_seed,
                    epoch,
                );
                let mut p = old.clone();
                p.id = id;
                p.epoch = epoch;
                p.closed = None;
                p.winner = None;
                p.voided = false;
                p.h_v = h;
                self.problems.insert(id, p);
                self.rounds.insert(
                    (sn, round),
                    Round {
                        pool_seq: old.pool_seq,
                        leaf_index: old.leaf_index,
                        instance_seed: old.instance_seed,
                        h_d: announcement,
                        status: RoundStatus::Open,
                        problem: Some(id),
                    },
                );
                self.slots[slot] = Slot {
                    round,
                    scheduled: None,
                    active: Some((sn, round)),
                };
                Self::round_event(e, sn, round, 0xc1, old.pool_seq, old.leaf_index, Some(id));
                continue;
            }
            let hd = self.slots[slot].scheduled.unwrap_or(h);
            let eligible: Vec<_> = self
                .pools
                .iter()
                .filter(|(_, p)| p.slot == sn && p.start <= hd && !p.unused.is_empty())
                .map(|(seq, p)| (*seq, p.unused.len() as u64))
                .collect();
            let total = eligible
                .iter()
                .try_fold(0u64, |n, (_, len)| n.checked_add(*len));
            if let Some(total) = total.filter(|n| *n > 0) {
                let seed = sha(&[
                    b"BOOLE-M2/DRAW/v2",
                    &c.chain_id.to_be_bytes(),
                    &[sn],
                    &round.to_be_bytes(),
                    &mix,
                ]);
                let mut j = u64::from_be_bytes(seed[..8].try_into().unwrap_or([0; 8])) % total;
                for (seq, len) in eligible {
                    if j >= len {
                        j -= len;
                        continue;
                    }
                    let p = self.pools.get_mut(&seq).expect("eligible pool");
                    if let Some(index) = p.unused.iter().nth(j as usize).copied() {
                        p.unused.remove(&index);
                        self.rounds.insert(
                            (sn, round),
                            Round {
                                pool_seq: seq,
                                leaf_index: index,
                                instance_seed: seed,
                                h_d: announcement,
                                status: RoundStatus::Drawn,
                                problem: None,
                            },
                        );
                        self.slots[slot] = Slot {
                            round,
                            scheduled: None,
                            active: Some((sn, round)),
                        };
                        Self::round_event(e, sn, round, 0xc1, seq, index, None);
                    }
                    break;
                }
            } else {
                self.slots[slot].scheduled = Some(h);
            }
        }
    }
    fn item(&mut self, c: &Config, h: u64, x: &Input, accepted: &mut u64) -> u8 {
        match x.kind {
            2 => {
                let Ok(mut r) = Reader::version(&x.body) else {
                    return 1;
                };
                let Some((poster, p, ch)) =
                    (|| Some((r.array::<20>()?, r.array::<32>()?, r.array::<32>()?)))()
                else {
                    return 1;
                };
                if !r.done() {
                    return 1;
                }
                let Some(problem) = self.problems.get(&p) else {
                    return 4;
                };
                if h <= problem.h_v {
                    return 9;
                }
                if problem.closed.is_some()
                    || h > plus(
                        problem.h_v,
                        c.slot_table[problem.slot as usize].solve_deadline,
                    )
                {
                    return 5;
                }
                if self.candidates.contains_key(&(p, ch)) {
                    return 10;
                }
                if *accepted >= c.c_commit {
                    return 11;
                }
                *accepted += 1;
                self.candidates.insert(
                    (p, ch),
                    Candidate::Live(Box::new(LiveCandidate {
                        h_c: h,
                        transaction_index: x.transaction_index,
                        log_index: x.log_index,
                        t: None,
                        poster,
                        status: Status::Committed,
                        open: None,
                    })),
                );
                0x80
            }
            3 => {
                let o = match parse_open(c, &x.body) {
                    Ok(o) => o,
                    Err(code) => return code,
                };
                let key = (
                    o.problem,
                    commit_hash(c.chain_id, o.problem, o.cancel, o.record.body_digest),
                );
                let Some(record) = self.candidates.get_mut(&key) else {
                    return 6;
                };
                match record.status() {
                    Status::Expired | Status::Cancelled => return 9,
                    Status::Open
                    | Status::Checking
                    | Status::Passed
                    | Status::Accepted
                    | Status::Rejected
                    | Status::Void
                    | Status::Stale
                    | Status::Superseded => return 10,
                    _ => {}
                }
                let Candidate::Live(l) = record else {
                    return 9;
                };
                let Some(problem) = self.problems.get(&o.problem) else {
                    return 4;
                };
                if !problem.required_pkgs.iter().all(|p| o.packages.contains(p))
                    || o.export_format != c.slot_table[problem.slot as usize].lane
                {
                    return 2;
                }
                if h < plus(l.h_c, c.d) || h > plus(l.h_c, c.w) {
                    return 9;
                }
                let mut record = o.record;
                record.h_o = h;
                record.open_pos = (h, x.transaction_index, x.log_index);
                l.open = Some(record);
                l.status = Status::Open;
                self.opened.insert(key.1, key);
                0x81
            }
            4 => {
                let Ok(mut r) = Reader::version(&x.body) else {
                    return 1;
                };
                let Some((_poster, ch, index)) =
                    (|| Some((r.array::<20>()?, r.array::<32>()?, r.u16()?)))()
                else {
                    return 1;
                };
                let Some(key) = self.opened.get(&ch) else {
                    return 7;
                };
                let Some(record) = self.candidates.get_mut(key) else {
                    return 7;
                };
                match record.status() {
                    Status::Expired | Status::Cancelled => return 9,
                    Status::Checking
                    | Status::Passed
                    | Status::Accepted
                    | Status::Rejected
                    | Status::Void
                    | Status::Stale
                    | Status::Superseded => return 10,
                    _ => {}
                }
                let Candidate::Live(l) = record else {
                    return 9;
                };
                let Some(o) = l.open.as_mut() else {
                    return 7;
                };
                if index >= o.n {
                    return 8;
                }
                let d = depth(u32::from(o.n));
                let Some(path) = r.hashes(d) else {
                    return 8;
                };
                let data = r.rest();
                let expected = if index + 1 == o.n {
                    usize::try_from(o.z_len)
                        .unwrap_or(0)
                        .checked_sub((usize::from(o.n) - 1) * PART_SIZE)
                } else {
                    Some(PART_SIZE)
                };
                if expected != Some(data.len())
                    || !verify(
                        part_leaf(&o.solver, &o.salt, index, data),
                        u32::from(index),
                        &path,
                        o.data_root,
                    )
                {
                    return 8;
                }
                let word = usize::from(index / 64);
                let bit = 1u64 << (index % 64);
                if o.bitmap[word] & bit != 0 {
                    return 10;
                }
                o.bitmap[word] |= bit;
                if o.bitmap.iter().map(|x| x.count_ones()).sum::<u32>() == u32::from(o.n) {
                    l.status = Status::Checking;
                    o.h_r = Some(h);
                    o.done_pos = Some((h, x.transaction_index, x.log_index));
                    0x83
                } else {
                    0x82
                }
            }
            7 => {
                let Ok(mut r) = Reader::version(&x.body) else {
                    return 1;
                };
                let Some((p, ch, bd, secret)) = (|| {
                    Some((
                        r.array::<32>()?,
                        r.array::<32>()?,
                        r.array::<32>()?,
                        r.array::<32>()?,
                    ))
                })() else {
                    return 1;
                };
                if !r.done() {
                    return 1;
                }
                if commit_hash(c.chain_id, p, cancel_hash(&secret), bd) != ch {
                    return 12;
                }
                let Some(record) = self.candidates.get_mut(&(p, ch)) else {
                    return 6;
                };
                match record.status() {
                    Status::Cancelled => return 10,
                    Status::Expired
                    | Status::Checking
                    | Status::Passed
                    | Status::Accepted
                    | Status::Rejected
                    | Status::Void
                    | Status::Stale
                    | Status::Superseded => return 9,
                    _ => {}
                }
                *record = Candidate::Tombstone(Status::Cancelled);
                0x84
            }
            8 => self.pool(c, h, &x.body),
            9 => self.problem(c, h, &x.body, (x.transaction_index, x.log_index)),
            _ => 0x3f,
        }
    }
    fn pool(&mut self, c: &Config, h: u64, b: &[u8]) -> u8 {
        let Ok(mut r) = Reader::with_version(b, 2) else {
            return 1;
        };
        let Some((slot, seq, root, count, start, _battery, sig)) = (|| {
            Some((
                r.u8()?,
                r.u32()?,
                r.array::<32>()?,
                r.u32()?,
                r.u64()?,
                r.array::<32>()?,
                r.array::<65>()?,
            ))
        })() else {
            return 1;
        };
        if !r.done() {
            return 1;
        }
        let digest = sha(&[b"BOOLE-M2/POOL/v2", &c.chain_id.to_be_bytes(), &b[..82]]);
        let valid = (|| {
            let signature = Signature::from_slice(&sig[..64]).ok()?;
            if signature.normalize_s().is_some() || sig[64] > 1 {
                return None;
            }
            let id = RecoveryId::from_byte(sig[64])?;
            let key = VerifyingKey::recover_from_prehash(&digest, &signature, id).ok()?;
            let p = key.to_encoded_point(false);
            let hash = crate::user_tx::keccak(&p.as_bytes()[1..]);
            Some(hash[12..] == c.pool_key)
        })();
        if valid != Some(true) {
            return 13;
        }
        let last = self.pools.last_key_value().map(|(s, _)| *s).unwrap_or(0);
        let unused = self
            .pools
            .values()
            .filter(|p| p.slot == slot)
            .try_fold(0u64, |n, p| n.checked_add(p.unused.len() as u64));
        if usize::from(slot) >= c.slot_table.len()
            || unused
                .and_then(|n| n.checked_add(u64::from(count)))
                .is_none_or(|n| n > u64::from(c.slot_leaves_max))
            || last.checked_add(1) != Some(seq)
            || count == 0
            || count > c.pool_max
            || h.checked_add(c.pool_lead).is_none_or(|min| start < min)
        {
            return 13;
        }
        if self.pools.values().any(|p| p.root == root) {
            return 10;
        }
        self.pools.insert(
            seq,
            Pool {
                slot,
                root,
                start,
                count,
                unused: (0..count).collect(),
            },
        );
        let s = &mut self.slots[slot as usize];
        if s.active.is_none() && s.scheduled.is_none() {
            s.scheduled = Some(start);
        }
        0x85
    }
    fn problem(&mut self, c: &Config, h: u64, b: &[u8], position: (u64, u64)) -> u8 {
        let Ok(mut r) = Reader::with_version(b, 2) else {
            return 1;
        };
        let Some((slot, round, index, kind, req, salt, codec, raw_len, path, package)) = (|| {
            let slot = r.u8()?;
            let round = r.u32()?;
            let index = r.u32()?;
            let kind = r.u8()?;
            let k = usize::from(r.u8()?);
            let req = r.hashes(k)?;
            let salt = r.array::<32>()?;
            let codec = r.u8()?;
            let raw_len = r.u32()?;
            let d = usize::from(r.u8()?);
            let path = r.hashes(d)?;
            let n = usize::try_from(r.u32()?).ok()?;
            let package = r.take(n)?;
            Some((
                slot, round, index, kind, req, salt, codec, raw_len, path, package,
            ))
        })(
        ) else {
            return 1;
        };
        if !r.done() {
            return 1;
        }
        let Some(draw) = self.rounds.get_mut(&(slot, round)) else {
            return 14;
        };
        if draw.leaf_index != index {
            return 14;
        }
        match draw.status {
            RoundStatus::Open | RoundStatus::Closed => return 10,
            RoundStatus::Withheld => return 9,
            _ => {}
        }
        if package.len() > c.statement_max
            || ![1, 2].contains(&kind)
            || ![0, 1].contains(&codec)
            || (codec == 0 && usize::try_from(raw_len).ok() != Some(package.len()))
            || (codec == 1
                && (package.is_empty()
                    || raw_len == 0
                    || usize::try_from(raw_len)
                        .ok()
                        .is_none_or(|n| n > c.statement_raw_max)))
            || !ascending(&req)
            || !req.iter().all(|p| c.pkg_table.binary_search(p).is_ok())
        {
            return 2;
        }
        let pool = &self.pools[&draw.pool_seq];
        if path.len() != depth(pool.count)
            || !verify(
                pool_leaf(c.chain_id, kind, &req, codec, raw_len, package, &salt),
                index,
                &path,
                pool.root,
            )
        {
            return 14;
        }
        let id = problem_id(c.chain_id, pool.root, index, draw.instance_seed, 0);
        draw.status = RoundStatus::Open;
        draw.problem = Some(id);
        self.problems.insert(
            id,
            Problem {
                reveal_pos: (h, position.0, position.1),
                winner: None,
                voided: false,
                slot,
                id,
                required_pkgs: req,
                h_v: h,
                closed: None,
                pool_seq: draw.pool_seq,
                leaf_index: index,
                instance_seed: draw.instance_seed,
                epoch: 0,
            },
        );
        0x86
    }
}
pub fn problem_id(chain: u64, root: Hash, index: u32, seed: Hash, epoch: u32) -> Hash {
    sha(&[
        b"BOOLE-M2/PROBLEM/v2",
        &chain.to_be_bytes(),
        &root,
        &index.to_be_bytes(),
        &seed,
        &epoch.to_be_bytes(),
    ])
}
struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn version(b: &'a [u8]) -> Result<Self, u8> {
        Self::with_version(b, 1)
    }
    fn with_version(b: &'a [u8], version: u8) -> Result<Self, u8> {
        if b.first() != Some(&version) {
            Err(1)
        } else {
            Ok(Self { b, at: 1 })
        }
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n)?;
        let v = self.b.get(self.at..end)?;
        self.at = end;
        Some(v)
    }
    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.array::<1>()?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes(self.array()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.array()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_be_bytes(self.array()?))
    }
    fn hashes(&mut self, n: usize) -> Option<Vec<Hash>> {
        if n > self.b.len().checked_sub(self.at)? / 32 {
            return None;
        }
        (0..n).map(|_| self.array()).collect()
    }
    fn done(&self) -> bool {
        self.at == self.b.len()
    }
    fn rest(&self) -> &'a [u8] {
        &self.b[self.at..]
    }
}
struct ParsedOpen {
    export_format: u8,
    problem: Hash,
    cancel: Hash,
    packages: Vec<Hash>,
    record: OpenRecord,
}
fn parse_open(c: &Config, b: &[u8]) -> Result<ParsedOpen, u8> {
    let decoded = (|| {
        let mut r = Reader::version(b).ok()?;
        let poster = r.array::<20>()?;
        let cancel = r.array::<32>()?;
        let start = r.at;
        if r.u8() != Some(1) {
            return None;
        }
        let problem = r.array::<32>()?;
        let solver = r.array::<20>()?;
        let salt = r.array::<32>()?;
        let mode = r.u8()?;
        let schema = r.array::<32>()?;
        let base = r.array::<32>()?;
        let k = usize::from(r.u8()?);
        let pkgs = r.hashes(k)?;
        let codec = r.u8()?;
        let format = r.u8()?;
        let raw = r.u64()?;
        let export = r.array::<32>()?;
        let z = r.u32()?;
        let n = r.u16()?;
        let root = r.array::<32>()?;
        let deps = r.u16()?;
        let ext = r.u16()?;
        let meta_len = usize::from(r.u16()?);
        r.take(usize::from(deps).checked_mul(64)?)?;
        r.take(usize::from(ext))?;
        let meta = r.take(meta_len)?;
        if !r.done()
            || meta.len() < 40
            || meta[0] != 1
            || usize::from(meta[1]).checked_add(40) != Some(meta_len)
        {
            return None;
        }
        Some((
            poster, cancel, start, problem, solver, salt, mode, schema, base, pkgs, codec, format,
            raw, export, z, n, root, deps, ext, meta,
        ))
    })();
    let (
        poster,
        cancel,
        start,
        problem,
        solver,
        salt,
        mode,
        schema,
        base,
        packages,
        codec,
        format,
        raw,
        export,
        z,
        n,
        root,
        deps,
        ext,
        meta,
    ) = decoded.ok_or(1u8)?;
    if mode != 1
        || schema != c.schema_id
        || base != c.base_root
        || !c.valid_pkgs(&packages)
        || !c.env_allowed.contains(&packages)
        || codec != 1
        || ![1, 2].contains(&format)
        || deps != 0
        || ext != 0
    {
        return Err(2);
    }
    let model_len = usize::from(meta[1]);
    let model = &meta[2..2 + model_len];
    let at = 2 + model_len;
    let license = meta[at];
    let permission = meta[at + 1];
    let trace = &meta[at + 2..at + 34];
    let embargo = u32::from_be_bytes(meta[at + 34..at + 38].try_into().map_err(|_| 1u8)?);
    let has_trace = trace.iter().any(|b| *b != 0);
    if model_len > 64
        || model.iter().any(|b| !(0x20..=0x7e).contains(b))
        || license == 0
        || !c.terms_table.contains_key(&license)
        || permission > 1
        || (!has_trace && (permission != 0 || embargo != 0))
        || embargo > c.trace_embargo_max
    {
        return Err(2);
    }
    let expected = usize::try_from(z)
        .ok()
        .and_then(|z| z.checked_add(PART_SIZE - 1))
        .map(|z| z / PART_SIZE);
    if z == 0 || expected != Some(usize::from(n)) {
        return Err(2);
    }
    if n > MAX_PARTS
        || raw == 0
        || raw > RAW_CAP
        || u64::from(z)
            .checked_mul(c.ratio_max)
            .is_none_or(|max| raw > max)
    {
        return Err(3);
    }
    Ok(ParsedOpen {
        export_format: format,
        problem,
        cancel,
        record: OpenRecord {
            open_pos: (0, 0, 0),
            done_pos: None,
            export_digest: export,
            queue_index: None,
            poster,
            h_o: 0,
            solver,
            salt,
            n,
            z_len: z,
            raw_len: raw,
            data_root: root,
            body_digest: body_digest(&b[start..]),
            env_id: env_id(schema, base, &packages),
            mode,
            bitmap: [0; 2],
            h_r: None,
        },
        packages,
    })
}
pub fn rule(code: u8) -> &'static str {
    match code {
        1 => "M2-DECODE-INVALID",
        2 => "M2-POLICY-INVALID",
        3 => "M2-SIZE-CAP",
        4 => "M2-UNKNOWN-PROBLEM",
        5 => "M2-PROBLEM-CLOSED",
        6 => "M2-COMMIT-NOT-FOUND",
        7 => "M2-REVEAL-NOT-OPEN",
        8 => "M2-PART-INVALID",
        9 => "M2-OUTSIDE-WINDOW",
        10 => "M2-DUPLICATE",
        11 => "M2-BLOCK-BUDGET-EXCEEDED",
        12 => "M2-CANCEL-INVALID",
        13 => "M2-POOL-INVALID",
        14 => "M2-MEMBERSHIP-INVALID",
        0x80 => "M2-COMMIT-RECORDED",
        0x81 => "M2-REVEAL-OPENED",
        0x82 => "M2-PART-RECORDED",
        0x83 => "M2-REVEAL-COMPLETE",
        0x84 => "M2-CANCELLED",
        0x85 => "M2-POOL-RECORDED",
        0x86 => "M2-PROBLEM-OPENED",
        _ => "ITEM-KIND-NOT-ACCEPTED",
    }
}

#[cfg(test)]
mod continuity_tests {
    use super::*;
    #[test]
    fn height_overflow_refuses_to_fold() {
        let c = Config::devnet();
        let mut state = State::new(&c, 30_000_000).unwrap();
        state.height = u64::MAX;
        let l1 = L1Block {
            number: 0,
            hash: String::new(),
            parent_hash: String::new(),
            timestamp: 0,
            mix_hash: String::new(),
            base_fee_per_gas: String::new(),
            parent_beacon_block_root: String::new(),
        };
        assert!(matches!(
            state.step(&c, &l1, &[]),
            Err(FoldError::Height {
                expected: None,
                actual: 0
            })
        ));
    }
}
