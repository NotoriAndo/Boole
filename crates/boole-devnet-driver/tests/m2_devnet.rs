//! I4–I7: pinned clients, independent replicas and finalized inbox scenarios.
#[allow(dead_code)]
mod support;
use anyhow::{bail, Context, Result};
use boole_derivation::{
    attributes::{hex_bytes, hex_string},
    m2::*,
    Derived, InboxLog, L1Block,
};
use boole_devnet_driver::{
    driver::{Driver, StepOutcome},
    engine::{Engine, Rpc, RpcEngine},
    l1::{L1Source, RecordedL1, RpcL1},
};
use serde_json::{json, Value};
use std::{fs, path::Path};
use support::m2::pool_leaf;
use support::{m2::*, *};
type Live = Driver<RpcL1, RpcEngine>;
type InboxItem = (u8, Vec<u8>);
type Posted = (Value, Vec<InboxItem>);
fn result(out: StepOutcome) -> Result<Derived> {
    match out {
        StepOutcome::Derived(d) => Ok(*d),
        _ => bail!("unexpected finality wait"),
    }
}
fn driver(node: &Node, rt: &Runtime, c: &Config) -> Result<Live> {
    let mut engine = node.engine()?;
    let l2_gas_limit = engine.head()?.gas_limit;
    let mut d = Driver::with_config(
        RpcL1::new_m2("http://127.0.0.1:28545")?,
        engine,
        c.clone(),
        l2_gas_limit,
    )?;
    d.set_evidence(&rt.path.join(format!("{}.jsonl", node.name)))?;
    d.set_journal(&rt.path.join(format!("{}-checkpoint.json", node.name)))?;
    Ok(d)
}
fn view(d: &mut Live, node: &Node, h: u64) -> Result<(Value, Derived)> {
    let out = result(d.step(h)?)?;
    let v = json!({"observed":observe(node,&out)?,"attributes":out.attributes,"decisions":out.decisions,"m2_root":hex_string(&d.m2_state().root)});
    Ok((v, out))
}
fn l1_genesis(rt: &Runtime) -> Result<std::path::PathBuf> {
    let g = l1_genesis_value()?;
    let p = rt.path.join("l1-genesis.json");
    fs::write(&p, serde_json::to_vec_pretty(&g)?)?;
    Ok(p)
}
fn source_row(l1: &Rpc, h: u64) -> Result<Value> {
    let b = l1.call("eth_getBlockByNumber", json!([format!("0x{h:x}"), false]))?;
    let logs=l1.call("eth_getLogs",json!([{"blockHash":b["hash"],"address":boole_derivation::config::INBOX,"topics":[boole_derivation::config::posted_topic()]}]))?;
    let header: L1Block = serde_json::from_value(b)?;
    Ok(json!({"height":h,"l1":header,"logs":logs}))
}
fn fold(model: &mut State, c: &Config, row: &Value) -> Result<Vec<u8>> {
    let l: L1Block = serde_json::from_value(row["l1"].clone())?;
    let logs: Vec<InboxLog> = serde_json::from_value(row["logs"].clone())?;
    let mut items = vec![];
    for log in logs {
        let bytes = hex_bytes(&log.data, "test log")?;
        if let Ok((kind, body)) = boole_derivation::inbox::item(&bytes) {
            if (2..=15).contains(&kind) {
                items.push(Input {
                    transaction_index: log.transaction_index,
                    log_index: log.log_index,
                    kind,
                    body: body.to_vec(),
                });
            }
        }
    }
    let step = model.step(c, &l, &items)?;
    *model = step.state;
    Ok(step.codes)
}
fn mine(l1: &Rpc, h: u64, posts: Vec<Posted>, gas: &mut Vec<Value>) -> Result<Value> {
    l1.call("evm_mine", json!([1_800_000_000 + 12 * h]))?;
    for (tx, items) in posts {
        let r = l1.call("eth_getTransactionReceipt", json!([tx]))?;
        assert_eq!(r["status"], "0x1", "post at {h}: {r}");
        assert_eq!(r["logs"].as_array().context("logs")?.len(), items.len());
        let transaction = l1.call("eth_getTransactionByHash", json!([tx]))?;
        let data = hex_bytes(
            transaction["input"].as_str().context("calldata")?,
            "calldata",
        )?;
        let tokens = data
            .iter()
            .map(|b| if *b == 0 { 1u64 } else { 4 })
            .sum::<u64>();
        let floor = 21000 + 10 * tokens;
        let used = quantity(&r["gasUsed"])?;
        gas.push(json!({"height":h,"kinds":items.iter().map(|x|x.0).collect::<Vec<_>>(),"items":items.len(),"gas_used":used,"calldata_floor":floor,"floor_applied":used>=floor,"transaction_hash":tx}));
    }
    source_row(l1, h)
}
fn send(
    l1: &Rpc,
    user: &str,
    items: Vec<(u8, Vec<u8>)>,
    batch: bool,
    posts: &mut Vec<Posted>,
) -> Result<()> {
    if !items.is_empty() {
        let tx = post(l1, user, &items, batch)?;
        posts.push((tx, items));
    }
    Ok(())
}
fn record_row(row: &mut Value, parent: boole_derivation::ParentHeader, v: Value) {
    row["parent"] = json!(parent);
    row["result"] = v;
}
fn recorded_abc(
    rt: &mut Runtime,
    binary: &Path,
    genesis: &Path,
    c: &Config,
    rows: &[Value],
) -> Result<()> {
    let text = serde_json::to_string(rows)?;
    for (index, name) in ["replay-a", "replay-b", "replay-c"].into_iter().enumerate() {
        let node = rt.node(binary, genesis, name, index as u16)?;
        let mut d = Driver::with_config(
            RecordedL1::from_json(&text)?,
            node.engine()?,
            c.clone(),
            30_000_000,
        )?;
        d.set_evidence(&rt.path.join(format!("{name}.jsonl")))?;
        for row in rows {
            let h = row["height"].as_u64().context("height")?;
            let out = result(d.step(h)?)?;
            let v = json!({"observed":observe(&node,&out)?,"attributes":out.attributes,"decisions":out.decisions,"m2_root":hex_string(&d.m2_state().root)});
            assert_eq!(v, row["result"], "recorded {name} height {h}");
        }
        rt.stop(name)?;
    }
    Ok(())
}

