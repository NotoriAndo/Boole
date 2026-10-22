//! Restore real-client coverage of M1 cases 10 and 13 without changing old fixtures.
#[allow(dead_code)]
mod support;
use anyhow::{bail, Context, Result};
use boole_derivation::{attributes::hex_string, Derived};
use boole_devnet_driver::{
    driver::{Driver, StepOutcome},
    engine::Engine,
    l1::{RecordedL1, RpcL1},
};
use serde_json::{json, Value};
use std::fs;
use support::m2::hash;
use support::*;
const NEW: &str = "s1b-recorded-schedule-budget-v2.json";
fn derived(out: StepOutcome) -> Result<Derived> {
    match out {
        StepOutcome::Derived(d) => Ok(*d),
        _ => bail!("unexpected wait"),
    }
}
fn schedule() -> Result<Vec<Value>> {
    let original = fixture_json("s1b-cases.json")?;
    let mut rows = original["main"]
        .as_array()
        .context("main schedule")?
        .clone();
    let third = rows[23]["raws"].as_array_mut().unwrap().remove(2);
    rows[23]["expect"] = json!([1, 1]);
    rows[24]["raws"].as_array_mut().unwrap().insert(0, third);
    rows[24]["expect"] = json!([1, 1]);
    Ok(rows)
}
#[test]
fn new_schedule_fixture_preserves_raws_and_binds_provenance() -> Result<()> {
    let f = fixture_json(NEW)?;
    assert_eq!(f["acceptance"]["live_abc"], true);
    assert_eq!(f["acceptance"]["recorded_abc"], true);
    assert_eq!(
        f["provenance"]["source_cases_sha256"],
        hex_string(&hash(&fs::read(fixture("s1b-cases.json"))?))
    );
    assert_eq!(f["schedule"], json!(schedule()?));
    let mut old: Vec<_> = fixture_json("s1b-cases.json")?["main"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|r| r["raws"].as_array().unwrap().iter().map(|x| x.to_string()))
        .collect();
    let mut new: Vec<_> = f["schedule"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|r| r["raws"].as_array().unwrap().iter().map(|x| x.to_string()))
        .collect();
    old.sort();
    new.sort();
    assert_eq!(old, new);
    assert_eq!(
        f["heights"][25]["result"]["observed"]["receipts"][0]["status"],
        "0x1"
    );
    assert!(f["heights"][25]["result"]["observed"]["receipts"][1].is_null());
    assert_eq!(
        f["heights"][33]["result"]["observed"]["receipts"][0]["status"],
        "0x0"
    );
    assert!(f["heights"][33]["result"]["execution_failure_fee"].is_object());
    Ok(())
}
#[test]
fn m1_budget_compliant_schedule_live_and_recorded_abc() -> Result<()> {
    let Some(binary) = pinned("BOOLE_DEVNET_OP_RETH", "op_reth")? else {
        return Ok(());
    };
    let Some(anvil) = pinned("BOOLE_DEVNET_ANVIL", "anvil")? else {
        return Ok(());
    };
    let mut rt = Runtime::new("m1-schedule-budget-v2")?;
    let l1 = rt.anvil(&anvil)?;
    let accounts = l1.call("eth_accounts", json!([]))?;
    let user = accounts[0].as_str().context("account")?;
    let plan = schedule()?;
    let mut rows = vec![];
    for case in &plan {
        let h = case["height"].as_u64().context("height")?;
        let mut posts = vec![];
        for raw in case["raws"].as_array().context("raws")? {
            posts.push(l1.call("eth_sendTransaction",json!([{"from":user,"to":boole_derivation::config::INBOX,"data":post_calldata(raw.as_str().context("raw")?)?,"gas":"0xf4240"}]))?);
        }
        l1.call("evm_mine", json!([1_800_000_000 + 12 * h]))?;
        for tx in posts {
            let receipt = l1.call("eth_getTransactionReceipt", json!([tx]))?;
            assert_eq!(receipt["status"], "0x1");
        }
        let block = l1.call("eth_getBlockByNumber", json!([format!("0x{h:x}"), false]))?;
        let logs=l1.call("eth_getLogs",json!([{"blockHash":block["hash"],"address":boole_derivation::config::INBOX,"topics":[boole_derivation::config::posted_topic()]}]))?;
        let block: boole_derivation::L1Block = serde_json::from_value(block)?;
        rows.push(json!({"height":h,"l1":block,"logs":logs}));
    }
    l1.call("anvil_mine", json!(["0x8"]))?;
    let g = fixture("l2-genesis.json");
    let mut expected = vec![];
    for (index, name) in ["a", "b", "c"].into_iter().enumerate() {
        let node = rt.node(&binary, &g, name, index as u16)?;
        let mut d = Driver::new(RpcL1::new("http://127.0.0.1:28545")?, node.engine()?);
        d.set_evidence(&rt.path.join(format!("{name}.jsonl")))?;
        for (i, case) in plan.iter().enumerate() {
            let h = (i + 1) as u64;
            let parent = d.engine.head()?;
            let addr = case["addresses"][0].as_str().context("address")?;
            let before = if h == 34 {
                Some(node.state(addr, &parent.hash)?)
            } else {
                None
            };
            let out = derived(d.step(h)?)?;
            let obs = observe(&node, &out)?;
            let statuses: Vec<_> = obs["receipts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| {
                    if r.is_null() {
                        Ok(Value::Null)
                    } else {
                        Ok(json!(quantity(&r["status"])?))
                    }
                })
                .collect::<Result<_>>()?;
            assert_eq!(json!(statuses), case["expect"], "schedule at {h}");
            let mut v = json!({"observed":obs,"attributes":out.attributes,"decisions":out.decisions,"m2_root":hex_string(&d.m2_state().root)});
            if let Some(before) = before {
                let after =
                    node.state(addr, v["observed"]["blockHash"].as_str().context("hash")?)?;
                let receipt = &v["observed"]["receipts"][0];
                assert_eq!(receipt["status"], "0x0");
                let fee = amount(&receipt["gasUsed"])? * amount(&receipt["effectiveGasPrice"])?;
                assert!(fee > 0);
                assert_eq!(
                    amount(&before["balance"])? - amount(&after["balance"])?,
                    fee
                );
                assert_eq!(quantity(&after["nonce"])?, quantity(&before["nonce"])? + 1);
                v["execution_failure_fee"] =
                    json!({"charged":format!("0x{fee:x}"),"before":before,"after":after});
            }
            if index == 0 {
                rows[i]["parent"] = json!(parent);
                rows[i]["result"] = v.clone();
                expected.push(v);
            } else {
                assert_eq!(v, expected[i], "live {name} at {h}");
            }
        }
        rt.stop(name)?;
    }
    let text = serde_json::to_string(&rows)?;
    for (index, name) in ["replay-a", "replay-b", "replay-c"].into_iter().enumerate() {
        let node = rt.node(&binary, &g, name, index as u16)?;
        let mut d = Driver::new(RecordedL1::from_json(&text)?, node.engine()?);
        d.set_evidence(&rt.path.join(format!("{name}.jsonl")))?;
        for (i, row) in rows.iter().enumerate() {
            let out = derived(d.step((i + 1) as u64)?)?;
            let observed = observe(&node, &out)?;
            assert_eq!(observed, row["result"]["observed"], "recorded {name}");
            assert_eq!(json!(out.attributes), row["result"]["attributes"]);
            assert_eq!(hex_string(&d.m2_state().root), row["result"]["m2_root"]);
        }
        rt.stop(name)?;
    }
    let data = json!({"schema":"boole-devnet-m1-schedule-budget-v2","provenance":{"source_cases":"s1b-cases.json","source_cases_sha256":hex_string(&hash(&fs::read(fixture("s1b-cases.json"))?)),"pins":fixture_json("pins.json")?,"change":"Move the third 12M-gas candidate from height 24 to the beginning of height 25. Preserve every original raw transaction occurrence and restore the sender nonce sequence."},"acceptance":{"live_abc":true,"recorded_abc":true,"case10_included_and_duplicate_skipped":true,"case13_included_status0_and_fee_charged":true},"schedule":plan,"heights":rows});
    fs::write(
        rt.path.join("recording.json"),
        serde_json::to_vec_pretty(&data)?,
    )?;
    rt.finish(true)?;
    if let Some(path) = std::env::var_os("BOOLE_DEVNET_SCHEDULE_RECORDING") {
        fs::write(path, serde_json::to_vec_pretty(&data)?)?;
    }
    eprintln!("M1 schedule PASS: 35 heights live + recorded A/B/C; case10 [1,null], case13 included status0 and fee");
    Ok(())
}
