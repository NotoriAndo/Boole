use boole_devnet_audit::{
    mark_mismatch,
    replay::{report_cleanup, report_failure, Failure},
};
use serde_json::{json, Value};
fn receipt() -> Value {
    json!({"first_mismatch":null,"checks":{"prefix":"ok","fold":"ok","execution":"ok"},"replayed_blocks":"7"})
}
#[test]
fn l6_cleanup_failure_reports_the_completed_height_and_its_own_class() {
    for (stopped, issues) in [(false, 0), (true, 1)] {
        let mut r = receipt();
        report_cleanup(&mut r, stopped, issues);
        assert_eq!(
            r["first_mismatch"],
            json!({"height":"7","class":"engine-cleanup"})
        );
        assert_eq!(r["checks"]["engine-cleanup"], "mismatch");
        assert_eq!(r["checks"]["prefix"], "ok");
        assert_eq!(r["checks"]["execution"], "mismatch");
    }
}
#[test]
fn l6_cleanup_never_overwrites_an_earlier_replay_mismatch() {
    let mut r = receipt();
    mark_mismatch(&mut r, 3, "prefix");
    report_cleanup(&mut r, false, 0);
    assert_eq!(r["first_mismatch"], json!({"height":"3","class":"prefix"}));
    assert_eq!(r["checks"]["prefix"], "mismatch");
    assert_eq!(r["checks"]["engine-cleanup"], "mismatch");
}
#[test]
fn l6_setup_input_errors_keep_their_height_without_exposing_source_details() {
    let mut r = receipt();
    let error = boole_devnet_archive::error::integrity(4, "input", "private replay error canary");
    report_failure(&mut r, &error, "unavailable");
    assert_eq!(r["first_mismatch"], json!({"height":"4","class":"input"}));
    assert_eq!(r["checks"]["input"], "mismatch");
    assert!(!r.to_string().contains("private replay error canary"));
}
#[test]
fn l6_replay_errors_keep_the_typed_height_and_update_the_named_check() {
    let mut r = receipt();
    let error = anyhow::Error::new(Failure {
        height: 5,
        class: "prefix",
    });
    report_failure(&mut r, &error, "mismatch");
    assert_eq!(r["first_mismatch"], json!({"height":"5","class":"prefix"}));
    assert_eq!(r["checks"]["prefix"], "mismatch");
}

#[test]
fn b2_replay_resource_stop_keeps_its_height_without_first_mismatch() {
    let mut receipt = receipt();
    let error = anyhow::Error::new(boole_devnet_audit::resource::ResourceStop {
        resource: "rpc-bytes",
        limit: 16 * 1024 * 1024,
    })
    .context(boole_devnet_audit::replay::Failure {
        height: 12,
        class: "execution",
    });
    boole_devnet_audit::replay::report_failure(&mut receipt, &error, "unavailable");
    assert!(receipt["first_mismatch"].is_null());
    assert_eq!(receipt["resource_stop"]["height"], "12");
    assert_eq!(receipt["checks"]["execution"], "resource-stop");
}