#[test]
fn i4_i5_live_and_recorded_abc_mid_reveal_restart_reorg() -> Result<()> {
    let Some(binary) = pinned("BOOLE_DEVNET_OP_RETH", "op_reth")? else {
        return Ok(());
    };
    let Some(anvil) = pinned("BOOLE_DEVNET_ANVIL", "anvil")? else {
        return Ok(());
    };
    let mut rt = Runtime::new("i4-i5")?;
    let l1 = rt.anvil_with_genesis(&anvil, &l1_genesis(&rt)?)?;
    let accounts = l1.call("eth_accounts", json!([]))?;
    let user = accounts[0].as_str().context("account")?;
    let c = config();
    let mut model = State::new(&c, 30_000_000)?;
    let mut rows = vec![];
    let mut gas = vec![];
    let (raw, z, codec) = frame(&rt.path)?;
    let package_fixture: Value = serde_json::from_slice(&fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/devnet-m2/package-codec-v1.1.json"),
    )?)?;
    let package = hex_bytes(
        &format!(
            "0x{}",
            package_fixture["carried_hex"]
                .as_str()
                .context("carried package")?
        ),
        "package",
    )?;
    let package_raw = package_fixture["raw_len"]
        .as_u64()
        .context("package raw length")? as u32;
    let root =
        boole_derivation::m2::pool_leaf(c.chain_id, 2, &[], 1, package_raw, &package, &[9; 32]);
    let mut proof = None;
    let mut cancelled = None;
    let mut expired = None;
    for h in 1..=73 {
        let mut posts = vec![];
        match h {
            1 => {
                send(
                    &l1,
                    user,
                    vec![(8, pool(&c, root, 1, 3, 1))],
                    false,
                    &mut posts,
                )?;
                send(
                    &l1,
                    user,
                    vec![(1, transfer(0, 21000, &address(&user_key()), &[]))],
                    false,
                    &mut posts,
                )?;
            }
            5 => send(
                &l1,
                user,
                vec![(
                    9,
                    problem_encoded(0, 1, 0, [9; 32], &[], &package, (1, package_raw)),
                )],
                false,
                &mut posts,
            )?,
            6 => {
                let p = *model.problems.keys().next().context("draw opened")?;
                let mut a = Proof::new(&c, p, &z, [7; 32], [6; 20]);
                a.body[153..161].copy_from_slice(&(raw.len() as u64).to_be_bytes());
                a.body[161..193].copy_from_slice(&hash(&raw));
                a.commit = commit_hash(c.chain_id, p, cancel_hash(&a.secret), body_digest(&a.body));
                let b = Proof::new(&c, p, b"cancel", [8; 32], [6; 20]);
                let e = Proof::new(&c, p, b"expire", [10; 32], [6; 20]);
                send(
                    &l1,
                    user,
                    vec![(2, a.commit_body()), (2, b.commit_body())],
                    true,
                    &mut posts,
                )?;
                send(&l1, user, vec![(2, e.commit_body())], false, &mut posts)?;
                proof = Some(a);
                cancelled = Some(b);
                expired = Some(e);
            }
            7 => send(
                &l1,
                user,
                vec![(7, cancelled.as_ref().unwrap().cancel())],
                false,
                &mut posts,
            )?,
            14 | 15 => {
                send(
                    &l1,
                    user,
                    vec![(3, proof.as_ref().unwrap().open())],
                    false,
                    &mut posts,
                )?;
                if h == 15 {
                    send(
                        &l1,
                        user,
                        vec![(1, transfer(1, 21000, &address(&user_key()), &[]))],
                        false,
                        &mut posts,
                    )?;
                }
            }
            16..=18 => {
                let a = proof.as_ref().unwrap();
                let start = ((h - 16) * 6) as usize;
                for i in start..(start + 6).min(a.chunks.len()) {
                    send(&l1, user, vec![(4, a.part(i))], false, &mut posts)?;
                }
            }
            19 => send(
                &l1,
                user,
                vec![(7, proof.as_ref().unwrap().cancel())],
                false,
                &mut posts,
            )?,
            20 => send(
                &l1,
                user,
                vec![(1, transfer(2, 21000, &address(&user_key()), &[]))],
                false,
                &mut posts,
            )?,
            _ => {}
        }
        let row = mine(&l1, h, posts, &mut gas)?;
        let codes = fold(&mut model, &c, &row)?;
        {
            use std::io::Write;
            writeln!(
                fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(rt.path.join("build.jsonl"))?,
                "{}",
                json!({"row":row,"codes":codes})
            )?;
        }
        if h <= 20 {
            eprintln!("build {h}: {codes:?}");
        }
        let expected = match h {
            1 => vec![0x85],
            5 => vec![0x86],
            6 => vec![0x80; 3],
            7 => vec![0x84],
            14 | 19 => vec![9],
            15 => vec![0x81],
            16 | 17 => vec![0x82; 6],
            18 => vec![0x82, 0x83],
            _ => vec![],
        };
        assert_eq!(codes, expected, "I4 build decisions at {h}");
        rows.push(row);
    }
    let p = proof.as_ref().unwrap();
    assert_eq!(
        model.candidates[&(p.problem, p.commit)].status(),
        Status::Checking
    );
    let b = cancelled.as_ref().unwrap();
    assert_eq!(
        model.candidates[&(b.problem, b.commit)].status(),
        Status::Cancelled
    );
    let e = expired.as_ref().unwrap();
    assert_eq!(
        model.candidates[&(e.problem, e.commit)].status(),
        Status::Expired
    );
    let genesis_path = rt.path.join("l2-genesis.json");
    let g = genesis()?;
    fs::write(&genesis_path, serde_json::to_vec_pretty(&g)?)?;
    let a = rt.node(&binary, &genesis_path, "a", 0)?;
    let mut b = rt.node(&binary, &genesis_path, "b", 1)?;
    let mut da = driver(&a, &rt, &c)?;
    let mut db = driver(&b, &rt, &c)?;
    let finalized = da.l1.finalized()?;
    assert!(finalized < 73);
    let mut compared = vec![];
    for h in 1..=finalized {
        let parent = da.engine.head()?;
        let (av, _) = view(&mut da, &a, h)?;
        let (bv, _) = view(&mut db, &b, h)?;
        assert_eq!(av, bv);
        record_row(&mut rows[(h - 1) as usize], parent, av.clone());
        compared.push(av);
        if h == 17 {
            assert_eq!(
                da.m2_state().candidates[&(p.problem, p.commit)].status(),
                Status::Open
            );
            rt.stop("b")?;
            b = rt.node(&binary, &genesis_path, "b", 1)?;
            db = driver(&b, &rt, &c)?;
        }
    }
    let before = da.m2_state().root;
    assert!(matches!(
        da.step(finalized + 1)?,
        StepOutcome::WaitingForFinality
    ));
    assert_eq!(before, da.m2_state().root);
    assert!(matches!(
        db.step(finalized + 1)?,
        StepOutcome::WaitingForFinality
    ));
    l1.call("anvil_mine", json!(["0x8"]))?;
    for h in finalized + 1..=73 {
        let parent = da.engine.head()?;
        let (av, _) = view(&mut da, &a, h)?;
        let (bv, _) = view(&mut db, &b, h)?;
        assert_eq!(av, bv);
        record_row(&mut rows[(h - 1) as usize], parent, av.clone());
        compared.push(av);
    }
    let cc = rt.node(&binary, &genesis_path, "c", 2)?;
    let mut dc = driver(&cc, &rt, &c)?;
    for h in 1..=73 {
        assert_eq!(view(&mut dc, &cc, h)?.0, compared[(h - 1) as usize]);
    }
    assert_eq!(da.m2_state().root, model.root);
    // Rebuild first accepted parts from the finalized L1 log stream.
    let mut reconstructed = vec![None; p.chunks.len()];
    for row in &rows {
        for log in row["logs"].as_array().context("logs")? {
            let bytes = hex_bytes(log["data"].as_str().context("data")?, "log")?;
            if let Ok((4, body)) = boole_derivation::inbox::item(&bytes) {
                let index = u16::from_be_bytes(body[53..55].try_into().unwrap()) as usize;
                if body[21..53] == p.commit && reconstructed[index].is_none() {
                    let d = (usize::from(p.chunks.len() > 1))
                        * ((usize::BITS - (p.chunks.len() - 1).leading_zeros()) as usize);
                    reconstructed[index] = Some(body[55 + 32 * d..].to_vec());
                }
            }
        }
    }
    let rebuilt: Vec<u8> = reconstructed
        .into_iter()
        .map(|p| p.context("missing part"))
        .collect::<Result<Vec<_>>>()?
        .concat();
    assert_eq!(rebuilt, z);
    assert_eq!(
        u64::from_be_bytes(p.body[153..161].try_into()?),
        raw.len() as u64
    );
    assert_eq!(&p.body[161..193], &hash(&raw));
    assert_eq!(
        commit_hash(
            c.chain_id,
            p.problem,
            cancel_hash(&p.secret),
            body_digest(&p.body)
        ),
        p.commit
    );
    let gate_seam = check_frame(&rt.path, &rebuilt, &raw)?;
    let settled_finalized = da.l1.finalized()?;
    for h in 74..=settled_finalized {
        let mut row = source_row(&l1, h)?;
        let parent = da.engine.head()?;
        let (av, _) = view(&mut da, &a, h)?;
        assert_eq!(av, view(&mut db, &b, h)?.0);
        assert_eq!(av, view(&mut dc, &cc, h)?.0);
        record_row(&mut row, parent, av);
        rows.push(row);
    }
    let latest = quantity(&l1.call("eth_blockNumber", json!([]))?)?;
    for h in latest + 1..=latest + 2 {
        let items = vec![
            (2, commit(p.problem, hash(b"unfinalized commit"))),
            (7, p.cancel()),
            (4, p.part(0)),
        ];
        let tx = post(&l1, user, &items, true)?;
        let row = mine(&l1, h, vec![(tx, items)], &mut gas)?;
        assert_eq!(row["logs"].as_array().unwrap().len(), 3);
    }
    let pre_reorg_finalized = da.l1.finalized()?;
    for h in settled_finalized + 1..=pre_reorg_finalized {
        let mut row = source_row(&l1, h)?;
        let parent = da.engine.head()?;
        let (av, _) = view(&mut da, &a, h)?;
        assert_eq!(av, view(&mut db, &b, h)?.0);
        assert_eq!(av, view(&mut dc, &cc, h)?.0);
        record_row(&mut row, parent, av);
        rows.push(row);
    }
    let stable = [
        a.block()?["hash"].clone(),
        b.block()?["hash"].clone(),
        cc.block()?["hash"].clone(),
    ];
    let stable_roots = [da.m2_state().root, db.m2_state().root, dc.m2_state().root];
    for d in [&mut da, &mut db, &mut dc] {
        assert!(matches!(
            d.step(pre_reorg_finalized + 1)?,
            StepOutcome::WaitingForFinality
        ));
    }
    let old = l1.call("eth_getBlockByNumber", json!(["latest", false]))?;
    l1.call("anvil_reorg", json!([2, []]))?;
    let new = l1.call("eth_getBlockByNumber", json!(["latest", false]))?;
    assert_ne!(old["hash"], new["hash"]);
    assert_eq!(
        stable,
        [
            a.block()?["hash"].clone(),
            b.block()?["hash"].clone(),
            cc.block()?["hash"].clone()
        ]
    );
    assert_eq!(
        stable_roots,
        [da.m2_state().root, db.m2_state().root, dc.m2_state().root]
    );
    l1.call("anvil_mine", json!(["0x10"]))?;
    let end = latest + 3;
    for h in pre_reorg_finalized + 1..=end {
        let mut row = source_row(&l1, h)?;
        if h > latest {
            assert!(row["logs"].as_array().unwrap().is_empty());
        }
        let parent = da.engine.head()?;
        let (av, _) = view(&mut da, &a, h)?;
        assert_eq!(av, view(&mut db, &b, h)?.0);
        assert_eq!(av, view(&mut dc, &cc, h)?.0);
        record_row(&mut row, parent, av);
        rows.push(row);
    }
    let gas_limit_probe = l1.call(
        "eth_sendTransaction",
        json!([{"from":user,"to":user,"gas":"0x1000001"}]),
    );
    let eip7825 = match gas_limit_probe {
        Ok(v) => json!({"accepted":true,"result":v}),
        Err(e) => json!({"accepted":false,"error":format!("{e:#}")}),
    };
    for name in ["a", "b", "c"] {
        rt.stop(name)?;
    }
    recorded_abc(&mut rt, &binary, &genesis_path, &c, &rows)?;
    let mut replay_model = State::new(&c, 30_000_000)?;
    for row in &rows {
        fold(&mut replay_model, &c, row)?;
        assert_eq!(row["result"]["m2_root"], hex_string(&replay_model.root));
    }
    let mix: std::collections::BTreeSet<_> = rows
        .iter()
        .map(|r| r["l1"]["mixHash"].as_str().unwrap_or("").to_owned())
        .collect();
    assert!(gas
        .iter()
        .filter(|g| g["kinds"] == json!([4]))
        .all(|g| g["floor_applied"] == true));
    let full: Vec<_> = gas
        .iter()
        .filter(|g| g["kinds"] == json!([4]) && g["gas_used"].as_u64().unwrap_or(0) > 1_000_000)
        .collect();
    assert_eq!(full.len(), 13);
    assert!(full.iter().all(|g| g["gas_used"] == g["calldata_floor"]));
    let mut data = json!({"schema":"boole-devnet-m2-recording-v1.1","provenance":{"pins":fixture_json("pins.json")?,"source_genesis_sha256":hex_string(&hash(&fs::read(fixture("l2-genesis.json"))?)),"rules":"M2 v1.1; finalized L1 inbox; synthetic generator and environment identities"},"acceptance":{"live_abc":true,"recorded_abc":true,"restart_height":17,"unfinalized_reorg":true},"config":c,"l2_genesis":g,"codec":codec,"gate_seam":gate_seam,"mix_hash":{"unique":mix,"count":mix.len()},"eip7825":eip7825,"gas":gas,"heights":rows});
    data["package_encoding"] = json!({"codec":1,"raw_len":package_raw,"carried_len":package.len(),"carried_sha256":hex_string(&hash(&package)),"fixture":"package-codec-v1.1.json"});
    bind_provenance(&mut data)?;
    fs::write(
        rt.path.join("recording.json"),
        serde_json::to_vec_pretty(&data)?,
    )?;
    rt.finish(true)?;
    if let Some(path) = std::env::var_os("BOOLE_DEVNET_M2_RECORDING") {
        fs::write(path, serde_json::to_vec_pretty(&data)?)?;
    }
    eprintln!("I4/I5 PASS: {end} heights live and recorded A/B/C; 14 parts, restart, fresh C, reorg, codec seam");
    Ok(())
}

