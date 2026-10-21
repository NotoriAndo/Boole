//! I1–I3: D3/D4/D12/D15/D18, real pinned clients on disposable loopback state.
mod support;
use anyhow::{bail, Context, Result};
use boole_derivation::{attributes::hex_string, Derived};
use boole_devnet_driver::{
    driver::{Driver, StepOutcome},
    engine::{Engine, RpcEngine},
    l1::{L1Source, RecordedL1, RpcL1},
};
use serde_json::{json, Value};
use std::fs;
use support::*;

fn derived(outcome: StepOutcome) -> Result<Derived> {
    match outcome {
        StepOutcome::Derived(d) => Ok(*d),
        StepOutcome::WaitingForFinality => bail!("unexpected finality wait"),
    }
}

const FIRST_BUDGET_HEIGHT: u64 = 24;

fn budget_main() -> Result<Value> {
    let candidate = std::env::var_os("BOOLE_DEVNET_BUDGET_CANDIDATE");
    let path = candidate
        .clone()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| fixture("s1b-recorded-main-budget-v1.json"));
    let value: Value = serde_json::from_str(&fs::read_to_string(path)?)?;
    if candidate.is_none() {
        assert_eq!(value["acceptance"]["recorded_abc"], true);
        assert_eq!(value["acceptance"]["live_abc"], true);
    }
    Ok(value)
}

fn check_recorded_observation(observed: &Value, row: &Value, profile: &str, h: u64) -> Result<()> {
    for key in ["blockHash", "stateRoot", "receiptsRoot"] {
        assert_eq!(observed[key], row[key], "{profile} height {h} {key}");
    }
    let actual = observed["receipts"].as_array().context("receipts")?;
    let expected = row["decisions"].as_array().context("decisions")?;
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!(
            !actual.is_null(),
            expected["included"],
            "{profile} height {h}"
        );
        assert_eq!(actual["status"], expected["status"]);
        assert_eq!(actual, &expected["receipt"]);
    }
    Ok(())
}

