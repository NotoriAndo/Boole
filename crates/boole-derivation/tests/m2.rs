//! M2-R1–R28: independently encoded inbox items and transition oracles.
use boole_derivation::{m2::*, L1Block};
use k256::ecdsa::SigningKey;
use sha2::{Digest, Sha256};

fn hash(b: &[u8]) -> Hash {
    Sha256::digest(b).into()
}
fn cfg() -> Config {
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
fn key() -> SigningKey {
    SigningKey::from_bytes(&hash(b"boole-devnet-m2-public-test-pool-v1").into()).unwrap()
}
fn block(h: u64) -> L1Block {
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
fn input(kind: u8, body: Vec<u8>, i: u64) -> Input {
    Input {
        transaction_index: i / 2,
        log_index: i,
        kind,
        body,
    }
}
fn run(s: &mut State, c: &Config, items: Vec<Input>) -> Vec<u8> {
    let h = s.height() + 1;
    let out = s.step(c, &block(h), &items).unwrap();
    *s = out.state;
    out.codes
}
fn until(s: &mut State, c: &Config, h: u64) {
    while s.height() < h {
        run(s, c, vec![]);
    }
}
fn pool(c: &Config, root: Hash, count: u32, start: u64, seq: u32) -> Vec<u8> {
    pool_at(c, 0, root, count, start, seq)
}
fn pool_leaf(chain: u64, kind: u8, req: &[Hash], package: &[u8], salt: &Hash) -> Hash {
    boole_derivation::m2::pool_leaf(chain, kind, req, 0, package.len() as u32, package, salt)
}

fn problem_body(
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
fn opened(c: &Config) -> (State, Hash) {
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
fn body(
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
fn commit(p: Hash, ch: Hash) -> Vec<u8> {
    [vec![1], vec![3; 20], p.to_vec(), ch.to_vec()].concat()
}
fn open(b: &[u8], cancel: Hash) -> Vec<u8> {
    [vec![1], vec![4; 20], cancel.to_vec(), b.to_vec()].concat()
}
fn part(ch: Hash, i: u16, path: &[Hash], data: &[u8]) -> Vec<u8> {
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
fn cancel(p: Hash, ch: Hash, b: &[u8], secret: Hash) -> Vec<u8> {
    [
        vec![1],
        p.to_vec(),
        ch.to_vec(),
        body_digest(b).to_vec(),
        secret.to_vec(),
    ]
    .concat()
}
fn candidate(
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

#[test]
fn t7_r1_r2_r3_r4_r12_r20_r24_r26_layouts() {
    let c = cfg();
    let (mut s, p) = opened(&c);
    let (ch, b, parts, leaves) = candidate(&c, &mut s, p, 1);
    for (kind, good) in [
        (2, commit(p, ch)),
        (3, open(&b, cancel_hash(&[8; 32]))),
        (4, part(ch, 0, &[], &parts[0])),
        (7, cancel(p, ch, &b, [8; 32])),
        (8, pool(&c, [1; 32], 1, 99, 2)),
        (9, problem_body(0, 1, 0, &[], [9; 32], &[], b"generator-v1")),
    ] {
        for delta in [-1, 1] {
            let mut bad = good.clone();
            if delta < 0 {
                bad.pop();
            } else {
                bad.push(0);
            }
            let o = s
                .step(&c, &block(s.height() + 1), &[input(kind, bad, 0)])
                .unwrap();
            if kind != 4 {
                assert_eq!(o.codes, [1], "kind {kind}");
            }
        }
        let mut bad = good;
        bad[0] = if [8, 9].contains(&kind) { 1 } else { 2 };
        assert_eq!(
            s.step(&c, &block(s.height() + 1), &[input(kind, bad, 0)])
                .unwrap()
                .codes,
            [1]
        );
    }
    for k in [5, 6, 10, 11, 12, 13, 14, 15] {
        assert_eq!(
            s.step(&c, &block(s.height() + 1), &[input(k, vec![], 0)])
                .unwrap()
                .codes,
            [0x3f]
        );
    }
    assert_eq!(leaves.len(), 1);
}
#[test]
fn t8_r3_binding_all_bytes_domains_network() {
    assert_eq!(
        cancel_hash(&[8; 32]),
        hash(&[b"BOOLE-M2/CANCEL/v1".as_slice(), &[8; 32]].concat())
    );
    assert_eq!(
        body_digest(b"vector"),
        hash(&[b"BOOLE-M2/BODY/v1".as_slice(), b"vector"].concat())
    );
    assert_eq!(
        commit_hash(720926, [1; 32], [2; 32], [3; 32]),
        hash(
            &[
                b"BOOLE-M2/COMMIT/v2".as_slice(),
                &720926u64.to_be_bytes(),
                &[1; 32],
                &[2; 32],
                &[3; 32]
            ]
            .concat()
        )
    );

    let c = cfg();
    let p = [4; 32];
    let (b, _, _) = body(&c, p, [6; 20], [7; 32], b"data", &[]);
    let ca = cancel_hash(&[8; 32]);
    let ch = commit_hash(c.chain_id, p, ca, body_digest(&b));
    for i in 0..b.len() {
        let mut x = b.clone();
        x[i] ^= 1;
        assert_ne!(ch, commit_hash(c.chain_id, p, ca, body_digest(&x)));
    }
    assert_ne!(ch, commit_hash(c.chain_id + 1, p, ca, body_digest(&b)));
    assert_ne!(ch, commit_hash(c.chain_id, [0; 32], ca, body_digest(&b)));
    assert_ne!(ch, commit_hash(c.chain_id, p, [0; 32], body_digest(&b)));
    assert_ne!(body_digest(&b), hash(&b));
}
#[test]
fn t9_r8_r10_windows_checking_never_expires() {
    let c = cfg();
    let (mut s, p) = opened(&c);
    let (ch, b, parts, _) = candidate(&c, &mut s, p, 1);
    let hc = s.height();
    until(&mut s, &c, hc + c.d - 2);
    assert_eq!(
        run(
            &mut s,
            &c,
            vec![input(3, open(&b, cancel_hash(&[8; 32])), 0)]
        ),
        [9]
    );
    assert_eq!(
        run(
            &mut s,
            &c,
            vec![input(3, open(&b, cancel_hash(&[8; 32])), 0)]
        ),
        [0x81]
    );
    until(&mut s, &c, hc + c.w - 1);
    assert_eq!(
        run(&mut s, &c, vec![input(4, part(ch, 0, &[], &parts[0]), 0)]),
        [0x83]
    );
    until(&mut s, &c, hc + 10 * c.w);
    assert_eq!(s.candidates[&(p, ch)].status(), Status::Checking);
    let (mut s, p) = opened(&c);
    let (ch, _, _, _) = candidate(&c, &mut s, p, 1);
    let end = s.height() + c.w;
    until(&mut s, &c, end);
    assert_eq!(
        run(&mut s, &c, vec![input(4, part(ch, 0, &[], b"*"), 0)]),
        [7]
    );
    assert_eq!(s.candidates[&(p, ch)].status(), Status::Expired);
}
#[test]
fn t10_r9_r19_r21_parts_copy_garbage_order_and_posters() {
    let c = cfg();
    for n in [1, 2, 3, 5, 9, 17, 33, 65, 128] {
        let (mut s, p) = opened(&c);
        let (ch, b, parts, leaves) = candidate(&c, &mut s, p, (n - 1) * PART_SIZE + 7);
        assert_eq!(
            run(
                &mut s,
                &c,
                vec![input(
                    4,
                    part(ch, 0, &merkle_path(&leaves, 0), &parts[0]),
                    0
                )]
            ),
            [7]
        );
        let hc = s.height() - 1;
        until(&mut s, &c, hc + c.d - 1);
        assert_eq!(
            run(
                &mut s,
                &c,
                vec![input(3, open(&b, cancel_hash(&[8; 32])), 0)]
            ),
            [0x81]
        );
        let mut all = vec![input(
            4,
            part(ch, 0, &merkle_path(&leaves, 0), b"garbage"),
            0,
        )];
        for i in (0..n).rev() {
            all.push(input(
                4,
                part(ch, i as u16, &merkle_path(&leaves, i), &parts[i]),
                all.len() as u64,
            ));
        }
        let got = run(&mut s, &c, all);
        assert_eq!(got[0], 8);
        assert_eq!(got.last(), Some(&0x83));
        assert_eq!(s.candidates[&(p, ch)].live().unwrap().poster, [3; 20]);
        assert_eq!(
            s.candidates[&(p, ch)]
                .live()
                .unwrap()
                .open
                .as_ref()
                .unwrap()
                .poster,
            [4; 20]
        );
        let stolen: Vec<_> = parts
            .iter()
            .enumerate()
            .map(|(i, d)| part_leaf(&[1; 20], &[7; 32], i as u16, d))
            .collect();
        assert_ne!(merkle_root(&leaves), merkle_root(&stolen));
    }
}
#[test]
fn t11_r7_r11_admission_only_commits_and_drops_free() {
    let mut c = cfg();
    c.c_commit = 2;
    let (mut s, p) = opened(&c);
    let (ch, b, parts, _) = candidate(&c, &mut s, p, 1);
    let hc = s.height();
    let a = hash(b"a");
    let z = hash(b"z");
    assert_eq!(
        run(
            &mut s,
            &c,
            vec![
                input(2, vec![], 0),
                input(2, commit([0; 32], a), 1),
                input(2, commit(p, a), 2),
                input(2, commit(p, a), 3),
                input(2, commit(p, z), 4),
                input(2, commit(p, hash(b"third")), 5)
            ]
        ),
        [1, 4, 0x80, 10, 0x80, 11]
    );
    until(&mut s, &c, hc + c.d - 1);
    let mut items = vec![];
    for i in 0..(10 * c.c_commit) {
        items.push(input(3, vec![], i));
        items.push(input(4, vec![], 100 + i));
    }
    items.push(input(3, open(&b, cancel_hash(&[8; 32])), 999));
    items.push(input(4, part(ch, 0, &[], &parts[0]), 1000));
    let codes = run(&mut s, &c, items);
    assert!(codes.contains(&0x81));
    assert!(codes.contains(&0x83));
}
#[test]
fn t12_r5_r6_r14_r16_r28_order_refold_and_root_golden() {
    let c = cfg();
    let mut s = State::new(&c, 30_000_000).unwrap();
    let b = block(1);
    let items = vec![input(5, vec![1], 4), input(2, vec![], 2)];
    let a = s.step(&c, &b, &items).unwrap();
    let mut rev = items;
    rev.reverse();
    let z = s.step(&c, &b, &rev).unwrap();
    assert_eq!(a.events, z.events);
    assert_eq!(a.state.root, z.state.root);
    let mut e = vec![];
    for (tx, log, kind, code) in [(1u64, 2u64, 2u8, 1u8), (2, 4, 5, 0x3f)] {
        e.push(1);
        e.extend(tx.to_be_bytes());
        e.extend(log.to_be_bytes());
        e.extend([kind, code]);
    }
    assert_eq!(a.events, e);
    assert_eq!(
        a.state.root,
        hash(
            &[
                b"BOOLE-M2/ROOT/v2".as_slice(),
                &c.digest(),
                &1u64.to_be_bytes(),
                &hex::decode(&b.hash[2..]).unwrap(),
                &hash(&e)
            ]
            .concat()
        )
    );
    s = a.state;
    assert_ne!(s.root, [0; 32]);
    let (mut s, p) = opened(&c);
    let hs = [hash(b"c"), hash(b"b"), hash(b"a")];
    run(
        &mut s,
        &c,
        hs.iter()
            .enumerate()
            .map(|(i, h)| input(2, commit(p, *h), i as u64))
            .collect(),
    );
    let hc = s.height();
    run(&mut s, &c, vec![]);
    let mix: Hash = hex::decode(&block(hc + 1).mix_hash[2..])
        .unwrap()
        .try_into()
        .unwrap();
    let mut expected = hs.to_vec();
    expected.sort_by_key(|ch| hash(&[b"BOOLE-M2/TIE/v1".as_slice(), ch, &mix].concat()));
    assert_eq!(s.priority(p), expected);
}
#[test]
fn model_barrier_expiry_sequences() {
    let c = cfg();
    let (mut s, p) = opened(&c);
    let mut reference = std::collections::BTreeMap::new();
    for i in 0u64..200 {
        let ch = hash(&i.to_be_bytes());
        let hc = s.height() + 1;
        run(&mut s, &c, vec![input(2, commit(p, ch), 0)]);
        if hc <= 53 {
            reference.insert(ch, hc + c.w + 1);
        }
        reference.retain(|_, expiry| *expiry > s.height());
        let actual = s
            .priority(p)
            .into_iter()
            .filter(|h| s.candidates[&(p, *h)].status() == Status::Committed)
            .count();
        assert_eq!(actual, reference.len());
    }
}
#[test]
fn t14_r13_r14_mutation_50000_no_panic() {
    let c = cfg();
    let (s, p) = opened(&c);
    let (b, _, _) = body(&c, p, [6; 20], [7; 32], b"data", &[]);
    let seeds = [
        commit(p, [1; 32]),
        open(&b, cancel_hash(&[8; 32])),
        part([1; 32], 0, &[], b"data"),
        cancel(p, [1; 32], &b, [8; 32]),
        pool(&c, [1; 32], 1, 99, 2),
        problem_body(0, 1, 0, &[], [9; 32], &[], b"generator-v1"),
    ];
    let kinds = [2, 3, 4, 7, 8, 9];
    let mut rng = 0x8675_309a_1234_5678u64;
    for i in 0..50_000 {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        let kind = 2 + (i % 14) as u8;
        let mut raw = kinds
            .iter()
            .position(|k| *k == kind)
            .map_or_else(|| vec![1], |k| seeds[k].clone());
        let at = (rng as usize) % raw.len();
        raw[at] ^= (rng >> 32) as u8 | 1;
        if i % 3 == 0 {
            raw.truncate(at);
        }
        let o = s
            .step(&c, &block(s.height() + 1), &[input(kind, raw, 0)])
            .unwrap();
        assert_eq!(o.codes.len(), 1);
        let valid: &[u8] = match kind {
            2 => &[1, 4, 5, 9, 10, 11, 0x80],
            3 => &[1, 2, 3, 6, 9, 10, 0x81],
            4 => &[1, 7, 8, 9, 10, 0x82, 0x83],
            7 => &[1, 6, 9, 10, 12, 0x84],
            8 => &[1, 10, 13, 0x85],
            9 => &[1, 2, 9, 10, 14, 0x86],
            _ => &[0x3f],
        };
        assert!(
            valid.contains(&o.codes[0]),
            "kind {kind}, code {}",
            o.codes[0]
        );
    }
}
#[test]
fn t17_r20_cancel_releases_barrier_and_tombstone() {
    let c = cfg();
    let (mut s, p) = opened(&c);
    let (ch, b, _, _) = candidate(&c, &mut s, p, 1);
    assert_eq!(
        run(&mut s, &c, vec![input(7, cancel(p, ch, &b, [0; 32]), 0)]),
        [12]
    );
    assert_eq!(
        run(
            &mut s,
            &c,
            vec![
                input(7, cancel(p, ch, &b, [8; 32]), 0),
                input(2, commit(p, ch), 1)
            ]
        ),
        [0x84, 10]
    );
    assert!(s.priority(p).is_empty());
    assert!(matches!(
        s.candidates[&(p, ch)],
        Candidate::Tombstone(Status::Cancelled)
    ));
    let close = s.problems[&p].h_v + c.slot_table[0].solve_deadline + 1;
    until(&mut s, &c, close + c.w + 1);
    assert!(!s.candidates.contains_key(&(p, ch)));
    assert_eq!(run(&mut s, &c, vec![input(2, commit(p, ch), 0)]), [5]);
}
#[test]
fn t18_r24_r25_pool_signature_draw_no_replacement() {
    let c = cfg();
    let mut s = State::new(&c, 30_000_000).unwrap();
    let leaves = [
        pool_leaf(c.chain_id, 2, &[], b"a", &[1; 32]),
        pool_leaf(c.chain_id, 2, &[], b"b", &[2; 32]),
    ];
    let root = merkle_root(&leaves);
    let mut bad = pool(&c, root, 2, 3, 1);
    bad[100] ^= 1;
    assert_eq!(run(&mut s, &c, vec![input(8, bad, 0)]), [13]);
    assert_eq!(
        run(&mut s, &c, vec![input(8, pool(&c, root, 2, 4, 1), 0)]),
        [0x85]
    );
    until(&mut s, &c, 5);
    let a = &s.rounds[&(0, 1)];
    let seed = hash(
        &[
            b"BOOLE-M2/DRAW/v2".as_slice(),
            &c.chain_id.to_be_bytes(),
            &[0],
            &1u32.to_be_bytes(),
            &hex::decode(&block(5).mix_hash[2..]).unwrap(),
        ]
        .concat(),
    );
    assert_eq!(a.instance_seed, seed);
    assert_eq!(
        a.leaf_index,
        (u64::from_be_bytes(seed[..8].try_into().unwrap()) % 2) as u32
    );
    assert_eq!(
        run(&mut s, &c, vec![input(8, pool(&c, root, 2, 99, 2), 0)]),
        [10]
    );
    until(&mut s, &c, 50);
    assert_eq!(s.rounds.len(), 2);
    assert_ne!(s.rounds[&(0, 1)].leaf_index, s.rounds[&(0, 2)].leaf_index);
}
#[test]
fn t19_r26_r27_problem_reveal_deadline_withheld_repose() {
    let c = cfg();
    let (mut s, p) = opened(&c);
    let hv = s.height();
    assert!(matches!(
        s.step(&c, &block(hv), &[]),
        Err(FoldError::Height { .. })
    ));
    assert_eq!(
        run(&mut s, &c, vec![input(2, commit(p, [1; 32]), 0)]),
        [0x80]
    );
    assert_eq!(
        run(
            &mut s,
            &c,
            vec![input(
                9,
                problem_body(0, 1, 0, &[], [9; 32], &[], b"bad"),
                0
            )]
        ),
        [10]
    );
    until(&mut s, &c, hv + c.slot_table[0].solve_deadline);
    assert_eq!(run(&mut s, &c, vec![input(2, commit(p, [2; 32]), 0)]), [5]);
    assert_eq!(
        s.slots[0].scheduled,
        Some(s.height() + c.slot_table[0].announce_gap)
    );
    assert!(s.enqueue_repose(p));
    until(
        &mut s,
        &c,
        hv + c.slot_table[0].solve_deadline + 1 + c.slot_table[0].announce_gap + 1,
    );
    let new = s.problems.values().find(|x| x.epoch == 1).unwrap();
    assert_ne!(new.id, p);
    let mut s = State::new(&c, 30_000_000).unwrap();
    let root = pool_leaf(c.chain_id, 2, &[], b"a", &[1; 32]);
    run(&mut s, &c, vec![input(8, pool(&c, root, 1, 3, 1), 0)]);
    until(&mut s, &c, 5 + c.slot_table[0].reveal_grace);
    assert_eq!(s.rounds[&(0, 1)].status, RoundStatus::Withheld);
    assert_eq!(
        run(
            &mut s,
            &c,
            vec![input(9, problem_body(0, 1, 0, &[], [1; 32], &[], b"a"), 0)]
        ),
        [9]
    );
}
#[test]
fn t20_r17_r18_r22_r23_environment_policy_order() {
    let c = cfg();
    let (s, p) = opened(&c);
    for pkgs in [vec![], vec![c.pkg_table[0]], vec![c.pkg_table[1]]] {
        let (b, _, _) = body(&c, p, [6; 20], [7; 32], b"data", &pkgs);
        assert_eq!(
            s.step(
                &c,
                &block(s.height() + 1),
                &[input(3, open(&b, [0; 32]), 0)]
            )
            .unwrap()
            .codes,
            [6]
        );
    }
    for pkgs in [
        vec![c.pkg_table[0], c.pkg_table[0]],
        vec![[99; 32]],
        c.pkg_table.iter().copied().rev().collect(),
        vec![c.pkg_table[0], c.pkg_table[1]],
        vec![[0; 32]; 5],
    ] {
        let (b, _, _) = body(&c, p, [6; 20], [7; 32], b"data", &pkgs);
        assert_eq!(
            s.step(
                &c,
                &block(s.height() + 1),
                &[input(3, open(&b, [0; 32]), 0)]
            )
            .unwrap()
            .codes,
            [2]
        );
    }
    let (b, _, _) = body(&c, p, [6; 20], [7; 32], b"data", &[]);
    for at in [85, 86, 118, 151, 152, 231, 233, 239, 240, 273] {
        let mut x = b.clone();
        x[at] ^= 3;
        if at == 152 {
            x[at] = 3;
        }
        let code = s
            .step(
                &c,
                &block(s.height() + 1),
                &[input(3, open(&x, [0; 32]), 0)],
            )
            .unwrap()
            .codes[0];
        assert!([1, 2].contains(&code), "offset {at}, {code}");
    }
    assert_ne!(
        env_id(c.schema_id, c.base_root, &[]),
        env_id(c.schema_id, c.base_root, &[c.pkg_table[0]])
    );
}
#[test]
fn t21_r23_size_caps() {
    let c = cfg();
    let (s, p) = opened(&c);
    let (mut b, _, _) = body(&c, p, [6; 20], [7; 32], b"data", &[]);
    let q = 151;
    for (z, n, raw, expected) in [
        (Z_CAP as u32, 128, RAW_CAP, 6),
        (Z_CAP as u32 + 1, 129, RAW_CAP, 3),
        (1, 1, RAW_CAP + 1, 3),
        (1, 1, 17, 3),
        (1, 1, 16, 6),
        (0, 0, 1, 2),
    ] {
        b[q + 2..q + 10].copy_from_slice(&raw.to_be_bytes());
        b[q + 42..q + 46].copy_from_slice(&z.to_be_bytes());
        b[q + 46..q + 48].copy_from_slice(&(n as u16).to_be_bytes());
        assert_eq!(
            s.step(
                &c,
                &block(s.height() + 1),
                &[input(3, open(&b, [0; 32]), 0)]
            )
            .unwrap()
            .codes,
            [expected]
        );
    }
}
#[test]
fn t22_s1_s6_profiles_startup() {
    for c in [
        Config::devnet(),
        Config::lean_testnet(11),
        Config::production(12),
    ] {
        assert!(c.validate(30_000_000).is_ok());
        assert!(State::new(&c, 30_000_000).is_ok());
    }
    for which in 1..=6 {
        let mut c = cfg();
        match which {
            1 => c.d = 1,
            2 => c.w = 1,
            3 => c.c_commit = c.q_max * c.v_per_deposit + 1,
            4 => c.verdict_gas = 30_000_000,
            5 => c.pool_limit = 1,
            6 => c.slot_table = vec![c.slot_table[0].clone(); 4],
            _ => unreachable!(),
        }
        assert_eq!(c.validate(30_000_000), Err(which));
    }
    let mut c = cfg();
    c.env_allowed.insert(vec![c.pkg_table[0], c.pkg_table[1]]);
    assert_eq!(c.validate(30_000_000), Err(6));
}

#[test]
fn t11_r13_m1_attributes_and_evidence_37_heights_unchanged() {
    use boole_derivation::{derive, derive_with_m2, InboxLog, ParentHeader};
    let main: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m1/s1b-recorded-main-budget-v1.json"
    ))
    .unwrap();
    let base: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m1/s1b-recorded-basefee.json"
    ))
    .unwrap();
    let c = cfg();
    let mut count = 0;
    let mut accepted = std::collections::BTreeSet::new();
    for rows in [main["heights"].as_array().unwrap().clone(), base] {
        let mut state = State::new(&c, 30_000_000).unwrap();
        for row in rows {
            let l1: L1Block = serde_json::from_value(row["l1"].clone()).unwrap();
            let parent: ParentHeader = serde_json::from_value(row["parent"].clone()).unwrap();
            let logs: Vec<InboxLog> = serde_json::from_value(row["logs"].clone()).unwrap();
            let old = derive(&l1, &logs, &parent).unwrap();
            assert!(serde_json::to_value(&old.decisions)
                .unwrap()
                .as_array()
                .unwrap()
                .iter()
                .all(|v| v.get("m2").is_none()));
            let p = state.problems.keys().next().copied().unwrap_or([4; 32]);
            let (b, parts, _) = body(&c, p, [6; 20], [7; 32], b"data", &[]);
            let secret = [8; 32];
            let ch = commit_hash(c.chain_id, p, cancel_hash(&secret), body_digest(&b));
            let root = pool_leaf(c.chain_id, 2, &[], b"generator-v1", &[9; 32]);
            let other = commit_hash(c.chain_id, [99; 32], cancel_hash(&secret), body_digest(&b));
            let items = [
                (2, commit(p, ch)),
                (3, open(&b, cancel_hash(&secret))),
                (4, part(ch, 0, &[], &parts[0])),
                (7, cancel([99; 32], other, &b, secret)),
                (8, pool(&c, root, 1, l1.number + 2, 1)),
                (9, problem_body(0, 1, 0, &[], [9; 32], &[], b"generator-v1")),
            ];
            let mut mixed = logs.clone();
            for (kind, body) in items {
                let item = [vec![kind], body].concat();
                let mut data = vec![0; 64];
                data[31] = 32;
                data[56..64].copy_from_slice(&(item.len() as u64).to_be_bytes());
                data.extend(item);
                mixed.push(InboxLog {
                    address: boole_derivation::config::INBOX.into(),
                    topics: vec![boole_derivation::config::posted_topic()],
                    block_hash: l1.hash.clone(),
                    removed: false,
                    transaction_index: 100 + u64::from(kind),
                    log_index: 100 + u64::from(kind),
                    data: format!("0x{}", hex::encode(data)),
                });
            }
            let (new, next) = derive_with_m2(&state, &c, &l1, &mixed, &parent).unwrap();
            state = next;
            assert_eq!(
                new.canonical_bytes().unwrap(),
                old.canonical_bytes().unwrap()
            );
            assert_eq!(&new.decisions[..logs.len()], &old.decisions);
            for d in &new.decisions[logs.len()..] {
                assert!(!d.forward);
                assert!(d.m2.is_some());
                accepted.insert(d.m2.as_ref().unwrap().code);
            }
            count += 1;
        }
    }
    assert_eq!(count, 37);
    assert!([0x80, 0x81, 0x83, 0x85, 0x86]
        .iter()
        .all(|code| accepted.contains(code)));
}
#[test]
fn t7_r23_all_rows_and_k0_k4_layout() {
    let mut c = cfg();
    c.incompatible.clear();
    for i in 0..2 {
        c.pkg_table.push(hash(&[i]));
    }
    c.pkg_table.sort();
    let p = [4; 32];
    for k in 0..=4 {
        let pkgs = c.pkg_table[..k].to_vec();
        c.env_allowed.insert(pkgs.clone());
        let mut s = State::new(&c, 30_000_000).unwrap();
        until(&mut s, &c, 5);
        let (b, _, _) = body(&c, p, [6; 20], [7; 32], b"data", &pkgs);
        assert_eq!(b.len(), 277 + 32 * k);
        let good = open(&b, [0; 32]);
        assert_eq!(good.len(), 330 + 32 * k);
        assert_eq!(
            s.step(&c, &block(6), &[input(3, good.clone(), 0)])
                .unwrap()
                .codes,
            [6]
        );
        for delta in [-1, 1] {
            let mut x = good.clone();
            if delta < 0 {
                x.pop();
            } else {
                x.push(0);
            }
            assert_eq!(s.step(&c, &block(6), &[input(3, x, 0)]).unwrap().codes, [1]);
        }
    }
    let mut s = State::new(&c, 30_000_000).unwrap();
    until(&mut s, &c, 5);
    let (b, _, _) = body(&c, p, [6; 20], [7; 32], b"data", &[]);
    for (offset, value, code) in [
        (0, 2, 1),
        (85, 2, 2),
        (86, 0, 2),
        (118, 0, 2),
        (151, 2, 2),
        (152, 3, 2),
        (237, 2, 1),
        (239, 0, 2),
        (240, 1, 2),
    ] {
        let mut x = b.clone();
        x[offset] = value;
        assert_eq!(
            s.step(&c, &block(6), &[input(3, open(&x, [0; 32]), 0)])
                .unwrap()
                .codes,
            [code],
            "row offset {offset}"
        );
    }
}
#[test]
fn t18_pool_rejects_high_s_seq_lead_chain_and_reserved_parity() {
    let c = cfg();
    for mode in 0..5 {
        let s = State::new(&c, 30_000_000).unwrap();
        let mut b = pool(&c, [1; 32], 1, 3, 1);
        match mode {
            0 => b = pool(&c, [1; 32], 1, 3, 2),
            1 => b = pool(&c, [1; 32], 1, 2, 1),
            2 => {
                let mut other = c.clone();
                other.chain_id += 1;
                b = pool(&other, [1; 32], 1, 3, 1);
            }
            3 => b[146] = 2,
            4 => {
                let order =
                    hex::decode("fffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141")
                        .unwrap();
                let mut borrow = 0i16;
                for i in (0..32).rev() {
                    let n = i16::from(order[i]) - i16::from(b[114 + i]) - borrow;
                    b[114 + i] = n.rem_euclid(256) as u8;
                    borrow = i16::from(n < 0);
                }
                b[146] ^= 1;
            }
            _ => unreachable!(),
        }
        assert_eq!(
            s.step(&c, &block(1), &[input(8, b, 0)]).unwrap().codes,
            [13]
        );
    }
}
#[test]
fn t19_problem_reveal_invalid_path_round_size_and_required_package() {
    let c = cfg();
    let mut s = State::new(&c, 30_000_000).unwrap();
    let pkg = c.pkg_table[0];
    let root = pool_leaf(c.chain_id, 2, &[pkg], b"a", &[1; 32]);
    run(&mut s, &c, vec![input(8, pool(&c, root, 1, 3, 1), 0)]);
    assert_eq!(
        run(
            &mut s,
            &c,
            vec![input(
                9,
                problem_body(0, 1, 0, &[pkg], [1; 32], &[], b"a"),
                0
            )]
        ),
        [14]
    );
    until(&mut s, &c, 4);
    for (round, package, expected) in [
        (2, b"a".to_vec(), 14),
        (1, b"b".to_vec(), 14),
        (1, vec![1; c.statement_max + 1], 2),
    ] {
        assert_eq!(
            s.step(
                &c,
                &block(5),
                &[input(
                    9,
                    problem_body(0, round, 0, &[pkg], [1; 32], &[], &package),
                    0
                )]
            )
            .unwrap()
            .codes,
            [expected]
        );
    }
    assert_eq!(
        run(
            &mut s,
            &c,
            vec![input(
                9,
                problem_body(0, 1, 0, &[pkg], [1; 32], &[], b"a"),
                0
            )]
        ),
        [0x86]
    );
    let p = *s.problems.keys().next().unwrap();
    let (ch, b, _, _) = candidate(&c, &mut s, p, 1);
    let hc = s.height();
    until(&mut s, &c, hc + c.d - 1);
    assert_eq!(
        run(
            &mut s,
            &c,
            vec![input(3, open(&b, cancel_hash(&[8; 32])), 0)]
        ),
        [2]
    );
    assert_eq!(s.candidates[&(p, ch)].status(), Status::Committed);
}
#[test]
fn t18_future_pool_never_postpones_active_pool_draw() {
    let c = cfg();
    let mut s = State::new(&c, 30_000_000).unwrap();
    let root = pool_leaf(c.chain_id, 2, &[], b"a", &[1; 32]);
    run(&mut s, &c, vec![input(8, pool(&c, root, 1, 3, 1), 0)]);
    assert_eq!(
        run(&mut s, &c, vec![input(8, pool(&c, [2; 32], 1, 100, 2), 0)]),
        [0x85]
    );
    until(&mut s, &c, 4);
    assert_eq!(s.rounds[&(0, 1)].pool_seq, 1);
}
#[test]
fn t19_required_package_list_has_no_open_composition_cap() {
    let mut c = cfg();
    for i in 0..3 {
        c.pkg_table.push(hash(&[90 + i]));
    }
    c.pkg_table.sort();
    let req = c.pkg_table.clone();
    let root = pool_leaf(c.chain_id, 2, &req, b"a", &[1; 32]);
    let mut s = State::new(&c, 30_000_000).unwrap();
    run(&mut s, &c, vec![input(8, pool(&c, root, 1, 3, 1), 0)]);
    until(&mut s, &c, 4);
    assert_eq!(
        run(
            &mut s,
            &c,
            vec![input(9, problem_body(0, 1, 0, &req, [1; 32], &[], b"a"), 0)]
        ),
        [0x86]
    );
}
#[test]
fn t21_reported_size_boundary_fixture() {
    let c = cfg();
    let (s, p) = opened(&c);
    let f: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m2/size-boundaries-v1.json"
    ))
    .unwrap();
    for row in f["rows"].as_array().unwrap() {
        let z = row["z_len"].as_u64().unwrap() as u32;
        let n = row["n"].as_u64().unwrap() as u16;
        assert_eq!(usize::from(n), (z as usize).div_ceil(PART_SIZE));
        let (mut b, _, _) = body(&c, p, [6; 20], [7; 32], b"data", &[]);
        b[153..161].copy_from_slice(&1u64.to_be_bytes());
        b[193..197].copy_from_slice(&z.to_be_bytes());
        b[197..199].copy_from_slice(&n.to_be_bytes());
        assert_eq!(
            s.step(&c, &block(6), &[input(3, open(&b, [0; 32]), 0)])
                .unwrap()
                .codes,
            [if row["within_z_cap"] == true { 6 } else { 3 }]
        );
    }
}
/// Reference adapter for the specified service subset: reveal_window=W+1,
/// D=2, CHECKING+PASS=VALID-REVEAL; injected verdicts stay outside M2 state.
#[test]
fn model_shared_subset_with_injected_verdicts() {
    use std::collections::BTreeMap;
    let mut c = cfg();
    c.d = 2;
    let (mut s, p) = opened(&c);
    let mut proof = BTreeMap::new();
    let mut commits = vec![];
    for i in 0u8..32 {
        let (b, parts, _) = body(&c, p, [6; 20], hash(&[i]), &[i], &[]);
        let ch = commit_hash(c.chain_id, p, cancel_hash(&[8; 32]), body_digest(&b));
        proof.insert(ch, (b, parts[0].clone()));
        commits.push(input(2, commit(p, ch), u64::from(i)));
    }
    run(&mut s, &c, commits);
    let hc = s.height();
    run(&mut s, &c, vec![]);
    let mix = hex::decode(&block(hc + 1).mix_hash[2..]).unwrap();
    let mut order: Vec<_> = proof.keys().copied().collect();
    order.sort_by_key(|ch| hash(&[b"BOOLE-M2/TIE/v1".as_slice(), ch, &mix].concat()));
    let mut reference: BTreeMap<Hash, (bool, Option<bool>)> =
        proof.keys().map(|ch| (*ch, (false, None))).collect();
    let mut verdict = BTreeMap::new();
    for h in hc + 2..hc + c.w + 12 {
        let mut items = vec![];
        for (i, ch) in order.iter().enumerate() {
            if h == hc + 2 + (i as u64 % 5) && i % 3 != 0 {
                let (b, data) = &proof[ch];
                items.push(input(3, open(b, cancel_hash(&[8; 32])), items.len() as u64));
                items.push(input(4, part(*ch, 0, &[], data), items.len() as u64));
                reference.get_mut(ch).unwrap().0 = true;
            }
            if h == hc + 10 + i as u64 && reference[ch].0 {
                let pass = i % 4 == 1;
                reference.get_mut(ch).unwrap().1 = Some(pass);
                verdict.insert(*ch, pass);
            }
        }
        run(&mut s, &c, items);
        let mut expected = None;
        for ch in &order {
            let (revealed, result) = reference[ch];
            if let Some(pass) = result {
                if pass {
                    expected = Some(*ch);
                    break;
                }
            } else if revealed || h < hc + c.w + 1 {
                break;
            }
        }
        let mut actual = None;
        for ch in s.priority(p) {
            if let Some(pass) = verdict.get(&ch) {
                if *pass {
                    actual = Some(ch);
                    break;
                }
            } else {
                break;
            }
        }
        assert_eq!(actual, expected, "height {h}");
    }
}

#[test]
fn review_prune_same_hash_other_problem_keeps_live_parts_k2() {
    let mut c = cfg();
    c.slot_table = vec![c.slot_table[0].clone(); 2];
    let leaves: Vec<_> = (0u8..4)
        .map(|i| pool_leaf(c.chain_id, 2, &[], &[b'G', i], &[i; 32]))
        .collect();
    let mut leaves1 = leaves.clone();
    leaves1.push(pool_leaf(c.chain_id, 2, &[], &[b'G', 4], &[4; 32]));
    let mut s = State::new(&c, 30_000_000).unwrap();
    run(
        &mut s,
        &c,
        vec![
            input(8, pool(&c, merkle_root(&leaves), 4, 3, 1), 0),
            input(8, pool_at(&c, 1, merkle_root(&leaves1), 5, 3, 2), 1),
        ],
    );
    until(&mut s, &c, 4);
    let i = s.rounds[&(0, 1)].leaf_index as usize;
    assert_eq!(
        run(
            &mut s,
            &c,
            vec![input(
                9,
                problem_body(
                    0,
                    1,
                    i as u32,
                    &[],
                    [i as u8; 32],
                    &merkle_path(&leaves, i),
                    &[b'G', i as u8]
                ),
                0
            )]
        ),
        [0x86]
    );
    let old = s.rounds[&(0, 1)].problem.unwrap();
    until(&mut s, &c, 14);
    let i = s.rounds[&(1, 2)].leaf_index as usize;
    assert_eq!(
        run(
            &mut s,
            &c,
            vec![input(
                9,
                problem_body(
                    1,
                    2,
                    i as u32,
                    &[],
                    [i as u8; 32],
                    &merkle_path(&leaves1, i),
                    &[b'G', i as u8]
                ),
                0
            )]
        ),
        [0x86]
    );
    let p = s.rounds[&(1, 2)].problem.unwrap();
    let (b, parts, part_leaves) = body(&c, p, [6; 20], [7; 32], &vec![42; PART_SIZE + 1], &[]);
    let ch = commit_hash(c.chain_id, p, cancel_hash(&[8; 32]), body_digest(&b));
    assert_eq!(run(&mut s, &c, vec![input(2, commit(old, ch), 0)]), [0x80]);
    until(&mut s, &c, 59);
    assert_eq!(run(&mut s, &c, vec![input(2, commit(p, ch), 0)]), [0x80]);
    let hc = s.height();
    until(&mut s, &c, hc + c.d - 1);
    assert_eq!(
        run(
            &mut s,
            &c,
            vec![
                input(3, open(&b, cancel_hash(&[8; 32])), 0),
                input(4, part(ch, 0, &merkle_path(&part_leaves, 0), &parts[0]), 1)
            ]
        ),
        [0x81, 0x82]
    );
    let prune = s.problems[&old].closed.unwrap() + c.w + 1;
    until(&mut s, &c, prune - 1);
    assert!(s.candidates.contains_key(&(old, ch)));
    run(&mut s, &c, vec![]);
    assert!(!s.candidates.contains_key(&(old, ch)));
    until(&mut s, &c, hc + c.w - 1);
    assert_eq!(
        run(
            &mut s,
            &c,
            vec![input(
                4,
                part(ch, 1, &merkle_path(&part_leaves, 1), &parts[1]),
                0
            )]
        ),
        [0x83]
    );
}
#[test]
fn review_prune_same_hash_keeps_checking_index_k1() {
    let c = cfg();
    let (mut s, p) = opened(&c);
    let (ch, b, parts, _) = candidate(&c, &mut s, p, 1);
    let hc = s.height();
    until(&mut s, &c, hc + c.d - 1);
    assert_eq!(
        run(
            &mut s,
            &c,
            vec![
                input(3, open(&b, cancel_hash(&[8; 32])), 0),
                input(4, part(ch, 0, &[], &parts[0]), 1)
            ]
        ),
        [0x81, 0x83]
    );
    let next_root = pool_leaf(c.chain_id, 2, &[], b"next generator", &[11; 32]);
    assert_eq!(
        run(&mut s, &c, vec![input(8, pool(&c, next_root, 1, 18, 2), 0)]),
        [0x85]
    );
    until(&mut s, &c, 59);
    assert_eq!(
        run(
            &mut s,
            &c,
            vec![input(
                9,
                problem_body(0, 2, 0, &[], [11; 32], &[], b"next generator"),
                0
            )]
        ),
        [0x86]
    );
    let other = s.rounds[&(0, 2)].problem.unwrap();
    assert_eq!(
        run(&mut s, &c, vec![input(2, commit(other, ch), 0)]),
        [0x80]
    );
    let prune = s.problems[&other].h_v + c.slot_table[0].solve_deadline + 1 + c.w + 1;
    until(&mut s, &c, prune - 1);
    assert!(s.candidates.contains_key(&(other, ch)));
    run(&mut s, &c, vec![]);
    assert!(!s.candidates.contains_key(&(other, ch)));
    assert_eq!(
        run(&mut s, &c, vec![input(4, part(ch, 0, &[], &parts[0]), 0)]),
        [10]
    );
}

#[test]
fn review_height_continuity_and_overflow_are_typed_errors() {
    let c = cfg();
    let s = State::new(&c, 30_000_000).unwrap();
    assert!(matches!(
        s.step(&c, &block(0), &[]),
        Err(FoldError::Height { .. })
    ));
    assert!(matches!(
        s.step(&c, &block(2), &[]),
        Err(FoldError::Height { .. })
    ));
    let next = s.step(&c, &block(1), &[]).unwrap().state;
    assert!(matches!(
        next.step(&c, &block(1), &[]),
        Err(FoldError::Height { .. })
    ));
    assert!(matches!(
        next.step(&c, &block(3), &[]),
        Err(FoldError::Height { .. })
    ));
    assert_eq!(next.height(), 1);
    let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m1/s1b-recorded-main.json"
    ))
    .unwrap();
    let row = &rows[1];
    let l1 = serde_json::from_value(row["l1"].clone()).unwrap();
    let parent = serde_json::from_value(row["parent"].clone()).unwrap();
    assert!(matches!(
        boole_derivation::derive_with_m2(&s, &c, &l1, &[], &parent),
        Err(boole_derivation::InputError::M2(FoldError::Height { .. }))
    ));
}
#[test]
fn review_every_startup_violation_cannot_construct_or_fold() {
    let good = cfg();
    let state = State::new(&good, 30_000_000).unwrap();
    for which in 1..=6 {
        let mut c = good.clone();
        let mut l2 = 30_000_000;
        match which {
            1 => c.d = 1,
            2 => c.w = 1,
            3 => c.c_commit = c.q_max * c.v_per_deposit + 1,
            4 => l2 = 1,
            5 => c.pool_limit = 1,
            6 => c.slot_table.clear(),
            _ => unreachable!(),
        };
        assert!(matches!(State::new(&c,l2),Err(FoldError::Startup(n)) if n==which));
        if which != 4 {
            assert!(matches!(state.step(&c,&block(1),&[]),Err(FoldError::Startup(n)) if n==which));
        }
    }
    let mut changed = good.clone();
    changed.chain_id += 1;
    assert!(matches!(
        state.step(&changed, &block(1), &[]),
        Err(FoldError::ConfigurationChanged)
    ));
    let mut from_json: Config =
        serde_json::from_str(&serde_json::to_string(&good).unwrap()).unwrap();
    from_json.slot_table.clear();
    assert!(State::new(&from_json, 30_000_000).is_err());
}
#[test]
fn review_s4_uses_explicit_l2_gas_independently_of_l1_limit() {
    let mut c = cfg();
    c.l1_gas_limit = 60_000_000;
    let needed = 1_000_000 + c.q_max * c.verdict_gas + 16_777_216;
    assert_eq!(c.validate(needed), Ok(()));
    assert_eq!(c.validate(needed - 1), Err(4));
    c.l1_gas_limit = 1;
    assert_eq!(c.validate(needed), Ok(()));
}

