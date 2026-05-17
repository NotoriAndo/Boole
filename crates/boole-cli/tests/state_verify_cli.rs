//! P2.8 — `boole state verify` offline CLI.
//!
//! Operators need a way to ask "is my durable block log consistent?"
//! without booting a `boole-node`. Today the only paths that exercise
//! [`boole_core::replay_blocks`] are the per-fixture `chain replay`
//! command (which loads a one-shot JSON envelope, not a live state
//! directory) and `boole-node` boot (which acquires the state lock
//! and serves traffic). Neither is appropriate for the recovery
//! playbook step "boole state verify --report-json shows mismatched
//! (c, pk, amount)" called out in the production-readiness master plan.
//!
//! Contract: `boole state verify --blocks <ndjson> --json` reads the
//! NDJSON block log via the same `FileBlockStore::recover` shape the
//! node uses at boot, runs `replay_blocks`, and emits an envelope with
//! `{ok, height, latestC}`. Exit code 0 on a clean replay; nonzero on
//! any error (missing file, invalid line, chain break).

use std::io::Write as _;
use std::process::Command;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StateVerifyOutput {
    ok: bool,
    height: u64,
    latest_c: String,
    block_count: u64,
}

#[derive(Debug, Deserialize)]
struct Fixture {
    blocks: Vec<serde_json::Value>,
    expected: Expected,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Expected {
    latest_c: String,
    height: u64,
}

#[test]
fn cli_state_verify_replays_durable_blocks_and_reports_height() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/replay/v1.json"))
            .expect("fixture parses");

    let dir = std::env::temp_dir().join(format!("boole-cli-state-verify-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let blocks_path = dir.join("blocks.ndjson");

    let mut file = std::fs::File::create(&blocks_path).expect("create blocks");
    for block in &fixture.blocks {
        writeln!(
            file,
            "{}",
            serde_json::to_string(block).expect("block json")
        )
        .expect("write block");
    }

    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "state",
            "verify",
            "--blocks",
            blocks_path.to_str().expect("utf8 blocks path"),
            "--json",
        ])
        .output()
        .expect("run boole-cli");

    assert!(
        output.status.success(),
        "state verify must succeed for a clean block log; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let parsed: StateVerifyOutput =
        serde_json::from_slice(&output.stdout).expect("json envelope on stdout");
    assert!(parsed.ok, "ok=true expected on clean replay");
    assert_eq!(parsed.height, fixture.expected.height);
    assert_eq!(parsed.latest_c, fixture.expected.latest_c);
    assert_eq!(parsed.block_count, fixture.blocks.len() as u64);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cli_state_verify_rejects_a_tampered_block_log_with_nonzero_exit() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/replay/v1.json"))
            .expect("fixture parses");

    let dir = std::env::temp_dir().join(format!(
        "boole-cli-state-verify-tampered-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let blocks_path = dir.join("blocks.ndjson");

    // Tamper: write only block[1] (height 1) without block[0], breaking
    // FileBlockStore::recover's height==line_index invariant.
    let only_second = fixture
        .blocks
        .get(1)
        .expect("fixture has at least two blocks");
    std::fs::write(
        &blocks_path,
        format!(
            "{}\n",
            serde_json::to_string(only_second).expect("block json")
        ),
    )
    .expect("write tampered block");

    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "state",
            "verify",
            "--blocks",
            blocks_path.to_str().expect("utf8 blocks path"),
            "--json",
        ])
        .output()
        .expect("run boole-cli");

    assert!(
        !output.status.success(),
        "tampered block log must produce nonzero exit; stdout={}",
        String::from_utf8_lossy(&output.stdout)
    );

    let _ = std::fs::remove_dir_all(&dir);
}
