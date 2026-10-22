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
        "import\n Lean\nexample : True := trivial\n",
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
        // Policy v4 forbids the `unsafe` word anywhere in solver text, so that
        // case now fails at source intake; `partial` still reaches the replay audit.
        if name == "unsafe" {
            let error = runner
                .check_file(&proof)
                .expect_err("the text policy rejects `unsafe` before any checker child runs");
            assert!(
                error.to_string().contains("forbidden `unsafe` token")
                    && error.to_string().contains("Skipped.lean:2"),
                "unexpected unsafe rejection: {error:#}"
            );
            continue;
        }
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
    workspace.write_checker_project();
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
    workspace.write_checker_project();
    let proof = workspace.write_proof(
        "InvalidProof.lean",
        &format!(
            "import Boole.Family.V0Helpers\ntheorem invalid : True := by\n  exact {}\n",
            "unknown_identifier".repeat(200)
        ),
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
    assert!(
        matches!(result.verdict, boole_lean_runner::LeanVerdict::DeterministicReject { ref reason } if reason == "lean_rejected")
    );
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
    boole_testkit::lake_and_lean_available()
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
        for (relative, bytes) in [
            (
                "lean-toolchain",
                include_str!("../../../lean/checker/lean-toolchain"),
            ),
            (
                "lakefile.lean",
                include_str!("../../../lean/checker/lakefile.lean"),
            ),
            (
                "lake-manifest.json",
                include_str!("../../../lean/checker/lake-manifest.json"),
            ),
        ] {
            std::fs::write(self.root.join(relative), bytes).expect("copy release configuration");
        }
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
            include_str!("../../../lean/checker/Boole/Family/V0Helpers.lean"),
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

#[test]
fn both_known_releases_verify_with_their_explicit_pin() {
    if !lake_and_lean_available() {
        return;
    }
    for (directory, pin) in [
        ("checker-v1", boole_lean_runner::CHECKER_RELEASE_HASH_V1),
        ("checker", boole_lean_runner::CHECKER_RELEASE_HASH),
    ] {
        let workspace = TestLeanWorkspace::new(directory);
        let proof = workspace.write_proof(
            "Proof.lean",
            "import Boole.Family.V0Helpers\ntheorem release_valid : True := trivial\n",
        );
        let package = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../lean")
            .join(directory);
        let runner = LeanRunner::new(
            LeanRunnerConfig::new("release-compatibility")
                .with_package_dir(package)
                .with_checker_artifact_hash(pin)
                .with_timeout_ms(30000),
        );
        let result = runner.check_file(&proof).unwrap();
        assert!(result.accepted, "{directory}: {result:?}");
        assert_eq!(result.evidence.checker_artifact_hash, pin);
    }
}
