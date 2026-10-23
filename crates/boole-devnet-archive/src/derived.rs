//! Public object verification always starts from the caller's frozen state/pin.
use anyhow::{ensure, Context, Result};
use boole_derivation::attributes::hex_string;
use boole_derivation::m2::{Hash, Key, OpenRecord, State};
fn opening(state: &State, key: Key) -> Result<&OpenRecord> {
    state
        .candidates
        .get(&key)
        .and_then(|c| c.live())
        .and_then(|c| c.open.as_ref())
        .context("frozen candidate OPEN unavailable")
}
pub fn verify_export(state: &State, key: Key, address: &str, bytes: &[u8]) -> Result<()> {
    let o = opening(state, key)?;
    ensure!(
        bytes.len() as u64 == o.raw_len && o.raw_len <= boole_derivation::m2::RAW_CAP,
        "frozen export length/cap"
    );
    ensure!(
        address == hex_string(&o.export_digest) && crate::archive::sha(bytes) == address,
        "export differs from this commit's frozen digest"
    );
    Ok(())
}
pub fn check_transport(
    solver: &[u8; 20],
    salt: &Hash,
    n: u16,
    z_len: u32,
    data_root: Hash,
    bytes: &[u8],
) -> Result<()> {
    use boole_derivation::m2::{merkle_root, part_leaf, MAX_PARTS, PART_SIZE, Z_CAP};
    ensure!(
        (1..=MAX_PARTS).contains(&n)
            && !bytes.is_empty()
            && bytes.len() <= Z_CAP
            && bytes.len() == z_len as usize
            && bytes.len().div_ceil(PART_SIZE) == usize::from(n),
        "frozen transport length/part cap"
    );
    let leaves = bytes
        .chunks(PART_SIZE)
        .enumerate()
        .map(|(i, b)| part_leaf(solver, salt, i as u16, b))
        .collect::<Vec<_>>();
    ensure!(
        merkle_root(&leaves) == data_root,
        "transport differs from frozen keyed leaf root"
    );
    Ok(())
}
pub fn verify_transport(state: &State, key: Key, address: &str, bytes: &[u8]) -> Result<()> {
    let o = opening(state, key)?;
    ensure!(address == hex_string(&key.1), "transport commit address");
    check_transport(&o.solver, &o.salt, o.n, o.z_len, o.data_root, bytes)
}
pub fn content_limit(class: &str) -> Result<usize> {
    Ok(match class {
        "exp" | "env" | "gate" | "pkg" => boole_derivation::m2::RAW_CAP as usize,
        "tr" => boole_derivation::m2::Z_CAP,
        "vstmt" => 1_048_576,
        _ => anyhow::bail!("not a public archive object class"),
    })
}
pub fn verify_content(class: &str, address: &str, bytes: &[u8], expected: Hash) -> Result<()> {
    ensure!(
        !bytes.is_empty() && bytes.len() <= content_limit(class)?,
        "object class cap"
    );
    ensure!(
        address == hex_string(&expected) && crate::archive::sha(bytes) == address,
        "object differs from frozen content pin"
    );
    Ok(())
}
pub fn verify_index(
    row: &crate::archive::IndexRow,
    height: u64,
    l1_hash: &str,
    l2_hash: &str,
    root: &str,
) -> Result<()> {
    row.validate()?;
    ensure!(
        row.l1_number == height
            && row.l1_hash == l1_hash
            && row.l2_hash == l2_hash
            && row.root == root,
        "archive index differs; local fold wins"
    );
    Ok(())
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TransportEnvelope {
    chain_id: u64,
    problem_id: Hash,
    cancel_hash: Hash,
    body: String,
    transport: String,
}
fn array<const N: usize>(b: &[u8], at: usize) -> Result<[u8; N]> {
    Ok(b.get(at..at + N)
        .context("transport envelope layout")?
        .try_into()?)
}
pub fn transport_envelope(
    state: &State,
    key: Key,
    chain_id: u64,
    t: Hash,
    body: &[u8],
    bytes: &[u8],
) -> Result<Vec<u8>> {
    let o = opening(state, key)?;
    verify_transport(state, key, &hex_string(&key.1), bytes)?;
    ensure!(
        boole_derivation::m2::body_digest(body) == o.body_digest
            && boole_derivation::m2::commit_hash(chain_id, key.0, t, o.body_digest) == key.1,
        "frozen transport body/commit binding"
    );
    let encoded = serde_json::to_vec(&TransportEnvelope {
        chain_id,
        problem_id: key.0,
        cancel_hash: t,
        body: hex_string(body),
        transport: hex_string(bytes),
    })?;
    read_transport_envelope(&hex_string(&key.1), &encoded)?;
    Ok(encoded)
}
pub fn read_transport_envelope(address: &str, encoded: &[u8]) -> Result<Vec<u8>> {
    use boole_derivation::{
        attributes::hex_bytes,
        m2::{body_digest, commit_hash, Z_CAP},
    };
    ensure!(encoded.len() <= 2 * Z_CAP + 65536, "transport framing cap");
    let v: TransportEnvelope = serde_json::from_slice(encoded)?;
    let b = hex_bytes(&v.body, "public OPEN body")?;
    ensure!(
        b.len() <= 65536 && b.first() == Some(&1),
        "OPEN body framing"
    );
    let k = usize::from(*b.get(150).context("OPEN component count")?);
    let q = 151 + 32 * k;
    ensure!(
        b.len() >= q + 126 && b[q] == 1 && b[q + 80..q + 84] == [0; 4],
        "OPEN accepted transport layout"
    );
    let meta_len = usize::from(u16::from_be_bytes(array(&b, q + 84)?));
    ensure!(
        meta_len >= 40
            && b.len() == q + 86 + meta_len
            && b[q + 86] == 1
            && usize::from(b[q + 87]) + 40 == meta_len,
        "OPEN metadata framing"
    );
    ensure!(
        hex_string(&commit_hash(
            v.chain_id,
            v.problem_id,
            v.cancel_hash,
            body_digest(&b)
        )) == address,
        "transport envelope commit preimage"
    );
    let raw = hex_bytes(&v.transport, "transport")?;
    check_transport(
        &array(&b, 33)?,
        &array(&b, 53)?,
        u16::from_be_bytes(array(&b, q + 46)?),
        u32::from_be_bytes(array(&b, q + 42)?),
        array(&b, q + 48)?,
        &raw,
    )?;
    Ok(raw)
}
