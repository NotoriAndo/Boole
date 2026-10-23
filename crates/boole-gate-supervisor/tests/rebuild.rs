use boole_derivation::m2::*;
use boole_gate_supervisor::rebuild::rebuild;
mod support;
use support::*;
#[test]
fn t42_rebuild_uses_first_r9_accepted_parts_and_frozen_positions() {
    let (c, s, key, mut a) = fixture();
    let r = rebuild(&s, &c, key, &mut a, None, &std::env::temp_dir()).unwrap();
    assert_eq!(r.transport, [42; 8]);
    assert_eq!(
        body_digest(&r.body),
        s.candidates[&key]
            .live()
            .unwrap()
            .open
            .as_ref()
            .unwrap()
            .body_digest
    );
    assert_eq!(r.input.package, b"generator-v1");
    assert!(!a.reads.contains(&15) && !a.reads.contains(&18));
    assert_eq!(r.contributions.len(), 3);
}
#[test]
fn t42_missing_anchored_bytes_never_become_a_reject() {
    let (c, s, key, mut a) = fixture();
    a.rows.remove(&16);
    assert!(rebuild(&s, &c, key, &mut a, None, &std::env::temp_dir()).is_err());
    assert_eq!(s.candidates[&key].status(), Status::Checking);
}
