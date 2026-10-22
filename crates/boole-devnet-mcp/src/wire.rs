//! Client-side M2 encoders; hash and Merkle primitives come from derivation.
use anyhow::{bail, ensure, Result};
use boole_derivation::{attributes::hex_bytes, m2::*};
use serde::{Deserialize, Serialize};

pub struct Material {
    pub problem: Hash,
    pub payee: [u8; 20],
    pub salt: Hash,
    pub cancel_secret: Hash,
    pub raw_len: u64,
    pub export_digest: Hash,
    pub compressed: Vec<u8>,
    pub packages: Vec<Hash>,
    pub format: u8,
}

// Deliberately no Debug: the body contains the salt, and the token is private.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prepared {
    pub problem: Hash,
    pub commit: Hash,
    pub body: Vec<u8>,
    pub chunks: Vec<Vec<u8>>,
    pub leaves: Vec<Hash>,
    pub cancel_secret: Hash,
}
impl Prepared {
    pub fn new(c: &Config, m: Material) -> Result<Self> {
        ensure!(m.raw_len > 0 && m.raw_len <= RAW_CAP, "raw export size cap");
        ensure!(
            !m.compressed.is_empty() && m.compressed.len() <= Z_CAP,
            "compressed export size cap"
        );
        ensure!(
            m.raw_len
                <= (m.compressed.len() as u64)
                    .checked_mul(c.ratio_max)
                    .unwrap_or(0),
            "export ratio cap"
        );
        ensure!([1, 2].contains(&m.format), "export format");
        ensure!(
            m.packages.len() <= c.k_max && c.env_allowed.contains(&m.packages),
            "environment not allowed"
        );
        ensure!(
            c.terms_table.contains_key(&1),
            "placeholder license unavailable"
        );
        let chunks: Vec<Vec<u8>> = m.compressed.chunks(PART_SIZE).map(|x| x.to_vec()).collect();
        let leaves: Vec<_> = chunks
            .iter()
            .enumerate()
            .map(|(i, d)| part_leaf(&m.payee, &m.salt, i as u16, d))
            .collect();
        let mut b = vec![1];
        b.extend(m.problem);
        b.extend(m.payee);
        b.extend(m.salt);
        b.push(1);
        b.extend(c.schema_id);
        b.extend(c.base_root);
        b.push(m.packages.len() as u8);
        for pkg in m.packages {
            b.extend(pkg);
        }
        b.extend([1, m.format]);
        b.extend(m.raw_len.to_be_bytes());
        b.extend(m.export_digest);
        b.extend((m.compressed.len() as u32).to_be_bytes());
        b.extend((chunks.len() as u16).to_be_bytes());
        b.extend(merkle_root(&leaves));
        b.extend([0; 4]);
        b.extend(40u16.to_be_bytes());
        b.extend([1, 0, 1, 0]);
        b.extend([0; 36]);
        Ok(Self {
            problem: m.problem,
            commit: commit_hash(
                c.chain_id,
                m.problem,
                cancel_hash(&m.cancel_secret),
                body_digest(&b),
            ),
            body: b,
            chunks,
            leaves,
            cancel_secret: m.cancel_secret,
        })
    }
    pub fn validate(&self, c: &Config) -> Result<()> {
        ensure!(self.body.len() >= 277, "corrupt submission body");
        let b = &self.body;
        let k = usize::from(b[150]);
        let q = 151 + 32 * k;
        ensure!(q + 126 == b.len(), "corrupt submission layout");
        let packages = b[151..q]
            .chunks_exact(32)
            .map(|x| x.try_into().expect("hash chunk"))
            .collect();
        let m = Material {
            problem: b[1..33].try_into()?,
            payee: b[33..53].try_into()?,
            salt: b[53..85].try_into()?,
            cancel_secret: self.cancel_secret,
            raw_len: u64::from_be_bytes(b[q + 2..q + 10].try_into()?),
            export_digest: b[q + 10..q + 42].try_into()?,
            compressed: self.chunks.concat(),
            packages,
            format: b[q + 1],
        };
        let expected = Self::new(c, m)?;
        ensure!(
            expected.problem == self.problem
                && expected.commit == self.commit
                && expected.body == self.body
                && expected.chunks == self.chunks
                && expected.leaves == self.leaves,
            "corrupt submission identity"
        );
        Ok(())
    }
    pub fn commit_body(&self, poster: [u8; 20]) -> Vec<u8> {
        [
            vec![1],
            poster.to_vec(),
            self.problem.to_vec(),
            self.commit.to_vec(),
        ]
        .concat()
    }
    pub fn open(&self, poster: [u8; 20]) -> Vec<u8> {
        [
            vec![1],
            poster.to_vec(),
            cancel_hash(&self.cancel_secret).to_vec(),
            self.body.clone(),
        ]
        .concat()
    }
    pub fn part(&self, poster: [u8; 20], index: u16) -> Result<Vec<u8>> {
        let i = usize::from(index);
        if i >= self.chunks.len() {
            bail!("part index");
        }
        let mut b = [
            vec![1],
            poster.to_vec(),
            self.commit.to_vec(),
            index.to_be_bytes().to_vec(),
        ]
        .concat();
        for node in merkle_path(&self.leaves, i) {
            b.extend(node);
        }
        b.extend(&self.chunks[i]);
        Ok(b)
    }
    pub fn cancel(&self) -> Vec<u8> {
        [
            vec![1],
            self.problem.to_vec(),
            self.commit.to_vec(),
            body_digest(&self.body).to_vec(),
            self.cancel_secret.to_vec(),
        ]
        .concat()
    }
}
pub fn calldata(kind: u8, body: &[u8]) -> Vec<u8> {
    let item = [vec![kind], body.to_vec()].concat();
    let mut b =
        hex_bytes(boole_derivation::config::POST_SELECTOR, "selector").expect("constant selector");
    let mut head = [0; 64];
    head[31] = 32;
    head[56..].copy_from_slice(&(item.len() as u64).to_be_bytes());
    b.extend(head);
    b.extend(&item);
    b.resize(4 + 64 + item.len().div_ceil(32) * 32, 0);
    b
}
