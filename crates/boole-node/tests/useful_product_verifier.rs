//! BF.5 — first useful-product verifier adapter (B4-limited scope).
//!
//! B4 rule: an adapter without a deterministic resource meter cannot be
//! activated for testnet receipt consensus, and the FIRST activatable
//! surface is the byte-exact digest comparison over a pinned packet
//! (the Lean verdict path already exists behind its own heartbeat/
//! recursion meters — SC.9). This slice therefore ships the adapter
//! interface + the `release-digest.v0` adapter: deterministic caps
//! first, declared-length check before any hashing (compile-bomb
//! guard), per-file SHA-256 against the packet's own manifest, and the
//! C7 split — a tampered PRESENT file is invalid, an ABSENT
//! hash-referenced file is retryable-unavailable, never invalid.
//! Golden inputs come ONLY from `fixtures/useful-product/golden/`
//! (BF.5-pre rule: tests never read local-docs).

use boole_node::{
    AdapterActivationSet, DeterministicBudget, PacketAuditOutcome, PacketAuditReject,
    PinnedPacketDigestAdapter, UsefulProductAdapter,
};
use boole_testkit::rand_suffix;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

const GOLDEN_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/useful-product/golden"
);

fn golden(path: &str) -> PathBuf {
    PathBuf::from(GOLDEN_DIR).join(path)
}

fn scratch_copy(source: &Path) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-bf5-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    copy_tree(source, &dir);
    dir
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("mkdir");
    for entry in fs::read_dir(from).expect("readable") {
        let entry = entry.expect("entry");
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("copy");
        }
    }
}

#[test]
fn digest_adapter_accepts_the_full_golden_llm_packet() {
    let adapter = PinnedPacketDigestAdapter;
    let outcome = adapter.audit(&golden("llm-mining-strong-r1/package"));
    let PacketAuditOutcome::Accepted {
        files_verified,
        bytes_verified,
    } = outcome
    else {
        panic!("golden packet must verify byte-exact, got {outcome:?}");
    };
    assert_eq!(files_verified, 15, "all 15 release files re-verified");
    assert!(bytes_verified > 0);
}

#[test]
fn absent_hash_referenced_files_are_unavailable_not_invalid() {
    // The supply-chain golden packet deliberately omits its >64 KiB
    // artifacts (hash references). C7: that is availability, never a
    // reject verdict — every PRESENT file still verifies byte-exact.
    let adapter = PinnedPacketDigestAdapter;
    let outcome = adapter.audit(&golden("supply-chain-poseidon/package"));
    let PacketAuditOutcome::RetryableUnavailable { missing } = outcome else {
        panic!("partial packet must be unavailable, got {outcome:?}");
    };
    assert_eq!(missing.len(), 5, "the five large artifacts are absent");
    assert!(missing.contains(&"setup/pot10_final.ptau".to_string()));
}

