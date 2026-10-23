#![allow(dead_code)]
//! Independent M2 encoders used to drive the staged M3 public fold API.
use boole_derivation::{m2::*, L1Block};
use k256::ecdsa::SigningKey;
use sha2::{Digest, Sha256};

pub fn hash(b: &[u8]) -> Hash {
    Sha256::digest(b).into()
}
pub fn cfg() -> Config {
    let mut c = Config::devnet();
    c.slot_table = vec![SlotParams {
        lane: 1,
        solve_deadline: 48,
        announce_gap: 4,
        reveal_grace: 4,
    }];
    let key = key();
    let enc = key.verifying_key().to_encoded_point(false);
    c.pool_key
        .copy_from_slice(&boole_derivation::user_tx::keccak(&enc.as_bytes()[1..])[12..]);
    c
}
pub fn key() -> SigningKey {
    SigningKey::from_bytes(&hash(b"boole-devnet-m2-public-test-pool-v1").into()).unwrap()
}
pub fn block(h: u64) -> L1Block {
    L1Block {
        number: h,
        hash: format!("0x{}", hex::encode(hash(&h.to_be_bytes()))),
        parent_hash: format!(
            "0x{}",
            hex::encode(hash(&h.saturating_sub(1).to_be_bytes()))
        ),
        timestamp: h,
        mix_hash: format!(
            "0x{}",
            hex::encode(hash(&[h.to_be_bytes().as_slice(), b"randao"].concat()))
        ),
        base_fee_per_gas: "0x1".into(),
        parent_beacon_block_root: format!("0x{}", hex::encode([0; 32])),
    }
}
pub fn input(kind: u8, body: Vec<u8>, i: u64) -> Input {
    Input {
        transaction_index: i / 2,
        log_index: i,
        kind,
        body,
    }
}
pub fn run(s: &mut State, c: &Config, items: Vec<Input>) -> Vec<u8> {
    let h = s.height() + 1;
    let out = if c.m3.is_some() {
        s.step_with_verdicts(
            c,
            &block(h),
            &items,
            &[],
            &boole_derivation::m3::ForkTable::genesis(c).unwrap(),
        )
        .unwrap()
    } else {
        s.step(c, &block(h), &items).unwrap()
    };
    *s = out.state;
    out.codes
}
pub fn until(s: &mut State, c: &Config, h: u64) {
    while s.height() < h {
        run(s, c, vec![]);
    }
}
pub fn pool(c: &Config, root: Hash, count: u32, start: u64, seq: u32) -> Vec<u8> {
    pool_at(c, 0, root, count, start, seq)
}
pub fn pool_leaf(chain: u64, kind: u8, req: &[Hash], package: &[u8], salt: &Hash) -> Hash {
    boole_derivation::m2::pool_leaf(chain, kind, req, 0, package.len() as u32, package, salt)
}

