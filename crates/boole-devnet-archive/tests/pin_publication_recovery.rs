//! T88: SIGKILL at each bytes -> pin -> selected binding -> checkpoint cut.
#![cfg(feature = "qualification-faults")]
use anyhow::{ensure, Result};
use boole_derivation::{
    attributes::hex_string,
    m2::{Config, State},
    ParentHeader,
};
use boole_devnet_archive::{
    durability::PrivateTempDir,
    fault,
    input::InputRow,
    journal::{publish_new, InputJournal, NetworkIdentity},
    pins::{ArtifactRole, HeaderBinding, PinStore},
};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
const BODY: &[u8] = b"public retained stub environment fixture";
fn source(root: &Path) -> Result<InputJournal> {
    let value: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m4/input-auth-v0.json"
    ))?;
    let anchor: InputRow = serde_json::from_value(value["rows"][0].clone())?;
    let identity = NetworkIdentity::new(&Config::devnet(), None, hex_string(&[1; 32]), 30_000_000)?;
    InputJournal::open(root, identity, || Ok(anchor), false)
}
fn genesis(source: &InputJournal) -> Result<HeaderBinding> {
    Ok(HeaderBinding {
        header: ParentHeader {
            number: 0,
            hash: source.network().l2_genesis_hash.clone(),
            parent_hash: None,
            timestamp: 0,
            gas_limit: 30_000_000,
            miner: hex_string(&[0; 20]),
            extra_data: "0x".into(),
            l1_info: None,
            m2_root_deposit: None,
        },
        transactions: vec![],
        root: State::new(&Config::devnet(), 30_000_000)?.root,
    })
}
fn publish(source: &InputJournal, pins: &mut PinStore) -> Result<()> {
    let digest = Sha256::digest(BODY).into();
    pins.install_bytes(
        ArtifactRole::Environment { component: digest },
        digest,
        BODY,
        false,
    )?;
    let b = genesis(source)?;
    pins.prepare(source, b.clone(), &[])?;
    pins.select(0, &b.header.hash, b.root)?;
    publish_new(
        &source.root().join("pin-checkpoint.json"),
        &serde_json::to_vec(&pins.view().anchor(0)?)?,
    )
}
#[test]
#[ignore = "fixture subprocess only"]
fn pin_publication_child() -> Result<()> {
    let root = std::env::var_os("BOOLE_PIN_PUBLICATION_CHILD").unwrap();
    let source = source(Path::new(&root))?;
    let mut pins = PinStore::open(&source)?;
    publish(&source, &mut pins)
}
#[test]
fn t88_restart_preserves_bytes_at_every_pin_publication_boundary() -> Result<()> {
    for point in [
        "K4-PIN-BYTES",
        "K4-PIN-RECORD",
        "K4-PIN-PREPARED",
        "K4-PIN-SELECTED",
    ] {
        let dir = PrivateTempDir::new("m4c2-pin-kill")?;
        let marker = dir.path().join("marker.json");
        let root = dir.path().join("node");
        let mut child = Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "pin_publication_child",
                "--ignored",
                "--nocapture",
            ])
            .env("BOOLE_PIN_PUBLICATION_CHILD", &root)
            .env("BOOLE_M4_KILL_POINT", point)
            .env("BOOLE_M4_KILL_HEIGHT", "0")
            .env("BOOLE_M4_KILL_ROLE", "pins")
            .env("BOOLE_M4_KILL_MARKER", &marker)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let outcome = (|| -> Result<()> {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if fault::stopped_marker(&marker, child.id())?.is_some() {
                    break;
                }
                ensure!(
                    child.try_wait()?.is_none(),
                    "{point}: child exited before the cut"
                );
                ensure!(Instant::now() < deadline, "{point}: marker deadline");
                std::thread::sleep(Duration::from_millis(10));
            }
            child.kill()?;
            ensure!(!child.wait()?.success(), "SIGKILL required");
            ensure!(
                !root.join("pin-checkpoint.json").exists(),
                "dependent checkpoint escaped the cut"
            );
            let source = source(&root)?;
            let mut pins = PinStore::open(&source)?;
            let digest = Sha256::digest(BODY).into();
            ensure!(
                std::fs::read(pins.view().artifact_path(digest))? == BODY,
                "{point}: required bytes were erased by recovery"
            );
            if point == "K4-PIN-BYTES" {
                ensure!(
                    pins.view()
                        .artifact(&ArtifactRole::Environment { component: digest })
                        .is_none(),
                    "uncommitted bytes falsely became durable pin authority"
                );
            }
            publish(&source, &mut pins)?;
            let anchor = serde_json::from_slice(&std::fs::read(root.join("pin-checkpoint.json"))?)?;
            pins.view().verify_anchor(&anchor)?;
            pins.view().verify(&source, &[], 0)?;
            Ok(())
        })();
        let _ = child.kill();
        let _ = child.wait();
        outcome?;
    }
    Ok(())
}
