use std::collections::BTreeSet;

use boole_core::{
    build_block_selection, calibration_policy, BlockBuilderConfig, BuildSelectionResult,
    CalibrationReport, CandidateShare,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    chain_head: String,
    config: ConfigFixture,
    input_shares: Vec<CandidateShare>,
    accepted_canon_tags: Vec<u8>,
    expected: Expected,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigFixture {
    #[serde(flatten)]
    report: CalibrationReport,
    min_share_score: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Expected {
    ok: bool,
    selected_labels: Vec<String>,
    selected_keys: Vec<String>,
    proposer_index: usize,
    min_share_score: String,
    dropped_below_min_score: usize,
    dropped_kernel_reject: usize,
    truncated_by_kmax: usize,
    kernel_checked_tags: Vec<u8>,
    kernel_accepted: Vec<bool>,
}

#[test]
fn block_builder_matches_typescript_golden_fixture() {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../fixtures/protocol/block-builder/v1.json"
    ))
    .expect("fixture parses");
    assert!(fixture.expected.ok);

    let policy = calibration_policy(&fixture.config.report).expect("policy parses");
    let cfg = BlockBuilderConfig::from_policy(&policy).expect("block builder config from policy");
    assert_eq!(
        cfg.min_share_score.to_string(),
        fixture.expected.min_share_score
    );
    assert_eq!(
        cfg.min_share_score.to_string(),
        fixture.config.min_share_score
    );
    let accepted = fixture
        .accepted_canon_tags
        .into_iter()
        .collect::<BTreeSet<_>>();
    let result = build_block_selection(&fixture.chain_head, &fixture.input_shares, &cfg, &accepted)
        .expect("build selection");

    let BuildSelectionResult::Ok(selection) = result else {
        panic!("expected ok selection");
    };
    let selected_labels = selection
        .selected
        .iter()
        .map(|s| s.label.clone())
        .collect::<Vec<_>>();
    let selected_keys = selection
        .selected
        .iter()
        .map(|s| format!("{}|{}|{}", s.pk, s.n, s.j))
        .collect::<Vec<_>>();

    assert_eq!(selected_labels, fixture.expected.selected_labels);
    assert_eq!(selected_keys, fixture.expected.selected_keys);
    assert_eq!(selection.proposer_index, fixture.expected.proposer_index);
    assert_eq!(
        selection.dropped_below_min_score,
        fixture.expected.dropped_below_min_score
    );
    assert_eq!(
        selection.dropped_kernel_reject,
        fixture.expected.dropped_kernel_reject
    );
    assert_eq!(
        selection.truncated_by_kmax,
        fixture.expected.truncated_by_kmax
    );
    assert_eq!(
        selection.kernel_checked_tags,
        fixture.expected.kernel_checked_tags
    );
    assert_eq!(selection.kernel_accepted, fixture.expected.kernel_accepted);
}
