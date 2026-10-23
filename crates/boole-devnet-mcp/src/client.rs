use crate::{
    codec::{digest, read_bounded, Codec},
    hook::Precheck,
    store::Store,
    wire::{calldata, Material, Prepared},
};
use anyhow::{bail, ensure, Context, Result};
use boole_derivation::{
    attributes::{hex_bytes, hex_string},
    m2::*,
    InboxLog, L1Block,
};
use boole_devnet_driver::{
    engine::{retry_rpc, Rpc},
    l1::L1Source,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct RevertedReceipt;
impl std::fmt::Display for RevertedReceipt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("devnet posting transaction reverted")
    }
}
impl std::error::Error for RevertedReceipt {}

pub struct CommitOptions {
    pub allow_stale_gate: bool,
    pub license_id: u8,
}
impl Default for CommitOptions {
    fn default() -> Self {
        Self {
            allow_stale_gate: false,
            license_id: 1,
        }
    }
}
#[derive(Debug)]
pub struct StaleGate {
    pub declared: Option<Hash>,
    pub expected: Hash,
    pub estimated_earliest_due_height: u64,
}
impl std::fmt::Display for StaleGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            json!({"local_check":"stale-gate","commit_refused":true,"adapter_gate_rule_digest":self.declared.map(|h|hex_string(&h)),"expected_gate_rule_digest":hex_string(&self.expected),"estimated_earliest_due_height":self.estimated_earliest_due_height})
        )
    }
}
impl std::error::Error for StaleGate {}

pub trait Poster {
    fn head(&mut self) -> Result<u64>;
    fn post(&mut self, kind: u8, body: &[u8]) -> Result<String>;
    /// Only a successful receipt in the current canonical L1 counts.
    fn included(&mut self, tx: &str) -> Result<Option<u64>>;
}
pub struct RpcPoster {
    rpc: Rpc,
    account: String,
}
impl RpcPoster {
    pub fn new(url: &str, account: [u8; 20]) -> Result<Self> {
        let rpc = Rpc::new(url)?;
        let account = hex_string(&account);
        ensure!(
            rpc.call("eth_chainId", json!([]))?
                == format!("0x{:x}", boole_derivation::config::L1_CHAIN_ID),
            "unexpected devnet L1 chain"
        );
        let accounts = rpc.call("eth_accounts", json!([]))?;
        ensure!(
            accounts.as_array().is_some_and(|a| a
                .iter()
                .any(|x| x.as_str().is_some_and(|s| s.eq_ignore_ascii_case(&account)))),
            "account is not an unlocked devnet account"
        );
        Ok(Self { rpc, account })
    }
}
fn quantity(value: &Value) -> Result<u64> {
    Ok(u64::from_str_radix(
        value
            .as_str()
            .and_then(|x| x.strip_prefix("0x"))
            .context("RPC quantity")?,
        16,
    )?)
}
impl Poster for RpcPoster {
    fn head(&mut self) -> Result<u64> {
        quantity(&self.rpc.call("eth_blockNumber", json!([]))?)
    }
    fn post(&mut self, kind: u8, body: &[u8]) -> Result<String> {
        let tx=self.rpc.call("eth_sendTransaction",json!([{"from":self.account,"to":boole_derivation::config::INBOX,"data":hex_string(&calldata(kind,body)),"gas":if kind==4 {"0x4dd1e0"}else{"0xf4240"}}])).map_err(|_|anyhow::anyhow!("devnet post outcome unknown; reconciliation required"))?;
        let tx = tx.as_str().context("missing transaction hash")?;
        let _: Hash = fixed(tx)?;
        Ok(tx.into())
    }
    fn included(&mut self, tx: &str) -> Result<Option<u64>> {
        let r = self.rpc.call("eth_getTransactionReceipt", json!([tx]))?;
        if r.is_null() {
            return Ok(None);
        }
        let h = quantity(&r["blockNumber"])?;
        let b = self
            .rpc
            .call("eth_getBlockByNumber", json!([r["blockNumber"], false]))?;
        if fixed::<32>(b["hash"].as_str().context("receipt canonical hash")?)?
            != fixed::<32>(r["blockHash"].as_str().context("receipt block hash")?)?
        {
            return Ok(None);
        }
        if r["status"] == "0x0" {
            return Err(RevertedReceipt.into());
        }
        ensure!(r["status"] == "0x1", "invalid receipt status");
        Ok(Some(h))
    }
}
/// Read the same L2 head gasLimit that the driver supplies to State::new.
pub fn l2_gas_limit(url: &str) -> Result<u64> {
    let node = Rpc::new(url)?;
    ensure!(
        retry_rpc(|| node.call("eth_chainId", json!([])))?
            == format!("0x{:x}", boole_derivation::config::L2_CHAIN_ID),
        "unexpected devnet L2 chain"
    );
    let block = retry_rpc(|| node.call("eth_getBlockByNumber", json!(["latest", false])))?;
    let head: boole_derivation::ParentHeader =
        serde_json::from_value(block).context("decode L2 head")?;
    Ok(head.gas_limit)
}
pub fn fixed<const N: usize>(s: &str) -> Result<[u8; N]> {
    hex_bytes(s, "tool hex field")?
        .try_into()
        .map_err(|_| anyhow::anyhow!("hex field length"))
}

