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
            .with_memory_limit_mb(8192),
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
    assert_eq!(result.evidence.memory_limit_mb, 8192);
}

#[test]
fn lake_exec_checker_times_out_and_returns_rejection_envelope() {
    if !lake_and_lean_available() {
        eprintln!("skipping real Lean runner timeout test: lake/lean unavailable");
        return;
    }
    let workspace = TestLeanWorkspace::new("timeout");
    workspace.write_checker_project_with_main(
        r#"def main (_args : List String) : IO UInt32 := do
  IO.sleep 1000
  IO.println "unexpected completion"
  return 0
"#,
    );
    let proof = workspace.write_proof(
        "ValidProof.lean",
        "theorem trivial : True := by\n  trivial\n",
    );

    let runner = LeanRunner::new(
        LeanRunnerConfig::new("fixture-verifier-hash")
            .with_package_dir(workspace.root.clone())
            .with_timeout_ms(50)
            .with_output_limit_bytes(1024),
    );

    let result = runner.check_file(&proof).expect("timeout returns envelope");
    assert!(!result.accepted, "timed-out check must reject");
    assert!(result.timed_out, "result should record timeout: {result:?}");
    assert_eq!(result.exit_code, -1);
    assert!(
        result.stderr.contains("timeout"),
        "timeout rejection should be visible in stderr: {:?}",
        result.stderr
    );
}

#[test]
fn lake_exec_checker_caps_captured_output_and_marks_truncation() {
    if !lake_and_lean_available() {
        eprintln!("skipping real Lean runner output-cap test: lake/lean unavailable");
        return;
    }
    let workspace = TestLeanWorkspace::new("output-cap");
    workspace.write_checker_project_with_main(
        r#"partial def repeatPrint : Nat -> IO Unit
  | 0 => pure ()
  | n + 1 => do
    IO.print "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
    repeatPrint n

def main (_args : List String) : IO UInt32 := do
  repeatPrint 64
  return 1
"#,
    );
    let proof = workspace.write_proof(
        "ValidProof.lean",
        "theorem trivial : True := by\n  trivial\n",
    );

    let runner = LeanRunner::new(
        LeanRunnerConfig::new("fixture-verifier-hash")
            .with_package_dir(workspace.root.clone())
            .with_timeout_ms(5_000)
            .with_output_limit_bytes(256),
    );

    let result = runner.check_file(&proof).expect("checker returns envelope");
    assert!(!result.accepted, "non-zero checker should reject");
    assert!(
        result.output_truncated,
        "result should record output truncation: {result:?}"
    );
    assert!(
        result.stdout.len() <= 256,
        "stdout must be capped instead of captured unboundedly: len={} stdout={:?}",
        result.stdout.len(),
        result.stdout
    );
    assert_eq!(result.evidence.output_limit_bytes, 256);
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
            .with_memory_limit_mb(8192),
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
        self.write_checker_project_with_main(
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
        );
    }

    fn write_checker_project_with_main(&self, main_lean: &str) {
        std::fs::write(
            self.root.join("lean-toolchain"),
            "leanprover/lean4:v4.29.1\n",
        )
        .expect("write lean-toolchain");
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
            self.root.join("lake-manifest.json"),
            r#"{"version": "1.1.0",
 "packagesDir": ".lake/packages",
 "packages": [],
 "name": "boole_check_fixture",
 "lakeDir": ".lake"}
"#,
        )
        .expect("write lake-manifest");
        std::fs::write(self.root.join("BooleCheck/Main.lean"), main_lean)
            .expect("write checker main");
        std::fs::create_dir_all(self.root.join("Boole/Family")).expect("create Boole/Family");
        std::fs::write(
            self.root.join("Boole/Family/V0Helpers.lean"),
            "-- fixture stub: pinned by checker_artifact_hash\n",
        )
        .expect("write V0Helpers stub");
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
