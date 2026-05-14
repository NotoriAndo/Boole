use std::path::Path;

use boole_core::{bounties_from_list, Bounty, BountyList};

/// Read a `BountyList` from disk and validate `version == 1`.
///
/// Node owns this boot-time local file IO. Core owns the decoded schema contract
/// and version validation.
pub fn load_bounties_from_path(path: impl AsRef<Path>) -> anyhow::Result<Vec<Bounty>> {
    let raw = std::fs::read_to_string(path)?;
    let list: BountyList = serde_json::from_str(&raw)?;
    bounties_from_list(list)
}
