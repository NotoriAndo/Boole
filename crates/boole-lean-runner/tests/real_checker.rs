use boole_lean_runner::{IsolationMode, LeanRunner, LeanRunnerConfig};
use std::path::PathBuf;
use std::process::Command;

#[test]
fn direct_source_checker_accepts_valid_lean_file_with_evidence() {
    if !lake_and_lean_available() {
        eprintln!("skipping real Lean runner test: lake/lean unavailable");
        return;
    }
    let workspace = TestLeanWorkspace::new("valid");
    workspace.write_checker_project();
    let proof = workspace.write_proof(
        "ValidProof.lean",
        r#"import Boole.Family.V0Helpers
theorem boole_valid : 1 + 1 = 2 := by
  decide
"#,
    );

    let runner = LeanRunner::new(
        LeanRunnerConfig::new("fixture-verifier-hash")
            .with_package_dir(workspace.root.clone())
            // This is the real three-stage correctness control, not the
            // timeout control below. A clean macOS runner must cold-start
            // the direct Lean elaborator and the artifact audit, so give
            // correctness a bounded but non-performance-sensitive window.
            .with_timeout_ms(30_000)
            .with_memory_limit_mb(8192),
    );

    let result = runner.check_file(&proof).expect("checker runs");
    assert!(result.accepted, "valid proof should pass: {result:?}");
    assert_eq!(result.evidence.verifier_hash, "fixture-verifier-hash");
    assert_eq!(
        result.evidence.checker,
        "direct lean source checker + artifact audit"
    );
    assert_eq!(result.evidence.checker_exe, "lean");
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
    assert_eq!(result.evidence.timeout_ms, 30_000);
    assert_eq!(result.evidence.memory_limit_mb, 8192);
}

#[test]
fn production_audit_rejects_an_artifact_without_the_exact_helper_import() {
    if !lake_and_lean_available() {
        eprintln!("skipping exact-import audit test: lake/lean unavailable");
        return;
    }
    let workspace = TestLeanWorkspace::new("missing-helper-import");
    workspace.write_checker_project();
    let proof = workspace.write_proof(
        "MissingHelperImport.lean",
        "theorem missing_helper_import : True := trivial\n",
    );
    let runner = LeanRunner::new(
        LeanRunnerConfig::new("fixture-verifier-hash")
            .with_package_dir(workspace.root.clone())
            .with_isolation_mode(IsolationMode::Log)
            .with_timeout_ms(5_000),
    );

    let result = runner.check_file(&proof).expect("checker returns envelope");
    assert!(
        !result.accepted
            && matches!(
                result.verdict,
                boole_lean_runner::LeanVerdict::DeterministicReject { ref reason }
                    if reason == "axiom_audit_rejected"
            ),
        "an artifact without the single pinned helper import must be a deterministic reject: {result:?}"
    );
    assert!(
        result.stderr.contains("BOOLE_UNEXPECTED_IMPORT"),
        "the trusted audit must identify the import-set mismatch: {result:?}"
    );
}

#[test]
fn production_audit_rejects_an_unexpected_import_if_intake_is_bypassed() {
    if !lake_and_lean_available() {
        eprintln!("skipping unexpected-import audit test: lake/lean unavailable");
        return;
    }
    let workspace = TestLeanWorkspace::new("unexpected-artifact-import");
    workspace.write_checker_project();
    let proof = workspace.write_proof(
        "UnexpectedImport.lean",
        "import Lean\ntheorem unexpected_import : True := trivial\n",
    );
    let artifact = workspace.root.join("UnexpectedImport.olean");

    // Invoke the trusted primary directly to model a future intake bug. The
    // normal Rust entrypoint rejects this source before any child is spawned;
    // the audit must still enforce the exact canonical import vector itself.
    let primary = Command::new("lake")
        .args(["env", "lean", "--run", "BooleCheck/Main.lean"])
        .arg(&proof)
        .args(["400000", "512"])
        .arg(&artifact)
        .current_dir(&workspace.root)
        .output()
        .expect("run production primary directly");
    assert!(
        primary.status.success(),
        "direct primary failed: stdout={} stderr={}",
        String::from_utf8_lossy(&primary.stdout),
        String::from_utf8_lossy(&primary.stderr)
    );

    let audit = Command::new("lake")
        .args(["env", "lean", "--run", "BooleCheck/Audit.lean"])
        .arg(&artifact)
        .current_dir(&workspace.root)
        .output()
        .expect("run production audit directly");
    assert!(!audit.status.success(), "unexpected import must not audit");
    assert!(
        String::from_utf8_lossy(&audit.stderr).contains("BOOLE_UNEXPECTED_IMPORT"),
        "unexpected import must have a typed semantic rejection: stdout={} stderr={}",
        String::from_utf8_lossy(&audit.stdout),
        String::from_utf8_lossy(&audit.stderr)
    );
}

