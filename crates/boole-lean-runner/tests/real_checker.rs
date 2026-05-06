use boole_lean_runner::{LeanRunner, LeanRunnerConfig};
use std::path::PathBuf;
use std::process::Command;

#[test]
fn lake_exec_checker_accepts_valid_lean_file_with_evidence() {
    if !lake_and_lean_available() {
        eprintln!("skipping real Lean runner test: lake/lean unavailable");
        return;
    }
    let workspace = TestLeanWorkspace::new("valid");
    workspace.write_checker_project();
    let proof = workspace.write_proof(
        "ValidProof.lean",
        r#"theorem boole_valid : 1 + 1 = 2 := by
  decide
"#,
    );

    let runner = LeanRunner::new(
        LeanRunnerConfig::new("fixture-verifier-hash")
            .with_package_dir(workspace.root.clone())
            .with_timeout_ms(5_000)
            .with_memory_limit_mb(256),
    );

    let result = runner.check_file(&proof).expect("checker runs");
    assert!(result.accepted, "valid proof should pass: {result:?}");
    assert_eq!(result.evidence.verifier_hash, "fixture-verifier-hash");
    assert_eq!(result.evidence.checker, "lake exec boole_check");
    assert_eq!(result.evidence.checker_artifact_hash.len(), 64);
    assert!(
        result
            .evidence
            .checker_artifact_hash
            .chars()
            .all(|c| c.is_ascii_hexdigit()),
        "checker artifact hash should be hex"
    );
    assert!(
        result.evidence.lean_version.starts_with("Lean"),
        "lean version evidence should be captured: {:?}",
        result.evidence.lean_version
    );
    assert_eq!(result.evidence.timeout_ms, 5_000);
    assert_eq!(result.evidence.memory_limit_mb, 256);
}

#[test]
fn lake_exec_checker_rejects_invalid_lean_file_without_panicking() {
    if !lake_and_lean_available() {
        eprintln!("skipping real Lean runner test: lake/lean unavailable");
        return;
    }
    let workspace = TestLeanWorkspace::new("invalid");
    workspace.write_checker_project();
    let proof = workspace.write_proof(
        "InvalidProof.lean",
        r#"theorem boole_invalid : 1 + 1 = 3 := by
  decide
"#,
    );

    let runner = LeanRunner::new(
        LeanRunnerConfig::new("fixture-verifier-hash")
            .with_package_dir(workspace.root.clone())
            .with_timeout_ms(5_000)
            .with_memory_limit_mb(256),
    );

    let result = runner.check_file(&proof).expect("checker returns envelope");
    assert!(!result.accepted, "invalid proof should reject");
    assert_ne!(result.exit_code, 0);
    let rejection_output = format!("{}{}", result.stdout, result.stderr);
    assert!(
        rejection_output.contains("failed") || rejection_output.contains("error"),
        "checker output should carry Lean rejection details: stdout={} stderr={}",
        result.stdout,
        result.stderr
    );
    assert_eq!(result.evidence.checker, "lake exec boole_check");
}

fn lake_and_lean_available() -> bool {
    let lake_ok = Command::new("lake")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    let lean_ok = Command::new("lean")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    lake_ok && lean_ok
}

struct TestLeanWorkspace {
    root: PathBuf,
}

impl TestLeanWorkspace {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "boole-lean-runner-{name}-{}-{}",
            std::process::id(),
            unique_nanos()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("BooleCheck")).expect("create workspace");
        Self { root }
    }

    fn write_checker_project(&self) {
        std::fs::write(
            self.root.join("lakefile.lean"),
            r#"import Lake
open Lake DSL

package boole_check_fixture

lean_exe boole_check where
  root := `BooleCheck.Main
"#,
        )
        .expect("write lakefile");
        std::fs::write(
            self.root.join("BooleCheck/Main.lean"),
            r#"def main (args : List String) : IO UInt32 := do
  let some proofPath := args.head?
    | IO.eprintln "usage: boole_check <proof.lean>"; return 64
  let output ← IO.Process.output {
    cmd := "lean"
    args := #[proofPath]
  }
  if output.stdout.length > 0 then
    IO.print output.stdout
  if output.stderr.length > 0 then
    IO.eprint output.stderr
  if output.exitCode == 0 then
    return 0
  else
    return 1
"#,
        )
        .expect("write checker main");
    }

    fn write_proof(&self, name: &str, content: &str) -> PathBuf {
        let path = self.root.join(name);
        std::fs::write(&path, content).expect("write proof");
        path
    }
}

impl Drop for TestLeanWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn unique_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time")
        .as_nanos()
}
