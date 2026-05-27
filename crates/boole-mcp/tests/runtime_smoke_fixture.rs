//! P2.1 closure (slice 55) — closes criterion 3: the runtime-smoke
//! fixture is embedded into the `boole-mcp` binary via `include_bytes!`
//! so the binary has zero filesystem dependency on `fixtures/` at the
//! user's host.
//!
//! Contract pinned:
//!   * `boole_mcp::RUNTIME_SMOKE_FIXTURE_BYTES` is a non-empty byte
//!     slice (the file is ~10+ KB on disk; we require > 1 KB to catch
//!     accidental truncation).
//!   * The bytes parse as JSON.
//!   * `.version == 1` (matches the on-disk fixture's contract).
//!   * `.cfg.T_submit` is the canonical max-256-bit hex pin so a
//!     downstream slice can lift it into a `BigUint` threshold without
//!     a fresh on-disk read.
//!
//! Use of the embedded bytes by `default_in_process_inputs()` is held
//! for slice 56+ so this slice is purely additive — no behaviour change
//! to existing `boole.mine` / `boole.status` tests.

use serde_json::Value;

#[test]
fn runtime_smoke_fixture_is_embedded_and_parses_as_expected_v1_scenario() {
    let bytes: &[u8] = boole_mcp::RUNTIME_SMOKE_FIXTURE_BYTES;
    assert!(
        bytes.len() > 1024,
        "embedded fixture must be > 1 KB to catch truncation; got {}",
        bytes.len()
    );

    let v: Value = serde_json::from_slice(bytes).expect("embedded fixture parses as JSON");
    assert_eq!(v["version"], 1, "fixture version pin");
    assert_eq!(
        v["cfg"]["T_submit"], "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "T_submit canonical max-256-bit pin",
    );
}