#[test]
fn review_open_part_cancel_status_checks() {
    let c = cfg();
    let (mut s, p) = opened(&c);
    let (ch, b, parts, leaves) = candidate(&c, &mut s, p, PART_SIZE + 1);
    let hc = s.height();
    let committed = s.clone();
    until(&mut s, &c, hc + c.d - 1);
    assert_eq!(
        run(
            &mut s,
            &c,
            vec![input(3, open(&b, cancel_hash(&[8; 32])), 0)]
        ),
        [0x81]
    );
    let opened = s.clone();
    assert_eq!(
        run(
            &mut s,
            &c,
            vec![input(3, open(&b, cancel_hash(&[8; 32])), 0)]
        ),
        [10]
    );
    assert_eq!(
        run(
            &mut s,
            &c,
            vec![input(
                4,
                part(ch, 0, &merkle_path(&leaves, 0), &parts[0]),
                0
            )]
        ),
        [0x82]
    );
    assert_eq!(
        run(
            &mut s,
            &c,
            vec![input(
                4,
                part(ch, 0, &merkle_path(&leaves, 0), &parts[0]),
                0
            )]
        ),
        [10]
    );
    assert_eq!(
        run(
            &mut s,
            &c,
            vec![input(
                4,
                part(ch, 1, &merkle_path(&leaves, 1), &parts[1]),
                0
            )]
        ),
        [0x83]
    );
    assert_eq!(
        run(
            &mut s,
            &c,
            vec![
                input(3, open(&b, cancel_hash(&[8; 32])), 0),
                input(4, part(ch, 1, &merkle_path(&leaves, 1), &parts[1]), 1),
                input(7, cancel(p, ch, &b, [8; 32]), 2)
            ]
        ),
        [10, 10, 9]
    );
    let mut expired = opened;
    until(&mut expired, &c, hc + c.w);
    assert_eq!(expired.candidates[&(p, ch)].status(), Status::Open);
    assert_eq!(
        run(
            &mut expired,
            &c,
            vec![
                input(3, open(&b, cancel_hash(&[8; 32])), 0),
                input(4, part(ch, 0, &merkle_path(&leaves, 0), &parts[0]), 1),
                input(7, cancel(p, ch, &b, [8; 32]), 2)
            ]
        ),
        [9, 9, 9]
    );
    let mut cancelled = committed;
    assert_eq!(
        run(
            &mut cancelled,
            &c,
            vec![input(7, cancel(p, ch, &b, [8; 32]), 0)]
        ),
        [0x84]
    );
    assert_eq!(
        run(
            &mut cancelled,
            &c,
            vec![
                input(7, cancel(p, ch, &b, [8; 32]), 0),
                input(3, open(&b, cancel_hash(&[8; 32])), 1)
            ]
        ),
        [10, 9]
    );
    let missing = commit_hash(c.chain_id, p, cancel_hash(&[99; 32]), body_digest(&b));
    assert_eq!(
        run(
            &mut cancelled,
            &c,
            vec![input(7, cancel(p, missing, &b, [99; 32]), 0)]
        ),
        [6]
    );
}
#[test]
fn review_r23_reserved_lengths_and_policy_before_size() {
    let c = cfg();
    let (s, p) = opened(&c);
    let (b, _, _) = body(&c, p, [6; 20], [7; 32], b"data", &[]);
    for (offset, bytes) in [(231, 64), (233, 1)] {
        let mut x = b.clone();
        x[offset..offset + 2].copy_from_slice(&1u16.to_be_bytes());
        x.splice(237..237, vec![0; bytes]);
        assert_eq!(
            s.step(&c, &block(6), &[input(3, open(&x, [0; 32]), 0)])
                .unwrap()
                .codes,
            [2]
        );
    }
    let mut x = b.clone();
    x[153..161].copy_from_slice(&(RAW_CAP + 1).to_be_bytes());
    assert_eq!(
        s.step(&c, &block(6), &[input(3, open(&x, [0; 32]), 0)])
            .unwrap()
            .codes,
        [3]
    );
    x[151] = 2;
    assert_eq!(
        s.step(&c, &block(6), &[input(3, open(&x, [0; 32]), 0)])
            .unwrap()
            .codes,
        [2]
    );
    x[85] = 2;
    assert_eq!(
        s.step(&c, &block(6), &[input(3, open(&x, [0; 32]), 0)])
            .unwrap()
            .codes,
        [2]
    );
}
#[test]
fn review_commit_deadline_and_problem_grace_boundaries() {
    let c = cfg();
    let (mut s, p) = opened(&c);
    let hv = s.height();
    until(&mut s, &c, hv + c.slot_table[0].solve_deadline - 1);
    assert_eq!(
        run(&mut s, &c, vec![input(2, commit(p, [1; 32]), 0)]),
        [0x80]
    );
    assert_eq!(run(&mut s, &c, vec![input(2, commit(p, [2; 32]), 0)]), [5]);
    let root = pool_leaf(c.chain_id, 2, &[], b"a", &[1; 32]);
    let mut drawn = State::new(&c, 30_000_000).unwrap();
    run(&mut drawn, &c, vec![input(8, pool(&c, root, 1, 3, 1), 0)]);
    until(&mut drawn, &c, 4);
    let mut same = drawn.clone();
    let p = problem_id(c.chain_id, root, 0, same.rounds[&(0, 1)].instance_seed, 0);
    assert_eq!(
        run(
            &mut same,
            &c,
            vec![
                input(9, problem_body(0, 1, 0, &[], [1; 32], &[], b"a"), 0),
                input(2, commit(p, [1; 32]), 1)
            ]
        ),
        [0x86, 9]
    );
    let mut last = drawn.clone();
    until(&mut last, &c, 7);
    assert_eq!(
        run(
            &mut last,
            &c,
            vec![input(9, problem_body(0, 1, 0, &[], [1; 32], &[], b"a"), 0)]
        ),
        [0x86]
    );
    until(&mut drawn, &c, 8);
    assert_eq!(drawn.rounds[&(0, 1)].status, RoundStatus::Drawn);
    assert_eq!(
        run(
            &mut drawn,
            &c,
            vec![input(9, problem_body(0, 1, 0, &[], [1; 32], &[], b"a"), 0)]
        ),
        [9]
    );
    assert_eq!(drawn.rounds[&(0, 1)].status, RoundStatus::Withheld);
}
#[test]
fn t13_real_service_vectors() {
    use std::collections::BTreeMap;
    let vectors: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m2/service-parity-v1.json"
    ))
    .unwrap();
    assert_eq!(
        vectors["provenance"]["source_sha256"],
        hex::encode(hash(include_bytes!(
            "../../../scripts/zk_registry/service.py"
        )))
    );
    for case in vectors["cases"].as_array().unwrap() {
        let mut c = cfg();
        c.d = vectors["mapping"]["d"].as_u64().unwrap();
        assert!(c.d >= 2);
        c.w = vectors["mapping"]["w"].as_u64().unwrap();
        let (mut s, p) = opened(&c);
        let mut proofs = BTreeMap::new();
        let mut sequence = BTreeMap::new();
        let mut verdict = BTreeMap::new();
        let mut winner = None;
        for row in case["rows"].as_array().unwrap() {
            let h = row["height"].as_u64().unwrap();
            if h <= s.height() {
                continue;
            }
            let mut items = vec![];
            let mut matched = vec![];
            for action in row["actions"].as_array().unwrap() {
                let seq = action["sequence"].as_u64().unwrap();
                let position = items.len() as u64;
                if action["op"] == "commit" {
                    let (b, parts, _) =
                        body(&c, p, [6; 20], hash(&seq.to_be_bytes()), &[seq as u8], &[]);
                    let ch = commit_hash(c.chain_id, p, cancel_hash(&[8; 32]), body_digest(&b));
                    items.push(input(2, commit(p, ch), position));
                    sequence.insert(ch, seq);
                    proofs.insert(seq, (ch, b, parts[0].clone()));
                } else {
                    let (ch, b, data) = &proofs[&seq];
                    let mut opening = b.clone();
                    if action["correct_binding"] == false {
                        opening[53] ^= 1;
                    }
                    items.push(input(3, open(&opening, cancel_hash(&[8; 32])), position));
                    if action["matched"] == true {
                        items.push(input(4, part(*ch, 0, &[], data), position + 1));
                        matched.push((seq, action["verdict"] == "PASS"));
                    }
                }
            }
            let codes = run(&mut s, &c, items);
            assert!(
                codes.iter().all(|c| [6, 9, 0x80, 0x81, 0x83].contains(c)),
                "{codes:?}"
            );
            for (seq, pass) in matched {
                verdict.insert(seq, pass);
            }
            if winner.is_none() {
                for ch in s.priority(p) {
                    let seq = sequence[&ch];
                    match verdict.get(&seq) {
                        Some(false) => {}
                        Some(true) => {
                            winner = Some(seq);
                            break;
                        }
                        None => break,
                    }
                }
            }
            let statuses: BTreeMap<String, &str> = proofs
                .iter()
                .map(|(seq, (ch, _, _))| {
                    let status = match s.candidates[&(p, *ch)].status() {
                        Status::Expired => "EXPIRED",
                        Status::Committed | Status::Open => "COMMITTED",
                        Status::Checking => match verdict.get(seq) {
                            Some(false) => "REJECTED",
                            Some(true) => match winner {
                                Some(w) if w == *seq => "ACCEPTED",
                                Some(_) => "DUPLICATE",
                                None => "VALID-REVEAL",
                            },
                            None => panic!("missing injected verdict"),
                        },
                        Status::Cancelled => panic!("CANCEL excluded"),
                    };
                    (seq.to_string(), status)
                })
                .collect();
            assert_eq!(
                serde_json::to_value(statuses).unwrap(),
                row["expected"]["statuses"],
                "{} height {h}",
                case["name"]
            );
            assert_eq!(
                serde_json::to_value(winner).unwrap(),
                row["expected"]["winner"],
                "{} height {h}",
                case["name"]
            );
        }
    }
}

