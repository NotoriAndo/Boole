#[path = "support/posting.rs"]
mod support;
use anyhow::Result;
use boole_derivation::{attributes::hex_string, user_tx::keccak};
use boole_devnet_archive::durability::PrivateTempDir;
use boole_devnet_mcp::{
    client::{Poster, RpcPoster},
    store::Store,
};
use serde_json::json;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use support::{signed, Server};
#[test]
fn t81_rpc_post_persists_locally_hashed_signed_metadata_before_send() -> Result<()> {
    let dir = PrivateTempDir::new("m4c5-posting")?;
    let store = Store::open(dir.path().join("private"))?;
    let (raw, account) = signed(2, b"posting body", 7);
    let tx = hex_string(&keccak(&raw));
    let path = store.path.join("before-send.json");
    let send_path = path.clone();
    let expected_raw = hex_string(&raw);
    let account_reply = account.clone();
    let hash_reply = tx.clone();
    let ordered = Arc::new(AtomicBool::new(false));
    let seen = ordered.clone();
    let server = Server::new(move |request| {
        Ok(match request["method"].as_str().unwrap() {
            "eth_chainId" => json!("0x7a69"),
            "eth_accounts" => json!([account_reply]),
            "eth_getTransactionCount" => json!("0x7"),
            "eth_gasPrice" => json!("0x3b9aca00"),
            "eth_signTransaction" => json!({"raw":expected_raw}),
            "eth_sendRawTransaction" | "eth_sendTransaction" => {
                seen.store(send_path.exists(), Ordering::SeqCst);
                json!(hash_reply)
            }
            other => anyhow::bail!("unexpected method {other}"),
        })
    });
    let mut poster = RpcPoster::new(&server.url, boole_devnet_mcp::client::fixed(&account)?)?;
    let mut persisted = None;
    let returned = poster.post_durable(2, b"posting body", &mut |p| {
        store.write("before-send.json", &p.metadata)?;
        persisted = Some(p.metadata.clone());
        Ok(())
    })?;
    assert!(
        ordered.load(Ordering::SeqCst),
        "network send preceded durable assigned-nonce metadata"
    );
    let metadata = persisted.expect("assigned posting metadata");
    assert_eq!(metadata.sender, account);
    assert_eq!(metadata.chain_id, 31337);
    assert_eq!(metadata.nonce, 7);
    assert_eq!(metadata.transaction_hash, tx);
    assert_eq!(returned, tx);
    assert_eq!(metadata.kind, 2);
    assert_eq!(
        metadata.body_digest,
        hex_string(&boole_devnet_mcp::codec::digest(b"posting body"))
    );
    Ok(())
}

#[test]
fn t81_bad_signer_bindings_and_persistence_failure_never_send_rpc_bytes() -> Result<()> {
    use boole_devnet_mcp::posting::PostingNotSent;
    use std::sync::atomic::AtomicUsize;
    for case in [
        "sender",
        "chain",
        "nonce",
        "body",
        "persist",
        "priority-fee",
        "maximum-fee",
    ] {
        let (_, account) = signed(2, b"posting body", 7);
        let (raw, _) = support::signed_with_fees(
            2,
            if case == "body" {
                b"another body"
            } else {
                b"posting body"
            },
            if case == "nonce" { 8 } else { 7 },
            if case == "chain" { 31338 } else { 31337 },
            if case == "sender" { 72 } else { 71 },
            (
                u64::from(case == "priority-fee"),
                if case == "maximum-fee" {
                    2_000_000_000
                } else {
                    1_000_000_000
                },
            ),
        );
        let account_reply = account.clone();
        let raw = hex_string(&raw);
        let sends = Arc::new(AtomicUsize::new(0));
        let count = sends.clone();
        let server = Server::new(move |q| {
            Ok(match q["method"].as_str().unwrap() {
                "eth_chainId" => json!("0x7a69"),
                "eth_accounts" => json!([account_reply]),
                "eth_getTransactionCount" => json!("0x7"),
                "eth_gasPrice" => json!("0x3b9aca00"),
                "eth_signTransaction" => json!({"raw":raw}),
                "eth_sendRawTransaction" => {
                    count.fetch_add(1, Ordering::SeqCst);
                    json!("0x00")
                }
                other => anyhow::bail!("unexpected signing control RPC {other}"),
            })
        });
        let mut poster = RpcPoster::new(&server.url, boole_devnet_mcp::client::fixed(&account)?)?;
        let mut callbacks = 0;
        let error = poster
            .post_durable(2, b"posting body", &mut |_| {
                callbacks += 1;
                anyhow::bail!("private-persistence-error-canary")
            })
            .unwrap_err();
        assert!(error.is::<PostingNotSent>(), "{case}: {error:#}");
        assert_eq!(
            sends.load(Ordering::SeqCst),
            0,
            "{case} broadcast before validation/fsync"
        );
        assert_eq!(callbacks, usize::from(case == "persist"));
        assert!(!error.to_string().contains("canary"));
    }
    Ok(())
}

