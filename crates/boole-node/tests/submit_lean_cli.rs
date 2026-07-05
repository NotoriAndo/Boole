use boole_lean_runner::{LeanRunner, LeanRunnerConfig};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn submit_lean_cli_accepts_valid_proof_into_replayable_block() {
    if !lake_and_lean_available() {
        eprintln!("skipping submit-lean CLI test: lake/lean unavailable");
        return;
    }
    let repo_root = repo_root();
    let fixture_path = repo_root.join("fixtures/protocol/admission/v1.json");
    let workspace = TestLeanWorkspace::new("submit-lean-valid");
    workspace.write_checker_project();
    let proof = workspace.write_proof(
        "ValidSubmitLean.lean",
        r#"theorem boole_submit_lean_valid : 2 + 2 = 4 := by
  decide
"#,
    );
    let expected_artifact_hash = checker_artifact_hash(&workspace, "submit-lean-cli-test-verifier");
    let block_path = workspace.root.join("blockstore.ndjson");

    let output = Command::new(env!("CARGO_BIN_EXE_boole-node"))
        .args([
            "submit-lean",
            "--proof",
            proof.to_str().expect("proof path utf8"),
            "--checker-dir",
            workspace.root.to_str().expect("checker dir utf8"),
            "--fixture",
            fixture_path.to_str().expect("fixture path utf8"),
            "--block-store",
            block_path.to_str().expect("block path utf8"),
            "--verifier-hash",
            "submit-lean-cli-test-verifier",
            "--require-checker-artifact-hash",
            &expected_artifact_hash,
            "--difficulty-mode",
            "preflight-easy",
        ])
        .output()
        .expect("run boole-node submit-lean");
    assert!(
        output.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        output.stderr.is_empty(),
        "successful submit-lean must keep stderr empty: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let parsed: Value = serde_json::from_slice(&output.stdout).expect("json stdout");
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["command"], "submit-lean");
    assert_eq!(parsed["accepted"], true);
    assert_eq!(parsed["lean"]["accepted"], true);
    assert_eq!(parsed["shareAccepted"], true);
    assert_eq!(parsed["block"]["height"], 0);
    assert_eq!(parsed["replayMatchesRuntime"], true);
    assert_eq!(parsed["invalidAccepted"], 0);
    assert_eq!(parsed["canonTag"], 0);
    assert_eq!(parsed["submissionBody"]["c"], parsed["block"]["prevC"]);
    assert_eq!(parsed["submissionBody"]["bytes"], parsed["packageBytes"]);
    assert!(parsed["submissionBody"]["pk"].as_str().is_some());
    assert!(parsed["submissionBody"]["nonceS"].as_str().is_some());
    assert_eq!(
        parsed["blockStorePath"].as_str(),
        Some(block_path.to_string_lossy().as_ref())
    );
}

#[test]
fn submit_lean_cli_uses_fixture_block_difficulty_by_default() {
    if !lake_and_lean_available() {
        eprintln!("skipping submit-lean CLI difficulty test: lake/lean unavailable");
        return;
    }
    let repo_root = repo_root();
    let fixture_path = repo_root.join("fixtures/protocol/admission/v1.json");
    let workspace = TestLeanWorkspace::new("submit-lean-fixture-difficulty");
    workspace.write_checker_project();
    let proof = workspace.write_proof(
        "ValidNoBlockAtFixtureDifficulty.lean",
        r#"theorem boole_submit_lean_valid_no_block : 2 + 2 = 4 := by
  decide
"#,
    );
    let expected_artifact_hash = checker_artifact_hash(&workspace, "submit-lean-cli-test-verifier");
    let block_path = workspace.root.join("blockstore.ndjson");

    let output = Command::new(env!("CARGO_BIN_EXE_boole-node"))
        .args([
            "submit-lean",
            "--proof",
            proof.to_str().expect("proof path utf8"),
            "--checker-dir",
            workspace.root.to_str().expect("checker dir utf8"),
            "--fixture",
            fixture_path.to_str().expect("fixture path utf8"),
            "--block-store",
            block_path.to_str().expect("block path utf8"),
            "--verifier-hash",
            "submit-lean-cli-test-verifier",
            "--require-checker-artifact-hash",
            &expected_artifact_hash,
        ])
        .output()
        .expect("run boole-node submit-lean at fixture difficulty");

    assert!(
        output.status.success(),
        "fixture-difficulty valid share must be reported, not crash: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(output.stderr.is_empty());
    let parsed: Value = serde_json::from_slice(&output.stdout).expect("json stdout");
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["accepted"], true);
    assert_eq!(parsed["shareAccepted"], true);
    assert_eq!(parsed["blockProduced"], false);
    assert_eq!(parsed["block"], Value::Null);
    assert_eq!(parsed["replayMatchesRuntime"], true);
    assert_eq!(parsed["invalidAccepted"], 0);
    assert!(
        !block_path.exists(),
        "share below actual block difficulty must not append a block"
    );
}

