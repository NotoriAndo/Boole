use anyhow::{bail, Result};
use boole_derivation::{
    attributes::{hex_bytes, hex_string},
    m2::*,
    InboxLog, L1Block,
};
use boole_devnet_driver::l1::L1Source;
use boole_devnet_mcp::{
    client::{Client, ClientOptions, Poster},
    codec::Scratch,
    store::Store,
};
use serde_json::{json, Value};

struct EmptySource;
impl L1Source for EmptySource {
    fn finalized(&mut self) -> Result<u64> {
        Ok(0)
    }
    fn genesis_hash(&mut self) -> Result<String> {
        Ok(hex_string(&[1; 32]))
    }
    fn block(&mut self, _: u64) -> Result<L1Block> {
        bail!("no block")
    }
    fn inbox_logs(&mut self, _: &str) -> Result<Vec<InboxLog>> {
        bail!("no logs")
    }
    fn validate_m2_inbox(&mut self) -> Result<()> {
        Ok(())
    }
}
struct NoPosts;
impl Poster for NoPosts {
    fn head(&mut self) -> Result<u64> {
        Ok(0)
    }
    fn post(&mut self, _: u8, _: &[u8]) -> Result<String> {
        panic!("unexpected post")
    }
    fn included(&mut self, _: &str) -> Result<Option<u64>> {
        panic!("unexpected receipt")
    }
}

#[test]
fn t64_v1_private_store_is_refused_with_an_explicit_schema_error() -> Result<()> {
    let scratch = Scratch::new(&std::env::temp_dir())?;
    let path = scratch.0.join("client");
    let config = Config::devnet();
    let store = Store::open(path.clone())?;
    // This is exactly the pre-M3 store identity, without a schema tag.
    store.write(
        "identity.json",
        &json!({
            "config_digest":hex_string(&config.digest()),
            "l1_genesis_hash":hex_string(&[1;32]),
            "l2_gas_limit":30_000_000
        }),
    )?;
    drop(store);
    let result = Client::new(
        EmptySource,
        NoPosts,
        config,
        [2; 20],
        path,
        ClientOptions::new(30_000_000),
    );
    let error = result.err().expect("a v1 store was silently opened");
    assert!(error.to_string().contains("v1 store"), "{error}");
    Ok(())
}

#[derive(Clone)]
struct TapeSource {
    rows: Vec<Value>,
    finalized: u64,
}
impl L1Source for TapeSource {
    fn finalized(&mut self) -> Result<u64> {
        Ok(self.finalized)
    }
    fn genesis_hash(&mut self) -> Result<String> {
        Ok(self.rows[0]["l1"]["parentHash"].as_str().unwrap().into())
    }
    fn validate_m2_inbox(&mut self) -> Result<()> {
        Ok(())
    }
    fn block(&mut self, h: u64) -> Result<L1Block> {
        Ok(serde_json::from_value(
            self.rows[h as usize - 1]["l1"].clone(),
        )?)
    }
    fn inbox_logs(&mut self, hash: &str) -> Result<Vec<InboxLog>> {
        Ok(serde_json::from_value(
            self.rows.iter().find(|r| r["l1"]["hash"] == hash).unwrap()["logs"].clone(),
        )?)
    }
}
fn round_tape() -> Result<(Config, TapeSource)> {
    let v: Value =
        serde_json::from_str(include_str!("../../../fixtures/devnet-m3/round-v1.3.json"))?;
    Ok((
        serde_json::from_str(v["config_json"].as_str().unwrap())?,
        TapeSource {
            rows: v["heights"].as_array().unwrap().clone(),
            finalized: 1,
        },
    ))
}
struct MalformedNode {
    root: Hash,
    block: boole_devnet_mcp::client::CanonicalBlock,
}
impl boole_devnet_mcp::client::L2Source for MalformedNode {
    fn root(&mut self, _: u64, _: &str) -> Result<Option<Hash>> {
        Ok(Some(self.root))
    }
    fn block(&mut self, _: u64) -> Result<Option<boole_devnet_mcp::client::CanonicalBlock>> {
        Ok(Some(self.block.clone()))
    }
}
#[test]
fn t64_malformed_verdict_is_not_ignored_even_with_a_matching_m2root() -> Result<()> {
    let (mut c, source) = round_tape()?;
    // The previous client only understands the M2 profile. Its valid M2ROOT
    // must not authorize an unchecked malformed M3 transaction after it.
    c.m3 = None;
    let l1: L1Block = serde_json::from_value(source.rows[0]["l1"].clone())?;
    let logs: Vec<InboxLog> = serde_json::from_value(source.rows[0]["logs"].clone())?;
    let mut inputs = vec![];
    for log in logs {
        let bytes = hex_bytes(&log.data, "log")?;
        let (kind, body) = boole_derivation::inbox::item(&bytes).map_err(anyhow::Error::msg)?;
        inputs.push(Input {
            transaction_index: log.transaction_index,
            log_index: log.log_index,
            kind,
            body: body.into(),
        });
    }
    let root = State::new(&c, 30_000_000)?
        .step(&c, &l1, &inputs)?
        .state
        .root;
    let mut bad = source.rows[25]["result"]["transactions"][2]
        .as_str()
        .unwrap()
        .to_string();
    bad.truncate(bad.len() - 2);
    let block = boole_devnet_mcp::client::CanonicalBlock {
        number: 1,
        hash: [2; 32],
        parent_hash: [1; 32],
        transactions: vec![
            boole_derivation::system::l1_info(&l1)?,
            boole_derivation::system::m2_root_deposit(&l1, root)?,
            bad,
        ],
    };
    let scratch = Scratch::new(&std::env::temp_dir())?;
    let mut options = ClientOptions::new(30_000_000);
    options.l2 = Some(Box::new(MalformedNode { root, block }));
    let mut client = Client::new(
        source,
        NoPosts,
        c,
        [2; 20],
        scratch.0.join("client"),
        options,
    )?;
    assert!(
        client.sync().is_err(),
        "malformed VERDICT was silently accepted with a matching root"
    );
    assert_eq!(client.status()["l2_m2_root_match"], false);
    assert!(
        client.tick().is_err(),
        "posting must stay stopped after malformed deposit"
    );
    Ok(())
}

