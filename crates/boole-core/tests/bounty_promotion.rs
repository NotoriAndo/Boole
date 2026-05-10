//! S22b — `select_promoted_bounty_shares(side_pool, registry, runtime_height, operator_pks)`
//! is the activation gate that decides which side-pool shares may enter
//! `build_block_selection`'s `promoted_bounty_shares` argument.
//!
//! Gates (all must pass):
//!   1. `manifest.activation_height ≤ runtime_height`,
//!   2. `manifest.signature` present,
//!   3. signature verifies against at least one operator pk in `operator_pks`,
//!   4. `manifest.caps.max_shares_per_block ≥ 1` (caps absent → no promotion).
//!
//! When all gates pass, the helper emits up to `caps.max_shares_per_block`
//! shares per family (FIFO order from the side-pool), mapped from
//! `BountyShare` to `PromotedBountyShare`.

use boole_core::{
    parse_family_manifest, select_promoted_bounty_selection, select_promoted_bounty_shares,
    BountyShare, BountySidePool, FamilyCaps, FamilyManifest, FamilyManifestParseResult,
    FamilyManifestRegistry, SigningKeyV2,
};
use serde_json::{json, Value};

const FAMILY_ID: &str = "test.gamma";
const ALT_FAMILY_ID: &str = "test.delta";

fn manifest_value(family_id: &str, activation_height: u64, max_shares: u64) -> Value {
    manifest_value_with_credit(family_id, activation_height, max_shares, "0")
}

fn manifest_value_with_credit(
    family_id: &str,
    activation_height: u64,
    max_shares: u64,
    max_credit: &str,
) -> Value {
    json!({
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
        "activationHeight": activation_height,
        "status": "experimental",
        "caps": {
            "maxSharesPerBlock": max_shares,
            "maxScoreMultiplierBps": 10000,
            "maxRewardCreditPerBlock": max_credit
        }
    })
}

fn parse(value: &Value) -> FamilyManifest {
    match parse_family_manifest(value) {
        FamilyManifestParseResult::Ok(m) => *m,
        FamilyManifestParseResult::Err(e) => panic!("parse: {e}"),
    }
}

fn signed_manifest(family_id: &str, activation_height: u64, max_shares: u64) -> (FamilyManifest, String) {
    signed_manifest_with_credit(family_id, activation_height, max_shares, "0")
}

fn signed_manifest_with_credit(
    family_id: &str,
    activation_height: u64,
    max_shares: u64,
    max_credit: &str,
) -> (FamilyManifest, String) {
    let key = SigningKeyV2::from_dev_id(&format!("op-{family_id}"));
    let body = manifest_value_with_credit(family_id, activation_height, max_shares, max_credit);
    let mut manifest = parse(&body);
    let envelope = key.sign(&serde_json::to_value(&manifest).unwrap()).unwrap();
    manifest.signature = Some(envelope.signature);
    (manifest, key.pk_hex())
}

fn make_share(family_id: &str, idx: u8) -> BountyShare {
    BountyShare {
        bounty_id: format!("{family_id}-b{idx}"),
        proof_hash: format!("{:064x}", idx as u128 * 0x11),
        prover: format!("{:064x}", idx as u128 * 0x101),
        family_id: family_id.to_string(),
        ts: 1_700_000_000_000 + idx as u64,
        reward: 0,
    }
}

fn make_share_with_reward(family_id: &str, idx: u8, reward: u128) -> BountyShare {
    BountyShare {
        reward,
        ..make_share(family_id, idx)
    }
}

#[test]
fn empty_registry_yields_no_promoted() {
    let pool = BountySidePool::new();
    let registry = FamilyManifestRegistry::new();
    let promoted = select_promoted_bounty_shares(&pool, &registry, 100, &[]);
    assert!(promoted.is_empty());
}

#[test]
fn unsigned_manifest_blocks_promotion() {
    let mut pool = BountySidePool::new();
    pool.insert(make_share(FAMILY_ID, 1));
    let mut registry = FamilyManifestRegistry::new();
    let mut manifest = parse(&manifest_value(FAMILY_ID, 0, 5));
    manifest.signature = None;
    registry.register(manifest);

    let promoted = select_promoted_bounty_shares(&pool, &registry, 100, &["00".repeat(32)]);
    assert!(promoted.is_empty(), "unsigned manifest must not promote");
}

