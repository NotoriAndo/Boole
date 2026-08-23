//! Startup cleanup of fixed `run-*` cgroup leaves.
//!
//! This boundary is deliberately narrower than readiness: it proves only
//! that startup recovery inspected and removed all exact execution leaves
//! beneath the already-verified fixed service cgroup.

#[cfg(any(target_os = "linux", test))]
use std::collections::BTreeSet;
#[cfg(any(target_os = "linux", test))]
use std::time::Duration;

use thiserror::Error;

use crate::manager_cgroup::VerifiedManagerCgroup;

/// Opaque proof that startup cgroup recovery completed after manager entry.
///
/// It is not a readiness, socket, toolchain, journal, or execution proof.
///
/// ```compile_fail
/// use boole_native_shadow_launcher::startup_recovery::VerifiedStartupCgroupRecovery;
/// let _forged = VerifiedStartupCgroupRecovery {};
/// ```
#[must_use]
#[allow(dead_code)]
pub struct VerifiedStartupCgroupRecovery {
    manager: VerifiedManagerCgroup,
    recovered_orphans: usize,
}

impl VerifiedStartupCgroupRecovery {
    pub fn recovered_orphan_count(&self) -> usize {
        self.recovered_orphans
    }
}

#[derive(Debug, Error)]
pub enum StartupCgroupRecoveryFailure {
    #[error("startup cgroup recovery platform failure: {0}")]
    Platform(String),
    #[error("unsafe startup cgroup state: {0}")]
    UnsafeState(String),
    #[error("the single 10-second startup cleanup deadline expired")]
    DeadlineExceeded,
}

#[derive(Debug, Error)]
pub enum StartupCgroupRecoveryError {
    #[error("startup cgroup recovery requires Linux")]
    UnsupportedPlatform,
    #[error("startup cgroup recovery failed after manager movement during {stage}: {failure}")]
    PostMoveFatal {
        stage: &'static str,
        #[source]
        failure: StartupCgroupRecoveryFailure,
    },
}

#[cfg(any(target_os = "linux", test))]
const STARTUP_CLEANUP_DEADLINE: Duration = Duration::from_secs(10);

#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct RunLeafName([u8; 32]);

#[cfg(any(target_os = "linux", test))]
impl RunLeafName {
    fn parse(name: &str) -> Result<Self, StartupCgroupRecoveryFailure> {
        let payload = name
            .strip_prefix("run-")
            .ok_or_else(|| unsafe_state(format!("unexpected direct cgroup child: {name:?}")))?;
        if payload.len() != 64
            || !payload
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(unsafe_state(format!(
                "run leaf is not exact lowercase 32-byte hex: {name:?}"
            )));
        }
        let decoded = hex::decode(payload)
            .map_err(|_| unsafe_state(format!("run leaf hex is malformed: {name:?}")))?;
        let operation_id: [u8; 32] = decoded
            .try_into()
            .map_err(|_| unsafe_state(format!("run leaf operation ID has wrong size: {name:?}")))?;
        Ok(Self(operation_id))
    }

    fn canonical(&self) -> String {
        format!("run-{}", hex::encode(self.0))
    }
}

#[cfg(any(target_os = "linux", test))]
#[derive(Debug)]
struct EstablishedRecovery {
    recovered_orphans: usize,
}

#[cfg(any(target_os = "linux", test))]
trait StartupRecoveryOperations {
    type Leaf;
    type Deadline: Copy;