#[test]
fn t81_rpc_selected_hash_never_replaces_the_locally_computed_hash() -> Result<()> {
    use boole_devnet_mcp::posting::PostingAmbiguous;
    let dir = PrivateTempDir::new("m4c5-ack")?;
    let store = Store::open(dir.path().join("private"))?;
    let (raw, account) = signed(2, b"posting body", 7);
    let hash = hex_string(&keccak(&raw));
    let raw = hex_string(&raw);
    let address = account.clone();
    let server = Server::new(move |q| {
        Ok(match q["method"].as_str().unwrap() {
            "eth_chainId" => json!("0x7a69"),
            "eth_accounts" => json!([address]),
            "eth_getTransactionCount" => json!("0x7"),
            "eth_gasPrice" => json!("0x3b9aca00"),
            "eth_signTransaction" => json!({"raw":raw}),
            "eth_sendRawTransaction" => json!(hex_string(&[93; 32])),
            other => anyhow::bail!("unexpected ack control {other}"),
        })
    });
    let mut poster = RpcPoster::new(&server.url, boole_devnet_mcp::client::fixed(&account)?)?;
    let mut assigned = None;
    let error = poster
        .post_durable(2, b"posting body", &mut |p| {
            p.persist(&store)?;
            assigned = Some(p.metadata.clone());
            Ok(())
        })
        .unwrap_err();
    assert!(error.is::<PostingAmbiguous>());
    let metadata = assigned.unwrap();
    assert_eq!(metadata.transaction_hash, hash);
    let record: serde_json::Value =
        store.read(&format!("posting-{}.json", hash.trim_start_matches("0x")))?;
    assert_eq!(record["metadata"]["transaction_hash"], hash);
    Ok(())
}

#[test]
fn pending_nonce_policy_preserves_two_own_items_with_the_same_assigned_nonce() -> Result<()> {
    use boole_derivation::inbox::item;
    use boole_devnet_mcp::posting::PostingAmbiguous;
    let dir = PrivateTempDir::new("m4c5-nonce-policy")?;
    let store = Store::open(dir.path().join("private"))?;
    let (_, account) = signed(2, b"A", 7);
    let address = account.clone();
    let mut sends = 0;
    let server = Server::new(move |q| {
        Ok(match q["method"].as_str().unwrap() {
            "eth_chainId" => json!("0x7a69"),
            "eth_accounts" => json!([address]),
            "eth_getTransactionCount" => {
                assert_eq!(q["params"], json!([address, "pending"]));
                json!("0x7")
            }
            "eth_gasPrice" => json!("0x3b9aca00"),
            "eth_signTransaction" => {
                assert_eq!(q["params"][0]["nonce"], "0x7");
                let data = boole_derivation::attributes::hex_bytes(
                    q["params"][0]["data"].as_str().unwrap(),
                    "data",
                )?;
                let (kind, body) = item(&data[4..]).map_err(anyhow::Error::msg)?;
                json!({"raw":hex_string(&signed(kind,body,7).0)})
            }
            "eth_sendRawTransaction" => {
                sends += 1;
                if sends == 1 {
                    json!("test-close-without-response")
                } else {
                    let raw = boole_derivation::attributes::hex_bytes(
                        q["params"][0].as_str().unwrap(),
                        "raw",
                    )?;
                    json!(hex_string(&keccak(&raw)))
                }
            }
            other => anyhow::bail!("unexpected nonce allocation RPC {other}"),
        })
    });
    let mut poster = RpcPoster::new(&server.url, boole_devnet_mcp::client::fixed(&account)?)?;
    let mut metadata = vec![];
    let a = poster.post_durable(2, b"A", &mut |p| {
        p.persist(&store)?;
        metadata.push(p.metadata.clone());
        Ok(())
    });
    assert!(a.unwrap_err().is::<PostingAmbiguous>());
    let b = poster.post_durable(2, b"B", &mut |p| {
        p.persist(&store)?;
        metadata.push(p.metadata.clone());
        Ok(())
    })?;
    assert_eq!(metadata.len(), 2);
    assert_eq!(metadata[0].nonce, 7);
    assert_eq!(metadata[1].nonce, 7);
    assert_ne!(metadata[0].transaction_hash, b);
    assert_eq!(metadata[1].transaction_hash, b);
    Ok(())
}