#[test]
fn review_expiry_cross_problem_order() {
    let mut c = cfg();
    c.slot_table = vec![c.slot_table[0].clone(); 2];
    let mut s = State::new(&c, 30_000_000).unwrap();
    let leaves = [
        pool_leaf(c.chain_id, 2, &[], b"a", &[1; 32]),
        pool_leaf(c.chain_id, 2, &[], b"b", &[2; 32]),
    ];
    let leaves1 = [
        pool_leaf(c.chain_id, 2, &[], b"a", &[3; 32]),
        pool_leaf(c.chain_id, 2, &[], b"b", &[4; 32]),
    ];
    run(
        &mut s,
        &c,
        vec![
            input(8, pool(&c, merkle_root(&leaves), 2, 3, 1), 0),
            input(8, pool_at(&c, 1, merkle_root(&leaves1), 2, 3, 2), 1),
        ],
    );
    until(&mut s, &c, 4);
    let mut reveal = vec![];
    for slot in 0..2 {
        let i = s.rounds[&(slot, 1)].leaf_index as usize;
        reveal.push(input(
            9,
            problem_body(
                slot,
                1,
                i as u32,
                &[],
                [(i + 1 + usize::from(slot) * 2) as u8; 32],
                &merkle_path(if slot == 0 { &leaves } else { &leaves1 }, i),
                if i == 0 { b"a" } else { b"b" },
            ),
            slot as u64,
        ));
    }
    run(&mut s, &c, reveal);
    let p: Vec<_> = s.problems.keys().copied().collect();
    let mix = hex::decode(&block(7).mix_hash[2..]).unwrap();
    let ch0 = hash(b"cross problem expiry");
    let t0 = hash(&[b"BOOLE-M2/TIE/v1".as_slice(), &ch0, &mix].concat());
    let ch1 = (0u64..1000)
        .map(|i| hash(&i.to_be_bytes()))
        .find(|ch| hash(&[b"BOOLE-M2/TIE/v1".as_slice(), ch, &mix].concat()) < t0)
        .unwrap();
    run(
        &mut s,
        &c,
        vec![
            input(2, commit(p[0], ch0), 0),
            input(2, commit(p[1], ch1), 1),
        ],
    );
    until(&mut s, &c, 70);
    let next = s.step(&c, &block(71), &[]).unwrap();
    let mut expected = vec![];
    for (p, ch) in [(p[0], ch0), (p[1], ch1)] {
        expected.push(2);
        expected.extend(p);
        expected.extend(ch);
        expected.push(4);
    }
    assert_eq!(next.events, expected);
}
#[test]
fn review_fixed_event_hex_and_next_header_tie_order() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m2/event-golden-v1.1.json"
    ))
    .unwrap();
    let c = cfg();
    let mut drawn = State::new(&c, 30_000_000).unwrap();
    let root = pool_leaf(c.chain_id, 2, &[], b"generator-v1", &[9; 32]);
    run(&mut drawn, &c, vec![input(8, pool(&c, root, 1, 3, 1), 0)]);
    until(&mut drawn, &c, 3);
    let draw = drawn.step(&c, &block(4), &[]).unwrap();
    assert_eq!(hex::encode(&draw.events), fixture["events"]["draw"]);
    drawn = draw.state;
    until(&mut drawn, &c, 8);
    let withheld = drawn.step(&c, &block(9), &[]).unwrap();
    assert_eq!(hex::encode(withheld.events), fixture["events"]["withheld"]);
    let (mut s, p) = opened(&c);
    let (ch, _, _, _) = candidate(&c, &mut s, p, 1);
    assert_eq!(hex::encode(p), fixture["problem_id"]);
    assert_eq!(hex::encode(ch), fixture["commit_hash"]);
    until(&mut s, &c, 53);
    let next = s.step(&c, &block(54), &[]).unwrap();
    assert_eq!(hex::encode(next.events), fixture["events"]["deadline"]);
    s = next.state;
    until(&mut s, &c, 70);
    let next = s.step(&c, &block(71), &[]).unwrap();
    assert_eq!(hex::encode(next.events), fixture["events"]["expiry"]);
    s = next.state;
    until(&mut s, &c, 118);
    assert!(s.candidates.contains_key(&(p, ch)));
    let next = s.step(&c, &block(119), &[]).unwrap();
    assert_eq!(hex::encode(next.events), fixture["events"]["prune"]);
    assert!(!next.state.candidates.contains_key(&(p, ch)));
    let (mut s, p) = opened(&c);
    let names = ["a", "b", "c"];
    run(
        &mut s,
        &c,
        names
            .iter()
            .enumerate()
            .map(|(i, x)| input(2, commit(p, hash(x.as_bytes())), i as u64))
            .collect(),
    );
    let mut next_header = block(7);
    next_header.mix_hash = format!("0x{}", fixture["tie"]["next_mix"].as_str().unwrap());
    s = s.step(&c, &next_header, &[]).unwrap().state;
    let actual: Vec<_> = s
        .priority(p)
        .into_iter()
        .map(|ch| *names.iter().find(|x| hash(x.as_bytes()) == ch).unwrap())
        .collect();
    assert_eq!(serde_json::json!(actual), fixture["tie"]["expected_order"]);
    assert_ne!(
        fixture["tie"]["expected_order"],
        fixture["tie"]["commit_mix_order"]
    );
}
#[test]
fn review_incremental_mixed_fold_equals_full_refold() {
    let c = cfg();
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m2/event-golden-v1.1.json"
    ))
    .unwrap();
    let p: Hash = hex::decode(fixture["problem_id"].as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let make = |salt| {
        let (b, parts, _) = body(&c, p, [6; 20], [salt; 32], b"*", &[]);
        let ch = commit_hash(c.chain_id, p, cancel_hash(&[8; 32]), body_digest(&b));
        (b, parts, ch)
    };
    let a = make(7);
    let b = make(8);
    let e = make(9);
    let root = pool_leaf(c.chain_id, 2, &[], b"generator-v1", &[9; 32]);
    let mut state = State::new(&c, 30_000_000).unwrap();
    let mut history = vec![];
    for h in 1..=125 {
        let mut items = match h {
            1 => vec![input(8, pool(&c, root, 1, 3, 1), 0)],
            5 => vec![input(
                9,
                problem_body(0, 1, 0, &[], [9; 32], &[], b"generator-v1"),
                0,
            )],
            6 => vec![
                input(2, commit(p, a.2), 0),
                input(2, commit(p, b.2), 1),
                input(2, commit(p, e.2), 2),
            ],
            7 => vec![input(7, cancel(p, b.2, &b.0, [8; 32]), 0)],
            14 => vec![input(3, open(&a.0, cancel_hash(&[8; 32])), 0)],
            15 => vec![
                input(3, open(&a.0, cancel_hash(&[8; 32])), 0),
                input(4, part(a.2, 0, &[], &a.1[0]), 1),
            ],
            16 => vec![
                input(5, vec![1], 0),
                input(4, part(a.2, 0, &[], &a.1[0]), 1),
            ],
            _ => vec![],
        };
        if h % 2 == 0 {
            items.reverse();
        }
        let l1 = block(h);
        let next = state.step(&c, &l1, &items).unwrap();
        history.push((
            l1,
            items,
            next.events.clone(),
            next.codes.clone(),
            next.state.root,
        ));
        state = next.state;
    }
    let mut refold = State::new(&c, 30_000_000).unwrap();
    for (l1, mut items, events, codes, root) in history {
        items.reverse();
        let next = refold.step(&c, &l1, &items).unwrap();
        assert_eq!(next.events, events);
        assert_eq!(next.codes, codes);
        assert_eq!(next.state.root, root);
        refold = next.state;
    }
    assert_eq!(format!("{state:?}"), format!("{refold:?}"));
}
#[test]
fn review_s4_stateful_parent_gas_is_checked() {
    let c = cfg();
    let state = State::new(&c, 30_000_000).unwrap();
    let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m1/s1b-recorded-main.json"
    ))
    .unwrap();
    let l1 = serde_json::from_value(rows[0]["l1"].clone()).unwrap();
    let mut parent: boole_derivation::ParentHeader =
        serde_json::from_value(rows[0]["parent"].clone()).unwrap();
    parent.gas_limit = 25_000_000;
    assert!(matches!(
        boole_derivation::derive_with_m2(&state, &c, &l1, &[], &parent),
        Err(boole_derivation::InputError::M2(FoldError::Startup(4)))
    ));
}

