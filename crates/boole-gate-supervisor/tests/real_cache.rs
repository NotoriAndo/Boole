use boole_gate_supervisor::{
    codec::Scratch,
    real_cache::{Cache, CacheKey},
};
#[test]
fn real_cache_authenticates_epoch_input_and_release_without_reinterpreting_stub_entries(
) -> anyhow::Result<()> {
    let root = Scratch::new(&std::env::temp_dir())?;
    let cache = Cache::new(&root.0)?;
    let key = CacheKey {
        gate: [1; 32],
        queue_index: 2,
        commit_hash: [3; 32],
        kind: 1,
    };
    let binding = [4; 32];
    let reply = b"canonical reply";
    cache.save_real(&key, binding, 5, reply)?;
    assert_eq!(
        cache.load_real(&key, binding, 5).as_deref(),
        Some(reply.as_slice())
    );
    assert!(
        cache.load_real(&key, binding, 6).is_none(),
        "foreign epoch hit"
    );
    assert!(
        cache.load_real(&key, [9; 32], 5).is_none(),
        "foreign problem/package binding hit"
    );
    let mut foreign = key.clone();
    foreign.gate = [8; 32];
    assert!(
        cache.load_real(&foreign, binding, 5).is_none(),
        "new descriptor/table hit"
    );
    let path = cache.real_entry_path(&key, binding, 5);
    let mut value: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
    value["entry"]["epoch"] = serde_json::json!(99);
    std::fs::write(&path, serde_json::to_vec(&value)?)?;
    assert!(
        cache.load_real(&key, binding, 5).is_none(),
        "unauthenticated real cache accepted"
    );
    Ok(())
}
