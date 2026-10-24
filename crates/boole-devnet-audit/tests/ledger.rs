use anyhow::{Context, Result};
use boole_derivation::{
    attributes::hex_bytes,
    m2::{Config, Input, State},
    m3::{decode_verdict_deposits, ForkTable},
    L1Block,
};
use boole_devnet_audit::ledger::Ledger;
use serde_json::Value;
fn frames_from(tape_json: &str) -> Result<Vec<Frame>> {
    let tape: Value = serde_json::from_str(tape_json)?;
    let c: Config = serde_json::from_str(tape["config_json"].as_str().context("config")?)?;
    let f: ForkTable = serde_json::from_value(tape["forks"].clone())?;
    let mut s = State::new(&c, 30_000_000)?;
    let mut out = vec![];
    for row in tape["heights"].as_array().unwrap() {
        let l1: L1Block = serde_json::from_value(row["l1"].clone())?;
        let txs: Vec<String> = serde_json::from_value(row["result"]["transactions"].clone())?;
        let mut deposits = vec![];
        for tx in txs.iter().skip(2).take_while(|tx| tx.starts_with("0x7e")) {
            let raw = hex_bytes(tx, "tx")?;
            let fields = boole_derivation::rlp::decode(&raw[1..])
                .map_err(anyhow::Error::msg)?
                .children
                .unwrap();
            if fields[7].bytes.starts_with(b"m3vd") {
                deposits.push(tx.clone());
            }
        }
        let verdicts = decode_verdict_deposits(&c, &l1, &deposits)?;
        let logs: Vec<boole_derivation::InboxLog> = serde_json::from_value(row["logs"].clone())?;
        let mut inputs = vec![];
        for log in logs {
            let bytes = hex_bytes(&log.data, "item")?;
            if let Ok((kind, body)) = boole_derivation::inbox::item(&bytes) {
                if (2..=15).contains(&kind) {
                    inputs.push(Input {
                        kind,
                        body: body.to_vec(),
                        transaction_index: log.transaction_index,
                        log_index: log.log_index,
                    });
                }
            }
        }
        let step = s.step_with_verdicts(&c, &l1, &inputs, &verdicts, &f)?;
        boole_devnet_audit::verify_fork_events(&f, &s, &step.state, l1.number, &step.events)?;
        let statuses = row["result"]["receipts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["status"] == "0x1")
            .collect();
        out.push((
            s.clone(),
            step.state.clone(),
            c.clone(),
            l1,
            step.events,
            step.payments,
            txs,
            statuses,
        ));
        s = step.state;
    }
    Ok(out)
}
fn frames() -> Result<Vec<Frame>> {
    frames_from(include_str!("../../../fixtures/devnet-m3/round-v1.3.json"))
}
type Frame = (
    State,
    State,
    Config,
    L1Block,
    Vec<u8>,
    Vec<boole_derivation::m3::RewardPayment>,
    Vec<String>,
    Vec<bool>,
);
fn event(events: &[u8], tag: u8) -> Option<Vec<u8>> {
    let mut at = 0;
    while at < events.len() {
        let len = match events[at] {
            1 => 19,
            2 => 66,
            3 => 47,
            4 => 37,
            5 => 89,
            6 => 118,
            7 => 74,
            8 => 70,
            _ => panic!("event format"),
        };
        if events[at] == tag {
            return Some(events[at..at + len].to_vec());
        }
        at += len;
    }
    None
}
fn attack(attack: u8) -> Result<()> {
    let mut ledger = Ledger::default();
    let mut tested = false;
    for (before, after, c, l1, mut events, payments, txs, statuses) in frames()? {
        let reward = event(&events, 6);
        let admission = event(&events, 7);
        if attack == 0 && reward.as_ref().is_some_and(|r| r[117] == 2) {
            events.extend(reward.unwrap());
            assert!(
                ledger
                    .observe(&before, &after, &c, &l1, &events, &payments, &txs, &statuses)
                    .is_err(),
                "reward vested twice accepted"
            );
            tested = true;
            break;
        }
        if attack == 1 && reward.as_ref().is_some_and(|r| r[117] == 1) {
            ledger.observe(
                &before, &after, &c, &l1, &events, &payments, &txs, &statuses,
            )?;
            let mut cancel = reward.unwrap();
            cancel[117] = 3;
            assert!(
                ledger
                    .observe(&after, &after, &c, &l1, &cancel, &[], &[], &[])
                    .is_err(),
                "CANCELLED without STALE accepted"
            );
            tested = true;
            break;
        }
        if attack == 2 && !payments.is_empty() {
            let mut disabled = c.clone();
            disabled.m3.as_mut().unwrap().mint_enabled = false;
            assert!(
                ledger
                    .observe(&before, &after, &disabled, &l1, &events, &payments, &txs, &statuses)
                    .is_err(),
                "disabled mint accepted"
            );
            tested = true;
            break;
        }
        if let Some(admission) = admission.filter(|_| attack == 3) {
            ledger.observe(
                &before, &after, &c, &l1, &events, &payments, &txs, &statuses,
            )?;
            assert!(
                ledger
                    .observe(&after, &after, &c, &l1, &admission, &[], &[], &[])
                    .is_err(),
                "reused admission k accepted"
            );
            tested = true;
            break;
        }
        ledger.observe(
            &before, &after, &c, &l1, &events, &payments, &txs, &statuses,
        )?;
    }
    assert!(tested);
    Ok(())
}
#[test]
fn t82_duplicate_vest_refuses() -> Result<()> {
    attack(0)
}
#[test]
fn t82_cancel_without_stale_refuses() -> Result<()> {
    attack(1)
}
#[test]
fn t82_disabled_mint_refuses() -> Result<()> {
    attack(2)
}
#[test]
fn t82_reused_k_refuses() -> Result<()> {
    attack(3)
}
#[test]
fn t82_totals_equal_the_i13_recorded_vested_amount() -> Result<()> {
    let mut ledger = Ledger::default();
    for (before, after, c, l1, events, payments, txs, statuses) in frames()? {
        ledger.observe(
            &before, &after, &c, &l1, &events, &payments, &txs, &statuses,
        )?;
    }
    let i13: Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m3/miner-client-v1.3.json"
    ))?;
    let amount = i13["rewards"]["rewards"][0]["amount"].as_str().unwrap();
    let totals = ledger.totals();
    assert_eq!(totals["vested"], amount);
    assert_eq!(totals["minted"], amount);
    assert_eq!(totals["locked"], "0");
    Ok(())
}

#[test]
fn fork_and_quarantine_recordings_recompute_the_independent_ledger() -> Result<()> {
    for tape in [
        include_str!("../../../fixtures/devnet-m3/kernel-fix-v1.3.json"),
        include_str!("../../../fixtures/devnet-m3/quarantine-recheck-v1.3.json"),
        include_str!("../../../fixtures/devnet-m3/quarantine-void-v1.3.json"),
    ] {
        let mut ledger = Ledger::default();
        let rows = frames_from(tape)?;
        assert_eq!(rows.len(), 100);
        for (before, after, c, l1, events, payments, txs, statuses) in rows {
            ledger.observe(
                &before, &after, &c, &l1, &events, &payments, &txs, &statuses,
            )?;
            if let Some(mut activation) = event(&events, 8) {
                activation[2] ^= 1;
                let table: Value = serde_json::from_str(tape)?;
                let forks: ForkTable = serde_json::from_value(table["forks"].clone())?;
                assert!(boole_devnet_audit::verify_fork_events(
                    &forks,
                    &before,
                    &after,
                    l1.number,
                    &activation
                )
                .is_err());
            }
        }
    }
    Ok(())
}