#[test]
fn v11_t24_legacy_config_is_refused() {
    let mut value = serde_json::to_value(Config::devnet()).unwrap();
    value["k_open"] = serde_json::json!(1);
    assert!(serde_json::from_value::<Config>(value).is_err());
}

fn pool_at(c: &Config, slot: u8, root: Hash, count: u32, start: u64, seq: u32) -> Vec<u8> {
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
// Keep the independent wire encoder arguments in field order.
#[allow(clippy::too_many_arguments)]
fn pkg_body(
    slot: u8,
    round: u32,
    index: u32,
    codec: u8,
    raw: u32,
    salt: Hash,
    path: &[Hash],
    pkg: &[u8],
) -> Vec<u8> {
    let mut b = vec![2, slot];
    b.extend(round.to_be_bytes());
    b.extend(index.to_be_bytes());
    b.extend([2, 0]);
    b.extend(salt);
    b.push(codec);
    b.extend(raw.to_be_bytes());
    b.push(path.len() as u8);
    for h in path {
        b.extend(h);
    }
    b.extend((pkg.len() as u32).to_be_bytes());
    b.extend(pkg);
    b
}
#[test]
fn v11_t24_slot_table_startup_and_profiles() {
    let c = cfg();
    for n in [0, 4] {
        let mut bad = c.clone();
        bad.slot_table = vec![c.slot_table[0].clone(); n];
        assert_eq!(bad.validate(30_000_000), Err(6));
    }
    for lane in [0, 3] {
        let mut bad = c.clone();
        bad.slot_table[0].lane = lane;
        assert_eq!(bad.validate(30_000_000), Err(6));
    }
    for field in 0..3 {
        for value in [0, (1u64 << 32) + 1] {
            let mut bad = c.clone();
            let row = &mut bad.slot_table[0];
            match field {
                0 => row.solve_deadline = value,
                1 => row.announce_gap = value,
                _ => row.reveal_grace = value,
            };
            assert_eq!(bad.validate(30_000_000), Err(7));
        }
    }
    let mut bad = c.clone();
    bad.pool_lead = 0;
    assert_eq!(bad.validate(30_000_000), Err(7));
    for raw in [c.statement_max - 1, (1usize << 27) + 1] {
        let mut bad = c.clone();
        bad.statement_raw_max = raw;
        assert_eq!(bad.validate(30_000_000), Err(8));
    }
    for leaves in [c.pool_max - 1, (1 << 20) + 1] {
        let mut bad = c.clone();
        bad.slot_leaves_max = leaves;
        assert_eq!(bad.validate(30_000_000), Err(8));
    }
    let mut bad = c.clone();
    bad.pkg_table = (0..400u32).map(|n| hash(&n.to_be_bytes())).collect();
    bad.pkg_table.sort();
    bad.env_allowed = [vec![]].into();
    assert_eq!(bad.validate(30_000_000), Err(5));
    for profile in [
        Config::devnet(),
        Config::lean_testnet(99),
        Config::production(99),
    ] {
        assert_eq!(profile.validate(60_000_000), Ok(()));
    }
    let mut edge = c.clone();
    edge.statement_raw_max = 1 << 27;
    assert_eq!(edge.validate(30_000_000), Ok(()));
    for value in [1, 1u64 << 32] {
        let mut edge = c.clone();
        let row = &mut edge.slot_table[0];
        row.solve_deadline = value;
        row.announce_gap = value;
        row.reveal_grace = value;
        assert_eq!(edge.validate(30_000_000), Ok(()));
    }
    let t = Config::lean_testnet(99);
    assert_eq!(
        t.slot_table
            .iter()
            .map(|s| (s.lane, s.solve_deadline, s.announce_gap, s.reveal_grace))
            .collect::<Vec<_>>(),
        vec![(2, 1800, 300, 25), (1, 7200, 3600, 25), (2, 1800, 300, 25)]
    );
}
#[test]
fn v11_t25_pool_versions_slot_signature_union_cap_and_global_sequence() {
    let mut c = cfg();
    c.slot_table.push(c.slot_table[0].clone());
    c.pool_max = 2;
    c.slot_leaves_max = 2;
    let s = State::new(&c, 30_000_000).unwrap();
    let mut future = State::new(&c, 30_000_000).unwrap();
    assert_eq!(
        run(
            &mut future,
            &c,
            vec![input(8, pool_at(&c, 0, [7; 32], 2, 100, 1), 0)]
        ),
        [0x85]
    );
    assert_eq!(
        run(
            &mut future,
            &c,
            vec![input(8, pool_at(&c, 0, [8; 32], 1, 4, 2), 0)]
        ),
        [13]
    );
    let good = pool_at(&c, 0, [1; 32], 1, 3, 1);
    let mut old = good.clone();
    old[0] = 1;
    assert_eq!(
        s.step(&c, &block(1), &[input(8, old, 0)]).unwrap().codes,
        [1]
    );
    let mut v1 = good.clone();
    v1.remove(1);
    v1[0] = 1;
    assert_eq!(
        s.step(&c, &block(1), &[input(8, v1, 0)]).unwrap().codes,
        [1]
    );
    let mut flipped = good.clone();
    flipped[1] = 1;
    assert_eq!(
        s.step(&c, &block(1), &[input(8, flipped, 0)])
            .unwrap()
            .codes,
        [13]
    );
    let mut wrong_domain = good[..82].to_vec();
    let pre = hash(
        &[
            b"BOOLE-M2/POOL/v1".as_slice(),
            &c.chain_id.to_be_bytes(),
            &wrong_domain,
        ]
        .concat(),
    );
    let (sig, id) = key().sign_prehash_recoverable(&pre).unwrap();
    wrong_domain.extend(sig.to_bytes());
    wrong_domain.push(id.to_byte());
    assert_eq!(
        s.step(&c, &block(1), &[input(8, wrong_domain, 0)])
            .unwrap()
            .codes,
        [13]
    );
    assert_eq!(
        s.step(
            &c,
            &block(1),
            &[input(8, pool_at(&c, 2, [1; 32], 1, 3, 1), 0)]
        )
        .unwrap()
        .codes,
        [13]
    );
    let mut s = s;
    assert_eq!(run(&mut s, &c, vec![input(8, good, 0)]), [0x85]);
    assert!(s.slots[1].scheduled.is_none());
    assert_eq!(
        run(
            &mut s,
            &c,
            vec![input(8, pool_at(&c, 1, [2; 32], 1, 4, 2), 0)]
        ),
        [0x85]
    );
    assert_eq!(
        run(
            &mut s,
            &c,
            vec![input(8, pool_at(&c, 0, [3; 32], 2, 5, 3), 0)]
        ),
        [13]
    );
    assert_eq!(
        run(
            &mut s,
            &c,
            vec![input(8, pool_at(&c, 1, [1; 32], 1, 6, 3), 0)]
        ),
        [10]
    );
}
#[test]
fn v11_t26_union_mapping_seed_and_no_replacement() {
    let mut c = cfg();
    c.slot_table[0].reveal_grace = 1;
    c.slot_table[0].announce_gap = 1;
    c.slot_table.push(c.slot_table[0].clone());
    let mut s = State::new(&c, 30_000_000).unwrap();
    assert_eq!(
        run(
            &mut s,
            &c,
            vec![
                input(8, pool_at(&c, 0, [1; 32], 6, 3, 1), 0),
                input(8, pool_at(&c, 1, [2; 32], 2, 3, 2), 1),
                input(8, pool_at(&c, 0, [3; 32], 2, 3, 3), 2),
                input(8, pool_at(&c, 0, [4; 32], 1, 100, 4), 3)
            ]
        ),
        [0x85; 4]
    );
    s.pools.get_mut(&1).unwrap().unused = [0, 2, 5].into();
    s.pools.get_mut(&3).unwrap().unused = [1].into();
    until(&mut s, &c, 3);
    let golden: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m2/event-golden-v1.1.json"
    ))
    .unwrap();
    for row in golden["union_draws"].as_array().unwrap() {
        let mut b = block(4);
        b.mix_hash = format!("0x{}", row["mix"].as_str().unwrap());
        let out = s.step(&c, &b, &[]).unwrap();
        let draw = &out.state.rounds[&(0, 1)];
        assert_eq!(hex::encode(draw.instance_seed), row["seed"]);
        assert_eq!(
            serde_json::json!([draw.pool_seq, draw.leaf_index]),
            row["pair"]
        );
        if row["j"] == 3 {
            s = out.state;
            break;
        }
    }
    assert_eq!(
        (s.rounds[&(0, 1)].pool_seq, s.rounds[&(0, 1)].leaf_index),
        (3, 1)
    );
    assert!(!s.pools[&1].unused.is_empty());
    until(&mut s, &c, 20);
    let selected: std::collections::BTreeSet<_> = s
        .rounds
        .iter()
        .filter(|((slot, _), _)| *slot == 0)
        .map(|(_, r)| (r.pool_seq, r.leaf_index))
        .collect();
    assert_eq!(selected, [(1, 0), (1, 2), (1, 5), (3, 1)].into());
    assert_eq!(s.pools[&4].unused.len(), 1);
}
fn package_state(c: &Config, codec: u8, raw: u32, pkg: &[u8]) -> State {
    let root = boole_derivation::m2::pool_leaf(c.chain_id, 2, &[], codec, raw, pkg, &[9; 32]);
    let mut s = State::new(c, 30_000_000).unwrap();
    assert_eq!(
        run(&mut s, c, vec![input(8, pool_at(c, 0, root, 1, 3, 1), 0)]),
        [0x85]
    );
    until(&mut s, c, 4);
    s
}
#[test]
fn v11_t27_carried_package_policy_membership_and_caps() {
    let c = cfg();
    let s = package_state(&c, 0, 1, b"a");
    for (codec, raw, pkg) in [
        (2, 1, b"a".to_vec()),
        (0, 2, b"a".to_vec()),
        (1, 0, b"a".to_vec()),
        (1, c.statement_raw_max as u32 + 1, b"a".to_vec()),
        (1, 1, vec![]),
        (0, c.statement_max as u32 + 1, vec![1; c.statement_max + 1]),
    ] {
        assert_eq!(
            s.step(
                &c,
                &block(5),
                &[input(
                    9,
                    pkg_body(0, 1, 0, codec, raw, [9; 32], &[], &pkg),
                    0
                )]
            )
            .unwrap()
            .codes,
            [2]
        );
    }
    let mut old = pkg_body(0, 1, 0, 0, 1, [9; 32], &[], b"a");
    old[0] = 1;
    assert_eq!(
        s.step(&c, &block(5), &[input(9, old, 0)]).unwrap().codes,
        [1]
    );
    let pkg = vec![1; c.statement_max];
    let mut exact = package_state(&c, 0, pkg.len() as u32, &pkg);
    assert_eq!(
        run(
            &mut exact,
            &c,
            vec![input(
                9,
                pkg_body(0, 1, 0, 0, pkg.len() as u32, [9; 32], &[], &pkg),
                0
            )]
        ),
        [0x86]
    );
    // Valid carried bytes need not be decoded, and no ratio cap applies to packages.
    let fixture: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/devnet-m2/package-codec-v1.1.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let pkg = hex::decode(fixture["carried_hex"].as_str().unwrap()).unwrap();
    let raw = c.statement_raw_max as u32;
    let mut compressed = package_state(&c, 1, raw, &pkg);
    assert!(u64::from(raw) > pkg.len() as u64 * c.ratio_max);
    assert_eq!(
        run(
            &mut compressed,
            &c,
            vec![input(9, pkg_body(0, 1, 0, 1, raw, [9; 32], &[], &pkg), 0)]
        ),
        [0x86]
    );
    let wrong = pool_leaf(c.chain_id, 2, &[], b"a", &[9; 32]);
    let mut s = State::new(&c, 30_000_000).unwrap();
    run(
        &mut s,
        &c,
        vec![input(8, pool_at(&c, 0, wrong, 1, 3, 1), 0)],
    );
    until(&mut s, &c, 4);
    assert_eq!(
        run(
            &mut s,
            &c,
            vec![input(9, pkg_body(0, 1, 0, 1, 1, [9; 32], &[], b"a"), 0)]
        ),
        [14]
    );
}
#[test]
fn v11_t28_lane_formats_and_cancel_after_wrong_lane() {
    for (lane, format, expected) in [(1, 1, 0x81), (2, 2, 0x81), (1, 2, 2), (2, 1, 2)] {
        let mut c = cfg();
        c.slot_table[0].lane = lane;
        let (mut s, p) = opened(&c);
        let (mut b, _, _) = body(&c, p, [6; 20], [7; 32], b"a", &[]);
        b[152] = format;
        let ch = commit_hash(c.chain_id, p, cancel_hash(&[8; 32]), body_digest(&b));
        run(&mut s, &c, vec![input(2, commit(p, ch), 0)]);
        let hc = s.height();
        until(&mut s, &c, hc + c.d - 1);
        assert_eq!(
            run(
                &mut s,
                &c,
                vec![input(3, open(&b, cancel_hash(&[8; 32])), 0)]
            ),
            [expected]
        );
        if expected == 2 {
            assert_eq!(s.candidates[&(p, ch)].status(), Status::Committed);
            assert_eq!(
                run(&mut s, &c, vec![input(7, cancel(p, ch, &b, [8; 32]), 0)]),
                [0x84]
            );
        }
    }
}