#[test]
fn manifest_with_activation_height_above_runtime_blocks_promotion() {
    let mut pool = BountySidePool::new();
    pool.insert(make_share(FAMILY_ID, 1));
    let (manifest, pk) = signed_manifest(FAMILY_ID, 9999, 5);
    let mut registry = FamilyManifestRegistry::new();
    registry.register(manifest);

    let promoted = select_promoted_bounty_shares(&pool, &registry, 100, &[pk]);
    assert!(promoted.is_empty(), "inactive manifest must not promote");
}

#[test]
fn signature_not_in_operator_pks_blocks_promotion() {
    let mut pool = BountySidePool::new();
    pool.insert(make_share(FAMILY_ID, 1));
    let (manifest, _signing_pk) = signed_manifest(FAMILY_ID, 0, 5);
    let mut registry = FamilyManifestRegistry::new();
    registry.register(manifest);

    let stranger = SigningKeyV2::from_dev_id("stranger").pk_hex();
    let promoted = select_promoted_bounty_shares(&pool, &registry, 100, &[stranger]);
    assert!(
        promoted.is_empty(),
        "signature not produced by an allowed operator must not promote"
    );
}

#[test]
fn signed_active_manifest_with_matching_pk_promotes_shares() {
    let mut pool = BountySidePool::new();
    pool.insert(make_share(FAMILY_ID, 1));
    pool.insert(make_share(FAMILY_ID, 2));
    let (manifest, pk) = signed_manifest(FAMILY_ID, 0, 10);
    let mut registry = FamilyManifestRegistry::new();
    registry.register(manifest);

    let promoted = select_promoted_bounty_shares(&pool, &registry, 100, &[pk]);
    assert_eq!(promoted.len(), 2);
    assert_eq!(promoted[0].family_id, FAMILY_ID);
    assert_eq!(promoted[0].bounty_id, format!("{FAMILY_ID}-b1"));
    assert_eq!(promoted[1].bounty_id, format!("{FAMILY_ID}-b2"));
}

#[test]
fn caps_max_shares_per_block_truncates_promoted_count() {
    let mut pool = BountySidePool::new();
    for i in 1..=5 {
        pool.insert(make_share(FAMILY_ID, i));
    }
    let (manifest, pk) = signed_manifest(FAMILY_ID, 0, 2);
    let mut registry = FamilyManifestRegistry::new();
    registry.register(manifest);

    let promoted = select_promoted_bounty_shares(&pool, &registry, 100, &[pk]);
    assert_eq!(promoted.len(), 2, "cap=2 must truncate 5 shares to 2");
    // FIFO order from side-pool: oldest first.
    assert_eq!(promoted[0].bounty_id, format!("{FAMILY_ID}-b1"));
    assert_eq!(promoted[1].bounty_id, format!("{FAMILY_ID}-b2"));
}

#[test]
fn manifest_without_caps_blocks_promotion() {
    let mut pool = BountySidePool::new();
    pool.insert(make_share(FAMILY_ID, 1));
    let key = SigningKeyV2::from_dev_id("op-no-caps");
    let mut body = manifest_value(FAMILY_ID, 0, 0);
    body.as_object_mut().unwrap().remove("caps");
    let mut manifest = parse(&body);
    let envelope = key.sign(&serde_json::to_value(&manifest).unwrap()).unwrap();
    manifest.signature = Some(envelope.signature);
    let mut registry = FamilyManifestRegistry::new();
    registry.register(manifest);

    let promoted = select_promoted_bounty_shares(&pool, &registry, 100, &[key.pk_hex()]);
    assert!(
        promoted.is_empty(),
        "manifest without caps must default to no promotion (conservative)"
    );
}

#[test]
fn caps_with_zero_max_shares_per_block_blocks_promotion() {
    let mut pool = BountySidePool::new();
    pool.insert(make_share(FAMILY_ID, 1));
    let (manifest, pk) = signed_manifest(FAMILY_ID, 0, 0);
    let mut registry = FamilyManifestRegistry::new();
    registry.register(manifest);

    let promoted = select_promoted_bounty_shares(&pool, &registry, 100, &[pk]);
    assert!(
        promoted.is_empty(),
        "caps.maxSharesPerBlock=0 must produce no promotion"
    );
}

