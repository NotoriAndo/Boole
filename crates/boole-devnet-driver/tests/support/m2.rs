//! Test-only encoders and a calldata batch caller; none enters derivation.
use anyhow::{bail, Result};
use boole_derivation::{
    attributes::{hex_bytes, hex_string},
    m2::*,
    rlp,
    user_tx::keccak,
};
use boole_devnet_driver::engine::Rpc;
use k256::ecdsa::SigningKey;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};

pub fn hash(b: &[u8]) -> Hash {
    Sha256::digest(b).into()
}
pub fn pool_key() -> SigningKey {
    SigningKey::from_bytes(&hash(b"boole-devnet-m2-public-test-pool-v1").into()).expect("test key")
}
pub fn user_key() -> SigningKey {
    SigningKey::from_bytes(&hash(b"boole-devnet-m2-public-test-user-v1").into()).expect("test key")
}
pub fn address(key: &SigningKey) -> [u8; 20] {
    let p = key.verifying_key().to_encoded_point(false);
    keccak(&p.as_bytes()[1..])[12..]
        .try_into()
        .expect("address")
}
pub fn config() -> Config {
    let mut c = Config::devnet();
    c.pool_key = address(&pool_key());
    c
}
pub fn genesis() -> Result<Value> {
    let mut g = super::fixture_json("l2-genesis.json")?;
    g["alloc"][hex_string(&address(&user_key()))] = json!({"balance":"0x56bc75e2d63100000"});
    Ok(g)
}
pub fn pool(c: &Config, root: Hash, count: u32, start: u64, seq: u32) -> Vec<u8> {
    pool_at(c, 0, root, count, start, seq)
}
pub fn pool_leaf(chain: u64, kind: u8, req: &[Hash], package: &[u8], salt: &Hash) -> Hash {
    boole_derivation::m2::pool_leaf(chain, kind, req, 0, package.len() as u32, package, salt)
}
pub fn pool_at(c: &Config, slot: u8, root: Hash, count: u32, start: u64, seq: u32) -> Vec<u8> {
    let mut b = vec![2, slot];
    b.extend(seq.to_be_bytes());
    b.extend(root);
    b.extend(count.to_be_bytes());
    b.extend(start.to_be_bytes());
    b.extend([0; 32]);
    let pre = hash(
        &[
            b"BOOLE-M2/POOL/v2".as_slice(),
            &c.chain_id.to_be_bytes(),
            &b,
        ]
        .concat(),
    );
    let (sig, id) = pool_key()
        .sign_prehash_recoverable(&pre)
        .expect("public test pool key");
    b.extend(sig.to_bytes());
    b.push(id.to_byte());
    b
}

