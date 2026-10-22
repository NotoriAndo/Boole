//! Accepted client recording remains replayable by the pure fold.
#[allow(dead_code)]
mod support;
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

#[test]
fn review_i6_i7_recordings_replay_against_pure_model() -> Result<()> {
    for name in ["adversarial-v1.json", "rounds-v1.json"] {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/devnet-m2");
        let value: Value = serde_json::from_slice(&fs::read(root.join(name))?)?;
        let mut generated = value.clone();
        support::m2::bind_provenance(&mut generated)?;
        assert_eq!(generated["provenance"], value["provenance"]);
        assert_eq!(generated["l1_genesis"], value["l1_genesis"]);
        assert!(value["acceptance"]
            .get("comparison_output_sha256")
            .is_none());
        let c: Config = serde_json::from_value(value["config"].clone())?;
        let mut state = State::new(&c, 30_000_000)?;
        for row in value["heights"].as_array().context("heights")? {
            let l1: L1Block = serde_json::from_value(row["l1"].clone())?;
            let logs: Vec<InboxLog> = serde_json::from_value(row["logs"].clone())?;
            let parent: ParentHeader = serde_json::from_value(row["parent"].clone())?;
            let (d, next) = derive_with_m2(&state, &c, &l1, &logs, &parent)?;
            assert_eq!(hex_string(&next.root), row["result"]["m2_root"]);
            assert_eq!(
                serde_json::to_value(d.attributes)?,
                row["result"]["attributes"]
            );
            assert_eq!(
                serde_json::to_value(d.decisions)?,
                row["result"]["decisions"]
            );
            state = next;
        }
        assert_eq!(value["acceptance"]["live_abc"], true);
        assert_eq!(value["acceptance"]["recorded_abc"], true);
        if name.starts_with("adversarial") {
            let codes: Vec<_> = value["heights"][15]["result"]["decisions"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|d| d["m2"]["code"].as_u64())
                .collect();
            assert_eq!(codes.iter().filter(|c| **c == 1).count(), 200);
            assert_eq!(codes.iter().filter(|c| **c == 128).count(), 937);
            assert_eq!(codes.iter().filter(|c| **c == 11).count(), 1);
            assert_eq!(&codes[codes.len() - 3..], &[8, 129, 131]);
        } else {
            assert_eq!(
                state.rounds[&(1, 1)].status,
                boole_derivation::m2::RoundStatus::Withheld
            );
            let selected: std::collections::BTreeSet<_> =
                state.rounds.values().map(|r| r.leaf_index).collect();
            assert_eq!(selected.len(), state.rounds.len());
        }
    }
    Ok(())
}

#[test]
fn review_tracked_generator_reproduces_provenance() -> Result<()> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/devnet-m2/commit-reveal-v1.json");
    let value: Value = serde_json::from_slice(&fs::read(path)?)?;
    let mut generated = value.clone();
    support::m2::bind_provenance(&mut generated)?;
    assert_eq!(generated["provenance"], value["provenance"]);
    assert_eq!(generated["l1_genesis"], value["l1_genesis"]);
    assert!(value["acceptance"]
        .get("comparison_output_sha256")
        .is_none());
    assert_eq!(
        value["codec"]["binary_sha256"],
        serde_json::from_slice::<Value>(include_bytes!(
            "../../../fixtures/devnet-m2/zstd-pin-v1.json"
        ))?["sha256"]
    );
    Ok(())
}
