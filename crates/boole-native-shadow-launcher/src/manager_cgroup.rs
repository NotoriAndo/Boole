//! Fixed manager-cgroup entry after launcher-instance creation.

use thiserror::Error;

use crate::instance_id::VerifiedLauncherInstance;

/// Opaque proof that the launcher owns its lifetime lock and instance ID and
/// has entered a verified manager cgroup under the fixed systemd service root.
///
/// This is not startup recovery or readiness: `run-*` leaves have not yet
/// been inspected or cleaned.
///
/// ```compile_fail
/// use boole_native_shadow_launcher::manager_cgroup::VerifiedManagerCgroup;
/// let _forged = VerifiedManagerCgroup {};
/// ```
#[must_use]
#[allow(dead_code)]
pub struct VerifiedManagerCgroup {
    instance: VerifiedLauncherInstance,
    #[cfg(target_os = "linux")]
    service_root: crate::cgroupfs_fd::CgroupDirectory,
    #[cfg(target_os = "linux")]
    manager: crate::cgroupfs_fd::CgroupDirectory,
}

#[derive(Debug, Error)]
pub enum ManagerCgroupFailure {
    #[error("manager-cgroup platform failure: {0}")]
    Platform(String),
    #[error("current launcher process is not single-threaded: expected TID {expected}, observed {observed:?}")]
    NotSingleThreaded { expected: u32, observed: Vec<u32> },
}

#[derive(Debug, Error)]
pub enum ManagerCgroupError {
    #[error("native-shadow manager-cgroup setup requires Linux")]
    UnsupportedPlatform,
    #[error("manager-cgroup setup failed before process movement during {stage}: {failure}")]
    PreMove {
        stage: &'static str,
        #[source]
        failure: ManagerCgroupFailure,
    },
    #[error("manager-cgroup setup became fatal at or after process-move attempt during {stage}: {failure}")]
    PostMoveFatal {
        stage: &'static str,
        #[source]
        failure: ManagerCgroupFailure,
    },
}

