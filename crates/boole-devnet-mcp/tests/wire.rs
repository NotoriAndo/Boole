use boole_derivation::{attributes::hex_bytes, m2::*};
use boole_devnet_mcp::wire::{Material, Prepared};
use sha2::{Digest, Sha256};

#[test]
fn client_preimage_parts_and_cancel_reproduce_the_recorded_candidate() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m2/commit-reveal-v1.1.json"
    ))
    .unwrap();
    let c: Config = serde_json::from_value(fixture["config"].clone()).unwrap();
    let mut items = vec![];
    for row in fixture["heights"].as_array().unwrap() {
        for log in row["logs"].as_array().unwrap() {
            let bytes = hex_bytes(log["data"].as_str().unwrap(), "fixture").unwrap();
            if let Ok((kind, body)) = boole_derivation::inbox::item(&bytes) {
                items.push((kind, body.to_vec()));
            }
        }
    }
    let open = &items
        .iter()
        .find(|(k, b)| {
            *k == 3 && b.len() >= 330 && u16::from_be_bytes(b[250..252].try_into().unwrap()) == 14
        })
        .unwrap()
        .1;
    let b = &open[53..];
    let salt: Hash = b[53..85].try_into().unwrap();
    let secret: Hash = Sha256::digest([b"test cancel".as_slice(), &salt].concat()).into();
    let p: Hash = b[1..33].try_into().unwrap();
    let ch = commit_hash(c.chain_id, p, cancel_hash(&secret), body_digest(b));
    let n = u16::from_be_bytes(b[197..199].try_into().unwrap());
    assert_eq!(n, 14);
    let mut parts = std::collections::BTreeMap::new();
    for (kind, body) in &items {
        if *kind == 4 && body.get(21..53) == Some(ch.as_slice()) {
            let i = u16::from_be_bytes(body[53..55].try_into().unwrap());
            parts.insert(i, body[55 + 4 * 32..].to_vec());
        }
    }
    assert_eq!(parts.len(), 14);
    let compressed = parts.values().flatten().copied().collect();
    let prepared = Prepared::new(
        &c,
        Material {
            problem: p,
            payee: b[33..53].try_into().unwrap(),
            salt,
            cancel_secret: secret,
            raw_len: u64::from_be_bytes(b[153..161].try_into().unwrap()),
            export_digest: b[161..193].try_into().unwrap(),
            compressed,
            packages: vec![],
            format: 2,
        },
    )
    .unwrap();
    assert_eq!(prepared.body, b);
    assert_eq!(prepared.commit, ch);
    assert_eq!(prepared.open(open[1..21].try_into().unwrap()), *open);
    for (i, data) in parts {
        assert_eq!(prepared.chunks[i as usize], data);
        let part = prepared.part([5; 20], i).unwrap();
        assert_eq!(part.len(), 55 + 4 * 32 + data.len());
        assert!(items.iter().any(|(k, body)| *k == 4 && *body == part));
    }
    let commit = items
        .iter()
        .find(|(k, body)| *k == 2 && body[53..85] == ch)
        .unwrap();
    assert_eq!(
        prepared.commit_body(commit.1[1..21].try_into().unwrap()),
        commit.1
    );
    let cancel = items
        .iter()
        .find(|(k, body)| *k == 7 && body[33..65] == ch)
        .unwrap();
    assert_eq!(prepared.cancel(), cancel.1);
    assert!(prepared.validate(&c).is_ok());
    let mut tampered = prepared;
    tampered.chunks[0][0] ^= 1;
    assert!(tampered.validate(&c).is_err());
}

#[test]
fn frame_contract_rejects_streaming_trailing_dictionary_and_bad_lengths() {
    let v: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m2/package-codec-v1.1.json"
    ))
    .unwrap();
    let frame = hex_bytes(
        &format!("0x{}", v["carried_hex"].as_str().unwrap()),
        "frame",
    )
    .unwrap();
    assert!(boole_devnet_mcp::codec::validate_frame(&frame, 1_048_576).is_ok());
    for bytes in [
        frame[..frame.len() - 1].to_vec(),
        [frame.as_slice(), &[0]].concat(),
        [frame.as_slice(), frame.as_slice()].concat(),
    ] {
        assert!(boole_devnet_mcp::codec::validate_frame(&bytes, 1_048_576).is_err());
    }
    assert!(boole_devnet_mcp::codec::validate_frame(&frame, 1_048_575).is_err());
    let mut streaming = frame.clone();
    streaming[4] &= 0x3f;
    assert!(boole_devnet_mcp::codec::validate_frame(&streaming, 1_048_576).is_err());
    let mut large_window = frame.clone();
    large_window[5] = 0x58;
    assert!(boole_devnet_mcp::codec::validate_frame(&large_window, 1_048_576).is_err());
    for mask in [1, 4, 8, 16] {
        let mut bad = frame.clone();
        bad[4] ^= mask;
        assert!(boole_devnet_mcp::codec::validate_frame(&bad, 1_048_576).is_err());
    }
}

#[test]
fn nonempty_packages_and_lane_one_use_variable_body_offsets() {
    let c = Config::devnet();
    let packages = vec![c.pkg_table[0]];
    let prepared = Prepared::new(
        &c,
        Material {
            problem: [7; 32],
            payee: [6; 20],
            salt: [8; 32],
            cancel_secret: [9; 32],
            raw_len: (PART_SIZE + 3) as u64,
            export_digest: [10; 32],
            compressed: vec![11; PART_SIZE + 3],
            packages: packages.clone(),
            format: 1,
        },
    )
    .unwrap();
    assert_eq!(prepared.body[150], 1);
    assert_eq!(&prepared.body[151..183], packages[0].as_slice());
    assert_eq!(prepared.body[183..185], [1, 1]);
    assert_eq!(prepared.body.len(), 309);
    assert_eq!(prepared.chunks.len(), 2);
    assert!(prepared.validate(&c).is_ok());
    let mut bad = prepared;
    bad.body[151] ^= 1;
    assert!(bad.validate(&c).is_err());
}
