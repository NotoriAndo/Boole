use std::io::Write;

use boole_core::SessionState;
use boole_node::FileSessionStore;

#[test]
fn session_store_recovers_and_truncates_partial_trailing_line_after_crash() {
    let session = SessionState::test_fixture();

    let dir = std::env::temp_dir().join(format!(
        "boole-node-session-partial-recovery-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let session_path = dir.join("sessionstore.ndjson");

    let mut writer = FileSessionStore::default();
    writer
        .append_register(&session_path, &session, session.activation_height)
        .expect("append complete register");

    let stable_len = std::fs::metadata(&session_path).expect("metadata").len();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&session_path)
        .expect("open ledger")
        .write_all(br#"{"kind":"register","session":{"sessionPk":"trunc"#)
        .expect("write partial trailing line");

    let recovered = FileSessionStore::recover(&session_path).expect("recover ignores torn tail");
    assert_eq!(recovered.size(), 1);
    assert!(recovered.get(&session.session_pk).is_some());
    assert_eq!(
        std::fs::metadata(&session_path)
            .expect("metadata after recovery")
            .len(),
        stable_len,
        "recovery must truncate the torn trailing line so restart is idempotent"
    );

    let recovered_again =
        FileSessionStore::recover(&session_path).expect("second recover stays clean");
    assert_eq!(recovered_again.size(), 1);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn session_store_rejects_complete_corrupt_line_instead_of_truncating_history() {
    let session = SessionState::test_fixture();

    let dir = std::env::temp_dir().join(format!(
        "boole-node-session-complete-corrupt-line-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let session_path = dir.join("sessionstore.ndjson");

    let mut writer = FileSessionStore::default();
    writer
        .append_register(&session_path, &session, session.activation_height)
        .expect("append complete register");
    std::fs::OpenOptions::new()
        .append(true)
        .open(&session_path)
        .expect("open ledger")
        .write_all(b"not-json\n")
        .expect("write complete corrupt line");

    let err =
        FileSessionStore::recover(&session_path).expect_err("complete corrupt lines must fail");
    assert!(
        err.to_string().contains("invalid JSON"),
        "unexpected error: {err}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
