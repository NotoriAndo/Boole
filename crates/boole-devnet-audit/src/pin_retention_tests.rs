//! Portable CI coverage of the actual non-empty rebuild + durable K4 seam.
//! Verdicts are injected through the fold using the pure public stub evaluator;
//! this test does not qualify worker execution or an Engine (I16 does that).
use super::*;
use crate::{Inputs, JournalInputs};
use boole_derivation::{
    attributes::{hex_bytes, hex_string},
    rlp,
    user_tx::keccak,
    ParentHeader,
};
use boole_devnet_archive::{
    durability::PrivateTempDir,
    input::{receipts_root, InputRow},
    journal::{InputJournal, NetworkIdentity, ReadOnlyJournal},
    pins::{ArtifactRole, HeaderBinding, PinStore},
};
use boole_gate_supervisor::{
    codec::{digest, Codec},
    rebuild,
    stub::{self, Descriptor},
    supervisor::{self, install_devnet_artifacts},
    GateReply,
};
use std::collections::BTreeMap;
fn row(h: u64, parent: Hash, items: &[Input]) -> Result<InputRow> {
    let inbox = hex_bytes(boole_derivation::config::INBOX, "inbox")?;
    let topic = hex_bytes(&boole_derivation::config::posted_topic(), "topic")?;
    let mut bloom = [0u8; 256];
    if !items.is_empty() {
        for bytes in [&inbox, &topic] {
            let k = keccak(bytes);
            for i in 0..3 {
                let bit = usize::from(u16::from_be_bytes([k[2 * i], k[2 * i + 1]]) & 2047);
                bloom[255 - bit / 8] |= 1 << (bit % 8);
            }
        }
    }
    let logs: Vec<_> = items
        .iter()
        .map(|item| {
            let item = [vec![item.kind], item.body.clone()].concat();
            let mut data = vec![0; 64];
            data[31] = 32;
            data[56..64].copy_from_slice(&(item.len() as u64).to_be_bytes());
            data.extend(item);
            data.resize(data.len().div_ceil(32) * 32, 0);
            rlp::list(&[
                rlp::bytes(&inbox),
                rlp::list(&[rlp::bytes(&topic)]),
                rlp::bytes(&data),
            ])
        })
        .collect();
    let receipts = if logs.is_empty() {
        vec![]
    } else {
        vec![rlp::list(&[
            rlp::uint(1),
            rlp::uint(21000),
            rlp::bytes(&bloom),
            rlp::list(&logs),
        ])]
    };
    let raw = rlp::list(&[
        rlp::bytes(&parent),
        rlp::bytes(&[0; 32]),
        rlp::bytes(&[0; 20]),
        rlp::bytes(&[0; 32]),
        rlp::bytes(&receipts_root(&[])),
        rlp::bytes(&receipts_root(&receipts)),
        rlp::bytes(&bloom),
        rlp::uint(0),
        rlp::uint(h),
        rlp::uint(30_000_000),
        rlp::uint(0),
        rlp::uint(1_800_000_000 + 12 * h),
        rlp::bytes(&[]),
        rlp::bytes(&[1; 32]),
        rlp::bytes(&[0; 8]),
        rlp::uint(1),
        rlp::bytes(&[0; 32]),
        rlp::uint(0),
        rlp::uint(0),
        rlp::bytes(&[0; 32]),
        rlp::bytes(&[0; 32]),
    ]);
    let row = InputRow {
        schema_version: 1,
        block_hash: hex_string(&keccak(&raw)),
        header_rlp: hex_string(&raw),
        receipts: Some(receipts.iter().map(|r| hex_string(r)).collect()),
        transactions: None,
    };
    row.verify()?;
    Ok(row)
}
#[test]
fn fix1_nonempty_k4_rebuild_and_durable_pins_run_without_workers_or_environment_variables(
) -> Result<()> {
    let dir = PrivateTempDir::new("portable-k4-pins")?;
    let mut c = History::new()?.c;
    let binary = b"public nonexecuted release fixture";
    let descriptor = Descriptor::pinned(digest(binary));
    c.m3.as_mut().unwrap().genesis_gate_rule_digest = descriptor.digest();
    let forks = ForkTable::genesis(&c)?;
    let artifact_root = dir.path().join("original-artifacts");
    install_devnet_artifacts(&c, &artifact_root)?;
    let mut header = ParentHeader {
        number: 0,
        hash: hex_string(&[1; 32]),
        parent_hash: None,
        timestamp: 0,
        gas_limit: 30_000_000,
        miner: hex_string(&[0; 20]),
        extra_data: "0x0100000032000000060000000000000000".into(),
        l1_info: None,
        m2_root_deposit: None,
    };
    let network = NetworkIdentity::new(&c, Some(&forks), header.hash.clone(), header.gas_limit)?;
    let mut source = InputJournal::open(
        &dir.path().join("node"),
        network.clone(),
        || row(0, [0; 32], &[]),
        false,
    )?;
    let mut pins = PinStore::open(&source)?;
    pins.install_bytes(
        ArtifactRole::StubDescriptor {
            epoch: 0,
            gate: descriptor.digest(),
        },
        digest(&serde_json::to_vec(&descriptor)?),
        &serde_json::to_vec(&descriptor)?,
        false,
    )?;
    pins.install_bytes(
        ArtifactRole::StubBinary {
            epoch: 0,
            gate: descriptor.digest(),
            profile: boole_gate_supervisor::process::host_profile(),
        },
        digest(binary),
        binary,
        true,
    )?;
    boole_devnet_driver::retention::environment(&mut pins, &c, &artifact_root)?;
    let mut state = State::new(&c, header.gas_limit)?;
    pins.prepare(
        &source,
        HeaderBinding {
            header: header.clone(),
            transactions: vec![],
            root: state.root,
        },
        &[],
    )?;
    pins.select(0, &header.hash, state.root)?;
    let mut observed = crate::retention::Observed::from([(
        0,
        (header.clone(), crate::retention::tx_digest(&[])?),
    )]);
    let package = stub::package(10000);
    let raw = stub::export(0, 1);
    let encoded = boole_gate_supervisor::raw_frame::encode(&raw)?;
    let salt = [7; 32];
    let cancel = boole_derivation::m2::cancel_hash(&[8; 32]);
    let mut body = vec![];
    let mut chunks = vec![];
    let mut leaves = vec![];
    let mut key = ([0; 32], [0; 32]);
    for h in 1..=11 {
        let items = match h {
            1 => vec![wire::input(
                8,
                wire::pool_at(
                    &c,
                    0,
                    wire::pool_leaf(c.chain_id, 2, &[], &package, &[9; 32]),
                    1,
                    3,
                    1,
                ),
                0,
            )],
            5 => vec![wire::input(
                9,
                wire::problem_body(0, 1, 0, &[], [9; 32], &[], &package),
                0,
            )],
            6 => {
                key.0 = state.rounds[&(0, 1)].problem.unwrap();
                (body, chunks, leaves) = wire::body(&c, key.0, [6; 20], salt, &encoded, &[]);
                body[153..161].copy_from_slice(&(raw.len() as u64).to_be_bytes());
                body[161..193].copy_from_slice(&digest(&raw));
                key.1 = boole_derivation::m2::commit_hash(
                    c.chain_id,
                    key.0,
                    cancel,
                    boole_derivation::m2::body_digest(&body),
                );
                vec![wire::input(2, wire::commit(key.0, key.1), 0)]
            }
            8 => vec![wire::input(3, wire::open(&body, cancel), 0)],
            9 => vec![wire::input(
                4,
                wire::part(
                    key.1,
                    0,
                    &boole_derivation::m2::merkle_path(&leaves, 0),
                    &chunks[0],
                ),
                0,
            )],
            _ => vec![],
        };
        let parent = hex_bytes(source.hash(h - 1).unwrap(), "parent")?
            .try_into()
            .unwrap();
        let input = row(h, parent, &items)?;
        source.append(&input)?;
        let v = input.verify()?;
        let records = if h == 11 {
            let mut history = JournalInputs(ReadOnlyJournal::open(source.root(), &network)?);
            let rebuilt = rebuild::rebuild(
                &state,
                &c,
                key,
                &mut history,
                Some(&Codec::portable_stub()),
                dir.path(),
            )?;
            assert_eq!(rebuilt.input.raw, raw, "actual K4 transport decoding");
            let GateReply::Result { value } = stub::evaluate(&descriptor, &rebuilt.input) else {
                panic!("public stub deferred")
            };
            vec![IndexedVerdict {
                queue_index: state.m3.as_ref().unwrap().queue[0].queue_index,
                record: supervisor::checked_record(
                    &rebuilt.input,
                    &descriptor,
                    &state.m3.as_ref().unwrap().queue[0],
                    0,
                    &value,
                )?,
            }]
        } else {
            vec![]
        };
        let (derived, next) = boole_derivation::attributes::derive_with_m3(
            &state, &c, &v.block, &v.logs, &header, &records, &forks,
        )?;
        header.parent_hash = Some(header.hash.clone());
        header.hash = hex_string(&digest(&h.to_be_bytes()));
        header.number = h;
        header.timestamp = u64::from_str_radix(&derived.attributes.timestamp[2..], 16)?;
        header.l1_info = derived.attributes.transactions.first().cloned();
        header.m2_root_deposit = derived.attributes.transactions.get(1).cloned();
        observed.insert(
            h,
            (
                header.clone(),
                crate::retention::tx_digest(&derived.attributes.transactions)?,
            ),
        );
        let required = boole_devnet_archive::pins::requirements(&next, &c, &forks, true)?;
        pins.prepare(
            &source,
            HeaderBinding {
                header: header.clone(),
                transactions: derived.attributes.transactions,
                root: next.root,
            },
            &required,
        )?;
        pins.select(h, &header.hash, next.root)?;
        state = next;
    }
    assert_eq!(state.candidates[&key].status(), Status::Accepted);
    let mut inputs = JournalInputs(ReadOnlyJournal::open(source.root(), &network)?);
    let rebuilt = crate::k4::check(
        &state,
        &c,
        Some(&forks),
        &mut inputs,
        Some(&Codec::portable_stub()),
        &BTreeMap::from([(key, 11)]),
        dir.path(),
    )
    .map_err(|e| {
        anyhow::anyhow!(
            "real K4 rebuild failed at{} checked{}",
            e.height,
            e.coverage.checked
        )
    })?;
    assert_eq!((rebuilt.checked, rebuilt.total), (1, 1));
    let mut receipt = serde_json::json!({"k4":{}});
    let view = inputs.pin_view();
    crate::retention::verify(
        &inputs,
        &view,
        &state,
        &c,
        Some(&forks),
        &observed,
        rebuilt,
        &mut receipt,
    )
    .unwrap();
    crate::retention::finish(&view, &mut receipt, 11).unwrap();
    assert_eq!(receipt["k4"]["status"], "ok");
    assert_eq!(receipt["k4"]["checked"], "1");
    assert_eq!(receipt["k4"]["verification_height"], "11");
    // Actual source loss after a genuine nonempty rebuild cannot be readiness.
    drop(view);
    drop(pins);
    let saved = dir.path().join("preserved-pins");
    std::fs::rename(source.root().join("pins-v1"), &saved)?;
    let mut missing = serde_json::json!({"k4":{}});
    let absent = inputs.pin_view();
    crate::retention::verify(
        &inputs,
        &absent,
        &state,
        &c,
        Some(&forks),
        &observed,
        rebuilt,
        &mut missing,
    )
    .unwrap();
    assert_eq!(missing["k4"]["rebuilt"], "1");
    assert_eq!(missing["k4"]["checked"], "0");
    assert_eq!(missing["k4"]["status"], "pending");
    std::fs::rename(saved, source.root().join("pins-v1"))?;
    let view = inputs.pin_view();
    let mut wrong = observed.clone();
    wrong.get_mut(&8).unwrap().0.hash = hex_string(&[0; 32]);
    let failure = crate::retention::verify(
        &inputs,
        &view,
        &state,
        &c,
        Some(&forks),
        &wrong,
        rebuilt,
        &mut serde_json::json!({"k4":{}}),
    )
    .unwrap_err();
    assert_eq!(failure.class, "K4");
    Ok(())
}
