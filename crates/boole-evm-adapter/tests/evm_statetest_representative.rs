//! Representative feasibility test (operator msg 3459).
//!
//! Runs ONE official EVM state-transition test
//! (`test_random_statetest179`, ported from
//! `state_tests/stRandom/randomStatetest179Filler.json`) through the
//! common adapter seam against the pinned execution-specs engine and
//! proves: valid → Accepted, and input / expected-output / final-state
//! tampering each reject with its own distinct reason.
//!
//! The engine (`ethereum-spec-evm`) lives under a gitignored local
//! checkout, so it cannot run in CI. The test locates it via the
//! `BOOLE_EVM_ENGINE` env var and SKIPS (compiles, does not execute) when
//! that is unset — CI stays green; the representative is proven locally.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use boole_evm_adapter::{AuditOutcome, AuditReject, EvmStateTestAdapter, UsefulProductAdapter};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/statetest179")
}

/// Copy the pinned fixture into a fresh temp dir so a test can tamper one
/// file without touching the committed golden.
fn copy_fixture() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("boole-evm-{}-{}", std::process::id(), n));
    std::fs::create_dir_all(&dir).expect("mkdir temp packet");
    for name in ["alloc.json", "env.json", "txs.json", "packet.json"] {
        std::fs::copy(fixture_dir().join(name), dir.join(name))
            .unwrap_or_else(|e| panic!("copy {name}: {e}"));
    }
    dir
}

fn tamper(dir: &Path, file: &str, from: &str, to: &str) {
    let path = dir.join(file);
    let s = std::fs::read_to_string(&path).expect("read to tamper");
    assert!(
        s.contains(from),
        "tamper anchor {from:?} not found in {file}"
    );
    std::fs::write(&path, s.replacen(from, to, 1)).expect("write tampered");
}

fn engine() -> Option<EvmStateTestAdapter> {
    let bin = std::env::var_os("BOOLE_EVM_ENGINE")?;
    Some(EvmStateTestAdapter::new(PathBuf::from(bin)))
}

#[test]
fn evm_statetest179_accept_and_three_distinct_tamper_rejects() {
    let Some(adapter) = engine() else {
        eprintln!(
            "SKIP: BOOLE_EVM_ENGINE unset — the execution-specs engine is a \
             gitignored local checkout and cannot run in CI. Run locally with \
             BOOLE_EVM_ENGINE=<path to ethereum-spec-evm>."
        );
        return;
    };

    // (1) VALID → Accepted, with the pinned golden state root.
    let out = adapter.audit(&fixture_dir());
    match &out {
        AuditOutcome::Accepted { state_root } => {
            assert_eq!(
                state_root, "0xf1bf91e7c82c302c2e5b1a368117587fbb6b59ddab54dc5adf066d0968898609",
                "accepted state root must equal the pinned golden"
            );
        }
        other => panic!("valid representative must be Accepted, got {other:?}"),
    }

    // (2) INPUT TAMPER — mutate a byte of alloc.json (sender balance).
    //     Rejects on the input-digest gate, BEFORE the engine runs.
    let d = copy_fixture();
    tamper(&d, "alloc.json", "0xDE0B6B3A7640000", "0xDE0B6B3A7640001");
    assert!(
        matches!(
            adapter.audit(&d),
            AuditOutcome::Rejected(AuditReject::InputDigestMismatch { .. })
        ),
        "input tamper must reject as InputDigestMismatch, got {:?}",
        adapter.audit(&d)
    );

    // (3) EXPECTED-OUTPUT TAMPER — mutate the declared post storage value.
    //     Input digest still matches; the engine's true post ≠ declared.
    let d = copy_fixture();
    tamper(
        &d,
        "packet.json",
        "0x515480126a50a173506e066762129255",
        "0x515480126a50a173506e066762129256",
    );
    assert!(
        matches!(
            adapter.audit(&d),
            AuditOutcome::Rejected(AuditReject::ExpectedPostAllocMismatch { .. })
        ),
        "expected-output tamper must reject as ExpectedPostAllocMismatch, got {:?}",
        adapter.audit(&d)
    );

    // (4) FINAL-STATE TAMPER — mutate the declared state root only.
    //     post_alloc still matches; the state-root gate catches it.
    let d = copy_fixture();
    tamper(
        &d,
        "packet.json",
        "0xf1bf91e7c82c302c2e5b1a368117587fbb6b59ddab54dc5adf066d0968898609",
        "0xf1bf91e7c82c302c2e5b1a368117587fbb6b59ddab54dc5adf066d0968898600",
    );
    assert!(
        matches!(
            adapter.audit(&d),
            AuditOutcome::Rejected(AuditReject::ExpectedStateRootMismatch { .. })
        ),
        "final-state tamper must reject as ExpectedStateRootMismatch, got {:?}",
        adapter.audit(&d)
    );
}
