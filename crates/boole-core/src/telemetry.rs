//! P0.5 — minimal structured telemetry surface.
//!
//! L8 contract: every Boole binary calls [`init`] from `main` so a single
//! call site reaches the telemetry layer before any other work runs.
//! Later P0.5 slices wire this into a real `tracing` subscriber for
//! request IDs, panic hooks, and counters; the current slice is the
//! minimum that lets the contract test in
//! `scripts/test_telemetry_contract.py` find a public `init` entry point
//! and one proven caller without taking on a new build dependency that
//! would force a Cargo.lock churn.
//!
//! Boot emission is gated on `BOOLE_TELEMETRY_BOOT=1`. The default is
//! silent so binaries (e.g. `boole-node`) that contract on a clean
//! stderr keep that contract; opt-in surfaces the boot line for
//! operators who want it.
//!
//! `BinaryName` is an enum (not a `&str`) so a typo at the call site is a
//! compile error, satisfying the master plan's "typed boundaries" rule.

use std::sync::Once;

/// Identifies the calling binary in startup telemetry so a single log
/// stream multiplexed from several Boole processes stays attributable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryName {
    Node,
    Cli,
    Miner,
}

impl BinaryName {
    /// Stable on-the-wire identifier; never localized.
    pub fn as_str(self) -> &'static str {
        match self {
            BinaryName::Node => "boole-node",
            BinaryName::Cli => "boole-cli",
            BinaryName::Miner => "boole-miner",
        }
    }
}

static INIT: Once = Once::new();

/// Run telemetry boot. Idempotent — a second call (e.g. a binary that
/// re-enters `main` under a test harness) is a no-op so the record never
/// doubles. Emission is gated on `BOOLE_TELEMETRY_BOOT=1`; without it
/// the call is silent, which preserves the stderr-clean contract that
/// node/cli integration tests assert on.
pub fn init(name: BinaryName) {
    INIT.call_once(|| {
        if std::env::var("BOOLE_TELEMETRY_BOOT").as_deref() == Ok("1") {
            eprintln!(
                "boole.telemetry boot binary={} version={} pid={}",
                name.as_str(),
                env!("CARGO_PKG_VERSION"),
                std::process::id()
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_name_strings_are_stable() {
        assert_eq!(BinaryName::Node.as_str(), "boole-node");
        assert_eq!(BinaryName::Cli.as_str(), "boole-cli");
        assert_eq!(BinaryName::Miner.as_str(), "boole-miner");
    }

    #[test]
    fn init_is_idempotent_and_does_not_panic() {
        init(BinaryName::Node);
        init(BinaryName::Node);
    }
}
