//! M2-R1–R28: independently encoded inbox items and transition oracles.
use boole_derivation::{m2::*, L1Block};
use k256::ecdsa::SigningKey;
use sha2::{Digest, Sha256};

fn hash(b: &[u8]) -> Hash {
    Sha256::digest(b).into()
}
fn cfg() -> Config {
    let mut c = Config::devnet();
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
    let h = s.height + 1;
    let out = s.step(c, &block(h), &items);
    *s = out.state;
    out.codes
}
fn until(s: &mut State, c: &Config, h: u64) {
    while s.height < h {
        run(s, c, vec![]);
    }
}
fn pool(c: &Config, root: Hash, count: u32, start: u64, seq: u32) -> Vec<u8> {
    let mut b = vec![1];
    b.extend(seq.to_be_bytes());
    b.extend(root);
    b.extend(count.to_be_bytes());
    b.extend(start.to_be_bytes());
    b.extend([0; 32]);
    let pre = hash(
        &[
            b"BOOLE-M2/POOL/v1".as_slice(),
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
fn problem_body(
    slot: u8,
    round: u32,
    index: u32,
    req: &[Hash],
    salt: Hash,
    path: &[Hash],
    package: &[u8],
) -> Vec<u8> {
    let mut b = vec![1, slot];
    b.extend(round.to_be_bytes());
    b.extend(index.to_be_bytes());
    b.extend([2, req.len() as u8]);
    for p in req {
        b.extend(p);
    }
    b.extend(salt);
    b.push(path.len() as u8);
    for p in path {
        b.extend(p);
    }
    b.extend((package.len() as u32).to_be_bytes());
    b.extend(package);
    b
}
fn opened(c: &Config) -> (State, Hash) {
    let mut s = State::new(c);
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
            let o = s.step(&c, &block(s.height + 1), &[input(kind, bad, 0)]);
            if kind != 4 {
                assert_eq!(o.codes, [1], "kind {kind}");
            }
        }
        let mut bad = good;
        bad[0] = 2;
        assert_eq!(
            s.step(&c, &block(s.height + 1), &[input(kind, bad, 0)])
                .codes,
            [1]
        );
    }
    for k in [5, 6, 10, 11, 12, 13, 14, 15] {
        assert_eq!(
            s.step(&c, &block(s.height + 1), &[input(k, vec![], 0)])
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
    let hc = s.height;
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
    let end = s.height + c.w;
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
        let hc = s.height - 1;
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
    let hc = s.height;
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
    let mut s = State::new(&c);
    let b = block(1);
    let items = vec![input(5, vec![1], 4), input(2, vec![], 2)];
    let a = s.step(&c, &b, &items);
    let mut rev = items;
    rev.reverse();
    let z = s.step(&c, &b, &rev);
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
                b"BOOLE-M2/ROOT/v1".as_slice(),
                &[0; 32],
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
    let hc = s.height;
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
fn t13_r10_reference_barrier_sequences() {
    let c = cfg();
    let (mut s, p) = opened(&c);
    let mut reference = std::collections::BTreeMap::new();
    for i in 0u64..200 {
        let ch = hash(&i.to_be_bytes());
        let hc = s.height + 1;
        run(&mut s, &c, vec![input(2, commit(p, ch), 0)]);
        if hc <= 53 {
            reference.insert(ch, hc + c.w + 1);
        }
        reference.retain(|_, expiry| *expiry > s.height);
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
        let o = s.step(&c, &block(s.height + 1), &[input(kind, raw, 0)]);
        assert_eq!(o.codes.len(), 1);
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
    let close = s.problems[&p].h_v + c.solve_deadline + 1;
    until(&mut s, &c, close + c.w + 1);
    assert!(!s.candidates.contains_key(&(p, ch)));
    assert_eq!(run(&mut s, &c, vec![input(2, commit(p, ch), 0)]), [5]);
}
#[test]
fn t18_r24_r25_pool_signature_draw_no_replacement() {
    let mut c = cfg();
    c.k_open = 2;
    let mut s = State::new(&c);
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
    let b = &s.rounds[&(1, 1)];
    assert_ne!(a.leaf_index, b.leaf_index);
    let seed = hash(
        &[
            b"BOOLE-M2/DRAW/v1".as_slice(),
            &c.chain_id.to_be_bytes(),
            &root,
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
}
#[test]
fn t19_r26_r27_problem_reveal_deadline_withheld_repose() {
    let c = cfg();
    let (mut s, p) = opened(&c);
    let hv = s.height;
    assert_eq!(
        s.step(&c, &block(hv), &[input(2, commit(p, [1; 32]), 0)])
            .codes,
        [9]
    );
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
    until(&mut s, &c, hv + c.solve_deadline);
    assert_eq!(run(&mut s, &c, vec![input(2, commit(p, [2; 32]), 0)]), [5]);
    assert_eq!(s.slots[0].scheduled, Some(s.height + c.announce_gap));
    assert!(s.enqueue_repose(p));
    until(&mut s, &c, hv + c.solve_deadline + 1 + c.announce_gap + 1);
    let new = s.problems.values().find(|x| x.epoch == 1).unwrap();
    assert_ne!(new.id, p);
    let mut s = State::new(&c);
    let root = pool_leaf(c.chain_id, 2, &[], b"a", &[1; 32]);
    run(&mut s, &c, vec![input(8, pool(&c, root, 1, 3, 1), 0)]);
    until(&mut s, &c, 5 + c.reveal_grace);
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
            s.step(&c, &block(s.height + 1), &[input(3, open(&b, [0; 32]), 0)])
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
            s.step(&c, &block(s.height + 1), &[input(3, open(&b, [0; 32]), 0)])
                .codes,
            [2]
        );
    }
    let (b, _, _) = body(&c, p, [6; 20], [7; 32], b"data", &[]);
    for at in [85, 86, 118, 151, 152, 231, 233, 239, 240, 273] {
        let mut x = b.clone();
        x[at] ^= 3;
        let code = s
            .step(&c, &block(s.height + 1), &[input(3, open(&x, [0; 32]), 0)])
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
            s.step(&c, &block(s.height + 1), &[input(3, open(&b, [0; 32]), 0)])
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
        assert!(c.validate().is_ok());
    }
    for which in 1..=6 {
        let mut c = cfg();
        match which {
            1 => c.d = 1,
            2 => c.w = 1,
            3 => c.c_commit = c.q_max * c.v_per_deposit + 1,
            4 => c.gas_limit = 1,
            5 => c.pool_limit = 1,
            6 => c.k_open = 4,
            _ => unreachable!(),
        }
        assert_eq!(c.validate(), Err(which));
    }
    let mut c = cfg();
    c.env_allowed.insert(vec![c.pkg_table[0], c.pkg_table[1]]);
    assert_eq!(c.validate(), Err(6));
}

#[test]
fn t11_r13_m1_attributes_and_evidence_37_heights_unchanged() {
    use boole_derivation::{derive, derive_with_m2, InboxLog, ParentHeader};
    let f: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m1/s1b-recorded-main-budget-v1.json"
    ))
    .unwrap();
    let mut rows = f["heights"].as_array().unwrap().clone();
    rows.extend(
        serde_json::from_str::<Vec<serde_json::Value>>(include_str!(
            "../../../fixtures/devnet-m1/s1b-recorded-basefee.json"
        ))
        .unwrap(),
    );
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
        let mut mixed = logs.clone();
        for kind in 2..16 {
            let mut data = vec![0; 64];
            data[31] = 32;
            data[63] = 1;
            data.push(kind);
            mixed.push(InboxLog {
                address: boole_derivation::config::INBOX.into(),
                topics: vec![boole_derivation::config::posted_topic()],
                block_hash: l1.hash.clone(),
                removed: false,
                transaction_index: 100 + kind as u64,
                log_index: 100 + kind as u64,
                data: format!("0x{}", hex::encode(data)),
            });
        }
        let c = cfg();
        let mut state = State::new(&c);
        state.height = parent.number;
        let (new, _) = derive_with_m2(&state, &c, &l1, &mixed, &parent).unwrap();
        assert_eq!(
            new.canonical_bytes().unwrap(),
            old.canonical_bytes().unwrap()
        );
        for d in &new.decisions[logs.len()..] {
            assert!(!d.forward);
            assert!(d.m2.is_some());
        }
    }
}
#[test]
fn t7_r23_all_rows_and_k0_k4_layout() {
    let mut c = cfg();
    c.incompatible.clear();
    for i in 0..2 {
        c.pkg_table.push(hash(&[i]));
    }
    c.pkg_table.sort();
    let (s, p) = opened(&c);
    for k in 0..=4 {
        let pkgs = c.pkg_table[..k].to_vec();
        c.env_allowed.insert(pkgs.clone());
        let (b, _, _) = body(&c, p, [6; 20], [7; 32], b"data", &pkgs);
        assert_eq!(b.len(), 277 + 32 * k);
        let good = open(&b, [0; 32]);
        assert_eq!(good.len(), 330 + 32 * k);
        assert_eq!(
            s.step(&c, &block(6), &[input(3, good.clone(), 0)]).codes,
            [6]
        );
        for delta in [-1, 1] {
            let mut x = good.clone();
            if delta < 0 {
                x.pop();
            } else {
                x.push(0);
            }
            assert_eq!(s.step(&c, &block(6), &[input(3, x, 0)]).codes, [1]);
        }
    }
    let (b, _, _) = body(&c, p, [6; 20], [7; 32], b"data", &[]);
    for (offset, value, code) in [
        (0, 2, 1),
        (85, 2, 2),
        (86, 0, 2),
        (118, 0, 2),
        (151, 2, 2),
        (152, 2, 2),
        (237, 2, 1),
        (239, 0, 2),
        (240, 1, 2),
    ] {
        let mut x = b.clone();
        x[offset] = value;
        assert_eq!(
            s.step(&c, &block(6), &[input(3, open(&x, [0; 32]), 0)])
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
        let s = State::new(&c);
        let mut b = pool(&c, [1; 32], 1, 3, 1);
        match mode {
            0 => b = pool(&c, [1; 32], 1, 3, 2),
            1 => b = pool(&c, [1; 32], 1, 2, 1),
            2 => {
                let mut other = c.clone();
                other.chain_id += 1;
                b = pool(&other, [1; 32], 1, 3, 1);
            }
            3 => b[145] = 2,
            4 => {
                let order =
                    hex::decode("fffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141")
                        .unwrap();
                let mut borrow = 0i16;
                for i in (0..32).rev() {
                    let n = i16::from(order[i]) - i16::from(b[113 + i]) - borrow;
                    b[113 + i] = n.rem_euclid(256) as u8;
                    borrow = i16::from(n < 0);
                }
                b[145] ^= 1;
            }
            _ => unreachable!(),
        }
        assert_eq!(s.step(&c, &block(1), &[input(8, b, 0)]).codes, [13]);
    }
}
#[test]
fn t19_problem_reveal_invalid_path_round_size_and_required_package() {
    let c = cfg();
    let mut s = State::new(&c);
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
    let hc = s.height;
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
    let mut s = State::new(&c);
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
    let mut s = State::new(&c);
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
                .codes,
            [if row["within_z_cap"] == true { 6 } else { 3 }]
        );
    }
}
/// Reference adapter for the specified service subset: reveal_window=W+1,
/// D=0, CHECKING+PASS=VALID-REVEAL; injected verdicts stay outside M2 state.
#[test]
fn t13_shared_subset_with_injected_verdicts() {
    use std::collections::BTreeMap;
    let mut c = cfg();
    c.d = 0;
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
    let hc = s.height;
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