#[test]
fn promotion_is_per_family_independent() {
    let mut pool = BountySidePool::new();
    pool.insert(make_share(FAMILY_ID, 1));
    pool.insert(make_share(ALT_FAMILY_ID, 1));
    let (m1, pk1) = signed_manifest(FAMILY_ID, 0, 5);
    let (m2, pk2) = signed_manifest(ALT_FAMILY_ID, 9999, 5); // inactive
    let mut registry = FamilyManifestRegistry::new();
    registry.register(m1);
    registry.register(m2);

    let promoted = select_promoted_bounty_shares(&pool, &registry, 100, &[pk1, pk2]);
    assert_eq!(
        promoted.len(),
        1,
        "only the active family's share must be promoted"
    );
    assert_eq!(promoted[0].family_id, FAMILY_ID);
}

#[test]
fn caps_present_but_other_field_constraints_apply() {
    // Sanity: caps with a non-zero count and explicit FamilyCaps shape
    // round-trips through parse and is honoured by the helper.
    let caps_check = FamilyCaps {
        max_shares_per_block: 3,
        max_score_multiplier_bps: 10000,
        max_reward_credit_per_block: "0".to_string(),
    };
    let body = json!({
        "version": "1",
        "familyId": FAMILY_ID,
        "generatorHash": "abababababababababababababababababababababababababababababababab",
        "verifierHash": "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd",
        "canonicalizerHash": "efefefefefefefefefefefefefefefefefefefefefefefefefefefefefefefef",
        "promptSpecHash": "0101010101010101010101010101010101010101010101010101010101010101",
        "calibrationReportHash": "2323232323232323232323232323232323232323232323232323232323232323",
        "testVectorsHash": "4545454545454545454545454545454545454545454545454545454545454545",
        "resourceLimits": { "maxProofBytes": 16384, "verifyTimeoutMs": 30000, "maxDecls": 1024 },
        "rewardPolicy": { "mode": "no_protocol_reward", "maxBlockRewardShareBps": 0 },
        "activationHeight": 0,
        "status": "experimental",
        "caps": serde_json::to_value(&caps_check).unwrap()
    });
    let key = SigningKeyV2::from_dev_id("op-cap-shape");
    let mut manifest = parse(&body);
    let envelope = key.sign(&serde_json::to_value(&manifest).unwrap()).unwrap();
    manifest.signature = Some(envelope.signature);

    let mut pool = BountySidePool::new();
    for i in 1..=4 {
        pool.insert(make_share(FAMILY_ID, i));
    }
    let mut registry = FamilyManifestRegistry::new();
    registry.register(manifest);

    let promoted = select_promoted_bounty_shares(&pool, &registry, 100, &[key.pk_hex()]);
    assert_eq!(promoted.len(), 3);
}

// ----- S23a — credit math -----

#[test]
fn selection_back_compat_shares_match_shares_only_helper() {
    // The shares slice from `select_promoted_bounty_selection` must be
    // byte-identical to the legacy `select_promoted_bounty_shares` so
    // S22 callers cannot drift away from S23 callers.
    let mut pool = BountySidePool::new();
    pool.insert(make_share_with_reward(FAMILY_ID, 1, 100));
    pool.insert(make_share_with_reward(FAMILY_ID, 2, 50));
    let (manifest, pk) = signed_manifest_with_credit(FAMILY_ID, 0, 5, "1000");
    let mut registry = FamilyManifestRegistry::new();
    registry.register(manifest);

    let legacy =
        select_promoted_bounty_shares(&pool, &registry, 100, std::slice::from_ref(&pk));
    let selection = select_promoted_bounty_selection(&pool, &registry, 100, &[pk]);
    assert_eq!(legacy, selection.shares);
}