#[test]
fn i1_recorded_replay_37_heights() -> Result<()> {
    use sha2::{Digest, Sha256};
    let Some(binary) = pinned("BOOLE_DEVNET_OP_RETH", "op_reth")? else {
        return Ok(());
    };
    let generate = std::env::var_os("BOOLE_DEVNET_GENERATE_BUDGET");
    let mut candidate = None;
    for profile in ["main", "basefee"] {
        let mut runtime = Runtime::new(&format!("i1-{profile}"))?;
        let genesis = fixture(if profile == "main" {
            "l2-genesis.json"
        } else {
            "l2-genesis-basefee.json"
        });
        let text = fs::read_to_string(fixture(&format!("s1b-recorded-{profile}.json")))?;
        let rows: Vec<Value> = serde_json::from_str(&text)?;
        let expected = if profile == "main" && generate.is_none() {
            Some(budget_main()?)
        } else {
            None
        };
        let mut nodes = vec![runtime.node(&binary, &genesis, "a", 0)?];
        if profile == "main" && generate.is_some() {
            nodes.push(runtime.node(&binary, &genesis, "b", 1)?);
            nodes.push(runtime.node(&binary, &genesis, "c", 2)?);
        }
        let mut drivers = nodes
            .iter()
            .map(|node| {
                let mut d = Driver::new(RecordedL1::from_json(&text)?, node.engine()?);
                d.set_evidence(&runtime.path.join(format!("{}.jsonl", node.name)))?;
                Ok(d)
            })
            .collect::<Result<Vec<_>>>()?;
        let mut generated = vec![];
        for (index, source) in rows.iter().enumerate() {
            let h = source["height"].as_u64().context("recorded height")?;
            let mut agreed = None;
            for (node, driver) in nodes.iter().zip(&mut drivers) {
                let parent = driver.engine.head()?;
                let d = derived(driver.step(h)?)?;
                let observed = observe(node, &d)?;
                if profile == "basefee" || h < FIRST_BUDGET_HEIGHT {
                    check_recorded_observation(&observed, source, profile, h)?;
                    assert_eq!(serde_json::to_value(&d.attributes)?, source["attributes"]);
                }
                if let Some(expected) = &expected {
                    check_recorded_observation(&observed, &expected["heights"][index], profile, h)?;
                    assert_eq!(
                        serde_json::to_value(&d.attributes)?,
                        expected["heights"][index]["attributes"]
                    );
                }
                let mut row = source.clone();
                row["parent"] = serde_json::to_value(parent)?;
                row["attributes"] = serde_json::to_value(&d.attributes)?;
                row["attribute_digest"] = json!(hex_string(&Sha256::digest(d.canonical_bytes()?)));
                for ((item, decision), receipt) in row["decisions"]
                    .as_array_mut()
                    .context("record decisions")?
                    .iter_mut()
                    .zip(&d.decisions)
                    .zip(observed["receipts"].as_array().context("record receipts")?)
                {
                    let user = decision.user.as_ref().context("user fixture")?;
                    item["rule"] = json!(decision.rule);
                    item["forward"] = json!(decision.forward);
                    item["signer"] = json!(user.sender);
                    item["tx_hash"] = json!(user.tx_hash);
                    item["included"] = json!(!receipt.is_null());
                    item["status"] = receipt["status"].clone();
                    item["receipt"] = receipt.clone();
                }
                for key in ["blockHash", "stateRoot", "receiptsRoot"] {
                    row[key] = observed[key].clone();
                }
                if let Some(reference) = &agreed {
                    assert_eq!(&row, reference, "recorded A/B/C mismatch at {h}");
                } else {
                    agreed = Some(row);
                }
            }
            generated.push(agreed.context("recorded row")?);
        }
        runtime.finish(true)?;
        if profile == "main" && generate.is_some() {
            assert_ne!(generated[23]["blockHash"], rows[23]["blockHash"]);
            candidate = Some(
                json!({"schema":"boole-devnet-budget-v1","provenance":{"source":"S1b local spike, 2026-10-20, op-reth v2.6.0, Foundry v1.8.5","source_recording":"s1b-recorded-main.json","source_sha256":hex_string(&Sha256::digest(text.as_bytes())),"rule":"D12 cumulative gas budget, DEVNET-M1b, 2026-10-21","first_changed_height":FIRST_BUDGET_HEIGHT,"pins":fixture_json("pins.json")?},"acceptance":{"recorded_abc":true,"live_abc":false},"heights":generated}),
            );
        }
        eprintln!(
            "I1 {profile}: PASS {} recorded heights (replicas={})",
            rows.len(),
            nodes.len()
        );
    }
    if let Some(path) = generate {
        fs::write(
            std::path::PathBuf::from(path),
            serde_json::to_vec_pretty(&candidate.context("main candidate")?)?,
        )?;
        eprintln!("budget candidate written; requires live A/B/C acceptance");
    }
    Ok(())
}

type LiveDriver = Driver<RpcL1, RpcEngine>;
fn live_driver(node: &Node, runtime: &Runtime) -> Result<LiveDriver> {
    let mut driver = Driver::new(RpcL1::new("http://127.0.0.1:28545")?, node.engine()?);
    driver.set_evidence(&runtime.path.join(format!("{}.jsonl", node.name)))?;
    Ok(driver)
}

fn live_step(driver: &mut LiveDriver, node: &Node, case: &Value) -> Result<Value> {
    let h = case["height"].as_u64().context("height")?;
    let parent = driver.engine.head()?;
    let user = case["addresses"][0].as_str().context("user address")?;
    let before = if case["label"] == "13-execution-revert" && case["role"] == "case" {
        Some(node.state(user, &parent.hash)?)
    } else {
        None
    };
    let d = derived(driver.step(h)?)?;
    let observed = observe(node, &d)?;
    let statuses: Vec<Value> = observed["receipts"]
        .as_array()
        .context("receipts")?
        .iter()
        .map(|r| {
            if r.is_null() {
                Ok(Value::Null)
            } else {
                Ok(json!(quantity(&r["status"])?))
            }
        })
        .collect::<Result<_>>()?;
    assert_eq!(
        json!(statuses),
        case["expect"],
        "height {h}: {} {}",
        case["label"],
        case["role"]
    );
    if let Some(expected) = case["budget_decisions"].as_array() {
        assert_eq!(d.decisions.len(), expected.len());
        for (actual, expected) in d.decisions.iter().zip(expected) {
            assert_eq!(actual.rule, expected["rule"]);
            assert_eq!(actual.forward, expected["forward"]);
        }
    }
    if case["classification"] == "FILTER-BY-DRIVER" {
        assert!(d.decisions.iter().all(|x| !x.forward));
    }
    assert!(d
        .attributes
        .transactions
        .iter()
        .skip(1)
        .all(|s| !s.starts_with("0x7e")));
    let hash = observed["blockHash"].as_str().context("block hash")?;
    if case["label"] == "8a-sequential-balance" && case["role"] == "case" {
        let address = case["addresses"][1].as_str().context("8a sender")?;
        let state = node.state(address, hash)?;
        assert_eq!(amount(&state["balance"])?, 979000);
        assert_eq!(quantity(&state["nonce"])?, 1);
    }
    if let Some(before) = before {
        let after = node.state(user, hash)?;
        let receipt = &observed["receipts"][0];
        if receipt.is_null() {
            assert_eq!(before, after);
        } else {
            let fee = amount(&receipt["gasUsed"])?
                .checked_mul(amount(&receipt["effectiveGasPrice"])?)
                .context("fee overflow")?;
            assert!(fee > 0);
            assert_eq!(
                amount(&before["balance"])? - amount(&after["balance"])?,
                fee
            );
            assert_eq!(quantity(&after["nonce"])?, quantity(&before["nonce"])? + 1);
        }
    }
    use sha2::{Digest, Sha256};
    Ok(
        json!({"observed":observed,"attribute_digest":hex_string(&Sha256::digest(d.canonical_bytes()?)),"decisions":d.decisions}),
    )
}