pub fn problem(
    slot: u8,
    round: u32,
    index: u32,
    salt: Hash,
    path: &[Hash],
    package: &[u8],
) -> Vec<u8> {
    let mut b = vec![2, slot];
    b.extend(round.to_be_bytes());
    b.extend(index.to_be_bytes());
    b.extend([2, 0]);
    b.extend(salt);
    b.push(0);
    b.extend((package.len() as u32).to_be_bytes());
    b.push(path.len() as u8);
    for p in path {
        b.extend(p);
    }
    b.extend((package.len() as u32).to_be_bytes());
    b.extend(package);
    b
}
/// Test wire encoder: req_k is zero, so codec/raw length are at offsets 44/45.
pub fn problem_encoded(
    slot: u8,
    round: u32,
    index: u32,
    salt: Hash,
    path: &[Hash],
    package: &[u8],
    encoding: (u8, u32),
) -> Vec<u8> {
    let mut b = problem(slot, round, index, salt, path, package);
    b[44] = encoding.0;
    b[45..49].copy_from_slice(&encoding.1.to_be_bytes());
    b
}
pub struct Proof {
    pub body: Vec<u8>,
    pub chunks: Vec<Vec<u8>>,
    pub leaves: Vec<Hash>,
    pub commit: Hash,
    pub problem: Hash,
    pub secret: Hash,
}
impl Proof {
    pub fn new(c: &Config, p: Hash, data: &[u8], salt: Hash, solver: [u8; 20]) -> Self {
        let chunks: Vec<Vec<u8>> = data.chunks(PART_SIZE).map(|v| v.to_vec()).collect();
        let leaves: Vec<_> = chunks
            .iter()
            .enumerate()
            .map(|(i, d)| part_leaf(&solver, &salt, i as u16, d))
            .collect();
        let mut b = vec![1];
        b.extend(p);
        b.extend(solver);
        b.extend(salt);
        b.push(1);
        b.extend(c.schema_id);
        b.extend(c.base_root);
        b.push(0);
        b.extend([1, 2]);
        b.extend((data.len() as u64).to_be_bytes());
        b.extend(hash(data));
        b.extend((data.len() as u32).to_be_bytes());
        b.extend((chunks.len() as u16).to_be_bytes());
        b.extend(merkle_root(&leaves));
        b.extend([0; 4]);
        b.extend(40u16.to_be_bytes());
        b.extend([1, 0, 1, 0]);
        b.extend([0; 36]);
        let secret = hash(&[b"test cancel".as_slice(), &salt].concat());
        let commit = commit_hash(c.chain_id, p, cancel_hash(&secret), body_digest(&b));
        Self {
            body: b,
            chunks,
            leaves,
            commit,
            problem: p,
            secret,
        }
    }
    pub fn commit_body(&self) -> Vec<u8> {
        commit(self.problem, self.commit)
    }
    pub fn open(&self) -> Vec<u8> {
        [
            vec![1],
            vec![4; 20],
            cancel_hash(&self.secret).to_vec(),
            self.body.clone(),
        ]
        .concat()
    }
    pub fn part(&self, i: usize) -> Vec<u8> {
        part(
            self.commit,
            i as u16,
            &merkle_path(&self.leaves, i),
            &self.chunks[i],
        )
    }
    pub fn cancel(&self) -> Vec<u8> {
        [
            vec![1],
            self.problem.to_vec(),
            self.commit.to_vec(),
            body_digest(&self.body).to_vec(),
            self.secret.to_vec(),
        ]
        .concat()
    }
}
pub fn commit(p: Hash, ch: Hash) -> Vec<u8> {
    [vec![1], vec![3; 20], p.to_vec(), ch.to_vec()].concat()
}
pub fn part(ch: Hash, i: u16, path: &[Hash], data: &[u8]) -> Vec<u8> {
    let mut b = vec![1];
    b.extend([5; 20]);
    b.extend(ch);
    b.extend(i.to_be_bytes());
    for p in path {
        b.extend(p);
    }
    b.extend(data);
    b
}
pub fn calldata(kind: u8, body: &[u8]) -> Vec<u8> {
    let item = [vec![kind], body.to_vec()].concat();
    let mut b = hex_bytes(boole_derivation::config::POST_SELECTOR, "selector").expect("selector");
    let mut head = vec![0; 64];
    head[31] = 32;
    head[56..64].copy_from_slice(&(item.len() as u64).to_be_bytes());
    b.extend(head);
    b.extend(&item);
    b.extend(vec![0; (32 - item.len() % 32) % 32]);
    b
}
pub const BATCHER: &str = "0xb002000000000000000000000000000000000001";
/// Runtime loops through [u256 length | post(bytes) calldata] and CALLs inbox.
pub fn batch_runtime() -> Vec<u8> {
    let mut b = vec![0x5f, 0x5f, 0x52];
    let loop_at = b.len();
    b.push(0x5b);
    b.extend([0x36, 0x5f, 0x51, 0x10, 0x15, 0x61, 0, 0, 0x57]);
    let done_patch = b.len() - 3;
    b.extend([0x5f, 0x51, 0x35, 0x60, 0x20, 0x52]);
    b.extend([
        0x60, 0x20, 0x51, 0x5f, 0x51, 0x60, 0x20, 0x01, 0x60, 0x40, 0x37,
    ]);
    b.extend([0x5f, 0x5f, 0x60, 0x20, 0x51, 0x60, 0x40, 0x5f, 0x73]);
    b.extend(hex_bytes(boole_derivation::config::INBOX, "inbox").expect("inbox"));
    b.extend([0x5a, 0xf1, 0x50]);
    b.extend([
        0x5f, 0x51, 0x60, 0x20, 0x51, 0x01, 0x60, 0x20, 0x01, 0x5f, 0x52, 0x61,
    ]);
    b.extend((loop_at as u16).to_be_bytes());
    b.push(0x56);
    let done = b.len();
    b.extend([0x5b, 0x00]);
    b[done_patch..done_patch + 2].copy_from_slice(&(done as u16).to_be_bytes());
    b
}
pub fn post(rpc: &Rpc, user: &str, items: &[(u8, Vec<u8>)], batch: bool) -> Result<Value> {
    let (to, data, gas) = if batch {
        let mut b = vec![];
        for (k, v) in items {
            let call = calldata(*k, v);
            b.extend([0; 24]);
            b.extend((call.len() as u64).to_be_bytes());
            b.extend(call);
        }
        let tokens: u64 = b.iter().map(|x| if *x == 0 { 1 } else { 4 }).sum();
        let gas = 21_000 + items.len() as u64 * 22_000 + tokens * 16;
        (BATCHER, b, gas)
    } else {
        assert_eq!(items.len(), 1);
        (
            boole_derivation::config::INBOX,
            calldata(items[0].0, &items[0].1),
            if items[0].0 == 4 {
                5_100_000
            } else {
                1_000_000
            },
        )
    };
    rpc.call(
        "eth_sendTransaction",
        json!([{"from":user,"to":to,"data":hex_string(&data),"gas":format!("0x{gas:x}")}]),
    )
}
pub fn transfer(nonce: u64, gas: u64, to: &[u8], data: &[u8]) -> Vec<u8> {
    let mut f = vec![
        rlp::uint(720926),
        rlp::uint(nonce),
        rlp::uint(1),
        rlp::uint(2),
        rlp::uint(gas),
        rlp::bytes(to),
        rlp::uint(0),
        rlp::bytes(data),
        rlp::list(&[]),
    ];
    let (sig, id) = user_key()
        .sign_prehash_recoverable(&keccak(&[vec![2], rlp::list(&f)].concat()))
        .expect("user signature");
    f.push(rlp::uint(u64::from(id.to_byte())));
    let s = sig.to_bytes();
    for x in [&s[..32], &s[32..]] {
        f.push(rlp::bytes(
            &x[x.iter().position(|x| *x != 0).unwrap_or(x.len())..],
        ));
    }
    [vec![2], rlp::list(&f)].concat()
}
pub fn frame(path: &Path) -> Result<(Vec<u8>, Vec<u8>, Value)> {
    let mut raw = vec![0; 13 * PART_SIZE + 100];
    let mut x = 0x1234_5678_90ab_cdefu64;
    for b in &mut raw {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *b = x as u8;
    }
    let input = path.join("export.raw");
    fs::write(&input, &raw)?;
    let output = Command::new(zstd_binary()?)
        .args(["-q", "-1", "--check", "--no-dictID", "--stdout"])
        .arg(&input)
        .output()?;
    if !output.status.success() {
        bail!("zstd compression failed");
    }
    let compressed = output.stdout;
    assert_eq!(compressed.len().div_ceil(PART_SIZE), 14);
    let version = Command::new(zstd_binary()?).arg("--version").output()?;
    let info = json!({"decoder":String::from_utf8(version.stdout)?.trim(),"raw_len":raw.len(),"z_len":compressed.len(),"export_digest":hex_string(&hash(&raw)),"codec":1,"binary_sha256":format!("{:x}",Sha256::digest(fs::read(zstd_binary()?)?))});
    Ok((raw, compressed, info))
}
pub fn check_frame(path: &Path, compressed: &[u8], raw: &[u8]) -> Result<Value> {
    if compressed.get(..4) != Some(&[0x28, 0xb5, 0x2f, 0xfd]) {
        bail!("zstd magic");
    }
    let d = compressed[4];
    assert_eq!(d & 3, 0, "no dictionary");
    assert_ne!(d & 4, 0, "checksum");
    assert_eq!(d & 0x18, 0, "reserved frame bits");
    let single = d & 0x20 != 0;
    let mut at = 5;
    let window = if single {
        None
    } else {
        let w = compressed[at];
        at += 1;
        let base = 1u64 << (10 + (w >> 3));
        Some(base + (base / 8) * u64::from(w & 7))
    };
    let size = match d >> 6 {
        0 => usize::from(single),
        1 => 2,
        2 => 4,
        _ => 8,
    };
    assert_ne!(size, 0, "content size required");
    let mut bytes = [0; 8];
    bytes[..size].copy_from_slice(&compressed[at..at + size]);
    let content = u64::from_le_bytes(bytes) + if size == 2 { 256 } else { 0 };
    assert_eq!(content, raw.len() as u64);
    assert!(window.unwrap_or(content) <= content.min(1 << 27));
    let file = path.join("reconstructed.zst");
    fs::write(&file, compressed)?;
    let out = Command::new(zstd_binary()?)
        .args(["-q", "-d", "--stdout"])
        .arg(&file)
        .output()?;
    assert!(out.status.success());
    assert_eq!(out.stdout, raw);
    let listed = Command::new(zstd_binary()?)
        .args(["--list", "--verbose"])
        .arg(&file)
        .output()?;
    assert!(listed.status.success());
    let text = String::from_utf8(listed.stdout)?;
    assert!(
        text.lines()
            .any(|l| l.contains("Frames") && l.split_whitespace().last() == Some("1")),
        "{text}"
    );
    let public_list = text
        .lines()
        .filter(|line| !line.contains(file.to_string_lossy().as_ref()))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(
        json!({"content_size":content,"window":window.unwrap_or(content),"checksum":true,"no_dictionary":true,"single_frame":true,"export_digest":hex_string(&hash(raw)),"list":public_list}),
    )
}

