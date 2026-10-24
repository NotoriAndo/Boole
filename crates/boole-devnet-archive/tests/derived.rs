use anyhow::Result;
use boole_derivation::m2::*;
use boole_devnet_archive::{archive::sha, derived::verify_export};
fn frozen(export: Hash, solver: [u8; 20], salt: Hash, transport: &[u8]) -> OpenRecord {
    OpenRecord {
        open_pos: (2, 0, 0),
        done_pos: Some((3, 0, 0)),
        export_digest: export,
        queue_index: Some(0),
        poster: solver,
        h_o: 2,
        solver,
        salt,
        n: 1,
        z_len: transport.len() as u32,
        raw_len: 3,
        data_root: merkle_root(&[part_leaf(&solver, &salt, 0, transport)]),
        body_digest: [0; 32],
        env_id: [0; 32],
        mode: 1,
        bitmap: [1, 0],
        h_r: Some(3),
    }
}
fn state() -> (State, Key, Key) {
    let c = Config::devnet();
    let mut s = State::new(&c, 30_000_000).unwrap();
    let k1 = ([1; 32], [2; 32]);
    let k2 = ([1; 32], [3; 32]);
    for (k, bytes) in [(k1, b"one"), (k2, b"two")] {
        let digest = boole_derivation::attributes::hex_bytes(&sha(bytes), "fixture")
            .unwrap()
            .try_into()
            .unwrap();
        let o = frozen(digest, [4; 20], [5; 32], b"zip");
        s.candidates.insert(
            k,
            Candidate::Live(Box::new(LiveCandidate {
                h_c: 1,
                transaction_index: 0,
                log_index: 0,
                t: Some([9; 32]),
                poster: [4; 20],
                status: Status::Checking,
                open: Some(o),
            })),
        );
    }
    (s, k1, k2)
}
#[test]
fn t73_export_cannot_use_a_valid_digest_from_another_commit() -> Result<()> {
    let (s, k1, k2) = state();
    verify_export(&s, k2, &sha(b"two"), b"two")?;
    assert!(
        verify_export(&s, k1, &sha(b"two"), b"two").is_err(),
        "another commit's export was accepted"
    );
    assert!(
        verify_export(&s, k1, &sha(b"one"), b"onf").is_err(),
        "mutated export accepted"
    );
    Ok(())
}
#[test]
fn t73_transport_and_public_class_bindings() -> Result<()> {
    use boole_devnet_archive::derived::{verify_content, verify_transport};
    let (s, k, _) = state();
    let address = boole_derivation::attributes::hex_string(&k.1);
    verify_transport(&s, k, &address, b"zip")?;
    assert!(verify_transport(&s, k, &address, b"zit").is_err());
    assert!(verify_transport(&s, k, &sha(b"zip"), b"zip").is_err());
    for class in ["env", "gate", "pkg", "vstmt"] {
        let digest = boole_derivation::attributes::hex_bytes(&sha(b"public bytes"), "fixture")?
            .try_into()
            .unwrap();
        verify_content(class, &sha(b"public bytes"), b"public bytes", digest)?;
        assert!(verify_content(class, &sha(b"public bytes"), b"Public bytes", digest).is_err());
    }
    assert!(verify_content("client", &sha(b"private"), b"private", [0; 32]).is_err());
    Ok(())
}
struct MutatedResponse(Vec<u8>);
impl boole_devnet_archive::client::Fetch for MutatedResponse {
    fn get(&mut self, _: &str, _: &str, _: usize) -> Result<Vec<u8>> {
        Ok(self.0.clone())
    }
    fn backoff(&mut self, millis: u64) {
        assert!(matches!(millis, 100 | 300));
    }
}
#[test]
fn t89_fifty_thousand_mutated_objects_and_responses_never_adopt_or_reject() -> Result<()> {
    use boole_devnet_archive::{
        archive::verify_header,
        client::ArchiveClient,
        derived::{verify_content, verify_transport},
    };
    let (s, k, _) = state();
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m4/input-auth-v0.json"
    ))?;
    let row: boole_devnet_archive::input::InputRow =
        serde_json::from_value(fixture["rows"][1].clone())?;
    let header = boole_derivation::attributes::hex_bytes(&row.header_rlp, "fixture")?;
    let pin: Hash = boole_derivation::attributes::hex_bytes(&sha(b"public bytes"), "pin")?
        .try_into()
        .unwrap();
    let index = boole_devnet_archive::archive::IndexRow {
        l1_number: 1,
        l2_number: 1,
        l1_hash: row.block_hash.clone(),
        parent_hash: row.block()?.parent_hash,
        bloom_positive: true,
        blob_ids: vec![],
        l2_hash: format!("0x{}", "03".repeat(32)),
        root: format!("0x{}", "04".repeat(32)),
        inputs: "archive-verified".into(),
        verdicts: "computed".into(),
        system_digest: sha(b"[]"),
    };
    index.validate()?;
    let mut counts = [0u64; 8];
    let mut discarded = 0;
    for i in 0u64..50_000 {
        let class = (i % 8) as usize;
        counts[class] += 1;
        let body = match class {
            0 => {
                let mut b = header.clone();
                let end = b.len();
                let old = b.clone();
                b[end - 8..].copy_from_slice(&i.to_be_bytes());
                if b == old {
                    b[end - 8] ^= 128;
                }
                b
            }
            1 => {
                let mut rs = row.receipts.clone().unwrap();
                let mut receipt = boole_derivation::attributes::hex_bytes(&rs[0], "receipt")?;
                let end = receipt.len();
                let old = receipt.clone();
                receipt[end - 8..].copy_from_slice(&i.to_be_bytes());
                if receipt == old {
                    receipt[end - 8] ^= 128;
                }
                rs[0] = boole_derivation::attributes::hex_string(&receipt);
                serde_json::to_vec(&rs)?
            }
            2 | 3 => i.to_be_bytes()[5..].to_vec(),
            4..=6 => {
                let mut b = b"public bytes".to_vec();
                let n = b.len();
                b[n - 8..].copy_from_slice(&i.to_be_bytes());
                b
            }
            _ => {
                let mut bad = index.clone();
                bad.root = boole_derivation::attributes::hex_string(&[i.to_be_bytes(); 4].concat());
                serde_json::to_vec(&bad)?
            }
        };
        let mut client = ArchiveClient::new(
            vec!["http://127.0.0.1:1".into()],
            Box::new(MutatedResponse(body)),
        )?;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.fetch_verified("mutation", 135_266_304, 1, |bytes| -> Result<()> {
                match class {
                    0 => verify_header(&row.block_hash, bytes),
                    1 => {
                        let mut r = row.clone();
                        r.receipts = Some(serde_json::from_slice(bytes)?);
                        r.verify()?;
                        Ok(())
                    }
                    2 => verify_export(&s, k, &sha(b"one"), bytes),
                    3 => verify_transport(
                        &s,
                        k,
                        &boole_derivation::attributes::hex_string(&k.1),
                        bytes,
                    ),
                    4 => verify_content(
                        "env",
                        &boole_derivation::attributes::hex_string(&pin),
                        bytes,
                        pin,
                    ),
                    5 => verify_content(
                        "gate",
                        &boole_derivation::attributes::hex_string(&pin),
                        bytes,
                        pin,
                    ),
                    6 => verify_content(
                        "vstmt",
                        &boole_derivation::attributes::hex_string(&pin),
                        bytes,
                        pin,
                    ),
                    _ => {
                        let candidate: boole_devnet_archive::archive::IndexRow =
                            serde_json::from_slice(bytes)?;
                        boole_devnet_archive::derived::verify_index(
                            &candidate,
                            1,
                            &index.l1_hash,
                            &index.l2_hash,
                            &index.root,
                        )
                    }
                }
            })
        }));
        let error = result
            .expect("mutated archive response panicked")
            .expect_err("mutated object adopted");
        assert!(
            matches!(
                error.downcast_ref::<boole_devnet_archive::error::Error>(),
                Some(boole_devnet_archive::error::Error::Availability { .. })
            ),
            "archive data escaped as a verdict/integrity rejection"
        );
        assert_eq!(client.failures[0], 3);
        discarded += client.discarded;
    }
    println!("T89 mutations=50000 classes={counts:?} discarded={discarded} panics=0 wrong_adoptions=0 reject_outcomes=0");
    // The corpus must admit real controls: rejecting every byte is not a pass.
    verify_export(&s, k, &sha(b"one"), b"one")?;
    verify_transport(
        &s,
        k,
        &boole_derivation::attributes::hex_string(&k.1),
        b"zip",
    )?;
    verify_content(
        "env",
        &boole_derivation::attributes::hex_string(&pin),
        b"public bytes",
        pin,
    )?;
    Ok(())
}
#[test]
fn t73_store_reverifies_transport_commit_preimage_and_variable_metadata() -> Result<()> {
    use boole_derivation::attributes::hex_string;
    use boole_devnet_archive::{
        archive::{ArchiveIdentity, ArchiveProfile, ArchiveStore},
        durability::PrivateTempDir,
        journal::NetworkIdentity,
    };
    let c = Config::devnet();
    let (mut s, old, _) = state();
    let mut candidate = s.candidates.remove(&old).unwrap();
    let mut b = vec![1];
    b.extend(old.0);
    b.extend([4; 20]);
    b.extend([5; 32]);
    b.push(1);
    b.extend(c.schema_id);
    b.extend(c.base_root);
    b.push(0);
    b.extend([1, 2]);
    b.extend(3u64.to_be_bytes());
    let export_pin: Hash = boole_derivation::attributes::hex_bytes(&sha(b"one"), "pin")?
        .try_into()
        .unwrap();
    b.extend(export_pin);
    b.extend(3u32.to_be_bytes());
    b.extend(1u16.to_be_bytes());
    b.extend(merkle_root(&[part_leaf(&[4; 20], &[5; 32], 0, b"zip")]));
    b.extend([0; 4]);
    b.extend(43u16.to_be_bytes());
    b.extend([1, 3]);
    b.extend(b"cpu");
    b.extend([1, 0]);
    b.extend([0; 36]);
    let cancel = [9; 32];
    let key = (
        old.0,
        commit_hash(c.chain_id, old.0, cancel, body_digest(&b)),
    );
    if let Candidate::Live(ref mut live) = candidate {
        live.open.as_mut().unwrap().body_digest = body_digest(&b);
    }
    s.candidates.insert(key, candidate);
    let d = PrivateTempDir::new("m4-public-objects")?;
    let x = ArchiveStore::open(
        d.path(),
        ArchiveProfile {
            identity: ArchiveIdentity {
                network: NetworkIdentity::new(&c, None, hex_string(&[1; 32]), 30_000_000)?,
                l1_anchor_number: 0,
                l1_anchor_hash: hex_string(&[2; 32]),
            },
            bind: "127.0.0.1:0".parse()?,
            prune_before: None,
        },
        false,
    )?;
    x.put_transport(&s, key, cancel, &b, b"zip")?;
    assert_eq!(
        x.object("tr", &hex_string(&key.1))?.as_deref(),
        Some(b"zip".as_slice())
    );
    assert!(x.put_transport(&s, key, [8; 32], &b, b"zip").is_err());
    assert!(x.put_transport(&s, key, cancel, &b, b"zit").is_err());
    x.put_export(&s, key, b"one")?;
    assert_eq!(
        x.object("exp", &sha(b"one"))?.as_deref(),
        Some(b"one".as_slice())
    );
    for class in [
        "client",
        "secret",
        "gate-cache",
        "journal",
        "header",
        "system",
    ] {
        assert!(x.object(class, &sha(b"one"))?.is_none());
        assert!(x.put_content(class, export_pin, b"one").is_err());
    }
    let oversized = vec![0; 1_048_577];
    let matching_pin = boole_derivation::attributes::hex_bytes(&sha(&oversized), "pin")?
        .try_into()
        .unwrap();
    let cap_error = x
        .put_content("vstmt", matching_pin, &oversized)
        .unwrap_err();
    assert_eq!(
        cap_error.to_string(),
        "object class cap",
        "vstmt cap failed for a different reason: {cap_error:#}"
    );
    let file = d.path().join("tr").join(&hex_string(&key.1)[2..]);
    std::fs::write(&file, b"corrupt transport envelope")?;
    let server = boole_devnet_archive::http::ArchiveServer::start(
        std::sync::Arc::new(std::sync::Mutex::new(x)),
        "127.0.0.1:0".parse()?,
    )?;
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(server.addr())?;
    write!(
        stream,
        "GET /boole/archive/v1/obj/tr/{} HTTP/1.1\r\n\r\n",
        &hex_string(&key.1)[2..]
    )?;
    let mut response = vec![];
    stream.read_to_end(&mut response)?;
    assert!(response.starts_with(b"HTTP/1.1 500"));
    assert!(String::from_utf8_lossy(&response).contains("object-quarantine"));
    assert!(!file.exists());
    assert_eq!(std::fs::read_dir(d.path().join("tr/corrupt"))?.count(), 1);
    Ok(())
}