/// Consume one opaque launcher instance and enter the exact fixed manager
/// cgroup. No path, PID, controller set, policy, or fallback is caller chosen.
///
/// Failures before the move are typed `PreMove`. The move attempt is the
/// irreversible boundary: its own failure and every later failure are typed
/// `PostMoveFatal`, which a future top-level executable must handle by exiting
/// immediately without binding a listener or reporting readiness.
pub fn enter_fixed_manager_cgroup(
    instance: VerifiedLauncherInstance,
) -> Result<VerifiedManagerCgroup, ManagerCgroupError> {
    #[cfg(target_os = "linux")]
    {
        let mut operations = linux::LinuxManagerOperations;
        let established = establish_manager(instance, &mut operations)?;
        Ok(VerifiedManagerCgroup {
            instance: established.token,
            service_root: established.root,
            manager: established.manager,
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        drop(instance);
        Err(ManagerCgroupError::UnsupportedPlatform)
    }
}

#[cfg(any(target_os = "linux", test))]
#[derive(Debug)]
#[allow(dead_code)]
struct EstablishedManager<T, R, M> {
    token: T,
    root: R,
    manager: M,
}

#[cfg(any(target_os = "linux", test))]
trait ManagerOperations {
    type Root;
    type Manager;

    fn verify_single_thread(&mut self) -> Result<(), ManagerCgroupFailure>;
    fn open_fixed_root(&mut self) -> Result<Self::Root, ManagerCgroupFailure>;
    fn open_or_create_manager(
        &mut self,
        root: &Self::Root,
    ) -> Result<Self::Manager, ManagerCgroupFailure>;
    fn verify_manager_before_move(
        &mut self,
        manager: &Self::Manager,
    ) -> Result<(), ManagerCgroupFailure>;
    fn move_current_process(&mut self, manager: &Self::Manager)
        -> Result<(), ManagerCgroupFailure>;
    fn verify_root_after_move(&mut self, root: &Self::Root) -> Result<(), ManagerCgroupFailure>;
    fn enable_controllers(&mut self, root: &Self::Root) -> Result<(), ManagerCgroupFailure>;
    fn verify_controllers(&mut self, root: &Self::Root) -> Result<(), ManagerCgroupFailure>;
    fn verify_manager_after_move(
        &mut self,
        manager: &Self::Manager,
    ) -> Result<(), ManagerCgroupFailure>;
}

#[cfg(any(target_os = "linux", test))]
fn establish_manager<T, O: ManagerOperations>(
    token: T,
    operations: &mut O,
) -> Result<EstablishedManager<T, O::Root, O::Manager>, ManagerCgroupError> {
    operations
        .verify_single_thread()
        .map_err(|failure| pre_move("verify single-thread launcher", failure))?;
    let root = operations
        .open_fixed_root()
        .map_err(|failure| pre_move("open fixed service cgroup", failure))?;
    let manager = operations
        .open_or_create_manager(&root)
        .map_err(|failure| pre_move("open or create manager cgroup", failure))?;
    operations
        .verify_manager_before_move(&manager)
        .map_err(|failure| pre_move("verify manager before move", failure))?;

    operations
        .move_current_process(&manager)
        .map_err(|failure| post_move("move current process", failure))?;
    operations
        .verify_root_after_move(&root)
        .map_err(|failure| post_move("verify service root after move", failure))?;
    operations
        .enable_controllers(&root)
        .map_err(|failure| post_move("enable delegated controllers", failure))?;
    operations
        .verify_controllers(&root)
        .map_err(|failure| post_move("verify delegated controllers", failure))?;
    operations
        .verify_manager_after_move(&manager)
        .map_err(|failure| post_move("verify manager after move", failure))?;

    Ok(EstablishedManager {
        token,
        root,
        manager,
    })
}

#[cfg(any(target_os = "linux", test))]
fn pre_move(stage: &'static str, failure: ManagerCgroupFailure) -> ManagerCgroupError {
    ManagerCgroupError::PreMove { stage, failure }
}

#[cfg(any(target_os = "linux", test))]
fn post_move(stage: &'static str, failure: ManagerCgroupFailure) -> ManagerCgroupError {
    ManagerCgroupError::PostMoveFatal { stage, failure }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::fs;

    use super::{ManagerCgroupFailure, ManagerOperations};
    use crate::cgroupfs_fd::{self, CgroupDirectory};

    pub(super) struct LinuxManagerOperations;

    impl ManagerOperations for LinuxManagerOperations {
        type Root = CgroupDirectory;
        type Manager = CgroupDirectory;

        fn verify_single_thread(&mut self) -> Result<(), ManagerCgroupFailure> {
            let expected = current_tid()?;
            let mut observed = fs::read_dir("/proc/self/task")
                .map_err(platform_io)?
                .map(|entry| {
                    let entry = entry.map_err(platform_io)?;
                    let name = entry.file_name();
                    name.to_str()
                        .ok_or_else(|| platform_failure("/proc/self/task entry is not UTF-8"))?
                        .parse::<u32>()
                        .map_err(|_| platform_failure("/proc/self/task entry is not a numeric TID"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            observed.sort_unstable();
            if observed == [expected] {
                Ok(())
            } else {
                Err(ManagerCgroupFailure::NotSingleThreaded { expected, observed })
            }
        }

        fn open_fixed_root(&mut self) -> Result<Self::Root, ManagerCgroupFailure> {
            cgroupfs_fd::open_fixed_service_root().map_err(platform_cgroup)
        }

        fn open_or_create_manager(
            &mut self,
            root: &Self::Root,
        ) -> Result<Self::Manager, ManagerCgroupFailure> {
            cgroupfs_fd::open_or_create_manager(root).map_err(platform_cgroup)
        }

        fn verify_manager_before_move(
            &mut self,
            manager: &Self::Manager,
        ) -> Result<(), ManagerCgroupFailure> {
            cgroupfs_fd::verify_manager_empty_before_move(manager).map_err(platform_cgroup)
        }

        fn move_current_process(
            &mut self,
            manager: &Self::Manager,
        ) -> Result<(), ManagerCgroupFailure> {
            cgroupfs_fd::move_current_process_into_manager(manager).map_err(platform_cgroup)
        }

        fn verify_root_after_move(
            &mut self,
            root: &Self::Root,
        ) -> Result<(), ManagerCgroupFailure> {
            cgroupfs_fd::verify_service_root_has_no_processes(root).map_err(platform_cgroup)
        }

        fn enable_controllers(&mut self, root: &Self::Root) -> Result<(), ManagerCgroupFailure> {
            cgroupfs_fd::enable_required_controllers(root).map_err(platform_cgroup)
        }

        fn verify_controllers(&mut self, root: &Self::Root) -> Result<(), ManagerCgroupFailure> {
            cgroupfs_fd::verify_required_controllers(root).map_err(platform_cgroup)
        }

        fn verify_manager_after_move(
            &mut self,
            manager: &Self::Manager,
        ) -> Result<(), ManagerCgroupFailure> {
            cgroupfs_fd::verify_manager_after_move(manager).map_err(platform_cgroup)
        }
    }

    #[allow(unsafe_code)]
    fn current_tid() -> Result<u32, ManagerCgroupFailure> {
        // SAFETY: `gettid` has no preconditions.
        let tid = unsafe { libc::gettid() };
        u32::try_from(tid).map_err(|_| platform_failure("current TID is outside u32"))
    }

    fn platform_cgroup(source: cgroupfs_fd::CgroupFsError) -> ManagerCgroupFailure {
        ManagerCgroupFailure::Platform(source.to_string())
    }

    fn platform_io(source: std::io::Error) -> ManagerCgroupFailure {
        ManagerCgroupFailure::Platform(source.to_string())
    }

    fn platform_failure(reason: impl Into<String>) -> ManagerCgroupFailure {
        ManagerCgroupFailure::Platform(reason.into())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::{establish_manager, ManagerCgroupError, ManagerCgroupFailure, ManagerOperations};

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Step {
        SingleThread,
        OpenRoot,
        OpenManager,
        VerifyManagerBefore,
        Move,
        VerifyRoot,
        EnableControllers,
        VerifyControllers,
        VerifyManagerAfter,
    }

    struct FakeOperations {
        observed: Vec<Step>,
        failures: VecDeque<Step>,
    }

    impl FakeOperations {
        fn new(failures: impl IntoIterator<Item = Step>) -> Self {
            Self {
                observed: Vec::new(),
                failures: failures.into_iter().collect(),
            }
        }

        fn step(&mut self, step: Step) -> Result<(), ManagerCgroupFailure> {
            self.observed.push(step);
            if self.failures.front() == Some(&step) {
                self.failures.pop_front();
                Err(ManagerCgroupFailure::Platform("injected".to_string()))
            } else {
                Ok(())
            }
        }
    }

    impl ManagerOperations for FakeOperations {
        type Root = ();
        type Manager = ();

        fn verify_single_thread(&mut self) -> Result<(), ManagerCgroupFailure> {
            self.step(Step::SingleThread)
        }
        fn open_fixed_root(&mut self) -> Result<Self::Root, ManagerCgroupFailure> {
            self.step(Step::OpenRoot)
        }
        fn open_or_create_manager(
            &mut self,
            _root: &Self::Root,
        ) -> Result<Self::Manager, ManagerCgroupFailure> {
            self.step(Step::OpenManager)
        }
        fn verify_manager_before_move(
            &mut self,
            _manager: &Self::Manager,
        ) -> Result<(), ManagerCgroupFailure> {
            self.step(Step::VerifyManagerBefore)
        }
        fn move_current_process(
            &mut self,
            _manager: &Self::Manager,
        ) -> Result<(), ManagerCgroupFailure> {
            self.step(Step::Move)
        }
        fn verify_root_after_move(
            &mut self,
            _root: &Self::Root,
        ) -> Result<(), ManagerCgroupFailure> {
            self.step(Step::VerifyRoot)
        }
        fn enable_controllers(&mut self, _root: &Self::Root) -> Result<(), ManagerCgroupFailure> {
            self.step(Step::EnableControllers)
        }
        fn verify_controllers(&mut self, _root: &Self::Root) -> Result<(), ManagerCgroupFailure> {
            self.step(Step::VerifyControllers)
        }
        fn verify_manager_after_move(
            &mut self,
            _manager: &Self::Manager,
        ) -> Result<(), ManagerCgroupFailure> {
            self.step(Step::VerifyManagerAfter)
        }
    }

    const COMPLETE_ORDER: [Step; 9] = [
        Step::SingleThread,
        Step::OpenRoot,
        Step::OpenManager,
        Step::VerifyManagerBefore,
        Step::Move,
        Step::VerifyRoot,
        Step::EnableControllers,
        Step::VerifyControllers,
        Step::VerifyManagerAfter,
    ];

    #[test]
    fn manager_setup_runs_once_in_the_frozen_order() {
        let mut operations = FakeOperations::new([]);
        let result = establish_manager(7_u8, &mut operations).expect("manager setup");
        assert_eq!(result.token, 7);
        assert_eq!(operations.observed, COMPLETE_ORDER);
    }

    #[test]
    fn every_failure_before_the_move_is_pre_move_and_stops_work() {
        for (index, failed_step) in COMPLETE_ORDER[..4].iter().copied().enumerate() {
            let mut operations = FakeOperations::new([failed_step]);
            let error = establish_manager(7_u8, &mut operations).expect_err("injected failure");
            assert!(matches!(error, ManagerCgroupError::PreMove { .. }));
            assert_eq!(operations.observed, COMPLETE_ORDER[..=index]);
        }
    }

    #[test]
    fn move_attempt_and_every_later_failure_are_post_move_fatal() {
        for (index, failed_step) in COMPLETE_ORDER[4..].iter().copied().enumerate() {
            let mut operations = FakeOperations::new([failed_step]);
            let error = establish_manager(7_u8, &mut operations).expect_err("injected failure");
            assert!(matches!(error, ManagerCgroupError::PostMoveFatal { .. }));
            assert_eq!(operations.observed, COMPLETE_ORDER[..=index + 4]);
        }
    }
}
