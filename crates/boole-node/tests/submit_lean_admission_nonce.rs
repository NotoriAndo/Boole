//! S(B2) — `boole-node submit-lean --admission-nonce <hex32>` override.
//!
//! For benchmarks run without a live node (CI, smoke), the operator
//! overrides the fixture's admission nonce `n` so back-to-back runs
//! produce diverse `share_hash` values without spinning up a node.
//! Validation (`len == 64 && all ascii_hexdigit`) happens before
//! fixture parse + Lean spawn, so a malformed value fails fast and
//! never pays for `lake exec boole_check`.

use boole_lean_runner::{LeanRunner, LeanRunnerConfig};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn submit_lean_rejects_malformed_admission_nonce_before_lean() {
    // No lake/lean gate — the validation must short-circuit before any
    // Lean tooling is touched. This test would still exit 1 cleanly on
    // a host with no lake installed at all.
    let workspace = TestLeanWorkspace::new("submit-lean-malformed-nonce");
    let proof = workspace.write_proof(
        "Trivial.lean",
        "theorem boole_admission_nonce_trivial : 2 + 2 = 4 := by\n  decide\n",
    );
    let block_path = workspace.root.join("blockstore.ndjson");
    let fixture_path = repo_root().join("fixtures/protocol/admission/v1.json");

    for nonce in ["tooshort", &"A".repeat(64)] {
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
                "--require-checker-artifact-hash",
                "ignored-because-validation-runs-first",
                "--admission-nonce",
                nonce,
            ])
            .output()
            .expect("run boole-node submit-lean");
        assert!(
            !output.status.success(),
            "expected non-zero exit for {nonce}"
        );
        assert!(
            output.stdout.is_empty(),
            "rejected submit-lean must keep stdout empty: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        let parsed: Value = serde_json::from_str(stderr.trim())
            .unwrap_or_else(|err| panic!("expected stderr JSON, got {stderr:?} ({err})"));
        assert_eq!(parsed["ok"], false);
        assert_eq!(parsed["command"], "submit-lean");
        assert_eq!(parsed["accepted"], false);
        assert_eq!(parsed["error"], "malformed-admission-nonce");
        assert_eq!(parsed["shareAccepted"], false);
        assert_eq!(parsed["blockProduced"], false);
        assert_eq!(parsed["invalidAccepted"], 0);
    }
    // The block store must NOT exist — we bailed before any I/O.
    assert!(
        !block_path.exists(),
        "validation must short-circuit before block store touch"
    );
}

#[test]
fn submit_lean_admission_nonce_changes_submission_body_n_and_share_hash() {
    if !lake_and_lean_available() {
        eprintln!("skipping admission-nonce override test: lake/lean unavailable");
        return;
    }
    let nonce_a = "1111111111111111111111111111111111111111111111111111111111111111";
    let nonce_b = "2222222222222222222222222222222222222222222222222222222222222222";

    let parsed_a = run_submit_lean_with_nonce("admission-nonce-override-a", Some(nonce_a));
    let parsed_b = run_submit_lean_with_nonce("admission-nonce-override-b", Some(nonce_b));

    assert_eq!(parsed_a["ok"], true);
    assert_eq!(parsed_b["ok"], true);
    assert_eq!(parsed_a["submissionBody"]["n"], nonce_a);
    assert_eq!(parsed_b["submissionBody"]["n"], nonce_b);
    let hash_a = parsed_a["shareHash"]
        .as_str()
        .expect("shareHash for run a")
        .to_string();
    let hash_b = parsed_b["shareHash"]
        .as_str()
        .expect("shareHash for run b")
        .to_string();
    assert_ne!(
        hash_a, hash_b,
        "different admission-nonce inputs must yield different share_hash outputs"
    );
}

#[test]
fn submit_lean_admission_nonce_default_uses_fixture_value() {
    if !lake_and_lean_available() {
        eprintln!("skipping admission-nonce default test: lake/lean unavailable");
        return;
    }
    let parsed = run_submit_lean_with_nonce("admission-nonce-default-fixture", None);
    assert_eq!(parsed["ok"], true);
    let fixture_text =
        std::fs::read_to_string(repo_root().join("fixtures/protocol/admission/v1.json"))
            .expect("read admission fixture");
    let fixture: Value = serde_json::from_str(&fixture_text).expect("admission fixture json");
    assert_eq!(
        parsed["submissionBody"]["n"], fixture["constants"]["n"],
        "no override → submissionBody.n must match fixture.constants.n byte-for-byte"
    );
}

fn run_submit_lean_with_nonce(tag: &str, nonce: Option<&str>) -> Value {
    let workspace = TestLeanWorkspace::new(tag);
    workspace.write_checker_project();
    let proof = workspace.write_proof(
        "ValidNonceRun.lean",
        "theorem boole_admission_nonce_run : 2 + 2 = 4 := by\n  decide\n",
    );
    let expected_artifact_hash =
        checker_artifact_hash(&workspace, "submit-lean-admission-nonce-test-verifier");
    let block_path = workspace.root.join("blockstore.ndjson");

    let proof_str = proof.to_str().expect("proof utf8");
    let checker_dir = workspace.root.to_str().expect("checker utf8");
    let fixture_path = repo_root().join("fixtures/protocol/admission/v1.json");
    let fixture_str = fixture_path.to_str().expect("fixture utf8");
    let block_str = block_path.to_str().expect("block utf8");

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_boole-node"));
    cmd.args([
        "submit-lean",
        "--proof",
        proof_str,
        "--checker-dir",
        checker_dir,
        "--fixture",
        fixture_str,
        "--block-store",
        block_str,
        "--verifier-hash",
        "submit-lean-admission-nonce-test-verifier",
        "--require-checker-artifact-hash",
        &expected_artifact_hash,
        "--difficulty-mode",
        "preflight-easy",
    ]);
    if let Some(value) = nonce {
        cmd.args(["--admission-nonce", value]);
    }
    let output = cmd.output().expect("run boole-node submit-lean");
    assert!(
        output.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    serde_json::from_slice(&output.stdout).expect("stdout json")
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

#[allow(dead_code)]
fn assert_path_exists(path: &Path) {
    assert!(path.exists(), "expected path to exist: {}", path.display());
}
