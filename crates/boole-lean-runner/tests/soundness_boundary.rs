//! TB.1 / ADR-0013 — checker soundness boundary.
//!
//! Each PoC below demonstrates a distinct way a submitted proof could escape
//! the checker's intended trust boundary before this slice landed. All four
//! are `accepted` under the OLD (pre-TB.1) `FORBIDDEN_TOKENS` list, since
//! none of them writes `sorry`, a top-level `axiom` declaration,
//! `native_decide`, or `#eval`. TB.1 closes each gap either via the extended
//! pre-spawn blacklist (PoC1/2/3) or, for PoC4, which is specifically
//! constructed to slip past every blacklist entry, via the post-elaboration
//! axiom-closure audit — the PRIMARY boundary. See
//! `crates/boole-lean-runner/src/lib.rs`'s `enforce_axiom_allowlist` and
//! `lean/checker/BooleCheck/Audit.lean` for the mechanism.

use boole_lean_runner::{LeanCheckResult, LeanRunner, LeanRunnerConfig};
use std::path::PathBuf;
use std::process::Command;

// PoC1 — a custom `elab` command runs arbitrary IO (shelling out via
// `IO.Process.output`) *during elaboration*, before the checker's own
// top-level scan of the resulting theorem could ever matter.
const POC1_CUSTOM_ELAB_IO: &str = r#"import Lean

open Lean Elab Command

elab "runPoc1" : command => do
  let out ← IO.Process.output { cmd := "id" }
  IO.FS.writeFile "/tmp/boole_soundness_poc1_marker.txt" out.stdout

runPoc1

theorem trivial_thm : True := trivial
"#;

// PoC2 — `Lean.addDecl` injects a bogus axiom directly into the
// environment from inside a custom `elab` command, without the literal
// word `axiom` ever appearing, then closes a false theorem with it.
const POC2_ADDDECL_AXIOM_INJECTION: &str = r#"import Lean

open Lean Elab Command Meta

