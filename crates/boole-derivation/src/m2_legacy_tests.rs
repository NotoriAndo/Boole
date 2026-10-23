//! The old externally callable re-pose seam is now private. Keep its v1.2
//! behavior checks inside the crate; M3 integration tests enter it via STALE.
use crate::{
    m2::{SlotParams, State},
    m3_scenario::*,
};

#[test]
fn t19_legacy_repose_keeps_epoch_increment() {
    let c = cfg();
    let (mut s, p) = opened(&c);
    let hv = s.problems[&p].h_v;
    until(&mut s, &c, hv + c.slot_table[0].solve_deadline + 1);
    assert!(s.enqueue_repose(p));
    until(
        &mut s,
        &c,
        hv + c.slot_table[0].solve_deadline + 1 + c.slot_table[0].announce_gap + 1,
    );
    let new = s.problems.values().find(|p| p.epoch == 1).unwrap();
    assert_ne!(new.id, p);
}
#[test]
fn v11_t31_repose_cannot_cross_slots() {
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
    run(
        &mut s,
        &c,
        vec![
            input(9, problem_body(0, 1, 0, &[], [1; 32], &[], b"a"), 0),
            input(9, problem_body(1, 1, 0, &[], [2; 32], &[], b"a"), 1),
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
