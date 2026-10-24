#[test]
#[cfg(not(feature = "qualification-faults"))]
fn ordinary_binary_refuses_every_fault_selector_before_startup() {
    for key in [
        "BOOLE_M4_KILL_POINT",
        "BOOLE_M4_KILL_HEIGHT",
        "BOOLE_M4_KILL_ROLE",
        "BOOLE_M4_KILL_MARKER",
        "BOOLE_M4_KILL_UNKNOWN",
    ] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_boole-devnet-mcp"))
            .arg("--help")
            .env(key, "fixture")
            .output()
            .unwrap();
        assert!(!output.status.success(), "ordinary binary accepted {key}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("fault-hooks"));
    }
}
