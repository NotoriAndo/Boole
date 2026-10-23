//! Reconstruct only authenticated, first-accepted L1 bytes. No cache is authority.
use crate::{
    codec::{digest, validate_frame, Codec},
    GateInput,
};
use anyhow::{ensure, Context, Result};
use boole_derivation::{m2::*, InboxLog, L1Block};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
pub trait History {
    fn origin(&mut self, height: u64) -> Result<(L1Block, Vec<InboxLog>)>;
}
pub struct Rebuilt {
    pub input: GateInput,
    pub transport: Vec<u8>,
    pub body: Vec<u8>,
    pub contributions: Vec<Contribution>,
    pub components: Vec<Hash>,
}
#[derive(Debug)]
pub struct Contribution {
    pub height: u64,
    pub poster: [u8; 20],
    pub item_len: usize,
    pub base_fee: String,
}
#[derive(Debug)]
pub struct Integrity(pub &'static str);
impl std::fmt::Display for Integrity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "anchored input integrity: {}", self.0)
    }
}
impl std::error::Error for Integrity {}
#[derive(Debug)]
pub struct Availability;
impl std::fmt::Display for Availability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("anchored L1 bytes unavailable")
    }
}
impl std::error::Error for Availability {}
#[derive(Debug)]
pub struct NodeUnavailable;
impl std::fmt::Display for NodeUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("node-local decoder unavailable")
    }
}
impl std::error::Error for NodeUnavailable {}
fn contribution(h: u64, b: &[u8], block: &L1Block) -> Result<Contribution> {
    Ok(Contribution {
        height: h,
        poster: b.get(1..21).context("poster")?.try_into()?,
        item_len: b.len() + 1,
        base_fee: block.base_fee_per_gas.clone(),
    })
}
struct Origin {
    block: L1Block,
    items: Vec<Input>,
}
fn read(history: &mut dyn History, h: u64) -> Result<Origin> {
    let (block, mut logs) = match history.origin(h) {
        Ok(origin) => origin,
        Err(e) if e.is::<Integrity>() || e.is::<boole_derivation::InputError>() => return Err(e),
        Err(e) => return Err(e.context(Availability)),
    };
    ensure!(block.number == h, "anchored origin height");
    boole_derivation::system::l1_info(&block)?;
    logs.sort_by_key(|l| (l.transaction_index, l.log_index));
    let mut seen = BTreeSet::new();
    let mut items = vec![];
    for l in logs {
        ensure!(
            !l.removed
                && l.block_hash.eq_ignore_ascii_case(&block.hash)
                && l.address
                    .eq_ignore_ascii_case(boole_derivation::config::INBOX)
                && l.topics.len() == 1
                && l.topics[0].eq_ignore_ascii_case(&boole_derivation::config::posted_topic())
                && seen.insert((l.transaction_index, l.log_index)),
            "anchored log identity"
        );
        let data = boole_derivation::attributes::hex_bytes(&l.data, "rebuild log")?;
        if let Ok((kind, body)) = boole_derivation::inbox::item(&data) {
            items.push(Input {
                kind,
                body: body.to_vec(),
                transaction_index: l.transaction_index,
                log_index: l.log_index,
            });
        }
    }
    Ok(Origin { block, items })
}
fn frozen(origin: &Origin, pos: (u64, u64, u64), kind: u8) -> Result<&Input> {
    origin
        .items
        .iter()
        .find(|i| i.transaction_index == pos.1 && i.log_index == pos.2 && i.kind == kind)
        .context(Availability)
}
fn depth(n: u32) -> usize {
    if n <= 1 {
        0
    } else {
        (32 - (n - 1).leading_zeros()) as usize
    }
}
fn proof(mut leaf: Hash, mut index: u32, path: &[Hash], root: Hash) -> bool {
    for n in path {
        leaf = if index & 1 == 0 {
            digest(&[&[1], leaf.as_slice(), n.as_slice()].concat())
        } else {
            digest(&[&[1], n.as_slice(), leaf.as_slice()].concat())
        };
        index >>= 1;
    }
    leaf == root
}
fn array<const N: usize>(b: &[u8], at: usize) -> Result<[u8; N]> {
    Ok(b.get(at..at + N).context("rebuild layout")?.try_into()?)
}
fn decode(
    bytes: &[u8],
    len: u64,
    codec: Option<&Codec>,
    scratch: &Path,
) -> Result<Option<Vec<u8>>> {
    if validate_frame(bytes, len).is_err() {
        return Ok(None);
    }
    let decoder = codec.context(NodeUnavailable)?;
    match decoder.decode(bytes, len, scratch) {
        Ok(raw) => Ok(Some(raw)),
        Err(e) if e.is::<crate::codec::InvalidData>() => Ok(None),
        Err(e) => Err(e.context(NodeUnavailable)),
    }
}
pub fn rebuild(
    s: &State,
    c: &Config,
    key: Key,
    history: &mut dyn History,
    codec: Option<&Codec>,
    scratch: &Path,
) -> Result<Rebuilt> {
    let live = s
        .candidates
        .get(&key)
        .and_then(Candidate::live)
        .context("retained candidate")?;
    let open = live.open.as_ref().context("frozen OPEN")?;
    let done = open.done_pos.context("frozen completion")?;
    let p = s.problems.get(&key.0).context("retained problem")?;
    ensure!(
        done.0 == open.h_r.context("completion height")?
            && open.open_pos.0 == open.h_o
            && done.0 <= s.height(),
        "frozen heights"
    );
    let start = read(history, open.h_o)?;
    let opening = frozen(&start, open.open_pos, 3)?;
    let b = opening.body.get(53..).context("OPEN B")?.to_vec();
    ensure!(
        opening.body[0] == 1 && b.first() == Some(&1) && body_digest(&b) == open.body_digest,
        "OPEN body digest"
    );
    ensure!(
        array::<32>(&b, 1)? == key.0
            && array::<20>(&b, 33)? == open.solver
            && array::<32>(&b, 53)? == open.salt,
        "OPEN frozen identity"
    );
    ensure!(
        commit_hash(
            c.chain_id,
            key.0,
            array(&opening.body, 21)?,
            body_digest(&b)
        ) == key.1,
        "OPEN commit identity"
    );
    let k = usize::from(*b.get(150).context("package count")?);
    let q = 151 + 32 * k;
    ensure!(b.len() == q + 126 && b[q] == 1, "accepted B layout");
    ensure!(
        array::<20>(&opening.body, 1)? == open.poster
            && u64::from_be_bytes(array(&b, q + 2)?) == open.raw_len
            && u32::from_be_bytes(array(&b, q + 42)?) == open.z_len
            && u16::from_be_bytes(array(&b, q + 46)?) == open.n
            && array::<32>(&b, q + 48)? == open.data_root,
        "OPEN frozen transport fields"
    );
    let packages: Vec<Hash> = b[151..q]
        .chunks_exact(32)
        .map(|v| v.try_into().expect("hash"))
        .collect();
    ensure!(
        env_id(array(&b, 86)?, array(&b, 118)?, &packages) == open.env_id
            && array::<32>(&b, q + 10)? == open.export_digest,
        "OPEN environment/export binding"
    );
    ensure!(
        array::<32>(&b, 86)? == c.schema_id
            && array::<32>(&b, 118)? == c.base_root
            && c.env_allowed.contains(&packages),
        "OPEN allowed components"
    );
    let commit_origin = read(history, live.h_c)?;
    let commit = frozen(
        &commit_origin,
        (live.h_c, live.transaction_index, live.log_index),
        2,
    )?;
    ensure!(
        array::<32>(&commit.body, 21)? == key.0
            && array::<32>(&commit.body, 53)? == key.1
            && array::<20>(&commit.body, 1)? == live.poster,
        "COMMIT frozen identity"
    );
    let mut contributions = vec![
        contribution(live.h_c, &commit.body, &commit_origin.block)?,
        contribution(open.h_o, &opening.body, &start.block)?,
    ];
    let mut parts = BTreeMap::new();
    let mut last = None;
    for h in open.h_o..=done.0 {
        let origin = if h == open.h_o {
            None
        } else {
            Some(read(history, h)?)
        };
        let origin = origin.as_ref().unwrap_or(&start);
        for item in &origin.items {
            let pos = (h, item.transaction_index, item.log_index);
            if item.kind != 4
                || pos <= open.open_pos
                || pos > done
                || item.body.get(..1) != Some(&[1])
                || item.body.get(21..53) != Some(key.1.as_slice())
            {
                continue;
            }
            let Ok(index) = array::<2>(&item.body, 53).map(u16::from_be_bytes) else {
                continue;
            };
            if index >= open.n || parts.contains_key(&index) {
                continue;
            }
            let end = 55 + depth(u32::from(open.n)) * 32;
            let Some(raw) = item.body.get(end..) else {
                continue;
            };
            let expected = if index + 1 == open.n {
                open.z_len as usize - (usize::from(open.n) - 1) * PART_SIZE
            } else {
                PART_SIZE
            };
            if raw.len() != expected {
                continue;
            }
            let path = item.body[55..end]
                .chunks_exact(32)
                .map(|v| v.try_into().expect("hash"))
                .collect::<Vec<_>>();
            if !proof(
                part_leaf(&open.solver, &open.salt, index, raw),
                u32::from(index),
                &path,
                open.data_root,
            ) {
                continue;
            }
            parts.insert(index, raw.to_vec());
            last = Some(pos);
            contributions.push(contribution(h, &item.body, &origin.block)?);
        }
    }
    ensure!(
        parts.len() == usize::from(open.n) && last == Some(done),
        "accepted parts missing or completion changed"
    );
    let transport = parts.into_values().flatten().collect::<Vec<_>>();
    ensure!(transport.len() == open.z_len as usize, "transport length");
    let raw = decode(&transport, open.raw_len, codec, scratch)?;
    let mut early = None;
    let raw = match raw {
        Some(raw) if digest(&raw) == open.export_digest => raw,
        Some(_) => {
            early = Some((2, 0x0102));
            vec![]
        }
        None => {
            early = Some((2, 0x0101));
            vec![]
        }
    };
    let reveal_origin = read(history, p.reveal_pos.0)?;
    let reveal = frozen(&reveal_origin, p.reveal_pos, 9)?;
    let r = &reveal.body;
    ensure!(
        r.first() == Some(&2)
            && r.get(1) == Some(&p.slot)
            && u32::from_be_bytes(array(r, 6)?) == p.leaf_index,
        "original reveal identity"
    );
    let kind = *r.get(10).context("leaf kind")?;
    let req = usize::from(*r.get(11).context("required count")?);
    let at = 12 + 32 * req;
    let required = r
        .get(12..at)
        .context("required packages")?
        .chunks_exact(32)
        .map(|v| v.try_into().expect("hash"))
        .collect::<Vec<Hash>>();
    ensure!(required == p.required_pkgs, "original required packages");
    let salt: Hash = array(r, at)?;
    let encoding = *r.get(at + 32).context("package codec")?;
    let raw_len = u32::from_be_bytes(array(r, at + 33)?);
    let d = usize::from(*r.get(at + 37).context("leaf path")?);
    let end = at + 38 + d * 32;
    let size = u32::from_be_bytes(array(r, end)?) as usize;
    let carried = r.get(end + 4..).context("carried package")?;
    let pool = s.pools.get(&p.pool_seq).context("original pool")?;
    let path = r[at + 38..end]
        .chunks_exact(32)
        .map(|v| v.try_into().expect("hash"))
        .collect::<Vec<_>>();
    ensure!(
        size == carried.len()
            && d == depth(pool.count)
            && proof(
                pool_leaf(c.chain_id, kind, &required, encoding, raw_len, carried, &salt),
                p.leaf_index,
                &path,
                pool.root
            ),
        "original package membership"
    );
    let package = if encoding == 0 {
        Some(carried.to_vec())
    } else {
        decode(carried, u64::from(raw_len), codec, scratch)?
    };
    if package.is_none() && early.is_none() {
        early = Some((3, 0x0801));
    }
    let mut components = vec![c.base_root];
    components.extend(packages);
    Ok(Rebuilt {
        input: GateInput {
            chain_id: c.chain_id,
            problem_id: key.0,
            commit_hash: key.1,
            solver: open.solver,
            env_id: open.env_id,
            export_digest: open.export_digest,
            lane: c.slot_table[p.slot as usize].lane,
            leaf_kind: kind,
            instance_seed: p.instance_seed,
            fuel_slot: c.m3.as_ref().context("M3 profile")?.fuel_slot[p.slot as usize],
            raw,
            package: package.unwrap_or_default(),
            artifacts: vec![],
            early,
        },
        transport,
        body: b,
        contributions,
        components,
    })
}