#[derive(Clone)]
struct TapeNode {
    rows: Vec<Value>,
}
impl boole_devnet_mcp::client::L2Source for TapeNode {
    fn root(&mut self, h: u64, _: &str) -> Result<Option<Hash>> {
        Ok(Some(boole_devnet_mcp::client::fixed(
            self.rows[h as usize - 1]["result"]["m2_root"]
                .as_str()
                .unwrap(),
        )?))
    }
    fn block(&mut self, h: u64) -> Result<Option<boole_devnet_mcp::client::CanonicalBlock>> {
        use boole_devnet_mcp::client::{fixed, CanonicalBlock};
        if h == 0 {
            return Ok(Some(CanonicalBlock {
                number: 0,
                hash: fixed(self.rows[0]["parent"]["hash"].as_str().unwrap())?,
                parent_hash: [0; 32],
                transactions: vec![],
            }));
        }
        let Some(row) = self.rows.get(h as usize - 1) else {
            return Ok(None);
        };
        Ok(Some(CanonicalBlock {
            number: h,
            hash: fixed(row["result"]["block_hash"].as_str().unwrap())?,
            parent_hash: fixed(row["parent"]["hash"].as_str().unwrap())?,
            transactions: serde_json::from_value(row["result"]["transactions"].clone())?,
        }))
    }
}
#[test]
fn c1_client_follows_real_m3_verdicts_and_rewards_through_vest() -> Result<()> {
    let (c, source) = round_tape()?;
    let node = TapeNode {
        rows: source.rows.clone(),
    };
    let scratch = Scratch::new(&std::env::temp_dir())?;
    let mut options = ClientOptions::new(30_000_000);
    options.l2 = Some(Box::new(node));
    let rows = source.rows.clone();
    let mut client = Client::new(
        source,
        NoPosts,
        c,
        [2; 20],
        scratch.0.join("client"),
        options,
    )?;
    for row in &rows {
        let height = row["height"].as_u64().unwrap();
        client.source.finalized = height;
        client.sync()?;
        assert_eq!(client.status()["fold_height"], height);
        assert_eq!(
            client.status()["m2_root"],
            row["result"]["m2_root"],
            "height {height}"
        );
        assert_eq!(client.status()["l2_m2_root_match"], true, "height {height}");
    }
    let rewards = &client.m2_state().m3.as_ref().unwrap().rewards;
    assert_eq!(rewards.len(), 1);
    assert_eq!(
        rewards[&0].status,
        boole_derivation::m3::RewardStatus::Vested
    );
    Ok(())
}

#[test]
fn c1_real_fork_and_completion_reorg_tapes_match_every_driver_root() -> Result<()> {
    for name in [
        "kernel-fix",
        "quarantine-void",
        "quarantine-recheck",
        "reorg",
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/devnet-m3")
            .join(format!("{name}-v1.3.json"));
        let v: Value = serde_json::from_slice(&std::fs::read(path)?)?;
        let c: Config = serde_json::from_str(v["config_json"].as_str().unwrap())?;
        let rows = v["heights"].as_array().unwrap().clone();
        let source = TapeSource {
            rows: rows.clone(),
            finalized: 0,
        };
        let mut options = ClientOptions::new(30_000_000);
        options.l2 = Some(Box::new(TapeNode { rows: rows.clone() }));
        options.forks = Some(serde_json::from_value(v["forks"].clone())?);
        let scratch = Scratch::new(&std::env::temp_dir())?;
        let mut client = Client::new(
            source,
            NoPosts,
            c,
            [2; 20],
            scratch.0.join("client"),
            options,
        )?;
        for row in rows {
            let h = row["height"].as_u64().unwrap();
            client.source.finalized = h;
            client.sync()?;
            assert_eq!(client.status()["fold_height"], h, "{name}");
            assert_eq!(
                client.status()["m2_root"],
                row["result"]["m2_root"],
                "{name} height{h}"
            );
            assert_eq!(client.status()["l2_m2_root_match"], true);
        }
    }
    Ok(())
}