#[test]
fn credit_capped_by_share_reward_when_budget_is_loose() {
    // Budget per family per block is far above sum-of-rewards: each
    // share gets credit == its own reward, untouched.
    let mut pool = BountySidePool::new();
    pool.insert(make_share_with_reward(FAMILY_ID, 1, 100));
    pool.insert(make_share_with_reward(FAMILY_ID, 2, 50));
    pool.insert(make_share_with_reward(FAMILY_ID, 3, 25));
    let (manifest, pk) = signed_manifest_with_credit(FAMILY_ID, 0, 10, "1000");
    let mut registry = FamilyManifestRegistry::new();
    registry.register(manifest);

    let selection = select_promoted_bounty_selection(&pool, &registry, 100, &[pk]);
    assert_eq!(selection.shares.len(), 3);
    assert_eq!(selection.credits.len(), 3);
    assert_eq!(selection.credits[0].amount, "100");
    assert_eq!(selection.credits[1].amount, "50");
    assert_eq!(selection.credits[2].amount, "25");
    assert_eq!(selection.credits[0].family_id, FAMILY_ID);
    assert_eq!(selection.credits[0].bounty_id, format!("{FAMILY_ID}-b1"));
}

#[test]
fn credit_budget_exhaustion_drops_zero_credit_rows_but_keeps_promoted_shares() {
    // Budget = 120; rewards = 100 + 50 + 25. First share consumes 100,
    // second is capped to 20 (budget left), third is 0 → DROPPED from
    // credits but still listed in shares.
    let mut pool = BountySidePool::new();
    pool.insert(make_share_with_reward(FAMILY_ID, 1, 100));
    pool.insert(make_share_with_reward(FAMILY_ID, 2, 50));
    pool.insert(make_share_with_reward(FAMILY_ID, 3, 25));
    let (manifest, pk) = signed_manifest_with_credit(FAMILY_ID, 0, 10, "120");
    let mut registry = FamilyManifestRegistry::new();
    registry.register(manifest);

    let selection = select_promoted_bounty_selection(&pool, &registry, 100, &[pk]);
    assert_eq!(selection.shares.len(), 3, "all three shares are promoted");
    assert_eq!(selection.credits.len(), 2, "third share gets 0 credit, dropped");
    assert_eq!(selection.credits[0].amount, "100");
    assert_eq!(selection.credits[1].amount, "20");
}

#[test]
fn zero_max_reward_credit_per_block_promotes_shares_but_emits_no_credits() {
    // Caps allow share promotion but the family is not paying any
    // bounty credit this block — every share's credit collapses to 0.
    let mut pool = BountySidePool::new();
    pool.insert(make_share_with_reward(FAMILY_ID, 1, 100));
    pool.insert(make_share_with_reward(FAMILY_ID, 2, 50));
    let (manifest, pk) = signed_manifest_with_credit(FAMILY_ID, 0, 5, "0");
    let mut registry = FamilyManifestRegistry::new();
    registry.register(manifest);

    let selection = select_promoted_bounty_selection(&pool, &registry, 100, &[pk]);
    assert_eq!(selection.shares.len(), 2);
    assert!(
        selection.credits.is_empty(),
        "credit cap=0 must emit no credit rows"
    );
}

#[test]
fn per_family_budgets_are_independent_across_families() {
    // Each family carries its own `max_reward_credit_per_block`. Family
    // gamma's budget exhausting cannot shrink delta's credits.
    let mut pool = BountySidePool::new();
    pool.insert(make_share_with_reward(FAMILY_ID, 1, 100));
    pool.insert(make_share_with_reward(FAMILY_ID, 2, 100)); // gamma over budget
    pool.insert(make_share_with_reward(ALT_FAMILY_ID, 3, 75));
    let (m1, pk1) = signed_manifest_with_credit(FAMILY_ID, 0, 5, "150");
    let (m2, pk2) = signed_manifest_with_credit(ALT_FAMILY_ID, 0, 5, "200");
    let mut registry = FamilyManifestRegistry::new();
    registry.register(m1);
    registry.register(m2);

    let selection = select_promoted_bounty_selection(&pool, &registry, 100, &[pk1, pk2]);
    assert_eq!(selection.shares.len(), 3);
    assert_eq!(selection.credits.len(), 3);
    let gamma_credits: Vec<_> = selection
        .credits
        .iter()
        .filter(|c| c.family_id == FAMILY_ID)
        .map(|c| c.amount.clone())
        .collect();
    let delta_credits: Vec<_> = selection
        .credits
        .iter()
        .filter(|c| c.family_id == ALT_FAMILY_ID)
        .map(|c| c.amount.clone())
        .collect();
    assert_eq!(gamma_credits, vec!["100", "50"]);
    assert_eq!(delta_credits, vec!["75"]);
}