#[test]
fn review_real_multislot_tables_select_the_problem_lane() {
    for profile in [Config::devnet(), Config::lean_testnet(99)] {
        let mut c = profile.clone();
        c.pool_key = cfg().pool_key;
        // Only shorten pool admission lead; retain every real slot row and D/W.
        c.pool_lead = 2;
        assert_eq!(c.slot_table, profile.slot_table);
        let mut s = State::new(&c, 30_000_000).unwrap();
        let pools = (0..c.slot_table.len())
            .map(|slot| {
                let salt = [slot as u8 + 1; 32];
                let root = pool_leaf(c.chain_id, 2, &[], b"a", &salt);
                input(
                    8,
                    pool_at(&c, slot as u8, root, 1, 3, slot as u32 + 1),
                    slot as u64,
                )
            })
            .collect();
        assert_eq!(run(&mut s, &c, pools), vec![0x85; c.slot_table.len()]);
        until(&mut s, &c, 4);
        let reveals = (0..c.slot_table.len())
            .map(|slot| {
                input(
                    9,
                    problem_body(slot as u8, 1, 0, &[], [slot as u8 + 1; 32], &[], b"a"),
                    slot as u64,
                )
            })
            .collect();
        assert_eq!(run(&mut s, &c, reveals), vec![0x86; c.slot_table.len()]);
        let mut commits = vec![];
        let mut opens = vec![];
        let mut expected = vec![];
        for slot in 0..c.slot_table.len() {
            let p = s.rounds[&(slot as u8, 1)].problem.unwrap();
            assert_eq!(s.problems[&p].slot, slot as u8);
            for format in [1, 2] {
                let (mut b, _, _) = body(&c, p, [6; 20], [7; 32], b"a", &[]);
                b[152] = format;
                let ch = commit_hash(c.chain_id, p, cancel_hash(&[8; 32]), body_digest(&b));
                let index = commits.len() as u64;
                commits.push(input(2, commit(p, ch), index));
                opens.push(input(3, open(&b, cancel_hash(&[8; 32])), index));
                expected.push(if format == c.slot_table[slot].lane {
                    0x81
                } else {
                    2
                });
            }
        }
        assert_eq!(run(&mut s, &c, commits), vec![0x80; expected.len()]);
        let hc = s.height();
        until(&mut s, &c, hc + c.d - 1);
        assert_eq!(run(&mut s, &c, opens), expected);
        assert_eq!(
            expected,
            if c.slot_table.len() == 2 {
                vec![2, 0x81, 0x81, 2]
            } else {
                vec![2, 0x81, 0x81, 2, 2, 0x81]
            }
        );
    }
}

