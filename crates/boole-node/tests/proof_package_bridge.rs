use boole_core::{
    replay_blocks, validate_proof_package, AdmissionDecision, CalibrationReport, ValidationResult,
};
use boole_lean_runner::{LeanCheckResult, LeanRunner, LeanRunnerConfig, LeanRunnerEvidence};
use boole_node::FileBlockStore;
use boole_node::{
    canonical_pofp_package_from_lean_result, LeanProofBridge, LeanProofBridgePolicy,
    ProofSubmissionTemplate,
};
use boole_node::{RuntimeAdmissionState, RuntimeConfig};
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
fn lean_canonical_package_uses_pofp_v2_256_bit_slots() {
    let fixture = easy_runtime_fixture();
    let lean = synthetic_accepted_lean_result("stdout-a");
    let package = canonical_pofp_package_from_lean_result(&lean);

    assert_eq!(&package[0..4], b"POFP");
    assert_eq!(u32::from_le_bytes(package[4..8].try_into().unwrap()), 2);
    assert_eq!(package.len(), 86);
    assert_eq!(package[16], 0x19);
    assert_eq!(package[49], 0x19);
    assert_ne!(&package[17..49], [0u8; 32].as_slice());
    assert_ne!(&package[50..82], [0u8; 32].as_slice());

    assert!(matches!(
        validate_proof_package(&package, &fixture.cfg),
        ValidationResult::Ok {
            decl_count: 0,
            size: 86,
            universe_arity: 0,
        }
    ));
}

#[test]
fn lean_canonical_package_hash_surface_changes_across_full_digest_slots() {
    let first =
        canonical_pofp_package_from_lean_result(&synthetic_accepted_lean_result("stdout-a"));
    let second =
        canonical_pofp_package_from_lean_result(&synthetic_accepted_lean_result("stdout-b"));

    assert_ne!(&first[17..49], &second[17..49]);
    assert_ne!(&first[50..82], &second[50..82]);
}

#[test]
fn lean_bridge_policy_requires_verifier_and_checker_hash() {
    let runner = LeanRunner::new(LeanRunnerConfig::new("bridge-verifier-hash"));
    let missing_verifier = LeanProofBridge::try_new_with_policy(
        runner.clone(),
        LeanProofBridgePolicy::new().allow_checker_artifact_hash("abc"),
    )
    .expect_err("bridge policy must require verifier hash");
    assert!(missing_verifier
        .to_string()
        .contains("required verifier hash"));

    let missing_artifact = LeanProofBridge::try_new_with_policy(
        runner,
        LeanProofBridgePolicy::new().require_verifier_hash("bridge-verifier-hash"),
    )
    .expect_err("bridge policy must pin at least one checker artifact hash");
    assert!(missing_artifact
        .to_string()
        .contains("checker artifact allowlist"));
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

    let runner = LeanRunner::new(
        LeanRunnerConfig::new("bridge-verifier-hash")
            .with_package_dir(workspace.root.clone())
            .with_timeout_ms(5_000)
            .with_memory_limit_mb(8192),
    );
    let expected_artifact_hash = runner
        .evidence()
        .expect("evidence hashes checker before bridge construction")
        .checker_artifact_hash;
    let bridge = LeanProofBridge::try_new_with_policy(
        runner,
        LeanProofBridgePolicy::new()
            .require_verifier_hash("bridge-verifier-hash")
            .allow_checker_artifact_hash(expected_artifact_hash),
    )
    .expect("pinned proof bridge policy");
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
fn lean_bridge_binds_proof_source_into_canonical_package() {
    if !lake_and_lean_available() {
        eprintln!("skipping Lean proof bridge source-binding test: lake/lean unavailable");
        return;
    }
    let fixture = easy_runtime_fixture();
    let workspace = TestLeanWorkspace::new("bridge-source-binding");
    workspace.write_checker_project();
    let first_proof = workspace.write_proof(
        "FirstProof.lean",
        r#"theorem boole_bridge_source_binding_first : "aaa" = "aaa" :=
  rfl
"#,
    );
    let second_proof = workspace.write_proof(
        "SecondProof.lean",
        r#"theorem boole_bridge_source_binding_second : "bbb" = "bbb" :=
  rfl
"#,
    );

    let base_config = LeanRunnerConfig::new("bridge-verifier-hash")
        .with_package_dir(workspace.root.clone())
        .with_timeout_ms(5_000)
        .with_memory_limit_mb(8192);
    let expected_artifact_hash = LeanRunner::new(base_config.clone())
        .evidence()
        .expect("evidence hashes checker before bridge construction")
        .checker_artifact_hash;
    let bridge = LeanProofBridge::new_with_policy(
        LeanRunner::new(base_config),
        LeanProofBridgePolicy::new()
            .require_verifier_hash("bridge-verifier-hash")
            .allow_checker_artifact_hash(expected_artifact_hash),
    );
    let template = template_from_fixture(&fixture.constants);

    let first = bridge
        .build_submission_body(&first_proof, &template)
        .expect("first proof accepted");
    let second = bridge
        .build_submission_body(&second_proof, &template)
        .expect("second proof accepted");

    assert_ne!(
        first.package_bytes, second.package_bytes,
        "distinct per-attempt proof sources must not collapse to one canonical package"
    );
    assert_ne!(
        first.body.get("bytes"),
        second.body.get("bytes"),
        "submission body bytes must bind the proof source"
    );
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

    let runner = LeanRunner::new(
        LeanRunnerConfig::new("bridge-verifier-hash")
            .with_package_dir(workspace.root.clone())
            .with_timeout_ms(5_000)
            .with_memory_limit_mb(8192),
    );
    let expected_artifact_hash = runner
        .evidence()
        .expect("evidence hashes checker before bridge construction")
        .checker_artifact_hash;
    let bridge = LeanProofBridge::try_new_with_policy(
        runner,
        LeanProofBridgePolicy::new()
            .require_verifier_hash("bridge-verifier-hash")
            .allow_checker_artifact_hash(expected_artifact_hash),
    )
    .expect("pinned proof bridge policy");
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
    fixture.cfg.MinShareScoreMultiplier = serde_json::Number::from(1);
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

fn synthetic_accepted_lean_result(stdout: &str) -> LeanCheckResult {
    LeanCheckResult {
        accepted: true,
        exit_code: 0,
        stdout: stdout.to_string(),
        stderr: String::new(),
        timed_out: false,
        output_truncated: false,
        evidence: LeanRunnerEvidence {
            verifier_hash: "bridge-verifier-hash".to_string(),
            checker: "lake exec boole_check".to_string(),
            checker_exe: "lake".to_string(),
            checker_artifact_hash:
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string(),
            package_dir: "/tmp/boole-check".to_string(),
            lean_version: "Lean 4.29.1".to_string(),
            lake_version: "Lake 5.0.0".to_string(),
            timeout_ms: 5_000,
            memory_limit_mb: 8_192,
            output_limit_bytes: 65_536,
        },
    }
}
