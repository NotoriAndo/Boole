//! Retained-record rebuild qualification, separate from fold authority.
use boole_derivation::{
    m2::{Config, Key, State, Status},
    m3::{ForkKind, ForkTable, RecordKind, KERNEL_FAULT},
};
use boole_gate_supervisor::{
    codec::Codec,
    rebuild::{self, History},
};
#[derive(Debug, Clone, Copy)]
pub struct Coverage {
    pub total: usize,
    pub checked: usize,
}
#[derive(Debug)]
pub struct Failure {
    pub height: u64,
    pub coverage: Coverage,
    pub resource: Option<crate::resource::ResourceStop>,
}
pub fn check(
    state: &State,
    c: &Config,
    forks: Option<&ForkTable>,
    history: &mut dyn History,
    codec: Option<&Codec>,
    record_heights: &std::collections::BTreeMap<boole_derivation::m2::Key, u64>,
    work: &std::path::Path,
) -> Result<Coverage, Failure> {
    let Some(m) = &state.m3 else {
        return Ok(Coverage {
            total: 0,
            checked: 0,
        });
    };
    // A quarantine row's wire hash names the first-verdict job retagged at
    // activation, not a foreign COMMIT with the same unproven hash. Preserve
    // the resolved full keys once rather than scanning the queue per candidate.
    let quarantined: std::collections::BTreeSet<Key> = m
        .queue
        .iter()
        .filter(|job| {
            job.kind == RecordKind::Verdict
                && forks.is_some_and(|f| {
                    f.rows.iter().any(|row| {
                        job.epoch_override == Some(row.epoch)
                            && row.kind == ForkKind::Quarantine
                            && row.list.iter().any(|item| item.commit_hash == job.key.1)
                    })
                })
        })
        .map(|job| job.key)
        .collect();
    let retained: Vec<_> = state
        .candidates
        .iter()
        .filter(|(key, candidate)| {
            matches!(candidate.status(), Status::Accepted | Status::Stale)
                || (candidate.status() == Status::Rejected
                    && m.results.get(key).is_some_and(|r| r.reason == KERNEL_FAULT))
                || quarantined.contains(key)
        })
        .map(|(key, _)| *key)
        .collect();
    let mut coverage = Coverage {
        total: retained.len(),
        checked: 0,
    };
    for key in retained {
        let height = record_heights
            .get(&key)
            .copied()
            .or_else(|| {
                state
                    .candidates
                    .get(&key)
                    .and_then(|v| v.live())
                    .map(|v| v.open.as_ref().and_then(|o| o.h_r).unwrap_or(v.h_c))
            })
            .unwrap_or(0);
        let scratch = boole_devnet_archive::durability::PrivateTempDir::new_in(work, "k4")
            .map_err(|_| Failure {
                height,
                coverage,
                resource: None,
            })?;
        if let Err(error) = rebuild::rebuild(state, c, key, history, codec, scratch.path()) {
            return Err(Failure {
                height,
                coverage,
                resource: error
                    .downcast_ref::<crate::resource::ResourceStop>()
                    .copied(),
            });
        }
        coverage.checked += 1;
    }
    Ok(coverage)
}
