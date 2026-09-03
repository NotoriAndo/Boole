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
        "resourceLimits": { "maxProofBytes": 16384, "verifyTimeoutMs": 30000, "maxDecls": 1024, "maxHeartbeats": 400000, "maxRecDepth": 512 },
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

#[test]
fn family_root_matches_golden_vector() {
    let mut forward = FamilyManifestRegistry::new();
    forward.register(manifest("test.zeta"));
    forward.register(manifest("test.alpha"));

    let mut reverse = FamilyManifestRegistry::new();
    reverse.register(manifest("test.alpha"));
    reverse.register(manifest("test.zeta"));

    let expected_bytes = concat!(
        r#"[{"activationHeight":18446744073709551615,"calibrationReportHash":"2323232323232323232323232323232323232323232323232323232323232323","canonicalizerHash":"efefefefefefefefefefefefefefefefefefefefefefefefefefefefefefefef","familyId":"test.alpha","generatorHash":"abababababababababababababababababababababababababababababababab","promptSpecHash":"0101010101010101010101010101010101010101010101010101010101010101","resourceLimits":{"maxDecls":1024,"maxHeartbeats":400000,"maxProofBytes":16384,"maxRecDepth":512,"verifyTimeoutMs":30000},"rewardPolicy":{"maxBlockRewardShareBps":0,"mode":"no_protocol_reward"},"status":"experimental","testVectorsHash":"4545454545454545454545454545454545454545454545454545454545454545","verifierHash":"cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd","version":"1"},"#,
        r#"{"activationHeight":18446744073709551615,"calibrationReportHash":"2323232323232323232323232323232323232323232323232323232323232323","canonicalizerHash":"efefefefefefefefefefefefefefefefefefefefefefefefefefefefefefefef","familyId":"test.zeta","generatorHash":"abababababababababababababababababababababababababababababababab","promptSpecHash":"0101010101010101010101010101010101010101010101010101010101010101","resourceLimits":{"maxDecls":1024,"maxHeartbeats":400000,"maxProofBytes":16384,"maxRecDepth":512,"verifyTimeoutMs":30000},"rewardPolicy":{"maxBlockRewardShareBps":0,"mode":"no_protocol_reward"},"status":"experimental","testVectorsHash":"4545454545454545454545454545454545454545454545454545454545454545","verifierHash":"cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd","version":"1"}]"#,
    )
    .as_bytes();

    assert_eq!(forward.canonical_bytes(), expected_bytes);
    assert_eq!(reverse.canonical_bytes(), expected_bytes);
    assert_eq!(
        forward.root().to_hex(),
        "ba92240514354a09111a350baed799b09f32f0d608c1596c68f5f8b4d99fbcf6"
    );
    assert_eq!(reverse.root(), forward.root());
}