#[test]
fn a_single_tampered_byte_is_a_typed_rejection() {
    let dir = scratch_copy(&golden("llm-mining-strong-r1/package"));
    let target = dir.join("proof/public.json");
    let mut bytes = fs::read(&target).expect("read");
    bytes[0] ^= 0x01;
    fs::write(&target, bytes).expect("tamper");

    let outcome = PinnedPacketDigestAdapter.audit(&dir);
    let PacketAuditOutcome::Rejected(PacketAuditReject::DigestMismatch { path }) = outcome else {
        panic!("tampered byte must reject, got {outcome:?}");
    };
    assert_eq!(path, "proof/public.json");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_present_tamper_beats_missing_files() {
    // C7 ordering: invalid wins over unavailable — a packet with a
    // tampered present file is rejected even while other files are
    // still missing.
    let dir = scratch_copy(&golden("supply-chain-poseidon/package"));
    let target = dir.join("proof/proof.json");
    let mut bytes = fs::read(&target).expect("read");
    bytes[0] ^= 0x01;
    fs::write(&target, bytes).expect("tamper");

    let outcome = PinnedPacketDigestAdapter.audit(&dir);
    assert!(
        matches!(
            outcome,
            PacketAuditOutcome::Rejected(PacketAuditReject::DigestMismatch { .. })
        ),
        "tamper must beat unavailability, got {outcome:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn unlisted_files_and_release_only_submissions_are_rejected() {
    // An extra file the manifest does not pin: the packet is not the
    // byte-exact release any more.
    let dir = scratch_copy(&golden("llm-mining-strong-r1/package"));
    fs::write(dir.join("bonus.txt"), b"smuggled").expect("write");
    let outcome = PinnedPacketDigestAdapter.audit(&dir);
    assert!(
        matches!(
            outcome,
            PacketAuditOutcome::Rejected(PacketAuditReject::UnlistedFile { .. })
        ),
        "unlisted file must reject, got {outcome:?}"
    );
    let _ = fs::remove_dir_all(&dir);

    // "release만 제출" — a packet with a manifest but none of the pinned
    // source files is not verifiable source-first work; with every file
    // absent it is pure unavailability with nothing verified, which the
    // caller must never treat as an accept.
    let dir = std::env::temp_dir().join(format!(
        "boole-bf5-empty-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    fs::create_dir_all(&dir).expect("mkdir");
    fs::copy(
        golden("llm-mining-strong-r1/package/manifest.json"),
        dir.join("manifest.json"),
    )
    .expect("copy manifest");
    let outcome = PinnedPacketDigestAdapter.audit(&dir);
    let PacketAuditOutcome::RetryableUnavailable { missing } = outcome else {
        panic!("manifest-only packet is fully unavailable, got {outcome:?}");
    };
    assert_eq!(missing.len(), 15);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn declared_length_mismatch_rejects_before_hashing() {
    // Compile-bomb guard (B4): the declared byte length is checked
    // against file metadata BEFORE any content is read, so an inflated
    // file cannot buy unbounded hashing work.
    let dir = scratch_copy(&golden("llm-mining-strong-r1/package"));
    let target = dir.join("source/main.circom");
    let mut bytes = fs::read(&target).expect("read");
    bytes.extend_from_slice(b"\n// padding beyond the declared length");
    fs::write(&target, bytes).expect("inflate");

    let outcome = PinnedPacketDigestAdapter.audit(&dir);
    let PacketAuditOutcome::Rejected(PacketAuditReject::DeclaredLengthMismatch { path }) = outcome
    else {
        panic!("length mismatch must reject, got {outcome:?}");
    };
    assert_eq!(path, "source/main.circom");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn budget_caps_are_deterministic_and_enforced() {
    let adapter = PinnedPacketDigestAdapter;
    let budget = adapter
        .resource_meter()
        .expect("the digest adapter is metered");
    assert_eq!(budget.max_total_bytes, 8 * 1024 * 1024, "8 MiB packet cap");
    assert!(budget.max_files >= 19, "must fit the golden packets");

    // A manifest declaring more than the cap rejects WITHOUT reading
    // any artifact bytes (the declaration alone breaks the budget).
    let dir = std::env::temp_dir().join(format!(
        "boole-bf5-oversize-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    fs::create_dir_all(&dir).expect("mkdir");
    let manifest: Value = serde_json::json!({
        "schema": "test.oversize",
        "release_root": "00",
        "files": [{
            "path": "huge.bin",
            "bytes": 9 * 1024 * 1024,
            "sha256": "a".repeat(64),
        }]
    });
    fs::write(
        dir.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .expect("write manifest");
    let outcome = adapter.audit(&dir);
    assert!(
        matches!(
            outcome,
            PacketAuditOutcome::Rejected(PacketAuditReject::BudgetExceeded { .. })
        ),
        "declared oversize must reject, got {outcome:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn missing_or_malformed_manifest_is_a_typed_rejection() {
    let dir = std::env::temp_dir().join(format!(
        "boole-bf5-nomanifest-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    fs::create_dir_all(&dir).expect("mkdir");
    let outcome = PinnedPacketDigestAdapter.audit(&dir);
    assert!(
        matches!(
            outcome,
            PacketAuditOutcome::Rejected(PacketAuditReject::ManifestUnreadable { .. })
        ),
        "missing manifest must reject, got {outcome:?}"
    );
    fs::write(dir.join("manifest.json"), b"not json").expect("write");
    let outcome = PinnedPacketDigestAdapter.audit(&dir);
    assert!(
        matches!(
            outcome,
            PacketAuditOutcome::Rejected(PacketAuditReject::ManifestUnreadable { .. })
        ),
        "malformed manifest must reject, got {outcome:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn meterless_adapters_cannot_join_receipt_consensus() {
    // B4 activation ban: no deterministic meter, no consensus role.
    struct MeterlessAdapter;
    impl UsefulProductAdapter for MeterlessAdapter {
        fn adapter_id(&self) -> &'static str {
            "wallclock-only.v0"
        }
        fn resource_meter(&self) -> Option<DeterministicBudget> {
            None
        }
        fn audit(&self, _packet_dir: &Path) -> PacketAuditOutcome {
            PacketAuditOutcome::RetryableUnavailable { missing: vec![] }
        }
    }

    let mut set = AdapterActivationSet::default();
    set.activate(Box::new(PinnedPacketDigestAdapter))
        .expect("metered adapter activates");
    let err = set.activate(Box::new(MeterlessAdapter)).unwrap_err();
    assert!(
        err.to_string().contains("deterministic resource meter"),
        "unexpected error: {err}"
    );
    assert!(set.is_active("release-digest.v0"));
    assert!(!set.is_active("wallclock-only.v0"));
}
