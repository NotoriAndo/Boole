//! BF.1a — closed protocol-owned useful-task registry admission (B3).
//!
//! The registry is the supply inlet for the useful-work lane: only three
//! task sources are admissible (protocol-owned K-LADDER, pinned real ZK
//! release, strict-ready adapter card), only the protocol authority can
//! register, and at epoch cutoff the eligible list + sort rule + registry
//! root are frozen — that frozen list is BF.2's forced-assignment input
//! (B2 precondition). Permissionless registration, bonds, challenges and
//! governance are explicit non-goals (future ADR, post-BF.8).

use boole_core::useful_task_registry::{
    RegistryEntry, RegistryError, SpecFidelity, TaskSource, UsefulTaskRegistry,
};
use boole_core::Hex32;
use serde_json::{json, Value};

const FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/protocol/useful-work/registry-v0.json"
);

fn digest(byte: u8) -> String {
    hex::encode([byte; 32])
}

fn commit(byte: u8) -> String {
    hex::encode([byte; 20])
}

fn authority() -> Hex32 {
    Hex32::from_hex(&digest(0xaa)).unwrap()
}

fn task_json(spec_id: &str, variant_id: &str) -> Value {
    json!({
        "specId": spec_id,
        "variantId": variant_id,
        "componentId": "full-round",
        "propertyId": "constraint-completeness",
        "specVersion": 1,
        "taskKind": {
            "kind": "auditExisting",
            "inputArtifactDigest": digest(0x11),
            "targetReleaseDigest": digest(0x22)
        }
    })
}

fn entry_json(spec_id: &str, variant_id: &str, source: Value) -> Value {
    json!({
        "task": task_json(spec_id, variant_id),
        "source": source,
        "specFidelity": "strictAudited",
        "eligible": true
    })
}

fn k_ladder_source() -> Value {
    json!({ "kind": "protocolKLadder", "ladderId": "k-ladder-sumcheck", "step": 3 })
}

fn pinned_release_source() -> Value {
    json!({
        "kind": "pinnedZkRelease",
        "repoUrl": "https://github.com/iden3/circomlib",
        "commit": commit(0x77),
        "sourceHash": digest(0x88),
        "license": "GPL-3.0"
    })
}

fn adapter_source() -> Value {
    json!({
        "kind": "strictReadyAdapter",
        "adapterCardId": "fflonk-round-trip-r5",
        "goldenDigest": digest(0x99)
    })
}

fn entry(spec_id: &str, variant_id: &str, source: Value) -> RegistryEntry {
    RegistryEntry::from_json_value(&entry_json(spec_id, variant_id, source)).expect("valid entry")
}

#[test]
fn only_the_protocol_authority_can_register() {
    let mut registry = UsefulTaskRegistry::new(authority());
    let outsider = Hex32::from_hex(&digest(0xbb)).unwrap();
    let err = registry
        .register(entry("spec-a", "v1", k_ladder_source()), outsider)
        .unwrap_err();
    assert_eq!(err.label(), "not-protocol-authority");
    registry
        .register(entry("spec-a", "v1", k_ladder_source()), authority())
        .expect("authority registers");
}

#[test]
fn registration_after_freeze_is_rejected_and_root_is_immutable() {
    let mut registry = UsefulTaskRegistry::new(authority());
    registry
        .register(entry("spec-a", "v1", k_ladder_source()), authority())
        .expect("register before cutoff");
    let root = registry.freeze().expect("freeze at cutoff");

    // Seed is public now — late registration must be rejected...
    let err = registry
        .register(entry("spec-b", "v1", adapter_source()), authority())
        .unwrap_err();
    assert_eq!(err.label(), "registration-closed");
    // ...and the frozen root must not move.
    assert_eq!(registry.registry_root().expect("root after freeze"), root);
    // Double freeze is a typed error, not a silent re-root.
    assert_eq!(registry.freeze().unwrap_err().label(), "already-frozen");
}

#[test]
fn unknown_source_kind_and_unknown_fields_are_rejected() {
    let bad_kind = entry_json(
        "spec-a",
        "v1",
        json!({ "kind": "communityUpload", "url": "https://example.com" }),
    );
    let err = RegistryEntry::from_json_value(&bad_kind).unwrap_err();
    assert_eq!(err.label(), "unknown-task-source");

    let mut extra = entry_json("spec-a", "v1", k_ladder_source());
    extra
        .as_object_mut()
        .unwrap()
        .insert("governanceNote".into(), json!("approve me"));
    let err = RegistryEntry::from_json_value(&extra).unwrap_err();
    assert_eq!(err.label(), "malformed-json");
}