fn tape_inputs(row: &Value) -> Result<Vec<Input>> {
    let logs: Vec<InboxLog> = serde_json::from_value(row["logs"].clone())?;
    let mut out = vec![];
    for log in logs {
        let bytes = hex_bytes(&log.data, "log")?;
        let (kind, body) = boole_derivation::inbox::item(&bytes).map_err(anyhow::Error::msg)?;
        if (2..=15).contains(&kind) {
            out.push(Input {
                transaction_index: log.transaction_index,
                log_index: log.log_index,
                kind,
                body: body.into(),
            });
        }
    }
    Ok(out)
}
#[test]
fn c1_fuel_lookahead_waits_without_posting_then_recomputes_the_later_due() -> Result<()> {
    use boole_derivation::m3::*;
    let (mut c, mut source) = round_tape()?;
    c.m3.as_mut().unwrap().cost_fixed = 10_000;
    let forks = ForkTable::genesis(&c)?;
    let original_l1: L1Block = serde_json::from_value(source.rows[25]["l1"].clone())?;
    let records = decode_verdict_deposits(
        &c,
        &original_l1,
        &[source.rows[25]["result"]["transactions"][2]
            .as_str()
            .unwrap()
            .into()],
    )?;
    let mut state = State::new(&c, 30_000_000)?;
    let mut parent = source.rows[0]["parent"]["hash"].clone();
    let mut due = 0;
    for row in &mut source.rows {
        let l1: L1Block = serde_json::from_value(row["l1"].clone())?;
        let supplied =
            if state.m3.as_ref().unwrap().queue.len() > state.m3.as_ref().unwrap().next_applied {
                records.as_slice()
            } else {
                &[]
            };
        let step = state.step_with_verdicts(&c, &l1, &tape_inputs(row)?, supplied, &forks)?;
        let mut txs = vec![
            boole_derivation::system::l1_info(&l1)?,
            boole_derivation::system::m2_root_deposit(&l1, step.state.root)?,
        ];
        txs.extend(verdict_deposits(&c, &l1, &step.verdicts)?);
        if !step.verdicts.is_empty() {
            due = l1.number;
        }
        for p in &step.payments {
            txs.push(reward_deposit(&c, &l1, p)?);
        }
        row["parent"]["hash"] = parent;
        row["result"]["m2_root"] = json!(hex_string(&step.state.root));
        row["result"]["block_hash"] = json!(hex_string(&step.state.root));
        row["result"]["transactions"] = json!(txs);
        parent = row["result"]["block_hash"].clone();
        state = step.state;
    }
    assert!(due > 26);
    let node = TapeNode {
        rows: source.rows.clone(),
    };
    let expected = source.rows[(due - 1) as usize]["result"]["m2_root"].clone();
    source.finalized = 26;
    let scratch = Scratch::new(&std::env::temp_dir())?;
    let mut options = ClientOptions::new(30_000_000);
    options.l2 = Some(Box::new(node));
    let mut client = Client::new(
        source,
        NoPosts,
        c,
        [2; 20],
        scratch.0.join("client"),
        options,
    )?;
    client.sync()?;
    assert_eq!(client.status()["fold_height"], 25);
    assert!(client.status()["l2_m2_root_match"].is_null());
    assert!(client.status()["stalled"].is_null());
    assert!(client
        .tick()
        .unwrap_err()
        .to_string()
        .contains("waiting for the L2 root"));
    client.source.finalized = due;
    client.sync()?;
    assert_eq!(client.status()["fold_height"], due);
    assert_eq!(client.status()["m2_root"], expected);
    assert_eq!(client.status()["l2_m2_root_match"], true);
    Ok(())
}