#[test]
fn pkg_hash_alone_cannot_authorize_an_archive_package() -> Result<()> {
    let pin = boole_derivation::attributes::hex_bytes(&sha(b"package"), "pin")?
        .try_into()
        .unwrap();
    let mut client = boole_devnet_archive::client::ArchiveClient::new(
        vec!["http://127.0.0.1:1".into()],
        Box::new(MutatedResponse(b"package".to_vec())),
    )?;
    assert!(
        client.content("pkg", pin, 0).is_err(),
        "hash-only package accepted without frozen POOL leaf/path"
    );
    Ok(())
}
#[test]
fn pkg_helper_binds_exact_leaf_index_path_and_frozen_pool() -> Result<()> {
    use boole_devnet_archive::derived::{verify_package, PackageProof};
    let c = Config::devnet();
    let mut state = State::new(&c, 30_000_000)?;
    let bytes = b"package";
    let pin = boole_derivation::attributes::hex_bytes(&sha(bytes), "pin")?
        .try_into()
        .unwrap();
    let id = [22; 32];
    let sibling = [23; 32];
    let proof = PackageProof {
        kind: 2,
        codec: 0,
        raw_len: bytes.len() as u32,
        salt: [9; 32],
        path: vec![sibling],
    };
    let leaf = pool_leaf(c.chain_id, 2, &[], 0, proof.raw_len, bytes, &proof.salt);
    state.pools.insert(
        1,
        Pool {
            slot: 0,
            root: merkle_root(&[sibling, leaf]),
            start: 1,
            count: 2,
            unused: Default::default(),
        },
    );
    state.problems.insert(
        id,
        Problem {
            reveal_pos: (1, 0, 0),
            winner: None,
            voided: false,
            slot: 0,
            id,
            required_pkgs: vec![],
            h_v: 1,
            closed: None,
            pool_seq: 1,
            leaf_index: 1,
            instance_seed: [0; 32],
            epoch: 0,
        },
    );
    verify_package(&state, &c, id, pin, &proof, bytes)?;
    let mut client = boole_devnet_archive::client::ArchiveClient::new(
        vec!["http://127.0.0.1:1".into()],
        Box::new(MutatedResponse(bytes.to_vec())),
    )?;
    assert_eq!(client.package(&state, &c, id, pin, &proof)?, bytes);
    let mut bad = proof.clone();
    bad.path.clear();
    assert!(verify_package(&state, &c, id, pin, &bad, bytes).is_err());
    let mut bad = proof.clone();
    bad.salt = [8; 32];
    assert!(verify_package(&state, &c, id, pin, &bad, bytes).is_err());
    state.problems.get_mut(&id).unwrap().leaf_index = 0;
    assert!(verify_package(&state, &c, id, pin, &proof, bytes).is_err());
    state.problems.get_mut(&id).unwrap().leaf_index = 1;
    state.pools.get_mut(&1).unwrap().root = [7; 32];
    assert!(verify_package(&state, &c, id, pin, &proof, bytes).is_err());
    Ok(())
}