fn compare_live_rows(
    rt: &mut Runtime,
    binary: &Path,
    genesis: &Path,
    c: &Config,
    rows: &mut [Value],
) -> Result<()> {
    let mut baseline = Vec::new();
    let mut model = State::new(c, 30_000_000)?;
    for (i, name) in ["a", "b", "c"].into_iter().enumerate() {
        let n = rt.node(binary, genesis, name, i as u16)?;
        let mut d = driver(&n, rt, c)?;
        for (at, row) in rows.iter_mut().enumerate() {
            let h = row["height"].as_u64().context("height")?;
            let parent = d.engine.head()?;
            let (v, _) = view(&mut d, &n, h)?;
            if i == 0 {
                fold(&mut model, c, row)?;
                assert_eq!(
                    v["m2_root"],
                    hex_string(&model.root),
                    "pure model height {h}"
                );
                record_row(row, parent, v.clone());
                baseline.push(v);
            } else {
                assert_eq!(v, baseline[at], "replica {name} height {h}");
            }
        }
        rt.stop(name)?;
    }
    Ok(())
}
#[test]
fn i6_adversarial_uncapped_reveal_copy_cap_and_gas_abc() -> Result<()> {
    let Some(binary) = pinned("BOOLE_DEVNET_OP_RETH", "op_reth")? else {
        return Ok(());
    };
    let Some(anvil) = pinned("BOOLE_DEVNET_ANVIL", "anvil")? else {
        return Ok(());
    };
    let mut rt = Runtime::new("i6")?;
    let l1 = rt.anvil_with_genesis(&anvil, &l1_genesis(&rt)?)?;
    let accounts = l1.call("eth_accounts", json!([]))?;
    let user = accounts[0].as_str().context("account")?;
    let c = config();
    let mut model = State::new(&c, 30_000_000)?;
    let mut rows = vec![];
    let mut gas = vec![];
    let package = b"adversarial-generator-v1";
    let root = pool_leaf(c.chain_id, 2, &[], package, &[9; 32]);
    let mut victim = None;
    let mut copier = None;
    let mut expiry = None;
    for h in 1..=72 {
        let mut posts = vec![];
        match h {
            1 => {
                send(
                    &l1,
                    user,
                    vec![
                        (8, pool(&c, root, 1, 3, 1)),
                        (1, transfer(0, 30_000_000, &address(&user_key()), &[])),
                    ],
                    true,
                    &mut posts,
                )?;
            }
            2 => send(
                &l1,
                user,
                vec![
                    (3, vec![1]),
                    (1, transfer(0, 16_000_000, &[], &[0x5b, 0x60, 0, 0x56])),
                    (1, transfer(1, 15_000_000, &address(&user_key()), &[])),
                ],
                true,
                &mut posts,
            )?,
            5 => send(
                &l1,
                user,
                vec![(9, problem(0, 1, 0, [9; 32], &[], package))],
                false,
                &mut posts,
            )?,
            6 => {
                let p = *model.problems.keys().next().context("problem")?;
                let v = Proof::new(&c, p, b"honest", [7; 32], [6; 20]);
                let mut copy = Proof::new(&c, p, b"honest", [11; 32], [1; 20]);
                copy.body[199..231].copy_from_slice(&v.body[199..231]);
                copy.commit = commit_hash(
                    c.chain_id,
                    p,
                    cancel_hash(&copy.secret),
                    body_digest(&copy.body),
                );
                let e = Proof::new(&c, p, b"expiry", [8; 32], [6; 20]);
                send(
                    &l1,
                    user,
                    vec![
                        (2, v.commit_body()),
                        (2, copy.commit_body()),
                        (2, e.commit_body()),
                    ],
                    true,
                    &mut posts,
                )?;
                victim = Some(v);
                copier = Some(copy);
                expiry = Some(e);
            }
            7 => {
                let mut cancel = victim.as_ref().unwrap().cancel();
                cancel[128] ^= 1;
                let mut forged = pool(&c, [1; 32], 1, 20, 2);
                forged[90] ^= 1;
                send(&l1, user, vec![(7, cancel), (8, forged)], true, &mut posts)?;
            }
            14 => send(
                &l1,
                user,
                vec![(3, victim.as_ref().unwrap().open())],
                false,
                &mut posts,
            )?,
            15 => send(
                &l1,
                user,
                vec![(3, copier.as_ref().unwrap().open())],
                false,
                &mut posts,
            )?,
            16 => {
                let v = victim.as_ref().unwrap();
                let commits: Vec<_> = (0..=c.c_commit)
                    .map(|i| (2, commit(v.problem, hash(&i.to_be_bytes()))))
                    .collect();
                for items in commits.chunks(469) {
                    send(&l1, user, items.to_vec(), true, &mut posts)?;
                }
                let mut flood = vec![];
                for _ in 0..100 {
                    flood.push((3, vec![1]));
                    flood.push((4, vec![1]));
                }
                flood.push((4, copier.as_ref().unwrap().part(0)));
                flood.push((3, v.open()));
                flood.push((4, v.part(0)));
                send(&l1, user, flood, true, &mut posts)?;
            }
            17 => {
                let v = victim.as_ref().unwrap();
                let mut huge = v.open();
                let b = 53;
                huge[b + 153..b + 161].copy_from_slice(&(Z_CAP as u64 + 1).to_be_bytes());
                huge[b + 193..b + 197].copy_from_slice(&(Z_CAP as u32 + 1).to_be_bytes());
                huge[b + 197..b + 199].copy_from_slice(&129u16.to_be_bytes());
                send(
                    &l1,
                    user,
                    vec![(7, v.cancel()), (3, huge)],
                    true,
                    &mut posts,
                )?;
            }
            _ => {}
        }
        let row = mine(&l1, h, posts, &mut gas)?;
        let codes = fold(&mut model, &c, &row)?;
        match h {
            7 => assert_eq!(codes, [12, 13]),
            14 => assert_eq!(codes, [9]),
            16 => {
                assert_eq!(
                    codes.iter().filter(|x| **x == 0x80).count(),
                    c.c_commit as usize
                );
                assert_eq!(codes.iter().filter(|x| **x == 11).count(), 1);
                assert_eq!(codes.len(), c.c_commit as usize + 1 + 200 + 3);
                assert_eq!(
                    &codes[c.c_commit as usize + 1..c.c_commit as usize + 201],
                    vec![1; 200]
                );
                assert_eq!(&codes[codes.len() - 3..], &[8, 0x81, 0x83]);
            }
            17 => assert_eq!(codes, [9, 3]),
            _ => {}
        }
        rows.push(row);
    }
    let v = victim.as_ref().unwrap();
    assert_eq!(
        model.candidates[&(v.problem, v.commit)].status(),
        Status::Checking
    );
    let e = expiry.as_ref().unwrap();
    assert_eq!(
        model.candidates[&(e.problem, e.commit)].status(),
        Status::Expired
    );
    l1.call("anvil_mine", json!(["0x8"]))?;
    let g = rt.path.join("l2-genesis.json");
    fs::write(&g, serde_json::to_vec_pretty(&genesis()?)?)?;
    compare_live_rows(&mut rt, &binary, &g, &c, &mut rows)?;
    assert_eq!(
        rows[0]["result"]["decisions"][1]["rule"],
        "USER-GAS-EXCEEDS-BLOCK-BUDGET"
    );
    assert_eq!(
        rows[1]["result"]["observed"]["receipts"][1]["status"],
        "0x0"
    );
    assert_eq!(
        rows[1]["result"]["decisions"][2]["rule"],
        "USER-GAS-EXCEEDS-BLOCK-BUDGET"
    );
    recorded_abc(&mut rt, &binary, &g, &c, &rows)?;
    let mut data = json!({"schema":"boole-devnet-m2-adversarial-v1.1","provenance":{"pins":fixture_json("pins.json")?},"acceptance":{"live_abc":true,"recorded_abc":true},"config":c,"l2_genesis":genesis()?,"heights":rows,"gas":gas});
    bind_provenance(&mut data)?;
    fs::write(
        rt.path.join("adversarial.json"),
        serde_json::to_vec_pretty(&data)?,
    )?;
    rt.finish(true)?;
    if let Some(path) = std::env::var_os("BOOLE_DEVNET_I6_RECORDING") {
        fs::write(path, serde_json::to_vec_pretty(&data)?)?;
    }
    eprintln!("I6 PASS: 72 live A/B/C heights, cap 937/+1, uncapped reveal, keyed copy, expiry, gas probes");
    Ok(())
}
#[test]
fn i7_rounds_two_slots_deadline_withheld_next_draw_abc() -> Result<()> {
    let Some(binary) = pinned("BOOLE_DEVNET_OP_RETH", "op_reth")? else {
        return Ok(());
    };
    let Some(anvil) = pinned("BOOLE_DEVNET_ANVIL", "anvil")? else {
        return Ok(());
    };
    let mut rt = Runtime::new("i7")?;
    let l1 = rt.anvil_with_genesis(&anvil, &l1_genesis(&rt)?)?;
    let accounts = l1.call("eth_accounts", json!([]))?;
    let user = accounts[0].as_str().context("account")?;
    let c = config();
    assert_eq!(c.slot_table.len(), 2);
    c.validate(30_000_000)
        .map_err(|s| anyhow::anyhow!("S{s}"))?;
    let packages: Vec<_> = (0u8..8).map(|i| vec![b'G', b'0' + i]).collect();
    let leaves: Vec<_> = packages
        .iter()
        .enumerate()
        .map(|(i, p)| pool_leaf(c.chain_id, 2, &[], p, &[i as u8; 32]))
        .collect();
    let roots = [merkle_root(&leaves[..4]), merkle_root(&leaves[4..])];
    let mut model = State::new(&c, 30_000_000)?;
    let mut rows = vec![];
    let mut gas = vec![];
    let mut first = None;
    for h in 1..=140 {
        let mut posts = vec![];
        if h == 1 {
            send(
                &l1,
                user,
                vec![
                    (8, pool_at(&c, 0, roots[0], 4, 3, 1)),
                    (8, pool_at(&c, 1, roots[1], 4, 3, 2)),
                ],
                true,
                &mut posts,
            )?;
        }
        if let Some((slot, round)) = match h {
            5 => Some((0, 1)),
            19 => Some((1, 2)),
            60 => Some((0, 2)),
            115 => Some((0, 3)),
            126 => Some((1, 3)),
            _ => None,
        } {
            let draw = &model.rounds[&(slot, round)];
            let i = draw.leaf_index as usize;
            let offset = usize::from(slot) * 4;
            send(
                &l1,
                user,
                vec![(
                    9,
                    problem(
                        slot,
                        round,
                        draw.leaf_index,
                        [(i + offset) as u8; 32],
                        &merkle_path(&leaves[offset..offset + 4], i),
                        &packages[i + offset],
                    ),
                )],
                false,
                &mut posts,
            )?;
        }
        if h == 6 {
            let p = *model.problems.keys().next().context("problem")?;
            first = Some(p);
            send(
                &l1,
                user,
                vec![(2, commit(p, hash(b"unrevealed round candidate")))],
                false,
                &mut posts,
            )?;
        }
        let row = mine(&l1, h, posts, &mut gas)?;
        fold(&mut model, &c, &row)?;
        if h == 9 {
            assert_eq!(model.rounds[&(1, 1)].status, RoundStatus::Withheld);
        }
        if h == 20 {
            assert_eq!(
                model
                    .problems
                    .values()
                    .filter(|p| p.closed.is_none())
                    .count(),
                2
            );
        }
        if h == 54 {
            assert_eq!(model.problems[&first.unwrap()].closed, Some(54));
            assert_eq!(model.slots[0].scheduled, Some(58));
        }
        if h == 59 {
            assert_eq!(model.rounds[&(0, 2)].status, RoundStatus::Drawn);
            assert!(
                !model.priority(first.unwrap()).is_empty(),
                "earlier barrier still pending; next round independent"
            );
        }
        if h == 116 {
            assert_eq!(model.rounds[&(1, 2)].status, RoundStatus::Closed);
            assert_eq!(model.slots[1].scheduled, Some(124));
        }
        if h == 125 {
            assert_eq!(model.rounds[&(1, 3)].status, RoundStatus::Drawn);
        }
        rows.push(row);
    }
    let selected: std::collections::BTreeSet<_> = model
        .rounds
        .values()
        .map(|r| (r.pool_seq, r.leaf_index))
        .collect();
    assert_eq!(selected.len(), model.rounds.len());
    l1.call("anvil_mine", json!(["0x8"]))?;
    let g = rt.path.join("l2-genesis.json");
    fs::write(&g, serde_json::to_vec_pretty(&genesis()?)?)?;
    compare_live_rows(&mut rt, &binary, &g, &c, &mut rows)?;
    recorded_abc(&mut rt, &binary, &g, &c, &rows)?;
    let mut data = json!({"schema":"boole-devnet-m2-rounds-v1.1","provenance":{"pins":fixture_json("pins.json")?},"acceptance":{"live_abc":true,"recorded_abc":true},"config":c,"l2_genesis":genesis()?,"heights":rows,"gas":gas});
    bind_provenance(&mut data)?;
    fs::write(
        rt.path.join("rounds.json"),
        serde_json::to_vec_pretty(&data)?,
    )?;
    rt.finish(true)?;
    if let Some(path) = std::env::var_os("BOOLE_DEVNET_I7_RECORDING") {
        fs::write(path, serde_json::to_vec_pretty(&data)?)?;
    }
    eprintln!("I7 PASS: 140 live and recorded A/B/C heights with distinct SD/GAP, two slots, withheld, deadline and next draw");
    Ok(())
}