struct CodeNode {
    inner: TapeNode,
    code: Vec<u8>,
}
impl boole_devnet_mcp::client::L2Source for CodeNode {
    fn root(&mut self, h: u64, o: &str) -> Result<Option<Hash>> {
        self.inner.root(h, o)
    }
    fn block(&mut self, h: u64) -> Result<Option<boole_devnet_mcp::client::CanonicalBlock>> {
        self.inner.block(h)
    }
    fn code(&mut self, _: [u8; 20]) -> Result<Vec<u8>> {
        Ok(self.code.clone())
    }
}
#[derive(Default)]
struct CountingPoster {
    head: u64,
    posts: Vec<u8>,
}
impl Poster for CountingPoster {
    fn head(&mut self) -> Result<u64> {
        Ok(self.head)
    }
    fn post(&mut self, kind: u8, _: &[u8]) -> Result<String> {
        self.posts.push(kind);
        Ok(hex_string(&[self.posts.len() as u8; 32]))
    }
    fn included(&mut self, _: &str) -> Result<Option<u64>> {
        Ok(None)
    }
}
#[test]
fn t64_code_payee_is_refused_before_any_commit_is_posted() -> Result<()> {
    use boole_devnet_mcp::codec::{private_file, Codec};
    let (c, mut source) = round_tape()?;
    source.finalized = 5;
    let node = CodeNode {
        inner: TapeNode {
            rows: source.rows.clone(),
        },
        code: vec![0x60, 0],
    };
    let scratch = Scratch::new(&std::env::temp_dir())?;
    let mut options = ClientOptions::new(30_000_000);
    options.l2 = Some(Box::new(node));
    options.codec = Some(Codec::portable_stub());
    let mut client = Client::new(
        source,
        CountingPoster {
            head: 5,
            ..Default::default()
        },
        c,
        [2; 20],
        scratch.0.join("client"),
        options,
    )?;
    client.sync()?;
    let problem = client.m2_state().rounds[&(0, 1)].problem.unwrap();
    let export = scratch.0.join("export");
    private_file(&export, &boole_gate_supervisor::stub::export(0, 1))?;
    let result = client.commit(&export, [6; 20], problem);
    if result.is_ok() {
        client.tick()?;
    }
    assert!(
        result.is_err(),
        "a code payee was accepted and its COMMIT posted"
    );
    assert!(client.poster.posts.is_empty());
    assert!(client.submission([0; 32]).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn t64_stale_gate_local_accept_cannot_post_a_commit() -> Result<()> {
    use boole_devnet_mcp::{
        codec::{digest, private_file, Codec},
        hook::Precheck,
    };
    use std::{os::unix::fs::PermissionsExt, time::Duration};
    let (c, mut source) = round_tape()?;
    source.finalized = 5;
    let node = CodeNode {
        inner: TapeNode {
            rows: source.rows.clone(),
        },
        code: vec![],
    };
    let scratch = Scratch::new(&std::env::temp_dir())?;
    let hook = scratch.0.join("accept");
    let script = b"#!/bin/sh\nexit 0\n";
    private_file(&hook, script)?;
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o700))?;
    let mut options = ClientOptions::new(30_000_000);
    options.l2 = Some(Box::new(node));
    options.codec = Some(Codec::portable_stub());
    options.hook = Some(Precheck {
        executable: hook,
        sha256: digest(script),
        gate_rule_digest: Some([0xfe; 32]),
        timeout: Duration::from_secs(2),
    });
    let mut client = Client::new(
        source,
        CountingPoster {
            head: 5,
            ..Default::default()
        },
        c,
        [2; 20],
        scratch.0.join("client"),
        options,
    )?;
    client.sync()?;
    let problem = client.m2_state().rounds[&(0, 1)].problem.unwrap();
    let export = scratch.0.join("export");
    private_file(&export, &boole_gate_supervisor::stub::export(0, 1))?;
    let result = client.commit(&export, [6; 20], problem);
    if result.is_ok() {
        client.tick()?;
    }
    let error = result.expect_err("stale-gate local accept posted a COMMIT");
    let report: Value = serde_json::from_str(&error.to_string())?;
    assert_eq!(report["local_check"], "stale-gate");
    assert!(client.poster.posts.is_empty());
    Ok(())
}

