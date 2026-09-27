//! Keep spawned native fixtures on the same guarded test profile as the parent.
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;
use std::process::{Command, Stdio};

pub fn build(cli: &Path) {
    let output = Command::new(env!("CARGO"))
        .args([
            "build",
            "--profile",
            "test",
            "--locked",
            "--message-format=json",
            "-p",
            "boole-node",
            "-p",
            "boole-wallet-agent",
        ])
        .stderr(Stdio::inherit())
        .output()
        .unwrap();
    assert!(output.status.success(), "native fixture build failed");
    let mut libraries = BTreeSet::new();
    let mut executables = BTreeSet::new();
    let mut finished = false;
    for line in output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let record: Value = serde_json::from_slice(line).expect("Cargo JSON record");
        if record["reason"] == "build-finished" {
            assert_eq!(record["success"], true);
            finished = true;
        }
        if record["reason"] != "compiler-artifact" {
            continue;
        }
        let name = record["target"]["name"].as_str().unwrap();
        let kinds = record["target"]["kind"].as_array().unwrap();
        let expected = if kinds.iter().any(|kind| kind == "lib") {
            match name {
                "curve25519_dalek" | "argon2" => Some("3"),
                "boole_core" | "boole_node" => Some("0"),
                _ => None,
            }
        } else if kinds.iter().any(|kind| kind == "bin")
            && matches!(name, "boole-node" | "boole-wallet-agent")
        {
            let executable = Path::new(record["executable"].as_str().unwrap());
            assert_eq!(executable, cli.parent().unwrap().join(name));
            executables.insert(name.to_owned());
            Some("0")
        } else {
            None
        };
        if let Some(expected) = expected {
            let profile = &record["profile"];
            assert_eq!(profile["opt_level"], expected, "fixture profile for {name}");
            assert_eq!(profile["debug_assertions"], true, "debug guards for {name}");
            assert_eq!(
                profile["overflow_checks"], true,
                "overflow guards for {name}"
            );
            if kinds.iter().any(|kind| kind == "lib") {
                libraries.insert(name.to_owned());
            }
        }
    }
    assert!(finished, "missing Cargo build-finished record");
    assert_eq!(
        libraries.len(),
        4,
        "missing native fixture library artifacts"
    );
    assert_eq!(
        executables.len(),
        2,
        "missing native fixture executable artifacts"
    );
}
