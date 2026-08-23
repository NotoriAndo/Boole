//! Per-request Linux containment for one native-shadow checker execution.

use thiserror::Error;

use boole_native_shadow_protocol::{sha256_hex, VerifiedClosedLocalReplayAuthorization};

const OPERATION_ID_BYTES: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RunOperationId([u8; OPERATION_ID_BYTES]);

#[derive(Debug, Error)]
pub(crate) enum RunOperationIdError {
    #[error("operation ID must be exact lowercase 32-byte hexadecimal")]
    Invalid,
}

impl RunOperationId {
    pub(crate) fn parse(value: &str) -> Result<Self, RunOperationIdError> {
        if value.len() != OPERATION_ID_BYTES * 2
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(RunOperationIdError::Invalid);
        }
        let decoded = hex::decode(value).map_err(|_| RunOperationIdError::Invalid)?;
        let bytes = decoded
            .try_into()
            .map_err(|_| RunOperationIdError::Invalid)?;
        Ok(Self(bytes))
    }

    pub(crate) fn leaf_name(&self) -> String {
        format!("run-{}", hex::encode(self.0))
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ResourceSnapshot {
    pub(crate) cpu_usage_usec: u64,
    pub(crate) memory_peak_bytes: u64,
    pub(crate) memory_events_low: u64,
    pub(crate) memory_events_high: u64,
    pub(crate) memory_events_max: u64,
    pub(crate) memory_events_oom: u64,
    pub(crate) memory_events_oom_kill: u64,
    pub(crate) memory_events_oom_group_kill: u64,
    pub(crate) pids_events_max: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TerminalWait {
    Exited(u8),
    Signaled { signal: u8, core_dumped: bool },
}

/// Opaque proof that one child was observed and its entire cgroup leaf was
/// confirmed empty and removed before a result crossed the launcher boundary.
#[derive(Debug)]
pub(crate) struct ContainedExecution {
    wait: TerminalWait,
    resources: ResourceSnapshot,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    stdout_bytes: u64,
    stderr_bytes: u64,
    stdout_sha256: [u8; 32],
    stderr_sha256: [u8; 32],
    timed_out: bool,
    output_overflow: bool,
}

impl ContainedExecution {
    pub(crate) fn wait(&self) -> TerminalWait {
        self.wait
    }

    pub(crate) fn resources(&self) -> &ResourceSnapshot {
        &self.resources
    }

    pub(crate) fn stdout(&self) -> &[u8] {
        &self.stdout
    }

    pub(crate) fn stderr(&self) -> &[u8] {
        &self.stderr
    }

    pub(crate) fn stdout_bytes(&self) -> u64 {
        self.stdout_bytes
    }

    pub(crate) fn stderr_bytes(&self) -> u64 {
        self.stderr_bytes
    }

    pub(crate) fn stdout_sha256(&self) -> [u8; 32] {
        self.stdout_sha256
    }

    pub(crate) fn stderr_sha256(&self) -> [u8; 32] {
        self.stderr_sha256
    }

    pub(crate) fn timed_out(&self) -> bool {
        self.timed_out
    }

    pub(crate) fn output_overflow(&self) -> bool {
        self.output_overflow
    }
}

#[derive(Debug, Error)]
pub(crate) enum ContainmentFailure {
    #[cfg(not(target_os = "linux"))]
    #[error("native-shadow per-request containment requires Linux")]
    UnsupportedPlatform,
    #[error("one native-shadow execution is already active")]
    Busy,
    #[error("per-request containment platform failure: {0}")]
    Platform(String),
    #[error("fatal per-request containment cleanup failure: {0}")]
    FatalCleanup(String),
}

/// Already-verified task/anchor/submission bytes for the one fixed checker.
///
/// This type and its constructor are crate-private: an external caller cannot
/// inject an argv, executable, environment, path or fake execution outcome.
pub(crate) struct VerifiedCheckerMaterials {
    operation: RunOperationId,
    task: Vec<u8>,
    anchor: Vec<u8>,
    submission: Vec<u8>,
}

impl VerifiedCheckerMaterials {
    fn from_authorization(
        authorization: VerifiedClosedLocalReplayAuthorization,
        submission: Vec<u8>,
    ) -> Result<Self, ContainmentFailure> {
        if sha256_hex(&submission) != authorization.submission_source_digest_hex() {
            return Err(ContainmentFailure::Platform(
                "authorized submission source digest mismatch".to_string(),
            ));
        }
        let operation = RunOperationId::parse(authorization.operation_id_hex())
            .map_err(|error| ContainmentFailure::Platform(error.to_string()))?;
        if authorization.max_checker_executions() != 1
            || authorization.task_bytes().is_empty()
            || authorization.anchor_bytes().is_empty()
            || submission.is_empty()
        {
            return Err(ContainmentFailure::Platform(
                "authorized checker materials violate the one-shot contract".to_string(),
            ));
        }
        Ok(Self {
            operation,
            task: authorization.task_bytes().to_vec(),
            anchor: authorization.anchor_bytes().to_vec(),
            submission,
        })
    }
}

#[cfg(any(target_os = "linux", test))]
trait ContainmentOperations {
    type Leaf;
    type Child;
    type Output;

    fn create_leaf(&mut self, operation: &RunOperationId)
        -> Result<Self::Leaf, ContainmentFailure>;
    fn apply_fixed_limits(&mut self, leaf: &Self::Leaf) -> Result<(), ContainmentFailure>;
    fn clone_child_atomically(
        &mut self,
        leaf: &Self::Leaf,
    ) -> Result<Self::Child, ContainmentFailure>;
    fn wait_and_observe(
        &mut self,
        leaf: &Self::Leaf,
        child: &mut Self::Child,
    ) -> Result<(TerminalWait, ResourceSnapshot), ContainmentFailure>;
    fn close_child_handles(
        &mut self,
        child: Self::Child,
    ) -> Result<Self::Output, ContainmentFailure>;
    fn terminate_tree(
        &mut self,
        leaf: &Self::Leaf,
        child: &mut Self::Child,
    ) -> Result<(), ContainmentFailure>;
    fn discard_child_handles(&mut self, child: Self::Child);
    fn confirm_unpopulated(&mut self, leaf: &Self::Leaf) -> Result<(), ContainmentFailure>;
    fn remove_leaf(&mut self, leaf: Self::Leaf) -> Result<(), ContainmentFailure>;
    fn finish_report(
        &mut self,
        wait: TerminalWait,
        resources: ResourceSnapshot,
        output: Self::Output,
    ) -> ContainedExecution;
}

#[cfg(any(target_os = "linux", test))]
fn execute_with_operations<O: ContainmentOperations>(
    operation: RunOperationId,
    operations: &mut O,
) -> Result<ContainedExecution, ContainmentFailure> {
    let leaf = operations.create_leaf(&operation)?;
    if let Err(failure) = operations.apply_fixed_limits(&leaf) {
        if let Err(cleanup) = operations.remove_leaf(leaf) {
            return Err(fatal_cleanup([("remove-leaf", cleanup)]));
        }
        return Err(failure);
    }
    let mut child = match operations.clone_child_atomically(&leaf) {
        Ok(child) => child,
        Err(failure) => {
            if let Err(cleanup) = operations.remove_leaf(leaf) {
                return Err(fatal_cleanup([("remove-leaf", cleanup)]));
            }
            return Err(failure);
        }
    };
    let (wait, resources) = match operations.wait_and_observe(&leaf, &mut child) {
        Ok(observed) => observed,
        Err(failure) => {
            let terminate = operations.terminate_tree(&leaf, &mut child).err();
            operations.discard_child_handles(child);
            let unpopulated = operations.confirm_unpopulated(&leaf).err();
            let removed = operations.remove_leaf(leaf).err();
            let cleanup = [
                terminate.map(|error| ("terminate-tree", error)),
                unpopulated.map(|error| ("confirm-unpopulated", error)),
                removed.map(|error| ("remove-leaf", error)),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
            if !cleanup.is_empty() {
                return Err(fatal_cleanup(cleanup));
            }
            return Err(failure);
        }
    };
    let output = operations.close_child_handles(child);
    let unpopulated = operations.confirm_unpopulated(&leaf).err();
    let removed = operations.remove_leaf(leaf).err();
    let mut cleanup = Vec::new();
    if let Err(error) = output.as_ref() {
        cleanup.push(("close-child-handles", error.to_string()));
    }
    if let Some(error) = unpopulated {
        cleanup.push(("confirm-unpopulated", error.to_string()));
    }
    if let Some(error) = removed {
        cleanup.push(("remove-leaf", error.to_string()));
    }
    if !cleanup.is_empty() {
        return Err(ContainmentFailure::FatalCleanup(
            cleanup
                .into_iter()
                .map(|(stage, reason)| format!("{stage}: {reason}"))
                .collect::<Vec<_>>()
                .join("; "),
        ));
    }
    Ok(operations.finish_report(
        wait,
        resources,
        output.expect("cleanup collection proved output success"),
    ))
}

fn fatal_cleanup<I>(failures: I) -> ContainmentFailure
where
    I: IntoIterator<Item = (&'static str, ContainmentFailure)>,
{
    ContainmentFailure::FatalCleanup(
        failures
            .into_iter()
            .map(|(stage, error)| format!("{stage}: {error}"))
            .collect::<Vec<_>>()
            .join("; "),
    )
}

static EXECUTION_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(target_os = "linux")]
mod linux;

/// Execute the one fixed native checker beneath an already-verified startup
/// and toolchain proof. There is no degraded or non-Linux fallback.
pub(crate) fn execute_fixed_checker(
    compatibility: &crate::toolchain_compatibility::VerifiedStartupToolchainCompatibility,
    authorization: VerifiedClosedLocalReplayAuthorization,
    submission: Vec<u8>,
) -> Result<ContainedExecution, ContainmentFailure> {
    let materials = VerifiedCheckerMaterials::from_authorization(authorization, submission)?;
    let _guard = EXECUTION_LOCK
        .try_lock()
        .map_err(|_| ContainmentFailure::Busy)?;
    #[cfg(target_os = "linux")]
    {
        linux::execute(compatibility, materials)
    }
    #[cfg(not(target_os = "linux"))]
    {
        drop((compatibility, materials));
        Err(ContainmentFailure::UnsupportedPlatform)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::{
        execute_fixed_checker, execute_with_operations, ContainedExecution, ContainmentFailure,
        ContainmentOperations, ResourceSnapshot, RunOperationId, TerminalWait,
    };

    #[test]
    fn fixed_executor_accepts_only_one_request_bound_permit_by_value() {
        let _entrypoint: for<'a> fn(
            crate::closed_local_replay_startup::VerifiedClosedLocalReplayExecutionPermit<'a>,
        ) -> Result<ContainedExecution, ContainmentFailure> = execute_fixed_checker;
    }

    #[test]
    fn operation_id_is_exact_lower_hex_and_derives_one_direct_leaf_name() {
        let value = "0123456789abcdef".repeat(4);
        let operation = RunOperationId::parse(&value).expect("exact operation ID");
        assert_eq!(operation.leaf_name(), format!("run-{value}"));

        for malformed in [
            "0".repeat(63),
            "0".repeat(65),
            "A".repeat(64),
            "g".repeat(64),
            format!("{}../manager", "0".repeat(52)),
        ] {
            assert!(RunOperationId::parse(&malformed).is_err(), "{malformed:?}");
        }
    }

    #[derive(Default)]
    struct RecordingOperations {
        events: RefCell<Vec<&'static str>>,
        fail_at: Option<&'static str>,
        cleanup_fail_at: Option<&'static str>,
    }

    impl RecordingOperations {
        fn record(&self, event: &'static str) -> Result<(), ContainmentFailure> {
            self.events.borrow_mut().push(event);
            if self.fail_at == Some(event) || self.cleanup_fail_at == Some(event) {
                Err(ContainmentFailure::Platform(event.to_string()))
            } else {
                Ok(())
            }
        }
    }

    impl ContainmentOperations for RecordingOperations {
        type Leaf = ();
        type Child = ();
        type Output = ();

        fn create_leaf(&mut self, _: &RunOperationId) -> Result<Self::Leaf, ContainmentFailure> {
            self.record("create-leaf")
        }

        fn apply_fixed_limits(&mut self, _: &Self::Leaf) -> Result<(), ContainmentFailure> {
            self.record("apply-limits")
        }

        fn clone_child_atomically(
            &mut self,
            _: &Self::Leaf,
        ) -> Result<Self::Child, ContainmentFailure> {
            self.record("clone3")
        }

        fn wait_and_observe(
            &mut self,
            _: &Self::Leaf,
            _: &mut Self::Child,
        ) -> Result<(TerminalWait, ResourceSnapshot), ContainmentFailure> {
            self.record("observe")?;
            Ok((TerminalWait::Exited(0), ResourceSnapshot::default()))
        }

        fn close_child_handles(
            &mut self,
            _: Self::Child,
        ) -> Result<Self::Output, ContainmentFailure> {
            self.record("close-child-handles")?;
            Ok(())
        }

        fn terminate_tree(
            &mut self,
            _: &Self::Leaf,
            _: &mut Self::Child,
        ) -> Result<(), ContainmentFailure> {
            self.record("terminate-tree")
        }

        fn discard_child_handles(&mut self, _: Self::Child) {
            self.events.borrow_mut().push("discard-child-handles");
        }

        fn confirm_unpopulated(&mut self, _: &Self::Leaf) -> Result<(), ContainmentFailure> {
            self.record("confirm-unpopulated")
        }

        fn remove_leaf(&mut self, _: Self::Leaf) -> Result<(), ContainmentFailure> {
            self.record("remove-leaf")
        }

        fn finish_report(
            &mut self,
            wait: TerminalWait,
            resources: ResourceSnapshot,
            (): Self::Output,
        ) -> ContainedExecution {
            ContainedExecution {
                wait,
                resources,
                stdout: Vec::new(),
                stderr: Vec::new(),
                stdout_bytes: 0,
                stderr_bytes: 0,
                stdout_sha256: [0; 32],
                stderr_sha256: [0; 32],
                timed_out: false,
                output_overflow: false,
            }
        }
    }

    #[test]
    fn report_exists_only_after_atomic_clone_observation_and_complete_cleanup() {
        let operation = RunOperationId::parse(&"1".repeat(64)).unwrap();
        let mut operations = RecordingOperations::default();
        let report = execute_with_operations(operation, &mut operations).expect("contained run");

        assert_eq!(report.wait(), TerminalWait::Exited(0));
        assert_eq!(report.resources(), &ResourceSnapshot::default());
        assert_eq!(
            operations.events.into_inner(),
            [
                "create-leaf",
                "apply-limits",
                "clone3",
                "observe",
                "close-child-handles",
                "confirm-unpopulated",
                "remove-leaf",
            ]
        );
    }

    #[test]
    fn cleanup_failure_is_fail_closed_and_never_issues_a_report() {
        for fail_at in ["close-child-handles", "confirm-unpopulated", "remove-leaf"] {
            let operation = RunOperationId::parse(&"2".repeat(64)).unwrap();
            let mut operations = RecordingOperations {
                fail_at: Some(fail_at),
                ..RecordingOperations::default()
            };
            assert!(matches!(
                execute_with_operations(operation, &mut operations),
                Err(ContainmentFailure::FatalCleanup(stage)) if stage.contains(fail_at)
            ));
        }
    }

    #[test]
    fn cleanup_attempts_continue_after_the_first_failure_and_poison_the_launcher() {
        let operation = RunOperationId::parse(&"4".repeat(64)).unwrap();
        let mut operations = RecordingOperations {
            fail_at: Some("observe"),
            cleanup_fail_at: Some("terminate-tree"),
            ..RecordingOperations::default()
        };
        assert!(matches!(
            execute_with_operations(operation, &mut operations),
            Err(ContainmentFailure::FatalCleanup(stage)) if stage.contains("terminate-tree")
        ));
        assert_eq!(
            operations.events.into_inner(),
            [
                "create-leaf",
                "apply-limits",
                "clone3",
                "observe",
                "terminate-tree",
                "discard-child-handles",
                "confirm-unpopulated",
                "remove-leaf",
            ]
        );
    }

    #[test]
    fn every_failure_after_leaf_creation_removes_or_kills_the_leaf_before_returning() {
        for (fail_at, expected) in [
            (
                "apply-limits",
                vec!["create-leaf", "apply-limits", "remove-leaf"],
            ),
            (
                "clone3",
                vec!["create-leaf", "apply-limits", "clone3", "remove-leaf"],
            ),
            (
                "observe",
                vec![
                    "create-leaf",
                    "apply-limits",
                    "clone3",
                    "observe",
                    "terminate-tree",
                    "discard-child-handles",
                    "confirm-unpopulated",
                    "remove-leaf",
                ],
            ),
        ] {
            let operation = RunOperationId::parse(&"3".repeat(64)).unwrap();
            let mut operations = RecordingOperations {
                fail_at: Some(fail_at),
                ..RecordingOperations::default()
            };
            assert!(execute_with_operations(operation, &mut operations).is_err());
            assert_eq!(operations.events.into_inner(), expected);
        }
    }

    fn _report_is_opaque(_: ContainedExecution) {}
}
