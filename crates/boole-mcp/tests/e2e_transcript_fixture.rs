//! P2.2 closure criterion 3 — the external-user e2e smoke transcript.
//!
//! `docs/boole-mcp-e2e.md` walks an operator through install → `boole.status`
//! idle → `boole.mine` (zero-cycle) → `boole.status` completed. The §6.5 P2.2
//! criterion also asks for the transcript to be captured under
//! `tests/fixtures/boole-mcp-e2e/`. This test pins that fixture to the exact
//! envelope shapes the live binary produces.
//!
//! The LIVE verification (spawn `boole-mcp`, drive `/mcp/invoke`, assert the
//! envelopes) lives in `mining_tool_surface.rs`. This test pins the captured
//! transcript to the SAME constants so the doc + fixture cannot drift from the
//! contract `mining_tool_surface.rs` enforces against the real binary.

use serde_json::Value;

const TRANSCRIPT: &str = include_str!("../../../tests/fixtures/boole-mcp-e2e/transcript.v1.json");

fn transcript() -> Value {
    serde_json::from_str(TRANSCRIPT).expect("boole-mcp-e2e transcript is valid JSON")
}

#[test]
fn transcript_declares_closed_local_claim_boundary() {
    let t = transcript();
    assert_eq!(t["version"], 1);
    assert_eq!(t["domain"], "boole-mcp-e2e");
    assert_eq!(
        t["publicMiningEvidence"], false,
        "the e2e smoke is closed-local; it must never assert public-mining evidence"
    );
    assert_eq!(t["publicScoringEligible"], false);
    assert_eq!(
        t["claimBoundary"],
        "closed local smoke; not public-network mining"
    );
}

#[test]
fn transcript_status_idle_envelope_matches_contract() {
    let t = transcript();
    // mining_tool_surface.rs asserts the live binary returns {"state":"idle"}
    // before any boole.mine invocation.
    assert_eq!(t["expected"]["statusIdleBeforeMine"]["state"], "idle");
}

#[test]
fn transcript_mine_zero_cycle_counters_all_zero() {
    let t = transcript();
    // mining_tool_surface::invoke_boole_mine_zero_cycle_returns_protocol_summary_envelope_200
    // asserts every counter is 0 at max_cycles = 0.
    let m = &t["expected"]["mineZeroCycle"];
    for key in [
        "cycles_run",
        "tickets_found",
        "shares_accepted",
        "network_errors",
    ] {
        assert_eq!(
            m[key], 0,
            "mineZeroCycle.{key} must be 0 in the contract transcript"
        );
    }
}

#[test]
fn transcript_status_completed_carries_last_summary() {
    let t = transcript();
    let c = &t["expected"]["statusCompletedAfterMine"];
    assert_eq!(c["state"], "completed");
    let s = &c["last_summary"];
    for key in [
        "cycles_run",
        "tickets_found",
        "shares_accepted",
        "network_errors",
    ] {
        assert_eq!(
            s[key], 0,
            "statusCompletedAfterMine.last_summary.{key} must mirror the zero-cycle mine"
        );
    }
}
