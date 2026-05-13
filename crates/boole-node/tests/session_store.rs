use boole_core::SessionState;
use boole_node::FileSessionStore;

fn fresh_tmp(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-node-session-store-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const PK_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PK_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const PK_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const ROOT: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const PK_D: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

fn fixture_session(session_pk: &str) -> SessionState {
    SessionState {
        session_pk: session_pk.to_string(),
        owner_pk: PK_B.to_string(),
        agent_pk: PK_C.to_string(),
        fixed_reward_recipient: PK_D.to_string(),
        allowed_family_root: ROOT.to_string(),
        max_fee_per_request: "12".to_string(),
        activation_height: 0,
        expiry_height: 100,
        revoked: false,
        policy_hash: ROOT.to_string(),
    }
}

#[test]
fn session_store_recovers_registered_sessions_and_revocations() {
    let dir = fresh_tmp("recover");
    let path = dir.join("sessions.ndjson");

    let mut store = FileSessionStore::recover(&path).expect("empty recover");
    let session = fixture_session(PK_A);
    store.append_register(&path, &session, 0).expect("register");
    store.append_revoke(&path, PK_A, 42).expect("revoke");

    let recovered = FileSessionStore::recover(&path).expect("recover");
    let s = recovered.get(PK_A).expect("session present after recover");
    assert!(s.revoked, "session should be revoked after recover");
    assert_eq!(s.session_pk, PK_A);
}

#[test]
fn session_store_rejects_duplicate_active_session() {
    let dir = fresh_tmp("dup");
    let path = dir.join("sessions.ndjson");

    let mut store = FileSessionStore::recover(&path).expect("empty recover");
    let session = fixture_session(PK_A);
    store
        .append_register(&path, &session, 0)
        .expect("first register");
    let err = store
        .append_register(&path, &session, 0)
        .expect_err("duplicate register must fail");
    assert!(
        err.to_string().contains("already") || err.to_string().contains("duplicate"),
        "error should mention duplicate; got {err}"
    );
}
