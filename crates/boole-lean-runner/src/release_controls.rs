//! Real stage controls use a known package and the production confinement constructor.
use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

fn package() -> PathBuf {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../lean/checker");
    let target = std::env::temp_dir().join(format!(
        "boole-release-controls-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    for name in [
        "lean-toolchain",
        "lakefile.lean",
        "lake-manifest.json",
        "Boole/Family/V0Helpers.lean",
        "BooleCheck/Main.lean",
        "BooleCheck/Audit.lean",
    ] {
        let path = target.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::copy(source.join(name), path).unwrap();
    }
    assert_eq!(
        checker_artifact_hash(&target).unwrap(),
        CHECKER_RELEASE_HASH
    );
    target
}

#[test]
fn primary_and_artifact_audit_enforce_real_stage_boundaries() {
    if !boole_testkit::lake_and_lean_available() {
        return;
    }
    let package = package();
    let workspace = CheckerArtifactWorkspace::create().unwrap();
    let runtime = effective_toolchain_runtime(&package, IsolationMode::Enforce).unwrap();
    let path = isolated_lean_path(&[&runtime.lean_stdlib_dir]).unwrap();
    let base = LeanRunnerConfig::new("stage-control")
        .with_package_dir(package.clone())
        .with_checker_artifact_hash(CHECKER_RELEASE_HASH)
        .with_timeout_ms(30000);
    let runner = LeanRunner::new(base.clone());
    let primary_config = verification_stage_config(
        &base,
        &runtime,
        path.clone(),
        Some(workspace.primary_dir.clone()),
    );
    let audit_config = verification_stage_config(&base, &runtime, path, None);
    let marker = workspace.primary_dir.join("source-elaboration-count");
    let primary = workspace.primary_dir.join("PrimaryControl.lean");
    let code = format!(
        r#"import Lean
def main (_ : List String) : IO UInt32 := do
  try
    IO.FS.writeFile {package_file:?} "unexpected write"
    IO.println "package-write-allowed"
  catch _ => IO.println "package-write-denied"
  IO.FS.writeFile {marker:?} "x"
  IO.FS.writeFile {artifact:?} "serialized-artifact"
  return 0
"#,
        package_file = package.join("BooleCheck/Audit.lean").display().to_string(),
        marker = marker.display().to_string(),
        artifact = workspace.primary_artifact.display().to_string()
    );
    std::fs::write(&primary, code).unwrap();
    let mut command = Command::new(&runtime.lean_executable);
    command.arg("--run").arg(&primary).current_dir(&package);
    let outcome = runner
        .run_sandboxed_with_config(command, &primary_config)
        .unwrap();
    assert!(outcome.success, "{} {}", outcome.stdout, outcome.stderr);
    assert!(outcome.stdout.contains("package-write-denied"));
    assert_eq!(
        checker_artifact_hash(&package).unwrap(),
        CHECKER_RELEASE_HASH
    );
    let sealed = workspace.promote_for_audit().unwrap();
    let audit = workspace.primary_dir.join("AuditControl.lean");
    let code = format!(
        r#"import Lean
def main (args : List String) : IO UInt32 := do
  let [input] := args | return 64
  let data ← IO.FS.readFile input
  if data != "serialized-artifact" then return 1
  IO.println "artifact-only-input"
  try
    IO.FS.writeFile {package_file:?} "unexpected write"
    IO.println "package-write-allowed"
  catch _ => IO.println "package-write-denied"
  try
    IO.FS.writeFile {artifact:?} "unexpected artifact write"
    IO.println "artifact-write-allowed"
  catch _ => IO.println "artifact-write-denied"
  return 0
"#,
        package_file = package.join("BooleCheck/Main.lean").display().to_string(),
        artifact = workspace.primary_artifact.display().to_string()
    );
    std::fs::write(&audit, code).unwrap();
    let mut command = Command::new(&runtime.lean_executable);
    command
        .arg("--run")
        .arg(&audit)
        .arg(inherited_stdin_descriptor_path().unwrap())
        .current_dir(&package);
    let outcome = runner
        .run_sandboxed_with_config_and_stdin(
            command,
            &audit_config,
            Some(sealed.stdin_for_child().unwrap()),
        )
        .unwrap();
    assert!(outcome.success, "{} {}", outcome.stdout, outcome.stderr);
    for expected in [
        "artifact-only-input",
        "package-write-denied",
        "artifact-write-denied",
    ] {
        assert!(outcome.stdout.contains(expected), "{}", outcome.stdout);
    }
    assert_eq!(std::fs::read_to_string(marker).unwrap(), "x");
    assert_eq!(
        checker_artifact_hash(&package).unwrap(),
        CHECKER_RELEASE_HASH
    );
    assert_eq!(
        std::fs::read_to_string(&workspace.primary_artifact).unwrap(),
        "serialized-artifact"
    );
    std::fs::remove_dir_all(package).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn audit_and_helper_stage_cannot_spawn_descendants() {
    if !boole_testkit::lake_and_lean_available() {
        return;
    }
    let package = package();
    let workspace = CheckerArtifactWorkspace::create().unwrap();
    let runtime = effective_toolchain_runtime(&package, IsolationMode::Enforce).unwrap();
    let base = LeanRunnerConfig::new("stage-spawn-control")
        .with_package_dir(package.clone())
        .with_checker_artifact_hash(CHECKER_RELEASE_HASH)
        .with_timeout_ms(30000);
    let runner = LeanRunner::new(base.clone());
    let source = workspace.primary_dir.join("SpawnControl.lean");
    std::fs::write(&source,"import Lean\ndef main : IO UInt32 := do\n  let _ ← IO.Process.output { cmd := \"/usr/bin/true\" }\n  return 0\n").unwrap();
    for scratch in [None, Some(workspace.helper_import_root.clone())] {
        let config = verification_stage_config(
            &base,
            &runtime,
            isolated_lean_path(&[&runtime.lean_stdlib_dir]).unwrap(),
            scratch,
        );
        let mut command = Command::new(&runtime.lean_executable);
        command.arg("--run").arg(&source).current_dir(&package);
        let outcome = runner.run_sandboxed_with_config(command, &config).unwrap();
        assert!(!outcome.success, "descendant escaped");
    }
    assert_eq!(
        checker_artifact_hash(&package).unwrap(),
        CHECKER_RELEASE_HASH
    );
    std::fs::remove_dir_all(package).unwrap();
}