    fn begin_deadline(&mut self, duration: Duration) -> Self::Deadline;
    fn confirm_deadline(
        &mut self,
        deadline: Self::Deadline,
    ) -> Result<(), StartupCgroupRecoveryFailure>;
    fn scan_inventory(
        &mut self,
        deadline: Self::Deadline,
    ) -> Result<Vec<String>, StartupCgroupRecoveryFailure>;
    fn open_and_validate_leaf(
        &mut self,
        name: &RunLeafName,
        deadline: Self::Deadline,
    ) -> Result<Self::Leaf, StartupCgroupRecoveryFailure>;
    fn freeze_leaf(
        &mut self,
        leaf: &Self::Leaf,
        deadline: Self::Deadline,
    ) -> Result<(), StartupCgroupRecoveryFailure>;
    fn wait_frozen(
        &mut self,
        leaf: &Self::Leaf,
        deadline: Self::Deadline,
    ) -> Result<(), StartupCgroupRecoveryFailure>;
    fn kill_leaf(
        &mut self,
        leaf: &Self::Leaf,
        deadline: Self::Deadline,
    ) -> Result<(), StartupCgroupRecoveryFailure>;
    fn wait_unpopulated(
        &mut self,
        leaf: &Self::Leaf,
        deadline: Self::Deadline,
    ) -> Result<(), StartupCgroupRecoveryFailure>;
    fn verify_ids_empty(
        &mut self,
        leaf: &Self::Leaf,
        deadline: Self::Deadline,
    ) -> Result<(), StartupCgroupRecoveryFailure>;
    fn remove_leaf(
        &mut self,
        leaf: Self::Leaf,
        deadline: Self::Deadline,
    ) -> Result<(), StartupCgroupRecoveryFailure>;
    fn verify_root_invariants(
        &mut self,
        deadline: Self::Deadline,
    ) -> Result<(), StartupCgroupRecoveryFailure>;
    fn verify_manager_invariants(
        &mut self,
        deadline: Self::Deadline,
    ) -> Result<(), StartupCgroupRecoveryFailure>;
}

#[cfg(any(target_os = "linux", test))]
fn recover_with_operations<O: StartupRecoveryOperations>(
    operations: &mut O,
) -> Result<EstablishedRecovery, StartupCgroupRecoveryError> {
    let deadline = operations.begin_deadline(STARTUP_CLEANUP_DEADLINE);
    let inventory = operations
        .scan_inventory(deadline)
        .map_err(|failure| fatal("scan startup cgroup inventory", failure))?;
    confirm_deadline(operations, deadline)?;
    let names = validate_inventory(inventory)
        .map_err(|failure| fatal("validate startup cgroup inventory", failure))?;
    confirm_deadline(operations, deadline)?;

    let mut leaves = Vec::with_capacity(names.len());
    for name in &names {
        let leaf = operations
            .open_and_validate_leaf(name, deadline)
            .map_err(|failure| fatal("open and validate startup run leaf", failure))?;
        confirm_deadline(operations, deadline)?;
        leaves.push(leaf);
    }

    for leaf in leaves {
        operations
            .freeze_leaf(&leaf, deadline)
            .map_err(|failure| fatal("freeze startup run leaf", failure))?;
        confirm_deadline(operations, deadline)?;
        operations
            .wait_frozen(&leaf, deadline)
            .map_err(|failure| fatal("confirm startup run leaf frozen", failure))?;
        confirm_deadline(operations, deadline)?;
        operations
            .kill_leaf(&leaf, deadline)
            .map_err(|failure| fatal("kill startup run leaf", failure))?;
        confirm_deadline(operations, deadline)?;
        operations
            .wait_unpopulated(&leaf, deadline)
            .map_err(|failure| fatal("confirm startup run leaf unpopulated", failure))?;
        confirm_deadline(operations, deadline)?;
        operations
            .verify_ids_empty(&leaf, deadline)
            .map_err(|failure| fatal("verify startup run leaf IDs empty", failure))?;
        confirm_deadline(operations, deadline)?;
        operations
            .remove_leaf(leaf, deadline)
            .map_err(|failure| fatal("remove startup run leaf", failure))?;
        confirm_deadline(operations, deadline)?;
    }

    let final_inventory = operations
        .scan_inventory(deadline)
        .map_err(|failure| fatal("rescan startup cgroup inventory", failure))?;
    confirm_deadline(operations, deadline)?;
    let remaining = validate_inventory(final_inventory)
        .map_err(|failure| fatal("validate final startup cgroup inventory", failure))?;
    confirm_deadline(operations, deadline)?;
    if !remaining.is_empty() {
        return Err(fatal(
            "validate final startup cgroup inventory",
            unsafe_state("startup recovery left one or more run leaves"),
        ));
    }
    operations
        .verify_root_invariants(deadline)
        .map_err(|failure| fatal("verify service root after startup recovery", failure))?;
    confirm_deadline(operations, deadline)?;
    operations
        .verify_manager_invariants(deadline)
        .map_err(|failure| fatal("verify manager after startup recovery", failure))?;
    confirm_deadline(operations, deadline)?;

    Ok(EstablishedRecovery {
        recovered_orphans: names.len(),
    })
}

