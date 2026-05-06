use boole_core::{replay_blocks, AdmissionDecision, CalibrationReport};
use boole_lean_runner::{LeanRunner, LeanRunnerConfig};
use boole_node::block_store::FileBlockStore;
use boole_node::proof_bridge::{LeanProofBridge, LeanProofBridgePolicy, ProofSubmissionTemplate};
use boole_node::runtime::{RuntimeAdmissionState, RuntimeConfig};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    constants: Constants,
    cfg: CalibrationReport,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Constants {
    c: String,
    pk: String,
    n: String,
    j: String,
    nonce_s: String,
    ip: String,
}

#[test]
fn lean_checked_proof_package_is_admitted_as_block_and_replays() {
    if !lake_and_lean_available() {
        eprintln!("skipping Lean proof bridge test: lake/lean unavailable");
        return;
    }
    let fixture = easy_runtime_fixture();
    let workspace = TestLeanWorkspace::new("bridge-valid");
    workspace.write_checker_project();
    let proof = workspace.write_proof(
        "ValidBridgeProof.lean",
        r#"theorem boole_bridge_valid : 1 + 1 = 2 := by
  decide
"#,
    );

    let bridge = LeanProofBridge::new(LeanRunner::new(
        LeanRunnerConfig::new("bridge-verifier-hash")
            .with_package_dir(workspace.root.clone())
            .with_timeout_ms(5_000)
            .with_memory_limit_mb(8192),
    ));
    let template = template_from_fixture(&fixture.constants);
    let bridged = bridge
        .build_submission_body(&proof, &template)
        .expect("valid Lean proof becomes a canonical submission body");
    assert!(bridged.lean.accepted, "Lean checker must accept first");
    assert_eq!(bridged.lean.evidence.verifier_hash, "bridge-verifier-hash");
    assert_ne!(
        bridged.body.get("bytes"),
        Some(&Value::String(String::new())),
        "bridge must emit canonical proof package bytes"
    );

    let config =
        RuntimeConfig::from_calibration_report(fixture.cfg, 60_000).expect("runtime config boots");
    let mut runtime = RuntimeAdmissionState::new(config);
    runtime.set_current_c(fixture.constants.c.clone());
    runtime
        .observe_ticket_from_body(&bridged.body)
        .expect("observe ticket");
    let decision = runtime.admit_body_with_canon_tag(
        1_800_000_000_000,
        &fixture.constants.ip,
        &bridged.body,
        bridged.canon_tag,
    );
    assert!(
        matches!(decision, AdmissionDecision::Accepted { .. }),
        "Lean-backed canonical package should be admitted: {decision:?}"
    );
    assert_eq!(runtime.pool_size(), 1);

    let accepted_tags = BTreeSet::from([bridged.canon_tag]);
    let block = runtime
        .produce_block_for_current_c(0, 1_800_000_000_123, &accepted_tags)
        .expect("admitted Lean-backed share produces block");
    assert_eq!(block.height, 0);
    assert_eq!(block.selected_share_hashes.len(), 1);
    block.validate_shape().expect("block shape is valid");

    let dir = std::env::temp_dir().join(format!(
        "boole-proof-bridge-block-{}-{}",
        std::process::id(),
        unique_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let block_path = dir.join("blockstore.ndjson");
    FileBlockStore::append(&block_path, &block).expect("append block");
    let recovered = FileBlockStore::recover(&block_path).expect("recover block store");
    let replay = replay_blocks(recovered.blocks()).expect("replay succeeds");
    assert_eq!(replay.latest_c, block.c);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn lean_bridge_rejects_checker_artifact_not_in_allowlist_before_submission_body() {
    if !lake_and_lean_available() {
        eprintln!("skipping Lean proof bridge artifact guard test: lake/lean unavailable");
        return;
    }
    let fixture = easy_runtime_fixture();
    let workspace = TestLeanWorkspace::new("bridge-artifact-guard");
    workspace.write_checker_project();
    let proof = workspace.write_proof(
        "ValidBridgeProof.lean",
        r#"theorem boole_bridge_valid : 1 + 1 = 2 := by
  decide
"#,
    );

    let base_config = LeanRunnerConfig::new("bridge-verifier-hash")
        .with_package_dir(workspace.root.clone())
        .with_timeout_ms(5_000)
        .with_memory_limit_mb(8192);
    let expected_artifact_hash = LeanRunner::new(base_config.clone())
        .evidence()
        .expect("evidence hashes baseline checker")
        .checker_artifact_hash;

    workspace.write_checker_project_with_main(
        r#"def main (_args : List String) : IO UInt32 := do
  IO.println "tampered checker accepts without checking proof"
  return 0
"#,
    );

    let bridge = LeanProofBridge::new_with_policy(
        LeanRunner::new(base_config),
        LeanProofBridgePolicy::new()
            .require_verifier_hash("bridge-verifier-hash")
            .allow_checker_artifact_hash(expected_artifact_hash),
    );
    let template = template_from_fixture(&fixture.constants);
    let rejected = bridge
        .build_submission_body(&proof, &template)
        .expect_err("tampered checker artifact must not produce a submission body");
    assert_eq!(rejected.kind(), "lean_artifact_not_allowed");
    assert!(
        rejected.lean().accepted,
        "guard should reject even when the tampered checker exits success"
    );
}

#[test]
fn invalid_lean_proof_is_rejected_before_admission_or_block() {
    if !lake_and_lean_available() {
        eprintln!("skipping Lean proof bridge test: lake/lean unavailable");
        return;
    }
    let fixture = easy_runtime_fixture();
    let workspace = TestLeanWorkspace::new("bridge-invalid");
    workspace.write_checker_project();
    let proof = workspace.write_proof(
        "InvalidBridgeProof.lean",
        r#"theorem boole_bridge_invalid : 1 + 1 = 3 := by
  decide
"#,
    );

    let bridge = LeanProofBridge::new(LeanRunner::new(
        LeanRunnerConfig::new("bridge-verifier-hash")
            .with_package_dir(workspace.root.clone())
            .with_timeout_ms(5_000)
            .with_memory_limit_mb(8192),
    ));
    let template = template_from_fixture(&fixture.constants);
    let rejected = bridge
        .build_submission_body(&proof, &template)
        .expect_err("invalid Lean proof must not produce a submission body");
    assert_eq!(rejected.kind(), "lean_rejected");

    let config =
        RuntimeConfig::from_calibration_report(fixture.cfg, 60_000).expect("runtime config boots");
    let mut runtime = RuntimeAdmissionState::new(config);
    runtime.set_current_c(fixture.constants.c.clone());
    assert_eq!(runtime.pool_size(), 0);
    let accepted_tags = BTreeSet::from([0]);
    assert!(
        runtime
            .produce_block_for_current_c(0, 1_800_000_000_123, &accepted_tags)
            .is_err(),
        "no share should exist after invalid Lean rejection"
    );
}

fn easy_runtime_fixture() -> Fixture {
    let mut fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/admission/v1.json"))
            .expect("fixture parses");
    fixture.constants.c =
        "0000000000000000000000000000000000000000000000000000000000000000".to_string();
    fixture.cfg.T_submit =
        "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string();
    fixture.cfg.T_ticket =
        "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string();
    fixture.cfg.T_share =
        "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string();
    fixture.cfg.T_block =
        "0xfffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe".to_string();
    fixture.cfg.MinShareScoreMultiplier = 1.0;
    fixture.cfg.K_max = 4;
    fixture.cfg.perIpRateLimitPer60s = 10;
    fixture
}

fn template_from_fixture(constants: &Constants) -> ProofSubmissionTemplate {
    ProofSubmissionTemplate {
        c: constants.c.clone(),
        pk: constants.pk.clone(),
        n: constants.n.clone(),
        j: constants.j.clone(),
        nonce_s: constants.nonce_s.clone(),
    }
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
            "boole-proof-bridge-{name}-{}-{}",
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
