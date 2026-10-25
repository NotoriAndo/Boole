//! Select retention from real frozen v1.3 histories without changing their bytes.
use anyhow::{ensure, Context, Result};
use boole_derivation::{
    attributes::{hex_bytes, hex_string},
    m2::{Config, State, Status},
    m3::{decode_verdict_deposits, ForkTable},
    rlp, InboxLog, L1Block, ParentHeader,
};
use boole_devnet_archive::pins::{requirements, Reason};
use serde_json::Value;
#[test]
fn t88_real_fork_histories_keep_original_gate_and_every_input_height() -> Result<()> {
    let mut saw_quarantine_void = false;
    let mut saw_stale = false;
    let mut saw_two_epochs = false;
    for name in [
        "round",
        "kernel-fix",
        "quarantine-void",
        "quarantine-recheck",
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/devnet-m3")
            .join(format!("{name}-v1.3.json"));
        let tape: Value = serde_json::from_slice(&std::fs::read(path)?)?;
        let c: Config = serde_json::from_str(tape["config_json"].as_str().context("config")?)?;
        let forks: ForkTable = serde_json::from_value(tape["forks"].clone())?;
        let mut state = State::new(&c, 30_000_000)?;
        for row in tape["heights"].as_array().context("heights")? {
            let l1: L1Block = serde_json::from_value(row["l1"].clone())?;
            let logs: Vec<InboxLog> = serde_json::from_value(row["logs"].clone())?;
            let parent: ParentHeader = serde_json::from_value(row["parent"].clone())?;
            let txs: Vec<String> = serde_json::from_value(row["result"]["transactions"].clone())?;
            let mut deposits = vec![];
            for tx in txs.iter().skip(2).take_while(|t| t.starts_with("0x7e")) {
                let raw = hex_bytes(tx, "deposit")?;
                let fields = rlp::decode(&raw[1..]).map_err(anyhow::Error::msg)?;
                if fields
                    .children
                    .context("fields")?
                    .get(7)
                    .context("data")?
                    .bytes
                    .starts_with(b"m3vd")
                {
                    deposits.push(tx.clone());
                }
            }
            let records = decode_verdict_deposits(&c, &l1, &deposits)?;
            let (_, next) = boole_derivation::attributes::derive_with_m3(
                &state, &c, &l1, &logs, &parent, &records, &forks,
            )?;
            ensure!(
                hex_string(&next.root) == row["result"]["m2_root"],
                "frozen root changed"
            );
            let pins = requirements(&next, &c, &forks, true)?;
            for pin in &pins {
                let candidate = next.candidates[&pin.key]
                    .live()
                    .context("retained live metadata")?;
                let open = candidate.open.as_ref().context("OPEN")?;
                let ranges = pin.intervals();
                ensure!(ranges.contains(candidate.h_c), "COMMIT omitted");
                ensure!(
                    ranges.contains(next.problems[&pin.key.0].reveal_pos.0),
                    "reveal omitted"
                );
                for h in open.h_o..=open.h_r.context("completion")? {
                    ensure!(ranges.contains(h), "interior PART height omitted");
                }
                ensure!(
                    pin.components.contains(&c.base_root),
                    "base artifact omitted"
                );
                if name == "kernel-fix" && candidate.status == Status::Stale {
                    ensure!(pin.reasons.contains(&Reason::Stale), "STALE omitted");
                    ensure!(
                        pin.gates.iter().any(|(epoch, _)| *epoch == 0),
                        "original gate omitted"
                    );
                    ensure!(
                        pin.gates.iter().any(|(epoch, _)| *epoch == 1),
                        "recheck gate omitted"
                    );
                    saw_stale = true;
                    saw_two_epochs = true;
                }
                if name == "quarantine-void" && candidate.status == Status::Void {
                    ensure!(
                        pin.reasons.contains(&Reason::Quarantine),
                        "quarantine VOID inputs lost"
                    );
                    ensure!(
                        pin.gates.iter().any(|(epoch, _)| *epoch == 0),
                        "quarantine lost old gate"
                    );
                    saw_quarantine_void = true;
                }
            }
            // Terminal K4 candidates must stay selected through later expiry heights.
            for (key, candidate) in &next.candidates {
                if matches!(candidate.status(), Status::Accepted | Status::Stale) {
                    ensure!(pins.iter().any(|p| p.key == *key), "K4 candidate omitted");
                }
            }
            state = next;
        }
    }
    ensure!(
        saw_stale && saw_two_epochs && saw_quarantine_void,
        "frozen controls were not exercised"
    );
    Ok(())
}