#[test]
fn c7_closed_license_cannot_silently_commit_with_the_default_license() -> Result<()> {
    use boole_devnet_mcp::{
        codec::{private_file, Codec},
        protocol,
    };
    let (c, mut source) = round_tape()?;
    source.finalized = 5;
    let node = CodeNode {
        inner: TapeNode {
            rows: source.rows.clone(),
        },
        code: vec![],
    };
    let scratch = Scratch::new(&std::env::temp_dir())?;
    let mut options = ClientOptions::new(30_000_000);
    options.l2 = Some(Box::new(node));
    options.codec = Some(Codec::portable_stub());
    let mut client = Client::new(
        source,
        CountingPoster {
            head: 5,
            ..Default::default()
        },
        c,
        [2; 20],
        scratch.0.join("client"),
        options,
    )?;
    client.sync()?;
    let problem = client.m2_state().rounds[&(0, 1)].problem.unwrap();
    let export = scratch.0.join("export");
    private_file(&export, &boole_gate_supervisor::stub::export(0, 1))?;
    let request = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"boole.devnet_commit","arguments":{"export_file":export,"payee":hex_string(&[6;20]),"problem_id":hex_string(&problem),"license_id":255}}});
    let response = protocol::dispatch(&request.to_string(), &mut |n, a| client.call(n, a)).unwrap();
    if response["result"]["isError"] == false {
        client.tick()?;
    }
    assert_eq!(
        response["result"]["isError"], true,
        "closed license was silently replaced by the default and posted"
    );
    assert!(response["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("license"));
    assert!(client.poster.posts.is_empty());
    Ok(())
}

fn owned_prepared(
    c: &Config,
    source: &TapeSource,
    which: usize,
) -> Result<boole_devnet_mcp::wire::Prepared> {
    use boole_devnet_mcp::{
        codec::digest,
        wire::{Material, Prepared},
    };
    let mut items = vec![];
    for row in &source.rows {
        items.extend(tape_inputs(row)?);
    }
    let body = &items
        .iter()
        .filter(|i| i.kind == 3)
        .nth(which)
        .unwrap()
        .body[53..];
    let salt: Hash = body[53..85].try_into()?;
    let secret = digest(&[b"test cancel".as_slice(), &salt].concat());
    let problem = body[1..33].try_into()?;
    let commit = commit_hash(c.chain_id, problem, cancel_hash(&secret), body_digest(body));
    let q = 151 + 32 * usize::from(body[150]);
    let n = u16::from_be_bytes(body[q + 46..q + 48].try_into()?) as usize;
    let depth = n.next_power_of_two().trailing_zeros() as usize;
    let mut parts = std::collections::BTreeMap::new();
    for item in &items {
        if item.kind == 4 && item.body.get(21..53) == Some(commit.as_slice()) {
            let index = u16::from_be_bytes(item.body[53..55].try_into()?);
            parts
                .entry(index)
                .or_insert_with(|| item.body[55 + 32 * depth..].to_vec());
        }
    }
    let prepared = Prepared::new(
        c,
        Material {
            problem,
            payee: body[33..53].try_into()?,
            salt,
            cancel_secret: secret,
            raw_len: u64::from_be_bytes(body[q + 2..q + 10].try_into()?),
            export_digest: body[q + 10..q + 42].try_into()?,
            compressed: parts.into_values().flatten().collect(),
            packages: body[151..q]
                .chunks_exact(32)
                .map(|b| b.try_into().unwrap())
                .collect(),
            format: body[q + 1],
        },
    )?;
    assert_eq!(prepared.body, body);
    assert_eq!(prepared.commit, commit);
    Ok(prepared)
}
fn seed_owned_store(
    path: std::path::PathBuf,
    c: &Config,
    source: &TapeSource,
    prepared: &boole_devnet_mcp::wire::Prepared,
) -> Result<()> {
    let mut zero = source.clone();
    zero.finalized = 0;
    let mut options = ClientOptions::new(30_000_000);
    options.l2 = Some(Box::new(TapeNode {
        rows: source.rows.clone(),
    }));
    let initial = Client::new(zero, NoPosts, c.clone(), [2; 20], path.clone(), options)?;
    drop(initial);
    let store = Store::open(path)?;
    store.write(&format!("submission-{}.json",hex_string(&prepared.commit)),&json!({"schema_version":2,"config_digest":c.digest(),"prepared":prepared,"cancel_requested":false,"local_check":"not_run","sent":{},"last_error":null}))?;
    Ok(())
}
#[test]
fn c3_own_status_continues_from_checking_through_verdict_and_vest_without_reposting() -> Result<()>
{
    let (c, mut source) = round_tape()?;
    let prepared = owned_prepared(&c, &source, 0)?;
    let scratch = Scratch::new(&std::env::temp_dir())?;
    let path = scratch.0.join("client");
    seed_owned_store(path.clone(), &c, &source, &prepared)?;
    source.finalized = 26;
    let mut options = ClientOptions::new(30_000_000);
    options.l2 = Some(Box::new(CodeNode {
        inner: TapeNode {
            rows: source.rows.clone(),
        },
        code: vec![],
    }));
    let mut client = Client::new(
        source,
        CountingPoster {
            head: 26,
            ..Default::default()
        },
        c,
        [2; 20],
        path,
        options,
    )?;
    client.sync()?;
    let view = client.submission(prepared.commit)?;
    assert_eq!(view["queue_index"], 0);
    assert_eq!(view["earliest_due_height"], 26);
    assert_eq!(view["status"], "ACCEPTED");
    assert_eq!(view["verdict"]["status"], "ACCEPT");
    assert_eq!(view["verdict"]["reason"], 0);
    assert_eq!(view["verdict"]["fuel"], 777);
    assert_eq!(view["verdict"]["epoch"], 0);
    assert_eq!(view["earlier_candidates"], json!([]));
    assert_eq!(view["rewards"][0]["status"], "LOCKED");
    assert_eq!(view["rewards"][0]["amount"], "1000000000000000000000");
    assert_eq!(view["rewards"][0]["vest_h"], 90);
    assert!(
        client.cancel(prepared.commit).is_err(),
        "ACCEPTED candidate was reported as cancellable"
    );
    client.tick()?;
    assert!(
        client.poster.posts.is_empty(),
        "terminal M3 candidate reposted OPEN/parts"
    );
    client.source.finalized = 90;
    client.sync()?;
    let vested = client.submission(prepared.commit)?;
    assert_eq!(vested["rewards"][0]["status"], "VESTED");
    let output = format!("{view}{vested}");
    assert!(!output.contains(&hex_string(&prepared.body)));
    assert!(!output.contains(&hex_string(&prepared.body[53..85])));
    assert!(!output.contains(&hex_string(&prepared.cancel_secret)));
    Ok(())
}

struct BalanceNode {
    inner: TapeNode,
}
impl boole_devnet_mcp::client::L2Source for BalanceNode {
    fn root(&mut self, h: u64, o: &str) -> Result<Option<Hash>> {
        self.inner.root(h, o)
    }
    fn block(&mut self, h: u64) -> Result<Option<boole_devnet_mcp::client::CanonicalBlock>> {
        self.inner.block(h)
    }
    fn balance(&mut self, payee: [u8; 20], h: u64) -> Result<Hash> {
        assert_eq!(payee, [6; 20]);
        let mut word = [0; 32];
        if h >= 90 {
            word[16..].copy_from_slice(&1_000_000_000_000_000_000_000u128.to_be_bytes());
        }
        Ok(word)
    }
}
#[test]
fn c4_rewards_tool_reports_the_actual_vest_height_and_l2_balance_delta() -> Result<()> {
    let (c, mut source) = round_tape()?;
    source.finalized = 90;
    let scratch = Scratch::new(&std::env::temp_dir())?;
    let mut options = ClientOptions::new(30_000_000);
    options.l2 = Some(Box::new(BalanceNode {
        inner: TapeNode {
            rows: source.rows.clone(),
        },
    }));
    let mut client = Client::new(
        source,
        NoPosts,
        c,
        [2; 20],
        scratch.0.join("client"),
        options,
    )?;
    let view = client.call(
        "boole.devnet_rewards",
        &json!({"payee":hex_string(&[6;20])}),
    )?;
    assert_eq!(view["rewards"].as_array().unwrap().len(), 1);
    let reward = &view["rewards"][0];
    assert_eq!(reward["status"], "VESTED");
    assert_eq!(reward["vested_at"], 90);
    assert_eq!(reward["balance_before"], "0x0");
    assert_eq!(
        reward["balance_change"],
        format!("0x{:x}", 1_000_000_000_000_000_000_000u128)
    );
    assert_eq!(reward["balance_after"], reward["balance_change"]);
    assert_eq!(reward["balance_change_scope"], "payee-total-in-block");
    Ok(())
}

#[cfg(unix)]
#[test]
fn c5_matching_gate_and_explicit_override_are_distinct_from_stale_refusal() -> Result<()> {
    use boole_devnet_mcp::{
        client::CommitOptions,
        codec::{digest, private_file, Codec},
        hook::Precheck,
    };
    use std::{os::unix::fs::PermissionsExt, time::Duration};
    for (correct, override_pin, missing) in [
        (true, false, false),
        (false, true, false),
        (false, false, true),
    ] {
        let (c, mut source) = round_tape()?;
        source.finalized = 5;
        let current = c.m3.as_ref().unwrap().genesis_gate_rule_digest;
        let terms = c.terms_table[&1];
        let scratch = Scratch::new(&std::env::temp_dir())?;
        let executable = scratch.0.join("accept");
        let script = b"#!/bin/sh\nexit 0\n";
        private_file(&executable, script)?;
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))?;
        let mut options = ClientOptions::new(30_000_000);
        options.codec = Some(Codec::portable_stub());
        options.l2 = Some(Box::new(CodeNode {
            inner: TapeNode {
                rows: source.rows.clone(),
            },
            code: vec![],
        }));
        options.hook = Some(Precheck {
            executable,
            sha256: digest(script),
            gate_rule_digest: if missing {
                None
            } else {
                Some(if correct { current } else { [0xee; 32] })
            },
            timeout: Duration::from_secs(2),
        });
        let mut client = Client::new(
            source,
            CountingPoster {
                head: 5,
                ..Default::default()
            },
            c,
            [2; 20],
            scratch.0.join("client"),
            options,
        )?;
        client.sync()?;
        let problem = client.m2_state().rounds[&(0, 1)].problem.unwrap();
        let export = scratch.0.join("export");
        private_file(&export, &boole_gate_supervisor::stub::export(0, 1))?;
        let result = client.commit_with_options(
            &export,
            [6; 20],
            problem,
            CommitOptions {
                allow_stale_gate: override_pin,
                ..Default::default()
            },
        );
        if missing {
            assert!(result.unwrap_err().to_string().contains("stale-gate"));
            assert!(client.poster.posts.is_empty());
            continue;
        }
        let view = result?;
        assert_eq!(
            view["local_check"],
            if correct {
                "local_accept"
            } else {
                "stale-gate"
            }
        );
        assert_eq!(view["license_id"], 1);
        assert_eq!(view["terms_digest"], hex_string(&terms));
        client.tick()?;
        assert_eq!(client.poster.posts, vec![2]);
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn c5_commit_guard_rechecks_the_epoch_when_waiting_crosses_a_fork_boundary() -> Result<()> {
    use boole_derivation::m3::{ForkKind, ForkRow, ForkTable};
    use boole_devnet_mcp::{
        codec::{digest, private_file, Codec},
        hook::Precheck,
    };
    use std::{os::unix::fs::PermissionsExt, time::Duration};
    let (c, mut source) = round_tape()?;
    source.finalized = 5;
    let current = c.m3.as_ref().unwrap().genesis_gate_rule_digest;
    let mut forks = ForkTable::genesis(&c)?;
    let first_estimate = 6 + c.d + 1 + c.m3.as_ref().unwrap().k;
    forks.rows.push(ForkRow {
        epoch: 1,
        height: first_estimate + 1,
        gate_rule_digest: [0xdd; 32],
        kind: ForkKind::Policy,
        list: vec![],
    });
    let scratch = Scratch::new(&std::env::temp_dir())?;
    let executable = scratch.0.join("accept");
    let script = b"#!/bin/sh\nexit 0\n";
    private_file(&executable, script)?;
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))?;
    let mut options = ClientOptions::new(30_000_000);
    options.codec = Some(Codec::portable_stub());
    options.forks = Some(forks);
    options.l2 = Some(Box::new(CodeNode {
        inner: TapeNode {
            rows: source.rows.clone(),
        },
        code: vec![],
    }));
    options.hook = Some(Precheck {
        executable,
        sha256: digest(script),
        gate_rule_digest: Some(current),
        timeout: Duration::from_secs(2),
    });
    let mut client = Client::new(
        source,
        CountingPoster {
            head: 5,
            ..Default::default()
        },
        c,
        [2; 20],
        scratch.0.join("client"),
        options,
    )?;
    client.sync()?;
    let problem = client.m2_state().rounds[&(0, 1)].problem.unwrap();
    let export = scratch.0.join("export");
    private_file(&export, &boole_gate_supervisor::stub::export(0, 1))?;
    let initial = client.commit(&export, [6; 20], problem)?;
    assert_eq!(initial["local_check"], "local_accept");
    let id = boole_devnet_mcp::client::fixed(initial["submission_id"].as_str().unwrap())?;
    client.poster.head = 6;
    client.tick()?;
    assert!(client.poster.posts.is_empty());
    assert_eq!(client.submission(id)?["local_check"], "stale-gate");
    Ok(())
}