#[allow(clippy::too_many_arguments)] // Independent encoder keeps wire field order.
pub fn problem_body(
    slot: u8,
    round: u32,
    index: u32,
    req: &[Hash],
    salt: Hash,
    path: &[Hash],
    package: &[u8],
) -> Vec<u8> {
    let mut b = vec![2, slot];
    b.extend(round.to_be_bytes());
    b.extend(index.to_be_bytes());
    b.extend([2, req.len() as u8]);
    for p in req {
        b.extend(p);
    }
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
pub fn opened(c: &Config) -> (State, Hash) {
    let mut s = State::new(c, 30_000_000).unwrap();
    let root = pool_leaf(c.chain_id, 2, &[], b"generator-v1", &[9; 32]);
    assert_eq!(
        run(&mut s, c, vec![input(8, pool(c, root, 1, 3, 1), 0)]),
        [0x85]
    );
    until(&mut s, c, 4);
    assert_eq!(
        run(
            &mut s,
            c,
            vec![input(
                9,
                problem_body(0, 1, 0, &[], [9; 32], &[], b"generator-v1"),
                0
            )]
        ),
        [0x86]
    );
    let p = *s.problems.keys().next().unwrap();
    (s, p)
}
pub fn body(
    c: &Config,
    p: Hash,
    solver: [u8; 20],
    salt: Hash,
    data: &[u8],
    pkgs: &[Hash],
) -> (Vec<u8>, Vec<Vec<u8>>, Vec<Hash>) {
    let chunks: Vec<Vec<u8>> = data.chunks(PART_SIZE).map(|x| x.to_vec()).collect();
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
    b.push(pkgs.len() as u8);
    for p in pkgs {
        b.extend(p);
    }
    b.extend([1, 1]);
    b.extend((data.len() as u64).to_be_bytes());
    b.extend(hash(data));
    b.extend((data.len() as u32).to_be_bytes());
    b.extend((chunks.len() as u16).to_be_bytes());
    b.extend(merkle_root(&leaves));
    b.extend([0, 0, 0, 0]);
    b.extend(40u16.to_be_bytes());
    b.extend([1, 0, 1, 0]);
    b.extend([0; 32]);
    b.extend(0u32.to_be_bytes());
    (b, chunks, leaves)
}
pub fn commit(p: Hash, ch: Hash) -> Vec<u8> {
    [vec![1], vec![3; 20], p.to_vec(), ch.to_vec()].concat()
}
pub fn open(b: &[u8], cancel: Hash) -> Vec<u8> {
    [vec![1], vec![4; 20], cancel.to_vec(), b.to_vec()].concat()
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
pub fn cancel(p: Hash, ch: Hash, b: &[u8], secret: Hash) -> Vec<u8> {
    [
        vec![1],
        p.to_vec(),
        ch.to_vec(),
        body_digest(b).to_vec(),
        secret.to_vec(),
    ]
    .concat()
}
pub fn candidate(
    c: &Config,
    s: &mut State,
    p: Hash,
    n: usize,
) -> (Hash, Vec<u8>, Vec<Vec<u8>>, Vec<Hash>) {
    let (b, chunks, leaves) = body(c, p, [6; 20], [7; 32], &vec![42; n], &[]);
    let ch = commit_hash(c.chain_id, p, cancel_hash(&[8; 32]), body_digest(&b));
    assert_eq!(run(s, c, vec![input(2, commit(p, ch), 0)]), [0x80]);
    (ch, b, chunks, leaves)
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
    let (sig, id) = key().sign_prehash_recoverable(&pre).unwrap();
    b.extend(sig.to_bytes());
    b.push(id.to_byte());
    b
}

pub fn m3_cfg() -> Config {
    let mut c = cfg();
    let mut p = boole_derivation::m3::M3Config::devnet(1);
    p.k = 2;
    p.maturity = 4;
    c.m3 = Some(p);
    c
}
pub fn checking_with_blocker(c: &Config) -> (State, Hash, Hash, Hash, Vec<u8>) {
    let (mut s, p) = opened(c);
    let (a, ab, _, _) = candidate(c, &mut s, p, 7);
    let (b, bb, parts, _) = candidate(c, &mut s, p, 8);
    let h = s.height();
    until(&mut s, c, h + c.d - 1);
    assert_eq!(
        run(
            &mut s,
            c,
            vec![input(3, open(&bb, cancel_hash(&[8; 32])), 0)]
        ),
        [0x81]
    );
    assert_eq!(
        run(&mut s, c, vec![input(4, part(b, 0, &[], &parts[0]), 0)]),
        [0x83]
    );
    (s, p, a, b, ab)
}
pub fn verdict(
    index: u64,
    ch: Hash,
    outcome: boole_derivation::m3::Outcome,
    reason: u16,
    fuel: u64,
) -> boole_derivation::m3::IndexedVerdict {
    use boole_derivation::m3::*;
    IndexedVerdict {
        queue_index: index,
        record: VerdictRecord {
            kind: RecordKind::Verdict,
            commit_hash: ch,
            outcome,
            reason,
            gate_epoch: 0,
            fuel,
            vstmt_digest: hash(b"injected statement"),
        },
    }
}
pub fn settled(c: &Config) -> (State, Hash, Hash) {
    use boole_derivation::m3::*;
    let (mut s, p, a, b, ab) = checking_with_blocker(c);
    run(&mut s, c, vec![]);
    let out = s
        .step_with_verdicts(
            c,
            &block(s.height() + 1),
            &[input(7, cancel(p, a, &ab, [8; 32]), 0)],
            &[verdict(0, b, Outcome::Accept, 0, 0)],
            &ForkTable::genesis(c).unwrap(),
        )
        .unwrap();
    (out.state, p, b)
}
pub fn parent(h: u64) -> boole_derivation::ParentHeader {
    boole_derivation::ParentHeader {
        number: h,
        hash: format!("0x{}", hex::encode(hash(b"parent L2"))),
        parent_hash: None,
        timestamp: h,
        gas_limit: 30_000_000,
        miner: format!("0x{}", hex::encode([0; 20])),
        extra_data: format!("0x01{}", "00".repeat(16)),
        l1_info: Some(boole_derivation::system::l1_info(&block(h)).unwrap()),
        m2_root_deposit: None,
    }
}
pub fn user_logs(header: &L1Block, gas_limits: &[u64]) -> Vec<boole_derivation::InboxLog> {
    use boole_derivation::rlp;
    let rows: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../fixtures/devnet-m1/s1b-recorded-main.json"
    ))
    .unwrap();
    let raw = hex::decode(&rows[0]["decisions"][0]["raw"].as_str().unwrap()[2..]).unwrap();
    let decoded = rlp::decode(&raw[1..]).unwrap();
    let fields: Vec<_> = decoded
        .children
        .unwrap()
        .iter()
        .map(|f| raw[1 + f.span.start..1 + f.span.end].to_vec())
        .collect();
    let template: boole_derivation::InboxLog =
        serde_json::from_value(rows[0]["logs"][0].clone()).unwrap();
    gas_limits
        .iter()
        .enumerate()
        .map(|(i, gas)| {
            let mut f = fields.clone();
            f[4] = rlp::uint(*gas);
            let item = [vec![1, 2], rlp::list(&f)].concat();
            let mut data = vec![0; 64];
            data[31] = 32;
            data[56..64].copy_from_slice(&(item.len() as u64).to_be_bytes());
            data.extend(item);
            let mut log = template.clone();
            log.block_hash = header.hash.clone();
            log.transaction_index = i as u64;
            log.log_index = i as u64;
            log.data = format!("0x{}", hex::encode(data));
            log
        })
        .collect()
}
/// A retained queue fixture isolates the R3 transition matrix. Separate tests
/// qualify real activation and full replay through the public fold.
pub fn recheck_fixture(
    s: &mut State,
    key: Key,
    epoch: u32,
) -> boole_derivation::m3::IndexedVerdict {
    use boole_derivation::m3::*;
    let avail = s.height();
    let m = s.m3.as_mut().unwrap();
    let index = m.queue.len() as u64;
    m.queue.push(Job {
        queue_index: index,
        key,
        kind: RecordKind::Recheck,
        avail,
        epoch_override: Some(epoch),
    });
    m.pending_rechecks.insert(key);
    let mut v = verdict(index, key.1, Outcome::Reject, 0x0301, 0);
    v.record.kind = RecordKind::Recheck;
    v.record.gate_epoch = epoch;
    v
}
pub fn kernel_fault_and_winner(c: &Config) -> (State, Hash, Hash, Hash) {
    use boole_derivation::m3::*;
    let (mut s, p, a, b, ab) = checking_with_blocker(c);
    run(
        &mut s,
        c,
        vec![input(3, open(&ab, cancel_hash(&[8; 32])), 0)],
    );
    let out = s
        .step_with_verdicts(
            c,
            &block(s.height() + 1),
            &[input(4, part(a, 0, &[], &[42; 7]), 0)],
            &[verdict(0, b, Outcome::Accept, 0, 0)],
            &ForkTable::genesis(c).unwrap(),
        )
        .unwrap();
    s = out.state;
    run(&mut s, c, vec![]);
    let out = s
        .step_with_verdicts(
            c,
            &block(s.height() + 1),
            &[],
            &[verdict(1, a, Outcome::Reject, 0x0306, 0)],
            &ForkTable::genesis(c).unwrap(),
        )
        .unwrap();
    (out.state, p, a, b)
}