elab "injectAxiom" ty:term : command => do
  let type ← liftTermElabM do
    let e ← Elab.Term.elabType ty
    Elab.Term.synthesizeSyntheticMVarsNoPostponing
    instantiateMVars e
  let decl := Declaration.axiomDecl {
    name := `boole_poc2_bad_axiom
    levelParams := []
    type := type
    isUnsafe := false
  }
  liftCoreM <| addDecl decl

injectAxiom ((1 : Nat) = 2)

theorem poc2_false_theorem : (1 : Nat) = 2 := boole_poc2_bad_axiom
"#;

// PoC3 — `set_option debug.skipKernelTC true` disables kernel
// typechecking entirely. Deliberately proves a TRUE, ordinary theorem (no
// `axiom`/`sorry`/other escape) so the only thing distinguishing this file
// from an everyday accepted proof is the debug option itself — no
// `debug.*` option has a legitimate use in a submitted proof, so it must be
// rejected on sight regardless of what it is used to prove.
const POC3_DEBUG_SKIP_KERNEL_TC: &str = r#"set_option debug.skipKernelTC true

theorem poc3_trivial_with_kernel_tc_skipped : True := trivial
"#;

// PoC4 — reaches a non-allowlisted axiom (`Lean.trustCompiler`) WITHOUT
// tripping any pre-spawn check at all: no `addDecl`/`elab`/`macro`/
// `initialize`/`debug.`/`sorry`/`axiom`/`native_decide`/`#eval` token, and
// no `import` line whatsoever. Empirically confirmed against the pinned
// 4.29.1 toolchain: `Lean.trustCompiler` resolves from the default prelude
// (no `import Lean` needed) and `lean` accepts this file with only a
// deprecation warning, exit code 0 — so only the post-elaboration axiom
// audit, not the blacklist, can reject it.
const POC4_NON_ALLOWLISTED_AXIOM_TERM: &str = r#"theorem poc4_trusts_compiler : True :=
  let _ := @Lean.trustCompiler
  trivial
"#;

// Acceptance — a legitimate proof in the official v1-lenbound module shape,
// importing the one reviewed helper surface and using its real lemmas. TB.1
// must not reject this: its axiom closure is exactly the standard
// {propext, Classical.choice, Quot.sound} subset the helper library itself
// relies on.
const LENBOUND_STYLE_ACCEPTANCE_PROOF: &str = r#"import Boole.Family.V0Helpers

namespace BooleVerifyMod

open Boole.Family.V0Helpers

theorem instance_thm : ∀ (xs : List Int), (dedup xs).length ≤ xs.length :=
  length_dedup_le

end BooleVerifyMod
"#;

#[test]
fn rejects_custom_elab_io() {
    if !lake_and_lean_available() {
        eprintln!("skipping real Lean runner test: lake/lean unavailable");
        return;
    }
    let workspace = TestLeanWorkspace::new("poc1-elab-io");
    workspace.write_checker_project();
    let proof = workspace.write_proof("Poc1.lean", POC1_CUSTOM_ELAB_IO);
    assert_rejected(
        runner_for(&workspace).check_file(&proof),
        "a custom `elab` command that runs IO during elaboration",
    );
}

#[test]
#[allow(non_snake_case)] // TB.1 spec pins this exact test name (addDecl).
fn rejects_addDecl_axiom_injection() {
    if !lake_and_lean_available() {
        eprintln!("skipping real Lean runner test: lake/lean unavailable");
        return;
    }
    let workspace = TestLeanWorkspace::new("poc2-adddecl");
    workspace.write_checker_project();
    let proof = workspace.write_proof("Poc2.lean", POC2_ADDDECL_AXIOM_INJECTION);
    assert_rejected(
        runner_for(&workspace).check_file(&proof),
        "`Lean.addDecl` injecting an axiom without ever writing `axiom`",
    );
}

#[test]
fn rejects_debug_skip_kernel_tc() {
    if !lake_and_lean_available() {
        eprintln!("skipping real Lean runner test: lake/lean unavailable");
        return;
    }
    let workspace = TestLeanWorkspace::new("poc3-debug-skip");
    workspace.write_checker_project();
    let proof = workspace.write_proof("Poc3.lean", POC3_DEBUG_SKIP_KERNEL_TC);
    assert_rejected(
        runner_for(&workspace).check_file(&proof),
        "`set_option debug.skipKernelTC true`",
    );
}

#[test]
fn rejects_proof_depending_on_non_allowlisted_axiom() {
    if !lake_and_lean_available() {
        eprintln!("skipping real Lean runner test: lake/lean unavailable");
        return;
    }
    let workspace = TestLeanWorkspace::new("poc4-trust-compiler");
    workspace.write_checker_project();
    let proof = workspace.write_proof("Poc4.lean", POC4_NON_ALLOWLISTED_AXIOM_TERM);

    // `.expect` (not `assert_rejected`) is deliberate: PoC4 must reach
    // `check_file`'s `Ok(..)` path (primary checker + audit both actually
    // ran) rather than being turned away as an `Err` by the pre-spawn
    // blacklist — this is the one PoC that must specifically exercise the
    // audit layer, not the blacklist.
    let result = runner_for(&workspace).check_file(&proof).expect(
        "PoC4 contains no forbidden token and no import line at all, so the \
         pre-spawn blacklist must let it through to the primary checker and \
         the axiom audit — a rejection here would mean the blacklist wrongly \
         intercepted it",
    );
    assert!(
        !result.accepted,
        "a proof whose axiom closure includes a non-allowlisted axiom \
         (Lean.trustCompiler) must be rejected: {result:?}"
    );
    assert!(
        result.stderr.contains("axiom"),
        "the rejection must come from the axiom audit (enforce_axiom_allowlist), \
         not a blacklist token match: stderr={:?}",
        result.stderr
    );
}

#[test]
fn accepts_lenbound_style_proof_importing_helper_surface() {
    if !lake_and_lean_available() {
        eprintln!("skipping real Lean runner test: lake/lean unavailable");
        return;
    }
    let checker_dir = real_checker_package_dir();
    if !checker_dir.is_dir() {
        eprintln!(
            "skipping: real checker package dir not found at {}",
            checker_dir.display()
        );
        return;
    }
    // Mirrors `scripts/self-test.sh`'s `lean-checker-build` gate: the raw
    // `lean <path>` subprocess `BooleCheck.Main` shells out to (and the
    // audit's `lake env lean --run`) both need `Boole.Family.V0Helpers`
    // already compiled to resolve `import Boole.Family.V0Helpers` — `lake
    // exec boole_check` does not build it as a side effect, since the
    // checker's own Main.lean never `import`s it itself.
    let build_status = Command::new("lake")
        .arg("build")
        .arg("Boole.Family.V0Helpers")
        .arg("boole_check")
        .current_dir(&checker_dir)
        .status()
        .expect("run lake build on the real checker package");
    assert!(
        build_status.success(),
        "lake build of the real checker package must succeed before this test"
    );

    let dir = std::env::temp_dir().join(format!(
        "boole-soundness-accept-{}-{}",
        std::process::id(),
        unique_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create proof dir");
    let proof = dir.join("LenboundStyle.lean");
    std::fs::write(&proof, LENBOUND_STYLE_ACCEPTANCE_PROOF).expect("write proof");

    let runner = LeanRunner::new(
        LeanRunnerConfig::new("fixture-verifier-hash")
            .with_package_dir(checker_dir)
            .with_timeout_ms(60_000)
            .with_memory_limit_mb(8192),
    );
    let result = runner.check_file(&proof).expect("checker returns envelope");
    assert!(
        result.accepted,
        "a legitimate v1-lenbound-style proof importing the official helper \
         surface must still be accepted under the TB.1 soundness boundary: {result:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

fn assert_rejected(outcome: anyhow::Result<LeanCheckResult>, context: &str) {
    match outcome {
        // Rejected by the pre-spawn blacklist before `lake` ever ran.
        Err(_) => {}
        // Rejected by the primary checker or the axiom audit.
        Ok(result) => assert!(
            !result.accepted,
            "{context} must be rejected under the TB.1 soundness boundary: {result:?}"
        ),
    }
}

fn runner_for(workspace: &TestLeanWorkspace) -> LeanRunner {
    LeanRunner::new(
        LeanRunnerConfig::new("fixture-verifier-hash")
            .with_package_dir(workspace.root.clone())
            .with_timeout_ms(60_000)
            .with_memory_limit_mb(8192),
    )
}

fn real_checker_package_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../lean/checker")
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
            "boole-soundness-{name}-{}-{}",
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
        // TB.1 / ADR-0013 — `check_file` runs a second, separate process
        // (`lake env lean --run BooleCheck/Audit.lean`) after the primary
        // checker accepts, so this fixture needs its own copy of the real
        // audit script. `include_str!` pulls the production file in
        // verbatim at compile time so the fixture can never drift from
        // what actually ships.
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