#[cfg(any(target_os = "linux", test))]
fn confirm_deadline<O: StartupRecoveryOperations>(
    operations: &mut O,
    deadline: O::Deadline,
) -> Result<(), StartupCgroupRecoveryError> {
    operations
        .confirm_deadline(deadline)
        .map_err(|failure| fatal("enforce absolute startup cleanup deadline", failure))
}

#[cfg(any(target_os = "linux", test))]
fn validate_inventory(
    inventory: Vec<String>,
) -> Result<Vec<RunLeafName>, StartupCgroupRecoveryFailure> {
    let mut manager_seen = false;
    let mut leaves = BTreeSet::new();
    for name in inventory {
        if name == "manager" {
            if manager_seen {
                return Err(unsafe_state("startup inventory contains duplicate manager"));
            }
            manager_seen = true;
            continue;
        }
        let leaf = RunLeafName::parse(&name)?;
        if !leaves.insert(leaf) {
            return Err(unsafe_state(format!(
                "startup inventory contains duplicate run leaf: {name}"
            )));
        }
    }
    if !manager_seen {
        return Err(unsafe_state("startup inventory is missing manager"));
    }
    Ok(leaves.into_iter().collect())
}

#[cfg(any(target_os = "linux", test))]
fn fatal(stage: &'static str, failure: StartupCgroupRecoveryFailure) -> StartupCgroupRecoveryError {
    StartupCgroupRecoveryError::PostMoveFatal { stage, failure }
}

#[cfg(any(target_os = "linux", test))]
fn unsafe_state(reason: impl Into<String>) -> StartupCgroupRecoveryFailure {
    StartupCgroupRecoveryFailure::UnsafeState(reason.into())
}