#[test]
fn i2_live_finality_restart_reorg_abc() -> Result<()> {
    let Some(binary) = pinned("BOOLE_DEVNET_OP_RETH", "op_reth")? else {
        return Ok(());
    };
    let Some(anvil) = pinned("BOOLE_DEVNET_ANVIL", "anvil")? else {
        return Ok(());
    };
    let cases = fixture_json("s1b-cases.json")?;
    for profile in ["main", "basefee"] {
        let mut blocks = cases[profile].as_array().context("profile blocks")?.clone();
        if profile == "main" {
            let budget = budget_main()?;
            for (block, row) in blocks
                .iter_mut()
                .zip(budget["heights"].as_array().context("budget heights")?)
            {
                let expected = row["decisions"].as_array().context("budget decisions")?;
                block["expect"] = json!(expected
                    .iter()
                    .map(|d| if d["status"].is_null() {
                        Ok(Value::Null)
                    } else {
                        Ok(json!(quantity(&d["status"])?))
                    })
                    .collect::<Result<Vec<_>>>()?);
                block["budget_decisions"] = json!(expected
                    .iter()
                    .map(|d| json!({"rule":d["rule"],"forward":d["forward"]}))
                    .collect::<Vec<_>>());
            }
        }
        let mut runtime = Runtime::new(&format!("i2-{profile}"))?;
        let l1 = runtime.anvil(&anvil)?;
        let accounts = l1.call("eth_accounts", json!([]))?;
        let user = accounts[0].as_str().context("unlocked account 0")?;
        for block in &blocks {
            let mut posts = vec![];
            for raw in block["raws"].as_array().context("raws")? {
                posts.push(l1.call("eth_sendTransaction",json!([{"from":user,"to":boole_derivation::config::INBOX,"data":post_calldata(raw.as_str().context("raw")?)?,"gas":"0xf4240"}]))?);
            }
            let h = block["height"].as_u64().context("height")?;
            l1.call("evm_mine", json!([1_800_000_000 + 12 * h]))?;
            for post in posts {
                let receipt = l1.call("eth_getTransactionReceipt", json!([post]))?;
                assert_eq!(receipt["status"], "0x1");
                assert!(!receipt["logs"].as_array().context("post logs")?.is_empty());
            }
        }
        let genesis = fixture(if profile == "main" {
            "l2-genesis.json"
        } else {
            "l2-genesis-basefee.json"
        });
        let a = runtime.node(&binary, &genesis, "a", 0)?;
        let mut b = runtime.node(&binary, &genesis, "b", 1)?;
        let mut da = live_driver(&a, &runtime)?;
        let mut db = live_driver(&b, &runtime)?;
        let finalized = da.l1.finalized()?;
        assert!(finalized < (blocks.len() as u64));
        let mut expected = Vec::new();
        let mut restart = false;
        for block in blocks.iter().take(finalized as usize) {
            let arow = live_step(&mut da, &a, block)?;
            let brow = live_step(&mut db, &b, block)?;
            assert_eq!(arow, brow);
            expected.push(arow);
            if block["height"] == 18 {
                runtime.stop("b")?;
                b = runtime.node(&binary, &genesis, "b", 1)?;
                db = live_driver(&b, &runtime)?;
                assert_eq!(db.engine.head()?.number, 18);
                restart = true;
            }
        }
        for d in [&mut da, &mut db] {
            assert!(matches!(
                d.step(finalized + 1)?,
                StepOutcome::WaitingForFinality
            ));
        }
        l1.call("anvil_mine", json!(["0x10"]))?;
        for block in blocks.iter().skip(finalized as usize) {
            let arow = live_step(&mut da, &a, block)?;
            let brow = live_step(&mut db, &b, block)?;
            assert_eq!(arow, brow);
            expected.push(arow);
        }
        if profile == "main" {
            assert!(restart);
        }
        let c = runtime.node(&binary, &genesis, "c", 2)?;
        let mut dc = live_driver(&c, &runtime)?;
        for (block, expected) in blocks.iter().zip(&expected) {
            assert_eq!(&live_step(&mut dc, &c, block)?, expected);
        }
        let before: Vec<_> = [&a, &b, &c]
            .into_iter()
            .map(|n| n.block().map(|v| v["hash"].clone()))
            .collect::<Result<_>>()?;
        let old_l1 = l1.call("eth_getBlockByNumber", json!(["latest", false]))?;
        l1.call("anvil_reorg", json!([2, []]))?;
        let new_l1 = l1.call("eth_getBlockByNumber", json!(["latest", false]))?;
        assert_ne!(old_l1["hash"], new_l1["hash"]);
        let after: Vec<_> = [&a, &b, &c]
            .into_iter()
            .map(|n| n.block().map(|v| v["hash"].clone()))
            .collect::<Result<_>>()?;
        assert_eq!(before, after);
        let next = blocks.len() as u64 + 1;
        let mut post_reorg = Vec::new();
        for (driver, node) in [(&mut da, &a), (&mut db, &b), (&mut dc, &c)] {
            let d = derived(driver.step(next)?)?;
            assert_eq!(driver.engine.head()?.number, next);
            post_reorg.push(json!({"observed":observe(node,&d)?,"attributes":d.attributes,"decisions":d.decisions}));
        }
        assert_eq!(post_reorg[0], post_reorg[1]);
        assert_eq!(post_reorg[0], post_reorg[2]);
        fs::write(
            runtime.path.join("comparisons.json"),
            serde_json::to_vec_pretty(
                &json!({"profile":profile,"heights":expected,"finality_wait_at":finalized+1,"restart_at_18":restart,"reorg_heads_unchanged":true,"post_reorg_finalized_height":next,"post_reorg":post_reorg}),
            )?,
        )?;
        runtime.finish(true)?;
        eprintln!(
            "I2 {profile}: PASS {} A/B/C heights, finality wait, restart={restart}, reorg",
            blocks.len()
        );
    }
    Ok(())
}

