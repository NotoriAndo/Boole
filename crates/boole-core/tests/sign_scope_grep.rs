// P2.10 (ADR-0003) §92 enforcement gate. This test walks every `.rs`
// file under `crates/**/src/**` and asserts the strict invariant: a
// production source line that calls `.sign(` MUST either be a
// `.sign_for_network(` call, or carry the literal `// P2.10-exempt`
// annotation justifying why it stays on the legacy unscoped digest.
//
// Test scaffolding under `tests/`, `benches/`, `examples/` is excluded
// by ADR-0003 (the testnet-path scope is defined as production code
// that constructs a `boole.signed.v1` envelope with a bounty/signer
// payload schema).
//
// This file lives in `boole-core` because every workspace crate
// depends on `boole-core`, so a CI break here is guaranteed to fire
// from any signed-envelope-producing crate's pipeline.

use std::path::{Path, PathBuf};

#[test]
fn p2_10_no_unannotated_sign_calls_in_production_src() {
    let crates_root = workspace_crates_root();
    let mut offenders: Vec<String> = Vec::new();
    for src_file in collect_production_src_files(&crates_root) {
        let contents = std::fs::read_to_string(&src_file)
            .unwrap_or_else(|e| panic!("read {}: {}", src_file.display(), e));
        for (idx, line) in contents.lines().enumerate() {
            if !line_has_dot_sign_call(line) {
                continue;
            }
            if line.contains("// P2.10-exempt") {
                continue;
            }
            offenders.push(format!(
                "{}:{}: {}",
                relative_to(&crates_root, &src_file),
                idx + 1,
                line.trim()
            ));
        }
    }
    assert!(
        offenders.is_empty(),
        "P2.10 (ADR-0003) gate: found {} unannotated `.sign(` call(s) in production src; \
         either migrate the site to `sign_for_network(payload, network_id)` or add the \
         literal `// P2.10-exempt: <reason>` annotation on the same line per ADR-0003:\n{}",
        offenders.len(),
        offenders.join("\n"),
    );
}

#[test]
fn p2_10_keys_sign_exemption_annotation_is_present() {
    // ADR-0003 names `keys.sign` at `crates/boole-cli/src/main.rs` as
    // the binding exemption. If this annotation ever disappears, the
    // exemption is no longer in the source and the policy must be
    // re-evaluated.
    let main_rs = workspace_crates_root().join("boole-cli/src/main.rs");
    let contents = std::fs::read_to_string(&main_rs)
        .unwrap_or_else(|e| panic!("read {}: {}", main_rs.display(), e));
    assert!(
        contents.contains("P2.10-exempt: user-utility, see ADR-0003"),
        "expected `keys.sign` site to carry the binding ADR-0003 annotation \
         `// P2.10-exempt: user-utility, see ADR-0003` in {}",
        main_rs.display()
    );
}

fn workspace_crates_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .expect("CARGO_MANIFEST_DIR has a parent")
        .to_path_buf()
}

fn collect_production_src_files(crates_root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for crate_entry in std::fs::read_dir(crates_root).expect("read crates/") {
        let crate_entry = crate_entry.expect("crate dirent");
        let src_dir = crate_entry.path().join("src");
        if !src_dir.is_dir() {
            continue;
        }
        walk_rust_files(&src_dir, &mut out);
    }
    out.sort();
    out
}

fn walk_rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read {}: {}", dir.display(), e))
    {
        let entry = entry.expect("dirent");
        let path = entry.path();
        if path.is_dir() {
            walk_rust_files(&path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

fn line_has_dot_sign_call(line: &str) -> bool {
    // Match a literal `.sign(` token that is not part of `.sign_for_network(`,
    // `.signature(`, `.signer(`, `.signed(`, etc. We anchor on the exact
    // 6-char sequence `.sign(` — `sign_for_network` starts with `.sign_`
    // (underscore, not open-paren), so a naive `contains(".sign(")` already
    // discriminates correctly.
    line.contains(".sign(")
}

fn relative_to(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}
