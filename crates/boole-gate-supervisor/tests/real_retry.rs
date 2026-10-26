use boole_gate_supervisor::real_retry::Retry;
#[test]
fn real_v2_a2_a3_slow_canary_and_d_node_boundaries() {
    let mut r = Retry::default();
    r.deferred(10, "T", false);
    assert_eq!((r.attempts, r.next_at), (1, 70));
    assert!(!r.ready(69));
    assert!(r.ready(70));
    assert_eq!(r.stage(), "A2");
    r.deferred(70, "I", false);
    assert!(r.third_authorized);
    assert_eq!((r.attempts, r.next_at), (2, 670));
    assert!(!r.ready(669));
    assert!(r.ready(670));
    assert_eq!(r.stage(), "A3");
    let before = (r.attempts, r.next_at);
    r.deferred(80, "D-node", true);
    assert_eq!((r.attempts, r.next_at), before);
    r.deferred(670, "I", false);
    assert_eq!((r.attempts, r.next_at), (3, 4270));
    r.canary(4270, true);
    assert_eq!(r.next_at, 7870);
    assert!(!r.ready(7869));
    assert!(r.ready(7870));
    assert_eq!(r.stage(), "slow");
    let mut same = Retry::default();
    same.deferred(0, "T", false);
    same.deferred(60, "T", false);
    assert!(!same.third_authorized);
    assert_eq!(same.next_at, 3660);
    same.canary(100, false);
    assert!(same.third_authorized);
    assert_eq!(same.attempts, 2);
    assert_eq!(same.next_at, 700);
    same.canary(700, true);
    assert_eq!(same.next_at, 700);
    assert!(same.ready(700));
    assert_eq!(same.stage(), "A3");
}

#[test]
fn d_node_backoff_is_bounded_without_submitted_attempts() {
    let mut r = Retry::default();
    r.deferred(10, "D-node", false);
    assert_eq!(r.attempts, 0);
    assert_eq!(r.next_at, 70);
    assert!(!r.ready(69));
    assert!(r.ready(70));
    assert_eq!(r.stage(), "A1");
}