#[test]
fn c3_passed_candidate_reports_earlier_commit_barrier_instead_of_queue_order() -> Result<()> {
    let v: Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m3/quarantine-void-v1.3.json"
    ))?;
    let c: Config = serde_json::from_str(v["config_json"].as_str().unwrap())?;
    let source = TapeSource {
        rows: v["heights"].as_array().unwrap().clone(),
        finalized: 26,
    };
    let prepared = owned_prepared(&c, &source, 1)?;
    let scratch = Scratch::new(&std::env::temp_dir())?;
    let path = scratch.0.join("client");
    seed_owned_store(path.clone(), &c, &source, &prepared)?;
    let mut options = ClientOptions::new(30_000_000);
    options.forks = Some(serde_json::from_value(v["forks"].clone())?);
    options.l2 = Some(Box::new(TapeNode {
        rows: source.rows.clone(),
    }));
    let mut client = Client::new(
        source,
        CountingPoster {
            head: 26,
            ..Default::default()
        },
        c,
        [2; 20],
        path,
        options,
    )?;
    client.sync()?;
    let view = client.submission(prepared.commit)?;
    assert_eq!(view["queue_index"], 0);
    assert_eq!(view["status"], "PASSED");
    assert_eq!(view["earlier_candidates"].as_array().unwrap().len(), 1);
    assert_eq!(view["earlier_candidates"][0]["queue_index"], 1);
    assert_eq!(view["earlier_candidates"][0]["status"], "CHECKING");
    assert_eq!(view["rewards"], json!([]));
    client.tick()?;
    assert!(client.poster.posts.is_empty());
    client.source.finalized = 28;
    client.sync()?;
    assert_eq!(
        client.submission(prepared.commit)?["rewards"][0]["status"],
        "LOCKED"
    );
    Ok(())
}