#[test]
fn review_union_excludes_pool_starting_at_draw_execution_height() {
    let c = cfg();
    let mut s = State::new(&c, 30_000_000).unwrap();
    assert_eq!(
        run(
            &mut s,
            &c,
            vec![
                input(8, pool_at(&c, 0, [1; 32], 1, 3, 1), 0),
                input(8, pool_at(&c, 0, [2; 32], 1, 4, 2), 1),
            ]
        ),
        [0x85, 0x85]
    );
    until(&mut s, &c, 3);
    assert_eq!(s.slots[0].scheduled, Some(3));
    // Choose j=1 for a wrongly enlarged two-leaf union, so the boundary error
    // cannot hide behind a draw that happens to select the first pool.
    let mix = (0u64..256)
        .map(|n| hash(&n.to_be_bytes()))
        .find(|mix| {
            let seed = hash(
                &[
                    b"BOOLE-M2/DRAW/v2".as_slice(),
                    &c.chain_id.to_be_bytes(),
                    &[0],
                    &1u32.to_be_bytes(),
                    mix,
                ]
                .concat(),
            );
            u64::from_be_bytes(seed[..8].try_into().unwrap()) % 2 == 1
        })
        .unwrap();
    let mut execution = block(4);
    execution.mix_hash = format!("0x{}", hex::encode(mix));
    let out = s.step(&c, &execution, &[]).unwrap();
    assert_eq!(
        (
            out.state.rounds[&(0, 1)].pool_seq,
            out.state.rounds[&(0, 1)].leaf_index
        ),
        (1, 0)
    );
    assert!(out.state.pools[&1].unused.is_empty());
    assert_eq!(out.state.pools[&2].unused, [0].into());
}

