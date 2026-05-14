//! Pure `BountyList` schema validation.
//!
//! Runtime crates own file IO. Core owns the decoded bounty-catalog envelope
//! contract and version gate.

use boole_core::{bounties_from_list, BountyList};

#[test]
fn bounty_list_accepts_empty_v1_list() {
    let bounties = bounties_from_list(BountyList {
        version: 1,
        bounties: Vec::new(),
    })
    .expect("v1 empty list parses");

    assert!(bounties.is_empty(), "empty list returned as empty Vec");
}

#[test]
fn bounty_list_rejects_bad_version() {
    let err = bounties_from_list(BountyList {
        version: 2,
        bounties: Vec::new(),
    })
    .expect_err("non-1 version must error");
    let message = format!("{err:#}");

    assert!(
        message.contains("version") && message.contains("1"),
        "error must mention expected version: {message}"
    );
}
