use boole_gate_supervisor::{
    cache::*,
    codec::{digest, Scratch},
    GateInput, GateResult,
};
#[test]
fn g10_cache_is_bound_to_inputs_epoch_descriptor_and_authenticated_bytes() {
    let root = Scratch::new(&std::env::temp_dir()).unwrap();
    let path = root.0.join("cache");
    let cache = Cache::new(&path).unwrap();
    let key = CacheKey {
        gate: [1; 32],
        queue_index: 4,
        commit_hash: [2; 32],
        kind: 1,
    };
    let input = GateInput {
        chain_id: 720926,
        problem_id: [3; 32],
        commit_hash: [2; 32],
        solver: [4; 20],
        env_id: [5; 32],
        export_digest: digest(b"export"),
        lane: 1,
        leaf_kind: 1,
        instance_seed: [6; 32],
        fuel_slot: 10000,
        raw: b"export".to_vec(),
        package: b"statement".to_vec(),
        artifacts: vec![],
        early: None,
    };
    let result = GateResult {
        outcome: 1,
        reason: 0,
        fuel: 9,
        statement_id: [7; 32],
        ext: vec![],
    };
    cache.save(&key, &input, 0, &result).unwrap();
    assert_eq!(cache.load(&key, &input, 0), Some(result.clone()));
    let reopened = Cache::new(&path).unwrap();
    assert_eq!(reopened.load(&key, &input, 0), Some(result.clone()));
    assert!(cache.load(&key, &input, 1).is_none());
    let mut other = key.clone();
    other.gate[0] ^= 1;
    assert!(cache.load(&other, &input, 0).is_none());
    for which in 0..6 {
        let mut changed = input.clone();
        match which {
            0 => changed.solver[0] ^= 1,
            1 => changed.env_id[0] ^= 1,
            2 => changed.export_digest[0] ^= 1,
            3 => changed.problem_id[0] ^= 1,
            4 => changed.raw.push(0),
            _ => changed.package.push(0),
        };
        assert!(cache.load(&key, &changed, 0).is_none());
    }
    let entry = cache.entry_path(&key);
    let mut json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&entry).unwrap()).unwrap();
    json["entry"]["result"]["fuel"] = serde_json::json!(10);
    std::fs::write(entry, serde_json::to_vec(&json).unwrap()).unwrap();
    assert!(cache.load(&key, &input, 0).is_none());
}
#[test]
fn g6_retry_clock_keeps_fast_retries_and_hourly_retries_node_local() {
    use boole_gate_supervisor::supervisor::Retry;
    let mut r = Retry::default();
    r.deferred(10, false);
    assert_eq!((r.attempts, r.next_at), (1, 70));
    r.deferred(70, false);
    assert_eq!((r.attempts, r.next_at), (2, 3670));
    let mut r = Retry::default();
    r.deferred(10, true);
    r.deferred(70, true);
    assert_eq!(r.next_at, 130);
    r.deferred(130, true);
    assert_eq!(r.next_at, 3730);
}