#[test]
fn review_unused_leaf_capacity_is_per_slot_including_future_pools() {
    let mut c = cfg();
    c.slot_table.push(c.slot_table[0].clone());
    c.pool_max = 2;
    c.slot_leaves_max = 2;
    let mut s = State::new(&c, 30_000_000).unwrap();
    assert_eq!(
        run(
            &mut s,
            &c,
            vec![
                input(8, pool_at(&c, 0, [1; 32], 2, 100, 1), 0),
                input(8, pool_at(&c, 1, [2; 32], 2, 100, 2), 1),
            ]
        ),
        [0x85, 0x85]
    );
    assert_eq!(s.pools.values().map(|p| p.unused.len()).sum::<usize>(), 4);
    assert_eq!(
        run(
            &mut s,
            &c,
            vec![input(8, pool_at(&c, 1, [3; 32], 1, 100, 3), 0)]
        ),
        [13]
    );
    assert_eq!(s.pools.len(), 2);
}

#[test]
fn review_s8_statement_max_zero_and_one() {
    let mut c = cfg();
    c.statement_max = 0;
    assert_eq!(c.validate(30_000_000), Err(8));
    assert!(matches!(
        State::new(&c, 30_000_000),
        Err(FoldError::Startup(8))
    ));
    c.statement_max = 1;
    assert_eq!(c.validate(30_000_000), Ok(()));
}

