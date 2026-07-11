//! SC.6 — `FamilyManifestRegistry` iteration is deterministic.
//!
//! Block production consumes `registry.iter()` (bounty promotion walk), so
//! iteration order is production-visible state: two nodes holding the same
//! manifest set must walk families in the same order regardless of the order
//! the manifests were loaded. The contract is iteration sorted by `family_id`.

use boole_core::{
    parse_family_manifest, FamilyManifest, FamilyManifestParseResult, FamilyManifestRegistry,
};
use serde_json::json;

fn manifest(family_id: &str) -> FamilyManifest {
    let v = json!({
        "version": "1",
        "familyId": family_id,
        "generatorHash": "abababababababababababababababababababababababababababababababab",
        "verifierHash": "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd",
        "canonicalizerHash": "efefefefefefefefefefefefefefefefefefefefefefefefefefefefefefefef",
        "promptSpecHash": "0101010101010101010101010101010101010101010101010101010101010101",
        "calibrationReportHash": "2323232323232323232323232323232323232323232323232323232323232323",
        "testVectorsHash": "4545454545454545454545454545454545454545454545454545454545454545",
        "resourceLimits": { "maxProofBytes": 16384, "verifyTimeoutMs": 30000, "maxDecls": 1024 },
        "rewardPolicy": { "mode": "no_protocol_reward", "maxBlockRewardShareBps": 0 },
        "activationHeight": u64::MAX,
        "status": "experimental"
    });
    match parse_family_manifest(&v) {
        FamilyManifestParseResult::Ok(m) => *m,
        FamilyManifestParseResult::Err(e) => panic!("parse {family_id}: {e}"),
    }
}

#[test]
fn family_registry_iteration_is_deterministic_across_load_orders() {
    let ids = [
        "test.kappa",
        "test.alpha",
        "test.omega",
        "test.delta",
        "test.sigma",
        "test.beta",
        "test.zeta",
        "test.gamma",
        "test.theta",
        "test.epsilon",
    ];

    let mut forward = FamilyManifestRegistry::new();
    for id in ids {
        forward.register(manifest(id));
    }
    let mut reverse = FamilyManifestRegistry::new();
    for id in ids.iter().rev() {
        reverse.register(manifest(id));
    }

    let forward_walk: Vec<String> = forward.iter().map(|m| m.family_id.clone()).collect();
    let reverse_walk: Vec<String> = reverse.iter().map(|m| m.family_id.clone()).collect();

    assert_eq!(
        forward_walk, reverse_walk,
        "iteration order must not depend on load order"
    );

    let mut sorted: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
    sorted.sort();
    assert_eq!(
        forward_walk, sorted,
        "iteration must be sorted by family_id so every node walks families identically"
    );
}