#[test]
fn pinned_release_requires_immutable_commit_and_license() {
    // Mutable ref instead of a 40-hex commit SHA => not a pinned source.
    let mut floating = entry_json("spec-a", "v1", pinned_release_source());
    floating["source"]["commit"] = json!("main");
    let err = RegistryEntry::from_json_value(&floating).unwrap_err();
    assert_eq!(err.label(), "invalid-commit");

    // Missing license => provenance incomplete.
    let mut unlicensed = entry_json("spec-a", "v1", pinned_release_source());
    unlicensed["source"]["license"] = json!("");
    let err = RegistryEntry::from_json_value(&unlicensed).unwrap_err();
    assert_eq!(err.label(), "empty-field");

    // Source hash must be a strict 32-byte digest.
    let mut unpinned = entry_json("spec-a", "v1", pinned_release_source());
    unpinned["source"]["sourceHash"] = json!("deadbeef");
    let err = RegistryEntry::from_json_value(&unpinned).unwrap_err();
    assert_eq!(err.label(), "invalid-digest");

    // Repo URL must be an https origin, not an arbitrary string.
    let mut odd_url = entry_json("spec-a", "v1", pinned_release_source());
    odd_url["source"]["repoUrl"] = json!("ftp://mirror.example/zk");
    let err = RegistryEntry::from_json_value(&odd_url).unwrap_err();
    assert_eq!(err.label(), "invalid-repo-url");
}

#[test]
fn eligible_requires_strict_audited_spec_fidelity() {
    let mut pending = entry_json("spec-a", "v1", k_ladder_source());
    pending["specFidelity"] = json!("pending");
    let err = RegistryEntry::from_json_value(&pending).unwrap_err();
    assert_eq!(err.label(), "eligible-requires-strict-audit");

    // Pending fidelity is fine as long as the entry stays ineligible.
    let mut parked = entry_json("spec-b", "v1", k_ladder_source());
    parked["specFidelity"] = json!("pending");
    parked["eligible"] = json!(false);
    let parked = RegistryEntry::from_json_value(&parked).expect("ineligible pending entry");
    assert_eq!(parked.spec_fidelity, SpecFidelity::Pending);
    assert!(!parked.eligible);
}

#[test]
fn duplicate_task_and_meaningless_variant_are_rejected() {
    let mut registry = UsefulTaskRegistry::new(authority());
    registry
        .register(entry("spec-a", "v1", pinned_release_source()), authority())
        .expect("first registration");

    // Same TaskSpecIdentity twice.
    let err = registry
        .register(entry("spec-a", "v1", pinned_release_source()), authority())
        .unwrap_err();
    assert_eq!(err.label(), "duplicate-task");

    // Variant renamed but bound to the same source bytes => no new supply.
    let err = registry
        .register(entry("spec-a", "v2", pinned_release_source()), authority())
        .unwrap_err();
    assert_eq!(err.label(), "meaningless-variant");

    // A different pinned source is a genuinely different problem.
    let mut other = pinned_release_source();
    other["sourceHash"] = json!(digest(0x89));
    other["commit"] = json!(commit(0x78));
    registry
        .register(entry("spec-a", "v2-halo2", other), authority())
        .expect("distinct source registers");
}

#[test]
fn registry_root_is_order_independent_and_deterministic() {
    let entries = [
        entry("spec-a", "v1", k_ladder_source()),
        entry("spec-b", "v1", pinned_release_source()),
        entry("spec-c", "v1", adapter_source()),
    ];

    let mut forward = UsefulTaskRegistry::new(authority());
    for e in entries.iter() {
        forward.register(e.clone(), authority()).expect("register");
    }
    let root_forward = forward.freeze().expect("freeze");

    let mut reverse = UsefulTaskRegistry::new(authority());
    for e in entries.iter().rev() {
        reverse.register(e.clone(), authority()).expect("register");
    }
    let root_reverse = reverse.freeze().expect("freeze");

    assert_eq!(
        root_forward, root_reverse,
        "registration order must not change the frozen root"
    );

    // The frozen eligible list is the BF.2 assignment input: sorted by
    // task_id ascending, so every node derives the identical ordering.
    let eligible = forward.eligible_tasks().expect("eligible after freeze");
    let mut sorted = eligible.clone();
    sorted.sort_by_key(|task| task.task_id());
    assert_eq!(eligible, sorted);
    assert_eq!(eligible.len(), 3);
}

#[test]
fn ineligible_entries_stay_out_of_the_frozen_assignment_list() {
    let mut registry = UsefulTaskRegistry::new(authority());
    registry
        .register(entry("spec-a", "v1", k_ladder_source()), authority())
        .expect("eligible entry");
    let mut parked = entry_json("spec-b", "v1", adapter_source());
    parked["specFidelity"] = json!("pending");
    parked["eligible"] = json!(false);
    registry
        .register(
            RegistryEntry::from_json_value(&parked).expect("ineligible entry"),
            authority(),
        )
        .expect("ineligible entry registers");
    registry.freeze().expect("freeze");
    let eligible = registry.eligible_tasks().expect("eligible list");
    assert_eq!(eligible.len(), 1);
    assert_eq!(eligible[0].spec_id, "spec-a");
}

