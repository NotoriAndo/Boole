use boole_core::{h_protocol, Hex32};

use boole_miner::{
    target_seed, FixedSeedTargetEmitter, StubTargetEmitter, TargetEmitArgs, TargetEmitter,
};

fn pk32() -> Hex32 {
    let mut x = [0u8; 32];
    for (i, b) in x.iter_mut().enumerate() {
        *b = i as u8;
    }
    Hex32::from_bytes(x)
}

fn c32() -> Hex32 {
    let mut x = [0u8; 32];
    for (i, b) in x.iter_mut().enumerate() {
        *b = (255 - i) as u8;
    }
    Hex32::from_bytes(x)
}

fn n32() -> Hex32 {
    Hex32::from_bytes([0x42; 32])
}

#[test]
fn test_target_seed_matches_explicit_h_protocol_call() {
    let c = c32();
    let pk = pk32();
    let n = n32();
    let j: u32 = 7;
    let expected = h_protocol(
        b"target",
        &[c.as_bytes(), pk.as_bytes(), n.as_bytes(), &j.to_be_bytes()],
    );
    let actual = target_seed(&c, &pk, &n, j);
    assert_eq!(actual.as_bytes(), expected.as_bytes());
}

#[test]
fn test_target_seed_distinct_for_distinct_j() {
    let c = c32();
    let pk = pk32();
    let n = n32();
    let s0 = target_seed(&c, &pk, &n, 0);
    let s1 = target_seed(&c, &pk, &n, 1);
    let s2 = target_seed(&c, &pk, &n, 2);
    assert_ne!(s0.as_bytes(), s1.as_bytes());
    assert_ne!(s1.as_bytes(), s2.as_bytes());
    assert_ne!(s0.as_bytes(), s2.as_bytes());
}

#[test]
fn test_stub_emitter_emits_seed_matching_target_seed() {
    let c = c32();
    let pk = pk32();
    let n = n32();
    let emitter = StubTargetEmitter::new("synthetic invariant");
    let target = emitter
        .emit(&TargetEmitArgs {
            c: &c,
            pk: &pk,
            n: &n,
            j_index: 3,
            d: 1,
            profile: "v01".to_string(),
            n_param: None,
        })
        .unwrap();
    assert_eq!(target.seed_hex, target_seed(&c, &pk, &n, 3).to_hex());
    assert_eq!(target.d, 1);
    assert_eq!(target.profile, "v01");
    assert_eq!(target.n, 1);
    assert_eq!(target.render, "synthetic invariant");
}

#[test]
fn test_stub_emitter_passes_through_n_param() {
    let c = c32();
    let pk = pk32();
    let n = n32();
    let emitter = StubTargetEmitter::new("x");
    let t = emitter
        .emit(&TargetEmitArgs {
            c: &c,
            pk: &pk,
            n: &n,
            j_index: 0,
            d: 4,
            profile: "v031".to_string(),
            n_param: Some(3),
        })
        .unwrap();
    assert_eq!(t.n, 3);
    assert_eq!(t.profile, "v031");
}

#[test]
fn test_fixed_seed_emitter_returns_pinned_pair() {
    let c = c32();
    let pk = pk32();
    let n = n32();
    let emitter = FixedSeedTargetEmitter {
        seed_hex: "deadbeef".to_string(),
        render: "fixed render".to_string(),
        d: 2,
        profile: "v02".to_string(),
        n: Some(1),
    };
    let t = emitter
        .emit(&TargetEmitArgs {
            c: &c,
            pk: &pk,
            n: &n,
            j_index: 99,
            d: 7, // ignored
            profile: "ignored".to_string(),
            n_param: None,
        })
        .unwrap();
    assert_eq!(t.seed_hex, "deadbeef");
    assert_eq!(t.render, "fixed render");
    assert_eq!(t.d, 2);
    assert_eq!(t.profile, "v02");
    assert_eq!(t.n, 1);
}