#[test]
fn review_s5_exact_abi_padding_boundary() {
    let mut c = cfg();
    c.pool_max = 1 << 20;
    c.slot_leaves_max = 1 << 20;
    c.pkg_table = (0u8..4).map(|i| hash(&[i])).collect();
    c.pkg_table.sort();
    c.env_allowed = [vec![]].into();
    // 55 bytes including kind + 4 required packages + 20 siblings. The
    // middle case ends exactly on a 32-byte ABI boundary; its neighbors do not.
    for (statement_len, exact_call_len) in
        [(122_888, 123_780), (122_889, 123_780), (122_890, 123_812)]
    {
        c.statement_max = statement_len;
        let wire = problem_body(
            0,
            1,
            0,
            &c.pkg_table,
            [9; 32],
            &[[0; 32]; 20],
            &vec![1; statement_len],
        );
        let mut item = vec![9];
        item.extend(wire);
        while item.len() % 32 != 0 {
            item.push(0);
        }
        assert_eq!(4 + 64 + item.len(), exact_call_len);
        c.pool_limit = exact_call_len;
        assert_eq!(c.validate(30_000_000), Ok(()));
        c.pool_limit -= 1;
        assert_eq!(c.validate(30_000_000), Err(5));
    }
}
fn two_slots() -> (Config, State, [Hash; 2]) {
    let mut c = cfg();
    c.slot_table = vec![
        SlotParams {
            lane: 1,
            solve_deadline: 8,
            announce_gap: 2,
            reveal_grace: 1,
        },
        SlotParams {
            lane: 2,
            solve_deadline: 16,
            announce_gap: 4,
            reveal_grace: 3,
        },
    ];
    let mut s = State::new(&c, 30_000_000).unwrap();
    let roots = [
        pool_leaf(c.chain_id, 2, &[], b"a", &[1; 32]),
        pool_leaf(c.chain_id, 2, &[], b"a", &[2; 32]),
    ];
    run(
        &mut s,
        &c,
        vec![
            input(8, pool_at(&c, 0, roots[0], 1, 3, 1), 0),
            input(8, pool_at(&c, 1, roots[1], 1, 3, 2), 1),
        ],
    );
    until(&mut s, &c, 4);
    (c, s, roots)
}
#[test]
fn v11_t29_per_slot_deadlines_grace_and_gaps() {
    let (c, mut s, _) = two_slots();
    let withheld = s.clone();
    run(
        &mut s,
        &c,
        vec![
            input(9, pkg_body(0, 1, 0, 0, 1, [1; 32], &[], b"a"), 0),
            input(9, pkg_body(1, 1, 0, 0, 1, [2; 32], &[], b"a"), 1),
        ],
    );
    let p0 = s.rounds[&(0, 1)].problem.unwrap();
    let p1 = s.rounds[&(1, 1)].problem.unwrap();
    until(&mut s, &c, 13);
    assert_eq!(
        run(
            &mut s,
            &c,
            vec![
                input(2, commit(p0, [1; 32]), 0),
                input(2, commit(p1, [2; 32]), 1)
            ]
        ),
        [5, 0x80]
    );
    assert_eq!(s.slots[0].scheduled, Some(16));
    until(&mut s, &c, 22);
    assert_eq!(s.problems[&p1].closed, Some(22));
    assert_eq!(s.slots[1].scheduled, Some(26));
    let mut s = withheld;
    until(&mut s, &c, 6);
    assert_eq!(s.rounds[&(0, 1)].status, RoundStatus::Withheld);
    assert_eq!(s.rounds[&(1, 1)].status, RoundStatus::Drawn);
    until(&mut s, &c, 8);
    assert_eq!(s.rounds[&(1, 1)].status, RoundStatus::Withheld);
}
#[test]
fn v11_t30_config_seed_root_domain_and_round_pool_sequence() {
    let c = cfg();
    let mut bytes = b"BOOLE-M2/CONFIG/v1".to_vec();
    bytes.extend(serde_json::to_vec(&c).unwrap());
    let digest = hash(&bytes);
    assert_eq!(c.digest(), digest);
    let mut s = State::new(&c, 30_000_000).unwrap();
    assert_eq!(s.root, digest);
    let mut other = c.clone();
    other.slot_table[0].lane = 2;
    assert_ne!(s.root, State::new(&other, 30_000_000).unwrap().root);
    let root = pool_leaf(c.chain_id, 2, &[], b"a", &[9; 32]);
    run(&mut s, &c, vec![input(8, pool_at(&c, 0, root, 1, 3, 1), 0)]);
    until(&mut s, &c, 3);
    let previous = s.root;
    let step = s.step(&c, &block(4), &[]).unwrap();
    let mut event = vec![3, 0];
    event.extend(1u32.to_be_bytes());
    event.push(0xc1);
    event.extend(1u32.to_be_bytes());
    event.extend(0u32.to_be_bytes());
    event.extend([0; 32]);
    assert_eq!(step.events, event);
    assert_eq!(event.len(), 47);
    assert_eq!(
        step.state.root,
        hash(
            &[
                b"BOOLE-M2/ROOT/v2".as_slice(),
                &previous,
                &4u64.to_be_bytes(),
                &hex::decode(&block(4).hash[2..]).unwrap(),
                &hash(&event)
            ]
            .concat()
        )
    );
}
#[test]
fn v11_t31_repose_cannot_cross_slots() {
    let (c, mut s, _) = two_slots();
    run(
        &mut s,
        &c,
        vec![
            input(9, pkg_body(0, 1, 0, 0, 1, [1; 32], &[], b"a"), 0),
            input(9, pkg_body(1, 1, 0, 0, 1, [2; 32], &[], b"a"), 1),
        ],
    );
    let p = s.rounds[&(1, 1)].problem.unwrap();
    until(&mut s, &c, 22);
    assert!(s.enqueue_repose(p));
    assert!(!s.enqueue_repose(p));
    until(&mut s, &c, 26);
    assert!(!s.problems.values().any(|p| p.epoch == 1));
    run(&mut s, &c, vec![]);
    let new = s.problems.values().find(|p| p.epoch == 1).unwrap();
    assert_eq!(new.slot, 1);
    assert_eq!(new.pool_seq, 2);
    assert_eq!(s.rounds[&(1, 2)].problem, Some(new.id));
    assert_eq!(s.rounds[&(1, 2)].pool_seq, 2);
}

#[test]
fn v11_t24_t30_independent_config_and_slot_digest_goldens() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m2/config-golden-v1.1.json"
    ))
    .unwrap();
    assert_eq!(
        fixture["provenance"]["generator_sha256"],
        hex::encode(hash(include_bytes!("generate_v11_vectors.py")))
    );
    for (at, c) in [
        Config::devnet(),
        Config::lean_testnet(720927),
        Config::production(720928),
    ]
    .into_iter()
    .enumerate()
    {
        let row = &fixture["profiles"][at];
        assert_eq!(serde_json::to_value(&c).unwrap(), row["config"]);
        assert_eq!(
            hex::encode(serde_json::to_vec(&c).unwrap()),
            row["canonical_json_hex"]
        );
        assert_eq!(hex::encode(c.digest()), row["config_digest"]);
        assert_eq!(hex::encode(c.slot_table_digest()), row["slot_table_digest"]);
    }
}

#[test]
fn v11_t27_leaf_binds_transmission_codec_and_raw_length() {
    let c = cfg();
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m2/package-codec-v1.1.json"
    ))
    .unwrap();
    let pkg = hex::decode(fixture["carried_hex"].as_str().unwrap()).unwrap();
    let raw = fixture["raw_len"].as_u64().unwrap() as u32;
    let legacy = hash(
        &[
            &[0u8][..],
            b"BOOLE-M2/POOL-LEAF/v1",
            &c.chain_id.to_be_bytes(),
            &[2, 0],
            &hash(&pkg),
            &[9; 32],
        ]
        .concat(),
    );
    let decoded = boole_derivation::m2::pool_leaf(
        c.chain_id,
        2,
        &[],
        1,
        raw,
        &vec![b'a'; raw as usize],
        &[9; 32],
    );
    for root in [legacy, decoded] {
        let mut s = State::new(&c, 30_000_000).unwrap();
        run(&mut s, &c, vec![input(8, pool_at(&c, 0, root, 1, 3, 1), 0)]);
        until(&mut s, &c, 4);
        assert_eq!(
            run(
                &mut s,
                &c,
                vec![input(9, pkg_body(0, 1, 0, 1, raw, [9; 32], &[], &pkg), 0)]
            ),
            [14]
        );
    }
    let s = package_state(&c, 1, raw, &pkg);
    assert_eq!(
        s.step(
            &c,
            &block(5),
            &[input(
                9,
                pkg_body(0, 1, 0, 1, raw - 1, [9; 32], &[], &pkg),
                0
            )]
        )
        .unwrap()
        .codes,
        [14]
    );
    let mut s = package_state(&c, 1, 100, b"not a compressed frame");
    assert_eq!(
        run(
            &mut s,
            &c,
            vec![input(
                9,
                pkg_body(0, 1, 0, 1, 100, [9; 32], &[], b"not a compressed frame"),
                0
            )]
        ),
        [0x86]
    );
}

#[test]
fn v11_t12_fixed_root_and_events_match_independent_vector() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m2/config-golden-v1.1.json"
    ))
    .unwrap();
    let c = Config::devnet();
    let state = State::new(&c, 30_000_000).unwrap();
    let step = state
        .step(&c, &block(1), &[input(5, vec![1], 4), input(2, vec![], 2)])
        .unwrap();
    assert_eq!(
        hex::encode(&step.events),
        fixture["profiles"][0]["probe_event_hex"]
    );
    assert_eq!(
        hex::encode(step.state.root),
        fixture["profiles"][0]["root_height1_probe"]
    );
}

#[test]
fn v11_t27_package_fixture_has_required_frame_header() {
    let value: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m2/package-codec-v1.1.json"
    ))
    .unwrap();
    let b = hex::decode(value["carried_hex"].as_str().unwrap()).unwrap();
    assert_eq!(&b[..4], &[0x28, 0xb5, 0x2f, 0xfd]);
    let d = b[4];
    assert_eq!(d & 3, 0);
    assert_ne!(d & 4, 0);
    assert_eq!(d & 0x18, 0);
    let single = d & 32 != 0;
    let mut at = 5;
    let window = if single {
        None
    } else {
        let w = b[at];
        at += 1;
        let base = 1u64 << (10 + (w >> 3));
        Some(base + base / 8 * u64::from(w & 7))
    };
    let n = match d >> 6 {
        0 => usize::from(single),
        1 => 2,
        2 => 4,
        _ => 8,
    };
    assert_ne!(n, 0, "R32 requires Frame_Content_Size");
    let mut raw = [0; 8];
    raw[..n].copy_from_slice(&b[at..at + n]);
    let raw = u64::from_le_bytes(raw) + if n == 2 { 256 } else { 0 };
    assert_eq!(raw, value["raw_len"].as_u64().unwrap());
    assert!(window.unwrap_or(raw) <= raw);
}