#[test]
fn eligible_tasks_are_unavailable_before_freeze() {
    let registry = UsefulTaskRegistry::new(authority());
    assert_eq!(
        registry.eligible_tasks().unwrap_err().label(),
        "registry-not-frozen"
    );
    assert_eq!(
        registry.registry_root().unwrap_err().label(),
        "registry-not-frozen"
    );
}

#[test]
fn golden_registry_fixture_is_stable() {
    let fixture: Value =
        serde_json::from_str(&std::fs::read_to_string(FIXTURE_PATH).expect("fixture readable"))
            .expect("fixture parses");
    let mut registry = UsefulTaskRegistry::new(
        Hex32::from_hex(fixture["authority"].as_str().expect("authority")).expect("authority hex"),
    );
    let auth = Hex32::from_hex(fixture["authority"].as_str().unwrap()).unwrap();
    for case in fixture["entries"].as_array().expect("entries") {
        let entry = RegistryEntry::from_json_value(case).expect("fixture entry valid");
        registry.register(entry, auth).expect("fixture registers");
    }
    let root = registry.freeze().expect("freeze");
    assert_eq!(
        root.to_hex(),
        fixture["expectedRegistryRoot"].as_str().expect("root"),
        "frozen registry root must match the golden fixture"
    );
    let eligible: Vec<String> = registry
        .eligible_tasks()
        .expect("eligible")
        .iter()
        .map(|task| task.task_id().to_hex())
        .collect();
    let expected: Vec<String> = fixture["expectedEligibleTaskIds"]
        .as_array()
        .expect("expectedEligibleTaskIds")
        .iter()
        .map(|v| v.as_str().expect("task id").to_string())
        .collect();
    assert_eq!(eligible, expected);
    for case in fixture["rejectedEntries"]
        .as_array()
        .expect("rejectedEntries")
    {
        let err = RegistryEntry::from_json_value(&case["entry"]).unwrap_err();
        assert_eq!(err.label(), case["reason"].as_str().expect("reason"));
    }
}

/// Regen helper mirroring repo conventions — rewrites the golden fixture
/// in place from the in-code catalog.
#[test]
#[ignore = "regen helper: cargo test -p boole-core --test useful_task_registry regen_registry_golden_fixture -- --ignored"]
fn regen_registry_golden_fixture() {
    let catalog = vec![
        entry_json("k-ladder-sumcheck-step3", "v1", k_ladder_source()),
        entry_json(
            "poseidon-circomlib",
            "circom-bn254",
            pinned_release_source(),
        ),
        entry_json("fflonk-serialization", "r5-card", adapter_source()),
    ];
    let auth = authority();
    let mut registry = UsefulTaskRegistry::new(auth);
    for case in &catalog {
        registry
            .register(RegistryEntry::from_json_value(case).expect("valid"), auth)
            .expect("register");
    }
    let root = registry.freeze().expect("freeze");
    let eligible: Vec<String> = registry
        .eligible_tasks()
        .expect("eligible")
        .iter()
        .map(|task| task.task_id().to_hex())
        .collect();

    let mut rejected_kind = entry_json("spec-x", "v1", k_ladder_source());
    rejected_kind["source"] = json!({ "kind": "communityUpload", "url": "https://example.com" });
    let mut rejected_commit = entry_json("spec-y", "v1", pinned_release_source());
    rejected_commit["source"]["commit"] = json!("main");
    let mut rejected_fidelity = entry_json("spec-z", "v1", adapter_source());
    rejected_fidelity["specFidelity"] = json!("pending");

    let fixture = json!({
        "domain": "boole.useful-work.registry.v0",
        "authority": auth.to_hex(),
        "entries": catalog,
        "expectedRegistryRoot": root.to_hex(),
        "expectedEligibleTaskIds": eligible,
        "rejectedEntries": [
            { "entry": rejected_kind, "reason": "unknown-task-source" },
            { "entry": rejected_commit, "reason": "invalid-commit" },
            { "entry": rejected_fidelity, "reason": "eligible-requires-strict-audit" },
        ],
    });
    let pretty = format!("{}\n", serde_json::to_string_pretty(&fixture).unwrap());
    std::fs::write(FIXTURE_PATH, pretty).expect("write fixture");
}

#[test]
fn source_variants_are_typed() {
    // The three admissible source classes are a closed enum — sanity-pin
    // that each parses into its own variant.
    let k = entry("spec-a", "v1", k_ladder_source());
    assert!(matches!(k.source, TaskSource::ProtocolKLadder { .. }));
    let p = entry("spec-b", "v1", pinned_release_source());
    assert!(matches!(p.source, TaskSource::PinnedZkRelease { .. }));
    let a = entry("spec-c", "v1", adapter_source());
    assert!(matches!(a.source, TaskSource::StrictReadyAdapter { .. }));
    let _ = RegistryError::RegistrationClosed.label();
}
