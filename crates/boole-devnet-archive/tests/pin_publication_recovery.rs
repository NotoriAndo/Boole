//! T88: SIGKILL at each bytes -> pin -> selected binding -> checkpoint cut.
#![cfg(feature = "qualification-faults")]
use anyhow::{ensure, Result};
use boole_derivation::{
    attributes::hex_string,
    m2::{Config, State},
    rlp,
    user_tx::keccak,
    ParentHeader,
};
use boole_devnet_archive::{
    durability::PrivateTempDir,
    fault,
    input::{receipts_root, InputRow},
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
// Test-local copy of the authenticated empty-header constructor; no runtime fixture API.
fn empty_fixture(number: u64, parent: [u8; 32]) -> InputRow {
    let fields = vec![
        rlp::bytes(&parent),
        rlp::bytes(&[0; 32]),
        rlp::bytes(&[0; 20]),
        rlp::bytes(&[0; 32]),
        rlp::bytes(&receipts_root(&[])),
        rlp::bytes(&receipts_root(&[])),
        rlp::bytes(&[0; 256]),
        rlp::uint(0),
        rlp::uint(number),
        rlp::uint(60_000_000),
        rlp::uint(0),
        rlp::uint(1_790_000_000 + 12 * number),
        rlp::bytes(&[]),
        rlp::bytes(&[1; 32]),
        rlp::bytes(&[0; 8]),
        rlp::uint(1),
        rlp::bytes(&[0; 32]),
        rlp::uint(0),
        rlp::uint(0),
        rlp::bytes(&[0; 32]),
        rlp::bytes(&[0; 32]),
    ];
    let raw = rlp::list(&fields);
    InputRow {
        schema_version: 1,
        block_hash: hex_string(&keccak(&raw)),
        header_rlp: hex_string(&raw),
        receipts: Some(vec![]),
        transactions: Some(vec![]),
    }
}

fn append_through(source: &mut InputJournal, height: u64) -> Result<()> {
    while source.tip() < height {
        let h = source.tip() + 1;
        let parent =
            boole_derivation::attributes::hex_bytes(source.hash(h - 1).unwrap(), "parent")?
                .try_into()
                .unwrap();
        source.append(&empty_fixture(h, parent))?;
    }
    Ok(())
}
fn binding(source: &InputJournal, h: u64) -> Result<HeaderBinding> {
    if h == 0 {
        return genesis(source);
    }
    let l1 = source.get(h)?.block;
    let root = State::new(&Config::devnet(), 30_000_000)?.root;
    let transactions = vec![
        boole_derivation::system::l1_info(&l1)?,
        boole_derivation::system::m2_root_deposit(&l1, root)?,
    ];
    Ok(HeaderBinding {
        header: ParentHeader {
            number: h,
            hash: hex_string(&Sha256::digest(h.to_be_bytes())),
            parent_hash: Some(if h == 1 {
                source.network().l2_genesis_hash.clone()
            } else {
                hex_string(&Sha256::digest((h - 1).to_be_bytes()))
            }),
            timestamp: h,
            gas_limit: 30_000_000,
            miner: hex_string(&[0; 20]),
            extra_data: "0x".into(),
            l1_info: Some(transactions[0].clone()),
            m2_root_deposit: Some(transactions[1].clone()),
        },
        transactions,
        root,
    })
}
fn publish(source: &mut InputJournal, pins: &mut PinStore) -> Result<()> {
    let digest = Sha256::digest(BODY).into();
    pins.install_bytes(
        ArtifactRole::Environment { component: digest },
        digest,
        BODY,
        false,
    )?;
    append_through(source, 5)?;
    let b = binding(source, 5)?;
    pins.prepare(source, b.clone(), &[])?;
    pins.select(5, &b.header.hash, b.root)?;
    publish_new(
        &source.root().join("pin-checkpoint-5.json"),
        &serde_json::to_vec(&pins.view().anchor(5)?)?,
    )
}
#[test]
#[ignore = "fixture subprocess only"]
fn pin_publication_child() -> Result<()> {
    let root = std::env::var_os("BOOLE_PIN_PUBLICATION_CHILD").unwrap();
    let mut source = source(Path::new(&root))?;
    let mut pins = PinStore::open(&source)?;
    publish(&mut source, &mut pins)
}
#[test]
fn t88_nonzero_restart_preserves_bytes_and_resumes_the_incremental_writer() -> Result<()> {
    for point in [
        "K4-PIN-BYTES",
        "K4-PIN-RECORD",
        "K4-PIN-PREPARED",
        "K4-PIN-SELECTED",
    ] {
        let dir = PrivateTempDir::new("m4c2-pin-kill")?;
        let marker = dir.path().join("marker.json");
        let root = dir.path().join("node");
        let initial_checkpoint = {
            let mut source = source(&root)?;
            append_through(&mut source, 4)?;
            let mut pins = PinStore::open(&source)?;
            for h in 0..=4 {
                let b = binding(&source, h)?;
                pins.prepare(&source, b.clone(), &[])?;
                pins.select(h, &b.header.hash, b.root)?;
            }
            let bytes = serde_json::to_vec(&pins.view().anchor(4)?)?;
            publish_new(&root.join("pin-checkpoint-4.json"), &bytes)?;
            bytes
        };
        let mut child = Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "pin_publication_child",
                "--ignored",
                "--nocapture",
            ])
            .env("BOOLE_PIN_PUBLICATION_CHILD", &root)
            .env("BOOLE_M4_KILL_POINT", point)
            .env("BOOLE_M4_KILL_HEIGHT", "*")
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
                !root.join("pin-checkpoint-5.json").exists(),
                "dependent checkpoint escaped the cut"
            );
            ensure!(
                std::fs::read(root.join("pin-checkpoint-4.json"))? == initial_checkpoint,
                "old checkpoint changed at cut"
            );
            let mut source = source(&root)?;
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
            publish(&mut source, &mut pins)?;
            let anchor =
                serde_json::from_slice(&std::fs::read(root.join("pin-checkpoint-5.json"))?)?;
            pins.view().verify_anchor(&anchor)?;
            pins.view().verify(&source, &[], 5)?;
            let prefix = pins.view().record_id();
            pins.refresh(&source)?;
            append_through(&mut source, 6)?;
            let b = binding(&source, 6)?;
            pins.prepare(&source, b.clone(), &[])?;
            pins.select(6, &b.header.hash, b.root)?;
            pins.refresh(&source)?;
            ensure!(
                pins.view().height() == 6 && pins.view().contains_record(prefix),
                "incremental continuation lost the recovered prefix"
            );
            pins.view().verify(&source, &[], 6)?;
            drop(pins);
            let pins = PinStore::open(&source)?;
            ensure!(pins.view().height() == 6, "repeated restart lost height6");
            pins.view().verify_anchor(&anchor)?;
            Ok(())
        })();
        let _ = child.kill();
        let _ = child.wait();
        outcome?;
    }
    Ok(())
}