fn probe_tx(
    key: &k256::ecdsa::SigningKey,
    nonce: u64,
    gas: u64,
    to: &[u8],
    data: &[u8],
) -> Result<String> {
    use boole_derivation::{rlp, user_tx::keccak};
    let mut fields = vec![
        rlp::uint(boole_derivation::config::L2_CHAIN_ID),
        rlp::uint(nonce),
        rlp::uint(1),
        rlp::uint(2),
        rlp::uint(gas),
        rlp::bytes(to),
        rlp::uint(0),
        rlp::bytes(data),
        rlp::list(&[]),
    ];
    let digest = keccak(&[vec![2], rlp::list(&fields)].concat());
    let (signature, recovery) = key
        .sign_prehash_recoverable(&digest)
        .map_err(|e| anyhow::anyhow!("probe signature: {e}"))?;
    if recovery.to_byte() > 1 {
        bail!("probe signature recovery overflow");
    }
    let bytes = signature.to_bytes();
    fields.push(rlp::uint(u64::from(recovery.to_byte())));
    for scalar in [&bytes[..32], &bytes[32..]] {
        let start = scalar.iter().position(|b| *b != 0).unwrap_or(scalar.len());
        fields.push(rlp::bytes(&scalar[start..]));
    }
    Ok(hex_string(&[vec![2], rlp::list(&fields)].concat()))
}

