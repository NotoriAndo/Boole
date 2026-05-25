//! P2.1 closure (slice 41) — capture the build-time identifiers that
//! flow into `boole-mcp --version`:
//!
//!   * `BOOLE_MCP_GIT_SHA`  — short (12-char) HEAD SHA; `unknown` if
//!     `git rev-parse` is unavailable on the build host.
//!   * `BOOLE_MCP_BUILD_UTC` — ISO-8601 build timestamp from `date -u`,
//!     falling back to epoch seconds if `date` is unavailable.
//!
//! These are surfaced as `cargo:rustc-env=...` so `main.rs` can embed
//! them via `env!()` and clap's `version` attribute.

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let sha = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=BOOLE_MCP_GIT_SHA={sha}");

    let build_utc = Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs().to_string())
                .unwrap_or_else(|_| "unknown".to_string())
        });
    println!("cargo:rustc-env=BOOLE_MCP_BUILD_UTC={build_utc}");

    // Rebuild when HEAD moves so the embedded SHA stays current. The
    // .git/HEAD file is touched on every checkout/commit; if .git is
    // absent (e.g. source archive build), this hint is a no-op and the
    // SHA falls back to `unknown` above.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
}