/// The complete L1 input used by all M2 scenarios, including the test caller.
pub fn l1_genesis_value() -> Result<Value> {
    let mut g: Value = serde_json::from_slice(&fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/devnet-m2/l1-genesis-v1.json"),
    )?)?;
    g["alloc"][BATCHER] = json!({"code":hex_string(&batch_runtime()),"balance":"0x0"});
    Ok(g)
}
/// Called by the tracked generators before writing accepted recordings.
pub fn bind_provenance(data: &mut Value) -> Result<()> {
    if let Some(config) = data.get("config") {
        let c: Config = serde_json::from_value(config.clone())?;
        data["provenance"]["config_digest"] = json!(hex_string(&c.digest()));
        data["provenance"]["slot_table_digest"] = json!(hex_string(&c.slot_table_digest()));
    }
    let g = l1_genesis_value()?;
    data["l1_genesis"] = g.clone();
    data["provenance"]["l1_genesis_sha256"] = json!(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec_pretty(&g)?)
    ));
    data["provenance"]["source_l1_genesis_sha256"] = json!(format!(
        "{:x}",
        Sha256::digest(fs::read(super::fixture("l1-genesis.json"))?)
    ));
    data["provenance"]["source_genesis_sha256"] = json!(hex_string(&hash(&fs::read(
        super::fixture("l2-genesis.json")
    )?)));
    data["provenance"]["inbox_change"]=json!("Keep size scratch at max(0x10000, MSIZE after calldata copy), outside emitted bytes; preserve small-call memory cost.");
    if let Some(a) = data["acceptance"].as_object_mut() {
        a.remove("comparison_output_sha256");
    }
    Ok(())
}
pub fn zstd_binary() -> Result<std::path::PathBuf> {
    let path = std::env::var_os("BOOLE_DEVNET_ZSTD").ok_or_else(|| {
        anyhow::anyhow!("BOOLE_DEVNET_ZSTD must name the pinned codec executable")
    })?;
    let path = std::path::PathBuf::from(path);
    let pin: Value = serde_json::from_slice(&fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/devnet-m2/zstd-pin-v1.json"),
    )?)?;
    if format!("{:x}", Sha256::digest(fs::read(&path)?)) != pin["sha256"].as_str().unwrap_or("") {
        bail!("zstd binary digest mismatch");
    }
    Ok(path)
}