#[derive(Clone)]
struct Package {
    codec: u8,
    raw_len: u32,
    bytes: Vec<u8>,
}
struct Anchor {
    number: u64,
    hash: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Sent {
    head: u64,
    tx: Option<String>,
    #[serde(default)]
    failure: Option<String>,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum SubmissionState {
    CommitPending,
    Committed,
    Open,
    Checking,
    Expired,
    Cancelled,
    Passed,
    Accepted,
    Rejected,
    Void,
    Stale,
    Superseded,
    CommitNotIncluded,
    ProblemClosed,
    Aborted,
}
impl SubmissionState {
    fn from_candidate(status: Status) -> Self {
        match status {
            Status::Committed => Self::Committed,
            Status::Open => Self::Open,
            Status::Checking => Self::Checking,
            Status::Expired => Self::Expired,
            Status::Cancelled => Self::Cancelled,
            Status::Passed => Self::Passed,
            Status::Accepted => Self::Accepted,
            Status::Rejected => Self::Rejected,
            Status::Void => Self::Void,
            Status::Stale => Self::Stale,
            Status::Superseded => Self::Superseded,
        }
    }
    fn final_state(self) -> bool {
        matches!(
            self,
            Self::Checking
                | Self::Expired
                | Self::Cancelled
                | Self::Passed
                | Self::Accepted
                | Self::Rejected
                | Self::Void
                | Self::Stale
                | Self::Superseded
                | Self::CommitNotIncluded
                | Self::ProblemClosed
                | Self::Aborted
        )
    }
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Observation {
    state: SubmissionState,
    height: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Submission {
    schema_version: u8,
    config_digest: Hash,
    prepared: Prepared,
    cancel_requested: bool,
    local_check: String,
    sent: BTreeMap<String, Sent>,
    last_error: Option<String>,
    #[serde(default)]
    observed: Option<Observation>,
    #[serde(default)]
    prune_height: Option<u64>,
    #[serde(default)]
    precheck_gate: Option<Hash>,
    #[serde(default)]
    precheck_override: bool,
}
#[derive(Clone)]
pub struct CanonicalBlock {
    pub number: u64,
    pub hash: Hash,
    pub parent_hash: Hash,
    pub transactions: Vec<String>,
}
pub trait L2Source {
    /// None means the node has not built this finalized L1 height yet.
    fn root(&mut self, height: u64, origin: &str) -> Result<Option<Hash>>;
    /// Actual block transaction bytes, for M3's strict system-prefix fold.
    fn block(&mut self, _height: u64) -> Result<Option<CanonicalBlock>> {
        Ok(None)
    }
    fn code(&mut self, _payee: [u8; 20]) -> Result<Vec<u8>> {
        bail!("L2 payee code probe unavailable")
    }
    fn balance(&mut self, _payee: [u8; 20], _height: u64) -> Result<Hash> {
        bail!("L2 balance probe unavailable")
    }
}
pub struct RpcL2 {
    rpc: Rpc,
}
impl RpcL2 {
    pub fn new(url: &str) -> Result<Self> {
        let rpc = Rpc::new(url)?;
        ensure!(
            retry_rpc(|| rpc.call("eth_chainId", json!([])))?
                == format!("0x{:x}", boole_derivation::config::L2_CHAIN_ID),
            "unexpected devnet L2 chain"
        );
        Ok(Self { rpc })
    }
}
impl L2Source for RpcL2 {
    fn balance(&mut self, payee: [u8; 20], height: u64) -> Result<Hash> {
        let value = self.rpc.call(
            "eth_getBalance",
            json!([hex_string(&payee), format!("0x{height:x}")]),
        )?;
        crate::m3_status::quantity_word(value.as_str().context("L2 balance response")?)
    }
    fn code(&mut self, payee: [u8; 20]) -> Result<Vec<u8>> {
        hex_bytes(
            self.rpc
                .call("eth_getCode", json!([hex_string(&payee), "latest"]))?
                .as_str()
                .context("L2 code response")?,
            "L2 code",
        )
        .map_err(Into::into)
    }
    fn block(&mut self, height: u64) -> Result<Option<CanonicalBlock>> {
        let value = self.rpc.call(
            "eth_getBlockByNumber",
            json!([format!("0x{height:x}"), false]),
        )?;
        if value.is_null() {
            return Ok(None);
        }
        ensure!(
            quantity(&value["number"])? == height,
            "L2 block height mismatch"
        );
        let mut transactions = vec![];
        for hash in value["transactions"]
            .as_array()
            .context("L2 transaction list")?
        {
            let hash = hash.as_str().context("L2 transaction hash")?;
            let bytes = self
                .rpc
                .call("eth_getRawTransactionByHash", json!([hash]))?;
            let raw = bytes.as_str().context("raw L2 transaction unavailable")?;
            let bytes = hex_bytes(raw, "raw L2 transaction")?;
            ensure!(
                fixed::<32>(hash)? == boole_derivation::user_tx::keccak(&bytes),
                "raw L2 transaction hash mismatch"
            );
            transactions.push(raw.into());
        }
        Ok(Some(CanonicalBlock {
            number: height,
            hash: fixed(value["hash"].as_str().context("L2 block hash")?)?,
            parent_hash: fixed(value["parentHash"].as_str().context("L2 parent hash")?)?,
            transactions,
        }))
    }
    fn root(&mut self, height: u64, origin: &str) -> Result<Option<Hash>> {
        let Some(header) =
            boole_devnet_driver::engine::read_l2_header(&self.rpc, json!(format!("0x{height:x}")))?
        else {
            return Ok(None);
        };
        ensure!(header.number == height, "L2 root height mismatch");
        let failure = boole_derivation::InputError::M2Root { height };
        let l1 =
            boole_derivation::system::l1_origin(header.l1_info.as_deref().ok_or(failure)?, height)?;
        if fixed::<32>(&l1)? != fixed::<32>(origin)? {
            return Err(boole_derivation::InputError::M2Root { height }.into());
        }
        Ok(Some(boole_derivation::system::read_m2_root(
            header
                .m2_root_deposit
                .as_deref()
                .ok_or(boole_derivation::InputError::M2Root { height })?,
            height,
            origin,
        )?))
    }
}
pub struct ClientOptions {
    pub l2: Option<Box<dyn L2Source>>,
    pub l2_gas_limit: u64,
    pub codec: Option<Codec>,
    pub hook: Option<Precheck>,
    pub forks: Option<boole_derivation::m3::ForkTable>,
    pub own: Option<crate::own::OwnVerification>,
}
impl ClientOptions {
    pub fn new(l2_gas_limit: u64) -> Self {
        Self {
            l2: None,
            l2_gas_limit,
            codec: None,
            hook: None,
            forks: None,
            own: None,
        }
    }
}
pub struct Client<L, P> {
    pub source: L,
    pub poster: P,
    pub config: Config,
    state: State,
    anchor: Anchor,
    packages: BTreeMap<(u32, u32), Package>,
    store: Store,
    submissions: BTreeMap<Hash, Submission>,
    codec: Option<Codec>,
    hook: Option<Precheck>,
    account: [u8; 20],
    stalled: Option<String>,
    l2: Option<Box<dyn L2Source>>,
    l2_m2_root_match: Option<bool>,
    budget_head: Option<u64>,
    posted: u64,
    chain: Option<crate::chain::Chain>,
    waiting_l2: bool,
    vested_at: BTreeMap<u64, u64>,
    own: Option<crate::own::Verifier>,
}
impl<L: L1Source, P: Poster> Client<L, P> {
    pub fn new(
        mut source: L,
        poster: P,
        config: Config,
        account: [u8; 20],
        path: PathBuf,
        options: ClientOptions,
    ) -> Result<Self> {
        ensure!(
            config.chain_id == boole_derivation::config::L2_CHAIN_ID,
            "client supports the devnet L2 only"
        );
        ensure!(
            config.m3.is_none() || options.l2.is_some(),
            "M3 client requires a canonical L2 source"
        );
        ensure!(
            config.m3.is_some() || options.own.is_none(),
            "--verify-own requires M3"
        );
        ensure!(
            config.m3.is_some() || options.forks.is_none(),
            "fork table requires M3 profile"
        );
        let chain = if config.m3.is_some() {
            Some(crate::chain::Chain::new(&config, options.forks)?)
        } else {
            None
        };
        source.validate_m2_inbox()?;
        let state = State::new(&config, options.l2_gas_limit)?;
        let anchor = Anchor {
            number: 0,
            hash: source.genesis_hash()?,
        };
        let _: Hash = fixed(&anchor.hash)?;
        let store = Store::open(path)?;
        let identity = json!({"schema_version":2,"config_digest":hex_string(&config.digest()),"l1_genesis_hash":anchor.hash,"l2_gas_limit":options.l2_gas_limit});
        if store.path.join("identity.json").exists() {
            let saved: Value = store.read("identity.json")?;
            ensure!(
                saved.get("schema_version").and_then(Value::as_u64) == Some(2),
                "v1 store refused: schema_version 2 requires a fresh private directory"
            );
            ensure!(
                saved == identity,
                "private store belongs to another genesis/config"
            );
        } else {
            store.write("identity.json", &identity)?;
        }
        let mut submissions = BTreeMap::new();
        for name in store.submissions()? {
            let raw: Value = store.read(&name)?;
            ensure!(
                raw.get("schema_version").and_then(Value::as_u64) == Some(2),
                "v1 store submission refused: schema_version 2 requires a fresh private directory"
            );
            let sub: Submission = serde_json::from_value(raw)?;
            ensure!(
                sub.schema_version == 2 && sub.config_digest == config.digest(),
                "submission config/version mismatch"
            );
            sub.prepared.validate(&config)?;
            ensure!(
                name == submission_name(sub.prepared.commit),
                "submission file identity mismatch"
            );
            submissions.insert(sub.prepared.commit, sub);
        }
        let own = if let Some(settings) = options.own {
            Some(crate::own::Verifier::new(
                &config,
                &chain.as_ref().context("--verify-own requires M3")?.forks,
                settings,
                options.codec.clone(),
                &store.path,
            )?)
        } else {
            None
        };
        Ok(Self {
            source,
            poster,
            config,
            state,
            anchor,
            packages: BTreeMap::new(),
            store,
            submissions,
            codec: options.codec,
            hook: options.hook,
            account,
            stalled: None,
            l2: options.l2,
            l2_m2_root_match: None,
            budget_head: None,
            posted: 0,
            chain,
            waiting_l2: false,
            vested_at: BTreeMap::new(),
            own,
        })
    }
    pub fn m2_state(&self) -> &State {
        &self.state
    }
    pub fn sync(&mut self) -> Result<()> {
        if let Some(reason) = &self.stalled {
            bail!("{reason}");
        }
        if let Err(e) = self.fold_finalized().and_then(|_| self.check_l2_root()) {
            if self.config.m3.is_some() {
                self.l2_m2_root_match = Some(false);
            }
            self.stalled = Some(format!("finalized fold refused: {e}"));
            return Err(e.context(
                self.stalled
                    .clone()
                    .unwrap_or_else(|| "fold stalled".into()),
            ));
        }
        Ok(())
    }
    fn check_l2_root(&mut self) -> Result<()> {
        self.l2_m2_root_match = None;
        if self.state.height() == 0 {
            return Ok(());
        }
        if let Some(chain) = &self.chain {
            if !self.waiting_l2 {
                let block = &chain
                    .blocks
                    .get(&self.state.height())
                    .context("verified L2 block")?
                    .block;
                let root = boole_derivation::system::read_m2_root(
                    &block.transactions[1],
                    self.state.height(),
                    &self.anchor.hash,
                )?;
                self.l2_m2_root_match = Some(root == self.state.root);
                ensure!(root == self.state.root, "M3 root mismatch");
            }
            return Ok(());
        }
        let Some(node) = &mut self.l2 else {
            return Ok(());
        };
        match retry_rpc(|| node.root(self.state.height(), &self.anchor.hash)) {
            Ok(Some(root)) => {
                self.l2_m2_root_match = Some(root == self.state.root);
                if root != self.state.root {
                    return Err(boole_derivation::InputError::M2Root {
                        height: self.state.height(),
                    }
                    .into());
                }
                if self.config.m3.is_none() {
                    let checked = (|| -> Result<()> {
                        if let Some(block) = retry_rpc(|| node.block(self.state.height()))? {
                            ensure!(
                                block.number == self.state.height(),
                                "L2 prefix height mismatch"
                            );
                            let l1 = retry_rpc(|| self.source.block(self.state.height()))?;
                            let expected = vec![
                                boole_derivation::system::l1_info(&l1)?,
                                boole_derivation::system::m2_root_deposit(&l1, self.state.root)?,
                            ];
                            boole_derivation::m3::check_system_prefix(
                                &block.transactions,
                                &expected,
                            )?;
                        }
                        Ok(())
                    })();
                    if let Err(error) = checked {
                        self.l2_m2_root_match = Some(false);
                        return Err(error);
                    }
                }
            }
            Ok(None) => {}
            Err(e) => {
                if e.is::<boole_derivation::InputError>() {
                    self.l2_m2_root_match = Some(false);
                }
                return Err(e);
            }
        }
        Ok(())
    }
    fn fold_finalized(&mut self) -> Result<()> {
        let finalized = retry_rpc(|| self.source.finalized())?;
        ensure!(
            finalized >= self.state.height(),
            "finalized height moved backwards"
        );
        let cached = if self.anchor.number == 0 {
            retry_rpc(|| self.source.genesis_hash())?
        } else {
            let block = retry_rpc(|| self.source.block(self.anchor.number))?;
            ensure!(block.number == self.anchor.number, "cached anchor height");
            block.hash
        };
        ensure!(
            fixed::<32>(&cached)? == fixed::<32>(&self.anchor.hash)?,
            "finalized anchor changed"
        );
        self.waiting_l2 = false;
        if let Some(chain) = &mut self.chain {
            if !chain.refresh(
                &self.config,
                &mut self.source,
                self.l2.as_mut().context("M3 L2 source")?.as_mut(),
                finalized,
            )? {
                self.waiting_l2 = true;
                return Ok(());
            }
        }
        while self.state.height() < finalized {
            let h = self
                .state
                .height()
                .checked_add(1)
                .context("fold height overflow")?;
            let block = retry_rpc(|| self.source.block(h))?;
            ensure!(
                fixed::<32>(&block.parent_hash)? == fixed::<32>(&self.anchor.hash)?,
                "L1 parent link"
            );
            // Match derive_m1's field checks before M2 can consume mixHash.
            let _: Hash = fixed(&block.mix_hash)?;
            let _: Hash = fixed(&block.parent_beacon_block_root)?;
            boole_derivation::system::l1_info(&block)?;
            let logs = retry_rpc(|| self.source.inbox_logs(&block.hash))?;
            let inputs = validated_inputs(&block, logs)?;
            let step = if let Some(chain) = &self.chain {
                if let Some(own) = &self.own {
                    let records = chain.inputs(&self.state, &self.config, h)?;
                    let owned = self.submissions.keys().copied().collect();
                    let mut history = crate::own::AnchoredHistory {
                        chain,
                        l1: &mut self.source,
                        maximum: self.state.height(),
                    };
                    own.verify(
                        &self.state,
                        &self.config,
                        &chain.forks,
                        &records,
                        &owned,
                        &mut history,
                    )?;
                }
                let Some(step) = chain.step(&self.state, &self.config, &block, &inputs)? else {
                    self.waiting_l2 = true;
                    break;
                };
                step
            } else {
                self.state.step(&self.config, &block, &inputs)?
            };
            let repeated = retry_rpc(|| self.source.block(h))?;
            ensure!(
                fixed::<32>(&repeated.hash)? == fixed::<32>(&block.hash)?,
                "L1 changed during read"
            );
            let mut additions = vec![];
            for (item, code) in inputs.iter().zip(&step.codes) {
                if item.kind == 9 && *code == 0x86 {
                    let b = &item.body;
                    let slot = b[1];
                    let round = u32::from_be_bytes(b[2..6].try_into()?);
                    let at = 12 + 32 * usize::from(b[11]) + 32;
                    let codec = b[at];
                    let raw_len = u32::from_be_bytes(b[at + 1..at + 5].try_into()?);
                    let data = at + 6 + 32 * usize::from(b[at + 5]) + 4;
                    let r = &step.state.rounds[&(slot, round)];
                    additions.push((
                        (r.pool_seq, r.leaf_index),
                        Package {
                            codec,
                            raw_len,
                            bytes: b[data..].to_vec(),
                        },
                    ));
                }
            }
            self.packages.extend(additions);
            if let Some(next) = &step.state.m3 {
                for reward in next.rewards.values() {
                    if reward.status == boole_derivation::m3::RewardStatus::Vested
                        && self
                            .state
                            .m3
                            .as_ref()
                            .and_then(|m| m.rewards.get(&reward.id))
                            .is_none_or(|r| r.status != boole_derivation::m3::RewardStatus::Vested)
                    {
                        self.vested_at.insert(reward.id, h);
                    }
                }
            }
            self.state = step.state;
            self.anchor = Anchor {
                number: block.number,
                hash: block.hash,
            };
            // Observe every height, including an expiry immediately before prune,
            // even when a restarted worker catches up in a single sync.
            self.observe_submissions()?;
        }
        self.observe_submissions()?;
        Ok(())
    }
    fn observe_submissions(&mut self) -> Result<()> {
        let height = self.state.height();
        let mut changed = vec![];
        for sub in self.submissions.values() {
            if sub.observed.as_ref().is_some_and(|o| o.height > height) {
                continue;
            }
            let problem = self.state.problems.get(&sub.prepared.problem);
            let prune_height = if let Some(close) = problem.and_then(|p| p.closed) {
                Some(
                    close
                        .checked_add(self.config.w)
                        .and_then(|h| h.checked_add(1))
                        .context("prune height overflow")?,
                )
            } else {
                sub.prune_height
            };
            let candidate = self
                .state
                .candidates
                .get(&(sub.prepared.problem, sub.prepared.commit));
            let state = if let Some(c) = candidate {
                SubmissionState::from_candidate(c.status())
            } else if let Some(o) = sub.observed.as_ref().filter(|o| o.state.final_state()) {
                o.state
            } else if sub.cancel_requested && !sub.sent.contains_key("commit") {
                SubmissionState::Aborted
            } else if let Some(p) = problem {
                let deadline = p
                    .h_v
                    .checked_add(self.config.slot_table[p.slot as usize].solve_deadline)
                    .context("commit deadline overflow")?;
                if p.closed.is_some() && !sub.sent.contains_key("commit") {
                    SubmissionState::ProblemClosed
                } else if height > deadline {
                    SubmissionState::CommitNotIncluded
                } else {
                    SubmissionState::CommitPending
                }
            } else {
                SubmissionState::CommitPending
            };
            let observed = if sub.observed.as_ref().is_none_or(|o| o.state != state) {
                Some(Observation { state, height })
            } else {
                sub.observed.clone()
            };
            if observed != sub.observed || prune_height != sub.prune_height {
                let mut next = sub.clone();
                next.observed = observed;
                next.prune_height = prune_height;
                changed.push(next);
            }
        }
        for sub in changed {
            self.save(&sub)?;
        }
        Ok(())
    }
    pub fn status(&self) -> Value {
        let slots:Vec<_>=self.state.slots.iter().enumerate().map(|(slot,s)| {
            let p=s.active.and_then(|key|self.state.rounds.get(&key)).and_then(|r|r.problem).and_then(|id|self.state.problems.get(&id));
            let pending=p.map(|p|self.state.candidates.iter().filter(|((id,_),v)|*id==p.id && matches!(v.status(),Status::Committed|Status::Open)).count()).unwrap_or(0);
            json!({"slot":slot,"lane":self.config.slot_table[slot].lane,"problem_id":p.map(|p|hex_string(&p.id)),"last_commit_height":p.and_then(|p|p.h_v.checked_add(self.config.slot_table[slot].solve_deadline)),"pending_commits":pending,"next_draw_height":s.scheduled.and_then(|h|h.checked_add(1))})
        }).collect();
        json!({"devnet_only":true,"fold_height":self.state.height(),"m2_root":hex_string(&self.state.root),"config_digest":hex_string(&self.config.digest()),"slot_table_digest":hex_string(&self.config.slot_table_digest()),"slots":slots,"stalled":self.stalled,"l2_m2_root_match":self.l2_m2_root_match,"waiting_for_l2":self.waiting_l2,"m3":crate::m3_status::summary(&self.state),"verdict_trust":if self.own.is_some(){"l2-node-with-own-verification"}else if self.config.m3.is_some(){"l2-node"}else{"l1-only"}})
    }
    fn decoded_problem(&self, id: Hash) -> Result<Vec<u8>> {
        let p = self.state.problems.get(&id).context("problem not found")?;
        let pkg = self
            .packages
            .get(&(p.pool_seq, p.leaf_index))
            .context("accepted problem package not found")?;
        match pkg.codec {
            0 => Ok(pkg.bytes.clone()),
            1 => self
                .codec
                .as_ref()
                .context("pinned codec is not configured")?
                .decode(&pkg.bytes, u64::from(pkg.raw_len), &self.store.path)
                .context("problem package operator fault; do not commit"),
            _ => bail!("problem codec operator fault"),
        }
    }
    pub fn problem(&self, id: Hash) -> Result<Value> {
        let p = self.state.problems.get(&id).context("problem not found")?;
        ensure!(p.closed.is_none(), "problem closed");
        let bytes = self.decoded_problem(id)?;
        Ok(
            json!({"problem_id":hex_string(&id),"slot":p.slot,"lane":self.config.slot_table[p.slot as usize].lane,"membership":"accepted_by_finalized_fold","decoded_len":bytes.len(),"package_hex":hex_string(&bytes),"package_utf8":std::str::from_utf8(&bytes).ok()}),
        )
    }
    pub fn commit(&mut self, path: &Path, payee: [u8; 20], id: Hash) -> Result<Value> {
        self.commit_with_options(path, payee, id, CommitOptions::default())
    }
    pub fn commit_with_options(
        &mut self,
        path: &Path,
        payee: [u8; 20],
        id: Hash,
        options: CommitOptions,
    ) -> Result<Value> {
        self.sync()?;
        Prepared::license_terms(&self.config, options.license_id)?;
        self.check_payee(payee)?;
        let p = self
            .state
            .problems
            .get(&id)
            .context("problem not found")?
            .clone();
        ensure!(p.closed.is_none(), "problem closed");
        let next = retry_rpc(|| self.poster.head())?
            .checked_add(1)
            .context("head overflow")?;
        ensure!(
            next > p.h_v
                && next
                    <= p.h_v
                        .checked_add(self.config.slot_table[p.slot as usize].solve_deadline)
                        .context("deadline overflow")?,
            "problem commit deadline"
        );
        let problem = self.decoded_problem(id)?;
        let raw = read_bounded(path, RAW_CAP as usize)?;
        let packages = self
            .config
            .env_allowed
            .iter()
            .find(|pkgs| p.required_pkgs.iter().all(|p| pkgs.contains(p)))
            .context("required environment not allowed")?
            .clone();
        let format = self.config.slot_table[p.slot as usize].lane;
        let compressed = self
            .codec
            .as_ref()
            .context("pinned codec is not configured")?
            .compress(&raw, &self.store.path)?;
        let mut local_check = "not_run";
        let mut precheck_gate = None;
        if let Some(hook) = &self.hook {
            hook.run(
                &self.store.path,
                &raw,
                &problem,
                env_id(self.config.schema_id, self.config.base_root, &packages),
                format,
            )?;
            precheck_gate = hook.gate_rule_digest;
            local_check = "local_accept";
            if let Some(stale) =
                self.stale_gate(precheck_gate, next, compressed.len().div_ceil(PART_SIZE))?
            {
                local_check = "stale-gate";
                if !options.allow_stale_gate {
                    return Err(stale.into());
                }
            }
        }
        let mut salt = [0; 32];
        let mut cancel_secret = [0; 32];
        let rng = ring::rand::SystemRandom::new();
        use ring::rand::SecureRandom;
        rng.fill(&mut salt)
            .map_err(|_| anyhow::anyhow!("salt randomness"))?;
        rng.fill(&mut cancel_secret)
            .map_err(|_| anyhow::anyhow!("cancel randomness"))?;
        let prepared = Prepared::new_with_license(
            &self.config,
            Material {
                problem: id,
                payee,
                salt,
                cancel_secret,
                raw_len: raw.len() as u64,
                export_digest: digest(&raw),
                compressed,
                packages,
                format,
            },
            options.license_id,
        )?;
        let commit = prepared.commit;
        let sub = Submission {
            schema_version: 2,
            config_digest: self.config.digest(),
            prepared,
            cancel_requested: false,
            local_check: local_check.into(),
            sent: BTreeMap::new(),
            last_error: None,
            observed: None,
            prune_height: None,
            precheck_gate,
            precheck_override: options.allow_stale_gate,
        };
        self.save(&sub)?;
        self.submission(commit)
    }
    fn save(&mut self, sub: &Submission) -> Result<()> {
        self.store
            .write(&submission_name(sub.prepared.commit), sub)?;
        self.submissions.insert(sub.prepared.commit, sub.clone());
        Ok(())
    }
    fn check_payee(&mut self, payee: [u8; 20]) -> Result<()> {
        // Pure legacy M2 harnesses have no L2 endpoint. M3 always requires one.
        if self.l2.is_none() && self.config.m3.is_none() {
            return Ok(());
        }
        let node = self.l2.as_mut().context("L2 payee code probe required")?;
        let code = retry_rpc(|| node.code(payee))
            .map_err(|_| anyhow::anyhow!("L2 payee code probe unavailable; COMMIT refused"))?;
        ensure!(code.is_empty(), "payee has L2 code; COMMIT refused");
        Ok(())
    }
    fn stale_gate(
        &self,
        declared: Option<Hash>,
        next_commit: u64,
        parts: usize,
    ) -> Result<Option<StaleGate>> {
        let Some(profile) = &self.config.m3 else {
            return Ok(None);
        };
        let avail = next_commit
            .checked_add(self.config.d)
            .and_then(|h| h.checked_add((parts as u64).div_ceil(self.config.p_blk)))
            .context("estimated availability overflow")?;
        let height = avail
            .checked_add(profile.k)
            .context("estimated due overflow")?;
        let forks = &self.chain.as_ref().context("M3 chain")?.forks;
        let epoch = forks.job_epoch(avail, profile.k)?;
        let expected = forks.rows[epoch as usize].gate_rule_digest;
        Ok((declared != Some(expected)).then_some(StaleGate {
            declared,
            expected,
            estimated_earliest_due_height: height,
        }))
    }
    pub fn submission(&self, id: Hash) -> Result<Value> {
        let sub = self.submissions.get(&id).context("submission not found")?;
        let license_id = sub.prepared.license_id()?;
        let terms_digest = Prepared::license_terms(&self.config, license_id)?;
        let candidate = self.state.candidates.get(&(sub.prepared.problem, id));
        let status = candidate
            .map(|c| SubmissionState::from_candidate(c.status()))
            .or_else(|| sub.observed.as_ref().map(|o| o.state))
            .unwrap_or(SubmissionState::CommitPending);
        let open = candidate
            .and_then(|c| c.live())
            .and_then(|c| c.open.as_ref());
        let complete = open
            .map(|o| o.bitmap.iter().map(|b| b.count_ones()).sum::<u32>())
            .unwrap_or(0);
        let mut view = json!({"schema_version":sub.schema_version,"submission_id":hex_string(&id),"problem_id":hex_string(&sub.prepared.problem),"status":status,"cancel_requested":sub.cancel_requested,"local_check":sub.local_check,"precheck_gate_rule_digest":sub.precheck_gate.map(|h|hex_string(&h)),"allow_stale_gate":sub.precheck_override,"license_id":license_id,"terms_digest":hex_string(&terms_digest),"finalized_parts":complete,"parts":sub.prepared.chunks.len(),"transactions":sub.sent.iter().map(|(stage,s)|json!({"stage":stage,"transaction_hash":s.tx,"failure":s.failure})).collect::<Vec<_>>(),"last_error":sub.last_error,"last_observed_state":sub.observed,"prune_height":sub.prune_height,"fold_height":self.state.height()});
        view.as_object_mut()
            .expect("submission view")
            .extend(crate::m3_status::submission(
                &self.state,
                &self.config,
                (sub.prepared.problem, id),
            )?);
        Ok(view)
    }
    pub fn rewards(&mut self, payee: [u8; 20]) -> Result<Value> {
        let entries: Vec<_> = self
            .state
            .m3
            .as_ref()
            .map(|m| {
                m.rewards
                    .values()
                    .filter(|r| r.solver == payee)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let mut views = vec![];
        let mut balances = BTreeMap::new();
        for reward in entries {
            let mut view = crate::m3_status::reward(&self.config, &reward);
            let vested = self.vested_at.get(&reward.id).copied();
            view["vested_at"] = json!(vested);
            view["balance_before"] = Value::Null;
            view["balance_after"] = Value::Null;
            view["balance_change"] = Value::Null;
            view["balance_change_scope"] = json!("payee-total-in-block");
            if let Some(h) = vested {
                if let std::collections::btree_map::Entry::Vacant(entry) = balances.entry(h) {
                    let observation = (|| -> Result<(Hash, Hash)> {
                        ensure!(
                            self.stalled.is_none(),
                            "stalled client cannot refresh balance observations"
                        );
                        let node = self.l2.as_mut().context("L2 balance source")?;
                        let before = retry_rpc(|| {
                            node.balance(payee, h.checked_sub(1).context("vest height")?)
                        })?;
                        let after = retry_rpc(|| node.balance(payee, h))?;
                        Ok((before, after))
                    })()
                    .ok();
                    entry.insert(observation);
                }
                if let Some((before, after)) = balances[&h] {
                    view["balance_before"] = json!(crate::m3_status::word_hex(&before));
                    view["balance_after"] = json!(crate::m3_status::word_hex(&after));
                    view["balance_change"] =
                        json!(crate::m3_status::balance_delta(&before, &after));
                    view["balance_status"] = json!("observed");
                } else {
                    view["balance_status"] = json!("unavailable");
                }
            } else {
                view["balance_status"] = json!("not-vested");
            }
            views.push(view);
        }
        Ok(
            json!({"payee":hex_string(&payee),"fold_height":self.state.height(),"rewards":views,"stalled":self.stalled,"l2_m2_root_match":self.l2_m2_root_match}),
        )
    }
    pub fn cancel(&mut self, id: Hash) -> Result<Value> {
        self.sync()?;
        let mut sub = self
            .submissions
            .get(&id)
            .context("submission not found")?
            .clone();
        if let Some(c) = self.state.candidates.get(&(sub.prepared.problem, id)) {
            ensure!(
                matches!(
                    c.status(),
                    Status::Committed | Status::Open | Status::Cancelled
                ),
                "candidate can no longer cancel"
            );
        }
        sub.cancel_requested = true;
        self.save(&sub)?;
        self.observe_submissions()?;
        self.submission(id)
    }
    pub fn tick(&mut self) -> Result<()> {
        self.sync()?;
        ensure!(
            self.l2.is_none() || self.l2_m2_root_match == Some(true),
            "waiting for the L2 root at the finalized fold height"
        );
        let head = retry_rpc(|| self.poster.head())?;
        ensure!(
            head >= self.state.height(),
            "posting head behind finalized fold"
        );
        if self.budget_head != Some(head) {
            self.budget_head = Some(head);
            self.posted = self
                .submissions
                .values()
                .flat_map(|s| s.sent.values())
                .filter(|s| s.head == head)
                .count() as u64;
        }
        let ids: Vec<_> = self.submissions.keys().copied().collect();
        for id in ids {
            if self.advance(id, head).is_err() {
                let mut sub = self.submissions[&id].clone();
                sub.last_error = Some("submission progress read failed; retry pending".into());
                self.save(&sub)?;
            }
        }
        Ok(())
    }
    fn advance(&mut self, id: Hash, head: u64) -> Result<()> {
        let mut sub = self.submissions[&id].clone();
        let p = sub.prepared.problem;
        let candidate = self.state.candidates.get(&(p, id)).cloned();
        let Some(candidate) = candidate else {
            if sub.cancel_requested || sub.observed.as_ref().is_some_and(|o| o.state.final_state())
            {
                return Ok(());
            }
            let Some(problem) = self.state.problems.get(&p) else {
                return Ok(());
            };
            let deadline = problem
                .h_v
                .checked_add(self.config.slot_table[problem.slot as usize].solve_deadline)
                .context("commit deadline overflow")?;
            if problem.closed.is_none() && head.checked_add(1).is_some_and(|n| n <= deadline) {
                if self.check_payee(sub.prepared.payee()?).is_err() {
                    sub.last_error = Some(
                        "COMMIT refused: payee has L2 code or its code probe is unavailable".into(),
                    );
                    self.save(&sub)?;
                    return Ok(());
                }
                if sub.local_check != "not_run" {
                    if self
                        .stale_gate(
                            sub.precheck_gate,
                            head.checked_add(1).context("COMMIT height overflow")?,
                            sub.prepared.chunks.len(),
                        )?
                        .is_some()
                    {
                        sub.local_check = "stale-gate".into();
                        if !sub.precheck_override {
                            sub.last_error=Some("local_check: stale-gate; COMMIT refused at the current estimated epoch".into());
                            self.save(&sub)?;
                            return Ok(());
                        }
                    } else {
                        sub.local_check = "local_accept".into();
                    }
                    self.save(&sub)?;
                }
                let body = sub.prepared.commit_body(self.account);
                self.send(&mut sub, "commit", 2, body, head)?;
            }
            return Ok(());
        };
        if SubmissionState::from_candidate(candidate.status()).final_state() {
            return Ok(());
        }
        let live = candidate.live().context("live candidate missing")?;
        let next = head.checked_add(1).context("post height overflow")?;
        let end = live
            .h_c
            .checked_add(self.config.w)
            .context("reveal deadline overflow")?;
        if sub.cancel_requested {
            if next <= end {
                let body = sub.prepared.cancel();
                self.send(&mut sub, "cancel", 7, body, head)?;
            }
            return Ok(());
        }
        if next > end
            || next
                < live
                    .h_c
                    .checked_add(self.config.d)
                    .context("minimum reveal overflow")?
        {
            return Ok(());
        }
        let mut open_included = candidate.status() == Status::Open;
        if !open_included {
            open_included = self
                .receipt(&mut sub, "open")?
                .is_some_and(|h| h > self.state.height());
            if !open_included {
                let reserve = (sub.prepared.chunks.len() as u64)
                    .div_ceil(self.config.p_blk)
                    .checked_add(2)
                    .context("posting reserve overflow")?;
                if next <= end.saturating_sub(reserve) {
                    let body = sub.prepared.open(self.account);
                    self.send(&mut sub, "open", 3, body, head)?;
                }
                return Ok(());
            }
        }
        let bitmap = live.open.as_ref().map(|o| o.bitmap).unwrap_or([0; 2]);
        for i in 0..sub.prepared.chunks.len() {
            if bitmap[i / 64] & (1u64 << (i % 64)) != 0 {
                continue;
            }
            let body = sub.prepared.part(self.account, i as u16)?;
            self.send(&mut sub, &format!("part:{i}"), 4, body, head)?;
            if self.posted >= self.config.p_blk {
                break;
            }
        }
        Ok(())
    }
    fn send(
        &mut self,
        sub: &mut Submission,
        stage: &str,
        kind: u8,
        body: Vec<u8>,
        head: u64,
    ) -> Result<()> {
        if self.posted >= self.config.p_blk {
            return Ok(());
        }
        if let Some(sent) = sub.sent.get(stage) {
            if sent.head == head {
                return Ok(());
            }
            let old_head = sent.head;
            match self.receipt(sub, stage)? {
                Some(h) if h > self.state.height() => return Ok(()),
                None if head <= old_head.saturating_add(1) => return Ok(()),
                _ => {}
            }
        }
        sub.sent.insert(
            stage.into(),
            Sent {
                head,
                tx: None,
                failure: None,
            },
        );
        self.save(sub)?;
        self.posted += 1;
        match self.poster.post(kind, &body) {
            Ok(tx) => {
                sub.sent
                    .get_mut(stage)
                    .context("posting intent missing")?
                    .tx = Some(tx);
                sub.last_error = None;
            }
            Err(_) => {
                sub.last_error =
                    Some("posting outcome unknown; waiting to reconcile this exact item".into());
            }
        }
        self.save(sub)?;
        Ok(())
    }
    fn receipt(&mut self, sub: &mut Submission, stage: &str) -> Result<Option<u64>> {
        let Some(sent) = sub.sent.get(stage) else {
            return Ok(None);
        };
        if sent.failure.is_some() {
            return Ok(None);
        }
        let Some(tx) = sent.tx.as_deref() else {
            return Ok(None);
        };
        match retry_rpc(|| self.poster.included(tx)) {
            Err(e) if e.is::<RevertedReceipt>() => {
                sub.sent
                    .get_mut(stage)
                    .context("posting step missing")?
                    .failure = Some("REVERTED".into());
                sub.last_error = Some(format!(
                    "{stage} transaction reverted; retry only within the M2 window"
                ));
                self.save(sub)?;
                Ok(None)
            }
            result => result,
        }
    }
    pub fn call(&mut self, name: &str, args: &Value) -> Result<Value> {
        if name == "boole.devnet_status" {
            let _ = self.sync();
            return Ok(self.status());
        }
        // Durable progress remains inspectable when a bad finalized input stalls.
        if !matches!(name, "boole.devnet_submission" | "boole.devnet_rewards") {
            self.sync()?;
        } else {
            let _ = self.sync();
        }
        let text = |key: &str| args[key].as_str().with_context(|| format!("missing {key}"));
        match name {
            "boole.devnet_status" => Ok(self.status()),
            "boole.devnet_problem" => {
                let id = if let Some(id) = args["problem_id"].as_str() {
                    fixed(id)?
                } else {
                    let slot = args["slot"]
                        .as_u64()
                        .context("missing slot or problem_id")?;
                    let slot = usize::try_from(slot)?;
                    self.state
                        .slots
                        .get(slot)
                        .and_then(|s| s.active)
                        .and_then(|k| self.state.rounds.get(&k))
                        .and_then(|r| r.problem)
                        .context("slot has no open problem")?
                };
                self.problem(id)
            }
            "boole.devnet_commit" => self.commit_with_options(
                Path::new(text("export_file")?),
                fixed(text("payee")?)?,
                fixed(text("problem_id")?)?,
                CommitOptions {
                    allow_stale_gate: args
                        .get("allow_stale_gate")
                        .map(|v| v.as_bool().context("allow_stale_gate must be boolean"))
                        .transpose()?
                        .unwrap_or(false),
                    license_id: args
                        .get("license_id")
                        .map(|v| {
                            v.as_u64()
                                .and_then(|n| u8::try_from(n).ok())
                                .context("license_id must be a byte id")
                        })
                        .transpose()?
                        .unwrap_or(1),
                },
            ),
            "boole.devnet_submission" => self.submission(fixed(text("submission_id")?)?),
            "boole.devnet_cancel" => self.cancel(fixed(text("submission_id")?)?),
            "boole.devnet_rewards" => self.rewards(fixed(text("payee")?)?),
            _ => bail!("unknown devnet tool"),
        }
    }
}
fn submission_name(id: Hash) -> String {
    format!("submission-{}.json", hex_string(&id))
}
pub(crate) fn validated_inputs(block: &L1Block, mut logs: Vec<InboxLog>) -> Result<Vec<Input>> {
    logs.sort_by_key(|l| (l.transaction_index, l.log_index));
    ensure!(
        !logs
            .windows(2)
            .any(|p| (p[0].transaction_index, p[0].log_index)
                == (p[1].transaction_index, p[1].log_index)),
        "duplicate inbox log"
    );
    let inbox = fixed::<20>(boole_derivation::config::INBOX)?;
    let topic = fixed::<32>(&boole_derivation::config::posted_topic())?;
    let block_hash = fixed::<32>(&block.hash)?;
    let mut inputs = vec![];
    for log in logs {
        ensure!(
            !log.removed
                && fixed::<20>(&log.address)? == inbox
                && fixed::<32>(&log.block_hash)? == block_hash
                && log.topics.len() == 1
                && fixed::<32>(&log.topics[0])? == topic,
            "inconsistent inbox log"
        );
        let bytes = hex_bytes(&log.data, "inbox data")?;
        if let Ok((kind, body)) = boole_derivation::inbox::item(&bytes) {
            if (2..=15).contains(&kind) {
                inputs.push(Input {
                    transaction_index: log.transaction_index,
                    log_index: log.log_index,
                    kind,
                    body: body.to_vec(),
                });
            }
        }
    }
    Ok(inputs)
}
