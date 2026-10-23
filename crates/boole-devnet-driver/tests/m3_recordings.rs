//! Offline replay of independently compared v1.3 live tapes, with byte-input stub recomputation.
use anyhow::{ensure, Context, Result};
use boole_derivation::{
    attributes::hex_string,
    m2::{Config, State},
    m3::*,
    InboxLog, L1Block, ParentHeader,
};
use boole_gate_supervisor::{
    codec::{digest, Codec},
    rebuild::{self, History},
    stub::{self, Descriptor},
    supervisor::{checked_record, jobs_at_height},
    GateReply,
};
use serde_json::Value;
use std::{fs, path::Path};
struct Tape<'a>(&'a [Value]);
impl History for Tape<'_> {
    fn origin(&mut self, h: u64) -> Result<(L1Block, Vec<InboxLog>)> {
        let row = self
            .0
            .get(h.checked_sub(1).context("genesis not a job input")? as usize)
            .context("origin row")?;
        let l1: L1Block = serde_json::from_value(row["l1"].clone())?;
        ensure!(
            row["result"]["transactions"][0] == boole_derivation::system::l1_info(&l1)?,
            "frozen L1-info anchor"
        );
        Ok((l1, serde_json::from_value(row["logs"].clone())?))
    }
}
#[test]
fn v13_live_tapes_recompute_stub_verdicts_and_replay_every_root_and_attribute() -> Result<()> {
    for file in [
        "round-v1.3.json",
        "kernel-fix-v1.3.json",
        "quarantine-void-v1.3.json",
        "quarantine-recheck-v1.3.json",
        "reorg-v1.3.json",
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/devnet-m3")
            .join(file);
        let v: Value = serde_json::from_slice(&fs::read(path)?)?;
        ensure!(
            v["schema"] == "boole-devnet-m3-recording-v1.3" && v["acceptance"]["live_abc"] == true,
            "recording acceptance"
        );
        ensure!(
            v["provenance"]["generator_sha256"]
                == hex_string(&digest(if file == "reorg-v1.3.json" {
                    include_bytes!("m3_reorg.rs").as_slice()
                } else {
                    include_bytes!("m3_devnet.rs").as_slice()
                })),
            "tracked producer changed; regenerate recordings"
        );
        ensure!(
            v["provenance"]["support_sha256"]
                == hex_string(&digest(include_bytes!("support/mod.rs")))
                && v["provenance"]["wire_support_sha256"]
                    == hex_string(&digest(include_bytes!("support/m2.rs"))),
            "shared producer changed"
        );
        ensure!(
            v["cleanup"]["log_issues"]
                .as_array()
                .context("cleanup log issues")?
                .is_empty(),
            "unexpected live warning/error"
        );
        let c: Config =
            serde_json::from_str(v["config_json"].as_str().context("canonical config JSON")?)?;
        let forks: ForkTable = serde_json::from_value(v["forks"].clone())?;
        let descriptors: Vec<Descriptor> = serde_json::from_value(v["gate_descriptors"].clone())?;
        let rows = v["heights"].as_array().context("heights")?;
        ensure!(
            rows.len() == if file == "reorg-v1.3.json" { 30 } else { 100 },
            "incomplete tape"
        );
        let mut state = State::new(&c, 30_000_000)?;
        let codec = Codec::portable_stub();
        let mut tape = Tape(rows);
        for row in rows {
            let l1: L1Block = serde_json::from_value(row["l1"].clone())?;
            let logs: Vec<InboxLog> = serde_json::from_value(row["logs"].clone())?;
            let parent: ParentHeader = serde_json::from_value(row["parent"].clone())?;
            let mut computed = vec![];
            for (job, forced) in jobs_at_height(&state, l1.number, &forks)?
                .into_iter()
                .skip(state.m3.as_ref().unwrap().next_applied)
            {
                let epoch = job
                    .epoch_override
                    .map(Ok)
                    .unwrap_or_else(|| forks.job_epoch(job.avail, c.m3.as_ref().unwrap().k))?;
                let descriptor = descriptors
                    .iter()
                    .find(|d| d.digest() == forks.rows[epoch as usize].gate_rule_digest)
                    .context("epoch descriptor")?;
                let mut input = rebuild::rebuild(
                    &state,
                    &c,
                    job.key,
                    &mut tape,
                    Some(&codec),
                    &std::env::temp_dir(),
                )?
                .input;
                if forced {
                    input.early = Some((3, QUARANTINE_VOID));
                }
                match stub::evaluate(descriptor, &input) {
                    GateReply::Result { value } => computed.push(IndexedVerdict {
                        queue_index: job.queue_index,
                        record: checked_record(&input, descriptor, &job, epoch, &value)?,
                    }),
                    GateReply::Defer { .. } => break,
                }
            }
            let (d, next) = boole_derivation::attributes::derive_with_m3(
                &state, &c, &l1, &logs, &parent, &computed, &forks,
            )?;
            ensure!(
                serde_json::to_value(&d.attributes)? == row["result"]["attributes"]
                    && serde_json::to_value(&d.decisions)? == row["result"]["decisions"],
                "{file} attributes/decisions at {}",
                l1.number
            );
            ensure!(
                hex_string(&next.root) == row["result"]["m2_root"],
                "{file} root at {}",
                l1.number
            );
            let raw: Vec<String> = serde_json::from_value(row["result"]["transactions"].clone())?;
            boole_devnet_driver::driver::check_payload_transactions(
                &serde_json::json!({"transactions":raw}),
                &d,
            )?;
            state = next;
        }
        ensure!(
            hex_string(&state.root) == v["final_state"]["root"],
            "final root"
        );
    }
    Ok(())
}