#[test]
fn multiline_import_continuation_is_rejected_before_any_checker_child_runs() {
    let workspace = TestLeanWorkspace::new("multiline-import-continuation");
    let proof = workspace.write_proof(
        "MultilineImport.lean",
        "import\n Lean\n#check Lean.Environment\n",
    );
    let runner = LeanRunner::new(
        LeanRunnerConfig::new("fixture-verifier-hash")
            .with_package_dir(workspace.root.clone())
            .with_isolation_mode(IsolationMode::Log),
    );

    let error = runner
        .check_file(&proof)
        .expect_err("an empty import tail must be rejected during source intake");
    assert!(
        error
            .to_string()
            .contains("forbidden `import <missing module>` token")
            && error.to_string().contains("MultilineImport.lean:1"),
        "unexpected multiline-import rejection: {error:#}"
    );
}

#[test]
fn audit_process_receives_checker_artifact_not_submitted_source() {
    if !lake_and_lean_available() {
        eprintln!("skipping artifact-boundary test: lake/lean unavailable");
        return;
    }
    let workspace = TestLeanWorkspace::new("artifact-boundary");
    workspace.write_checker_project_with_scripts(
        r#"def main (args : List String) : IO UInt32 := do
  let some artifactPath := (args.drop 3).head?
    | IO.eprintln "missing checker artifact path"; return 64
  IO.FS.writeFile artifactPath "checker-produced-artifact"
  return 0
"#,
        r#"def main (args : List String) : IO UInt32 := do
  let some artifactPath := args.head?
    | IO.eprintln "missing audit artifact path"; return 64
  if artifactPath.endsWith ".lean" then
    IO.eprintln "audit received submitted source"
    return 1
  let payload ← IO.FS.readFile artifactPath
  if payload != "checker-produced-artifact" then
    IO.eprintln "audit did not receive the checker artifact"
    return 1
  IO.println "BOOLE_AXIOM_AUDIT_DONE"
  return 0
"#,
    );
    let proof = workspace.write_proof(
        "ValidProof.lean",
        "theorem artifact_boundary : True := trivial\n",
    );

    let runner = LeanRunner::new(
        LeanRunnerConfig::new("fixture-verifier-hash")
            .with_package_dir(workspace.root.clone())
            .with_isolation_mode(IsolationMode::Log)
            .with_timeout_ms(5_000),
    );

    let result = runner.check_file(&proof).expect("checker runs");
    assert!(
        result.accepted,
        "the audit must receive only the checker-produced artifact: {result:?}"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn primary_cannot_modify_the_trusted_checker_package() {
    if !lake_and_lean_available() {
        eprintln!("skipping package-readonly test: lake/lean unavailable");
        return;
    }
    let workspace = TestLeanWorkspace::new("primary-package-readonly");
    workspace.write_checker_project_with_scripts(
        r#"def main (args : List String) : IO UInt32 := do
  let some artifactPath := (args.drop 3).head?
    | return 64
  IO.FS.writeFile "BooleCheck/Audit.lean" "tampered"
  IO.FS.writeFile artifactPath "artifact"
  return 0
"#,
        r#"def main (_args : List String) : IO UInt32 := do
  IO.println "BOOLE_AXIOM_AUDIT_DONE"
  return 0
"#,
    );
    let audit_path = workspace.root.join("BooleCheck/Audit.lean");
    let before = std::fs::read(&audit_path).expect("read trusted audit");
    let proof = workspace.write_proof("Proof.lean", "theorem t : True := trivial\n");
    let runner = LeanRunner::new(
        LeanRunnerConfig::new("fixture-verifier-hash")
            .with_package_dir(workspace.root.clone())
            .with_timeout_ms(5_000),
    );
    let result = runner.check_file(&proof).expect("checker returns envelope");
    assert!(!result.accepted, "package mutation attempt must not accept");
    assert_eq!(
        std::fs::read(&audit_path).expect("reread trusted audit"),
        before,
        "primary process must see the checker package as read-only"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn audit_cannot_modify_the_trusted_checker_package_or_artifact() {
    if !lake_and_lean_available() {
        eprintln!("skipping audit-readonly test: lake/lean unavailable");
        return;
    }
    let workspace = TestLeanWorkspace::new("audit-package-readonly");
    workspace.write_checker_project_with_scripts(
        r#"def main (args : List String) : IO UInt32 := do
  let some artifactPath := (args.drop 3).head?
    | return 64
  IO.FS.writeFile artifactPath "artifact"
  return 0
"#,
        r#"def main (args : List String) : IO UInt32 := do
  let some artifactPath := args.head?
    | return 64
  try IO.FS.writeFile "BooleCheck/Main.lean" "tampered" catch _ => pure ()
  try IO.FS.writeFile artifactPath "tampered-artifact" catch _ => pure ()
  IO.println "BOOLE_AXIOM_AUDIT_DONE"
  return 0
"#,
    );
    let main_path = workspace.root.join("BooleCheck/Main.lean");
    let before = std::fs::read(&main_path).expect("read trusted primary");
    let proof = workspace.write_proof("Proof.lean", "theorem t : True := trivial\n");
    let runner = LeanRunner::new(
        LeanRunnerConfig::new("fixture-verifier-hash")
            .with_package_dir(workspace.root.clone())
            .with_timeout_ms(5_000),
    );
    let result = runner.check_file(&proof).expect("checker returns envelope");
    assert!(
        result.accepted,
        "denied mutation attempts must not stop a synthetic semantic audit: {result:?}"
    );
    assert_eq!(
        std::fs::read(&main_path).expect("reread trusted primary"),
        before,
        "audit process must see the checker package as read-only"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn artifact_audit_cannot_spawn_a_descendant_process() {
    if !lake_and_lean_available() {
        eprintln!("skipping audit process-containment test: lake/lean unavailable");
        return;
    }
    let workspace = TestLeanWorkspace::new("audit-process-spawn");
    workspace.write_checker_project_with_scripts(
        r#"def main (args : List String) : IO UInt32 := do
  let some artifactPath := (args.drop 3).head?
    | return 64
  IO.FS.writeFile artifactPath "artifact"
  return 0
"#,
        r#"import Lean

def main (_args : List String) : IO UInt32 := do
  let _ ← IO.Process.output { cmd := "/usr/bin/true" }
  IO.println "BOOLE_AXIOM_AUDIT_DONE"
  return 0
"#,
    );
    let proof = workspace.write_proof("Proof.lean", "theorem t : True := trivial\n");
    let runner = LeanRunner::new(
        LeanRunnerConfig::new("fixture-verifier-hash")
            .with_package_dir(workspace.root.clone())
            .with_timeout_ms(5_000),
    );
    let result = runner.check_file(&proof).expect("checker returns envelope");
    assert!(
        !result.accepted,
        "the artifact-only audit must not be able to spawn a descendant: {result:?}"
    );
    assert!(
        matches!(
            result.verdict,
            boole_lean_runner::LeanVerdict::RetryableUnavailable { ref reason }
                if reason == "axiom_audit_unavailable"
        ),
        "a blocked audit process spawn is an audit availability failure: {result:?}"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn request_private_helper_compiler_cannot_spawn_a_process() {
    if !lake_and_lean_available() {
        eprintln!("skipping helper-compile containment test: lake/lean unavailable");
        return;
    }
    let workspace = TestLeanWorkspace::new("helper-compile-process-spawn");
    workspace.write_checker_project();
    let marker = workspace.root.join("helper-spawn-marker");
    workspace.write_helper(&format!(
        r#"import Lean
run_cmd do
  let _ ← IO.Process.output {{ cmd := "/usr/bin/touch", args := #[{marker:?}] }}
"#,
        marker = marker.display().to_string()
    ));
    let proof = workspace.write_proof("Proof.lean", "theorem t : True := trivial\n");
    let runner = LeanRunner::new(
        LeanRunnerConfig::new("fixture-verifier-hash")
            .with_package_dir(workspace.root.clone())
            .with_timeout_ms(5_000),
    );

    let result = runner.check_file(&proof).expect("checker returns envelope");
    assert!(
        !result.accepted
            && matches!(
                result.verdict,
                boole_lean_runner::LeanVerdict::RetryableUnavailable { .. }
            ),
        "blocked trusted-helper process creation is availability failure: {result:?}"
    );
    assert!(
        !marker.exists(),
        "the helper compilation sandbox must not let a child process persist work"
    );
}

#[test]
fn production_audit_has_no_submitted_source_execution_surface() {
    let audit = include_str!("../../../lean/checker/BooleCheck/Audit.lean");
    for forbidden in [
        "processCommands",
        "runFrontend",
        "Parser.mkInputContext",
        "IO.FS.readFile proofPath",
    ] {
        assert!(
            !audit.contains(forbidden),
            "audit process must not execute submitted source via `{forbidden}`"
        );
    }
    assert!(
        audit.contains("readModuleData"),
        "audit must read the checker-produced serialized module"
    );
    assert!(
        audit.contains(".replay"),
        "audit must replay serialized declarations through the kernel"
    );
    assert!(
        audit.contains("artifact.isModule"),
        "multipart module artifacts must fail closed until every part is audited"
    );
    assert!(
        audit.contains("info.isUnsafe || info.isPartial"),
        "declarations skipped by Lean replay must fail closed"
    );
}

#[test]
fn production_primary_elaborates_in_process_without_a_spawn_surface() {
    let primary = include_str!("../../../lean/checker/BooleCheck/Main.lean");
    for forbidden in ["IO.Process", "Process.output", "cmd := \"lean\""] {
        assert!(
            !primary.contains(forbidden),
            "the source-reading primary must not spawn a descendant via `{forbidden}`"
        );
    }
    assert!(
        primary.contains("runFrontend"),
        "the source-reading primary must elaborate and serialize in its own process"
    );
}

#[test]
fn corrupt_checker_artifact_is_unavailable_not_a_proof_reject() {
    if !lake_and_lean_available() {
        eprintln!("skipping corrupt-artifact test: lake/lean unavailable");
        return;
    }
    let workspace = TestLeanWorkspace::new("corrupt-artifact");
    workspace.write_checker_project_with_main(
        r#"def main (args : List String) : IO UInt32 := do
  let some artifactPath := (args.drop 3).head?
    | return 64
  IO.FS.writeFile artifactPath "not-an-olean"
  return 0
"#,
    );
    let proof = workspace.write_proof("Proof.lean", "theorem t : True := trivial\n");
    let runner = LeanRunner::new(
        LeanRunnerConfig::new("fixture-verifier-hash")
            .with_package_dir(workspace.root.clone())
            .with_isolation_mode(IsolationMode::Log)
            .with_timeout_ms(5_000),
    );
    let result = runner.check_file(&proof).expect("checker returns envelope");
    assert!(
        matches!(
            result.verdict,
            boole_lean_runner::LeanVerdict::RetryableUnavailable { .. }
        ),
        "artifact read/deserialize failure is availability, not proof truth: {result:?}"
    );
}

#[test]
fn production_audit_rejects_declarations_that_kernel_replay_would_skip() {
    if !lake_and_lean_available() {
        eprintln!("skipping replay-skip test: lake/lean unavailable");
        return;
    }
    for (name, source) in [
        (
            "unsafe",
            "import Boole.Family.V0Helpers\nunsafe def uncheckedValue : Nat := 7\n",
        ),
        (
            "partial",
            "import Boole.Family.V0Helpers\npartial def unbounded : Nat := unbounded\n",
        ),
    ] {
        let workspace = TestLeanWorkspace::new(&format!("replay-skip-{name}"));
        workspace.write_checker_project();
        let proof = workspace.write_proof("Skipped.lean", source);
        let runner = LeanRunner::new(
            LeanRunnerConfig::new("fixture-verifier-hash")
                .with_package_dir(workspace.root.clone())
                .with_isolation_mode(IsolationMode::Log)
                .with_timeout_ms(5_000),
        );
        let result = runner.check_file(&proof).expect("checker returns envelope");
        assert!(
            !result.accepted,
            "{name} declaration skipped by replay must fail closed: {result:?}"
        );
    }
}

#[test]
fn production_audit_rejects_multipart_module_artifacts_until_all_parts_are_audited() {
    if !lake_and_lean_available() {
        eprintln!("skipping multipart-module test: lake/lean unavailable");
        return;
    }
    let workspace = TestLeanWorkspace::new("multipart-module");
    workspace.write_checker_project();
    let proof = workspace.write_proof(
        "Multipart.lean",
        "module\n\npublic theorem module_private : True := trivial\n",
    );
    let runner = LeanRunner::new(
        LeanRunnerConfig::new("fixture-verifier-hash")
            .with_package_dir(workspace.root.clone())
            .with_isolation_mode(IsolationMode::Log)
            .with_timeout_ms(5_000),
    );
    let result = runner.check_file(&proof).expect("checker returns envelope");
    assert!(
        !result.accepted,
        "base-only audit must fail closed for multipart modules: {result:?}"
    );
}

#[test]
fn submitted_elaboration_side_effect_runs_exactly_once() {
    if !lake_and_lean_available() {
        eprintln!("skipping exactly-once test: lake/lean unavailable");
        return;
    }
    let workspace = TestLeanWorkspace::new("elaboration-exactly-once");
    workspace.write_checker_project();
    let marker = workspace.root.join("elaboration-side-effect.txt");
    workspace.write_helper(&format!(
        r#"import Lean
open Lean Elab Command

elab "booleTick" : command => do
  let marker := {marker:?}
  let prior ← if ← System.FilePath.pathExists marker then
    IO.FS.readFile marker
  else
    pure ""
  IO.FS.writeFile marker (prior ++ "x")
"#,
        marker = marker.display().to_string()
    ));
    workspace.build(&["Boole.Family.V0Helpers"]);
    assert!(
        !marker.exists(),
        "building the trusted command declaration must not execute it"
    );
    let proof = workspace.write_proof(
        "ExactlyOnce.lean",
        "import Boole.Family.V0Helpers\nbooleTick\ntheorem exact_once : True := trivial\n",
    );
    let runner = LeanRunner::new(
        LeanRunnerConfig::new("fixture-verifier-hash")
            .with_package_dir(workspace.root.clone())
            .with_isolation_mode(IsolationMode::Log)
            .with_timeout_ms(5_000),
    );
    let result = runner.check_file(&proof).expect("checker returns envelope");
    assert!(
        result.accepted,
        "trusted helper command should pass: {result:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&marker).expect("read side-effect marker"),
        "x",
        "the artifact-only audit must not execute submitted commands again"
    );
}

#[test]
fn checker_identity_must_not_accept_a_stale_helper_olean_after_source_changes() {
    if !lake_and_lean_available() {
        eprintln!("skipping stale-helper identity test: lake/lean unavailable");
        return;
    }
    let workspace = TestLeanWorkspace::new("stale-helper-identity");
    workspace.write_checker_project();
    workspace.write_helper(
        r#"namespace Boole.Family.V0Helpers
theorem pinnedWitness : True := trivial
end Boole.Family.V0Helpers
"#,
    );
    workspace.build(&["Boole.Family.V0Helpers"]);

    // Change only the source covered by checker_artifact_hash. The old
    // prebuilt `.olean` still exports `pinnedWitness : True`; rebuilding this
    // new source would instead give it an incompatible type.
    workspace.write_helper(
        r#"namespace Boole.Family.V0Helpers
theorem pinnedWitness : 1 = 1 := rfl
end Boole.Family.V0Helpers
"#,
    );
    let proof = workspace.write_proof(
        "StaleHelper.lean",
        r#"import Boole.Family.V0Helpers
theorem stale_helper_boundary : True := Boole.Family.V0Helpers.pinnedWitness
"#,
    );
    let runner = LeanRunner::new(
        LeanRunnerConfig::new("fixture-verifier-hash")
            .with_package_dir(workspace.root.clone())
            .with_isolation_mode(IsolationMode::Log)
            .with_timeout_ms(5_000),
    );

    let result = runner.check_file(&proof).expect("checker returns envelope");
    assert!(
        !result.accepted,
        "checker identity must bind the helper bytes actually imported, not only source bytes; \
         a stale helper artifact was accepted as if it came from the changed pinned source: {result:?}"
    );
}

#[test]
fn direct_source_checker_times_out_and_returns_rejection_envelope() {
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
fn direct_source_checker_caps_captured_output_and_marks_truncation() {
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
fn direct_source_checker_rejects_invalid_lean_file_without_panicking() {
    if !lake_and_lean_available() {
        eprintln!("skipping real Lean runner test: lake/lean unavailable");
        return;
    }
    let workspace = TestLeanWorkspace::new("invalid");
    workspace.write_checker_project();
    let proof = workspace.write_proof(
        "InvalidProof.lean",
        r#"import Boole.Family.V0Helpers
theorem boole_invalid : 1 + 1 = 3 := by
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
    assert_eq!(
        result.evidence.checker,
        "direct lean source checker + artifact audit"
    );
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
        // SC.9a — the default fixture checker IS the production
        // `BooleCheck/Main.lean` (pulled in verbatim at compile time, like
        // the audit script below), so the budget-args contract between the
        // runner and the checker can never drift in these tests.
        self.write_checker_project_with_main(include_str!(
            "../../../lean/checker/BooleCheck/Main.lean"
        ));
    }

    fn write_checker_project_with_main(&self, main_lean: &str) {
        self.write_checker_project_with_scripts(
            main_lean,
            include_str!("../../../lean/checker/BooleCheck/Audit.lean"),
        );
    }

    fn write_checker_project_with_scripts(&self, main_lean: &str, audit_lean: &str) {
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

lean_lib «Boole» where
  globs := #[.submodules `Boole.Family]

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
        // process (`lean --run BooleCheck/Audit.lean`) after the
        // primary checker accepts, so every synthetic fixture package needs
        // its own copy of the real audit script or that stage fails to spawn.
        // `include_str!` pulls the production file in verbatim at compile
        // time so the fixture can never drift from what actually ships.
        std::fs::write(self.root.join("BooleCheck/Audit.lean"), audit_lean)
            .expect("write axiom audit script");
        std::fs::create_dir_all(self.root.join("Boole/Family")).expect("create Boole/Family");
        std::fs::write(
            self.root.join("Boole/Family/V0Helpers.lean"),
            "-- fixture stub: pinned by checker_artifact_hash\n",
        )
        .expect("write V0Helpers stub");
        // Leave `.lake/build` cold. Production verification must compile the
        // pinned helper source into its own request-private import tree; a
        // fixture prebuild here would hide a regression back to trusting the
        // package's stale `.olean`.
        assert!(
            !self.root.join(".lake/build/bin/boole_check").exists(),
            "the direct-source runner test must not rely on a prebuilt checker binary"
        );
        assert!(
            !self
                .root
                .join(".lake/build/lib/lean/Boole/Family/V0Helpers.olean")
                .exists(),
            "the default runner test must exercise the cold private-helper compiler"
        );
    }

    fn write_proof(&self, name: &str, content: &str) -> PathBuf {
        let path = self.root.join(name);
        std::fs::write(&path, content).expect("write proof");
        path
    }

    fn write_helper(&self, content: &str) {
        std::fs::write(self.root.join("Boole/Family/V0Helpers.lean"), content)
            .expect("write V0Helpers fixture");
    }

    fn build(&self, targets: &[&str]) {
        let output = Command::new("lake")
            .arg("build")
            .args(targets)
            .current_dir(&self.root)
            .output()
            .expect("run lake build");
        assert!(
            output.status.success(),
            "lake build failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
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
