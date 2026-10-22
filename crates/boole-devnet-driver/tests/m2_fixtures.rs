//! Accepted client recording remains replayable by the pure fold.
use anyhow::{Context, Result};
use boole_derivation::{
    attributes::hex_string,
    derive_with_m2,
    m2::{Config, State, Status},
    InboxLog, L1Block, ParentHeader,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
#[test]
fn accepted_m2_recording_has_provenance_and_pure_replay() -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let value: Value =
        serde_json::from_slice(&fs::read(root.join("devnet-m2/commit-reveal-v1.json"))?)?;
    assert_eq!(value["acceptance"]["live_abc"], true);
    assert_eq!(value["acceptance"]["recorded_abc"], true);
    assert_eq!(value["acceptance"]["restart_height"], 17);
    assert_eq!(value["acceptance"]["unfinalized_reorg"], true);
    assert_eq!(
        value["provenance"]["source_genesis_sha256"],
        hex_string(&Sha256::digest(fs::read(
            root.join("devnet-m1/l2-genesis.json")
        )?))
    );
    assert_eq!(
        value["provenance"]["source_l1_genesis_sha256"],
        format!(
            "{:x}",
            Sha256::digest(fs::read(root.join("devnet-m1/l1-genesis.json"))?)
        )
    );
    assert_eq!(
        value["provenance"]["l1_genesis_sha256"],
        format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec_pretty(&value["l1_genesis"])?)
        )
    );
    let c: Config = serde_json::from_value(value["config"].clone())?;
    let gas = u64::from_str_radix(
        value["l2_genesis"]["gasLimit"]
            .as_str()
            .context("gas limit")?
            .trim_start_matches("0x"),
        16,
    )?;
    let mut state = State::new(&c, gas)?;
    for row in value["heights"].as_array().context("heights")? {
        let l1: L1Block = serde_json::from_value(row["l1"].clone())?;
        let logs: Vec<InboxLog> = serde_json::from_value(row["logs"].clone())?;
        let parent: ParentHeader = serde_json::from_value(row["parent"].clone())?;
        let (d, next) = derive_with_m2(&state, &c, &l1, &logs, &parent)?;
        assert_eq!(
            serde_json::to_value(&d.attributes)?,
            row["result"]["attributes"]
        );
        assert_eq!(
            serde_json::to_value(&d.decisions)?,
            row["result"]["decisions"]
        );
        assert_eq!(hex_string(&next.root), row["result"]["m2_root"]);
        assert!(d
            .decisions
            .iter()
            .filter(|x| x.m2.is_some())
            .all(|x| !x.forward));
        state = next;
    }
    let checking = state
        .candidates
        .values()
        .find(|x| x.status() == Status::Checking)
        .context("completed candidate")?;
    let open = checking
        .live()
        .context("live")?
        .open
        .as_ref()
        .context("open")?;
    assert_eq!(open.n, 14);
    assert_eq!(open.h_r, Some(18));
    assert_eq!(Value::from(open.raw_len), value["codec"]["raw_len"]);
    assert!(state
        .candidates
        .values()
        .any(|x| x.status() == Status::Cancelled));
    assert!(state
        .candidates
        .values()
        .any(|x| x.status() == Status::Expired));
    assert_eq!(
        value["gate_seam"]["export_digest"],
        value["codec"]["export_digest"]
    );
    Ok(())
}
