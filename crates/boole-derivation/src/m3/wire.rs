use super::Error;
use crate::{
    attributes::{hex_bytes, hex_string},
    config,
    m2::{Config, Hash},
    rlp, L1Block,
};
use sha2::{Digest, Sha256};

pub const KERNEL_FAULT: u16 = 0x0306;
/// Stub descriptor assignment inside the reserved policy range; no gate is run here.
pub const POLICY_RESERVED_NAME: u16 = 0x0201;
pub const PACKAGE_INVALID: u16 = 0x0801;
pub const QUARANTINE_VOID: u16 = 0x0802;
pub const FUEL_CAP_INVALID: u16 = 0x0803;
pub const GENERATOR_UNAVAILABLE: u16 = 0x0804;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RecordKind {
    Verdict = 1,
    Recheck = 2,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Outcome {
    Accept = 1,
    Reject = 2,
    Void = 3,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerdictRecord {
    pub kind: RecordKind,
    pub commit_hash: Hash,
    pub outcome: Outcome,
    pub reason: u16,
    pub gate_epoch: u32,
    pub fuel: u64,
    pub vstmt_digest: Hash,
}
impl VerdictRecord {
    pub fn validate(&self) -> Result<(), Error> {
        validate_reason(self.outcome, self.reason)
    }
    pub fn encode(&self) -> Result<[u8; 80], Error> {
        self.validate()?;
        let mut b = [0; 80];
        b[0] = self.kind as u8;
        b[1..33].copy_from_slice(&self.commit_hash);
        b[33] = self.outcome as u8;
        b[34..36].copy_from_slice(&self.reason.to_be_bytes());
        b[36..40].copy_from_slice(&self.gate_epoch.to_be_bytes());
        b[40..48].copy_from_slice(&self.fuel.to_be_bytes());
        b[48..].copy_from_slice(&self.vstmt_digest);
        Ok(b)
    }
    pub fn decode(b: &[u8]) -> Result<Self, Error> {
        if b.len() != 80 {
            return Err(Error::Format("record length"));
        }
        let kind = match b[0] {
            1 => RecordKind::Verdict,
            2 => RecordKind::Recheck,
            _ => return Err(Error::Format("record kind")),
        };
        let outcome = match b[33] {
            1 => Outcome::Accept,
            2 => Outcome::Reject,
            3 => Outcome::Void,
            _ => return Err(Error::Format("record outcome")),
        };
        let r = Self {
            kind,
            commit_hash: b[1..33].try_into().map_err(|_| Error::Format("commit"))?,
            outcome,
            reason: u16::from_be_bytes(b[34..36].try_into().map_err(|_| Error::Format("reason"))?),
            gate_epoch: u32::from_be_bytes(
                b[36..40].try_into().map_err(|_| Error::Format("epoch"))?,
            ),
            fuel: u64::from_be_bytes(b[40..48].try_into().map_err(|_| Error::Format("fuel"))?),
            vstmt_digest: b[48..].try_into().map_err(|_| Error::Format("statement"))?,
        };
        r.validate()?;
        Ok(r)
    }
}

pub fn validate_reason(outcome: Outcome, reason: u16) -> Result<(), Error> {
    // The descriptor closes individual labels. The fold fixes categories and
    // only interprets the explicitly reserved codes above.
    let valid = match outcome {
        Outcome::Accept => reason == 0,
        Outcome::Reject => matches!(reason >> 8, 1..=3),
        Outcome::Void => matches!(
            reason,
            PACKAGE_INVALID | QUARANTINE_VOID | FUEL_CAP_INVALID | GENERATOR_UNAVAILABLE
        ),
    };
    if valid {
        Ok(())
    } else {
        Err(Error::Format("outcome/reason"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedVerdict {
    pub queue_index: u64,
    pub record: VerdictRecord,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewardPayment {
    pub reward_id: u64,
    pub solver: [u8; 20],
    pub amount: u128,
}

fn number(value: u128) -> Vec<u8> {
    let raw = value.to_be_bytes();
    raw[raw.iter().position(|b| *b != 0).unwrap_or(raw.len())..].to_vec()
}
fn deposit(
    domain: &[u8],
    l1: &L1Block,
    index: u64,
    to: &[u8; 20],
    amount: u128,
    gas: u64,
    data: &[u8],
) -> Result<String, Error> {
    let origin = hex_bytes(&l1.hash, "origin").map_err(|_| Error::Format("origin"))?;
    if origin.len() != 32 {
        return Err(Error::Format("origin"));
    }
    let mut position = [0; 32];
    position[24..].copy_from_slice(&index.to_be_bytes());
    let source = Sha256::digest([domain, &origin, &position].concat());
    let from = hex_bytes(config::DEPOSITOR, "depositor").map_err(|_| Error::Format("depositor"))?;
    let fields = vec![
        rlp::bytes(&source),
        rlp::bytes(&from),
        rlp::bytes(to),
        rlp::bytes(&number(amount)),
        rlp::bytes(&number(amount)),
        rlp::uint(gas),
        rlp::uint(0),
        rlp::bytes(data),
    ];
    Ok(hex_string(&[vec![0x7e], rlp::list(&fields)].concat()))
}
pub fn verdict_deposits(
    c: &Config,
    l1: &L1Block,
    records: &[IndexedVerdict],
) -> Result<Vec<String>, Error> {
    let p = c.m3.as_ref().ok_or(Error::ProfileRequired)?;
    if records.is_empty() {
        return Ok(vec![]);
    }
    let per = usize::try_from(c.v_per_deposit).map_err(|_| Error::Overflow)?;
    if per == 0 || per > usize::from(u16::MAX) {
        return Err(Error::Format("records per deposit"));
    }
    let count = records.len().div_ceil(per);
    if u64::try_from(count).map_err(|_| Error::Overflow)? > c.q_max || count > usize::from(u8::MAX)
    {
        return Err(Error::Format("deposit count"));
    }
    let first = records[0].queue_index;
    for (i, r) in records.iter().enumerate() {
        if first.checked_add(u64::try_from(i).map_err(|_| Error::Overflow)?) != Some(r.queue_index)
        {
            return Err(Error::Format("noncontiguous indices"));
        }
    }
    let mut out = vec![];
    for (j, batch) in records.chunks(per).enumerate() {
        let mut data = b"m3vd".to_vec();
        data.extend(l1.number.to_be_bytes());
        data.extend([j as u8, count as u8]);
        data.extend(batch[0].queue_index.to_be_bytes());
        data.extend((batch.len() as u16).to_be_bytes());
        for item in batch {
            data.extend(item.record.encode()?);
        }
        out.push(deposit(
            b"BOOLE-S1/VERDICT",
            l1,
            j as u64,
            &p.verdict_sink,
            0,
            c.verdict_gas,
            &data,
        )?);
    }
    Ok(out)
}
fn fields(raw: &[u8]) -> Result<Vec<rlp::Item<'_>>, Error> {
    if raw.first() != Some(&0x7e) {
        return Err(Error::Format("deposit type"));
    }
    let f = rlp::decode(&raw[1..])
        .map_err(|_| Error::Format("deposit RLP"))?
        .children
        .ok_or(Error::Format("deposit list"))?;
    if f.len() != 8 || f.iter().any(|x| x.children.is_some()) {
        return Err(Error::Format("deposit fields"));
    }
    Ok(f)
}
pub fn decode_verdict_deposits(
    c: &Config,
    l1: &L1Block,
    deposits: &[String],
) -> Result<Vec<IndexedVerdict>, Error> {
    c.m3.as_ref().ok_or(Error::ProfileRequired)?;
    if deposits.len() as u64 > c.q_max {
        return Err(Error::Format("deposit count"));
    }
    let maximum = c
        .v_per_deposit
        .checked_mul(80)
        .and_then(|n| n.checked_add(536))
        .and_then(|n| n.checked_mul(2))
        .and_then(|n| n.checked_add(2))
        .ok_or(Error::Overflow)?;
    let mut out = vec![];
    let mut original = vec![];
    for tx in deposits {
        if tx.len() as u64 > maximum {
            return Err(Error::Format("deposit size"));
        }
        let raw = hex_bytes(tx, "verdict").map_err(|_| Error::Format("deposit hex"))?;
        let f = fields(&raw)?;
        let data = f[7].bytes;
        if data.len() < 24 || &data[..4] != b"m3vd" || data[4..12] != l1.number.to_be_bytes() {
            return Err(Error::Format("verdict header"));
        }
        let first = u64::from_be_bytes(
            data[14..22]
                .try_into()
                .map_err(|_| Error::Format("first index"))?,
        );
        let n = usize::from(u16::from_be_bytes(
            data[22..24]
                .try_into()
                .map_err(|_| Error::Format("record count"))?,
        ));
        if n == 0 || n as u64 > c.v_per_deposit || data.len() != 24 + 80 * n {
            return Err(Error::Format("record count"));
        }
        for (i, b) in data[24..].chunks_exact(80).enumerate() {
            out.push(IndexedVerdict {
                queue_index: first.checked_add(i as u64).ok_or(Error::Overflow)?,
                record: VerdictRecord::decode(b)?,
            });
        }
        original.push(raw);
    }
    let canonical = verdict_deposits(c, l1, &out)?;
    if canonical.len() != original.len() {
        return Err(Error::Format("batch layout"));
    }
    for (expected, raw) in canonical.iter().zip(original) {
        if hex_bytes(expected, "canonical verdict").map_err(|_| Error::Format("deposit hex"))?
            != raw
        {
            return Err(Error::Format("noncanonical verdict deposit"));
        }
    }
    Ok(out)
}
pub fn reward_deposit(c: &Config, l1: &L1Block, payment: &RewardPayment) -> Result<String, Error> {
    let p = c.m3.as_ref().ok_or(Error::ProfileRequired)?;
    if !p.mint_enabled || payment.amount == 0 {
        return Err(Error::Format("reward mint disabled or zero"));
    }
    deposit(
        b"BOOLE-S1/REWARD",
        l1,
        payment.reward_id,
        &payment.solver,
        payment.amount,
        p.reward_gas,
        &[],
    )
}
pub fn decode_reward_deposit(
    c: &Config,
    l1: &L1Block,
    reward_id: u64,
    tx: &str,
) -> Result<RewardPayment, Error> {
    c.m3.as_ref().ok_or(Error::ProfileRequired)?;
    if tx.len() > 1026 {
        return Err(Error::Format("reward size"));
    }
    let raw = hex_bytes(tx, "reward").map_err(|_| Error::Format("deposit hex"))?;
    let f = fields(&raw)?;
    let solver = f[2].bytes.try_into().map_err(|_| Error::Format("payee"))?;
    if f[3].bytes.len() > 16 {
        return Err(Error::Overflow);
    }
    let amount = f[3]
        .bytes
        .iter()
        .fold(0u128, |n, b| (n << 8) | u128::from(*b));
    let payment = RewardPayment {
        reward_id,
        solver,
        amount,
    };
    let expected = reward_deposit(c, l1, &payment)?;
    if hex_bytes(&expected, "canonical reward").map_err(|_| Error::Format("deposit hex"))? != raw {
        return Err(Error::Format("noncanonical reward deposit"));
    }
    Ok(payment)
}
pub fn check_reward_deposit(
    c: &Config,
    l1: &L1Block,
    expected: &RewardPayment,
    tx: &str,
) -> Result<(), Error> {
    if decode_reward_deposit(c, l1, expected.reward_id, tx)? != *expected {
        return Err(Error::Format("reward differs from local fold"));
    }
    Ok(())
}
pub fn check_verdict_deposits(
    c: &Config,
    l1: &L1Block,
    expected: &[IndexedVerdict],
    txs: &[String],
) -> Result<(), Error> {
    if decode_verdict_deposits(c, l1, txs)? != expected {
        return Err(Error::Format("verdict differs from local fold"));
    }
    Ok(())
}

/// Require every system transaction in order; only user transactions may follow.
/// The M3b driver must use this instead of allowing system-prefix subsequences.
pub fn check_system_prefix(actual: &[String], expected: &[String]) -> Result<(), Error> {
    if actual.len() < expected.len() {
        return Err(Error::Format("missing system deposit"));
    }
    for (a, e) in actual.iter().zip(expected) {
        if hex_bytes(a, "system").map_err(|_| Error::Format("system hex"))?
            != hex_bytes(e, "system").map_err(|_| Error::Format("system hex"))?
        {
            return Err(Error::Format("system prefix differs"));
        }
    }
    for tx in &actual[expected.len()..] {
        if hex_bytes(tx, "user")
            .map_err(|_| Error::Format("transaction hex"))?
            .first()
            == Some(&0x7e)
        {
            return Err(Error::Format("extra system deposit"));
        }
    }
    Ok(())
}