struct PureOwnGate(boole_gate_supervisor::stub::Descriptor);
impl boole_devnet_mcp::own::OwnGate for PureOwnGate {
    fn descriptor(&self) -> &boole_gate_supervisor::stub::Descriptor {
        &self.0
    }
    fn validate(&self, _: u64, _: &std::path::Path) -> Result<()> {
        Ok(())
    }
    fn run(
        &self,
        input: &boole_gate_supervisor::GateInput,
        _: &std::path::Path,
    ) -> boole_gate_supervisor::GateReply {
        boole_gate_supervisor::stub::evaluate(&self.0, input)
    }
}
fn materialize_with_record(
    c: &Config,
    source: &mut TapeSource,
    records: &[boole_derivation::m3::IndexedVerdict],
) -> Result<()> {
    use boole_derivation::m3::*;
    let forks = ForkTable::genesis(c)?;
    let mut state = State::new(c, 30_000_000)?;
    let mut parent = source.rows[0]["parent"]["hash"].clone();
    for row in &mut source.rows {
        let l1: L1Block = serde_json::from_value(row["l1"].clone())?;
        let supplied =
            if state.m3.as_ref().unwrap().queue.len() > state.m3.as_ref().unwrap().next_applied {
                records
            } else {
                &[]
            };
        let step = state.step_with_verdicts(c, &l1, &tape_inputs(row)?, supplied, &forks)?;
        let mut txs = vec![
            boole_derivation::system::l1_info(&l1)?,
            boole_derivation::system::m2_root_deposit(&l1, step.state.root)?,
        ];
        txs.extend(verdict_deposits(c, &l1, &step.verdicts)?);
        for p in &step.payments {
            txs.push(reward_deposit(c, &l1, p)?);
        }
        row["parent"]["hash"] = parent;
        row["result"]["m2_root"] = json!(hex_string(&step.state.root));
        row["result"]["block_hash"] = json!(hex_string(&step.state.root));
        row["result"]["transactions"] = json!(txs);
        parent = row["result"]["block_hash"].clone();
        state = step.state;
    }
    Ok(())
}
fn verify_own_record_mutation(mutation: u8) -> Result<()> {
    use boole_derivation::m3::*;
    let (c, mut source) = round_tape()?;
    let prepared = owned_prepared(&c, &source, 0)?;
    let v: Value =
        serde_json::from_str(include_str!("../../../fixtures/devnet-m3/round-v1.3.json"))?;
    let descriptor = serde_json::from_value(v["gate_descriptors"][0].clone())?;
    let l1: L1Block = serde_json::from_value(source.rows[25]["l1"].clone())?;
    let mut records = decode_verdict_deposits(
        &c,
        &l1,
        &[source.rows[25]["result"]["transactions"][2]
            .as_str()
            .unwrap()
            .into()],
    )?;
    match mutation {
        0 => {
            records[0].record.outcome = Outcome::Reject;
            records[0].record.reason = 0x0301;
            records[0].record.vstmt_digest = [0xa5; 32];
        }
        1 => records[0].record.fuel += 1,
        2 => records[0].record.vstmt_digest[0] ^= 1,
        _ => unreachable!(),
    }
    materialize_with_record(&c, &mut source, &records)?;
    source.finalized = 100;
    let scratch = Scratch::new(&std::env::temp_dir())?;
    let mut options = ClientOptions::new(30_000_000);
    options.l2 = Some(Box::new(TapeNode {
        rows: source.rows.clone(),
    }));
    let mut trusted = Client::new(
        source.clone(),
        NoPosts,
        c.clone(),
        [2; 20],
        scratch.0.join("trusted"),
        options,
    )?;
    trusted.sync()?;
    assert_eq!(trusted.status()["l2_m2_root_match"], true);
    let path = scratch.0.join("owned");
    seed_owned_store(path.clone(), &c, &source, &prepared)?;
    let artifacts = scratch.0.join("artifacts");
    boole_gate_supervisor::supervisor::install_devnet_artifacts(&c, &artifacts)?;
    let mut options = ClientOptions::new(30_000_000);
    options.l2 = Some(Box::new(TapeNode {
        rows: source.rows.clone(),
    }));
    options.codec = Some(boole_devnet_mcp::codec::Codec::portable_stub());
    options.own = Some(boole_devnet_mcp::own::OwnVerification {
        gates: vec![Box::new(PureOwnGate(descriptor))],
        artifacts,
    });
    let mut verified = Client::new(source, NoPosts, c, [2; 20], path, options)?;
    assert!(
        verified.sync().is_err(),
        "verify-own trusted a forged own verdict"
    );
    assert_eq!(verified.status()["l2_m2_root_match"], false);
    assert!(verified.status()["stalled"]
        .as_str()
        .unwrap()
        .contains("verify-own mismatch"));
    assert!(verified.tick().is_err());
    Ok(())
}

#[test]
fn c2_verify_own_refuses_a_well_formed_forged_verdict_even_with_a_matching_root() -> Result<()> {
    verify_own_record_mutation(0)
}
#[test]
fn c2_verify_own_checks_fuel_even_when_the_verdict_statement_is_unchanged() -> Result<()> {
    verify_own_record_mutation(1)
}
#[test]
fn c2_verify_own_checks_the_verdict_statement_digest() -> Result<()> {
    verify_own_record_mutation(2)
}