/// Consume the fixed manager proof and remove every exact startup `run-*`
/// orphan. No path, operation ID, timeout, or policy is caller-selected.
pub fn recover_fixed_startup_orphans(
    manager: VerifiedManagerCgroup,
) -> Result<VerifiedStartupCgroupRecovery, StartupCgroupRecoveryError> {
    #[cfg(target_os = "linux")]
    {
        let recovered_orphans = {
            let (service_root, manager_directory) = manager.recovery_directories();
            let mut operations = linux::LinuxRecoveryOperations {
                service_root,
                manager: manager_directory,
            };
            recover_with_operations(&mut operations)?.recovered_orphans
        };
        Ok(VerifiedStartupCgroupRecovery {
            manager,
            recovered_orphans,
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        drop(manager);
        Err(StartupCgroupRecoveryError::UnsupportedPlatform)
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::time::{Duration, Instant};

    use super::{RunLeafName, StartupCgroupRecoveryFailure, StartupRecoveryOperations};
    use crate::cgroupfs_fd::{self, CgroupDirectory, RecoveryLeaf};

    pub(super) struct LinuxRecoveryOperations<'a> {
        pub(super) service_root: &'a CgroupDirectory,
        pub(super) manager: &'a CgroupDirectory,
    }

    impl StartupRecoveryOperations for LinuxRecoveryOperations<'_> {
        type Leaf = RecoveryLeaf;
        type Deadline = Instant;

        fn begin_deadline(&mut self, duration: Duration) -> Self::Deadline {
            Instant::now() + duration
        }

        fn confirm_deadline(
            &mut self,
            deadline: Self::Deadline,
        ) -> Result<(), StartupCgroupRecoveryFailure> {
            ensure_before_deadline(deadline)
        }

        fn scan_inventory(
            &mut self,
            deadline: Self::Deadline,
        ) -> Result<Vec<String>, StartupCgroupRecoveryFailure> {
            ensure_before_deadline(deadline)?;
            cgroupfs_fd::scan_service_child_cgroups(self.service_root).map_err(platform)
        }

        fn open_and_validate_leaf(
            &mut self,
            name: &RunLeafName,
            deadline: Self::Deadline,
        ) -> Result<Self::Leaf, StartupCgroupRecoveryFailure> {
            ensure_before_deadline(deadline)?;
            cgroupfs_fd::open_and_validate_recovery_leaf(self.service_root, &name.canonical())
                .map_err(platform)
        }

        fn freeze_leaf(
            &mut self,
            leaf: &Self::Leaf,
            deadline: Self::Deadline,
        ) -> Result<(), StartupCgroupRecoveryFailure> {
            ensure_before_deadline(deadline)?;
            cgroupfs_fd::freeze_recovery_leaf(leaf).map_err(platform)
        }

        fn wait_frozen(
            &mut self,
            leaf: &Self::Leaf,
            deadline: Self::Deadline,
        ) -> Result<(), StartupCgroupRecoveryFailure> {
            ensure_before_deadline(deadline)?;
            cgroupfs_fd::wait_recovery_leaf_event(leaf, "frozen", 1, deadline).map_err(platform)
        }

        fn kill_leaf(
            &mut self,
            leaf: &Self::Leaf,
            deadline: Self::Deadline,
        ) -> Result<(), StartupCgroupRecoveryFailure> {
            ensure_before_deadline(deadline)?;
            cgroupfs_fd::kill_recovery_leaf(leaf).map_err(platform)
        }

        fn wait_unpopulated(
            &mut self,
            leaf: &Self::Leaf,
            deadline: Self::Deadline,
        ) -> Result<(), StartupCgroupRecoveryFailure> {
            ensure_before_deadline(deadline)?;
            cgroupfs_fd::wait_recovery_leaf_event(leaf, "populated", 0, deadline).map_err(platform)
        }

        fn verify_ids_empty(
            &mut self,
            leaf: &Self::Leaf,
            deadline: Self::Deadline,
        ) -> Result<(), StartupCgroupRecoveryFailure> {
            ensure_before_deadline(deadline)?;
            cgroupfs_fd::verify_recovery_leaf_ids_empty(leaf).map_err(platform)
        }

        fn remove_leaf(
            &mut self,
            leaf: Self::Leaf,
            deadline: Self::Deadline,
        ) -> Result<(), StartupCgroupRecoveryFailure> {
            ensure_before_deadline(deadline)?;
            cgroupfs_fd::remove_recovery_leaf(self.service_root, leaf).map_err(platform)
        }

        fn verify_root_invariants(
            &mut self,
            deadline: Self::Deadline,
        ) -> Result<(), StartupCgroupRecoveryFailure> {
            ensure_before_deadline(deadline)?;
            cgroupfs_fd::verify_service_root_has_no_processes(self.service_root)
                .and_then(|()| cgroupfs_fd::verify_required_controllers(self.service_root))
                .map_err(platform)
        }

        fn verify_manager_invariants(
            &mut self,
            deadline: Self::Deadline,
        ) -> Result<(), StartupCgroupRecoveryFailure> {
            ensure_before_deadline(deadline)?;
            cgroupfs_fd::verify_manager_after_move(self.manager).map_err(platform)
        }
    }

    fn ensure_before_deadline(deadline: Instant) -> Result<(), StartupCgroupRecoveryFailure> {
        if Instant::now() < deadline {
            Ok(())
        } else {
            Err(StartupCgroupRecoveryFailure::DeadlineExceeded)
        }
    }

    fn platform(source: cgroupfs_fd::CgroupFsError) -> StartupCgroupRecoveryFailure {
        match source {
            cgroupfs_fd::CgroupFsError::UnsafeState(reason) => {
                StartupCgroupRecoveryFailure::UnsafeState(reason)
            }
            cgroupfs_fd::CgroupFsError::DeadlineExceeded => {
                StartupCgroupRecoveryFailure::DeadlineExceeded
            }
            source @ cgroupfs_fd::CgroupFsError::Io { .. } => {
                StartupCgroupRecoveryFailure::Platform(source.to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::time::Duration;

    use super::{
        recover_with_operations, RunLeafName, StartupCgroupRecoveryFailure,
        StartupRecoveryOperations,
    };

    const A: &str = "run-0000000000000000000000000000000000000000000000000000000000000001";
    const B: &str = "run-0000000000000000000000000000000000000000000000000000000000000002";
    const DEADLINE_ID: u8 = 41;

    struct FakeOperations {
        inventories: VecDeque<Vec<String>>,
        steps: Vec<String>,
        fail_at: Option<String>,
        confirmations: usize,
        deadline_expiry_after_confirmation: Option<usize>,
    }

    impl FakeOperations {
        fn new(initial: &[&str], final_inventory: &[&str]) -> Self {
            Self {
                inventories: VecDeque::from([
                    initial.iter().map(|value| (*value).to_string()).collect(),
                    final_inventory
                        .iter()
                        .map(|value| (*value).to_string())
                        .collect(),
                ]),
                steps: Vec::new(),
                fail_at: None,
                confirmations: 0,
                deadline_expiry_after_confirmation: None,
            }
        }

        fn with_failure(mut self, step: &str) -> Self {
            self.fail_at = Some(step.to_string());
            self
        }

        fn with_deadline_expiry_after(mut self, confirmation: usize) -> Self {
            self.deadline_expiry_after_confirmation = Some(confirmation);
            self
        }

        fn record(&mut self, step: String) -> Result<(), StartupCgroupRecoveryFailure> {
            self.steps.push(step.clone());
            if self.fail_at.as_deref() == Some(step.as_str()) {
                Err(StartupCgroupRecoveryFailure::Platform(
                    "injected".to_string(),
                ))
            } else {
                Ok(())
            }
        }

        fn leaf_step(
            &mut self,
            action: &str,
            leaf: &str,
            deadline: u8,
        ) -> Result<(), StartupCgroupRecoveryFailure> {
            self.record(format!("{action}:{leaf}:{deadline}"))
        }
    }

    impl StartupRecoveryOperations for FakeOperations {
        type Leaf = String;
        type Deadline = u8;

        fn begin_deadline(&mut self, duration: Duration) -> Self::Deadline {
            self.steps
                .push(format!("deadline:{}", duration.as_millis()));
            DEADLINE_ID
        }

        fn confirm_deadline(
            &mut self,
            _deadline: Self::Deadline,
        ) -> Result<(), StartupCgroupRecoveryFailure> {
            self.confirmations += 1;
            if self.deadline_expiry_after_confirmation == Some(self.confirmations) {
                Err(StartupCgroupRecoveryFailure::DeadlineExceeded)
            } else {
                Ok(())
            }
        }

        fn scan_inventory(
            &mut self,
            deadline: Self::Deadline,
        ) -> Result<Vec<String>, StartupCgroupRecoveryFailure> {
            self.record(format!("scan:{deadline}"))?;
            self.inventories.pop_front().ok_or_else(|| {
                StartupCgroupRecoveryFailure::Platform("missing fake inventory".to_string())
            })
        }

        fn open_and_validate_leaf(
            &mut self,
            name: &RunLeafName,
            deadline: Self::Deadline,
        ) -> Result<Self::Leaf, StartupCgroupRecoveryFailure> {
            let name = name.canonical();
            self.leaf_step("open", &name, deadline)?;
            Ok(name)
        }

        fn freeze_leaf(
            &mut self,
            leaf: &Self::Leaf,
            deadline: Self::Deadline,
        ) -> Result<(), StartupCgroupRecoveryFailure> {
            self.leaf_step("freeze", leaf, deadline)
        }

        fn wait_frozen(
            &mut self,
            leaf: &Self::Leaf,
            deadline: Self::Deadline,
        ) -> Result<(), StartupCgroupRecoveryFailure> {
            self.leaf_step("frozen", leaf, deadline)
        }

        fn kill_leaf(
            &mut self,
            leaf: &Self::Leaf,
            deadline: Self::Deadline,
        ) -> Result<(), StartupCgroupRecoveryFailure> {
            self.leaf_step("kill", leaf, deadline)
        }

        fn wait_unpopulated(
            &mut self,
            leaf: &Self::Leaf,
            deadline: Self::Deadline,
        ) -> Result<(), StartupCgroupRecoveryFailure> {
            self.leaf_step("unpopulated", leaf, deadline)
        }

        fn verify_ids_empty(
            &mut self,
            leaf: &Self::Leaf,
            deadline: Self::Deadline,
        ) -> Result<(), StartupCgroupRecoveryFailure> {
            self.leaf_step("ids-empty", leaf, deadline)
        }

        fn remove_leaf(
            &mut self,
            leaf: Self::Leaf,
            deadline: Self::Deadline,
        ) -> Result<(), StartupCgroupRecoveryFailure> {
            self.leaf_step("remove", &leaf, deadline)
        }

        fn verify_root_invariants(
            &mut self,
            deadline: Self::Deadline,
        ) -> Result<(), StartupCgroupRecoveryFailure> {
            self.record(format!("root:{deadline}"))
        }

        fn verify_manager_invariants(
            &mut self,
            deadline: Self::Deadline,
        ) -> Result<(), StartupCgroupRecoveryFailure> {
            self.record(format!("manager:{deadline}"))
        }
    }

    #[test]
    fn exact_run_leaf_name_is_canonical_lower_hex_only() {
        assert_eq!(RunLeafName::parse(A).expect("exact name").canonical(), A);
        for invalid in [
            "manager",
            "run_0000000000000000000000000000000000000000000000000000000000000001",
            "run-000000000000000000000000000000000000000000000000000000000000001",
            "run-00000000000000000000000000000000000000000000000000000000000000001",
            "run-000000000000000000000000000000000000000000000000000000000000000G",
            "run-000000000000000000000000000000000000000000000000000000000000000g",
            "run-0000000000000000000000000000000000000000000000000000000000000001/",
            " run-0000000000000000000000000000000000000000000000000000000000000001",
        ] {
            assert!(RunLeafName::parse(invalid).is_err(), "accepted {invalid:?}");
        }
    }

    #[test]
    fn unexpected_inventory_is_rejected_before_any_leaf_open_or_mutation() {
        for inventory in [
            vec![A],
            vec!["manager", "manager"],
            vec!["manager", A, A],
            vec!["manager", "manager-extra"],
            vec!["manager", A, "zzz-unexpected"],
        ] {
            let mut operations = FakeOperations::new(&inventory, &["manager"]);
            let error = recover_with_operations(&mut operations)
                .expect_err("unexpected inventory must fail closed");
            assert!(matches!(
                error,
                super::StartupCgroupRecoveryError::PostMoveFatal {
                    stage: "validate startup cgroup inventory",
                    failure: StartupCgroupRecoveryFailure::UnsafeState(_),
                }
            ));
            assert_eq!(operations.steps, ["deadline:10000", "scan:41"]);
        }
    }

    #[test]
    fn every_leaf_is_opened_before_the_first_mutation() {
        let failure = format!("open:{B}:{DEADLINE_ID}");
        let mut operations =
            FakeOperations::new(&["manager", B, A], &["manager"]).with_failure(&failure);
        assert!(recover_with_operations(&mut operations).is_err());
        assert_eq!(
            operations.steps,
            [
                "deadline:10000".to_string(),
                "scan:41".to_string(),
                format!("open:{A}:41"),
                format!("open:{B}:41"),
            ]
        );
    }

    #[test]
    fn two_orphans_are_cleaned_once_in_canonical_order_with_one_deadline() {
        let mut operations = FakeOperations::new(&[B, "manager", A], &["manager"]);
        let result = recover_with_operations(&mut operations).expect("recovery");
        assert_eq!(result.recovered_orphans, 2);
        assert_eq!(
            operations.steps,
            [
                "deadline:10000".to_string(),
                "scan:41".to_string(),
                format!("open:{A}:41"),
                format!("open:{B}:41"),
                format!("freeze:{A}:41"),
                format!("frozen:{A}:41"),
                format!("kill:{A}:41"),
                format!("unpopulated:{A}:41"),
                format!("ids-empty:{A}:41"),
                format!("remove:{A}:41"),
                format!("freeze:{B}:41"),
                format!("frozen:{B}:41"),
                format!("kill:{B}:41"),
                format!("unpopulated:{B}:41"),
                format!("ids-empty:{B}:41"),
                format!("remove:{B}:41"),
                "scan:41".to_string(),
                "root:41".to_string(),
                "manager:41".to_string(),
            ]
        );
    }

    #[test]
    fn every_cleanup_failure_is_fatal_and_stops_immediately() {
        for action in [
            "freeze",
            "frozen",
            "kill",
            "unpopulated",
            "ids-empty",
            "remove",
        ] {
            let failed_step = format!("{action}:{A}:41");
            let mut operations =
                FakeOperations::new(&["manager", A], &["manager"]).with_failure(&failed_step);
            assert!(recover_with_operations(&mut operations).is_err());
            assert_eq!(operations.steps.last(), Some(&failed_step));
            assert!(!operations.steps.iter().any(|step| step == "root:41"));
        }
    }

    #[test]
    fn zero_leaf_recovery_still_rechecks_root_and_manager() {
        let mut operations = FakeOperations::new(&["manager"], &["manager"]);
        let result = recover_with_operations(&mut operations).expect("zero-leaf recovery");
        assert_eq!(result.recovered_orphans, 0);
        assert_eq!(
            operations.steps,
            [
                "deadline:10000",
                "scan:41",
                "scan:41",
                "root:41",
                "manager:41"
            ]
        );
    }

    #[test]
    fn post_cleanup_drift_or_invariant_failure_withholds_success() {
        let mut drift = FakeOperations::new(&["manager", A], &["manager", B]);
        assert!(recover_with_operations(&mut drift).is_err());

        for failed_step in ["root:41", "manager:41"] {
            let mut operations =
                FakeOperations::new(&["manager"], &["manager"]).with_failure(failed_step);
            assert!(recover_with_operations(&mut operations).is_err());
            assert_eq!(operations.steps.last(), Some(&failed_step.to_string()));
        }
    }

    #[test]
    fn an_operation_that_returns_after_the_absolute_deadline_withholds_success() {
        let mut operations =
            FakeOperations::new(&["manager"], &["manager"]).with_deadline_expiry_after(1);
        let error = recover_with_operations(&mut operations).expect_err("deadline must be fatal");
        assert!(matches!(
            error,
            super::StartupCgroupRecoveryError::PostMoveFatal {
                failure: StartupCgroupRecoveryFailure::DeadlineExceeded,
                ..
            }
        ));
        assert_eq!(operations.steps, ["deadline:10000", "scan:41"]);
    }
}
