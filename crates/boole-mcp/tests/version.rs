//! P2.1 closure (slice 41) — `boole-mcp --version` must emit the
//! canonical identifier line so an external operator can pin down
//! exactly which binary is registered into their IDE config.
//!
//! Contract (per §6.5 P2.1): `boole-mcp <ver> (sha=<git> build=<utc>)`,
//! where `<ver>` is the crate version, `<git>` is the short HEAD SHA
//! captured at build time, and `<utc>` is the build UTC timestamp.

use std::path::PathBuf;
use std::process::Command;

fn bin_path() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p.push("target");
    p.push(if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    });
    p.push("boole-mcp");
    p
}

#[test]
fn version_flag_emits_canonical_line_with_sha_and_build_utc() {
    let out = Command::new(bin_path())
        .arg("--version")
        .output()
        .expect("spawn boole-mcp --version");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = String::from_utf8_lossy(&out.stdout).to_string();
    let line = s.trim();
    assert!(
        line.starts_with("boole-mcp "),
        "version line must start with `boole-mcp `: {line:?}"
    );
    assert!(
        line.contains("(sha="),
        "version line must include `(sha=`: {line:?}"
    );
    assert!(
        line.contains(" build="),
        "version line must include ` build=`: {line:?}"
    );
    assert!(
        line.ends_with(')'),
        "version line must end with `)`: {line:?}"
    );
    // SHA token must be non-empty (we tolerate `unknown` when git is
    // unavailable on the build host, but never an empty placeholder).
    let sha_section = line.split("(sha=").nth(1).expect("sha section");
    let sha_token = sha_section.split_whitespace().next().expect("sha token");
    assert!(
        !sha_token.is_empty(),
        "sha token must be non-empty: {line:?}"
    );
}
