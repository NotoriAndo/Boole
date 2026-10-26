#[path = "support/posting.rs"]
mod support;
use anyhow::Result;
use boole_derivation::{
    attributes::{hex_bytes, hex_string},
    rlp,
    user_tx::keccak,
};
use boole_devnet_archive::{
    durability::PrivateTempDir,
    input::{receipts_root, InputRow},
    journal::{InputJournal, NetworkIdentity},
};
use boole_devnet_mcp::{
    client::{fixed, Poster, RpcPoster},
    posting::PostingMetadata,
};
use serde_json::{json, Value};
use std::{cell::RefCell, rc::Rc};
use support::{signed, Server};

fn account(nonce: u64, address: &str) -> (Vec<u8>, Value) {
    let storage = keccak(&rlp::bytes(&[]));
    let code = keccak(&[]);
    let value = rlp::list(&[
        rlp::uint(nonce),
        rlp::uint(0),
        rlp::bytes(&storage),
        rlp::bytes(&code),
    ]);
    let key = keccak(&hex_bytes(address, "address").unwrap());
    let path = [vec![0x20], key.to_vec()].concat();
    let node = rlp::list(&[rlp::bytes(&path), rlp::bytes(&value)]);
    let proof = json!({"address":address,"nonce":format!("0x{nonce:x}"),"balance":"0x0","storageHash":hex_string(&storage),"codeHash":hex_string(&code),"accountProof":[hex_string(&node)],"storageProof":[]});
    (node, proof)
}
fn row(h: u64, parent: &[u8], state_root: &[u8], raw: &[u8], status: u64) -> InputRow {
    let txs = if h == 0 { vec![] } else { vec![raw.to_vec()] };
    let receipts = if h == 0 {
        vec![]
    } else {
        vec![rlp::list(&[
            rlp::uint(status),
            rlp::uint(21000),
            rlp::bytes(&[0; 256]),
            rlp::list(&[]),
        ])]
    };
    let fields = vec![
        rlp::bytes(parent),
        rlp::bytes(&[0; 32]),
        rlp::bytes(&[0; 20]),
        rlp::bytes(state_root),
        rlp::bytes(&receipts_root(&txs)),
        rlp::bytes(&receipts_root(&receipts)),
        rlp::bytes(&[0; 256]),
        rlp::uint(0),
        rlp::uint(h),
        rlp::uint(60_000_000),
        rlp::uint(if h == 0 { 0 } else { 21000 }),
        rlp::uint(1_790_000_000 + 12 * h),
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
    let header = rlp::list(&fields);
    InputRow {
        schema_version: 1,
        block_hash: hex_string(&keccak(&header)),
        header_rlp: hex_string(&header),
        receipts: Some(receipts.iter().map(|r| hex_string(r)).collect()),
        transactions: Some(txs.iter().map(|r| hex_string(r)).collect()),
    }
}
struct Fixture {
    _dir: PrivateTempDir,
    journal: Rc<RefCell<InputJournal>>,
    header: Value,
    proof: Value,
    metadata: PostingMetadata,
    address: String,
}
fn fixture(status: u64) -> Result<Fixture> {
    fixture_with_nonce(status, 8, true)
}
fn fixture_with_nonce(status: u64, nonce: u64, included: bool) -> Result<Fixture> {
    let dir = PrivateTempDir::new("m4c5-nonce")?;
    let (raw, address) = signed(2, b"posted body", 7);
    let (node, proof) = account(nonce, &address);
    let root = keccak(&node);
    let genesis = row(0, &[0; 32], &root, &[], 1);
    let network = NetworkIdentity::new(
        &boole_derivation::m2::Config::devnet(),
        None,
        hex_string(&[1; 32]),
        30_000_000,
    )?;
    let mut journal = InputJournal::open(
        &dir.path().join("journal"),
        network,
        || Ok(genesis.clone()),
        false,
    )?;
    let tip = if included {
        let row = row(
            1,
            &hex_bytes(&genesis.block_hash, "parent")?,
            &root,
            &raw,
            status,
        );
        journal.append(&row)?;
        row
    } else {
        genesis
    };
    let metadata = PostingMetadata {
        version: 1,
        sender: address.clone(),
        chain_id: 31337,
        nonce: 7,
        transaction_hash: hex_string(&keccak(&raw)),
        body_digest: hex_string(&boole_devnet_mcp::codec::digest(b"posted body")),
        kind: 2,
    };
    Ok(Fixture {
        _dir: dir,
        journal: Rc::new(RefCell::new(journal)),
        header: json!({"number":if included {"0x1"}else{"0x0"},"hash":tip.block_hash,"stateRoot":hex_string(&root)}),
        proof,
        metadata,
        address,
    })
}

#[test]
fn t79_advanced_nonce_is_only_inclusion_unresolved_for_original_success_or_revert() -> Result<()> {
    let mut observed = vec![];
    for status in [1, 0] {
        let f = fixture(status)?;
        let address = f.address.clone();
        let header = f.header.clone();
        let proof = f.proof.clone();
        let server = Server::new(move |q| {
            Ok(match q["method"].as_str().unwrap() {
                "eth_chainId" => json!("0x7a69"),
                "eth_accounts" => json!([address]),
                "eth_getBlockByNumber" => {
                    assert_eq!(q["params"], json!(["finalized", false]));
                    header.clone()
                }
                "eth_getProof" => {
                    assert_eq!(
                        q["params"],
                        json!([address,[],{"blockHash":header["hash"],"requireCanonical":true}])
                    );
                    proof.clone()
                }
                other => anyhow::bail!("unexpected nonce fallback {other}"),
            })
        });
        let mut poster = RpcPoster::new(&server.url, fixed(&f.address)?)?;
        poster.bind_journal(f.journal.clone());
        // Receipt authority remains independent, even when nonce proof is known.
        assert_eq!(
            f.journal
                .borrow()
                .transaction_receipt(&f.metadata.transaction_hash)?,
            Some((1, status == 1))
        );
        observed.push(serde_json::to_value(poster.nonce_observation(&f.metadata))?);
    }
    for (status, result) in [1, 0].into_iter().zip(observed) {
        assert_eq!(
            result["state"], "nonce-consumed",
            "original receipt status{status}: {result}"
        );
        assert_eq!(result["inclusion"], "unresolved");
        assert_eq!(result["account_nonce"], 8);
    }
    Ok(())
}

#[test]
fn t79_missing_account_proof_is_unavailable_without_fallback() -> Result<()> {
    let f = fixture(1)?;
    let address = f.address.clone();
    let header = f.header.clone();
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = calls.clone();
    let server = Server::new(move |q| {
        Ok(match q["method"].as_str().unwrap() {
            "eth_chainId" => json!("0x7a69"),
            "eth_accounts" => json!([address]),
            "eth_getBlockByNumber" => {
                assert_eq!(q["params"], json!(["finalized", false]));
                header.clone()
            }
            "eth_getProof" => {
                seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Value::Null
            }
            other => anyhow::bail!("missing proof attempted fallback {other}"),
        })
    });
    let mut poster = RpcPoster::new(&server.url, fixed(&f.address)?)?;
    poster.bind_journal(f.journal.clone());
    let observed = serde_json::to_value(poster.nonce_observation(&f.metadata))?;
    assert_eq!(observed["state"], "unavailable");
    assert_eq!(observed["reason"], "proof-unavailable");
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(observed["inclusion"], "unresolved");
    Ok(())
}

#[test]
fn unavailable_proof_is_reprobed_with_a_bounded_delay_per_own_tip() -> Result<()> {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let f = fixture(1)?;
    let address = f.address.clone();
    let header = f.header.clone();
    let proof = f.proof.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let server = Server::new(move |q| {
        Ok(match q["method"].as_str().unwrap() {
            "eth_chainId" => json!("0x7a69"),
            "eth_accounts" => json!([address]),
            "eth_getBlockByNumber" => header.clone(),
            "eth_getProof" => {
                if count.fetch_add(1, Ordering::SeqCst) == 0 {
                    Value::Null
                } else {
                    proof.clone()
                }
            }
            other => anyhow::bail!("unavailable proof fallback {other}"),
        })
    });
    let mut poster = RpcPoster::new(&server.url, fixed(&f.address)?)?;
    poster.bind_journal(f.journal.clone());
    assert_eq!(
        serde_json::to_value(poster.nonce_observation(&f.metadata))?["state"],
        "unavailable"
    );
    assert_eq!(
        serde_json::to_value(poster.nonce_observation(&f.metadata))?["state"],
        "unavailable",
        "an immediate repeated proof lookup bypassed availability backoff"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert_eq!(
        serde_json::to_value(poster.nonce_observation(&f.metadata))?["state"],
        "nonce-consumed",
        "missing proof became a sticky stop"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    Ok(())
}

#[test]
fn t79_another_sender_forked_out_proof_and_stale_tip_never_consume_the_own_nonce() -> Result<()> {
    for case in [
        "metadata-sender",
        "proof-sender",
        "forked-out-proof",
        "stale-tip",
        "changed-during-proof",
        "claimed-nonce",
        "claimed-balance",
        "claimed-code",
        "claimed-storage",
    ] {
        let f = if case == "forked-out-proof" {
            fixture_with_nonce(1, 7, false)?
        } else {
            fixture(1)?
        };
        let mut metadata = f.metadata.clone();
        let mut proof = f.proof.clone();
        let mut header = f.header.clone();
        if case == "metadata-sender" {
            metadata.sender = hex_string(&[92; 20]);
        }
        if case == "proof-sender" {
            proof["address"] = json!(hex_string(&[92; 20]));
        }
        if case == "forked-out-proof" {
            proof = account(8, &f.address).1;
            assert_eq!(
                f.journal
                    .borrow()
                    .transaction_receipt(&metadata.transaction_hash)?,
                None
            );
        }
        if case == "stale-tip" {
            header["number"] = json!("0x2");
            header["hash"] = json!(hex_string(&[92; 32]));
        }
        match case {
            "claimed-nonce" => proof["nonce"] = json!("0x9"),
            "claimed-balance" => proof["balance"] = json!("0x1"),
            "claimed-code" => proof["codeHash"] = json!(hex_string(&[92; 32])),
            "claimed-storage" => proof["storageHash"] = json!(hex_string(&[92; 32])),
            _ => {}
        }
        let address = f.address.clone();
        let mut reads = 0;
        let server = Server::new(move |q| {
            Ok(match q["method"].as_str().unwrap() {
                "eth_chainId" => json!("0x7a69"),
                "eth_accounts" => json!([address]),
                "eth_getBlockByNumber" => {
                    assert_eq!(q["params"], json!(["finalized", false]));
                    reads += 1;
                    if case == "changed-during-proof" && reads > 1 {
                        json!({"number":"0x2","hash":hex_string(&[92;32])})
                    } else {
                        header.clone()
                    }
                }
                "eth_getProof" => {
                    assert!(q["params"][2].is_object());
                    assert_eq!(q["params"][2]["requireCanonical"], true);
                    proof.clone()
                }
                other => anyhow::bail!("{case} attempted forbidden nonce fallback {other}"),
            })
        });
        let mut poster = RpcPoster::new(&server.url, fixed(&f.address)?)?;
        poster.bind_journal(f.journal.clone());
        let observed = serde_json::to_value(poster.nonce_observation(&metadata))?;
        assert_eq!(observed["state"], "unavailable", "{case}: {observed}");
        assert_eq!(observed["inclusion"], "unresolved");
    }
    Ok(())
}

#[test]
fn t79_nonce_equality_and_cached_tip_recheck_do_not_claim_a_drop() -> Result<()> {
    let f = fixture_with_nonce(1, 7, false)?;
    let address = f.address.clone();
    let header = f.header.clone();
    let proof = f.proof.clone();
    let changed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = changed.clone();
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = calls.clone();
    let server = Server::new(move |q| {
        Ok(match q["method"].as_str().unwrap() {
            "eth_chainId" => json!("0x7a69"),
            "eth_accounts" => json!([address]),
            "eth_getBlockByNumber" => {
                assert_eq!(q["params"], json!(["finalized", false]));
                if flag.load(std::sync::atomic::Ordering::SeqCst) {
                    json!({"number":"0x1","hash":hex_string(&[92;32])})
                } else {
                    header.clone()
                }
            }
            "eth_getProof" => {
                count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                proof.clone()
            }
            other => anyhow::bail!("cache attempted forbidden nonce fallback {other}"),
        })
    });
    let mut poster = RpcPoster::new(&server.url, fixed(&f.address)?)?;
    poster.bind_journal(f.journal.clone());
    for _ in 0..2 {
        let observed = serde_json::to_value(poster.nonce_observation(&f.metadata))?;
        assert_eq!(observed["state"], "not-advanced");
        assert_eq!(observed["inclusion"], "unresolved");
    }
    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "proof cache must be bound to the own header and journal row"
    );
    changed.store(true, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        serde_json::to_value(poster.nonce_observation(&f.metadata))?["state"],
        "unavailable",
        "a cached proof bypassed current finality"
    );
    Ok(())
}

#[test]
fn t79_pinned_anvil_branch_proof_matches_the_own_journal_state_root() -> Result<()> {
    let fixture: Value = serde_json::from_slice(include_bytes!(
        "../../../fixtures/devnet-mcp/nonce-account-proof-mined-v1.json"
    ))?;
    let header = fixture["header"].clone();
    let proof = fixture["proof"].clone();
    let address = proof["address"].as_str().unwrap().to_owned();
    let headers = fixture["bootstrap_headers"].as_array().unwrap();
    let first = InputRow::from_rpc(&headers[0], None)?;
    let dir = PrivateTempDir::new("m4c5-real-proof")?;
    let network = NetworkIdentity::new(
        &boole_derivation::m2::Config::devnet(),
        None,
        hex_string(&[1; 32]),
        30_000_000,
    )?;
    let mut journal = InputJournal::open(dir.path(), network, || Ok(first), false)?;
    for header in headers.iter().skip(1) {
        journal.append(&InputRow::from_rpc(header, None)?)?;
    }
    assert_eq!(journal.tip(), 8);
    let response_address = address.clone();
    let server = Server::new(move |q| {
        Ok(match q["method"].as_str().unwrap() {
            "eth_chainId" => json!("0x7a69"),
            "eth_accounts" => json!([response_address]),
            "eth_getBlockByNumber" => {
                assert_eq!(q["params"], json!(["finalized", false]));
                header.clone()
            }
            "eth_getProof" => proof.clone(),
            other => anyhow::bail!("native proof fallback {other}"),
        })
    });
    let mut poster = RpcPoster::new(&server.url, fixed(&address)?)?;
    poster.bind_journal(Rc::new(RefCell::new(journal)));
    let metadata = PostingMetadata {
        version: 1,
        sender: address,
        chain_id: 31337,
        nonce: 0,
        transaction_hash: hex_string(&[11; 32]),
        body_digest: hex_string(&[12; 32]),
        kind: 2,
    };
    let observed = serde_json::to_value(poster.nonce_observation(&metadata))?;
    assert_eq!(observed["state"], "not-advanced", "{observed}");
    assert_eq!(observed["account_nonce"], 0);
    Ok(())
}

#[test]
fn t79_tip_changed_has_a_bounded_backoff_then_rechecks_the_own_tip() -> Result<()> {
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    };
    let f = fixture(1)?;
    let address = f.address.clone();
    let header = f.header.clone();
    let proof = f.proof.clone();
    let good = Arc::new(AtomicBool::new(false));
    let flag = good.clone();
    let reads = Arc::new(AtomicUsize::new(0));
    let count = reads.clone();
    let server = Server::new(move |q| {
        Ok(match q["method"].as_str().unwrap() {
            "eth_chainId" => json!("0x7a69"),
            "eth_accounts" => json!([address]),
            "eth_getBlockByNumber" => {
                count.fetch_add(1, Ordering::SeqCst);
                if flag.load(Ordering::SeqCst) {
                    header.clone()
                } else {
                    json!({"number":"0x9","hash":hex_string(&[99;32])})
                }
            }
            "eth_getProof" => proof.clone(),
            other => anyhow::bail!("tip fallback {other}"),
        })
    });
    let mut poster = RpcPoster::new(&server.url, fixed(&f.address)?)?;
    poster.bind_journal(f.journal.clone());
    assert_eq!(
        serde_json::to_value(poster.nonce_observation(&f.metadata))?["reason"],
        "tip-changed"
    );
    good.store(true, Ordering::SeqCst);
    assert_eq!(
        serde_json::to_value(poster.nonce_observation(&f.metadata))?["reason"],
        "tip-changed",
        "TipChanged bypasses bounded per-tip backoff"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert_eq!(
        serde_json::to_value(poster.nonce_observation(&f.metadata))?["state"],
        "nonce-consumed",
        "TipChanged became sticky"
    );
    assert_eq!(reads.load(Ordering::SeqCst), 3);
    Ok(())
}
