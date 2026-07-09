//! N5.1 (ADR-0014) — content-addressed genesis.
//!
//! A named network's identity is the BLAKE3 hash of its `GenesisSpec`'s
//! canonical-JSON encoding: network id, every Tier-1 consensus parameter
//! (ADR-0014 (c)), and the initial chain state. Two nodes agree on a
//! genesis hash iff they agree on ALL of it — `any_param_change_changes_
//! genesis_hash` is the defining property (quoted by ADR-0012/0014 as the
//! reason rule changes after N5 become hard forks).

use boole_core::{
    DifficultyRetargetPolicy, GenesisInitialState, GenesisParams, GenesisSpec,
    CONSENSUS_RULE_VERSION,
};

fn testnet_spec() -> GenesisSpec {
    GenesisSpec {
        network_id: "boole-testnet".to_string(),
        params: GenesisParams {
            consensus_rule_version: CONSENSUS_RULE_VERSION,
            t_block: format!("0x{}e", "f".repeat(63)),
            t_share: format!("0x{}", "f".repeat(64)),
            k_max: 4,
            retarget: Some(DifficultyRetargetPolicy {
                target_block_ms: 60_000,
                retarget_every_blocks: 2,
                max_adjustment_factor: 4,
            }),
            seed_binding_required: true,
            checker_artifact_hash: Some("ab".repeat(32)),
        },
        initial_state: GenesisInitialState {
            genesis_c: "00".repeat(32),
        },
    }
}

#[test]
fn genesis_hash_is_deterministic_over_canonical_encoding() {
    let spec = testnet_spec();

    // Same spec, same hash — across separately-constructed values.
    assert_eq!(
        spec.hash().to_hex(),
        testnet_spec().hash().to_hex(),
        "equal specs must hash equal"
    );

    // The hash is over the CANONICAL encoding: a spec that round-trips
    // through JSON (whatever key order the wire used) hashes identically.
    let round_tripped: GenesisSpec =
        serde_json::from_str(&serde_json::to_string(&spec).expect("serialize"))
            .expect("round trip");
    assert_eq!(
        round_tripped.hash().to_hex(),
        spec.hash().to_hex(),
        "a JSON round trip must not change the genesis hash"
    );
}

type Mutation = (&'static str, Box<dyn Fn(&mut GenesisSpec)>);

#[test]
fn any_param_change_changes_genesis_hash() {
    let baseline = testnet_spec().hash().to_hex();

    let mutations: Vec<Mutation> = vec![
        (
            "network_id",
            Box::new(|s| s.network_id = "boole-dev".to_string()),
        ),
        (
            "consensus_rule_version",
            Box::new(|s| s.params.consensus_rule_version += 1),
        ),
        (
            "t_block",
            Box::new(|s| s.params.t_block = format!("0x{}d", "f".repeat(63))),
        ),
        (
            "t_share",
            Box::new(|s| s.params.t_share = format!("0x{}e", "f".repeat(63))),
        ),
        ("k_max", Box::new(|s| s.params.k_max += 1)),
        (
            "retarget.target_block_ms",
            Box::new(|s| s.params.retarget.as_mut().unwrap().target_block_ms += 1),
        ),
        (
            "retarget.retarget_every_blocks",
            Box::new(|s| s.params.retarget.as_mut().unwrap().retarget_every_blocks += 1),
        ),
        (
            "retarget.max_adjustment_factor",
            Box::new(|s| s.params.retarget.as_mut().unwrap().max_adjustment_factor += 1),
        ),
        ("retarget=None", Box::new(|s| s.params.retarget = None)),
        (
            "seed_binding_required",
            Box::new(|s| s.params.seed_binding_required = false),
        ),
        (
            "checker_artifact_hash",
            Box::new(|s| s.params.checker_artifact_hash = Some("cd".repeat(32))),
        ),
        (
            "checker_artifact_hash=None",
            Box::new(|s| s.params.checker_artifact_hash = None),
        ),
        (
            "initial_state.genesis_c",
            Box::new(|s| s.initial_state.genesis_c = "11".repeat(32)),
        ),
    ];

    for (name, mutate) in mutations {
        let mut spec = testnet_spec();
        mutate(&mut spec);
        assert_ne!(
            spec.hash().to_hex(),
            baseline,
            "changing {name} must change the genesis hash"
        );
    }
}