#[test]
fn i3_block_gas_probe_identical_blocks() -> Result<()> {
    use sha2::{Digest, Sha256};
    let Some(binary) = pinned("BOOLE_DEVNET_OP_RETH", "op_reth")? else {
        return Ok(());
    };
    let seed = Sha256::digest(b"boole-devnet-m1-probe");
    let key = k256::ecdsa::SigningKey::from_bytes(&seed)
        .map_err(|e| anyhow::anyhow!("probe key: {e}"))?;
    let point = key.verifying_key().to_encoded_point(false);
    let hash = boole_derivation::user_tx::keccak(&point.as_bytes()[1..]);
    let address = &hash[12..];
    for probe in ["p1", "p2"] {
        let mut runtime = Runtime::new(&format!("i3-{probe}"))?;
        let mut genesis = fixture_json("l2-genesis.json")?;
        genesis["alloc"][hex_string(address)] = json!({"balance":"0x56bc75e2d63100000"});
        let genesis_path = runtime.path.join("genesis.json");
        fs::write(&genesis_path, serde_json::to_vec_pretty(&genesis)?)?;
        let a = runtime.node(&binary, &genesis_path, "a", 0)?;
        let b = runtime.node(&binary, &genesis_path, "b", 1)?;
        let parent = a.engine()?.head()?;
        assert_eq!(parent.gas_limit, 30_000_000);
        let raws = if probe == "p1" {
            vec![probe_tx(&key, 0, parent.gas_limit, address, &[])?]
        } else {
            vec![
                probe_tx(&key, 0, 16_000_000, &[], &[0x5b, 0x60, 0x00, 0x56])?,
                probe_tx(&key, 1, 15_000_000, address, &[])?,
            ]
        };
        let original = fixture_json("s1b-recorded-main.json")?;
        let l1 = original[0]["l1"].clone();
        let mut logs = vec![];
        for (index, raw) in raws.iter().enumerate() {
            let calldata = post_calldata(raw)?;
            let envelope = format!("0x{}", &calldata[10..]);
            logs.push(json!({"address":boole_derivation::config::INBOX,"topics":[boole_derivation::config::posted_topic()],"blockHash":l1["hash"],"removed":false,"transactionIndex":format!("0x{index:x}"),"logIndex":format!("0x{index:x}"),"data":envelope}));
        }
        let recording = serde_json::to_string(&json!([{"l1":l1,"logs":logs}]))?;
        let mut observations = vec![];
        let mut records = vec![];
        for node in [&a, &b] {
            let mut driver = Driver::new(RecordedL1::from_json(&recording)?, node.engine()?);
            let evidence = runtime.path.join(format!("{}.jsonl", node.name));
            driver.set_evidence(&evidence)?;
            let d = derived(driver.step(1)?)?;
            assert_eq!(driver.engine.head()?.number, 1);
            let expected = if probe == "p1" {
                vec![false]
            } else {
                vec![true, false]
            };
            assert_eq!(
                d.decisions.iter().map(|x| x.forward).collect::<Vec<_>>(),
                expected
            );
            for decision in d.decisions.iter().filter(|x| !x.forward) {
                assert_eq!(decision.rule, "USER-GAS-EXCEEDS-BLOCK-BUDGET");
            }
            let observed = observe(node, &d)?;
            if probe == "p1" {
                assert!(observed["receipts"][0].is_null());
            } else {
                assert_eq!(observed["receipts"][0]["status"], "0x0");
                assert_eq!(quantity(&observed["receipts"][0]["gasUsed"])?, 16_000_000);
                assert!(observed["receipts"][1].is_null());
            }
            let record: Value = serde_json::from_str(fs::read_to_string(evidence)?.trim())?;
            assert_eq!(record["outcome"], "DERIVED");
            assert_eq!(
                record["engine_calls"]
                    .as_array()
                    .context("probe calls")?
                    .len(),
                4
            );
            observations.push(observed);
            records.push(record);
        }
        assert_eq!(observations[0], observations[1]);
        assert_eq!(records[0], records[1]);
        fs::write(
            runtime.path.join("probe-result.json"),
            serde_json::to_vec_pretty(
                &json!({"probe":probe,"identical":true,"observations":observations,"records":records}),
            )?,
        )?;
        eprintln!("I3 {probe}: PASS identical blocks, no stall");
        runtime.finish(true)?;
    }
    Ok(())
}
