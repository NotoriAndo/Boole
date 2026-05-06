use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn agent_proof_fixture_valid_emits_untrusted_candidate_that_submit_lean_accepts() {
    if !lake_and_lean_available() {
        eprintln!("skipping agent-proof CLI test: lake/lean unavailable");
        return;
    }
    let repo_root = repo_root();
    let fixture_path = repo_root.join("fixtures/protocol/admission/v1.json");
    let workspace = TestLeanWorkspace::new("agent-proof-valid");
    workspace.write_checker_project();
    let out_dir = workspace.root.join("candidate-out");

    let candidate_output = Command::new(env!("CARGO_BIN_EXE_boole-node"))
        .args([
            "agent-proof",
            "--backend",
            "fixture-valid",
            "--out-dir",
            out_dir.to_str().expect("out dir utf8"),
        ])
        .output()
        .expect("run boole-node agent-proof fixture-valid");
    assert!(
        candidate_output.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&candidate_output.stderr),
        String::from_utf8_lossy(&candidate_output.stdout)
    );
    assert!(
        candidate_output.stderr.is_empty(),
        "successful agent-proof must keep stderr empty: {}",
        String::from_utf8_lossy(&candidate_output.stderr)
    );

    let candidate: Value =
        serde_json::from_slice(&candidate_output.stdout).expect("candidate json stdout");
    assert_eq!(candidate["ok"], true);
    assert_eq!(candidate["command"], "agent-proof");
    assert_eq!(candidate["backend"], "fixture-valid");
    assert_eq!(candidate["agentProofCandidate"], true);
    assert_eq!(candidate["trusted"], false);
    assert_eq!(candidate["proofFormat"], "lean");
    assert_eq!(candidate["consensusAccepted"], false);
    let proof_path = candidate["proofPath"]
        .as_str()
        .expect("candidate proof path string");
    assert!(Path::new(proof_path).exists(), "candidate proof exists");
    let proof = std::fs::read_to_string(proof_path).expect("read candidate proof");
    assert!(
        proof.contains("theorem boole_agent_fixture_valid"),
        "candidate proof should be the fixture-valid theorem: {proof}"
    );

    let block_path = workspace.root.join("blockstore.ndjson");
    let submit_output = Command::new(env!("CARGO_BIN_EXE_boole-node"))
        .args([
            "submit-lean",
            "--proof",
            proof_path,
            "--checker-dir",
            workspace.root.to_str().expect("checker dir utf8"),
            "--fixture",
            fixture_path.to_str().expect("fixture path utf8"),
            "--block-store",
            block_path.to_str().expect("block path utf8"),
            "--verifier-hash",
            "agent-proof-cli-test-verifier",
        ])
        .output()
        .expect("run boole-node submit-lean on agent candidate");
    assert!(
        submit_output.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&submit_output.stderr),
        String::from_utf8_lossy(&submit_output.stdout)
    );
    let submit: Value = serde_json::from_slice(&submit_output.stdout).expect("submit json stdout");
    assert_eq!(submit["ok"], true);
    assert_eq!(submit["accepted"], true);
    assert_eq!(submit["shareAccepted"], true);
    assert_eq!(submit["block"]["height"], 0);
    assert_eq!(submit["replayMatchesRuntime"], true);
    assert_eq!(submit["invalidAccepted"], 0);
}

#[test]
fn agent_proof_fixture_invalid_stays_untrusted_and_is_rejected_before_admission() {
    if !lake_and_lean_available() {
        eprintln!("skipping agent-proof CLI test: lake/lean unavailable");
        return;
    }
    let repo_root = repo_root();
    let fixture_path = repo_root.join("fixtures/protocol/admission/v1.json");
    let workspace = TestLeanWorkspace::new("agent-proof-invalid");
    workspace.write_checker_project();
    let out_dir = workspace.root.join("candidate-out");

    let candidate_output = Command::new(env!("CARGO_BIN_EXE_boole-node"))
        .args([
            "agent-proof",
            "--backend",
            "fixture-invalid",
            "--out-dir",
            out_dir.to_str().expect("out dir utf8"),
        ])
        .output()
        .expect("run boole-node agent-proof fixture-invalid");
    assert!(
        candidate_output.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&candidate_output.stderr),
        String::from_utf8_lossy(&candidate_output.stdout)
    );

    let candidate: Value =
        serde_json::from_slice(&candidate_output.stdout).expect("candidate json stdout");
    assert_eq!(candidate["ok"], true);
    assert_eq!(candidate["backend"], "fixture-invalid");
    assert_eq!(candidate["agentProofCandidate"], true);
    assert_eq!(candidate["trusted"], false);
    assert_eq!(candidate["consensusAccepted"], false);
    let proof_path = candidate["proofPath"]
        .as_str()
        .expect("candidate proof path string");
    assert!(Path::new(proof_path).exists(), "candidate proof exists");

    let block_path = workspace.root.join("blockstore.ndjson");
    let submit_output = Command::new(env!("CARGO_BIN_EXE_boole-node"))
        .args([
            "submit-lean",
            "--proof",
            proof_path,
            "--checker-dir",
            workspace.root.to_str().expect("checker dir utf8"),
            "--fixture",
            fixture_path.to_str().expect("fixture path utf8"),
            "--block-store",
            block_path.to_str().expect("block path utf8"),
            "--verifier-hash",
            "agent-proof-cli-test-verifier",
        ])
        .output()
        .expect("run boole-node submit-lean invalid agent candidate");
    assert!(
        !submit_output.status.success(),
        "invalid candidate must exit non-zero stdout={} stderr={}",
        String::from_utf8_lossy(&submit_output.stdout),
        String::from_utf8_lossy(&submit_output.stderr)
    );
    assert!(
        submit_output.stdout.is_empty(),
        "failed submit-lean must keep stdout empty: {}",
        String::from_utf8_lossy(&submit_output.stdout)
    );
    let submit: Value = serde_json::from_slice(&submit_output.stderr).expect("submit json stderr");
    assert_eq!(submit["ok"], false);
    assert_eq!(submit["accepted"], false);
    assert_eq!(submit["error"], "lean_rejected");
    assert_eq!(submit["shareAccepted"], false);
    assert_eq!(submit["blockProduced"], false);
    assert_eq!(submit["invalidAccepted"], 0);
    assert!(
        !block_path.exists(),
        "invalid agent proof must not create a block store"
    );
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
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
        std::fs::write(self.root.join("lean-toolchain"), "leanprover/lean4:v4.29.1\n")
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
