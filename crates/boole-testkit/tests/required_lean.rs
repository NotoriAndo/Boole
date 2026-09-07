use std::process::Command;

#[test]
fn probe_child() {
    if std::env::var_os("BOOLE_TESTKIT_LEAN_PROBE_CHILD").is_some() {
        println!("available={}", boole_testkit::lake_and_lean_available());
    }
}

#[test]
fn missing_lean_is_a_failure_in_a_required_lane() {
    let output = Command::new(std::env::current_exe().expect("test executable"))
        .args(["--exact", "probe_child", "--nocapture"])
        .env("BOOLE_TESTKIT_LEAN_PROBE_CHILD", "1")
        .env("BOOLE_REQUIRE_LEAN", "1")
        .env("PATH", "")
        .output()
        .expect("run isolated required probe");
    assert!(
        !output.status.success(),
        "a required Lean test must fail, not skip green: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let output_text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output_text.contains("BOOLE_REQUIRE_LEAN"), "{output_text}");
}

#[test]
fn missing_lean_remains_an_explicit_optional_local_skip() {
    let output = Command::new(std::env::current_exe().expect("test executable"))
        .args(["--exact", "probe_child", "--nocapture"])
        .env("BOOLE_TESTKIT_LEAN_PROBE_CHILD", "1")
        .env_remove("BOOLE_REQUIRE_LEAN")
        .env("PATH", "")
        .output()
        .expect("run isolated optional probe");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("available=false"));
}