#[test]
fn submit_lean_cli_rejects_invalid_proof_as_json_stderr_before_admission() {
    if !lake_and_lean_available() {
        eprintln!("skipping submit-lean CLI test: lake/lean unavailable");
        return;
    }
    let repo_root = repo_root();
    let fixture_path = repo_root.join("fixtures/protocol/admission/v1.json");
    let workspace = TestLeanWorkspace::new("submit-lean-invalid");
    workspace.write_checker_project();
    let proof = workspace.write_proof(
        "InvalidSubmitLean.lean",
        r#"theorem boole_submit_lean_invalid : 2 + 2 = 5 := by
  decide
"#,
    );
    let expected_artifact_hash = checker_artifact_hash(&workspace, "submit-lean-cli-test-verifier");
    let block_path = workspace.root.join("blockstore.ndjson");

    let output = Command::new(env!("CARGO_BIN_EXE_boole-node"))
        .args([
            "submit-lean",
            "--proof",
            proof.to_str().expect("proof path utf8"),
            "--checker-dir",
            workspace.root.to_str().expect("checker dir utf8"),
            "--fixture",
            fixture_path.to_str().expect("fixture path utf8"),
            "--block-store",
            block_path.to_str().expect("block path utf8"),
            "--verifier-hash",
            "submit-lean-cli-test-verifier",
            "--require-checker-artifact-hash",
            &expected_artifact_hash,
        ])
        .output()
        .expect("run boole-node submit-lean invalid");
    assert!(
        !output.status.success(),
        "invalid Lean proof must exit non-zero stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "failed submit-lean must keep stdout empty: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    let parsed: Value = serde_json::from_slice(&output.stderr).expect("json stderr");
    assert_eq!(parsed["ok"], false);
    assert_eq!(parsed["command"], "submit-lean");
    assert_eq!(parsed["accepted"], false);
    assert_eq!(parsed["error"], "lean_rejected");
    assert_eq!(parsed["lean"]["accepted"], false);
    assert_eq!(parsed["shareAccepted"], false);
    assert_eq!(parsed["blockProduced"], false);
    assert_eq!(parsed["invalidAccepted"], 0);
    assert!(
        !block_path.exists(),
        "invalid proof must not create a block store"
    );
}

#[test]
fn submit_lean_cli_rejects_missing_checker_artifact_hash_as_json_stderr() {
    if !lake_and_lean_available() {
        eprintln!("skipping submit-lean CLI missing policy test: lake/lean unavailable");
        return;
    }
    let repo_root = repo_root();
    let fixture_path = repo_root.join("fixtures/protocol/admission/v1.json");
    let workspace = TestLeanWorkspace::new("submit-lean-missing-policy");
    workspace.write_checker_project();
    let proof = workspace.write_proof(
        "ValidMissingPolicy.lean",
        r#"theorem boole_submit_lean_missing_policy : 2 + 2 = 4 := by
  decide
"#,
    );
    let block_path = workspace.root.join("blockstore.ndjson");

    let output = Command::new(env!("CARGO_BIN_EXE_boole-node"))
        .args([
            "submit-lean",
            "--proof",
            proof.to_str().expect("proof path utf8"),
            "--checker-dir",
            workspace.root.to_str().expect("checker dir utf8"),
            "--fixture",
            fixture_path.to_str().expect("fixture path utf8"),
            "--block-store",
            block_path.to_str().expect("block path utf8"),
            "--verifier-hash",
            "submit-lean-cli-test-verifier",
        ])
        .output()
        .expect("run boole-node submit-lean missing checker policy");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let parsed: Value = serde_json::from_slice(&output.stderr).expect("json stderr");
    assert_eq!(parsed["ok"], false);
    assert_eq!(parsed["command"], "submit-lean");
    assert_eq!(parsed["error"], "missing_checker_artifact_policy");
    assert_eq!(parsed["shareAccepted"], false);
    assert_eq!(parsed["blockProduced"], false);
    assert_eq!(parsed["invalidAccepted"], 0);
    assert!(!block_path.exists());
}

#[test]
fn submit_lean_cli_rejects_checker_artifact_not_in_required_allowlist() {
    if !lake_and_lean_available() {
        eprintln!("skipping submit-lean CLI artifact guard test: lake/lean unavailable");
        return;
    }
    let repo_root = repo_root();
    let fixture_path = repo_root.join("fixtures/protocol/admission/v1.json");
    let workspace = TestLeanWorkspace::new("submit-lean-artifact-guard");
    workspace.write_checker_project();
    let proof = workspace.write_proof(
        "ValidButTamperedChecker.lean",
        r#"theorem boole_submit_lean_tampered_checker : 3 + 3 = 6 := by
  decide
"#,
    );
    let expected_artifact_hash = LeanRunner::new(
        LeanRunnerConfig::new("submit-lean-cli-test-verifier")
            .with_package_dir(workspace.root.clone()),
    )
    .evidence()
    .expect("baseline checker evidence")
    .checker_artifact_hash;
    workspace.write_checker_project_with_main(
        r#"def main (_args : List String) : IO UInt32 := do
  IO.println "tampered checker accepts without checking proof"
  return 0
"#,
    );
    let block_path = workspace.root.join("blockstore.ndjson");

    let output = Command::new(env!("CARGO_BIN_EXE_boole-node"))
        .args([
            "submit-lean",
            "--proof",
            proof.to_str().expect("proof path utf8"),
            "--checker-dir",
            workspace.root.to_str().expect("checker dir utf8"),
            "--fixture",
            fixture_path.to_str().expect("fixture path utf8"),
            "--block-store",
            block_path.to_str().expect("block path utf8"),
            "--verifier-hash",
            "submit-lean-cli-test-verifier",
            "--require-checker-artifact-hash",
            &expected_artifact_hash,
        ])
        .output()
        .expect("run boole-node submit-lean with artifact guard");
    assert!(
        !output.status.success(),
        "tampered checker artifact must exit non-zero stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "failed submit-lean must keep stdout empty: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    let parsed: Value = serde_json::from_slice(&output.stderr).expect("json stderr");
    assert_eq!(parsed["ok"], false);
    assert_eq!(parsed["command"], "submit-lean");
    assert_eq!(parsed["accepted"], false);
    assert_eq!(parsed["error"], "lean_artifact_not_allowed");
    assert_eq!(parsed["lean"]["accepted"], true);
    assert_eq!(parsed["shareAccepted"], false);
    assert_eq!(parsed["blockProduced"], false);
    assert_eq!(parsed["invalidAccepted"], 0);
    assert!(
        !block_path.exists(),
        "artifact-guard rejection must not create a block store"
    );
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn checker_artifact_hash(workspace: &TestLeanWorkspace, verifier_hash: &str) -> String {
    LeanRunner::new(LeanRunnerConfig::new(verifier_hash).with_package_dir(workspace.root.clone()))
        .evidence()
        .expect("checker evidence")
        .checker_artifact_hash
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
            "boole-{name}-{}-{}",
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
        // TB.1 / ADR-0013 — `check_file` now runs a second, separate
        // process (`lake env lean --run BooleCheck/Audit.lean`) after the
        // primary checker accepts, so every synthetic fixture package needs
        // its own copy of the real audit script or that stage fails to spawn.
        // `include_str!` pulls the production file in verbatim at compile
        // time so the fixture can never drift from what actually ships.
        std::fs::write(
            self.root.join("BooleCheck/Audit.lean"),
            include_str!("../../../lean/checker/BooleCheck/Audit.lean"),
        )
        .expect("write axiom audit script");
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

#[allow(dead_code)]
fn assert_path_exists(path: &Path) {
    assert!(path.exists(), "expected path to exist: {}", path.display());
}
