use boole_gate_supervisor::codec::Scratch;
#[test]
fn tracked_preparer_reproduces_the_descriptor_and_passes_rust_s16() {
    let root = Scratch::new(&std::env::temp_dir()).unwrap();
    let out = root.0.join("install");
    let binary = env!("CARGO_BIN_EXE_boole-gate-supervisor");
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/devnet/prepare_m3_devnet.py");
    let status = std::process::Command::new("python3")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .arg(script)
        .args(["--gate-binary", binary, "--out"])
        .arg(&out)
        .status()
        .unwrap();
    assert!(status.success());
    let result = std::process::Command::new(binary)
        .arg("check-install")
        .arg(out.join("m3-config.json"))
        .arg(out.join("gates.json"))
        .arg(out.join("forks.json"))
        .arg(out.join("cache"))
        .arg(out.join("artifacts"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let record: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(record["ok"], true);
}
